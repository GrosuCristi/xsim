//! Toolbar icons. Editor-specific glyphs are small inline SVGs drawn in the
//! style of the kit's Lucide set (24 px grid, 2 px round strokes); generic
//! actions use the kit's bundled [`IconName`]s.

use crate::symbol_art::svg::symbol_svg;
use gpui_kit::component::{Icon, IconName};
use xsim_project::PartKind;

const SVG_OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#;

const SELECT: &str = r#"<path d="M5 3.5 18.5 10l-6 1.8-2.6 6.2z"/>"#;
const WIRE: &str = r#"<path d="M4 18h7V6h9"/><circle cx="4" cy="18" r="1.8" fill="currentColor"/><circle cx="20" cy="6" r="1.8" fill="currentColor"/>"#;
const LABEL: &str = r#"<path d="M2.5 12 7 6.5h14.5v11H7z"/><path d="M11 12h6"/>"#;
const MIRROR_H: &str =
    r#"<path d="M12 3v18" stroke-dasharray="2 3"/><path d="M9 7v10H3z"/><path d="M15 7v10h6z"/>"#;
const MIRROR_V: &str =
    r#"<path d="M3 12h18" stroke-dasharray="2 3"/><path d="M7 9h10V3z"/><path d="M7 15h10v6z"/>"#;
const TRASH: &str = r#"<path d="M4 7h16"/><path d="M9 7V4.5h6V7"/><path d="M6.5 7l1 13h9l1-13"/><path d="M10 11v5.5M14 11v5.5"/>"#;

fn inline(body: &str) -> Icon {
    let svg = format!("{SVG_OPEN}{body}</svg>");
    Icon::default().data(svg.as_bytes())
}

/// Icons of the editor's toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorIcon {
    Select,
    Wire,
    Label,
    Part(PartKind),
    Library,
    Rotate,
    MirrorHorizontal,
    MirrorVertical,
    Delete,
    Undo,
    Redo,
    ZoomToFit,
}

impl EditorIcon {
    pub fn icon(self) -> Icon {
        match self {
            EditorIcon::Select => inline(SELECT),
            EditorIcon::Wire => inline(WIRE),
            EditorIcon::Label => inline(LABEL),
            EditorIcon::Part(kind) => Icon::default().data(symbol_svg(kind).as_bytes()),
            EditorIcon::Library => Icon::new(IconName::BookOpen),
            EditorIcon::Rotate => Icon::new(IconName::RotateCw),
            EditorIcon::MirrorHorizontal => inline(MIRROR_H),
            EditorIcon::MirrorVertical => inline(MIRROR_V),
            EditorIcon::Delete => inline(TRASH),
            EditorIcon::Undo => Icon::new(IconName::Undo2),
            EditorIcon::Redo => Icon::new(IconName::Redo2),
            EditorIcon::ZoomToFit => Icon::new(IconName::Maximize),
        }
    }
}
