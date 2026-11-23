//! The ranges actually shown: a pane's stored range, or an automatic one
//! fitted to the data.

use super::axis::AxisRange;
use super::traces::extent;
use xsim_waveform::ticks::padded;

/// Fraction of the data span added above and below automatic y ranges.
pub const Y_PADDING: f64 = 0.05;

/// The extent of an x axis, computed once per result: scanning millions
/// of samples on every frame would make hovering lag (Q140).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AxisExtent {
    /// Finite minimum and maximum.
    pub all: Option<(f64, f64)>,
    /// Minimum and maximum of the positive samples (for log axes).
    pub positive: Option<(f64, f64)>,
}

impl AxisExtent {
    pub fn of(axis: &[f64]) -> Self {
        let positive = axis.iter().filter(|v| v.is_finite() && **v > 0.0).fold(
            None,
            |acc: Option<(f64, f64)>, &v| match acc {
                None => Some((v, v)),
                Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
            },
        );
        Self {
            all: extent(axis),
            positive,
        }
    }
}

/// The x range to show: `stored` when valid, otherwise the data extent
/// (positive samples only on a log axis).
pub fn x_range(stored: Option<[f64; 2]>, axis: AxisExtent, log: bool) -> AxisRange {
    if let Some(range) = stored.and_then(|[min, max]| AxisRange::new(min, max, log)) {
        return range;
    }
    let data = if log { axis.positive } else { axis.all };
    match data {
        Some((min, max)) if min < max => fallback(AxisRange::new(min, max, log), log),
        Some((value, _)) => fitted(value, value, log, Y_PADDING),
        None => fallback(None, log),
    }
}

/// The y range to show: `stored` when valid, otherwise the union of
/// `extents` padded by [`Y_PADDING`].
pub fn y_range(
    stored: Option<[f64; 2]>,
    extents: impl IntoIterator<Item = (f64, f64)>,
    log: bool,
) -> AxisRange {
    if let Some(range) = stored.and_then(|[min, max]| AxisRange::new(min, max, log)) {
        return range;
    }
    let union = extents
        .into_iter()
        .filter(|(lo, _)| !log || *lo > 0.0)
        .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)));
    match union {
        Some((lo, hi)) => fitted(lo, hi, log, Y_PADDING),
        None => fallback(None, log),
    }
}

/// `[lo, hi]` padded by `fraction` (in decades on a log axis).
fn fitted(lo: f64, hi: f64, log: bool, fraction: f64) -> AxisRange {
    let range = if log {
        let (a, b) = padded(lo.log10(), hi.log10(), fraction);
        AxisRange::from_display(a, b, true)
    } else {
        let (a, b) = padded(lo, hi, fraction);
        AxisRange::new(a, b, false)
    };
    fallback(range, log)
}

/// `range`, or a unit range when there is nothing sensible to show.
fn fallback(range: Option<AxisRange>, log: bool) -> AxisRange {
    range.unwrap_or(if log {
        AxisRange {
            min: 1.0,
            max: 10.0,
            log: true,
        }
    } else {
        AxisRange {
            min: -1.0,
            max: 1.0,
            log: false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_ranges_win_when_valid() {
        let range = x_range(Some([1.0, 2.0]), AxisExtent::of(&[0.0, 5.0]), false);
        assert_eq!((range.min, range.max), (1.0, 2.0));
        let invalid_on_log = x_range(Some([0.0, 2.0]), AxisExtent::of(&[1.0, 100.0]), true);
        assert_eq!((invalid_on_log.min, invalid_on_log.max), (1.0, 100.0));
        let y = y_range(Some([-3.0, 3.0]), [(0.0, 1.0)], false);
        assert_eq!((y.min, y.max), (-3.0, 3.0));
    }

    #[test]
    fn automatic_x_uses_the_exact_data_extent() {
        let range = x_range(None, AxisExtent::of(&[0.0, 1e-3, 5e-3]), false);
        assert_eq!((range.min, range.max), (0.0, 5e-3));
        let log = x_range(None, AxisExtent::of(&[0.0, 10.0, 1e5]), true);
        assert_eq!((log.min, log.max), (10.0, 1e5));
    }

    #[test]
    fn nothing_to_show_gives_unit_ranges() {
        assert_eq!(
            x_range(None, AxisExtent::default(), false).to_array(),
            [-1.0, 1.0]
        );
        assert_eq!(y_range(None, [], true).to_array(), [1.0, 10.0]);
    }
}
