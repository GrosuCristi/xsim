//! Shichman–Hodges junction FET (SPICE3 `jfetload`) with the Sydney
//! University doping-tail parameter `b`.
//!
//! Terminals are `[drain, gate, source]`. Equations are written for an
//! N-channel device; a P-channel device flips every voltage and current.
//!
//! * Channel (`vds ≥ 0`, `vgst = vgs − vto > 0`), with
//!   `bf = (1 − b)/(pb − vto)` and `β' = β·(1 + λ·vds)`:
//!   - saturation (`vds ≥ vgst`): `id = β'·vgst²·(b + bf·vgst)`,
//!   - linear: `id = β'·vds·(vds·(bf·vds − b) + vgst·(2b + 3bf·(vgst − vds)))`.
//!
//!   With `b = 1` these are the classic `β'·vgst²` and
//!   `β'·vds·(2·vgst − vds)`. For `vds < 0` source and drain swap roles.
//! * Gate-source and gate-drain junctions: ideal diodes with saturation
//!   current `is`, `gmin` in parallel and depletion charges with grading 0.5.
//! * Optional ohmic `rd` and `rs` behind internal nodes `<name>:d'` and
//!   `<name>:s'`.
//!
//! Temperature: `vto` shifts by `vtotc` per kelvin, `beta` scales by
//! `1.01^(betatce·ΔT)`, `is` follows the PSpice JFET law with `EG = 1.11 eV`
//! and `XTI = 3`, and the gate junction potential and capacitances follow the
//! SPICE3 junction equations.

use super::semiconductor::{
    AcControl, Control, Depletion, ZERO_CELSIUS, ac_branch_current, critical_voltage,
    current_converged, fetlim, finite_or, junction_at_temperature, nominal_kelvin, non_negative_or,
    pnjlim, positive_or, saturation_log_factor, series_conductance, shockley, solution_value,
    stamp_ac_branch, stamp_branch, thermal_voltage,
};
use super::{AcContext, DeviceInstance, LoadContext, SetupContext, StateSlot};
use crate::analysis::SimOptions;
use crate::circuit::NodeIndex;
use crate::mna::Unknown;
use crate::models::JfetModel;
use num_complex::Complex64;

/// Gate-junction bandgap and saturation-current exponent of the PSpice JFET
/// temperature law; the SPICE JFET model card has no parameters for them.
const GATE_BANDGAP: f64 = 1.11;
const GATE_XTI: f64 = 3.0;

/// SPICE `MODEINITJCT` starting voltage of both gate junctions.
const INITIAL_GATE_VOLTAGE: f64 = -1.0;

/// Channel current and its derivatives in polarity-normalised form.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Channel {
    /// Drain-to-source current (A).
    id: f64,
    /// `∂id/∂vgs` (S).
    gm: f64,
    /// `∂id/∂vds` (S).
    gds: f64,
}

/// Temperature- and area-adjusted parameters of one JFET instance.
#[derive(Debug, Clone, PartialEq)]
struct JfetParams {
    sign: f64,
    vto: f64,
    beta: f64,
    lambda: f64,
    b: f64,
    /// Doping-tail coefficient `(1 − b)/(pb − vto)`.
    b_factor: f64,
    drain: Option<f64>,
    source: Option<f64>,
    is: f64,
    vt: f64,
    vcrit: f64,
    gs_depletion: Depletion,
    gd_depletion: Depletion,
}

/// Normalised junction voltages and the model evaluated there.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct JfetState {
    vgs: f64,
    vgd: f64,
    /// Static gate-source and gate-drain junction currents including gmin.
    igs: f64,
    ggs: f64,
    igd: f64,
    ggd: f64,
    channel: Channel,
    qgs: f64,
    cgs: f64,
    qgd: f64,
    cgd: f64,
}

impl JfetParams {
    fn new(model: &JfetModel, area: f64, celsius: f64) -> Self {
        let defaults = JfetModel::default();
        let temp = celsius + ZERO_CELSIUS;
        let tnom = nominal_kelvin(model.tnom, defaults.tnom);
        let delta_t = temp - tnom;
        let vt = thermal_voltage(celsius);

        let vto_nominal = finite_or(model.vto, defaults.vto);
        let pb = positive_or(model.pb, defaults.pb);
        let b = finite_or(model.b, defaults.b);
        let tail_span = pb - vto_nominal;
        let b_factor = if tail_span != 0.0 {
            (1.0 - b) / tail_span
        } else {
            0.0
        };

        let is = non_negative_or(model.is, defaults.is)
            * area
            * saturation_log_factor(GATE_BANDGAP, GATE_XTI, tnom, temp).exp();
        let fc = non_negative_or(model.fc, defaults.fc);
        let (cgs, pb_t) =
            junction_at_temperature(non_negative_or(model.cgs, 0.0) * area, pb, 0.5, tnom, temp);
        let (cgd, _) =
            junction_at_temperature(non_negative_or(model.cgd, 0.0) * area, pb, 0.5, tnom, temp);

        Self {
            sign: model.polarity.sign(),
            vto: vto_nominal + finite_or(model.vtotc, 0.0) * delta_t,
            beta: non_negative_or(model.beta, defaults.beta)
                * area
                * 1.01f64.powf(finite_or(model.betatce, 0.0) * delta_t),
            lambda: finite_or(model.lambda, 0.0),
            b,
            b_factor,
            drain: series_conductance(model.rd, area),
            source: series_conductance(model.rs, area),
            is,
            vt,
            vcrit: critical_voltage(is, vt),
            gs_depletion: Depletion::new(cgs, pb_t, 0.5, fc),
            gd_depletion: Depletion::new(cgd, pb_t, 0.5, fc),
        }
    }

    /// Channel current for `vds ≥ 0` given `vgst = vgs − vto`, with partial
    /// derivatives with respect to `vgst` and `vds`.
    fn forward_channel(&self, vgst: f64, vds: f64) -> Channel {
        if vgst <= 0.0 {
            return Channel::default();
        }
        let (b, bf) = (self.b, self.b_factor);
        let beta_eff = self.beta * (1.0 + self.lambda * vds);
        let (shape, d_vgst, d_vds) = if vgst >= vds {
            // Linear region.
            let a = 2.0 * b + 3.0 * bf * (vgst - vds);
            let shape = vds * (vds * (bf * vds - b) + vgst * a);
            (shape, vds * (a + 3.0 * bf * vgst), (vgst - vds) * a)
        } else {
            // Saturation.
            let shape = vgst * vgst * (b + bf * vgst);
            (shape, vgst * (2.0 * b + 3.0 * bf * vgst), 0.0)
        };
        Channel {
            id: beta_eff * shape,
            gm: beta_eff * d_vgst,
            gds: beta_eff * d_vds + self.beta * self.lambda * shape,
        }
    }

    /// Drain-to-source channel current at normalised `vgs`, `vgd`.
    fn channel(&self, vgs: f64, vgd: f64) -> Channel {
        let vds = vgs - vgd;
        if vds >= 0.0 {
            self.forward_channel(vgs - self.vto, vds)
        } else {
            // Inverse mode: the drain acts as source. With F(vgdt, −vds) the
            // mirrored current, id = −F, ∂id/∂vgs = −F_u, ∂id/∂vds = F_u + F_w.
            let mirrored = self.forward_channel(vgd - self.vto, -vds);
            Channel {
                id: -mirrored.id,
                gm: -mirrored.gm,
                gds: mirrored.gm + mirrored.gds,
            }
        }
    }

    fn evaluate(&self, vgs: f64, vgd: f64, gmin: f64) -> JfetState {
        let (igs, ggs) = shockley(self.is, vgs, self.vt);
        let (igd, ggd) = shockley(self.is, vgd, self.vt);
        let (qgs, cgs) = self.gs_depletion.charge(vgs);
        let (qgd, cgd) = self.gd_depletion.charge(vgd);
        JfetState {
            vgs,
            vgd,
            igs: igs + gmin * vgs,
            ggs: ggs + gmin,
            igd: igd + gmin * vgd,
            ggd: ggd + gmin,
            channel: self.channel(vgs, vgd),
            qgs,
            cgs,
            qgd,
            cgd,
        }
    }

    /// pn-junction then FET limiting of one gate voltage.
    fn limit(&self, vnew: f64, vold: f64) -> (f64, bool) {
        let (v, junction_limited) = pnjlim(vnew, vold, self.vt, self.vcrit);
        let (v, fet_limited) = fetlim(v, vold, self.vto);
        (v, junction_limited || fet_limited)
    }
}

/// Unknowns of the external terminals and internal nodes.
#[derive(Debug, Clone, Copy, Default)]
struct Nodes {
    drain: Unknown,
    gate: Unknown,
    source: Unknown,
    drain_internal: Unknown,
    source_internal: Unknown,
}

/// Small-signal branch currents: gate `→ s'`, gate `→ d'` and the channel
/// `d' → s'`, each the sum of its controls.
#[derive(Debug, Clone, Copy)]
struct SmallSignal {
    gs: [AcControl; 1],
    gd: [AcControl; 1],
    ds: [AcControl; 2],
}

#[derive(Debug)]
pub(crate) struct Jfet {
    nodes: [NodeIndex; 3],
    params: JfetParams,
    unknowns: Nodes,
    gs_slot: Option<StateSlot>,
    gd_slot: Option<StateSlot>,
    state: JfetState,
    /// Charging currents and companion conductances of the gate charges.
    gs_charge: (f64, f64),
    gd_charge: (f64, f64),
}

impl Jfet {
    pub(crate) fn new(
        nodes: &[NodeIndex],
        model: &JfetModel,
        area: f64,
        options: &SimOptions,
    ) -> Self {
        Self {
            nodes: std::array::from_fn(|i| nodes[i]),
            params: JfetParams::new(model, area, options.temperature),
            unknowns: Nodes::default(),
            gs_slot: None,
            gd_slot: None,
            state: JfetState::default(),
            gs_charge: (0.0, 0.0),
            gd_charge: (0.0, 0.0),
        }
    }

    /// The small-signal branches at the last load and angular frequency
    /// `omega` (actual node voltages as controls, for both polarities).
    fn small_signal(&self, omega: f64) -> SmallSignal {
        let n = self.unknowns;
        let st = &self.state;
        let (g, d, s) = (n.gate, n.drain_internal, n.source_internal);
        SmallSignal {
            gs: [AcControl::rc(g, s, st.ggs, st.cgs, omega)],
            gd: [AcControl::rc(g, d, st.ggd, st.cgd, omega)],
            ds: [
                AcControl::rc(g, s, st.channel.gm, 0.0, omega),
                AcControl::rc(d, s, st.channel.gds, 0.0, omega),
            ],
        }
    }

    /// Normalised gate-source and gate-drain currents including charging.
    fn gate_currents(&self) -> (f64, f64) {
        (
            self.state.igs + self.gs_charge.0,
            self.state.igd + self.gd_charge.0,
        )
    }
}

impl DeviceInstance for Jfet {
    fn setup(&mut self, ctx: &mut SetupContext) {
        let [drain, gate, source] = self.nodes.map(|n| ctx.node(n));
        let drain_internal = match self.params.drain {
            Some(_) => ctx.internal_node("d'"),
            None => drain,
        };
        let source_internal = match self.params.source {
            Some(_) => ctx.internal_node("s'"),
            None => source,
        };
        self.unknowns = Nodes {
            drain,
            gate,
            source,
            drain_internal,
            source_internal,
        };
        if self.params.gs_depletion.is_present() {
            self.gs_slot = Some(ctx.state());
        }
        if self.params.gd_depletion.is_present() {
            self.gd_slot = Some(ctx.state());
        }
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let n = self.unknowns;
        let p = &self.params;
        let (vgs, vgd, limited) = if ctx.init_junctions {
            (INITIAL_GATE_VOLTAGE, INITIAL_GATE_VOLTAGE, true)
        } else {
            let vgs = p.sign * (ctx.v(n.gate) - ctx.v(n.source_internal));
            let vgd = p.sign * (ctx.v(n.gate) - ctx.v(n.drain_internal));
            let (vgs, gs_limited) = p.limit(vgs, self.state.vgs);
            let (vgd, gd_limited) = p.limit(vgd, self.state.vgd);
            (vgs, vgd, gs_limited || gd_limited)
        };
        ctx.limited |= limited;

        let st = p.evaluate(vgs, vgd, ctx.gmin);
        self.gs_charge = match self.gs_slot {
            Some(slot) => ctx.integrate(slot, st.qgs, st.cgs),
            None => (0.0, 0.0),
        };
        self.gd_charge = match self.gd_slot {
            Some(slot) => ctx.integrate(slot, st.qgd, st.cgd),
            None => (0.0, 0.0),
        };
        self.state = st;
        let (igs, igd) = self.gate_currents();
        let sign = p.sign;
        let (g, d, s) = (n.gate, n.drain_internal, n.source_internal);
        stamp_branch(
            ctx,
            sign,
            g,
            s,
            igs,
            &[Control::new(g, s, st.ggs + self.gs_charge.1, vgs)],
        );
        stamp_branch(
            ctx,
            sign,
            g,
            d,
            igd,
            &[Control::new(g, d, st.ggd + self.gd_charge.1, vgd)],
        );
        let ch = st.channel;
        stamp_branch(
            ctx,
            sign,
            d,
            s,
            ch.id,
            &[
                Control::new(g, s, ch.gm, vgs),
                Control::new(d, s, ch.gds, vgs - vgd),
            ],
        );
        if let Some(gd) = p.drain {
            ctx.stamp_conductance(n.drain, d, gd);
        }
        if let Some(gs) = p.source {
            ctx.stamp_conductance(n.source, s, gs);
        }
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let n = self.unknowns;
        let ss = self.small_signal(ctx.omega);
        let (g, d, s) = (n.gate, n.drain_internal, n.source_internal);
        stamp_ac_branch(ctx, g, s, &ss.gs);
        stamp_ac_branch(ctx, g, d, &ss.gd);
        stamp_ac_branch(ctx, d, s, &ss.ds);
        if let Some(gd) = self.params.drain {
            ctx.stamp_conductance(n.drain, d, gd);
        }
        if let Some(gs) = self.params.source {
            ctx.stamp_conductance(n.source, s, gs);
        }
    }

    /// The branch currents of the stamps of [`Self::load_ac`], combined per
    /// terminal as [`Self::currents`] combines the DC ones (with `rd`, `rs`
    /// the internal-node branch currents equal the resistance currents).
    fn ac_currents(&self, x: &[Complex64], omega: f64, out: &mut [Complex64]) -> bool {
        let ss = self.small_signal(omega);
        let i_gs = ac_branch_current(x, &ss.gs);
        let i_gd = ac_branch_current(x, &ss.gd);
        let i_ds = ac_branch_current(x, &ss.ds);
        let terminal = [i_ds - i_gd, i_gs + i_gd, -i_ds - i_gs];
        for (slot, value) in out.iter_mut().zip(terminal) {
            *slot = value;
        }
        true
    }

    fn currents(&self, out: &mut [f64]) {
        let (igs, igd) = self.gate_currents();
        let id = self.state.channel.id;
        let sign = self.params.sign;
        let terminal = [sign * (id - igd), sign * (igs + igd), sign * (-id - igs)];
        for (slot, value) in out.iter_mut().zip(terminal) {
            *slot = value;
        }
    }

    /// SPICE current-prediction test on the static gate and drain currents.
    /// Charging currents are left out: their companion conductances grow as
    /// `C/h` and would demand voltage changes below rounding at tiny
    /// timesteps; truncation-error control covers charge.
    fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
        let n = self.unknowns;
        let sign = self.params.sign;
        let vg = solution_value(x, n.gate);
        let dvgs = sign * (vg - solution_value(x, n.source_internal)) - self.state.vgs;
        let dvgd = sign * (vg - solution_value(x, n.drain_internal)) - self.state.vgd;
        let st = &self.state;
        let ch = st.channel;
        let gate_delta = st.ggs * dvgs + st.ggd * dvgd;
        let drain_delta = ch.gm * dvgs + ch.gds * (dvgs - dvgd) - st.ggd * dvgd;
        current_converged(st.igs + st.igd, gate_delta, options)
            && current_converged(ch.id - st.igd, drain_delta, options)
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

    fn model() -> JfetModel {
        JfetModel {
            vto: -2.0,
            beta: 1.3e-3,
            lambda: 0.02,
            pb: 0.8,
            ..JfetModel::default()
        }
    }

    fn params(model: &JfetModel) -> JfetParams {
        JfetParams::new(model, 1.0, 27.0)
    }

    #[test]
    fn classic_square_law_in_saturation_and_linear_regions() {
        let p = params(&model());
        let (beta, lambda) = (1.3e-3, 0.02);
        // Saturation: vgs = −1 (vgst = 1), vds = 5.
        let ch = p.channel(-1.0, -6.0);
        assert!(close(ch.id, beta * (1.0 + lambda * 5.0) * 1.0, 1e-14));
        // Linear: vgs = 0 (vgst = 2), vds = 0.5.
        let ch = p.channel(0.0, -0.5);
        assert!(close(
            ch.id,
            beta * (1.0 + lambda * 0.5) * 0.5 * (2.0 * 2.0 - 0.5),
            1e-14
        ));
        // Cut-off.
        assert_eq!(p.channel(-2.5, -7.5), Channel::default());
    }

    #[test]
    fn doping_tail_parameter_changes_saturation_law() {
        let m = JfetModel { b: 0.6, ..model() };
        let p = params(&m);
        let (vgst, vds) = (1.2, 4.0);
        let bf = (1.0 - 0.6) / (0.8 + 2.0);
        let expected = 1.3e-3 * (1.0 + 0.02 * vds) * vgst * vgst * (0.6 + bf * vgst);
        assert!(close(
            p.channel(vgst - 2.0, vgst - 2.0 - vds).id,
            expected,
            1e-14
        ));
    }

    #[test]
    fn channel_is_continuous_at_saturation_boundary() {
        for &b in &[1.0, 0.5, 1.4] {
            let p = params(&JfetModel { b, ..model() });
            let vgs = -0.8;
            let vgst = vgs + 2.0;
            let lin = p.channel(vgs, vgs - (vgst - 1e-10));
            let sat = p.channel(vgs, vgs - (vgst + 1e-10));
            assert!(close(lin.id, sat.id, 1e-8), "b={b}");
            assert!(close(lin.gm, sat.gm, 1e-8), "b={b}");
        }
    }

    #[test]
    fn channel_derivatives_match_finite_differences() {
        for &b in &[1.0, 0.7] {
            let p = params(&JfetModel { b, ..model() });
            for &(vgs, vds) in &[
                (-1.0, 5.0),
                (-0.5, 0.3),
                (0.2, 1.0),
                (-1.0, -0.4),
                (-0.5, -3.0),
            ] {
                let vgd = vgs - vds;
                let h = 1e-7;
                let ch = p.channel(vgs, vgd);
                // ∂/∂vgs at constant vds moves vgs and vgd together.
                let gm_fd =
                    (p.channel(vgs + h, vgd + h).id - p.channel(vgs - h, vgd - h).id) / (2.0 * h);
                let gds_fd = (p.channel(vgs, vgd - h).id - p.channel(vgs, vgd + h).id) / (2.0 * h);
                assert!(
                    close(ch.gm, gm_fd, 1e-6),
                    "b={b} vgs={vgs} vds={vds}: {} vs {gm_fd}",
                    ch.gm
                );
                assert!(
                    close(ch.gds, gds_fd, 1e-6),
                    "b={b} vgs={vgs} vds={vds}: {} vs {gds_fd}",
                    ch.gds
                );
            }
        }
    }

    #[test]
    fn swapping_drain_and_source_reverses_current() {
        // Exchanging the drain and source exchanges vgs and vgd.
        let p = params(&JfetModel { b: 0.8, ..model() });
        for &(vgs, vgd) in &[(-1.0, -4.0), (-0.5, -0.7), (0.1, -2.5)] {
            let forward = p.channel(vgs, vgd);
            let swapped = p.channel(vgd, vgs);
            assert!(close(swapped.id, -forward.id, 1e-15), "vgs={vgs} vgd={vgd}");
        }
    }

    #[test]
    fn gate_junctions_are_ideal_diodes_with_gmin() {
        let p = params(&model());
        let st = p.evaluate(0.4, -3.0, 1e-12);
        let vt = thermal_voltage(27.0);
        assert!(close(st.igs, 1e-14 * (0.4 / vt).exp_m1() + 0.4e-12, 1e-12));
        assert!(close(st.igd, 1e-14 * (-3.0 / vt).exp_m1() - 3e-12, 1e-12));
    }

    #[test]
    fn temperature_shifts_threshold_and_beta() {
        let m = JfetModel {
            vtotc: -2.5e-3,
            betatce: -0.5,
            ..model()
        };
        let hot = JfetParams::new(&m, 1.0, 77.0);
        assert!(close(hot.vto, -2.0 - 2.5e-3 * 50.0, 1e-14));
        assert!(close(hot.beta, 1.3e-3 * 1.01f64.powf(-0.5 * 50.0), 1e-14));
        assert!(hot.is > params(&m).is);
        assert_eq!(params(&m).vto, -2.0);
    }

    #[test]
    fn gate_charges_follow_square_root_grading() {
        let m = JfetModel {
            cgs: 2e-12,
            cgd: 1e-12,
            ..model()
        };
        let st = params(&m).evaluate(-2.0, -6.0, 0.0);
        assert!(close(st.cgs, 2e-12 / (1.0 + 2.0 / 0.8f64).sqrt(), 1e-12));
        assert!(close(st.cgd, 1e-12 / (1.0 + 6.0 / 0.8f64).sqrt(), 1e-12));
    }

    /// Loads one JFET (no capacitances) at node voltages `[d, g, s]` and
    /// returns its terminal currents and the assembled matrix.
    fn load_at(model: &JfetModel, v: [f64; 3]) -> ([f64; 3], Vec<Vec<f64>>, Vec<f64>) {
        let mut circuit = Circuit::new();
        let nodes = [circuit.node("d"), circuit.node("g"), circuit.node("s")];
        circuit.add(Element::new("J1", nodes, Device::jfet(model.clone())));
        let options = SimOptions::default();
        let mut layout = Layout::new(&circuit);
        let mut states = 0;
        let mut jfet = Jfet::new(&nodes, model, 1.0, &options);
        jfet.setup(&mut SetupContext {
            layout: &mut layout,
            state_count: &mut states,
            element: "J1",
        });
        let mut system = System::new(layout.size());
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, states);
        // Two loads: the first moves the stored iterate next to `v` so the
        // second is not limited.
        for _ in 0..30 {
            let mut ctx = LoadContext::new(&v, &mut system, &mut integrator, &options);
            jfet.load(&mut ctx);
            if !ctx.limited {
                break;
            }
            system.clear();
        }
        let mut out = [0.0; 3];
        jfet.currents(&mut out);
        (out, system.to_dense(), system.rhs().to_vec())
    }

    #[test]
    fn p_channel_mirrors_n_channel() {
        let n = model();
        let p = JfetModel {
            polarity: Polarity::P,
            ..model()
        };
        let (i_n, a_n, rhs_n) = load_at(&n, [5.0, -1.0, 0.0]);
        let (i_p, a_p, rhs_p) = load_at(&p, [-5.0, 1.0, 0.0]);
        for k in 0..3 {
            assert!(close(i_p[k], -i_n[k], 1e-14));
            assert!(close(rhs_p[k], -rhs_n[k], 1e-14));
        }
        assert_eq!(a_n, a_p);
        // Terminal currents obey KCL.
        assert!((i_n.iter().sum::<f64>()).abs() < 1e-18);
        assert!(i_n[0] > 0.0);
    }
}
