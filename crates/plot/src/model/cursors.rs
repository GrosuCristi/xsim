//! The two measurement cursors (Q131, D14, D19): sample snapping, hit
//! testing, each cursor's value of its trace, and the Δx, Δy and 1/Δx
//! readouts.
//!
//! A cursor moves on the x axis only (D19): it is a vertical line placed or
//! dragged along x and snapped to a sample. Its y is the value of the trace
//! it follows at that x ([`value_at`]), interpolated linearly in display
//! space between samples, so it lies on the drawn line also on logarithmic
//! axes.

use super::axis::{AxisMap, AxisRange};

/// One of the two cursors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorId {
    First,
    Second,
}

impl CursorId {
    /// Both cursors, in order.
    pub const ALL: [CursorId; 2] = [CursorId::First, CursorId::Second];

    /// `"C1"` or `"C2"`.
    pub fn label(self) -> &'static str {
        match self {
            Self::First => "C1",
            Self::Second => "C2",
        }
    }

    /// `"1"` or `"2"`, the cursor's flag on the plot.
    pub fn number(self) -> &'static str {
        match self {
            Self::First => "1",
            Self::Second => "2",
        }
    }

    /// 0 or 1, for per-cursor arrays.
    pub fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

/// Cursor state: each cursor's x, the only coordinate a cursor has (D19).
/// Placing a cursor or dragging its line snaps x to a sample.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cursors {
    pub enabled: bool,
    pub first: Option<f64>,
    pub second: Option<f64>,
}

impl Cursors {
    pub fn get(&self, id: CursorId) -> Option<f64> {
        match id {
            CursorId::First => self.first,
            CursorId::Second => self.second,
        }
    }

    pub fn set(&mut self, id: CursorId, x: f64) {
        match id {
            CursorId::First => self.first = Some(x),
            CursorId::Second => self.second = Some(x),
        }
    }

    /// Both positions in increasing order, when both cursors are placed.
    pub fn window(&self) -> Option<(f64, f64)> {
        let (a, b) = (self.first?, self.second?);
        Some((a.min(b), a.max(b)))
    }

    /// The enabled cursor whose line is within `tolerance` pixels of `px`,
    /// the nearer one when both are.
    pub fn hit(&self, map: &AxisMap, px: f32, tolerance: f32) -> Option<CursorId> {
        if !self.enabled {
            return None;
        }
        [CursorId::First, CursorId::Second]
            .into_iter()
            .filter_map(|id| Some((id, (map.px(self.get(id)?) - px).abs())))
            .filter(|(_, distance)| *distance <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The cursor a click at `px` moves: the unplaced one first, otherwise
    /// the nearer one.
    pub fn nearest(&self, map: &AxisMap, px: f32) -> CursorId {
        match (self.first, self.second) {
            (None, _) => CursorId::First,
            (_, None) => CursorId::Second,
            (Some(a), Some(b)) => {
                if (map.px(a) - px).abs() <= (map.px(b) - px).abs() {
                    CursorId::First
                } else {
                    CursorId::Second
                }
            }
        }
    }

    /// Turns the cursors on at one and three quarters of the visible range,
    /// snapped to samples.
    pub fn enable(&mut self, axis: &[f64], visible: AxisRange) {
        self.enabled = true;
        self.first = snap(axis, visible.at_fraction(0.25), visible.log).map(|i| axis[i]);
        self.second = snap(axis, visible.at_fraction(0.75), visible.log).map(|i| axis[i]);
    }
}

/// Index of the sample nearest `x` (comparing `log10` distances on a log
/// axis). `axis` may be increasing or decreasing; `None` when it is empty.
pub fn snap(axis: &[f64], x: f64, log: bool) -> Option<usize> {
    if axis.is_empty() || !x.is_finite() {
        return None;
    }
    let ascending = axis.first() <= axis.last();
    let right = if ascending {
        axis.partition_point(|v| *v < x)
    } else {
        axis.partition_point(|v| *v > x)
    };
    let candidates = [right.checked_sub(1), (right < axis.len()).then_some(right)];
    let distance = |i: usize| {
        if log && axis[i] > 0.0 && x > 0.0 {
            (axis[i].log10() - x.log10()).abs()
        } else {
            (axis[i] - x).abs()
        }
    };
    candidates
        .into_iter()
        .flatten()
        .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
}

/// The value of `y` at the sample nearest `x`.
pub fn sample_at(axis: &[f64], y: &[f64], x: f64, log: bool) -> Option<f64> {
    y.get(snap(axis, x, log)?).copied()
}

/// The trace a cursor follows: the pane it is in and its expression, so
/// the link survives other traces being added, removed or reordered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorTrace {
    pub pane: usize,
    pub expression: String,
}

/// `value` in display space: `log10` on a logarithmic axis (`NaN` for
/// non-positive values), the value itself otherwise.
fn display(value: f64, log: bool) -> f64 {
    match log {
        false => value,
        true if value > 0.0 => value.log10(),
        true => f64::NAN,
    }
}

fn from_display(value: f64, log: bool) -> f64 {
    if log { 10f64.powf(value) } else { value }
}

/// The value of the trace sampled as `y` over `axis` at `x`: interpolated
/// linearly (in display space, `x_log`/`y_log`) between the samples around
/// `x`, so it lies on the drawn line; the end sample outside the data.
/// `None` when the samples there are not finite (a gap in the trace).
pub fn value_at(axis: &[f64], y: &[f64], x: f64, x_log: bool, y_log: bool) -> Option<f64> {
    let n = axis.len().min(y.len());
    if n == 0 || !x.is_finite() {
        return None;
    }
    let finite = |v: f64| display(v, y_log).is_finite().then_some(v);
    let ascending = axis[0] <= axis[n - 1];
    let right = if ascending {
        axis[..n].partition_point(|v| *v < x)
    } else {
        axis[..n].partition_point(|v| *v > x)
    };
    if right == 0 {
        return finite(y[0]);
    }
    if right == n {
        return finite(y[n - 1]);
    }
    if axis[right] == x {
        return finite(y[right]);
    }
    let left = right - 1;
    let (x0, x1, xd) = (
        display(axis[left], x_log),
        display(axis[right], x_log),
        display(x, x_log),
    );
    let t = (xd - x0) / (x1 - x0);
    let (y0, y1) = (display(y[left], y_log), display(y[right], y_log));
    match (y0.is_finite(), y1.is_finite()) {
        (true, true) if t.is_finite() => Some(from_display(y0 + t * (y1 - y0), y_log)),
        (true, true) if t < 0.5 => Some(y[left]),
        (true, true) => Some(y[right]),
        _ => None,
    }
}

/// The cursors' deltas along x.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XDelta {
    /// `x2 − x1`.
    pub dx: f64,
    /// `1 / |Δx|`, e.g. the frequency of a period between the cursors.
    pub inverse: Option<f64>,
}

/// Δx between the two cursors, when both are placed.
pub fn x_delta(cursors: &Cursors) -> Option<XDelta> {
    let dx = cursors.second? - cursors.first?;
    let inverse = (dx != 0.0).then(|| 1.0 / dx.abs());
    Some(XDelta { dx, inverse })
}

/// The unit of `1/Δx` for an x axis in `unit`.
pub fn inverse_unit(unit: &str) -> String {
    match unit {
        "s" => "Hz".to_string(),
        "Hz" => "s".to_string(),
        "" => String::new(),
        other => format!("/{other}"),
    }
}

/// One trace's values at both cursors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readout {
    pub first: Option<f64>,
    pub second: Option<f64>,
}

impl Readout {
    /// Reads `y` at both cursor positions ([`value_at`]).
    pub fn new(cursors: &Cursors, axis: &[f64], y: &[f64], x_log: bool, y_log: bool) -> Self {
        let at = |x: Option<f64>| x.and_then(|x| value_at(axis, y, x, x_log, y_log));
        Self {
            first: at(cursors.first),
            second: at(cursors.second),
        }
    }

    /// `y2 − y1`.
    pub fn delta(&self) -> Option<f64> {
        Some(self.second? - self.first?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> AxisMap {
        AxisMap::new(AxisRange::new(0.0, 10.0, false).unwrap(), 0.0, 100.0)
    }

    #[test]
    fn snapping_finds_the_nearest_sample() {
        let axis = [0.0, 1.0, 2.0, 4.0];
        assert_eq!(snap(&axis, 2.9, false), Some(2));
        assert_eq!(snap(&axis, 3.1, false), Some(3));
        assert_eq!(snap(&axis, -5.0, false), Some(0));
        assert_eq!(snap(&axis, 99.0, false), Some(3));
        assert_eq!(snap(&[], 1.0, false), None);
        assert_eq!(snap(&axis, f64::NAN, false), None);
    }

    #[test]
    fn snapping_handles_descending_and_log_axes() {
        let descending = [5.0, 4.0, 3.0];
        assert_eq!(snap(&descending, 3.4, false), Some(2));
        let decades = [1.0, 10.0, 100.0];
        // 4 is nearer to 1 linearly but nearer to 10 in log10 distance.
        assert_eq!(snap(&decades, 4.0, true), Some(1));
        assert_eq!(snap(&decades, 4.0, false), Some(0));
    }

    #[test]
    fn values_between_samples_are_interpolated() {
        let axis = [0.0, 1.0, 2.0, 4.0];
        let y = [0.0, 10.0, 20.0, 0.0];
        assert_eq!(value_at(&axis, &y, 1.0, false, false), Some(10.0));
        assert_eq!(value_at(&axis, &y, 0.25, false, false), Some(2.5));
        assert_eq!(value_at(&axis, &y, 3.0, false, false), Some(10.0));
        assert_eq!(value_at(&axis, &y, -1.0, false, false), Some(0.0));
        assert_eq!(value_at(&axis, &y, 9.0, false, false), Some(0.0));
        assert_eq!(value_at(&[], &[], 1.0, false, false), None);
        let gap = [0.0, f64::NAN, 2.0, 3.0];
        assert_eq!(value_at(&axis, &gap, 0.5, false, false), None);
        // Descending axes work too.
        assert_eq!(
            value_at(&[2.0, 1.0, 0.0], &[4.0, 2.0, 0.0], 1.5, false, false),
            Some(3.0)
        );
    }

    #[test]
    fn log_axes_interpolate_on_the_drawn_line() {
        // A straight line on log-log axes: y = x.
        let axis = [1.0, 100.0];
        let y = [1.0, 100.0];
        let at_ten = value_at(&axis, &y, 10.0, true, true).unwrap();
        assert!((at_ten - 10.0).abs() < 1e-9, "{at_ten}");
        // Halfway in decades; interpolating in hertz would give 0.18.
        let linear_y = value_at(&axis, &[0.0, 2.0], 10.0, true, false).unwrap();
        assert!((linear_y - 1.0).abs() < 1e-12, "{linear_y}");
    }

    #[test]
    fn hit_testing_and_nearest_cursor() {
        let mut cursors = Cursors {
            enabled: true,
            first: Some(2.0),
            second: Some(6.0),
        };
        assert_eq!(cursors.hit(&map(), 23.0, 5.0), Some(CursorId::First));
        assert_eq!(cursors.hit(&map(), 40.0, 5.0), None);
        assert_eq!(cursors.nearest(&map(), 45.0), CursorId::Second);
        cursors.enabled = false;
        assert_eq!(cursors.hit(&map(), 20.0, 5.0), None);
        let unplaced = Cursors::default();
        assert_eq!(unplaced.nearest(&map(), 90.0), CursorId::First);
    }

    #[test]
    fn deltas_and_readouts() {
        let cursors = Cursors {
            enabled: true,
            first: Some(1e-3),
            second: Some(3e-3),
        };
        let delta = x_delta(&cursors).unwrap();
        assert!((delta.dx - 2e-3).abs() < 1e-15);
        assert!((delta.inverse.unwrap() - 500.0).abs() < 1e-9);
        assert_eq!(cursors.window(), Some((1e-3, 3e-3)));
        let axis = [0.0, 1e-3, 2e-3, 3e-3];
        let y = [0.0, 0.5, 0.8, 0.9];
        let readout = Readout::new(&cursors, &axis, &y, false, false);
        assert_eq!((readout.first, readout.second), (Some(0.5), Some(0.9)));
        assert!((readout.delta().unwrap() - 0.4).abs() < 1e-12);
        assert_eq!(inverse_unit("s"), "Hz");
        assert_eq!(inverse_unit("V"), "/V");
    }

    #[test]
    fn enabling_places_cursors_on_samples() {
        let axis: Vec<f64> = (0..=100).map(f64::from).collect();
        let mut cursors = Cursors::default();
        cursors.enable(&axis, AxisRange::new(0.0, 100.0, false).unwrap());
        assert!(cursors.enabled);
        assert_eq!((cursors.first, cursors.second), (Some(25.0), Some(75.0)));
        assert_eq!(CursorId::Second.label(), "C2");
    }
}
