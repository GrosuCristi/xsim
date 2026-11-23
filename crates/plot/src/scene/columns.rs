//! Decimated columns that turn back are drawn upright (D20).
//!
//! Decimation (Q140) keeps up to four samples per pixel column: the first,
//! the minimum, the maximum and the last. A spike far narrower than a column
//! (40 ns in a 5 ms view) becomes a line that climbs to the maximum and
//! comes back down inside one column, the two strokes a fraction of a pixel
//! apart. Where they part the line is wider than at the tip, so the spike
//! reads as a hair near its top. Drawing every vertex of such a column on
//! one x makes the two strokes coincide: the spike is a single upright line
//! of the trace's width from its minimum to its maximum.
//!
//! Columns that only rise or only fall keep their slant, so steep smooth
//! edges do not turn into staircases.

use crate::model::axis::AxisRange;

/// The decimation column of a sample, worked out exactly as
/// `xsim_waveform::lod::decimate` buckets it (in display space, so decades
/// on a logarithmic axis).
#[derive(Debug, Clone, Copy)]
pub struct Columns {
    range: AxisRange,
    origin: f64,
    /// Columns per display unit.
    scale: f64,
    last: usize,
}

impl Columns {
    /// `columns` equal columns across `range`; `None` when the range cannot
    /// be split (decimation then draws nothing either).
    pub fn new(range: AxisRange, columns: usize) -> Option<Self> {
        let origin = range.display_min();
        let scale = columns as f64 / (range.display_max() - origin);
        let last = columns.checked_sub(1)?;
        scale.is_finite().then_some(Self {
            range,
            origin,
            scale,
            last,
        })
    }

    /// The column of the sample at `x`, or `None` outside the range (the
    /// neighbours decimation adds so lines reach the plot edges).
    pub fn index(&self, x: f64) -> Option<usize> {
        if !self.range.contains(x) || (self.range.log && x <= 0.0) {
            return None;
        }
        let display = if self.range.log { x.log10() } else { x };
        // `as` saturates, like the decimation's own bucket index.
        Some((((display - self.origin) * self.scale).floor() as usize).min(self.last))
    }
}

/// Moves every vertex of a column whose line turns back (up then down, or
/// down then up) to the middle of that column's vertices. `points` are pixel
/// positions; `columns[i]` is the column of vertex `i`, `None` for line
/// breaks and for vertices outside the range, which belong to no column.
pub fn upright_reversals(points: &mut [[f64; 2]], columns: &[Option<usize>]) {
    let mut start = 0;
    while start < points.len() {
        let column = columns.get(start).copied().flatten();
        let end = match column {
            Some(_) => (start + 1..points.len())
                .find(|&i| columns.get(i).copied().flatten() != column)
                .unwrap_or(points.len()),
            None => start + 1,
        };
        let group = &mut points[start..end];
        if column.is_some() && turns_back(group) {
            let (low, high) = group
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), p| {
                    (low.min(p[0]), high.max(p[0]))
                });
            let middle = (low + high) / 2.0;
            for point in group.iter_mut() {
                point[0] = middle;
            }
        }
        start = end;
    }
}

/// Whether the line through `points` changes vertical direction.
fn turns_back(points: &[[f64; 2]]) -> bool {
    let mut direction = 0.0;
    for pair in points.windows(2) {
        let dy = pair[1][1] - pair[0][1];
        if dy == 0.0 {
            continue;
        }
        if direction * dy < 0.0 {
            return true;
        }
        direction = dy;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_waveform::lod::decimate;

    fn range(min: f64, max: f64, log: bool) -> AxisRange {
        AxisRange::new(min, max, log).expect("a valid range")
    }

    #[test]
    fn columns_split_the_range_evenly_in_display_space() {
        let linear = Columns::new(range(0.0, 10.0, false), 10).expect("columns");
        assert_eq!(linear.index(0.0), Some(0));
        assert_eq!(linear.index(0.99), Some(0));
        assert_eq!(linear.index(1.0), Some(1));
        assert_eq!(linear.index(10.0), Some(9), "the end is in the last column");
        assert_eq!(linear.index(-0.1), None);
        assert_eq!(linear.index(10.1), None);
        let log = Columns::new(range(1.0, 1000.0, true), 3).expect("columns");
        let decades: Vec<Option<usize>> = [1.0, 9.0, 10.0, 99.0, 100.0, 1000.0]
            .into_iter()
            .map(|x| log.index(x))
            .collect();
        assert_eq!(
            decades,
            [Some(0), Some(0), Some(1), Some(1), Some(2), Some(2)]
        );
        assert!(Columns::new(range(0.0, 1.0, false), 0).is_none());
    }

    /// Every vertex `decimate` keeps inside the range lies in the column
    /// [`Columns`] gives it: at most four vertices per column, holding that
    /// column's extremes.
    #[test]
    fn columns_match_the_decimation_buckets() {
        let x: Vec<f64> = (0..5000).map(|i| f64::from(i) * 1e-3).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|t| (t * 37.0).sin() * (t * 3.0).cos())
            .collect();
        for (min, max, count) in [(0.0, 4.999, 97), (0.3, 2.71, 211), (1.0, 1.5, 33)] {
            let columns = Columns::new(range(min, max, false), count).expect("columns");
            let vertices = decimate(&x, &y, min, max, count, false);
            let mut per_column = vec![Vec::new(); count];
            for v in &vertices {
                if let Some(c) = columns.index(v.x) {
                    per_column[c].push(v.y);
                }
            }
            for (i, (&xi, &yi)) in x.iter().zip(&y).enumerate() {
                let Some(c) = columns.index(xi) else {
                    continue;
                };
                let kept = &per_column[c];
                assert!(kept.len() <= 4, "column {c}: {kept:?}");
                let low = kept.iter().copied().fold(f64::INFINITY, f64::min);
                let high = kept.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                assert!(low <= yi && yi <= high, "sample {i} outside column {c}");
            }
        }
    }

    /// Review: the same on a logarithmic x axis (Bode plots), whose columns
    /// split decades evenly.
    #[test]
    fn log_columns_match_the_decimation_buckets() {
        let x: Vec<f64> = (0..5000)
            .map(|i| 10f64.powf(f64::from(i) * 6.0 / 4999.0))
            .collect();
        let y: Vec<f64> = x.iter().map(|f| (f.log10() * 41.0).sin()).collect();
        for (min, max, count) in [(1.0, 1e6, 97), (3.0, 2.5e4, 211), (10.0, 100.0, 33)] {
            let columns = Columns::new(range(min, max, true), count).expect("columns");
            let vertices = decimate(&x, &y, min, max, count, true);
            let mut per_column = vec![Vec::new(); count];
            for v in &vertices {
                if let Some(c) = columns.index(v.x) {
                    per_column[c].push(v.y);
                }
            }
            for (i, (&xi, &yi)) in x.iter().zip(&y).enumerate() {
                let Some(c) = columns.index(xi) else {
                    continue;
                };
                let kept = &per_column[c];
                assert!(kept.len() <= 4, "column {c}: {kept:?}");
                let low = kept.iter().copied().fold(f64::INFINITY, f64::min);
                let high = kept.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                assert!(low <= yi && yi <= high, "sample {i} outside column {c}");
            }
        }
        assert_eq!(
            Columns::new(range(1.0, 1000.0, true), 3)
                .expect("columns")
                .index(-1.0),
            None
        );
    }

    #[test]
    fn a_column_that_turns_back_is_drawn_on_one_x() {
        // Flat, then a spike inside column 3 (up to y 10 and down to 80),
        // then flat again.
        let mut points = vec![
            [1.2, 100.0],
            [2.5, 100.0],
            [3.1, 100.0],
            [3.3, 10.0],
            [3.9, 80.0],
            [4.4, 80.0],
        ];
        let columns = [Some(1), Some(2), Some(3), Some(3), Some(3), Some(4)];
        upright_reversals(&mut points, &columns);
        assert_eq!(points[2..5], [[3.5, 100.0], [3.5, 10.0], [3.5, 80.0]]);
        assert_eq!(points[..2], [[1.2, 100.0], [2.5, 100.0]]);
        assert_eq!(points[5], [4.4, 80.0]);
    }

    #[test]
    fn columns_that_only_rise_or_fall_keep_their_slant() {
        let original = vec![
            [1.1, 100.0],
            [1.5, 60.0],
            [1.9, 60.0],
            [2.2, 50.0],
            [2.8, 10.0],
        ];
        let mut points = original.clone();
        upright_reversals(&mut points, &[Some(1), Some(1), Some(1), Some(2), Some(2)]);
        assert_eq!(points, original);
    }

    #[test]
    fn breaks_and_neighbours_outside_the_range_split_columns() {
        // The same column on both sides of a break: each side is its own
        // group, and neither turns back on its own.
        let nan = f64::NAN;
        let mut points = vec![
            [0.5, 50.0],
            [1.1, 10.0],
            [nan, nan],
            [1.6, 90.0],
            [1.8, 20.0],
        ];
        let columns = [None, Some(1), None, Some(1), Some(1)];
        upright_reversals(&mut points, &columns);
        assert_eq!(points[0], [0.5, 50.0]);
        assert_eq!(points[1], [1.1, 10.0]);
        assert_eq!(points[3..], [[1.6, 90.0], [1.8, 20.0]]);
    }
}
