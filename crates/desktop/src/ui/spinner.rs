//! The run loader's spinner.
//!
//! The application runs with reduced motion so panels open and close without
//! animation (D19, [`crate::motion`]). The kit's `Spinner` turns with a
//! repeating animation, which reduced motion draws at its start and never
//! advances. A loader is not a panel, and it is there to show that the run
//! is going on, so this spinner turns on the executor's clock whatever the
//! motion setting.

use gpui_kit::component::{Icon, IconName, Sizable};
use gpui_kit::{App, ElementId, IntoElement, RenderOnce, Window, ease_in_out, radians};
use std::f32::consts::TAU;
use std::time::Duration;

/// How long one turn takes, as for the kit's spinner.
const TURN: Duration = Duration::from_millis(800);

/// A small loader icon that keeps turning while it is shown.
#[derive(IntoElement)]
pub struct LoaderSpinner {
    id: ElementId,
}

impl LoaderSpinner {
    /// `id` keys the moment the spinner appeared, from which it turns.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into() }
    }
}

impl RenderOnce for LoaderSpinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let now = cx.background_executor().now();
        let shown_since = window.use_keyed_state(self.id, cx, |_, _| now);
        let elapsed = now.saturating_duration_since(*shown_since.read(cx));
        // Unlike the kit's animations, ask for the next frame even with
        // reduced motion: it is the only thing that turns the spinner.
        window.request_animation_frame();
        Icon::new(IconName::Loader)
            .small()
            .rotate(radians(turn_angle(elapsed)))
    }
}

/// The angle in radians `elapsed` after the spinner appeared: one turn per
/// [`TURN`], easing in and out like the kit's spinner.
fn turn_angle(elapsed: Duration) -> f32 {
    // Reduced in whole nanoseconds, so long runs keep a precise phase.
    let into_turn = elapsed.as_nanos() % TURN.as_nanos();
    let phase = into_turn as f32 / TURN.as_nanos() as f32;
    ease_in_out(phase) * TAU
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::spinner::Spinner;
    use gpui_kit::{Context, ParentElement, Render, TestAppContext, div};
    use std::f32::consts::PI;

    #[test]
    fn it_turns_once_per_period_easing_in_and_out() {
        assert_eq!(turn_angle(Duration::ZERO), 0.0);
        assert!((turn_angle(TURN / 2) - PI).abs() < 1e-5, "half a turn");
        assert_eq!(turn_angle(TURN), 0.0, "a whole turn starts over");
        assert!(turn_angle(TURN / 4) < PI / 2.0, "eases in");
        assert!(turn_angle(TURN * 3 / 4) > 3.0 * PI / 2.0, "eases out");
        let samples: Vec<f32> = (0..8).map(|i| turn_angle(TURN * i / 8)).collect();
        assert!(samples.windows(2).all(|w| w[0] < w[1]), "{samples:?}");
        let after_an_hour = Duration::from_secs(3600) + TURN / 2;
        assert!(
            (turn_angle(after_an_hour) - PI).abs() < 1e-5,
            "precise late"
        );
    }

    struct Host {
        kit: bool,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            if self.kit {
                div().child(Spinner::new().small())
            } else {
                div().child(LoaderSpinner::new("spinner"))
            }
        }
    }

    /// How many animation frames a window showing a spinner asks for, frame
    /// after frame, with motion reduced as in the application.
    fn frames_asked(kit: bool, cx: &mut TestAppContext) -> Vec<usize> {
        let window = cx.add_window(move |_, _| Host { kit });
        cx.run_until_parked();
        (0..3)
            .map(|_| {
                cx.executor().advance_clock(TURN / 3);
                let asked = window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .expect("the window is open");
                cx.run_until_parked();
                asked
            })
            .collect()
    }

    #[gpui_kit::test]
    fn the_loader_keeps_turning_with_motion_reduced(cx: &mut TestAppContext) {
        crate::test_support::init(cx);
        assert_eq!(frames_asked(false, cx), [1, 1, 1], "frame after frame");
        // Why the loader does not use the kit's spinner.
        assert_eq!(frames_asked(true, cx), [0, 0, 0], "the kit's stands still");
    }
}
