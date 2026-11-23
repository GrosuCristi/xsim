//! Transient waveform types of the source form (Q72): parameter tables,
//! defaults and conversion between [`Waveform`] and parameter values.
//! GPUI-free.

use super::spec::{Constraint, FieldId, FieldSpec, Unit};
use xsim_spice_engine::Waveform;

/// The transient function selected for a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WaveKind {
    /// The source holds its DC value during transient analysis.
    None,
    Pulse,
    Sine,
    Exp,
    Pwl,
    Sffm,
}

/// Initial text of a parameter when the user first selects its waveform.
#[derive(Debug, Clone, Copy)]
enum Seed {
    /// The waveform's signal level: `1` for voltage sources, `1m` for
    /// current sources.
    Level,
    Text(&'static str),
}

/// One parameter of a waveform type.
#[derive(Debug, Clone, Copy)]
struct Param {
    label: &'static str,
    /// `None` for a value in the source's own unit (V or A).
    unit: Option<Unit>,
    constraint: Constraint,
    empty: Option<&'static str>,
    seed: Seed,
}

const fn signal(label: &'static str, seed: Seed) -> Param {
    Param {
        label,
        unit: None,
        constraint: Constraint::Finite,
        empty: None,
        seed,
    }
}

const fn param(
    label: &'static str,
    unit: Unit,
    constraint: Constraint,
    seed: &'static str,
) -> Param {
    Param {
        label,
        unit: Some(unit),
        constraint,
        empty: None,
        seed: Seed::Text(seed),
    }
}

const fn cycles(constraint: Constraint) -> Param {
    Param {
        label: "Cycles",
        unit: Some(Unit::None),
        constraint,
        empty: Some("Unlimited"),
        seed: Seed::Text(""),
    }
}

use Constraint::{Finite, NonNegative, Positive};
use Seed::{Level, Text};

const PULSE: [Param; 8] = [
    signal("Initial value", Text("0")),
    signal("Pulsed value", Level),
    param("Delay", Unit::Second, NonNegative, "0"),
    param("Rise time", Unit::Second, NonNegative, "1n"),
    param("Fall time", Unit::Second, NonNegative, "1n"),
    param("Pulse width", Unit::Second, NonNegative, "500u"),
    param("Period", Unit::Second, NonNegative, "1m"),
    cycles(Constraint::Count),
];

const SINE: [Param; 7] = [
    signal("Offset", Text("0")),
    signal("Amplitude", Level),
    param("Frequency", Unit::Hertz, Positive, "1k"),
    param("Delay", Unit::Second, NonNegative, "0"),
    param("Damping", Unit::PerSecond, Finite, "0"),
    param("Phase", Unit::Degree, Finite, "0"),
    cycles(Positive),
];

const EXP: [Param; 6] = [
    signal("Initial value", Text("0")),
    signal("Pulsed value", Level),
    param("Rise delay", Unit::Second, NonNegative, "0"),
    param("Rise time constant", Unit::Second, Positive, "100u"),
    param("Fall delay", Unit::Second, NonNegative, "500u"),
    param("Fall time constant", Unit::Second, Positive, "100u"),
];

const SFFM: [Param; 5] = [
    signal("Offset", Text("0")),
    signal("Amplitude", Level),
    param("Carrier frequency", Unit::Hertz, Positive, "10k"),
    param("Modulation index", Unit::None, Finite, "5"),
    param("Signal frequency", Unit::Hertz, Positive, "1k"),
];

impl WaveKind {
    /// Every type, in selector order.
    pub(crate) const ALL: [WaveKind; 6] = [
        WaveKind::None,
        WaveKind::Pulse,
        WaveKind::Sine,
        WaveKind::Exp,
        WaveKind::Pwl,
        WaveKind::Sffm,
    ];

    /// Selector label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Pulse => "PULSE",
            Self::Sine => "SINE",
            Self::Exp => "EXP",
            Self::Pwl => "PWL",
            Self::Sffm => "SFFM",
        }
    }

    /// One-line explanation shown under the selector.
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::None => "The source keeps its DC value during transient analysis.",
            Self::Pulse => "Trapezoidal pulse train. A period of 0 gives a single pulse.",
            Self::Sine => "Damped sine wave, starting after the delay.",
            Self::Exp => "Exponential rise towards the pulsed value, then decay back.",
            Self::Pwl => "Straight lines between (time, value) points.",
            Self::Sffm => "Single-frequency FM: a carrier modulated by a sine signal.",
        }
    }

    /// The type of `waveform`.
    pub(crate) fn of(waveform: Option<&Waveform>) -> Self {
        match waveform {
            None => Self::None,
            Some(Waveform::Pulse { .. }) => Self::Pulse,
            Some(Waveform::Sine { .. }) => Self::Sine,
            Some(Waveform::Exp { .. }) => Self::Exp,
            Some(Waveform::Pwl { .. }) => Self::Pwl,
            Some(Waveform::Sffm { .. }) => Self::Sffm,
        }
    }

    fn params(self) -> &'static [Param] {
        match self {
            Self::None | Self::Pwl => &[],
            Self::Pulse => &PULSE,
            Self::Sine => &SINE,
            Self::Exp => &EXP,
            Self::Sffm => &SFFM,
        }
    }

    /// Parameter fields of this type for a source whose output is in
    /// `signal`. PWL points are edited as rows instead (see
    /// [`super::spec::pwl_fields`]).
    pub(crate) fn fields(self, signal: Unit) -> Vec<FieldSpec> {
        self.params()
            .iter()
            .enumerate()
            .map(|(index, p)| FieldSpec {
                id: FieldId::Wave(self, index),
                label: p.label,
                unit: p.unit.unwrap_or(signal),
                constraint: p.constraint,
                empty: p.empty,
            })
            .collect()
    }

    /// Initial parameter texts, in [`Self::fields`] order.
    pub(crate) fn seeds(self, signal: Unit) -> Vec<&'static str> {
        let level = if signal == Unit::Ampere { "1m" } else { "1" };
        self.params()
            .iter()
            .map(|p| match p.seed {
                Seed::Level => level,
                Seed::Text(text) => text,
            })
            .collect()
    }
}

/// Parameter values of a non-PWL waveform, in [`WaveKind::fields`] order.
/// `None` stands for an empty optional field.
pub(crate) fn parameters(waveform: &Waveform) -> Vec<Option<f64>> {
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
        } => [initial, pulsed, delay, rise, fall, width, period]
            .into_iter()
            .map(Some)
            .chain([cycles.map(f64::from)])
            .collect(),
        Waveform::Sine {
            offset,
            amplitude,
            frequency,
            delay,
            damping,
            phase,
            cycles,
        } => [offset, amplitude, frequency, delay, damping, phase]
            .into_iter()
            .map(Some)
            .chain([cycles])
            .collect(),
        Waveform::Exp {
            initial,
            pulsed,
            rise_delay,
            rise_tau,
            fall_delay,
            fall_tau,
        } => [initial, pulsed, rise_delay, rise_tau, fall_delay, fall_tau]
            .into_iter()
            .map(Some)
            .collect(),
        Waveform::Sffm {
            offset,
            amplitude,
            carrier,
            index,
            signal,
        } => [offset, amplitude, carrier, index, signal]
            .into_iter()
            .map(Some)
            .collect(),
        Waveform::Pwl { .. } => Vec::new(),
    }
}

/// Rules between parameters of `kind` that single fields cannot check, as
/// `(parameter index, message)`. `values` are in [`WaveKind::fields`] order;
/// rules involving an invalid (`None`) value are skipped.
pub(crate) fn relation_errors(kind: WaveKind, values: &[Option<f64>]) -> Vec<(usize, String)> {
    let v = |i: usize| values.get(i).copied().flatten();
    let mut errors = Vec::new();
    match kind {
        WaveKind::Pulse => {
            if let (Some(rise), Some(fall), Some(width), Some(period)) = (v(3), v(4), v(5), v(6)) {
                let active = rise + width + fall;
                if period > 0.0 && period < active {
                    let active = Unit::Second.format(active);
                    errors.push((
                        6,
                        format!("Must be at least rise + width + fall ({active})"),
                    ));
                }
            }
        }
        WaveKind::Exp => {
            if let (Some(rise_delay), Some(fall_delay)) = (v(2), v(4))
                && fall_delay < rise_delay
            {
                errors.push((4, "Must not be earlier than the rise delay".into()));
            }
        }
        WaveKind::None | WaveKind::Sine | WaveKind::Pwl | WaveKind::Sffm => {}
    }
    errors
}

/// Builds a non-PWL waveform of `kind` from validated parameter values in
/// [`WaveKind::fields`] order. Returns `None` for [`WaveKind::None`],
/// [`WaveKind::Pwl`] or a wrong number of values.
pub(crate) fn build(kind: WaveKind, values: &[Option<f64>]) -> Option<Waveform> {
    let v = |i: usize| values.get(i).copied().flatten().unwrap_or(0.0);
    let optional = |i: usize| values.get(i).copied().flatten();
    if values.len() != kind.params().len() {
        return None;
    }
    Some(match kind {
        WaveKind::None | WaveKind::Pwl => return None,
        WaveKind::Pulse => Waveform::Pulse {
            initial: v(0),
            pulsed: v(1),
            delay: v(2),
            rise: v(3),
            fall: v(4),
            width: v(5),
            period: v(6),
            // Validated as a whole number in 1..=u32::MAX.
            cycles: optional(7).map(|c| c as u32),
        },
        WaveKind::Sine => Waveform::Sine {
            offset: v(0),
            amplitude: v(1),
            frequency: v(2),
            delay: v(3),
            damping: v(4),
            phase: v(5),
            cycles: optional(6),
        },
        WaveKind::Exp => Waveform::Exp {
            initial: v(0),
            pulsed: v(1),
            rise_delay: v(2),
            rise_tau: v(3),
            fall_delay: v(4),
            fall_tau: v(5),
        },
        WaveKind::Sffm => Waveform::Sffm {
            offset: v(0),
            amplitude: v(1),
            carrier: v(2),
            index: v(3),
            signal: v(4),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Waveform> {
        vec![
            Waveform::Pulse {
                initial: -1.0,
                pulsed: 5.0,
                delay: 1e-6,
                rise: 2e-9,
                fall: 3e-9,
                width: 4e-6,
                period: 1e-5,
                cycles: Some(7),
            },
            Waveform::Sine {
                offset: 0.5,
                amplitude: 2.0,
                frequency: 50.0,
                delay: 0.01,
                damping: 3.0,
                phase: 90.0,
                cycles: Some(2.5),
            },
            Waveform::Exp {
                initial: 0.0,
                pulsed: 1.0,
                rise_delay: 1e-3,
                rise_tau: 2e-3,
                fall_delay: 5e-3,
                fall_tau: 1e-3,
            },
            Waveform::Sffm {
                offset: 0.0,
                amplitude: 1.0,
                carrier: 1e4,
                index: 3.0,
                signal: 1e3,
            },
        ]
    }

    #[test]
    fn parameters_round_trip_through_build() {
        for waveform in samples() {
            let kind = WaveKind::of(Some(&waveform));
            let values = parameters(&waveform);
            assert_eq!(values.len(), kind.fields(Unit::Volt).len(), "{kind:?}");
            assert_eq!(build(kind, &values), Some(waveform));
        }
    }

    #[test]
    fn unlimited_cycles_are_empty_parameters() {
        let pulse = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay: 0.0,
            rise: 0.0,
            fall: 0.0,
            width: 1.0,
            period: 2.0,
            cycles: None,
        };
        assert_eq!(parameters(&pulse)[7], None);
        assert_eq!(build(WaveKind::Pulse, &parameters(&pulse)), Some(pulse));
    }

    #[test]
    fn seeds_cover_every_field_and_scale_with_the_source_type() {
        for kind in WaveKind::ALL {
            assert_eq!(kind.seeds(Unit::Volt).len(), kind.fields(Unit::Volt).len());
        }
        assert_eq!(WaveKind::Pulse.seeds(Unit::Volt)[1], "1");
        assert_eq!(WaveKind::Pulse.seeds(Unit::Ampere)[1], "1m");
        assert_eq!(WaveKind::Sine.fields(Unit::Ampere)[1].unit, Unit::Ampere);
        assert_eq!(WaveKind::Sine.fields(Unit::Ampere)[2].unit, Unit::Hertz);
    }

    #[test]
    fn field_labels_follow_the_spice_parameter_order() {
        let labels = |kind: WaveKind| {
            kind.fields(Unit::Volt)
                .iter()
                .map(|f| f.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(WaveKind::Pulse),
            [
                "Initial value",
                "Pulsed value",
                "Delay",
                "Rise time",
                "Fall time",
                "Pulse width",
                "Period",
                "Cycles"
            ]
        );
        assert_eq!(labels(WaveKind::Sffm).len(), 5);
        assert!(labels(WaveKind::None).is_empty());
        assert!(labels(WaveKind::Pwl).is_empty());
    }

    #[test]
    fn relations_between_parameters_mirror_the_engine_rules() {
        let pulse = |period: f64| {
            let mut values = vec![
                Some(0.0),
                Some(1.0),
                Some(0.0),
                Some(1.0),
                Some(1.0),
                Some(2.0),
            ];
            values.extend([Some(period), None]);
            relation_errors(WaveKind::Pulse, &values)
        };
        assert_eq!(pulse(4.0), []);
        assert_eq!(pulse(0.0), [], "a period of 0 is a single pulse");
        assert_eq!(pulse(3.0)[0].0, 6);
        let exp = |rise_delay: f64, fall_delay: f64| {
            let values = [
                Some(0.0),
                Some(1.0),
                Some(rise_delay),
                Some(1.0),
                Some(fall_delay),
                Some(1.0),
            ];
            relation_errors(WaveKind::Exp, &values)
        };
        assert_eq!(exp(1.0, 1.0), []);
        assert_eq!(exp(2.0, 1.0)[0].0, 4);
        assert_eq!(exp(2.0, f64::NAN), [], "comparisons with NaN never fail");
        assert_eq!(relation_errors(WaveKind::Pulse, &[None; 8]), []);
    }

    #[test]
    fn build_rejects_mismatched_values() {
        assert_eq!(build(WaveKind::Sine, &[Some(1.0)]), None);
        assert_eq!(build(WaveKind::None, &[]), None);
    }
}
