use super::validation::{DESIGNATOR_CHARS, NOT_A_NUMBER};
use super::*;
use crate::properties::spec::Unit;
use xsim_spice_engine::library;
use xsim_spice_engine::{
    Circuit, CurrentSwitchModel, Device, Element, Polarity, SwitchModel, units::parse_value,
};

fn form(part: &Part) -> PartForm {
    PartForm::new("X1", part, HashSet::new())
}

fn source(waveform: Option<Waveform>) -> Source {
    Source {
        dc: 0.25,
        ac_magnitude: 1.0,
        ac_phase: -45.0,
        waveform,
    }
}

/// One part per kind and per waveform type, with awkward values.
fn every_part() -> Vec<Part> {
    let pulse = Waveform::Pulse {
        initial: -1.0,
        pulsed: 3.3,
        delay: 1.234567e-6,
        rise: 2e-9,
        fall: 3e-9,
        width: 4e-6,
        period: 1e-5,
        cycles: Some(7),
    };
    let sine = Waveform::Sine {
        offset: 0.1,
        amplitude: 2.0,
        frequency: 60.0,
        delay: 0.0,
        damping: 1.5,
        phase: 90.0,
        cycles: None,
    };
    let exp = Waveform::Exp {
        initial: 0.0,
        pulsed: 1.0,
        rise_delay: 1e-3,
        rise_tau: 2e-3,
        fall_delay: 5e-3,
        fall_tau: 1e-3,
    };
    let pwl = Waveform::Pwl {
        points: vec![(0.0, 0.0), (1e-3, 1.0), (2.5e-3, -0.5)],
        repeat: true,
    };
    let sffm = Waveform::Sffm {
        offset: 0.0,
        amplitude: 1.0,
        carrier: 1e4,
        index: 3.0,
        signal: 1e3,
    };
    let mut parts = vec![
        Part::Resistor {
            resistance: 4712.3456789,
        },
        Part::Capacitor {
            capacitance: 1e-6,
            initial_voltage: None,
        },
        Part::Capacitor {
            capacitance: 2.2e-12,
            initial_voltage: Some(-1.5),
        },
        Part::Inductor {
            inductance: 0.1,
            initial_current: Some(1e-3),
        },
        Part::VoltageSource {
            source: Source::dc(5.0),
        },
        Part::CurrentSource {
            source: Source::ac(0.0, 1e-3, 0.0),
        },
    ];
    for waveform in [pulse, sine, exp, pwl, sffm] {
        parts.push(Part::VoltageSource {
            source: source(Some(waveform.clone())),
        });
        parts.push(Part::CurrentSource {
            source: source(Some(waveform)),
        });
    }
    parts.extend([
        Part::Diode {
            model: library::default_diode().into(),
        },
        Part::Bjt {
            polarity: Polarity::N,
            model: "NPN_GP".into(),
        },
        Part::Bjt {
            polarity: Polarity::P,
            model: "PNP_LOWNOISE".into(),
        },
        Part::Jfet {
            polarity: Polarity::N,
            model: "NJF_MID".into(),
        },
        Part::Jfet {
            polarity: Polarity::P,
            model: "PJF_MID".into(),
        },
        Part::VoltageSwitch {
            model: SwitchModel {
                name: "SWFAST".into(),
                ron: 0.0123,
                roff: 4.7e8,
                vt: 2.5,
                vh: -0.25,
            },
        },
        Part::default_for(PartKind::VoltageSwitch),
        // A control that names no source of the (empty) schematic still
        // opens and confirms unchanged: the netlist reports it.
        Part::CurrentSwitch {
            control: "VSENSE".into(),
            model: CurrentSwitchModel {
                name: "CSW".into(),
                ron: 1.0,
                roff: 1e6,
                it: 25e-6,
                ih: 5e-6,
            },
        },
        Part::CurrentSwitch {
            control: String::new(),
            model: CurrentSwitchModel::default(),
        },
        Part::Ground,
    ]);
    parts
}

#[test]
fn every_part_round_trips_unchanged() {
    for part in every_part() {
        let form = form(&part);
        assert_eq!(form.kind(), part.kind());
        assert_eq!(form.to_part(), Ok(part.clone()), "{part:?}");
        assert_eq!(form.pending_edit(), Ok(None), "{part:?}");
        assert!(form.errors().is_empty(), "{part:?}: {:?}", form.errors());
    }
}

#[test]
fn edited_values_parse_si_suffixes_case_sensitively() {
    let mut form = form(&Part::default_for(PartKind::Resistor));
    form.set_text(FieldId::Value, "4.7k");
    assert_eq!(form.to_part(), Ok(Part::Resistor { resistance: 4700.0 }));
    form.set_text(FieldId::Value, " 1M ");
    assert_eq!(form.to_part(), Ok(Part::Resistor { resistance: 1e6 }));
    form.set_text(FieldId::Value, "1m");
    assert_eq!(form.to_part(), Ok(Part::Resistor { resistance: 1e-3 }));
    form.set_text(FieldId::Value, "10kΩ");
    assert_eq!(form.to_part(), Ok(Part::Resistor { resistance: 1e4 }));
}

#[test]
fn invalid_values_are_reported_on_their_field() {
    let mut form = form(&Part::default_for(PartKind::Resistor));
    let spec = form.value_fields()[0];
    for (text, expected) in [
        ("abc", NOT_A_NUMBER),
        ("", "Enter a value"),
        ("0", "Must be greater than 0"),
        ("-1k", "Must be greater than 0"),
    ] {
        form.set_text(FieldId::Value, text);
        assert_eq!(
            form.status(&spec),
            FieldStatus::Invalid(expected.into()),
            "{text:?}"
        );
        let errors = form.to_part().expect_err(text);
        assert_eq!(errors, vec![(FieldId::Value, expected.to_string())]);
    }
    form.set_text(FieldId::Value, "2.2k");
    assert_eq!(form.status(&spec), FieldStatus::Valid("2.2 kΩ".into()));
}

#[test]
fn optional_fields_may_be_left_empty() {
    let mut form = form(&Part::Capacitor {
        capacitance: 1e-9,
        initial_voltage: Some(2.0),
    });
    let initial = form.value_fields()[1];
    form.set_text(FieldId::Initial, "");
    assert_eq!(form.status(&initial), FieldStatus::Empty("Not set"));
    assert_eq!(
        form.pending_edit(),
        Ok(Some((
            "X1".into(),
            Part::Capacitor {
                capacitance: 1e-9,
                initial_voltage: None
            }
        )))
    );
}

#[test]
fn designators_must_be_unique_and_without_spaces() {
    let taken = HashSet::from(["R2".to_string()]);
    let mut form = PartForm::new("R1", &Part::default_for(PartKind::Resistor), taken);
    assert_eq!(form.designator_status(), FieldStatus::Valid(String::new()));
    for (text, message) in [
        ("R2", "R2 is already used by another part"),
        ("  ", "Enter a designator, e.g. R1"),
        ("R 3", "Spaces are not allowed"),
    ] {
        form.set_text(FieldId::Designator, text);
        assert_eq!(
            form.designator_status(),
            FieldStatus::Invalid(message.into())
        );
        assert_eq!(form.errors()[0].0, FieldId::Designator);
    }
    form.set_text(FieldId::Designator, " Rload ");
    assert_eq!(
        form.pending_edit(),
        Ok(Some((
            "Rload".into(),
            Part::default_for(PartKind::Resistor)
        )))
    );
}

/// INT-4: a designator becomes a probe argument such as `I(R1)`, which
/// cannot hold brackets or commas.
#[test]
fn designators_use_only_letters_digits_and_underscores() {
    let mut form = PartForm::new("R1", &Part::default_for(PartKind::Resistor), HashSet::new());
    for text in [
        "R(1)", "R1,2", "R1)", "R-1", "R.1", "R\"1", "R'1", "R\u{e4}1", "R\u{3a9}",
    ] {
        form.set_text(FieldId::Designator, text);
        assert_eq!(
            form.designator_status(),
            FieldStatus::Invalid(DESIGNATOR_CHARS.into()),
            "{text}"
        );
        assert_eq!(form.errors()[0].0, FieldId::Designator, "{text}");
        assert!(form.pending_edit().is_err(), "{text} cannot be applied");
    }
    for text in ["R1", "Rload_2", "r_", "_R9", "VIN"] {
        form.set_text(FieldId::Designator, text);
        assert_eq!(
            form.designator_status(),
            FieldStatus::Valid(String::new()),
            "{text}"
        );
    }
}

#[test]
fn taken_designators_exclude_the_edited_part() {
    let mut schematic = Schematic::default();
    for (id, name) in [(1, "R1"), (2, "R2"), (3, "GND1")] {
        schematic.components.push(xsim_project::Component {
            id,
            name: name.into(),
            position: Default::default(),
            orientation: Default::default(),
            part: Part::Ground,
        });
    }
    let taken = taken_designators(&schematic, 2);
    assert_eq!(taken, HashSet::from(["R1".into(), "GND1".into()]));
}

#[test]
fn applying_resets_the_pending_edit() {
    let mut form = form(&Part::default_for(PartKind::Inductor));
    form.set_text(FieldId::Value, "10m");
    let (name, part) = form.pending_edit().unwrap().expect("changed");
    form.mark_applied(name, part);
    assert_eq!(form.pending_edit(), Ok(None));
    assert_eq!(form.applied_name(), "X1");
}

#[test]
fn switching_waveforms_seeds_defaults_and_keeps_typed_values() {
    let mut form = form(&Part::default_for(PartKind::VoltageSource));
    assert_eq!(form.wave_kind(), WaveKind::None);
    assert!(form.wave_fields().is_empty());
    form.set_wave_kind(WaveKind::Sine);
    let amplitude = FieldId::Wave(WaveKind::Sine, 1);
    assert_eq!(form.text(amplitude), "1");
    form.set_text(amplitude, "3");
    form.set_wave_kind(WaveKind::Pulse);
    assert_eq!(form.wave_fields().len(), 8);
    form.set_wave_kind(WaveKind::Sine);
    assert_eq!(form.text(amplitude), "3", "typed value survives switching");
    let Ok(Part::VoltageSource { source }) = form.to_part() else {
        panic!("valid sine source");
    };
    assert_eq!(
        source.waveform,
        Some(Waveform::Sine {
            offset: 0.0,
            amplitude: 3.0,
            frequency: 1e3,
            delay: 0.0,
            damping: 0.0,
            phase: 0.0,
            cycles: None,
        })
    );
    assert_eq!(
        source.dc, 1.0,
        "the DC value is independent of the waveform"
    );
}

#[test]
fn current_source_defaults_use_milliamps() {
    let mut form = form(&Part::default_for(PartKind::CurrentSource));
    form.set_wave_kind(WaveKind::Pulse);
    assert_eq!(form.text(FieldId::Wave(WaveKind::Pulse, 1)), "1m");
    assert_eq!(form.wave_fields()[1].unit, Unit::Ampere);
}

#[test]
fn pulse_cycles_must_be_whole_and_sine_cycles_may_be_fractional() {
    let mut form = form(&Part::default_for(PartKind::VoltageSource));
    form.set_wave_kind(WaveKind::Pulse);
    let cycles = form.wave_fields()[7];
    assert_eq!(form.status(&cycles), FieldStatus::Empty("Unlimited"));
    form.set_text(cycles.id, "2.5");
    assert!(matches!(form.status(&cycles), FieldStatus::Invalid(_)));
    form.set_text(cycles.id, "3");
    assert!(form.errors().is_empty());
    form.set_wave_kind(WaveKind::Sine);
    let sine_cycles = form.wave_fields()[6];
    form.set_text(sine_cycles.id, "2.5");
    assert!(form.errors().is_empty());
}

#[test]
fn pwl_rows_can_be_added_removed_and_must_be_in_time_order() {
    let mut form = form(&Part::default_for(PartKind::VoltageSource));
    form.set_wave_kind(WaveKind::Pwl);
    assert_eq!(form.pwl_rows().len(), 2, "seeded with two points");
    let third = form.add_pwl_row();
    assert_eq!(form.text(FieldId::PwlTime(third)), "2m");
    assert_eq!(form.text(FieldId::PwlValue(third)), "1");
    form.set_text(FieldId::PwlTime(third), "0.5m");
    let errors = form.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, FieldId::PwlTime(third));
    form.remove_pwl_row(third);
    form.set_pwl_repeat(true);
    let Ok(Part::VoltageSource { source }) = form.to_part() else {
        panic!("valid PWL source");
    };
    assert_eq!(
        source.waveform,
        Some(Waveform::Pwl {
            points: vec![(0.0, 0.0), (1e-3, 1.0)],
            repeat: true,
        })
    );
    let first = form.pwl_rows()[0];
    form.remove_pwl_row(first);
    assert!(!form.can_remove_pwl_row());
    form.remove_pwl_row(form.pwl_rows()[0]);
    assert_eq!(form.pwl_rows().len(), 1, "the last point stays");
}

#[test]
fn waveform_preview_needs_only_valid_waveform_fields() {
    let mut form = form(&Part::default_for(PartKind::VoltageSource));
    assert_eq!(form.waveform(), None);
    form.set_wave_kind(WaveKind::Exp);
    form.set_text(FieldId::Designator, "");
    assert!(form.waveform().is_some(), "designator errors do not matter");
    form.set_text(FieldId::Wave(WaveKind::Exp, 3), "0");
    assert_eq!(form.waveform(), None);
}

#[test]
fn ac_fields_are_empty_when_zero() {
    let form = form(&Part::default_for(PartKind::VoltageSource));
    assert_eq!(form.text(FieldId::AcMagnitude), "");
    assert_eq!(form.text(FieldId::AcPhase), "");
    let fields = form.source_fields();
    assert_eq!(form.status(&fields[1]), FieldStatus::Empty("None"));
}

#[test]
fn models_are_kept_and_required() {
    let mut form = form(&Part::Bjt {
        polarity: Polarity::P,
        model: "PNP_GP".into(),
    });
    assert_eq!(form.model(), "PNP_GP");
    form.set_model("PNP_LOWNOISE");
    assert_eq!(
        form.to_part(),
        Ok(Part::Bjt {
            polarity: Polarity::P,
            model: "PNP_LOWNOISE".into()
        })
    );
    assert_eq!(
        form.model_item(),
        Some(LibraryItem::Bjt {
            polarity: Polarity::P,
            model: "PNP_LOWNOISE".into()
        })
    );
    form.set_model("");
    assert_eq!(
        form.errors(),
        vec![(FieldId::Model, "Choose a model".into())]
    );
    let resistor = PartForm::new("R1", &Part::default_for(PartKind::Resistor), HashSet::new());
    assert_eq!(resistor.model_item(), None);
}

#[test]
fn text_fields_follow_the_visible_form() {
    let mut form = form(&Part::default_for(PartKind::VoltageSource));
    assert_eq!(
        form.text_fields(),
        [
            FieldId::Designator,
            FieldId::Dc,
            FieldId::AcMagnitude,
            FieldId::AcPhase
        ]
    );
    form.set_wave_kind(WaveKind::Pwl);
    let rows = form.pwl_rows().to_vec();
    assert_eq!(form.text_fields().len(), 4 + 2 * rows.len());
    assert!(form.text_fields().contains(&FieldId::PwlValue(rows[1])));
}

#[test]
fn rules_between_waveform_fields_are_reported_inline() {
    let mut form = form(&Part::default_for(PartKind::VoltageSource));
    form.set_wave_kind(WaveKind::Pulse);
    let period = form.wave_fields()[6];
    form.set_text(period.id, "100u");
    assert_eq!(
        form.status(&period),
        FieldStatus::Invalid("Must be at least rise + width + fall (500 µs)".into())
    );
    assert_eq!(
        form.errors(),
        vec![(
            period.id,
            "Must be at least rise + width + fall (500 µs)".to_string()
        )]
    );
    form.set_text(period.id, "0");
    assert!(form.errors().is_empty(), "a single pulse");
    form.set_wave_kind(WaveKind::Exp);
    form.set_text(FieldId::Wave(WaveKind::Exp, 2), "1m");
    assert_eq!(form.errors()[0].0, FieldId::Wave(WaveKind::Exp, 4));
}

#[test]
fn a_repeating_pwl_needs_two_points() {
    let mut form = form(&Part::default_for(PartKind::CurrentSource));
    form.set_wave_kind(WaveKind::Pwl);
    form.set_pwl_repeat(true);
    assert_eq!(form.repeat_error(), None);
    form.remove_pwl_row(form.pwl_rows()[1]);
    assert_eq!(
        form.repeat_error(),
        Some("Repeating needs at least two points")
    );
    assert_eq!(form.errors()[0].0, FieldId::PwlRepeat);
    assert_eq!(form.waveform(), None);
}

/// The value of `text` as the form parses it.
fn si(text: &str) -> f64 {
    parse_value(text).unwrap_or(f64::NAN)
}

/// Whether the engine's model check accepts `device` (D17: the form must
/// accept exactly what it does).
fn engine_accepts(device: Device) -> bool {
    let mut circuit = Circuit::new();
    let nodes: Vec<usize> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| circuit.node(n))
        .collect();
    let nodes = match &device {
        Device::VoltageSwitch { .. } => nodes,
        _ => nodes[..2].to_vec(),
    };
    circuit.add(Element::new(
        "V1",
        [nodes[0], 0],
        Device::VoltageSource(Source::dc(0.0)),
    ));
    circuit.add(Element::new("X1", nodes, device));
    circuit.validate().is_ok()
}

/// D17: RON, ROFF, VT and VH with SI suffixes, VT and VH negative too (a
/// negative VH is the smooth mode); the model card's name is kept.
#[test]
fn voltage_switch_fields_parse_and_build_the_model() {
    let mut form = form(&Part::VoltageSwitch {
        model: SwitchModel {
            name: "SWX".into(),
            ..SwitchModel::default()
        },
    });
    let labels: Vec<_> = form.switch_fields().iter().map(|f| f.label).collect();
    assert_eq!(
        labels,
        [
            "On resistance (RON)",
            "Off resistance (ROFF)",
            "Threshold (VT)",
            "Hysteresis (VH)"
        ]
    );
    let units: Vec<_> = form.switch_fields().iter().map(|f| f.unit).collect();
    assert_eq!(units, [Unit::Ohm, Unit::Ohm, Unit::Volt, Unit::Volt]);
    assert_eq!(
        form.numeric_fields()[0].id,
        FieldId::OnResistance,
        "RON first"
    );
    form.set_text(FieldId::OnResistance, "10m");
    form.set_text(FieldId::OffResistance, " 1G ");
    form.set_text(FieldId::Threshold, "-2.5");
    form.set_text(FieldId::Hysteresis, "-100m");
    let expected = SwitchModel {
        name: "SWX".into(),
        ron: si("10m"),
        roff: 1e9,
        vt: -2.5,
        vh: si("-100m"),
    };
    assert_eq!(
        form.to_part(),
        Ok(Part::VoltageSwitch {
            model: expected.clone()
        })
    );
    assert!(engine_accepts(Device::VoltageSwitch { model: expected }));
}

/// D17: the current switch's thresholds are currents; its control is kept.
#[test]
fn current_switch_fields_parse_and_build_the_model() {
    let mut form = form(&Part::default_for(PartKind::CurrentSwitch))
        .with_control_sources(vec!["V1".into(), "VSENSE".into()]);
    let units: Vec<_> = form.switch_fields().iter().map(|f| f.unit).collect();
    assert_eq!(units, [Unit::Ohm, Unit::Ohm, Unit::Ampere, Unit::Ampere]);
    assert_eq!(form.switch_fields()[2].label, "Threshold (IT)");
    assert_eq!(form.switch_fields()[3].label, "Hysteresis (IH)");
    form.set_text(FieldId::Threshold, "25u");
    form.set_text(FieldId::Hysteresis, "5u");
    form.set_control("VSENSE");
    let Ok(Part::CurrentSwitch { control, model }) = form.to_part() else {
        panic!("a valid current switch: {:?}", form.errors());
    };
    assert_eq!(control, "VSENSE");
    assert_eq!((model.it, model.ih), (si("25u"), si("5u")));
    assert_eq!(
        (model.ron, model.roff, model.name.as_str()),
        (1.0, 1e6, "CSW")
    );
}

/// The device a switch part of `kind` becomes, with these parameters.
fn switch_device(kind: PartKind, ron: f64, roff: f64, threshold: f64, hysteresis: f64) -> Device {
    match kind {
        PartKind::VoltageSwitch => Device::VoltageSwitch {
            model: SwitchModel {
                ron,
                roff,
                vt: threshold,
                vh: hysteresis,
                ..SwitchModel::default()
            },
        },
        _ => Device::CurrentSwitch {
            control: "V1".into(),
            model: CurrentSwitchModel {
                ron,
                roff,
                it: threshold,
                ih: hysteresis,
                ..CurrentSwitchModel::default()
            },
        },
    }
}

/// The form rejects exactly the switch parameters the engine rejects:
/// zero, negative or non-finite resistances, non-finite thresholds and
/// hysteresis; it accepts negative thresholds and hysteresis.
#[test]
fn switch_fields_follow_the_engine_model_check() {
    for kind in [PartKind::VoltageSwitch, PartKind::CurrentSwitch] {
        for (field, text, message) in [
            (FieldId::OnResistance, "0", "Must be greater than 0"),
            (FieldId::OffResistance, "-1M", "Must be greater than 0"),
            (FieldId::OnResistance, "1e999", NOT_A_NUMBER),
            (FieldId::Threshold, "abc", NOT_A_NUMBER),
            (FieldId::Hysteresis, "", "Enter a value"),
        ] {
            let mut form = form(&Part::default_for(kind));
            form.set_text(field, text);
            assert_eq!(
                form.to_part(),
                Err(vec![(field, message.to_string())]),
                "{kind:?} {field:?} = {text:?}"
            );
        }
        let accepts = |ron, roff, threshold, hysteresis| {
            engine_accepts(switch_device(kind, ron, roff, threshold, hysteresis))
        };
        assert!(!accepts(0.0, 1e6, 0.5, 0.0) && !accepts(1.0, -1.0, 0.5, 0.0));
        assert!(!accepts(f64::INFINITY, 1e6, 0.5, 0.0));
        assert!(!accepts(1.0, 1e6, f64::NAN, 0.0) && !accepts(1.0, 1e6, 0.5, f64::INFINITY));
        assert!(
            accepts(1.0, 1e6, -0.5, -0.1),
            "{kind:?}: negative thresholds and hysteresis are legal"
        );
        let mut form = form(&Part::default_for(kind));
        form.set_text(FieldId::Threshold, "-500m");
        form.set_text(FieldId::Hysteresis, "-100m");
        assert!(form.errors().is_empty(), "{kind:?}: {:?}", form.errors());
    }
}

/// D17: the control list and what the form says about the chosen source.
/// A control that is not listed is shown, not refused: the file opened
/// with it, and the netlist names it when the circuit runs.
#[test]
fn the_control_source_is_chosen_from_the_voltage_sources() {
    let part = Part::default_for(PartKind::CurrentSwitch);
    let form_without_sources = form(&part);
    assert!(form_without_sources.has_control());
    assert!(form_without_sources.control_sources().is_empty());
    let status = form_without_sources.control_status();
    assert!(
        matches!(&status, FieldStatus::Invalid(m) if m.contains("no voltage source")),
        "{status:?}"
    );
    assert!(
        form_without_sources.errors().is_empty(),
        "the problem does not block applying"
    );
    assert_eq!(form_without_sources.to_part(), Ok(part.clone()));

    let mut form = form_without_sources.with_control_sources(vec!["Vin".into(), "Vsense".into()]);
    assert_eq!(
        form.control_status(),
        FieldStatus::Invalid("V1 is not a voltage source in this schematic. Choose one.".into())
    );
    form.set_control("Vsense");
    assert_eq!(
        form.control_status(),
        FieldStatus::Valid("Senses I(Vsense), the current into its + pin".into())
    );
    let Ok(Some((_, Part::CurrentSwitch { control, .. }))) = form.pending_edit() else {
        panic!("choosing another source is an edit");
    };
    assert_eq!(control, "Vsense");
    form.set_control("");
    let status = form.control_status();
    assert!(
        matches!(&status, FieldStatus::Invalid(m) if m.starts_with("Choose")),
        "{status:?}"
    );
    for kind in [PartKind::Resistor, PartKind::VoltageSwitch] {
        let other = PartForm::new("X1", &Part::default_for(kind), HashSet::new());
        assert!(!other.has_control(), "{kind:?}");
    }
}

#[test]
fn voltage_sources_are_listed_in_schematic_order() {
    let mut schematic = Schematic::default();
    for (id, name, kind) in [
        (1, "V2", PartKind::VoltageSource),
        (2, "I1", PartKind::CurrentSource),
        (3, "W1", PartKind::CurrentSwitch),
        (4, "Vsense", PartKind::VoltageSource),
    ] {
        schematic.components.push(xsim_project::Component {
            id,
            name: name.into(),
            position: Default::default(),
            orientation: Default::default(),
            part: Part::default_for(kind),
        });
    }
    assert_eq!(voltage_sources(&schematic), ["V2", "Vsense"]);
}
