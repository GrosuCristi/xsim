//! SPICE3 temperature scaling of junction parameters.
//!
//! All functions take absolute temperatures in kelvin and return the
//! unchanged parameters when the device temperature equals the measurement
//! temperature, so models are reproduced bit-for-bit at `TNOM`.

use super::{BOLTZMANN, CHARGE};

/// Reference temperature of the SPICE3 bandgap and potential formulas (K).
const REFERENCE_KELVIN: f64 = 300.15;

/// Silicon bandgap at [`REFERENCE_KELVIN`] as written in SPICE3 (eV).
const REFERENCE_BANDGAP: f64 = 1.115_087_7;

/// Smallest built-in potential produced by extrapolation to very high
/// temperatures. The SPICE formula crosses zero near 175 °C for low-potential
/// junctions, which would make the depletion charge undefined.
const MIN_POTENTIAL: f64 = 1e-3;

/// Linear temperature coefficient of the zero-bias junction capacitance used
/// by SPICE3 (1/K).
const CAPACITANCE_TC: f64 = 4e-4;

/// Empirical silicon bandgap (eV) at `kelvin`.
fn silicon_bandgap(kelvin: f64) -> f64 {
    1.16 - 7.02e-4 * kelvin * kelvin / (kelvin + 1108.0)
}

/// Temperature-dependent term of the built-in potential (SPICE3 `pbfact`).
fn potential_offset(kelvin: f64) -> f64 {
    let vt = BOLTZMANN * kelvin / CHARGE;
    let arg = -silicon_bandgap(kelvin) / (2.0 * BOLTZMANN * kelvin)
        + REFERENCE_BANDGAP / (2.0 * BOLTZMANN * REFERENCE_KELVIN);
    -2.0 * vt * (1.5 * (kelvin / REFERENCE_KELVIN).ln() + CHARGE * arg)
}

/// Zero-bias capacitance and built-in potential at `temp` of a junction
/// characterised at `tnom` with capacitance `cj0`, potential `vj` and grading
/// coefficient `m` (SPICE3 `DIOtemp`/`BJTtemp`/`JFETtemp`).
pub(crate) fn junction_at_temperature(
    cj0: f64,
    vj: f64,
    m: f64,
    tnom: f64,
    temp: f64,
) -> (f64, f64) {
    if temp == tnom {
        return (cj0, vj);
    }
    let nominal_ratio = tnom / REFERENCE_KELVIN;
    let ratio = temp / REFERENCE_KELVIN;
    // Potential extrapolated back to the reference temperature.
    let pbo = (vj - potential_offset(tnom)) / nominal_ratio;
    let gamma_nominal = (vj - pbo) / pbo;
    let potential = (ratio * pbo + potential_offset(temp)).max(MIN_POTENTIAL);
    let gamma = (potential - pbo) / pbo;
    let at_reference =
        cj0 / (1.0 + m * (CAPACITANCE_TC * (tnom - REFERENCE_KELVIN) - gamma_nominal));
    let capacitance =
        at_reference * (1.0 + m * (CAPACITANCE_TC * (temp - REFERENCE_KELVIN) - gamma));
    (capacitance, potential)
}

/// Natural logarithm of the SPICE saturation-current temperature factor,
/// `(T/Tnom − 1)·EG/Vt(T) + XTI·ln(T/Tnom)`.
///
/// A diode scales `IS` by `exp(factor / N)`; a BJT scales `IS` by
/// `exp(factor)` and its leakage currents by `exp(factor / NE)`.
pub(crate) fn saturation_log_factor(eg: f64, xti: f64, tnom: f64, temp: f64) -> f64 {
    if temp == tnom {
        return 0.0;
    }
    let vt = BOLTZMANN * temp / CHARGE;
    let ratio = temp / tnom;
    (ratio - 1.0) * eg / vt + xti * ratio.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: f64 = 300.15;

    #[test]
    fn nominal_temperature_is_an_exact_identity() {
        assert_eq!(
            junction_at_temperature(2e-12, 0.75, 0.33, ROOM, ROOM),
            (2e-12, 0.75)
        );
        assert_eq!(saturation_log_factor(1.11, 3.0, ROOM, ROOM), 0.0);
    }

    #[test]
    fn potential_offset_vanishes_at_reference_temperature() {
        assert!(potential_offset(REFERENCE_KELVIN).abs() < 1e-7);
    }

    #[test]
    fn saturation_current_follows_bandgap_law() {
        // IS(T)/IS(Tnom) = (T/Tnom)^XTI · exp(EG·q/k · (1/Tnom − 1/T)).
        let (eg, xti, tnom, temp) = (1.11, 3.0, ROOM, 373.15);
        let expected =
            (temp / tnom).powf(xti) * (eg * CHARGE / BOLTZMANN * (1.0 / tnom - 1.0 / temp)).exp();
        let got = saturation_log_factor(eg, xti, tnom, temp).exp();
        assert!((got / expected - 1.0).abs() < 1e-12, "{got} vs {expected}");
        // A silicon junction's IS roughly doubles every 5–6 K near room temperature.
        let per_degree = saturation_log_factor(eg, xti, ROOM, ROOM + 1.0).exp();
        assert!(per_degree > 1.12 && per_degree < 1.17, "{per_degree}");
    }

    #[test]
    fn junction_potential_falls_and_capacitance_rises_with_temperature() {
        let (c27, v27) = (1e-12, 0.75);
        let (c100, v100) = junction_at_temperature(c27, v27, 0.5, ROOM, 373.15);
        // Built-in potential drops by roughly 1.5–2 mV/K for silicon.
        let slope = (v100 - v27) / 73.0;
        assert!(slope < -1.4e-3 && slope > -2.5e-3, "{slope}");
        assert!(c100 > c27);
        // Reproducing SPICE: going there and back is the identity.
        let (cb, vb) = junction_at_temperature(c100, v100, 0.5, 373.15, ROOM);
        assert!((cb - c27).abs() < 1e-24 && (vb - v27).abs() < 1e-12);
    }

    #[test]
    fn extreme_temperature_keeps_potential_positive() {
        let (c, v) = junction_at_temperature(1e-12, 0.4, 0.3, ROOM, 600.0);
        assert!(v >= MIN_POTENTIAL);
        assert!(c.is_finite());
    }
}
