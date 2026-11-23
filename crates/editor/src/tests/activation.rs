//! Shortcuts keep working in an active (foreground) window, as every real
//! window is.

use super::fixtures::two_resistors;
use super::harness::{open, primary};
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn shortcuts_work_after_the_window_is_activated(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.cx.update(|window, _| window.activate_window());
    h.cx.run_until_parked();
    h.focus();
    let stack =
        h.cx.update(|window, _| format!("{:?}", window.context_stack()));
    h.keys(&primary("a"));
    let selected = h.read(|e| e.selection().len());
    assert!(
        selected > 0,
        "Ctrl+A selected nothing; context stack {stack}"
    );
    h.close();
}

#[gpui_kit::test]
fn shortcuts_work_when_activation_follows_focus(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.cx.update(|window, _| window.activate_window());
    h.cx.run_until_parked();
    let stack =
        h.cx.update(|window, _| format!("{:?}", window.context_stack()));
    h.keys(&primary("a"));
    let selected = h.read(|e| e.selection().len());
    assert!(
        selected > 0,
        "Ctrl+A selected nothing; context stack {stack}"
    );
    h.close();
}
