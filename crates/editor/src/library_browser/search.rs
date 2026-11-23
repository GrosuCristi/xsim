//! Library search: case-insensitive matching on name, description,
//! manufacturer and category, ranked by how well the name matches.
//! GPUI-free.

use super::catalog::CatalogEntry;
use super::categories;
use std::collections::HashMap;

/// How well an entry matches a query; lower is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rank {
    /// The name equals the query.
    Exact,
    /// The name starts with the query.
    Prefix,
    /// The name contains the query.
    Name,
    /// Every word of the query occurs in the name, description,
    /// manufacturer or category.
    Words,
}

/// How `entry` matches `query`, or `None` when it does not. An empty query
/// matches everything.
pub(crate) fn rank(entry: &CatalogEntry, query: &str) -> Option<Rank> {
    let query = query.trim().to_lowercase();
    let name = entry.name.to_lowercase();
    if query.is_empty() || name == query {
        return Some(Rank::Exact);
    }
    if name.starts_with(&query) {
        return Some(Rank::Prefix);
    }
    if name.contains(&query) {
        return Some(Rank::Name);
    }
    let haystack = [
        name.as_str(),
        &entry.description.to_lowercase(),
        &entry.manufacturer.to_lowercase(),
        &entry.path.join(" ").to_lowercase(),
    ]
    .join("\n");
    query
        .split_whitespace()
        .all(|word| haystack.contains(word))
        .then_some(Rank::Words)
}

/// Indices of the entries in `category` matching `query`, best first; ties
/// keep catalog order.
pub(crate) fn search(entries: &[CatalogEntry], query: &str, category: &[String]) -> Vec<usize> {
    let mut hits: Vec<(Rank, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| categories::contains(category, &entry.path))
        .filter_map(|(ix, entry)| rank(entry, query).map(|rank| (rank, ix)))
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, ix)| ix).collect()
}

/// Number of entries matching `query` under every category path (the empty
/// path counts all matches).
pub(crate) fn counts(entries: &[CatalogEntry], query: &str) -> HashMap<Vec<String>, usize> {
    let mut counts = HashMap::new();
    for entry in entries.iter().filter(|e| rank(e, query).is_some()) {
        for depth in 0..=entry.path.len() {
            *counts.entry(entry.path[..depth].to_vec()).or_default() += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_browser::catalog::{BJTS, DIODES, SWITCHES, fixtures};

    fn names(query: &str, category: &[&str]) -> Vec<String> {
        let catalog = fixtures::catalog();
        let category: Vec<String> = category.iter().map(|s| s.to_string()).collect();
        search(catalog.entries(), query, &category)
            .into_iter()
            .map(|ix| catalog.entries()[ix].name.clone())
            .collect()
    }

    #[test]
    fn an_empty_query_lists_the_whole_category_in_catalog_order() {
        assert_eq!(names("", &[DIODES]), ["D_RECT", "D_SWITCH"]);
        assert_eq!(names("  ", &[]).len(), fixtures::catalog().entries().len());
    }

    #[test]
    fn search_is_case_insensitive_over_every_text_field() {
        assert_eq!(names("d_sw", &[]), ["D_SWITCH"]);
        assert_eq!(names("ACME", &[]), ["D_SWITCH", "NPN_GP", "PNP_GP"]);
        assert_eq!(names("rectifier", &[]), ["D_RECT"]);
        assert_eq!(names("small signal", &[]), ["NPN_SMALL"]);
        assert_eq!(names("pnp 60 v", &[]), ["PNP_GP"]);
        assert!(names("mosfet", &[]).is_empty());
    }

    #[test]
    fn name_matches_rank_before_description_matches() {
        // "npn_" prefixes two names; "general" only matches descriptions.
        assert_eq!(names("npn_", &[]), ["NPN_GP", "NPN_SMALL"]);
        let catalog = fixtures::catalog();
        let resistor = &catalog.entries()[0];
        assert_eq!(rank(resistor, "resistor"), Some(Rank::Exact));
        assert_eq!(rank(resistor, "res"), Some(Rank::Prefix));
        assert_eq!(rank(resistor, "sist"), Some(Rank::Name));
        assert_eq!(rank(resistor, "ideal"), Some(Rank::Words));
        assert_eq!(
            names("current", &[]),
            ["Current source", "Current-controlled switch", "Inductor"]
        );
    }

    /// D17: the switches are found by name and by their SPICE model names,
    /// and counted under their own category.
    #[test]
    fn switches_are_found_by_name_and_spice_model() {
        assert_eq!(
            names("switch", &[]),
            [
                "Voltage-controlled switch",
                "Current-controlled switch",
                "D_SWITCH"
            ]
        );
        assert_eq!(names("csw", &[]), ["Current-controlled switch"]);
        assert_eq!(
            names("controlled switch", &[SWITCHES]),
            ["Voltage-controlled switch", "Current-controlled switch"]
        );
        let catalog = fixtures::catalog();
        let counts = counts(catalog.entries(), "switch");
        assert_eq!(counts.get(&vec![SWITCHES.to_string()]), Some(&2));
        assert_eq!(counts.get(&vec![DIODES.to_string()]), Some(&1));
    }

    #[test]
    fn the_category_restricts_results() {
        assert_eq!(names("general", &[BJTS]), ["NPN_GP", "PNP_GP"]);
        assert_eq!(names("general", &[BJTS, "PNP"]), ["PNP_GP"]);
        assert!(names("D_SWITCH", &[BJTS]).is_empty());
    }

    #[test]
    fn counts_cover_every_ancestor_category() {
        let catalog = fixtures::catalog();
        let counts = counts(catalog.entries(), "general");
        let count = |path: &[&str]| {
            let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
            counts.get(&path).copied().unwrap_or(0)
        };
        assert_eq!(count(&[]), 4, "two BJTs, the rectifier and the JFET");
        assert_eq!(count(&[BJTS]), 2);
        assert_eq!(count(&[BJTS, "NPN"]), 1);
        assert_eq!(count(&[DIODES]), 1);
        assert_eq!(count(&["Sources"]), 0);
    }
}
