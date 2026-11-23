//! Layout maths for the schematic/results split (Q22).

/// Narrowest the schematic or results side may become, in pixels.
pub const MIN_SIDE_WIDTH: f32 = 280.0;

/// Default share of the width given to the schematic.
pub const DEFAULT_FRACTION: f32 = 0.5;

/// Keeps `fraction` of `total` width such that both sides stay at least
/// `min_side` wide. When the window is too narrow for that, the split is
/// centred. Non-finite input falls back to [`DEFAULT_FRACTION`].
pub fn clamp_fraction(fraction: f32, total: f32, min_side: f32) -> f32 {
    let fraction = if fraction.is_finite() {
        fraction
    } else {
        DEFAULT_FRACTION
    };
    if !(total.is_finite() && total > 0.0) || total < 2.0 * min_side {
        return DEFAULT_FRACTION;
    }
    let low = min_side / total;
    fraction.clamp(low, 1.0 - low)
}

/// A stored fraction (possibly hand-edited) limited to `0.1..=0.9`, so
/// neither side can vanish whatever the window size.
pub fn sanitize_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.1, 0.9)
    } else {
        DEFAULT_FRACTION
    }
}

/// The split fraction for a divider dragged to `x` in a container that
/// starts at `left` and is `width` wide.
pub fn fraction_at(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return DEFAULT_FRACTION;
    }
    clamp_fraction((x - left) / width, width, MIN_SIDE_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn keeps_both_sides_usable() {
        assert!(close(clamp_fraction(0.5, 1000.0, 280.0), 0.5));
        assert!(close(clamp_fraction(0.05, 1000.0, 280.0), 0.28));
        assert!(close(clamp_fraction(0.99, 1000.0, 280.0), 0.72));
    }

    #[test]
    fn narrow_or_invalid_input_centres_the_split() {
        assert!(close(clamp_fraction(0.9, 400.0, 280.0), 0.5));
        assert!(close(clamp_fraction(f32::NAN, 1000.0, 280.0), 0.5));
        assert!(close(clamp_fraction(0.3, 0.0, 280.0), 0.5));
    }

    #[test]
    fn stored_fractions_are_sanitized() {
        assert!(close(sanitize_fraction(0.0), 0.1));
        assert!(close(sanitize_fraction(0.6), 0.6));
        assert!(close(sanitize_fraction(f32::INFINITY), 0.5));
    }

    #[test]
    fn drag_position_maps_to_a_fraction() {
        assert!(close(fraction_at(600.0, 100.0, 1000.0), 0.5));
        assert!(close(fraction_at(0.0, 100.0, 1000.0), 0.28));
        assert!(close(fraction_at(10.0, 0.0, 0.0), 0.5));
    }
}
