//! Device stamping interface.
//!
//! Every [`crate::Element`] becomes one [`DeviceInstance`]. Devices are
//! *charge/flux based*: reactive behaviour is expressed as a state quantity
//! `q(x)` (charge for capacitances, flux for inductances) with its derivative,
//! and [`LoadContext::integrate`] turns it into a companion model using the
//! active integration method. Devices therefore never know which integration
//! method runs, and the integrator estimates truncation error from the state
//! histories alone.
//!
//! ## Sign conventions
//! * `stamp_current(a, b, i)` means a current `i` flows from `a` through the
//!   device to `b` (it leaves node `a`, enters node `b`).
//! * [`DeviceInstance::currents`] reports currents flowing *into* each terminal.

mod bjt;
mod diode;
mod jfet;
mod passive;
pub(crate) mod semiconductor;
mod sources;
mod switch;

use crate::analysis::SimOptions;
use crate::circuit::{Circuit, Device, Element, NodeIndex};
use crate::integration::Integrator;
use crate::mna::{Layout, Unknown};
use crate::solver::System;
use num_complex::Complex64;

/// Handle of one integrated state quantity (charge or flux).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StateSlot(pub(crate) usize);

/// Passed to [`DeviceInstance::setup`] to allocate unknowns and state slots.
pub(crate) struct SetupContext<'a> {
    pub(crate) layout: &'a mut Layout,
    pub(crate) state_count: &'a mut usize,
    /// Name of the element being set up, used to name internal unknowns.
    pub(crate) element: &'a str,
}

impl SetupContext<'_> {
    /// Unknown of an external circuit node.
    pub(crate) fn node(&self, node: NodeIndex) -> Unknown {
        self.layout.node(node)
    }

    /// Allocates an internal node such as a series-resistance node, named
    /// `<element>:<suffix>`.
    pub(crate) fn internal_node(&mut self, suffix: &str) -> Unknown {
        Some(
            self.layout
                .add_internal_node(format!("{}:{suffix}", self.element)),
        )
    }

    /// Allocates a branch-current unknown named `I(<element>)` (or
    /// `I(<element>:<suffix>)` when `suffix` is non-empty).
    pub(crate) fn branch(&mut self, suffix: &str) -> usize {
        let name = if suffix.is_empty() {
            self.element.to_string()
        } else {
            format!("{}:{suffix}", self.element)
        };
        self.layout.add_branch(name)
    }

    /// Allocates an integrated state quantity.
    pub(crate) fn state(&mut self) -> StateSlot {
        *self.state_count += 1;
        StateSlot(*self.state_count - 1)
    }
}

/// Which kind of solution is being computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadMode {
    /// Operating point / DC sweep / transient initial point: `d/dt = 0`.
    Dc,
    /// A transient timestep.
    Transient,
}

/// Which initial conditions constrain a DC solve at the start of a transient
/// (D16). The transient analysis selects them through
/// [`Integrator::set_initial_constraints`]; devices read them with
/// [`LoadContext::initial_constraints`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InitialConstraints {
    /// None: capacitors are open and inductors shorted, as at any DC point.
    #[default]
    Off,
    /// The operating-point start: a capacitor with an initial voltage is held
    /// at it and an inductor with an initial current carries it; the rest of
    /// the circuit is solved around them.
    Given,
    /// The `t = 0` point of a start from rest: every capacitor is held at its
    /// initial voltage (0 V without one) and every inductor carries its
    /// initial current (0 A without one).
    All,
}

/// How firmly a capacitor or inductor holds its initial condition in the DC
/// solves at the start of a transient (D16), set with
/// [`DeviceInstance::set_hold`]. The transient analysis releases a hold that
/// other parts of the circuit override, so that the conflict's current (or
/// voltage) reaches neither the samples nor the starting state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Hold {
    /// Held as firmly as the solve allows.
    #[default]
    Firm,
    /// An inductor's hold with a ten times stiffer shunt, to tell a voltage
    /// the circuit puts across it from one its hold makes up.
    Probe,
    /// Hardly held: a capacitor is nearly open, an inductor nearly a short.
    Released,
}

/// Passed to [`DeviceInstance::load`] once per Newton iteration.
pub(crate) struct LoadContext<'a> {
    /// Current Newton iterate, indexed by unknown.
    pub(crate) x: &'a [f64],
    pub(crate) system: &'a mut System<f64>,
    pub(crate) integrator: &'a mut Integrator,
    pub(crate) mode: LoadMode,
    /// Simulation time (0 outside transient).
    pub(crate) time: f64,
    /// Independent-source scale factor in `0..=1` (source stepping).
    pub(crate) source_factor: f64,
    /// True when independent sources follow their transient waveforms: every
    /// transient solve, including the transient's initial operating point.
    /// False for operating-point, DC-sweep and AC analyses, which apply each
    /// source's DC value.
    pub(crate) use_waveforms: bool,
    /// Conductance to add in parallel with every pn junction (GMIN stepping
    /// raises this above `options.gmin`).
    pub(crate) gmin: f64,
    /// True on the first iteration of a fresh DC solve: nonlinear devices
    /// should evaluate at their standard initial junction voltages instead
    /// of `x` (SPICE `MODEINITJCT`).
    pub(crate) init_junctions: bool,
    /// Set by devices that limited a junction voltage this iteration; the
    /// Newton loop must not declare convergence while set.
    pub(crate) limited: bool,
    #[allow(
        dead_code,
        reason = "interface for device models that read tolerances while loading"
    )]
    pub(crate) options: &'a SimOptions,
}

impl<'a> LoadContext<'a> {
    /// A DC-mode context at `x` with neutral settings: time 0, full source
    /// strength, DC source values, `options.gmin`, no junction
    /// initialisation. Callers (analyses, device unit tests) adjust the
    /// public fields from there.
    pub(crate) fn new(
        x: &'a [f64],
        system: &'a mut System<f64>,
        integrator: &'a mut Integrator,
        options: &'a SimOptions,
    ) -> Self {
        Self {
            x,
            system,
            integrator,
            mode: LoadMode::Dc,
            time: 0.0,
            source_factor: 1.0,
            use_waveforms: false,
            gmin: options.gmin,
            init_junctions: false,
            limited: false,
            options,
        }
    }
}

impl LoadContext<'_> {
    /// The initial conditions that constrain this solve: those the transient
    /// analysis selected for a DC solve at its start (`mode` DC with
    /// waveforms), [`InitialConstraints::Off`] in every other solve.
    pub(crate) fn initial_constraints(&self) -> InitialConstraints {
        if self.mode == LoadMode::Dc && self.use_waveforms {
            self.integrator.initial_constraints()
        } else {
            InitialConstraints::Off
        }
    }

    /// Value of unknown `u` in the current iterate (0 for ground).
    #[inline]
    pub(crate) fn v(&self, u: Unknown) -> f64 {
        u.map_or(0.0, |i| self.x[i])
    }

    /// Conductance `g` between `a` and `b`.
    #[inline]
    pub(crate) fn stamp_conductance(&mut self, a: Unknown, b: Unknown, g: f64) {
        self.system.add(a, a, g);
        self.system.add(b, b, g);
        self.system.add(a, b, -g);
        self.system.add(b, a, -g);
    }

    /// Independent current `i` flowing from `a` through the device to `b`.
    #[inline]
    pub(crate) fn stamp_current(&mut self, a: Unknown, b: Unknown, i: f64) {
        self.system.add_rhs(a, -i);
        self.system.add_rhs(b, i);
    }

    /// Voltage-controlled current `gm·(v(cp) − v(cn))` flowing from `a` to `b`.
    #[inline]
    pub(crate) fn stamp_transconductance(
        &mut self,
        a: Unknown,
        b: Unknown,
        cp: Unknown,
        cn: Unknown,
        gm: f64,
    ) {
        self.system.add(a, cp, gm);
        self.system.add(a, cn, -gm);
        self.system.add(b, cp, -gm);
        self.system.add(b, cn, gm);
    }

    /// Current-controlled current `gm·x[branch]` flowing from `a` to `b`,
    /// controlled by the branch-current unknown `branch`.
    #[inline]
    pub(crate) fn stamp_current_controlled(
        &mut self,
        a: Unknown,
        b: Unknown,
        branch: Unknown,
        gm: f64,
    ) {
        self.system.add(a, branch, gm);
        self.system.add(b, branch, -gm);
    }

    /// Linearised nonlinear branch from `a` to `b`: the branch current is
    /// `i` at branch voltage `v` with conductance `g = di/dv`. Stamps the
    /// Norton companion `g` in parallel with `i − g·v`.
    #[inline]
    pub(crate) fn stamp_nonlinear(&mut self, a: Unknown, b: Unknown, i: f64, g: f64, v: f64) {
        self.stamp_conductance(a, b, g);
        self.stamp_current(a, b, i - g * v);
    }

    /// Records state `slot` with value `q` and derivative `dq_dx = c` with
    /// respect to its controlling quantity, and returns `(i, geq)`: the time
    /// derivative `dq/dt` under the active integration method and its
    /// derivative `geq = d(dq/dt)/dx`. In [`LoadMode::Dc`] it records `q`
    /// and returns `(0.0, 0.0)`.
    #[inline]
    pub(crate) fn integrate(&mut self, slot: StateSlot, q: f64, c: f64) -> (f64, f64) {
        self.integrator.integrate(self.mode, slot, q, c)
    }

    /// [`Self::integrate`] for a flux `phi` with `l = dφ/di`, whose
    /// controlling quantity is a current (an inductor's).
    #[inline]
    pub(crate) fn integrate_flux(&mut self, slot: StateSlot, phi: f64, l: f64) -> (f64, f64) {
        self.integrator.integrate_flux(self.mode, slot, phi, l)
    }
}

/// Passed to [`DeviceInstance::load_ac`]. Small-signal stamps are linearised
/// at the operating point of the most recent [`DeviceInstance::load`].
pub(crate) struct AcContext<'a> {
    pub(crate) system: &'a mut System<Complex64>,
    /// Angular frequency (rad/s).
    pub(crate) omega: f64,
}

impl AcContext<'_> {
    /// Admittance `y` between `a` and `b`.
    #[inline]
    pub(crate) fn stamp_admittance(&mut self, a: Unknown, b: Unknown, y: Complex64) {
        self.system.add(a, a, y);
        self.system.add(b, b, y);
        self.system.add(a, b, -y);
        self.system.add(b, a, -y);
    }

    /// Conductance `g` between `a` and `b`.
    #[inline]
    pub(crate) fn stamp_conductance(&mut self, a: Unknown, b: Unknown, g: f64) {
        self.stamp_admittance(a, b, Complex64::new(g, 0.0));
    }

    /// Capacitance `c` between `a` and `b` (admittance `jωc`).
    #[inline]
    pub(crate) fn stamp_capacitance(&mut self, a: Unknown, b: Unknown, c: f64) {
        self.stamp_admittance(a, b, Complex64::new(0.0, self.omega * c));
    }

    /// Transadmittance `y·(v(cp) − v(cn))` flowing from `a` to `b`.
    #[inline]
    pub(crate) fn stamp_transadmittance(
        &mut self,
        a: Unknown,
        b: Unknown,
        cp: Unknown,
        cn: Unknown,
        y: Complex64,
    ) {
        self.system.add(a, cp, y);
        self.system.add(a, cn, -y);
        self.system.add(b, cp, -y);
        self.system.add(b, cn, y);
    }

    /// Small-signal current `y·x[branch]` flowing from `a` to `b`,
    /// controlled by the branch-current unknown `branch`.
    #[inline]
    pub(crate) fn stamp_current_controlled(
        &mut self,
        a: Unknown,
        b: Unknown,
        branch: Unknown,
        y: Complex64,
    ) {
        self.system.add(a, branch, y);
        self.system.add(b, branch, -y);
    }

    /// Small-signal independent current `i` flowing from `a` to `b`.
    #[inline]
    pub(crate) fn stamp_current(&mut self, a: Unknown, b: Unknown, i: Complex64) {
        self.system.add_rhs(a, -i);
        self.system.add_rhs(b, i);
    }
}

/// One simulated element.
pub(crate) trait DeviceInstance: Send + std::fmt::Debug {
    /// Allocates internal nodes, branch currents and state slots. Called
    /// exactly once, before any other method.
    fn setup(&mut self, ctx: &mut SetupContext);

    /// Called once after every element's [`Self::setup`], with the
    /// branch-current unknown of each element (indexed like
    /// `circuit.elements`; `None` for elements without one). A device
    /// controlled by another element's branch current (a current-controlled
    /// switch) looks it up here, because that element may be set up after
    /// it. Other devices ignore it.
    fn link(&mut self, branches: &[Option<usize>]) {
        let _ = branches;
    }

    /// Stamps the linearised companion model at `ctx.x`. Must also store
    /// whatever [`Self::currents`] and [`Self::load_ac`] need. The analyses
    /// call `load` once more at every converged solution before reading
    /// currents, so stored values always match the accepted solution.
    fn load(&mut self, ctx: &mut LoadContext);

    /// Stamps the small-signal model linearised at the last `load`.
    fn load_ac(&self, ctx: &mut AcContext);

    /// Currents flowing into each terminal at the last `load`. `out` has one
    /// entry per terminal (`Device::terminal_count`).
    fn currents(&self, out: &mut [f64]);

    /// Small-signal currents flowing into each terminal for the AC solution
    /// `x` at angular frequency `omega`, from the model of the last `load`
    /// and with the conventions of [`Self::currents`] (for example the BJT's
    /// substrate current enters only the collector). `out` has one entry per
    /// terminal. Returns `false` when the device does not provide them; the
    /// AC analysis then derives them from the device's stamps, which is exact
    /// for a two-terminal device unless both terminals share one node.
    fn ac_currents(&self, x: &[Complex64], omega: f64, out: &mut [Complex64]) -> bool {
        let _ = (x, omega, out);
        false
    }

    /// Device-level convergence test after a Newton update (SPICE-style
    /// current prediction check). `x` is the new iterate.
    fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
        let _ = (x, options);
        true
    }

    /// True when the stamps are affine in `x` for a fixed time and timestep
    /// (no junctions, no switching). When every device of a circuit is
    /// linear, one Newton solve is exact and the confirming iteration is
    /// skipped.
    fn is_linear(&self) -> bool {
        false
    }

    /// True for devices with memory between accepted solutions or transient
    /// events (a switch's latch): [`Self::accept`] and [`Self::locate`] are
    /// called only for them.
    fn has_memory(&self) -> bool {
        false
    }

    /// Replaces the DC value of an independent source (DC sweep). Other
    /// devices are never swept and ignore it.
    fn set_dc(&mut self, value: f64) {
        let _ = value;
    }

    /// Called after every accepted solution (the operating point, each DC
    /// sweep point, each accepted transient timepoint) with that solution.
    /// Devices with memory between timepoints (switch hysteresis) commit
    /// their state here; rejected trial steps never reach `accept`. Returns
    /// `true` when the device changed discontinuously at this transient
    /// point (a switch changed state): the transient then restarts its
    /// integration there, as after a source breakpoint.
    fn accept(&mut self, x: &[f64], time: f64) -> bool {
        let _ = (x, time);
        false
    }

    /// Event location in a transient. `x` is a converged trial solution at
    /// `time`: the end of a step from the last accepted point, or an
    /// intermediate point of it (a TR-BDF2 stage, a restart step's
    /// midpoint). A device whose state changes within the step at a time it
    /// must resolve returns the time the step should end at instead (after
    /// the last accepted point, and not later than `time`); the step is
    /// retried there. A step of a few `min_step` is never shortened, and the
    /// transient does not ask where no shorter step exists (within two
    /// `min_step` of an event): the device then changes state at the end of
    /// the step in [`Self::accept`]. Devices without events return `None`.
    fn locate(&mut self, x: &[f64], time: f64, min_step: f64) -> Option<f64> {
        let _ = (x, time, min_step);
        None
    }

    /// Times in `(0, stop]` where the device's stimulus is not smooth.
    fn breakpoints(&self, stop: f64, out: &mut Vec<f64>) {
        let _ = (stop, out);
    }

    /// Sets how firmly a capacitor or inductor holds its initial condition in
    /// the DC solves at the start of a transient (D16). Other devices ignore
    /// it.
    fn set_hold(&mut self, hold: Hold) {
        let _ = hold;
    }

    /// Initial conditions for [`crate::InitialState::ZeroState`]. The
    /// transient analysis starts from an all-zero solution vector, then writes
    /// each `(slot, q)` into the integrator's state history (e.g. a capacitor
    /// with initial voltage `v0` reports `q = C·v0`) and each `(unknown, value)`
    /// into the starting solution (e.g. an inductor's branch current).
    /// Capacitors and inductors report their state even without an initial
    /// condition (zero), because the `t = 0` sample is solved separately.
    fn initial_conditions(
        &self,
        states: &mut Vec<(StateSlot, f64)>,
        unknowns: &mut Vec<(usize, f64)>,
    ) {
        let _ = (states, unknowns);
    }
}

/// Builds the instance for `element`, applying temperature-dependent model
/// adjustments for `options.temperature`. `index` is the element's position in
/// `circuit.elements`.
pub(crate) fn build(
    circuit: &Circuit,
    index: usize,
    options: &SimOptions,
) -> Box<dyn DeviceInstance> {
    let element: &Element = &circuit.elements[index];
    let nodes = &element.nodes;
    match &element.device {
        Device::Resistor { resistance } => Box::new(passive::Resistor::new(nodes, *resistance)),
        Device::Capacitor {
            capacitance,
            initial_voltage,
        } => Box::new(passive::Capacitor::new(
            nodes,
            *capacitance,
            *initial_voltage,
        )),
        Device::Inductor {
            inductance,
            initial_current,
        } => Box::new(passive::Inductor::new(nodes, *inductance, *initial_current)),
        Device::VoltageSource(source) => {
            Box::new(sources::VoltageSource::new(nodes, source.clone()))
        }
        Device::CurrentSource(source) => {
            Box::new(sources::CurrentSource::new(nodes, source.clone()))
        }
        Device::Diode { model, area } => Box::new(diode::Diode::new(nodes, model, *area, options)),
        Device::Bjt { model, area } => Box::new(bjt::Bjt::new(nodes, model, *area, options)),
        Device::Jfet { model, area } => Box::new(jfet::Jfet::new(nodes, model, *area, options)),
        Device::VoltageSwitch { model } => Box::new(switch::Switch::voltage(nodes, model)),
        Device::CurrentSwitch { control, model } => {
            // Validation guarantees the control names a voltage source; an
            // unvalidated circuit without one senses no current.
            let source = circuit
                .elements
                .iter()
                .position(|e| e.name == *control && matches!(e.device, Device::VoltageSource(_)));
            Box::new(switch::Switch::current(nodes, source, model))
        }
    }
}
