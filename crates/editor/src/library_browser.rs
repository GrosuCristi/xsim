//! Component library window (Q54, Q144).
//!
//! Layout: a search field on top; a category tree on the left (Basic parts,
//! Sources, Switches, Diodes, Transistors, JFETs, Op-amps with the
//! library's sub-categories); the list of matching parts/models on the right; a
//! preview of the selected symbol with its metadata (description,
//! manufacturer, key parameters, provenance) at the bottom. Picking an item
//! calls the caller's callback.
//!
//! Search (D12): typing searches every category the window offers (with a
//! restrictive [`LibraryFilter`], every category that filter admits), not
//! only the selected one. While searching, the tree highlights "All …" and
//! counts the matches per category, and each result names its category.
//! Selecting a category while searching narrows the matches to it until the
//! query is edited again. Clearing the search lists the category selected
//! last in the tree in full. When nothing matches, the list says so.
//!
//! Keyboard: the search field has the focus on opening; Up and Down move
//! through the list and Enter picks. Tab reaches the category tree, where
//! Up/Down walk the categories, Left/Right close and open them (or go up
//! and down a level) and Enter or Space open and close one; then Cancel and
//! Place, which Enter presses. Escape closes the window.
//!
//! The library is a resizable window of its own (D22), opened from the
//! editor's toolbar or P, and from a part's model field.

mod browser;
mod catalog;
mod categories;
mod details;
mod dialog;
mod keys;
mod panels;
mod parameters;
mod results;
mod search;
mod tree;
mod view;

pub(crate) use keys::{ensure_key_bindings, pressable};

use gpui_kit::{App, Window};
use std::rc::Rc;
use xsim_project::{Part, PartKind};
use xsim_spice_engine::Polarity;

/// What the user picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryItem {
    /// A part without a library model, with its default parameters (R, C,
    /// L, sources, switches, ground).
    Part(PartKind),
    Diode {
        model: String,
    },
    Bjt {
        polarity: Polarity,
        model: String,
    },
    Jfet {
        polarity: Polarity,
        model: String,
    },
    /// An op-amp using a library model (D26).
    OpAmp {
        model: String,
    },
}

impl LibraryItem {
    /// Kind of the part this item places.
    pub fn kind(&self) -> PartKind {
        match self {
            Self::Part(kind) => *kind,
            Self::Diode { .. } => PartKind::Diode,
            Self::Bjt {
                polarity: Polarity::N,
                ..
            } => PartKind::NpnBjt,
            Self::Bjt {
                polarity: Polarity::P,
                ..
            } => PartKind::PnpBjt,
            Self::Jfet {
                polarity: Polarity::N,
                ..
            } => PartKind::NJfet,
            Self::Jfet {
                polarity: Polarity::P,
                ..
            } => PartKind::PJfet,
            Self::OpAmp { .. } => PartKind::OpAmp,
        }
    }

    /// The part to place for this item: default parameters for a basic part,
    /// or a semiconductor or op-amp using the picked model. Pass it to
    /// [`crate::SchematicEditor::start_placing`].
    pub fn to_part(&self) -> Part {
        match self {
            Self::Part(kind) => Part::default_for(*kind),
            Self::Diode { model } => Part::Diode {
                model: model.clone(),
            },
            Self::Bjt { polarity, model } => Part::Bjt {
                polarity: *polarity,
                model: model.clone(),
            },
            Self::Jfet { polarity, model } => Part::Jfet {
                polarity: *polarity,
                model: model.clone(),
            },
            Self::OpAmp { model } => Part::OpAmp {
                model: model.clone(),
            },
        }
    }

    /// The model item a semiconductor or op-amp uses; `None` for parts
    /// without a model.
    pub fn model_of(part: &Part) -> Option<Self> {
        match part {
            Part::Diode { model } => Some(Self::Diode {
                model: model.clone(),
            }),
            Part::Bjt { polarity, model } => Some(Self::Bjt {
                polarity: *polarity,
                model: model.clone(),
            }),
            Part::Jfet { polarity, model } => Some(Self::Jfet {
                polarity: *polarity,
                model: model.clone(),
            }),
            Part::OpAmp { model } => Some(Self::OpAmp {
                model: model.clone(),
            }),
            _ => None,
        }
    }
}

/// Restricts the browser to one family (used by the properties dialog to
/// choose a model for an existing part).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryFilter {
    All,
    Diodes,
    Bjts(Polarity),
    Jfets(Polarity),
    /// The op-amp models (D26).
    OpAmps,
}

impl LibraryFilter {
    /// Whether the browser shows `item` under this filter.
    pub fn admits(&self, item: &LibraryItem) -> bool {
        match (self, item) {
            (Self::All, _) => true,
            (Self::Diodes, LibraryItem::Diode { .. })
            | (Self::OpAmps, LibraryItem::OpAmp { .. }) => true,
            (Self::Bjts(wanted), LibraryItem::Bjt { polarity, .. })
            | (Self::Jfets(wanted), LibraryItem::Jfet { polarity, .. }) => wanted == polarity,
            _ => false,
        }
    }

    /// The filter that lists the models a part of `kind` can use; `None` for
    /// kinds without a model.
    pub fn for_kind(kind: PartKind) -> Option<Self> {
        match kind {
            PartKind::Diode => Some(Self::Diodes),
            PartKind::NpnBjt => Some(Self::Bjts(Polarity::N)),
            PartKind::PnpBjt => Some(Self::Bjts(Polarity::P)),
            PartKind::NJfet => Some(Self::Jfets(Polarity::N)),
            PartKind::PJfet => Some(Self::Jfets(Polarity::P)),
            PartKind::OpAmp => Some(Self::OpAmps),
            _ => None,
        }
    }
}

/// Library metadata of a model, shown where a part's model is chosen.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelSummary {
    pub description: String,
    pub manufacturer: String,
    /// Key parameters, formatted with units, as the library preview lists
    /// them.
    pub parameters: Vec<(&'static str, String)>,
}

/// Metadata of `item` from the built-in library; `None` when it is not
/// listed there.
pub(crate) fn model_summary(item: &LibraryItem) -> Option<ModelSummary> {
    catalog::Catalog::builtin()
        .find(item)
        .map(|entry| ModelSummary {
            description: entry.description.clone(),
            manufacturer: entry.manufacturer.clone(),
            parameters: entry.parameters.clone(),
        })
}

/// Opens the library in its own modal window over `window` (D22, see
/// [`crate::dialog_window`]). `on_pick` gets the picked item in `window`,
/// once the library has closed.
pub fn open_library(
    filter: LibraryFilter,
    on_pick: impl Fn(LibraryItem, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    open_library_at(filter, None, on_pick, window, cx);
}

/// [`open_library`] with `selected` highlighted (and its category expanded)
/// when the filter lists it, e.g. the model a part currently uses.
pub fn open_library_at(
    filter: LibraryFilter,
    selected: Option<LibraryItem>,
    on_pick: impl Fn(LibraryItem, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    dialog::open(filter, selected, Rc::new(on_pick), window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_convert_to_parts_of_their_kind() {
        let items = [
            LibraryItem::Part(PartKind::Capacitor),
            LibraryItem::Diode {
                model: "D_RECT_1A".into(),
            },
            LibraryItem::Bjt {
                polarity: Polarity::P,
                model: "PNP_LOWNOISE".into(),
            },
            LibraryItem::Jfet {
                polarity: Polarity::N,
                model: "NJF_MID".into(),
            },
            LibraryItem::OpAmp {
                model: "OA_FAST".into(),
            },
        ];
        for item in items {
            let part = item.to_part();
            assert_eq!(part.kind(), item.kind());
            if item.kind() != PartKind::Capacitor {
                assert_eq!(LibraryItem::model_of(&part), Some(item));
            }
        }
        assert_eq!(LibraryItem::model_of(&Part::Ground), None);
    }

    #[test]
    fn filters_admit_one_family_and_polarity() {
        let npn = LibraryItem::Bjt {
            polarity: Polarity::N,
            model: "NPN_GP".into(),
        };
        assert!(LibraryFilter::All.admits(&npn));
        assert!(LibraryFilter::Bjts(Polarity::N).admits(&npn));
        assert!(!LibraryFilter::Bjts(Polarity::P).admits(&npn));
        assert!(!LibraryFilter::Jfets(Polarity::N).admits(&npn));
        assert!(!LibraryFilter::Diodes.admits(&LibraryItem::Part(PartKind::Diode)));
        assert_eq!(
            LibraryFilter::for_kind(PartKind::PJfet),
            Some(LibraryFilter::Jfets(Polarity::P))
        );
        assert_eq!(LibraryFilter::for_kind(PartKind::Resistor), None);
        let opamp = LibraryItem::OpAmp {
            model: "OA_GP".into(),
        };
        assert!(LibraryFilter::OpAmps.admits(&opamp));
        assert!(!LibraryFilter::Diodes.admits(&opamp));
        assert!(!LibraryFilter::OpAmps.admits(&npn));
        assert_eq!(
            LibraryFilter::for_kind(PartKind::OpAmp),
            Some(LibraryFilter::OpAmps)
        );
    }
}
