//! The properties form model: a part's parameters as editable texts, with
//! validation while typing ([`validation`]) and conversion back to a
//! [`Part`] (`build`). GPUI-free.
//!
//! Every numeric field remembers the exact value it was loaded with. A field
//! the user did not change converts back to that value, so opening and
//! confirming the dialog never rounds a parameter.

mod build;
mod validation;

pub(crate) use validation::{FieldError, FieldStatus};

use super::pwl::PwlRows;
use super::spec::{self, FieldId, FieldSpec, Unit};
use super::waveform::{self, WaveKind};
use crate::library_browser::LibraryItem;
use std::collections::{HashMap, HashSet};
use xsim_project::{Id, Part, PartKind, Schematic};
use xsim_spice_engine::{Source, Waveform, units};

/// Editable state of the properties dialog for one part.
#[derive(Debug, Clone)]
pub(crate) struct PartForm {
    kind: PartKind,
    texts: HashMap<FieldId, String>,
    /// Text and exact value each numeric field was loaded with.
    originals: HashMap<FieldId, (String, f64)>,
    wave: WaveKind,
    /// Waveform types whose fields already hold texts.
    seeded: HashSet<WaveKind>,
    pwl: PwlRows,
    model: String,
    /// The voltage source whose current controls a current switch.
    control: String,
    /// Designators of the schematic's independent voltage sources, in
    /// schematic order: the choices for [`Self::control`].
    sources: Vec<String>,
    /// Designators used by the other parts of the schematic.
    taken: HashSet<String>,
    /// Designator and part last written to the schematic.
    applied: (String, Part),
}

/// Designators of every component of `schematic` except `id`.
pub(crate) fn taken_designators(schematic: &Schematic, id: Id) -> HashSet<String> {
    schematic
        .components
        .iter()
        .filter(|c| c.id != id)
        .map(|c| c.name.clone())
        .collect()
}

/// Designators of the independent voltage sources of `schematic`, in
/// schematic order: what a current switch can be controlled by (D17).
pub(crate) fn voltage_sources(schematic: &Schematic) -> Vec<String> {
    schematic
        .components
        .iter()
        .filter(|c| c.part.kind() == PartKind::VoltageSource)
        .map(|c| c.name.clone())
        .collect()
}

/// Editable text of `value`: engineering notation without unit, precise
/// enough that typical values read back unchanged.
fn display(value: f64) -> String {
    units::format_value_digits(value, "", 12)
}

impl PartForm {
    /// The form of part `part` named `name`; `taken` holds the designators of
    /// the other parts.
    pub(crate) fn new(name: &str, part: &Part, taken: HashSet<String>) -> Self {
        let mut form = Self {
            kind: part.kind(),
            texts: HashMap::from([(FieldId::Designator, name.to_string())]),
            originals: HashMap::new(),
            wave: WaveKind::None,
            seeded: HashSet::from([WaveKind::None]),
            pwl: PwlRows::default(),
            model: String::new(),
            control: String::new(),
            sources: Vec::new(),
            taken,
            applied: (name.to_string(), part.clone()),
        };
        form.load_part(part);
        form
    }

    /// The voltage sources a current switch can be controlled by, in the
    /// order its control list shows them.
    pub(crate) fn with_control_sources(mut self, sources: Vec<String>) -> Self {
        self.sources = sources;
        self
    }

    fn load_part(&mut self, part: &Part) {
        match part {
            Part::Resistor { resistance } => self.load(FieldId::Value, Some(*resistance)),
            Part::Capacitor {
                capacitance: value,
                initial_voltage: initial,
            }
            | Part::Inductor {
                inductance: value,
                initial_current: initial,
            } => {
                self.load(FieldId::Value, Some(*value));
                self.load(FieldId::Initial, *initial);
            }
            Part::VoltageSource { source } | Part::CurrentSource { source } => {
                self.load_source(source)
            }
            Part::Diode { model }
            | Part::Bjt { model, .. }
            | Part::Jfet { model, .. }
            | Part::OpAmp { model } => self.model = model.clone(),
            Part::VoltageSwitch { model } => {
                self.load_switch(model.ron, model.roff, model.vt, model.vh)
            }
            Part::CurrentSwitch { control, model } => {
                self.control = control.clone();
                self.load_switch(model.ron, model.roff, model.it, model.ih);
            }
            Part::Ground => {}
        }
    }

    fn load_switch(&mut self, ron: f64, roff: f64, threshold: f64, hysteresis: f64) {
        self.load(FieldId::OnResistance, Some(ron));
        self.load(FieldId::OffResistance, Some(roff));
        self.load(FieldId::Threshold, Some(threshold));
        self.load(FieldId::Hysteresis, Some(hysteresis));
    }

    fn load_source(&mut self, source: &Source) {
        let nonzero = |v: f64| (v != 0.0).then_some(v);
        self.load(FieldId::Dc, Some(source.dc));
        self.load(FieldId::AcMagnitude, nonzero(source.ac_magnitude));
        self.load(FieldId::AcPhase, nonzero(source.ac_phase));
        self.wave = WaveKind::of(source.waveform.as_ref());
        self.seeded.insert(self.wave);
        match &source.waveform {
            Some(Waveform::Pwl { points, repeat }) => {
                for &(time, value) in points {
                    let row = self.pwl.push();
                    self.load(FieldId::PwlTime(row), Some(time));
                    self.load(FieldId::PwlValue(row), Some(value));
                }
                self.pwl.repeat = *repeat;
            }
            Some(other) => {
                for (index, value) in waveform::parameters(other).into_iter().enumerate() {
                    self.load(FieldId::Wave(self.wave, index), value);
                }
            }
            None => {}
        }
    }

    fn load(&mut self, id: FieldId, value: Option<f64>) {
        let text = value.map(display).unwrap_or_default();
        if let Some(value) = value {
            self.originals.insert(id, (text.clone(), value));
        }
        self.texts.insert(id, text);
    }

    pub(crate) fn kind(&self) -> PartKind {
        self.kind
    }

    /// Current text of field `id` (empty for unknown fields).
    pub(crate) fn text(&self, id: FieldId) -> &str {
        self.texts.get(&id).map_or("", String::as_str)
    }

    pub(crate) fn set_text(&mut self, id: FieldId, text: impl Into<String>) {
        self.texts.insert(id, text.into());
    }

    pub(crate) fn is_source(&self) -> bool {
        matches!(self.kind, PartKind::VoltageSource | PartKind::CurrentSource)
    }

    /// Whether the part references a library model: a semiconductor or an
    /// op-amp (D26).
    pub(crate) fn has_model(&self) -> bool {
        matches!(
            self.kind,
            PartKind::Diode
                | PartKind::NpnBjt
                | PartKind::PnpBjt
                | PartKind::NJfet
                | PartKind::PJfet
                | PartKind::OpAmp
        )
    }

    /// Whether the model card lists the model's parameters (read-only): an
    /// op-amp's are few and decide how it behaves (D26).
    pub(crate) fn shows_model_parameters(&self) -> bool {
        self.kind == PartKind::OpAmp
    }

    /// Output unit of a source: amperes for current sources, else volts.
    pub(crate) fn signal_unit(&self) -> Unit {
        if self.kind == PartKind::CurrentSource {
            Unit::Ampere
        } else {
            Unit::Volt
        }
    }

    pub(crate) fn wave_kind(&self) -> WaveKind {
        self.wave
    }

    /// Selects the transient waveform type. Fields of a type selected for the
    /// first time are filled with sensible defaults; texts typed for other
    /// types are kept.
    pub(crate) fn set_wave_kind(&mut self, kind: WaveKind) {
        if self.seeded.insert(kind) {
            self.seed(kind);
        }
        self.wave = kind;
    }

    fn seed(&mut self, kind: WaveKind) {
        if kind == WaveKind::Pwl {
            let level = if self.signal_unit() == Unit::Ampere {
                "1m"
            } else {
                "1"
            };
            self.push_pwl_row("0".into(), "0".into());
            self.push_pwl_row("1m".into(), level.into());
            return;
        }
        let signal = self.signal_unit();
        for (index, seed) in kind.seeds(signal).into_iter().enumerate() {
            self.texts
                .insert(FieldId::Wave(kind, index), seed.to_string());
        }
    }

    /// PWL row ids in display order.
    pub(crate) fn pwl_rows(&self) -> &[u64] {
        self.pwl.ids()
    }

    pub(crate) fn can_remove_pwl_row(&self) -> bool {
        self.pwl.can_remove()
    }

    /// Appends a PWL point one step after the last one, with the same value,
    /// and returns its row id.
    pub(crate) fn add_pwl_row(&mut self) -> u64 {
        let (time, value) = self.next_pwl_point();
        self.push_pwl_row(time, value)
    }

    fn next_pwl_point(&self) -> (String, String) {
        let ids = self.pwl.ids();
        let Some(&last) = ids.last() else {
            return ("0".into(), "0".into());
        };
        let last_time = self.parsed(FieldId::PwlTime(last));
        let previous_time = ids
            .len()
            .checked_sub(2)
            .and_then(|i| self.parsed(FieldId::PwlTime(ids[i])));
        let step = match (previous_time, last_time) {
            (Some(previous), Some(last)) if last > previous => last - previous,
            _ => 1e-3,
        };
        let time = last_time.map(|t| display(t + step)).unwrap_or_default();
        (time, self.text(FieldId::PwlValue(last)).to_string())
    }

    fn push_pwl_row(&mut self, time: String, value: String) -> u64 {
        let row = self.pwl.push();
        self.texts.insert(FieldId::PwlTime(row), time);
        self.texts.insert(FieldId::PwlValue(row), value);
        row
    }

    /// Removes PWL row `row` unless it is the last one.
    pub(crate) fn remove_pwl_row(&mut self, row: u64) {
        if self.pwl.remove(row) {
            for id in [FieldId::PwlTime(row), FieldId::PwlValue(row)] {
                self.texts.remove(&id);
                self.originals.remove(&id);
            }
        }
    }

    pub(crate) fn pwl_repeat(&self) -> bool {
        self.pwl.repeat
    }

    pub(crate) fn set_pwl_repeat(&mut self, repeat: bool) {
        self.pwl.repeat = repeat;
    }

    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// The library item of the chosen model; `None` for parts without one.
    pub(crate) fn model_item(&self) -> Option<LibraryItem> {
        if self.has_model() {
            LibraryItem::model_of(&self.build_part())
        } else {
            None
        }
    }

    /// Resistance, capacitance or inductance fields (empty for other kinds).
    pub(crate) fn value_fields(&self) -> Vec<FieldSpec> {
        spec::passive_fields(self.kind)
    }

    /// RON, ROFF, threshold and hysteresis of a switch (empty for other
    /// kinds).
    pub(crate) fn switch_fields(&self) -> Vec<FieldSpec> {
        spec::switch_fields(self.kind)
    }

    /// Whether the part is a current-controlled switch, which names its
    /// control source.
    pub(crate) fn has_control(&self) -> bool {
        self.kind == PartKind::CurrentSwitch
    }

    /// The designator of the control source (empty when none is named).
    pub(crate) fn control(&self) -> &str {
        &self.control
    }

    pub(crate) fn set_control(&mut self, control: impl Into<String>) {
        self.control = control.into();
    }

    /// The voltage sources the control can be chosen from.
    pub(crate) fn control_sources(&self) -> &[String] {
        &self.sources
    }

    /// What the control source does, or why it cannot: the schematic has no
    /// voltage source, none is chosen, or the chosen one is not a voltage
    /// source of the schematic (deleted or renamed since). A problem is
    /// shown but does not block applying, as a file with a dangling control
    /// opens too: the netlist reports it, naming the switch and the
    /// control, when the circuit runs.
    pub(crate) fn control_status(&self) -> FieldStatus {
        // The netlist matches the designator exactly, so this does too.
        let control = self.control.as_str();
        if self.sources.iter().any(|source| source == control) {
            FieldStatus::Valid(format!("Senses I({control}), the current into its + pin"))
        } else if self.sources.is_empty() {
            FieldStatus::Invalid(
                "The schematic has no voltage source. Add one in series with the current \
                 that switches this part (a 0 V source works), then choose it here."
                    .into(),
            )
        } else if control.trim().is_empty() {
            FieldStatus::Invalid(
                "Choose the voltage source whose current switches this part".into(),
            )
        } else {
            FieldStatus::Invalid(format!(
                "{control} is not a voltage source in this schematic. Choose one."
            ))
        }
    }

    /// DC and AC fields of a source (empty for other kinds).
    pub(crate) fn source_fields(&self) -> Vec<FieldSpec> {
        if self.is_source() {
            spec::source_fields(self.signal_unit())
        } else {
            Vec::new()
        }
    }

    /// Parameter fields of the selected non-PWL waveform.
    pub(crate) fn wave_fields(&self) -> Vec<FieldSpec> {
        if self.is_source() {
            self.wave.fields(self.signal_unit())
        } else {
            Vec::new()
        }
    }

    /// `(time, value)` fields of each PWL row while PWL is selected.
    pub(crate) fn pwl_fields(&self) -> Vec<[FieldSpec; 2]> {
        if !self.is_source() || self.wave != WaveKind::Pwl {
            return Vec::new();
        }
        let signal = self.signal_unit();
        self.pwl
            .ids()
            .iter()
            .map(|&row| spec::pwl_fields(row, signal))
            .collect()
    }

    /// Every numeric field currently shown, in display order.
    pub(crate) fn numeric_fields(&self) -> Vec<FieldSpec> {
        let mut fields = self.value_fields();
        fields.extend(self.switch_fields());
        fields.extend(self.source_fields());
        fields.extend(self.wave_fields());
        fields.extend(self.pwl_fields().into_iter().flatten());
        fields
    }

    /// Ids of every text field currently shown.
    pub(crate) fn text_fields(&self) -> Vec<FieldId> {
        std::iter::once(FieldId::Designator)
            .chain(self.numeric_fields().iter().map(|f| f.id))
            .collect()
    }

    /// The value of field `id`: its loaded value while its text is
    /// unchanged, else its text parsed with SI suffixes.
    fn parsed(&self, id: FieldId) -> Option<f64> {
        let text = self.text(id).trim();
        if text.is_empty() {
            return None;
        }
        match self.originals.get(&id) {
            Some((original, value)) if original.trim() == text => Some(*value),
            _ => units::parse_value(text).ok(),
        }
    }
}

#[cfg(test)]
mod tests;
