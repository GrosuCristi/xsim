//! The measurements panel (Q135): statistics and timing of one trace over
//! the cursor window or the visible range, computed by
//! `xsim_waveform::measure`.

use super::cursors::inverse_unit;
use xsim_waveform::measure::{self, Window};

/// Settling band as a fraction of the final step size.
pub const SETTLING_TOLERANCE: f64 = 0.02;

/// One row of the measurements panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub name: &'static str,
    /// `None` when the quantity does not exist for this data (no edge, …).
    pub value: Option<f64>,
    pub unit: String,
}

/// Every measurement of `y` against `x` over `window` (`None` = all data):
/// the statistics, plus frequency and edge timing when `x` is time (they
/// mean nothing against frequency or a swept source). `x` may be
/// increasing or decreasing.
pub fn measure_all(
    x: &[f64],
    y: &[f64],
    window: Option<(f64, f64)>,
    x_unit: &str,
    y_unit: &str,
) -> Vec<Measurement> {
    let (x, y) = ascending(x, y);
    let window = window.map(|(from, to)| Window { from, to });
    let stats = measure::stats(&x, &y, window);
    let mut rows = vec![
        row("Average", stats.map(|s| s.average), y_unit),
        row("RMS", stats.map(|s| s.rms), y_unit),
        row("Minimum", stats.map(|s| s.min), y_unit),
        row("Maximum", stats.map(|s| s.max), y_unit),
        row("Peak-to-peak", stats.map(|s| s.peak_to_peak), y_unit),
    ];
    if x_unit == "s" {
        rows.extend(timing(&x, &y, window, x_unit));
    }
    rows
}

fn row(name: &'static str, value: Option<f64>, unit: &str) -> Measurement {
    Measurement {
        name,
        value,
        unit: unit.to_string(),
    }
}

/// Frequency and edge timing of a time-domain trace.
fn timing(x: &[f64], y: &[f64], window: Option<Window>, x_unit: &str) -> Vec<Measurement> {
    vec![
        row(
            "Frequency",
            measure::frequency(x, y, window),
            &inverse_unit(x_unit),
        ),
        row("Rise time", measure::rise_time(x, y, window), x_unit),
        row("Fall time", measure::fall_time(x, y, window), x_unit),
        row(
            "Settling time",
            measure::settling_time(x, y, SETTLING_TOLERANCE, window),
            x_unit,
        ),
    ]
}

/// The samples in increasing x order (a DC sweep may run downwards).
fn ascending(x: &[f64], y: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = x.len().min(y.len());
    let (mut x, mut y) = (x[..n].to_vec(), y[..n].to_vec());
    if x.first() > x.last() {
        x.reverse();
        y.reverse();
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_rows_only_on_time_axes() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [0.0, 1.0, 0.0, 1.0];
        let names = |unit| -> Vec<&str> {
            measure_all(&x, &y, None, unit, "V")
                .iter()
                .map(|m| m.name)
                .collect()
        };
        assert_eq!(names("Hz").len(), 5);
        assert_eq!(names("s").len(), 9);
        assert!(names("s").contains(&"Rise time"));
        let rms = measure_all(&x, &y, None, "s", "V")[1].clone();
        assert_eq!((rms.name, rms.unit.as_str()), ("RMS", "V"));
    }

    #[test]
    fn descending_sweeps_are_reversed() {
        let (x, y) = ascending(&[3.0, 2.0, 1.0], &[30.0, 20.0, 10.0, 99.0]);
        assert_eq!(x, vec![1.0, 2.0, 3.0]);
        assert_eq!(y, vec![10.0, 20.0, 30.0]);
    }
}
