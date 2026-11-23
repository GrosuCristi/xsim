//! Number formatting for labels and readouts: engineering notation with SI
//! prefixes (`500 mV`, `10 kHz`) for physical units, plain decimals for the
//! logarithmic and angular units (`-3.01 dB`, `-45°`), where a prefix such
//! as `mdB` would be meaningless.

use xsim_spice_engine::units::{format_value, format_value_digits};

/// Decimals shown for readouts in decibels or degrees.
const PLAIN_DECIMALS: usize = 3;
/// The most decimals a plain tick label gets.
const MAX_TICK_DECIMALS: usize = 6;

/// Whether `unit` is shown without SI prefixes.
pub fn is_plain(unit: &str) -> bool {
    matches!(unit, "dB" | "°")
}

/// A readout value: `"2.5 mA"`, `"-3.01 dB"`, `"-22.35°"`.
pub fn value(value: f64, unit: &str) -> String {
    if is_plain(unit) {
        plain(value, PLAIN_DECIMALS, unit)
    } else {
        format_value(value, unit)
    }
}

/// A tick label for ticks `step` apart, with `digits` significant digits in
/// engineering notation.
pub fn tick(value: f64, unit: &str, digits: usize, step: f64) -> String {
    if is_plain(unit) {
        plain(value, step_decimals(step), unit)
    } else {
        format_value_digits(value, unit, digits)
    }
}

/// Decimals that tell ticks `step` apart: 0 for 10, 1 for 0.5.
fn step_decimals(step: f64) -> usize {
    if !(step.is_finite() && step > 0.0) {
        return PLAIN_DECIMALS;
    }
    (-step.log10().floor()).clamp(0.0, MAX_TICK_DECIMALS as f64) as usize
}

/// `value` with at most `decimals` decimals, trailing zeros trimmed, then the
/// unit (degrees attach without a space).
fn plain(value: f64, decimals: usize, unit: &str) -> String {
    if !value.is_finite() {
        return format_value(value, "");
    }
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    let number = if text == "-0" { "0" } else { text };
    match unit {
        "°" => format!("{number}°"),
        _ => format!("{number} {unit}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_and_degrees_have_no_prefixes() {
        assert_eq!(value(0.8492, "dB"), "0.849 dB");
        assert_eq!(value(24.03e-6, "dB"), "0 dB");
        assert_eq!(value(-22.35, "°"), "-22.35°");
        assert_eq!(value(-0.0001, "°"), "0°");
        assert_eq!(value(f64::NAN, "dB"), "NaN");
    }

    #[test]
    fn physical_units_use_engineering_notation() {
        assert_eq!(value(2.5e-3, "A"), "2.5 mA");
        assert_eq!(tick(0.5, "V", 3, 0.1), "500 mV");
    }

    #[test]
    fn plain_ticks_show_the_step_resolution() {
        assert_eq!(tick(-20.0, "dB", 3, 10.0), "-20 dB");
        assert_eq!(tick(-0.5, "dB", 3, 0.5), "-0.5 dB");
        assert_eq!(tick(-45.0, "°", 3, 15.0), "-45°");
        assert_eq!(step_decimals(0.02), 2);
        assert_eq!(step_decimals(f64::NAN), PLAIN_DECIMALS);
    }
}
