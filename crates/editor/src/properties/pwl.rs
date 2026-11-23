//! The editable point list of a PWL waveform. Rows have stable ids so text
//! fields keep their identity when rows above them are removed. GPUI-free.

use super::spec::FieldId;

/// Row ids of a PWL table, in display order, plus the repeat flag.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PwlRows {
    ids: Vec<u64>,
    next_id: u64,
    /// Repeat the point sequence forever.
    pub repeat: bool,
}

impl PwlRows {
    /// Row ids in display order.
    pub(crate) fn ids(&self) -> &[u64] {
        &self.ids
    }

    /// Appends a row and returns its id.
    pub(crate) fn push(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.ids.push(id);
        id
    }

    /// Removes row `id`. The last remaining row cannot be removed, because a
    /// PWL waveform needs at least one point. Returns whether it was removed.
    pub(crate) fn remove(&mut self, id: u64) -> bool {
        if self.ids.len() <= 1 {
            return false;
        }
        let before = self.ids.len();
        self.ids.retain(|&row| row != id);
        self.ids.len() != before
    }

    /// Whether a row can be removed.
    pub(crate) fn can_remove(&self) -> bool {
        self.ids.len() > 1
    }
}

/// Checks that point times strictly increase. `points` pairs each row id
/// with its parsed `(time, value)`. Returns the error for each offending
/// time field.
pub(crate) fn order_errors(points: &[(u64, (f64, f64))]) -> Vec<(FieldId, String)> {
    points
        .windows(2)
        .filter(|pair| pair[1].1.0 <= pair[0].1.0)
        .map(|pair| {
            (
                FieldId::PwlTime(pair[1].0),
                "Must be later than the previous point".to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_keep_stable_ids_and_never_become_empty() {
        let mut rows = PwlRows::default();
        let a = rows.push();
        let b = rows.push();
        let c = rows.push();
        assert_eq!(rows.ids(), [a, b, c]);
        assert!(rows.remove(b));
        assert_eq!(rows.ids(), [a, c]);
        assert!(!rows.remove(b), "already removed");
        assert!(rows.remove(a));
        assert!(!rows.can_remove());
        assert!(!rows.remove(c), "the last point stays");
        let d = rows.push();
        assert!(d > c, "ids are never reused");
    }

    #[test]
    fn decreasing_or_repeated_times_are_reported_on_the_later_row() {
        let points = [
            (0, (0.0, 0.0)),
            (1, (1.0, 1.0)),
            (2, (1.0, 2.0)),
            (3, (0.5, 0.0)),
        ];
        let errors = order_errors(&points);
        let fields: Vec<_> = errors.iter().map(|e| e.0).collect();
        assert_eq!(fields, [FieldId::PwlTime(2), FieldId::PwlTime(3)]);
        assert!(order_errors(&points[..2]).is_empty());
    }
}
