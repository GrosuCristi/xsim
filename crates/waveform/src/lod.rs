//! Level-of-detail decimation for drawing (Q140).
//!
//! A trace with millions of samples is reduced to at most two points per
//! horizontal pixel column while preserving every local extreme, so the
//! drawn line looks identical to drawing every sample and no peak is lost.
//!
//! Each column keeps up to four samples — first, minimum, maximum and last —
//! which is what a polyline needs to cover the column's full vertical
//! extent and to join its neighbours exactly where the full-resolution line
//! would. The output therefore has at most `4·columns + 2` drawn vertices.
//! A break vertex (both coordinates NaN) is inserted between two kept
//! samples whenever a non-finite sample lies between them, so gaps survive
//! decimation; break vertices never repeat and never start or end the
//! output. Finite data thus yields at most `4·columns + 2` vertices in
//! total, and data with gaps at most `8·columns + 3`.

/// One output vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
}

impl Vertex {
    /// The marker that interrupts the line (see [`decimate`]).
    pub const BREAK: Self = Self {
        x: f64::NAN,
        y: f64::NAN,
    };

    /// Whether this vertex interrupts the line rather than being drawn.
    pub fn is_break(&self) -> bool {
        self.x.is_nan() || self.y.is_nan()
    }
}

/// Decimates `(x, y)` for display over `[x_min, x_max]` split into `columns`
/// equal buckets in *display* space (`log_x` buckets in log10(x)). Returns
/// the vertices to connect in order: for each bucket the first, min, max and
/// last samples in x order (duplicates removed), plus the nearest sample
/// just outside each end of the range so lines reach the plot edges.
/// Non-finite samples break the line: they are returned as `NaN` vertices.
///
/// Returns nothing for `columns == 0`, a non-finite or empty range, or a
/// logarithmic range that is not strictly positive. With `log_x`, samples
/// at `x <= 0` have no display position and are treated like non-finite
/// samples. Inputs of different lengths are truncated to the shorter one.
pub fn decimate(
    x: &[f64],
    y: &[f64],
    x_min: f64,
    x_max: f64,
    columns: usize,
    log_x: bool,
) -> Vec<Vertex> {
    let Some(buckets) = Buckets::new(x_min, x_max, columns, log_x) else {
        return Vec::new();
    };
    let n = x.len().min(y.len());
    let (x, y) = (&x[..n], &y[..n]);
    let drawable = |i: usize| is_drawable(x[i], y[i], log_x);

    // Indices of the samples inside the range.
    let start = x.partition_point(|&v| v < x_min);
    let end = x.partition_point(|&v| v <= x_max).max(start);

    let mut kept = Vec::with_capacity((end - start).min(columns.saturating_mul(4)) + 2);
    if let Some(before) = start.checked_sub(1).filter(|&i| drawable(i)) {
        kept.push(before);
    }
    let mut bucket: Option<(usize, Extremes)> = None;
    for i in (start..end).filter(|&i| drawable(i)) {
        let index = buckets.index(x[i]);
        match &mut bucket {
            Some((current, extremes)) if *current == index => extremes.include(i, y),
            _ => {
                if let Some((_, extremes)) = bucket.replace((index, Extremes::new(i))) {
                    extremes.push_into(&mut kept);
                }
            }
        }
    }
    if let Some((_, extremes)) = bucket {
        extremes.push_into(&mut kept);
    }
    if end < n && drawable(end) {
        kept.push(end);
    }

    connect(x, y, &kept, log_x)
}

/// Turns kept sample indices (strictly increasing) into vertices, with a
/// break wherever an undrawable sample was skipped in between.
fn connect(x: &[f64], y: &[f64], kept: &[usize], log_x: bool) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(kept.len());
    let mut previous: Option<usize> = None;
    for &i in kept {
        if let Some(previous) = previous
            && (previous + 1..i).any(|k| !is_drawable(x[k], y[k], log_x))
        {
            vertices.push(Vertex::BREAK);
        }
        vertices.push(Vertex { x: x[i], y: y[i] });
        previous = Some(i);
    }
    vertices
}

fn is_drawable(x: f64, y: f64, log_x: bool) -> bool {
    x.is_finite() && y.is_finite() && (!log_x || x > 0.0)
}

/// Maps x values to bucket indices in display space.
struct Buckets {
    log_x: bool,
    origin: f64,
    /// Buckets per display unit.
    scale: f64,
    last: usize,
}

impl Buckets {
    fn new(x_min: f64, x_max: f64, columns: usize, log_x: bool) -> Option<Self> {
        let valid = columns > 0
            && x_min.is_finite()
            && x_max.is_finite()
            && x_min < x_max
            && (!log_x || x_min > 0.0);
        if !valid {
            return None;
        }
        let display = |v: f64| if log_x { v.log10() } else { v };
        let (low, high) = (display(x_min), display(x_max));
        let scale = columns as f64 / (high - low);
        // A range narrower than one ulp in display space cannot be split.
        scale.is_finite().then_some(Self {
            log_x,
            origin: low,
            scale,
            last: columns - 1,
        })
    }

    /// Bucket of `x` (inside the range). The mapping only uses monotone
    /// floating-point operations, so indices never decrease as `x` grows.
    fn index(&self, x: f64) -> usize {
        let display = if self.log_x { x.log10() } else { x };
        // `as` saturates, so values a rounding step outside the range land
        // in the first or last bucket.
        (((display - self.origin) * self.scale).floor() as usize).min(self.last)
    }
}

/// The samples a bucket keeps: indices of its first, minimum, maximum and
/// last drawable samples.
struct Extremes {
    first: usize,
    min: usize,
    max: usize,
    last: usize,
}

impl Extremes {
    fn new(i: usize) -> Self {
        Self {
            first: i,
            min: i,
            max: i,
            last: i,
        }
    }

    fn include(&mut self, i: usize, y: &[f64]) {
        if y[i] < y[self.min] {
            self.min = i;
        }
        if y[i] > y[self.max] {
            self.max = i;
        }
        self.last = i;
    }

    /// Appends the kept indices in x order without duplicates.
    fn push_into(self, kept: &mut Vec<usize>) {
        let mut indices = [self.first, self.min, self.max, self.last];
        indices.sort_unstable();
        for i in indices {
            if kept.last() != Some(&i) {
                kept.push(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};
    use std::collections::BTreeMap;

    fn vertex(x: f64, y: f64) -> Vertex {
        Vertex { x, y }
    }

    fn points(vertices: &[Vertex]) -> Vec<(f64, f64)> {
        vertices.iter().map(|v| (v.x, v.y)).collect()
    }

    #[test]
    fn short_traces_pass_through_unchanged() {
        let x = [0.0, 1.0, 2.0];
        let y = [5.0, -1.0, 3.0];
        let vertices = decimate(&x, &y, 0.0, 2.0, 100, false);
        assert_eq!(points(&vertices), [(0.0, 5.0), (1.0, -1.0), (2.0, 3.0)]);
    }

    #[test]
    fn each_bucket_keeps_first_min_max_last() {
        // One bucket holding a burst: first 0, min −9 at 3, max 7 at 6, last 1.
        let x: Vec<f64> = (0..10).map(f64::from).collect();
        let y = [0.0, 1.0, -2.0, -9.0, 4.0, 5.0, 7.0, 2.0, 3.0, 1.0];
        let vertices = decimate(&x, &y, 0.0, 9.0, 1, false);
        assert_eq!(
            points(&vertices),
            [(0.0, 0.0), (3.0, -9.0), (6.0, 7.0), (9.0, 1.0)]
        );
    }

    #[test]
    fn adjacent_opposite_peaks_survive() {
        // Ported from the prototype: a −10/+10 spike pair on neighbouring
        // samples inside one of 20 columns keeps both extremes, adjacent.
        let x: Vec<f64> = (0..1000).map(f64::from).collect();
        let mut y = vec![0.0; 1000];
        y[321] = -10.0;
        y[322] = 10.0;
        let vertices = decimate(&x, &y, 0.0, 999.0, 20, false);
        assert!(vertices.len() <= 4 * 20 + 2);
        assert_eq!(vertices.first().map(|v| v.x), Some(0.0));
        assert_eq!(vertices.last().map(|v| v.x), Some(999.0));
        assert!(
            vertices
                .windows(2)
                .any(|w| w[0] == vertex(321.0, -10.0) && w[1] == vertex(322.0, 10.0))
        );
        assert!(vertices.windows(2).all(|w| w[0].x < w[1].x));
    }

    #[test]
    fn neighbours_outside_the_range_are_included() {
        let x = [0.0, 1.0, 2.0, 3.0, 4.0];
        let y = [0.0, 1.0, 2.0, 3.0, 4.0];
        let vertices = decimate(&x, &y, 1.5, 2.5, 4, false);
        assert_eq!(points(&vertices), [(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        // A range between two samples still draws the segment across it.
        let vertices = decimate(&x, &y, 1.2, 1.8, 4, false);
        assert_eq!(points(&vertices), [(1.0, 1.0), (2.0, 2.0)]);
    }

    #[test]
    fn non_finite_samples_break_the_line() {
        let x = [0.0, 1.0, 2.0, 3.0, 4.0];
        let y = [0.0, 1.0, f64::NAN, 3.0, 4.0];
        let vertices = decimate(&x, &y, 0.0, 4.0, 100, false);
        assert_eq!(vertices.len(), 5);
        assert!(vertices[2].is_break());
        assert_eq!(vertices[1], vertex(1.0, 1.0));
        assert_eq!(vertices[3], vertex(3.0, 3.0));
        // Consecutive gaps produce one break; leading and trailing gaps none.
        let y = [f64::INFINITY, 1.0, f64::NAN, f64::NAN, 4.0];
        let vertices = decimate(&x, &y, 0.0, 4.0, 100, false);
        assert_eq!(vertices.len(), 3);
        assert!(vertices[1].is_break());
    }

    #[test]
    fn a_gap_inside_one_bucket_is_kept() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [0.0, 5.0, f64::NAN, -5.0];
        let vertices = decimate(&x, &y, 0.0, 3.0, 1, false);
        assert_eq!(vertices.len(), 4);
        assert!(vertices[2].is_break());
    }

    #[test]
    fn logarithmic_buckets_are_decades_wide_in_display_space() {
        // Three decades over three columns: each decade is one bucket.
        let x = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
        let y = [1.0, 9.0, 1.0, 1.0, 9.0, 1.0, 1.0, 9.0, 1.0, 1.0];
        let buckets = Buckets::new(1.0, 1000.0, 3, true).unwrap();
        let indices: Vec<usize> = x.iter().map(|&v| buckets.index(v)).collect();
        assert_eq!(indices, [0, 0, 0, 1, 1, 1, 2, 2, 2, 2]);
        // The last decade holds 100, 200, 500 and 1000 (on the range end);
        // 500 is neither first, an extreme nor last.
        let vertices = decimate(&x, &y, 1.0, 1000.0, 3, true);
        let kept: Vec<f64> = vertices.iter().map(|v| v.x).collect();
        assert_eq!(
            kept,
            [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 1000.0]
        );
    }

    #[test]
    fn logarithmic_display_skips_non_positive_x() {
        let x = [-1.0, 0.0, 1.0, 10.0];
        let y = [1.0, 2.0, 3.0, 4.0];
        let vertices = decimate(&x, &y, 1.0, 10.0, 10, true);
        assert_eq!(points(&vertices), [(1.0, 3.0), (10.0, 4.0)]);
    }

    #[test]
    fn invalid_ranges_draw_nothing() {
        let x = [1.0, 2.0];
        let y = [1.0, 2.0];
        assert!(decimate(&x, &y, 0.0, 1.0, 0, false).is_empty());
        assert!(decimate(&x, &y, 2.0, 1.0, 10, false).is_empty());
        assert!(decimate(&x, &y, 1.0, 1.0, 10, false).is_empty());
        assert!(decimate(&x, &y, f64::NAN, 1.0, 10, false).is_empty());
        assert!(decimate(&x, &y, 0.0, f64::INFINITY, 10, false).is_empty());
        assert!(decimate(&x, &y, 0.0, 10.0, 10, true).is_empty());
        assert!(decimate(&[], &[], 0.0, 1.0, 10, false).is_empty());
    }

    #[test]
    fn huge_column_counts_do_not_allocate_per_column() {
        let x = [0.0, 1.0, 2.0];
        let y = [0.0, 1.0, 0.0];
        assert_eq!(decimate(&x, &y, 0.0, 2.0, usize::MAX, false).len(), 3);
    }

    #[test]
    fn unsorted_input_does_not_panic() {
        let x = [3.0, 1.0, f64::NAN, 0.0, 2.0, 5.0];
        let y = [1.0, 2.0, 3.0, f64::NAN, 5.0, 6.0];
        let _ = decimate(&x, &y, 0.0, 4.0, 2, false);
        let _ = decimate(&x, &y, 0.5, 4.0, 3, true);
    }

    /// One million samples decimate fast enough to redraw on every frame.
    #[test]
    #[cfg_attr(
        debug_assertions,
        ignore = "timing is only meaningful in optimized builds"
    )]
    fn a_million_samples_decimate_in_well_under_100_ms() {
        let n = 1_000_000;
        let x: Vec<f64> = (0..n).map(|i| 1.0 + i as f64 * 1e-3).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|t| (t * 37.0).sin() + (t * 1013.0).sin())
            .collect();
        for log_x in [false, true] {
            // The best of three runs filters out scheduling noise on a busy
            // machine; half the budget leaves a wide margin.
            let fastest = (0..3)
                .map(|_| {
                    let started = std::time::Instant::now();
                    let vertices = decimate(&x, &y, x[0], x[n - 1], 2000, log_x);
                    assert!(vertices.len() <= 4 * 2000 + 2);
                    started.elapsed()
                })
                .min()
                .unwrap_or_default();
            assert!(fastest.as_millis() < 50, "log_x = {log_x}: {fastest:?}");
        }
    }

    fn config() -> Config {
        Config {
            cases: 200,
            rng_seed: RngSeed::Fixed(0x10D),
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// A sample value: mostly finite, occasionally NaN or infinite.
    fn sample() -> impl Strategy<Value = f64> {
        prop_oneof![
            20 => -100.0f64..100.0,
            1 => Just(f64::NAN),
            1 => Just(f64::INFINITY),
        ]
    }

    /// Strictly increasing positive x samples with matching y samples.
    fn trace() -> impl Strategy<Value = (Vec<f64>, Vec<f64>)> {
        prop::collection::vec((0.001f64..2.0, sample()), 0..300).prop_map(|steps| {
            let mut position = 0.1;
            steps
                .into_iter()
                .map(|(step, y)| {
                    position += step;
                    (position, y)
                })
                .unzip()
        })
    }

    /// Display range covering part of (or beyond) the trace.
    fn range() -> impl Strategy<Value = (f64, f64)> {
        (0.0f64..200.0, 0.01f64..200.0).prop_map(|(low, width)| (0.05 + low, 0.05 + low + width))
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn output_is_bounded_ordered_and_faithful(
            (x, y) in trace(),
            (x_min, x_max) in range(),
            columns in 1usize..64,
            log_x in any::<bool>(),
        ) {
            let vertices = decimate(&x, &y, x_min, x_max, columns, log_x);
            let drawn: Vec<Vertex> = vertices.iter().copied().filter(|v| !v.is_break()).collect();

            // Size: four samples per bucket plus the two neighbours, and at
            // most one break between consecutive drawn vertices.
            prop_assert!(drawn.len() <= 4 * columns + 2);
            prop_assert!(vertices.len() <= (2 * drawn.len()).saturating_sub(1));
            prop_assert!(vertices.len() <= 8 * columns + 3);
            if y.iter().all(|v| v.is_finite()) {
                prop_assert_eq!(vertices.len(), drawn.len());
            }

            // Breaks never start, end or repeat.
            prop_assert!(!vertices.first().is_some_and(Vertex::is_break));
            prop_assert!(!vertices.last().is_some_and(Vertex::is_break));
            prop_assert!(vertices.windows(2).all(|w| !(w[0].is_break() && w[1].is_break())));

            // Every drawn vertex is an input sample, in strictly increasing x.
            prop_assert!(drawn.windows(2).all(|w| w[0].x < w[1].x));
            let index_of = |v: &Vertex| x.iter().position(|&xi| xi == v.x);
            for v in &drawn {
                let i = index_of(v);
                prop_assert!(i.is_some_and(|i| y[i] == v.y), "{:?} is not a sample", v);
            }

            // A break separates two drawn vertices exactly when an
            // undrawable sample lies between them.
            let mut previous: Option<usize> = None;
            let mut pending_break = false;
            for v in &vertices {
                if v.is_break() {
                    pending_break = true;
                    continue;
                }
                let i = index_of(v).unwrap_or_default();
                if let Some(p) = previous {
                    let gap = (p + 1..i).any(|k| !is_drawable(x[k], y[k], log_x));
                    prop_assert_eq!(gap, pending_break, "between samples {} and {}", p, i);
                }
                previous = Some(i);
                pending_break = false;
            }

            // Every bucket's extremes are drawn.
            if let Some(buckets) = Buckets::new(x_min, x_max, columns, log_x) {
                let mut extremes: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
                for (&xi, &yi) in x.iter().zip(&y) {
                    if xi < x_min || xi > x_max || !is_drawable(xi, yi, log_x) {
                        continue;
                    }
                    let entry = extremes.entry(buckets.index(xi)).or_insert((yi, yi));
                    *entry = (entry.0.min(yi), entry.1.max(yi));
                }
                for (bucket, (low, high)) in extremes {
                    let in_bucket = |v: &&Vertex| {
                        x_min <= v.x && v.x <= x_max && buckets.index(v.x) == bucket
                    };
                    prop_assert!(drawn.iter().filter(in_bucket).any(|v| v.y == low));
                    prop_assert!(drawn.iter().filter(in_bucket).any(|v| v.y == high));
                }
            }
        }
    }
}
