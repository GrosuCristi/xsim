//! Connectivity analysis: merges connection points into nets, names the nets
//! and finds junction dots, following the rules in the parent module docs.
//!
//! Every distinct grid point that carries a pin, a wire vertex or a label is
//! one element of a union-find forest. Objects at the same point therefore
//! join automatically; wires, T-junctions, ground pins and same-named labels
//! add explicit unions.

use super::geometry::on_segment_interior;
use super::union_find::DisjointSets;
use super::{Connectivity, Net, NetSegment};
use crate::schematic::{Component, PartKind, Point, Schematic};
use std::collections::{BTreeMap, BTreeSet};

/// The engine's name of the ground node. A label with this name is ground.
const GROUND_NAME: &str = "0";

/// Connectivity plus what netlisting needs beyond the public data.
pub(super) struct Topology {
    pub(super) connectivity: Connectivity,
    /// `pin_nets[c][p]` is the net of pin `p` of the component at *index*
    /// `c` of the schematic. Every pin is on a net: a net exists for every
    /// group of connection points that contains a pin.
    pub(super) pin_nets: Vec<Vec<usize>>,
}

pub(super) fn analyse(schematic: &Schematic) -> Topology {
    let points = Points::collect(schematic);
    let segments = segments(schematic, &points.wires);
    let taps = taps(&points);
    let hits = interior_hits(&points, &taps, &segments);

    let mut sets = DisjointSets::new(points.positions.len());
    join(schematic, &points, &segments, &hits, &mut sets);
    let groups = groups(schematic, &points, &mut sets);
    let names = names(&groups);

    let mut pin_nets: Vec<Vec<usize>> = points.pins.iter().map(|p| vec![0; p.len()]).collect();
    let mut root_net = BTreeMap::new();
    let mut nets = Vec::with_capacity(groups.len());
    for (index, ((root, group), name)) in groups.into_iter().zip(names).enumerate() {
        root_net.insert(root, index);
        for &(component, pin) in &group.pins {
            pin_nets[component][pin] = index;
        }
        nets.push(group.into_net(schematic, name));
    }

    let mut net_segments: Vec<NetSegment> = segments
        .iter()
        .filter_map(|s| {
            let net = *root_net.get(&sets.find(s.start_id))?;
            Some(NetSegment {
                start: s.start.min(s.end),
                end: s.start.max(s.end),
                net,
            })
        })
        .collect();
    net_segments.sort_unstable();

    Topology {
        connectivity: Connectivity {
            nets,
            junctions: junctions(&points, &segments, &hits),
            segments: net_segments,
        },
        pin_nets,
    }
}

/// Interned connection points and the point id of every object.
struct Points {
    /// Position of each point id.
    positions: Vec<Point>,
    /// `pins[c][p]`: point id of pin `p` of component `c`.
    pins: Vec<Vec<usize>>,
    /// `wires[w][v]`: point id of vertex `v` of wire `w`.
    wires: Vec<Vec<usize>>,
    /// Point id of each label.
    labels: Vec<usize>,
}

impl Points {
    fn collect(schematic: &Schematic) -> Self {
        let mut ids = BTreeMap::new();
        let mut positions = Vec::new();
        let mut intern = |p: Point| {
            *ids.entry(p).or_insert_with(|| {
                positions.push(p);
                positions.len() - 1
            })
        };
        let pins = schematic
            .components
            .iter()
            .map(|c| c.pins().into_iter().map(&mut intern).collect())
            .collect();
        let wires = schematic
            .wires
            .iter()
            .map(|w| w.points.iter().copied().map(&mut intern).collect())
            .collect();
        let labels = schematic
            .labels
            .iter()
            .map(|l| intern(l.position))
            .collect();
        Self {
            positions,
            pins,
            wires,
            labels,
        }
    }
}

/// A non-degenerate wire segment.
#[derive(Debug, Clone, Copy)]
struct Segment {
    start: Point,
    end: Point,
    start_id: usize,
    end_id: usize,
}

fn segments(schematic: &Schematic, wire_ids: &[Vec<usize>]) -> Vec<Segment> {
    schematic
        .wires
        .iter()
        .zip(wire_ids)
        .flat_map(|(wire, ids)| {
            wire.points
                .windows(2)
                .zip(ids.windows(2))
                .filter(|(p, _)| p[0] != p[1])
                .map(|(p, id)| Segment {
                    start: p[0],
                    end: p[1],
                    start_id: id[0],
                    end_id: id[1],
                })
        })
        .collect()
}

/// Points that connect to a wire segment whose interior they lie on: pins,
/// wire ends and labels. Wire bends deliberately do not.
fn taps(points: &Points) -> BTreeSet<usize> {
    let wire_ends = points
        .wires
        .iter()
        .flat_map(|ids| ids.first().into_iter().chain(ids.last()));
    points
        .pins
        .iter()
        .flatten()
        .chain(wire_ends)
        .chain(&points.labels)
        .copied()
        .collect()
}

/// `(tap point id, segment index)` for every tap on a segment's interior.
fn interior_hits(
    points: &Points,
    taps: &BTreeSet<usize>,
    segments: &[Segment],
) -> Vec<(usize, usize)> {
    taps.iter()
        .flat_map(|&tap| {
            let p = points.positions[tap];
            segments
                .iter()
                .enumerate()
                .filter(move |(_, s)| on_segment_interior(p, s.start, s.end))
                .map(move |(index, _)| (tap, index))
        })
        .collect()
}

fn join(
    schematic: &Schematic,
    points: &Points,
    segments: &[Segment],
    hits: &[(usize, usize)],
    sets: &mut DisjointSets,
) {
    // A wire conducts along its whole length.
    for ids in &points.wires {
        for pair in ids.windows(2) {
            sets.union(pair[0], pair[1]);
        }
    }
    for &(tap, segment) in hits {
        sets.union(tap, segments[segment].start_id);
    }
    // Ground is one global net, like a label named "0".
    union_all(
        sets,
        ground_pins(schematic, points).chain(labels_named(schematic, points, GROUND_NAME)),
    );
    // Labels with the same name are one net wherever they are.
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (label, &id) in schematic.labels.iter().zip(&points.labels) {
        if !label.name.is_empty() {
            by_name.entry(&label.name).or_default().push(id);
        }
    }
    for ids in by_name.into_values() {
        union_all(sets, ids);
    }
}

fn union_all(sets: &mut DisjointSets, ids: impl IntoIterator<Item = usize>) {
    let mut ids = ids.into_iter();
    if let Some(first) = ids.next() {
        for id in ids {
            sets.union(first, id);
        }
    }
}

fn ground_pins<'a>(
    schematic: &'a Schematic,
    points: &'a Points,
) -> impl Iterator<Item = usize> + 'a {
    schematic
        .components
        .iter()
        .zip(&points.pins)
        .filter(|(c, _)| is_ground(c))
        .flat_map(|(_, ids)| ids.iter().copied())
}

fn labels_named<'a>(
    schematic: &'a Schematic,
    points: &'a Points,
    name: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    schematic
        .labels
        .iter()
        .zip(&points.labels)
        .filter(move |(l, _)| l.name == name)
        .map(|(_, &id)| id)
}

fn is_ground(component: &Component) -> bool {
    component.part.kind() == PartKind::Ground
}

/// The objects of one union-find set, by schematic index.
#[derive(Debug, Default)]
struct Group<'a> {
    /// Sorted, distinct.
    points: Vec<Point>,
    /// `(component index, pin index)`.
    pins: Vec<(usize, usize)>,
    wires: Vec<usize>,
    labels: Vec<&'a str>,
    ground: bool,
}

impl Group<'_> {
    fn into_net(self, schematic: &Schematic, name: String) -> Net {
        let mut wires: Vec<_> = self.wires.iter().map(|&w| schematic.wires[w].id).collect();
        wires.sort_unstable();
        wires.dedup();
        let mut pins: Vec<_> = self
            .pins
            .iter()
            .map(|&(c, p)| (schematic.components[c].id, p))
            .collect();
        pins.sort_unstable();
        Net {
            name,
            points: self.points,
            wires,
            pins,
            is_ground: self.ground,
        }
    }
}

/// Groups that contain at least one pin, keyed by union-find root and sorted
/// by their smallest point. Pin-less groups (stray wires, lone labels) carry
/// no current and are not nets of the circuit.
fn groups<'a>(
    schematic: &'a Schematic,
    points: &Points,
    sets: &mut DisjointSets,
) -> Vec<(usize, Group<'a>)> {
    let mut groups: BTreeMap<usize, Group<'a>> = BTreeMap::new();
    for (id, &p) in points.positions.iter().enumerate() {
        groups.entry(sets.find(id)).or_default().points.push(p);
    }
    for (c, (component, ids)) in schematic.components.iter().zip(&points.pins).enumerate() {
        for (pin, &id) in ids.iter().enumerate() {
            let g = groups.entry(sets.find(id)).or_default();
            g.pins.push((c, pin));
            g.ground |= is_ground(component);
        }
    }
    for (w, ids) in points.wires.iter().enumerate() {
        if let Some(&first) = ids.first() {
            groups.entry(sets.find(first)).or_default().wires.push(w);
        }
    }
    for (label, &id) in schematic.labels.iter().zip(&points.labels) {
        let g = groups.entry(sets.find(id)).or_default();
        g.ground |= label.name == GROUND_NAME;
        if !label.name.is_empty() {
            g.labels.push(&label.name);
        }
    }
    let mut kept: Vec<_> = groups
        .into_iter()
        .filter(|(_, g)| !g.pins.is_empty())
        .collect();
    for (_, g) in &mut kept {
        g.points.sort_unstable();
    }
    // Groups have disjoint, non-empty point sets, so this order is strict.
    kept.sort_by_key(|(_, g)| g.points[0]);
    kept
}

/// Net names in group order (see the module docs of `netlist`).
fn names(groups: &[(usize, Group<'_>)]) -> Vec<String> {
    let taken: BTreeSet<&str> = groups
        .iter()
        .flat_map(|(_, g)| g.labels.iter().copied())
        .collect();
    let mut automatic = (1..)
        .map(|n| format!("N{n:03}"))
        .filter(|name| !taken.contains(name.as_str()));
    groups
        .iter()
        .map(|(_, g)| {
            if g.ground {
                GROUND_NAME.to_string()
            } else if let Some(label) = g.labels.iter().min() {
                (*label).to_string()
            } else {
                automatic
                    .next()
                    .expect("the automatic name sequence is unbounded")
            }
        })
        .collect()
}

/// Points where at least three connection branches meet. Each pin is one
/// branch, each segment ending at the point one, and each segment passing
/// through a tap (and therefore connected there) two.
fn junctions(points: &Points, segments: &[Segment], hits: &[(usize, usize)]) -> Vec<Point> {
    let mut branches = vec![0usize; points.positions.len()];
    for &id in points.pins.iter().flatten() {
        branches[id] += 1;
    }
    for s in segments {
        branches[s.start_id] += 1;
        branches[s.end_id] += 1;
    }
    for &(tap, _) in hits {
        branches[tap] += 2;
    }
    let mut dots: Vec<Point> = branches
        .iter()
        .zip(&points.positions)
        .filter(|&(&n, _)| n >= 3)
        .map(|(_, &p)| p)
        .collect();
    dots.sort_unstable();
    dots
}
