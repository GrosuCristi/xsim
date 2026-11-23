//! Undo/redo of whole-schematic snapshots (Q57).
//!
//! Every user action records the schematic as it was before the action, so
//! one undo step reverts one action (a whole drag included). History lives in
//! the editor instance only and is never saved.

use std::sync::Arc;
use xsim_project::Schematic;

/// Maximum number of undo steps kept.
const LIMIT: usize = 500;

/// Undo and redo stacks.
#[derive(Debug, Default, Clone)]
pub struct History {
    undo: Vec<Arc<Schematic>>,
    redo: Vec<Arc<Schematic>>,
}

impl History {
    /// Records the state before a new action and forgets the redo branch.
    pub fn record(&mut self, before: Arc<Schematic>) {
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Steps back: returns the previous state and remembers `current` for redo.
    pub fn undo(&mut self, current: Arc<Schematic>) -> Option<Arc<Schematic>> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }

    /// Steps forward again after an undo.
    pub fn redo(&mut self, current: Arc<Schematic>) -> Option<Arc<Schematic>> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::History;
    use std::sync::Arc;
    use xsim_project::Schematic;

    fn state(next_id: u64) -> Arc<Schematic> {
        Arc::new(Schematic {
            next_id,
            ..Schematic::default()
        })
    }

    #[test]
    fn undo_and_redo_walk_the_recorded_states() {
        let mut h = History::default();
        h.record(state(1));
        h.record(state(2));
        assert_eq!(h.undo(state(3)).map(|s| s.next_id), Some(2));
        assert_eq!(h.undo(state(2)).map(|s| s.next_id), Some(1));
        assert!(!h.can_undo());
        assert_eq!(h.redo(state(1)).map(|s| s.next_id), Some(2));
        assert_eq!(h.redo(state(2)).map(|s| s.next_id), Some(3));
        assert!(!h.can_redo());
    }

    #[test]
    fn a_new_action_discards_the_redo_branch() {
        let mut h = History::default();
        h.record(state(1));
        h.undo(state(2));
        assert!(h.can_redo());
        h.record(state(1));
        assert!(!h.can_redo());
    }
}
