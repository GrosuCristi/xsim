//! SI value parsing and engineering-notation formatting.
//!
//! Suffixes are case-sensitive (Q77): `m` is milli and `M` is mega.
//!
//! | Suffix | Factor | | Suffix | Factor |
//! |---|---|---|---|---|
//! | `f` | 1e-15 | | `k` | 1e3 |
//! | `p` | 1e-12 | | `M` | 1e6 |
//! | `n` | 1e-9  | | `G` | 1e9 |
//! | `u`, `µ`, `μ` | 1e-6 | | `T` | 1e12 |
//! | `m` | 1e-3  | | | |
//!
//! Plain numbers and exponents (`4.7e-6`) are accepted. A unit symbol may
//! follow the suffix and is ignored (`10kΩ`, `1uF`, `5V`, `2.2 mH`).
//!
//! The recognised unit symbols are `V`, `A`, `W`, `Ω` (or `Ω`, `ohm`, `Ohm`),
//! `F`, `H`, `s`, `Hz`, `S` and `°C`. Restricting units to this list is what
//! lets the parser reject typos such as `1kk` instead of silently reading
//! them as `1k`.
//!
//! Parsing is exact: the suffix is folded into the decimal exponent before
//! the single correctly rounded conversion to `f64`, so `"4.7k"` and
//! `"4700"` produce the same bits.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseValueError {
    pub input: String,
}

impl fmt::Display for ParseValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is not a valid value (examples: 4.7k, 100n, 2.2u, 1e-3)",
            self.input
        )
    }
}

impl std::error::Error for ParseValueError {}

/// SI prefixes with their decimal exponents, in the order used for parsing.
const PREFIXES: [(&str, i32); 11] = [
    ("f", -15),
    ("p", -12),
    ("n", -9),
    ("u", -6),
    ("µ", -6),
    ("μ", -6),
    ("m", -3),
    ("k", 3),
    ("M", 6),
    ("G", 9),
    ("T", 12),
];

/// Unit symbols that may follow the number or prefix and are ignored.
const UNITS: [&str; 13] = [
    "V", "A", "W", "Ω", "Ω", "ohm", "Ohm", "F", "H", "s", "Hz", "S", "°C",
];

/// Prefix symbols used for formatting, indexed by `(exponent + 15) / 3`.
const FORMAT_PREFIXES: [&str; 10] = ["f", "p", "n", "µ", "m", "", "k", "M", "G", "T"];
const SMALLEST_PREFIX_EXPONENT: i32 = -15;
const LARGEST_PREFIX_EXPONENT: i32 = 12;

/// Exponents beyond this magnitude overflow or underflow `f64` regardless of
/// the mantissa digits, so clamping keeps the arithmetic in range.
const EXPONENT_CLAMP: i64 = 100_000;

/// Parses `text` such as `"4.7k"`, `"100nF"`, `"1e-3"` or `"-2.5 mV"`.
///
/// Leading and trailing whitespace is ignored, and whitespace may separate
/// the number from its prefix or unit. The result must be finite.
pub fn parse_value(text: &str) -> Result<f64, ParseValueError> {
    let error = || ParseValueError {
        input: text.to_string(),
    };
    let trimmed = text.trim();
    let (number, rest) = split_number(trimmed).ok_or_else(error)?;
    let suffix = rest.trim_start();
    let (shift, unit) = split_prefix(suffix);
    if !unit.is_empty() && !is_unit(unit) {
        return Err(error());
    }
    let value = number.to_f64(shift);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(error())
    }
}

/// The numeric part of a value: sign, decimal digits and decimal exponent.
struct Number<'a> {
    negative: bool,
    /// Digits with the decimal point, as written (e.g. `"4.7"`, `".5"`).
    mantissa: &'a str,
    exponent: i64,
}

impl Number<'_> {
    /// Converts with one correct rounding after applying `shift` decimal
    /// places from the SI prefix.
    fn to_f64(&self, shift: i32) -> f64 {
        let exponent = (self.exponent + i64::from(shift)).clamp(-EXPONENT_CLAMP, EXPONENT_CLAMP);
        let sign = if self.negative { "-" } else { "" };
        // The mantissa was validated digit by digit, so this always parses.
        format!("{sign}{}e{exponent}", self.mantissa)
            .parse()
            .unwrap_or(f64::NAN)
    }
}

/// Splits a leading number (`[+-]digits[.digits][(e|E)[+-]digits]`) from
/// the rest of `text`. Returns `None` when there is no well-formed number.
fn split_number(text: &str) -> Option<(Number<'_>, &str)> {
    let bytes = text.as_bytes();
    let mut pos = 0;
    let negative = match bytes.first() {
        Some(b'-') => {
            pos = 1;
            true
        }
        Some(b'+') => {
            pos = 1;
            false
        }
        _ => false,
    };
    let mantissa_start = pos;
    let integer_digits = count_digits(&bytes[pos..]);
    pos += integer_digits;
    let mut fraction_digits = 0;
    if bytes.get(pos) == Some(&b'.') {
        fraction_digits = count_digits(&bytes[pos + 1..]);
        pos += 1 + fraction_digits;
    }
    if integer_digits + fraction_digits == 0 {
        return None;
    }
    let mantissa = &text[mantissa_start..pos];
    let mut exponent = 0_i64;
    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        let mut cursor = pos + 1;
        let exponent_negative = match bytes.get(cursor) {
            Some(b'-') => {
                cursor += 1;
                true
            }
            Some(b'+') => {
                cursor += 1;
                false
            }
            _ => false,
        };
        let digits = count_digits(&bytes[cursor..]);
        if digits == 0 {
            // "1e", "1e+" and "1E-" are incomplete exponents, not units.
            return None;
        }
        let magnitude = text[cursor..cursor + digits].bytes().fold(0_i64, |acc, d| {
            (acc * 10 + i64::from(d - b'0')).min(EXPONENT_CLAMP)
        });
        exponent = if exponent_negative {
            -magnitude
        } else {
            magnitude
        };
        pos = cursor + digits;
    }
    Some((
        Number {
            negative,
            mantissa,
            exponent,
        },
        &text[pos..],
    ))
}

fn count_digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// Splits an optional SI prefix from `suffix`, returning its decimal shift
/// and the remaining unit text. A prefix letter that is itself a complete
/// unit (`F`, `S`...) never collides because no prefix is also a unit.
fn split_prefix(suffix: &str) -> (i32, &str) {
    for (symbol, shift) in PREFIXES {
        if let Some(unit) = suffix.strip_prefix(symbol) {
            return (shift, unit);
        }
    }
    (0, suffix)
}

fn is_unit(text: &str) -> bool {
    UNITS.contains(&text)
}

/// Formats `value` in engineering notation with up to 4 significant digits
/// and the SI prefix, followed by `unit`: `format_value(4700.0, "Ω")` is
/// `"4.7 kΩ"`, `format_value(0.0, "V")` is `"0 V"`. With an empty unit the
/// space is omitted: `"4.7k"`. Non-finite values format as `"NaN"`, `"∞"`
/// or `"-∞"`.
pub fn format_value(value: f64, unit: &str) -> String {
    format_value_digits(value, unit, 4)
}

/// [`format_value`] with a chosen number of significant digits (1..=15).
/// Trailing zeros after the decimal point are removed.
///
/// Values outside the prefix range (below 1 f or from 1000 T upwards) use a
/// plain exponent instead (`"1.5e-18 V"`), which [`parse_value`] accepts.
/// Out-of-range `digits` are clamped to `1..=15`.
pub fn format_value_digits(value: f64, unit: &str, digits: usize) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "∞" } else { "-∞" }.to_string();
    }
    let digits = digits.clamp(1, 15);
    let (number, prefix) = if value == 0.0 {
        ("0".to_string(), "")
    } else {
        engineering(value, digits)
    };
    if unit.is_empty() {
        format!("{number}{prefix}")
    } else {
        format!("{number} {prefix}{unit}")
    }
}

/// Rounds `value` (finite, non-zero) to `digits` significant digits and
/// splits it into a mantissa string and an SI prefix.
fn engineering(value: f64, digits: usize) -> (String, &'static str) {
    // `{:e}` rounds correctly, and doing it first means a value such as
    // 999.96 that rounds up to 1000 moves to the next prefix ("1k").
    let scientific = format!("{:.*e}", digits - 1, value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digit_string: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if value < 0.0 { "-" } else { "" };

    let prefix_exponent = exponent.div_euclid(3) * 3;
    if !(SMALLEST_PREFIX_EXPONENT..=LARGEST_PREFIX_EXPONENT).contains(&prefix_exponent) {
        let mantissa = trim_fraction(mantissa);
        return (format!("{sign}{mantissa}e{exponent}"), "");
    }
    // Place the decimal point after `1 + exponent - prefix_exponent` digits.
    let integer_len = (exponent - prefix_exponent + 1) as usize;
    let padded = format!("{digit_string:0<integer_len$}");
    let (integer, fraction) = padded.split_at(integer_len);
    let number = trim_fraction(&format!("{integer}.{fraction}")).to_string();
    let prefix = FORMAT_PREFIXES[((prefix_exponent - SMALLEST_PREFIX_EXPONENT) / 3) as usize];
    (format!("{sign}{number}"), prefix)
}

/// Removes trailing zeros after a decimal point, and the point itself.
fn trim_fraction(number: &str) -> &str {
    if number.contains('.') {
        number.trim_end_matches('0').trim_end_matches('.')
    } else {
        number
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn parse(text: &str) -> f64 {
        parse_value(text).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn parses_plain_numbers_and_exponents() {
        assert_eq!(parse("0"), 0.0);
        assert_eq!(parse("42"), 42.0);
        assert_eq!(parse("-2.5"), -2.5);
        assert_eq!(parse("+3"), 3.0);
        assert_eq!(parse(".5"), 0.5);
        assert_eq!(parse("5."), 5.0);
        assert_eq!(parse("4.7e-6"), 4.7e-6);
        assert_eq!(parse("1E3"), 1000.0);
        assert_eq!(parse("1e+3"), 1000.0);
        assert_eq!(parse("  12  "), 12.0);
    }

    #[test]
    fn suffixes_are_case_sensitive() {
        assert_eq!(parse("1m"), 1e-3);
        assert_eq!(parse("1M"), 1e6);
        assert_eq!(parse("1f"), 1e-15);
        assert_eq!(parse("1p"), 1e-12);
        assert_eq!(parse("1n"), 1e-9);
        assert_eq!(parse("1k"), 1e3);
        assert_eq!(parse("1G"), 1e9);
        assert_eq!(parse("1T"), 1e12);
        assert!(parse_value("1K").is_err());
        assert!(parse_value("1g").is_err());
    }

    #[test]
    fn all_micro_spellings_are_equivalent() {
        for text in ["2.2u", "2.2µ", "2.2μ", "2.2 uF", "2.2µF", "2.2 μF"] {
            assert_eq!(parse(text), 2.2e-6, "{text}");
        }
    }

    #[test]
    fn prefix_parsing_is_correctly_rounded() {
        // Folding the prefix into the exponent avoids a second rounding.
        assert_eq!(parse("4.7k"), "4700".parse::<f64>().unwrap());
        assert_eq!(parse("100n"), "1e-7".parse::<f64>().unwrap());
        assert_eq!(parse("0.1u"), "1e-7".parse::<f64>().unwrap());
        assert_eq!(parse("3.3m"), "0.0033".parse::<f64>().unwrap());
        assert_eq!(parse("1e3k"), 1e6);
    }

    #[test]
    fn units_are_ignored_after_the_prefix() {
        assert_eq!(parse("10kΩ"), 1e4);
        assert_eq!(parse("10 kΩ"), 1e4);
        assert_eq!(parse("10kΩ"), 1e4);
        assert_eq!(parse("10kohm"), 1e4);
        assert_eq!(parse("1uF"), 1e-6);
        assert_eq!(parse("5V"), 5.0);
        assert_eq!(parse("2.2 mH"), 2.2e-3);
        assert_eq!(parse("-2.5 mV"), -2.5e-3);
        assert_eq!(parse("3 A"), 3.0);
        assert_eq!(parse("1MHz"), 1e6);
        assert_eq!(parse("10ms"), 1e-2);
        assert_eq!(parse("1s"), 1.0);
        assert_eq!(parse("1fF"), 1e-15);
        assert_eq!(parse("1F"), 1.0);
        assert_eq!(parse("2S"), 2.0);
        assert_eq!(parse("27°C"), 27.0);
        assert_eq!(parse("5 mW"), 5e-3);
    }

    #[test]
    fn rejects_malformed_values() {
        for text in [
            "", " ", "k", "1kk", "1e", "1e+", "E3", "1.2.3", "--1", "+-1", ".", "-.", "1 e3",
            "abc", "1x", "1 k Ω", "inf", "NaN", "infinity", "1e400", "1kV2", "1 2", "Meg", "1Meg",
            "1mm", "0x10",
        ] {
            assert!(parse_value(text).is_err(), "{text:?} should be rejected");
        }
    }

    #[test]
    fn huge_exponents_do_not_overflow_the_parser() {
        assert!(parse_value("1e99999999999999999999999").is_err());
        assert_eq!(parse("1e-99999999999999999999999"), 0.0);
    }

    #[test]
    fn error_message_quotes_the_input() {
        let message = parse_value("1kk").unwrap_err().to_string();
        assert!(message.contains("\"1kk\""), "{message}");
    }

    #[test]
    fn formats_with_prefixes_and_units() {
        assert_eq!(format_value(4700.0, "Ω"), "4.7 kΩ");
        assert_eq!(format_value(4700.0, ""), "4.7k");
        assert_eq!(format_value(0.0, "V"), "0 V");
        assert_eq!(format_value(-0.0, ""), "0");
        assert_eq!(format_value(1.0, "V"), "1 V");
        assert_eq!(format_value(1e-6, "F"), "1 µF");
        assert_eq!(format_value(-2.5e-3, "V"), "-2.5 mV");
        assert_eq!(format_value(1e12, "Ω"), "1 TΩ");
        assert_eq!(format_value(1.5e-15, "F"), "1.5 fF");
        assert_eq!(format_value(123_456.0, ""), "123.5k");
        assert_eq!(format_value(0.1, "s"), "100 ms");
    }

    #[test]
    fn rounding_moves_to_the_next_prefix() {
        assert_eq!(format_value(999.96, ""), "1k");
        assert_eq!(format_value(999_960.0, "Hz"), "1 MHz");
        assert_eq!(format_value(0.999_96, "V"), "1 V");
        assert_eq!(format_value(999.94, ""), "999.9");
    }

    #[test]
    fn digits_control_significant_figures() {
        assert_eq!(format_value_digits(std::f64::consts::PI, "", 1), "3");
        assert_eq!(format_value_digits(std::f64::consts::PI, "", 6), "3.14159");
        assert_eq!(format_value_digits(1234.5678, "", 8), "1.2345678k");
        assert_eq!(format_value_digits(1.5, "", 0), "2");
        assert_eq!(format_value_digits(0.1 + 0.2, "", 99), "300m");
    }

    /// Largest relative change of a value written with fifteen significant
    /// digits and parsed back: half a unit of the fifteenth digit of a
    /// mantissa of at least 1 (5e-15), plus the parser's own rounding to the
    /// nearest f64 and the f64 value's distance from its shortest decimal.
    const FIFTEEN_DIGITS: f64 = 5e-15 + 2.0 * f64::EPSILON;

    #[test]
    fn fifteen_digits_round_trip_within_half_a_unit_of_the_last_digit() {
        // Found by `full_precision_round_trip` (bits 17749941517621329968):
        // "-1.00665918253425e262" parses back 5.04e-15 away, beyond the 5e-15
        // that bound allowed without the parser's rounding.
        let value = -1.006_659_182_534_245e262;
        let text = format_value_digits(value, "", 15);
        assert_eq!(text, "-1.00665918253425e262");
        let parsed = parse_value(&text).unwrap();
        let change = (parsed - value).abs() / value.abs();
        assert!(change > 5e-15 && change <= FIFTEEN_DIGITS, "{change:e}");
    }

    #[test]
    fn values_outside_the_prefix_range_use_exponents() {
        assert_eq!(format_value(1.5e-18, "V"), "1.5e-18 V");
        assert_eq!(format_value(2e15, ""), "2e15");
        assert_eq!(format_value(-1e300, "A"), "-1e300 A");
        assert_eq!(parse("1.5e-18 V"), 1.5e-18);
    }

    #[test]
    fn non_finite_values_have_symbols() {
        assert_eq!(format_value(f64::NAN, "V"), "NaN");
        assert_eq!(format_value(f64::INFINITY, "V"), "∞");
        assert_eq!(format_value(f64::NEG_INFINITY, ""), "-∞");
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 2_000,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        /// Formatting then parsing returns the value to within the rounding
        /// of the requested number of significant digits.
        #[test]
        fn format_parse_round_trip(
            mantissa in 1.0_f64..10.0,
            exponent in -30_i32..30,
            negative in any::<bool>(),
            digits in 1_usize..=15,
            unit in prop::sample::select(vec!["", "V", "A", "Ω", "F", "H", "s", "Hz"]),
        ) {
            let magnitude = mantissa * 10f64.powi(exponent);
            let value = if negative { -magnitude } else { magnitude };
            let text = format_value_digits(value, unit, digits);
            let parsed = parse_value(&text).unwrap();
            let allowed = 0.5 * 10f64.powi(1 - digits as i32) * value.abs() * (1.0 + 1e-12);
            prop_assert!(
                (parsed - value).abs() <= allowed,
                "{value} -> {text:?} -> {parsed}"
            );
        }

        /// Fifteen significant digits round-trip through the parser exactly
        /// enough for any f64 to land within one part in 1e14.
        #[test]
        fn full_precision_round_trip(bits in any::<u64>()) {
            let value = f64::from_bits(bits);
            // Near f64::MAX the 15-digit decimal can round up past it.
            prop_assume!(value.is_finite() && value != 0.0 && value.abs() < 1e300);
            let parsed = parse_value(&format_value_digits(value, "", 15)).unwrap();
            prop_assert!((parsed - value).abs() <= FIFTEEN_DIGITS * value.abs());
        }
    }
}
