//! Keyboard bindings of the library window and the part properties dialog.
//!
//! In a dialog Enter confirms the dialog whatever has the focus.
//! These bindings leave it to the focused control instead: a dialog button
//! (see [`pressable`]) or the category tree, whose own keys are bound here
//! as actions.

use gpui_kit::component::button::Button;
use gpui_kit::{App, Global, InteractiveElement as _, KeyBinding, NoAction};

/// Key context of a dialog button that Enter presses, as Space does. The
/// same context as the application shell's, so the two bindings agree.
const PRESSABLE_CONTEXT: &str = "PressableButton";

/// Key context of the library window's category tree.
pub(crate) const TREE_CONTEXT: &str = "LibraryCategoryTree";

gpui_kit::actions!(
    library_tree,
    [
        /// Shows the category above the highlighted one.
        PreviousCategory,
        /// Shows the category below the highlighted one.
        NextCategory,
        /// Closes the highlighted category, or goes to its parent.
        CollapseCategory,
        /// Opens the highlighted category, or goes to its first
        /// sub-category.
        ExpandCategory,
        /// Opens or closes the highlighted category.
        ToggleCategory,
    ]
);

/// Lets Enter press `button` when it has the focus instead of confirming
/// the dialog it sits in.
pub(crate) fn pressable(button: Button) -> Button {
    button.key_context(PRESSABLE_CONTEXT)
}

/// Marks the bindings as installed in this application.
struct KeysBound;
impl Global for KeysBound {}

/// Installs the bindings once per application.
pub(crate) fn ensure_key_bindings(cx: &mut App) {
    if cx.has_global::<KeysBound>() {
        return;
    }
    cx.set_global(KeysBound);
    let tree = Some(TREE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", NoAction, Some(PRESSABLE_CONTEXT)),
        KeyBinding::new("up", PreviousCategory, tree),
        KeyBinding::new("down", NextCategory, tree),
        KeyBinding::new("left", CollapseCategory, tree),
        KeyBinding::new("right", ExpandCategory, tree),
        KeyBinding::new("enter", ToggleCategory, tree),
        KeyBinding::new("space", ToggleCategory, tree),
    ]);
}
