//! Path and naming rules for project files (Q31, Q32, Q46).

use std::path::{Path, PathBuf};
use xsim_project::PROJECT_EXTENSION;

/// Appends `.<extension>` unless `path` already ends with it (any case).
/// A different extension is kept and the required one appended, so
/// `rc.v2` becomes `rc.v2.xsim_proj` rather than losing `.v2`.
pub fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let has_it = path
        .extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension));
    if has_it {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

/// [`with_extension`] for project files.
pub fn with_project_extension(path: PathBuf) -> PathBuf {
    with_extension(path, PROJECT_EXTENSION)
}

/// The project name a file implies: its stem, e.g. `RC low-pass` for
/// `RC low-pass.xsim_proj`.
pub fn project_name_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
}

/// A file name derived from `name` that every supported OS accepts:
/// reserved characters become `_` and an empty result becomes `Untitled`.
pub fn safe_file_stem(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        "Untitled".into()
    } else {
        cleaned
    }
}

/// Text of a recent-project menu entry: the file name followed by its folder.
pub fn recent_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => format!("{name}  —  {}", parent.display()),
        None => name,
    }
}

/// Window title: `"● RC low-pass - xsim"` while there are unsaved changes.
pub fn window_title(project: &str, dirty: bool, read_only: bool) -> String {
    let marker = if dirty { "● " } else { "" };
    let suffix = if read_only { " (read-only)" } else { "" };
    format!("{marker}{project}{suffix} - xsim")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_the_project_extension_once() {
        assert_eq!(
            with_project_extension("a/rc".into()),
            PathBuf::from("a/rc.xsim_proj")
        );
        assert_eq!(
            with_project_extension("rc.XSIM_PROJ".into()),
            PathBuf::from("rc.XSIM_PROJ")
        );
        assert_eq!(
            with_project_extension("rc.v2".into()),
            PathBuf::from("rc.v2.xsim_proj")
        );
    }

    #[test]
    fn names_follow_the_file_stem() {
        assert_eq!(
            project_name_from_path(Path::new("x/RC low-pass.xsim_proj")).as_deref(),
            Some("RC low-pass")
        );
        assert_eq!(safe_file_stem(" a/b:c? "), "a_b_c_");
        assert_eq!(safe_file_stem("..."), "Untitled");
    }

    #[test]
    fn titles_mark_unsaved_and_read_only_projects() {
        assert_eq!(window_title("RC", false, false), "RC - xsim");
        assert_eq!(window_title("RC", true, false), "● RC - xsim");
        assert_eq!(window_title("RC", false, true), "RC (read-only) - xsim");
    }

    #[test]
    fn recent_labels_show_the_folder() {
        let label = recent_label(Path::new("dir/rc.xsim_proj"));
        assert!(label.starts_with("rc.xsim_proj"));
        assert!(label.ends_with("dir"));
    }
}
