//! Fuzz and property tests for trace expressions (Q156).
//!
//! Arbitrary text must never panic the parser or the evaluator, errors must
//! point inside the text, and well-formed expressions must evaluate exactly
//! like the arithmetic they denote.

mod common;

use common::{ac, proptest_config, transient};
use num_complex::Complex64;
use proptest::prelude::*;
use xsim_spice_engine::SimulationResult;
use xsim_waveform::{ExprError, Expression, Series};

const TIME: [f64; 6] = [0.0, 1e-6, 3e-6, 3.5e-6, 7e-6, 1e-5];
const V_OUT: [f64; 6] = [0.0, 0.5, -1.25, 2.0, 1e-9, -3.0];
const V_IN: [f64; 6] = [1.0, 1.0, 0.0, -0.5, 4.0, 1e3];
const I_R1: [f64; 6] = [1e-3, -2e-3, 0.0, 5e-4, 1.5, -0.25];

fn transient_result() -> SimulationResult {
    transient(
        TIME.to_vec(),
        &[
            ("V(out)", V_OUT.to_vec()),
            ("V(in)", V_IN.to_vec()),
            ("I(R1)", I_R1.to_vec()),
        ],
    )
}

fn ac_result() -> SimulationResult {
    let complex = |values: [f64; 6]| -> Vec<Complex64> {
        values
            .iter()
            .enumerate()
            .map(|(i, &re)| Complex64::new(re, 0.5 - i as f64))
            .collect()
    };
    ac(
        vec![1.0, 10.0, 100.0, 1e3, 1e4, 1e5],
        &[
            ("V(out)", complex(V_OUT)),
            ("V(in)", complex(V_IN)),
            ("I(R1)", complex(I_R1)),
        ],
    )
}

/// Parses and evaluates `text` against both results, checking every
/// invariant that must hold whatever the input.
fn check_robustness(text: &str) -> Result<(), TestCaseError> {
    match Expression::parse(text) {
        Ok(expression) => {
            prop_assert_eq!(expression.text(), text);
            let signals = expression.signals();
            prop_assert!(signals.windows(2).all(|w| w[0] < w[1]), "{:?}", signals);
            let _ = expression.unit();
            for result in [transient_result(), ac_result()] {
                if let Ok(series) = expression.evaluate(&result) {
                    prop_assert_eq!(series.len(), result.len());
                }
            }
        }
        Err(ExprError::Syntax { position, message }) => {
            prop_assert!(position <= text.len(), "{} beyond {:?}", position, text);
            prop_assert!(
                text.is_char_boundary(position),
                "{} inside a character",
                position
            );
            prop_assert!(!message.is_empty());
        }
        Err(_) => {}
    }
    Ok(())
}

/// Fragments that steer random sequences into every corner of the grammar.
const VOCABULARY: [&str; 52] = [
    "V(out)",
    "V(in)",
    "V(out, in)",
    "V(0)",
    "V(nope)",
    "V(",
    "V",
    "I(R1)",
    "Ic(Q1)",
    "I(",
    "time",
    "freq",
    "0",
    "1",
    "2.5",
    ".5",
    "1e-3",
    "1e",
    "4.7k",
    "10m",
    "1M",
    "2meg",
    "5V",
    "1e400",
    "+",
    "-",
    "*",
    "/",
    "^",
    "(",
    ")",
    ",",
    "abs",
    "sqrt",
    "exp",
    "ln",
    "log10",
    "sin",
    "cos",
    "tan",
    "atan",
    "min",
    "max",
    "db",
    "mag",
    "ph",
    "re",
    "im",
    "deriv",
    "integ",
    "foo",
    "µ",
];

fn token_sequence() -> impl Strategy<Value = String> {
    let token = prop::sample::select(&VOCABULARY[..]);
    (prop::collection::vec(token, 0..24), any::<bool>())
        .prop_map(|(tokens, spaced)| tokens.join(if spaced { " " } else { "" }))
}

proptest! {
    #![proptest_config(proptest_config(2048))]

    #[test]
    fn arbitrary_unicode_never_panics(text in "\\PC{0,64}") {
        check_robustness(&text)?;
    }

    #[test]
    fn arbitrary_characters_never_panic(chars in prop::collection::vec(any::<char>(), 0..48)) {
        check_robustness(&chars.into_iter().collect::<String>())?;
    }

    #[test]
    fn expression_alphabet_never_panics(text in "[-+*/^(),. 0-9eEkmMuµnpfGT_a-zA-Z]{0,48}") {
        check_robustness(&text)?;
    }

    #[test]
    fn token_sequences_never_panic(text in token_sequence()) {
        check_robustness(&text)?;
    }
}

/// A well-formed expression over the transient result, generated together
/// with the arithmetic it denotes.
#[derive(Debug, Clone)]
enum Tree {
    Number(f64),
    Signal(&'static str),
    Negate(Box<Tree>),
    Binary(char, Box<Tree>, Box<Tree>),
    Unary(&'static str, Box<Tree>),
    Extremum(&'static str, Box<Tree>, Box<Tree>),
}

const UNARY: [&str; 14] = [
    "abs", "sqrt", "exp", "ln", "log10", "sin", "cos", "tan", "atan", "db", "mag", "ph", "re", "im",
];

impl Tree {
    /// Source text; `spaced` puts whitespace between every token. Negation
    /// is parenthesized because `-a^b` means `-(a^b)`.
    fn text(&self, spaced: bool) -> String {
        let gap = if spaced { " " } else { "" };
        match self {
            Self::Number(value) => format!("{value:?}"),
            Self::Signal(name) => (*name).to_string(),
            Self::Negate(operand) => format!("({gap}-{gap}{}{gap})", operand.text(spaced)),
            Self::Binary(op, a, b) => format!(
                "({gap}{}{gap}{op}{gap}{}{gap})",
                a.text(spaced),
                b.text(spaced)
            ),
            Self::Unary(name, a) => format!("{name}({gap}{}{gap})", a.text(spaced)),
            Self::Extremum(name, a, b) => {
                format!(
                    "{name}({gap}{}{gap},{gap}{}{gap})",
                    a.text(spaced),
                    b.text(spaced)
                )
            }
        }
    }

    /// The value at sample `i`, computed with the same `f64` operations the
    /// documented semantics prescribe.
    fn value(&self, i: usize) -> f64 {
        match self {
            Self::Number(value) => *value,
            Self::Signal(name) => match *name {
                "V(out)" => V_OUT[i],
                "V(in)" => V_IN[i],
                "I(R1)" => I_R1[i],
                "V(out, in)" => V_OUT[i] - V_IN[i],
                _ => TIME[i],
            },
            Self::Negate(operand) => -operand.value(i),
            Self::Binary(op, a, b) => {
                let (a, b) = (a.value(i), b.value(i));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => a.powf(b),
                }
            }
            Self::Unary(name, a) => {
                let a = a.value(i);
                match *name {
                    "abs" | "mag" => a.abs(),
                    "sqrt" => a.sqrt(),
                    "exp" => a.exp(),
                    "ln" => a.ln(),
                    "log10" => a.log10(),
                    "sin" => a.sin(),
                    "cos" => a.cos(),
                    "tan" => a.tan(),
                    "atan" => a.atan(),
                    "db" => 20.0 * a.abs().log10(),
                    "ph" => 0.0f64.atan2(a).to_degrees(),
                    "re" => a,
                    _ => 0.0,
                }
            }
            // Ties and NaN keep the first argument.
            Self::Extremum(name, a, b) => {
                let (a, b) = (a.value(i), b.value(i));
                let prefer_b = if *name == "min" { b < a } else { b > a };
                if prefer_b { b } else { a }
            }
        }
    }

    fn signals(&self, names: &mut Vec<String>) {
        match self {
            Self::Number(_) => {}
            Self::Signal("V(out, in)") => names.extend(["V(out)".into(), "V(in)".into()]),
            Self::Signal("time") => {}
            Self::Signal(name) => names.push((*name).to_string()),
            Self::Negate(a) | Self::Unary(_, a) => a.signals(names),
            Self::Binary(_, a, b) | Self::Extremum(_, a, b) => {
                a.signals(names);
                b.signals(names);
            }
        }
    }
}

fn tree() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        prop_oneof![
            Just(0.0),
            Just(1.0),
            Just(2.5),
            Just(1e-3),
            Just(1e6),
            0.0f64..100.0
        ]
        .prop_map(Tree::Number),
        prop::sample::select(&["V(out)", "V(in)", "I(R1)", "V(out, in)", "time"][..])
            .prop_map(Tree::Signal),
    ];
    leaf.prop_recursive(5, 48, 2, |inner| {
        prop_oneof![
            inner.clone().prop_map(|a| Tree::Negate(Box::new(a))),
            (
                prop::sample::select(&['+', '-', '*', '/', '^'][..]),
                inner.clone(),
                inner.clone()
            )
                .prop_map(|(op, a, b)| Tree::Binary(op, Box::new(a), Box::new(b))),
            (prop::sample::select(&UNARY[..]), inner.clone())
                .prop_map(|(name, a)| Tree::Unary(name, Box::new(a))),
            (
                prop::sample::select(&["min", "max"][..]),
                inner.clone(),
                inner
            )
                .prop_map(|(name, a, b)| Tree::Extremum(
                    name,
                    Box::new(a),
                    Box::new(b)
                )),
        ]
    })
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

proptest! {
    #![proptest_config(proptest_config(256))]

    /// Generated expressions parse, list exactly their signals, and
    /// evaluate bit for bit like the reference arithmetic, with or without
    /// whitespace between tokens.
    #[test]
    fn well_formed_expressions_evaluate_exactly(tree in tree()) {
        let result = transient_result();
        let mut expected_signals = Vec::new();
        tree.signals(&mut expected_signals);
        expected_signals.sort();
        expected_signals.dedup();

        for spaced in [false, true] {
            let text = tree.text(spaced);
            let expression = Expression::parse(&text)
                .map_err(|e| TestCaseError::fail(format!("{text}: {e}")))?;
            prop_assert_eq!(expression.signals(), expected_signals.clone());
            let Series::Real(values) = expression
                .evaluate(&result)
                .map_err(|e| TestCaseError::fail(format!("{text}: {e}")))?
            else {
                return Err(TestCaseError::fail("a transient expression must be real"));
            };
            for (i, &value) in values.iter().enumerate() {
                let reference = tree.value(i);
                prop_assert!(same(value, reference), "{} at {}: {} vs {}", text, i, value, reference);
            }
        }
    }

    /// In AC results a value is complex exactly when it depends on signal
    /// data outside a real-valued function.
    #[test]
    fn ac_values_are_complex_only_when_derived_from_signals(tree in tree()) {
        fn is_complex(tree: &Tree) -> bool {
            match tree {
                Tree::Number(_) => false,
                Tree::Signal(name) => *name != "time",
                Tree::Negate(a) => is_complex(a),
                Tree::Binary(_, a, b) | Tree::Extremum(_, a, b) => is_complex(a) || is_complex(b),
                Tree::Unary(name, a) => {
                    !["abs", "mag", "db", "ph", "re", "im"].contains(name) && is_complex(a)
                }
            }
        }
        // `time` is not defined for AC results, so use `freq` in its place.
        let text = tree.text(true).replace("time", "freq");
        let expression = Expression::parse(&text).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let series = expression
            .evaluate(&ac_result())
            .map_err(|e| TestCaseError::fail(format!("{text}: {e}")))?;
        prop_assert_eq!(matches!(series, Series::Complex(_)), is_complex(&tree), "{}", text);
    }
}
