//! Trace expressions evaluated against hand-built results (Q132).

mod common;

use common::{ac, assert_close, dc_sweep, operating_point, transient, uneven_grid};
use num_complex::Complex64;
use std::f64::consts::PI;
use xsim_spice_engine::SimulationResult;
use xsim_waveform::{ExprError, Expression, Series};

fn evaluate(source: &str, result: &SimulationResult) -> Series {
    Expression::parse(source)
        .unwrap_or_else(|e| panic!("{source}: {e}"))
        .evaluate(result)
        .unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn real(source: &str, result: &SimulationResult) -> Vec<f64> {
    match evaluate(source, result) {
        Series::Real(values) => values,
        Series::Complex(_) => panic!("{source}: expected a real series"),
    }
}

fn complex(source: &str, result: &SimulationResult) -> Vec<Complex64> {
    match evaluate(source, result) {
        Series::Complex(values) => values,
        Series::Real(_) => panic!("{source}: expected a complex series"),
    }
}

fn evaluation_error(source: &str, result: &SimulationResult) -> ExprError {
    match Expression::parse(source)
        .expect("valid syntax")
        .evaluate(result)
    {
        Err(error) => error,
        Ok(series) => panic!("{source}: expected an error, got {series:?}"),
    }
}

/// A 2:1 resistive divider driven by a 1 V/s ramp: `V(in) = t`,
/// `V(out) = t/2` and `I(R1) = t/2 kΩ`.
fn divider() -> SimulationResult {
    let time = vec![0.0, 1e-3, 2.5e-3, 4e-3, 7e-3];
    let v_out = time.iter().map(|t| t / 2.0).collect();
    let i_r1 = time.iter().map(|t| t / 2e3).collect();
    transient(
        time.clone(),
        &[("V(in)", time), ("V(out)", v_out), ("I(R1)", i_r1)],
    )
}

/// First-order RC low-pass with a 1 kHz corner: `V(in) = 1`,
/// `V(out) = 1/(1 + j·f/fc)`.
const CORNER: f64 = 1e3;

fn low_pass() -> SimulationResult {
    let frequency = vec![10.0, 100.0, CORNER, 1e4, 1e5];
    let v_out = frequency
        .iter()
        .map(|f| Complex64::from(1.0) / Complex64::new(1.0, f / CORNER))
        .collect();
    let v_in = vec![Complex64::from(1.0); frequency.len()];
    ac(frequency, &[("V(in)", v_in), ("V(out)", v_out)])
}

#[test]
fn signal_arithmetic_from_q132() {
    let result = divider();
    let time = &result.axis.values;
    let difference = real("V(out)-V(in)", &result);
    let power = real("V(out)*I(R1)", &result);
    for (i, t) in time.iter().enumerate() {
        assert_eq!(difference[i], t / 2.0 - t);
        assert_eq!(power[i], (t / 2.0) * (t / 2e3));
    }
    assert_eq!(Expression::parse("V(out)*I(R1)").unwrap().unit(), "W");
    assert_eq!(Expression::parse("V(out)-V(in)").unwrap().unit(), "V");
}

#[test]
fn operator_precedence_in_evaluation() {
    let result = divider();
    let values = real("1 + 2 * 3 ^ 2 - -4 / 2", &result);
    assert!(values.iter().all(|&v| v == 1.0 + 2.0 * 9.0 + 2.0));
    let values = real("2 ^ 3 ^ 2", &result);
    assert!(values.iter().all(|&v| v == 512.0));
    let values = real("-2 ^ 2", &result);
    assert!(values.iter().all(|&v| v == -4.0));
}

#[test]
fn differential_probes_and_ground() {
    let result = divider();
    let v_in = real("V(in)", &result);
    let v_out = real("V(out)", &result);
    let differential = real("V(out, in)", &result);
    for i in 0..v_in.len() {
        assert_eq!(differential[i], v_out[i] - v_in[i]);
    }
    assert_eq!(real("V(out, 0)", &result), v_out);
    assert_eq!(real("V(0)", &result), vec![0.0; v_in.len()]);
    let negated: Vec<f64> = v_out.iter().map(|v| 0.0 - v).collect();
    assert_eq!(real("V(0, out)", &result), negated);
}

#[test]
fn si_suffixes_are_case_sensitive() {
    let result = operating_point(&[("V(out)", 1.0)]);
    // Each literal must equal the correctly rounded decimal it denotes.
    for (literal, decimal) in [
        ("1m", "1e-3"),
        ("1M", "1e6"),
        ("4.7k", "4.7e3"),
        ("2.2u", "2.2e-6"),
        ("1µ", "1e-6"),
        ("1μ", "1e-6"),
        ("10n", "10e-9"),
        ("3p", "3e-12"),
        ("5f", "5e-15"),
        ("2G", "2e9"),
        ("1T", "1e12"),
        ("1.5e3k", "1.5e6"),
    ] {
        let expected: f64 = decimal.parse().unwrap();
        assert_eq!(real(literal, &result), [expected], "{literal}");
    }
    assert_eq!(real("V(out)*10m", &result), [1.0 * 10e-3]);
}

#[test]
fn axis_variables_match_the_analysis() {
    let transient = divider();
    assert_eq!(real("time", &transient), transient.axis.values);
    assert_eq!(Expression::parse("time").unwrap().unit(), "s");

    let ac = low_pass();
    assert_eq!(real("freq", &ac), ac.axis.values, "freq is real even in AC");
    assert_eq!(Expression::parse("freq").unwrap().unit(), "Hz");

    let sweep = dc_sweep("V1", vec![0.0, 1.0], &[("V(out)", vec![0.0, 0.5])]);
    for (source, result, position) in [
        ("V(in) + freq", &transient, 8),
        ("2*time", &ac, 2),
        ("time", &sweep, 0),
        ("freq", &sweep, 0),
    ] {
        match evaluation_error(source, result) {
            ExprError::Syntax {
                position: at,
                message,
            } => {
                assert_eq!(at, position, "{source}");
                assert!(message.contains("only available"), "{message}");
            }
            other => panic!("{source}: unexpected error {other:?}"),
        }
    }
}

#[test]
fn missing_signals_are_named() {
    let result = divider();
    for (source, missing) in [
        ("V(nope)", "V(nope)"),
        ("V(out, ref)", "V(ref)"),
        ("I(R9) * 2", "I(R9)"),
        ("Ic(Q1)", "Ic(Q1)"),
        // Node names are case-sensitive.
        ("V(OUT)", "V(OUT)"),
    ] {
        assert_eq!(
            evaluation_error(source, &result),
            ExprError::UnknownSignal(missing.into()),
            "{source}"
        );
    }
}

#[test]
fn ac_transfer_function_in_complex_arithmetic() {
    let result = low_pass();
    let frequency = &result.axis.values;
    let gain = complex("V(out)/V(in)", &result);
    let db = real("db(V(out)/V(in))", &result);
    let db_difference = real("db(V(out)) - db(V(in))", &result);
    let phase = real("ph(V(out))", &result);
    let magnitude = real("mag(V(out))", &result);
    let absolute = real("abs(V(out))", &result);
    let re = real("re(V(out))", &result);
    let im = real("im(V(out))", &result);
    for (i, f) in frequency.iter().enumerate() {
        let ratio = f / CORNER;
        let expected = Complex64::from(1.0) / Complex64::new(1.0, ratio);
        assert!((gain[i] - expected).norm() < 1e-15);
        let expected_db = -10.0 * (1.0 + ratio * ratio).log10();
        assert_close(db[i], expected_db, 1e-12, "db");
        assert_close(db_difference[i], expected_db, 1e-12, "db difference");
        assert_close(phase[i], -ratio.atan().to_degrees(), 1e-12, "phase");
        assert_close(
            magnitude[i],
            1.0 / (1.0 + ratio * ratio).sqrt(),
            1e-15,
            "magnitude",
        );
        assert_eq!(absolute[i], magnitude[i]);
        assert_close(re[i], 1.0 / (1.0 + ratio * ratio), 1e-15, "re");
        assert_close(im[i], -ratio / (1.0 + ratio * ratio), 1e-15, "im");
    }
    // At the corner the gain is 1/√2: -3.0103 dB and -45°.
    assert_close(db[2], 20.0 * (0.5f64.sqrt()).log10(), 1e-12, "corner gain");
    assert_close(phase[2], -45.0, 1e-12, "corner phase");
    assert_eq!(Expression::parse("db(V(out)/V(in))").unwrap().unit(), "dB");
    assert_eq!(Expression::parse("ph(V(out))").unwrap().unit(), "°");
}

#[test]
fn ac_mixes_real_and_complex_operands() {
    let result = low_pass();
    let v_out = complex("V(out)", &result);
    let shifted = complex("(V(out) + 1)^2 * freq", &result);
    let scaled = complex("2 * V(out)", &result);
    for (i, f) in result.axis.values.iter().enumerate() {
        let expected = (v_out[i] + 1.0) * (v_out[i] + 1.0) * *f;
        assert!((shifted[i] - expected).norm() <= 1e-12 * expected.norm());
        assert_eq!(scaled[i], v_out[i] * 2.0);
    }
    // Comparisons on complex values use the real part; ties keep the first
    // argument (at the corner Re V(out) is exactly 0.5).
    let larger = complex("max(V(in), V(out))", &result);
    let smaller = complex("min(V(out), 0.5)", &result);
    for i in 0..v_out.len() {
        assert_eq!(larger[i], Complex64::from(1.0), "Re V(out) <= 1");
        let expected = if v_out[i].re <= 0.5 {
            v_out[i]
        } else {
            Complex64::from(0.5)
        };
        assert_eq!(smaller[i], expected);
    }
}

#[test]
fn deriv_and_integ_on_an_uneven_transient_grid() {
    let omega = 2.0 * PI * 1e3;
    let stop = 2e-3;
    let time = uneven_grid(0.0, stop, 2001);
    let max_step = time.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
    let v: Vec<f64> = time.iter().map(|t| (omega * t).sin()).collect();
    let result = transient(time.clone(), &[("V(out)", v)]);

    // Second-order differences: the error is bounded by h²·max|y'''|, with
    // y''' = ω³ for this sine.
    let derivative = real("deriv(V(out))", &result);
    let derivative_tolerance = max_step * max_step * omega.powi(3);
    for (t, d) in time.iter().zip(&derivative) {
        assert_close(*d, omega * (omega * t).cos(), derivative_tolerance, "deriv");
    }

    // Composite trapezoid: |error| <= (b − a)·h²·max|y''|/12, y'' = ω²·sin.
    let integral = real("integ(V(out))", &result);
    let integral_tolerance = stop * max_step * max_step * omega * omega / 12.0;
    for (t, value) in time.iter().zip(&integral) {
        assert_close(
            *value,
            (1.0 - (omega * t).cos()) / omega,
            integral_tolerance,
            "integ",
        );
    }
    assert_eq!(integral[0], 0.0);
}

/// An expression and the `f64` function it must apply to every sample.
type FunctionCase = (&'static str, fn(f64) -> f64);

#[test]
fn elementary_functions_on_a_dc_sweep() {
    let sweep: Vec<f64> = (1..=20).map(|k| f64::from(k) * 0.1).collect();
    let result = dc_sweep("V1", sweep.clone(), &[("V(x)", sweep.clone())]);
    let cases: [FunctionCase; 12] = [
        ("abs(-V(x))", |x| x),
        ("sqrt(V(x))", f64::sqrt),
        ("exp(V(x))", f64::exp),
        ("ln(V(x))", f64::ln),
        ("log10(V(x))", f64::log10),
        ("sin(V(x))", f64::sin),
        ("cos(V(x))", f64::cos),
        ("tan(V(x))", f64::tan),
        ("atan(V(x))", f64::atan),
        ("min(V(x), 1)", |x| x.min(1.0)),
        ("max(V(x), 1)", |x| x.max(1.0)),
        ("db(V(x))", |x| 20.0 * x.log10()),
    ];
    for (source, function) in cases {
        let expected: Vec<f64> = sweep.iter().map(|&x| function(x)).collect();
        assert_eq!(real(source, &result), expected, "{source}");
    }
    assert_eq!(real("ph(-V(x))", &result), vec![180.0; sweep.len()]);
    assert_eq!(real("im(V(x))", &result), vec![0.0; sweep.len()]);
    assert!(real("sqrt(-V(x))", &result).iter().all(|v| v.is_nan()));
}

#[test]
fn operating_point_has_one_sample() {
    let result = operating_point(&[("V(out)", 2.5), ("I(R1)", 1e-3)]);
    assert_eq!(real("V(out)*I(R1)", &result), [2.5 * 1e-3]);
    assert!(real("deriv(V(out))", &result)[0].is_nan());
    assert_eq!(real("integ(V(out))", &result), [0.0]);
}

#[test]
fn empty_result_yields_empty_series() {
    let result = transient(Vec::new(), &[("V(out)", Vec::new())]);
    for source in ["V(out) + 1", "deriv(V(out))", "integ(V(out))", "time"] {
        assert!(real(source, &result).is_empty(), "{source}");
    }
}

#[test]
fn malformed_signal_length_is_an_error() {
    let mut result = divider();
    result.signals[1].data = xsim_spice_engine::SignalData::Real(vec![1.0]);
    match evaluation_error("2 * V(out)", &result) {
        ExprError::Syntax { position, message } => {
            assert_eq!(position, 4);
            assert!(message.contains("V(out)"), "{message}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn referenced_signals_are_sorted_and_deduplicated() {
    let expression =
        Expression::parse("V(out)-V(in)*V(out) + I(R1) + V(a, b) + V(c, 0) + Ic(Q1) + V(0)")
            .unwrap();
    assert_eq!(
        expression.signals(),
        ["I(R1)", "Ic(Q1)", "V(a)", "V(b)", "V(c)", "V(in)", "V(out)"]
    );
    assert!(Expression::parse("2 * time").unwrap().signals().is_empty());
}

#[test]
fn source_text_is_kept_verbatim() {
    let text = "  V(out) *  2 ";
    assert_eq!(Expression::parse(text).unwrap().text(), text);
}

#[test]
fn error_messages_are_helpful() {
    let message = |source: &str| Expression::parse(source).unwrap_err().to_string();
    assert_eq!(
        message("1 +"),
        "Expression ends early: expected a value at character 4"
    );
    assert_eq!(message("sqrt(1, 2)"), "sqrt takes 1 argument(s), got 2");
    assert_eq!(message("foo(1)"), "Unknown function foo");
    assert!(message("out * 2").contains("V(out)"));
    assert!(message("2meg").contains("M is mega"));
    assert!(message("(1 + 2").contains("Missing ')'"));
    let unknown = Expression::parse("V(x)")
        .unwrap()
        .evaluate(&divider())
        .unwrap_err();
    assert_eq!(unknown.to_string(), "No signal named V(x) in the result");
}
