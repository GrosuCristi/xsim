//! Run control (Q110): progress, pause/resume and cancellation from another
//! thread, and bit-for-bit determinism (Q108).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use xsim_spice_engine::*;

/// Gives a background run time to reach a checkpoint without making the
/// tests slow; assertions never depend on it being long enough except where
/// a blocked run is checked to *stay* blocked.
const SETTLE: Duration = Duration::from_millis(100);
/// Upper bound for waiting on a background run before failing the test.
const PATIENCE: Duration = Duration::from_secs(60);

/// A pulse-driven parallel RLC: breakpoints, ringing and every device kind.
fn pulsed_rlc() -> Circuit {
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    let pulse = Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay: 10e-6,
        rise: 1e-6,
        fall: 2e-6,
        width: 20e-6,
        period: 50e-6,
        cycles: None,
    };
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::with_waveform(pulse)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("C1", [b, GROUND], Device::capacitor(10e-9)));
    c.add(Element::new("L1", [b, GROUND], Device::inductor(10e-3)));
    c
}

/// An RC charged from rest: 10⁵ steps of 2 µs, long enough to observe.
fn slow_rc() -> Circuit {
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("C1", [b, GROUND], Device::capacitor(1e-6)));
    c
}

fn long_transient() -> Analysis {
    transient_until(0.2)
}

/// A run that would take minutes (10⁷ steps, the most an explicit maximum
/// step may imply), for tests that always cancel it, so cancellation can
/// never race with completion.
fn endless_transient() -> Analysis {
    transient_until(20.0)
}

fn transient_until(stop: f64) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step: 1e-3,
        start_save: 0.0,
        max_step: Some(2e-6),
        initial_state: InitialState::ZeroState,
    })
}

fn spawn(
    control: &Arc<SimControl>,
    analysis: Analysis,
) -> (
    thread::JoinHandle<Result<SimulationResult, SimError>>,
    Arc<AtomicBool>,
) {
    let control = Arc::clone(control);
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let result = simulate(&slow_rc(), &analysis, &SimOptions::default(), &control);
        flag.store(true, Ordering::SeqCst);
        result
    });
    (handle, done)
}

fn wait_for_progress(control: &SimControl, fraction: f64) {
    let start = Instant::now();
    while control.progress() < fraction {
        assert!(
            start.elapsed() < PATIENCE,
            "the run never reached {fraction}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn cancelling_before_the_run_stops_it_immediately() {
    let control = SimControl::new();
    control.cancel();
    let error = simulate(
        &slow_rc(),
        &long_transient(),
        &SimOptions::default(),
        &control,
    )
    .unwrap_err();
    assert_eq!(error, SimError::Cancelled { progress: 0.0 });
    assert_eq!(error.to_string(), "Simulation cancelled at 0.0%");
}

#[test]
fn cancelling_mid_run_reports_how_far_it_got() {
    let control = Arc::new(SimControl::new());
    let (handle, _) = spawn(&control, endless_transient());
    wait_for_progress(&control, 1e-5);
    control.cancel();
    match handle.join().unwrap() {
        Err(SimError::Cancelled { progress }) => {
            assert!((1e-5..1.0).contains(&progress), "{progress}")
        }
        other => panic!("expected cancellation, got {other:?}"),
    }
}

#[test]
fn a_paused_run_waits_for_resume() {
    let control = Arc::new(SimControl::new());
    control.pause();
    let (handle, done) = spawn(&control, long_transient());
    thread::sleep(SETTLE);
    assert!(!done.load(Ordering::SeqCst), "a paused run must not finish");
    assert_eq!(control.progress(), 0.0);
    assert!(control.is_paused());
    control.resume();
    let result = handle.join().unwrap().unwrap();
    assert_eq!(*result.axis.values.last().unwrap(), 0.2);
    assert_eq!(control.progress(), 1.0);
}

#[test]
fn pausing_mid_run_freezes_progress() {
    let control = Arc::new(SimControl::new());
    let (handle, done) = spawn(&control, long_transient());
    wait_for_progress(&control, 0.02);
    control.pause();
    // The run stops at its next checkpoint, then stays there.
    thread::sleep(SETTLE);
    let frozen = control.progress();
    thread::sleep(SETTLE);
    assert_eq!(control.progress(), frozen);
    assert!(!done.load(Ordering::SeqCst));
    control.resume();
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn cancelling_releases_a_paused_run() {
    let control = Arc::new(SimControl::new());
    let (handle, _) = spawn(&control, endless_transient());
    wait_for_progress(&control, 1e-5);
    control.pause();
    thread::sleep(SETTLE);
    control.cancel();
    assert!(matches!(
        handle.join().unwrap(),
        Err(SimError::Cancelled { .. })
    ));
}

#[test]
fn progress_never_decreases() {
    let control = Arc::new(SimControl::new());
    let (handle, done) = spawn(&control, long_transient());
    let mut last = 0.0;
    while !done.load(Ordering::SeqCst) {
        let now = control.progress();
        assert!(now >= last && now <= 1.0, "{now} after {last}");
        last = now;
        thread::sleep(Duration::from_micros(200));
    }
    handle.join().unwrap().unwrap();
    assert_eq!(control.progress(), 1.0);
}

fn run(analysis: &Analysis, options: &SimOptions) -> SimulationResult {
    simulate(&pulsed_rlc(), analysis, options, &SimControl::new()).unwrap()
}

fn bits(result: &SimulationResult) -> Vec<u64> {
    let mut out: Vec<u64> = result.axis.values.iter().map(|v| v.to_bits()).collect();
    for signal in &result.signals {
        match &signal.data {
            SignalData::Real(v) => out.extend(v.iter().map(|x| x.to_bits())),
            SignalData::Complex { re, im } => out.extend(re.iter().chain(im).map(|x| x.to_bits())),
        }
    }
    out
}

#[test]
fn identical_runs_are_bit_identical() {
    let transient = Analysis::Transient(Transient {
        stop: 60e-6,
        step: 10e-6,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    });
    let sweep = Analysis::DcSweep(DcSweep {
        source: "V1".into(),
        start: -1.0,
        stop: 1.0,
        step: 0.1,
    });
    let ac = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 20,
        start: 10.0,
        stop: 1e6,
    });
    for method in [
        IntegrationMethod::BackwardEuler,
        IntegrationMethod::Trapezoidal,
        IntegrationMethod::ModifiedTrapezoidal,
        IntegrationMethod::Gear2,
    ] {
        for solver in [SolverKind::Dense, SolverKind::Sparse] {
            let options = SimOptions {
                method,
                solver,
                // Backward Euler needs ~1/reltol steps per time constant;
                // determinism does not depend on the tolerance.
                reltol: if method == IntegrationMethod::BackwardEuler {
                    1e-3
                } else {
                    1e-6
                },
                ..SimOptions::default()
            };
            for analysis in [&transient, &sweep, &ac] {
                let (first, second) = (run(analysis, &options), run(analysis, &options));
                assert_eq!(
                    bits(&first),
                    bits(&second),
                    "{method:?} {solver:?} {analysis:?}"
                );
            }
        }
    }
}

#[test]
fn failures_report_their_progress_as_a_percentage() {
    let error = SimError::NoConvergence {
        at: Some(FailurePoint::time(1.5e-3)),
        unknown: None,
        progress: 0.375,
        detail: "source stepping stalled at 12.5% of the source values".into(),
    };
    let text = error.to_string();
    assert!(
        text.contains("37.5%") && text.contains("at t = 1.5 ms") && text.contains("12.5%"),
        "{text}"
    );
    let text = SimError::TimestepTooSmall {
        time: 2e-6,
        progress: 0.25,
    }
    .to_string();
    assert!(text.contains("25.0%") && text.contains("2 µs"), "{text}");
}
