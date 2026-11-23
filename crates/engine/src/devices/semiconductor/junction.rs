//! pn-junction equations: Shockley current, critical voltage and depletion
//! charge.

/// Largest junction current (A) evaluated with the exact exponential. Above
/// it `is·e^x` continues along its tangent, so a wild Newton iterate produces
/// a large but finite current instead of `inf`/`NaN`. The limit is on the
/// current, not on the exponent: a junction with a tiny saturation current
/// (a wide-bandgap LED, any junction at cryogenic temperature) legitimately
/// operates at exponents far above 100. For `is = 10 fA` the tangent starts
/// at an exponent of about 78.
const MAX_JUNCTION_CURRENT: f64 = 1e20;

/// Largest exponent evaluated exactly whatever the saturation current.
/// `e^700 ≈ 1e304` leaves room below `f64::MAX` for the division by `n·Vt`.
const MAX_EXP_ARG: f64 = 700.0;

/// `scale·e^x` if it is evaluated exactly (see [`MAX_JUNCTION_CURRENT`]).
fn exact_exp(scale: f64, x: f64) -> Option<f64> {
    if x <= MAX_EXP_ARG {
        let value = scale * x.exp();
        (value <= MAX_JUNCTION_CURRENT).then_some(value)
    } else {
        None
    }
}

/// The tangent continuation of `scale·e^x` beyond the exponent where it
/// reaches [`MAX_JUNCTION_CURRENT`], with its derivative.
fn tangent_exp(scale: f64, x: f64) -> (f64, f64) {
    let limit = if scale > 0.0 {
        (MAX_JUNCTION_CURRENT / scale).ln().min(MAX_EXP_ARG)
    } else {
        MAX_EXP_ARG
    };
    let e = scale * limit.exp();
    (e * (1.0 + x - limit), e)
}

/// `scale·e^x` and its derivative with respect to `x`, exact up to
/// [`MAX_JUNCTION_CURRENT`] and continued along the tangent above it.
/// `scale` is a saturation current (non-negative).
pub(crate) fn scaled_exp(scale: f64, x: f64) -> (f64, f64) {
    match exact_exp(scale, x) {
        Some(value) => (value, value),
        None => tangent_exp(scale, x),
    }
}

/// Shockley junction current `is·(e^(v/nvt) − 1)` and its conductance.
///
/// `nvt` is the emission coefficient times the thermal voltage. Unlike
/// SPICE3, which replaces the exponential by a cubic approximation in
/// reverse bias, the exact exponential is used everywhere: it is smooth,
/// cannot underflow into trouble, and differs from SPICE by less than `is`.
/// It is exact up to a current of [`MAX_JUNCTION_CURRENT`]; only beyond
/// that, where no physical junction operates, it continues linearly.
pub(crate) fn shockley(is: f64, v: f64, nvt: f64) -> (f64, f64) {
    let x = v / nvt;
    match exact_exp(is, x) {
        // expm1 keeps full relative precision for tiny bias.
        Some(e) => (is * x.exp_m1(), e / nvt),
        None => {
            let (e, de) = tangent_exp(is, x);
            (e - is, de / nvt)
        }
    }
}

/// Junction voltage above which the exponential's curvature makes Newton
/// steps unreliable (SPICE `vcrit = nvt·ln(nvt / (√2·is))`). Infinite for a
/// junction without saturation current, which therefore is never limited.
pub(crate) fn critical_voltage(is: f64, nvt: f64) -> f64 {
    if is > 0.0 {
        nvt * (nvt / (std::f64::consts::SQRT_2 * is)).ln()
    } else {
        f64::INFINITY
    }
}

/// `(1 − e^(k·l)) / k`, continued to `−l` at `k = 0`.
///
/// This is the depletion-charge integral `(1 − (1 − v/vj)^(1−m)) / (1 − m)`
/// with `l = ln(1 − v/vj)` and `k = 1 − m`; the closed form divides by zero
/// for a grading coefficient of exactly one.
fn grading_integral(l: f64, k: f64) -> f64 {
    if k == 0.0 { -l } else { -(k * l).exp_m1() / k }
}

/// Depletion charge of a graded pn junction (SPICE3).
///
/// Below `fc·vj` the charge is `vj·cj0·(1 − (1 − v/vj)^(1−m)) / (1 − m)`
/// with capacitance `cj0·(1 − v/vj)^(−m)`. At and above `fc·vj` the
/// capacitance is continued linearly so it stays finite in forward bias.
/// Charge and capacitance are continuous at the boundary and `q(0) = 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Depletion {
    cj0: f64,
    vj: f64,
    m: f64,
    /// Forward-bias boundary `fc·vj`.
    boundary: f64,
    /// Charge at the boundary divided by `cj0` (SPICE `F1`).
    f1: f64,
    /// `(1 − fc)^(1+m)` (SPICE `F2`).
    f2: f64,
    /// `1 − fc·(1 + m)` (SPICE `F3`).
    f3: f64,
}

/// SPICE clamps the forward-bias coefficient so the linearisation starts
/// before the depletion formula's singularity at `v = vj`.
const MAX_FC: f64 = 0.95;

impl Depletion {
    /// `cj0` zero-bias capacitance (F), `vj` built-in potential (V, > 0),
    /// `m` grading coefficient, `fc` forward-bias coefficient (clamped to
    /// `0..=0.95`).
    pub(crate) fn new(cj0: f64, vj: f64, m: f64, fc: f64) -> Self {
        let fc = fc.clamp(0.0, MAX_FC);
        let l = (1.0 - fc).ln();
        Self {
            cj0,
            vj,
            m,
            boundary: fc * vj,
            f1: vj * grading_integral(l, 1.0 - m),
            f2: (1.0 - fc).powf(1.0 + m),
            f3: 1.0 - fc * (1.0 + m),
        }
    }

    /// Whether the junction has any depletion capacitance.
    pub(crate) fn is_present(&self) -> bool {
        self.cj0 != 0.0
    }

    /// Charge (C) and capacitance `dq/dv` (F) at junction voltage `v`.
    pub(crate) fn charge(&self, v: f64) -> (f64, f64) {
        if self.cj0 == 0.0 {
            return (0.0, 0.0);
        }
        if v < self.boundary {
            let l = (1.0 - v / self.vj).ln();
            let q = self.vj * self.cj0 * grading_integral(l, 1.0 - self.m);
            let c = self.cj0 * (-self.m * l).exp();
            (q, c)
        } else {
            let scale = self.cj0 / self.f2;
            let dv = v - self.boundary;
            let squares = (v + self.boundary) * dv;
            let q =
                self.cj0 * self.f1 + scale * (self.f3 * dv + self.m / (2.0 * self.vj) * squares);
            let c = scale * (self.f3 + self.m * v / self.vj);
            (q, c)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs()) + 1e-300
    }

    #[test]
    fn scaled_exp_is_exact_up_to_the_current_limit_and_tangent_above() {
        assert_eq!(
            scaled_exp(1e-14, 1.0),
            (1e-14 * 1f64.exp(), 1e-14 * 1f64.exp())
        );
        // The limit depends on the scale: 1e-37·e^81 ≈ 1.5e-2 is exact.
        let (v, d) = scaled_exp(1.33e-37, 81.0);
        assert_eq!((v, d), (1.33e-37 * 81f64.exp(), 1.33e-37 * 81f64.exp()));
        // For is = 1e-14 the tangent starts where the current reaches 1e20 A.
        let limit = (MAX_JUNCTION_CURRENT / 1e-14).ln();
        assert!(limit > 78.0 && limit < 79.0);
        let (v, d) = scaled_exp(1e-14, limit + 2.0);
        assert!(close(v, 3.0 * MAX_JUNCTION_CURRENT, 1e-12), "{v}");
        assert!(close(d, MAX_JUNCTION_CURRENT, 1e-12), "{d}");
        // Continuous at the limit.
        let below = scaled_exp(1e-14, limit - 1e-9).0;
        let above = scaled_exp(1e-14, limit + 1e-9).0;
        assert!(close(below, above, 1e-8));
        // Huge arguments stay finite, with or without saturation current.
        assert!(scaled_exp(1e-14, 1e6).0.is_finite());
        assert!(scaled_exp(1e-300, 1e6).0.is_finite());
        assert_eq!(scaled_exp(0.0, 1e6), (0.0, 0.0));
    }

    #[test]
    fn shockley_is_exact_for_tiny_saturation_currents() {
        // A 20 mA operating point at an exponent of about 81 (the review's
        // low-IS case) and of several hundred (cryogenic silicon).
        let (is, nvt): (f64, f64) = (1.33e-37, 1.5 * 0.025_864_7);
        let v = nvt * (20e-3 / is + 1.0).ln();
        let (i, g) = shockley(is, v, nvt);
        assert!(close(i, 20e-3, 1e-12), "{i}");
        assert!(close(g, (20e-3 + is) / nvt, 1e-12), "{g}");
        let (is, nvt): (f64, f64) = (1e-250, 1.4 * 0.006_3);
        let v = nvt * (1e-3 / is).ln();
        assert!(v / nvt > 500.0);
        assert!(close(shockley(is, v, nvt).0, 1e-3, 1e-12));
    }

    #[test]
    fn shockley_matches_formula_and_derivative() {
        let (is, nvt) = (1e-14, 0.025_864_7 * 1.7);
        for &v in &[-5.0, -0.1, -1e-9, 0.0, 1e-9, 0.3, 0.65, 0.9] {
            let (i, g) = shockley(is, v, nvt);
            let exact = is * (v / nvt).exp_m1();
            assert!(close(i, exact, 1e-12), "v={v}: {i} vs {exact}");
            assert!(close(g, is * (v / nvt).exp() / nvt, 1e-12), "v={v}");
            if v > -0.1 {
                // Below that the current is −is to within rounding.
                let h = 1e-7;
                let fd = (shockley(is, v + h, nvt).0 - shockley(is, v - h, nvt).0) / (2.0 * h);
                assert!(close(g, fd, 1e-6), "v={v}: g={g} fd={fd}");
            }
        }
    }

    #[test]
    fn shockley_stays_finite_for_runaway_voltages() {
        let (i, g) = shockley(1e-14, 1e3, 0.0258);
        assert!(i.is_finite() && g.is_finite() && i > 0.0 && g > 0.0);
    }

    #[test]
    fn critical_voltage_matches_spice_definition() {
        let (is, nvt) = (1e-14, 0.0258);
        let expected = nvt * (nvt / (2f64.sqrt() * is)).ln();
        assert_eq!(critical_voltage(is, nvt), expected);
        assert_eq!(critical_voltage(0.0, nvt), f64::INFINITY);
    }

    #[test]
    fn depletion_charge_is_zero_at_zero_bias_and_integrates_capacitance() {
        let d = Depletion::new(2e-12, 0.75, 0.33, 0.5);
        assert_eq!(d.charge(0.0), (0.0, 2e-12));
        // dq/dv equals the reported capacitance on both sides of fc·vj.
        for &v in &[-10.0, -1.0, 0.0, 0.2, 0.3749, 0.3751, 0.6, 1.2] {
            let h = 1e-6;
            let fd = (d.charge(v + h).0 - d.charge(v - h).0) / (2.0 * h);
            assert!(close(d.charge(v).1, fd, 1e-6), "v={v}");
        }
    }

    #[test]
    fn depletion_is_continuous_at_forward_bias_boundary() {
        let d = Depletion::new(1e-12, 0.8, 0.5, 0.5);
        let b = 0.4;
        let below = d.charge(b - 1e-12);
        let above = d.charge(b);
        assert!(close(below.0, above.0, 1e-9));
        assert!(close(below.1, above.1, 1e-9));
        // Reverse-bias capacitance follows cj0·(1 − v/vj)^−m exactly.
        let (_, c) = d.charge(-2.0);
        assert!(close(c, 1e-12 * (1.0 + 2.0 / 0.8f64).powf(-0.5), 1e-14));
    }

    #[test]
    fn depletion_handles_unit_grading_coefficient() {
        let d = Depletion::new(1e-12, 0.7, 1.0, 0.5);
        let (q, c) = d.charge(-1.0);
        assert!(close(q, -0.7e-12 * (1.0 + 1.0 / 0.7f64).ln(), 1e-12));
        assert!(close(c, 1e-12 / (1.0 + 1.0 / 0.7), 1e-12));
        let near = Depletion::new(1e-12, 0.7, 1.0 - 1e-9, 0.5).charge(-1.0).0;
        assert!(close(q, near, 1e-6));
    }

    #[test]
    fn zero_fc_linearises_from_zero_bias() {
        // SPICE's collector-substrate junction: q = cj0·(v + m·v²/(2·vj)) for v ≥ 0.
        let d = Depletion::new(3e-12, 0.75, 0.4, 0.0);
        let (q, c) = d.charge(0.5);
        assert!(close(q, 3e-12 * (0.5 + 0.4 * 0.25 / 1.5), 1e-14));
        assert!(close(c, 3e-12 * (1.0 + 0.4 * 0.5 / 0.75), 1e-14));
    }

    #[test]
    fn absent_junction_has_no_charge() {
        let d = Depletion::new(0.0, 0.75, 0.33, 0.5);
        assert!(!d.is_present());
        assert_eq!(d.charge(0.4), (0.0, 0.0));
    }
}
