//! The parts and models offered by the component library, with the metadata
//! shown in its list and preview (Q144). GPUI-free.

use super::categories;
use super::parameters::{self, Parameter};
use super::{LibraryFilter, LibraryItem};
use xsim_project::{Part, PartKind};
use xsim_spice_engine::{
    BjtModel, DiodeModel, JfetModel, OpAmpModel, Polarity,
    library::{self, ModelEntry},
    units::format_value,
};

/// Tree label of the basic passive parts and ground.
pub(crate) const BASIC_PARTS: &str = "Basic parts";
/// Tree label of the independent sources.
pub(crate) const SOURCES: &str = "Sources";
/// Tree label of the voltage- and current-controlled switches (D17).
pub(crate) const SWITCHES: &str = "Switches";
/// Tree label of the diode models.
pub(crate) const DIODES: &str = "Diodes";
/// Tree label of the bipolar transistor models.
pub(crate) const BJTS: &str = "Bipolar transistors";
/// Tree label of the JFET models.
pub(crate) const JFETS: &str = "JFETs";
/// Tree label of the op-amp models (D26).
pub(crate) const OPAMPS: &str = "Op-amps";

/// One part or model of the library.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CatalogEntry {
    /// What picking the entry returns.
    pub item: LibraryItem,
    /// Symbol drawn in the preview.
    pub kind: PartKind,
    pub name: String,
    /// One-line description.
    pub description: String,
    pub manufacturer: String,
    /// Category path in the tree, e.g. `["Diodes", "Switching"]`.
    pub path: Vec<String>,
    /// Where the parameter values come from.
    pub provenance: String,
    /// Key parameters, formatted with units.
    pub parameters: Vec<Parameter>,
}

/// Every entry of the library, in tree order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Catalog {
    entries: Vec<CatalogEntry>,
}

impl Catalog {
    /// The basic parts and the built-in model library, the op-amps (D26)
    /// last.
    pub(crate) fn builtin() -> Self {
        Self::from_models(&library::diodes(), &library::bjts(), &library::jfets())
            .with_opamps(&library::opamps())
    }

    /// The catalog with `opamps` appended, sorted by category and name.
    pub(crate) fn with_opamps(mut self, opamps: &[ModelEntry<OpAmpModel>]) -> Self {
        self.entries.extend(sorted(opamps.iter().map(opamp_entry)));
        self
    }

    /// The basic parts followed by the given models, each family sorted by
    /// category and name.
    pub(crate) fn from_models(
        diodes: &[ModelEntry<DiodeModel>],
        bjts: &[ModelEntry<BjtModel>],
        jfets: &[ModelEntry<JfetModel>],
    ) -> Self {
        let mut entries = basic_entries();
        entries.extend(sorted(diodes.iter().map(diode_entry)));
        entries.extend(sorted(bjts.iter().map(bjt_entry)));
        entries.extend(sorted(jfets.iter().map(jfet_entry)));
        Self { entries }
    }

    /// Keeps only the entries `filter` admits.
    pub(crate) fn restricted(mut self, filter: LibraryFilter) -> Self {
        self.entries.retain(|entry| filter.admits(&entry.item));
        self
    }

    pub(crate) fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// Index of the entry for `item`.
    pub(crate) fn position(&self, item: &LibraryItem) -> Option<usize> {
        self.entries.iter().position(|entry| &entry.item == item)
    }

    /// The entry for `item`.
    pub(crate) fn find(&self, item: &LibraryItem) -> Option<&CatalogEntry> {
        self.position(item).map(|ix| &self.entries[ix])
    }
}

fn sorted(entries: impl Iterator<Item = CatalogEntry>) -> Vec<CatalogEntry> {
    let mut entries: Vec<_> = entries.collect();
    entries.sort_by_cached_key(|e| (e.path.clone(), e.name.to_lowercase()));
    entries
}

fn basic_entries() -> Vec<CatalogEntry> {
    const SOURCE_FUNCTIONS: &str = "DC, AC, PULSE, SINE, EXP, PWL and SFFM";
    [
        (
            PartKind::Resistor,
            BASIC_PARTS,
            "Ideal resistor".to_string(),
        ),
        (
            PartKind::Capacitor,
            BASIC_PARTS,
            "Ideal capacitor with an optional initial voltage".into(),
        ),
        (
            PartKind::Inductor,
            BASIC_PARTS,
            "Ideal inductor with an optional initial current".into(),
        ),
        (
            PartKind::Ground,
            BASIC_PARTS,
            "Reference node at 0 V".into(),
        ),
        (
            PartKind::VoltageSource,
            SOURCES,
            format!("Independent voltage source: {SOURCE_FUNCTIONS}"),
        ),
        (
            PartKind::CurrentSource,
            SOURCES,
            format!("Independent current source: {SOURCE_FUNCTIONS}"),
        ),
    ]
    .into_iter()
    .map(|(kind, category, description)| {
        ideal_entry(kind, category, description, "Built-in ideal element")
    })
    .chain(switch_entries())
    .collect()
}

/// The switches (D17), filed after the sources: ideal elements like them,
/// whose models are the parts' own parameters rather than library models.
fn switch_entries() -> impl Iterator<Item = CatalogEntry> {
    const PROVENANCE: &str = "Built-in ideal switch with LTspice SW / CSW semantics";
    [
        (
            PartKind::VoltageSwitch,
            "Ideal switch from P to N, controlled by the voltage from C+ to C\u{2212} \
             (SPICE S, SW model)",
        ),
        (
            PartKind::CurrentSwitch,
            "Ideal switch from P to N, controlled by the current through a voltage source \
             (SPICE W, CSW model)",
        ),
    ]
    .into_iter()
    .map(|(kind, description)| ideal_entry(kind, SWITCHES, description.into(), PROVENANCE))
}

/// A part without a library model, placed with its default parameters.
fn ideal_entry(
    kind: PartKind,
    category: &str,
    description: String,
    provenance: &str,
) -> CatalogEntry {
    CatalogEntry {
        item: LibraryItem::Part(kind),
        kind,
        name: kind.label().into(),
        description,
        manufacturer: "Generic".into(),
        path: vec![category.into()],
        provenance: provenance.into(),
        parameters: default_parameters(&Part::default_for(kind)),
    }
}

/// The values a newly placed basic part or switch starts with.
fn default_parameters(part: &Part) -> Vec<Parameter> {
    let value = |text: String| vec![("Default value", text)];
    match part {
        Part::Resistor { resistance } => value(format_value(*resistance, "Ω")),
        Part::Capacitor { capacitance, .. } => value(format_value(*capacitance, "F")),
        Part::Inductor { inductance, .. } => value(format_value(*inductance, "H")),
        Part::VoltageSource { source } => value(format!("{} DC", format_value(source.dc, "V"))),
        Part::CurrentSource { source } => value(format!("{} DC", format_value(source.dc, "A"))),
        Part::VoltageSwitch { model } => vec![
            ("RON", format_value(model.ron, "Ω")),
            ("ROFF", format_value(model.roff, "Ω")),
            ("VT", format_value(model.vt, "V")),
            ("VH", format_value(model.vh, "V")),
        ],
        Part::CurrentSwitch { control, model } => vec![
            ("Control", control.clone()),
            ("RON", format_value(model.ron, "Ω")),
            ("ROFF", format_value(model.roff, "Ω")),
            ("IT", format_value(model.it, "A")),
            ("IH", format_value(model.ih, "A")),
        ],
        // Model parts list their model's parameters instead (`model_entry`).
        Part::Diode { .. }
        | Part::Bjt { .. }
        | Part::Jfet { .. }
        | Part::OpAmp { .. }
        | Part::Ground => Vec::new(),
    }
}

fn model_entry<M>(
    entry: &ModelEntry<M>,
    item: LibraryItem,
    kind: PartKind,
    path: Vec<String>,
    parameters: Vec<Parameter>,
) -> CatalogEntry {
    let name = match &item {
        LibraryItem::Diode { model }
        | LibraryItem::Bjt { model, .. }
        | LibraryItem::Jfet { model, .. }
        | LibraryItem::OpAmp { model } => model.clone(),
        LibraryItem::Part(kind) => kind.label().into(),
    };
    CatalogEntry {
        item,
        kind,
        name,
        description: entry.description.into(),
        manufacturer: entry.manufacturer.into(),
        path,
        provenance: entry.source.into(),
        parameters,
    }
}

fn diode_entry(entry: &ModelEntry<DiodeModel>) -> CatalogEntry {
    let path = categories::model_path(&[DIODES], entry.category);
    let item = LibraryItem::Diode {
        model: entry.model.name.clone(),
    };
    let parameters = parameters::diode(&entry.model);
    model_entry(entry, item, PartKind::Diode, path, parameters)
}

fn bjt_entry(entry: &ModelEntry<BjtModel>) -> CatalogEntry {
    let polarity = entry.model.polarity;
    let (label, kind) = match polarity {
        Polarity::N => ("NPN", PartKind::NpnBjt),
        Polarity::P => ("PNP", PartKind::PnpBjt),
    };
    let path = categories::model_path(&[BJTS, label], entry.category);
    let item = LibraryItem::Bjt {
        polarity,
        model: entry.model.name.clone(),
    };
    model_entry(entry, item, kind, path, parameters::bjt(&entry.model))
}

fn jfet_entry(entry: &ModelEntry<JfetModel>) -> CatalogEntry {
    let polarity = entry.model.polarity;
    let (label, kind) = match polarity {
        Polarity::N => ("N-channel", PartKind::NJfet),
        Polarity::P => ("P-channel", PartKind::PJfet),
    };
    let path = categories::model_path(&[JFETS, label], entry.category);
    let item = LibraryItem::Jfet {
        polarity,
        model: entry.model.name.clone(),
    };
    model_entry(entry, item, kind, path, parameters::jfet(&entry.model))
}

/// An op-amp model (D26), filed under "Op-amps" and its library category
/// (Ideal, General purpose, Fast, Rail-to-rail).
fn opamp_entry(entry: &ModelEntry<OpAmpModel>) -> CatalogEntry {
    let path = categories::model_path(&[OPAMPS], entry.category);
    let item = LibraryItem::OpAmp {
        model: entry.model.name.clone(),
    };
    let parameters = parameters::opamp(&entry.model);
    model_entry(entry, item, PartKind::OpAmp, path, parameters)
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! A small model library independent of the built-in one.
    use super::*;

    fn entry<M>(
        model: M,
        category: &'static [&'static str],
        description: &'static str,
        manufacturer: &'static str,
    ) -> ModelEntry<M> {
        ModelEntry {
            model,
            category,
            description,
            manufacturer,
            source: "Test fixture",
        }
    }

    fn diode(name: &str) -> DiodeModel {
        DiodeModel {
            name: name.into(),
            ..DiodeModel::default()
        }
    }

    fn bjt(name: &str, polarity: Polarity) -> BjtModel {
        BjtModel {
            name: name.into(),
            polarity,
            ..BjtModel::default()
        }
    }

    fn jfet(name: &str, polarity: Polarity) -> JfetModel {
        JfetModel {
            name: name.into(),
            polarity,
            ..JfetModel::default()
        }
    }

    /// Two diodes, three BJTs and two JFETs with category paths in the
    /// shapes the library may use.
    pub(crate) fn catalog() -> Catalog {
        Catalog::from_models(
            &[
                entry(
                    diode("D_SWITCH"),
                    &["Diodes", "Switching"],
                    "Fast switching diode, 100 V",
                    "Acme",
                ),
                entry(
                    diode("D_RECT"),
                    &["Rectifier"],
                    "General-purpose rectifier, 1 kV, 1 A",
                    "Northwind",
                ),
            ],
            &[
                entry(
                    bjt("NPN_SMALL", Polarity::N),
                    &["Transistors", "BJT", "NPN", "Small signal"],
                    "Small-signal NPN, 45 V",
                    "Generic",
                ),
                entry(
                    bjt("NPN_GP", Polarity::N),
                    &["BJT", "General purpose"],
                    "General-purpose NPN, 40 V, 600 mA",
                    "Acme",
                ),
                entry(
                    bjt("PNP_GP", Polarity::P),
                    &["General purpose"],
                    "General-purpose PNP, 60 V, 600 mA",
                    "Acme",
                ),
            ],
            &[
                entry(
                    jfet("NJF_RF", Polarity::N),
                    &["JFETs", "N-channel"],
                    "N-channel RF amplifier",
                    "Generic",
                ),
                entry(
                    jfet("PJF_GP", Polarity::P),
                    &[],
                    "P-channel general purpose",
                    "Generic",
                ),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(catalog: &Catalog) -> Vec<&str> {
        catalog.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn basic_parts_come_first_then_each_family_sorted() {
        let catalog = fixtures::catalog();
        assert_eq!(
            names(&catalog),
            [
                "Resistor",
                "Capacitor",
                "Inductor",
                "Ground",
                "Voltage source",
                "Current source",
                "Voltage-controlled switch",
                "Current-controlled switch",
                "D_RECT",
                "D_SWITCH",
                "NPN_GP",
                "NPN_SMALL",
                "PNP_GP",
                "NJF_RF",
                "PJF_GP",
            ]
        );
    }

    /// D17: both switches, in their own category, place their default part
    /// and list its model parameters.
    #[test]
    fn switches_have_their_own_category_and_default_parameters() {
        let catalog = fixtures::catalog();
        let entry = |kind| {
            catalog
                .find(&LibraryItem::Part(kind))
                .unwrap_or_else(|| panic!("{kind:?} is listed"))
        };
        let voltage = entry(PartKind::VoltageSwitch);
        assert_eq!(voltage.path, [SWITCHES]);
        assert_eq!(voltage.kind, PartKind::VoltageSwitch);
        assert_eq!(
            voltage.item.to_part(),
            Part::default_for(PartKind::VoltageSwitch)
        );
        assert_eq!(
            voltage.parameters,
            [
                ("RON", "1 Ω".to_string()),
                ("ROFF", "1 MΩ".into()),
                ("VT", "500 mV".into()),
                ("VH", "0 V".into()),
            ]
        );
        assert!(voltage.description.contains("SW model"));
        let current = entry(PartKind::CurrentSwitch);
        assert_eq!(current.path, [SWITCHES]);
        assert_eq!(
            current.item.to_part(),
            Part::default_for(PartKind::CurrentSwitch)
        );
        let labels: Vec<_> = current.parameters.iter().map(|p| p.0).collect();
        assert_eq!(labels, ["Control", "RON", "ROFF", "IT", "IH"]);
        assert_eq!(current.parameters[0].1, "V1");
        assert_eq!(current.parameters[3].1, "1 mA");
        assert!(current.description.contains("CSW model"));
        for kind in [PartKind::VoltageSwitch, PartKind::CurrentSwitch] {
            assert_eq!(
                LibraryFilter::for_kind(kind),
                None,
                "a switch has no library model to choose"
            );
            assert!(Catalog::builtin().find(&LibraryItem::Part(kind)).is_some());
        }
    }

    #[test]
    fn model_entries_carry_their_metadata_and_tree_path() {
        let catalog = fixtures::catalog();
        let item = LibraryItem::Bjt {
            polarity: Polarity::N,
            model: "NPN_SMALL".into(),
        };
        let entry = catalog.find(&item).expect("NPN_SMALL");
        assert_eq!(entry.kind, PartKind::NpnBjt);
        assert_eq!(entry.path, [BJTS, "NPN", "Small signal"]);
        assert_eq!(entry.description, "Small-signal NPN, 45 V");
        assert_eq!(entry.provenance, "Test fixture");
        assert_eq!(entry.parameters[0], ("Polarity", "NPN".to_string()));
        let jfet = catalog
            .find(&LibraryItem::Jfet {
                polarity: Polarity::P,
                model: "PJF_GP".into(),
            })
            .expect("PJF_GP");
        assert_eq!(jfet.path, [JFETS, "P-channel"]);
    }

    #[test]
    fn basic_parts_describe_their_default_value() {
        let catalog = fixtures::catalog();
        let resistor = catalog
            .find(&LibraryItem::Part(PartKind::Resistor))
            .expect("resistor");
        assert_eq!(resistor.parameters, [("Default value", "1 kΩ".to_string())]);
        assert_eq!(resistor.path, [BASIC_PARTS]);
        let ground = catalog
            .find(&LibraryItem::Part(PartKind::Ground))
            .expect("ground");
        assert!(ground.parameters.is_empty());
    }

    #[test]
    fn filters_restrict_the_catalog_to_one_family() {
        let pnp = fixtures::catalog().restricted(LibraryFilter::Bjts(Polarity::P));
        assert_eq!(names(&pnp), ["PNP_GP"]);
        let diodes = fixtures::catalog().restricted(LibraryFilter::Diodes);
        assert_eq!(names(&diodes), ["D_RECT", "D_SWITCH"]);
        let njf = fixtures::catalog().restricted(LibraryFilter::Jfets(Polarity::N));
        assert_eq!(names(&njf), ["NJF_RF"]);
        assert_eq!(
            fixtures::catalog().restricted(LibraryFilter::All),
            fixtures::catalog()
        );
    }

    #[test]
    fn builtin_catalog_contains_the_default_models() {
        let catalog = Catalog::builtin();
        for item in [
            LibraryItem::Diode {
                model: library::default_diode().into(),
            },
            LibraryItem::Bjt {
                polarity: Polarity::N,
                model: library::default_bjt(Polarity::N).into(),
            },
            LibraryItem::Bjt {
                polarity: Polarity::P,
                model: library::default_bjt(Polarity::P).into(),
            },
            LibraryItem::Jfet {
                polarity: Polarity::N,
                model: library::default_jfet(Polarity::N).into(),
            },
            LibraryItem::OpAmp {
                model: library::default_opamp().into(),
            },
        ] {
            assert!(catalog.find(&item).is_some(), "{item:?}");
        }
    }

    /// D26: the four generic op-amps come last, under "Op-amps" and their
    /// library category, each with its metadata and parameters, and place
    /// an op-amp using that model. No toolbar part lists them.
    #[test]
    fn opamps_have_their_own_category_with_their_metadata() {
        let catalog = Catalog::builtin();
        let opamps: Vec<&CatalogEntry> = catalog
            .entries()
            .iter()
            .filter(|e| e.kind == PartKind::OpAmp)
            .collect();
        let listed: Vec<(&str, Vec<&str>)> = opamps
            .iter()
            .map(|e| (e.name.as_str(), e.path.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            listed,
            [
                ("OA_FAST", vec![OPAMPS, "Fast"]),
                ("OA_GP", vec![OPAMPS, "General purpose"]),
                ("OA_IDEAL", vec![OPAMPS, "Ideal"]),
                ("OA_RRIO", vec![OPAMPS, "Rail-to-rail"]),
            ]
        );
        let last = catalog.entries().len() - opamps.len();
        assert!(
            catalog.entries()[last..]
                .iter()
                .all(|e| e.kind == PartKind::OpAmp),
            "the op-amps are the last family"
        );
        for entry in &opamps {
            let model = library::opamp(&entry.name).expect("a library model");
            assert_eq!(
                entry.item.to_part(),
                Part::OpAmp {
                    model: model.name.clone()
                }
            );
            assert_eq!(entry.parameters, parameters::opamp(&model));
            assert!(entry.provenance.starts_with("xsim generic model (D13)"));
            assert!(!entry.description.is_empty());
            assert!(LibraryFilter::OpAmps.admits(&entry.item));
        }
        assert_eq!(
            Catalog::builtin()
                .restricted(LibraryFilter::OpAmps)
                .entries()
                .len(),
            4
        );
        assert!(
            catalog.find(&LibraryItem::Part(PartKind::OpAmp)).is_none(),
            "an op-amp is always placed with a model"
        );
    }
}
