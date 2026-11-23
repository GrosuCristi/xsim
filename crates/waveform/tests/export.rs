//! CSV export of evaluated traces (Q137), read back with a minimal RFC 4180
//! reader to prove the file is lossless.

mod common;

use common::{ac, transient};
use num_complex::Complex64;
use xsim_spice_engine::SimulationResult;
use xsim_waveform::export::csv;
use xsim_waveform::{Expression, Series};

/// Splits CSV text into records of fields, honouring quoted fields with
/// doubled quotes and embedded delimiters or line breaks (RFC 4180).
fn read_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let (mut record, mut field) = (Vec::new(), String::new());
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\n') => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            (false, c) => field.push(c),
        }
    }
    assert!(
        field.is_empty() && record.is_empty(),
        "the last line must end with \\n"
    );
    records
}

fn column(result: &SimulationResult, source: &str) -> (String, Series) {
    let series = Expression::parse(source).unwrap().evaluate(result).unwrap();
    (source.to_string(), series)
}

fn parse(cell: &str) -> f64 {
    cell.parse()
        .unwrap_or_else(|_| panic!("{cell:?} is not a number"))
}

#[test]
fn transient_traces_round_trip_exactly() {
    let time = vec![0.0, 1e-9, 2.5e-6, 1e-3, 0.1];
    let v_a: Vec<f64> = time.iter().map(|t| (t * 1e4f64).sin() * 3.3).collect();
    let v_b: Vec<f64> = time.iter().map(|t| 1e-15 + t * 7.0).collect();
    let result = transient(time.clone(), &[("V(a)", v_a), ("V(b)", v_b)]);
    let columns = [
        column(&result, "max(V(a), V(b))"),
        column(&result, "V(a, b) * 1k"),
    ];

    let records = read_csv(&csv(&result, &columns));
    assert_eq!(records[0], ["time [s]", "max(V(a), V(b))", "V(a, b) * 1k"]);
    assert_eq!(records.len(), time.len() + 1);
    for (row, record) in records[1..].iter().enumerate() {
        assert_eq!(parse(&record[0]).to_bits(), time[row].to_bits());
        for (cell, (_, series)) in record[1..].iter().zip(&columns) {
            let Series::Real(values) = series else {
                panic!("transient traces are real");
            };
            assert_eq!(parse(cell).to_bits(), values[row].to_bits());
        }
    }
}

#[test]
fn ac_traces_export_magnitude_and_phase() {
    let frequency = vec![10.0, 1e3, 1e5];
    let v_out: Vec<Complex64> = frequency
        .iter()
        .map(|f| Complex64::from(1.0) / Complex64::new(1.0, f / 1e3))
        .collect();
    let result = ac(frequency.clone(), &[("V(out)", v_out.clone())]);
    let columns = [column(&result, "V(out)"), column(&result, "db(V(out))")];

    let records = read_csv(&csv(&result, &columns));
    assert_eq!(
        records[0],
        [
            "frequency [Hz]",
            "V(out) magnitude",
            "V(out) phase [deg]",
            "db(V(out))"
        ]
    );
    for (row, record) in records[1..].iter().enumerate() {
        let z = v_out[row];
        assert_eq!(parse(&record[0]), frequency[row]);
        assert_eq!(parse(&record[1]), z.norm());
        assert_eq!(parse(&record[2]), z.arg().to_degrees());
        assert_eq!(parse(&record[3]), 20.0 * z.norm().log10());
        // A first-order low-pass lags by atan(f/fc), between 0° and −90°.
        let phase = parse(&record[2]);
        assert!((-90.0..=0.0).contains(&phase));
    }
}
