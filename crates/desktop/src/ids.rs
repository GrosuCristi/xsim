//! Stable identifiers for circuit tabs.
//!
//! Tabs can be reordered or deleted while a simulation runs, so background
//! work refers to its tab by a [`TabId`] instead of an index.

/// Identifies one circuit tab for the lifetime of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

/// Hands out fresh [`TabId`]s.
#[derive(Debug, Default)]
pub struct TabIdAllocator {
    next: u64,
}

impl TabIdAllocator {
    /// Returns an id that was never returned before.
    pub fn allocate(&mut self) -> TabId {
        self.next += 1;
        TabId(self.next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids = TabIdAllocator::default();
        let a = ids.allocate();
        let b = ids.allocate();
        assert_ne!(a, b);
    }
}
