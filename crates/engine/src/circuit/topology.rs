//! Connectivity of a circuit: which nodes have a conducting path to ground
//! and which voltage sources (and, at DC, inductors) form loops.
//!
//! The operating point (D15) and the singular-matrix diagnostics use it:
//!
//! * A group of nodes without a DC path to ground that is reached only
//!   through capacitors (the middle of series capacitors) is tied to ground
//!   through gmin for operating-point solves, with a warning.
//! * A group fed by a current source, or connected to nothing that conducts,
//!   has no operating point; the error names a node and the reason.
//! * A loop of voltage sources and inductors makes the DC equations singular;
//!   the error names the element that closes the loop.
//!
//! Conducting means: resistors, inductors, voltage sources, switch outputs
//! (both terminals of a current switch, whose control is another element),
//! and every semiconductor device between all of its terminals (junction
//! gmin). Capacitors conduct only in a transient step, or at the transient's
//! starting operating point when they carry an initial voltage (D16).
//! Current sources and switch control inputs never conduct.
//!
//! An op-amp (D26) drives its output from a ground-referenced internal
//! stage, so its output conducts to ground. A macro-model's inputs conduct
//! to each other through `rin`; an ideal op-amp's inputs draw no current,
//! like a switch's control input. The supply pins carry the output current
//! only, so they need a path of their own (normally the supply sources).

use super::{Circuit, Device, GROUND, NodeIndex};

/// Which solve the connectivity describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Regime {
    /// Operating point, DC sweep point and AC linearisation point:
    /// capacitors open, inductors shorted.
    Dc,
    /// The transient's starting operating point: as [`Regime::Dc`], but
    /// initial conditions apply (D16), so a capacitor with an initial
    /// voltage conducts and an inductor with an initial current is not part
    /// of a short-circuit loop.
    TransientStart,
    /// A transient timestep: capacitors and inductors conduct.
    Transient,
}

/// Why a group of nodes has no conducting path to ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Isolation {
    /// Reached only through capacitors (tied through gmin at DC, D15).
    Capacitive,
    /// Fed by these current sources (element indices), whose current has no
    /// return path.
    CurrentSource(Vec<usize>),
    /// Nothing connects the group to the rest of the circuit, apart from
    /// these switches' control inputs (element indices), which draw no
    /// current.
    Unconnected(Vec<usize>),
}

/// Circuit nodes sharing one conducting island without a path to ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FloatingGroup {
    /// Circuit nodes in ascending order; never ground.
    pub(crate) nodes: Vec<NodeIndex>,
    pub(crate) isolation: Isolation,
}

/// A loop of voltage sources (and, at DC, inductors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLoop {
    /// The element (index) that closes the loop: the last one of the loop
    /// in element order.
    pub(crate) closing: usize,
    /// The loop's other elements (indices), in path order.
    pub(crate) others: Vec<usize>,
}

/// Result of [`Circuit::connectivity`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Connectivity {
    pub(crate) floating: Vec<FloatingGroup>,
    pub(crate) loops: Vec<SourceLoop>,
}

impl Connectivity {
    /// The floating group containing circuit node `node`.
    pub(crate) fn group_of(&self, node: NodeIndex) -> Option<&FloatingGroup> {
        self.floating.iter().find(|g| g.nodes.contains(&node))
    }

    /// Nodes tied to ground through gmin at DC (D15), ascending.
    pub(crate) fn tied_nodes(&self) -> Vec<NodeIndex> {
        let mut nodes: Vec<NodeIndex> = self
            .floating
            .iter()
            .filter(|g| g.isolation == Isolation::Capacitive)
            .flat_map(|g| g.nodes.iter().copied())
            .collect();
        nodes.sort_unstable();
        nodes
    }
}

/// How one element takes part in the connectivity of a regime.
struct Role<'a> {
    /// Terminals joined by a conducting path.
    conducting: &'a [NodeIndex],
    /// Part of a short-circuit loop check (two terminals).
    loop_member: bool,
    /// A current source (two terminals).
    injector: bool,
    /// An open capacitor (two terminals).
    coupler: bool,
    /// Switch control inputs (no current).
    sensed: &'a [NodeIndex],
    /// Terminals joined to ground (an op-amp's output).
    grounded: &'a [NodeIndex],
}

fn role<'a>(device: &Device, nodes: &'a [NodeIndex], regime: Regime) -> Role<'a> {
    let mut role = Role {
        conducting: nodes,
        loop_member: false,
        injector: false,
        coupler: false,
        sensed: &[],
        grounded: &[],
    };
    match device {
        Device::Capacitor {
            initial_voltage, ..
        } => {
            let conducts = match regime {
                Regime::Dc => false,
                Regime::TransientStart => initial_voltage.is_some(),
                Regime::Transient => true,
            };
            if !conducts {
                role.conducting = &[];
                role.coupler = true;
            }
        }
        Device::Inductor {
            initial_current, ..
        } => {
            role.loop_member = match regime {
                Regime::Dc => true,
                Regime::TransientStart => initial_current.is_none(),
                Regime::Transient => false,
            };
        }
        Device::VoltageSource(_) => role.loop_member = true,
        Device::CurrentSource(_) => {
            role.conducting = &[];
            role.injector = true;
        }
        Device::VoltageSwitch { .. } => {
            role.conducting = &nodes[..2.min(nodes.len())];
            role.sensed = &nodes[2.min(nodes.len())..];
        }
        Device::OpAmp { model } if nodes.len() == 5 => {
            if model.kind == crate::models::OpAmpKind::Ideal {
                role.conducting = &[];
                role.sensed = &nodes[..2];
            } else {
                role.conducting = &nodes[..2];
            }
            role.grounded = &nodes[2..3];
        }
        // Resistors, semiconductors (junction gmin joins every terminal)
        // and any other device conduct between all their terminals.
        _ => {}
    }
    role
}

/// Disjoint sets over node indices.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut a: usize) -> usize {
        while self.parent[a] != a {
            self.parent[a] = self.parent[self.parent[a]];
            a = self.parent[a];
        }
        a
    }

    /// Joins the sets of `a` and `b`; false when they were already one.
    fn union(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        // The smaller index becomes the root, so ground stays its own root.
        let (low, high) = if ra < rb { (ra, rb) } else { (rb, ra) };
        self.parent[high] = low;
        true
    }
}

impl Circuit {
    /// Conducting islands and source loops of the circuit for `regime`.
    /// Expects a validated circuit (node indices in range).
    pub(crate) fn connectivity(&self, regime: Regime) -> Connectivity {
        let n = self.nodes.len();
        let in_range = |nodes: &[NodeIndex]| nodes.iter().all(|&k| k < n);
        let mut sets = UnionFind::new(n);
        for e in self.elements.iter().filter(|e| in_range(&e.nodes)) {
            let role = role(&e.device, &e.nodes, regime);
            if let Some((&first, rest)) = role.conducting.split_first() {
                for &other in rest {
                    sets.union(first, other);
                }
            }
            for &node in role.grounded {
                sets.union(GROUND, node);
            }
        }
        let ground = sets.find(GROUND);
        let root: Vec<usize> = (0..n).map(|k| sets.find(k)).collect();

        let mut floating: Vec<FloatingGroup> = Vec::new();
        let mut group_of_root = vec![usize::MAX; n];
        for (node, &r) in root.iter().enumerate().skip(1) {
            if r == ground {
                continue;
            }
            if group_of_root[r] == usize::MAX {
                group_of_root[r] = floating.len();
                floating.push(FloatingGroup {
                    nodes: Vec::new(),
                    isolation: Isolation::Capacitive,
                });
            }
            floating[group_of_root[r]].nodes.push(node);
        }

        let mut injectors = vec![Vec::new(); floating.len()];
        let mut coupled = vec![false; floating.len()];
        let mut sensors = vec![Vec::new(); floating.len()];
        for (index, e) in self
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| in_range(&e.nodes))
        {
            let role = role(&e.device, &e.nodes, regime);
            if (role.injector || role.coupler) && e.nodes.len() == 2 {
                let (ra, rb) = (root[e.nodes[0]], root[e.nodes[1]]);
                if ra != rb {
                    for r in [ra, rb] {
                        let g = group_of_root[r];
                        if r != ground && g != usize::MAX {
                            if role.injector {
                                injectors[g].push(index);
                            } else {
                                coupled[g] = true;
                            }
                        }
                    }
                }
            }
            for &node in role.sensed {
                let g = group_of_root[root[node]];
                if root[node] != ground && g != usize::MAX && !sensors[g].contains(&index) {
                    sensors[g].push(index);
                }
            }
        }
        for (g, group) in floating.iter_mut().enumerate() {
            group.isolation = if !injectors[g].is_empty() {
                Isolation::CurrentSource(std::mem::take(&mut injectors[g]))
            } else if coupled[g] {
                Isolation::Capacitive
            } else {
                Isolation::Unconnected(std::mem::take(&mut sensors[g]))
            };
        }

        Connectivity {
            floating,
            loops: self.source_loops(regime),
        }
    }

    /// Loops of loop-member elements, each reported once by the element that
    /// closes it.
    fn source_loops(&self, regime: Regime) -> Vec<SourceLoop> {
        let n = self.nodes.len();
        let mut sets = UnionFind::new(n);
        // Forest edges per node: (neighbour, element).
        let mut forest: Vec<Vec<(NodeIndex, usize)>> = vec![Vec::new(); n];
        let mut loops = Vec::new();
        for (index, e) in self.elements.iter().enumerate() {
            if e.nodes.len() != 2 || e.nodes.iter().any(|&k| k >= n) {
                continue;
            }
            if !role(&e.device, &e.nodes, regime).loop_member {
                continue;
            }
            let (a, b) = (e.nodes[0], e.nodes[1]);
            if sets.union(a, b) {
                forest[a].push((b, index));
                forest[b].push((a, index));
            } else {
                loops.push(SourceLoop {
                    closing: index,
                    others: forest_path(&forest, a, b),
                });
            }
        }
        loops
    }
}

/// Elements on the forest path from `from` to `to` (empty when equal).
fn forest_path(forest: &[Vec<(NodeIndex, usize)>], from: NodeIndex, to: NodeIndex) -> Vec<usize> {
    let mut previous: Vec<Option<(NodeIndex, usize)>> = vec![None; forest.len()];
    let mut visited = vec![false; forest.len()];
    let mut queue = std::collections::VecDeque::from([from]);
    visited[from] = true;
    while let Some(node) = queue.pop_front() {
        if node == to {
            break;
        }
        for &(next, element) in &forest[node] {
            if !visited[next] {
                visited[next] = true;
                previous[next] = Some((node, element));
                queue.push_back(next);
            }
        }
    }
    let mut path = Vec::new();
    let mut node = to;
    while let Some((back, element)) = previous[node] {
        path.push(element);
        node = back;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::Element;
    use crate::models::{DiodeModel, SwitchModel};
    use crate::source::Source;

    fn v(name: &str, a: NodeIndex, b: NodeIndex) -> Element {
        Element::new(name, [a, b], Device::VoltageSource(Source::dc(1.0)))
    }

    #[test]
    fn series_capacitors_leave_a_capacitive_group() {
        let mut c = Circuit::new();
        let (a, m) = (c.node("a"), c.node("m"));
        c.add(v("V1", a, GROUND));
        c.add(Element::new("C1", [a, m], Device::capacitor(1e-9)));
        c.add(Element::new("C2", [m, GROUND], Device::capacitor(1e-9)));
        let dc = c.connectivity(Regime::Dc);
        assert_eq!(
            dc.floating,
            [FloatingGroup {
                nodes: vec![m],
                isolation: Isolation::Capacitive
            }]
        );
        assert_eq!(dc.tied_nodes(), [m]);
        assert!(c.connectivity(Regime::Transient).floating.is_empty());
    }

    #[test]
    fn a_current_source_without_return_path_is_reported() {
        let mut c = Circuit::new();
        let (m, n) = (c.node("m"), c.node("n"));
        c.add(Element::new(
            "I1",
            [GROUND, m],
            Device::CurrentSource(Source::dc(1e-3)),
        ));
        c.add(Element::new("R1", [m, n], Device::resistor(1e3)));
        c.add(Element::new("C1", [n, GROUND], Device::capacitor(1e-9)));
        let dc = c.connectivity(Regime::Dc);
        assert_eq!(dc.floating.len(), 1);
        assert_eq!(dc.floating[0].nodes, [m, n]);
        assert_eq!(dc.floating[0].isolation, Isolation::CurrentSource(vec![0]));
        assert!(dc.tied_nodes().is_empty());
    }

    #[test]
    fn a_circulating_current_source_does_not_isolate() {
        // I1 drives R1 inside one capacitor-coupled island: tie, no error.
        let mut c = Circuit::new();
        let (a, m, n) = (c.node("a"), c.node("m"), c.node("n"));
        c.add(v("V1", a, GROUND));
        c.add(Element::new(
            "I1",
            [m, n],
            Device::CurrentSource(Source::dc(1e-3)),
        ));
        c.add(Element::new("R1", [m, n], Device::resistor(1e3)));
        c.add(Element::new("C1", [a, m], Device::capacitor(1e-9)));
        let dc = c.connectivity(Regime::Dc);
        assert_eq!(dc.floating[0].isolation, Isolation::Capacitive);
        assert_eq!(dc.tied_nodes(), [m, n]);
    }

    #[test]
    fn junctions_conduct_and_switch_controls_do_not() {
        let mut c = Circuit::new();
        let (a, b, ctl) = (c.node("a"), c.node("b"), c.node("ctl"));
        c.add(v("V1", a, GROUND));
        c.add(Element::new(
            "D1",
            [a, b],
            Device::diode(DiodeModel::default()),
        ));
        c.add(Element::new(
            "S1",
            [b, GROUND, ctl, GROUND],
            Device::VoltageSwitch {
                model: SwitchModel::default(),
            },
        ));
        let dc = c.connectivity(Regime::Dc);
        assert_eq!(
            dc.floating,
            [FloatingGroup {
                nodes: vec![ctl],
                isolation: Isolation::Unconnected(vec![2])
            }]
        );
    }

    #[test]
    fn initial_conditions_join_capacitors_at_the_transient_start() {
        let mut c = Circuit::new();
        let (a, m) = (c.node("a"), c.node("m"));
        c.add(v("V1", a, GROUND));
        c.add(Element::new(
            "C1",
            [a, m],
            Device::Capacitor {
                capacitance: 1e-9,
                initial_voltage: Some(0.5),
            },
        ));
        assert_eq!(c.connectivity(Regime::Dc).tied_nodes(), [m]);
        assert!(c.connectivity(Regime::TransientStart).floating.is_empty());
    }

    #[test]
    fn source_loops_name_the_closing_element_and_the_path() {
        let mut c = Circuit::new();
        let (a, b) = (c.node("a"), c.node("b"));
        c.add(v("V1", a, GROUND));
        c.add(Element::new("L1", [a, b], Device::inductor(1e-3)));
        c.add(Element::new("R1", [b, GROUND], Device::resistor(1.0)));
        c.add(v("V2", b, GROUND));
        let dc = c.connectivity(Regime::Dc);
        assert_eq!(
            dc.loops,
            [SourceLoop {
                closing: 3,
                others: vec![1, 0]
            }]
        );
        // In a transient step the inductor is no short and the loop is gone.
        assert!(c.connectivity(Regime::Transient).loops.is_empty());
        // A source across a single node is a loop by itself.
        let mut c = Circuit::new();
        let a = c.node("a");
        c.add(v("V1", a, a));
        c.add(Element::new("R1", [a, GROUND], Device::resistor(1.0)));
        assert_eq!(
            c.connectivity(Regime::Dc).loops[0].others,
            Vec::<usize>::new()
        );
    }
}
