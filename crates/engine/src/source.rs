//! Independent source values and transient waveforms (Q72).
//!
//! ## Semantics shared by every waveform
//! * Waveforms are evaluated for `t ≥ 0`; before their first event they hold
//!   their initial level.
//! * Ideal edges (zero rise or fall time, a repeating PWL whose first point
//!   is at `t = 0` and whose last value differs from its first) are jumps.
//!   [`Waveform::value`] is right-continuous: at the edge instant it already
//!   returns the new level. [`Waveform::value_before`] is the left limit.
//!   A SINE that stops after `cycles` holds its final value, so its end is a
//!   corner, not a jump (as in LTspice).
//!   The transient solver evaluates a timestep that *ends* on an edge with
//!   the left limit, so the jump takes effect in the following step instead
//!   of being smeared over the step that leads up to it.
//! * Every corner and jump is reported by [`Waveform::breakpoints`]. The
//!   segment boundaries used for evaluation are computed with exactly the
//!   same floating-point expressions as the breakpoints, so evaluating at a
//!   breakpoint always selects the intended side of the corner.
//! * Smooth waveforms (SINE, EXP, SFFM) have no corners to land on; the
//!   transient analysis limits its step instead (`Waveform::interpolation_step`),
//!   so their samples reproduce them by linear interpolation within the
//!   result tolerance (D18).

use crate::error::CircuitError;
use serde::{Deserialize, Serialize};

/// Value specification of an independent voltage or current source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Value used by operating point and DC sweep analyses.
    pub dc: f64,
    /// Small-signal AC magnitude (V or A). Zero means the source is not an AC
    /// stimulus.
    pub ac_magnitude: f64,
    /// Small-signal AC phase in degrees.
    pub ac_phase: f64,
    /// Time-domain waveform. When `None` the transient value is `dc`.
    pub waveform: Option<Waveform>,
}

impl Source {
    /// A constant source.
    pub fn dc(value: f64) -> Self {
        Self {
            dc: value,
            ac_magnitude: 0.0,
            ac_phase: 0.0,
            waveform: None,
        }
    }

    /// A source that is `dc` for OP/DC and a unit-phase AC stimulus.
    pub fn ac(dc: f64, magnitude: f64, phase_degrees: f64) -> Self {
        Self {
            dc,
            ac_magnitude: magnitude,
            ac_phase: phase_degrees,
            waveform: None,
        }
    }

    /// A source following `waveform` in transient analysis. Its DC value is
    /// the waveform value at `t = 0`.
    pub fn with_waveform(waveform: Waveform) -> Self {
        Self {
            dc: waveform.value(0.0),
            ac_magnitude: 0.0,
            ac_phase: 0.0,
            waveform: Some(waveform),
        }
    }

    /// Value at time `t` during transient analysis.
    pub fn transient_value(&self, t: f64) -> f64 {
        match &self.waveform {
            Some(w) => w.value(t),
            None => self.dc,
        }
    }

    /// Left limit of [`Self::transient_value`] at `t` (see
    /// [`Waveform::value_before`]).
    pub fn transient_value_before(&self, t: f64) -> f64 {
        match &self.waveform {
            Some(w) => w.value_before(t),
            None => self.dc,
        }
    }

    pub(crate) fn validate(&self, name: &str) -> Result<(), CircuitError> {
        if !self.dc.is_finite() || !self.ac_magnitude.is_finite() || !self.ac_phase.is_finite() {
            return Err(CircuitError::new(format!(
                "{name}: source values must be finite"
            )));
        }
        match &self.waveform {
            Some(w) => w.validate(name),
            None => Ok(()),
        }
    }
}

/// Transient source functions. Times in seconds, frequencies in hertz,
/// phases in degrees, values in volts or amperes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Waveform {
    /// Trapezoidal pulse train. `period == 0` means a single pulse. `cycles`
    /// limits the number of periods (`None` = unlimited).
    Pulse {
        initial: f64,
        pulsed: f64,
        delay: f64,
        rise: f64,
        fall: f64,
        width: f64,
        period: f64,
        cycles: Option<u32>,
    },
    /// `offset + amplitude·e^(−damping·(t−delay))·sin(2πf(t−delay) + phase)`
    /// for `t ≥ delay`, else `offset + amplitude·sin(phase)`. `cycles` limits
    /// the number of periods (fractions allowed); after the last one the
    /// output holds the value it reached, as LTspice does.
    Sine {
        offset: f64,
        amplitude: f64,
        frequency: f64,
        delay: f64,
        damping: f64,
        phase: f64,
        cycles: Option<f64>,
    },
    /// Two-time-constant exponential (SPICE `EXP`).
    Exp {
        initial: f64,
        pulsed: f64,
        rise_delay: f64,
        rise_tau: f64,
        fall_delay: f64,
        fall_tau: f64,
    },
    /// Piecewise-linear `(time, value)` points with strictly increasing
    /// times. Holds the first value before the first point and the last value
    /// after the last point. With `repeat` the sequence repeats like LTspice's
    /// `PWL REPEAT FOREVER (…) ENDREPEAT`: repetition `k` starts at
    /// `k·t_last`, its points lie at `k·t_last + tⱼ`, and from the start of a
    /// repetition to its first point the value ramps from the previous
    /// repetition's last value (an ideal jump when the first point is at
    /// `t = 0`; LTspice rejects that case unless the last value equals the
    /// first).
    Pwl {
        points: Vec<(f64, f64)>,
        repeat: bool,
    },
    /// Single-frequency FM:
    /// `offset + amplitude·sin(2π·carrier·t + index·sin(2π·signal·t))`.
    Sffm {
        offset: f64,
        amplitude: f64,
        carrier: f64,
        index: f64,
        signal: f64,
    },
}

/// Most times [`Waveform::breakpoints`] returns. The transient analysis
/// rejects waveforms that would exceed it before simulating anything (see
/// [`Waveform::breakpoint_count`]).
pub(crate) const BREAKPOINT_LIMIT: usize = 10_000_000;

/// How many cycle-index corrections [`cycle_index`] may apply. Estimating the
/// cycle by division is off by at most one; the bound keeps evaluation finite
/// even when the period is too small to change `start + k·period` at all.
const CYCLE_CORRECTIONS: usize = 2;

/// Which side of a jump an evaluation takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// Right-continuous: at a boundary the later segment applies.
    After,
    /// Left limit: at a boundary the earlier segment applies.
    Before,
}

impl Side {
    /// Whether `t` belongs to the segment that ends at `boundary`.
    fn precedes(self, t: f64, boundary: f64) -> bool {
        match self {
            Side::After => t < boundary,
            Side::Before => t <= boundary,
        }
    }
}

impl Waveform {
    /// Value at time `t ≥ 0`.
    ///
    /// Right-continuous at ideal edges; never panics, even for parameters
    /// that validation rejects (an empty PWL evaluates to 0).
    pub fn value(&self, t: f64) -> f64 {
        self.evaluate(t, Side::After)
    }

    /// Left limit at `t`: equal to [`Self::value`] except exactly at an ideal
    /// edge, where it is the level before the jump.
    pub fn value_before(&self, t: f64) -> f64 {
        self.evaluate(t, Side::Before)
    }

    /// Times in `(0, stop]` where the waveform has a slope discontinuity, in
    /// increasing order without duplicates. The transient solver lands a
    /// timestep exactly on each one.
    ///
    /// At most 10 000 000 times are returned; [`Self::breakpoint_count`]
    /// tells callers in advance whether a waveform stays below that.
    pub fn breakpoints(&self, stop: f64) -> Vec<f64> {
        let mut times = Vec::new();
        match self {
            Self::Pulse { .. } => {
                if let Some(pulse) = PulseTrain::new(self) {
                    pulse.breakpoints(stop, &mut times);
                }
            }
            Self::Sine {
                frequency,
                delay,
                cycles,
                ..
            } => {
                times.push(*delay);
                if let Some(cycles) = cycles {
                    times.push(sine_end(*delay, *frequency, *cycles));
                }
            }
            Self::Exp {
                rise_delay,
                fall_delay,
                ..
            } => times.extend([*rise_delay, *fall_delay]),
            Self::Pwl { points, repeat } => Pwl::new(points, *repeat).breakpoints(stop, &mut times),
            Self::Sffm { .. } => {}
        }
        times.retain(|&t| t > 0.0 && t <= stop);
        times.sort_by(f64::total_cmp);
        times.dedup();
        times.truncate(BREAKPOINT_LIMIT);
        times
    }

    /// Longest step from `t` over which the straight line between the
    /// waveform's values at both ends stays within `reltol·swing + absolute`
    /// of the waveform, where `swing` is its peak-to-peak range (the result
    /// tolerance of D18). `f64::INFINITY` where the waveform is constant or
    /// piecewise linear: its corners are breakpoints instead.
    ///
    /// It bounds the interpolation error `h²/8·max|f''|` with `|f''|` taken
    /// at `t`, where the decaying envelopes of SINE and EXP are largest; a
    /// SINE with negative damping grows, so its envelope is taken at the end
    /// of the step. A sine needs about `π/(2√reltol)` steps per period (1571
    /// at reltol 1e-6).
    pub(crate) fn interpolation_step(&self, t: f64, reltol: f64, absolute: f64) -> f64 {
        let step = |curvature: f64, swing: f64| {
            if curvature > 0.0 && curvature.is_finite() {
                (8.0 * (reltol * swing + absolute) / curvature).sqrt()
            } else {
                f64::INFINITY
            }
        };
        match self {
            Self::Sine {
                amplitude,
                frequency,
                delay,
                damping,
                cycles,
                ..
            } => {
                let active =
                    t >= *delay && cycles.is_none_or(|n| t < sine_end(*delay, *frequency, n));
                if !active {
                    return f64::INFINITY;
                }
                let envelope = amplitude.abs() * (-damping * (t - delay)).exp();
                let rate = std::f64::consts::TAU * frequency + damping.abs();
                let h = step(envelope * rate * rate, 2.0 * envelope);
                if *damping < 0.0 && h.is_finite() {
                    let grown = envelope * (-damping * h).exp();
                    step(grown * rate * rate, 2.0 * envelope)
                } else {
                    h
                }
            }
            Self::Exp {
                initial,
                pulsed,
                rise_delay,
                rise_tau,
                fall_delay,
                fall_tau,
            } => {
                let delta = (pulsed - initial).abs();
                let mut curvature = 0.0;
                if t >= *rise_delay {
                    curvature +=
                        delta / (rise_tau * rise_tau) * (-(t - rise_delay) / rise_tau).exp();
                }
                if t >= *fall_delay {
                    curvature +=
                        delta / (fall_tau * fall_tau) * (-(t - fall_delay) / fall_tau).exp();
                }
                step(curvature, delta)
            }
            Self::Sffm {
                amplitude,
                carrier,
                index,
                signal,
                ..
            } => {
                use std::f64::consts::TAU;
                let (wc, ws, m) = (TAU * carrier, TAU * signal, index.abs());
                let rate = wc + m * ws;
                step(
                    amplitude.abs() * (rate * rate + m * ws * ws),
                    2.0 * amplitude.abs(),
                )
            }
            Self::Pulse { .. } | Self::Pwl { .. } => f64::INFINITY,
        }
    }

    /// Upper bound on the number of breakpoints in `(0, stop]`, computed
    /// without generating them, so absurd parameter combinations (a 1 ps
    /// period over a 1 s run) can be rejected cheaply.
    pub fn breakpoint_count(&self, stop: f64) -> f64 {
        match self {
            Self::Pulse {
                delay,
                period,
                cycles,
                ..
            } => {
                let mut count = if *period > 0.0 {
                    ((stop - delay) / period).floor().max(0.0) + 1.0
                } else {
                    1.0
                };
                if let Some(cycles) = cycles {
                    count = count.min(f64::from(*cycles));
                }
                4.0 * count
            }
            Self::Pwl { points, repeat } => match Pwl::new(points, *repeat).period() {
                Some(period) => ((stop / period).floor().max(0.0) + 1.0) * points.len() as f64,
                None => points.len() as f64,
            },
            Self::Sine { .. } | Self::Exp { .. } => 2.0,
            Self::Sffm { .. } => 0.0,
        }
    }

    fn evaluate(&self, t: f64, side: Side) -> f64 {
        match self {
            Self::Pulse { initial, .. } => {
                PulseTrain::new(self).map_or(*initial, |pulse| pulse.value(t, side))
            }
            Self::Sine {
                offset,
                amplitude,
                frequency,
                delay,
                damping,
                phase,
                cycles,
            } => {
                let phase = phase.to_radians();
                if t < *delay {
                    return offset + amplitude * phase.sin();
                }
                let mut local = t - delay;
                if let Some(cycles) = cycles
                    && !side.precedes(t, sine_end(*delay, *frequency, *cycles))
                {
                    // Hold the value reached at the end of the last cycle.
                    local = cycles / frequency;
                }
                offset
                    + amplitude
                        * (-damping * local).exp()
                        * (std::f64::consts::TAU * frequency * local + phase).sin()
            }
            Self::Exp {
                initial,
                pulsed,
                rise_delay,
                rise_tau,
                fall_delay,
                fall_tau,
            } => {
                // `-exp_m1(-x)` is `1 − e^(−x)` without cancellation near the delays.
                let mut v = *initial;
                if t > *rise_delay {
                    v += (pulsed - initial) * -(-(t - rise_delay) / rise_tau).exp_m1();
                }
                if t > *fall_delay {
                    v += (initial - pulsed) * -(-(t - fall_delay) / fall_tau).exp_m1();
                }
                v
            }
            Self::Pwl { points, repeat } => Pwl::new(points, *repeat).value(t, side),
            Self::Sffm {
                offset,
                amplitude,
                carrier,
                index,
                signal,
            } => {
                use std::f64::consts::TAU;
                offset + amplitude * (TAU * carrier * t + index * (TAU * signal * t).sin()).sin()
            }
        }
    }

    pub(crate) fn validate(&self, name: &str) -> Result<(), CircuitError> {
        let fail = |what: String| Err(CircuitError::new(format!("{name}: {what}")));
        let finite = |kind: &str, values: &[(&str, f64)]| match values
            .iter()
            .find(|(_, v)| !v.is_finite())
        {
            Some((what, v)) => Err(CircuitError::new(format!(
                "{name}: {kind} {what} must be finite, got {v}"
            ))),
            None => Ok(()),
        };
        let non_negative =
            |kind: &str, values: &[(&str, f64)]| match values.iter().find(|(_, v)| *v < 0.0) {
                Some((what, v)) => Err(CircuitError::new(format!(
                    "{name}: {kind} {what} must not be negative, got {v}"
                ))),
                None => Ok(()),
            };
        match self {
            Self::Pulse {
                initial,
                pulsed,
                delay,
                rise,
                fall,
                width,
                period,
                cycles,
            } => {
                let times = [
                    ("delay", *delay),
                    ("rise time", *rise),
                    ("fall time", *fall),
                    ("width", *width),
                    ("period", *period),
                ];
                finite(
                    "PULSE",
                    &[("initial value", *initial), ("pulsed value", *pulsed)],
                )?;
                finite("PULSE", &times)?;
                non_negative("PULSE", &times)?;
                let active = rise + width + fall;
                if *period > 0.0 && *period < active {
                    return fail(format!(
                        "PULSE period ({period} s) is shorter than rise + width + fall ({active} s)"
                    ));
                }
                if *cycles == Some(0) {
                    return fail("PULSE cycle count must be at least 1".into());
                }
                Ok(())
            }
            Self::Sine {
                offset,
                amplitude,
                frequency,
                delay,
                damping,
                phase,
                cycles,
            } => {
                finite(
                    "SINE",
                    &[
                        ("offset", *offset),
                        ("amplitude", *amplitude),
                        ("frequency", *frequency),
                        ("delay", *delay),
                        ("damping", *damping),
                        ("phase", *phase),
                    ],
                )?;
                non_negative("SINE", &[("frequency", *frequency), ("delay", *delay)])?;
                if let Some(cycles) = cycles {
                    if !(cycles.is_finite() && *cycles > 0.0) {
                        return fail(format!("SINE cycle count must be positive, got {cycles}"));
                    }
                    if *frequency == 0.0 {
                        return fail("SINE cycle count needs a non-zero frequency".into());
                    }
                }
                Ok(())
            }
            Self::Exp {
                initial,
                pulsed,
                rise_delay,
                rise_tau,
                fall_delay,
                fall_tau,
            } => {
                let times = [
                    ("rise delay", *rise_delay),
                    ("rise time constant", *rise_tau),
                    ("fall delay", *fall_delay),
                    ("fall time constant", *fall_tau),
                ];
                finite(
                    "EXP",
                    &[("initial value", *initial), ("pulsed value", *pulsed)],
                )?;
                finite("EXP", &times)?;
                non_negative("EXP", &times)?;
                if *rise_tau == 0.0 || *fall_tau == 0.0 {
                    return fail("EXP time constants must be positive".into());
                }
                if fall_delay < rise_delay {
                    return fail(format!(
                        "EXP fall delay ({fall_delay} s) must not precede the rise delay ({rise_delay} s)"
                    ));
                }
                Ok(())
            }
            Self::Pwl { points, repeat } => {
                if points.is_empty() {
                    return fail("PWL needs at least one point".into());
                }
                if *repeat && points.len() < 2 {
                    return fail("a repeating PWL needs at least two points".into());
                }
                for (i, &(time, value)) in points.iter().enumerate() {
                    if !time.is_finite() || !value.is_finite() {
                        return fail(format!("PWL point {} must be finite", i + 1));
                    }
                    if time < 0.0 {
                        return fail(format!(
                            "PWL point {} has a negative time ({time} s)",
                            i + 1
                        ));
                    }
                }
                if let Some(i) = points.windows(2).position(|w| w[1].0 <= w[0].0) {
                    return fail(format!(
                        "PWL times must increase strictly (point {} at {} s follows {} s)",
                        i + 2,
                        points[i + 1].0,
                        points[i].0
                    ));
                }
                Ok(())
            }
            Self::Sffm {
                offset,
                amplitude,
                carrier,
                index,
                signal,
            } => {
                finite(
                    "SFFM",
                    &[
                        ("offset", *offset),
                        ("amplitude", *amplitude),
                        ("carrier frequency", *carrier),
                        ("modulation index", *index),
                        ("signal frequency", *signal),
                    ],
                )?;
                non_negative(
                    "SFFM",
                    &[
                        ("carrier frequency", *carrier),
                        ("signal frequency", *signal),
                    ],
                )
            }
        }
    }
}

/// Time at which a SINE with `cycles` periods stops and holds its value.
fn sine_end(delay: f64, frequency: f64, cycles: f64) -> f64 {
    delay + cycles / frequency
}

/// Linear interpolation from `from` at `start` to `to` at `start +
/// duration`; a zero duration is an ideal edge that has not happened yet.
fn ramp(from: f64, to: f64, start: f64, duration: f64, t: f64) -> f64 {
    if duration > 0.0 {
        from + (to - from) * ((t - start) / duration).clamp(0.0, 1.0)
    } else {
        from
    }
}

/// Index of the cycle containing `t`, for cycles starting at `start(k)`
/// (increasing in `k`, with `start(0) ≤ t`). `estimate` is the cycle number
/// obtained by division, which rounding can put one cycle off.
fn cycle_index(t: f64, side: Side, estimate: f64, start: impl Fn(u64) -> f64) -> u64 {
    // `as` saturates, so NaN and huge estimates stay in range.
    let mut k = estimate.floor().max(0.0) as u64;
    for _ in 0..CYCLE_CORRECTIONS {
        if !side.precedes(t, start(k.saturating_add(1))) {
            k = k.saturating_add(1);
        } else if k > 0 && side.precedes(t, start(k)) {
            k -= 1;
        } else {
            break;
        }
    }
    k
}

/// The parameters of [`Waveform::Pulse`], with cycle arithmetic.
#[derive(Debug, Clone, Copy)]
struct PulseTrain {
    initial: f64,
    pulsed: f64,
    delay: f64,
    rise: f64,
    fall: f64,
    width: f64,
    period: f64,
    cycles: Option<u32>,
}

/// Segment boundaries of one pulse.
struct PulseEdges {
    /// The rise begins.
    start: f64,
    /// The rise ends and the flat top begins.
    risen: f64,
    /// The flat top ends and the fall begins.
    falling: f64,
    /// The fall ends.
    fallen: f64,
}

impl PulseTrain {
    fn new(waveform: &Waveform) -> Option<Self> {
        match *waveform {
            Waveform::Pulse {
                initial,
                pulsed,
                delay,
                rise,
                fall,
                width,
                period,
                cycles,
            } => Some(Self {
                initial,
                pulsed,
                delay,
                rise,
                fall,
                width,
                period,
                cycles,
            }),
            _ => None,
        }
    }

    fn start(&self, k: u64) -> f64 {
        self.delay + k as f64 * self.period
    }

    /// The single expression for pulse `k`'s boundaries, shared by
    /// evaluation and breakpoints.
    fn edges(&self, k: u64) -> PulseEdges {
        let start = self.start(k);
        let risen = start + self.rise;
        let falling = risen + self.width;
        let fallen = falling + self.fall;
        PulseEdges {
            start,
            risen,
            falling,
            fallen,
        }
    }

    /// Pulse index containing `t`, or `None` before the first pulse and
    /// after the last one.
    fn cycle(&self, t: f64, side: Side) -> Option<u64> {
        if side.precedes(t, self.delay) {
            return None;
        }
        let k = if self.period > 0.0 {
            cycle_index(t, side, (t - self.delay) / self.period, |k| self.start(k))
        } else {
            0
        };
        match self.cycles {
            Some(n) if k >= u64::from(n) => None,
            _ => Some(k),
        }
    }

    fn value(&self, t: f64, side: Side) -> f64 {
        let Some(k) = self.cycle(t, side) else {
            return self.initial;
        };
        let e = self.edges(k);
        if side.precedes(t, e.risen) {
            ramp(self.initial, self.pulsed, e.start, self.rise, t)
        } else if side.precedes(t, e.falling) {
            self.pulsed
        } else if side.precedes(t, e.fallen) {
            ramp(self.pulsed, self.initial, e.falling, self.fall, t)
        } else {
            self.initial
        }
    }

    fn breakpoints(&self, stop: f64, out: &mut Vec<f64>) {
        let pulses = match (self.period > 0.0, self.cycles) {
            (false, _) => 1,
            (true, Some(n)) => u64::from(n),
            (true, None) => u64::MAX,
        };
        for k in 0..pulses {
            let e = self.edges(k);
            if e.start.is_nan() || e.start > stop || out.len() >= BREAKPOINT_LIMIT {
                break;
            }
            out.extend([e.start, e.risen, e.falling, e.fallen]);
        }
    }
}

/// [`Waveform::Pwl`] evaluation. With `repeat`, repetition `k` covers
/// `[k·T, (k+1)·T)` where `T` is the time of the last point (LTspice
/// `REPEAT`); its points lie at `k·T + tⱼ`, and its last point is the start
/// of repetition `k + 1`.
struct Pwl<'a> {
    points: &'a [(f64, f64)],
    repeat: bool,
}

impl<'a> Pwl<'a> {
    fn new(points: &'a [(f64, f64)], repeat: bool) -> Self {
        Self { points, repeat }
    }

    /// Repetition period, when the sequence repeats and ends after `t = 0`.
    fn period(&self) -> Option<f64> {
        let last = self.points.last()?;
        (self.repeat && self.points.len() >= 2 && last.0 > 0.0).then_some(last.0)
    }

    /// Start of repetition `k`.
    fn start(k: u64, period: f64) -> f64 {
        k as f64 * period
    }

    /// Absolute time of point `j` in repetition `k`: the single expression
    /// shared by evaluation and breakpoints. The last point is the start of
    /// the next repetition, so both sides of the wrap share one boundary.
    fn time(&self, k: u64, j: usize, period: f64) -> f64 {
        if j + 1 == self.points.len() {
            Self::start(k.saturating_add(1), period)
        } else {
            Self::start(k, period) + self.points[j].0
        }
    }

    fn value(&self, t: f64, side: Side) -> f64 {
        let (Some(&(first_time, first_value)), Some(&(last_time, last_value))) =
            (self.points.first(), self.points.last())
        else {
            return 0.0;
        };
        if side.precedes(t, first_time) || self.points.len() == 1 {
            return first_value;
        }
        let last = self.points.len() - 1;
        let Some(period) = self.period() else {
            if !side.precedes(t, last_time) {
                return last_value;
            }
            // Segment `[point j, point j + 1]` containing `t`.
            let j = (1..last)
                .rev()
                .find(|&j| !side.precedes(t, self.points[j].0))
                .unwrap_or(0);
            let (start, end) = (self.points[j].0, self.points[j + 1].0);
            return ramp(
                self.points[j].1,
                self.points[j + 1].1,
                start,
                end - start,
                t,
            );
        };
        let k = cycle_index(t, side, t / period, |k| Self::start(k, period));
        let begin = Self::start(k, period);
        let first = self.time(k, 0, period);
        if k > 0 && side.precedes(t, first) {
            // From the previous repetition's last value to the first point.
            return ramp(last_value, first_value, begin, first - begin, t);
        }
        let j = (1..last)
            .rev()
            .find(|&j| !side.precedes(t, self.time(k, j, period)))
            .unwrap_or(0);
        let start = self.time(k, j, period);
        let end = self.time(k, j + 1, period);
        ramp(
            self.points[j].1,
            self.points[j + 1].1,
            start,
            end - start,
            t,
        )
    }

    fn breakpoints(&self, stop: f64, out: &mut Vec<f64>) {
        let Some(period) = self.period() else {
            out.extend(self.points.iter().map(|p| p.0));
            return;
        };
        // Each repetition contributes its start (the previous repetition's
        // last point) and every point but its last.
        let per_repetition = self.points.len() - 1;
        for k in 0.. {
            let start = Self::start(k, period);
            if start.is_nan() || start > stop || out.len() >= BREAKPOINT_LIMIT {
                break;
            }
            out.push(start);
            out.extend((0..per_repetition).map(|j| self.time(k, j, period)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{PI, TAU};

    fn pulse(rise: f64, fall: f64, period: f64, cycles: Option<u32>) -> Waveform {
        Waveform::Pulse {
            initial: -1.0,
            pulsed: 3.0,
            delay: 1.0,
            rise,
            fall,
            width: 2.0,
            period,
            cycles,
        }
    }

    fn ideal_pulse(delay: f64, period: f64) -> Waveform {
        Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay,
            rise: 0.0,
            fall: 0.0,
            width: period / 2.0,
            period,
            cycles: None,
        }
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12 * expected.abs().max(1.0),
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn pulse_follows_its_trapezoid() {
        let w = pulse(0.5, 0.25, 10.0, None);
        close(w.value(0.0), -1.0);
        close(w.value(1.0), -1.0);
        close(w.value(1.25), 1.0);
        close(w.value(1.5), 3.0);
        close(w.value(3.5), 3.0);
        close(w.value(3.625), 1.0);
        close(w.value(3.75), -1.0);
        close(w.value(9.0), -1.0);
        // The second period repeats the first.
        close(w.value(11.25), 1.0);
        close(w.value(13.625), 1.0);
    }

    #[test]
    fn pulse_cycles_and_single_pulse_end_at_the_initial_level() {
        let limited = pulse(0.5, 0.25, 10.0, Some(2));
        close(limited.value(12.0), 3.0);
        close(limited.value(22.0), -1.0);
        let single = pulse(0.5, 0.25, 0.0, None);
        close(single.value(2.0), 3.0);
        close(single.value(12.0), -1.0);
    }

    #[test]
    fn ideal_edges_are_right_continuous_with_a_left_limit() {
        let w = pulse(0.0, 0.0, 10.0, None);
        assert_eq!(w.value(1.0), 3.0);
        assert_eq!(w.value_before(1.0), -1.0);
        assert_eq!(w.value(3.0), -1.0);
        assert_eq!(w.value_before(3.0), 3.0);
        assert_eq!(w.value(11.0), 3.0);
        assert_eq!(w.value_before(11.0), -1.0);
        assert_eq!(w.value(2.0), w.value_before(2.0));
        // A pulse starting at t = 0 is already on at t = 0, which is also
        // its DC value.
        let at_zero = ideal_pulse(0.0, 2.0);
        assert_eq!(at_zero.value(0.0), 1.0);
        assert_eq!(Source::with_waveform(at_zero).dc, 1.0);
    }

    #[test]
    fn pulse_breakpoints_are_its_corners() {
        let w = pulse(0.5, 0.25, 10.0, None);
        assert_eq!(w.breakpoints(12.0), vec![1.0, 1.5, 3.5, 3.75, 11.0, 11.5]);
        let once = pulse(0.0, 0.0, 10.0, Some(1));
        assert_eq!(once.breakpoints(100.0), vec![1.0, 3.0]);
        assert_eq!(once.breakpoint_count(100.0), 4.0);
    }

    #[test]
    fn every_ideal_edge_breakpoint_selects_both_sides_exactly() {
        // A period that is not a binary fraction exercises the rounding of
        // `delay + k·period`.
        let w = ideal_pulse(0.1, 0.3);
        let times = w.breakpoints(30.0);
        assert_eq!(times.len(), 200);
        for t in times {
            assert_ne!(w.value(t), w.value_before(t), "edge at {t}");
        }
    }

    #[test]
    fn pulse_breakpoint_count_bounds_the_list() {
        let w = pulse(0.5, 0.25, 10.0, None);
        assert!(w.breakpoints(1000.0).len() as f64 <= w.breakpoint_count(1000.0));
        assert!(ideal_pulse(0.0, 1e-12).breakpoint_count(1.0) > 1e12);
    }

    #[test]
    fn sine_matches_its_closed_form() {
        let w = Waveform::Sine {
            offset: 0.5,
            amplitude: 2.0,
            frequency: 50.0,
            delay: 0.01,
            damping: 30.0,
            phase: 30.0,
            cycles: Some(2.0),
        };
        close(w.value(0.0), 0.5 + 2.0 * (PI / 6.0).sin());
        let t = 0.023;
        let local: f64 = t - 0.01;
        close(
            w.value(t),
            0.5 + 2.0 * (-30.0 * local).exp() * (TAU * 50.0 * local + PI / 6.0).sin(),
        );
        // Two cycles end at 0.05 s, after which the output holds the value
        // reached there (LTspice: 0.801194211912202), without a jump.
        let held = 0.5 + 2.0 * (-30.0 * 0.04_f64).exp() * (PI / 6.0).sin();
        close(w.value(0.06), held);
        close(w.value(0.05), held);
        close(w.value_before(0.05), held);
        assert!((held - 0.801194211912202).abs() < 1e-12);
        assert_eq!(w.breakpoints(1.0), vec![0.01, 0.05]);
        assert_eq!(w.breakpoints(0.03), vec![0.01]);
    }

    #[test]
    fn exp_matches_its_closed_form() {
        let w = Waveform::Exp {
            initial: 1.0,
            pulsed: 5.0,
            rise_delay: 1e-3,
            rise_tau: 2e-3,
            fall_delay: 4e-3,
            fall_tau: 1e-3,
        };
        close(w.value(0.5e-3), 1.0);
        let rising = |t: f64| 1.0 + 4.0 * (1.0 - (-(t - 1e-3) / 2e-3).exp());
        close(w.value(3e-3), rising(3e-3));
        let t = 6e-3;
        close(
            w.value(t),
            rising(t) - 4.0 * (1.0 - (-(t - 4e-3) / 1e-3).exp()),
        );
        assert_eq!(w.breakpoints(1.0), vec![1e-3, 4e-3]);
    }

    #[test]
    fn pwl_interpolates_and_holds() {
        let w = Waveform::Pwl {
            points: vec![(1.0, 2.0), (2.0, 4.0), (4.0, 0.0)],
            repeat: false,
        };
        close(w.value(0.0), 2.0);
        close(w.value(1.5), 3.0);
        close(w.value(3.0), 2.0);
        close(w.value(4.0), 0.0);
        close(w.value(10.0), 0.0);
        assert_eq!(w.breakpoints(3.0), vec![1.0, 2.0]);
        assert_eq!(w.breakpoints(10.0), vec![1.0, 2.0, 4.0]);
    }

    #[test]
    fn pwl_repeat_follows_ltspice_timing() {
        let w = Waveform::Pwl {
            points: vec![(1.0, 0.0), (2.0, 4.0), (3.0, 2.0)],
            repeat: true,
        };
        // Period 3 s (the last point's time); each repetition ramps from the
        // last value back to the first point (LTspice REPEAT).
        close(w.value(0.5), 0.0);
        close(w.value(1.5), 2.0);
        close(w.value(3.5), 1.0);
        close(w.value(4.5), 2.0);
        close(w.value(6.5), 1.0);
        close(w.value(8.5), 3.0);
        assert_eq!(w.value(3.0), 2.0);
        assert_eq!(w.value_before(3.0), 2.0);
        assert_eq!(w.value(4.0), 0.0);
        assert_eq!(w.breakpoints(7.0), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert!(w.breakpoints(100.0).len() as f64 <= w.breakpoint_count(100.0));
    }

    #[test]
    fn pwl_repeat_from_zero_jumps_back_to_its_first_value() {
        let w = Waveform::Pwl {
            points: vec![(0.0, 0.0), (1.0, 4.0), (2.0, 2.0)],
            repeat: true,
        };
        close(w.value(0.5), 2.0);
        close(w.value(2.5), 2.0);
        assert_eq!(w.value(2.0), 0.0);
        assert_eq!(w.value_before(2.0), 2.0);
        assert_eq!(w.value(4.0), 0.0);
        assert_eq!(w.value_before(4.0), 2.0);
        assert_eq!(w.breakpoints(4.0), vec![1.0, 2.0, 3.0, 4.0]);
        for t in w.breakpoints(40.0).into_iter().filter(|t| t % 2.0 == 0.0) {
            assert_ne!(w.value(t), w.value_before(t), "jump at {t}");
        }
    }

    #[test]
    fn smooth_waveforms_limit_the_step_to_their_interpolation_accuracy() {
        let sine = Waveform::Sine {
            offset: 0.0,
            amplitude: 3.0,
            frequency: 1e3,
            delay: 1e-3,
            damping: 0.0,
            phase: 0.0,
            cycles: Some(2.0),
        };
        let h = sine.interpolation_step(1.5e-3, 1e-6, 0.0);
        // About π/(2√reltol) steps per period.
        let per_period = 1e-3 / h;
        assert!((per_period - PI / (2.0 * 1e-3)).abs() < 1.0, "{per_period}");
        // Constant before the delay and after the last cycle.
        assert_eq!(sine.interpolation_step(0.5e-3, 1e-6, 0.0), f64::INFINITY);
        assert_eq!(sine.interpolation_step(3e-3, 1e-6, 0.0), f64::INFINITY);
        // The interpolation error over that step is within the tolerance.
        let (t, mid) = (1.6e-3, 1.6e-3 + h / 2.0);
        let line = (sine.value(t) + sine.value(t + h)) / 2.0;
        assert!((line - sine.value(mid)).abs() <= 1e-6 * 6.0 * (1.0 + 1e-9));
        let exp = Waveform::Exp {
            initial: 0.0,
            pulsed: 1.0,
            rise_delay: 0.0,
            rise_tau: 1e-6,
            fall_delay: 1.0,
            fall_tau: 1.0,
        };
        // The step grows as the exponential settles.
        assert!(
            exp.interpolation_step(10e-6, 1e-6, 1e-9)
                > 10.0 * exp.interpolation_step(0.0, 1e-6, 1e-9)
        );
        assert_eq!(
            pulse(0.5, 0.25, 10.0, None).interpolation_step(1.2, 1e-6, 0.0),
            f64::INFINITY
        );
    }

    #[test]
    fn pwl_single_point_is_constant() {
        let w = Waveform::Pwl {
            points: vec![(0.5, 7.0)],
            repeat: false,
        };
        assert_eq!(w.value(0.0), 7.0);
        assert_eq!(w.value(9.0), 7.0);
        assert_eq!(w.breakpoints(1.0), vec![0.5]);
    }

    #[test]
    fn sffm_matches_its_closed_form() {
        let w = Waveform::Sffm {
            offset: 1.0,
            amplitude: 0.5,
            carrier: 1e3,
            index: 2.0,
            signal: 50.0,
        };
        let t = 1.234e-3;
        close(
            w.value(t),
            1.0 + 0.5 * (TAU * 1e3 * t + 2.0 * (TAU * 50.0 * t).sin()).sin(),
        );
        assert!(w.breakpoints(1.0).is_empty());
    }

    #[test]
    fn evaluation_never_panics_on_invalid_parameters() {
        let empty = Waveform::Pwl {
            points: Vec::new(),
            repeat: true,
        };
        assert_eq!(empty.value(1.0), 0.0);
        assert!(empty.breakpoints(1.0).is_empty());
        let nan = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay: f64::NAN,
            rise: f64::NAN,
            fall: 0.0,
            width: 0.0,
            period: f64::NAN,
            cycles: None,
        };
        let _ = nan.value(1.0);
        assert!(nan.breakpoints(1.0).is_empty());
        let _ = ideal_pulse(0.0, 1e-300).value(1e10);
    }

    #[test]
    fn validation_rejects_meaningless_parameters() {
        assert!(pulse(0.5, 0.25, 10.0, None).validate("V1").is_ok());
        let sine = |frequency: f64, cycles: Option<f64>| Waveform::Sine {
            offset: 0.0,
            amplitude: 1.0,
            frequency,
            delay: 0.0,
            damping: 0.0,
            phase: 0.0,
            cycles,
        };
        let exp = |rise_delay: f64, rise_tau: f64, fall_delay: f64| Waveform::Exp {
            initial: 0.0,
            pulsed: 1.0,
            rise_delay,
            rise_tau,
            fall_delay,
            fall_tau: 1.0,
        };
        let pwl = |points: Vec<(f64, f64)>, repeat: bool| Waveform::Pwl { points, repeat };
        let invalid = [
            pulse(-1.0, 0.25, 10.0, None),
            pulse(0.5, f64::NAN, 10.0, None),
            pulse(0.5, 0.25, 1.0, None),
            pulse(0.5, 0.25, 10.0, Some(0)),
            sine(-1.0, None),
            sine(0.0, Some(1.0)),
            sine(1.0, Some(0.0)),
            exp(2.0, 1.0, 1.0),
            exp(0.0, 0.0, 1.0),
            pwl(Vec::new(), false),
            pwl(vec![(0.0, 1.0), (0.0, 2.0)], false),
            pwl(vec![(-1.0, 1.0), (1.0, 2.0)], false),
            pwl(vec![(1.0, 1.0)], true),
            pwl(vec![(0.0, f64::INFINITY)], false),
            Waveform::Sffm {
                offset: 0.0,
                amplitude: 1.0,
                carrier: -5.0,
                index: 1.0,
                signal: 1.0,
            },
        ];
        for w in invalid {
            let error = w
                .validate("V7")
                .expect_err(&format!("{w:?} should be invalid"));
            assert!(error.message.starts_with("V7: "), "{}", error.message);
        }
    }

    #[test]
    fn source_values_fall_back_to_dc_without_a_waveform() {
        let s = Source::ac(2.0, 1.0, 45.0);
        assert_eq!(s.transient_value(1.0), 2.0);
        assert_eq!(s.transient_value_before(1.0), 2.0);
        let mut bad = Source::dc(f64::NAN);
        assert!(bad.validate("I1").is_err());
        bad.dc = 1.0;
        bad.ac_phase = f64::INFINITY;
        assert!(bad.validate("I1").is_err());
    }
}
