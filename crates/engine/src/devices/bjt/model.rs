//! Gummel–Poon equations (SPICE3 `bjttemp` / `bjtload`) evaluated in
//! polarity-normalised (NPN) form.
//!
//! The intrinsic transistor between the internal nodes `b'`, `c'`, `e'` is
//! described by three branch currents:
//!
//! * `ibe` from `b'` to `e'`: `cbe/bf + ise·(e^(vbe/ne·Vt) − 1) + gmin·vbe`,
//! * `ibc` from `b'` to `c'`: `cbc/br + isc·(e^(vbc/nc·Vt) − 1) + gmin·vbc`,
//! * `ict` from `c'` to `e'`: `(cbe − cbc)/qb`,
//!
//! with `cbe = is·(e^(vbe/nf·Vt) − 1)`, `cbc = is·(e^(vbc/nr·Vt) − 1)` and the
//! normalised base charge `qb = q1·(1 + √(1 + 4·q2))/2`,
//! `q1 = 1/(1 − vbc/vaf − vbe/var)`, `q2 = cbe/ikf + cbc/ikr`.
//! As in ngspice, `gmin` appears only in the base currents, never in the
//! transport current.
//!
//! Charges: `qbe = tf_eff·cbe/qb + depletion(cje)`, `qbc = tr·cbc +
//! depletion(xcjc·cjc)`, `qbx = depletion((1 − xcjc)·cjc)` between the
//! external base and `c'`, and `qcs = depletion(cjs)` from the substrate
//! (ground) to `c'`, linearised from zero bias as in SPICE3.
//!
//! Temperature follows SPICE3 `bjttemp`: `is`, `ise`, `isc`, `bf`, `br` and
//! the B-E and B-C junction potentials and capacitances are adjusted; the
//! collector-substrate `cjs` and `vjs` are not (as in SPICE3 and LTspice).

use super::super::semiconductor::{
    Depletion, ZERO_CELSIUS, critical_voltage, finite_or, inverse_or_zero, junction_at_temperature,
    nominal_kelvin, non_negative_or, positive_or, saturation_log_factor, series_conductance,
    shockley, thermal_voltage,
};
use crate::models::BjtModel;

/// Smallest admitted value of the Early-effect denominator
/// `1 − vbc/vaf − vbe/var`. Real bias points stay far above it; the floor only
/// keeps wild Newton iterates from dividing by zero.
const MIN_EARLY_DENOMINATOR: f64 = 1e-6;

/// SPICE3 lower bound of `ib/irb` in the base-resistance formula.
const MIN_IRB_RATIO: f64 = 1e-9;

/// Base spreading resistance `rb` falling towards `rbm` with current.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BaseResistance {
    /// Minimum (high-current) resistance `rbm/area` (Ω).
    rbm: f64,
    /// Current-dependent part `(rb − rbm)/area` (Ω).
    rbpi: f64,
    /// Current where the resistance falls halfway (`irb·area`), if given.
    irb: Option<f64>,
}

impl BaseResistance {
    /// Conductance of the base resistance at static base current `ib` and
    /// normalised base charge `qb`.
    pub(super) fn conductance(&self, ib: f64, qb: f64) -> f64 {
        let r = match self.irb {
            Some(irb) => {
                let a = (ib / irb).max(MIN_IRB_RATIO);
                let z = ((1.0 + 14.590_25 * a).sqrt() - 1.0) / (2.4317 * a.sqrt());
                self.rbm + 3.0 * self.rbpi * tan_ratio(z)
            }
            None => self.rbm + self.rbpi / qb,
        };
        1.0 / r
    }
}

/// `(tan z − z) / (z·tan² z)` for `0 < z < π/2`; it falls from 1/3 at `z → 0`
/// to 0 at `π/2`. A series replaces the closed form near zero, where
/// `tan z − z` cancels catastrophically.
fn tan_ratio(z: f64) -> f64 {
    if z < 1e-3 {
        1.0 / 3.0 - 4.0 / 45.0 * z * z
    } else {
        let t = z.tan();
        (t - z) / (z * t * t)
    }
}

/// Temperature- and area-adjusted Gummel–Poon parameters.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BjtParams {
    pub(super) sign: f64,
    pub(super) vt: f64,
    /// Critical voltage of both junctions for step limiting.
    pub(super) vcrit: f64,
    is: f64,
    bf: f64,
    br: f64,
    nf_vt: f64,
    nr_vt: f64,
    ise: f64,
    ne_vt: f64,
    isc: f64,
    nc_vt: f64,
    inv_vaf: f64,
    inv_var: f64,
    inv_ikf: f64,
    inv_ikr: f64,
    pub(super) base: Option<BaseResistance>,
    pub(super) collector: Option<f64>,
    pub(super) emitter: Option<f64>,
    pub(super) be_depletion: Depletion,
    pub(super) bc_depletion: Depletion,
    pub(super) bx_depletion: Depletion,
    pub(super) cs_depletion: Depletion,
    tf: f64,
    xtf: f64,
    /// `1/(1.44·vtf)`, 0 without VBC dependence.
    inv_vtf: f64,
    itf: f64,
    tr: f64,
    /// Excess-phase delay `ptf·tf` in radians·seconds (AC only).
    pub(super) excess_phase: f64,
}

impl BjtParams {
    pub(super) fn new(model: &BjtModel, area: f64, celsius: f64) -> Self {
        let d = BjtModel::default();
        let temp = celsius + ZERO_CELSIUS;
        let tnom = nominal_kelvin(model.tnom, d.tnom);
        let vt = thermal_voltage(celsius);

        let log_factor = saturation_log_factor(
            positive_or(model.eg, d.eg),
            non_negative_or(model.xti, d.xti),
            tnom,
            temp,
        );
        let beta_factor = if temp == tnom {
            1.0
        } else {
            (temp / tnom).powf(non_negative_or(model.xtb, 0.0))
        };
        let ne = positive_or(model.ne, d.ne);
        let nc = positive_or(model.nc, d.nc);
        let is = non_negative_or(model.is, d.is) * area * log_factor.exp();

        let rb = non_negative_or(model.rb, 0.0);
        let rbm = non_negative_or(model.rbm, 0.0).min(rb);
        let base = (rb > 0.0).then(|| BaseResistance {
            rbm: rbm / area,
            rbpi: (rb - rbm) / area,
            irb: (model.irb.is_finite() && model.irb > 0.0).then_some(model.irb * area),
        });

        let fc = non_negative_or(model.fc, d.fc);
        let junction = |cj: f64, vj: f64, vj_default: f64, m: f64| {
            junction_at_temperature(
                non_negative_or(cj, 0.0) * area,
                positive_or(vj, vj_default),
                m,
                tnom,
                temp,
            )
        };
        let mje = non_negative_or(model.mje, d.mje);
        let mjc = non_negative_or(model.mjc, d.mjc);
        let mjs = non_negative_or(model.mjs, d.mjs);
        let xcjc = non_negative_or(model.xcjc, d.xcjc).min(1.0);
        let (cje, vje) = junction(model.cje, model.vje, d.vje, mje);
        let (cjc, vjc) = junction(model.cjc, model.vjc, d.vjc, mjc);
        // SPICE3 `bjttemp` and LTspice leave the collector-substrate junction
        // at its model values at every temperature (newer ngspice scales it
        // like the other junctions); xsim follows SPICE3 and LTspice.
        let (cjs, vjs) = (
            non_negative_or(model.cjs, 0.0) * area,
            positive_or(model.vjs, d.vjs),
        );

        let tf = non_negative_or(model.tf, 0.0);
        let vtf = model.vtf;
        Self {
            sign: model.polarity.sign(),
            vt,
            vcrit: critical_voltage(is, vt),
            is,
            bf: positive_or(model.bf, d.bf) * beta_factor,
            br: positive_or(model.br, d.br) * beta_factor,
            nf_vt: positive_or(model.nf, d.nf) * vt,
            nr_vt: positive_or(model.nr, d.nr) * vt,
            ise: non_negative_or(model.ise, 0.0) * area * (log_factor / ne).exp() / beta_factor,
            ne_vt: ne * vt,
            isc: non_negative_or(model.isc, 0.0) * area * (log_factor / nc).exp() / beta_factor,
            nc_vt: nc * vt,
            inv_vaf: inverse_or_zero(model.vaf),
            inv_var: inverse_or_zero(model.var),
            inv_ikf: inverse_or_zero(model.ikf) / area,
            inv_ikr: inverse_or_zero(model.ikr) / area,
            base,
            collector: series_conductance(model.rc, area),
            emitter: series_conductance(model.re, area),
            be_depletion: Depletion::new(cje, vje, mje, fc),
            bc_depletion: Depletion::new(cjc * xcjc, vjc, mjc, fc),
            bx_depletion: Depletion::new(cjc * (1.0 - xcjc), vjc, mjc, fc),
            cs_depletion: Depletion::new(cjs, vjs, mjs, 0.0),
            tf,
            xtf: non_negative_or(model.xtf, 0.0),
            inv_vtf: if vtf.is_finite() && vtf > 0.0 {
                1.0 / (1.44 * vtf)
            } else {
                0.0
            },
            itf: non_negative_or(model.itf, 0.0) * area,
            tr: non_negative_or(model.tr, 0.0),
            excess_phase: finite_or(model.ptf, 0.0).to_radians() * tf,
        }
    }
}

impl BjtParams {
    /// Whether the B-E charge has a diffusion part.
    pub(super) fn has_forward_transit(&self) -> bool {
        self.tf > 0.0
    }

    /// Whether the B-C charge has a diffusion part.
    pub(super) fn has_reverse_transit(&self) -> bool {
        self.tr > 0.0
    }
}

/// Normalised junction voltages and the model evaluated there.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct BjtState {
    pub(super) vbe: f64,
    pub(super) vbc: f64,
    /// External base to internal collector.
    pub(super) vbx: f64,
    /// Substrate (ground) to internal collector.
    pub(super) vcs: f64,
    /// `b'→e'` static current and `∂/∂vbe`.
    pub(super) ibe: f64,
    pub(super) gpi: f64,
    /// `b'→c'` static current and `∂/∂vbc`.
    pub(super) ibc: f64,
    pub(super) gmu: f64,
    /// `c'→e'` transport current; `∂/∂vbe = gm + go`, `∂/∂vbc = −go`.
    pub(super) ict: f64,
    pub(super) gm: f64,
    pub(super) go: f64,
    /// Base-resistance conductance (0 without one).
    pub(super) gx: f64,
    /// B-E charge and its derivatives with respect to `vbe` and `vbc`.
    pub(super) qbe: f64,
    pub(super) cbe: f64,
    pub(super) cbe_vbc: f64,
    pub(super) qbc: f64,
    pub(super) cbc: f64,
    pub(super) qbx: f64,
    pub(super) cbx: f64,
    pub(super) qcs: f64,
    pub(super) ccs: f64,
}

/// Normalised base charge `qb` and its derivatives.
struct BaseCharge {
    qb: f64,
    d_vbe: f64,
    d_vbc: f64,
}

impl BjtParams {
    fn base_charge(
        &self,
        vbe: f64,
        vbc: f64,
        cbe: f64,
        gbe: f64,
        cbc: f64,
        gbc: f64,
    ) -> BaseCharge {
        let early = 1.0 - vbc * self.inv_vaf - vbe * self.inv_var;
        let (q1, dq1_vbe, dq1_vbc) = if early > MIN_EARLY_DENOMINATOR {
            let q1 = 1.0 / early;
            (q1, q1 * q1 * self.inv_var, q1 * q1 * self.inv_vaf)
        } else {
            (1.0 / MIN_EARLY_DENOMINATOR, 0.0, 0.0)
        };
        if self.inv_ikf == 0.0 && self.inv_ikr == 0.0 {
            return BaseCharge {
                qb: q1,
                d_vbe: dq1_vbe,
                d_vbc: dq1_vbc,
            };
        }
        let q2 = self.inv_ikf * cbe + self.inv_ikr * cbc;
        let arg = (1.0 + 4.0 * q2).max(0.0);
        // SPICE3 keeps the derivative finite when the root vanishes.
        let root = if arg != 0.0 { arg.sqrt() } else { 1.0 };
        let half = (1.0 + root) / 2.0;
        BaseCharge {
            qb: q1 * half,
            d_vbe: dq1_vbe * half + q1 * self.inv_ikf * gbe / root,
            d_vbc: dq1_vbc * half + q1 * self.inv_ikr * gbc / root,
        }
    }

    /// Forward diffusion charge `tf·cbe_eff/qb` with the XTF/VTF/ITF bias
    /// dependence, and its derivatives with respect to `vbe` and `vbc`.
    fn forward_transit_charge(
        &self,
        vbe: f64,
        vbc: f64,
        cbe: f64,
        gbe: f64,
        qb: &BaseCharge,
    ) -> (f64, f64, f64) {
        if self.tf == 0.0 || vbe <= 0.0 {
            return (self.tf * cbe, self.tf * gbe, 0.0);
        }
        let (mut argtf, mut arg2, mut arg3) = (0.0, 0.0, 0.0);
        if self.xtf != 0.0 {
            argtf = self.xtf;
            if self.inv_vtf != 0.0 {
                argtf *= (vbc * self.inv_vtf).exp();
            }
            arg2 = argtf;
            if self.itf != 0.0 {
                let share = cbe / (cbe + self.itf);
                argtf *= share * share;
                arg2 = argtf * (3.0 - 2.0 * share);
            }
            arg3 = cbe * argtf * self.inv_vtf;
        }
        let current = cbe * (1.0 + argtf) / qb.qb;
        let d_vbe = (gbe * (1.0 + arg2) - current * qb.d_vbe) / qb.qb;
        let d_vbc = (arg3 - current * qb.d_vbc) / qb.qb;
        (self.tf * current, self.tf * d_vbe, self.tf * d_vbc)
    }

    /// Evaluates the transistor at normalised junction voltages.
    pub(super) fn evaluate(&self, vbe: f64, vbc: f64, vbx: f64, vcs: f64, gmin: f64) -> BjtState {
        let (cbe, gbe) = shockley(self.is, vbe, self.nf_vt);
        let (cbc, gbc) = shockley(self.is, vbc, self.nr_vt);
        let (cben, gben) = shockley(self.ise, vbe, self.ne_vt);
        let (cbcn, gbcn) = shockley(self.isc, vbc, self.nc_vt);

        let qb = self.base_charge(vbe, vbc, cbe, gbe, cbc, gbc);
        let transport = cbe - cbc;
        let ict = transport / qb.qb;
        let go = (gbc + transport * qb.d_vbc / qb.qb) / qb.qb;
        let gm = (gbe - transport * qb.d_vbe / qb.qb) / qb.qb - go;

        let ibe = cbe / self.bf + cben + gmin * vbe;
        let ibc = cbc / self.br + cbcn + gmin * vbc;
        let gx = self.base.map_or(0.0, |rb| rb.conductance(ibe + ibc, qb.qb));

        let (q_transit, c_transit, c_transit_vbc) =
            self.forward_transit_charge(vbe, vbc, cbe, gbe, &qb);
        let (q_be, c_be) = self.be_depletion.charge(vbe);
        let (q_bc, c_bc) = self.bc_depletion.charge(vbc);
        let (qbx, cbx) = self.bx_depletion.charge(vbx);
        let (qcs, ccs) = self.cs_depletion.charge(vcs);

        BjtState {
            vbe,
            vbc,
            vbx,
            vcs,
            ibe,
            gpi: gbe / self.bf + gben + gmin,
            ibc,
            gmu: gbc / self.br + gbcn + gmin,
            ict,
            gm,
            go,
            gx,
            qbe: q_transit + q_be,
            cbe: c_transit + c_be,
            cbe_vbc: c_transit_vbc,
            qbc: self.tr * cbc + q_bc,
            cbc: self.tr * gbc + c_bc,
            qbx,
            cbx,
            qcs,
            ccs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: f64 = 27.0;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs()) + 1e-300
    }

    fn params(model: &BjtModel) -> BjtParams {
        BjtParams::new(model, 1.0, ROOM)
    }

    /// A fully featured NPN for derivative checks.
    fn rich_model() -> BjtModel {
        BjtModel {
            is: 1.8e-14,
            bf: 180.0,
            br: 4.0,
            nf: 1.02,
            nr: 1.05,
            vaf: 80.0,
            var: 20.0,
            ikf: 0.3,
            ikr: 0.05,
            ise: 5e-14,
            ne: 1.6,
            isc: 1e-13,
            nc: 1.9,
            rb: 10.0,
            rbm: 2.0,
            irb: 1e-4,
            cje: 15e-12,
            vje: 0.7,
            mje: 0.35,
            cjc: 5e-12,
            vjc: 0.55,
            mjc: 0.3,
            xcjc: 0.8,
            cjs: 2e-12,
            mjs: 0.4,
            tf: 5e-10,
            xtf: 2.0,
            vtf: 4.0,
            itf: 0.2,
            tr: 40e-9,
            ..BjtModel::default()
        }
    }

    #[test]
    fn ideal_transistor_matches_ebers_moll() {
        // No Early effect, no high injection, no leakage: qb = 1 and the
        // Gummel–Poon model reduces to the transport Ebers–Moll model.
        let model = BjtModel {
            is: 1e-15,
            bf: 150.0,
            br: 3.0,
            ..BjtModel::default()
        };
        let p = params(&model);
        let vt = thermal_voltage(ROOM);
        for &(vbe, vbc) in &[(0.65, -5.0), (0.7, 0.3), (0.2, 0.6), (-1.0, -1.0)] {
            let st = p.evaluate(vbe, vbc, vbc, 0.0, 0.0);
            let i_f = 1e-15 * (vbe / vt).exp_m1();
            let i_r = 1e-15 * (vbc / vt).exp_m1();
            let ic = i_f - i_r - i_r / 3.0;
            let ib = i_f / 150.0 + i_r / 3.0;
            assert!(close(st.ict - st.ibc, ic, 1e-12), "vbe={vbe} vbc={vbc}");
            assert!(close(st.ibe + st.ibc, ib, 1e-12), "vbe={vbe} vbc={vbc}");
        }
    }

    #[test]
    fn forward_active_collector_current_in_closed_form() {
        let model = BjtModel {
            is: 1e-14,
            bf: 200.0,
            vaf: 50.0,
            ..BjtModel::default()
        };
        let p = params(&model);
        let vt = thermal_voltage(ROOM);
        let (vbe, vce) = (0.68, 5.0);
        let vbc = vbe - vce;
        let st = p.evaluate(vbe, vbc, vbc, 0.0, 0.0);
        let transport = 1e-14 * ((vbe / vt).exp_m1() - (vbc / vt).exp_m1());
        let expected = transport * (1.0 - vbc / 50.0) - 1e-14 * (vbc / vt).exp_m1();
        assert!(close(st.ict - st.ibc, expected, 1e-12));
    }

    #[test]
    fn high_injection_follows_base_charge_formula() {
        let model = BjtModel {
            is: 1e-14,
            ikf: 0.01,
            ..BjtModel::default()
        };
        let p = params(&model);
        let vt = thermal_voltage(ROOM);
        let vbe = 0.8;
        let st = p.evaluate(vbe, -5.0, -5.0, 0.0, 0.0);
        let cbe = 1e-14 * (vbe / vt).exp_m1();
        let cbc = 1e-14 * (-5.0 / vt).exp_m1();
        let qb = (1.0 + (1.0 + 4.0 * cbe / 0.01).sqrt()) / 2.0;
        assert!(close(st.ict, (cbe - cbc) / qb, 1e-12));
    }

    #[test]
    fn gmin_appears_only_in_base_currents() {
        let p = params(&BjtModel::default());
        let a = p.evaluate(0.6, -3.0, -3.0, 0.0, 0.0);
        let b = p.evaluate(0.6, -3.0, -3.0, 0.0, 1e-9);
        assert_eq!(a.ict, b.ict);
        assert!(close(b.ibe - a.ibe, 0.6e-9, 1e-9));
        assert!(close(b.ibc - a.ibc, -3e-9, 1e-9));
    }

    /// Central-difference derivative of `f` at `v`.
    fn derivative(f: impl Fn(f64) -> f64, v: f64) -> f64 {
        let h = 1e-7;
        (f(v + h) - f(v - h)) / (2.0 * h)
    }

    #[test]
    fn conductances_are_exact_partial_derivatives() {
        let p = params(&rich_model());
        for &(vbe, vbc) in &[
            (0.72, -4.0),
            (0.85, 0.2),
            (0.6, 0.65),
            (0.3, -0.2),
            (-0.5, -2.0),
        ] {
            let st = p.evaluate(vbe, vbc, vbc, 0.0, 1e-12);
            let at = |be: f64, bc: f64| p.evaluate(be, bc, bc, 0.0, 1e-12);
            // Deep in cut-off the transport current is a difference of two
            // nearly equal saturation currents; allow rounding noise there.
            let matches = |g: f64, fd: f64| (g - fd).abs() <= 1e-6 * g.abs().max(fd.abs()) + 1e-20;
            let d_ict_vbe = derivative(|v| at(v, vbc).ict, vbe);
            let d_ict_vbc = derivative(|v| at(vbe, v).ict, vbc);
            assert!(matches(st.gm + st.go, d_ict_vbe), "gm+go at {vbe},{vbc}");
            assert!(
                matches(-st.go, d_ict_vbc),
                "go at {vbe},{vbc}: {} vs {d_ict_vbc}",
                -st.go
            );
            assert!(matches(st.gpi, derivative(|v| at(v, vbc).ibe, vbe)), "gpi");
            assert!(matches(st.gmu, derivative(|v| at(vbe, v).ibc, vbc)), "gmu");
        }
    }

    #[test]
    fn capacitances_are_exact_partial_derivatives() {
        let p = params(&rich_model());
        for &(vbe, vbc) in &[
            (0.72, -4.0),
            (0.85, 0.2),
            (0.6, 0.65),
            (0.3, -0.2),
            (-0.5, -2.0),
        ] {
            let st = p.evaluate(vbe, vbc, vbc - 0.1, -vbc, 0.0);
            let at = |be: f64, bc: f64| p.evaluate(be, bc, bc - 0.1, -bc, 0.0);
            assert!(
                close(st.cbe, derivative(|v| at(v, vbc).qbe, vbe), 1e-6),
                "cbe at {vbe},{vbc}"
            );
            let fd = derivative(|v| at(vbe, v).qbe, vbc);
            assert!(
                (st.cbe_vbc - fd).abs() <= 1e-6 * st.cbe.abs().max(fd.abs()),
                "cbe_vbc at {vbe},{vbc}"
            );
            assert!(
                close(st.cbc, derivative(|v| at(vbe, v).qbc, vbc), 1e-6),
                "cbc at {vbe},{vbc}"
            );
            let bx = |v: f64| p.evaluate(vbe, vbc, v, -vbc, 0.0).qbx;
            assert!(close(st.cbx, derivative(bx, vbc - 0.1), 1e-6), "cbx");
            let cs = |v: f64| p.evaluate(vbe, vbc, vbc - 0.1, v, 0.0).qcs;
            assert!(close(st.ccs, derivative(cs, -vbc), 1e-6), "ccs");
        }
    }

    #[test]
    fn collector_capacitance_is_split_by_xcjc() {
        let model = BjtModel {
            cjc: 10e-12,
            xcjc: 0.3,
            ..BjtModel::default()
        };
        let st = params(&model).evaluate(0.0, 0.0, 0.0, 0.0, 0.0);
        assert!(close(st.cbc, 3e-12, 1e-12));
        assert!(close(st.cbx, 7e-12, 1e-12));
    }

    #[test]
    fn base_resistance_moves_from_rb_to_rbm() {
        let with_irb = BaseResistance {
            rbm: 2.0,
            rbpi: 8.0,
            irb: Some(1e-4),
        };
        assert!(close(1.0 / with_irb.conductance(0.0, 1.0), 10.0, 1e-8));
        assert!(close(1.0 / with_irb.conductance(1e3, 1.0), 2.0, 1e-3));
        let r_half = 1.0 / with_irb.conductance(1e-4, 1.0);
        assert!(r_half > 2.0 && r_half < 10.0);
        let with_qb = BaseResistance {
            rbm: 2.0,
            rbpi: 8.0,
            irb: None,
        };
        assert!(close(1.0 / with_qb.conductance(1e-3, 2.0), 6.0, 1e-15));
    }

    #[test]
    fn tan_ratio_series_joins_closed_form() {
        let below = tan_ratio(1e-3 - 1e-12);
        let above = tan_ratio(1e-3);
        assert!(close(below, above, 1e-9));
        assert!(close(tan_ratio(0.0), 1.0 / 3.0, 1e-15));
        assert!(
            tan_ratio(1.5) > 0.0 && tan_ratio(1.5) < tan_ratio(1.0) && tan_ratio(1.0) < 1.0 / 3.0
        );
    }

    #[test]
    fn rbm_above_rb_is_clamped_and_zero_rb_means_no_resistance() {
        let p = params(&BjtModel {
            rb: 5.0,
            rbm: 50.0,
            ..BjtModel::default()
        });
        assert_eq!(
            p.base,
            Some(BaseResistance {
                rbm: 5.0,
                rbpi: 0.0,
                irb: None
            })
        );
        assert_eq!(
            params(&BjtModel {
                rb: 0.0,
                rbm: 3.0,
                ..BjtModel::default()
            })
            .base,
            None
        );
    }

    #[test]
    fn temperature_scales_is_beta_and_leakage() {
        let model = BjtModel {
            is: 1e-15,
            bf: 100.0,
            ise: 1e-14,
            ne: 1.5,
            xtb: 1.5,
            xti: 3.0,
            eg: 1.11,
            ..BjtModel::default()
        };
        let hot = BjtParams::new(&model, 1.0, 100.0);
        let (t, tnom) = (373.15, 300.15);
        let log_factor = (t / tnom - 1.0) * 1.11 / thermal_voltage(100.0) + 3.0 * (t / tnom).ln();
        let beta_factor = (t / tnom).powf(1.5);
        assert!(close(hot.is, 1e-15 * log_factor.exp(), 1e-12));
        assert!(close(hot.bf, 100.0 * beta_factor, 1e-12));
        assert!(close(
            hot.ise,
            1e-14 * (log_factor / 1.5).exp() / beta_factor,
            1e-12
        ));
        let room = params(&model);
        assert_eq!((room.is, room.bf, room.ise), (1e-15, 100.0, 1e-14));
    }

    #[test]
    fn substrate_capacitance_does_not_depend_on_temperature() {
        // SPICE3 and LTspice keep CJS and VJS at their model values, while
        // CJE and CJC follow the junction temperature equations.
        let model = BjtModel {
            cje: 1e-12,
            cjs: 2e-12,
            vjs: 0.7,
            mjs: 0.3,
            ..BjtModel::default()
        };
        let room = params(&model);
        for celsius in [-40.0, 0.0, 100.0] {
            let p = BjtParams::new(&model, 1.0, celsius);
            assert_eq!(p.cs_depletion, room.cs_depletion, "{celsius} °C");
            assert_ne!(p.be_depletion, room.be_depletion, "{celsius} °C");
        }
        assert_eq!(room.cs_depletion, Depletion::new(2e-12, 0.7, 0.3, 0.0));
    }

    #[test]
    fn area_scales_currents_and_divides_resistances() {
        let model = BjtModel {
            rb: 10.0,
            rc: 1.0,
            re: 0.5,
            ikf: 0.1,
            cje: 1e-12,
            ..BjtModel::default()
        };
        let p = BjtParams::new(&model, 4.0, ROOM);
        assert!(close(p.is, 4e-16, 1e-15));
        assert_eq!(p.collector, Some(4.0));
        assert_eq!(p.emitter, Some(8.0));
        assert!(close(p.inv_ikf, 2.5, 1e-15));
        assert_eq!(p.base.map(|b| b.rbpi), Some(2.5));
    }

    #[test]
    fn excess_phase_is_ptf_in_radians_times_tf() {
        let p = params(&BjtModel {
            tf: 1e-9,
            ptf: 30.0,
            ..BjtModel::default()
        });
        assert!(close(
            p.excess_phase,
            std::f64::consts::PI / 6.0 * 1e-9,
            1e-15
        ));
    }
}
