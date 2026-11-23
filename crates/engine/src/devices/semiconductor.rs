//! Shared semiconductor helpers: thermal voltage, junction voltage limiting,
//! depletion charge, temperature scaling.
//!
//! The diode, BJT and JFET models are built from the same pieces:
//!
//! * [`junction`]: overflow-safe Shockley currents (exact up to a current of
//!   1e20 A whatever the saturation current), the critical voltage and
//!   depletion charge with the SPICE forward-bias (`FC`) linearisation.
//! * [`limiting`]: Newton step limiting for pn junctions (`pnjlim`) and FET
//!   gate voltages (`fetlim`), following SPICE3/ngspice.
//! * [`temperature`]: SPICE3 temperature scaling of saturation currents,
//!   built-in potentials and zero-bias capacitances.
//! * [`stamping`]: polarity-aware stamping of multi-controlled nonlinear
//!   branches, charges controlled by two voltages and the SPICE current
//!   convergence test.
//!
//! Every helper here is a pure function of its arguments so the device
//! equations can be unit-tested without assembling a circuit.

mod junction;
mod limiting;
mod stamping;
mod temperature;

pub(crate) use junction::{Depletion, critical_voltage, scaled_exp, shockley};
pub(crate) use limiting::{fetlim, pnjlim};
pub(crate) use stamping::{
    AcControl, Control, ac_branch_current, current_converged, integrate_two_controls,
    stamp_ac_branch, stamp_branch,
};
pub(crate) use temperature::{junction_at_temperature, saturation_log_factor};

use crate::mna::Unknown;

/// Boltzmann constant (J/K).
pub(crate) const BOLTZMANN: f64 = 1.380_649e-23;
/// Elementary charge (C).
pub(crate) const CHARGE: f64 = 1.602_176_634e-19;
/// 0 degrees Celsius in kelvin.
pub(crate) const ZERO_CELSIUS: f64 = 273.15;

/// Thermal voltage kT/q at `celsius`.
pub(crate) fn thermal_voltage(celsius: f64) -> f64 {
    BOLTZMANN * (celsius + ZERO_CELSIUS) / CHARGE
}

/// Measurement temperature `tnom` (°C) of a model in kelvin, or `fallback`
/// (°C) when `tnom` is not a temperature above absolute zero.
pub(crate) fn nominal_kelvin(tnom: f64, fallback: f64) -> f64 {
    let celsius = if tnom.is_finite() && tnom > -ZERO_CELSIUS {
        tnom
    } else {
        fallback
    };
    celsius + ZERO_CELSIUS
}

/// Value of unknown `u` in a real or complex (AC) solution vector (0 for
/// ground).
pub(crate) fn solution_value<T: Copy + Default>(x: &[T], u: Unknown) -> T {
    u.and_then(|i| x.get(i).copied()).unwrap_or_default()
}

/// `value` if it is finite and strictly positive, otherwise `fallback`.
///
/// Device constructors cannot report errors. [`crate::Circuit::validate`]
/// rejects model parameters that would make the equations meaningless (zero
/// emission coefficients, negative junction potentials, NaN) before any
/// simulation builds a device; the fallback to the SPICE default only keeps
/// the constructors total, so a device built without validation still
/// avoids NaN or a panic deep inside a Newton iteration.
pub(crate) fn positive_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

/// `value` if it is finite and not negative, otherwise `fallback`.
pub(crate) fn non_negative_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        fallback
    }
}

/// `value` if it is finite, otherwise `fallback`.
pub(crate) fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

/// Reciprocal of a parameter where infinity (or zero, the SPICE spelling of
/// "not given" for corner currents and Early voltages) means "no effect".
pub(crate) fn inverse_or_zero(value: f64) -> f64 {
    if value.is_finite() && value != 0.0 {
        1.0 / value
    } else {
        0.0
    }
}

/// Conductance of an ohmic series resistance scaled by `area`, or `None`
/// when the resistance is absent (zero, negative or not finite) and no
/// internal node is needed.
pub(crate) fn series_conductance(resistance: f64, area: f64) -> Option<f64> {
    (resistance.is_finite() && resistance > 0.0).then(|| area / resistance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_voltage_matches_codata_at_room_temperature() {
        // kT/q at 300.15 K with the exact 2019 SI constants.
        let expected = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;
        assert_eq!(thermal_voltage(27.0), expected);
        assert!(thermal_voltage(27.0) > 0.0258 && thermal_voltage(27.0) < 0.0259);
    }

    #[test]
    fn sanitizers_replace_meaningless_values() {
        assert_eq!(positive_or(2.0, 1.0), 2.0);
        assert_eq!(positive_or(0.0, 1.0), 1.0);
        assert_eq!(positive_or(-3.0, 1.0), 1.0);
        assert_eq!(positive_or(f64::NAN, 1.0), 1.0);
        assert_eq!(positive_or(f64::INFINITY, 1.0), 1.0);
        assert_eq!(non_negative_or(0.0, 1.0), 0.0);
        assert_eq!(non_negative_or(-1e-3, 1.0), 1.0);
        assert_eq!(finite_or(-4.0, 0.0), -4.0);
        assert_eq!(finite_or(f64::NEG_INFINITY, 0.0), 0.0);
    }

    #[test]
    fn inverse_treats_zero_and_infinity_as_absent() {
        assert_eq!(inverse_or_zero(50.0), 0.02);
        assert_eq!(inverse_or_zero(0.0), 0.0);
        assert_eq!(inverse_or_zero(f64::INFINITY), 0.0);
        assert_eq!(inverse_or_zero(-10.0), -0.1);
    }

    #[test]
    fn series_conductance_scales_with_area() {
        assert_eq!(series_conductance(10.0, 2.0), Some(0.2));
        assert_eq!(series_conductance(0.0, 1.0), None);
        assert_eq!(series_conductance(f64::NAN, 1.0), None);
    }
}
