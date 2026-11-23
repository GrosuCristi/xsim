//! Field descriptions of the properties form: identifiers, labels, units and
//! what each field accepts. GPUI-free.

use super::waveform::WaveKind;
use xsim_project::PartKind;
use xsim_spice_engine::units::format_value;

/// Identifies one text field of the properties form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FieldId {
    Designator,
    /// Resistance, capacitance or inductance.
    Value,
    /// Capacitor initial voltage or inductor initial current.
    Initial,
    Dc,
    AcMagnitude,
    AcPhase,
    /// Parameter `index` of a transient waveform type.
    Wave(WaveKind, usize),
    /// Time of the PWL row with this row id.
    PwlTime(u64),
    /// Value of the PWL row with this row id.
    PwlValue(u64),
    /// The PWL repeat option (reported when it needs more points).
    PwlRepeat,
    /// Semiconductor model name.
    Model,
    /// A switch's on resistance (RON).
    OnResistance,
    /// A switch's off resistance (ROFF).
    OffResistance,
    /// A switch's threshold (VT or IT).
    Threshold,
    /// A switch's hysteresis (VH or IH).
    Hysteresis,
}

/// What a numeric field accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Constraint {
    /// Any finite number.
    Finite,
    /// A finite number greater than zero.
    Positive,
    /// A finite number of zero or more.
    NonNegative,
    /// A whole number of one or more.
    Count,
}

impl Constraint {
    /// Why `value` is rejected, or `None` when it is accepted.
    pub(crate) fn violation(self, value: f64) -> Option<&'static str> {
        if !value.is_finite() {
            return Some("Enter a finite number");
        }
        match self {
            Self::Finite => None,
            Self::Positive => (value <= 0.0).then_some("Must be greater than 0"),
            Self::NonNegative => (value < 0.0).then_some("Must be 0 or more"),
            Self::Count => (value < 1.0 || value.fract() != 0.0 || value > f64::from(u32::MAX))
                .then_some("Enter a whole number of 1 or more"),
        }
    }
}

/// Physical unit of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    Ohm,
    Farad,
    Henry,
    Volt,
    Ampere,
    Second,
    Hertz,
    Degree,
    PerSecond,
    /// Dimensionless (modulation index, cycle counts).
    None,
}

impl Unit {
    /// Symbol shown after the input, e.g. `"Ω"`; empty when dimensionless.
    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Self::Ohm => "Ω",
            Self::Farad => "F",
            Self::Henry => "H",
            Self::Volt => "V",
            Self::Ampere => "A",
            Self::Second => "s",
            Self::Hertz => "Hz",
            Self::Degree => "°",
            Self::PerSecond => "1/s",
            Self::None => "",
        }
    }

    /// Engineering text of `value` in this unit, e.g. `"4.7 kΩ"` or `"90°"`.
    pub(crate) fn format(self, value: f64) -> String {
        match self {
            Self::Degree => format!("{}°", format_value(value, "")),
            Self::PerSecond => format!("{} 1/s", format_value(value, "")),
            unit => format_value(value, unit.symbol()),
        }
    }
}

/// Description of one numeric field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FieldSpec {
    pub id: FieldId,
    pub label: &'static str,
    pub unit: Unit,
    pub constraint: Constraint,
    /// What an empty field means (e.g. `"Unlimited"`), or `None` when the
    /// field is required.
    pub empty: Option<&'static str>,
}

impl FieldSpec {
    /// A required field.
    pub(crate) const fn required(
        id: FieldId,
        label: &'static str,
        unit: Unit,
        constraint: Constraint,
    ) -> Self {
        Self {
            id,
            label,
            unit,
            constraint,
            empty: None,
        }
    }

    /// A field that may be left empty, meaning `empty`.
    pub(crate) const fn optional(
        id: FieldId,
        label: &'static str,
        unit: Unit,
        constraint: Constraint,
        empty: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            unit,
            constraint,
            empty: Some(empty),
        }
    }
}

/// The "Value" fields of a resistor, capacitor or inductor; empty for other
/// kinds.
pub(crate) fn passive_fields(kind: PartKind) -> Vec<FieldSpec> {
    use Constraint::{Finite, Positive};
    match kind {
        PartKind::Resistor => vec![FieldSpec::required(
            FieldId::Value,
            "Resistance",
            Unit::Ohm,
            Positive,
        )],
        PartKind::Capacitor => vec![
            FieldSpec::required(FieldId::Value, "Capacitance", Unit::Farad, Positive),
            FieldSpec::optional(
                FieldId::Initial,
                "Initial voltage",
                Unit::Volt,
                Finite,
                "Not set",
            ),
        ],
        PartKind::Inductor => vec![
            FieldSpec::required(FieldId::Value, "Inductance", Unit::Henry, Positive),
            FieldSpec::optional(
                FieldId::Initial,
                "Initial current",
                Unit::Ampere,
                Finite,
                "Not set",
            ),
        ],
        _ => Vec::new(),
    }
}

/// What the initial condition of a capacitor or inductor does, shown under
/// its fields; `None` for other kinds. LTspice semantics (D16): it always
/// applies, to the starting operating point as well as with Start from
/// rest.
pub(crate) fn initial_condition_hint(kind: PartKind) -> Option<&'static str> {
    match kind {
        PartKind::Capacitor => Some(
            "An initial voltage applies to every transient: the starting operating point \
             is solved with the capacitor at this voltage, and Start from rest begins \
             from it too.",
        ),
        PartKind::Inductor => Some(
            "An initial current applies to every transient: the starting operating point \
             is solved with this current in the inductor, and Start from rest begins \
             from it too.",
        ),
        _ => None,
    }
}

/// The model fields of a voltage or current switch (D17); empty for other
/// kinds. They accept what the engine's model check does: positive, finite
/// resistances and finite thresholds and hysteresis, negative hysteresis
/// included (the smooth mode).
pub(crate) fn switch_fields(kind: PartKind) -> Vec<FieldSpec> {
    use Constraint::{Finite, Positive};
    let (threshold, hysteresis, unit) = match kind {
        PartKind::VoltageSwitch => ("Threshold (VT)", "Hysteresis (VH)", Unit::Volt),
        PartKind::CurrentSwitch => ("Threshold (IT)", "Hysteresis (IH)", Unit::Ampere),
        _ => return Vec::new(),
    };
    vec![
        FieldSpec::required(
            FieldId::OnResistance,
            "On resistance (RON)",
            Unit::Ohm,
            Positive,
        ),
        FieldSpec::required(
            FieldId::OffResistance,
            "Off resistance (ROFF)",
            Unit::Ohm,
            Positive,
        ),
        FieldSpec::required(FieldId::Threshold, threshold, unit, Finite),
        FieldSpec::required(FieldId::Hysteresis, hysteresis, unit, Finite),
    ]
}

/// How a switch's threshold and hysteresis act, shown under its fields;
/// `None` for other kinds. LTspice semantics, as the engine implements
/// them.
pub(crate) fn switch_hint(kind: PartKind) -> Option<&'static str> {
    match kind {
        PartKind::VoltageSwitch => Some(
            "On (RON) once V(C+, C\u{2212}) exceeds VT + VH, off (ROFF) once it falls below \
             VT \u{2212} VH; in between it keeps its state, and it starts off. A negative VH \
             switches smoothly between VT \u{2212} |VH| and VT + |VH|.",
        ),
        PartKind::CurrentSwitch => Some(
            "On (RON) once the control current exceeds IT + IH, off (ROFF) once it falls \
             below IT \u{2212} IH; in between it keeps its state, and it starts off. A \
             negative IH switches smoothly between IT \u{2212} |IH| and IT + |IH|.",
        ),
        _ => None,
    }
}

/// DC and small-signal AC fields of a source whose output is in `signal`
/// (volts or amperes).
pub(crate) fn source_fields(signal: Unit) -> Vec<FieldSpec> {
    use Constraint::{Finite, NonNegative};
    vec![
        FieldSpec::required(FieldId::Dc, "DC value", signal, Finite),
        FieldSpec::optional(
            FieldId::AcMagnitude,
            "AC magnitude",
            signal,
            NonNegative,
            "None",
        ),
        FieldSpec::optional(FieldId::AcPhase, "AC phase", Unit::Degree, Finite, "0°"),
    ]
}

/// PWL row fields for row `row`.
pub(crate) fn pwl_fields(row: u64, signal: Unit) -> [FieldSpec; 2] {
    [
        FieldSpec::required(
            FieldId::PwlTime(row),
            "Time",
            Unit::Second,
            Constraint::NonNegative,
        ),
        FieldSpec::required(FieldId::PwlValue(row), "Value", signal, Constraint::Finite),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_accept_and_reject_the_documented_ranges() {
        assert_eq!(Constraint::Finite.violation(-3.0), None);
        assert!(Constraint::Finite.violation(f64::NAN).is_some());
        assert!(Constraint::Positive.violation(0.0).is_some());
        assert_eq!(Constraint::Positive.violation(1e-15), None);
        assert_eq!(Constraint::NonNegative.violation(0.0), None);
        assert!(Constraint::NonNegative.violation(-1e-9).is_some());
        assert_eq!(Constraint::Count.violation(3.0), None);
        assert!(Constraint::Count.violation(0.0).is_some());
        assert!(Constraint::Count.violation(2.5).is_some());
        assert!(Constraint::Count.violation(1e12).is_some());
    }

    #[test]
    fn passive_tables_match_the_part_kind() {
        let labels = |kind| {
            passive_fields(kind)
                .iter()
                .map(|f| f.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(PartKind::Resistor), ["Resistance"]);
        assert_eq!(
            labels(PartKind::Capacitor),
            ["Capacitance", "Initial voltage"]
        );
        assert_eq!(
            labels(PartKind::Inductor),
            ["Inductance", "Initial current"]
        );
        assert!(labels(PartKind::Diode).is_empty());
        assert!(passive_fields(PartKind::Capacitor)[1].empty.is_some());
    }

    /// D16: initial conditions apply to the operating-point start too.
    #[test]
    fn initial_condition_hints_name_both_ways_a_transient_starts() {
        for kind in [PartKind::Capacitor, PartKind::Inductor] {
            let hint = initial_condition_hint(kind).unwrap_or_default();
            assert!(
                hint.contains("starting operating point"),
                "{kind:?}: {hint}"
            );
            assert!(hint.contains("Start from rest"), "{kind:?}: {hint}");
            assert!(!hint.contains("  "), "{kind:?}: {hint}");
        }
        assert_eq!(initial_condition_hint(PartKind::Resistor), None);
        assert_eq!(initial_condition_hint(PartKind::VoltageSource), None);
    }

    #[test]
    fn source_fields_use_the_signal_unit() {
        let fields = source_fields(Unit::Ampere);
        assert_eq!(fields[0].unit, Unit::Ampere);
        assert_eq!(fields[1].unit, Unit::Ampere);
        assert_eq!(fields[2].unit, Unit::Degree);
        assert!(fields[0].empty.is_none());
    }
}
