//! Property tests: random documents survive serialisation unchanged, and
//! connectivity depends only on the shape of a drawing, not on where it is.

use proptest::prelude::*;
use xsim_project::{
    AnalysisChoice, CircuitDocument, Component, Connectivity, NetLabel, Orientation, Part,
    PartKind, PlotPane, PlotSetup, Point, Project, Schematic, SchematicView, Trace, Wire,
    WorkspaceLayout, YScale, connectivity, netlist,
};
use xsim_spice_engine::{Polarity, Source, Waveform};

/// Any finite `f64`, including zeros of both signs and subnormals.
fn finite() -> impl Strategy<Value = f64> {
    use proptest::num::f64::{NEGATIVE, NORMAL, POSITIVE, SUBNORMAL, ZERO};
    POSITIVE | NEGATIVE | NORMAL | SUBNORMAL | ZERO
}

fn finite_f32() -> impl Strategy<Value = f32> {
    use proptest::num::f32::{NEGATIVE, NORMAL, POSITIVE, SUBNORMAL, ZERO};
    POSITIVE | NEGATIVE | NORMAL | SUBNORMAL | ZERO
}

/// Coordinates on a small grid, so that pins, vertices and labels collide
/// often and every connection rule is exercised.
fn point() -> impl Strategy<Value = Point> {
    (-6..=6, -6..=6).prop_map(|(x, y)| Point::new(x, y))
}

fn waveform() -> impl Strategy<Value = Waveform> {
    prop_oneof![
        (
            [
                finite(),
                finite(),
                finite(),
                finite(),
                finite(),
                finite(),
                finite()
            ],
            proptest::option::of(any::<u32>())
        )
            .prop_map(|(v, cycles)| Waveform::Pulse {
                initial: v[0],
                pulsed: v[1],
                delay: v[2],
                rise: v[3],
                fall: v[4],
                width: v[5],
                period: v[6],
                cycles,
            }),
        (
            [finite(), finite(), finite(), finite(), finite(), finite()],
            proptest::option::of(finite())
        )
            .prop_map(|(v, cycles)| Waveform::Sine {
                offset: v[0],
                amplitude: v[1],
                frequency: v[2],
                delay: v[3],
                damping: v[4],
                phase: v[5],
                cycles,
            }),
        [finite(), finite(), finite(), finite(), finite(), finite()].prop_map(|v| Waveform::Exp {
            initial: v[0],
            pulsed: v[1],
            rise_delay: v[2],
            rise_tau: v[3],
            fall_delay: v[4],
            fall_tau: v[5],
        }),
        (
            prop::collection::vec((finite(), finite()), 0..4),
            any::<bool>()
        )
            .prop_map(|(points, repeat)| Waveform::Pwl { points, repeat }),
        [finite(), finite(), finite(), finite(), finite()].prop_map(|v| Waveform::Sffm {
            offset: v[0],
            amplitude: v[1],
            carrier: v[2],
            index: v[3],
            signal: v[4],
        }),
    ]
}

fn source() -> impl Strategy<Value = Source> {
    (
        finite(),
        finite(),
        finite(),
        proptest::option::of(waveform()),
    )
        .prop_map(|(dc, ac_magnitude, ac_phase, waveform)| Source {
            dc,
            ac_magnitude,
            ac_phase,
            waveform,
        })
}

fn polarity() -> impl Strategy<Value = Polarity> {
    prop_oneof![Just(Polarity::N), Just(Polarity::P)]
}

/// Parts whose netlisting needs no model library.
fn passive_part() -> impl Strategy<Value = Part> {
    let positive = 1e-15..1e9f64;
    prop_oneof![
        positive
            .clone()
            .prop_map(|resistance| Part::Resistor { resistance }),
        (positive.clone(), proptest::option::of(-10.0..10.0f64)).prop_map(
            |(capacitance, initial_voltage)| Part::Capacitor {
                capacitance,
                initial_voltage
            }
        ),
        (positive, proptest::option::of(-1.0..1.0f64)).prop_map(|(inductance, initial_current)| {
            Part::Inductor {
                inductance,
                initial_current,
            }
        }),
        (-10.0..10.0f64).prop_map(|v| Part::VoltageSource {
            source: Source::dc(v)
        }),
        (-1.0..1.0f64).prop_map(|i| Part::CurrentSource {
            source: Source::dc(i)
        }),
        Just(Part::Ground),
    ]
}

/// Every part variant with arbitrary finite parameters.
fn any_part() -> impl Strategy<Value = Part> {
    let model = "[A-Za-z0-9_-]{0,8}";
    prop_oneof![
        finite().prop_map(|resistance| Part::Resistor { resistance }),
        (finite(), proptest::option::of(finite())).prop_map(|(capacitance, initial_voltage)| {
            Part::Capacitor {
                capacitance,
                initial_voltage,
            }
        }),
        (finite(), proptest::option::of(finite())).prop_map(|(inductance, initial_current)| {
            Part::Inductor {
                inductance,
                initial_current,
            }
        }),
        source().prop_map(|source| Part::VoltageSource { source }),
        source().prop_map(|source| Part::CurrentSource { source }),
        model.prop_map(|model| Part::Diode { model }),
        (polarity(), model).prop_map(|(polarity, model)| Part::Bjt { polarity, model }),
        (polarity(), model).prop_map(|(polarity, model)| Part::Jfet { polarity, model }),
        Just(Part::Ground),
    ]
}

fn orientation() -> impl Strategy<Value = Orientation> {
    (0..4u8, any::<bool>(), any::<bool>()).prop_map(|(rotation, mirror_x, mirror_y)| Orientation {
        rotation,
        mirror_x,
        mirror_y,
    })
}

/// A polyline with distinct consecutive points.
fn wire_points() -> impl Strategy<Value = Vec<Point>> {
    prop::collection::vec(point(), 2..5).prop_filter_map("needs two distinct points", |mut p| {
        p.dedup();
        (p.len() >= 2).then_some(p)
    })
}

fn label_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("0".to_string()),
        Just("N001".to_string()),
        "[a-c]{1,2}",
    ]
}

/// Assembles a schematic with unique ids and designators.
fn schematic(parts: impl Strategy<Value = Part>) -> impl Strategy<Value = Schematic> {
    (
        prop::collection::vec((parts, point(), orientation()), 0..8),
        prop::collection::vec(wire_points(), 0..8),
        prop::collection::vec((label_name(), point()), 0..4),
    )
        .prop_map(|(parts, wires, labels)| {
            let mut s = Schematic::default();
            for (part, position, orientation) in parts {
                let id = s.allocate_id();
                let name = s.next_designator(part.kind());
                s.components.push(Component {
                    id,
                    name,
                    position,
                    orientation,
                    part,
                });
            }
            for points in wires {
                let id = s.allocate_id();
                s.wires.push(Wire { id, points });
            }
            for (name, position) in labels {
                let id = s.allocate_id();
                s.labels.push(NetLabel { id, name, position });
            }
            s
        })
}

fn plot() -> impl Strategy<Value = PlotSetup> {
    let range = || proptest::option::of([finite(), finite()]);
    let y_scale = prop_oneof![
        Just(YScale::Linear),
        Just(YScale::Log),
        Just(YScale::Decibel),
        Just(YScale::Phase),
    ];
    let trace = (
        "\\PC{0,12}",
        proptest::option::of(0..=0xff_ffffu32),
        any::<bool>(),
        proptest::option::of(any::<u32>()),
    )
        .prop_map(|(expression, color, visible, slot)| Trace {
            expression,
            color,
            visible,
            slot,
        });
    let pane = (
        prop::collection::vec(trace, 0..3),
        range(),
        range(),
        proptest::option::of(any::<bool>()),
        y_scale,
        range(),
    )
        .prop_map(
            |(traces, x_range, y_range, x_log, y_scale, y2_range)| PlotPane {
                traces,
                x_range,
                y_range,
                x_log,
                y_scale,
                y2_range,
            },
        );
    prop::collection::vec(pane, 1..3).prop_flat_map(|panes| {
        let count = panes.len();
        (Just(panes), 0..count).prop_map(|(panes, active_pane)| PlotSetup { panes, active_pane })
    })
}

fn circuit() -> impl Strategy<Value = CircuitDocument> {
    let choice = prop_oneof![
        Just(AnalysisChoice::OperatingPoint),
        Just(AnalysisChoice::DcSweep),
        Just(AnalysisChoice::Ac),
        Just(AnalysisChoice::Transient),
    ];
    (
        "\\PC{0,16}",
        schematic(any_part()),
        ([finite_f32(), finite_f32()], finite_f32(), any::<bool>()),
        (choice, finite(), finite()),
        plot(),
    )
        .prop_map(
            |(name, schematic, (center, zoom, user_set), (selected, stop, temperature), plot)| {
                let mut doc = CircuitDocument::new(name);
                doc.schematic = schematic;
                doc.view = SchematicView {
                    center,
                    zoom,
                    user_set,
                };
                doc.simulation.selected = selected;
                doc.simulation.transient.stop = stop;
                doc.simulation.options.temperature = temperature;
                doc.plot = plot;
                doc
            },
        )
}

fn project() -> impl Strategy<Value = Project> {
    (
        "\\PC{0,16}",
        prop::collection::vec(circuit(), 1..3),
        any::<bool>(),
        finite_f32(),
    )
        .prop_flat_map(|(name, circuits, diagnostics_open, schematic_fraction)| {
            let count = circuits.len();
            (0..count).prop_map(move |active_circuit| {
                let mut project = Project::new(name.clone());
                project.circuits = circuits.clone();
                project.active_circuit = active_circuit;
                project.layout = WorkspaceLayout {
                    diagnostics_open,
                    schematic_fraction,
                };
                project
            })
        })
}

fn translated(schematic: &Schematic, d: Point) -> Schematic {
    let mut moved = schematic.clone();
    for c in &mut moved.components {
        c.position = c.position + d;
    }
    for w in &mut moved.wires {
        for p in &mut w.points {
            *p = *p + d;
        }
    }
    for l in &mut moved.labels {
        l.position = l.position + d;
    }
    moved
}

/// `moved` is `original` shifted by `d`, net for net and dot for dot.
fn assert_translated(original: &Connectivity, moved: &Connectivity, d: Point) {
    assert_eq!(original.nets.len(), moved.nets.len());
    for (a, b) in original.nets.iter().zip(&moved.nets) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.pins, b.pins);
        assert_eq!(a.wires, b.wires);
        assert_eq!(a.is_ground, b.is_ground);
        let shifted: Vec<Point> = a.points.iter().map(|&p| p + d).collect();
        assert_eq!(shifted, b.points);
    }
    let shifted: Vec<Point> = original.junctions.iter().map(|&p| p + d).collect();
    assert_eq!(shifted, moved.junctions);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn projects_round_trip_through_json_unchanged(project in project()) {
        let text = project.to_json();
        let loaded = Project::from_json(&text).unwrap();
        prop_assert_eq!(&loaded.project, &project);
        prop_assert_eq!(loaded.project.to_json(), text);
    }

    #[test]
    fn connectivity_is_invariant_under_translation(
        s in schematic(any_part()),
        dx in -100_000..100_000i32,
        dy in -100_000..100_000i32,
    ) {
        let d = Point::new(dx, dy);
        let original = connectivity(&s);
        let moved = connectivity(&translated(&s, d));
        assert_translated(&original, &moved, d);
        for x in -9..=9 {
            for y in -9..=9 {
                let p = Point::new(x, y);
                prop_assert_eq!(original.net_at(p), moved.net_at(p + d));
            }
        }
    }

    #[test]
    fn every_net_point_resolves_to_its_net(s in schematic(any_part())) {
        let c = connectivity(&s);
        for (i, net) in c.nets.iter().enumerate() {
            prop_assert!(!net.pins.is_empty());
            for &p in &net.points {
                prop_assert_eq!(c.net_at(p), Some(i));
            }
            for &(component, pin) in &net.pins {
                prop_assert_eq!(c.net_of_pin(component, pin), Some(i));
            }
        }
        // Every pin is on exactly one net.
        for component in &s.components {
            for pin in 0..component.pins().len() {
                let owners = c.nets.iter().filter(|n| n.pins.contains(&(component.id, pin))).count();
                prop_assert_eq!(owners, 1);
            }
        }
        let names: std::collections::BTreeSet<_> = c.nets.iter().map(|n| &n.name).collect();
        prop_assert_eq!(names.len(), c.nets.len(), "net names are unique");
    }

    #[test]
    fn netlists_are_invariant_under_translation(
        s in schematic(passive_part()),
        dx in -100_000..100_000i32,
        dy in -100_000..100_000i32,
    ) {
        let d = Point::new(dx, dy);
        let original = netlist(&s);
        let moved = netlist(&translated(&s, d));
        match (original, moved) {
            (Ok(a), Ok(b)) => {
                prop_assert_eq!(&a.circuit, &b.circuit);
                prop_assert_eq!(&a.net_nodes, &b.net_nodes);
                prop_assert_eq!(&a.elements, &b.elements);
                prop_assert!(a.circuit.validate().is_ok());
                let kinds = s.components.iter().filter(|c| c.part.kind() != PartKind::Ground).count();
                prop_assert_eq!(a.circuit.elements.len(), kinds);
            }
            (a, b) => prop_assert_eq!(a.err(), b.err()),
        }
    }
}
