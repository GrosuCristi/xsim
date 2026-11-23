//! Linear passive devices: resistor, capacitor, inductor.
//!
//! The capacitor integrates its charge `q = C·v` and is open in DC. The
//! inductor carries its current as a branch unknown, integrates its flux
//! `φ = L·i` and is a short in DC. Both report the integrated derivative as
//! their current (capacitor) or voltage (inductor), so recorded currents are
//! consistent with the integration method in use.
//!
//! ## Initial conditions (D16)
//! In the DC solves at the start of a transient
//! ([`LoadContext::initial_constraints`]) a capacitor that is held at an
//! initial voltage `v0` becomes a voltage source `v0` with a 1 pΩ series
//! resistance, through a branch current it allocates only when it has an
//! initial voltage. The resistance keeps a capacitor across a voltage source
//! solvable (the source wins, as in LTspice) and shifts the held voltage by
//! only `1 pΩ ×` its current. For the `t = 0` point of a start from rest, a
//! capacitor without an initial voltage is held at 0 V by a 1 TS
//! conductance, which shifts it by only its current `/ 1 TS`. An inductor
//! with an initial current `i0` (or carrying 0 A for that `t = 0` point)
//! becomes a current source `i0` with a gmin shunt.
//!
//! When other parts override a hold (a capacitor across a voltage source or
//! shorted by an inductor, an inductor in series with a current source), the
//! holding current or the shunt voltage would be enormous. The transient
//! analysis then releases that hold ([`Hold::Released`]): the capacitor's
//! series resistance becomes `1/gmin` (nearly open) and the inductor's shunt
//! 1 TS (nearly a short), so the rest of the circuit decides. Outside those
//! solves the capacitor's branch current is zero and decoupled.

use super::{
    AcContext, DeviceInstance, Hold, InitialConstraints, LoadContext, SetupContext, StateSlot,
};
use crate::circuit::NodeIndex;
use crate::mna::Unknown;
use num_complex::Complex64;

/// Series resistance (Ω) of the source that holds a capacitor at its initial
/// voltage.
const HOLD_RESISTANCE: f64 = 1e-12;
/// Conductance (S) that holds a capacitor without an initial voltage at 0 V
/// for the `t = 0` point of a start from rest.
const HOLD_CONDUCTANCE: f64 = 1.0 / HOLD_RESISTANCE;
/// Shunt conductance (S) of an inductor's initial-current source when gmin
/// is zero: LTspice's default gmin.
const DEFAULT_SHUNT: f64 = 1e-12;
/// How much stiffer an inductor's shunt is under [`Hold::Probe`].
const PROBE_FACTOR: f64 = 10.0;

/// The gmin of a DC solve, or [`DEFAULT_SHUNT`] when it is zero: the
/// conductance of a weak hold.
fn weak(ctx: &LoadContext) -> f64 {
    if ctx.gmin > 0.0 {
        ctx.gmin
    } else {
        DEFAULT_SHUNT
    }
}

#[derive(Debug)]
pub(crate) struct Resistor {
    nodes: [NodeIndex; 2],
    resistance: f64,
    terminals: [Unknown; 2],
    /// Current from the positive to the negative terminal at the last load.
    current: f64,
}

impl Resistor {
    pub(crate) fn new(nodes: &[NodeIndex], resistance: f64) -> Self {
        Self {
            nodes: [nodes[0], nodes[1]],
            resistance,
            terminals: [None; 2],
            current: 0.0,
        }
    }
}

impl DeviceInstance for Resistor {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.terminals = self.nodes.map(|n| ctx.node(n));
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [a, b] = self.terminals;
        let g = 1.0 / self.resistance;
        ctx.stamp_conductance(a, b, g);
        self.current = g * (ctx.v(a) - ctx.v(b));
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [a, b] = self.terminals;
        ctx.stamp_conductance(a, b, 1.0 / self.resistance);
    }

    fn currents(&self, out: &mut [f64]) {
        out[0] = self.current;
        out[1] = -self.current;
    }

    fn is_linear(&self) -> bool {
        true
    }
}

#[derive(Debug)]
pub(crate) struct Capacitor {
    nodes: [NodeIndex; 2],
    capacitance: f64,
    initial_voltage: Option<f64>,
    terminals: [Unknown; 2],
    /// Charge state; assigned in `setup`.
    charge: StateSlot,
    /// Branch current of the source that holds the initial voltage; only a
    /// capacitor with an initial voltage has one.
    branch: Option<usize>,
    /// `dq/dt` at the last load (zero in DC), or the holding source's current.
    current: f64,
    /// How firmly the initial voltage is held at a transient's start.
    hold: Hold,
}

impl Capacitor {
    pub(crate) fn new(nodes: &[NodeIndex], capacitance: f64, initial_voltage: Option<f64>) -> Self {
        Self {
            nodes: [nodes[0], nodes[1]],
            capacitance,
            initial_voltage,
            terminals: [None; 2],
            charge: StateSlot(0),
            branch: None,
            current: 0.0,
            hold: Hold::Firm,
        }
    }
}

impl DeviceInstance for Capacitor {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.terminals = self.nodes.map(|n| ctx.node(n));
        self.charge = ctx.state();
        if self.initial_voltage.is_some() {
            self.branch = Some(ctx.branch(""));
        }
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [a, b] = self.terminals;
        let v = ctx.v(a) - ctx.v(b);
        let (i, geq) = ctx.integrate(self.charge, self.capacitance * v, self.capacitance);
        let held = match ctx.initial_constraints() {
            InitialConstraints::Off => None,
            InitialConstraints::Given => self.initial_voltage,
            InitialConstraints::All => Some(self.initial_voltage.unwrap_or(0.0)),
        };
        let released = self.hold == Hold::Released;
        match (held, self.branch) {
            (Some(v0), Some(br)) => {
                // v(a) − v(b) − R·i = v0, with `i` flowing from `a` to `b`.
                let resistance = if released {
                    1.0 / weak(ctx)
                } else {
                    HOLD_RESISTANCE
                };
                let branch = Some(br);
                ctx.system.add(a, branch, 1.0);
                ctx.system.add(b, branch, -1.0);
                ctx.system.add(branch, a, 1.0);
                ctx.system.add(branch, b, -1.0);
                ctx.system.add(branch, branch, -resistance);
                ctx.system.add_rhs(branch, ctx.source_factor * v0);
                self.current = ctx.x[br];
            }
            (Some(_), None) => {
                // Held at 0 V (no initial voltage, start from rest).
                let g = if released {
                    weak(ctx)
                } else {
                    HOLD_CONDUCTANCE
                };
                ctx.stamp_conductance(a, b, g);
                self.current = g * v;
            }
            (None, branch) => {
                // Companion model: geq in parallel with the history current,
                // which reproduces `i` at the present voltage.
                ctx.stamp_conductance(a, b, geq);
                ctx.stamp_current(a, b, i - geq * v);
                if let Some(br) = branch {
                    ctx.system.add(Some(br), Some(br), 1.0);
                }
                self.current = i;
            }
        }
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [a, b] = self.terminals;
        ctx.stamp_capacitance(a, b, self.capacitance);
        if let Some(br) = self.branch {
            ctx.system.add(Some(br), Some(br), Complex64::new(1.0, 0.0));
        }
    }

    fn currents(&self, out: &mut [f64]) {
        out[0] = self.current;
        out[1] = -self.current;
    }

    fn is_linear(&self) -> bool {
        true
    }

    fn set_hold(&mut self, hold: Hold) {
        self.hold = hold;
    }

    fn initial_conditions(
        &self,
        states: &mut Vec<(StateSlot, f64)>,
        _unknowns: &mut Vec<(usize, f64)>,
    ) {
        let v0 = self.initial_voltage.unwrap_or(0.0);
        states.push((self.charge, self.capacitance * v0));
    }
}

#[derive(Debug)]
pub(crate) struct Inductor {
    nodes: [NodeIndex; 2],
    inductance: f64,
    initial_current: Option<f64>,
    terminals: [Unknown; 2],
    /// Branch-current unknown and flux state; assigned in `setup`.
    branch: usize,
    flux: StateSlot,
    /// Branch current at the last load.
    current: f64,
    /// How firmly the initial current is held at a transient's start.
    hold: Hold,
}

impl Inductor {
    pub(crate) fn new(nodes: &[NodeIndex], inductance: f64, initial_current: Option<f64>) -> Self {
        Self {
            nodes: [nodes[0], nodes[1]],
            inductance,
            initial_current,
            terminals: [None; 2],
            branch: 0,
            flux: StateSlot(0),
            current: 0.0,
            hold: Hold::Firm,
        }
    }
}

impl DeviceInstance for Inductor {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.terminals = self.nodes.map(|n| ctx.node(n));
        self.branch = ctx.branch("");
        self.flux = ctx.state();
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [a, b] = self.terminals;
        let br = Some(self.branch);
        let i = ctx.x[self.branch];
        let (v, req) = ctx.integrate_flux(self.flux, self.inductance * i, self.inductance);
        // KCL: the branch current leaves `a` and enters `b`.
        ctx.system.add(a, br, 1.0);
        ctx.system.add(b, br, -1.0);
        let held = match ctx.initial_constraints() {
            InitialConstraints::Off => None,
            InitialConstraints::Given => self.initial_current,
            InitialConstraints::All => Some(self.initial_current.unwrap_or(0.0)),
        };
        if let Some(i0) = held {
            // A current source i0 with a gmin shunt: i − g·(v(a) − v(b)) = i0.
            let g = match self.hold {
                Hold::Firm => weak(ctx),
                Hold::Probe => PROBE_FACTOR * weak(ctx),
                Hold::Released => HOLD_CONDUCTANCE,
            };
            ctx.system.add(br, br, 1.0);
            ctx.system.add(br, a, -g);
            ctx.system.add(br, b, g);
            ctx.system.add_rhs(br, ctx.source_factor * i0);
        } else {
            // KVL linearised in the branch current: v(a) − v(b) = v + req·(i' − i).
            ctx.system.add(br, a, 1.0);
            ctx.system.add(br, b, -1.0);
            ctx.system.add(br, br, -req);
            ctx.system.add_rhs(br, v - req * i);
        }
        self.current = i;
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [a, b] = self.terminals;
        let br = Some(self.branch);
        let one = Complex64::new(1.0, 0.0);
        ctx.system.add(a, br, one);
        ctx.system.add(b, br, -one);
        ctx.system.add(br, a, one);
        ctx.system.add(br, b, -one);
        ctx.system
            .add(br, br, Complex64::new(0.0, -ctx.omega * self.inductance));
    }

    fn currents(&self, out: &mut [f64]) {
        out[0] = self.current;
        out[1] = -self.current;
    }

    fn is_linear(&self) -> bool {
        true
    }

    fn set_hold(&mut self, hold: Hold) {
        self.hold = hold;
    }

    fn initial_conditions(
        &self,
        states: &mut Vec<(StateSlot, f64)>,
        unknowns: &mut Vec<(usize, f64)>,
    ) {
        let i0 = self.initial_current.unwrap_or(0.0);
        states.push((self.flux, self.inductance * i0));
        if self.initial_current.is_some() {
            unknowns.push((self.branch, i0));
        }
    }
}
