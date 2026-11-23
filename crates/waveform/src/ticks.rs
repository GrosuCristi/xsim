//! Axis tick generation.
//!
//! Tick values are built as an integer mantissa times a power of ten and
//! converted to `f64` in one correctly rounded step, so they print as the
//! short decimals they denote (`0.3`, never `0.30000000000000004`).
//! Stepping by repeated addition or multiplying by an inexact step would
//! accumulate exactly that kind of error.
//!
//! Non-finite input produces no ticks rather than an error: an axis with
//! no data simply stays blank.

/// Mantissas of "nice" steps within one decade.
const NICE_MANTISSAS: [i64; 3] = [1, 2, 5];

/// More ticks than any axis can label; reached only for nonsensical input.
const MAX_TICKS: f64 = 10_000.0;

/// "Nice" linear ticks (multiples of 1, 2 or 5 × 10ⁿ) covering `[min, max]`
/// with roughly `target` ticks, all inside the range, increasing. A
/// degenerate range (`min == max`) is widened symmetrically first.
///
/// The bounds may be given in either order. The step is the nice step whose
/// tick count is closest to `target` (the larger step on a tie).
pub fn linear(min: f64, max: f64, target: usize) -> Vec<f64> {
    if !min.is_finite() || !max.is_finite() || target == 0 {
        return Vec::new();
    }
    let (low, high) = ordered(min, max);
    let (low, high) = if low == high { widen(low) } else { (low, high) };
    let Some(step) = nice_step(low, high, target) else {
        return Vec::new();
    };
    // `low / step` rounds, so a multiple exactly on a bound can be missed
    // (0.7 / 0.1 is 6.999…): consider one extra multiple at each end and
    // keep the snapped values that fall inside the range. Counting with an
    // integer keeps the loop finite even where `k + 1.0 == k`.
    let (first, last) = step.multiples_within(low, high);
    let count = (last - first + 3.0) as usize;
    let mut ticks: Vec<f64> = Vec::new();
    for i in 0..count {
        let value = step.multiple(first - 1.0 + i as f64);
        // Below the resolution of f64, neighbouring multiples can coincide.
        if (low..=high).contains(&value) && ticks.last().is_none_or(|&previous| value > previous) {
            ticks.push(value);
        }
    }
    ticks
}

/// Logarithmic ticks for `0 < min < max`: every decade, plus 2 and 5 within
/// decades when the range spans fewer than `target` decades. Returns
/// `(value, is_major)` pairs; decades are major.
///
/// The bounds may be given in either order; anything else returns no ticks.
pub fn logarithmic(min: f64, max: f64, target: usize) -> Vec<(f64, bool)> {
    let (low, high) = ordered(min, max);
    if !(low > 0.0 && low < high && high.is_finite()) {
        return Vec::new();
    }
    let decades = high.log10() - low.log10();
    let mantissas: &[i64] = if decades < target as f64 {
        &NICE_MANTISSAS
    } else {
        &NICE_MANTISSAS[..1]
    };
    // f64 spans about 10^-324 to 10^308, so the casts are exact.
    let first = low.log10().floor() as i32;
    let last = high.log10().ceil() as i32;
    (first..=last)
        .flat_map(|exponent| mantissas.iter().map(move |&mantissa| (mantissa, exponent)))
        .map(|(mantissa, exponent)| (decimal(mantissa as f64, exponent), mantissa == 1))
        .filter(|(value, _)| (low..=high).contains(value))
        .collect()
}

/// Range padded by `fraction` of its span on both sides, or ±1 (relative to
/// magnitude) when empty, for automatic axis limits.
///
/// "±1 relative to magnitude" is one unit of the value's leading decimal
/// digit: `5` becomes `[4, 6]`, `0.03` becomes `[0.02, 0.04]` and `0`
/// becomes `[−1, 1]`. Non-finite bounds (an axis with no data) give
/// `[−1, 1]`; a negative or NaN `fraction` pads nothing.
pub fn padded(min: f64, max: f64, fraction: f64) -> (f64, f64) {
    if !min.is_finite() || !max.is_finite() {
        return (-1.0, 1.0);
    }
    let (low, high) = ordered(min, max);
    if low == high {
        return widen(low);
    }
    let padding = (high - low) * fraction.max(0.0);
    let padded = (low - padding, high + padding);
    if padded.0.is_finite() && padded.1.is_finite() {
        padded
    } else {
        (low, high)
    }
}

fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Widens an empty range at `value` by one unit of its leading digit.
fn widen(value: f64) -> (f64, f64) {
    if value == 0.0 {
        return (-1.0, 1.0);
    }
    let unit = decimal(1.0, value.abs().log10().floor() as i32);
    let widened = (value - unit, value + unit);
    // Near f64::MAX the upper end overflows; keep the range finite.
    if widened.1.is_finite() && widened.0.is_finite() {
        widened
    } else {
        (value - unit, value)
    }
}

/// A tick step `mantissa · 10^exponent`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Step {
    mantissa: i64,
    exponent: i32,
}

impl Step {
    fn value(self) -> f64 {
        decimal(self.mantissa as f64, self.exponent)
    }

    /// The `k`-th multiple of the step, `k · mantissa · 10^exponent`,
    /// correctly rounded. `k` is integer-valued.
    fn multiple(self, k: f64) -> f64 {
        // Adding zero turns -0 into +0 so the origin never prints as "-0".
        decimal(k * self.mantissa as f64, self.exponent) + 0.0
    }

    /// The first and last integer multipliers whose multiples lie in
    /// `[low, high]` (before snapping).
    fn multiples_within(self, low: f64, high: f64) -> (f64, f64) {
        let step = self.value();
        ((low / step).ceil(), (high / step).floor())
    }

    fn count_within(self, low: f64, high: f64) -> f64 {
        let (first, last) = self.multiples_within(low, high);
        (last - first + 1.0).max(0.0)
    }
}

/// The nice step whose tick count over `[low, high]` is closest to
/// `target`, preferring the larger step on a tie.
fn nice_step(low: f64, high: f64, target: usize) -> Option<Step> {
    let span = high - low;
    if !(span > 0.0 && span.is_finite()) {
        return None;
    }
    let target = target as f64;
    let exponent = (span / target).log10().floor() as i32;
    let candidates = (exponent - 1..=exponent + 1)
        .flat_map(|exponent| NICE_MANTISSAS.map(|mantissa| Step { mantissa, exponent }))
        .filter(|step| step.value() > 0.0 && step.value().is_finite());
    let mut best: Option<(Step, f64)> = None;
    for step in candidates {
        let miss = (step.count_within(low, high) - target).abs();
        // Candidates come in increasing order, so `<=` keeps the larger
        // step on a tie.
        if best.is_none_or(|(_, best_miss)| miss <= best_miss) {
            best = Some((step, miss));
        }
    }
    best.map(|(step, _)| step)
        .filter(|&step| step.count_within(low, high) <= MAX_TICKS)
}

/// `mantissa · 10^exponent` for an integer-valued `mantissa`, correctly
/// rounded. Going through the decimal text is the simplest exact route for
/// every exponent: `10^e` itself is not representable beyond `1e22`, so
/// multiplying or dividing by a computed power of ten could be off by an
/// ulp.
fn decimal(mantissa: f64, exponent: i32) -> f64 {
    format!("{mantissa}e{exponent}").parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};

    #[test]
    fn linear_ticks_use_nice_steps() {
        assert_eq!(linear(0.0, 1.0, 5), [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
        assert_eq!(
            linear(0.0, 10.0, 10),
            [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
        );
        assert_eq!(linear(-3.0, 7.0, 5), [-2.0, 0.0, 2.0, 4.0, 6.0]);
        assert_eq!(linear(0.13, 0.47, 4), [0.2, 0.3, 0.4]);
    }

    #[test]
    fn linear_ticks_are_free_of_float_junk() {
        let ticks = linear(0.1, 0.7, 6);
        assert_eq!(ticks, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]);
        let printed: Vec<String> = ticks.iter().map(f64::to_string).collect();
        assert_eq!(printed, ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6", "0.7"]);
        assert_eq!(linear(1e-9, 5e-9, 4), [1e-9, 2e-9, 3e-9, 4e-9, 5e-9]);
    }

    #[test]
    fn zero_never_prints_negative() {
        let ticks = linear(-1.0, 1.0, 3);
        assert!(ticks.contains(&0.0));
        assert!(ticks.iter().all(|t| t.to_string() != "-0"));
    }

    #[test]
    fn bounds_may_be_reversed() {
        assert_eq!(linear(1.0, 0.0, 5), linear(0.0, 1.0, 5));
        assert_eq!(logarithmic(1e3, 1.0, 2), logarithmic(1.0, 1e3, 2));
        assert_eq!(padded(2.0, 1.0, 0.0), (1.0, 2.0));
    }

    #[test]
    fn degenerate_range_is_widened_first() {
        assert_eq!(linear(5.0, 5.0, 3), [4.0, 5.0, 6.0]);
        assert_eq!(linear(0.0, 0.0, 3), [-1.0, 0.0, 1.0]);
    }

    #[test]
    fn invalid_input_gives_no_ticks() {
        assert!(linear(f64::NAN, 1.0, 5).is_empty());
        assert!(linear(0.0, f64::INFINITY, 5).is_empty());
        assert!(linear(0.0, 1.0, 0).is_empty());
        assert!(linear(-f64::MAX, f64::MAX, 5).is_empty());
        assert!(logarithmic(0.0, 10.0, 5).is_empty());
        assert!(logarithmic(-1.0, 10.0, 5).is_empty());
        assert!(logarithmic(10.0, 10.0, 5).is_empty());
        assert!(logarithmic(1.0, f64::INFINITY, 5).is_empty());
        assert!(logarithmic(f64::NAN, 10.0, 5).is_empty());
    }

    #[test]
    fn spans_below_float_resolution_stay_strictly_increasing() {
        let ticks = linear(1e16, 1e16 + 4.0, 10);
        assert!(!ticks.is_empty());
        assert!(ticks.windows(2).all(|w| w[0] < w[1]));
        assert!(ticks.iter().all(|t| (1e16..=1e16 + 4.0).contains(t)));
    }

    #[test]
    fn logarithmic_majors_are_decades() {
        let ticks = logarithmic(1.0, 1e4, 3);
        assert_eq!(
            ticks,
            [
                (1.0, true),
                (10.0, true),
                (100.0, true),
                (1e3, true),
                (1e4, true)
            ]
        );
    }

    #[test]
    fn logarithmic_minors_fill_short_ranges() {
        let ticks = logarithmic(15.0, 600.0, 5);
        assert_eq!(
            ticks,
            [
                (20.0, false),
                (50.0, false),
                (100.0, true),
                (200.0, false),
                (500.0, false)
            ]
        );
        let ticks = logarithmic(1e-12, 1e-11, 5);
        assert_eq!(
            ticks,
            [(1e-12, true), (2e-12, false), (5e-12, false), (1e-11, true)]
        );
    }

    #[test]
    fn decades_are_exact_across_the_whole_f64_range() {
        let ticks = logarithmic(1e-300, 1e300, 5);
        assert_eq!(ticks.len(), 601);
        for (value, major) in ticks {
            assert!(major);
            let printed = format!("{value:e}");
            assert!(printed.starts_with("1e"), "{printed}");
        }
    }

    #[test]
    fn padding() {
        assert_eq!(padded(0.0, 10.0, 0.1), (-1.0, 11.0));
        assert_eq!(padded(0.0, 10.0, -0.5), (0.0, 10.0));
        assert_eq!(padded(0.0, 10.0, f64::NAN), (0.0, 10.0));
        assert_eq!(padded(5.0, 5.0, 0.1), (4.0, 6.0));
        assert_eq!(padded(0.03, 0.03, 0.1), (0.03 - 0.01, 0.03 + 0.01));
        assert_eq!(padded(-250.0, -250.0, 0.1), (-350.0, -150.0));
        assert_eq!(padded(0.0, 0.0, 0.1), (-1.0, 1.0));
        assert_eq!(padded(f64::INFINITY, f64::NEG_INFINITY, 0.1), (-1.0, 1.0));
        assert_eq!(padded(-f64::MAX, f64::MAX, 0.1), (-f64::MAX, f64::MAX));
        let (low, high) = padded(f64::MAX, f64::MAX, 0.1);
        assert!(low.is_finite() && high.is_finite() && low < high);
    }

    fn config() -> Config {
        Config {
            cases: 500,
            rng_seed: RngSeed::Fixed(0x71C5),
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// Significant digits in the shortest round-trip representation.
    fn significant_digits(value: f64) -> usize {
        let printed = format!("{value:e}");
        let mantissa = printed.split('e').next().unwrap_or_default();
        mantissa.chars().filter(char::is_ascii_digit).count()
    }

    /// A range of moderate magnitude at a random scale.
    fn range() -> impl Strategy<Value = (f64, f64)> {
        (-1000.0f64..1000.0, 0.001f64..1000.0, -15i32..15).prop_map(|(start, width, scale)| {
            let factor = 10f64.powi(scale);
            (start * factor, (start + width) * factor)
        })
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn linear_ticks_are_inside_increasing_near_target_and_clean(
            (min, max) in range(),
            target in 2usize..12,
        ) {
            let ticks = linear(min, max, target);
            prop_assert!(ticks.iter().all(|t| min <= *t && *t <= max));
            prop_assert!(ticks.windows(2).all(|w| w[0] < w[1]));
            // Nice steps are at most 2.5× apart, so the closest one lands
            // within about a factor of two of the target.
            prop_assert!(ticks.len() * 2 + 1 >= target, "{} ticks for {}", ticks.len(), target);
            prop_assert!(ticks.len() <= 2 * target + 1, "{} ticks for {}", ticks.len(), target);
            // Ticks are short decimals (at most ~8 digits for these ranges);
            // snapping junk would show up as 16–17 digits.
            for t in &ticks {
                prop_assert!(significant_digits(*t) <= 10, "{:e}", t);
            }
            // Evenly spaced on one step.
            if ticks.len() >= 3 {
                let step = ticks[1] - ticks[0];
                for w in ticks.windows(2) {
                    prop_assert!(((w[1] - w[0]) - step).abs() <= 1e-9 * step.abs());
                }
            }
        }

        #[test]
        fn logarithmic_ticks_are_inside_increasing_and_clean(
            low_exponent in -20.0f64..20.0,
            decades in 0.05f64..12.0,
            target in 1usize..10,
        ) {
            let (min, max) = (10f64.powf(low_exponent), 10f64.powf(low_exponent + decades));
            let ticks = logarithmic(min, max, target);
            prop_assert!(ticks.iter().all(|(t, _)| min <= *t && *t <= max));
            prop_assert!(ticks.windows(2).all(|w| w[0].0 < w[1].0));
            for (t, major) in &ticks {
                let leading = format!("{t:e}").chars().next().unwrap_or_default();
                prop_assert_eq!(significant_digits(*t), 1);
                prop_assert_eq!(*major, leading == '1');
                prop_assert!(*major || decades < target as f64);
            }
        }

        #[test]
        fn padded_range_contains_the_input((min, max) in range(), fraction in 0.0f64..0.5) {
            let (low, high) = padded(min, max, fraction);
            prop_assert!(low <= min && max <= high);
            prop_assert!(low < high);
        }
    }
}
