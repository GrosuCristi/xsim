//! Numerical integration of device state quantities and local truncation
//! error estimation (Q105, Q106).
//!
//! Devices describe reactive behaviour by a state `q(x)` (charge or flux)
//! and receive its time derivative from [`Integrator::integrate`]. Every
//! method is written as
//!
//! ```text
//! dq/dt(tₙ₊₁) ≈ a₀·qₙ₊₁ + b₀·qₙ + b₁·qₙ₋₁ + c₀·(dq/dt)ₙ
//! ```
//!
//! with coefficients from the step sizes ([`Formula`]):
//!
//! | Method | Stages | Order | Error current |
//! |---|---|---|---|
//! | Backward Euler | BE | 1 | `h/2 · q''` |
//! | Trapezoidal | TR | 2 | `h²/12 · q'''` |
//! | Gear-2 | variable-step BDF2 | 2 | `h(h+h₁)/6 · q'''` |
//! | Modified trapezoidal | TR over `γh`, then BDF2 over the rest | 2 | `|k|·h² · q'''` |
//!
//! The modified trapezoidal method is TR-BDF2 with `γ = 2 − √2` (Bank et
//! al., 1985): the BDF2 stage through `tₙ`, `tₙ + γh` and `tₙ₊₁` damps the
//! oscillation that pure trapezoidal integration leaves after a
//! discontinuity, while keeping second-order accuracy. This `γ` gives both
//! stages the same `a₀`, and the error constant is
//! `k = (−3γ² + 4γ − 2) / (12(2 − γ)) ≈ −0.0404`.
//!
//! ## Truncation error
//! Error is controlled per unit time. Each step's contribution to the
//! long-run error of a state is expressed as an error *current* (table
//! above), with the `(p+1)`-th derivative estimated from divided differences
//! over the newest `p + 2` points of the state's history (TR-BDF2's
//! intermediate stages included). For the one-step methods this is the local
//! error divided by the step. For BDF2 it is the formula's residual, whose
//! constant (1/3 at a constant step) exceeds the 2/9 of its one-step local
//! error: a multistep method's global error accumulates the residual. The
//! error current is compared with
//!
//! ```text
//! trtol · (abstol + reltol · peak|dq/dt|) + floor / (t − t_restart) + 1e-13 · |q| / h
//! floor = max(chgtol, |dq/dx| · vntol)      (abstol instead of vntol for a flux)
//! ```
//!
//! (a flux at the minimum step: see the third point below).
//!
//! * Using the largest derivative magnitude seen so far (rather than the
//!   instantaneous one) makes the tolerance relative to the state's
//!   largest rate of change: the "signal-global" reference used by Spectre.
//!   This bounds the error each state accumulates while it settles, but
//!   not over arbitrarily long runs: the phase error of a lightly damped
//!   oscillation grows with the number of periods simulated (measured in
//!   `analyses::transient`: about `0.35 · reltol · amplitude` per period
//!   with the trapezoidal rule and Gear-2, 0.28 with TR-BDF2, at the
//!   transient's error target). "So far" starts again after a switch
//!   changes state ([`Integrator::restart_reference`]): a 300 A discharge
//!   must not set the tolerance of the microampere recharge after it.
//! * `floor` is an absolute charge error spread over the time since the
//!   last restart. Right after a breakpoint every derivative starts from
//!   zero, where a purely relative tolerance would never admit a first-order
//!   step; this term admits steps that move a negligible charge (like
//!   SPICE's `chgtol/h` on the first step) while its total contribution over
//!   a run grows only with the logarithm of the step count. It is `chgtol`,
//!   or for a capacitor above `chgtol / vntol` (100 nF) the charge that
//!   moves its voltage by vntol, and for an inductor above `chgtol / abstol`
//!   (100 µH) the flux that moves its current by abstol: a fixed 1e-16 C
//!   would ask for steps below the minimum step where an edge drives
//!   amperes per picosecond into a large capacitor (T3). For capacitors
//!   below 100 nF `chgtol` itself still dominates, and its charge is no
//!   longer negligible on a picofarad: at the default tolerances a 1 V step
//!   into 1 kΩ and 1 pF from rest deviates from the closed form by 5e-6 to
//!   9e-6 V (up to 3e-4 V on 1 fF), against below 1e-7 V from 1 nF up.
//! * An edge across a smaller inductor can still ask for steps below the
//!   minimum step, because its flux follows the full voltage slew: the
//!   restart step at the corner of a 1 V, 1 ps edge moves the flux by
//!   `slew·h²/4` = 2.5e-15 V·s at the 0.1 ps minimum step of a 100 s run,
//!   more than the floor of any inductor below 2.5 mH. A step the control
//!   cannot shorten (the minimum step) may therefore move a flux state by
//!   the flux dual of `chgtol` within the step itself:
//!   `max(chgtol·vntol/abstol, |dφ/di|·abstol) / h`, the floor of the dual
//!   capacitor at the impedance level `vntol/abstol` (1 kΩ). Charges keep
//!   their floor there, so a step that fails it still stops the run. The
//!   transient warns with the inductor-current error such steps imply (2.5 pA
//!   for 1 mH and 2.9 nA for 1 µH in that example, where the 1 µH current is
//!   measured 3.8 nA off): the edge is integrated as precisely as the
//!   minimum step allows, not to `abstol`.
//! * The last term sits just above the rounding noise of the state history
//!   and keeps a constant charge from demanding ever smaller steps.
//!
//! ## Restarts
//! After `t = 0`, every breakpoint and every change of a switch's state,
//! history before the restart point is ignored ([`Integrator::restart`]).
//! The transient driver then takes one step-doubled backward-Euler step,
//! which needs no history to estimate its error, before the method's own
//! formulas take over. Its error is the larger of two estimates of the
//! half steps' error current ([`Integrator::step_doubling_error_ratio`]):
//! the difference between one full step and the two half steps, and the
//! curvature through the restart point, the midpoint and the end. Where the
//! circuit settles far faster than the step (a 1 ns time constant behind an
//! ideal edge, with a restart step of a millisecond), both backward-Euler
//! results have settled and their difference vanishes, although the step
//! leaves the waveform between its points unresolved; the curvature does
//! not vanish, so such a step is shortened until it resolves the decay. A
//! state that jumps at the restart point and then moves smoothly (a
//! capacitor that a source or a capacitive divider charges at once) keeps
//! the difference alone: its curvature is the jump itself, which no shorter
//! step resolves, and the two estimates tell the cases apart
//! ([`STIFF_RATIO`]).

use crate::analysis::{IntegrationMethod, SimOptions};
use crate::devices::{InitialConstraints, LoadMode, StateSlot};

/// Accepted points kept per state: order-2 error estimates need the newest
/// point plus three before it, and one more lets the most recent point be
/// rolled back.
const HISTORY: usize = 5;

/// How the error of a pending point is estimated.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Estimate<'a> {
    /// A regular step of the method, from the state histories.
    Regular,
    /// A step-doubled backward-Euler step: the states after the one full
    /// step (`full`), against the pending point after two half steps;
    /// `curved`: the half steps' curvature counts too (see
    /// [`Integrator::step_doubling_error_ratio`]).
    Doubled { full: &'a [f64], curved: bool },
}

/// Fraction of each step covered by the trapezoidal stage of TR-BDF2.
pub(crate) const TR_BDF2_GAMMA: f64 = 2.0 - std::f64::consts::SQRT_2;

/// Relative rounding noise of state values in the truncation-error floor.
const STATE_ROUNDOFF: f64 = 1e-13;

/// The curvature of a restart step counts for a state only where its full
/// step and two half steps differ by more than this fraction of the second
/// half step's change: a decay of time constant `τ` gives the ratio
/// `x / (2(1 + x))` with `x = step/τ`, from 0.5 where the step spans it many
/// times over to below 0.1 once the step is shorter than `τ/4`, where the
/// difference alone estimates the error. A state that jumps at the restart
/// point (a capacitor that a source or other capacitors charge at once, an
/// inductor in series with a current source) and then moves smoothly gives
/// about `step/(2τ)` of its slower motion, or nothing at all where the
/// circuit forces it: its curvature is the jump itself, which no shorter
/// step resolves.
const STIFF_RATIO: f64 = 0.1;

/// A one-step integration formula, applied from the newest history point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Formula {
    BackwardEuler,
    Trapezoidal,
    /// Variable-step BDF2 through the two newest history points.
    Bdf2,
}

/// One Newton solve of a timestep: reach `fraction` of the step with
/// `formula`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stage {
    pub(crate) fraction: f64,
    pub(crate) formula: Formula,
}

/// The stages of one regular (non-restart) step of `method`.
pub(crate) fn stages(method: IntegrationMethod) -> &'static [Stage] {
    const fn whole(formula: Formula) -> Stage {
        Stage {
            fraction: 1.0,
            formula,
        }
    }
    const BACKWARD_EULER: [Stage; 1] = [whole(Formula::BackwardEuler)];
    const TRAPEZOIDAL: [Stage; 1] = [whole(Formula::Trapezoidal)];
    const GEAR2: [Stage; 1] = [whole(Formula::Bdf2)];
    const TR_BDF2: [Stage; 2] = [
        Stage {
            fraction: TR_BDF2_GAMMA,
            formula: Formula::Trapezoidal,
        },
        whole(Formula::Bdf2),
    ];
    match method {
        IntegrationMethod::BackwardEuler => &BACKWARD_EULER,
        IntegrationMethod::Trapezoidal => &TRAPEZOIDAL,
        IntegrationMethod::Gear2 => &GEAR2,
        IntegrationMethod::ModifiedTrapezoidal => &TR_BDF2,
    }
}

/// Order of accuracy of `method`.
pub(crate) fn order(method: IntegrationMethod) -> usize {
    match method {
        IntegrationMethod::BackwardEuler => 1,
        _ => 2,
    }
}

/// `dq/dt ≈ a0·q + b[0]·q₀ + b[1]·q₁ + c0·i₀`, where index 0 is the newest
/// history point.
#[derive(Debug, Clone, Copy, Default)]
struct Coefficients {
    a0: f64,
    b: [f64; 2],
    c0: f64,
}

/// Ring buffer of accepted points; index 0 is the newest.
#[derive(Debug, Clone)]
struct History {
    slots: usize,
    times: [f64; HISTORY],
    /// `q[point * slots + slot]`.
    q: Vec<f64>,
    /// `dq/dt` at each point, same layout as `q`.
    i: Vec<f64>,
    newest: usize,
    len: usize,
}

impl History {
    fn new(slots: usize) -> Self {
        Self {
            slots,
            times: [0.0; HISTORY],
            q: vec![0.0; HISTORY * slots],
            i: vec![0.0; HISTORY * slots],
            newest: 0,
            len: 0,
        }
    }

    fn position(&self, k: usize) -> usize {
        debug_assert!(k < self.len);
        (self.newest + HISTORY - k) % HISTORY
    }

    fn time(&self, k: usize) -> f64 {
        self.times[self.position(k)]
    }

    fn q(&self, k: usize, slot: usize) -> f64 {
        self.q[self.position(k) * self.slots + slot]
    }

    fn i(&self, k: usize, slot: usize) -> f64 {
        self.i[self.position(k) * self.slots + slot]
    }

    /// Whether the next push overwrites the oldest point.
    fn is_full(&self) -> bool {
        self.len == HISTORY
    }

    fn oldest_derivatives(&self) -> &[f64] {
        let base = self.position(self.len - 1) * self.slots;
        &self.i[base..base + self.slots]
    }

    fn push(&mut self, time: f64, q: &[f64], i: &[f64]) {
        self.newest = (self.newest + 1) % HISTORY;
        self.len = (self.len + 1).min(HISTORY);
        self.times[self.newest] = time;
        let base = self.newest * self.slots;
        self.q[base..base + self.slots].copy_from_slice(q);
        self.i[base..base + self.slots].copy_from_slice(i);
    }

    fn pop(&mut self) {
        debug_assert!(self.len > 0);
        self.newest = (self.newest + HISTORY - 1) % HISTORY;
        self.len -= 1;
    }
}

/// Holds the history of every state quantity and the current integration
/// coefficients. See [`crate::devices::LoadContext::integrate`].
#[derive(Debug, Clone)]
pub(crate) struct Integrator {
    method: IntegrationMethod,
    slots: usize,
    history: History,
    /// Points of `history` recorded since the last restart.
    valid: usize,
    /// State values and derivatives recorded by the latest load.
    trial_q: Vec<f64>,
    trial_i: Vec<f64>,
    /// `|dq/dx|` of each state at the latest load, and whether the state is
    /// a flux (controlled by a current) rather than a charge.
    trial_c: Vec<f64>,
    flux: Vec<bool>,
    /// Time of the point being computed.
    trial_time: f64,
    coefficients: Coefficients,
    /// Largest `|dq/dt|` of each state among points that left the history
    /// and count for the peak (see `peak_points`).
    retired_peak: Vec<f64>,
    /// Number of newest history points whose `|dq/dt|` counts for the peak:
    /// every point, until [`Self::restart_reference`] restricts the peak to
    /// the points after a discontinuity.
    peak_points: usize,
    /// Time of the last restart, from which `chgtol` is spread.
    restart_time: f64,
    /// Initial conditions that constrain DC solves at a transient's start.
    constraints: InitialConstraints,
}

impl Integrator {
    pub(crate) fn new(method: IntegrationMethod, slots: usize) -> Self {
        Self {
            method,
            slots,
            history: History::new(slots),
            valid: 0,
            trial_q: vec![0.0; slots],
            trial_i: vec![0.0; slots],
            trial_c: vec![0.0; slots],
            flux: vec![false; slots],
            trial_time: 0.0,
            coefficients: Coefficients::default(),
            retired_peak: vec![0.0; slots],
            peak_points: usize::MAX,
            restart_time: 0.0,
            constraints: InitialConstraints::Off,
        }
    }

    /// Selects the initial conditions that constrain the DC solves at the
    /// start of a transient (D16); [`InitialConstraints::Off`] again once the
    /// starting point is solved. Devices see them through
    /// [`crate::devices::LoadContext::initial_constraints`].
    pub(crate) fn set_initial_constraints(&mut self, constraints: InitialConstraints) {
        self.constraints = constraints;
    }

    /// See [`Self::set_initial_constraints`].
    pub(crate) fn initial_constraints(&self) -> InitialConstraints {
        self.constraints
    }

    /// Number of integrated states.
    pub(crate) fn slot_count(&self) -> usize {
        self.slots
    }

    /// See [`crate::devices::LoadContext::integrate`].
    pub(crate) fn integrate(
        &mut self,
        mode: LoadMode,
        slot: StateSlot,
        q: f64,
        c: f64,
    ) -> (f64, f64) {
        let s = slot.0;
        self.trial_q[s] = q;
        self.trial_c[s] = c.abs();
        match mode {
            LoadMode::Dc => {
                self.trial_i[s] = 0.0;
                (0.0, 0.0)
            }
            LoadMode::Transient => {
                let k = &self.coefficients;
                let mut i = k.a0 * q + k.b[0] * self.history.q(0, s) + k.c0 * self.history.i(0, s);
                if k.b[1] != 0.0 {
                    i += k.b[1] * self.history.q(1, s);
                }
                self.trial_i[s] = i;
                (i, k.a0 * c)
            }
        }
    }

    /// [`Self::integrate`] for a flux state `phi` with `l = dφ/di`, whose
    /// controlling quantity is a current.
    pub(crate) fn integrate_flux(
        &mut self,
        mode: LoadMode,
        slot: StateSlot,
        phi: f64,
        l: f64,
    ) -> (f64, f64) {
        self.flux[slot.0] = true;
        self.integrate(mode, slot, phi, l)
    }

    pub(crate) fn method(&self) -> IntegrationMethod {
        self.method
    }

    /// Prepares the coefficients for a solve at `time` with `formula`,
    /// integrating from the newest accepted point.
    pub(crate) fn begin_step(&mut self, formula: Formula, time: f64) {
        let h = time - self.history.time(0);
        self.trial_time = time;
        self.coefficients = match formula {
            Formula::BackwardEuler => Coefficients {
                a0: 1.0 / h,
                b: [-1.0 / h, 0.0],
                c0: 0.0,
            },
            Formula::Trapezoidal => Coefficients {
                a0: 2.0 / h,
                b: [-2.0 / h, 0.0],
                c0: -1.0,
            },
            Formula::Bdf2 => {
                debug_assert!(self.valid >= 2, "BDF2 needs two points since the restart");
                let (a0, b0, b1) = bdf2_weights(h, self.history.time(0) - self.history.time(1));
                Coefficients {
                    a0,
                    b: [b0, b1],
                    c0: 0.0,
                }
            }
        };
    }

    /// Accepts the latest load's values as the point at `time`.
    pub(crate) fn commit(&mut self, time: f64) {
        if self.history.is_full() && self.peak_points >= HISTORY {
            let retired = self.history.oldest_derivatives();
            for (peak, &i) in self.retired_peak.iter_mut().zip(retired) {
                *peak = peak.max(i.abs());
            }
        }
        self.history.push(time, &self.trial_q, &self.trial_i);
        self.valid = (self.valid + 1).min(HISTORY);
        self.peak_points = self.peak_points.saturating_add(1);
    }

    /// Discards the newest accepted point (a provisional stage whose step
    /// was rejected).
    pub(crate) fn rollback(&mut self) {
        self.history.pop();
        self.valid = self.valid.saturating_sub(1);
        if self.peak_points != usize::MAX {
            self.peak_points = self.peak_points.saturating_sub(1);
        }
    }

    /// Forgets everything before the newest point, which becomes the first
    /// point of a new smooth segment (after `t = 0` or a breakpoint).
    pub(crate) fn restart(&mut self) {
        self.valid = 1;
        self.restart_time = self.history.time(0);
    }

    /// [`Self::restart`] after a discontinuity of the circuit equations (a
    /// switch changed state), which also restarts the reference of the
    /// relative tolerance: the peak `|dq/dt|` counts only the points after
    /// the newest one, whose derivatives belong to the circuit before the
    /// change. Otherwise the discharge current of a switch closing onto a
    /// capacitor (amperes) would set the tolerance of the slow recharge
    /// that follows (microamperes) for the rest of the run.
    pub(crate) fn restart_reference(&mut self) {
        self.restart();
        self.retired_peak.fill(0.0);
        self.peak_points = 0;
    }

    /// Overrides the state value of the pending point (initial conditions).
    pub(crate) fn set_trial_state(&mut self, slot: StateSlot, q: f64) {
        self.trial_q[slot.0] = q;
    }

    /// State values recorded by the latest load.
    pub(crate) fn trial_states(&self) -> &[f64] {
        &self.trial_q
    }

    /// Replaces the newest point's derivatives, which backward Euler only
    /// knows to first order, by the second-order derivative of the quadratic
    /// through the three newest points. Trapezoidal steps continue from
    /// these derivatives, so this keeps a restart from leaving a persistent
    /// first-order error (and trapezoidal ringing) in the state currents.
    pub(crate) fn refine_latest_derivatives(&mut self) {
        if self.valid < 3 {
            return;
        }
        let h = self.history.time(0) - self.history.time(1);
        let h1 = self.history.time(1) - self.history.time(2);
        let (a0, b0, b1) = bdf2_weights(h, h1);
        let base = self.history.position(0) * self.slots;
        for s in 0..self.slots {
            self.history.i[base + s] =
                a0 * self.history.q(0, s) + b0 * self.history.q(1, s) + b1 * self.history.q(2, s);
        }
    }

    /// Error of the pending point relative to the tolerance (≤ 1 means
    /// acceptable) for a regular step of `step` seconds, or `None` while
    /// there is not enough history since the last restart. `at_minimum`:
    /// the step control cannot shorten the step (see [`Self::tolerance`]).
    pub(crate) fn truncation_error_ratio(
        &self,
        step: f64,
        options: &SimOptions,
        at_minimum: bool,
    ) -> Option<f64> {
        self.error_ratio(step, Estimate::Regular, options, at_minimum)
    }

    /// Error ratio of a step-doubled backward-Euler step: `full` holds the
    /// states after one step of `step` seconds, the pending point those
    /// after two half steps, whose midpoint is the newest history point and
    /// whose start (the restart point) the one before it. Their difference
    /// estimates the error of the half-step result, which is the one kept.
    /// With `curved`, so does the half steps' own error current from the
    /// curvature through the three points (`h/4·|q''|`, from their divided
    /// difference), and the larger of the two counts (see the module
    /// documentation), for a state that settles far faster than the step
    /// ([`STIFF_RATIO`]), not for one that jumps at the restart point and
    /// then moves smoothly. The transient leaves the curvature out where
    /// it cannot shorten the step. `at_minimum` as for
    /// [`Self::truncation_error_ratio`].
    pub(crate) fn step_doubling_error_ratio(
        &self,
        full: &[f64],
        step: f64,
        options: &SimOptions,
        at_minimum: bool,
        curved: bool,
    ) -> f64 {
        self.error_ratio(
            step,
            Estimate::Doubled { full, curved },
            options,
            at_minimum,
        )
        .unwrap_or(0.0)
    }

    /// The largest error in an inductor's current (A) that the estimated
    /// flux errors of the pending point imply, its flux error divided by
    /// its inductance (0 without inductors or history).
    pub(crate) fn current_error(&self, step: f64, estimate: Estimate) -> f64 {
        let mut worst = 0.0_f64;
        self.each_error(step, estimate, |s, error| {
            if self.flux[s] && self.trial_c[s] > 0.0 {
                worst = worst.max(error * step / self.trial_c[s]);
            }
        });
        worst
    }

    /// The error ratio of the pending point for `estimate`; `None` while a
    /// regular estimate lacks history since the last restart.
    fn error_ratio(
        &self,
        step: f64,
        estimate: Estimate,
        options: &SimOptions,
        at_minimum: bool,
    ) -> Option<f64> {
        let mut worst = 0.0_f64;
        let estimated = self.each_error(step, estimate, |s, error| {
            worst = worst.max(error / self.tolerance(s, step, options, at_minimum));
        });
        estimated.then_some(worst)
    }

    /// Calls `visit` with each state and the error current estimated for
    /// the pending point; `false` (and no call) while a regular estimate
    /// lacks history since the last restart.
    fn each_error(&self, step: f64, estimate: Estimate, mut visit: impl FnMut(usize, f64)) -> bool {
        if let Estimate::Doubled { full, curved } = estimate {
            // The midpoint is the newest history point and the restart point
            // the one before it (see `step_doubling_error_ratio`). Their
            // times, not `step / 2`: late in a long run the midpoint of a
            // step of a few minimum steps rounds visibly off the middle.
            let times = (curved && self.valid >= 2)
                .then(|| [self.trial_time, self.history.time(0), self.history.time(1)]);
            for (s, (&full, &half)) in full.iter().zip(&self.trial_q).enumerate() {
                let difference = (full - half).abs();
                let mut error = difference / step;
                let middle = self.history.q(0, s);
                if let Some(times) = &times
                    && difference > STIFF_RATIO * (half - middle).abs()
                {
                    let mut values = [half, middle, self.history.q(1, s)];
                    let curvature = divided_difference(times, &mut values);
                    error = error.max(0.5 * step * curvature.abs());
                }
                visit(s, error);
            }
            return true;
        }
        let p = order(self.method);
        if self.valid < p + 1 {
            return false;
        }
        let constant = match self.method {
            IntegrationMethod::BackwardEuler => 0.5,
            IntegrationMethod::Trapezoidal => 1.0 / 12.0,
            IntegrationMethod::Gear2 => {
                let previous = self.history.time(0) - self.history.time(1);
                (step + previous) / (6.0 * step)
            }
            IntegrationMethod::ModifiedTrapezoidal => {
                let g = TR_BDF2_GAMMA;
                ((-3.0 * g * g + 4.0 * g - 2.0) / (12.0 * (2.0 - g))).abs()
            }
        };
        // Error current K·hᵖ·q⁽ᵖ⁺¹⁾ with q⁽ᵖ⁺¹⁾ = (p+1)!·DD[p+1].
        let factorial = (1..=p + 1).product::<usize>() as f64;
        let scale = constant * factorial * step.powi(p as i32);
        let mut times = [0.0; HISTORY + 1];
        let mut values = [0.0; HISTORY + 1];
        let points = p + 2;
        times[0] = self.trial_time;
        for (k, time) in times[1..points].iter_mut().enumerate() {
            *time = self.history.time(k);
        }
        for s in 0..self.slots {
            values[0] = self.trial_q[s];
            for (k, value) in values[1..points].iter_mut().enumerate() {
                *value = self.history.q(k, s);
            }
            visit(
                s,
                scale * divided_difference(&times[..points], &mut values[..points]).abs(),
            );
        }
        true
    }

    /// Acceptable error current of state `s` for a step of `step` seconds.
    /// `at_minimum`: the step control cannot shorten the step, and a flux
    /// state may then move by the flux dual of `chgtol` in it (see the
    /// module documentation).
    fn tolerance(&self, s: usize, step: f64, options: &SimOptions, at_minimum: bool) -> f64 {
        let peak = (0..self.history.len.min(self.peak_points))
            .map(|k| self.history.i(k, s).abs())
            .fold(self.retired_peak[s].max(self.trial_i[s].abs()), f64::max);
        let charge = self.trial_q[s].abs().max(self.history.q(0, s).abs());
        // The charge that moves the controlling voltage by vntol (the flux
        // that moves the current by abstol), where it exceeds chgtol, spread
        // over the time since the restart.
        let (floor, spread) = if !self.flux[s] {
            let floor = options.chgtol.max(self.trial_c[s] * options.vntol);
            (floor, self.trial_time - self.restart_time)
        } else if at_minimum {
            let floor = flux_dual(options.chgtol, options).max(self.trial_c[s] * options.abstol);
            (floor, step)
        } else {
            let floor = options.chgtol.max(self.trial_c[s] * options.abstol);
            (floor, self.trial_time - self.restart_time)
        };
        options.trtol * (options.abstol + options.reltol * peak)
            + floor / spread
            + STATE_ROUNDOFF * charge / step
    }
}

/// The flux (V·s) dual to the charge `q` (C) at the impedance level
/// `vntol / abstol` that the tolerances imply (1 kΩ by default): the dual
/// of a capacitor `C` is an inductor `C·(vntol/abstol)²`, whose flux at the
/// dual current carries `q·vntol/abstol`.
fn flux_dual(q: f64, options: &SimOptions) -> f64 {
    q * options.vntol / options.abstol
}

/// Weights `(a0, b0, b1)` of the variable-step BDF2 derivative at the new
/// point from steps `h` (newest) and `h1` (the one before):
/// `dq/dt ≈ a0·qₙ₊₁ + b0·qₙ + b1·qₙ₋₁`.
fn bdf2_weights(h: f64, h1: f64) -> (f64, f64, f64) {
    let span = h + h1;
    (
        (2.0 * h + h1) / (h * span),
        -span / (h * h1),
        h / (h1 * span),
    )
}

/// Newton divided difference `f[t₀, …, tₙ]`, computed in place.
fn divided_difference(times: &[f64], values: &mut [f64]) -> f64 {
    let n = values.len();
    for level in 1..n {
        for k in 0..n - level {
            values[k] = (values[k] - values[k + 1]) / (times[k] - times[k + level]);
        }
    }
    values[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives one state `q(t)` through the integrator as if a device had
    /// stamped the exact solution, returning the derivative it reports.
    fn feed(integrator: &mut Integrator, formula: Formula, t: f64, q: f64) -> f64 {
        integrator.begin_step(formula, t);
        integrator
            .integrate(LoadMode::Transient, StateSlot(0), q, 1.0)
            .0
    }

    fn start(method: IntegrationMethod, q: impl Fn(f64) -> f64, times: &[f64]) -> Integrator {
        let mut integrator = Integrator::new(method, 1);
        integrator.integrate(LoadMode::Dc, StateSlot(0), q(times[0]), 1.0);
        integrator.commit(times[0]);
        integrator.restart();
        for &t in &times[1..] {
            feed(&mut integrator, Formula::BackwardEuler, t, q(t));
            integrator.commit(t);
        }
        integrator
    }

    #[test]
    fn large_states_get_a_charge_floor_worth_vntol_or_abstol() {
        // Right after a restart, with nothing moving yet, only the floor
        // spread over the time since the restart admits an error: chgtol for
        // a 1 pF capacitor, the charge that moves a 1 mF capacitor by vntol,
        // and the flux that moves a 1 H inductor's current by abstol.
        let options = SimOptions::default();
        let capacitances = [1e-12, 1e-3];
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, 3);
        let load = |integrator: &mut Integrator, mode| {
            for (s, &c) in capacitances.iter().enumerate() {
                integrator.integrate(mode, StateSlot(s), 0.0, c);
            }
            integrator.integrate_flux(mode, StateSlot(2), 0.0, 1.0);
        };
        load(&mut integrator, LoadMode::Dc);
        integrator.commit(0.0);
        integrator.restart();
        let h = 1e-9;
        integrator.begin_step(Formula::BackwardEuler, h);
        load(&mut integrator, LoadMode::Transient);
        let floor =
            |s| (integrator.tolerance(s, h, &options, false) - options.trtol * options.abstol) * h;
        let expected = [options.chgtol, 1e-3 * options.vntol, options.abstol];
        for (s, expected) in expected.into_iter().enumerate() {
            assert!(
                (floor(s) - expected).abs() < 1e-9 * expected,
                "state {s}: {:e} vs {expected:e}",
                floor(s)
            );
        }
    }

    #[test]
    fn at_the_minimum_step_a_flux_may_move_by_the_dual_of_chgtol() {
        // A 1 µH inductor's floor is chgtol (above the 1e-18 V·s that moves
        // its current by abstol), spread over the time since the restart. A
        // step the control cannot shorten admits the dual of chgtol at the
        // vntol/abstol impedance (1e-13 V·s) within the step itself, or the
        // inductor's own floor where larger (1 H); a capacitor's floor stays.
        let options = SimOptions::default();
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, 3);
        let load = |integrator: &mut Integrator, mode| {
            integrator.integrate_flux(mode, StateSlot(0), 0.0, 1e-6);
            integrator.integrate_flux(mode, StateSlot(1), 0.0, 1.0);
            integrator.integrate(mode, StateSlot(2), 0.0, 1e-12);
        };
        load(&mut integrator, LoadMode::Dc);
        integrator.commit(0.0);
        integrator.restart();
        let (h, since) = (1e-13, 4e-13);
        integrator.begin_step(Formula::BackwardEuler, since);
        load(&mut integrator, LoadMode::Transient);
        let allowed = |s, at_minimum| {
            integrator.tolerance(s, h, &options, at_minimum) - options.trtol * options.abstol
        };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9 * b;
        assert!(close(allowed(0, false), options.chgtol / since));
        assert!(close(allowed(0, true), 1e-16 * 1e3 / h));
        assert!(close(allowed(1, true), options.abstol / h));
        assert!(close(allowed(2, true), allowed(2, false)));
        assert!(close(allowed(2, false), options.chgtol / since));
    }

    #[test]
    fn a_restarted_reference_forgets_the_derivatives_before_a_discontinuity() {
        // A fast discharge (dq/dt = -300) sets the relative tolerance of the
        // states after it until the reference restarts; the restart point
        // itself belongs to the circuit before the discontinuity.
        let options = SimOptions {
            chgtol: 1e-300,
            vntol: 1e-300,
            ..SimOptions::default()
        };
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, 1);
        integrator.integrate(LoadMode::Dc, StateSlot(0), 3e-6, 1e-6);
        integrator.commit(0.0);
        integrator.restart();
        let mut q = 3e-6;
        for k in 1..=8 {
            q -= 300.0 * 1e-9;
            feed(
                &mut integrator,
                Formula::BackwardEuler,
                f64::from(k) * 1e-9,
                q,
            );
            integrator.commit(f64::from(k) * 1e-9);
        }
        let relative = |integrator: &Integrator| {
            integrator.tolerance(0, 1e-6, &options, false) - options.trtol * options.abstol
        };
        integrator.begin_step(Formula::BackwardEuler, 9e-9);
        integrator.integrate(LoadMode::Transient, StateSlot(0), q + 3e-13, 1e-6);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-3 * b;
        assert!(close(relative(&integrator), options.reltol * 300.0));
        integrator.restart_reference();
        integrator.begin_step(Formula::BackwardEuler, 9e-9);
        integrator.integrate(LoadMode::Transient, StateSlot(0), q + 3e-13, 1e-6);
        assert!(close(relative(&integrator), options.reltol * 3e-4));
        integrator.commit(9e-9);
        integrator.begin_step(Formula::BackwardEuler, 10e-9);
        integrator.integrate(LoadMode::Transient, StateSlot(0), q + 4e-13, 1e-6);
        assert!(close(relative(&integrator), options.reltol * 3e-4));
        // A plain restart keeps the reference.
        integrator.restart();
        integrator.begin_step(Formula::BackwardEuler, 11e-9);
        integrator.integrate(LoadMode::Transient, StateSlot(0), q + 5e-13, 1e-6);
        assert!(close(relative(&integrator), options.reltol * 3e-4));
    }

    /// Feeds a step-doubled backward-Euler restart step of `h` from rest at
    /// `t0` to an integrator: `full` after one step, `mid` at the midpoint
    /// (committed) and `end` after the second half step, for one capacitor
    /// of `c`. Returns the error ratio and the bare step-doubling ratio.
    fn doubled(t0: f64, h: f64, c: f64, [full, mid, end]: [f64; 3]) -> (f64, f64) {
        let options = SimOptions::default();
        let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, 1);
        integrator.integrate(LoadMode::Dc, StateSlot(0), 0.0, c);
        integrator.commit(t0);
        integrator.restart();
        integrator.begin_step(Formula::BackwardEuler, t0 + h / 2.0);
        integrator.integrate(LoadMode::Transient, StateSlot(0), mid, c);
        integrator.commit(t0 + h / 2.0);
        integrator.begin_step(Formula::BackwardEuler, t0 + h);
        integrator.integrate(LoadMode::Transient, StateSlot(0), end, c);
        let ratio = integrator.step_doubling_error_ratio(&[full], h, &options, false, true);
        let doubling = integrator.step_doubling_error_ratio(&[full], h, &options, false, false);
        (ratio, doubling)
    }

    #[test]
    fn a_restart_step_over_a_far_faster_decay_is_rejected() {
        // Behind an ideal edge 1 V charges 1 nF with τ = 1 ns. A restart step
        // of 1 ms finds the charge settled after one full backward-Euler step
        // and after two half steps alike, so their difference passes the
        // tolerance although the decay lies unresolved between the points;
        // the curvature through the restart point, the midpoint and the end
        // does not pass it.
        let (c, tau) = (1e-9, 1e-9);
        let be = |x: f64| x / (1.0 + x);
        let states = |h: f64| {
            let mid = c * be(h / 2.0 / tau);
            [c * be(h / tau), mid, mid + (c - mid) * be(h / 2.0 / tau)]
        };
        let (ratio, doubling) = doubled(0.5, 1e-3, c, states(1e-3));
        assert!(doubling < 1.0, "{doubling}");
        assert!(ratio > 1e5, "{ratio}");
        // Where the step resolves the decay both estimates agree.
        let (ratio, doubling) = doubled(0.5, 1e-11, c, states(1e-11));
        assert!(
            ratio > 1.0 && (ratio / doubling - 1.0).abs() < 0.02,
            "{ratio} vs {doubling}"
        );
        // A charge rising linearly has no curvature, also where the midpoint
        // of a step of a few minimum steps rounds off the middle (t = 100 s).
        let (t0, h, current): (f64, f64, f64) = (100.0, 1e-13, 1.0);
        let tm = t0 + h / 2.0;
        assert!(((tm - t0) / h - 0.5).abs() > 0.05, "the midpoint rounds");
        let linear = [current * h, current * (tm - t0), current * h];
        let (ratio, _) = doubled(t0, h, c, linear);
        assert!(ratio < 1e-6, "{ratio}");
        // A capacitor that a source forces to 1 V at once (a start from rest
        // with it across the supply) has the same curvature at every step:
        // the jump itself, which no shorter step resolves. It is accepted.
        let forced = [1e-6, 1e-6, 1e-6];
        assert_eq!(doubled(0.0, 1e-5, 1e-6, forced), (0.0, 0.0));
        // So does a capacitor that a capacitive divider charges by 0.5 µC at
        // the edge and a bleed resistor then discharges with τ = 2 s: within
        // the 0.2 ms step its full step and half steps differ by far less
        // than its motion, and only their difference counts.
        let (jump, slope, h) = (0.5e-6, -0.25e-6, 2e-4);
        let (mid, end) = (jump + slope * h / 2.0, jump + slope * h);
        let drifting = [end + slope * h * h / 8.0, mid, end];
        let (ratio, doubling) = doubled(0.5, h, 1e-6, drifting);
        assert!(ratio == doubling && ratio < 1.0, "{ratio} vs {doubling}");
    }

    #[test]
    fn divided_differences_recover_polynomial_derivatives() {
        // q = t³ has q''' = 6, so its third divided difference is 1.
        let times = [0.7, 0.4, 0.25, 0.1];
        let mut values = times.map(|t: f64| t.powi(3) + 2.0 * t);
        assert!((divided_difference(&times, &mut values) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn formulas_are_exact_for_their_order() {
        // BE is exact for linear q, TR and BDF2 for quadratic q.
        let linear = |t: f64| 3.0 * t + 1.0;
        let quadratic = |t: f64| 2.0 * t * t - t + 0.5;
        let mut be = start(IntegrationMethod::BackwardEuler, linear, &[0.0, 0.1]);
        assert!((feed(&mut be, Formula::BackwardEuler, 0.25, linear(0.25)) - 3.0).abs() < 1e-12);

        let mut bdf = start(IntegrationMethod::Gear2, quadratic, &[0.0, 0.1, 0.3]);
        let derivative = feed(&mut bdf, Formula::Bdf2, 0.45, quadratic(0.45));
        assert!((derivative - (4.0 * 0.45 - 1.0)).abs() < 1e-12);

        let mut tr = start(IntegrationMethod::Trapezoidal, quadratic, &[0.0, 0.1, 0.3]);
        tr.refine_latest_derivatives();
        let derivative = feed(&mut tr, Formula::Trapezoidal, 0.45, quadratic(0.45));
        assert!((derivative - (4.0 * 0.45 - 1.0)).abs() < 1e-12);
    }

    #[test]
    fn companion_conductance_scales_with_the_capacitance() {
        let mut integrator = start(IntegrationMethod::BackwardEuler, |t| t, &[0.0]);
        integrator.begin_step(Formula::BackwardEuler, 1e-3);
        let (_, geq) = integrator.integrate(LoadMode::Transient, StateSlot(0), 0.0, 2e-6);
        assert!((geq - 2e-3).abs() < 1e-15);
        let (i, geq) = integrator.integrate(LoadMode::Dc, StateSlot(0), 1.0, 2e-6);
        assert_eq!((i, geq), (0.0, 0.0));
    }

    #[test]
    fn truncation_error_matches_the_leading_term() {
        // q = t³: BDF2 with equal steps h has the residual (1/3)·h²·q''' with
        // q''' = 6.
        let cube = |t: f64| t * t * t;
        let h = 0.01;
        let mut integrator = start(IntegrationMethod::Gear2, cube, &[0.0, h, 2.0 * h, 3.0 * h]);
        feed(&mut integrator, Formula::Bdf2, 4.0 * h, cube(4.0 * h));
        // Only the abstol term remains: no relative part and no charge
        // floor (`chgtol` and, with `dq/dx = 1`, `vntol`).
        let options = SimOptions {
            reltol: 1e-300,
            abstol: 1.0,
            vntol: 1e-300,
            chgtol: 1e-300,
            ..SimOptions::default()
        };
        let ratio = integrator
            .truncation_error_ratio(h, &options, false)
            .unwrap();
        let expected = h * h * 6.0 / 3.0;
        assert!(
            (ratio - expected).abs() < 1e-9 * expected,
            "{ratio} vs {expected}"
        );
    }

    #[test]
    fn error_estimates_need_history_since_the_restart() {
        let options = SimOptions::default();
        let mut integrator = start(IntegrationMethod::Trapezoidal, |t| t * t, &[0.0, 0.1]);
        assert!(
            integrator
                .truncation_error_ratio(0.1, &options, false)
                .is_none()
        );
        feed(&mut integrator, Formula::BackwardEuler, 0.2, 0.04);
        integrator.commit(0.2);
        feed(&mut integrator, Formula::Trapezoidal, 0.3, 0.09);
        assert!(
            integrator
                .truncation_error_ratio(0.1, &options, false)
                .is_some()
        );
        integrator.restart();
        assert!(
            integrator
                .truncation_error_ratio(0.1, &options, false)
                .is_none()
        );
    }

    #[test]
    fn rollback_restores_the_previous_point() {
        let mut integrator = start(IntegrationMethod::BackwardEuler, |t| t, &[0.0, 1.0]);
        feed(&mut integrator, Formula::BackwardEuler, 2.0, 5.0);
        integrator.commit(2.0);
        integrator.rollback();
        // The derivative is again taken from the point at t = 1.
        assert!((feed(&mut integrator, Formula::BackwardEuler, 2.0, 2.0) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn tr_bdf2_stages_share_one_companion_scale() {
        let h = 1e-3;
        let mut integrator = start(
            IntegrationMethod::ModifiedTrapezoidal,
            |t| t,
            &[0.0, h, 2.0 * h],
        );
        integrator.begin_step(Formula::Trapezoidal, 2.0 * h + TR_BDF2_GAMMA * h);
        let (_, g_tr) = integrator.integrate(LoadMode::Transient, StateSlot(0), 0.0, 1.0);
        integrator.commit(2.0 * h + TR_BDF2_GAMMA * h);
        integrator.begin_step(Formula::Bdf2, 3.0 * h);
        let (_, g_bdf) = integrator.integrate(LoadMode::Transient, StateSlot(0), 0.0, 1.0);
        assert!((g_tr - g_bdf).abs() < 1e-9 * g_tr);
    }
}
