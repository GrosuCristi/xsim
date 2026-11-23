//! CSV export (Q137).
//!
//! Numbers are written with Rust's shortest round-trip formatting for
//! `f64` in its `Debug` form: positional for moderate magnitudes (`0.5`,
//! `1250.0`) and scientific for very small or large ones (`1e-9`), so every
//! value parses back to exactly the same `f64` and femto-scale currents do
//! not turn into long runs of zeros. NaN and infinities are written as
//! `NaN`, `inf` and `-inf`, which also parse back.

use crate::expr::Series;
use std::borrow::Cow;
use xsim_spice_engine::SimulationResult;

/// CSV text with a header row: the axis column (`time`, `frequency`, or the
/// sweep name, with unit in brackets, e.g. `time [s]`) followed by one
/// column per `(name, series)` pair. Complex series export two columns,
/// `name magnitude` and `name phase [deg]`. Numbers use Rust's shortest
/// round-trip formatting; lines end with `\n`. Names containing commas or
/// quotes are quoted per RFC 4180.
///
/// Header cells containing line breaks are quoted too. A series shorter
/// than the axis leaves its remaining cells empty; extra samples are
/// ignored. The phase is `atan2(im, re)` in degrees, within ±180°.
pub fn csv(result: &SimulationResult, columns: &[(String, Series)]) -> String {
    let mut text = String::new();
    write_header(&mut text, result, columns);
    for (row, &x) in result.axis.values.iter().enumerate() {
        text.push_str(&number(x));
        for (_, series) in columns {
            match series {
                Series::Real(values) => push_cell(&mut text, values.get(row).copied()),
                Series::Complex(values) => {
                    let value = values.get(row);
                    push_cell(&mut text, value.map(|z| z.norm()));
                    push_cell(&mut text, value.map(|z| z.arg().to_degrees()));
                }
            }
        }
        text.push('\n');
    }
    text
}

fn write_header(text: &mut String, result: &SimulationResult, columns: &[(String, Series)]) {
    let axis = &result.axis;
    if axis.unit.is_empty() {
        text.push_str(&field(&axis.name));
    } else {
        text.push_str(&field(&format!("{} [{}]", axis.name, axis.unit)));
    }
    for (name, series) in columns {
        match series {
            Series::Real(_) => push_header_cell(text, name),
            Series::Complex(_) => {
                push_header_cell(text, &format!("{name} magnitude"));
                push_header_cell(text, &format!("{name} phase [deg]"));
            }
        }
    }
    text.push('\n');
}

/// Appends a separator and `value`, or an empty cell for a missing sample.
fn push_cell(text: &mut String, value: Option<f64>) {
    text.push(',');
    if let Some(value) = value {
        text.push_str(&number(value));
    }
}

fn push_header_cell(text: &mut String, name: &str) {
    text.push(',');
    text.push_str(&field(name));
}

/// A header cell, quoted per RFC 4180 when it contains a delimiter, a quote
/// or a line break (quotes inside are doubled).
fn field(cell: &str) -> Cow<'_, str> {
    if cell.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", cell.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(cell)
    }
}

fn number(value: f64) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};
    use xsim_spice_engine::{AnalysisKind, Axis};

    fn result(
        analysis: AnalysisKind,
        name: &str,
        unit: &str,
        values: Vec<f64>,
    ) -> SimulationResult {
        SimulationResult {
            analysis,
            axis: Axis {
                name: name.to_string(),
                unit: unit.to_string(),
                values,
            },
            signals: Vec::new(),
        }
    }

    #[test]
    fn transient_columns() {
        let result = result(
            AnalysisKind::Transient,
            "time",
            "s",
            vec![0.0, 0.5e-3, 1e-3],
        );
        let columns = [
            ("V(out)".to_string(), Series::Real(vec![0.0, 0.25, -1.5])),
            (
                "I(R1)".to_string(),
                Series::Real(vec![1e-12, 2.5e-6, 1250.0]),
            ),
        ];
        assert_eq!(
            csv(&result, &columns),
            "time [s],V(out),I(R1)\n\
             0.0,0.0,1e-12\n\
             0.0005,0.25,2.5e-6\n\
             0.001,-1.5,1250.0\n"
        );
    }

    #[test]
    fn complex_columns_export_magnitude_and_phase() {
        let result = result(AnalysisKind::Ac, "frequency", "Hz", vec![1e3, 2e3]);
        let samples = [Complex64::new(3.0, 4.0), Complex64::new(0.0, -2.0)];
        let columns = [("V(out)".to_string(), Series::Complex(samples.to_vec()))];
        // |3 + 4j| = 5 and |−2j| = 2 exactly; phases are atan2 in degrees.
        let phase = |z: Complex64| format!("{:?}", z.im.atan2(z.re).to_degrees());
        assert_eq!(
            csv(&result, &columns),
            format!(
                "frequency [Hz],V(out) magnitude,V(out) phase [deg]\n\
                 1000.0,5.0,{}\n\
                 2000.0,2.0,{}\n",
                phase(samples[0]),
                phase(samples[1])
            )
        );
    }

    #[test]
    fn sweep_and_operating_point_axes() {
        let sweep = result(AnalysisKind::DcSweep, "V1", "V", vec![1.0]);
        assert!(csv(&sweep, &[]).starts_with("V1 [V]\n"));
        let point = result(AnalysisKind::OperatingPoint, "point", "", vec![0.0]);
        assert_eq!(csv(&point, &[]), "point\n0.0\n");
    }

    #[test]
    fn header_cells_are_quoted_per_rfc_4180() {
        let result = result(AnalysisKind::Transient, "time", "s", Vec::new());
        let columns = [
            ("V(a, b)".to_string(), Series::Real(Vec::new())),
            ("say \"hi\"".to_string(), Series::Real(Vec::new())),
            ("two\nlines".to_string(), Series::Real(Vec::new())),
            ("x,y".to_string(), Series::Complex(Vec::new())),
        ];
        assert_eq!(
            csv(&result, &columns),
            "time [s],\"V(a, b)\",\"say \"\"hi\"\"\",\"two\nlines\",\"x,y magnitude\",\"x,y phase [deg]\"\n"
        );
    }

    #[test]
    fn short_series_leave_cells_empty() {
        let result = result(AnalysisKind::Ac, "frequency", "Hz", vec![1.0, 2.0]);
        let columns = [
            ("a".to_string(), Series::Real(vec![7.0, 8.0, 9.0])),
            ("b".to_string(), Series::Real(vec![1.0])),
            ("c".to_string(), Series::Complex(Vec::new())),
            ("d".to_string(), Series::Real(vec![5.0, 6.0])),
        ];
        assert_eq!(
            csv(&result, &columns),
            "frequency [Hz],a,b,c magnitude,c phase [deg],d\n\
             1.0,7.0,1.0,,,5.0\n\
             2.0,8.0,,,,6.0\n"
        );
    }

    #[test]
    fn non_finite_values_round_trip() {
        let result = result(AnalysisKind::Transient, "time", "s", vec![0.0, 1.0, 2.0]);
        let columns = [(
            "v".to_string(),
            Series::Real(vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY]),
        )];
        let text = csv(&result, &columns);
        assert_eq!(text, "time [s],v\n0.0,NaN\n1.0,inf\n2.0,-inf\n");
        for line in text.lines().skip(1) {
            let cell = line.split(',').nth(1).unwrap();
            assert!(!cell.parse::<f64>().unwrap().is_finite());
        }
    }

    proptest! {
        #![proptest_config(Config {
            cases: 500,
            rng_seed: RngSeed::Fixed(0xC5F),
            failure_persistence: None,
            ..Config::default()
        })]

        /// Every exported number parses back to the identical bits.
        #[test]
        fn numbers_round_trip_exactly(value in any::<f64>().prop_filter("finite", |v| v.is_finite())) {
            let parsed: f64 = number(value).parse().unwrap();
            prop_assert_eq!(parsed.to_bits(), value.to_bits());
        }
    }
}
