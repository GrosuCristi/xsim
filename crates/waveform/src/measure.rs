//! Waveform measurements (Q135). All functions take strictly increasing `x`
//! samples and matching `y` samples, and an optional inclusive `[from, to]`
//! window (values between samples are linearly interpolated, so windows need
//! not fall on samples). Time-weighted statistics use trapezoidal
//! integration, so non-uniform transient steps are handled correctly.
//!
//! Measurements treat the waveform as the piecewise-linear curve through its
//! samples, which is also how it is drawn. Integrals are exact for that
//! curve: the trapezoid rule for `∫y dx`, and for `∫y² dx` the exact
//! integral of the squared line segment, `h·(a² + ab + b²)/3`. A trapezoid
//! over `y²` would overstate the RMS of a triangle wave, and the exact form
//! also makes a window boundary inside a segment give the same result as a
//! sample there.
//!
//! Window bounds may come in either order (two cursors), are clamped to the
//! data, and a window that is empty after clamping measures nothing. Inputs
//! of different lengths are truncated to the shorter one. Nothing here
//! panics on malformed input; NaN samples simply propagate.

/// Inclusive x window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub from: f64,
    pub to: f64,
}

/// Summary statistics over a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub min: f64,
    pub max: f64,
    /// `max − min`.
    pub peak_to_peak: f64,
    /// Time-weighted mean `∫y dx / Δx`.
    pub average: f64,
    /// `sqrt(∫y² dx / Δx)`.
    pub rms: f64,
    /// `∫y dx`.
    pub integral: f64,
}

/// Statistics of `y` over `window` (whole range when `None`). `None` when
/// fewer than two samples fall in or bound the window.
pub fn stats(x: &[f64], y: &[f64], window: Option<Window>) -> Option<Stats> {
    Some(Clip::new(x, y, window)?.stats())
}

/// Linearly interpolated value at `at` (clamped to the data range).
pub fn value_at(x: &[f64], y: &[f64], at: f64) -> Option<f64> {
    let (x, y) = paired(x, y);
    if x.is_empty() || at.is_nan() {
        return None;
    }
    Some(interpolate(x, y, at))
}

/// Direction of a threshold crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Rising,
    Falling,
    Either,
}

impl Edge {
    fn admits(self, direction: Edge) -> bool {
        self == Edge::Either || self == direction
    }
}

/// Interpolated x positions where `y` crosses `level` in the given direction.
///
/// A crossing is a change of side of `level`. Samples exactly on the level
/// belong to the crossing, which is placed at the first of them, so a signal
/// that only touches the level does not cross it and a crossing through a
/// sample on the level is reported once.
pub fn crossings(x: &[f64], y: &[f64], level: f64, edge: Edge, window: Option<Window>) -> Vec<f64> {
    Clip::new(x, y, window).map_or_else(Vec::new, |clip| clip.crossing_positions(level, edge))
}

/// 10 %–90 % rise time of the first rising edge between the window's min
/// and max levels.
pub fn rise_time(x: &[f64], y: &[f64], window: Option<Window>) -> Option<f64> {
    Clip::new(x, y, window)?.transition_time(0.1, 0.9, Edge::Rising)
}

/// 90 %–10 % fall time of the first falling edge.
pub fn fall_time(x: &[f64], y: &[f64], window: Option<Window>) -> Option<f64> {
    Clip::new(x, y, window)?.transition_time(0.9, 0.1, Edge::Falling)
}

/// Time after the window start at which `y` last enters and then stays
/// within `±tolerance·|final − initial|` of its final value.
///
/// `initial` and `final` are the values at the window bounds. `None` for a
/// negative or NaN tolerance.
pub fn settling_time(x: &[f64], y: &[f64], tolerance: f64, window: Option<Window>) -> Option<f64> {
    if tolerance.is_nan() || tolerance < 0.0 {
        return None;
    }
    Clip::new(x, y, window)?.settling_time(tolerance)
}

/// Fundamental frequency from the mean period between rising crossings of
/// the window average. `None` with fewer than two such crossings.
pub fn frequency(x: &[f64], y: &[f64], window: Option<Window>) -> Option<f64> {
    let clip = Clip::new(x, y, window)?;
    let rising = clip.crossing_positions(clip.stats().average, Edge::Rising);
    let (&first, &last) = (rising.first()?, rising.last()?);
    let periods = rising.len() - 1;
    (periods > 0 && last > first).then(|| periods as f64 / (last - first))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

/// Where `y` crosses a level, and in which direction.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Crossing {
    x: f64,
    direction: Edge,
}

/// The part of a waveform inside a window: interpolated end points around
/// the samples strictly between them. It always spans a positive width.
struct Clip<'a> {
    first: Point,
    inner_x: &'a [f64],
    inner_y: &'a [f64],
    last: Point,
}

impl<'a> Clip<'a> {
    fn new(x: &'a [f64], y: &'a [f64], window: Option<Window>) -> Option<Self> {
        let (x, y) = paired(x, y);
        let (&x_first, &x_last) = (x.first()?, x.last()?);
        let (from, to) = match window {
            None => (x_first, x_last),
            Some(Window { from, to }) => {
                if from.is_nan() || to.is_nan() {
                    return None;
                }
                (from.min(to).max(x_first), from.max(to).min(x_last))
            }
        };
        // `<` is false for NaN, so unordered bounds are rejected too.
        let has_width = from < to;
        if !has_width {
            return None;
        }
        // Samples strictly inside (from, to). `max` keeps the range valid
        // even if unsorted input breaks the binary searches' assumptions.
        let start = x.partition_point(|&v| v <= from);
        let end = x.partition_point(|&v| v < to).max(start);
        Some(Self {
            first: Point {
                x: from,
                y: interpolate(x, y, from),
            },
            inner_x: &x[start..end],
            inner_y: &y[start..end],
            last: Point {
                x: to,
                y: interpolate(x, y, to),
            },
        })
    }

    fn points(&self) -> impl Iterator<Item = Point> + '_ {
        let inner = self
            .inner_x
            .iter()
            .zip(self.inner_y)
            .map(|(&x, &y)| Point { x, y });
        std::iter::once(self.first)
            .chain(inner)
            .chain(std::iter::once(self.last))
    }

    fn segments(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.points().zip(self.points().skip(1))
    }

    fn stats(&self) -> Stats {
        let (min, max) = self
            .points()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), p| {
                (min.min(p.y), max.max(p.y))
            });
        let (mut integral, mut square_integral) = (0.0, 0.0);
        for (a, b) in self.segments() {
            let h = b.x - a.x;
            integral += h * (a.y + b.y) / 2.0;
            square_integral += h * (a.y * a.y + a.y * b.y + b.y * b.y) / 3.0;
        }
        let span = self.last.x - self.first.x;
        Stats {
            min,
            max,
            peak_to_peak: max - min,
            average: integral / span,
            // a² + ab + b² is never negative, but rounding can leave a tiny
            // negative sum when the signal is ~0. (A comparison rather than
            // `f64::max` keeps NaN visible.)
            rms: if square_integral < 0.0 {
                0.0
            } else {
                (square_integral / span).sqrt()
            },
            integral,
        }
    }

    /// Positions of the crossings of `level` that `edge` admits.
    fn crossing_positions(&self, level: f64, edge: Edge) -> Vec<f64> {
        self.crossings(level)
            .into_iter()
            .filter(|crossing| edge.admits(crossing.direction))
            .map(|crossing| crossing.x)
            .collect()
    }

    /// Every crossing of `level`, in x order (see [`crossings`]).
    fn crossings(&self, level: f64) -> Vec<Crossing> {
        let mut found = Vec::new();
        if level.is_nan() {
            return found;
        }
        // The last point strictly off the level, and the first point on the
        // level since then (where a crossing through the level is placed).
        let mut off_level: Option<(Point, f64)> = None;
        let mut on_level: Option<f64> = None;
        for point in self.points() {
            let offset = point.y - level;
            if offset.is_nan() {
                // A gap in the data: nothing crosses it.
                (off_level, on_level) = (None, None);
                continue;
            }
            if offset == 0.0 {
                on_level.get_or_insert(point.x);
                continue;
            }
            if let Some((previous, previous_offset)) = off_level
                && (previous_offset < 0.0) != (offset < 0.0)
            {
                let x = on_level.unwrap_or_else(|| {
                    // Opposite signs, so the denominator is non-zero.
                    let t = previous_offset / (previous_offset - offset);
                    previous.x + t * (point.x - previous.x)
                });
                let direction = if offset > 0.0 {
                    Edge::Rising
                } else {
                    Edge::Falling
                };
                found.push(Crossing { x, direction });
            }
            (off_level, on_level) = (Some((point, offset)), None);
        }
        found
    }

    /// Time from the crossing of the `start` fraction of the min–max range
    /// to the next crossing of the `end` fraction, for the first edge in
    /// `direction` that passes through both.
    fn transition_time(&self, start: f64, end: f64, direction: Edge) -> Option<f64> {
        let Stats { min, max, .. } = self.stats();
        // False for a flat signal and for NaN extremes alike.
        let has_range = max > min;
        if !has_range {
            return None;
        }
        let level = |fraction: f64| min + fraction * (max - min);
        let starts = self.crossing_positions(level(start), direction);
        let ends = self.crossing_positions(level(end), direction);
        // The edge begins at the last start crossing before its end
        // crossing, so a false start that falls back is skipped.
        ends.iter().find_map(|&end| {
            let begin = starts.iter().rev().find(|&&begin| begin <= end)?;
            Some(end - begin)
        })
    }

    fn settling_time(&self, tolerance: f64) -> Option<f64> {
        let target = self.last.y;
        let band = tolerance * (target - self.first.y).abs();
        if band.is_nan() {
            return None;
        }
        // NaN samples count as unsettled.
        let settled = |p: Point| (p.y - target).abs() <= band;
        // The last segment that starts outside the band ends inside it: the
        // final point is the target itself.
        let entry =
            self.segments()
                .filter(|&(a, _)| !settled(a))
                .last()
                .map_or(self.first.x, |(a, b)| {
                    let boundary = target + band.copysign(a.y - target);
                    let t = (a.y - boundary) / (a.y - b.y);
                    if t.is_finite() {
                        a.x + t.clamp(0.0, 1.0) * (b.x - a.x)
                    } else {
                        b.x
                    }
                });
        Some(entry - self.first.x)
    }
}

/// `x` and `y` truncated to their common length.
fn paired<'a>(x: &'a [f64], y: &'a [f64]) -> (&'a [f64], &'a [f64]) {
    let n = x.len().min(y.len());
    (&x[..n], &y[..n])
}

/// Linear interpolation of non-empty, equally long `x`/`y` at `at`,
/// clamped to the end values.
fn interpolate(x: &[f64], y: &[f64], at: f64) -> f64 {
    let i = x.partition_point(|&v| v <= at);
    match (i.checked_sub(1), x.get(i)) {
        (None, _) => y[0],
        (Some(_), None) => y[y.len() - 1],
        (Some(left), Some(&x_right)) => {
            let (x_left, y_left) = (x[left], y[left]);
            if x_left == at {
                y_left
            } else {
                y_left + (y[i] - y_left) * ((at - x_left) / (x_right - x_left))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};

    const X: [f64; 5] = [0.0, 1.0, 2.0, 4.0, 5.0];
    const Y: [f64; 5] = [0.0, 2.0, 2.0, -2.0, 0.0];

    fn window(from: f64, to: f64) -> Option<Window> {
        Some(Window { from, to })
    }

    #[test]
    fn stats_of_piecewise_linear_data_are_exact() {
        let s = stats(&X, &Y, None).unwrap();
        assert_eq!((s.min, s.max, s.peak_to_peak), (-2.0, 2.0, 4.0));
        // Areas: 1 + 2 + 0 + (-1) = 2.
        assert_eq!(s.integral, 2.0);
        assert_eq!(s.average, 2.0 / 5.0);
        // ∫y²: ramps contribute h·peak²/3, the plateau h·peak², and the
        // 2 → −2 segment h·(4 − 4 + 4)/3.
        let square = 4.0 / 3.0 + 4.0 + 2.0 * 4.0 / 3.0 + 4.0 / 3.0;
        assert!((s.rms - (square / 5.0f64).sqrt()).abs() < 1e-15);
    }

    #[test]
    fn window_bounds_are_interpolated_and_order_free() {
        let s = stats(&X, &Y, window(3.5, 0.5)).unwrap();
        assert_eq!(s.max, 2.0);
        assert_eq!(s.min, -1.0, "V at 3.5 is interpolated");
        // Areas: 0.75 (0.5→1) + 2 (1→2) + 1 (2→3) − 0.25 (3→3.5).
        assert!((s.integral - 3.5).abs() < 1e-15);
        assert!((s.average - 3.5 / 3.0).abs() < 1e-15);
    }

    #[test]
    fn windows_are_clamped_to_the_data() {
        assert_eq!(stats(&X, &Y, window(-10.0, 10.0)), stats(&X, &Y, None));
        assert_eq!(stats(&X, &Y, window(6.0, 9.0)), None);
        assert_eq!(stats(&X, &Y, window(2.0, 2.0)), None);
        assert_eq!(stats(&X, &Y, window(f64::NAN, 2.0)), None);
        let inside_one_segment = stats(&X, &Y, window(0.25, 0.75)).unwrap();
        assert_eq!((inside_one_segment.min, inside_one_segment.max), (0.5, 1.5));
    }

    #[test]
    fn degenerate_inputs_measure_nothing() {
        assert_eq!(stats(&[], &[], None), None);
        assert_eq!(stats(&[1.0], &[1.0], None), None);
        assert_eq!(stats(&[1.0, 1.0], &[1.0, 2.0], None), None);
        assert_eq!(value_at(&[], &[], 0.0), None);
        assert_eq!(value_at(&X, &Y, f64::NAN), None);
        assert!(crossings(&[], &[], 0.0, Edge::Either, None).is_empty());
        assert_eq!(rise_time(&[0.0, 1.0], &[1.0, 1.0], None), None);
        assert_eq!(settling_time(&X, &Y, -0.1, None), None);
        assert_eq!(settling_time(&X, &Y, f64::NAN, None), None);
        assert_eq!(frequency(&X, &Y, None), None);
    }

    #[test]
    fn mismatched_lengths_use_the_common_prefix() {
        let s = stats(&X, &Y[..3], None).unwrap();
        assert_eq!(s.integral, 3.0);
        assert_eq!(value_at(&X[..2], &Y, 10.0), Some(2.0));
    }

    #[test]
    fn unsorted_or_nan_input_does_not_panic() {
        let x = [3.0, f64::NAN, 1.0, 0.0, 2.0];
        let y = [1.0, 2.0, f64::NAN, 4.0, 5.0];
        let _ = stats(&x, &y, None);
        let _ = stats(&x, &y, window(0.5, 2.5));
        let _ = crossings(&x, &y, 1.5, Edge::Either, window(0.0, 3.0));
        let _ = rise_time(&x, &y, None);
        let _ = settling_time(&x, &y, 0.1, None);
        let _ = frequency(&x, &y, window(-1.0, 4.0));
        let _ = value_at(&x, &y, 1.5);
    }

    #[test]
    fn value_at_interpolates_and_clamps() {
        assert_eq!(value_at(&X, &Y, 0.25), Some(0.5));
        assert_eq!(value_at(&X, &Y, 3.0), Some(0.0));
        assert_eq!(value_at(&X, &Y, 4.0), Some(-2.0));
        assert_eq!(value_at(&X, &Y, -1.0), Some(0.0));
        assert_eq!(value_at(&X, &Y, 9.0), Some(0.0));
        assert_eq!(value_at(&[1.0], &[7.0], 0.0), Some(7.0));
    }

    #[test]
    fn crossings_are_interpolated_with_direction() {
        assert_eq!(crossings(&X, &Y, 1.0, Edge::Rising, None), [0.5]);
        assert_eq!(crossings(&X, &Y, 1.0, Edge::Falling, None), [2.5]);
        assert_eq!(crossings(&X, &Y, 1.0, Edge::Either, None), [0.5, 2.5]);
        assert_eq!(
            crossings(&X, &Y, 1.0, Edge::Either, window(1.0, 5.0)),
            [2.5]
        );
    }

    #[test]
    fn samples_on_the_level_are_counted_once() {
        let x = [0.0, 1.0, 2.0, 3.0];
        // Through the level at a sample: one crossing, at that sample.
        assert_eq!(
            crossings(&x, &[-1.0, 0.0, 1.0, 2.0], 0.0, Edge::Either, None),
            [1.0]
        );
        // A plateau on the level: the crossing is where the plateau starts.
        assert_eq!(
            crossings(&x, &[-1.0, 0.0, 0.0, 1.0], 0.0, Edge::Rising, None),
            [1.0]
        );
        // Touching the level from below is not a crossing.
        assert!(crossings(&x, &[-1.0, 0.0, -1.0, -2.0], 0.0, Edge::Either, None).is_empty());
        // Neither are end points on the level: Y starts and ends at 0 and
        // only crosses it between samples, at x = 3.
        assert_eq!(crossings(&X, &Y, 0.0, Edge::Either, None), [3.0]);
    }

    #[test]
    fn nan_samples_break_crossings() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [-1.0, f64::NAN, 1.0, -1.0];
        assert_eq!(crossings(&x, &y, 0.0, Edge::Either, None), [2.5]);
    }

    #[test]
    fn rise_time_skips_a_false_start() {
        // Rises to 0.5, falls back to 0, then makes a full 0 → 1 edge.
        let x = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [0.0, 0.5, 0.0, 0.0, 1.0, 1.0];
        assert!((rise_time(&x, &y, None).unwrap() - 0.8).abs() < 1e-15);
        assert_eq!(fall_time(&x, &y, None), None);
    }

    #[test]
    fn fall_time_of_a_ramp() {
        let x = [0.0, 1.0, 11.0, 12.0];
        let y = [5.0, 5.0, -5.0, -5.0];
        assert!((fall_time(&x, &y, None).unwrap() - 8.0).abs() < 1e-12);
        assert_eq!(rise_time(&x, &y, None), None);
    }

    #[test]
    fn settling_time_of_a_step_with_overshoot() {
        // 0 → 1.5 → 1 with a ±10 % band [0.9, 1.1]: enters for good while
        // falling from 1.5 through 1.1 at x = 2 + 0.4/0.5.
        let x = [0.0, 1.0, 2.0, 3.0, 4.0];
        let y = [0.0, 1.0, 1.5, 1.0, 1.0];
        let t = settling_time(&x, &y, 0.1, None).unwrap();
        assert!((t - 2.8).abs() < 1e-12, "{t}");
        // Measured from the window start, where V = 0.5: the band shrinks
        // to ±0.05 and is entered at x = 2 + 0.45/0.5.
        let t = settling_time(&x, &y, 0.1, window(0.5, 4.0)).unwrap();
        assert!((t - 2.4).abs() < 1e-12, "{t}");
        // Already settled when the window starts inside the band.
        assert_eq!(settling_time(&x, &y, 0.1, window(3.0, 4.0)), Some(0.0));
    }

    #[test]
    fn settling_with_zero_tolerance_requires_reaching_the_final_value() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [0.0, 2.0, 1.0, 1.0];
        assert_eq!(settling_time(&x, &y, 0.0, None), Some(2.0));
    }

    fn config() -> Config {
        Config {
            cases: 200,
            rng_seed: RngSeed::Fixed(0x5EED),
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// Strictly increasing x samples with matching y samples.
    fn waveform() -> impl Strategy<Value = (Vec<f64>, Vec<f64>)> {
        prop::collection::vec((0.01f64..3.0, -10.0f64..10.0), 2..40).prop_map(|steps| {
            let mut x = Vec::with_capacity(steps.len());
            let mut position = 0.0;
            for (step, _) in &steps {
                position += step;
                x.push(position);
            }
            (x, steps.into_iter().map(|(_, y)| y).collect())
        })
    }

    proptest! {
        #![proptest_config(config())]

        /// Adding a sample on the drawn line changes no statistic, because
        /// the integrals are exact for the piecewise-linear curve. (A
        /// trapezoid over y² would fail this for the RMS.)
        #[test]
        fn an_interpolated_sample_changes_nothing((x, y) in waveform(), at in 0.0f64..1.0) {
            let span = x[x.len() - 1] - x[0];
            let position = x[0] + at * span;
            prop_assume!(x.iter().all(|&xi| (xi - position).abs() > 1e-9 * span));
            let index = x.partition_point(|&xi| xi < position);
            let (mut refined_x, mut refined_y) = (x.clone(), y.clone());
            refined_x.insert(index, position);
            refined_y.insert(index, value_at(&x, &y, position).unwrap());

            let original = stats(&x, &y, None).unwrap();
            let refined = stats(&refined_x, &refined_y, None).unwrap();
            // The interpolated value may round past its neighbours by an ulp.
            prop_assert!((original.min - refined.min).abs() <= 1e-12 * 10.0);
            prop_assert!((original.max - refined.max).abs() <= 1e-12 * 10.0);
            let scale = 10.0 * span;
            prop_assert!((original.integral - refined.integral).abs() <= 1e-12 * scale);
            prop_assert!((original.rms - refined.rms).abs() <= 1e-12 * scale);
        }

        /// Windows split additively: the integral over [a, c] is the sum
        /// over [a, b] and [b, c].
        #[test]
        fn integral_is_additive_over_windows((x, y) in waveform(), split in 0.05f64..0.95) {
            let (start, stop) = (x[0], x[x.len() - 1]);
            let middle = start + split * (stop - start);
            let whole = stats(&x, &y, None).unwrap();
            let left = stats(&x, &y, window(start, middle)).unwrap();
            let right = stats(&x, &y, window(middle, stop)).unwrap();
            prop_assert!((left.integral + right.integral - whole.integral).abs() <= 1e-9 * (stop - start) * 10.0);
            prop_assert!(left.min >= whole.min && left.max <= whole.max);
        }

        /// Statistics are bounded by the extremes, and RMS by the peak.
        #[test]
        fn statistics_are_consistent((x, y) in waveform()) {
            let s = stats(&x, &y, None).unwrap();
            prop_assert!(s.min <= s.average + 1e-9 && s.average <= s.max + 1e-9);
            prop_assert!(s.rms + 1e-9 >= s.average.abs());
            prop_assert!(s.rms <= s.min.abs().max(s.max.abs()) + 1e-9);
            prop_assert_eq!(s.peak_to_peak, s.max - s.min);
        }

        /// Every reported crossing lies inside the window and on the level.
        #[test]
        fn crossings_lie_on_the_level((x, y) in waveform(), level in -10.0f64..10.0) {
            let found = crossings(&x, &y, level, Edge::Either, None);
            prop_assert!(found.windows(2).all(|w| w[0] <= w[1]));
            for c in found {
                prop_assert!(x[0] <= c && c <= x[x.len() - 1]);
                let value = value_at(&x, &y, c).unwrap();
                prop_assert!((value - level).abs() <= 1e-9 * 20.0, "{} at {}", value, c);
            }
        }
    }
}
