//! Axis ticks with engineering-unit labels (`500 mV`, `10 kHz`, `-90°`).
//!
//! Tick values come from `xsim_waveform::ticks`; this module decides how
//! many fit, adds the unlabelled minor lines of logarithmic axes and formats
//! the labels with `xsim_spice_engine::units`.

use super::axis::AxisRange;
use super::format;
use xsim_waveform::ticks;

/// One grid line, labelled when `label` is set.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub value: f64,
    pub major: bool,
    pub label: Option<String>,
}

/// Ticks for `range` drawn `length` pixels long, keeping labelled ticks at
/// least `spacing` pixels apart.
pub fn axis_ticks(range: AxisRange, unit: &str, length: f32, spacing: f32) -> Vec<Tick> {
    let target = tick_target(length, spacing);
    if range.log {
        log_ticks(range, unit, length, target)
    } else {
        linear_ticks(range, unit, target)
    }
}

/// How many labelled ticks fit: between 2 and 12.
pub fn tick_target(length: f32, spacing: f32) -> usize {
    if spacing <= 0.0 || !length.is_finite() {
        return 2;
    }
    ((length / spacing).floor() as usize).clamp(2, 12)
}

fn linear_ticks(range: AxisRange, unit: &str, target: usize) -> Vec<Tick> {
    let values = ticks::linear(range.min, range.max, target);
    let step = match values.as_slice() {
        [a, b, ..] => b - a,
        _ => range.max - range.min,
    };
    let digits = label_digits(range.min.abs().max(range.max.abs()), step);
    values
        .into_iter()
        .map(|value| {
            let value = snap_zero(value, step);
            Tick {
                value,
                major: true,
                label: Some(format::tick(value, unit, digits, step)),
            }
        })
        .collect()
}

fn log_ticks(range: AxisRange, unit: &str, length: f32, target: usize) -> Vec<Tick> {
    let labelled = ticks::logarithmic(range.min, range.max, target);
    let label_all = labelled.iter().filter(|(_, major)| *major).count() < 2;
    let mut out: Vec<Tick> = labelled
        .iter()
        .map(|&(value, major)| Tick {
            value,
            major,
            label: (major || label_all).then(|| format::tick(value, unit, 3, value)),
        })
        .collect();
    let decade_px = f64::from(length) / range.display_span();
    if decade_px >= 48.0 {
        out.extend(minor_log_lines(range, &labelled));
    }
    out.sort_by(|a, b| a.value.total_cmp(&b.value));
    out
}

/// The 2…9 × 10ⁿ lines of every decade that are not already ticks.
fn minor_log_lines(range: AxisRange, existing: &[(f64, bool)]) -> Vec<Tick> {
    let first = range.min.log10().floor() as i32;
    let last = range.max.log10().ceil() as i32;
    (first..=last)
        .flat_map(|decade| (2..10).map(move |m| f64::from(m) * 10f64.powi(decade)))
        .filter(|v| range.contains(*v))
        .filter(|v| !existing.iter().any(|(e, _)| ((e - v) / v).abs() < 1e-9))
        .map(|value| Tick {
            value,
            major: false,
            label: None,
        })
        .collect()
}

/// Significant digits that tell ticks `step` apart near `magnitude`, plus one
/// spare digit (trailing zeros are trimmed by the formatter).
pub fn label_digits(magnitude: f64, step: f64) -> usize {
    if step.is_nan() || step <= 0.0 || !magnitude.is_finite() || magnitude == 0.0 {
        return 3;
    }
    let spread = magnitude.log10().floor() - step.log10().floor();
    (spread.max(0.0) as usize + 2).clamp(1, 15)
}

/// Rounding noise such as `5.55e-17` for a tick at zero becomes exactly 0.
fn snap_zero(value: f64, step: f64) -> f64 {
    if value.abs() < step.abs() * 1e-9 {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_count_follows_the_available_length() {
        assert_eq!(tick_target(400.0, 80.0), 5);
        assert_eq!(tick_target(10.0, 80.0), 2);
        assert_eq!(tick_target(10_000.0, 80.0), 12);
        assert_eq!(tick_target(100.0, 0.0), 2);
    }

    #[test]
    fn digits_grow_with_the_magnitude_to_step_ratio() {
        assert_eq!(label_digits(1.0, 0.2), 3);
        assert_eq!(label_digits(10.0, 0.5), 4);
        assert_eq!(label_digits(5e-3, 1e-3), 2);
        assert_eq!(label_digits(0.0, 1.0), 3);
    }

    #[test]
    fn rounding_noise_snaps_to_zero() {
        assert_eq!(snap_zero(5.55e-17, 0.1), 0.0);
        assert_eq!(snap_zero(0.1, 0.1), 0.1);
    }

    #[test]
    fn minor_log_lines_skip_existing_ticks() {
        let range = AxisRange::new(1.0, 100.0, true).unwrap();
        let minors = minor_log_lines(
            range,
            &[(1.0, true), (10.0, true), (100.0, true), (20.0, false)],
        );
        let values: Vec<f64> = minors.iter().map(|t| t.value).collect();
        assert_eq!(values.len(), 15);
        assert!(!values.iter().any(|v| (*v - 20.0).abs() < 1e-9));
        assert!(minors.iter().all(|t| !t.major && t.label.is_none()));
    }
}
