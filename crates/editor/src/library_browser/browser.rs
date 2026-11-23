//! State of the library window: query, selected category, matching entries
//! and the selected entry, with keyboard navigation. GPUI-free.
//!
//! Search scope (D12): while the query is not blank the list shows the
//! matches of every category the window offers (every category its
//! [`LibraryFilter`](super::LibraryFilter) admits), best first, and the tree
//! counts the matches per category. Selecting a category while searching
//! narrows the list to that category's matches until the query is edited:
//! every edit searches every category again. Clearing the query shows the
//! full list of the category selected last in the tree, whether it was
//! selected before or during the search.

use super::LibraryItem;
use super::catalog::{Catalog, CatalogEntry};
use super::categories::{self, CategoryNode, TreeRow};
use super::search;
use std::collections::{HashMap, HashSet};

/// Separator between the segments of a category path shown as text.
pub(crate) const PATH_SEPARATOR: &str = " \u{203a} ";

/// What the library window shows.
#[derive(Debug, Clone)]
pub(crate) struct Browser {
    catalog: Catalog,
    tree: Vec<CategoryNode>,
    expanded: HashSet<Vec<String>>,
    /// Category selected in the tree; empty for "all".
    category: Vec<String>,
    /// Whether `category` was selected during the current search and so
    /// narrows it; otherwise a search covers every category.
    narrowed: bool,
    query: String,
    /// Catalog indices of the matching entries, best first.
    results: Vec<usize>,
    /// Matches per category for the current query.
    counts: HashMap<Vec<String>, usize>,
    /// Catalog index of the selected entry.
    selected: Option<usize>,
}

impl Browser {
    /// Shows every entry of `catalog` with the top-level categories expanded
    /// and `initial` (or else the first entry) selected.
    pub(crate) fn new(catalog: Catalog, initial: Option<&LibraryItem>) -> Self {
        let tree = categories::build_tree(catalog.entries());
        let expanded = tree.iter().map(|node| node.path.clone()).collect();
        let selected = initial.and_then(|item| catalog.position(item));
        let mut browser = Self {
            catalog,
            tree,
            expanded,
            category: Vec::new(),
            narrowed: false,
            query: String::new(),
            results: Vec::new(),
            counts: HashMap::new(),
            selected,
        };
        if let Some(entry) = browser.selected_entry() {
            let ancestors: Vec<_> = categories::ancestors(&entry.path).collect();
            browser.expanded.extend(ancestors);
        }
        browser.refresh();
        browser
    }

    fn refresh(&mut self) {
        let entries = self.catalog.entries();
        self.results = search::search(entries, &self.query, self.scope());
        self.counts = search::counts(entries, &self.query);
        if self.selected.is_none_or(|ix| !self.results.contains(&ix)) {
            self.selected = self.results.first().copied();
        }
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    /// Whether the query searches (it is not blank).
    pub(crate) fn is_searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Sets the query; any edit searches every category again.
    pub(crate) fn set_query(&mut self, query: &str) {
        if self.query != query {
            self.query = query.to_string();
            self.narrowed = false;
            self.refresh();
        }
    }

    /// The category selected in the tree; empty for "all". It is the one
    /// listed while the query is blank.
    #[cfg(test)]
    fn category(&self) -> &[String] {
        &self.category
    }

    /// The category the list is drawn from: empty (every category) while an
    /// un-narrowed search runs, else the selected category.
    pub(crate) fn scope(&self) -> &[String] {
        if self.is_searching() && !self.narrowed {
            &[]
        } else {
            &self.category
        }
    }

    /// Selects category `path`; during a search this narrows the matches to
    /// it.
    pub(crate) fn select_category(&mut self, path: Vec<String>) {
        self.category = path;
        self.narrowed = self.is_searching();
        self.refresh();
    }

    /// Expands or collapses category `path`.
    pub(crate) fn toggle_expanded(&mut self, path: &[String]) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_vec());
        }
    }

    /// The visible rows of the category tree.
    pub(crate) fn tree_rows(&self) -> Vec<TreeRow> {
        categories::visible_rows(&self.tree, &self.expanded)
    }

    /// Position of the highlighted category ([`Self::scope`]) among the rows
    /// the keyboard walks: 0 for "all", then the visible tree rows from 1.
    /// `None` when it sits under a collapsed category.
    pub(crate) fn highlighted_row(&self) -> Option<usize> {
        let scope = self.scope();
        if scope.is_empty() {
            return Some(0);
        }
        let rows = self.tree_rows();
        rows.iter()
            .position(|row| row.path == scope)
            .map(|ix| ix + 1)
    }

    /// The tree row of the highlighted category; `None` for "all".
    fn highlighted_tree_row(&self) -> Option<TreeRow> {
        let scope = self.scope();
        self.tree_rows().into_iter().find(|row| row.path == scope)
    }

    /// Shows category `path` unless it is already the one shown.
    fn highlight(&mut self, path: Vec<String>) {
        if path != self.scope() {
            self.select_category(path);
        }
    }

    /// Up and Down in the tree: shows the category `delta` rows away from
    /// the highlighted one ("all" first), stopping at the first and last.
    pub(crate) fn step_category(&mut self, delta: isize) {
        let rows = self.tree_rows();
        let current = self.highlighted_row().unwrap_or(0);
        let target = current.saturating_add_signed(delta).min(rows.len());
        let path = match target.checked_sub(1) {
            Some(ix) => rows[ix].path.clone(),
            None => Vec::new(),
        };
        self.highlight(path);
    }

    /// Right in the tree: opens the highlighted category, or moves to its
    /// first sub-category when it is already open ("all" moves to the first
    /// category).
    pub(crate) fn expand_highlighted(&mut self) {
        match self.highlighted_tree_row() {
            Some(row) if row.has_children && !row.expanded => self.toggle_expanded(&row.path),
            Some(row) if !row.has_children => {}
            _ => self.step_category(1),
        }
    }

    /// Left in the tree: closes the highlighted category, or moves to the
    /// category it sits in ("all" for a top-level one).
    pub(crate) fn collapse_highlighted(&mut self) {
        let Some(row) = self.highlighted_tree_row() else {
            return;
        };
        if row.has_children && row.expanded {
            self.toggle_expanded(&row.path);
        } else {
            let parent = row.path[..row.path.len().saturating_sub(1)].to_vec();
            self.highlight(parent);
        }
    }

    /// Enter in the tree: opens or closes the highlighted category. Returns
    /// `false`, changing nothing, when it has no sub-categories.
    pub(crate) fn toggle_highlighted(&mut self) -> bool {
        match self.highlighted_tree_row() {
            Some(row) if row.has_children => {
                self.toggle_expanded(&row.path);
                true
            }
            _ => false,
        }
    }

    /// Entries matching the query under `path` (empty for all), whatever
    /// the list shows.
    pub(crate) fn count(&self, path: &[String]) -> usize {
        self.counts.get(path).copied().unwrap_or(0)
    }

    /// The matching entries of [`Self::scope`] with their catalog indices,
    /// best first.
    pub(crate) fn results(&self) -> impl Iterator<Item = (usize, &CatalogEntry)> {
        let entries = self.catalog.entries();
        self.results.iter().map(move |&ix| (ix, &entries[ix]))
    }

    pub(crate) fn has_results(&self) -> bool {
        !self.results.is_empty()
    }

    /// While searching, the category of a listed `entry` below
    /// [`Self::scope`], e.g. `"NPN › Small signal"` when the search is
    /// narrowed to bipolar transistors; `None` when not searching or when
    /// the entry sits directly in the scope.
    pub(crate) fn result_category(&self, entry: &CatalogEntry) -> Option<String> {
        if !self.is_searching() {
            return None;
        }
        let below = entry.path.strip_prefix(self.scope())?;
        (!below.is_empty()).then(|| below.join(PATH_SEPARATOR))
    }

    /// Catalog index of the selected entry.
    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn selected_entry(&self) -> Option<&CatalogEntry> {
        self.selected.map(|ix| &self.catalog.entries()[ix])
    }

    /// Position of the selected entry in the result list.
    pub(crate) fn selected_position(&self) -> Option<usize> {
        let selected = self.selected?;
        self.results.iter().position(|&ix| ix == selected)
    }

    /// Selects catalog entry `ix` if it is among the results.
    pub(crate) fn select(&mut self, ix: usize) {
        if self.results.contains(&ix) {
            self.selected = Some(ix);
        }
    }

    /// Moves the selection `delta` rows through the results, stopping at the
    /// first and last row.
    pub(crate) fn move_selection(&mut self, delta: isize) {
        let Some(last) = self.results.len().checked_sub(1) else {
            return;
        };
        let target = match self.selected_position() {
            Some(position) => position.saturating_add_signed(delta).min(last),
            None => 0,
        };
        self.selected = Some(self.results[target]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_browser::LibraryFilter;
    use crate::library_browser::catalog::{
        BASIC_PARTS, BJTS, DIODES, JFETS, SOURCES, SWITCHES, fixtures,
    };
    use xsim_project::PartKind;
    use xsim_spice_engine::Polarity;

    fn browser() -> Browser {
        Browser::new(fixtures::catalog(), None)
    }

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| s.to_string()).collect()
    }

    fn selected_name(browser: &Browser) -> Option<&str> {
        browser.selected_entry().map(|e| e.name.as_str())
    }

    fn result_names(browser: &Browser) -> Vec<&str> {
        browser.results().map(|(_, e)| e.name.as_str()).collect()
    }

    fn result_categories(browser: &Browser) -> Vec<Option<String>> {
        browser
            .results()
            .map(|(_, e)| browser.result_category(e))
            .collect()
    }

    #[test]
    fn opens_on_everything_with_the_first_entry_selected() {
        let browser = browser();
        assert!(browser.category().is_empty());
        assert!(!browser.is_searching());
        assert_eq!(
            result_names(&browser).len(),
            fixtures::catalog().entries().len()
        );
        assert_eq!(selected_name(&browser), Some("Resistor"));
        let top = browser.tree_rows().iter().filter(|r| r.depth == 0).count();
        assert_eq!(top, 6);
        assert!(
            browser.tree_rows().iter().any(|r| r.label() == "NPN"),
            "top level expanded"
        );
    }

    #[test]
    fn the_initial_item_is_selected_and_revealed() {
        let item = LibraryItem::Bjt {
            polarity: Polarity::N,
            model: "NPN_SMALL".into(),
        };
        let browser = Browser::new(fixtures::catalog(), Some(&item));
        assert_eq!(selected_name(&browser), Some("NPN_SMALL"));
        assert!(
            browser
                .tree_rows()
                .iter()
                .any(|r| r.label() == "Small signal")
        );
    }

    #[test]
    fn typing_narrows_the_list_and_selects_the_best_match() {
        let mut browser = browser();
        browser.set_query("pnp_");
        assert_eq!(result_names(&browser), ["PNP_GP"]);
        assert_eq!(selected_name(&browser), Some("PNP_GP"));
        assert_eq!(browser.count(&[]), 1);
        browser.set_query("zzz");
        assert!(!browser.has_results());
        assert_eq!(browser.selected_entry(), None);
        browser.set_query("");
        assert_eq!(selected_name(&browser), Some("Resistor"));
    }

    #[test]
    fn the_selection_survives_a_query_that_still_matches_it() {
        let mut browser = browser();
        browser.set_query("npn_");
        browser.move_selection(1);
        assert_eq!(selected_name(&browser), Some("NPN_SMALL"));
        browser.set_query("np");
        assert_eq!(result_names(&browser).len(), 3, "a wider query");
        assert_eq!(selected_name(&browser), Some("NPN_SMALL"));
    }

    #[test]
    fn categories_restrict_the_list() {
        let mut browser = browser();
        browser.select_category(path(&[DIODES]));
        assert_eq!(result_names(&browser), ["D_RECT", "D_SWITCH"]);
        assert_eq!(selected_name(&browser), Some("D_RECT"));
        browser.select_category(path(&[BJTS, "PNP"]));
        assert_eq!(result_names(&browser), ["PNP_GP"]);
        assert_eq!(browser.scope(), path(&[BJTS, "PNP"]));
        assert_eq!(
            browser.count(&path(&[DIODES])),
            2,
            "counts ignore the category"
        );
    }

    #[test]
    fn typing_searches_every_category_whatever_is_selected() {
        let mut browser = browser();
        browser.select_category(path(&[SOURCES]));
        browser.set_query("general");
        assert!(browser.is_searching());
        assert!(browser.scope().is_empty(), "every category");
        assert_eq!(browser.category(), path(&[SOURCES]), "still selected");
        assert_eq!(
            result_names(&browser),
            ["D_RECT", "NPN_GP", "PNP_GP", "PJF_GP"]
        );
        assert_eq!(selected_name(&browser), Some("D_RECT"));
        assert_eq!(browser.count(&path(&[BJTS])), 2);
        assert_eq!(browser.count(&path(&[SOURCES])), 0);
    }

    #[test]
    fn clearing_the_search_returns_to_the_selected_category() {
        let mut browser = browser();
        browser.select_category(path(&[SOURCES]));
        browser.set_query("res");
        assert_eq!(selected_name(&browser), Some("Resistor"));
        browser.set_query("");
        assert!(!browser.is_searching());
        assert_eq!(browser.scope(), path(&[SOURCES]));
        assert_eq!(result_names(&browser), ["Voltage source", "Current source"]);
        assert_eq!(selected_name(&browser), Some("Voltage source"));
    }

    #[test]
    fn a_category_selected_while_searching_narrows_until_the_query_changes() {
        let mut browser = browser();
        browser.set_query("general");
        browser.select_category(path(&[BJTS]));
        assert_eq!(browser.scope(), path(&[BJTS]));
        assert_eq!(result_names(&browser), ["NPN_GP", "PNP_GP"]);
        assert_eq!(selected_name(&browser), Some("NPN_GP"));
        assert_eq!(browser.count(&[]), 4, "the tree still counts everything");
        assert_eq!(browser.count(&path(&[DIODES])), 1);

        browser.set_query("genera");
        assert!(browser.scope().is_empty(), "an edit searches everywhere");
        assert_eq!(result_names(&browser).len(), 4);
        assert_eq!(selected_name(&browser), Some("NPN_GP"), "kept");

        browser.select_category(path(&[JFETS]));
        assert_eq!(result_names(&browser), ["PJF_GP"]);
        browser.set_query("");
        assert_eq!(
            browser.scope(),
            path(&[JFETS]),
            "clearing shows the category selected last"
        );
        assert_eq!(result_names(&browser), ["NJF_RF", "PJF_GP"]);
        assert_eq!(selected_name(&browser), Some("PJF_GP"));

        browser.set_query("pjf");
        assert!(browser.scope().is_empty(), "a new search starts everywhere");
    }

    #[test]
    fn selecting_all_while_searching_keeps_every_category() {
        let mut browser = browser();
        browser.select_category(path(&[DIODES]));
        browser.set_query("general");
        browser.select_category(Vec::new());
        assert_eq!(result_names(&browser).len(), 4);
        browser.set_query("");
        assert!(browser.category().is_empty());
        assert_eq!(
            result_names(&browser).len(),
            fixtures::catalog().entries().len()
        );
    }

    #[test]
    fn a_narrowed_search_can_be_empty_while_other_categories_match() {
        let mut browser = browser();
        browser.set_query("resistor");
        browser.select_category(path(&[SOURCES]));
        assert!(!browser.has_results());
        assert_eq!(browser.selected_entry(), None);
        assert_eq!(browser.count(&[]), 1);
        assert_eq!(browser.count(&path(&[BASIC_PARTS])), 1);
    }

    #[test]
    fn a_blank_query_is_not_a_search() {
        let mut browser = browser();
        browser.select_category(path(&[DIODES]));
        browser.set_query("   ");
        assert!(!browser.is_searching());
        assert_eq!(browser.scope(), path(&[DIODES]));
        assert_eq!(result_names(&browser), ["D_RECT", "D_SWITCH"]);
        assert_eq!(result_categories(&browser), [None, None]);
    }

    #[test]
    fn results_name_their_category_only_while_searching() {
        let mut browser = browser();
        assert!(result_categories(&browser).iter().all(Option::is_none));

        browser.set_query("general");
        assert_eq!(
            result_categories(&browser),
            [
                Some("Diodes \u{203a} Rectifier".to_string()),
                Some("Bipolar transistors \u{203a} NPN \u{203a} General purpose".into()),
                Some("Bipolar transistors \u{203a} PNP \u{203a} General purpose".into()),
                Some("JFETs \u{203a} P-channel".into()),
            ]
        );

        browser.select_category(path(&[BJTS]));
        assert_eq!(
            result_categories(&browser),
            [
                Some("NPN \u{203a} General purpose".to_string()),
                Some("PNP \u{203a} General purpose".into()),
            ],
            "only the part below the narrowed category"
        );

        browser.select_category(path(&[JFETS, "P-channel"]));
        assert_eq!(result_names(&browser), ["PJF_GP"]);
        assert_eq!(
            result_categories(&browser),
            [None],
            "directly in the narrowed category"
        );
    }

    #[test]
    fn a_family_filter_limits_every_category_to_the_admitted_ones() {
        let catalog = fixtures::catalog().restricted(LibraryFilter::Bjts(Polarity::P));
        let item = LibraryItem::Bjt {
            polarity: Polarity::P,
            model: "PNP_GP".into(),
        };
        let mut browser = Browser::new(catalog, Some(&item));
        browser.set_query("general");
        assert_eq!(result_names(&browser), ["PNP_GP"]);
        assert_eq!(browser.count(&[]), 1, "the NPN and diode matches are out");
        assert_eq!(browser.count(&path(&[DIODES])), 0);
        browser.set_query("npn_gp");
        assert!(!browser.has_results());
        assert_eq!(browser.count(&[]), 0);
        browser.set_query("");
        assert_eq!(result_names(&browser), ["PNP_GP"]);
    }

    #[test]
    fn keyboard_navigation_stops_at_the_ends() {
        let mut browser = browser();
        browser.move_selection(-1);
        assert_eq!(selected_name(&browser), Some("Resistor"));
        browser.move_selection(2);
        assert_eq!(selected_name(&browser), Some("Inductor"));
        browser.move_selection(100);
        assert_eq!(selected_name(&browser), Some("PJF_GP"));
        assert_eq!(browser.selected_position(), Some(14));
    }

    #[test]
    fn keyboard_navigation_crosses_categories_while_searching() {
        let mut browser = browser();
        browser.select_category(path(&[SOURCES]));
        browser.set_query("current");
        assert_eq!(
            result_names(&browser),
            ["Current source", "Current-controlled switch", "Inductor"]
        );
        browser.move_selection(1);
        assert_eq!(selected_name(&browser), Some("Current-controlled switch"));
        browser.move_selection(1);
        assert_eq!(selected_name(&browser), Some("Inductor"));
        browser.move_selection(1);
        assert_eq!(selected_name(&browser), Some("Inductor"), "last row");
        browser.move_selection(-5);
        assert_eq!(selected_name(&browser), Some("Current source"));
    }

    #[test]
    fn clicking_selects_only_listed_entries() {
        let mut browser = browser();
        browser.select_category(path(&[DIODES]));
        browser.select(0);
        assert_eq!(
            selected_name(&browser),
            Some("D_RECT"),
            "resistor is not listed"
        );
        let ground = fixtures::catalog()
            .position(&LibraryItem::Part(PartKind::Ground))
            .expect("ground");
        browser.select_category(Vec::new());
        browser.select(ground);
        assert_eq!(selected_name(&browser), Some("Ground"));
    }

    #[test]
    fn categories_expand_and_collapse() {
        let mut browser = browser();
        let bjts = path(&[BJTS]);
        browser.toggle_expanded(&bjts);
        assert!(!browser.tree_rows().iter().any(|r| r.label() == "NPN"));
        browser.toggle_expanded(&bjts);
        assert!(browser.tree_rows().iter().any(|r| r.label() == "NPN"));
    }

    fn shows_row(browser: &Browser, label: &str) -> bool {
        browser.tree_rows().iter().any(|r| r.label() == label)
    }

    /// Up/Down walk the visible rows, Left/Right close, open, leave and
    /// enter categories, Enter opens and closes them.
    #[test]
    fn the_keyboard_walks_opens_and_closes_the_tree() {
        let mut browser = browser();
        assert_eq!(browser.highlighted_row(), Some(0), "all");
        browser.step_category(-1);
        assert!(browser.scope().is_empty(), "the first row stops Up");
        // All, Basic parts, Sources, Switches, Diodes.
        browser.step_category(4);
        assert_eq!(browser.scope(), path(&[DIODES]));
        assert_eq!(browser.highlighted_row(), Some(4));
        assert_eq!(result_names(&browser), ["D_RECT", "D_SWITCH"]);

        browser.collapse_highlighted();
        assert!(!shows_row(&browser, "Switching"), "Left closes");
        assert_eq!(browser.scope(), path(&[DIODES]), "and stays");
        browser.collapse_highlighted();
        assert!(browser.scope().is_empty(), "then goes up to all");

        browser.step_category(4);
        browser.expand_highlighted();
        assert!(shows_row(&browser, "Switching"), "Right opens");
        assert_eq!(browser.scope(), path(&[DIODES]), "and stays");
        browser.expand_highlighted();
        assert_eq!(browser.scope(), path(&[DIODES, "Rectifier"]), "then enters");
        browser.expand_highlighted();
        assert_eq!(browser.scope(), path(&[DIODES, "Rectifier"]), "a leaf");
        browser.collapse_highlighted();
        assert_eq!(browser.scope(), path(&[DIODES]), "Left leaves a leaf");

        assert!(browser.toggle_highlighted());
        assert!(!shows_row(&browser, "Switching"), "Enter closes");
        assert!(browser.toggle_highlighted());
        assert!(shows_row(&browser, "Switching"), "Enter opens");
        browser.step_category(-1);
        assert_eq!(browser.scope(), path(&[SWITCHES]));
        assert!(!browser.toggle_highlighted(), "nothing to open");
        assert!(shows_row(&browser, "Switching"));

        browser.step_category(isize::MAX);
        assert_eq!(browser.scope(), path(&[JFETS, "P-channel"]), "the last row");
        browser.step_category(isize::MIN);
        assert!(browser.scope().is_empty(), "back at the first row");
        browser.expand_highlighted();
        assert_eq!(browser.scope(), path(&[BASIC_PARTS]), "Right on all");
    }

    #[test]
    fn walking_the_tree_while_searching_narrows_like_a_click() {
        let mut browser = browser();
        browser.select_category(path(&[SOURCES]));
        browser.set_query("general");
        assert_eq!(browser.highlighted_row(), Some(0), "every category");
        browser.step_category(-1);
        assert_eq!(
            browser.category(),
            path(&[SOURCES]),
            "staying on all keeps the category to return to"
        );
        // All, Basic parts, Sources, Switches, Diodes, Rectifier, Switching,
        // Bipolar transistors.
        browser.step_category(7);
        assert_eq!(browser.scope(), path(&[BJTS]));
        assert_eq!(result_names(&browser), ["NPN_GP", "PNP_GP"]);
        browser.set_query("");
        assert_eq!(browser.scope(), path(&[BJTS]), "the walked-to category");
    }
}
