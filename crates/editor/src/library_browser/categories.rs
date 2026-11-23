//! The category tree of the library window. GPUI-free.

use super::catalog::CatalogEntry;
use std::collections::HashSet;

/// Category labels that only repeat the family or polarity a model is
/// already filed under, so a library path such as
/// `["Transistors", "BJT", "NPN", "Small signal"]` does not nest twice.
const REDUNDANT: &[&str] = &[
    "diode",
    "diodes",
    "transistor",
    "transistors",
    "bjt",
    "bjts",
    "bipolar",
    "bipolar transistor",
    "bipolar transistors",
    "npn",
    "pnp",
    "jfet",
    "jfets",
    "n-channel",
    "p-channel",
    "n-channel jfet",
    "p-channel jfet",
];

/// The tree path of a model: `family` (e.g. `["Bipolar transistors",
/// "NPN"]`) followed by the library `category` without leading segments that
/// repeat the family.
pub(crate) fn model_path(family: &[&str], category: &[&str]) -> Vec<String> {
    let redundant = |segment: &&str| REDUNDANT.contains(&segment.trim().to_lowercase().as_str());
    family
        .iter()
        .copied()
        .chain(category.iter().copied().skip_while(redundant))
        .filter(|segment| !segment.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// A category and its sub-categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CategoryNode {
    /// Full path from the top level, e.g. `["Diodes", "Switching"]`.
    pub path: Vec<String>,
    pub children: Vec<CategoryNode>,
}

impl CategoryNode {
    /// The last path segment.
    pub(crate) fn label(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }
}

/// The categories of `entries`. Top-level categories keep catalog order
/// (basic parts, sources, diodes, transistors, JFETs); deeper levels are
/// sorted by name.
pub(crate) fn build_tree(entries: &[CatalogEntry]) -> Vec<CategoryNode> {
    let mut roots: Vec<CategoryNode> = Vec::new();
    for entry in entries {
        insert(&mut roots, &entry.path, 1);
    }
    for root in &mut roots {
        sort_children(root);
    }
    roots
}

fn insert(level: &mut Vec<CategoryNode>, path: &[String], depth: usize) {
    if depth > path.len() {
        return;
    }
    let prefix = &path[..depth];
    let ix = match level.iter().position(|node| node.path == prefix) {
        Some(ix) => ix,
        None => {
            level.push(CategoryNode {
                path: prefix.to_vec(),
                children: Vec::new(),
            });
            level.len() - 1
        }
    };
    insert(&mut level[ix].children, path, depth + 1);
}

fn sort_children(node: &mut CategoryNode) {
    node.children
        .sort_by_key(|child| child.label().to_lowercase());
    for child in &mut node.children {
        sort_children(child);
    }
}

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeRow {
    pub path: Vec<String>,
    /// Nesting level, 0 for top-level categories.
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
}

impl TreeRow {
    pub(crate) fn label(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }
}

/// The rows of `tree` whose ancestors are all in `expanded`, depth first.
pub(crate) fn visible_rows(tree: &[CategoryNode], expanded: &HashSet<Vec<String>>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    push_rows(tree, expanded, 0, &mut rows);
    rows
}

fn push_rows(
    level: &[CategoryNode],
    expanded: &HashSet<Vec<String>>,
    depth: usize,
    rows: &mut Vec<TreeRow>,
) {
    for node in level {
        let is_expanded = expanded.contains(&node.path);
        rows.push(TreeRow {
            path: node.path.clone(),
            depth,
            has_children: !node.children.is_empty(),
            expanded: is_expanded,
        });
        if is_expanded {
            push_rows(&node.children, expanded, depth + 1, rows);
        }
    }
}

/// Whether `path` lies in `category` (an empty category contains everything).
pub(crate) fn contains(category: &[String], path: &[String]) -> bool {
    path.starts_with(category)
}

/// Every proper prefix of `path` (its ancestors), shortest first.
pub(crate) fn ancestors(path: &[String]) -> impl Iterator<Item = Vec<String>> + '_ {
    (1..path.len()).map(|n| path[..n].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_browser::catalog::fixtures;

    fn strings(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn model_paths_drop_segments_that_repeat_the_family() {
        let family = ["Bipolar transistors", "NPN"];
        assert_eq!(
            model_path(&family, &["Transistors", "BJT", "npn", "Small signal"]),
            strings(&["Bipolar transistors", "NPN", "Small signal"])
        );
        assert_eq!(
            model_path(&["Diodes"], &["Diodes", "Zener", "Low voltage"]),
            strings(&["Diodes", "Zener", "Low voltage"])
        );
        assert_eq!(model_path(&["Diodes"], &[]), strings(&["Diodes"]));
        assert_eq!(
            model_path(&["Diodes"], &["Switching", "Diodes"]),
            strings(&["Diodes", "Switching", "Diodes"]),
            "only leading segments are redundant"
        );
    }

    #[test]
    fn the_tree_nests_families_polarities_and_library_categories() {
        let catalog = fixtures::catalog();
        let tree = build_tree(catalog.entries());
        let top: Vec<_> = tree.iter().map(CategoryNode::label).collect();
        assert_eq!(
            top,
            [
                "Basic parts",
                "Sources",
                "Switches",
                "Diodes",
                "Bipolar transistors",
                "JFETs"
            ]
        );
        let bjts = &tree[4];
        let polarities: Vec<_> = bjts.children.iter().map(CategoryNode::label).collect();
        assert_eq!(polarities, ["NPN", "PNP"]);
        let npn: Vec<_> = bjts.children[0]
            .children
            .iter()
            .map(CategoryNode::label)
            .collect();
        assert_eq!(npn, ["General purpose", "Small signal"]);
        assert!(tree[0].children.is_empty(), "basic parts are leaf items");
    }

    #[test]
    fn visible_rows_follow_expansion() {
        let catalog = fixtures::catalog();
        let tree = build_tree(catalog.entries());
        let collapsed = visible_rows(&tree, &HashSet::new());
        assert_eq!(collapsed.len(), 6);
        assert!(!collapsed[2].has_children, "the switches are leaf items");
        assert!(collapsed[4].has_children && !collapsed[4].expanded);
        let expanded = HashSet::from([strings(&["Bipolar transistors"])]);
        let rows = visible_rows(&tree, &expanded);
        let labels: Vec<_> = rows.iter().map(|r| (r.label(), r.depth)).collect();
        assert_eq!(
            labels,
            [
                ("Basic parts", 0),
                ("Sources", 0),
                ("Switches", 0),
                ("Diodes", 0),
                ("Bipolar transistors", 0),
                ("NPN", 1),
                ("PNP", 1),
                ("JFETs", 0)
            ]
        );
    }

    #[test]
    fn category_membership_and_ancestors() {
        let path = strings(&["Diodes", "Switching"]);
        assert!(contains(&[], &path));
        assert!(contains(&strings(&["Diodes"]), &path));
        assert!(!contains(&strings(&["Sources"]), &path));
        assert_eq!(ancestors(&path).collect::<Vec<_>>(), [strings(&["Diodes"])]);
    }
}
