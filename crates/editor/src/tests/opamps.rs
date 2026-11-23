//! The op-amp (D26): placing it from the component library, probing its
//! output current and the hover card of a simulated non-inverting
//! amplifier.

use super::harness::open;
use super::probing::probes;
use super::switches::pick_from_library;
use crate::model::measure::MeasurementKind;
use crate::ui::hover::HOVER_DELAY;
use crate::{ObjectRef, ProbeContext, ProbeRequest, Tool};
use gpui_kit::TestAppContext;
use std::sync::Arc;
use xsim_project::{Component, NetLabel, Orientation, Part, PartKind, Point, Schematic, netlist};
use xsim_spice_engine::{
    Analysis, SimControl, SimOptions, SimulationResult, Source, library, simulate,
};

/// D26: an op-amp is placed from the library window with the picked model,
/// the next free U designator and the pins of its symbol; the library's
/// default op-amp is the general-purpose one.
#[gpui_kit::test]
fn opamps_are_placed_from_the_component_library(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    pick_from_library(&mut h, "OA_FAST");
    assert_eq!(h.read(|e| e.tool()), Tool::Place(PartKind::OpAmp));
    assert_eq!(
        h.read(|e| e.placing().cloned()),
        Some(Part::OpAmp {
            model: "OA_FAST".into()
        })
    );
    h.click(0, 0);
    h.keys("escape");
    pick_from_library(&mut h, "rail-to-rail");
    h.click(20, 0);
    h.keys("escape");
    let s = h.schematic();
    let placed: Vec<(&str, &Part)> = s
        .components
        .iter()
        .map(|c| (c.name.as_str(), &c.part))
        .collect();
    let model = |name: &str| Part::OpAmp { model: name.into() };
    assert_eq!(
        placed,
        [("U1", &model("OA_FAST")), ("U2", &model("OA_RRIO"))]
    );
    assert_eq!(
        s.components[0].pins(),
        [
            Point::new(-4, 2),
            Point::new(-4, -2),
            Point::new(4, 0),
            Point::new(0, -4),
            Point::new(0, 4),
        ]
    );
    assert_eq!(
        Part::default_for(PartKind::OpAmp),
        model(library::default_opamp())
    );
    assert!(h.read(|e| e.can_undo()));
    h.close();
}

/// Where the op-amp of [`amplifier`] sits.
const U1_AT: (i32, i32) = (0, 0);

/// A part of [`amplifier`]: designator, position, part and the nets of its
/// pins.
type BenchPart = (&'static str, (i32, i32), Part, &'static [&'static str]);

/// A non-inverting amplifier of gain 10 on ±15 V rails: `Vin` (1 V) into
/// `+`, `Rf` 9 kΩ from `out` to `fb` (`−`), `Rg` 1 kΩ from `fb` to ground
/// and a 2 kΩ load `RL`, every net joined by labels on the pins.
fn amplifier(model: &str) -> Schematic {
    let r = |resistance| Part::Resistor { resistance };
    let dc = |volts| Part::VoltageSource {
        source: Source::dc(volts),
    };
    let parts: [BenchPart; 7] = [
        (
            "U1",
            U1_AT,
            Part::OpAmp {
                model: model.into(),
            },
            &["in", "fb", "out", "vp", "vn"],
        ),
        ("Vin", (-24, 16), dc(1.0), &["in", "0"]),
        ("Vp", (-24, -16), dc(15.0), &["vp", "0"]),
        ("Vn", (0, -16), dc(-15.0), &["vn", "0"]),
        ("Rf", (24, -16), r(9e3), &["out", "fb"]),
        ("Rg", (24, 16), r(1e3), &["fb", "0"]),
        ("RL", (0, 16), r(2e3), &["out", "0"]),
    ];
    let mut s = Schematic::default();
    for (name, (x, y), part, nets) in parts {
        let id = s.allocate_id();
        let c = Component {
            id,
            name: name.into(),
            position: Point::new(x, y),
            orientation: Orientation::default(),
            part,
        };
        for (pin, net) in c.pins().into_iter().zip(nets) {
            let id = s.allocate_id();
            s.labels.push(NetLabel {
                id,
                name: net.to_string(),
                position: pin,
            });
        }
        s.components.push(c);
    }
    s
}

fn operating_point(schematic: &Schematic) -> ProbeContext {
    let netlist = netlist(schematic).expect("the amplifier netlists");
    let result = simulate(
        &netlist.circuit,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .expect("the amplifier simulates");
    ProbeContext {
        netlist: Arc::new(netlist),
        result: Arc::new(result),
    }
}

fn value(result: &SimulationResult, name: &str) -> f64 {
    result
        .signal(name)
        .and_then(|s| s.real())
        .and_then(|v| v.first().copied())
        .unwrap_or_else(|| panic!("{name} is recorded"))
}

/// D26, Q62: clicking an op-amp probes `Io(U1)`, the current into its
/// output; resting on it shows V(out), that current and the power it
/// dissipates: what the supplies deliver into V+ and V− less what the
/// output delivers, from all five terminal currents. Checked against the
/// supply sources' own currents, for the macro-model and the ideal one.
#[gpui_kit::test]
fn opamps_probe_their_output_current_and_show_their_dissipation(cx: &mut TestAppContext) {
    for model in ["OA_GP", "OA_IDEAL"] {
        let schematic = amplifier(model);
        let context = operating_point(&schematic);
        let result = context.result.clone();
        for terminal in ["Ip", "In", "Io", "Isp", "Isn"] {
            let name = format!("{terminal}(U1)");
            assert!(result.signal(&name).is_some(), "{model}: {name}");
        }
        let mut h = open(cx, schematic);
        h.update(|e, cx| e.set_probe_context(Some(context.clone()), cx));
        h.events();
        h.click(U1_AT.0 + 1, U1_AT.1);
        assert_eq!(h.read(|e| e.selection()), [ObjectRef::Component(1)]);
        assert_eq!(
            probes(&mut h),
            [ProbeRequest::Current {
                signal: "Io(U1)".into()
            }],
            "{model}"
        );

        let p = h.at(U1_AT.0 + 1, U1_AT.1);
        h.hover(p);
        h.wait(HOVER_DELAY);
        assert!(h.control("hover-card").is_some(), "{model}: the card");
        let card = h.read(|e| e.hover.card.clone()).expect("a card");
        assert_eq!(card.title, format!("U1  ·  {model}"));
        let m = card.measurement.expect("an operating point has values");
        assert_eq!(m.kind, MeasurementKind::OperatingPoint);
        assert_eq!((m.voltage_label, m.current_label), ("V_OUT", "Io"));
        let v = |name: &str| value(&result, name);
        let out = v("V(out)");
        assert!((out - 10.0).abs() < 2e-3, "{model}: gain 10, V(out) {out}");
        assert_eq!(m.voltage, Some(out), "{model}");
        assert_eq!(m.current, Some(v("Io(U1)")), "{model}");
        // The output sources the load and feedback currents.
        let sourced = out / 2e3 + (out - v("V(fb)")) / 9e3;
        let io = v("Io(U1)");
        assert!(((io + sourced) / sourced).abs() < 1e-9, "{model}: Io {io}");
        // What V+ and V− draw from their sources (each source's current
        // flows into its own + pin), less what the output delivers.
        let dissipated = 15.0 * -v("I(Vp)")
            + -15.0 * -v("I(Vn)")
            + out * io
            + v("Ip(U1)")
            + v("V(fb)") * v("In(U1)");
        let power = m.power.expect("all five currents are recorded");
        assert!(
            ((power - dissipated) / dissipated).abs() < 1e-9,
            "{model}: P {power} vs {dissipated}"
        );
        assert!(
            ((power - (15.0 - out) * sourced) / power).abs() < 1e-6,
            "{model}: sourcing, the current comes from V+ ({power} W)"
        );
        h.close();
    }
}
