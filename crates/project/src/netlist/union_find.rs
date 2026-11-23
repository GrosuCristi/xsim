//! Disjoint-set forest used to merge connection points into nets.

/// Union-find over `0..len` with path halving and union by size.
#[derive(Debug, Clone)]
pub(super) struct DisjointSets {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl DisjointSets {
    pub(super) fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            size: vec![1; len],
        }
    }

    /// Representative of the set containing `item`.
    pub(super) fn find(&mut self, mut item: usize) -> usize {
        while self.parent[item] != item {
            self.parent[item] = self.parent[self.parent[item]];
            item = self.parent[item];
        }
        item
    }

    /// Merges the sets containing `a` and `b`.
    pub(super) fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return;
        }
        let (large, small) = if self.size[a] >= self.size[b] {
            (a, b)
        } else {
            (b, a)
        };
        self.parent[small] = large;
        self.size[large] += self.size[small];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unions_are_transitive_and_disjoint_sets_stay_apart() {
        let mut sets = DisjointSets::new(6);
        sets.union(0, 1);
        sets.union(2, 3);
        sets.union(1, 3);
        assert_eq!(sets.find(0), sets.find(2));
        assert_ne!(sets.find(0), sets.find(4));
        assert_ne!(sets.find(4), sets.find(5));
        sets.union(3, 3);
        assert_eq!(sets.find(3), sets.find(1));
    }
}
