//! D22: schema 3 stores coordinates at half the schema-2 unit, so parts,
//! wires and labels snap to every dot of the editor's grid. Schema-2 (and,
//! through it, schema-1) files migrate on load by doubling every
//! coordinate, and keep their connectivity, netlist, stored result and view.
//!
//! `fixtures/schema2_mixed.xsim_proj` was written by the schema-2 code: the
//! RC low-pass plus a transistor, a diagonal wire, a mirrored part, a
//! T-junction on a pin, labels joining distant pins, both switches, a
//! stored operating point and a user-set view. The expected nets and the
//! stored fingerprint below were computed by that code too, so they are
//! the old drawing's ground truth, not this version's own output.

mod common;

use common::P;
use serde_json::Value;
use xsim_project::{
    Compatibility, Point, Project, ProjectError, SCHEMA_VERSION, SchematicView, StoredResult,
    connectivity, netlist,
};

const MIXED: &str = include_str!("fixtures/schema2_mixed.xsim_proj");

/// The fingerprint the schema-2 code stored with the fixture's result.
const SCHEMA2_FINGERPRINT: &str = "fnv1a64-68974e596cbb10bd";

fn migrated() -> Project {
    let loaded = Project::from_json(MIXED).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Migrated { from: 2 });
    loaded.project
}

/// The fixture decoded as it stands, in schema-2 units.
fn as_written() -> Project {
    serde_json::from_str(MIXED).unwrap()
}

fn doubled(p: Point) -> Point {
    Point::new(2 * p.x, 2 * p.y)
}

#[test]
fn every_coordinate_doubles_and_nothing_else_changes() {
    let mut expected = as_written();
    assert_eq!(expected.schema_version, 2);
    expected.schema_version = SCHEMA_VERSION;
    for circuit in &mut expected.circuits {
        let s = &mut circuit.schematic;
        for c in &mut s.components {
            c.position = doubled(c.position);
        }
        for w in &mut s.wires {
            for p in &mut w.points {
                *p = doubled(*p);
            }
        }
        for l in &mut s.labels {
            l.position = doubled(l.position);
        }
        circuit.view = SchematicView {
            center: [14.5, -7.0],
            zoom: 13.75,
            user_set: true,
        };
    }
    let project = migrated();
    assert_eq!(project, expected);
    assert_eq!(SCHEMA_VERSION, 3);
}

/// The nets the schema-2 code found in the fixture: (pins, wires, points in
/// schema-2 units), in its order (by smallest point).
#[allow(clippy::type_complexity)]
fn schema2_nets() -> Vec<(Vec<(u64, usize)>, Vec<u64>, Vec<(i32, i32)>)> {
    vec![
        (
            vec![(27, 1), (29, 0)],
            vec![28],
            vec![(-10, 12), (-10, 16), (6, 16)],
        ),
        (
            vec![
                (1, 1),
                (3, 1),
                (4, 0),
                (10, 2),
                (12, 0),
                (15, 1),
                (17, 0),
                (18, 1),
                (18, 3),
                (20, 0),
                (22, 0),
                (23, 1),
                (25, 0),
                (27, 0),
                (29, 1),
            ],
            vec![7, 26, 30],
            vec![
                (-6, 12),
                (0, 6),
                (0, 8),
                (6, 8),
                (10, 16),
                (12, 6),
                (12, 8),
                (12, 12),
                (14, 8),
                (14, 16),
                (18, -14),
                (19, -10),
                (22, 6),
                (32, 4),
                (40, 6),
            ],
        ),
        (
            vec![(1, 0), (2, 0), (18, 2)],
            vec![5],
            vec![(0, 0), (0, 2), (4, 0), (21, -10)],
        ),
        (
            vec![(2, 1), (3, 0), (10, 1), (18, 0), (23, 0)],
            vec![6],
            vec![(8, 0), (12, 0), (12, 2), (18, 4), (22, -14), (40, 2)],
        ),
        (vec![(10, 0), (13, 0)], vec![14], vec![(22, 2), (24, -2)]),
        (
            vec![(13, 1), (15, 0)],
            vec![16],
            vec![(28, -2), (32, -2), (32, 0)],
        ),
    ]
}

#[test]
fn the_migrated_drawing_has_the_schema2_connectivity() {
    let project = migrated();
    let c = connectivity(&project.circuits[0].schematic);
    let nets: Vec<_> = c
        .nets
        .iter()
        .map(|n| (n.pins.clone(), n.wires.clone(), n.points.clone()))
        .collect();
    let expected: Vec<_> = schema2_nets()
        .into_iter()
        .map(|(pins, wires, points)| {
            let points = points.into_iter().map(|(x, y)| P(2 * x, 2 * y)).collect();
            (pins, wires, points)
        })
        .collect();
    assert_eq!(nets, expected);
    // The ground rail's dots: the ground pin on its interior and the corner
    // the second loop joins.
    assert_eq!(c.junctions, [P(12, 16), P(24, 16)]);
}

#[test]
fn the_migrated_netlist_is_the_schema2_one_and_the_result_stays_current() {
    let project = migrated();
    let doc = &project.circuits[0];
    let n = netlist(&doc.schematic).unwrap();
    let stored = doc.result.as_ref().expect("the fixture has a result");
    assert_eq!(
        stored.circuit_fingerprint.as_deref(),
        Some(SCHEMA2_FINGERPRINT)
    );
    // The fingerprint hashes every node name, element and value: the
    // circuit is exactly the one the schema-2 code netlisted.
    assert_eq!(StoredResult::fingerprint(&n.circuit), SCHEMA2_FINGERPRINT);
    assert_eq!(stored.computed_from(&n.circuit), Some(true));
    let elements: Vec<(u64, usize)> = n.elements.iter().map(|(&id, &e)| (id, e)).collect();
    assert_eq!(
        elements,
        [
            (1, 0),
            (2, 1),
            (3, 2),
            (10, 3),
            (13, 4),
            (15, 5),
            (18, 6),
            (23, 7),
            (27, 8),
            (29, 9)
        ]
    );
    // Probing finds the same nodes at the migrated positions.
    for (x, y, node) in [(0, 0, "in"), (12, 0, "out"), (6, 8, "0"), (23, 0, "N")] {
        let name = n.node_name_at(P(2 * x, 2 * y));
        assert!(
            name.is_some_and(|name| name.starts_with(node)),
            "({x}, {y}): {name:?}"
        );
    }
}

#[test]
fn the_migrated_view_shows_the_same_picture() {
    let old = as_written().circuits[0].view;
    let new = migrated().circuits[0].view;
    assert_eq!(new.user_set, old.user_set);
    // A point's screen offset from the canvas centre is (p − centre) × zoom.
    let offset = |view: SchematicView, x: f32, y: f32| {
        [
            (x - view.center[0]) * view.zoom,
            (y - view.center[1]) * view.zoom,
        ]
    };
    let project = as_written();
    let points = project.circuits[0]
        .schematic
        .components
        .iter()
        .flat_map(|c| c.pins())
        .chain([Point::new(-1_000, 3), Point::new(7, -3)]);
    for p in points {
        let (x, y) = (p.x as f32, p.y as f32);
        assert_eq!(
            offset(new, 2.0 * x, 2.0 * y),
            offset(old, x, y),
            "{p:?} moved on screen"
        );
    }
}

#[test]
fn a_migrated_file_saves_as_schema_3_and_reopens_unchanged() {
    let project = migrated();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mixed.xsim_proj");
    project.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("{\n  \"schema_version\": 3,\n"));
    let reopened = Project::load(&path).unwrap();
    assert_eq!(reopened.compatibility, Compatibility::Current);
    assert_eq!(reopened.project, project);
}

/// A schema-2 view without a zoom (xsim always writes one) meant the
/// schema-2 default; it opens at the schema-3 default, the same picture.
#[test]
fn missing_schema2_fields_take_the_equivalent_defaults() {
    let mut value: Value = serde_json::from_str(MIXED).unwrap();
    let view = value["circuits"][0]["view"].as_object_mut().unwrap();
    view.remove("zoom");
    view.remove("center");
    let circuit = value["circuits"][0].as_object_mut().unwrap();
    let mut no_view = circuit.clone();
    no_view.remove("view");
    no_view.insert("name".into(), Value::from("No view"));
    value["circuits"]
        .as_array_mut()
        .unwrap()
        .push(no_view.into());
    let project = Project::from_json(&value.to_string()).unwrap().project;
    let default = SchematicView::default();
    assert_eq!(project.circuits[0].view.zoom, default.zoom);
    assert_eq!(project.circuits[0].view.center, [0.0, 0.0]);
    assert_eq!(project.circuits[1].view, default);
}

/// A schema-2 view centre is any finite `f32`. Beyond half that range its
/// double is not one: the file still opens, the view as far out as an
/// `f32` reaches, and nearer centres double exactly.
#[test]
fn a_far_schema2_view_centre_still_opens() {
    let mut value: Value = serde_json::from_str(MIXED).unwrap();
    value["circuits"][0]["view"]["center"] = serde_json::json!([3.0e38_f32, -1.0e38_f32]);
    let project = Project::from_json(&value.to_string()).unwrap().project;
    assert_eq!(project.circuits[0].view.center, [f32::MAX, -2.0e38]);
    // The schema-3 limit is where it saves and reopens.
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("far.xsim_proj");
    project.save(&path).unwrap();
    assert_eq!(Project::load(&path).unwrap().project, project);
}

/// The schema-3 drawing area is ±2·10⁸ units, the schema-2 area of ±10⁸
/// units twice as large: an RC low-pass spread over its corners connects,
/// netlists, saves and reopens, and one unit beyond is invalid.
#[test]
fn the_drawing_area_reaches_its_corners() {
    const E: i32 = 200_000_000;
    let mut s = common::Sketch::new();
    s.voltage(1.0, (-E, 0), 1); // pins (−E, −4) and (−E, 4)
    s.resistor((E - 4, -E), 0); // pins (E − 8, −E) and (E, −E)
    s.capacitor((E, E - 4), 1); // pins (E, E − 8) and (E, E)
    s.ground((-E, E));
    s.wire(&[(-E, -4), (-E, -E), (E - 8, -E)]);
    s.wire(&[(E, -E), (E, E - 8)]);
    s.wire(&[(E, E), (-E, E), (-E, 4)]);
    s.label("in", (-E, -E));
    s.label("out", (E, 0));
    let n = netlist(&s.schematic).unwrap();
    assert_eq!(n.circuit.nodes.len(), 3, "ground, in and out");
    for (at, node) in [(P(-E, -E), "in"), (P(E, 0), "out"), (P(0, E), "0")] {
        let name = n.node_name_at(at);
        assert_eq!(name, Some(node), "{at:?}");
    }
    let mut project = Project::new("Corners");
    project.circuits[0].schematic = s.schematic;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("corners.xsim_proj");
    project.save(&path).unwrap();
    assert_eq!(Project::load(&path).unwrap().project, project);
    let labels = &mut project.circuits[0].schematic.labels;
    labels[1].position.x = E + 1;
    match project.save(&path) {
        Err(ProjectError::Invalid(message)) => {
            assert!(message.contains("outside the drawing area"), "{message}");
            assert!(message.contains("200000000"), "{message}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn schema2_coordinates_outside_its_drawing_area_are_invalid() {
    for (pointer, coordinate) in [
        (
            "/circuits/0/schematic/components/1/position/x",
            100_000_001_i64,
        ),
        ("/circuits/0/schematic/wires/0/points/1/y", -2_000_000_000),
        ("/circuits/0/schematic/labels/0/position/y", 3_000_000_000),
    ] {
        let mut value: Value = serde_json::from_str(MIXED).unwrap();
        *value.pointer_mut(pointer).unwrap() = Value::from(coordinate);
        match Project::from_json(&value.to_string()) {
            Err(ProjectError::Invalid(message)) => {
                assert!(message.contains("outside the drawing area"), "{message}")
            }
            other => panic!("{pointer}: expected Invalid, got {other:?}"),
        }
    }
    // The schema-2 limit itself still opens, at the schema-3 limit.
    let mut value: Value = serde_json::from_str(MIXED).unwrap();
    let pointer = "/circuits/0/schematic/labels/0/position/x";
    *value.pointer_mut(pointer).unwrap() = Value::from(-100_000_000);
    let project = Project::from_json(&value.to_string()).unwrap().project;
    assert_eq!(
        project.circuits[0].schematic.labels[0].position.x,
        -200_000_000
    );
}

#[test]
fn schema1_files_migrate_through_schema2() {
    let loaded = Project::from_json(include_str!("fixtures/schema1_rc_lowpass.xsim_proj")).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Migrated { from: 1 });
    let project = loaded.project;
    assert_eq!(project.schema_version, SCHEMA_VERSION);
    let doc = &project.circuits[0];
    // The prototype's R1 sat at (6, 0) with pins at (4, 0) and (8, 0).
    let r1 = doc.schematic.components.iter().find(|c| c.name == "R1");
    assert_eq!(r1.map(|c| c.pins()), Some(vec![P(8, 0), P(16, 0)]));
    assert_eq!(doc.view, SchematicView::default());
    let n = netlist(&doc.schematic).unwrap();
    assert!(n.node_name_at(P(24, 4)).is_some(), "C1's top pin is wired");
    assert_eq!(n.circuit.nodes.len(), 3, "ground and the two RC nodes");
}
