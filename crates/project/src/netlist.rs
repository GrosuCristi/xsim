//! Schematic connectivity and conversion to an engine circuit.
//!
//! ## Connection rules (Q50, Q52)
//! Two objects are connected when they share a grid point:
//! * a wire vertex (end or bend) and a pin, another wire's vertex, or a label;
//! * a wire *end* or a pin lying on the interior of another wire's segment
//!   (a T-junction);
//! * two labels with the same name, anywhere in the schematic.
//!
//! Following the same "shared grid point" rule, two pins at one point are
//! connected, and a label lying on the interior of a segment joins that
//! wire's net (a label names the net at its position). A wire *bend* on
//! another segment's interior does not connect: only ends, pins and labels
//! make T-junctions.
//!
//! Two wire segments that merely cross (neither has a vertex at the crossing)
//! are **not** connected. A junction dot is drawn at every point where three
//! or more connection branches meet: each pin counts one branch, each segment
//! ending at the point one, and each segment connected at a point of its
//! interior two. Labels add no branches.
//!
//! All ground pins, and every label named `0`, form one ground net.
//!
//! Only groups of connected objects that contain at least one pin are nets.
//! A stray wire or a lone label carries no current and would leave the
//! engine with an unconstrained node, so it belongs to no net (junction dots
//! on it are still reported).
//!
//! ## Net naming
//! A net touching a ground pin (or labelled `0`) is node `0`, whatever other
//! labels it has. Otherwise a net with a label takes the label's name
//! (alphabetically first if it has several). Remaining nets are named
//! `N001`, `N002`, ... ordered by their smallest connection point (`Point`
//! ordering: x then y), skipping names that labels already use, so names are
//! deterministic and a label can never merge two nets by accident. Nets are
//! listed in the same order: by smallest point.

mod convert;
mod geometry;
mod graph;
mod union_find;

use crate::schematic::{Id, Point, Schematic};
use std::collections::BTreeMap;
use std::fmt;
use xsim_spice_engine::{Circuit, NodeIndex};

/// One electrical net in the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    pub name: String,
    /// Every grid point belonging to the net: pins, wire vertices, label
    /// positions and T-junction points, sorted.
    pub points: Vec<Point>,
    /// Wires belonging to this net.
    pub wires: Vec<Id>,
    /// `(component id, pin index)` pairs on this net.
    pub pins: Vec<(Id, usize)>,
    pub is_ground: bool,
}

/// Connectivity of a schematic, independent of device models.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Connectivity {
    pub nets: Vec<Net>,
    /// Points where a junction dot is drawn, sorted.
    pub junctions: Vec<Point>,
    /// Wire segments of every net, so [`Connectivity::net_at`] can resolve
    /// points on segment interiors.
    segments: Vec<NetSegment>,
}

/// A wire segment and the net it belongs to. `start <= end`, and the list
/// is sorted, so equal drawings compare equal whatever their object order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct NetSegment {
    start: Point,
    end: Point,
    net: usize,
}

impl Connectivity {
    /// Index into `nets` of the net containing grid point `p` (a pin, wire
    /// vertex, label or any point on a wire segment).
    ///
    /// A connection point (pin, vertex, label) belongs to exactly one net.
    /// A point that only lies on segments of *different* nets — where wires
    /// cross without connecting — is ambiguous and yields `None`, as does a
    /// point on no net at all.
    pub fn net_at(&self, p: Point) -> Option<usize> {
        if let Some(net) = self
            .nets
            .iter()
            .position(|n| n.points.binary_search(&p).is_ok())
        {
            return Some(net);
        }
        let mut found = None;
        for s in self
            .segments
            .iter()
            .filter(|s| geometry::on_segment(p, s.start, s.end))
        {
            match found {
                None => found = Some(s.net),
                Some(net) if net != s.net => return None,
                Some(_) => {}
            }
        }
        found
    }

    /// Net index of a wire.
    pub fn net_of_wire(&self, wire: Id) -> Option<usize> {
        self.nets.iter().position(|n| n.wires.contains(&wire))
    }

    /// Net index of a component pin.
    pub fn net_of_pin(&self, component: Id, pin: usize) -> Option<usize> {
        self.nets
            .iter()
            .position(|n| n.pins.contains(&(component, pin)))
    }
}

/// Computes nets and junction dots.
pub fn connectivity(schematic: &Schematic) -> Connectivity {
    graph::analyse(schematic).connectivity
}

/// A schematic converted to an engine circuit.
#[derive(Debug, Clone, PartialEq)]
pub struct Netlist {
    pub circuit: Circuit,
    pub connectivity: Connectivity,
    /// Engine node of each net (`connectivity.nets[i]` → `net_nodes[i]`).
    pub net_nodes: Vec<NodeIndex>,
    /// Engine element index of each non-ground component.
    pub elements: BTreeMap<Id, usize>,
}

impl Netlist {
    /// Engine node name at grid point `p`, e.g. `"out"`, `"N002"` or `"0"`.
    pub fn node_name_at(&self, p: Point) -> Option<&str> {
        let net = self.connectivity.net_at(p)?;
        Some(&self.circuit.nodes[self.net_nodes[net]])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NetlistError {
    /// The schematic has no parts to simulate.
    Empty,
    /// No ground symbol is connected to the circuit.
    NoGround,
    /// A part references a model the library does not contain (Q44).
    UnknownModel { component: String, model: String },
    /// A part's designator is empty or holds a character other than ASCII
    /// letters, digits and `_` (see [`is_valid_part_name`]).
    InvalidName(String),
    /// Two parts share a reference designator.
    DuplicateName(String),
    /// A pin is not connected to anything.
    UnconnectedPin { component: String, pin: String },
    /// A current-controlled switch names no control, or a control that is
    /// not the designator of a voltage source in the schematic (D17).
    UnknownControl { component: String, control: String },
}

impl fmt::Display for NetlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("The schematic is empty"),
            Self::NoGround => f.write_str("Add a ground symbol: the circuit needs a 0 V reference"),
            Self::UnknownModel { component, model } => {
                write!(
                    f,
                    "{component} uses model \"{model}\", which is not in the model library"
                )
            }
            Self::InvalidName(name) if name.trim().is_empty() => {
                f.write_str("A part has no name: give every part a designator such as R1")
            }
            Self::InvalidName(name) => write!(
                f,
                "\"{name}\" is not a valid part name: use only letters, digits and _"
            ),
            Self::DuplicateName(name) => write!(f, "Two parts are named {name}"),
            Self::UnconnectedPin { component, pin } => {
                write!(f, "Pin {pin} of {component} is not connected")
            }
            Self::UnknownControl { component, control } if control.trim().is_empty() => write!(
                f,
                "{component} has no control source: name the voltage source whose current \
                 switches it"
            ),
            Self::UnknownControl { component, control } => write!(
                f,
                "{component} is controlled by the current through \"{control}\", which is not a \
                 voltage source in this circuit"
            ),
        }
    }
}

impl std::error::Error for NetlistError {}

/// Whether `name` can be a part's designator: non-empty, ASCII letters,
/// digits and `_` only. Element names become probe and trace arguments
/// such as `I(R1)` or `Ic(Q1)`, whose grammar ends a name at `,` or `)`
/// and has no quoting, so anything else could make a current unplottable.
pub fn is_valid_part_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Converts `schematic` to an engine circuit. Semiconductor models are
/// resolved from the built-in library ([`xsim_spice_engine::library`]).
/// Element names are the component designators; element order follows
/// component order in the schematic.
///
/// Engine nodes are created in order of first use by the elements' terminals,
/// as a SPICE netlist would. Errors are reported in a fixed order: `Empty`
/// (no part besides ground symbols), `InvalidName` (a non-ground part whose
/// designator fails [`is_valid_part_name`], e.g. in a file edited by hand),
/// `DuplicateName` (among non-ground parts), `UnconnectedPin`, `NoGround`
/// (no device terminal on the ground net), then `UnknownModel` or
/// `UnknownControl` — each for the first offending part in schematic order.
/// A semiconductor model must exist under its exact name with the part's
/// polarity; a current switch's control must be the exact (case-sensitive)
/// designator of a voltage source, and becomes that element's name in the
/// circuit. Switch models are the parts' own, not library models.
pub fn netlist(schematic: &Schematic) -> Result<Netlist, NetlistError> {
    convert::netlist(schematic)
}
