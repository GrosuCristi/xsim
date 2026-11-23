//! The analysis settings form (Q75, Q76) without any UI: the text of every
//! input box, parsing with SI suffixes and validation with messages written
//! for the user.
//!
//! Only the selected analysis and the solver options must be valid to
//! apply. Boxes of the other analyses keep their previous value when their
//! text does not parse; their errors are shown inline as the user types.

use std::collections::BTreeMap;
use xsim_project::{AnalysisChoice, SimulationSetup};
use xsim_spice_engine::{
    InitialState, IntegrationMethod, SolverKind, SweepScale,
    units::{format_value_digits, parse_value},
};

/// Significant digits used to show stored values, enough to round-trip
/// anything a person types.
const DISPLAY_DIGITS: usize = 12;

/// Largest number of DC sweep points accepted.
const MAX_DC_POINTS: f64 = 1e6;

/// One input box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    DcSource,
    DcStart,
    DcStop,
    DcStep,
    AcPoints,
    AcStart,
    AcStop,
    TranStop,
    TranStep,
    TranStartSave,
    TranMaxStep,
    Reltol,
    Abstol,
    Vntol,
    Gmin,
    Temperature,
}

/// The text boxes of each analysis, in display order. The DC source is a
/// choice, not a text box.
pub fn text_fields(choice: AnalysisChoice) -> &'static [Field] {
    match choice {
        AnalysisChoice::OperatingPoint => &[],
        AnalysisChoice::DcSweep => &[Field::DcStart, Field::DcStop, Field::DcStep],
        AnalysisChoice::Ac => &[Field::AcPoints, Field::AcStart, Field::AcStop],
        AnalysisChoice::Transient => &[
            Field::TranStop,
            Field::TranStep,
            Field::TranStartSave,
            Field::TranMaxStep,
        ],
    }
}

/// The solver option boxes of the Advanced section.
pub const OPTION_FIELDS: [Field; 5] = [
    Field::Reltol,
    Field::Abstol,
    Field::Vntol,
    Field::Gmin,
    Field::Temperature,
];

/// Every text box, analyses first.
pub const ALL_TEXT_FIELDS: [Field; 15] = [
    Field::DcStart,
    Field::DcStop,
    Field::DcStep,
    Field::AcPoints,
    Field::AcStart,
    Field::AcStop,
    Field::TranStop,
    Field::TranStep,
    Field::TranStartSave,
    Field::TranMaxStep,
    Field::Reltol,
    Field::Abstol,
    Field::Vntol,
    Field::Gmin,
    Field::Temperature,
];

impl Field {
    /// Label next to the box.
    pub fn label(self) -> &'static str {
        match self {
            Self::DcSource => "Source",
            Self::DcStart | Self::AcStart => "Start",
            Self::DcStop | Self::AcStop => "Stop",
            Self::DcStep => "Step",
            Self::AcPoints => "Points",
            Self::TranStop => "Stop time",
            Self::TranStep => "Output step",
            Self::TranStartSave => "Save from",
            Self::TranMaxStep => "Max step",
            Self::Reltol => "Relative tolerance",
            Self::Abstol => "Current tolerance",
            Self::Vntol => "Voltage tolerance",
            Self::Gmin => "Gmin",
            Self::Temperature => "Temperature",
        }
    }

    /// Unit shown after the box.
    pub fn unit(self) -> &'static str {
        match self {
            Self::AcStart | Self::AcStop => "Hz",
            Self::TranStop | Self::TranStep | Self::TranStartSave | Self::TranMaxStep => "s",
            Self::Abstol => "A",
            Self::Vntol => "V",
            Self::Gmin => "S",
            Self::Temperature => "°C",
            Self::AcPoints | Self::Reltol | Self::DcSource => "",
            Self::DcStart | Self::DcStop | Self::DcStep => "V/A",
        }
    }

    /// Placeholder text.
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::TranMaxStep => "automatic",
            Self::AcPoints => "e.g. 20",
            Self::Temperature => "e.g. 27",
            _ => "e.g. 1m, 4.7k, 1e-6",
        }
    }
}

/// Messages keyed by the box they belong to.
pub type FieldErrors = BTreeMap<Field, String>;

/// Display labels of the integration methods, in menu order.
pub const METHODS: [(IntegrationMethod, &str); 4] = [
    (IntegrationMethod::Trapezoidal, "Trapezoidal"),
    (
        IntegrationMethod::ModifiedTrapezoidal,
        "Modified trapezoidal (TR-BDF2)",
    ),
    (IntegrationMethod::Gear2, "Gear (BDF2)"),
    (IntegrationMethod::BackwardEuler, "Backward Euler"),
];

/// Display labels of the linear solvers, in menu order.
pub const SOLVERS: [(SolverKind, &str); 3] = [
    (SolverKind::Auto, "Automatic"),
    (SolverKind::Dense, "Dense LU"),
    (SolverKind::Sparse, "Sparse LU"),
];

/// Display labels of the AC sweep spacings, in menu order.
pub const SCALES: [(SweepScale, &str); 3] = [
    (SweepScale::Decade, "Points per decade"),
    (SweepScale::Octave, "Points per octave"),
    (SweepScale::Linear, "Linear, total points"),
];

/// Label of `value` in a `(value, label)` table.
pub fn label_of<T: PartialEq + Copy>(table: &[(T, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(v, _)| *v == value)
        .map_or("", |(_, label)| label)
}

/// The editable state of the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisForm {
    pub selected: AnalysisChoice,
    pub dc_source: String,
    pub ac_scale: SweepScale,
    /// "Start from rest" (D5): `InitialState::ZeroState`.
    pub start_from_rest: bool,
    pub method: IntegrationMethod,
    pub solver: SolverKind,
    text: BTreeMap<Field, String>,
    /// Text the boxes started with; unchanged boxes keep the exact stored
    /// value instead of a re-parsed rounding of it.
    initial: BTreeMap<Field, String>,
}

impl AnalysisForm {
    /// The form showing `setup`.
    pub fn from_setup(setup: &SimulationSetup) -> Self {
        let text: BTreeMap<Field, String> = ALL_TEXT_FIELDS
            .iter()
            .map(|&field| (field, stored_text(setup, field)))
            .collect();
        Self {
            selected: setup.selected,
            dc_source: setup.dc.source.clone(),
            ac_scale: setup.ac.scale,
            start_from_rest: setup.transient.initial_state == InitialState::ZeroState,
            method: setup.options.method,
            solver: setup.options.solver,
            initial: text.clone(),
            text,
        }
    }

    /// Current text of a box.
    pub fn text(&self, field: Field) -> &str {
        self.text.get(&field).map_or("", String::as_str)
    }

    /// Replaces the text of a box (as the user types).
    pub fn set_text(&mut self, field: Field, text: impl Into<String>) {
        self.text.insert(field, text.into());
    }

    /// The message for `field` alone (syntax and range), or `None` when it
    /// is fine. Used for inline validation while typing.
    pub fn field_error(&self, field: Field) -> Option<String> {
        self.number(field).err()
    }

    /// Checks the selected analysis and the options against `sources` (the
    /// schematic's independent sources) and returns the new setup based on
    /// `base`.
    pub fn validate(
        &self,
        base: &SimulationSetup,
        sources: &[String],
    ) -> Result<SimulationSetup, FieldErrors> {
        let mut errors = FieldErrors::new();
        let mut setup = base.clone();
        setup.selected = self.selected;
        setup.ac.scale = self.ac_scale;
        setup.transient.initial_state = if self.start_from_rest {
            InitialState::ZeroState
        } else {
            InitialState::OperatingPoint
        };
        setup.options.method = self.method;
        setup.options.solver = self.solver;
        let strict: Vec<Field> = text_fields(self.selected)
            .iter()
            .chain(OPTION_FIELDS.iter())
            .copied()
            .collect();
        for field in ALL_TEXT_FIELDS {
            match self.number(field) {
                Ok(value) => self.assign(&mut setup, field, value),
                Err(message) if strict.contains(&field) => {
                    errors.insert(field, message);
                }
                Err(_) => {}
            }
        }
        self.check_source(&mut setup, sources, &mut errors);
        check_relations(&setup, self.selected, &mut errors);
        if errors.is_empty() {
            Ok(setup)
        } else {
            Err(errors)
        }
    }

    fn check_source(
        &self,
        setup: &mut SimulationSetup,
        sources: &[String],
        errors: &mut FieldErrors,
    ) {
        if self.selected != AnalysisChoice::DcSweep {
            return;
        }
        if sources.is_empty() {
            errors.insert(
                Field::DcSource,
                "Add a voltage or current source to sweep".into(),
            );
        } else if !sources.contains(&self.dc_source) {
            errors.insert(
                Field::DcSource,
                format!("{} is not in the schematic", self.dc_source),
            );
        } else {
            setup.dc.source = self.dc_source.clone();
        }
    }

    /// Parses one box. `Ok(None)` is an empty optional box.
    fn number(&self, field: Field) -> Result<Option<f64>, String> {
        let text = self.text(field).trim();
        if text.is_empty() {
            return if field == Field::TranMaxStep {
                Ok(None)
            } else {
                Err("Enter a value".into())
            };
        }
        let value = if field == Field::AcPoints {
            parse_count(text)?
        } else {
            parse_value(text).map_err(|e| e.to_string())?
        };
        check_range(field, value)?;
        Ok(Some(value))
    }

    /// Stores a parsed box in `setup`, unless its text is unchanged.
    fn assign(&self, setup: &mut SimulationSetup, field: Field, value: Option<f64>) {
        if self.initial.get(&field).map(String::as_str) == Some(self.text(field)) {
            return;
        }
        let v = value.unwrap_or_default();
        match field {
            Field::DcStart => setup.dc.start = v,
            Field::DcStop => setup.dc.stop = v,
            Field::DcStep => setup.dc.step = v,
            // Counts are validated as whole numbers, so the cast is exact.
            Field::AcPoints => setup.ac.points = v as usize,
            Field::AcStart => setup.ac.start = v,
            Field::AcStop => setup.ac.stop = v,
            Field::TranStop => setup.transient.stop = v,
            Field::TranStep => setup.transient.step = v,
            Field::TranStartSave => setup.transient.start_save = v,
            Field::TranMaxStep => setup.transient.max_step = value,
            Field::Reltol => setup.options.reltol = v,
            Field::Abstol => setup.options.abstol = v,
            Field::Vntol => setup.options.vntol = v,
            Field::Gmin => setup.options.gmin = v,
            Field::Temperature => setup.options.temperature = v,
            Field::DcSource => {}
        }
    }
}

/// The text a box shows for the stored value.
fn stored_text(setup: &SimulationSetup, field: Field) -> String {
    let value = match field {
        Field::DcStart => setup.dc.start,
        Field::DcStop => setup.dc.stop,
        Field::DcStep => setup.dc.step,
        Field::AcPoints => return setup.ac.points.to_string(),
        Field::AcStart => setup.ac.start,
        Field::AcStop => setup.ac.stop,
        Field::TranStop => setup.transient.stop,
        Field::TranStep => setup.transient.step,
        Field::TranStartSave => setup.transient.start_save,
        Field::TranMaxStep => match setup.transient.max_step {
            Some(v) => v,
            None => return String::new(),
        },
        Field::Reltol => setup.options.reltol,
        Field::Abstol => setup.options.abstol,
        Field::Vntol => setup.options.vntol,
        Field::Gmin => setup.options.gmin,
        Field::Temperature => return trim_number(setup.options.temperature),
        Field::DcSource => return setup.dc.source.clone(),
    };
    format_value_digits(value, "", DISPLAY_DIGITS)
}

/// A plain number without SI prefix, e.g. `27` or `-40.5`.
fn trim_number(value: f64) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
}

fn parse_count(text: &str) -> Result<f64, String> {
    text.parse::<usize>()
        .map(|n| n as f64)
        .map_err(|_| format!("\"{text}\" is not a whole number of points"))
}

/// Range rules that need only the box itself.
fn check_range(field: Field, value: f64) -> Result<(), String> {
    let positive = |what: &str| {
        if value > 0.0 {
            Ok(())
        } else {
            Err(format!("{what} must be greater than zero"))
        }
    };
    match field {
        Field::TranStop => positive("The stop time"),
        Field::TranStep => positive("The output step"),
        Field::TranMaxStep => positive("The maximum step"),
        Field::TranStartSave if value < 0.0 => Err("Saving cannot start before 0 s".into()),
        Field::AcPoints if value < 1.0 => Err("Use at least 1 point".into()),
        Field::AcStart | Field::AcStop => positive("Frequencies"),
        Field::DcStep if value == 0.0 => Err("The step cannot be zero".into()),
        Field::Reltol | Field::Abstol | Field::Vntol => positive("Tolerances"),
        Field::Gmin if value < 0.0 => Err("Gmin cannot be negative".into()),
        Field::Temperature if value < -273.15 => {
            Err("The temperature must be above absolute zero (−273.15 °C)".into())
        }
        _ => Ok(()),
    }
}

/// Rules that relate several boxes of the selected analysis.
fn check_relations(setup: &SimulationSetup, selected: AnalysisChoice, errors: &mut FieldErrors) {
    let mut fail = |field: Field, message: &str| {
        errors.entry(field).or_insert_with(|| message.to_string());
    };
    match selected {
        AnalysisChoice::Transient => {
            let t = &setup.transient;
            if t.step > t.stop {
                fail(
                    Field::TranStep,
                    "The output step cannot exceed the stop time",
                );
            }
            if t.start_save >= t.stop {
                fail(
                    Field::TranStartSave,
                    "Saving must start before the stop time",
                );
            }
            if t.max_step.is_some_and(|m| m > t.stop) {
                fail(
                    Field::TranMaxStep,
                    "The maximum step cannot exceed the stop time",
                );
            }
        }
        AnalysisChoice::Ac => {
            if setup.ac.stop < setup.ac.start {
                fail(
                    Field::AcStop,
                    "The stop frequency must be at least the start",
                );
            }
        }
        AnalysisChoice::DcSweep => {
            let dc = &setup.dc;
            let points = (dc.stop - dc.start) / dc.step;
            if points < 0.0 {
                fail(Field::DcStep, "The step must point from start towards stop");
            } else if points > MAX_DC_POINTS {
                fail(
                    Field::DcStep,
                    "The step is too small: over a million points",
                );
            }
        }
        AnalysisChoice::OperatingPoint => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Vec<String> {
        vec!["V1".into(), "I1".into()]
    }

    #[test]
    fn an_untouched_form_reproduces_the_setup() {
        let setup = SimulationSetup::default();
        let form = AnalysisForm::from_setup(&setup);
        assert_eq!(form.text(Field::TranStop), "5m");
        assert_eq!(form.text(Field::TranMaxStep), "");
        assert_eq!(form.text(Field::Temperature), "27");
        assert_eq!(form.validate(&setup, &sources()), Ok(setup));
    }

    #[test]
    fn values_parse_with_si_suffixes() {
        let setup = SimulationSetup::default();
        let mut form = AnalysisForm::from_setup(&setup);
        form.set_text(Field::TranStop, "20m");
        form.set_text(Field::TranStep, "1u");
        form.set_text(Field::TranMaxStep, "10u");
        form.start_from_rest = true;
        form.method = IntegrationMethod::Gear2;
        let applied = form.validate(&setup, &sources()).expect("valid");
        assert_eq!(applied.transient.stop, 20e-3);
        assert_eq!(applied.transient.step, 1e-6);
        assert_eq!(applied.transient.max_step, Some(10e-6));
        assert_eq!(applied.transient.initial_state, InitialState::ZeroState);
        assert_eq!(applied.options.method, IntegrationMethod::Gear2);
    }

    #[test]
    fn bad_boxes_of_the_selected_analysis_block_apply() {
        let setup = SimulationSetup::default();
        let mut form = AnalysisForm::from_setup(&setup);
        form.set_text(Field::TranStop, "fast");
        form.set_text(Field::TranStep, "0");
        form.set_text(Field::Reltol, "");
        let errors = form.validate(&setup, &sources()).expect_err("invalid");
        assert!(errors[&Field::TranStop].contains("not a valid value"));
        assert!(errors[&Field::TranStep].contains("greater than zero"));
        assert_eq!(errors[&Field::Reltol], "Enter a value");
        assert_eq!(
            form.field_error(Field::TranStep).as_deref(),
            Some("The output step must be greater than zero")
        );
        assert_eq!(form.field_error(Field::TranStartSave), None);
    }

    #[test]
    fn other_analyses_keep_their_value_when_invalid() {
        let setup = SimulationSetup::default();
        let mut form = AnalysisForm::from_setup(&setup);
        form.set_text(Field::AcStart, "oops");
        form.set_text(Field::DcStop, "10");
        let applied = form.validate(&setup, &sources()).expect("transient valid");
        assert_eq!(applied.ac.start, setup.ac.start);
        assert_eq!(applied.dc.stop, 10.0);
    }

    #[test]
    fn relations_between_boxes_are_checked() {
        let setup = SimulationSetup::default();
        let mut form = AnalysisForm::from_setup(&setup);
        form.set_text(Field::TranStep, "1");
        let errors = form.validate(&setup, &sources()).expect_err("step > stop");
        assert!(errors.contains_key(&Field::TranStep));

        form = AnalysisForm::from_setup(&setup);
        form.selected = AnalysisChoice::Ac;
        form.set_text(Field::AcStop, "0.5");
        let errors = form.validate(&setup, &sources()).expect_err("stop < start");
        assert!(errors.contains_key(&Field::AcStop));

        form = AnalysisForm::from_setup(&setup);
        form.selected = AnalysisChoice::DcSweep;
        form.set_text(Field::DcStep, "-1");
        let errors = form.validate(&setup, &sources()).expect_err("wrong way");
        assert!(errors.contains_key(&Field::DcStep));
    }

    #[test]
    fn the_dc_source_must_exist() {
        let setup = SimulationSetup::default();
        let mut form = AnalysisForm::from_setup(&setup);
        form.selected = AnalysisChoice::DcSweep;
        assert!(form.validate(&setup, &[]).is_err());
        form.dc_source = "V9".into();
        let errors = form.validate(&setup, &sources()).expect_err("unknown");
        assert_eq!(errors[&Field::DcSource], "V9 is not in the schematic");
        form.dc_source = "I1".into();
        let applied = form.validate(&setup, &sources()).expect("valid");
        assert_eq!(applied.dc.source, "I1");
        assert_eq!(applied.selected, AnalysisChoice::DcSweep);
    }

    #[test]
    fn points_are_whole_numbers() {
        let setup = SimulationSetup::default();
        let mut form = AnalysisForm::from_setup(&setup);
        form.selected = AnalysisChoice::Ac;
        form.set_text(Field::AcPoints, "2.5");
        assert!(form.validate(&setup, &sources()).is_err());
        form.set_text(Field::AcPoints, "50");
        form.ac_scale = SweepScale::Octave;
        let applied = form.validate(&setup, &sources()).expect("valid");
        assert_eq!(
            (applied.ac.points, applied.ac.scale),
            (50, SweepScale::Octave)
        );
    }

    #[test]
    fn labels_are_looked_up() {
        assert_eq!(label_of(&SOLVERS, SolverKind::Dense), "Dense LU");
        assert_eq!(Field::TranStop.unit(), "s");
        assert_eq!(text_fields(AnalysisChoice::OperatingPoint).len(), 0);
    }
}
