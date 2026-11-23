//! Connectivity rules: which drawn objects form one net, net names and
//! junction dots (see the `netlist` module docs).

mod common;

use common::{P, Sketch, rc_low_pass_schematic};
use xsim_project::{Connectivity, Id, Point, connectivity};

/// Net index of a pin, which every pin has.
fn net_of(c: &Connectivity, component: Id, pin: usize) -> usize {
    c.net_of_pin(component, pin).expect("every pin is on a net")
}

fn names(c: &Connectivity) -> Vec<&str> {
    c.nets.iter().map(|n| n.name.as_str()).collect()
}

#[test]
fn a_wire_chain_joins_the_pins_it_touches() {
    let mut s = Sketch::new();
    let r1 = s.resistor((0, 0), 0); // pins (−2, 0), (2, 0)
    let r2 = s.resistor((6, 0), 0); // pins (4, 0), (8, 0)
    let w = s.wire(&[(2, 0), (3, 0), (4, 0)]);
    let c = connectivity(&s.schematic);

    assert_eq!(c.nets.len(), 3);
    let middle = net_of(&c, r1, 1);
    assert_eq!(net_of(&c, r2, 0), middle);
    assert_ne!(net_of(&c, r1, 0), middle);
    assert_ne!(net_of(&c, r2, 1), middle);
    let net = &c.nets[middle];
    assert_eq!(net.wires, [w]);
    assert_eq!(net.pins, [(r1, 1), (r2, 0)]);
    assert_eq!(net.points, [P(2, 0), P(3, 0), P(4, 0)]);
    assert!(!net.is_ground);
    assert_eq!(c.net_of_wire(w), Some(middle));
    // Nets are ordered and auto-named by their smallest point.
    assert_eq!(names(&c), ["N001", "N002", "N003"]);
    assert_eq!(c.nets[0].points, [P(-2, 0)]);
    assert!(c.junctions.is_empty());
}

#[test]
fn coincident_pins_connect_without_a_wire() {
    let mut s = Sketch::new();
    let r1 = s.resistor((0, 0), 0);
    let r2 = s.resistor((4, 0), 0); // pin 0 at (2, 0) = r1 pin 1
    let c = connectivity(&s.schematic);
    assert_eq!(net_of(&c, r1, 1), net_of(&c, r2, 0));
    assert_eq!(c.nets.len(), 3);
    assert!(c.junctions.is_empty());
}

#[test]
fn a_wire_end_on_a_segment_interior_is_a_t_junction() {
    let mut s = Sketch::new();
    let left = s.resistor((-2, 0), 0); // pin 1 at (0, 0)
    let right = s.resistor((12, 0), 0); // pin 0 at (10, 0)
    let below = s.resistor((5, 7), 1); // pin 0 at (5, 5)
    s.wire(&[(0, 0), (10, 0)]);
    let stub = s.wire(&[(5, 5), (5, 0)]);
    let c = connectivity(&s.schematic);
    let net = net_of(&c, left, 1);
    assert_eq!(net_of(&c, right, 0), net);
    assert_eq!(net_of(&c, below, 0), net);
    assert_eq!(c.net_of_wire(stub), Some(net));
    assert_eq!(c.junctions, [P(5, 0)]);
    // The T-junction point is a point of the net.
    assert!(c.nets[net].points.contains(&P(5, 0)));
}

#[test]
fn diagonal_segments_form_t_junctions_only_at_exact_lattice_points() {
    let mut s = Sketch::new();
    let a = s.resistor((-2, 0), 0); // pin 1 at (0, 0)
    let b = s.resistor((8, 3), 0); // pin 0 at (6, 3)
    let on = s.resistor((2, 5), 1); // pin 0 at (2, 3)
    let off = s.resistor((1, 3), 1); // pin 0 at (1, 1): not on the diagonal
    s.wire(&[(0, 0), (6, 3)]);
    s.wire(&[(2, 3), (2, 1)]); // (2, 1) lies on the diagonal
    s.wire(&[(1, 1), (1, 0)]); // (1, 0.5) would be needed: no contact
    let c = connectivity(&s.schematic);
    let diagonal = net_of(&c, a, 1);
    assert_eq!(net_of(&c, b, 0), diagonal);
    assert_eq!(net_of(&c, on, 0), diagonal);
    assert_ne!(net_of(&c, off, 0), diagonal);
    assert_eq!(c.junctions, [P(2, 1)]);
}

#[test]
fn crossing_wires_do_not_connect() {
    let mut s = Sketch::new();
    let h = s.resistor((-7, 0), 0); // pin 1 at (−5, 0)
    let v = s.resistor((0, -7), 1); // pin 1 at (0, −5)
    s.wire(&[(-5, 0), (5, 0)]);
    s.wire(&[(0, -5), (0, 5)]);
    let c = connectivity(&s.schematic);
    assert_ne!(net_of(&c, h, 1), net_of(&c, v, 1));
    assert!(c.junctions.is_empty());
    // On either wire alone the net is clear; at the crossing it is not.
    assert_eq!(c.net_at(P(3, 0)), Some(net_of(&c, h, 1)));
    assert_eq!(c.net_at(P(0, 3)), Some(net_of(&c, v, 1)));
    assert_eq!(c.net_at(P(0, 0)), None);
}

#[test]
fn a_wire_bend_on_another_segment_does_not_connect() {
    let mut s = Sketch::new();
    let line = s.resistor((0, -7), 1); // pin 1 at (0, −5)
    let vee = s.resistor((-7, -5), 0); // pin 1 at (−5, −5)
    s.wire(&[(0, -5), (0, 5)]);
    // A "V" whose bend touches the vertical line at (0, 0).
    s.wire(&[(-5, -5), (0, 0), (5, -5)]);
    let c = connectivity(&s.schematic);
    assert_ne!(net_of(&c, line, 1), net_of(&c, vee, 1));
    assert!(c.junctions.is_empty());
    // The bend is a connection point of the V's net.
    assert_eq!(c.net_at(P(0, 0)), Some(net_of(&c, vee, 1)));
}

#[test]
fn a_pin_on_a_segment_interior_connects() {
    let mut s = Sketch::new();
    let a = s.resistor((-2, 0), 0); // pin 1 at (0, 0)
    let tap = s.resistor((5, 2), 1); // pin 0 at (5, 0), on the wire
    s.wire(&[(0, 0), (10, 0)]);
    let c = connectivity(&s.schematic);
    assert_eq!(net_of(&c, tap, 0), net_of(&c, a, 1));
    assert_eq!(c.junctions, [P(5, 0)]);
}

#[test]
fn labels_with_the_same_name_join_distant_nets() {
    let mut s = Sketch::new();
    let a = s.resistor((0, 0), 0);
    let b = s.resistor((100, 50), 0);
    let other = s.resistor((0, 20), 0);
    s.label("bus", (2, 0)); // on a's pin 1
    s.label("bus", (98, 50)); // on b's pin 0
    s.label("Bus", (2, 20)); // names are case-sensitive
    let c = connectivity(&s.schematic);
    let bus = net_of(&c, a, 1);
    assert_eq!(net_of(&c, b, 0), bus);
    assert_eq!(c.nets[bus].name, "bus");
    assert_eq!(c.nets[bus].points, [P(2, 0), P(98, 50)]);
    let other_net = net_of(&c, other, 1);
    assert_ne!(other_net, bus);
    assert_eq!(c.nets[other_net].name, "Bus");
}

#[test]
fn a_net_with_several_labels_takes_the_alphabetically_first() {
    let mut s = Sketch::new();
    let r = s.resistor((0, 0), 0);
    s.wire(&[(2, 0), (6, 0)]);
    s.label("zeta", (2, 0));
    s.label("alpha", (6, 0));
    s.label("mid", (4, 0)); // a label on a segment interior joins the wire
    let c = connectivity(&s.schematic);
    let net = net_of(&c, r, 1);
    assert_eq!(c.nets[net].name, "alpha");
    assert_eq!(c.nets[net].points, [P(2, 0), P(4, 0), P(6, 0)]);
}

#[test]
fn all_ground_symbols_form_one_ground_net() {
    let mut s = Sketch::new();
    let r1 = s.resistor((2, 0), 0); // pins (0, 0), (4, 0)
    let r2 = s.resistor((20, 0), 0); // pins (18, 0), (22, 0)
    let g1 = s.ground((0, 0));
    let g2 = s.ground((22, 0));
    s.label("named", (0, 0));
    let c = connectivity(&s.schematic);
    let ground = net_of(&c, r1, 0);
    assert_eq!(net_of(&c, r2, 1), ground);
    let net = &c.nets[ground];
    assert!(net.is_ground);
    // Ground wins over any label.
    assert_eq!(net.name, "0");
    assert_eq!(net.pins, [(r1, 0), (r2, 1), (g1, 0), (g2, 0)]);
    assert_eq!(c.nets.iter().filter(|n| n.is_ground).count(), 1);
}

#[test]
fn a_label_named_0_is_ground() {
    let mut s = Sketch::new();
    let r = s.resistor((0, 0), 0);
    s.label("0", (2, 0));
    let c = connectivity(&s.schematic);
    let net = &c.nets[net_of(&c, r, 1)];
    assert!(net.is_ground);
    assert_eq!(net.name, "0");
}

#[test]
fn automatic_names_skip_names_taken_by_labels() {
    let mut s = Sketch::new();
    let r = s.resistor((0, 0), 0);
    s.label("N001", (2, 0));
    let c = connectivity(&s.schematic);
    // Pin 0 at (−2, 0) is the smallest point but N001 is taken.
    assert_eq!(c.nets[net_of(&c, r, 0)].name, "N002");
    assert_eq!(c.nets[net_of(&c, r, 1)].name, "N001");
}

#[test]
fn net_names_do_not_depend_on_object_order() {
    let schematic = rc_low_pass_schematic();
    let mut shuffled = schematic.clone();
    shuffled.components.reverse();
    shuffled.wires.reverse();
    shuffled.labels.reverse();
    for wire in &mut shuffled.wires {
        wire.points.reverse();
    }
    assert_eq!(connectivity(&schematic), connectivity(&shuffled));
}

#[test]
fn junction_dots_appear_exactly_where_three_branches_meet() {
    let mut s = Sketch::new();
    // An anchor pin makes every group below a net; dots do not depend on it.
    s.resistor((-2, 0), 0); // pin 1 at (0, 0)
    // Corner (2 branches): no dot.
    s.wire(&[(0, 0), (10, 0), (10, 10)]);
    // T at a segment interior: dot at (5, 0).
    s.wire(&[(5, 0), (5, -5)]);
    // Straight continuation of two wire ends (2 branches): no dot.
    s.wire(&[(10, 10), (10, 20)]);
    // Three wire ends meeting: dot at (20, 20).
    s.wire(&[(10, 20), (20, 20)]);
    s.wire(&[(20, 20), (30, 20)]);
    s.wire(&[(20, 20), (20, 30)]);
    // A pin met by two wire ends: dot at (40, 0).
    s.resistor((42, 0), 0); // pin 0 at (40, 0)
    s.wire(&[(30, 20), (40, 20), (40, 0)]);
    s.wire(&[(40, 0), (40, -10)]);
    // A pin met by one wire end (2 branches): no dot at (44, 0).
    s.wire(&[(44, 0), (50, 0)]);
    // Four ends meeting: dot at (60, 0).
    for end in [(55, 0), (65, 0), (60, -5), (60, 5)] {
        s.wire(&[(60, 0), end]);
    }
    s.wire(&[(50, 0), (55, 0)]);
    // A crossing without a vertex: no dot at (60, 10).
    s.wire(&[(55, 10), (65, 10)]);
    s.wire(&[(60, 5), (60, 15)]);
    // A wire passing through a pin via a bend (3 branches): dot at (70, 0).
    s.resistor((72, 0), 0); // pin 0 at (70, 0)
    s.wire(&[(65, 0), (70, 0), (70, 5)]);
    // Labels add no branch: none at (5, -5).
    s.label("tap", (5, -5));
    let c = connectivity(&s.schematic);
    assert_eq!(
        c.junctions,
        [P(5, 0), P(20, 20), P(40, 0), P(60, 0), P(70, 0)]
    );
}

#[test]
fn stray_wires_and_lone_labels_belong_to_no_net() {
    let mut s = Sketch::new();
    let r = s.resistor((0, 0), 0);
    let stray = s.wire(&[(10, 10), (20, 10)]);
    s.wire(&[(15, 10), (15, 15)]); // a T on the stray wire still gets a dot
    s.label("lonely", (30, 30));
    s.label("pair", (40, 40));
    s.label("pair", (50, 50));
    let c = connectivity(&s.schematic);
    assert_eq!(c.nets.len(), 2);
    assert!(c.nets.iter().all(|n| !n.pins.is_empty()));
    assert_eq!(c.net_of_wire(stray), None);
    assert_eq!(c.net_at(P(12, 10)), None);
    assert_eq!(c.net_at(P(30, 30)), None);
    assert_eq!(c.junctions, [P(15, 10)]);
    assert!(c.net_of_pin(r, 0).is_some());
}

#[test]
fn net_at_finds_pins_vertices_labels_and_segment_points() {
    let schematic = rc_low_pass_schematic();
    let c = connectivity(&schematic);
    let name_at = |x, y| c.net_at(P(x, y)).map(|n| c.nets[n].name.as_str());
    assert_eq!(name_at(0, 2), Some("in")); // V1 + pin
    assert_eq!(name_at(0, 0), Some("in")); // bend and label
    assert_eq!(name_at(2, 0), Some("in")); // segment interior
    assert_eq!(name_at(10, 0), Some("out"));
    assert_eq!(name_at(12, 1), Some("out"));
    assert_eq!(name_at(6, 8), Some("0")); // ground pin on the rail
    assert_eq!(name_at(3, 8), Some("0"));
    assert_eq!(name_at(6, 0), None); // inside R1, between its pins
    assert_eq!(name_at(3, 3), None);
}

#[test]
fn example_schematic_has_the_expected_nets() {
    let schematic = rc_low_pass_schematic();
    let c = connectivity(&schematic);
    // Ordered by smallest point: (0, 0), (0, 6), (8, 0).
    assert_eq!(names(&c), ["in", "0", "out"]);
    assert_eq!(c.junctions, [P(6, 8)]);
    let ground = &c.nets[1];
    assert_eq!(
        ground.points,
        [P(0, 6), P(0, 8), P(6, 8), P(12, 6), P(12, 8)]
    );
}

#[test]
fn empty_schematic_has_no_nets() {
    let c = connectivity(&Default::default());
    assert!(c.nets.is_empty());
    assert!(c.junctions.is_empty());
    assert_eq!(c.net_at(Point::default()), None);
}
