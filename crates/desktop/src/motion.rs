//! D19: panels open and close without animation. That covers the dialogs
//! (part properties, analysis settings, component library, settings,
//! recovery), the diagnostics dock, the plot's side panels, menus and
//! popovers.
//!
//! The dialogs are windows of their own (D22, `xsim_editor::dialog_window`),
//! which draw no transition. The kit's components animate unless reduced
//! motion is on, so it is set once for the whole application, after the
//! kit has read the system's preference, which it then leaves alone. The
//! dock and the plot's side panels have no motion of their own, and the
//! kit's menus and popovers do not animate. (The operating system may still
//! animate a window as it appears.)
//!
//! Reduced motion also settles the kit's other transitions on their first
//! frame. Toasts appear and leave at once but keep their lifecycle and
//! timeout. Tab highlights, tooltips and scrollbar fades no longer move,
//! and the progress bar steps instead of easing. The kit's spinner stands
//! still too, so the run loader turns its own ([`crate::ui::spinner`]).

use gpui_kit::App;

/// Turns the kit's open and close motion off. Called after `gpui_kit::init`
/// by the application and by the headless tests alike.
pub fn init(cx: &mut App) {
    cx.set_reduce_motion(true);
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn the_tests_run_with_motion_reduced_like_the_application(cx: &mut TestAppContext) {
        crate::test_support::init(cx);
        cx.update(|cx| assert!(cx.reduce_motion()));
    }
}
