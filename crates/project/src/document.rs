//! Project and circuit documents.

use crate::schematic::Schematic;
use serde::{Deserialize, Serialize};
use xsim_spice_engine::{
    AcSweep, Analysis, Circuit, DcSweep, InitialState, SimOptions, SimulationResult, SweepScale,
    Transient,
};

/// A whole `.xsim_proj` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub schema_version: u32,
    pub name: String,
    /// At least one circuit (Q35).
    pub circuits: Vec<CircuitDocument>,
    /// Index of the circuit tab shown when the project opens.
    pub active_circuit: usize,
    pub layout: WorkspaceLayout,
}

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl Project {
    /// A project with one empty circuit named "Circuit 1".
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema_version: crate::io::SCHEMA_VERSION,
            name: name.into(),
            circuits: vec![CircuitDocument::new("Circuit 1")],
            active_circuit: 0,
            layout: WorkspaceLayout::default(),
        }
    }

    /// The lowest unused "Circuit N" name.
    pub fn next_circuit_name(&self) -> String {
        let mut n = 1_usize;
        loop {
            let name = format!("Circuit {n}");
            if !self.circuits.iter().any(|c| c.name == name) {
                return name;
            }
            n += 1;
        }
    }
}

/// Per-project workspace layout (Q39).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceLayout {
    /// Whether the diagnostics dock under the schematic is open (Q22).
    pub diagnostics_open: bool,
    /// Fraction of the width given to the schematic when results are shown.
    pub schematic_fraction: f32,
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        Self {
            diagnostics_open: false,
            schematic_fraction: 0.5,
        }
    }
}

/// One circuit tab: schematic, simulation setup, plots, last result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CircuitDocument {
    pub name: String,
    pub schematic: Schematic,
    pub view: SchematicView,
    pub simulation: SimulationSetup,
    pub plot: PlotSetup,
    /// Last simulation result, stored in the project by default (Q38).
    pub result: Option<StoredResult>,
}

impl Default for CircuitDocument {
    fn default() -> Self {
        Self::new("Circuit 1")
    }
}

impl CircuitDocument {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schematic: Schematic::default(),
            view: SchematicView::default(),
            simulation: SimulationSetup::default(),
            plot: PlotSetup::default(),
            result: None,
        }
    }
}

/// Saved schematic camera.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SchematicView {
    /// Grid point at the centre of the canvas.
    pub center: [f32; 2],
    /// Pixels per grid unit at UI scale 1.
    pub zoom: f32,
    /// `false` until the user pans or zooms; the editor then fits the drawing.
    pub user_set: bool,
}

impl Default for SchematicView {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            zoom: 20.0,
            user_set: false,
        }
    }
}

/// Which analysis the Run button executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisChoice {
    OperatingPoint,
    DcSweep,
    Ac,
    Transient,
}

/// Settings of every analysis, so switching analysis keeps the values the
/// user typed (Q75, Q76).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SimulationSetup {
    pub selected: AnalysisChoice,
    pub dc: DcSweep,
    pub ac: AcSweep,
    pub transient: Transient,
    pub options: SimOptions,
}

impl Default for SimulationSetup {
    fn default() -> Self {
        Self {
            selected: AnalysisChoice::Transient,
            dc: DcSweep {
                source: "V1".into(),
                start: 0.0,
                stop: 5.0,
                step: 0.1,
            },
            ac: AcSweep {
                scale: SweepScale::Decade,
                points: 20,
                start: 1.0,
                stop: 1e6,
            },
            transient: Transient {
                stop: 5e-3,
                step: 1e-5,
                start_save: 0.0,
                max_step: None,
                initial_state: InitialState::OperatingPoint,
            },
            options: SimOptions::default(),
        }
    }
}

impl SimulationSetup {
    /// The engine analysis for the selected choice.
    pub fn analysis(&self) -> Analysis {
        match self.selected {
            AnalysisChoice::OperatingPoint => Analysis::OperatingPoint,
            AnalysisChoice::DcSweep => Analysis::DcSweep(self.dc.clone()),
            AnalysisChoice::Ac => Analysis::Ac(self.ac.clone()),
            AnalysisChoice::Transient => Analysis::Transient(self.transient.clone()),
        }
    }
}

/// Vertical axis mode of a plot pane (Q129).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum YScale {
    Linear,
    Log,
    /// 20·log10(|y|).
    Decibel,
    /// Phase in degrees (complex signals).
    Phase,
}

/// One plotted trace: an expression such as `V(out)` or `V(out)-V(in)` (Q132).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trace {
    pub expression: String,
    /// Explicit `0xRRGGBB` colour; when set it wins over [`Self::slot`].
    pub color: Option<u32>,
    pub visible: bool,
    /// Palette slot pinned when the trace was added (D11), so removing
    /// another trace never recolours this one. A slot rather than a colour,
    /// so the trace follows the light and dark theme palettes. `None` (files
    /// written before D11) is filled in by the waveform viewer. Omitted from
    /// the file when `None`, so older files round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            expression: String::new(),
            color: None,
            visible: true,
            slot: None,
        }
    }
}

impl Trace {
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            ..Self::default()
        }
    }
}

/// One plot pane (Q128).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlotPane {
    pub traces: Vec<Trace>,
    /// Visible x range; `None` = auto.
    pub x_range: Option<[f64; 2]>,
    /// Visible y range; `None` = auto.
    pub y_range: Option<[f64; 2]>,
    /// Logarithmic x axis; `None` = automatic (log for AC).
    pub x_log: Option<bool>,
    pub y_scale: YScale,
    /// Visible range of the right y axis, which carries a second unit (D9);
    /// `None` = auto. Omitted from the file when `None`, so older files
    /// round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y2_range: Option<[f64; 2]>,
}

impl Default for PlotPane {
    fn default() -> Self {
        Self {
            traces: Vec::new(),
            x_range: None,
            y_range: None,
            x_log: None,
            y_scale: YScale::Linear,
            y2_range: None,
        }
    }
}

/// Plot configuration, kept across simulation runs (Q136).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlotSetup {
    /// At least one pane once anything is plotted.
    pub panes: Vec<PlotPane>,
    /// Index of the pane new probes add traces to.
    pub active_pane: usize,
}

impl Default for PlotSetup {
    fn default() -> Self {
        Self {
            panes: vec![PlotPane::default()],
            active_pane: 0,
        }
    }
}

/// A result together with the analysis that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredResult {
    pub analysis: Analysis,
    /// [`StoredResult::fingerprint`] of the netlisted circuit the result
    /// was computed from, so that an opened or recovered project can tell
    /// whether the result still belongs to its schematic: a result whose
    /// fingerprint differs from the current netlist's is out of date.
    ///
    /// `None` in files written before the fingerprint existed. Such a
    /// result counts as current only while it still records exactly the
    /// voltages of the current netlist's nodes; a changed value cannot be
    /// detected for it. Omitted from the file when `None`, so older files
    /// round-trip byte-identically.
    ///
    /// A malformed fingerprint makes a current-format file invalid. In a
    /// file of a newer schema it is read as one that never matches, so the
    /// file still opens (read-only) with its result out of date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit_fingerprint: Option<String>,
    pub result: SimulationResult,
}

/// Prefix of the fingerprints [`StoredResult::fingerprint`] produces; it
/// names the scheme, so a later scheme never matches an earlier one.
const FINGERPRINT_SCHEME: &str = "fnv1a64-";

/// Longest fingerprint accepted from a file.
pub(crate) const MAX_FINGERPRINT_LEN: usize = 128;

/// Stands in for a fingerprint a newer xsim wrote in a form this version
/// cannot read (see [`StoredResult::forget_unreadable_fingerprint`]). It is
/// well formed but names no scheme, so it never matches.
const UNREADABLE_FINGERPRINT: &str = "unreadable";

impl StoredResult {
    /// A stable fingerprint of `circuit`: an FNV-1a 64-bit hash of its
    /// canonical JSON, which covers every node name, element, value and
    /// model parameter in netlist order. The same circuit always gives the
    /// same text, across runs and platforms.
    pub fn fingerprint(circuit: &Circuit) -> String {
        let mut hash = Fnv1a::default();
        if serde_json::to_writer(&mut hash, circuit).is_err() {
            // Engine types serialise without errors and the hasher never
            // fails, so this is not reached; the debug text still covers
            // every field.
            hash = Fnv1a::default();
            hash.update(format!("{circuit:?}").as_bytes());
        }
        format!("{FINGERPRINT_SCHEME}{:016x}", hash.0)
    }

    /// Whether the result was computed from `circuit`; `None` when the
    /// result carries no fingerprint (files written before it existed).
    pub fn computed_from(&self, circuit: &Circuit) -> Option<bool> {
        self.circuit_fingerprint
            .as_ref()
            .map(|stored| *stored == Self::fingerprint(circuit))
    }

    /// Whether `text` is shaped like a fingerprint: a short token of ASCII
    /// letters, digits and `-`. Schemes this version does not know are
    /// accepted and simply never match, so the result shows as out of date.
    pub(crate) fn is_valid_fingerprint(text: &str) -> bool {
        !text.is_empty()
            && text.len() <= MAX_FINGERPRINT_LEN
            && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }

    /// For files of a newer schema (Q45): a fingerprint this version cannot
    /// read becomes one that never matches, so the file still opens and the
    /// result shows as out of date. Dropping it instead would fall back to
    /// the weaker check of files without a fingerprint.
    pub(crate) fn forget_unreadable_fingerprint(&mut self) {
        if let Some(fingerprint) = &mut self.circuit_fingerprint
            && !Self::is_valid_fingerprint(fingerprint)
        {
            *fingerprint = UNREADABLE_FINGERPRINT.to_string();
        }
    }
}

/// 64-bit FNV-1a, written out because the standard hasher is not stable
/// across Rust releases.
struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv1a {
    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

impl std::io::Write for Fnv1a {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        for (input, expected) in [
            ("", 0xcbf2_9ce4_8422_2325_u64),
            ("a", 0xaf63_dc4c_8601_ec8c),
            ("foobar", 0x8594_4171_f739_67e8),
        ] {
            let mut hash = Fnv1a::default();
            hash.update(input.as_bytes());
            assert_eq!(hash.0, expected, "{input:?}");
        }
    }

    #[test]
    fn fingerprints_are_well_formed_tokens() {
        let fingerprint = StoredResult::fingerprint(&Circuit::new());
        assert!(fingerprint.starts_with(FINGERPRINT_SCHEME));
        assert_eq!(fingerprint.len(), FINGERPRINT_SCHEME.len() + 16);
        assert!(StoredResult::is_valid_fingerprint(&fingerprint));
        for bad in ["", "fnv1a64 0123", "fnv1a64-ü", &"a".repeat(129)] {
            assert!(!StoredResult::is_valid_fingerprint(bad), "{bad:?}");
        }
        assert!(StoredResult::is_valid_fingerprint("sha256-abc"));
    }
}
