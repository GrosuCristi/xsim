//! Junction diode (SPICE level 1, Q83).
//!
//! The intrinsic junction carries
//!
//! * the Shockley current `is·(e^(v/(n·Vt)) − 1)`,
//! * with a finite `bv`, the breakdown current `−is·e^(−(v + BV')/(nbv·Vt))`,
//!   where the knee `BV'` is solved in closed form so the total current at
//!   `v = −bv` is exactly `−ibv` (the SPICE3 "BV adjustment"),
//! * `gmin·v` in parallel (numerical aid, raised by GMIN stepping),
//! * the charge `tt·i_shockley + q_depletion(v)` (diffusion plus depletion).
//!
//! An ohmic `rs` puts the junction behind an internal node `<name>:a'`.
//! Temperature scales `is` (through `eg`, `xti`), `vj` and `cjo` with the
//! SPICE3 equations; `bv` is temperature independent.
//!
//! Unlike SPICE3, which splices a cubic reverse-bias approximation and a
//! separate breakdown exponential, the current is one smooth expression in
//! every region. The two differ by less than `is` away from breakdown.

use super::semiconductor::{
    Depletion, ZERO_CELSIUS, critical_voltage, current_converged, junction_at_temperature,
    nominal_kelvin, non_negative_or, pnjlim, positive_or, saturation_log_factor, scaled_exp,
    series_conductance, shockley, solution_value, thermal_voltage,
};
use super::{AcContext, DeviceInstance, LoadContext, SetupContext, StateSlot};
use crate::analysis::SimOptions;
use crate::circuit::NodeIndex;
use crate::mna::Unknown;
use crate::models::DiodeModel;
use num_complex::Complex64;

/// Reverse breakdown branch of the junction.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Breakdown {
    /// Effective knee voltage `BV'` (V, positive).
    knee: f64,
    /// `nbv·Vt` (V).
    nvt: f64,
    /// Critical voltage of the breakdown exponential, for step limiting.
    vcrit: f64,
}

impl Breakdown {
    /// Breakdown with the knee solved so that `I(−bv) = −ibv` exactly, given
    /// the forward saturation current `is` and emission voltage `forward_nvt`.
    /// Returns `None` when `bv` is not a positive finite voltage.
    fn new(bv: f64, ibv: f64, nvt: f64, is: f64, forward_nvt: f64) -> Option<Self> {
        if !(bv.is_finite() && bv > 0.0) {
            return None;
        }
        // At v = −bv the forward term contributes −is·(1 − e^(−bv/n·Vt)); the
        // breakdown term must supply the rest of −ibv.
        let forward = -(-bv / forward_nvt).exp_m1();
        let ratio = ibv / is - forward;
        // When ibv does not exceed the saturation current there is no knee to
        // match; SPICE then also keeps the nominal voltage.
        let knee = if ratio > 0.0 && is > 0.0 {
            bv - nvt * ratio.ln()
        } else {
            bv
        };
        Some(Self {
            knee,
            nvt,
            vcrit: critical_voltage(is, nvt),
        })
    }
}

/// Temperature- and area-adjusted parameters of one diode instance.
#[derive(Debug, Clone, PartialEq)]
struct DiodeParams {
    is: f64,
    /// `n·Vt` (V).
    nvt: f64,
    vcrit: f64,
    /// Conductance of the series resistance, if any.
    series: Option<f64>,
    breakdown: Option<Breakdown>,
    depletion: Depletion,
    tt: f64,
}

/// Junction quantities at one bias point (DC part only; the charging current
/// depends on the integration history and is stored separately).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct JunctionState {
    /// Junction voltage the model was evaluated at (after limiting).
    vd: f64,
    /// Static current including `gmin·vd` (A).
    current: f64,
    /// `d(current)/d(vd)` (S).
    conductance: f64,
    /// Diffusion plus depletion charge (C).
    charge: f64,
    /// `d(charge)/d(vd)` (F).
    capacitance: f64,
}

impl DiodeParams {
    fn new(model: &DiodeModel, area: f64, celsius: f64) -> Self {
        let defaults = DiodeModel::default();
        let temp = celsius + ZERO_CELSIUS;
        let tnom = nominal_kelvin(model.tnom, defaults.tnom);
        let vt = thermal_voltage(celsius);
        let n = positive_or(model.n, defaults.n);
        let nvt = n * vt;

        let log_factor = saturation_log_factor(
            positive_or(model.eg, defaults.eg),
            non_negative_or(model.xti, defaults.xti),
            tnom,
            temp,
        );
        let is = non_negative_or(model.is, defaults.is) * area * (log_factor / n).exp();

        let m = non_negative_or(model.m, defaults.m);
        let (cj0, vj) = junction_at_temperature(
            non_negative_or(model.cjo, 0.0) * area,
            positive_or(model.vj, defaults.vj),
            m,
            tnom,
            temp,
        );
        let fc = non_negative_or(model.fc, defaults.fc);
        let breakdown = Breakdown::new(
            model.bv,
            positive_or(model.ibv, defaults.ibv) * area,
            positive_or(model.nbv, defaults.nbv) * vt,
            is,
            nvt,
        );
        Self {
            is,
            nvt,
            vcrit: critical_voltage(is, nvt),
            series: series_conductance(model.rs, area),
            breakdown,
            depletion: Depletion::new(cj0, vj, m, fc),
            tt: non_negative_or(model.tt, 0.0),
        }
    }

    /// Whether the junction stores charge (needs an integrated state).
    fn is_reactive(&self) -> bool {
        self.tt > 0.0 || self.depletion.is_present()
    }

    /// SPICE `MODEINITJCT` starting voltage.
    fn initial_voltage(&self) -> f64 {
        if self.vcrit.is_finite() {
            self.vcrit
        } else {
            0.0
        }
    }

    /// Newton step limiting of the junction voltage. Deep in reverse bias the
    /// breakdown exponential is limited in its own (mirrored) coordinate.
    fn limit(&self, vnew: f64, vold: f64) -> (f64, bool) {
        if let Some(bd) = self.breakdown
            && vnew < (10.0 * bd.nvt - bd.knee).min(0.0)
        {
            let (mirrored, limited) =
                pnjlim(-(vnew + bd.knee), -(vold + bd.knee), bd.nvt, bd.vcrit);
            return (-(mirrored + bd.knee), limited);
        }
        pnjlim(vnew, vold, self.nvt, self.vcrit)
    }

    fn evaluate(&self, vd: f64, gmin: f64) -> JunctionState {
        let (forward, g_forward) = shockley(self.is, vd, self.nvt);
        let (reverse, g_reverse) = match self.breakdown {
            Some(bd) => {
                let (e, de) = scaled_exp(self.is, -(vd + bd.knee) / bd.nvt);
                (-e, de / bd.nvt)
            }
            None => (0.0, 0.0),
        };
        let (q_depletion, c_depletion) = self.depletion.charge(vd);
        JunctionState {
            vd,
            current: forward + reverse + gmin * vd,
            conductance: g_forward + g_reverse + gmin,
            charge: self.tt * forward + q_depletion,
            capacitance: self.tt * g_forward + c_depletion,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Diode {
    nodes: [NodeIndex; 2],
    params: DiodeParams,
    anode: Unknown,
    cathode: Unknown,
    /// Internal anode behind the series resistance (the anode without one).
    junction: Unknown,
    charge_slot: Option<StateSlot>,
    /// Model evaluation at the last load.
    state: JunctionState,
    /// Charging current `dq/dt` and its conductance at the last load.
    charge_current: f64,
    charge_conductance: f64,
}

impl Diode {
    pub(crate) fn new(
        nodes: &[NodeIndex],
        model: &DiodeModel,
        area: f64,
        options: &SimOptions,
    ) -> Self {
        Self {
            nodes: std::array::from_fn(|i| nodes[i]),
            params: DiodeParams::new(model, area, options.temperature),
            anode: None,
            cathode: None,
            junction: None,
            charge_slot: None,
            state: JunctionState::default(),
            charge_current: 0.0,
            charge_conductance: 0.0,
        }
    }

    fn total_current(&self) -> f64 {
        self.state.current + self.charge_current
    }

    /// Small-signal junction admittance at the last load.
    fn admittance(&self, omega: f64) -> Complex64 {
        Complex64::new(self.state.conductance, omega * self.state.capacitance)
    }
}

impl DeviceInstance for Diode {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.anode = ctx.node(self.nodes[0]);
        self.cathode = ctx.node(self.nodes[1]);
        self.junction = match self.params.series {
            Some(_) => ctx.internal_node("a'"),
            None => self.anode,
        };
        if self.params.is_reactive() {
            self.charge_slot = Some(ctx.state());
        }
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let (vd, limited) = if ctx.init_junctions {
            (self.params.initial_voltage(), true)
        } else {
            let proposed = ctx.v(self.junction) - ctx.v(self.cathode);
            self.params.limit(proposed, self.state.vd)
        };
        ctx.limited |= limited;

        self.state = self.params.evaluate(vd, ctx.gmin);
        (self.charge_current, self.charge_conductance) = match self.charge_slot {
            Some(slot) => ctx.integrate(slot, self.state.charge, self.state.capacitance),
            None => (0.0, 0.0),
        };
        ctx.stamp_nonlinear(
            self.junction,
            self.cathode,
            self.total_current(),
            self.state.conductance + self.charge_conductance,
            vd,
        );
        if let Some(g) = self.params.series {
            ctx.stamp_conductance(self.anode, self.junction, g);
        }
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        ctx.stamp_admittance(self.junction, self.cathode, self.admittance(ctx.omega));
        if let Some(g) = self.params.series {
            ctx.stamp_conductance(self.anode, self.junction, g);
        }
    }

    fn ac_currents(&self, x: &[Complex64], omega: f64, out: &mut [Complex64]) -> bool {
        let v = solution_value(x, self.junction) - solution_value(x, self.cathode);
        let i = self.admittance(omega) * v;
        for (slot, value) in out.iter_mut().zip([i, -i]) {
            *slot = value;
        }
        true
    }

    fn currents(&self, out: &mut [f64]) {
        let i = self.total_current();
        for (slot, value) in out.iter_mut().zip([i, -i]) {
            *slot = value;
        }
    }

    /// SPICE current-prediction test on the static junction current. The
    /// charging current is left out on purpose: its companion conductance
    /// grows as `C/h`, so at tiny timesteps the test would demand voltage
    /// changes below rounding, while the integrator's truncation-error
    /// control already governs the charge.
    fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
        let vd = solution_value(x, self.junction) - solution_value(x, self.cathode);
        current_converged(
            self.state.current,
            self.state.conductance * (vd - self.state.vd),
            options,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::IntegrationMethod;
    use crate::circuit::{Circuit, Device, Element, GROUND};
    use crate::integration::Integrator;
    use crate::mna::Layout;
    use crate::solver::System;

    const ROOM: f64 = 27.0;

    fn params(model: &DiodeModel) -> DiodeParams {
        DiodeParams::new(model, 1.0, ROOM)
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs())
    }

    #[test]
    fn forward_and_reverse_current_follow_shockley() {
        let model = DiodeModel {
            is: 3e-9,
            n: 1.8,
            ..DiodeModel::default()
        };
        let p = params(&model);
        let nvt = 1.8 * thermal_voltage(ROOM);
        for &v in &[-20.0, -1.0, -0.05, 0.0, 0.1, 0.45, 0.7] {
            let state = p.evaluate(v, 0.0);
            let expected = 3e-9 * ((v / nvt).exp() - 1.0);
            assert!(
                close(state.current, expected, 1e-12),
                "v={v}: {} vs {expected}",
                state.current
            );
        }
    }

    #[test]
    fn gmin_adds_a_parallel_conductance() {
        let p = params(&DiodeModel::default());
        let without = p.evaluate(-2.0, 0.0);
        let with = p.evaluate(-2.0, 1e-12);
        assert!(close(with.current - without.current, -2e-12, 1e-9));
        assert!(close(with.conductance - without.conductance, 1e-12, 1e-9));
    }

    #[test]
    fn breakdown_current_at_bv_equals_ibv() {
        for &(bv, ibv, nbv, is, n) in &[
            (5.1, 1e-3, 1.0, 1e-14, 1.0),
            (100.0, 1e-5, 1.0, 4e-9, 1.9),
            (12.0, 5e-3, 1.6, 1e-12, 1.2),
            (800.0, 2.0, 1.0, 5e-8, 1.5),
        ] {
            let model = DiodeModel {
                bv,
                ibv,
                nbv,
                is,
                n,
                ..DiodeModel::default()
            };
            let state = params(&model).evaluate(-bv, 0.0);
            // The knee is computed in closed form; the residual is rounding in
            // the exponent `(bv − BV')/(nbv·Vt)`, which grows with bv.
            assert!(
                close(state.current, -ibv, 1e-10),
                "bv={bv}: {} vs {}",
                state.current,
                -ibv
            );
        }
    }

    #[test]
    fn breakdown_current_grows_exponentially_beyond_the_knee() {
        let model = DiodeModel {
            bv: 5.1,
            ibv: 1e-3,
            ..DiodeModel::default()
        };
        let p = params(&model);
        let vt = thermal_voltage(ROOM);
        // One more thermal voltage of reverse bias multiplies the breakdown
        // current by e (the forward saturation part is negligible here).
        let i1 = p.evaluate(-5.1, 0.0).current;
        let i2 = p.evaluate(-5.1 - vt, 0.0).current;
        assert!(close(i2 / i1, std::f64::consts::E, 1e-9));
    }

    #[test]
    fn small_ibv_keeps_nominal_breakdown_voltage() {
        let b = Breakdown::new(50.0, 1e-16, 0.0258, 1e-14, 0.0258).unwrap();
        assert_eq!(b.knee, 50.0);
        assert!(Breakdown::new(f64::INFINITY, 1e-3, 0.0258, 1e-14, 0.0258).is_none());
        assert!(Breakdown::new(-3.0, 1e-3, 0.0258, 1e-14, 0.0258).is_none());
    }

    #[test]
    fn conductance_and_capacitance_are_exact_derivatives() {
        let model = DiodeModel {
            is: 1e-14,
            n: 1.3,
            cjo: 4e-12,
            vj: 0.6,
            m: 0.4,
            tt: 12e-9,
            bv: 30.0,
            ibv: 1e-4,
            ..DiodeModel::default()
        };
        let p = params(&model);
        for &v in &[-31.0, -30.0, -5.0, -0.2, 0.0, 0.25, 0.31, 0.6, 0.75] {
            let h = 1e-7;
            let lo = p.evaluate(v - h, 1e-12);
            let hi = p.evaluate(v + h, 1e-12);
            let st = p.evaluate(v, 1e-12);
            let g_fd = (hi.current - lo.current) / (2.0 * h);
            let c_fd = (hi.charge - lo.charge) / (2.0 * h);
            assert!(
                close(st.conductance, g_fd, 1e-5),
                "v={v}: g {} vs {g_fd}",
                st.conductance
            );
            assert!(
                close(st.capacitance, c_fd, 1e-5),
                "v={v}: c {} vs {c_fd}",
                st.capacitance
            );
        }
    }

    #[test]
    fn diffusion_charge_is_transit_time_times_current() {
        let model = DiodeModel {
            tt: 5e-9,
            ..DiodeModel::default()
        };
        let st = params(&model).evaluate(0.7, 0.0);
        assert!(close(st.charge, 5e-9 * st.current, 1e-12));
        assert!(close(st.capacitance, 5e-9 * st.conductance, 1e-12));
    }

    #[test]
    fn area_scales_currents_capacitance_and_series_resistance() {
        let model = DiodeModel {
            cjo: 1e-12,
            rs: 10.0,
            ..DiodeModel::default()
        };
        let one = DiodeParams::new(&model, 1.0, ROOM);
        let three = DiodeParams::new(&model, 3.0, ROOM);
        assert!(close(three.is, 3.0 * one.is, 1e-15));
        assert_eq!(three.series, Some(0.3));
        assert!(close(
            three.evaluate(-1.0, 0.0).capacitance,
            3.0 * one.evaluate(-1.0, 0.0).capacitance,
            1e-12
        ));
    }

    #[test]
    fn saturation_current_scales_with_temperature() {
        let model = DiodeModel {
            is: 1e-14,
            n: 1.5,
            eg: 1.11,
            xti: 3.0,
            ..DiodeModel::default()
        };
        let hot = DiodeParams::new(&model, 1.0, 100.0);
        let (t, tnom) = (373.15, 300.15);
        let vt = thermal_voltage(100.0);
        let expected =
            1e-14 * ((t / tnom - 1.0) * 1.11 / (1.5 * vt) + 3.0 / 1.5 * (t / tnom).ln()).exp();
        assert!(close(hot.is, expected, 1e-12));
        // At constant current an ideal (n = 1) silicon diode's forward drop
        // falls by roughly 1.5–2 mV/K.
        let ideal = DiodeModel { n: 1.0, ..model };
        let drop = |p: &DiodeParams| p.nvt * (1e-3 / p.is).ln_1p();
        let slope = (drop(&DiodeParams::new(&ideal, 1.0, 100.0)) - drop(&params(&ideal))) / 73.0;
        assert!(slope < -1.4e-3 && slope > -2.2e-3, "{slope}");
    }

    #[test]
    fn unusable_parameters_fall_back_to_defaults() {
        let model = DiodeModel {
            n: 0.0,
            vj: -1.0,
            is: f64::NAN,
            tnom: f64::NAN,
            ..DiodeModel::default()
        };
        let p = params(&model);
        assert_eq!(p, params(&DiodeModel::default()));
        assert!(p.evaluate(0.7, 0.0).current.is_finite());
    }

    #[test]
    fn breakdown_region_is_limited_in_mirrored_coordinates() {
        let model = DiodeModel {
            bv: 10.0,
            ibv: 1e-3,
            ..DiodeModel::default()
        };
        let p = params(&model);
        let knee = p.breakdown.unwrap().knee;
        // A jump from just inside breakdown deep into it is compressed.
        let (v, limited) = p.limit(-20.0, -knee - 0.65);
        assert!(limited);
        assert!(v > -20.0 && v < -knee - 0.65);
        // Ordinary forward limiting is unchanged.
        assert_eq!(p.limit(0.62, 0.6), (0.62, false));
    }

    /// Stamps one resistive diode at `vd` and returns the dense matrix and rhs.
    fn stamp(model: DiodeModel, vd: f64) -> (Vec<Vec<f64>>, Vec<f64>, Diode) {
        let mut circuit = Circuit::new();
        let a = circuit.node("a");
        circuit.add(Element::new(
            "D1",
            [a, GROUND],
            Device::diode(model.clone()),
        ));
        let options = SimOptions::default();
        let mut layout = Layout::new(&circuit);
        let mut states = 0;
        let mut diode = Diode::new(&[a, GROUND], &model, 1.0, &options);
        diode.setup(&mut SetupContext {
            layout: &mut layout,
            state_count: &mut states,
            element: "D1",
        });
        let x = vec![vd; layout.size()];
        let mut system = System::new(layout.size());
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, states);
        let mut ctx = LoadContext::new(&x, &mut system, &mut integrator, &options);
        diode.load(&mut ctx);
        assert!(!ctx.limited);
        (system.to_dense(), system.rhs().to_vec(), diode)
    }

    #[test]
    fn load_stamps_norton_companion_and_reports_currents() {
        let (a, rhs, diode) = stamp(DiodeModel::default(), 0.6);
        let st = diode.state;
        assert_eq!(a, vec![vec![st.conductance]]);
        assert!(close(rhs[0], -(st.current - st.conductance * 0.6), 1e-15));
        let mut out = [0.0; 2];
        diode.currents(&mut out);
        assert_eq!(out, [st.current, -st.current]);
        // The current prediction test passes at the linearisation point only.
        assert!(diode.converged(&[0.6], &SimOptions::default()));
        assert!(!diode.converged(&[0.61], &SimOptions::default()));
    }

    #[test]
    fn series_resistance_adds_internal_node() {
        let model = DiodeModel {
            rs: 2.0,
            ..DiodeModel::default()
        };
        let (a, _, diode) = stamp(model, 0.6);
        assert_eq!(a.len(), 2);
        assert_eq!(diode.junction, Some(1));
        assert!(close(a[0][0], 0.5, 1e-15));
        assert!(close(a[0][1], -0.5, 1e-15));
        assert!(close(a[1][1], 0.5 + diode.state.conductance, 1e-15));
    }
}
