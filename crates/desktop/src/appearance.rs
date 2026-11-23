//! Light/dark theme (Q25) and interface scale (Q30).
//!
//! The kit's root sets the window's rem size from `Theme::font_size` every
//! frame, so scaling the interface means scaling the theme's font sizes.
//! Reloading a theme can reset them, so [`apply`] always sets both.

use gpui_kit::{
    App,
    component::{Theme, ThemeMode},
    px,
};
use xsim_project::appdata::{AppSettings, ThemePreference};

/// Interface scales offered in the settings dialog.
pub const SCALE_PRESETS: [f32; 8] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// Smallest and largest supported interface scale.
pub const SCALE_RANGE: (f32, f32) = (0.75, 2.0);

/// Base font size of the kit theme at 100 %.
const BASE_FONT_SIZE: f32 = 16.0;

/// Base monospace font size of the kit theme at 100 %.
const BASE_MONO_FONT_SIZE: f32 = 13.0;

/// Brings an arbitrary (possibly hand-edited) scale into the supported range.
pub fn clamp_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1)
    } else {
        1.0
    }
}

/// `"125%"`.
pub fn scale_label(scale: f32) -> String {
    format!("{}%", (clamp_scale(scale) * 100.0).round())
}

/// The kit theme mode for a stored preference.
pub fn theme_mode(theme: ThemePreference) -> ThemeMode {
    match theme {
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
    }
}

/// Applies `settings` to every window.
pub fn apply(settings: &AppSettings, cx: &mut App) {
    Theme::change(theme_mode(settings.theme), None, cx);
    let scale = clamp_scale(settings.ui_scale);
    Theme::update(cx, |theme| {
        theme.font_size = px(BASE_FONT_SIZE * scale);
        theme.mono_font_size = px(BASE_MONO_FONT_SIZE * scale);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_stay_in_range() {
        assert_eq!(clamp_scale(0.1), 0.75);
        assert_eq!(clamp_scale(9.0), 2.0);
        assert_eq!(clamp_scale(f32::NAN), 1.0);
        assert_eq!(scale_label(1.25), "125%");
        assert!(SCALE_PRESETS.iter().all(|&s| clamp_scale(s) == s));
    }

    #[test]
    fn preferences_map_to_kit_modes() {
        assert!(theme_mode(ThemePreference::Dark).is_dark());
        assert!(!theme_mode(ThemePreference::Light).is_dark());
    }
}
