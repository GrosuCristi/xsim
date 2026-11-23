//! Linear and logarithmic axes: data ↔ display ↔ pixel mapping.
//!
//! *Display space* is the space the axis is uniform in: the value itself on a
//! linear axis and `log10(value)` on a logarithmic one. Zooming, panning and
//! decimation buckets all work in display space.
//!
//! [`AxisMap`] implements the GPUI Kit plot [`Scale`] trait, adding the
//! logarithmic scale the base plot infrastructure lacks.

use gpui_kit::base::plot::scale::Scale;

/// A visible axis range in data units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisRange {
    pub min: f64,
    pub max: f64,
    pub log: bool,
}

impl AxisRange {
    /// A finite, non-empty range; logarithmic ranges need `0 < min`.
    pub fn new(min: f64, max: f64, log: bool) -> Option<Self> {
        let valid = min.is_finite() && max.is_finite() && min < max && (!log || min > 0.0);
        valid.then_some(Self { min, max, log })
    }

    /// A range from display-space limits (`log10` limits on a log axis).
    pub fn from_display(min: f64, max: f64, log: bool) -> Option<Self> {
        if log {
            Self::new(10f64.powf(min), 10f64.powf(max), true)
        } else {
            Self::new(min, max, false)
        }
    }

    /// `value` in display space; `NaN` for non-positive values on a log axis.
    pub fn to_display(&self, value: f64) -> f64 {
        if !self.log {
            value
        } else if value > 0.0 {
            value.log10()
        } else {
            f64::NAN
        }
    }

    /// The data value at display coordinate `display`.
    pub fn from_display_value(&self, display: f64) -> f64 {
        if self.log {
            10f64.powf(display)
        } else {
            display
        }
    }

    pub fn display_min(&self) -> f64 {
        self.to_display(self.min)
    }

    pub fn display_max(&self) -> f64 {
        self.to_display(self.max)
    }

    pub fn display_span(&self) -> f64 {
        self.display_max() - self.display_min()
    }

    /// Where `value` sits along the axis: 0 at `min`, 1 at `max`.
    pub fn fraction(&self, value: f64) -> f64 {
        (self.to_display(value) - self.display_min()) / self.display_span()
    }

    /// The data value at fraction `t` along the axis.
    pub fn at_fraction(&self, t: f64) -> f64 {
        self.from_display_value(self.display_min() + t * self.display_span())
    }

    /// Whether `value` lies inside the range (inclusive).
    pub fn contains(&self, value: f64) -> bool {
        (self.min..=self.max).contains(&value)
    }

    /// `[min, max]`, as stored in a `PlotPane`.
    pub fn to_array(&self) -> [f64; 2] {
        [self.min, self.max]
    }
}

/// Maps an [`AxisRange`] onto the pixel interval `start..end`. `end < start`
/// is allowed (a y axis grows upwards).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisMap {
    range: AxisRange,
    start: f32,
    end: f32,
}

impl AxisMap {
    pub fn new(range: AxisRange, start: f32, end: f32) -> Self {
        Self { range, start, end }
    }

    pub fn range(&self) -> AxisRange {
        self.range
    }

    /// Pixel position of `value`; `NaN` for non-positive values on a log axis.
    pub fn px(&self, value: f64) -> f32 {
        self.px_f64(value) as f32
    }

    /// [`Self::px`] in double precision, for clipping far-off-screen points
    /// without bending the visible part of a segment.
    pub fn px_f64(&self, value: f64) -> f64 {
        let t = self.range.fraction(value);
        f64::from(self.start) + t * f64::from(self.end - self.start)
    }

    /// The fraction along the axis at pixel `px` (unclamped).
    pub fn fraction_at(&self, px: f32) -> f64 {
        let length = self.end - self.start;
        if length == 0.0 {
            return 0.0;
        }
        f64::from((px - self.start) / length)
    }

    /// The data value at pixel `px`.
    pub fn value(&self, px: f32) -> f64 {
        self.range.at_fraction(self.fraction_at(px))
    }
}

impl Scale<f64> for AxisMap {
    fn tick(&self, value: &f64) -> Option<f32> {
        let px = self.px(*value);
        px.is_finite().then_some(px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_non_positive_log_ranges() {
        assert!(AxisRange::new(1.0, 1.0, false).is_none());
        assert!(AxisRange::new(2.0, 1.0, false).is_none());
        assert!(AxisRange::new(0.0, 10.0, true).is_none());
        assert!(AxisRange::new(f64::NAN, 1.0, false).is_none());
        assert!(AxisRange::new(-1.0, 1.0, false).is_some());
    }

    #[test]
    fn linear_map_is_affine_and_invertible() {
        let range = AxisRange::new(0.0, 10.0, false).unwrap();
        let map = AxisMap::new(range, 50.0, 150.0);
        assert_eq!(map.px(5.0), 100.0);
        assert_eq!(map.value(125.0), 7.5);
        assert_eq!(map.tick(&10.0), Some(150.0));
    }

    #[test]
    fn log_map_spaces_decades_evenly() {
        let range = AxisRange::new(1.0, 1e4, true).unwrap();
        let map = AxisMap::new(range, 0.0, 400.0);
        assert!((map.px(10.0) - 100.0).abs() < 1e-3);
        assert!((map.px(1e3) - 300.0).abs() < 1e-3);
        assert!((map.value(200.0) - 100.0).abs() < 1e-9);
        assert_eq!(map.tick(&0.0), None);
        assert_eq!(map.tick(&-5.0), None);
    }

    #[test]
    fn inverted_y_axis_grows_upwards() {
        let range = AxisRange::new(-1.0, 1.0, false).unwrap();
        let map = AxisMap::new(range, 200.0, 0.0);
        assert_eq!(map.px(1.0), 0.0);
        assert_eq!(map.px(-1.0), 200.0);
        assert_eq!(map.value(100.0), 0.0);
    }

    #[test]
    fn display_round_trip() {
        let range = AxisRange::from_display(0.0, 3.0, true).unwrap();
        assert_eq!(range.min, 1.0);
        assert!((range.max - 1000.0).abs() < 1e-9);
        assert!((range.at_fraction(1.0 / 3.0) - 10.0).abs() < 1e-9);
        assert!(range.contains(500.0));
        assert_eq!(range.to_array(), [range.min, range.max]);
    }
}
