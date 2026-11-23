//! Gummel-Poon bipolar junction transistor.
//!
//! Terminals are `[collector, base, emitter]`; the substrate of the
//! collector-substrate capacitance is ground. The equations live in
//! [`model`]; this module allocates the internal nodes `<name>:c'`,
//! `<name>:b'` and `<name>:e'` (only for non-zero `rc`, `rb`, `re`), limits
//! the junction voltages, integrates the four junction charges and stamps the
//! companion model.
//!
//! Reported terminal currents flow into the device. They sum to zero except
//! for the collector-substrate charging current, which leaves through the
//! (implicit) grounded substrate: it is part of the collector current only,
//! in transients and in AC analysis alike (as in LTspice).
//!
//! Differences from SPICE3 `bjtload`, all far below solver tolerances in
//! normal operation:
//! * junction exponentials are exact in reverse bias (SPICE linearises them),
//! * `gmin` loads only the base currents (as in ngspice),
//! * excess phase (`ptf`) applies to AC analysis only; transient analysis
//!   ignores it.
//!
//! As in SPICE, the bias dependence of the base resistance is not part of the
//! Jacobian: each iteration uses the resistance at the current iterate, which
//! converges to the same solution.

mod model;

use super::semiconductor::{
    AcControl, Control, ac_branch_current, current_converged, integrate_two_controls, pnjlim,
    solution_value, stamp_ac_branch, stamp_branch,
};
use super::{AcContext, DeviceInstance, LoadContext, SetupContext, StateSlot};
use crate::analysis::SimOptions;
use crate::circuit::NodeIndex;
use crate::mna::Unknown;
use crate::models::BjtModel;
use model::{BjtParams, BjtState};
use num_complex::Complex64;

/// Unknowns of the terminals and of the internal nodes (which are the
/// terminals themselves when the corresponding resistance is absent).
#[derive(Debug, Clone, Copy, Default)]
struct Nodes {
    collector: Unknown,
    base: Unknown,
    emitter: Unknown,
    collector_internal: Unknown,
    base_internal: Unknown,
    emitter_internal: Unknown,
}

/// State slots of the junction charges that are present.
#[derive(Debug, Clone, Copy, Default)]
struct Slots {
    be: Option<StateSlot>,
    bc: Option<StateSlot>,
    bx: Option<StateSlot>,
    cs: Option<StateSlot>,
}

/// Small-signal branch currents of the intrinsic transistor, each the sum
/// of its controls: `b'→e'`, `b'→c'`, `c'→e'`, external base `→ c'`
/// (the part of the B-C capacitance outside `xcjc`) and substrate `→ c'`.
#[derive(Debug, Clone, Copy)]
struct SmallSignal {
    be: [AcControl; 2],
    bc: [AcControl; 1],
    ce: [AcControl; 2],
    bx: [AcControl; 1],
    cs: [AcControl; 1],
}

/// Charging currents (normalised) and companion conductances at the last
/// load. Zero in DC.
#[derive(Debug, Clone, Copy, Default)]
struct Charging {
    /// B-E charge current and its derivatives w.r.t. `vbe` and `vbc`.
    be: (f64, f64, f64),
    bc: (f64, f64),
    bx: (f64, f64),
    cs: (f64, f64),
}

#[derive(Debug)]
pub(crate) struct Bjt {
    nodes: [NodeIndex; 3],
    params: BjtParams,
    unknowns: Nodes,
    slots: Slots,
    state: BjtState,
    charging: Charging,
}

/// Integrates a single-control charge if its slot exists.
fn integrate(ctx: &mut LoadContext, slot: Option<StateSlot>, q: f64, c: f64) -> (f64, f64) {
    slot.map_or((0.0, 0.0), |slot| ctx.integrate(slot, q, c))
}

impl Bjt {
    pub(crate) fn new(
        nodes: &[NodeIndex],
        model: &BjtModel,
        area: f64,
        options: &SimOptions,
    ) -> Self {
        Self {
            nodes: std::array::from_fn(|i| nodes[i]),
            params: BjtParams::new(model, area, options.temperature),
            unknowns: Nodes::default(),
            slots: Slots::default(),
            state: BjtState::default(),
            charging: Charging::default(),
        }
    }

    /// Normalised B-E and B-C branch currents including charging.
    fn junction_currents(&self) -> (f64, f64) {
        (
            self.state.ibe + self.charging.be.0,
            self.state.ibc + self.charging.bc.0,
        )
    }

    /// Normalised `vbe`, `vbc` of solution `x`.
    fn junction_voltages(&self, x: &[f64]) -> (f64, f64) {
        let n = &self.unknowns;
        let vb = solution_value(x, n.base_internal);
        let sign = self.params.sign;
        (
            sign * (vb - solution_value(x, n.emitter_internal)),
            sign * (vb - solution_value(x, n.collector_internal)),
        )
    }

    /// The small-signal branches at the last load and angular frequency
    /// `omega`. Their controls are actual (not polarity-normalised) node
    /// voltages, so the branch currents are actual currents for both
    /// polarities.
    fn small_signal(&self, omega: f64) -> SmallSignal {
        let n = self.unknowns;
        let st = &self.state;
        let (b, c, e) = (n.base_internal, n.collector_internal, n.emitter_internal);
        let transport = Complex64::from_polar(st.gm + st.go, -omega * self.params.excess_phase);
        SmallSignal {
            be: [
                AcControl::rc(b, e, st.gpi, st.cbe, omega),
                AcControl::rc(b, c, 0.0, st.cbe_vbc, omega),
            ],
            bc: [AcControl::rc(b, c, st.gmu, st.cbc, omega)],
            ce: [
                AcControl::new(b, e, transport),
                AcControl::rc(b, c, -st.go, 0.0, omega),
            ],
            bx: [AcControl::rc(n.base, c, 0.0, st.cbx, omega)],
            cs: [AcControl::rc(None, c, 0.0, st.ccs, omega)],
        }
    }
}

impl DeviceInstance for Bjt {
    fn setup(&mut self, ctx: &mut SetupContext) {
        let [collector, base, emitter] = self.nodes.map(|n| ctx.node(n));
        let p = &self.params;
        let mut internal = |present: bool, terminal: Unknown, suffix: &str| {
            if present {
                ctx.internal_node(suffix)
            } else {
                terminal
            }
        };
        let collector_internal = internal(p.collector.is_some(), collector, "c'");
        let base_internal = internal(p.base.is_some(), base, "b'");
        let emitter_internal = internal(p.emitter.is_some(), emitter, "e'");
        self.unknowns = Nodes {
            collector,
            base,
            emitter,
            collector_internal,
            base_internal,
            emitter_internal,
        };

        let be = p.be_depletion.is_present() || p.has_forward_transit();
        let bc = p.bc_depletion.is_present() || p.has_reverse_transit();
        let mut slot = |present: bool| present.then(|| ctx.state());
        self.slots = Slots {
            be: slot(be),
            bc: slot(bc),
            bx: slot(p.bx_depletion.is_present()),
            cs: slot(p.cs_depletion.is_present()),
        };
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let n = self.unknowns;
        let p = &self.params;
        let sign = p.sign;
        let (vbe, vbc, vbx, vcs, limited) = if ctx.init_junctions {
            // SPICE MODEINITJCT: emitter junction at vcrit, the others at 0.
            let vbe = if p.vcrit.is_finite() { p.vcrit } else { 0.0 };
            (vbe, 0.0, 0.0, 0.0, true)
        } else {
            let vb = ctx.v(n.base_internal);
            let vc = ctx.v(n.collector_internal);
            let (vbe, be_limited) = pnjlim(
                sign * (vb - ctx.v(n.emitter_internal)),
                self.state.vbe,
                p.vt,
                p.vcrit,
            );
            let (vbc, bc_limited) = pnjlim(sign * (vb - vc), self.state.vbc, p.vt, p.vcrit);
            let vbx = sign * (ctx.v(n.base) - vc);
            (vbe, vbc, vbx, -sign * vc, be_limited || bc_limited)
        };
        ctx.limited |= limited;

        let st = p.evaluate(vbe, vbc, vbx, vcs, ctx.gmin);
        self.charging = Charging {
            be: match self.slots.be {
                Some(slot) => integrate_two_controls(ctx, slot, st.qbe, st.cbe, st.cbe_vbc),
                None => (0.0, 0.0, 0.0),
            },
            bc: integrate(ctx, self.slots.bc, st.qbc, st.cbc),
            bx: integrate(ctx, self.slots.bx, st.qbx, st.cbx),
            cs: integrate(ctx, self.slots.cs, st.qcs, st.ccs),
        };
        self.state = st;
        let ch = self.charging;
        let (ibe, ibc) = self.junction_currents();
        let (b, c, e) = (n.base_internal, n.collector_internal, n.emitter_internal);

        stamp_branch(
            ctx,
            sign,
            b,
            e,
            ibe,
            &[
                Control::new(b, e, st.gpi + ch.be.1, vbe),
                Control::new(b, c, ch.be.2, vbc),
            ],
        );
        stamp_branch(
            ctx,
            sign,
            b,
            c,
            ibc,
            &[Control::new(b, c, st.gmu + ch.bc.1, vbc)],
        );
        stamp_branch(
            ctx,
            sign,
            c,
            e,
            st.ict,
            &[
                Control::new(b, e, st.gm + st.go, vbe),
                Control::new(b, c, -st.go, vbc),
            ],
        );
        if self.slots.bx.is_some() {
            stamp_branch(
                ctx,
                sign,
                n.base,
                c,
                ch.bx.0,
                &[Control::new(n.base, c, ch.bx.1, vbx)],
            );
        }
        if self.slots.cs.is_some() {
            stamp_branch(
                ctx,
                sign,
                None,
                c,
                ch.cs.0,
                &[Control::new(None, c, ch.cs.1, vcs)],
            );
        }
        if p.base.is_some() {
            ctx.stamp_conductance(n.base, b, st.gx);
        }
        if let Some(g) = p.collector {
            ctx.stamp_conductance(n.collector, c, g);
        }
        if let Some(g) = p.emitter {
            ctx.stamp_conductance(n.emitter, e, g);
        }
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let n = self.unknowns;
        let ss = self.small_signal(ctx.omega);
        let (b, c, e) = (n.base_internal, n.collector_internal, n.emitter_internal);
        stamp_ac_branch(ctx, b, e, &ss.be);
        stamp_ac_branch(ctx, b, c, &ss.bc);
        stamp_ac_branch(ctx, c, e, &ss.ce);
        stamp_ac_branch(ctx, n.base, c, &ss.bx);
        stamp_ac_branch(ctx, None, c, &ss.cs);
        let p = &self.params;
        if p.base.is_some() {
            ctx.stamp_conductance(n.base, b, self.state.gx);
        }
        if let Some(g) = p.collector {
            ctx.stamp_conductance(n.collector, c, g);
        }
        if let Some(g) = p.emitter {
            ctx.stamp_conductance(n.emitter, e, g);
        }
    }

    /// The branch currents of the stamps of [`Self::load_ac`], combined per
    /// terminal exactly as [`Self::currents`] combines the DC ones. With
    /// `rb`, `rc` or `re` the branch currents at the internal nodes equal the
    /// series-resistance currents by KCL, so the same formula holds.
    fn ac_currents(&self, x: &[Complex64], omega: f64, out: &mut [Complex64]) -> bool {
        let ss = self.small_signal(omega);
        let i_be = ac_branch_current(x, &ss.be);
        let i_bc = ac_branch_current(x, &ss.bc);
        let i_ce = ac_branch_current(x, &ss.ce);
        let i_bx = ac_branch_current(x, &ss.bx);
        let i_cs = ac_branch_current(x, &ss.cs);
        let terminal = [i_ce - i_bc - i_bx - i_cs, i_be + i_bc + i_bx, -i_be - i_ce];
        for (slot, value) in out.iter_mut().zip(terminal) {
            *slot = value;
        }
        true
    }

    fn currents(&self, out: &mut [f64]) {
        let (ibe, ibc) = self.junction_currents();
        let (ict, ibx, ics) = (self.state.ict, self.charging.bx.0, self.charging.cs.0);
        let sign = self.params.sign;
        let terminal = [
            sign * (ict - ibc - ibx - ics),
            sign * (ibe + ibc + ibx),
            sign * (-ibe - ict),
        ];
        for (slot, value) in out.iter_mut().zip(terminal) {
            *slot = value;
        }
    }

    /// SPICE current-prediction test on the static base and collector
    /// currents. Charging currents are left out: their companion
    /// conductances grow as `C/h` and would demand voltage changes below
    /// rounding at tiny timesteps; truncation-error control covers charge.
    fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
        let (vbe, vbc) = self.junction_voltages(x);
        let (dvbe, dvbc) = (vbe - self.state.vbe, vbc - self.state.vbc);
        let st = &self.state;
        let base_delta = st.gpi * dvbe + st.gmu * dvbc;
        let collector_delta = (st.gm + st.go) * dvbe - (st.go + st.gmu) * dvbc;
        current_converged(st.ibe + st.ibc, base_delta, options)
            && current_converged(st.ict - st.ibc, collector_delta, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::IntegrationMethod;
    use crate::circuit::{Circuit, Device, Element};
    use crate::integration::Integrator;
    use crate::mna::Layout;
    use crate::models::Polarity;
    use crate::solver::System;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs()) + 1e-30
    }

    /// A DC-only model (no charges, so loading needs no integration history).
    fn resistive_model(polarity: Polarity) -> BjtModel {
        BjtModel {
            polarity,
            is: 1e-14,
            bf: 200.0,
            br: 3.0,
            vaf: 80.0,
            ikf: 0.2,
            ise: 1e-14,
            rb: 20.0,
            rbm: 5.0,
            irb: 1e-3,
            rc: 1.5,
            re: 0.4,
            ..BjtModel::default()
        }
    }

    struct Loaded {
        bjt: Bjt,
        matrix: Vec<Vec<f64>>,
        rhs: Vec<f64>,
        x: Vec<f64>,
    }

    /// Sets up one transistor and loads it repeatedly at fixed terminal
    /// voltages `[c, b, e]` (internal nodes at the terminal voltages) until
    /// no junction is limited any more.
    fn load_at(model: &BjtModel, v: [f64; 3]) -> Loaded {
        let mut circuit = Circuit::new();
        let nodes = [circuit.node("c"), circuit.node("b"), circuit.node("e")];
        circuit.add(Element::new("Q1", nodes, Device::bjt(model.clone())));
        let options = SimOptions::default();
        let mut layout = Layout::new(&circuit);
        let mut states = 0;
        let mut bjt = Bjt::new(&nodes, model, 1.0, &options);
        bjt.setup(&mut SetupContext {
            layout: &mut layout,
            state_count: &mut states,
            element: "Q1",
        });
        assert_eq!(states, 0);
        let mut x = vec![0.0; layout.size()];
        x[..3].copy_from_slice(&v);
        let n = bjt.unknowns;
        for (internal, terminal) in [
            (n.collector_internal, 0),
            (n.base_internal, 1),
            (n.emitter_internal, 2),
        ] {
            x[internal.unwrap()] = v[terminal];
        }
        let mut system = System::new(layout.size());
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, states);
        for _ in 0..50 {
            system.clear();
            let mut ctx = LoadContext::new(&x, &mut system, &mut integrator, &options);
            bjt.load(&mut ctx);
            if !ctx.limited {
                break;
            }
        }
        Loaded {
            bjt,
            matrix: system.to_dense(),
            rhs: system.rhs().to_vec(),
            x,
        }
    }

    #[test]
    fn internal_nodes_exist_only_for_nonzero_resistances() {
        let loaded = load_at(&resistive_model(Polarity::N), [5.0, 0.7, 0.0]);
        assert_eq!(loaded.x.len(), 6);
        let plain = load_at(&BjtModel::default(), [5.0, 0.7, 0.0]);
        assert_eq!(plain.x.len(), 3);
    }

    #[test]
    fn terminal_currents_obey_kcl_and_forward_active_signs() {
        let loaded = load_at(&resistive_model(Polarity::N), [5.0, 0.7, 0.0]);
        let mut i = [0.0; 3];
        loaded.bjt.currents(&mut i);
        assert!(i[0] > 0.0 && i[1] > 0.0 && i[2] < 0.0, "{i:?}");
        assert!(i.iter().sum::<f64>().abs() <= 1e-15 * i[2].abs());
        assert!(i[0] / i[1] > 50.0);
    }

    #[test]
    fn pnp_mirrors_npn() {
        let npn = load_at(&resistive_model(Polarity::N), [5.0, 0.7, 0.0]);
        let pnp = load_at(&resistive_model(Polarity::P), [-5.0, -0.7, 0.0]);
        let (mut i_n, mut i_p) = ([0.0; 3], [0.0; 3]);
        npn.bjt.currents(&mut i_n);
        pnp.bjt.currents(&mut i_p);
        for k in 0..3 {
            assert!(close(i_p[k], -i_n[k], 1e-14), "terminal {k}");
        }
        assert_eq!(npn.matrix, pnp.matrix);
        for (a, b) in npn.rhs.iter().zip(&pnp.rhs) {
            assert!(close(*a, -*b, 1e-14));
        }
    }

    #[test]
    fn linearised_stamp_reproduces_currents_at_the_operating_point() {
        // At the linearisation point (A·x − b)_k is the current the device
        // draws from node k. Each terminal and its internal node together
        // carry exactly the reported terminal current.
        let loaded = load_at(&resistive_model(Polarity::N), [5.0, 0.72, 0.0]);
        let mut i = [0.0; 3];
        loaded.bjt.currents(&mut i);
        let residual: Vec<f64> = loaded
            .matrix
            .iter()
            .zip(&loaded.rhs)
            .map(|(row, b)| row.iter().zip(&loaded.x).map(|(a, x)| a * x).sum::<f64>() - b)
            .collect();
        let n = loaded.bjt.unknowns;
        let groups = [
            (n.collector, n.collector_internal),
            (n.base, n.base_internal),
            (n.emitter, n.emitter_internal),
        ];
        for (k, (terminal, internal)) in groups.into_iter().enumerate() {
            let drawn = residual[terminal.unwrap()] + residual[internal.unwrap()];
            assert!(
                close(drawn, i[k], 1e-9),
                "terminal {k}: {drawn} vs {}",
                i[k]
            );
        }
    }

    #[test]
    fn converged_accepts_the_linearisation_point_only() {
        let loaded = load_at(&resistive_model(Polarity::N), [5.0, 0.7, 0.0]);
        let options = SimOptions::default();
        assert!(loaded.bjt.converged(&loaded.x, &options));
        let mut moved = loaded.x.clone();
        let b = loaded.bjt.unknowns.base_internal.unwrap();
        moved[b] += 1e-4;
        assert!(!loaded.bjt.converged(&moved, &options));
    }

    #[test]
    fn init_junctions_starts_at_vcrit_and_flags_limiting() {
        let model = BjtModel::default();
        let mut circuit = Circuit::new();
        let nodes = [circuit.node("c"), circuit.node("b"), circuit.node("e")];
        let options = SimOptions::default();
        let mut layout = Layout::new(&circuit);
        let mut states = 0;
        let mut bjt = Bjt::new(&nodes, &model, 1.0, &options);
        bjt.setup(&mut SetupContext {
            layout: &mut layout,
            state_count: &mut states,
            element: "Q1",
        });
        let x = vec![0.0; layout.size()];
        let mut system = System::new(layout.size());
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, states);
        let mut ctx = LoadContext::new(&x, &mut system, &mut integrator, &options);
        ctx.init_junctions = true;
        bjt.load(&mut ctx);
        assert!(ctx.limited);
        assert_eq!(bjt.state.vbe, bjt.params.vcrit);
        assert_eq!(bjt.state.vbc, 0.0);
    }
}
