//! Conversion of a validated form back to a [`Part`]. GPUI-free.

use super::{FieldError, PartForm};
use crate::properties::spec::FieldId;
use crate::properties::waveform::{self, WaveKind};
use xsim_project::{Part, PartKind};
use xsim_spice_engine::{CurrentSwitchModel, Polarity, Source, SwitchModel, Waveform};

impl PartForm {
    /// The part described by the form, or every rejected field.
    pub(crate) fn to_part(&self) -> Result<Part, Vec<FieldError>> {
        let errors = self.errors();
        if errors.is_empty() {
            Ok(self.build_part())
        } else {
            Err(errors)
        }
    }

    /// The designator and part to write to the schematic, `None` when they
    /// equal what was last applied, or every rejected field.
    pub(crate) fn pending_edit(&self) -> Result<Option<(String, Part)>, Vec<FieldError>> {
        let part = self.to_part()?;
        let name = self.text(FieldId::Designator).trim().to_string();
        let unchanged = self.applied.0 == name && self.applied.1 == part;
        Ok((!unchanged).then_some((name, part)))
    }

    /// The designator last written to the schematic (or loaded).
    pub(crate) fn applied_name(&self) -> &str {
        &self.applied.0
    }

    /// Records that `name` and `part` were written to the schematic.
    pub(crate) fn mark_applied(&mut self, name: String, part: Part) {
        self.applied = (name, part);
    }

    /// The transient waveform when every waveform field is valid.
    pub(crate) fn waveform(&self) -> Option<Waveform> {
        let fields = self.wave_fields();
        let pwl = self.pwl_fields();
        let valid = self.repeat_error().is_none()
            && fields
                .iter()
                .chain(pwl.iter().flatten())
                .all(|spec| self.field_error(spec).is_none());
        if valid { self.build_waveform() } else { None }
    }

    pub(super) fn build_part(&self) -> Part {
        let value = |id| self.parsed(id).unwrap_or(0.0);
        let model = self.model.trim().to_string();
        match self.kind {
            PartKind::Resistor => Part::Resistor {
                resistance: value(FieldId::Value),
            },
            PartKind::Capacitor => Part::Capacitor {
                capacitance: value(FieldId::Value),
                initial_voltage: self.parsed(FieldId::Initial),
            },
            PartKind::Inductor => Part::Inductor {
                inductance: value(FieldId::Value),
                initial_current: self.parsed(FieldId::Initial),
            },
            PartKind::VoltageSource => Part::VoltageSource {
                source: self.build_source(),
            },
            PartKind::CurrentSource => Part::CurrentSource {
                source: self.build_source(),
            },
            PartKind::Diode => Part::Diode { model },
            PartKind::NpnBjt => Part::Bjt {
                polarity: Polarity::N,
                model,
            },
            PartKind::PnpBjt => Part::Bjt {
                polarity: Polarity::P,
                model,
            },
            PartKind::NJfet => Part::Jfet {
                polarity: Polarity::N,
                model,
            },
            PartKind::PJfet => Part::Jfet {
                polarity: Polarity::P,
                model,
            },
            PartKind::VoltageSwitch => Part::VoltageSwitch {
                model: SwitchModel {
                    ron: value(FieldId::OnResistance),
                    roff: value(FieldId::OffResistance),
                    vt: value(FieldId::Threshold),
                    vh: value(FieldId::Hysteresis),
                    ..self.loaded_switch_model()
                },
            },
            PartKind::CurrentSwitch => Part::CurrentSwitch {
                control: self.control.clone(),
                model: CurrentSwitchModel {
                    ron: value(FieldId::OnResistance),
                    roff: value(FieldId::OffResistance),
                    it: value(FieldId::Threshold),
                    ih: value(FieldId::Hysteresis),
                    ..self.loaded_current_switch_model()
                },
            },
            PartKind::Ground => Part::Ground,
            PartKind::OpAmp => Part::OpAmp { model },
        }
    }

    /// The voltage switch model the form was opened with, so that what the
    /// form does not show (the model card's name) is kept.
    fn loaded_switch_model(&self) -> SwitchModel {
        match &self.applied.1 {
            Part::VoltageSwitch { model } => model.clone(),
            _ => SwitchModel::default(),
        }
    }

    /// The current switch model the form was opened with.
    fn loaded_current_switch_model(&self) -> CurrentSwitchModel {
        match &self.applied.1 {
            Part::CurrentSwitch { model, .. } => model.clone(),
            _ => CurrentSwitchModel::default(),
        }
    }

    fn build_source(&self) -> Source {
        let value = |id| self.parsed(id).unwrap_or(0.0);
        Source {
            dc: value(FieldId::Dc),
            ac_magnitude: value(FieldId::AcMagnitude),
            ac_phase: value(FieldId::AcPhase),
            waveform: self.build_waveform(),
        }
    }

    fn build_waveform(&self) -> Option<Waveform> {
        match self.wave {
            WaveKind::None => None,
            WaveKind::Pwl => Some(Waveform::Pwl {
                points: self
                    .pwl
                    .ids()
                    .iter()
                    .map(|&row| {
                        let time = self.parsed(FieldId::PwlTime(row)).unwrap_or(0.0);
                        let value = self.parsed(FieldId::PwlValue(row)).unwrap_or(0.0);
                        (time, value)
                    })
                    .collect(),
                repeat: self.pwl.repeat,
            }),
            kind => {
                let values: Vec<_> = self
                    .wave_fields()
                    .iter()
                    .map(|spec| self.parsed(spec.id))
                    .collect();
                waveform::build(kind, &values)
            }
        }
    }
}
