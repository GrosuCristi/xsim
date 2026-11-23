//! The editable schematic: parts, wires and net labels on an integer grid.
//!
//! Coordinates are grid units with `x` to the right and `y` downwards. Every
//! pin and wire vertex lies on a grid point.

mod summary;

use serde::{Deserialize, Serialize};
use xsim_spice_engine::{CurrentSwitchModel, Polarity, Source, SwitchModel};

/// Stable identifier of a schematic object, unique within one schematic.
pub type Id = u64;

/// A grid point.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, rhs: Point) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, rhs: Point) -> Point {
        Point::new(self.x - rhs.x, self.y - rhs.y)
    }
}

/// Placement transform. A local symbol point is first mirrored (`mirror_x`
/// negates x, `mirror_y` negates y), then rotated clockwise on screen by
/// `rotation` quarter turns (`(x, y) → (−y, x)` per turn), then translated
/// by the component position (Q53).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Orientation {
    /// Quarter turns clockwise, `0..=3`.
    pub rotation: u8,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl Orientation {
    /// Maps a symbol-local point to an offset from the component position.
    pub fn apply(self, local: Point) -> Point {
        let mut p = local;
        if self.mirror_x {
            p.x = -p.x;
        }
        if self.mirror_y {
            p.y = -p.y;
        }
        for _ in 0..(self.rotation % 4) {
            p = Point::new(-p.y, p.x);
        }
        p
    }

    /// This orientation followed by one more clockwise quarter turn.
    pub fn rotated(self) -> Self {
        Self {
            rotation: (self.rotation + 1) % 4,
            ..self
        }
    }
}

/// Part kinds, used for symbols, designator prefixes and the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PartKind {
    Resistor,
    Capacitor,
    Inductor,
    VoltageSource,
    CurrentSource,
    Diode,
    NpnBjt,
    PnpBjt,
    NJfet,
    PJfet,
    Ground,
    /// Voltage-controlled switch (D17), SPICE `S`.
    VoltageSwitch,
    /// Current-controlled switch (D17), SPICE `W`.
    CurrentSwitch,
}

impl PartKind {
    pub const ALL: [PartKind; 13] = [
        PartKind::Resistor,
        PartKind::Capacitor,
        PartKind::Inductor,
        PartKind::VoltageSource,
        PartKind::CurrentSource,
        PartKind::Diode,
        PartKind::NpnBjt,
        PartKind::PnpBjt,
        PartKind::NJfet,
        PartKind::PJfet,
        PartKind::VoltageSwitch,
        PartKind::CurrentSwitch,
        PartKind::Ground,
    ];

    /// Reference designator prefix (`R`, `C`, `L`, `V`, `I`, `D`, `Q`, `J`,
    /// `S`, `W`, `GND`).
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Resistor => "R",
            Self::Capacitor => "C",
            Self::Inductor => "L",
            Self::VoltageSource => "V",
            Self::CurrentSource => "I",
            Self::Diode => "D",
            Self::NpnBjt | Self::PnpBjt => "Q",
            Self::NJfet | Self::PJfet => "J",
            Self::VoltageSwitch => "S",
            Self::CurrentSwitch => "W",
            Self::Ground => "GND",
        }
    }

    /// Human-readable name ("Resistor", "NPN transistor", ...).
    pub fn label(self) -> &'static str {
        match self {
            Self::Resistor => "Resistor",
            Self::Capacitor => "Capacitor",
            Self::Inductor => "Inductor",
            Self::VoltageSource => "Voltage source",
            Self::CurrentSource => "Current source",
            Self::Diode => "Diode",
            Self::NpnBjt => "NPN transistor",
            Self::PnpBjt => "PNP transistor",
            Self::NJfet => "N-channel JFET",
            Self::PJfet => "P-channel JFET",
            Self::VoltageSwitch => "Voltage-controlled switch",
            Self::CurrentSwitch => "Current-controlled switch",
            Self::Ground => "Ground",
        }
    }
}

/// A part and its electrical parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Part {
    Resistor {
        resistance: f64,
    },
    Capacitor {
        capacitance: f64,
        initial_voltage: Option<f64>,
    },
    Inductor {
        inductance: f64,
        initial_current: Option<f64>,
    },
    VoltageSource {
        source: Source,
    },
    CurrentSource {
        source: Source,
    },
    /// Semiconductor parts reference a model by name in the model library.
    Diode {
        model: String,
    },
    Bjt {
        polarity: Polarity,
        model: String,
    },
    Jfet {
        polarity: Polarity,
        model: String,
    },
    /// Voltage-controlled switch (D17). Switch models are not in the model
    /// library: each switch carries its own RON / ROFF / VT / VH.
    VoltageSwitch {
        model: SwitchModel,
    },
    /// Current-controlled switch (D17), switched by the current through the
    /// independent voltage source whose designator is `control` (as SPICE's
    /// `W` names a source), with its own RON / ROFF / IT / IH. The current
    /// counts positive into the source's `+` pin, as its `I(V1)` trace does.
    CurrentSwitch {
        control: String,
        model: CurrentSwitchModel,
    },
    Ground,
}

impl Part {
    pub fn kind(&self) -> PartKind {
        match self {
            Self::Resistor { .. } => PartKind::Resistor,
            Self::Capacitor { .. } => PartKind::Capacitor,
            Self::Inductor { .. } => PartKind::Inductor,
            Self::VoltageSource { .. } => PartKind::VoltageSource,
            Self::CurrentSource { .. } => PartKind::CurrentSource,
            Self::Diode { .. } => PartKind::Diode,
            Self::Bjt {
                polarity: Polarity::N,
                ..
            } => PartKind::NpnBjt,
            Self::Bjt {
                polarity: Polarity::P,
                ..
            } => PartKind::PnpBjt,
            Self::Jfet {
                polarity: Polarity::N,
                ..
            } => PartKind::NJfet,
            Self::Jfet {
                polarity: Polarity::P,
                ..
            } => PartKind::PJfet,
            Self::VoltageSwitch { .. } => PartKind::VoltageSwitch,
            Self::CurrentSwitch { .. } => PartKind::CurrentSwitch,
            Self::Ground => PartKind::Ground,
        }
    }

    /// Default parameters for a newly placed part: 1 kΩ, 1 µF, 1 mH, 1 V DC,
    /// 1 mA DC and the library's default semiconductor models.
    ///
    /// Switches default to RON = 1 Ω and ROFF = 1 MΩ, a ratio of 10⁶ that
    /// keeps the off state far below the on state without the extreme
    /// conductance spread of SPICE's default ROFF = 1/GMIN. The voltage
    /// switch turns at VT = 0.5 V, the middle of a 0 to 1 V drive such as
    /// the default 1 V source; the current switch is controlled by `V1` and
    /// turns at IT = 1 mA, the scale of the default 1 mA current source.
    /// Both have no hysteresis (VH = IH = 0).
    pub fn default_for(kind: PartKind) -> Self {
        use xsim_spice_engine::library;
        match kind {
            PartKind::Resistor => Self::Resistor { resistance: 1e3 },
            PartKind::Capacitor => Self::Capacitor {
                capacitance: 1e-6,
                initial_voltage: None,
            },
            PartKind::Inductor => Self::Inductor {
                inductance: 1e-3,
                initial_current: None,
            },
            PartKind::VoltageSource => Self::VoltageSource {
                source: Source::dc(1.0),
            },
            PartKind::CurrentSource => Self::CurrentSource {
                source: Source::dc(1e-3),
            },
            PartKind::Diode => Self::Diode {
                model: library::default_diode().into(),
            },
            PartKind::NpnBjt => Self::Bjt {
                polarity: Polarity::N,
                model: library::default_bjt(Polarity::N).into(),
            },
            PartKind::PnpBjt => Self::Bjt {
                polarity: Polarity::P,
                model: library::default_bjt(Polarity::P).into(),
            },
            PartKind::NJfet => Self::Jfet {
                polarity: Polarity::N,
                model: library::default_jfet(Polarity::N).into(),
            },
            PartKind::PJfet => Self::Jfet {
                polarity: Polarity::P,
                model: library::default_jfet(Polarity::P).into(),
            },
            PartKind::VoltageSwitch => Self::VoltageSwitch {
                model: SwitchModel {
                    ron: 1.0,
                    roff: 1e6,
                    vt: 0.5,
                    vh: 0.0,
                    ..SwitchModel::default()
                },
            },
            PartKind::CurrentSwitch => Self::CurrentSwitch {
                control: "V1".into(),
                model: CurrentSwitchModel {
                    ron: 1.0,
                    roff: 1e6,
                    it: 1e-3,
                    ih: 0.0,
                    ..CurrentSwitchModel::default()
                },
            },
            PartKind::Ground => Self::Ground,
        }
    }

    /// Short value text shown next to the symbol, e.g. `"1 kΩ"`, `"1 µF"`,
    /// `"PULSE(0 1 …)"`, `"NPN_GP"`, `"RON=1 ROFF=1M VT=500m"` for a
    /// voltage switch and `"V1 IT=1m"` for a current switch. Empty for
    /// ground.
    pub fn summary(&self) -> String {
        summary::summary(self)
    }
}

/// A placed part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: Id,
    /// Reference designator, unique among non-ground parts (`R1`, `Q2`).
    pub name: String,
    pub position: Point,
    #[serde(default)]
    pub orientation: Orientation,
    pub part: Part,
}

impl Component {
    /// World positions of the pins, in engine terminal order.
    pub fn pins(&self) -> Vec<Point> {
        crate::symbols::geometry(self.part.kind())
            .pins
            .iter()
            .map(|&p| self.position + self.orientation.apply(p))
            .collect()
    }

    /// World-space bounding box `(min, max)` of the symbol body and pins.
    pub fn bounds(&self) -> (Point, Point) {
        let g = crate::symbols::geometry(self.part.kind());
        let a = self.position + self.orientation.apply(g.body_min);
        let b = self.position + self.orientation.apply(g.body_max);
        (
            Point::new(a.x.min(b.x), a.y.min(b.y)),
            Point::new(a.x.max(b.x), a.y.max(b.y)),
        )
    }
}

/// A polyline wire. Consecutive points form segments; segments are
/// horizontal/vertical by default but any angle is allowed (Q49).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub id: Id,
    /// At least two points; consecutive points are distinct.
    pub points: Vec<Point>,
}

impl Wire {
    /// Segments as `(start, end)` pairs.
    pub fn segments(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.points.windows(2).map(|w| (w[0], w[1]))
    }
}

/// A net label. It names the net at `position`; every label with the same
/// (case-sensitive) name in the schematic joins the same net (Q52).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetLabel {
    pub id: Id,
    pub name: String,
    pub position: Point,
}

/// One circuit drawing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schematic {
    pub components: Vec<Component>,
    pub wires: Vec<Wire>,
    pub labels: Vec<NetLabel>,
    /// Next unused [`Id`].
    pub next_id: Id,
}

impl Schematic {
    /// Returns a fresh id and advances `next_id`. Ids are never reused.
    pub fn allocate_id(&mut self) -> Id {
        let used = self
            .components
            .iter()
            .map(|c| c.id)
            .chain(self.wires.iter().map(|w| w.id))
            .chain(self.labels.iter().map(|l| l.id))
            .max()
            .map_or(0, |m| m + 1);
        let id = self.next_id.max(used).max(1);
        self.next_id = id + 1;
        id
    }

    /// The lowest unused designator for `kind`, e.g. `R3` when `R1` and `R2`
    /// exist. Ground parts are named `GND1`, `GND2`, ...
    pub fn next_designator(&self, kind: PartKind) -> String {
        let prefix = kind.prefix();
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|name| !self.components.iter().any(|c| &c.name == name))
            .expect("unbounded range")
    }

    pub fn component(&self, id: Id) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn component_mut(&mut self, id: Id) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
    }

    pub fn wire(&self, id: Id) -> Option<&Wire> {
        self.wires.iter().find(|w| w.id == id)
    }

    pub fn label(&self, id: Id) -> Option<&NetLabel> {
        self.labels.iter().find(|l| l.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.wires.is_empty() && self.labels.is_empty()
    }
}
