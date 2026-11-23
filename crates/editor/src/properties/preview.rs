//! Samples a transient waveform for the live preview in the properties
//! dialog, places it vertically and labels its axes. GPUI-free.
//!
//! The preview shows exactly one period of a periodic waveform (D14):
//! - PULSE with a period: from the delay to one period later; without a
//!   period, the delay and the single pulse with a short tail.
//! - SINE: the delay and one cycle (just one cycle without a delay).
//! - SFFM: one period of the slower of its two frequencies.
//! - PWL with repeat: the first repeat, from 0 to its last point. The
//!   engine repeats with the last point's time as the period (LTspice
//!   `REPEAT`): later repeats ramp from the last value to the first point
//!   where the first holds its value.
//! - EXP and PWL without repeat: the whole shape with a short tail.

use super::spec::Unit;
use xsim_spice_engine::{Waveform, units};

/// Evenly spaced samples across the preview window (breakpoints are added).
const SAMPLES: usize = 240;
/// Extra samples across each stretch where the waveform curves (a SINE
/// cycle, an EXP transition), so a long delay cannot starve it.
const DETAIL_SAMPLES: usize = 120;
/// Samples per period of the fastest frequency of an SFFM waveform.
const SAMPLES_PER_CYCLE: f64 = 16.0;
/// Upper bound on the uniform samples of one window.
const MAX_SAMPLES: usize = 4_000;
/// Upper bound on breakpoints taken into the preview.
const MAX_BREAKPOINTS: usize = 2_000;
/// Time constants an EXP transition is drawn for.
const EXP_TAUS: f64 = 5.0;
/// Tail after a one-shot shape, as a fraction of the shape's duration.
const TAIL: f64 = 0.2;
/// Significant digits of the axis labels.
const LABEL_DIGITS: usize = 3;
/// Most significant digits of a time label: enough to tell a short period
/// from its long delay (1 µs after 1 s).
const MAX_TIME_DIGITS: usize = 12;

/// The stretch of time the preview shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TimeWindow {
    pub start: f64,
    pub end: f64,
}

impl TimeWindow {
    pub(crate) fn duration(&self) -> f64 {
        self.end - self.start
    }
}

/// The window that shows one period of `waveform`, or its whole shape when
/// it does not repeat.
pub(crate) fn window(waveform: &Waveform) -> TimeWindow {
    let (start, end) = match *waveform {
        Waveform::Pulse {
            delay,
            rise,
            fall,
            width,
            period,
            ..
        } => {
            if period > 0.0 {
                (delay, delay + period)
            } else {
                let active = rise + width + fall;
                (0.0, delay + (1.0 + TAIL) * active)
            }
        }
        Waveform::Sine {
            frequency, delay, ..
        } => (0.0, delay + 1.0 / frequency),
        Waveform::Exp {
            rise_delay,
            rise_tau,
            fall_delay,
            fall_tau,
            ..
        } => {
            // Both transitions run their course, then a short flat tail.
            let shape = (fall_delay + EXP_TAUS * fall_tau).max(rise_delay + EXP_TAUS * rise_tau);
            (0.0, (1.0 + TAIL) * shape)
        }
        Waveform::Pwl { ref points, repeat } => {
            let first = points.first().map_or(0.0, |p| p.0);
            let last = points.last().map_or(0.0, |p| p.0);
            // Repeat k covers [k·last, (k + 1)·last), as in the engine.
            if repeat && points.len() >= 2 && last > 0.0 {
                (0.0, last)
            } else {
                let start = first.min(0.0);
                (start, last + TAIL * (last - start))
            }
        }
        // One period of the slower frequency.
        Waveform::Sffm {
            carrier, signal, ..
        } => (0.0, 1.0 / signal.min(carrier)),
    };
    if start.is_finite() && end.is_finite() && start >= 0.0 && end > start {
        TimeWindow { start, end }
    } else {
        TimeWindow {
            start: 0.0,
            end: 1.0,
        }
    }
}

/// `count + 1` evenly spaced times from `start` to `end`.
fn uniform(start: f64, end: f64, count: usize) -> impl Iterator<Item = f64> {
    (0..=count).map(move |i| start + (end - start) * i as f64 / count as f64)
}

/// Stretches of `waveform` that curve and get [`DETAIL_SAMPLES`] of their
/// own, as `(start, end)`.
fn detail_stretches(waveform: &Waveform) -> Vec<(f64, f64)> {
    match *waveform {
        Waveform::Sine {
            frequency, delay, ..
        } => vec![(delay, delay + 1.0 / frequency)],
        Waveform::Exp {
            rise_delay,
            rise_tau,
            fall_delay,
            fall_tau,
            ..
        } => vec![
            (rise_delay, fall_delay.min(rise_delay + EXP_TAUS * rise_tau)),
            (fall_delay, fall_delay + EXP_TAUS * fall_tau),
        ],
        _ => Vec::new(),
    }
}

/// Uniform sample count across `window`: more for an SFFM waveform, whose
/// carrier may fit many cycles into one signal period.
fn sample_count(waveform: &Waveform, window: &TimeWindow) -> usize {
    match *waveform {
        Waveform::Sffm {
            carrier,
            index,
            signal,
            ..
        } => {
            // The instantaneous frequency peaks at carrier + index · signal.
            let fastest = carrier + index.abs() * signal;
            let wanted = (window.duration() * fastest * SAMPLES_PER_CYCLE).ceil();
            if wanted.is_finite() {
                (wanted as usize).clamp(SAMPLES, MAX_SAMPLES)
            } else {
                MAX_SAMPLES
            }
        }
        _ => SAMPLES,
    }
}

/// `(time, value)` points of `waveform` across `window`, in time order.
/// Ideal edges appear as two points at the same time (before and after),
/// the window's start included; the window's end takes the value just
/// before it, so the next period's first edge is not drawn on the border.
pub(crate) fn samples(waveform: &Waveform, window: &TimeWindow) -> Vec<(f64, f64)> {
    let (start, end) = (window.start, window.end);
    let mut times: Vec<(f64, bool)> = uniform(start, end, sample_count(waveform, window))
        .map(|t| (t, false))
        .collect();
    for (from, to) in detail_stretches(waveform) {
        let (from, to) = (from.max(start), to.min(end));
        if to > from {
            times.extend(uniform(from, to, DETAIL_SAMPLES).map(|t| (t, false)));
        }
    }
    let edges = waveform
        .breakpoints(end)
        .into_iter()
        .filter(|&t| t >= start)
        .take(MAX_BREAKPOINTS)
        .map(|t| (t, true));
    times.extend(edges);
    times.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, bool)> = Vec::with_capacity(times.len());
    for (t, edge) in times {
        match merged.last_mut() {
            Some(last) if last.0 == t => last.1 |= edge,
            _ => merged.push((t, edge)),
        }
    }
    merged
        .into_iter()
        .flat_map(|(t, edge)| {
            if t >= end {
                return vec![(t, waveform.value_before(t))];
            }
            let mut points = Vec::with_capacity(2);
            // An edge right at the start (a PULSE without delay) is drawn
            // too, so the period starts with its rise.
            if edge || t <= start {
                points.push((t, waveform.value_before(t)));
            }
            points.push((t, waveform.value(t)));
            points
        })
        .collect()
}

/// The smallest and largest finite value of `points`.
pub(crate) fn extremes(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let finite = points.iter().map(|p| p.1).filter(|v| v.is_finite());
    let (lo, hi) = finite.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    (lo <= hi).then_some((lo, hi))
}

/// Where `value` is drawn between the extremes `lo` and `hi`: 0 at the top
/// of the chart (the largest value) to 1 at its bottom (the smallest). The
/// chart spans exactly the curve, so the value labels at its top and bottom
/// edges mark the extremes. A flat line is drawn in the middle, level with
/// its one label.
pub(crate) fn level(value: f64, lo: f64, hi: f64) -> f64 {
    if hi > lo {
        (hi - value) / (hi - lo)
    } else {
        0.5
    }
}

/// The axis labels around the preview chart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Labels {
    /// Time at the left edge.
    pub start: String,
    /// Time at the right edge.
    pub end: String,
    /// Largest value, beside the top of the curve.
    pub max: String,
    /// Smallest value, beside the bottom of the curve; `None` for a
    /// flat line, whose one value is `max`.
    pub min: Option<String>,
}

/// Engineering text of `value` for an axis label.
fn short(value: f64, unit: Unit) -> String {
    units::format_value_digits(value, unit.symbol(), LABEL_DIGITS)
}

/// Texts of the window's start and end times, with as many digits as it
/// takes to tell them apart (a 1 µs period after a 1 s delay reads
/// "1 s" to "1.000001 s", not "1 s" twice).
fn time_labels(window: &TimeWindow) -> (String, String) {
    let at = |digits: usize| {
        let text = |t: f64| units::format_value_digits(t, Unit::Second.symbol(), digits);
        (text(window.start), text(window.end))
    };
    (LABEL_DIGITS..MAX_TIME_DIGITS)
        .map(at)
        .find(|(start, end)| start != end)
        .unwrap_or_else(|| at(MAX_TIME_DIGITS))
}

/// The labels of a preview of `points` over `window`, values in `signal`.
pub(crate) fn labels(window: &TimeWindow, points: &[(f64, f64)], signal: Unit) -> Labels {
    let (lo, hi) = extremes(points).unwrap_or((0.0, 0.0));
    // Rounding noise such as sin(π) ≈ 1e-16 reads as zero.
    let scale = lo.abs().max(hi.abs());
    let clean = |v: f64| if v.abs() <= 1e-9 * scale { 0.0 } else { v };
    let (lo, hi) = (clean(lo), clean(hi));
    let (start, end) = time_labels(window);
    Labels {
        start,
        end,
        max: short(hi, signal),
        min: (hi > lo).then(|| short(lo, signal)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 0 → 1 pulse with ideal edges, 1 ms wide.
    fn pulse(delay: f64, period: f64) -> Waveform {
        trapezoid(delay, 0.0, period, None)
    }

    /// A 0 → 1 pulse 1 ms wide with rise and fall `edge`.
    fn trapezoid(delay: f64, edge: f64, period: f64, cycles: Option<u32>) -> Waveform {
        Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay,
            rise: edge,
            fall: edge,
            width: 1e-3,
            period,
            cycles,
        }
    }

    fn sine(delay: f64) -> Waveform {
        Waveform::Sine {
            offset: 0.0,
            amplitude: 1.0,
            frequency: 1e3,
            delay,
            damping: 0.0,
            phase: 0.0,
            cycles: None,
        }
    }

    fn sffm(carrier: f64, signal: f64) -> Waveform {
        Waveform::Sffm {
            offset: 0.0,
            amplitude: 1.0,
            carrier,
            index: 5.0,
            signal,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1e-30)
    }

    fn assert_window(waveform: &Waveform, start: f64, end: f64) {
        let w = window(waveform);
        assert!(
            close(w.start, start) && close(w.end, end),
            "{waveform:?}: {w:?}, expected {start}..{end}"
        );
    }

    #[test]
    fn a_periodic_pulse_shows_one_period_from_its_delay() {
        assert_window(&pulse(0.0, 2e-3), 0.0, 2e-3);
        assert_window(&pulse(1e-3, 2e-3), 1e-3, 3e-3);
        assert_window(&trapezoid(1e-3, 0.0, 2e-3, Some(3)), 1e-3, 3e-3);
    }

    #[test]
    fn a_single_pulse_shows_delay_pulse_and_a_tail() {
        // Delay 1 ms, then 1 ms of pulse and a 0.2 ms tail.
        assert_window(&pulse(1e-3, 0.0), 0.0, 2.2e-3);
        assert_window(&trapezoid(0.0, 1e-3, 0.0, None), 0.0, 3.6e-3);
    }

    #[test]
    fn a_sine_shows_its_delay_and_one_cycle() {
        assert_window(&sine(0.0), 0.0, 1e-3);
        assert_window(&sine(0.5e-3), 0.0, 1.5e-3);
    }

    #[test]
    fn an_sffm_shows_one_period_of_the_slower_frequency() {
        assert_window(&sffm(1e4, 1e3), 0.0, 1e-3);
        assert_window(&sffm(500.0, 1e4), 0.0, 2e-3);
    }

    #[test]
    fn a_repeating_pwl_shows_one_repeat() {
        let pwl = |repeat| Waveform::Pwl {
            points: vec![(1e-3, 0.0), (2e-3, 1.0), (4e-3, 0.0)],
            repeat,
        };
        // The engine repeats every 4 ms (the last point's time), not every
        // 3 ms (last minus first point).
        assert_window(&pwl(true), 0.0, 4e-3);
        let from_zero = Waveform::Pwl {
            points: vec![(0.0, 0.0), (1e-3, 1.0)],
            repeat: true,
        };
        assert_window(&from_zero, 0.0, 1e-3);
        // Without repeat: from 0 to the last point and a fifth more.
        assert_window(&pwl(false), 0.0, 4.8e-3);
    }

    /// The window of a periodic waveform spans exactly its period: the
    /// engine's waveform repeats after the window's duration.
    #[test]
    fn periodic_windows_span_exactly_one_period_of_the_engine_waveform() {
        let sffm = Waveform::Sffm {
            offset: 0.0,
            amplitude: 1.0,
            carrier: 1e4,
            index: 5.0,
            signal: 1e3,
        };
        let repeating_pwl = Waveform::Pwl {
            points: vec![(1e-3, 0.0), (2e-3, 1.0), (4e-3, 0.5)],
            repeat: true,
        };
        let cases = [
            (trapezoid(1e-3, 1e-4, 2e-3, None), 1e-3),
            (sine(0.0), 0.0),
            (sffm, 0.0),
            // Past the first repeat, which holds instead of ramping in.
            (repeating_pwl, 4e-3),
        ];
        for (waveform, settled) in cases {
            let period = window(&waveform).duration();
            for step in 0..40 {
                let t = settled + period * (0.37 + 0.11 * f64::from(step));
                let (now, later) = (waveform.value(t), waveform.value(t + period));
                assert!(
                    (now - later).abs() <= 1e-6,
                    "{waveform:?} at {t}: {now} then {later} a window later"
                );
            }
        }
    }

    #[test]
    fn time_labels_tell_a_short_period_from_its_long_delay() {
        let waveform = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay: 1.0,
            rise: 0.0,
            fall: 0.0,
            width: 0.5e-6,
            period: 1e-6,
            cycles: None,
        };
        let w = window(&waveform);
        let text = labels(&w, &samples(&waveform, &w), Unit::Volt);
        assert_eq!(text.start, "1 s");
        assert_eq!(text.end, "1.000001 s");
    }

    #[test]
    fn exp_shows_the_whole_shape_with_a_tail() {
        let exp = Waveform::Exp {
            initial: 0.0,
            pulsed: 1.0,
            rise_delay: 1e-3,
            rise_tau: 1e-4,
            fall_delay: 2e-3,
            fall_tau: 2e-4,
        };
        // The fall settles 5 τ after its delay, at 3 ms; a fifth more is tail.
        assert_window(&exp, 0.0, 3.6e-3);
        // A rise that is still settling when the fall starts.
        let overlapping = Waveform::Exp {
            initial: 0.0,
            pulsed: 1.0,
            rise_delay: 0.0,
            rise_tau: 1e-3,
            fall_delay: 1e-3,
            fall_tau: 1e-4,
        };
        assert_window(&overlapping, 0.0, 6e-3);
    }

    #[test]
    fn degenerate_windows_fall_back_to_one_second() {
        let flat = Waveform::Pwl {
            points: vec![(0.0, 3.0)],
            repeat: false,
        };
        assert_window(&flat, 0.0, 1.0);
        let single = Waveform::Pwl {
            points: vec![(0.0, 3.0)],
            repeat: true,
        };
        assert_window(&single, 0.0, 1.0);
    }

    #[test]
    fn samples_cover_exactly_the_window() {
        let waveform = pulse(1e-3, 2e-3);
        let points = samples(&waveform, &window(&waveform));
        assert!(points.windows(2).all(|w| w[0].0 <= w[1].0), "time order");
        assert_eq!(points.first().map(|p| p.0), Some(1e-3));
        assert_eq!(points.last().map(|p| p.0), Some(3e-3));
    }

    #[test]
    fn ideal_edges_are_vertical_inside_and_left_out_at_the_end() {
        // Rises at 1 ms (the window start), falls at 2 ms, rises again at
        // 3 ms (the window end).
        let waveform = pulse(1e-3, 2e-3);
        let points = samples(&waveform, &window(&waveform));
        let at = |t: f64| -> Vec<f64> { points.iter().filter(|p| p.0 == t).map(|p| p.1).collect() };
        assert_eq!(at(1e-3), [0.0, 1.0], "the first rise");
        assert_eq!(at(2e-3), [1.0, 0.0], "the fall");
        assert_eq!(at(3e-3), [0.0], "not the next period's rise");

        let undelayed = pulse(0.0, 2e-3);
        let points = samples(&undelayed, &window(&undelayed));
        assert_eq!(&points[..2], [(0.0, 0.0), (0.0, 1.0)], "the rise at 0");
    }

    #[test]
    fn a_long_delay_keeps_the_sine_cycle_detailed() {
        let waveform = sine(1.0);
        let w = window(&waveform);
        let points = samples(&waveform, &w);
        let in_cycle = points.iter().filter(|p| p.0 >= 1.0).count();
        assert!(in_cycle > DETAIL_SAMPLES, "{in_cycle} samples in the cycle");
        let (lo, hi) = extremes(&points).expect("finite values");
        assert!(lo < -0.99 && hi > 0.99, "the peaks are sampled: {lo}..{hi}");
    }

    #[test]
    fn an_sffm_gets_samples_for_every_carrier_cycle() {
        let waveform = sffm(1e6, 1e3);
        let points = samples(&waveform, &window(&waveform));
        assert!(points.len() >= MAX_SAMPLES, "{} samples", points.len());
        let slow = sffm(1e5, 1e3);
        let points = samples(&slow, &window(&slow));
        // Up to 105 kHz (100 kHz carrier, 5 kHz deviation) for 1 ms.
        assert!(points.len() > 1_600, "{} samples", points.len());
        assert!(points.len() < MAX_SAMPLES);
    }

    #[test]
    fn exp_transitions_are_sampled_in_detail() {
        let exp = Waveform::Exp {
            initial: 0.0,
            pulsed: 1.0,
            rise_delay: 1.0,
            rise_tau: 1e-6,
            fall_delay: 2.0,
            fall_tau: 1e-6,
        };
        let points = samples(&exp, &window(&exp));
        let rising = points
            .iter()
            .filter(|p| p.0 > 1.0 && p.0 < 1.0 + 5e-6)
            .count();
        assert!(rising >= DETAIL_SAMPLES - 1, "{rising} samples on the rise");
    }

    #[test]
    fn labels_name_the_window_ends_and_the_value_extremes() {
        let waveform = pulse(1e-3, 2e-3);
        let w = window(&waveform);
        let points = samples(&waveform, &w);
        let text = labels(&w, &points, Unit::Volt);
        assert_eq!(text.start, "1 ms");
        assert_eq!(text.end, "3 ms");
        assert_eq!(text.max, "1 V");
        assert_eq!(text.min.as_deref(), Some("0 V"));

        let waveform = sine(0.0);
        let w = window(&waveform);
        let text = labels(&w, &samples(&waveform, &w), Unit::Ampere);
        assert_eq!(text.start, "0 s");
        assert_eq!(text.end, "1 ms");
        assert_eq!(text.max, "1 A");
        assert_eq!(text.min.as_deref(), Some("-1 A"));
    }

    #[test]
    fn a_flat_line_has_one_value_label() {
        let flat = Waveform::Pwl {
            points: vec![(0.0, 2.5e-3), (1e-3, 2.5e-3)],
            repeat: false,
        };
        let w = window(&flat);
        let text = labels(&w, &samples(&flat, &w), Unit::Ampere);
        assert_eq!(text.max, "2.5 mA");
        assert_eq!(text.min, None);
        assert_eq!(text.end, "1.2 ms");
    }

    /// The extremes touch the chart's top and bottom edges, beside the value
    /// labels; nothing pads the curve away from them or from the title.
    #[test]
    fn the_extremes_span_the_whole_chart_height() {
        assert_eq!(level(10.0, 0.0, 10.0), 0.0, "the largest value on top");
        assert_eq!(level(0.0, 0.0, 10.0), 1.0, "the smallest at the bottom");
        assert_eq!(level(2.5, 0.0, 10.0), 0.75);
        assert_eq!(level(0.0, -1.0, 1.0), 0.5, "zero of a symmetric sine");
        assert_eq!(level(3.0, 3.0, 3.0), 0.5, "a flat line in the middle");
    }
}
