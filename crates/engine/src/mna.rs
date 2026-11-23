//! Unknown layout for modified nodal analysis.
//!
//! Unknowns are, in order: circuit node voltages `1..node_count` (ground is
//! eliminated), then internal device nodes and branch currents in the order
//! devices request them during setup. Every unknown has a diagnostic name
//! used in error messages, e.g. `V(out)`, `I(V1)`, `V(Q1:b')`.
//!
//! [`Mna`] holds a circuit prepared for solving: one device instance per
//! element, the layout they set up, and the real linear system with its
//! solver. Analyses drive it through [`Mna::load`] and [`Mna::solve_into`]; the
//! Newton iteration lives in [`crate::newton`].
//!
//! ## Floating nodes (D15)
//! [`Mna`] also knows the circuit's connectivity (`circuit::topology`).
//! Every operating-point load ([`LoadMode::Dc`]: the operating point, each
//! DC-sweep point, the AC linearisation point and the transient's starting
//! point) ties each node that has no DC path to ground but is reached
//! through capacitors to ground through `options.gmin`, as LTspice does;
//! [`crate::newton::operating_point`] raises the warning that names the
//! node. Transient steps and the AC system are unchanged: there the
//! capacitors connect the node. Nodes fed only by a current source or
//! connected to nothing, and loops of voltage sources and inductors, are
//! reported before solving, with the reason ([`Mna::dc_obstacle`],
//! [`Mna::singular_reason`]).

use crate::analysis::SimOptions;
use crate::circuit::topology::{Connectivity, Isolation, Regime};
use crate::circuit::{Circuit, NodeIndex};
use crate::devices::{
    self, AcContext, DeviceInstance, LoadContext, LoadMode, SetupContext, StateSlot,
};
use crate::integration::Integrator;
use crate::solver::{LinearSolver, SingularError, SolveFailure, System, solver_for};
use num_complex::Complex64;

/// Tie conductance (S) for floating capacitor nodes when `options.gmin` is
/// zero: LTspice's default gmin.
const DEFAULT_TIE: f64 = 1e-12;

/// Index of an unknown in the solution vector; `None` is ground.
pub(crate) type Unknown = Option<usize>;

#[derive(Debug, Clone)]
pub(crate) struct Layout {
    names: Vec<String>,
    /// Whether each unknown is a voltage (rather than a branch current).
    voltage: Vec<bool>,
    circuit_nodes: usize,
}

impl Layout {
    /// Layout with one unknown per non-ground circuit node.
    pub(crate) fn new(circuit: &Circuit) -> Self {
        Self {
            names: circuit.nodes[1..]
                .iter()
                .map(|n| format!("V({n})"))
                .collect(),
            voltage: vec![true; circuit.nodes.len().saturating_sub(1)],
            circuit_nodes: circuit.nodes.len(),
        }
    }

    /// Unknown of circuit node `node`.
    #[inline]
    pub(crate) fn node(&self, node: NodeIndex) -> Unknown {
        debug_assert!(node < self.circuit_nodes);
        node.checked_sub(1)
    }

    /// Allocates an internal node voltage unknown.
    pub(crate) fn add_internal_node(&mut self, name: String) -> usize {
        self.names.push(format!("V({name})"));
        self.voltage.push(true);
        self.names.len() - 1
    }

    /// Allocates a branch current unknown.
    pub(crate) fn add_branch(&mut self, name: String) -> usize {
        self.names.push(format!("I({name})"));
        self.voltage.push(false);
        self.names.len() - 1
    }

    /// Number of unknowns.
    pub(crate) fn size(&self) -> usize {
        self.names.len()
    }

    /// Number of circuit-node voltage unknowns (they come first).
    pub(crate) fn circuit_node_unknowns(&self) -> usize {
        self.circuit_nodes - 1
    }

    /// Diagnostic name of unknown `i`.
    pub(crate) fn name(&self, i: usize) -> &str {
        &self.names[i]
    }

    /// Whether unknown `i` is a voltage (as opposed to a branch current).
    #[inline]
    pub(crate) fn is_voltage(&self, i: usize) -> bool {
        self.voltage[i]
    }
}

/// Everything a single solve of the circuit equations depends on besides the
/// iterate: the kind of solve, the stimulus, and the continuation aids that
/// the DC convergence ladder adds (Q103).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Conditions {
    pub(crate) mode: LoadMode,
    /// Simulation time (0 outside transient).
    pub(crate) time: f64,
    /// See [`LoadContext::use_waveforms`].
    pub(crate) use_waveforms: bool,
    /// Independent-source scale factor (source stepping).
    pub(crate) source_factor: f64,
    /// Conductance across every pn junction.
    pub(crate) gmin: f64,
    /// Conductance from every voltage unknown to ground (GMIN stepping).
    pub(crate) node_gmin: f64,
    /// Conductance pulling every voltage unknown towards the anchor solution
    /// passed to [`Mna::load`] (pseudo-transient continuation: a capacitor
    /// to ground divided by the pseudo timestep).
    pub(crate) pseudo_conductance: f64,
}

impl Conditions {
    /// A plain DC solve: sources at their DC values (or their waveform at
    /// `t = 0` when `use_waveforms`), no continuation aids.
    pub(crate) fn dc(options: &SimOptions, use_waveforms: bool) -> Self {
        Self {
            mode: LoadMode::Dc,
            time: 0.0,
            use_waveforms,
            source_factor: 1.0,
            gmin: options.gmin,
            node_gmin: 0.0,
            pseudo_conductance: 0.0,
        }
    }

    /// A transient timestep ending at `time`.
    pub(crate) fn transient(options: &SimOptions, time: f64) -> Self {
        Self {
            mode: LoadMode::Transient,
            time,
            use_waveforms: true,
            ..Self::dc(options, true)
        }
    }

    /// The connectivity regime these conditions solve in: a DC solve with
    /// waveforms is the transient's starting operating point.
    pub(crate) fn regime(&self) -> Regime {
        match (self.mode, self.use_waveforms) {
            (LoadMode::Transient, _) => Regime::Transient,
            (LoadMode::Dc, true) => Regime::TransientStart,
            (LoadMode::Dc, false) => Regime::Dc,
        }
    }
}

/// Connectivity facts per regime, computed once from the circuit.
struct Topology {
    /// Indexed by [`regime_index`].
    connectivity: [Connectivity; 3],
    /// Unknowns tied to ground through gmin in the two DC regimes (D15).
    ties: [Vec<usize>; 2],
    tie_conductance: f64,
    node_names: Vec<String>,
    element_names: Vec<String>,
    /// Branch-current unknown of each element, if it has one.
    element_branch: Vec<Option<usize>>,
    /// First non-ground terminal of each element (ground if none).
    element_node: Vec<NodeIndex>,
}

fn regime_index(regime: Regime) -> usize {
    match regime {
        Regime::Dc => 0,
        Regime::TransientStart => 1,
        Regime::Transient => 2,
    }
}

impl Topology {
    /// `element_branch` is the branch-current unknown of each element.
    fn new(
        circuit: &Circuit,
        layout: &Layout,
        options: &SimOptions,
        element_branch: Vec<Option<usize>>,
    ) -> Self {
        let connectivity = [Regime::Dc, Regime::TransientStart, Regime::Transient]
            .map(|regime| circuit.connectivity(regime));
        let ties = [0, 1].map(|k| {
            connectivity[k]
                .tied_nodes()
                .into_iter()
                .filter_map(|node| layout.node(node))
                .collect()
        });
        Self {
            connectivity,
            ties,
            tie_conductance: if options.gmin > 0.0 {
                options.gmin
            } else {
                DEFAULT_TIE
            },
            node_names: circuit.nodes.clone(),
            element_names: circuit.elements.iter().map(|e| e.name.clone()).collect(),
            element_branch,
            element_node: circuit
                .elements
                .iter()
                .map(|e| e.nodes.iter().copied().find(|&n| n != 0).unwrap_or(0))
                .collect(),
        }
    }

    /// "A", "A and B", "A, B and C".
    fn names(&self, elements: &[usize]) -> String {
        let names: Vec<&str> = elements
            .iter()
            .map(|&e| self.element_names[e].as_str())
            .collect();
        match names.as_slice() {
            [] => String::new(),
            [one] => (*one).to_string(),
            [init @ .., last] => format!("{} and {last}", init.join(", ")),
        }
    }
}

/// Values a transient started from rest begins with (see
/// [`crate::devices::DeviceInstance::initial_conditions`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct InitialConditions {
    /// State values written into the integrator's history.
    pub(crate) states: Vec<(StateSlot, f64)>,
    /// Unknown values written into the starting solution.
    pub(crate) unknowns: Vec<(usize, f64)>,
}

/// A circuit prepared for solving: device instances, their unknowns and
/// states, the options they were built for, and the real linear system.
pub(crate) struct Mna {
    layout: Layout,
    devices: Vec<Box<dyn DeviceInstance>>,
    integrator: Integrator,
    linear: bool,
    /// Indices of the devices with memory ([`DeviceInstance::has_memory`]).
    eventful: Vec<usize>,
    options: SimOptions,
    system: System<f64>,
    solver: Box<dyn LinearSolver<f64>>,
    topology: Topology,
    /// Conditions of the last load or obstacle check (what a failure refers
    /// to).
    last_conditions: Option<Conditions>,
    /// How the last [`Self::solve_into`] failed, if it did.
    last_failure: Option<SolveFailure>,
    /// Unknown with the largest normalised update in the last Newton
    /// iteration (see [`crate::newton`]).
    worst: Option<usize>,
    /// The unknown whose estimated rounding noise most exceeded its update
    /// tolerance in the current Newton solve, and that noise (see
    /// [`crate::newton`]).
    precision_floor: Option<(usize, f64)>,
    /// Unknowns already reported as resolved only to their rounding floor.
    precision_warned: Vec<bool>,
    /// Storage of the Newton updates, kept between solves.
    iterate: Vec<f64>,
}

impl Mna {
    /// Instantiates every element of `circuit` and lays out the unknowns.
    pub(crate) fn new(circuit: &Circuit, options: &SimOptions) -> Self {
        let devices = (0..circuit.elements.len())
            .map(|index| devices::build(circuit, index, options))
            .collect();
        Self::with_devices(circuit, devices, options)
    }

    /// Like [`Self::new`] with given device instances, one per element of
    /// `circuit` in element order (tests substitute instrumented devices).
    pub(crate) fn with_devices(
        circuit: &Circuit,
        mut devices: Vec<Box<dyn DeviceInstance>>,
        options: &SimOptions,
    ) -> Self {
        debug_assert_eq!(devices.len(), circuit.elements.len());
        let mut layout = Layout::new(circuit);
        let mut state_count = 0;
        // The branch current `I(<element>)` among the unknowns each element
        // allocates, if it has one.
        let mut branches = Vec::with_capacity(devices.len());
        for (device, element) in devices.iter_mut().zip(&circuit.elements) {
            let first = layout.size();
            device.setup(&mut SetupContext {
                layout: &mut layout,
                state_count: &mut state_count,
                element: &element.name,
            });
            let name = format!("I({})", element.name);
            branches.push((first..layout.size()).find(|&i| layout.name(i) == name));
        }
        for device in &mut devices {
            device.link(&branches);
        }
        let size = layout.size();
        Self {
            linear: devices.iter().all(|d| d.is_linear()),
            eventful: (0..devices.len())
                .filter(|&i| devices[i].has_memory())
                .collect(),
            topology: Topology::new(circuit, &layout, options, branches),
            layout,
            devices,
            integrator: Integrator::new(options.method, state_count),
            options: options.clone(),
            system: System::new(size),
            solver: solver_for(options.solver, size),
            last_conditions: None,
            last_failure: None,
            worst: None,
            precision_floor: None,
            precision_warned: vec![false; size],
            iterate: Vec::with_capacity(size),
        }
    }

    pub(crate) fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Number of unknowns.
    pub(crate) fn size(&self) -> usize {
        self.layout.size()
    }

    pub(crate) fn options(&self) -> &SimOptions {
        &self.options
    }

    /// The state histories shared by all devices.
    pub(crate) fn integrator(&self) -> &Integrator {
        &self.integrator
    }

    pub(crate) fn integrator_mut(&mut self) -> &mut Integrator {
        &mut self.integrator
    }

    /// Whether every device is linear (one Newton solve is exact).
    pub(crate) fn is_linear(&self) -> bool {
        self.linear
    }

    pub(crate) fn devices_mut(&mut self) -> &mut [Box<dyn DeviceInstance>] {
        &mut self.devices
    }

    /// Assembles the system linearised at `x`. Returns whether a device
    /// limited its update, in which case the iterate cannot have converged.
    pub(crate) fn load(
        &mut self,
        x: &[f64],
        conditions: &Conditions,
        init_junctions: bool,
        anchor: &[f64],
    ) -> bool {
        self.system.clear();
        let mut ctx = LoadContext::new(x, &mut self.system, &mut self.integrator, &self.options);
        ctx.mode = conditions.mode;
        ctx.time = conditions.time;
        ctx.use_waveforms = conditions.use_waveforms;
        ctx.source_factor = conditions.source_factor;
        ctx.gmin = conditions.gmin;
        ctx.init_junctions = init_junctions;
        for device in &mut self.devices {
            device.load(&mut ctx);
        }
        let limited = ctx.limited;
        self.stamp_continuation(conditions, anchor);
        self.stamp_ties(conditions);
        self.last_conditions = Some(*conditions);
        limited
    }

    /// Ties every floating capacitor node to ground through gmin in an
    /// operating-point load (D15).
    fn stamp_ties(&mut self, conditions: &Conditions) {
        let k = match conditions.regime() {
            Regime::Dc => 0,
            Regime::TransientStart => 1,
            Regime::Transient => return,
        };
        let g = self.topology.tie_conductance;
        for &i in &self.topology.ties[k] {
            self.system.add(Some(i), Some(i), g);
        }
    }

    /// Names of the circuit nodes tied to ground through gmin under
    /// `conditions` (D15), and the tie conductance.
    pub(crate) fn tied_nodes(&self, conditions: &Conditions) -> (Vec<&str>, f64) {
        let names = match conditions.regime() {
            Regime::Transient => Vec::new(),
            regime => self
                .connectivity(regime)
                .tied_nodes()
                .into_iter()
                .map(|n| self.topology.node_names[n].as_str())
                .collect(),
        };
        (names, self.topology.tie_conductance)
    }

    fn connectivity(&self, regime: Regime) -> &Connectivity {
        &self.topology.connectivity[regime_index(regime)]
    }

    /// The unknown at which an operating-point solve under `conditions`
    /// cannot succeed whatever the iterate: a node without a DC path that
    /// is fed by a current source or connected to nothing, or the branch of
    /// the element closing a loop of voltage sources and inductors. Records
    /// the failure for [`Self::singular_reason`].
    pub(crate) fn dc_obstacle(&mut self, conditions: &Conditions) -> Option<usize> {
        let connectivity = self.connectivity(conditions.regime());
        let floating = connectivity
            .floating
            .iter()
            .filter(|g| g.isolation != Isolation::Capacitive)
            .find_map(|g| g.nodes.first().and_then(|&n| self.layout.node(n)));
        let looped = connectivity.loops.first().map(|l| {
            self.topology.element_branch[l.closing]
                .or_else(|| self.layout.node(self.topology.element_node[l.closing]))
                .unwrap_or(0)
        });
        let row = floating.or(looped)?;
        self.last_conditions = Some(*conditions);
        self.last_failure = Some(SolveFailure::Singular);
        Some(row)
    }

    /// Why the matrix is singular at unknown `row`, for `regime` (by
    /// default that of the last load): the floating node or source loop
    /// involved, or that no topological cause exists (an ill-conditioned
    /// circuit).
    pub(crate) fn singular_reason(&self, row: usize, regime: Option<Regime>) -> String {
        let regime = regime
            .or(self.last_conditions.map(|c| c.regime()))
            .unwrap_or(Regime::Dc);
        let connectivity = self.connectivity(regime);
        let transient = regime == Regime::Transient;
        if row < self.layout.circuit_node_unknowns()
            && let Some(group) = connectivity.group_of(row + 1)
        {
            let node = &self.topology.node_names[row + 1];
            let why = match &group.isolation {
                Isolation::CurrentSource(sources) => {
                    let (noun, verb) = if sources.len() == 1 {
                        ("current source", "feeds")
                    } else {
                        ("current sources", "feed")
                    };
                    format!(
                        "only {noun} {} {verb} it, so its voltage is undefined",
                        self.topology.names(sources)
                    )
                }
                Isolation::Unconnected(sensors) if !sensors.is_empty() => format!(
                    "it connects only to the control input of {}, which draws no current",
                    self.topology.names(sensors)
                ),
                Isolation::Unconnected(_) => {
                    "nothing connects it to the rest of the circuit".to_string()
                }
                Isolation::Capacitive => "it is reached only through capacitors".to_string(),
            };
            let path = if transient {
                "no path to ground, not even through capacitors"
            } else {
                "no DC path to ground"
            };
            return format!("node {node} has {path} ({why})");
        }
        for source_loop in &connectivity.loops {
            let mut members = source_loop.others.clone();
            members.push(source_loop.closing);
            if members
                .iter()
                .any(|&e| self.topology.element_branch[e] == Some(row))
            {
                let kinds = if transient {
                    "voltage sources"
                } else {
                    "voltage sources and inductors (shorts at DC)"
                };
                let names = self.topology.names(&members);
                return if members.len() == 1 {
                    format!("{names} is connected across a single node, a loop of {kinds}")
                } else {
                    format!("{names} form a loop of {kinds}, so the loop current is undefined")
                };
            }
        }
        let path = if transient {
            "a path to ground"
        } else {
            "a DC path to ground"
        };
        format!(
            "every node has {path} and there is no voltage-source loop, so the equations are \
             singular only to double precision: element values around this unknown span too \
             many decades (for example a micro-ohm resistance next to a node held only by \
             junction gmin)"
        )
    }

    /// How the last [`Self::solve_into`] or [`Self::dc_obstacle`] failed.
    pub(crate) fn last_failure(&self) -> Option<SolveFailure> {
        self.last_failure
    }

    /// Conditions of the last load (what a failure refers to).
    pub(crate) fn last_conditions(&self) -> Option<Conditions> {
        self.last_conditions
    }

    /// Records the unknown with the largest normalised Newton update.
    pub(crate) fn set_worst(&mut self, worst: Option<usize>) {
        self.worst = worst;
    }

    /// The unknown recorded by [`Self::set_worst`].
    pub(crate) fn worst(&self) -> Option<usize> {
        self.worst
    }

    /// Records the unknown whose rounding noise most exceeds its tolerance.
    pub(crate) fn set_precision_floor(&mut self, floor: Option<(usize, f64)>) {
        self.precision_floor = floor;
    }

    /// The unknown and noise recorded by [`Self::set_precision_floor`].
    pub(crate) fn precision_floor(&self) -> Option<(usize, f64)> {
        self.precision_floor
    }

    /// Marks `unknown` as reported for its precision floor; false when it
    /// already was.
    pub(crate) fn mark_precision_warned(&mut self, unknown: usize) -> bool {
        !std::mem::replace(&mut self.precision_warned[unknown], true)
    }

    /// Estimated rounding noise of each component of `x`, the solution of
    /// the system just solved: `|A⁻¹·w|` with `wᵢ = ε·(Σ|aᵢⱼ·xⱼ| + |bᵢ|)`
    /// summed over the individual stamps, i.e. roughly how far the solution
    /// moves when every stamp is perturbed by one rounding error. It is
    /// negligible except around nodes whose conductances span many decades
    /// (a series resistance between nodes held only by junction
    /// conductances), where `G + g` loses the digits of `g` when assembled.
    /// `None` when the solver holds no factorization.
    pub(crate) fn solution_noise(&mut self, x: &[f64]) -> Option<Vec<f64>> {
        let mut w: Vec<f64> = self.system.rhs().iter().map(|b| b.abs()).collect();
        for &(r, c, a) in self.system.entries() {
            w[r] += (a * x[c]).abs();
        }
        w.iter_mut().for_each(|v| *v *= f64::EPSILON);
        let z = self.solver.resolve(&w)?;
        Some(z.into_iter().map(f64::abs).collect())
    }

    /// Adds the GMIN-stepping and pseudo-transient conductances to every
    /// voltage unknown. Branch equations are constraints and stay exact.
    /// `anchor` must hold a full solution when the pseudo conductance is set.
    fn stamp_continuation(&mut self, conditions: &Conditions, anchor: &[f64]) {
        let (node_gmin, pseudo) = (conditions.node_gmin, conditions.pseudo_conductance);
        if node_gmin == 0.0 && pseudo == 0.0 {
            return;
        }
        for i in (0..self.layout.size()).filter(|&i| self.layout.is_voltage(i)) {
            self.system.add(Some(i), Some(i), node_gmin + pseudo);
            if pseudo != 0.0 {
                self.system.add_rhs(Some(i), pseudo * anchor[i]);
            }
        }
    }

    /// Whether the right-hand side assembled by the last [`Self::load`] is
    /// exactly zero (no source and no device current drives the circuit).
    pub(crate) fn excitation_is_zero(&self) -> bool {
        self.system.rhs().iter().all(|&b| b == 0.0)
    }

    /// The storage for Newton updates (see [`Self::return_iterate`]).
    pub(crate) fn take_iterate(&mut self) -> Vec<f64> {
        std::mem::take(&mut self.iterate)
    }

    /// Gives back the storage from [`Self::take_iterate`] for the next solve.
    pub(crate) fn return_iterate(&mut self, iterate: Vec<f64>) {
        self.iterate = iterate;
    }

    /// Solves the system assembled by the last [`Self::load`] into `x`.
    pub(crate) fn solve_into(&mut self, x: &mut Vec<f64>) -> Result<(), SingularError> {
        let outcome = self.solver.solve_into(&self.system, x);
        self.last_failure = outcome.as_ref().err().map(|e| e.kind);
        outcome
    }

    /// Whether every device accepts `x` as converged.
    pub(crate) fn devices_converged(&self, x: &[f64]) -> bool {
        self.devices.iter().all(|d| d.converged(x, &self.options))
    }

    /// Commits an accepted solution to every device. Returns whether a
    /// device changed discontinuously there (see
    /// [`DeviceInstance::accept`]).
    pub(crate) fn accept(&mut self, x: &[f64], time: f64) -> bool {
        let mut changed = false;
        for &index in &self.eventful {
            changed |= self.devices[index].accept(x, time);
        }
        changed
    }

    /// The earliest time at which a device asks a trial step that reaches
    /// `time` with solution `x` to end instead (see
    /// [`DeviceInstance::locate`]).
    pub(crate) fn locate(&mut self, x: &[f64], time: f64, min_step: f64) -> Option<f64> {
        let mut earliest: Option<f64> = None;
        for &index in &self.eventful {
            if let Some(at) = self.devices[index].locate(x, time, min_step) {
                earliest = Some(earliest.map_or(at, |e| e.min(at)));
            }
        }
        earliest
    }

    /// Breakpoints of every device in `(0, stop]`, unsorted.
    pub(crate) fn breakpoints(&self, stop: f64) -> Vec<f64> {
        let mut out = Vec::new();
        for device in &self.devices {
            device.breakpoints(stop, &mut out);
        }
        out
    }

    /// Initial states and unknown values for a transient that starts from
    /// rest ([`crate::InitialState::ZeroState`]).
    pub(crate) fn initial_conditions(&self) -> InitialConditions {
        let mut conditions = InitialConditions::default();
        for device in &self.devices {
            device.initial_conditions(&mut conditions.states, &mut conditions.unknowns);
        }
        conditions
    }

    /// Terminal currents of element `index` at the last load.
    pub(crate) fn currents(&self, index: usize, out: &mut [f64]) {
        self.devices[index].currents(out);
    }

    /// Assembles the small-signal system at angular frequency `omega`.
    pub(crate) fn load_ac(&self, system: &mut System<Complex64>, omega: f64) {
        system.clear();
        let mut ctx = AcContext { system, omega };
        for device in &self.devices {
            device.load_ac(&mut ctx);
        }
    }

    /// Small-signal stamps of element `index` alone (used to derive AC
    /// terminal currents).
    pub(crate) fn load_ac_device(&self, index: usize, system: &mut System<Complex64>, omega: f64) {
        system.clear();
        self.devices[index].load_ac(&mut AcContext { system, omega });
    }
}
