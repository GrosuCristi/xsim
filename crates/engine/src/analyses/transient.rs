//! Transient analysis (Q106).
//!
//! ## Initial point
//! [`InitialState::OperatingPoint`] solves the DC operating point with every
//! source at its waveform's `t = 0` value. Capacitor and inductor initial
//! conditions constrain it (D16, as LTspice without `uic`): a capacitor with
//! an initial voltage is held at it, an inductor with an initial current
//! carries it, and the rest of the circuit is solved around them. When other
//! parts override such a hold (a voltage source across the capacitor or an
//! inductor shorting it, a current source or another inductor forcing a
//! different current through the inductor), the hold is released, one part
//! at a time in element order, and the operating point solved again, so the
//! overriding parts decide and the conflict's enormous holding current or
//! shunt voltage reaches neither the samples nor the starting state; the run
//! warns, naming the part and the value it starts from. An initial current
//! with no path at all (an inductor whose only other paths are capacitors
//! without an initial voltage) is an error naming the inductor.
//!
//! [`InitialState::ZeroState`] skips the operating point: the state
//! histories start at each capacitor's `initial_voltage` and each inductor's
//! `initial_current` (default zero), and the first step starts from the zero
//! vector plus the inductors' initial currents. The `t = 0` sample is the
//! solution consistent with those states: capacitors held at their initial
//! voltages, inductors carrying their initial currents and sources at their
//! `t = 0` values, which LTspice's `uic` shows from its first step on; holds
//! that other parts override (a capacitor directly across a voltage source)
//! are released there as above, without a warning. When that cannot be
//! solved, or other devices carry integrated charge (junction capacitances,
//! which it cannot hold at rest), the sample is the starting vector itself.
//!
//! ## Timestep control
//! Each attempt follows Q106's flowchart:
//!
//! * Newton–Raphson fails: the step is rejected and divided by 8.
//! * A device changes state within the step at a time it must resolve (a
//!   switch's threshold crossing, see [`crate::devices::DeviceInstance::locate`]):
//!   the step is retried ending at the time the device asks for. Within two
//!   minimum steps of an event no shorter step exists (event landing would
//!   stretch the retry back to the event), so there the device changes
//!   state where the step ends, less than two minimum steps late.
//! * The truncation error exceeds its tolerance: the step is rejected and
//!   shrunk to the size the error estimate predicts would pass.
//! * Otherwise the step is accepted and the next one may grow by at most 2×.
//!
//! Only rejections can push the step below the minimum step; a step the
//! controller proposes on its own is never shorter than it. A rejection that
//! asks for less first retries at the minimum step itself (as SPICE does),
//! and only a rejection there ends the run with
//! [`SimError::TimestepTooSmall`]. At the minimum step (or the step that
//! event landing stretches it to) an inductor's flux may move by the dual of
//! `chgtol` ([`crate::integration`]); when a step needed that, the run warns
//! with the inductor-current resolution it implies and where.
//!
//! The truncation error is controlled per unit time (see
//! [`crate::integration`]); steps aim at a fraction of the tolerance
//! ([`ERROR_TARGET`]). That bounds the error each state accumulates while
//! it settles, but the phase error of a lightly damped oscillation keeps
//! growing with the number of periods simulated; see [`ERROR_TARGET`] for
//! the measured rate and the reltol a long run needs.
//!
//! After `t = 0`, after every breakpoint and at every accepted point where a
//! switch changed state the integrator restarts: the first step is a
//! step-doubled backward-Euler step (two half steps checked against one
//! full step and for their curvature, so that it cannot step over a decay
//! far faster than itself; see [`crate::integration`]), because no smooth
//! history exists yet to estimate errors from.
//! It starts at a hundredth of the step the controller took before the
//! breakpoint (before a cluster of breakpoints, such as both corners of a
//! picosecond edge), but at no fewer than a thousand minimum steps. The
//! method's own formulas continue from there. A switch's change of state is
//! a jump of the circuit equations, so there the reference of the relative
//! tolerance restarts too ([`crate::integration::Integrator::restart_reference`]).
//!
//! The step is also limited by the maximum step (by default
//! `(stop − start_save) / 50`, as in SPICE) and, while results are saved, by
//! the smooth source waveforms (SINE, EXP, SFFM): linear interpolation
//! between samples must reproduce them within the result tolerance below,
//! even where no reactive state sees them (a resistive divider, a clipper).
//!
//! ## Sampling
//! Steps land exactly on every breakpoint, on every multiple of the output
//! step from `start_save` on, on `start_save` and on the stop time. Events
//! closer together than the minimum step are merged; a breakpoint then wins
//! over a grid time and the stop time over both. A step that would leave less
//! than half of itself before the next event is split into two equal steps
//! instead of leaving a sliver.
//!
//! ## Stored points (D18)
//! The result keeps every event the steps land on (output grid,
//! breakpoints, `start_save`, stop) and every other accepted point at or
//! after `start_save` that linear interpolation between the kept points
//! needs to reproduce each signal within `reltol·swing + vntol` (`abstol`
//! for currents), with `swing` the signal's range so far. The test runs
//! online on the cone of slopes from the last kept point that pass every
//! dropped point within its tolerance ("swinging door"), so every dropped
//! point stays within tolerance, not only the newest. TR-BDF2's intermediate
//! stage is internal. A run whose result would still exceed [`MAX_VALUES`]
//! stored values (samples × (signals + 1)) stops with
//! [`SimError::ResultTooLarge`]; an output grid or an explicit maximum step
//! that exceeds the limits by itself is rejected before simulating.

use super::{MAX_POINTS, MAX_VALUES, Recorder, dc_error, invalid, singular};
use crate::analysis::{InitialState, IntegrationMethod, SimOptions, Transient};
use crate::circuit::{Circuit, Device, NodeIndex};
use crate::control::SimControl;
use crate::devices::{Hold, InitialConstraints};
use crate::error::SimError;
use crate::integration::{self, Estimate, Formula};
use crate::mna::{Conditions, InitialConditions, Mna};
use crate::newton::{self, NewtonError, Request};
use crate::result::{AnalysisKind, Axis, Quantity, SimulationResult};
use crate::source::{BREAKPOINT_LIMIT, Waveform};
use crate::units::format_value;

/// Fraction of the truncation-error tolerance that steps of a first-order
/// (backward Euler) and a second-order method aim for. Halving the target
/// doubles the steps of a first-order method but costs a second-order one
/// only 41 % more, so the second-order methods aim much lower: ringing
/// circuits amplify the per-state errors by their quality factor.
///
/// The control bounds the error per unit time, so the phase error of a
/// lightly damped oscillation grows linearly with the periods simulated.
/// Measured at the default tolerances (release build, series RLC switched
/// onto 1 V from rest, 1 mH and 1 µF, 5 ms = 25 periods; worst deviation
/// from the closed form over all samples):
///
/// | Q | trapezoidal, Gear-2 | TR-BDF2 |
/// |---|---|---|
/// | 1 | 8.4e-8 V | 6.7e-8 V |
/// | 10 | 1.06e-6 V | 8.5e-7 V |
/// | 100 | 6.0e-6 V | 4.8e-6 V |
///
/// An undamped LC tank accumulates `0.35 · reltol · amplitude` per period
/// with the trapezoidal rule and Gear-2 and `0.28 · reltol · amplitude` with
/// TR-BDF2, linearly up to the 1000 periods measured; the error scales with
/// reltol. Six decimals therefore hold for Q up to about 10 over a few
/// dozen periods; a run of `N` lightly damped periods needs
/// `reltol ≈ 3e-6 / N` for six digits of its amplitude. A target of 0.005
/// was measured to give ten times the accuracy for about three times the
/// run time, but with it the diode doubler with milliohm ESR of
/// `tests/ill_conditioned.rs` stopped with a timestep too small at 6.8 µs,
/// so it stays at 0.05.
const ERROR_TARGET: [f64; 2] = [0.5, 0.05];
/// Largest growth of the step from one accepted step to the next. It also
/// keeps variable-step BDF2 within its zero-stability limit (1 + √2).
const MAX_GROWTH: f64 = 2.0;
/// Smallest reduction applied after a truncation-error rejection, so a wild
/// estimate cannot collapse the step in one go.
const MAX_SHRINK: f64 = 1e-3;
/// Reduction after a Newton failure (Q106).
const NEWTON_SHRINK: f64 = 8.0;
/// The first step after a restart is this fraction of the step the
/// controller took before it; step doubling then validates it.
const RESTART_FRACTION: f64 = 0.01;
/// The first step after a restart is at least this many minimum steps
/// (event landing still shortens it).
const RESTART_FLOOR: f64 = 1e3;
/// The default maximum step divides the saved interval into this many parts.
const DEFAULT_STEPS: f64 = 50.0;
/// An inductor's shunt diverts current because its hold is overridden when
/// the ten times stiffer probe shunt diverts less than this many times as
/// much; where the circuit merely puts a voltage across the inductor, it
/// diverts ten times as much.
const PROBE_GROWTH: f64 = 3.0;

pub(crate) fn run(
    circuit: &Circuit,
    spec: &Transient,
    options: &SimOptions,
    control: &SimControl,
) -> Result<SimulationResult, SimError> {
    let plan = Plan::new(circuit, spec, options)?;
    let mut mna = Mna::new(circuit, options);
    simulate(circuit, &mut mna, &plan, control)
}

/// The validated time parameters of one transient run.
#[derive(Debug, Clone)]
struct Plan {
    stop: f64,
    step: f64,
    start_save: f64,
    max_step: f64,
    min_step: f64,
    initial_state: InitialState,
    /// Most values the result may store (D18).
    max_values: usize,
    /// Whether the result drops the points interpolation does not need
    /// (always, except in unit tests that observe every step).
    thin: bool,
}

impl Plan {
    fn new(circuit: &Circuit, spec: &Transient, options: &SimOptions) -> Result<Self, SimError> {
        let Transient {
            stop,
            step,
            start_save,
            max_step: requested_max_step,
            initial_state,
        } = *spec;
        if !(stop.is_finite() && stop > 0.0) {
            return Err(invalid(format!(
                "transient stop time must be positive and finite, got {stop}"
            )));
        }
        if !(step.is_finite() && step > 0.0) {
            return Err(invalid(format!(
                "transient output step must be positive and finite, got {step}"
            )));
        }
        if !(start_save.is_finite() && (0.0..stop).contains(&start_save)) {
            return Err(invalid(format!(
                "transient start of saving ({start_save} s) must lie in [0, stop)"
            )));
        }
        let saved = stop - start_save;
        if saved / step >= MAX_POINTS as f64 {
            return Err(invalid(format!(
                "a {saved} s transient with an output step of {step} s needs more than {MAX_POINTS} samples"
            )));
        }
        let min_step = stop * options.min_step_ratio;
        let max_step = requested_max_step.unwrap_or(saved / DEFAULT_STEPS);
        if !(max_step.is_finite() && max_step >= min_step) {
            return Err(invalid(format!(
                "transient maximum step must be finite and at least the minimum step {min_step} s, got {max_step}"
            )));
        }
        if requested_max_step.is_some() && stop / max_step > MAX_POINTS as f64 {
            return Err(invalid(format!(
                "a maximum step of {} needs more than {MAX_POINTS} steps over the {} run; \
                 increase the maximum step",
                format_value(max_step, "s"),
                format_value(stop, "s"),
            )));
        }
        // The output grid, `start_save` and the stop time are always kept.
        let signals = Recorder::new(circuit, false).width();
        let grid = (saved / step).ceil() + 2.0;
        if grid * (signals + 1) as f64 > MAX_VALUES as f64 {
            return Err(invalid(format!(
                "a {} transient with an output step of {} keeps {grid} samples of {signals} \
                 signals, more than the {MAX_VALUES} values a result may store; increase the \
                 output step or save a shorter interval",
                format_value(saved, "s"),
                format_value(step, "s"),
            )));
        }
        let mut breakpoints = 0.0;
        for element in &circuit.elements {
            if let Device::VoltageSource(source) | Device::CurrentSource(source) = &element.device
                && let Some(waveform) = &source.waveform
            {
                breakpoints += waveform.breakpoint_count(stop);
            }
        }
        if breakpoints > BREAKPOINT_LIMIT as f64 {
            return Err(invalid(format!(
                "the source waveforms have more than {BREAKPOINT_LIMIT} corners before {stop} s"
            )));
        }
        Ok(Self {
            stop,
            step,
            start_save,
            max_step,
            min_step,
            initial_state,
            max_values: MAX_VALUES,
            thin: true,
        })
    }
}

/// A time the solver must land on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Event {
    time: f64,
    /// A waveform corner: the integrator restarts after it.
    breakpoint: bool,
}

/// Breakpoints, output grid, `start_save` and the stop time.
#[derive(Debug)]
struct Events {
    breakpoints: Vec<f64>,
    next_breakpoint: usize,
    step: f64,
    start_save: f64,
    stop: f64,
    /// Events closer than this are one event.
    resolution: f64,
}

impl Events {
    fn new(mut breakpoints: Vec<f64>, plan: &Plan) -> Self {
        breakpoints.retain(|&t| t > 0.0 && t <= plan.stop);
        breakpoints.sort_by(f64::total_cmp);
        let resolution = plan.min_step;
        breakpoints.dedup_by(|later, earlier| *later - *earlier <= resolution);
        Self {
            breakpoints,
            next_breakpoint: 0,
            step: plan.step,
            start_save: plan.start_save,
            stop: plan.stop,
            resolution,
        }
    }

    /// The first event after `t`.
    fn next_after(&mut self, t: f64) -> Event {
        let horizon = t + self.resolution;
        while self
            .breakpoints
            .get(self.next_breakpoint)
            .is_some_and(|&b| b <= horizon)
        {
            self.next_breakpoint += 1;
        }
        let breakpoint = self.breakpoints.get(self.next_breakpoint).copied();
        let mut time = self.stop.min(self.next_grid(horizon));
        if self.start_save > horizon {
            time = time.min(self.start_save);
        }
        let mut event = Event {
            time,
            breakpoint: false,
        };
        if let Some(b) = breakpoint.filter(|&b| b <= time + self.resolution) {
            event = Event {
                time: b,
                breakpoint: true,
            };
        }
        if self.stop - event.time <= self.resolution {
            event.time = self.stop;
        }
        event
    }

    /// The first multiple of the output step after `horizon` that is not
    /// before `start_save` (`+∞` when there is none).
    fn next_grid(&self, horizon: f64) -> f64 {
        let from = horizon.max(self.start_save - self.resolution);
        let mut k = (from / self.step).floor();
        while k * self.step <= from {
            k += 1.0;
        }
        k * self.step
    }
}

/// The stored samples of a run and the online thinning of D18.
///
/// Every offered sample becomes *pending*. The next offer decides it: the
/// pending sample is dropped when the straight line from the last kept
/// sample (the *anchor*) to the new one passes every dropped sample and the
/// pending one within tolerance, which holds exactly when the new sample's
/// slope from the anchor lies in the intersection of their slope intervals;
/// otherwise it is kept and becomes the anchor. Forced samples (events) are
/// always kept.
struct Store {
    recorder: Recorder,
    times: Vec<f64>,
    /// Values per stored sample: the signals and the time.
    values_per_sample: usize,
    max_values: usize,
    thin: bool,
    reltol: f64,
    /// Absolute tolerance of each signal: vntol for voltages, abstol for
    /// currents.
    absolute: Vec<f64>,
    /// Smallest and largest value of each signal so far.
    low: Vec<f64>,
    high: Vec<f64>,
    /// The last kept sample.
    anchor: Option<f64>,
    anchor_row: Vec<f64>,
    /// The latest offered sample, not kept yet.
    pending: Option<f64>,
    pending_row: Vec<f64>,
    /// Slopes from the anchor at which a line passes every dropped sample
    /// and the pending one within their tolerance.
    slope_low: Vec<f64>,
    slope_high: Vec<f64>,
}

impl Store {
    fn new(recorder: Recorder, plan: &Plan, options: &SimOptions) -> Self {
        let absolute: Vec<f64> = recorder
            .quantities()
            .map(|quantity| match quantity {
                Quantity::Voltage => options.vntol,
                Quantity::Current => options.abstol,
            })
            .collect();
        let width = absolute.len();
        Self {
            recorder,
            times: Vec::new(),
            values_per_sample: width + 1,
            max_values: plan.max_values,
            thin: plan.thin,
            reltol: options.reltol,
            absolute,
            low: vec![f64::INFINITY; width],
            high: vec![f64::NEG_INFINITY; width],
            anchor: None,
            anchor_row: vec![0.0; width],
            pending: None,
            pending_row: vec![0.0; width],
            slope_low: vec![f64::NEG_INFINITY; width],
            slope_high: vec![f64::INFINITY; width],
        }
    }

    /// Offers the accepted sample `row` at `t`; a `forced` sample is kept.
    /// Fails with the time of the sample that would exceed the value limit.
    fn offer(&mut self, t: f64, row: &[f64], forced: bool) -> Result<(), f64> {
        if !self.thin {
            return self.keep(t, row);
        }
        for ((low, high), &v) in self.low.iter_mut().zip(&mut self.high).zip(row) {
            *low = low.min(v);
            *high = high.max(v);
        }
        let Some(anchor) = self.anchor else {
            return self.keep(t, row);
        };
        if let Some(pending) = self.pending.take() {
            let dt = t - anchor;
            let covered = row
                .iter()
                .zip(&self.anchor_row)
                .zip(self.slope_low.iter().zip(&self.slope_high))
                .all(|((&v, &a), (&low, &high))| {
                    let slope = (v - a) / dt;
                    low <= slope && slope <= high
                });
            if !covered {
                let kept = std::mem::take(&mut self.pending_row);
                let outcome = self.keep(pending, &kept);
                self.pending_row = kept;
                outcome?;
            }
        }
        let Some(anchor) = self.anchor.filter(|_| !forced) else {
            return self.keep(t, row);
        };
        let dt = t - anchor;
        let bounds = self.slope_low.iter_mut().zip(&mut self.slope_high);
        let limits = self.low.iter().zip(&self.high).zip(&self.absolute);
        for (((low_slope, high_slope), ((&low, &high), &absolute)), (&v, &a)) in
            bounds.zip(limits).zip(row.iter().zip(&self.anchor_row))
        {
            let tolerance = self.reltol * (high - low) + absolute;
            *low_slope = low_slope.max((v - tolerance - a) / dt);
            *high_slope = high_slope.min((v + tolerance - a) / dt);
        }
        self.pending = Some(t);
        self.pending_row.copy_from_slice(row);
        Ok(())
    }

    fn keep(&mut self, t: f64, row: &[f64]) -> Result<(), f64> {
        if (self.times.len() + 1) * self.values_per_sample > self.max_values {
            return Err(t);
        }
        self.recorder.push(row);
        self.times.push(t);
        self.anchor = Some(t);
        self.anchor_row.copy_from_slice(row);
        self.slope_low.fill(f64::NEG_INFINITY);
        self.slope_high.fill(f64::INFINITY);
        Ok(())
    }

    /// The recorder and sample times, with a sample still pending kept.
    fn finish(mut self) -> (Recorder, Vec<f64>) {
        if let Some(t) = self.pending.take() {
            self.recorder.push(&self.pending_row);
            self.times.push(t);
        }
        (self.recorder, self.times)
    }
}

/// A capacitor (voltage) or inductor (current) whose initial condition a DC
/// solve at the start of a transient holds at `target`.
#[derive(Debug, Clone, Copy)]
struct Held {
    index: usize,
    inductor: bool,
    target: f64,
}

/// The outcome of one step attempt.
enum Attempt {
    /// Accepted, with its truncation-error ratio and the length of the last
    /// solver interval it produced (half the step for a doubled step).
    Accepted { error: f64, last_interval: f64 },
    /// Truncation error too large: the ratio and the order it refers to.
    TooInaccurate { error: f64, order: usize },
    /// Newton–Raphson failed (at a singular matrix, when known).
    Diverged { singular: Option<usize> },
    /// A device changes state within the step at a time it must resolve
    /// (a switch's threshold crossing): retry the step ending at `end`.
    Located { end: f64 },
}

/// What the step control allows one attempt.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// The step ends at an event, whose sample is always kept.
    landed: bool,
    /// The step control cannot shorten the step (see [`crate::integration`]
    /// for the error floor of flux states there).
    minimal: bool,
    /// A device may ask for the step to end earlier
    /// ([`crate::devices::DeviceInstance::locate`]); false where no shorter
    /// retry exists, so a device changes state where the step ends.
    locate: bool,
}

/// Working vectors of a restart step, kept between restarts.
#[derive(Debug, Default)]
struct RestartWork {
    /// The midpoint solution, its sample, and the states after the one
    /// full step.
    midpoint: Vec<f64>,
    middle_row: Vec<f64>,
    full: Vec<f64>,
}

/// The transient run in progress.
struct Run<'a> {
    mna: &'a mut Mna,
    plan: &'a Plan,
    control: &'a SimControl,
    circuit: &'a Circuit,
    store: Store,
    /// Smooth source waveforms (SINE, EXP, SFFM) and the absolute tolerance
    /// of the quantity each one drives.
    sources: Vec<(&'a Waveform, f64)>,
    /// Accepted time and solution.
    t: f64,
    x: Vec<f64>,
    /// Working copies for trial solves.
    trial: Vec<f64>,
    row: Vec<f64>,
    restart_work: RestartWork,
    /// Whether a device changed discontinuously at the last accepted point
    /// (a switch changed state), which restarts the integration there.
    switched: bool,
    /// The time of the first step accepted at the minimum step beyond the
    /// strict tolerance, and the sum of the inductor-current errors of all
    /// such steps (see [`Run::minimal_error`]).
    coarsest: Option<(f64, f64)>,
}

fn simulate(
    circuit: &Circuit,
    mna: &mut Mna,
    plan: &Plan,
    control: &SimControl,
) -> Result<SimulationResult, SimError> {
    let size = mna.size();
    let options = mna.options().clone();
    let sources = circuit
        .elements
        .iter()
        .filter_map(|element| match &element.device {
            Device::VoltageSource(source) => Some((source.waveform.as_ref()?, options.vntol)),
            Device::CurrentSource(source) => Some((source.waveform.as_ref()?, options.abstol)),
            _ => None,
        })
        .filter(|(waveform, _)| {
            matches!(
                waveform,
                Waveform::Sine { .. } | Waveform::Exp { .. } | Waveform::Sffm { .. }
            )
        })
        .collect();
    let mut run = Run {
        store: Store::new(Recorder::new(circuit, false), plan, &options),
        sources,
        t: 0.0,
        x: vec![0.0; size],
        trial: vec![0.0; size],
        row: Vec::new(),
        restart_work: RestartWork::default(),
        switched: false,
        coarsest: None,
        mna,
        plan,
        control,
        circuit,
    };
    run.initial_point()?;
    let outcome = run.integrate();
    run.warn_coarse_edges();
    outcome?;
    let (recorder, times) = run.store.finish();
    Ok(recorder.finish(
        AnalysisKind::Transient,
        Axis {
            name: "time".into(),
            unit: "s".into(),
            values: times,
        },
    ))
}

impl Run<'_> {
    fn options(&self) -> &SimOptions {
        self.mna.options()
    }

    fn progress(&self) -> f64 {
        self.t / self.plan.stop
    }

    /// Establishes the `t = 0` solution and state history.
    fn initial_point(&mut self) -> Result<(), SimError> {
        let conditions = Conditions::dc(self.options(), true);
        match self.plan.initial_state {
            InitialState::OperatingPoint => {
                let released = self
                    .solve_held(conditions, InitialConstraints::Given)?
                    .unwrap_or_default();
                self.check_initial_conditions(&released)?;
            }
            InitialState::ZeroState => {
                let InitialConditions { states, unknowns } = self.mna.initial_conditions();
                for (unknown, value) in unknowns {
                    self.x[unknown] = value;
                }
                if self.plan.start_save == 0.0 {
                    self.record_consistent_start(conditions)?;
                }
                // The run itself continues from the starting vector, so only
                // the t = 0 sample is the consistent solution. Loading there
                // records every state's value at rest (junction charges
                // included); the initial conditions override those of the
                // capacitors and inductors.
                self.mna.load(&self.x, &conditions, false, &[]);
                let integrator = self.mna.integrator_mut();
                for (slot, q) in states {
                    integrator.set_trial_state(slot, q);
                }
            }
        }
        let integrator = self.mna.integrator_mut();
        integrator.commit(0.0);
        integrator.restart();
        self.mna.accept(&self.x, 0.0);
        if self.plan.start_save == 0.0 && self.plan.initial_state == InitialState::OperatingPoint {
            self.record_current_point(true)?;
        }
        Ok(())
    }

    /// Records the `t = 0` sample of a start from rest (T6): the solution
    /// consistent with the initial states, every capacitor held at its
    /// initial voltage, every inductor carrying its initial current and the
    /// sources at their `t = 0` values. The starting vector itself is the
    /// sample when that cannot be solved, and when other devices carry
    /// integrated charge (junction capacitances), which the solve cannot
    /// hold at rest. Leaves `self.x` at the starting vector.
    fn record_consistent_start(&mut self, conditions: Conditions) -> Result<(), SimError> {
        let start = self.x.clone();
        let reactive = self
            .circuit
            .elements
            .iter()
            .filter(|e| matches!(e.device, Device::Capacitor { .. } | Device::Inductor { .. }))
            .count();
        let consistent = self.mna.integrator().slot_count() == reactive
            && self
                .solve_held(conditions, InitialConstraints::All)?
                .is_some();
        if !consistent {
            self.x.copy_from_slice(&start);
            self.mna.load(&self.x, &conditions, false, &[]);
        }
        self.record_current_point(true)?;
        self.x.copy_from_slice(&start);
        Ok(())
    }

    /// Solves the DC point at the start of a transient with `constraints`
    /// (D16) and releases, one at a time in element order, every capacitor
    /// or inductor whose initial condition the solution does not hold, until
    /// every remaining hold holds. A conflict's holding current (a capacitor
    /// held against a voltage source or shorted by an inductor) or shunt
    /// voltage (an inductor held against a current source) thus reaches
    /// neither the samples nor the starting state. Returns the released
    /// elements in release order; `None` when a start from rest's `t = 0`
    /// point cannot be solved, while a failed operating point is an error.
    fn solve_held(
        &mut self,
        conditions: Conditions,
        constraints: InitialConstraints,
    ) -> Result<Option<Vec<usize>>, SimError> {
        self.mna
            .integrator_mut()
            .set_initial_constraints(constraints);
        let outcome = self.release_overridden_holds(conditions, constraints);
        self.mna
            .integrator_mut()
            .set_initial_constraints(InitialConstraints::Off);
        outcome
    }

    fn release_overridden_holds(
        &mut self,
        conditions: Conditions,
        constraints: InitialConstraints,
    ) -> Result<Option<Vec<usize>>, SimError> {
        let guess = self.x.clone();
        let parts = self.held_parts(constraints == InitialConstraints::All);
        let mut released: Vec<usize> = Vec::new();
        // Every round but the last releases a part.
        for _ in 0..=parts.len() {
            if !self.solve_start(conditions, constraints, &guess)? {
                return Ok(None);
            }
            let errors: Vec<(Held, f64, bool)> = parts
                .iter()
                .filter(|part| !released.contains(&part.index))
                .map(|&part| {
                    let (error, tolerance) = self.hold_error(part);
                    (part, error, error > tolerance)
                })
                .collect();
            // An inductor's shunt diverts a little current wherever the
            // circuit puts a voltage across the inductor; its hold is
            // overridden only when the diverted current does not grow with
            // the shunt.
            let suspects: Vec<(Held, f64)> = errors
                .iter()
                .filter(|&&(part, _, off)| part.inductor && off)
                .map(|&(part, error, _)| (part, error))
                .collect();
            let mut overridden: Vec<usize> = Vec::new();
            if !suspects.is_empty() {
                for (part, _) in &suspects {
                    self.mna.devices_mut()[part.index].set_hold(Hold::Probe);
                }
                let probed = self.solve_start(conditions, constraints, &guess)?;
                for &(part, diverted) in &suspects {
                    self.mna.devices_mut()[part.index].set_hold(Hold::Firm);
                    if !probed || self.hold_error(part).0 < PROBE_GROWTH * diverted {
                        overridden.push(part.index);
                    }
                }
            }
            let first = errors.iter().find(|&&(part, _, off)| {
                off && (!part.inductor || overridden.contains(&part.index))
            });
            match first {
                Some(&(part, _, _)) => {
                    self.mna.devices_mut()[part.index].set_hold(Hold::Released);
                    released.push(part.index);
                }
                None => {
                    // The probe left its own solution behind.
                    if !suspects.is_empty() && !self.solve_start(conditions, constraints, &guess)? {
                        return Ok(None);
                    }
                    return Ok(Some(released));
                }
            }
        }
        Ok(Some(released))
    }

    /// One DC solve at the start of a transient from `guess`: the operating
    /// point for [`InitialConstraints::Given`] (a failure is an error), the
    /// `t = 0` point of a start from rest otherwise (`false` on failure).
    fn solve_start(
        &mut self,
        conditions: Conditions,
        constraints: InitialConstraints,
        guess: &[f64],
    ) -> Result<bool, SimError> {
        self.x.copy_from_slice(guess);
        if constraints == InitialConstraints::Given {
            newton::operating_point(self.mna, &mut self.x, conditions, true, self.control)
                .map_err(|failure| dc_error(failure, self.mna, Some(0.0), 0.0))?;
            return Ok(true);
        }
        let request = Request {
            conditions,
            init_junctions: true,
            max_iterations: self.options().dc_iterations,
            anchor: &[],
        };
        match newton::solve(self.mna, &mut self.x, &request, self.control) {
            Ok(_) => Ok(self.x.iter().all(|v| v.is_finite())),
            Err(NewtonError::Interrupted(error)) => Err(error),
            Err(_) => Ok(false),
        }
    }

    /// The capacitors and inductors a DC solve at the start of a transient
    /// holds: those with an initial condition, and with `all` every other one
    /// at zero.
    fn held_parts(&self, all: bool) -> Vec<Held> {
        self.circuit
            .elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| {
                let (inductor, target) = match element.device {
                    Device::Capacitor {
                        initial_voltage, ..
                    } => (false, initial_voltage),
                    Device::Inductor {
                        initial_current, ..
                    } => (true, initial_current),
                    _ => return None,
                };
                let target = if all {
                    Some(target.unwrap_or(0.0))
                } else {
                    target
                }?;
                Some(Held {
                    index,
                    inductor,
                    target,
                })
            })
            .collect()
    }

    /// How far the solution is from holding `part`, and the tolerance of
    /// that: the capacitor's voltage error against `vntol`, or the current
    /// the inductor's shunt diverts against `abstol` (both plus reltol).
    fn hold_error(&self, part: Held) -> (f64, f64) {
        let options = self.options();
        let (actual, absolute) = if part.inductor {
            (self.element_current(part.index), options.abstol)
        } else {
            (self.element_voltage(part.index), options.vntol)
        };
        (
            (actual - part.target).abs(),
            options.reltol * actual.abs().max(part.target.abs()) + absolute,
        )
    }

    /// Current into the positive terminal of two-terminal element `index`
    /// at the last load.
    fn element_current(&self, index: usize) -> f64 {
        let mut current = [0.0; 2];
        self.mna.currents(index, &mut current);
        current[0]
    }

    /// Voltage across two-terminal element `index` in the solution.
    fn element_voltage(&self, index: usize) -> f64 {
        let nodes = &self.circuit.elements[index].nodes;
        self.node_voltage(nodes[0]) - self.node_voltage(nodes[1])
    }

    /// Reports the initial conditions the operating point could not hold
    /// (D16): a capacitor or inductor that other parts override is a warning
    /// naming the value the transient starts from, an initial current with
    /// no path at all an error.
    fn check_initial_conditions(&self, released: &[usize]) -> Result<(), SimError> {
        let options = self.options();
        let mut released = released.to_vec();
        released.sort_unstable();
        for index in released {
            let element = &self.circuit.elements[index];
            match element.device {
                Device::Capacitor {
                    initial_voltage: Some(v0),
                    ..
                } => {
                    let v = self.element_voltage(index);
                    self.control.warn(format!(
                        "{}: its initial voltage of {} cannot hold at the start of the \
                         transient, because other parts fix the voltage across it at {}; the \
                         transient starts from {} (Start from rest skips the operating point and \
                         starts the capacitor at {})",
                        element.name,
                        format_value(v0, "V"),
                        format_value(v, "V"),
                        format_value(v, "V"),
                        format_value(v0, "V"),
                    ));
                }
                Device::Inductor {
                    initial_current: Some(i0),
                    ..
                } => {
                    let i = self.element_current(index);
                    if i.abs() <= options.reltol * i0.abs() + options.abstol {
                        return Err(invalid(format!(
                            "{}: its initial current of {} cannot flow at the start of the \
                             transient: the operating point has no path for it (capacitors \
                             without an initial voltage are open there). Give the capacitors \
                             around it an initial voltage, or start from rest",
                            element.name,
                            format_value(i0, "A"),
                        )));
                    }
                    self.control.warn(format!(
                        "{}: its initial current of {} cannot flow at the start of the transient, \
                         because other parts force {} through it; the transient starts from {} \
                         (Start from rest skips the operating point and starts the inductor at {})",
                        element.name,
                        format_value(i0, "A"),
                        format_value(i, "A"),
                        format_value(i, "A"),
                        format_value(i0, "A"),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Voltage of circuit node `node` in the accepted solution.
    fn node_voltage(&self, node: NodeIndex) -> f64 {
        self.mna.layout().node(node).map_or(0.0, |u| self.x[u])
    }

    /// Steps from the initial point to the stop time.
    fn integrate(&mut self) -> Result<(), SimError> {
        let mut events = Events::new(self.mna.breakpoints(self.plan.stop), self.plan);
        let method = self.mna.integrator().method();
        let order = integration::order(method);
        let mut restart = true;
        // The step the controller took before the latest cluster of
        // breakpoints; every restart begins at a fraction of it.
        let mut cruise = self.plan.max_step.min(self.plan.step);
        // Step the controller wants next, before the maximum step and event
        // landing limit it.
        let mut proposal = self.restart_step(cruise);
        let mut rejected = false;
        // Whether the step being retried is the minimum step itself.
        let mut at_minimum = false;
        let mut last_singular = None;
        // The end of the last attempt a device asked to end earlier.
        let mut located_from: Option<f64> = None;
        while self.t < self.plan.stop {
            self.control.set_progress(self.progress());
            self.control.checkpoint()?;
            let event = events.next_after(self.t);
            let wanted = if rejected {
                proposal
            } else {
                proposal.max(self.plan.min_step)
            };
            let (h, end) = self.limit_step(wanted, event);
            if h < self.plan.min_step {
                return Err(self.step_too_small(last_singular));
            }
            let landed = end == event.time;
            // The shortest step the control can take here: the minimum
            // step, or the retry of a rejection below it, which event
            // landing may stretch to the rest of the way to the event.
            let minimal = at_minimum || h <= self.plan.min_step * (1.0 + 1e-9);
            // A device may ask for a shorter retry only where one exists:
            // within two minimum steps of an event every retry is stretched
            // back to the event (`limit_step`), and a retry must end before
            // the step it shortens. Otherwise the device changes state where
            // the step ends.
            let locate = event.time - self.t >= 2.0 * self.plan.min_step
                && located_from.is_none_or(|previous| end < previous);
            let limits = Limits {
                landed,
                minimal,
                locate,
            };
            let attempt = if restart {
                self.doubled_step(h, end, limits)?
            } else {
                self.regular_step(method, h, end, limits)?
            };
            located_from = None;
            match attempt {
                Attempt::Accepted {
                    error,
                    last_interval,
                } => {
                    rejected = false;
                    at_minimum = false;
                    last_singular = None;
                    let attempted_order = if restart { 1 } else { order };
                    proposal = last_interval * growth(error, attempted_order).min(MAX_GROWTH);
                    if !restart {
                        cruise = proposal;
                    }
                    restart = (landed && event.breakpoint) || self.switched;
                    if self.switched {
                        self.mna.integrator_mut().restart_reference();
                    } else if restart {
                        self.mna.integrator_mut().restart();
                    }
                    if restart {
                        proposal = self.restart_step(cruise);
                    }
                }
                Attempt::TooInaccurate { error, order } => {
                    rejected = true;
                    proposal = h * growth(error, order).max(MAX_SHRINK);
                }
                Attempt::Located { end: at } => {
                    // Not a rejection for accuracy: the retry is never below
                    // the minimum step (where rounding could put it).
                    rejected = true;
                    located_from = Some(end);
                    proposal = (at - self.t).max(self.plan.min_step);
                }
                Attempt::Diverged { singular: row } => {
                    if let Some(row) = row
                        && self.mna.is_linear()
                    {
                        // A linear circuit's matrix is singular at every step.
                        return Err(singular(self.mna, row, self.progress()));
                    }
                    rejected = true;
                    last_singular = row;
                    proposal = h / NEWTON_SHRINK;
                }
            }
            if rejected && proposal < self.plan.min_step {
                // As in SPICE, the minimum step itself is tried once before
                // the run gives up: the shrink assumes the error scales with
                // the method's order, which overshoots where a source ramp
                // makes it scale faster.
                if at_minimum {
                    return Err(self.step_too_small(last_singular));
                }
                at_minimum = true;
                proposal = self.plan.min_step;
            }
        }
        Ok(())
    }

    /// The error of a step rejected at the minimum step: the singular row
    /// of the last failed solve when known, else a timestep too small.
    fn step_too_small(&self, last_singular: Option<usize>) -> SimError {
        match last_singular {
            Some(row) => singular(self.mna, row, self.progress()),
            None => SimError::TimestepTooSmall {
                time: self.t,
                progress: self.progress(),
            },
        }
    }

    /// The error ratio that decides a step: `strict`, or for a step that
    /// cannot be shortened and fails it, the `relaxed` ratio, in which an
    /// inductor's flux may move by the dual of `chgtol` within the step (see
    /// [`crate::integration`]), with the inductor-current error the step's
    /// flux errors imply. An edge faster than the minimum step is then
    /// integrated as precisely as the minimum step allows instead of
    /// stopping the run, and the run warns at its end, with the sum of those
    /// current errors, that the result there is coarser than `abstol`
    /// ([`Run::warn_coarse_edges`]).
    fn minimal_error(&mut self, strict: f64, relaxed: Option<(f64, f64)>) -> f64 {
        let Some((error, current)) = relaxed else {
            return strict;
        };
        if error <= 1.0 {
            let (first, total) = self.coarsest.unwrap_or((self.t, 0.0));
            self.coarsest = Some((first, total + current));
        }
        error
    }

    /// Warns about the edges integrated at the minimum step beyond the
    /// strict tolerance (see [`Run::minimal_error`]), naming the first one
    /// and the inductor-current resolution all of them imply together.
    fn warn_coarse_edges(&self) {
        let Some((time, current)) = self.coarsest else {
            return;
        };
        self.control.warn(format!(
            "At {} the transient ran at its minimum timestep ({}) on an edge too fast for it: \
             an inductor current there carries an estimated error of at least ±{}, coarser \
             than abstol ({}). A smaller min_step_ratio resolves the edge",
            format_value(time, "s"),
            format_value(self.plan.min_step, "s"),
            format_value(current, "A"),
            format_value(self.options().abstol, "A"),
        ));
    }

    /// The first step after a restart, from the step taken before it.
    fn restart_step(&self, cruise: f64) -> f64 {
        (cruise * RESTART_FRACTION).max(RESTART_FLOOR * self.plan.min_step)
    }

    /// The step actually taken towards `event` for a wanted step `h`, and
    /// the time it ends at.
    fn limit_step(&self, wanted: f64, event: Event) -> (f64, f64) {
        let h = wanted.min(self.plan.max_step).min(self.source_limit());
        let remaining = event.time - self.t;
        let half = remaining / 2.0;
        if h >= remaining || (h > half && half < self.plan.min_step) {
            (remaining, event.time)
        } else if h > half {
            (half, self.t + half)
        } else {
            (h, self.t + h)
        }
    }

    /// Longest step from `self.t` over which linear interpolation reproduces
    /// every smooth source waveform within the result tolerance, while
    /// results are saved; never below the minimum step.
    fn source_limit(&self) -> f64 {
        if self.t < self.plan.start_save {
            return f64::INFINITY;
        }
        let reltol = self.options().reltol;
        self.sources
            .iter()
            .map(|&(waveform, absolute)| waveform.interpolation_step(self.t, reltol, absolute))
            .fold(f64::INFINITY, f64::min)
            .max(self.plan.min_step)
    }

    /// One Newton solve of a timestep ending at `time`, starting from `guess`
    /// and leaving the solution in it. Returns the failed attempt, if any.
    fn solve_at(
        &mut self,
        formula: Formula,
        time: f64,
        guess: &mut [f64],
    ) -> Result<Option<Attempt>, SimError> {
        self.mna.integrator_mut().begin_step(formula, time);
        let request = Request {
            conditions: Conditions::transient(self.options(), time),
            init_junctions: false,
            max_iterations: self.options().tran_iterations,
            anchor: &[],
        };
        match newton::solve(self.mna, guess, &request, self.control) {
            Ok(_) => Ok(None),
            Err(NewtonError::Interrupted(error)) => Err(error),
            Err(NewtonError::Singular(row)) => Ok(Some(Attempt::Diverged {
                singular: Some(row),
            })),
            Err(NewtonError::NoConvergence) => Ok(Some(Attempt::Diverged { singular: None })),
        }
    }

    /// A step of `method` from `self.t` to `end` within `limits`.
    fn regular_step(
        &mut self,
        method: IntegrationMethod,
        h: f64,
        end: f64,
        limits: Limits,
    ) -> Result<Attempt, SimError> {
        let Limits {
            landed,
            minimal,
            locate,
        } = limits;
        let stages = integration::stages(method);
        let mut trial = std::mem::take(&mut self.trial);
        trial.copy_from_slice(&self.x);
        let mut committed = 0;
        let mut outcome = None;
        for (index, stage) in stages.iter().enumerate() {
            let last = index + 1 == stages.len();
            let time = if last {
                end
            } else {
                self.t + stage.fraction * h
            };
            match self.solve_at(stage.formula, time, &mut trial) {
                Ok(Some(failure)) => {
                    outcome = Some(failure);
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    self.trial = trial;
                    return Err(error);
                }
            }
            if locate && let Some(end) = self.mna.locate(&trial, time, self.plan.min_step) {
                outcome = Some(Attempt::Located { end });
                break;
            }
            if !last {
                self.mna.integrator_mut().commit(time);
                committed += 1;
            }
        }
        let outcome = outcome.unwrap_or_else(|| {
            // Enough history always exists after the restart step.
            let ratio = |relaxed| {
                self.mna
                    .integrator()
                    .truncation_error_ratio(h, self.options(), relaxed)
                    .unwrap_or(0.0)
            };
            let strict = ratio(false);
            let relaxed = (minimal && strict > 1.0).then(|| {
                let current = self.mna.integrator().current_error(h, Estimate::Regular);
                (ratio(true), current)
            });
            let error = self.minimal_error(strict, relaxed);
            if error > 1.0 {
                Attempt::TooInaccurate {
                    error,
                    order: integration::order(method),
                }
            } else {
                Attempt::Accepted {
                    error,
                    last_interval: h,
                }
            }
        });
        if let Attempt::Accepted { .. } = outcome {
            self.mna.integrator_mut().commit(end);
            self.t = end;
            self.x.copy_from_slice(&trial);
            self.switched = self.mna.accept(&self.x, end);
            self.trial = trial;
            self.record_current_point(landed || self.switched)?;
        } else {
            for _ in 0..committed {
                self.mna.integrator_mut().rollback();
            }
            self.trial = trial;
        }
        Ok(outcome)
    }

    /// The restart step: backward Euler over `h`, checked by repeating it as
    /// two half steps (see [`crate::integration`] for its error estimate).
    /// The half-step solutions are kept.
    fn doubled_step(&mut self, h: f64, end: f64, limits: Limits) -> Result<Attempt, SimError> {
        let mut trial = std::mem::take(&mut self.trial);
        let mut work = std::mem::take(&mut self.restart_work);
        let outcome = self.try_doubled_step(h, end, limits, &mut trial, &mut work);
        self.trial = trial;
        self.restart_work = work;
        outcome
    }

    fn try_doubled_step(
        &mut self,
        h: f64,
        end: f64,
        limits: Limits,
        trial: &mut [f64],
        work: &mut RestartWork,
    ) -> Result<Attempt, SimError> {
        let Limits {
            landed,
            minimal,
            locate,
        } = limits;
        let RestartWork {
            midpoint,
            full,
            middle_row,
        } = work;
        trial.copy_from_slice(&self.x);
        if let Some(failure) = self.solve_at(Formula::BackwardEuler, end, trial)? {
            return Ok(failure);
        }
        full.clear();
        full.extend_from_slice(self.mna.integrator().trial_states());

        let middle = self.t + h / 2.0;
        midpoint.clear();
        midpoint.extend_from_slice(&self.x);
        if let Some(failure) = self.solve_at(Formula::BackwardEuler, middle, midpoint)? {
            return Ok(failure);
        }
        let min_step = self.plan.min_step;
        if locate && let Some(end) = self.mna.locate(midpoint, middle, min_step) {
            return Ok(Attempt::Located { end });
        }
        self.store
            .recorder
            .sample_into(self.mna, midpoint, middle_row);
        self.mna.integrator_mut().commit(middle);

        trial.copy_from_slice(midpoint);
        if let Some(failure) = self.solve_at(Formula::BackwardEuler, end, trial)? {
            self.mna.integrator_mut().rollback();
            return Ok(failure);
        }
        if locate && let Some(end) = self.mna.locate(trial, end, min_step) {
            self.mna.integrator_mut().rollback();
            return Ok(Attempt::Located { end });
        }
        // The half steps' curvature asks for a shorter step where the step
        // spans a decay far faster than itself; a step the control cannot
        // shorten leaves it out (a decay faster than the minimum step).
        let curved = !minimal;
        let ratio = |relaxed| {
            self.mna.integrator().step_doubling_error_ratio(
                full,
                h,
                self.options(),
                relaxed,
                curved,
            )
        };
        let strict = ratio(false);
        let relaxed = (minimal && strict > 1.0).then(|| {
            let estimate = Estimate::Doubled {
                full,
                curved: false,
            };
            let current = self.mna.integrator().current_error(h, estimate);
            (ratio(true), current)
        });
        let error = self.minimal_error(strict, relaxed);
        if error > 1.0 {
            self.mna.integrator_mut().rollback();
            return Ok(Attempt::TooInaccurate { error, order: 1 });
        }

        let integrator = self.mna.integrator_mut();
        integrator.commit(end);
        if matches!(
            integrator.method(),
            IntegrationMethod::Trapezoidal | IntegrationMethod::ModifiedTrapezoidal
        ) {
            integrator.refine_latest_derivatives();
        }
        self.mna.accept(midpoint, middle);
        self.switched = self.mna.accept(trial, end);
        if middle >= self.plan.start_save {
            self.store
                .offer(middle, middle_row, false)
                .map_err(|time| self.too_large(time))?;
        }
        self.t = end;
        self.x.copy_from_slice(trial);
        self.record_current_point(landed || self.switched)?;
        Ok(Attempt::Accepted {
            error,
            last_interval: h / 2.0,
        })
    }

    /// Offers the accepted point `(self.t, self.x)` to the result if it is
    /// saved; a `forced` point is always kept. The devices are loaded at it.
    fn record_current_point(&mut self, forced: bool) -> Result<(), SimError> {
        if self.t >= self.plan.start_save {
            let mut row = std::mem::take(&mut self.row);
            self.store.recorder.sample_into(self.mna, &self.x, &mut row);
            let outcome = self.store.offer(self.t, &row, forced);
            self.row = row;
            outcome.map_err(|time| self.too_large(time))?;
        }
        Ok(())
    }

    /// The error of a result that reached its value limit at `time`.
    fn too_large(&self, time: f64) -> SimError {
        SimError::ResultTooLarge {
            time,
            progress: time / self.plan.stop,
            limit: self.plan.max_values,
        }
    }
}

/// Step-size factor that brings an error ratio of `error` for a method of
/// `order` to the target ratio.
fn growth(error: f64, order: usize) -> f64 {
    if error <= 0.0 {
        return f64::INFINITY;
    }
    (ERROR_TARGET[order - 1] / error).powf(1.0 / order as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::IntegrationMethod;
    use crate::circuit::{Element, GROUND};
    use crate::devices;
    use crate::newton::test_support::Junction;
    use crate::source::{Source, Waveform};

    /// A 5 V pulse through 1 kΩ into a junction with 1 nF across it. The
    /// junction's place in the circuit description is held by a resistor.
    fn clamp() -> Circuit {
        let pulse = Waveform::Pulse {
            initial: 0.0,
            pulsed: 5.0,
            delay: 1e-6,
            rise: 1e-6,
            fall: 1e-6,
            width: 5e-6,
            period: 0.0,
            cycles: None,
        };
        let mut c = Circuit::new();
        let (a, d) = (c.node("a"), c.node("d"));
        c.add(Element::new(
            "V1",
            [a, GROUND],
            Device::VoltageSource(Source::with_waveform(pulse)),
        ));
        c.add(Element::new("R1", [a, d], Device::resistor(1e3)));
        c.add(Element::new("D1", [d, GROUND], Device::resistor(1.0)));
        c.add(Element::new("C1", [d, GROUND], Device::capacitor(1e-9)));
        c
    }

    fn run_clamp(options: &SimOptions, thin: bool) -> Result<SimulationResult, SimError> {
        let circuit = clamp();
        let spec = Transient {
            stop: 20e-6,
            step: 1e-6,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        };
        let plan = Plan {
            thin,
            ..Plan::new(&circuit, &spec, options)?
        };
        let devices = (0..circuit.elements.len())
            .map(|i| match i {
                2 => Box::new(Junction::new(2, GROUND, None)) as Box<dyn devices::DeviceInstance>,
                _ => devices::build(&circuit, i, options),
            })
            .collect();
        let mut mna = Mna::with_devices(&circuit, devices, options);
        simulate(&circuit, &mut mna, &plan, &SimControl::new())
    }

    fn options(iterations: usize, min_step_ratio: f64) -> SimOptions {
        SimOptions {
            tran_iterations: iterations,
            min_step_ratio,
            method: IntegrationMethod::Trapezoidal,
            ..SimOptions::default()
        }
    }

    /// Runs `spec` on `circuit` with the plan adjusted by `adjust`.
    fn run_plan(
        circuit: &Circuit,
        spec: &Transient,
        options: &SimOptions,
        adjust: impl FnOnce(&mut Plan),
    ) -> Result<SimulationResult, SimError> {
        let mut plan = Plan::new(circuit, spec, options)?;
        adjust(&mut plan);
        let mut mna = Mna::new(circuit, options);
        simulate(circuit, &mut mna, &plan, &SimControl::new())
    }

    /// 1 V, 1 kΩ, 1 µF.
    fn rc() -> Circuit {
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

    /// A pulse-driven parallel RLC: corners, ringing and decay.
    fn pulsed_rlc() -> Circuit {
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
        let mut c = Circuit::new();
        let (a, b) = (c.node("in"), c.node("out"));
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

    #[test]
    fn newton_failures_shrink_the_step_without_changing_the_answer() {
        let reference = run_clamp(&options(50, 1e-15), false).unwrap();
        // Two iterations cannot follow the limited junction through the
        // edges, so some steps fail and are retried at an eighth.
        let constrained = run_clamp(&options(2, 1e-15), false).unwrap();
        assert_ne!(
            constrained.len(),
            reference.len(),
            "the step sequence must differ"
        );
        let (a, b) = (
            reference.signal("V(d)").unwrap(),
            constrained.signal("V(d)").unwrap(),
        );
        for k in 0..=20 {
            let t = k as f64 * 1e-6;
            // Pulse corners replace grid times within the event resolution.
            let index = |r: &SimulationResult| {
                r.axis
                    .values
                    .iter()
                    .position(|&x| (x - t).abs() < 1e-15)
                    .unwrap()
            };
            let (x, y) = (
                a.real().unwrap()[index(&reference)],
                b.real().unwrap()[index(&constrained)],
            );
            assert!((x - y).abs() < 1e-6, "at {t}: {x} vs {y}");
        }
    }

    #[test]
    fn persistent_newton_failure_ends_in_timestep_too_small() {
        // A single iteration only converges when the step barely moves the
        // solution, which the 2 ns minimum step does not allow on the edge.
        match run_clamp(&options(1, 1e-4), true) {
            Err(SimError::TimestepTooSmall { time, progress }) => {
                assert!((1e-6..2e-6).contains(&time), "failed at {time}");
                assert_eq!(progress, time / 20e-6);
            }
            other => panic!("expected a timestep failure, got {other:?}"),
        }
    }

    #[test]
    fn dropped_points_are_reproduced_by_interpolation_within_tolerance() {
        let circuit = pulsed_rlc();
        let spec = Transient {
            stop: 200e-6,
            step: 10e-6,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        };
        let options = SimOptions::default();
        let full = run_plan(&circuit, &spec, &options, |plan| plan.thin = false).unwrap();
        let thinned = run_plan(&circuit, &spec, &options, |_| {}).unwrap();
        assert!(
            thinned.len() * 3 < full.len(),
            "{} of {} points kept",
            thinned.len(),
            full.len()
        );
        // The kept points are a subset of the accepted ones.
        let times = &thinned.axis.values;
        assert!(times.iter().all(|t| full.axis.values.contains(t)));
        for (signal, kept) in full.signals.iter().zip(&thinned.signals) {
            let (values, kept) = (signal.real().unwrap(), kept.real().unwrap());
            let swing = values.iter().copied().fold(f64::MIN, f64::max)
                - values.iter().copied().fold(f64::MAX, f64::min);
            let absolute = match signal.quantity {
                Quantity::Voltage => options.vntol,
                Quantity::Current => options.abstol,
            };
            let tolerance = options.reltol * swing + absolute;
            for (&t, &v) in full.axis.values.iter().zip(values) {
                let k = times.partition_point(|&x| x <= t).clamp(1, times.len() - 1);
                let (t0, t1) = (times[k - 1], times[k]);
                let line = kept[k - 1] + (kept[k] - kept[k - 1]) * (t - t0) / (t1 - t0);
                assert!(
                    (line - v).abs() <= tolerance * (1.0 + 1e-9),
                    "{} at {t}: {line} vs {v} (tolerance {tolerance:e})",
                    signal.name
                );
            }
        }
    }

    #[test]
    fn a_result_beyond_its_value_limit_stops_with_its_progress() {
        let spec = Transient {
            stop: 200e-6,
            step: 10e-6,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        };
        let circuit = pulsed_rlc();
        // Two nodes, four currents and the time axis: room for 100 samples.
        let error = run_plan(&circuit, &spec, &SimOptions::default(), |plan| {
            plan.max_values = 700;
        })
        .unwrap_err();
        match error {
            SimError::ResultTooLarge {
                time,
                progress,
                limit,
            } => {
                assert_eq!(limit, 700);
                assert!(time > 0.0 && time < 200e-6, "{time}");
                assert_eq!(progress, time / 200e-6);
                let text = error.to_string();
                assert!(
                    text.contains("% complete") && text.contains("700"),
                    "{text}"
                );
            }
            other => panic!("expected a size failure, got {other:?}"),
        }
    }

    #[test]
    fn the_default_max_step_is_a_fiftieth_of_the_saved_interval() {
        let spec = Transient {
            stop: 50.0,
            step: 50.0,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        };
        let r = run_plan(&rc(), &spec, &SimOptions::default(), |plan| {
            plan.thin = false
        })
        .unwrap();
        assert!(
            r.axis
                .values
                .windows(2)
                .all(|w| w[1] - w[0] <= 1.0 * (1.0 + 1e-9))
        );
        assert!(r.len() >= 51);
    }

    #[test]
    fn steps_respect_an_explicit_max_step() {
        let spec = Transient {
            stop: 5e-3,
            step: 0.5e-3,
            start_save: 1.25e-3,
            max_step: Some(20e-6),
            initial_state: InitialState::ZeroState,
        };
        let r = run_plan(&rc(), &spec, &SimOptions::default(), |plan| {
            plan.thin = false
        })
        .unwrap();
        assert_eq!(r.axis.values[0], 1.25e-3);
        assert!(
            r.axis
                .values
                .windows(2)
                .all(|w| w[1] - w[0] <= 20e-6 * (1.0 + 1e-9))
        );
    }
}
