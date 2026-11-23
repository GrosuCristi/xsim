//! Relative pane heights (D23).
//!
//! Every pane stores an optional relative height, a weight
//! ([`xsim_project::PlotPane::height`]); `None` is a weight of 1. The panes
//! share the height of the pane stack in proportion to their weights, but
//! none gets less than its minimum height; with more panes than fit at
//! their minimum, the stack scrolls and every pane is at its minimum.
//! Because only the weights are stored, a taller or shorter dock scales
//! every pane in proportion.
//!
//! * Dragging the divider between two panes moves height from one to the
//!   other ([`DividerDrag`]): in pixels, from the heights painted when the
//!   press began, keeping the pair's total, so no other pane moves. Each of
//!   the two stops at the minimum height, so with every pane at its minimum
//!   (a scrolling stack) a divider does not move.
//! * A double click on a divider makes its two panes equally high
//!   ([`DividerDrag::equalized`], or [`equalize`] before the panes were
//!   painted), again without moving any other pane. With every pane at its
//!   minimum nothing moves on screen, and only the two stored weights are
//!   made equal.
//! * A new pane gets the mean weight of the others, an average share
//!   ([`new_pane_height`]); removing a pane keeps the others' weights, so
//!   they share its height in proportion. A trace moved to another pane
//!   leaves both heights alone.
//! * Stored weights are rounded to four significant digits ([`stored`]), a
//!   weight of exactly 1 is stored as `None`, and when every pane has the
//!   same weight none is stored ([`tidy`]), so a plot whose panes are all
//!   equally high writes no heights at all.
//! * Opening a project or adapting a setup to a result never changes a
//!   stored height; only a new pane gets one.

use xsim_project::{PlotPane, PlotSetup};

/// The weight of a pane without a usable stored height.
pub const DEFAULT_WEIGHT: f64 = 1.0;

/// How far, in pixels, a painted pane may be from the share its weight
/// gives it while the weights still describe the screen: layout snaps every
/// edge to a device pixel.
const SHARE_TOLERANCE: f64 = 1.5;

/// Divider movement, in pixels, below which a drag changed nothing.
const MOVE_EPSILON: f64 = 0.25;

/// Significant digits a stored weight keeps.
const DIGITS: i32 = 4;

/// The weight of `pane`: its stored height, or [`DEFAULT_WEIGHT`] when it
/// has none or one that is not a positive finite number (a setup handed to
/// the viewer need not have passed the project's validation).
pub fn weight(pane: &PlotPane) -> f64 {
    pane.height
        .filter(|&height| PlotPane::is_valid_height(height))
        .map_or(DEFAULT_WEIGHT, f64::from)
}

/// The weight of every pane, top to bottom.
pub fn weights(setup: &PlotSetup) -> Vec<f64> {
    setup.panes.iter().map(weight).collect()
}

/// Flex grow factors for panes of `weights`: each weight divided by their
/// mean, so the factors sum to the number of panes. Flex layout fills only
/// part of the free space when the factors sum to less than one, so raw
/// weights such as 0.2 and 0.3 would leave the stack partly empty.
pub fn grow_factors(weights: &[f64]) -> Vec<f32> {
    let sum: f64 = weights.iter().copied().map(usable).sum();
    let count = weights.len() as f64;
    weights
        .iter()
        .map(|&w| {
            if sum.is_finite() && sum > 0.0 {
                (usable(w) * count / sum) as f32
            } else {
                1.0
            }
        })
        .collect()
}

/// Splits `total` pixels among panes of `weights` as the screen's flex
/// layout does: in proportion to the weights, except that no pane gets
/// less than `min`, and the others share what is left in proportion. When
/// the panes do not fit at `min` they share `total` equally (an image
/// cannot scroll as the screen does).
pub fn distribute(total: f64, weights: &[f64], min: f64) -> Vec<f64> {
    let count = weights.len();
    if count == 0 {
        return Vec::new();
    }
    let total = total.max(0.0);
    let min = min.max(0.0).min(total / count as f64);
    let weights: Vec<f64> = weights.iter().copied().map(usable).collect();
    let mut held = vec![false; count];
    loop {
        let free = total - min * held.iter().filter(|&&h| h).count() as f64;
        let sum: f64 = weights
            .iter()
            .zip(&held)
            .filter(|&(_, &h)| !h)
            .map(|(w, _)| w)
            .sum();
        let sizes: Vec<f64> = weights
            .iter()
            .zip(&held)
            .map(|(&w, &h)| if h || sum <= 0.0 { min } else { free * w / sum })
            .collect();
        let short: Vec<usize> = (0..count).filter(|&i| !held[i] && sizes[i] < min).collect();
        if short.is_empty() {
            return sizes;
        }
        for i in short {
            held[i] = true;
        }
    }
}

/// A weight that can be divided by: a positive finite number, else the
/// default.
fn usable(weight: f64) -> f64 {
    if weight.is_finite() && weight > 0.0 {
        weight
    } else {
        DEFAULT_WEIGHT
    }
}

/// A drag of the divider between pane `upper` and the pane below it.
#[derive(Debug, Clone, PartialEq)]
pub struct DividerDrag {
    upper: usize,
    /// The weights the drag starts from (see [`DividerDrag::start`]).
    start: Vec<f64>,
    /// The painted heights of the two panes when the press began.
    heights: [f64; 2],
    /// The smallest height either pane may get, in pixels.
    min: f64,
    /// Whether some pane is above its minimum, so a divider can move.
    movable: bool,
}

impl DividerDrag {
    /// Starts a drag of the divider under pane `upper` of panes of
    /// `weights`, given every pane's `painted` height and the minimum pane
    /// height `min` in pixels. `None` when there is no pane below `upper`
    /// or `painted` does not describe every pane.
    ///
    /// The drag starts from `weights` while the divider's two panes are
    /// painted in proportion to them: moving weight between the two then
    /// moves no other pane, whether or not some other pane is held at its
    /// minimum, and only the two panes' stored heights change. When one of
    /// the two is held at its minimum height (the dock got shorter after the
    /// heights were set), it is painted taller than its weight's share, and
    /// moving weight between them would make others jump; the drag then
    /// starts from weights that match the painted heights instead, with the
    /// same total.
    ///
    /// With every pane at its minimum (a stack that scrolls, or just fits)
    /// the divider cannot move, and the painted heights, all equal, say
    /// nothing about the weights: the drag keeps `weights`, so a double
    /// click equalises the two stored weights and keeps every other pane's.
    pub fn start(weights: &[f64], painted: &[f64], upper: usize, min: f64) -> Option<Self> {
        if upper + 1 >= weights.len() || painted.len() != weights.len() {
            return None;
        }
        if painted.iter().any(|&h| !(h.is_finite() && h > 0.0)) {
            return None;
        }
        let weights: Vec<f64> = weights.iter().copied().map(usable).collect();
        let total_weight: f64 = weights.iter().sum();
        let total_height: f64 = painted.iter().sum();
        let (above, below) = (painted[upper], painted[upper + 1]);
        let pair_weight = weights[upper] + weights[upper + 1];
        let described =
            ((above + below) * weights[upper] / pair_weight - above).abs() <= SHARE_TOLERANCE;
        let movable = painted.iter().any(|&h| h > min + SHARE_TOLERANCE);
        let start = if described || !movable {
            weights
        } else {
            painted
                .iter()
                .map(|h| total_weight * h / total_height)
                .collect()
        };
        Some(Self {
            upper,
            start,
            heights: [above, below],
            min,
            movable,
        })
    }

    /// The pane above the divider.
    pub fn upper(&self) -> usize {
        self.upper
    }

    /// The weights the drag started from.
    pub fn start_weights(&self) -> &[f64] {
        &self.start
    }

    /// The weights with the divider `dy` pixels below where the press
    /// began (above it for a negative `dy`), and whether the divider moved
    /// at all. Each of the two panes keeps at least the minimum height, and
    /// the two keep their total height and weight, so no other pane moves.
    /// Without movement the start weights are returned unchanged: also a
    /// pane painted a fraction of a pixel below the minimum (layout snaps
    /// its edges to device pixels) does not count as moved by the clamp.
    pub fn weights_at(&self, dy: f64) -> (Vec<f64>, bool) {
        let [above, below] = self.heights;
        let pair = above + below;
        // Written so that a minimum that is not a number moves nothing
        // (`clamp` needs ordered bounds).
        let room = pair >= 2.0 * self.min;
        if !(self.movable && room && dy.is_finite()) {
            return (self.start.clone(), false);
        }
        let rest = above.clamp(self.min, pair - self.min);
        let placed = (above + dy).clamp(self.min, pair - self.min);
        if (placed - rest).abs() < MOVE_EPSILON {
            return (self.start.clone(), false);
        }
        let mut weights = self.start.clone();
        let sum = weights[self.upper] + weights[self.upper + 1];
        weights[self.upper] = sum * placed / pair;
        weights[self.upper + 1] = sum - weights[self.upper];
        (weights, true)
    }

    /// The weights with the two panes of the divider made equally high:
    /// both get the mean of their start weights, so the pair keeps its
    /// total and no other pane moves, also when a pane was held at its
    /// minimum ([`DividerDrag::start`]).
    pub fn equalized(&self) -> Vec<f64> {
        let mut weights = self.start.clone();
        let mean = (weights[self.upper] + weights[self.upper + 1]) / 2.0;
        weights[self.upper] = mean;
        weights[self.upper + 1] = mean;
        weights
    }
}

/// `weight` as it is stored: rounded to four significant digits, and
/// `None` for the default weight of 1 (or a weight that is not a positive
/// finite number).
pub fn stored(weight: f64) -> Option<f32> {
    if !(weight.is_finite() && weight > 0.0) {
        return None;
    }
    let weight = weight.clamp(f64::from(f32::MIN_POSITIVE), f64::from(f32::MAX));
    let magnitude = weight.log10().floor() as i32;
    let scale = 10f64.powi(DIGITS - 1 - magnitude);
    let rounded = ((weight * scale).round() / scale) as f32;
    let rounded = if rounded.is_finite() {
        rounded
    } else {
        f32::MAX
    };
    (PlotPane::is_valid_height(rounded) && f64::from(rounded) != DEFAULT_WEIGHT).then_some(rounded)
}

/// Stores `weights` as the panes' heights ([`stored`], then [`tidy`]) and
/// returns whether a stored height changed. A pane whose weight is the one
/// it has keeps its stored value untouched.
pub fn store(setup: &mut PlotSetup, weights: &[f64]) -> bool {
    let before: Vec<Option<f32>> = setup.panes.iter().map(|p| p.height).collect();
    for (pane, &w) in setup.panes.iter_mut().zip(weights) {
        if w != weight(pane) {
            pane.height = stored(w);
        }
    }
    tidy(setup);
    setup.panes.iter().map(|p| p.height).ne(before)
}

/// When every pane has the same weight, none needs storing: the heights
/// are cleared, so equally high panes write none. Returns whether a stored
/// height changed.
pub fn tidy(setup: &mut PlotSetup) -> bool {
    let mut all = setup.panes.iter().map(weight);
    let equal = all.next().is_none_or(|first| all.all(|w| w == first));
    let stored = setup.panes.iter().any(|p| p.height.is_some());
    if equal && stored {
        for pane in &mut setup.panes {
            pane.height = None;
        }
    }
    equal && stored
}

/// Makes pane `upper` and the pane below it equally high, keeping their
/// total weight so no other pane moves; returns whether a stored height
/// changed.
pub fn equalize(setup: &mut PlotSetup, upper: usize) -> bool {
    if upper + 1 >= setup.panes.len() {
        return false;
    }
    let mut weights = weights(setup);
    let mean = (weights[upper] + weights[upper + 1]) / 2.0;
    weights[upper] = mean;
    weights[upper + 1] = mean;
    store(setup, &weights)
}

/// The stored height for a pane added to `setup`: the mean weight of its
/// panes, so the new pane gets an average share (`None` without panes, or
/// when the mean is the default weight).
pub fn new_pane_height(setup: &PlotSetup) -> Option<f32> {
    let weights = weights(setup);
    if weights.is_empty() {
        return None;
    }
    stored(weights.iter().sum::<f64>() / weights.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(heights: &[Option<f32>]) -> PlotSetup {
        PlotSetup {
            panes: heights
                .iter()
                .map(|&height| PlotPane {
                    height,
                    ..PlotPane::default()
                })
                .collect(),
            active_pane: 0,
        }
    }

    fn heights(setup: &PlotSetup) -> Vec<Option<f32>> {
        setup.panes.iter().map(|p| p.height).collect()
    }

    fn near(a: &[f64], b: &[f64], tolerance: f64) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
    }

    #[test]
    fn panes_without_a_usable_height_weigh_one() {
        let setup = setup(&[
            None,
            Some(2.5),
            Some(0.0),
            Some(-1.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
        ]);
        assert_eq!(weights(&setup), [1.0, 2.5, 1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn grow_factors_always_fill_the_stack() {
        let sum = |f: &[f32]| f.iter().sum::<f32>();
        let small = grow_factors(&[0.2, 0.3]);
        assert!((small[0] - 0.8).abs() < 1e-6 && (small[1] - 1.2).abs() < 1e-6);
        assert!(
            (sum(&small) - 2.0).abs() < 1e-6,
            "they sum to the pane count"
        );
        assert_eq!(grow_factors(&[1.0, 1.0, 1.0]), [1.0, 1.0, 1.0]);
        // Huge weights neither overflow nor lose their proportion.
        let huge = grow_factors(&[f64::from(f32::MAX), f64::from(f32::MAX) / 2.0]);
        assert!((huge[0] / huge[1] - 2.0).abs() < 1e-5, "{huge:?}");
        assert!(grow_factors(&[]).is_empty());
    }

    #[test]
    fn distribute_shares_in_proportion_above_the_minimum() {
        assert!(near(
            &distribute(600.0, &[2.0, 1.0], 100.0),
            &[400.0, 200.0],
            1e-9
        ));
        // A pane whose share is below the minimum is held there, and the
        // others share the rest in proportion.
        assert!(near(
            &distribute(600.0, &[0.1, 1.0, 1.0], 120.0),
            &[120.0, 240.0, 240.0],
            1e-9
        ));
        // Holding one pane can push another below the minimum.
        assert!(near(
            &distribute(400.0, &[0.1, 1.0, 3.0], 100.0),
            &[100.0, 100.0, 200.0],
            1e-9
        ));
        // Panes that do not fit at the minimum share the height equally.
        assert!(near(
            &distribute(300.0, &[1.0, 5.0, 2.0], 120.0),
            &[100.0, 100.0, 100.0],
            1e-9
        ));
        assert!(distribute(100.0, &[], 10.0).is_empty());
        let sizes = distribute(500.0, &[1.0, f64::NAN, -2.0], 0.0);
        assert!(near(&sizes, &[500.0 / 3.0; 3], 1e-9), "{sizes:?}");
    }

    #[test]
    fn a_divider_drag_moves_height_between_its_two_panes_only() {
        let weights = [1.0, 1.0, 1.0];
        let painted = [200.0, 200.0, 200.0];
        let drag = DividerDrag::start(&weights, &painted, 0, 120.0).expect("a drag");
        assert_eq!(drag.upper(), 0);
        let (moved, changed) = drag.weights_at(50.0);
        assert!(changed);
        assert!(near(&moved, &[1.25, 0.75, 1.0], 1e-12), "{moved:?}");
        let laid_out = distribute(600.0, &moved, 120.0);
        assert!(
            near(&laid_out, &[250.0, 150.0, 200.0], 1e-9),
            "{laid_out:?}"
        );
        // Upwards, the lower pane grows.
        let (moved, _) = drag.weights_at(-60.0);
        assert!(near(
            &distribute(600.0, &moved, 120.0),
            &[140.0, 260.0, 200.0],
            1e-9
        ));
        // No movement, no change.
        let (still, changed) = drag.weights_at(0.0);
        assert!(!changed);
        assert_eq!(still, weights);
    }

    #[test]
    fn a_divider_stops_at_the_minimum_height() {
        let drag = DividerDrag::start(&[1.0, 1.0], &[300.0, 300.0], 0, 120.0).unwrap();
        for (dy, expected) in [(1000.0, [480.0, 120.0]), (-1000.0, [120.0, 480.0])] {
            let (moved, changed) = drag.weights_at(dy);
            assert!(changed);
            let laid_out = distribute(600.0, &moved, 120.0);
            assert!(near(&laid_out, &expected, 1e-9), "{dy}: {laid_out:?}");
        }
        // Two panes at their minimum (a scrolling stack) cannot move.
        let drag = DividerDrag::start(&[1.0, 1.0], &[120.0, 120.0], 0, 120.0).unwrap();
        let (moved, changed) = drag.weights_at(40.0);
        assert!(!changed);
        assert_eq!(moved, [1.0, 1.0]);
    }

    /// A pane held at its minimum is painted taller than its share: the
    /// drag then starts from the painted heights, so the divider follows
    /// the pointer and the third pane stays where it is.
    #[test]
    fn a_pane_held_at_its_minimum_does_not_make_the_others_jump() {
        let weights = [0.1, 1.0, 1.0];
        let painted = distribute(600.0, &weights, 120.0);
        assert!(near(&painted, &[120.0, 240.0, 240.0], 1e-9));
        let drag = DividerDrag::start(&weights, &painted, 0, 120.0).unwrap();
        let total: f64 = drag.start_weights().iter().sum();
        assert!((total - 2.1).abs() < 1e-12, "the total weight is kept");
        let (moved, changed) = drag.weights_at(10.0);
        assert!(changed);
        let laid_out = distribute(600.0, &moved, 120.0);
        assert!(
            near(&laid_out, &[130.0, 230.0, 240.0], 1e-9),
            "{laid_out:?}"
        );
        // Splitting the stored weights of the two instead, the upper pane
        // would stay held at its minimum and the third pane would jump.
        let naive = [1.1 * 130.0 / 360.0, 1.1 * 230.0 / 360.0, 1.0];
        let jumped = distribute(600.0, &naive, 120.0);
        assert!((jumped[0] - 120.0).abs() < 1e-9, "{jumped:?}");
        assert!((jumped[2] - 240.0).abs() > 30.0, "{jumped:?}");
        // Weights that describe the screen are kept as they are.
        let painted = distribute(600.0, &[2.0, 1.0, 3.0], 50.0);
        let drag = DividerDrag::start(&[2.0, 1.0, 3.0], &painted, 1, 50.0).unwrap();
        assert_eq!(drag.start_weights(), [2.0, 1.0, 3.0]);
    }

    /// A double click works from the same weights as a drag: with a pane
    /// held at its minimum, the two panes become equally high and the third
    /// stays where it was.
    #[test]
    fn equalizing_a_divider_moves_no_other_pane() {
        let weights = [0.3, 1.0, 1.0];
        let painted = distribute(628.0, &weights, 120.0);
        assert!(near(&painted, &[120.0, 254.0, 254.0], 1e-9), "{painted:?}");
        let drag = DividerDrag::start(&weights, &painted, 0, 120.0).unwrap();
        let laid_out = distribute(628.0, &drag.equalized(), 120.0);
        assert!(
            near(&laid_out, &[187.0, 187.0, 254.0], 1e-9),
            "{laid_out:?}"
        );
        // From the stored weights the third pane would jump.
        let mut stored = setup(&[Some(0.3), None, None]);
        equalize(&mut stored, 0);
        let naive = distribute(628.0, &super::weights(&stored), 120.0);
        assert!((naive[2] - 254.0).abs() > 10.0, "{naive:?}");
        // Weights that describe the screen give what `equalize` gives.
        let weights = [2.0, 1.0, 1.0];
        let painted = distribute(600.0, &weights, 120.0);
        let drag = DividerDrag::start(&weights, &painted, 0, 120.0).unwrap();
        assert_eq!(drag.equalized(), [1.5, 1.5, 1.0]);
    }

    /// Review: a pane held at its minimum elsewhere in the stack does not
    /// matter to a divider whose two panes are painted by their weights.
    /// The drag starts from the stored weights, so only the two panes'
    /// weights change and the held pane keeps its own, which it needs when
    /// the dock gets taller.
    #[test]
    fn a_held_pane_away_from_the_divider_keeps_its_weight() {
        let weights = [0.3, 1.0, 1.0];
        let painted = distribute(628.0, &weights, 120.0);
        assert!(near(&painted, &[120.0, 254.0, 254.0], 1e-9), "{painted:?}");
        let drag = DividerDrag::start(&weights, &painted, 1, 120.0).unwrap();
        assert_eq!(drag.start_weights(), weights);
        let (moved, changed) = drag.weights_at(20.0);
        assert!(changed);
        assert_eq!(moved[0], 0.3, "the held pane keeps its weight");
        let laid_out = distribute(628.0, &moved, 120.0);
        assert!(
            near(&laid_out, &[120.0, 274.0, 234.0], 1e-9),
            "{laid_out:?}"
        );
        assert_eq!(drag.equalized(), weights, "already equal");
        // The 1 px border at the bottom of every pane but the last, which
        // flex layout gives on top of the share, does not count as held.
        let painted = [201.0, 101.0, 100.0];
        let drag = DividerDrag::start(&[2.0, 1.0, 1.0], &painted, 0, 50.0).unwrap();
        assert_eq!(drag.start_weights(), [2.0, 1.0, 1.0]);
    }

    /// Review: layout snaps pane edges to device pixels, so a pane at its
    /// minimum may be painted a fraction of a pixel below it. Pressing on
    /// its divider and moving the pointer by nothing, or towards that pane,
    /// is no movement, so a click there stores nothing.
    #[test]
    fn a_pane_painted_just_below_the_minimum_is_not_moved_by_the_clamp() {
        let weights = [0.5, 1.0, 1.0];
        let painted = [97.0, 150.0, 150.0];
        let drag = DividerDrag::start(&weights, &painted, 0, 97.5).unwrap();
        for dy in [0.0, 0.3, -5.0] {
            let (still, changed) = drag.weights_at(dy);
            assert!(!changed, "{dy}");
            assert_eq!(still, drag.start_weights(), "{dy}");
        }
        let (moved, changed) = drag.weights_at(5.0);
        assert!(changed);
        let laid_out = distribute(397.0, &moved, 97.5);
        assert!(
            near(&laid_out, &[102.0, 145.0, 150.0], 1e-9),
            "{laid_out:?}"
        );
    }

    /// Review: with every pane at its minimum (a scrolling stack) a divider
    /// cannot move, and a double click equalises the two stored weights
    /// only; the others keep theirs, as the painted heights, all equal, say
    /// nothing about them.
    #[test]
    fn with_every_pane_at_its_minimum_only_the_pair_is_equalized() {
        let weights = [2.0, 1.0, 1.0, 0.5];
        let drag = DividerDrag::start(&weights, &[120.0; 4], 0, 120.0).unwrap();
        assert_eq!(drag.start_weights(), weights);
        assert_eq!(drag.weights_at(30.0), (weights.to_vec(), false));
        assert_eq!(drag.equalized(), [1.5, 1.5, 1.0, 0.5]);
        let drag = DividerDrag::start(&weights, &[120.5; 4], 2, 120.0).unwrap();
        assert_eq!(drag.weights_at(-30.0), (weights.to_vec(), false));
        assert_eq!(drag.equalized(), [2.0, 1.0, 0.75, 0.75]);
    }

    #[test]
    fn a_drag_needs_a_pane_below_and_every_painted_height() {
        // A minimum that is not a number moves nothing, and never panics.
        let drag = DividerDrag::start(&[1.0, 1.0], &[100.0, 100.0], 0, f64::NAN).unwrap();
        assert_eq!(drag.weights_at(10.0), (vec![1.0, 1.0], false));
        assert!(DividerDrag::start(&[1.0, 1.0], &[100.0, 100.0], 1, 50.0).is_none());
        assert!(DividerDrag::start(&[1.0, 1.0], &[100.0], 0, 50.0).is_none());
        assert!(DividerDrag::start(&[1.0, 1.0], &[100.0, 0.0], 0, 50.0).is_none());
        assert!(DividerDrag::start(&[1.0, 1.0], &[100.0, f64::NAN], 0, 50.0).is_none());
    }

    #[test]
    fn stored_weights_keep_four_digits_and_omit_the_default() {
        assert_eq!(stored(1.0), None);
        assert_eq!(stored(0.999_96), None, "rounds to the default");
        assert_eq!(stored(1.234_567), Some(1.235));
        assert_eq!(stored(0.012_345_6), Some(0.012_35));
        assert_eq!(stored(4321.9), Some(4322.0));
        assert_eq!(stored(0.0), None);
        assert_eq!(stored(-2.0), None);
        assert_eq!(stored(f64::NAN), None);
        assert_eq!(stored(1e300), Some(f32::MAX));
        assert!(stored(1e-300).is_some_and(|h| h > 0.0));
        // As written to a project file: the shortest text that reads back.
        assert_eq!(
            stored(2.0 / 3.0).map(|h| h.to_string()).as_deref(),
            Some("0.6667")
        );
    }

    #[test]
    fn storing_weights_rounds_only_what_changed_and_tidies() {
        let mut s = setup(&[Some(1.234_567), None, None]);
        assert!(store(&mut s, &[1.234_567_f32.into(), 1.5, 0.5]));
        assert_eq!(heights(&s), [Some(1.234_567), Some(1.5), Some(0.5)]);
        let current = weights(&s);
        assert!(!store(&mut s, &current), "nothing changed");
        // Equal weights store nothing.
        assert!(store(&mut s, &[0.75, 0.75, 0.75]));
        assert_eq!(heights(&s), [None, None, None]);
        let mut s = setup(&[Some(2.0), Some(2.0)]);
        assert!(tidy(&mut s));
        assert_eq!(heights(&s), [None, None]);
        assert!(!tidy(&mut s));
        let mut single = setup(&[Some(0.5)]);
        assert!(tidy(&mut single));
        assert_eq!(heights(&single), [None]);
    }

    #[test]
    fn equalizing_a_divider_keeps_the_other_panes() {
        let mut s = setup(&[Some(1.5), Some(0.5), Some(2.0)]);
        assert!(equalize(&mut s, 0));
        assert_eq!(heights(&s), [None, None, Some(2.0)]);
        assert!(!equalize(&mut s, 0), "already equal");
        assert!(!equalize(&mut s, 2), "no pane below the last");
        assert!(equalize(&mut s, 1));
        assert_eq!(heights(&s), [None, Some(1.5), Some(1.5)]);
        // Two panes made equal store no height at all.
        let mut s = setup(&[Some(1.8), Some(0.2)]);
        assert!(equalize(&mut s, 0));
        assert_eq!(heights(&s), [None, None]);
        let mut s = setup(&[Some(3.0), Some(1.0)]);
        assert!(equalize(&mut s, 0));
        assert_eq!(heights(&s), [None, None], "2 and 2 are equal");
    }

    #[test]
    fn a_new_pane_gets_the_mean_weight() {
        assert_eq!(new_pane_height(&setup(&[])), None);
        assert_eq!(new_pane_height(&setup(&[None, None])), None);
        assert_eq!(new_pane_height(&setup(&[Some(1.5), Some(0.5)])), None);
        assert_eq!(new_pane_height(&setup(&[Some(0.5), None])), Some(0.75));
        assert_eq!(new_pane_height(&setup(&[Some(3.0), Some(-1.0)])), Some(2.0));
    }
}
