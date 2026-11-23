//! Autocompletion for the add-trace field: signal names that complete the
//! fragment typed at the end of the expression (`V(o` → `V(out)`,
//! `V(out)-V(i` → `V(out)-V(in)`, `db(V(o` → `db(V(out)`).

/// Characters after which a new operand starts.
const BOUNDARIES: &[char] = &['+', '-', '*', '/', '^', ',', '(', ' '];

/// One suggestion: `label` replaces the text from byte `from` to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub from: usize,
}

impl Completion {
    /// `text` with this completion applied.
    pub fn apply(&self, text: &str) -> String {
        let from = self.from.min(text.len());
        format!("{}{}", &text[..from], self.label)
    }
}

/// Up to `limit` completions for `text` from `names` (in their order). The
/// longest trailing fragment that prefixes a name wins; failing that, names
/// containing the last operand match. Empty text or a trailing operator
/// suggests every name.
pub fn complete(text: &str, names: &[String], limit: usize) -> Vec<Completion> {
    let starts = operand_starts(text);
    let Some(&last) = starts.last() else {
        return Vec::new();
    };
    if text[last..].trim().is_empty() {
        return suggest(names.iter(), text.len(), limit);
    }
    for &from in &starts {
        let fragment = text[from..].trim_start();
        let from = text.len() - fragment.len();
        let matches: Vec<&String> = names
            .iter()
            .filter(|name| starts_with_ignore_case(name, fragment))
            .collect();
        if matches.iter().any(|name| name.as_str() == fragment) {
            return Vec::new();
        }
        if !matches.is_empty() {
            return suggest(matches.into_iter(), from, limit);
        }
    }
    let fragment = text[last..].trim_start().to_lowercase();
    let from = text.len() - text[last..].trim_start().len();
    suggest(
        names
            .iter()
            .filter(|name| name.to_lowercase().contains(&fragment)),
        from,
        limit,
    )
}

/// Byte offsets where an operand may start, in increasing order.
fn operand_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            text.char_indices()
                .filter(|(_, c)| BOUNDARIES.contains(c))
                .map(|(i, c)| i + c.len_utf8()),
        )
        .collect()
}

fn starts_with_ignore_case(name: &str, fragment: &str) -> bool {
    name.len() >= fragment.len()
        && name.is_char_boundary(fragment.len())
        && name[..fragment.len()].eq_ignore_ascii_case(fragment)
}

fn suggest<'a>(
    names: impl Iterator<Item = &'a String>,
    from: usize,
    limit: usize,
) -> Vec<Completion> {
    names
        .take(limit)
        .map(|name| Completion {
            label: name.clone(),
            from,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["V(in)", "V(out)", "V(n001)", "I(R1)", "Ic(Q1)"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn labels(text: &str) -> Vec<String> {
        complete(text, &names(), 8)
            .into_iter()
            .map(|c| c.apply(text))
            .collect()
    }

    #[test]
    fn completes_a_partial_signal() {
        assert_eq!(labels("V(o"), ["V(out)"]);
        assert_eq!(labels("v(i"), ["V(in)"]);
        assert_eq!(labels("I"), ["I(R1)", "Ic(Q1)"]);
    }

    #[test]
    fn completes_after_operators_and_inside_calls() {
        assert_eq!(labels("V(out)-V(i"), ["V(out)-V(in)"]);
        assert_eq!(labels("db(V(o"), ["db(V(out)"]);
        assert_eq!(labels("V(out) * I"), ["V(out) * I(R1)", "V(out) * Ic(Q1)"]);
    }

    #[test]
    fn falls_back_to_substring_matches() {
        assert_eq!(labels("out"), ["V(out)"]);
        assert_eq!(labels("r1"), ["I(R1)"]);
        assert!(labels("zzz").is_empty());
    }

    #[test]
    fn empty_text_or_trailing_operator_lists_everything() {
        assert_eq!(complete("", &names(), 3).len(), 3);
        assert_eq!(labels("V(out)-").len(), 5);
        assert_eq!(labels("V(out)-")[0], "V(out)-V(in)");
    }

    #[test]
    fn a_complete_name_needs_no_suggestion() {
        assert!(labels("V(out)").is_empty());
    }
}
