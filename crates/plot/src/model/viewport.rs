//! Zoom and pan maths on [`AxisRange`]s, in display space so logarithmic
//! axes zoom around the cursor the same way linear ones do.
//!
//! Zooming out and panning are bounded by a *limit*: the full data extent
//! of the x axis, or the automatic (fitted) range of a y axis (D14). The
//! view never grows wider than its limit, so traces never shrink below the
//! plot's width or height, and it never drifts past the data. A view that
//! reaches its limit is reported as `None`, the automatic range, so a fully
//! zoomed-out axis stays automatic. Zooming in is bounded only by
//! floating-point resolution.

use super::axis::AxisRange;

/// The smallest display span relative to the magnitude of its limits; below
/// it `f64` cannot tell neighbouring pixels apart.
const MIN_RELATIVE_SPAN: f64 = 1e-12;

/// Mouse-wheel zoom factor per scrolled line (one wheel notch).
pub const WHEEL_STEP: f64 = 0.85;

/// The zoom factor for a wheel delta of `lines` (positive scrolls up and
/// zooms in).
pub fn wheel_factor(lines: f64) -> f64 {
    WHEEL_STEP.powf(lines)
}

/// `range` scaled by `factor` (< 1 zooms in) about the point at fraction
/// `anchor` along the axis, which stays put. Returns `range` unchanged when
/// the result would be too small or not finite.
pub fn zoom(range: AxisRange, anchor: f64, factor: f64) -> AxisRange {
    let (min, max) = (range.display_min(), range.display_max());
    let pivot = min + anchor * (max - min);
    let new_min = pivot - (pivot - min) * factor;
    let new_max = pivot + (max - pivot) * factor;
    checked(range, new_min, new_max)
}

/// `range` shifted by `delta` of its own span (positive moves the view
/// towards larger values).
pub fn pan(range: AxisRange, delta: f64) -> AxisRange {
    let shift = delta * range.display_span();
    checked(
        range,
        range.display_min() + shift,
        range.display_max() + shift,
    )
}

/// [`zoom`] bounded by `limit`: zooming out (`factor > 1`) never shows more
/// than `limit` and keeps the view inside it ([`within`]); `None` means the
/// view reached `limit` (the automatic range). Zooming in is not bounded,
/// but a view that still shows all of `limit` stays automatic
/// ([`bounded`]).
pub fn zoom_within(
    range: AxisRange,
    limit: AxisRange,
    anchor: f64,
    factor: f64,
) -> Option<AxisRange> {
    let zoomed = zoom(range, anchor, factor);
    if factor > 1.0 {
        within(zoomed, limit)
    } else {
        bounded(zoomed, limit)
    }
}

/// `range`, or `None` (the automatic range) when it shows all of `limit`,
/// so a gesture that ends up showing everything leaves the axis automatic.
pub fn bounded(range: AxisRange, limit: AxisRange) -> Option<AxisRange> {
    (!covers(range, limit)).then_some(range)
}

/// [`pan`] bounded by `limit`: the view moves at most to the edge of
/// `limit`, so an axis that shows all of `limit` does not move at all. A
/// view that already sticks out of `limit` may move back in but never
/// further out. `None` means the result covers `limit` (the automatic
/// range).
pub fn pan_within(range: AxisRange, limit: AxisRange, delta: f64) -> Option<AxisRange> {
    let (min, max) = (range.display_min(), range.display_max());
    let (low, high) = (limit.display_min(), limit.display_max());
    let wanted = delta * (max - min);
    // `max`/`min` rather than `clamp`, which panics on a NaN bound.
    let shift = wanted.max((low - min).min(0.0)).min((high - max).max(0.0));
    let panned = checked(range, min + shift, max + shift);
    (!covers(panned, limit)).then_some(panned)
}

/// `range` kept inside `limit`: `None` when it is at least as wide (the
/// automatic range), otherwise `range` shifted inside `limit` if it sticks
/// out.
pub fn within(range: AxisRange, limit: AxisRange) -> Option<AxisRange> {
    if covers(range, limit) || range.display_span() >= limit.display_span() {
        return None;
    }
    let (min, max) = (range.display_min(), range.display_max());
    let (low, high) = (limit.display_min(), limit.display_max());
    let shift = if min < low {
        low - min
    } else if max > high {
        high - max
    } else {
        return Some(range);
    };
    Some(checked(range, min + shift, max + shift))
}

/// Whether `range` shows all of `limit` (up to rounding).
fn covers(range: AxisRange, limit: AxisRange) -> bool {
    let tolerance = limit.display_span().abs() * 1e-9;
    range.display_min() <= limit.display_min() + tolerance
        && range.display_max() >= limit.display_max() - tolerance
}

/// The sub-range between fractions `from` and `to` (in any order), used by
/// drag-box zoom. `None` when the selection is empty or too small.
pub fn select(range: AxisRange, from: f64, to: f64) -> Option<AxisRange> {
    let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
    let span = range.display_span();
    let min = range.display_min() + lo * span;
    let max = range.display_min() + hi * span;
    usable(min, max).then(|| AxisRange::from_display(min, max, range.log))?
}

fn checked(range: AxisRange, min: f64, max: f64) -> AxisRange {
    if !usable(min, max) {
        return range;
    }
    AxisRange::from_display(min, max, range.log).unwrap_or(range)
}

fn usable(min: f64, max: f64) -> bool {
    let magnitude = min.abs().max(max.abs()).max(f64::MIN_POSITIVE);
    min.is_finite() && max.is_finite() && max - min > magnitude * MIN_RELATIVE_SPAN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(min: f64, max: f64) -> AxisRange {
        AxisRange::new(min, max, false).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn zoom_keeps_the_anchor_fixed() {
        let zoomed = zoom(linear(0.0, 10.0), 0.25, 0.5);
        assert!(close(zoomed.min, 1.25) && close(zoomed.max, 6.25));
        assert!(close(zoomed.at_fraction(0.25), 2.5));
    }

    #[test]
    fn log_zoom_works_in_decades() {
        let range = AxisRange::new(1.0, 1e6, true).unwrap();
        let zoomed = zoom(range, 0.5, 0.5);
        assert!(close(zoomed.min, 10f64.powf(1.5)));
        assert!(close(zoomed.max, 10f64.powf(4.5)));
    }

    #[test]
    fn zoom_refuses_degenerate_spans() {
        let range = linear(1.0, 1.0 + 1e-11);
        assert_eq!(zoom(range, 0.5, 1e-6), range);
        assert_eq!(zoom(range, 0.5, f64::INFINITY), range);
    }

    #[test]
    fn pan_shifts_by_a_fraction_of_the_span() {
        let panned = pan(linear(0.0, 4.0), 0.25);
        assert!(close(panned.min, 1.0) && close(panned.max, 5.0));
        let log = pan(AxisRange::new(10.0, 1000.0, true).unwrap(), -0.5);
        assert!(close(log.min, 1.0) && close(log.max, 100.0));
    }

    #[test]
    fn selection_orders_its_ends() {
        let range = linear(0.0, 100.0);
        let selected = select(range, 0.6, 0.2).unwrap();
        assert!(close(selected.min, 20.0) && close(selected.max, 60.0));
        assert!(select(range, 0.3, 0.3).is_none());
    }

    #[test]
    fn zooming_out_never_exceeds_the_limit() {
        let limit = linear(0.0, 10.0);
        // From the full view, zooming out stays automatic.
        assert_eq!(zoom_within(limit, limit, 0.5, 1.5), None);
        // A zoomed view grows back to the limit and then turns automatic.
        let view = linear(4.0, 6.0);
        let wider = zoom_within(view, limit, 0.5, 2.0).unwrap();
        assert!(close(wider.min, 3.0) && close(wider.max, 7.0));
        assert_eq!(zoom_within(view, limit, 0.5, 6.0), None);
        // Zooming out at the right end shifts the view back inside.
        let edge = linear(8.0, 10.0);
        let out = zoom_within(edge, limit, 0.0, 2.0).unwrap();
        assert!(close(out.min, 6.0) && close(out.max, 10.0), "{out:?}");
        // Zooming in is unbounded by the limit.
        let zoomed_in = zoom_within(limit, limit, 0.5, 0.5).unwrap();
        assert!(close(zoomed_in.min, 2.5) && close(zoomed_in.max, 7.5));
        // A factor of one (a wheel event without movement) keeps the full
        // view automatic, and so does a zoom in that still shows it all.
        assert_eq!(zoom_within(limit, limit, 0.5, 1.0), None);
        assert_eq!(zoom_within(linear(-5.0, 15.0), limit, 0.5, 0.9), None);
    }

    #[test]
    fn bounded_releases_a_view_showing_all_of_its_limit() {
        let limit = linear(0.0, 10.0);
        assert_eq!(bounded(limit, limit), None);
        assert_eq!(bounded(linear(-1.0, 11.0), limit), None);
        assert_eq!(bounded(linear(1.0, 10.0), limit), Some(linear(1.0, 10.0)));
    }

    #[test]
    fn log_limits_work_in_decades() {
        let limit = AxisRange::new(1.0, 1e6, true).unwrap();
        let view = AxisRange::new(1e4, 1e6, true).unwrap();
        let out = zoom_within(view, limit, 1.0, 2.0).unwrap();
        assert!(close(out.min, 100.0) && close(out.max, 1e6), "{out:?}");
        assert_eq!(zoom_within(view, limit, 1.0, 3.0), None);
    }

    #[test]
    fn panning_stops_at_the_limit() {
        let limit = linear(0.0, 10.0);
        // A fully zoomed-out axis does not move.
        assert_eq!(pan_within(limit, limit, 0.3), None);
        assert_eq!(pan_within(limit, limit, -0.3), None);
        // A zoomed view pans freely inside the limit …
        let view = linear(2.0, 4.0);
        let panned = pan_within(view, limit, 0.5).unwrap();
        assert!(close(panned.min, 3.0) && close(panned.max, 5.0));
        // … and stops at its edge.
        let stopped = pan_within(view, limit, 10.0).unwrap();
        assert!(close(stopped.min, 8.0) && close(stopped.max, 10.0));
        let stopped = pan_within(view, limit, -10.0).unwrap();
        assert!(close(stopped.min, 0.0) && close(stopped.max, 2.0));
        // A view already outside may come back but not go further out.
        let outside = linear(-3.0, -1.0);
        assert_eq!(pan_within(outside, limit, -1.0), Some(outside));
        let back = pan_within(outside, limit, 1.0).unwrap();
        assert!(close(back.min, -1.0) && close(back.max, 1.0));
    }

    #[test]
    fn within_shifts_or_releases_a_view() {
        let limit = linear(0.0, 10.0);
        assert_eq!(within(linear(-1.0, 11.0), limit), None);
        assert_eq!(within(linear(2.0, 3.0), limit), Some(linear(2.0, 3.0)));
        let shifted = within(linear(-2.0, 3.0), limit).unwrap();
        assert!(close(shifted.min, 0.0) && close(shifted.max, 5.0));
    }

    #[test]
    fn wheel_up_zooms_in() {
        assert!(wheel_factor(1.0) < 1.0);
        assert!(wheel_factor(-1.0) > 1.0);
        assert_eq!(wheel_factor(0.0), 1.0);
    }
}
