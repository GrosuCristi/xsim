//! Evaluated traces: expression text → samples → values on a pane's y scale.
//!
//! Evaluation happens when the result or the setup changes, never while
//! painting, so a pane with millions of samples repaints cheaply (Q140).

use super::units::expression_unit;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use xsim_project::{PlotSetup, YScale};
use xsim_spice_engine::SimulationResult;
use xsim_waveform::{ExprError, Expression, Series};

/// One trace expression evaluated against a result.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    pub series: Series,
    /// Unit inferred from the expression (`"V"`, `"A"`, `"dB"`, `"°"`, …).
    pub unit: &'static str,
}

/// Parses and evaluates `text` against `result`.
pub fn evaluate(text: &str, result: &SimulationResult) -> Result<Evaluated, ExprError> {
    let expression = Expression::parse(text)?;
    let series = expression.evaluate(result)?;
    Ok(Evaluated {
        series,
        unit: expression.unit(),
    })
}

/// A trace's samples on one y scale, ready to plot.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotValues {
    /// Process-unique identity, so caches never mistake one trace for another.
    pub id: u64,
    pub y: Vec<f64>,
    /// Unit of `y` on this scale (`"dB"` on a decibel axis, `"°"` for phase).
    pub unit: String,
    /// Finite minimum and maximum of `y` (positive values only on a log axis).
    pub extent: Option<(f64, f64)>,
}

/// `series` (whose expression has unit `unit`) on a pane with y scale `scale`:
/// magnitudes for linear and log axes, `20·log10|y|` for decibels and the
/// unwrapped phase in degrees for phase axes. Expressions that already yield
/// decibels or degrees (`db(…)`, `ph(…)`) are not converted twice.
///
/// One rule holds for every trace on a phase axis: it is unwrapped (INT-9).
/// `V(out)` and `ph(V(out))` in the same phase pane are the same line, and
/// a phase difference such as `ph(V(out))-ph(V(in))` is continuous too.
/// `ph(…)` itself stays wrapped to (−180°, 180°], as LTspice's `.meas` reports
/// it, so it reads wrapped on a linear axis, in the readout of a linear pane
/// and in a CSV export.
pub fn plot_values(series: &Series, unit: &str, scale: YScale) -> PlotValues {
    let (y, unit) = match scale {
        YScale::Linear => (magnitudes(series), unit.to_string()),
        YScale::Log => (positive(magnitudes(series)), unit.to_string()),
        YScale::Decibel if unit == "dB" => (magnitudes(series), unit.to_string()),
        YScale::Decibel => (decibels(series), "dB".to_string()),
        YScale::Phase if unit == "°" => (unwrap_degrees(&magnitudes(series)), unit.to_string()),
        YScale::Phase => (unwrap_degrees(&phases(series)), "°".to_string()),
    };
    PlotValues {
        id: next_id(),
        extent: extent(&y),
        y,
        unit,
    }
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Real samples as they are; complex samples as their magnitude.
fn magnitudes(series: &Series) -> Vec<f64> {
    match series {
        Series::Real(values) => values.clone(),
        Series::Complex(values) => values.iter().map(|c| c.norm()).collect(),
    }
}

fn positive(values: Vec<f64>) -> Vec<f64> {
    values
        .into_iter()
        .map(|v| if v > 0.0 { v } else { f64::NAN })
        .collect()
}

fn decibels(series: &Series) -> Vec<f64> {
    magnitudes(series)
        .into_iter()
        .map(|m| {
            let db = 20.0 * m.abs().log10();
            if db.is_finite() { db } else { f64::NAN }
        })
        .collect()
}

/// Phase in degrees: `atan2` of complex samples, 0° or 180° for real ones.
fn phases(series: &Series) -> Vec<f64> {
    match series {
        Series::Real(values) => values
            .iter()
            .map(|v| if *v < 0.0 { 180.0 } else { 0.0 })
            .collect(),
        Series::Complex(values) => values.iter().map(|c| c.arg().to_degrees()).collect(),
    }
}

/// Removes the ±360° jumps of a wrapped phase so Bode phase plots stay
/// continuous. Non-finite samples pass through and do not reset the offset.
pub fn unwrap_degrees(phases: &[f64]) -> Vec<f64> {
    let mut offset = 0.0;
    let mut previous: Option<f64> = None;
    phases
        .iter()
        .map(|&phase| {
            if !phase.is_finite() {
                return phase;
            }
            if let Some(prev) = previous {
                let jump = phase + offset - prev;
                offset -= 360.0 * full_turns(jump);
            }
            let unwrapped = phase + offset;
            previous = Some(unwrapped);
            unwrapped
        })
        .collect()
}

/// Whole turns to remove from a phase step of `jump` degrees so it lies in
/// `[-180°, 180°]`; an exact half turn (a real sign flip) is kept.
fn full_turns(jump: f64) -> f64 {
    if jump > 180.0 {
        ((jump - 180.0) / 360.0).ceil()
    } else if jump < -180.0 {
        -((-jump - 180.0) / 360.0).ceil()
    } else {
        0.0
    }
}

/// Finite minimum and maximum of `values`.
pub fn extent(values: &[f64]) -> Option<(f64, f64)> {
    values
        .iter()
        .filter(|v| v.is_finite())
        .fold(None, |acc, &v| match acc {
            None => Some((v, v)),
            Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
        })
}

/// Evaluated traces and their per-scale values, keyed by expression text.
#[derive(Debug, Default)]
pub struct TraceStore {
    evaluated: HashMap<String, Result<Arc<Evaluated>, ExprError>>,
    values: HashMap<(String, u8), Arc<PlotValues>>,
    /// Unit of every synced expression, available or not (D9 axes).
    units: HashMap<String, &'static str>,
}

impl TraceStore {
    /// Forgets everything, e.g. when a new result arrives.
    pub fn clear(&mut self) {
        self.evaluated.clear();
        self.values.clear();
        self.units.clear();
    }

    /// The unit of trace expression `expression` (cached once synced).
    pub fn unit(&self, expression: &str) -> &'static str {
        self.units
            .get(expression)
            .copied()
            .unwrap_or_else(|| expression_unit(expression))
    }

    /// Evaluates every trace of `setup` that is not cached yet and drops
    /// entries no pane uses any more.
    pub fn sync(&mut self, setup: &PlotSetup, result: Option<&SimulationResult>) {
        self.units.retain(|text, _| {
            setup
                .panes
                .iter()
                .flat_map(|p| &p.traces)
                .any(|t| t.expression == *text)
        });
        for trace in setup.panes.iter().flat_map(|p| &p.traces) {
            if !self.units.contains_key(&trace.expression) {
                let unit = expression_unit(&trace.expression);
                self.units.insert(trace.expression.clone(), unit);
            }
        }
        let Some(result) = result else {
            self.evaluated.clear();
            self.values.clear();
            return;
        };
        let wanted: Vec<(String, YScale)> = setup
            .panes
            .iter()
            .flat_map(|pane| {
                pane.traces
                    .iter()
                    .map(move |t| (t.expression.clone(), pane.y_scale))
            })
            .collect();
        self.evaluated
            .retain(|text, _| wanted.iter().any(|(w, _)| w == text));
        self.values.retain(|(text, scale), _| {
            wanted
                .iter()
                .any(|(w, s)| w == text && scale_key(*s) == *scale)
        });
        for (text, scale) in wanted {
            if self.check(&text, result).is_ok() {
                self.ensure_values(&text, scale);
            }
        }
    }

    /// Evaluates `text` against `result` (once; the evaluation is cached)
    /// and reports why it cannot be plotted, if it cannot.
    pub fn check(&mut self, text: &str, result: &SimulationResult) -> Result<(), ExprError> {
        self.evaluated
            .entry(text.to_string())
            .or_insert_with(|| evaluate(text, result).map(Arc::new))
            .as_ref()
            .map(|_| ())
            .map_err(Clone::clone)
    }

    fn ensure_values(&mut self, text: &str, scale: YScale) {
        let Some(evaluated) = self.evaluated(text).cloned() else {
            return;
        };
        self.values
            .entry((text.to_string(), scale_key(scale)))
            .or_insert_with(|| Arc::new(plot_values(&evaluated.series, evaluated.unit, scale)));
    }

    /// The evaluation of `expression`, if it was synced.
    pub fn evaluated(&self, expression: &str) -> Option<&Arc<Evaluated>> {
        self.evaluated.get(expression)?.as_ref().ok()
    }

    /// Why `expression` cannot be plotted, if it cannot.
    pub fn error(&self, expression: &str) -> Option<&ExprError> {
        self.evaluated.get(expression)?.as_ref().err()
    }

    /// The values of `expression` on scale `scale`, if it was synced and
    /// evaluated successfully.
    pub fn values(&self, expression: &str, scale: YScale) -> Option<&Arc<PlotValues>> {
        self.values.get(&(expression.to_string(), scale_key(scale)))
    }
}

/// `YScale` is not `Hash`; this stands in for it as a map key.
fn scale_key(scale: YScale) -> u8 {
    match scale {
        YScale::Linear => 0,
        YScale::Log => 1,
        YScale::Decibel => 2,
        YScale::Phase => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_scale_keeps_real_samples() {
        let values = plot_values(&Series::Real(vec![-1.0, 0.0, 2.0]), "V", YScale::Linear);
        assert_eq!(values.y, vec![-1.0, 0.0, 2.0]);
        assert_eq!(values.unit, "V");
        assert_eq!(values.extent, Some((-1.0, 2.0)));
    }

    #[test]
    fn log_scale_hides_non_positive_samples() {
        let values = plot_values(&Series::Real(vec![-1.0, 0.0, 10.0]), "A", YScale::Log);
        assert!(values.y[0].is_nan() && values.y[1].is_nan());
        assert_eq!(values.extent, Some((10.0, 10.0)));
    }

    #[test]
    fn decibels_of_real_samples_and_db_expressions() {
        let values = plot_values(&Series::Real(vec![10.0, 0.1, 0.0]), "V", YScale::Decibel);
        assert_eq!(values.unit, "dB");
        assert!((values.y[0] - 20.0).abs() < 1e-12);
        assert!((values.y[1] + 20.0).abs() < 1e-12);
        assert!(values.y[2].is_nan());
        let already = plot_values(&Series::Real(vec![-3.0]), "dB", YScale::Decibel);
        assert_eq!(already.y, vec![-3.0]);
    }

    #[test]
    fn phase_of_real_samples_is_zero_or_half_turn() {
        let values = plot_values(&Series::Real(vec![1.0, -1.0]), "V", YScale::Phase);
        assert_eq!(values.y, vec![0.0, 180.0]);
        assert_eq!(values.unit, "°");
    }

    #[test]
    fn unwrapping_removes_full_turn_jumps() {
        let unwrapped = unwrap_degrees(&[170.0, -175.0, -160.0, f64::NAN, 175.0]);
        assert_eq!(unwrapped[0], 170.0);
        assert_eq!(unwrapped[1], 185.0);
        assert_eq!(unwrapped[2], 200.0);
        assert!(unwrapped[3].is_nan());
        assert_eq!(unwrapped[4], 175.0);
    }

    /// INT-9: a degree-valued trace on a phase axis is unwrapped like a
    /// complex one, so `ph(…)` and the complex trace it was taken from agree.
    #[test]
    fn degree_traces_are_unwrapped_on_phase_axes() {
        let wrapped = [-170.0, 175.0, 120.0, f64::NAN, 90.0];
        let values = plot_values(&Series::Real(wrapped.to_vec()), "°", YScale::Phase);
        assert_eq!(values.unit, "°");
        assert_eq!(values.y[0], -170.0);
        assert_eq!(values.y[1], -185.0);
        assert_eq!(values.y[2], -240.0);
        assert!(values.y[3].is_nan(), "gaps pass through");
        assert_eq!(values.y[4], -270.0, "a gap keeps the offset");
        assert_eq!(values.extent, Some((-270.0, -170.0)));
        // Already continuous: unchanged.
        let smooth = [10.0, -100.0, -190.0, -260.0];
        let values = plot_values(&Series::Real(smooth.to_vec()), "°", YScale::Phase);
        assert_eq!(values.y, smooth);
        // On a linear axis `ph(…)` reads as evaluated: wrapped.
        let linear = plot_values(&Series::Real(wrapped.to_vec()), "°", YScale::Linear);
        assert_eq!(linear.y[1], 175.0);
    }

    #[test]
    fn ids_are_unique() {
        let a = plot_values(&Series::Real(vec![1.0]), "", YScale::Linear);
        let b = plot_values(&Series::Real(vec![1.0]), "", YScale::Linear);
        assert_ne!(a.id, b.id);
    }
}
