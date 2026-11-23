//! Operating points and DC sweeps that used to stall at the rounding floor
//! (DCAC-3): series diode strings with a series resistance at low forward
//! bias. Each diode's internal node is joined to its terminal by `1/RS` but
//! held to the rest of the circuit only by junction conductances about 1e11
//! times smaller, so double precision resolves those nodes only to about
//! `ε·G/g` of their voltage. Newton's updates stall at that noise; xsim now
//! accepts an iterate whose updates are within the estimated noise, and
//! warns that the result is less precise than reltol.
//!
//! References: the exact solution by bisection (by symmetry every diode
//! carries the same junction voltage), and LTspice 24.1.9 with its default
//! tolerances (`tests/reference/dc_diode_string*.cir`). At xsim's tolerances
//! LTspice does not converge on these circuits at all, and at its own it
//! lands as far from the exact solution as xsim does: both sit at the
//! double-precision floor.

use xsim_spice_engine::*;

const IS: f64 = 2e-14;
const N: f64 = 1.4;
const R1: f64 = 100.0;

/// `V1 n0 0`, `R1 n0 n1 100`, then `count` diodes `n1 → n2 → … → 0`.
fn string(count: usize, volts: f64, rs: f64) -> Circuit {
    let model = DiodeModel {
        is: IS,
        n: N,
        rs,
        ..DiodeModel::default()
    };
    let mut c = Circuit::new();
    let n0 = c.node("n0");
    c.add(Element::new(
        "V1",
        [n0, GROUND],
        Device::VoltageSource(Source::dc(volts)),
    ));
    let mut anode = c.node("n1");
    c.add(Element::new("R1", [n0, anode], Device::resistor(R1)));
    for k in 1..=count {
        let cathode = if k == count {
            GROUND
        } else {
            c.node(&format!("n{}", k + 1))
        };
        c.add(Element::new(
            format!("D{k}"),
            [anode, cathode],
            Device::diode(model.clone()),
        ));
        anode = cathode;
    }
    c
}

/// Exact `V(n_k)` of the string, by bisection on the string current.
fn exact(count: usize, volts: f64, rs: f64, k: usize) -> f64 {
    // kT/q at 27 °C with the exact SI constants, as xsim computes it.
    let vt = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;
    let gmin = SimOptions::default().gmin;
    let excess = |i: f64| {
        let vj = (volts - R1 * i) / count as f64 - rs * i;
        i - (IS * (vj / (N * vt)).exp_m1() + gmin * vj)
    };
    let (mut low, mut high) = (0.0, volts.max(0.0) / R1);
    for _ in 0..300 {
        let mid = 0.5 * (low + high);
        if excess(mid) < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    let v1 = volts - R1 * 0.5 * (low + high);
    v1 * (count + 1 - k) as f64 / count as f64
}

fn op(circuit: &Circuit, solver: SolverKind) -> (SimulationResult, Vec<String>) {
    let control = SimControl::new();
    let options = SimOptions {
        solver,
        ..SimOptions::default()
    };
    let result = simulate(circuit, &Analysis::OperatingPoint, &options, &control)
        .unwrap_or_else(|e| panic!("{solver:?}: {e}"));
    (result, control.warnings())
}

fn value(result: &SimulationResult, name: &str) -> f64 {
    result.signal(name).unwrap().real().unwrap()[0]
}

fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

fn floor_warning(warnings: &[String]) -> bool {
    warnings
        .iter()
        .any(|w| w.contains("could be resolved only to about ±") && w.contains("double precision"))
}

#[test]
fn a_two_diode_string_converges_at_low_bias() {
    let circuit = string(2, 0.05, 8.0);
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let (r, warnings) = op(&circuit, solver);
        // LTspice (default tolerances, direct Newton): V(n1) =
        // 0.0499999999955109, V(n2) = 0.024999985341612. The exact V(n2) is
        // 0.0249999999978; both simulators sit 1.5e-8 V below it (measured
        // agreement with LTspice: 3e-12 V). The iterate accepted at the
        // floor depends on the device code, so the tolerance is the floor.
        close(
            value(&r, "V(n1)"),
            0.049_999_999_995_510_9,
            1e-12,
            "V(n1) vs LTspice",
        );
        close(
            value(&r, "V(n2)"),
            0.024_999_985_341_612,
            1e-7,
            "V(n2) vs LTspice",
        );
        close(
            value(&r, "V(n2)"),
            exact(2, 0.05, 8.0, 2),
            1e-7,
            "V(n2) vs exact",
        );
        assert!(floor_warning(&warnings), "{warnings:?}");
    }
}

#[test]
fn a_twelve_diode_string_converges_at_low_and_high_bias() {
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let (r, warnings) = op(&string(12, 0.5, 8.0), solver);
        // LTspice: V(n2) = 0.458328170334794, V(n7) = 0.249984040562081,
        // V(n12) = 0.0416652419082393, each up to 5e-6 V from the exact
        // values; xsim is as close (measured: at most 1.7e-5 V from exact,
        // 2.3e-7 V from LTspice at V(n2)). The tolerance is the resolution
        // xsim itself states in its warning (about ±40 µV at V(n12)).
        const RESOLUTION: f64 = 5e-5;
        for (node, ltspice) in [
            (2, 0.458_328_170_334_794),
            (7, 0.249_984_040_562_081),
            (12, 0.041_665_241_908_239_3),
        ] {
            let name = format!("V(n{node})");
            close(
                value(&r, &name),
                ltspice,
                RESOLUTION,
                &format!("{name} vs LTspice"),
            );
        }
        for node in 1..=12 {
            let name = format!("V(n{node})");
            close(
                value(&r, &name),
                exact(12, 0.5, 8.0, node),
                RESOLUTION,
                &format!("{name} vs exact"),
            );
        }
        assert!(floor_warning(&warnings), "{warnings:?}");
        // Well into conduction the string is well conditioned again.
        let (r, warnings) = op(&string(12, 5.0, 8.0), solver);
        for node in 1..=12 {
            let name = format!("V(n{node})");
            close(
                value(&r, &name),
                exact(12, 5.0, 8.0, node),
                1e-8,
                &format!("{name} at 5 V vs exact"),
            );
        }
        close(
            value(&r, "V(n2)"),
            4.583_333_150_472_7,
            1e-8,
            "V(n2) at 5 V vs LTspice",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn diode_strings_sweep_from_zero() {
    for (count, stop, step) in [(2, 1.0, 0.05), (12, 30.0, 0.5)] {
        for solver in [SolverKind::Dense, SolverKind::Sparse] {
            let options = SimOptions {
                solver,
                ..SimOptions::default()
            };
            let sweep = Analysis::DcSweep(DcSweep {
                source: "V1".into(),
                start: 0.0,
                stop,
                step,
            });
            let r = simulate(
                &string(count, 0.0, 8.0),
                &sweep,
                &options,
                &SimControl::new(),
            )
            .unwrap_or_else(|e| panic!("{count} diodes, {solver:?}: {e}"));
            let v2 = r.signal("V(n2)").unwrap().real().unwrap();
            for (k, &volts) in r.axis.values.iter().enumerate() {
                let expected = exact(count, volts, 8.0, 2);
                // The rounding floor at low bias (measured: up to 2.6e-6 V
                // below 3.5 V), 9+ digits once the diodes conduct.
                let tolerance = if volts < 5.0 {
                    1e-5
                } else {
                    1e-9 * expected.abs()
                };
                close(
                    v2[k],
                    expected,
                    tolerance,
                    &format!("{count} diodes at {volts} V ({solver:?})"),
                );
            }
            if count == 12 {
                // LTspice .dc V1 0 30 0.5 (default tolerances).
                for (volts, ltspice, tolerance) in [
                    (1.0, 0.916_662_592_509_805, 2e-5),
                    (5.0, 4.583_333_150_472_7, 1e-8),
                    (30.0, 19.386_949_404_568_3, 2e-7),
                ] {
                    let k = r.axis.values.iter().position(|&v| v == volts).unwrap();
                    close(
                        v2[k],
                        ltspice,
                        tolerance,
                        &format!("V(n2) at {volts} V vs LTspice"),
                    );
                }
            }
        }
    }
}

#[test]
fn a_milliohm_series_resistance_is_a_precision_limit_not_a_floating_node() {
    // G/g ≈ 1e15: the internal nodes are resolvable only to about 1e-3 of
    // their voltage. This used to be reported as "Singular matrix at
    // V(D2:a') (check for floating nodes ...)".
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let (r, warnings) = op(&string(2, 0.05, 1e-3), solver);
        close(
            value(&r, "V(n2)"),
            exact(2, 0.05, 1e-3, 2),
            2.5e-4,
            "V(n2) vs exact",
        );
        assert!(floor_warning(&warnings), "{warnings:?}");
    }
}
