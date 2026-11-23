//! Dense (reference) and sparse (production) LU must agree (Q101, Q102),
//! componentwise: each solution component is accurate relative to its own
//! size (SR-2, a stop-band response 400 dB below the drive). Both must name
//! the unknown of a singular matrix, and neither may refuse a regular matrix
//! whose entries span many decades (SR-1).

use num_complex::Complex64;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use xsim_spice_engine::solver::{DenseLu, LinearSolver, Scalar, SingularError, SparseLu, System};
use xsim_spice_engine::*;

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0x5eed_0001),
        ..ProptestConfig::default()
    }
}

/// Builds a system from a dense row-major matrix, adding each entry as two
/// halves so duplicate summation is exercised too.
fn system<T: Scalar>(a: &[Vec<T>], b: &[T], half: T) -> System<T> {
    let n = b.len();
    let mut s = System::new(n);
    for (r, row) in a.iter().enumerate() {
        for (c, &v) in row.iter().enumerate() {
            if v != T::zero() {
                s.add(Some(r), Some(c), v * half);
                s.add(Some(r), Some(c), v * half);
            }
        }
        s.add_rhs(Some(r), b[r]);
    }
    s.add(None, Some(0), T::from_real(123.0));
    s.add_rhs(None, T::from_real(456.0));
    s
}

fn residual<T: Scalar>(a: &[Vec<T>], b: &[T], x: &[T]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(row, &bi)| {
            let ax = row
                .iter()
                .zip(x)
                .fold(T::zero(), |acc, (&aij, &xj)| acc + aij * xj);
            (ax - bi).magnitude()
        })
        .fold(0.0, f64::max)
}

fn max_relative_difference<T: Scalar>(x: &[T], y: &[T]) -> f64 {
    let scale = x
        .iter()
        .map(|v| v.magnitude())
        .fold(0.0, f64::max)
        .max(1e-300);
    x.iter()
        .zip(y)
        .map(|(&a, &b)| (a - b).magnitude() / scale)
        .fold(0.0, f64::max)
}

/// A random sparse matrix made strictly diagonally dominant, so it is
/// well conditioned whatever the off-diagonal pattern.
fn dominant(n: usize) -> impl Strategy<Value = (Vec<Vec<f64>>, Vec<f64>)> {
    (
        prop::collection::vec(prop::option::weighted(0.3, -1.0..1.0_f64), n * n),
        prop::collection::vec(-10.0..10.0_f64, n),
        prop::collection::vec(0.5..5.0_f64, n),
    )
        .prop_map(move |(entries, b, margin)| {
            let mut a: Vec<Vec<f64>> = entries
                .chunks(n)
                .map(|row| row.iter().map(|v| v.unwrap_or(0.0)).collect())
                .collect();
            for (i, row) in a.iter_mut().enumerate() {
                row[i] = 0.0;
                let off: f64 = row.iter().map(|v| v.abs()).sum();
                row[i] = if i % 2 == 0 {
                    off + margin[i]
                } else {
                    -(off + margin[i])
                };
            }
            (a, b)
        })
}

/// A random non-symmetric matrix `P·(I + E)` with a random permutation `P`
/// (zero diagonal entries force pivoting) and `‖E‖∞ < 1/2`, so its condition
/// number is below 3.
fn permuted_near_identity(n: usize) -> impl Strategy<Value = (Vec<Vec<f64>>, Vec<f64>)> {
    (
        Just((0..n).collect::<Vec<usize>>()).prop_shuffle(),
        prop::collection::vec(prop::option::weighted(0.4, -1.0..1.0_f64), n * n),
        prop::collection::vec(-1.0..1.0_f64, n),
    )
        .prop_map(move |(perm, entries, b)| {
            let mut m: Vec<Vec<f64>> = entries
                .chunks(n)
                .map(|row| row.iter().map(|v| v.unwrap_or(0.0)).collect())
                .collect();
            for row in &mut m {
                let sum: f64 = row.iter().map(|v| v.abs()).sum();
                let scale = if sum > 0.0 { 0.45 / sum } else { 0.0 };
                row.iter_mut().for_each(|v| *v *= scale);
            }
            for (i, row) in m.iter_mut().enumerate() {
                row[i] += 1.0;
            }
            let a = perm.iter().map(|&p| m[p].clone()).collect();
            (a, b)
        })
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn dense_and_sparse_agree_on_dominant_systems((a, b) in (1_usize..40).prop_flat_map(dominant)) {
        let s = system(&a, &b, 0.5);
        let dense = DenseLu::default().solve(&s).unwrap();
        let sparse = SparseLu::default().solve(&s).unwrap();
        prop_assert!(max_relative_difference(&dense, &sparse) < 1e-10);
        prop_assert!(residual(&a, &b, &dense) < 1e-10 * b.iter().map(|v| v.abs()).fold(1.0, f64::max));
    }

    #[test]
    fn dense_and_sparse_agree_on_non_symmetric_systems((a, b) in (1_usize..40).prop_flat_map(permuted_near_identity)) {
        let s = system(&a, &b, 0.5);
        let dense = DenseLu::default().solve(&s).unwrap();
        let sparse = SparseLu::default().solve(&s).unwrap();
        prop_assert!(max_relative_difference(&dense, &sparse) < 1e-10);
        prop_assert!(residual(&a, &b, &sparse) < 1e-12);
    }

    #[test]
    fn dense_and_sparse_agree_on_complex_systems(
        (re, b) in (1_usize..25).prop_flat_map(dominant),
        rotation in 0.0..std::f64::consts::TAU,
    ) {
        // Rotating every entry by the same phase keeps diagonal dominance.
        let phase = Complex64::from_polar(1.0, rotation);
        let a: Vec<Vec<Complex64>> = re
            .iter()
            .enumerate()
            .map(|(i, row)| row.iter().enumerate().map(|(j, &v)| {
                Complex64::new(v, if i == j { 0.0 } else { 0.5 * v }) * phase
            }).collect())
            .collect();
        let b: Vec<Complex64> = b.iter().map(|&v| Complex64::new(v, -v / 3.0)).collect();
        let s = system(&a, &b, Complex64::new(0.5, 0.0));
        let dense = DenseLu::default().solve(&s).unwrap();
        let sparse = SparseLu::default().solve(&s).unwrap();
        prop_assert!(max_relative_difference(&dense, &sparse) < 1e-10);
        prop_assert!(residual(&a, &b, &dense) < 1e-9);
    }
}

#[test]
fn sparse_solver_reuses_its_analysis_and_adapts_to_new_patterns() {
    let mut solver = SparseLu::default();
    let a = vec![
        vec![4.0, -1.0, 0.0],
        vec![-1.0, 4.0, -1.0],
        vec![0.0, -1.0, 4.0],
    ];
    let b = vec![1.0, 2.0, 3.0];
    let first = solver.solve(&system(&a, &b, 0.5)).unwrap();
    let again = solver.solve(&system(&a, &b, 0.5)).unwrap();
    assert_eq!(first, again);
    // A different pattern (and size) must not reuse the cached ordering.
    let a2 = vec![vec![2.0, 1.0], vec![1.0, 3.0]];
    let x = solver.solve(&system(&a2, &[1.0, 2.0], 0.5)).unwrap();
    assert!(residual(&a2, &[1.0, 2.0], &x) < 1e-14);
}

#[test]
fn reused_solvers_match_fresh_ones_across_patterns_and_failures() {
    // Both solvers keep their matrix, factor storage and work vectors from
    // one solve to the next. Reused across sizes, patterns, a singular
    // system and real and complex systems, every solution must equal (to
    // the bit) that of a solver used once, and a re-solve must refuse once
    // a solve failed.
    let dominant = |n: usize, shift: f64| -> (Vec<Vec<f64>>, Vec<f64>) {
        let a = (0..n)
            .map(|r| {
                (0..n)
                    .map(|c| match r.abs_diff(c) {
                        0 => 4.0 + shift,
                        1 => -1.0 - 0.1 * shift,
                        _ if (r + c) % 3 == 0 => 0.25,
                        _ => 0.0,
                    })
                    .collect()
            })
            .collect();
        let b = (0..n).map(|k| 1.0 + k as f64 * shift).collect();
        (a, b)
    };
    let systems: Vec<System<f64>> = [(3, 0.0), (7, 0.5), (3, 1.0), (12, 2.0), (2, 0.25)]
        .into_iter()
        .map(|(n, shift)| {
            let (a, b) = dominant(n, shift);
            system(&a, &b, 0.5)
        })
        .collect();
    let singular = singular_cases().remove(0).0;
    let mut dense = DenseLu::default();
    let mut sparse = SparseLu::default();
    let mut x = Vec::new();
    for (k, s) in systems.iter().enumerate() {
        let expected_dense = DenseLu::default().solve(s).unwrap();
        let expected_sparse = SparseLu::default().solve(s).unwrap();
        assert_eq!(dense.solve(s).unwrap(), expected_dense, "dense, system {k}");
        assert_eq!(
            sparse.solve(s).unwrap(),
            expected_sparse,
            "sparse, system {k}"
        );
        dense.solve_into(s, &mut x).unwrap();
        assert_eq!(x, expected_dense, "dense solve_into, system {k}");
        let rhs: Vec<f64> = s.rhs().iter().map(|v| 2.0 * v).collect();
        let twice = dense.resolve(&rhs).expect("factorization held");
        let close = |a: &[f64], b: &[f64]| {
            a.iter()
                .zip(b)
                .all(|(p, q)| (p - 2.0 * q).abs() <= 1e-12 * q.abs().max(1.0))
        };
        assert!(close(&twice, &expected_dense), "dense resolve, system {k}");
        assert!(close(&sparse.resolve(&rhs).unwrap(), &expected_sparse));
        // A failure in between leaves nothing to re-solve with.
        assert!(dense.solve(&singular).is_err() && sparse.solve(&singular).is_err());
        assert_eq!(dense.resolve(&rhs), None);
        assert_eq!(sparse.resolve(&rhs), None);
    }
    let complex: System<Complex64> = {
        let (a, b) = dominant(5, 0.5);
        let a: Vec<Vec<Complex64>> = a
            .iter()
            .map(|row| row.iter().map(|&v| Complex64::new(v, 0.3 * v)).collect())
            .collect();
        let b: Vec<Complex64> = b.iter().map(|&v| Complex64::new(v, -v)).collect();
        system(&a, &b, Complex64::new(0.5, 0.0))
    };
    let fresh = DenseLu::default().solve(&complex).unwrap();
    assert_eq!(
        LinearSolver::<Complex64>::solve(&mut dense, &complex).unwrap(),
        fresh
    );
    let fresh = SparseLu::default().solve(&complex).unwrap();
    assert_eq!(
        LinearSolver::<Complex64>::solve(&mut sparse, &complex).unwrap(),
        fresh
    );
}

/// A singular matrix: unknown 2 duplicates unknown 1 (two parallel voltage
/// sources), and unknown 3's column is empty (a floating node).
fn singular_cases() -> Vec<(System<f64>, Vec<usize>)> {
    // [V(n), I(V1), I(V2)]: KCL row and two identical source rows.
    let mut loop_ = System::new(3);
    loop_.add(Some(0), Some(1), 1.0);
    loop_.add(Some(0), Some(2), 1.0);
    loop_.add(Some(1), Some(0), 1.0);
    loop_.add(Some(2), Some(0), 1.0);
    loop_.add_rhs(Some(1), 1.0);
    loop_.add_rhs(Some(2), 2.0);

    // [V(a), V(b)] with b connected to nothing (an explicit zero entry).
    let mut floating = System::new(2);
    floating.add(Some(0), Some(0), 1e-3);
    floating.add(Some(1), Some(1), 0.0);

    // Two nodes joined by a resistor but not to ground: rounding leaves a
    // tiny pivot instead of an exact zero.
    let mut subnet = System::new(3);
    let g = 1.0 / 3.3e3;
    subnet.add(Some(0), Some(0), 1e-3);
    for (r, c, v) in [
        (1, 1, g),
        (2, 2, g),
        (1, 2, -g),
        (2, 1, -g),
        (2, 2, 0.1 * g),
        (2, 2, -0.1 * g),
    ] {
        subnet.add(Some(r), Some(c), v);
    }
    vec![
        (loop_, vec![1, 2]),
        (floating, vec![1]),
        (subnet, vec![1, 2]),
    ]
}

#[test]
fn dense_solver_names_the_dependent_unknown() {
    let rows: Vec<usize> = singular_cases()
        .iter()
        .map(|(s, _)| DenseLu::default().solve(s).unwrap_err().row)
        .collect();
    // In COLAMD column order (shared with the sparse solver) the later of
    // two dependent unknowns. Circuit-level errors name floating nodes and
    // source loops from the topology, not from this choice.
    assert_eq!(rows, [2, 1, 2]);
}

#[test]
fn sparse_solver_detects_the_same_singularities() {
    for (s, candidates) in singular_cases() {
        let SingularError { row, .. } = SparseLu::default().solve(&s).unwrap_err();
        assert!(candidates.contains(&row), "row {row} not in {candidates:?}");
    }
}

#[test]
fn complex_singularity_is_detected() {
    let mut s = System::<Complex64>::new(2);
    let y = Complex64::new(1e-3, 2e-3);
    for (r, c, v) in [(0, 0, y), (1, 1, y), (0, 1, -y), (1, 0, -y)] {
        s.add(Some(r), Some(c), v);
    }
    assert_eq!(DenseLu::default().solve(&s).unwrap_err().row, 1);
    assert!(SparseLu::default().solve(&s).is_err());
}

#[test]
fn ill_conditioned_but_regular_systems_still_solve() {
    // A 1 mΩ shunt beside a 1 GΩ leak: ratios of 1e12 are legitimate.
    let a = vec![vec![1e3 + 1e-9, -1e3], vec![-1e3, 1e3 + 1e-9]];
    let s = system(&a, &[1.0, 0.0], 0.5);
    for x in [
        DenseLu::default().solve(&s).unwrap(),
        SparseLu::default().solve(&s).unwrap(),
    ] {
        assert!(x.iter().all(|v| v.is_finite()));
    }
}

/// `[V(a), V(b)]`: `a` and `b` joined by conductance `g_series`, `b` held to
/// ground only by `g_shunt` (the AC-coupled clamp of SR-1 at DC).
fn gmin_held_node(g_series: f64, g_shunt: f64) -> System<f64> {
    let mut s = System::new(2);
    for (r, c, v) in [
        (0, 0, g_series),
        (0, 1, -g_series),
        (1, 0, -g_series),
        (1, 1, g_series),
        (1, 1, g_shunt),
    ] {
        s.add(Some(r), Some(c), v);
    }
    s.add_rhs(Some(0), 1e-15);
    s
}

#[test]
fn a_node_held_by_gmin_next_to_a_milliohm_resistor_is_regular() {
    // Two default diodes give 2·(Is/Vt + gmin) = 2.77 pS. Against 1/10 mΩ
    // the pivot is 2.8e-14 of the column maximum, still about 120 rounding
    // units: the old raw-column test (1e-13) refused it.
    let g = 2.0 * (1e-14 / 0.025_864_186 + 1e-12);
    for g_series in [100.0, 1e3] {
        for x in [
            DenseLu::default()
                .solve(&gmin_held_node(g_series, g))
                .unwrap(),
            SparseLu::default()
                .solve(&gmin_held_node(g_series, g))
                .unwrap(),
        ] {
            // V(b) = 1 fA / g, to the ε·G/g that assembling G + g leaves.
            let expected = 1e-15 / g;
            let floor = 4.0 * f64::EPSILON * g_series / g;
            assert!((x[1] - expected).abs() <= floor * expected, "{x:?}");
        }
    }
    // 1 µΩ: G + g rounds to G, singular to double precision.
    let s = gmin_held_node(1e6, g);
    assert!(DenseLu::default().solve(&s).is_err());
    assert!(SparseLu::default().solve(&s).is_err());
}

#[test]
fn non_finite_solutions_are_not_singular_matrices() {
    use xsim_spice_engine::solver::SolveFailure;
    let mut s = System::new(1);
    s.add(Some(0), Some(0), 1e-300);
    s.add_rhs(Some(0), 1e300);
    for error in [
        DenseLu::default().solve(&s).unwrap_err(),
        SparseLu::default().solve(&s).unwrap_err(),
    ] {
        assert_eq!(error, SingularError::non_finite(0));
        assert_eq!(error.kind, SolveFailure::NonFinite);
    }
}

/// 15 sections of 1 Ω, 1 µH and 1 nF to ground, loaded by 50 Ω (47
/// unknowns: dense under [`SolverKind::Auto`]).
fn ladder(sections: usize) -> Circuit {
    let mut c = Circuit::new();
    let input = c.node("in");
    c.add(Element::new(
        "V1",
        [input, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    let mut previous = input;
    for k in 0..sections {
        let (middle, node) = (c.node(&format!("m{k}")), c.node(&format!("n{k}")));
        c.add(Element::new(
            format!("R{k}"),
            [previous, middle],
            Device::resistor(1.0),
        ));
        c.add(Element::new(
            format!("L{k}"),
            [middle, node],
            Device::inductor(1e-6),
        ));
        c.add(Element::new(
            format!("C{k}"),
            [node, GROUND],
            Device::capacitor(1e-9),
        ));
        previous = node;
    }
    c.add(Element::new(
        "RL",
        [previous, GROUND],
        Device::resistor(50.0),
    ));
    c
}

/// Exact `V(n_last)` of [`ladder`] by impedance recursion from the load.
fn ladder_response(sections: usize, f: f64) -> Complex64 {
    let w = std::f64::consts::TAU * f;
    let j = Complex64::new(0.0, 1.0);
    let mut shunt = vec![Complex64::default(); sections];
    let mut input = vec![Complex64::default(); sections];
    let mut after: Option<Complex64> = None;
    for k in (0..sections).rev() {
        let mut y = j * w * 1e-9;
        if k == sections - 1 {
            y += 1.0 / 50.0;
        }
        if let Some(z) = after {
            y += 1.0 / z;
        }
        shunt[k] = 1.0 / y;
        input[k] = 1.0 + j * w * 1e-6 + shunt[k];
        after = Some(input[k]);
    }
    (0..sections).fold(Complex64::new(1.0, 0.0), |v, k| v / input[k] * shunt[k])
}

#[test]
fn a_stop_band_400_db_down_keeps_six_digits_with_every_solver() {
    let circuit = ladder(15);
    // 10 points per decade: −183.8 dB at 12.59 MHz, −268.9 dB at 15.85 MHz
    // and −408.2 dB at 25.12 MHz are where the old natural-order dense LU
    // lost 7.7e-7, 9.3e-3 and every digit.
    let analysis = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 10,
        start: 1e6,
        stop: 1e9,
    });
    // LTspice 24.1.9 (tests/reference/solver_ladder15_ac.cir), |V(n14)| in dB.
    let ltspice = [
        (1.584_893_192_461_114e7, -268.855_030_892_857),
        (2.511_886_431_509_582e7, -408.221_180_789_308),
        (1e8, -778.272_080_719_91),
        (1e9, -1_378.901_585_965_14),
    ];
    let mut results = Vec::new();
    for solver in [SolverKind::Auto, SolverKind::Dense, SolverKind::Sparse] {
        let options = SimOptions {
            solver,
            ..SimOptions::default()
        };
        let r = simulate(&circuit, &analysis, &options, &SimControl::new()).unwrap();
        let SignalData::Complex { re, im } = &r.signal("V(n14)").unwrap().data else {
            panic!("AC data is complex");
        };
        assert_eq!(r.axis.values.len(), 31);
        for (k, &f) in r.axis.values.iter().enumerate() {
            let (x, exact) = (Complex64::new(re[k], im[k]), ladder_response(15, f));
            let error = (x - exact).norm() / exact.norm();
            assert!(
                error < 1e-11,
                "{solver:?} at {f} Hz: relative error {error:e}"
            );
        }
        for (f, db) in ltspice {
            let k = r
                .axis
                .values
                .iter()
                .position(|&v| (v / f - 1.0).abs() < 1e-12)
                .unwrap();
            let ours = 20.0 * re[k].hypot(im[k]).log10();
            assert!(
                (ours - db).abs() < 1e-9,
                "{solver:?} at {f} Hz: {ours} dB vs {db} dB"
            );
        }
        results.push(r);
    }
    // Dense and sparse agree componentwise on every signal.
    let (dense, sparse) = (&results[1], &results[2]);
    for (d, s) in dense.signals.iter().zip(&sparse.signals) {
        let (SignalData::Complex { re: dr, im: di }, SignalData::Complex { re: sr, im: si }) =
            (&d.data, &s.data)
        else {
            panic!("AC data is complex");
        };
        for k in 0..dr.len() {
            let (a, b) = (Complex64::new(dr[k], di[k]), Complex64::new(sr[k], si[k]));
            let scale = a.norm().max(b.norm());
            assert!(
                (a - b).norm() <= 1e-11 * scale,
                "{} at point {k}: {a} vs {b}",
                d.name
            );
        }
    }
}

#[test]
fn empty_systems_have_empty_solutions() {
    let s = System::<f64>::new(0);
    assert!(DenseLu::default().solve(&s).unwrap().is_empty());
    assert!(SparseLu::default().solve(&s).unwrap().is_empty());
}
