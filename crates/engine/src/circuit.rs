//! Circuit description: nodes and elements.

mod params;
pub(crate) mod topology;

use crate::error::CircuitError;
use crate::models::{BjtModel, CurrentSwitchModel, DiodeModel, JfetModel, OpAmpModel, SwitchModel};
use crate::source::Source;
use serde::{Deserialize, Serialize};

/// Index of a circuit node. `0` is ground; other nodes are `1..node_count()`.
pub type NodeIndex = usize;

/// The ground (reference) node.
pub const GROUND: NodeIndex = 0;

/// A flat circuit. Node names are case-sensitive (Q77).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Circuit {
    /// `nodes[i]` is the name of node `i`. `nodes[0]` is always `"0"` (ground).
    pub nodes: Vec<String>,
    pub elements: Vec<Element>,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    /// An empty circuit containing only the ground node.
    pub fn new() -> Self {
        Self {
            nodes: vec!["0".to_string()],
            elements: Vec::new(),
        }
    }

    /// Returns the index of the node called `name`, creating it if needed.
    /// The name `"0"` always refers to ground.
    pub fn node(&mut self, name: &str) -> NodeIndex {
        if let Some(i) = self.nodes.iter().position(|n| n == name) {
            return i;
        }
        self.nodes.push(name.to_string());
        self.nodes.len() - 1
    }

    /// Index of an existing node, if any.
    pub fn find_node(&self, name: &str) -> Option<NodeIndex> {
        self.nodes.iter().position(|n| n == name)
    }

    /// Number of nodes including ground.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Appends an element.
    pub fn add(&mut self, element: Element) -> &mut Self {
        self.elements.push(element);
        self
    }

    /// Element by name.
    pub fn element(&self, name: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.name == name)
    }

    /// Structural checks: unique node names (only ground is called `"0"`),
    /// unique non-empty element names, correct terminal counts, node indices
    /// in range, physically meaningful parameter values, and current
    /// switches controlled by an independent voltage source of the circuit.
    pub fn validate(&self) -> Result<(), CircuitError> {
        if self.nodes.first().map(String::as_str) != Some("0") {
            return Err(CircuitError::new("node 0 must be ground and named \"0\""));
        }
        let mut node_names = std::collections::BTreeMap::new();
        for (index, name) in self.nodes.iter().enumerate() {
            if let Some(first) = node_names.insert(name.as_str(), index) {
                return Err(CircuitError::new(if name == "0" {
                    format!("node {index} is named \"0\", the name of ground")
                } else {
                    format!("nodes {first} and {index} are both named {name}")
                }));
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for e in &self.elements {
            if e.name.is_empty() {
                return Err(CircuitError::new("an element has an empty name"));
            }
            if !names.insert(e.name.as_str()) {
                return Err(CircuitError::new(format!(
                    "duplicate element name {}",
                    e.name
                )));
            }
            let expected = e.device.terminal_count();
            if e.nodes.len() != expected {
                return Err(CircuitError::new(format!(
                    "{} needs {expected} terminals but has {}",
                    e.name,
                    e.nodes.len()
                )));
            }
            if let Some(&bad) = e.nodes.iter().find(|&&n| n >= self.nodes.len()) {
                return Err(CircuitError::new(format!(
                    "{} refers to node {bad}, which does not exist",
                    e.name
                )));
            }
            e.device.validate(&e.name)?;
            if let Device::CurrentSwitch { control, .. } = &e.device {
                self.check_control(&e.name, control)?;
            }
        }
        Ok(())
    }

    /// The control of current switch `name` must name an independent voltage
    /// source of the circuit, whose branch current it senses.
    fn check_control(&self, name: &str, control: &str) -> Result<(), CircuitError> {
        let problem = if control.is_empty() {
            "no control is given".to_string()
        } else {
            match self.element(control).map(|e| &e.device) {
                Some(Device::VoltageSource(_)) => return Ok(()),
                Some(_) => format!("its control {control} is not one"),
                None => format!("its control {control} does not exist"),
            }
        };
        Err(CircuitError::new(format!(
            "{name}: a current-controlled switch senses the current through an independent \
             voltage source, but {problem}"
        )))
    }
}

/// One circuit element connected to `nodes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Unique, case-sensitive reference designator such as `R1` or `Q3`.
    pub name: String,
    /// Terminal nodes, in the order documented on each [`Device`] variant.
    pub nodes: Vec<NodeIndex>,
    pub device: Device,
}

impl Element {
    pub fn new(
        name: impl Into<String>,
        nodes: impl IntoIterator<Item = NodeIndex>,
        device: Device,
    ) -> Self {
        Self {
            name: name.into(),
            nodes: nodes.into_iter().collect(),
            device,
        }
    }
}

/// Device kinds and their instance parameters.
///
/// Terminal order and current sign conventions:
/// * Two-terminal devices: `[positive, negative]`. The recorded current
///   `I(name)` flows *into* the positive terminal, through the device, and out
///   of the negative terminal (passive sign convention). For a voltage source
///   delivering power this current is therefore negative.
/// * [`Device::Bjt`]: `[collector, base, emitter]`.
/// * [`Device::Jfet`]: `[drain, gate, source]`.
/// * [`Device::VoltageSwitch`]: `[positive, negative, control+, control-]`.
/// * [`Device::CurrentSwitch`]: `[positive, negative]`; the control is
///   another element, by name.
/// * [`Device::OpAmp`]: `[in+, in−, out, V+, V−]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Device {
    Resistor {
        resistance: f64,
    },
    Capacitor {
        capacitance: f64,
        /// Initial voltage (D16). It always applies: a transient that starts
        /// from [`crate::InitialState::ZeroState`] starts the capacitor at
        /// it, and one that starts from
        /// [`crate::InitialState::OperatingPoint`] holds the capacitor at it
        /// while solving the operating point around it (LTspice semantics).
        /// Where other parts fix the voltage across it (a voltage source, a
        /// shorting inductor), they win and the run warns.
        initial_voltage: Option<f64>,
    },
    Inductor {
        inductance: f64,
        /// Initial current (positive into the positive terminal, D16). It
        /// always applies: a start from rest starts the inductor at it, and
        /// an operating-point start makes the inductor carry it while the
        /// operating point is solved around it. Where other parts force a
        /// different current through it, they win and the run warns; where
        /// it has no path at all, the run stops with an error.
        initial_current: Option<f64>,
    },
    VoltageSource(Source),
    CurrentSource(Source),
    Diode {
        model: DiodeModel,
        /// Area factor (scales currents and capacitances).
        area: f64,
    },
    Bjt {
        model: BjtModel,
        area: f64,
    },
    Jfet {
        model: JfetModel,
        area: f64,
    },
    /// Voltage-controlled switch (Q87).
    VoltageSwitch {
        model: SwitchModel,
    },
    /// Current-controlled switch (Q87, D17), SPICE's `W`: a switch between
    /// its two terminals controlled by the branch current of the independent
    /// voltage source called `control`. That current is the source's
    /// recorded current `I(control)`: positive when it flows into the
    /// source's positive terminal, through the source, and out of its
    /// negative terminal. This is the current LTspice's `W` compares with
    /// `IT` (the current through the named source from + to −), so models
    /// and thresholds carry over without a sign change. The recorded current
    /// of the switch itself is `I(name)`, positive into its positive
    /// terminal.
    CurrentSwitch {
        control: String,
        model: CurrentSwitchModel,
    },
    /// Op-amp (D26) on `[in+, in−, out, V+, V−]`. It records one current
    /// per terminal, each flowing into that terminal: `Ip`, `In` (the
    /// inputs), `Io` (the output, negative while the op-amp sources
    /// current), `Isp` and `Isn` (the supply pins). The output current is
    /// drawn from V+ while sourcing and returned to V− while sinking, so the
    /// five currents sum to zero.
    OpAmp {
        model: OpAmpModel,
    },
}

impl Device {
    pub fn resistor(resistance: f64) -> Self {
        Self::Resistor { resistance }
    }

    pub fn capacitor(capacitance: f64) -> Self {
        Self::Capacitor {
            capacitance,
            initial_voltage: None,
        }
    }

    pub fn inductor(inductance: f64) -> Self {
        Self::Inductor {
            inductance,
            initial_current: None,
        }
    }

    pub fn diode(model: DiodeModel) -> Self {
        Self::Diode { model, area: 1.0 }
    }

    pub fn bjt(model: BjtModel) -> Self {
        Self::Bjt { model, area: 1.0 }
    }

    pub fn jfet(model: JfetModel) -> Self {
        Self::Jfet { model, area: 1.0 }
    }

    /// Number of terminals this device connects to.
    pub fn terminal_count(&self) -> usize {
        match self {
            Self::Bjt { .. } | Self::Jfet { .. } => 3,
            Self::VoltageSwitch { .. } => 4,
            Self::OpAmp { .. } => 5,
            _ => 2,
        }
    }

    /// Names of the terminal currents recorded for this device, in terminal
    /// order, without the element name. Two-terminal devices record one
    /// current (`"I"`), three-terminal devices and op-amps one per terminal.
    pub fn current_prefixes(&self) -> &'static [&'static str] {
        match self {
            Self::Bjt { .. } => &["Ic", "Ib", "Ie"],
            Self::Jfet { .. } => &["Id", "Ig", "Is"],
            Self::OpAmp { .. } => &["Ip", "In", "Io", "Isp", "Isn"],
            _ => &["I"],
        }
    }

    /// Instance and model parameter checks of the element called `name`.
    ///
    /// Model parameters that would make the device equations meaningless (a
    /// non-finite value, a zero or negative emission coefficient, junction
    /// potential, beta or switch resistance, a negative saturation current,
    /// capacitance, transit time or ohmic resistance, …) are rejected with an
    /// error naming the element, the model and the parameter, instead of
    /// being replaced by a default when the device is built.
    pub(crate) fn validate(&self, name: &str) -> Result<(), CircuitError> {
        let positive = |what: &str, v: f64| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(CircuitError::new(format!(
                    "{name}: {what} must be positive and finite, got {v}"
                )))
            }
        };
        // An initial condition beyond this magnitude is certainly a mistake
        // and would overflow the solution.
        const INITIAL_CONDITION_LIMIT: f64 = 1e30;
        let initial = |what: &str, v: Option<f64>| match v {
            Some(v) if !(v.is_finite() && v.abs() <= INITIAL_CONDITION_LIMIT) => {
                Err(CircuitError::new(format!(
                    "{name}: {what} must be finite and at most {INITIAL_CONDITION_LIMIT:e} in \
                     magnitude, got {v:e}"
                )))
            }
            _ => Ok(()),
        };
        match self {
            Self::Resistor { resistance } => positive("resistance", *resistance),
            Self::Capacitor {
                capacitance,
                initial_voltage,
            } => {
                positive("capacitance", *capacitance)?;
                initial("initial voltage", *initial_voltage)
            }
            Self::Inductor {
                inductance,
                initial_current,
            } => {
                positive("inductance", *inductance)?;
                initial("initial current", *initial_current)
            }
            Self::VoltageSource(s) | Self::CurrentSource(s) => s.validate(name),
            Self::Diode { model, area } => {
                positive("area", *area)?;
                params::diode(name, model)
            }
            Self::Bjt { model, area } => {
                positive("area", *area)?;
                params::bjt(name, model)
            }
            Self::Jfet { model, area } => {
                positive("area", *area)?;
                params::jfet(name, model)
            }
            Self::VoltageSwitch { model } => params::switch(name, model),
            Self::CurrentSwitch { model, .. } => params::current_switch(name, model),
            Self::OpAmp { model } => params::opamp(name, model),
        }
    }
}
