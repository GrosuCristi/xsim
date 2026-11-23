//! Validation of the form while typing: per-field constraints, rules between
//! waveform fields, the designator and the model. GPUI-free.

use super::PartForm;
use crate::properties::pwl;
use crate::properties::spec::{FieldId, FieldSpec};
use crate::properties::waveform::{self, WaveKind};
use xsim_project::netlist::is_valid_part_name;

/// Validation state of one field.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldStatus {
    /// Accepted; carries the value in engineering notation, e.g. `"4.7 kΩ"`.
    Valid(String),
    /// Left empty on purpose; carries what that means, e.g. `"Unlimited"`.
    Empty(&'static str),
    /// Rejected, with the message shown under the field.
    Invalid(String),
}

/// A field and the reason it is rejected.
pub(crate) type FieldError = (FieldId, String);

/// Short enough for one line under a narrow source field (D20), as typing
/// passes through it (`-`, `1e`); the dialog's footer explains the SI
/// prefixes.
pub(super) const NOT_A_NUMBER: &str = "Not a number";

/// Why a designator with another character is rejected. Only ASCII letters
/// count, so "Rä1" is refused too and the message says so.
pub(super) const DESIGNATOR_CHARS: &str = "Use only A\u{2013}Z, a\u{2013}z, 0\u{2013}9 and _";

impl PartForm {
    /// The validated value of a field (`None` when empty and optional).
    fn check(&self, spec: &FieldSpec) -> Result<Option<f64>, String> {
        if self.text(spec.id).trim().is_empty() {
            return match spec.empty {
                Some(_) => Ok(None),
                None => Err("Enter a value".into()),
            };
        }
        let value = self.parsed(spec.id).ok_or(NOT_A_NUMBER)?;
        match spec.constraint.violation(value) {
            Some(message) => Err(message.into()),
            None => Ok(Some(value)),
        }
    }

    pub(super) fn field_error(&self, spec: &FieldSpec) -> Option<String> {
        if let Err(message) = self.check(spec) {
            return Some(message);
        }
        self.relation_errors()
            .into_iter()
            .find(|(id, _)| *id == spec.id)
            .map(|(_, message)| message)
    }

    /// Errors of rules between fields (PULSE period, EXP delays, PWL order).
    fn relation_errors(&self) -> Vec<FieldError> {
        match self.wave {
            WaveKind::Pwl => self.pwl_order_errors(),
            kind => {
                let values: Vec<_> = self
                    .wave_fields()
                    .iter()
                    .map(|spec| self.check(spec).ok().flatten())
                    .collect();
                waveform::relation_errors(kind, &values)
                    .into_iter()
                    .map(|(index, message)| (FieldId::Wave(kind, index), message))
                    .collect()
            }
        }
    }

    /// Why the repeat option cannot be used, if it cannot.
    pub(crate) fn repeat_error(&self) -> Option<&'static str> {
        (self.is_source()
            && self.wave == WaveKind::Pwl
            && self.pwl.repeat
            && self.pwl.ids().len() < 2)
            .then_some("Repeating needs at least two points")
    }

    fn pwl_order_errors(&self) -> Vec<FieldError> {
        let points: Vec<_> = self
            .pwl
            .ids()
            .iter()
            .filter_map(|&row| {
                let time = self.parsed(FieldId::PwlTime(row))?;
                let value = self.parsed(FieldId::PwlValue(row))?;
                Some((row, (time, value)))
            })
            .collect();
        pwl::order_errors(&points)
    }

    /// Validation state of a numeric field.
    pub(crate) fn status(&self, spec: &FieldSpec) -> FieldStatus {
        if let Some(message) = self.field_error(spec) {
            return FieldStatus::Invalid(message);
        }
        match self.check(spec) {
            Ok(Some(value)) => FieldStatus::Valid(spec.unit.format(value)),
            _ => FieldStatus::Empty(spec.empty.unwrap_or_default()),
        }
    }

    /// Validation state of the designator field.
    pub(crate) fn designator_status(&self) -> FieldStatus {
        let name = self.text(FieldId::Designator).trim();
        let prefix = self.kind.prefix();
        if name.is_empty() {
            FieldStatus::Invalid(format!("Enter a designator, e.g. {prefix}1"))
        } else if name.chars().any(char::is_whitespace) {
            FieldStatus::Invalid("Spaces are not allowed".into())
        } else if !is_valid_part_name(name) {
            // The netlist's rule (INT-4): any other character could break
            // a probe expression such as `I(R1)`.
            FieldStatus::Invalid(DESIGNATOR_CHARS.into())
        } else if self.taken.contains(name) {
            FieldStatus::Invalid(format!("{name} is already used by another part"))
        } else {
            FieldStatus::Valid(String::new())
        }
    }

    /// Every rejected field, designator first.
    pub(crate) fn errors(&self) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if let FieldStatus::Invalid(message) = self.designator_status() {
            errors.push((FieldId::Designator, message));
        }
        for spec in self.numeric_fields() {
            if let Some(message) = self.field_error(&spec) {
                errors.push((spec.id, message));
            }
        }
        if let Some(message) = self.repeat_error() {
            errors.push((FieldId::PwlRepeat, message.into()));
        }
        if self.has_model() && self.model.trim().is_empty() {
            errors.push((FieldId::Model, "Choose a model".into()));
        }
        errors
    }
}
