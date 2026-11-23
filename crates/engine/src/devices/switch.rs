//! Voltage- and current-controlled switches with hysteresis (Q87, D17).
//!
//! A switch is a conductance between its two output terminals controlled by
//! one quantity, its *control* ([`Sense`]): the voltage across its last two
//! terminals (the voltage switch, SPICE `S`), or the branch current of an
//! independent voltage source (the current switch, SPICE `W`). Everything
//! below holds for both, with the thresholds `vt`, `vh` in volts or `it`,
//! `ih` in amperes; it is written for the voltage switch.
//!
//! * `vh ≥ 0`: a latch with the SPICE3 / LTspice semantics. The switch turns
//!   on when the control voltage exceeds `vt + vh` and off when it falls
//!   below `vt − vh`; between the thresholds, and exactly at either of them,
//!   it keeps its state. With `vh = 0` both thresholds are `vt`.
//! * `vh < 0`: LTspice's smooth mode. No hysteresis; the logarithm of the
//!   conductance follows a cubic smoothstep from `vt − |vh|` to `vt + |vh|`,
//!   so the conductance and its derivative are continuous.
//!
//! Every switch starts in the off state.
//!
//! ## Operating points and DC sweeps
//! The latch's conductance is exactly `1/ron` or `1/roff`. As in SPICE3 it
//! follows the Newton iterates of a solve: once an iterate crosses a
//! threshold the switch stays in its new state for the rest of that solve
//! unless a later iterate crosses the other threshold. The first iteration
//! of a fresh solve, which evaluates the starting guess rather than a
//! solution, keeps the committed state (off before the first accepted
//! solution), so a switch whose control sits inside a band containing 0 V
//! (or 0 A) stays off, as in LTspice. Each change of state forces another
//! iteration, and an iterate that would change the state is never accepted
//! as converged. A switch whose own current pulls its control
//! back across the thresholds (a relaxation oscillator) therefore has no
//! operating point, as in LTspice, instead of settling between them.
//!
//! ## Transients
//! In a transient the latch switches ideally. During every timestep its
//! conductance is exactly `1/ron` or `1/roff` (it is linear within a step),
//! and it changes state only at an accepted time point whose control lies
//! beyond the active threshold (`vt + vh` while off, `vt − vh` while on).
//! That point is the located threshold crossing:
//!
//! 1. A trial step whose control lies more than the switch's *resolution*
//!    ([`VOLTAGE_RESOLUTION`] for a voltage switch, [`current_resolution`]
//!    for a current switch) beyond the threshold, at its end or at an
//!    intermediate point (TR-BDF2's trapezoidal stage, a restart step's
//!    midpoint), is retried ([`DeviceInstance::locate`]). The retry ends
//!    half a resolution past the crossing that linear interpolation of the
//!    control from the last accepted point predicts. Within a step the state
//!    is fixed, so the control follows the smooth trajectory of the old
//!    state and the interpolation converges quickly; a control that is the
//!    same at the end of a shorter retry has jumped right after the start of
//!    the step (an ideal source edge) and switches at once. A step of at
//!    most [`LOCATE_FLOOR`] minimum timesteps is accepted wherever it ends,
//!    and so is a step the transient cannot shorten: one that starts less
//!    than two minimum steps before a breakpoint or output time, where
//!    event landing stretches any shorter retry back to that event.
//! 2. The change of state is a discontinuity of the circuit equations: the
//!    transient restarts its integration at that point as after a source
//!    breakpoint (a backward-Euler step, the history before it ignored,
//!    [`DeviceInstance::accept`] returns `true`). No integration formula
//!    spans the jump of the conductance, so the trapezoidal rule does not
//!    ring on it and the truncation-error estimates start fresh.
//!
//! The switching instant is therefore exact to within the time the control
//! takes to move one resolution (a nanosecond for a control moving at
//! 1 kV/s), whatever the step control would otherwise allow, but not finer
//! than the run resolves time: an ideal edge, or a crossing that falls on an
//! accepted point, switches one minimum step after it (up to two on a step
//! that cannot be shortened, such as across a gate edge shorter than two
//! minimum steps), and where the control moves more than a resolution
//! within a few rounding units of the time (a nanosecond edge 100 s into a
//! run) the switch changes state where the step that crosses ends. A latch
//! without hysteresis (`vh = 0`) whose own current pulls its control straight
//! back across `vt` has no well-defined motion; it chatters at the threshold
//! within its resolution, which is slow but stays within the tolerances.
//! The smooth mode has no state and no event: its conductance follows the
//! control.
//!
//! ## AC
//! The small-signal model is the conductance of the last load in parallel
//! with `dG/dc·v` times the small-signal control `c` (zero for a latch at
//! an operating point, whose conductance is an end value).

use super::semiconductor::{current_converged, finite_or, solution_value};
use super::{AcContext, DeviceInstance, LoadContext, LoadMode, SetupContext};
use crate::analysis::SimOptions;
use crate::circuit::NodeIndex;
use crate::mna::Unknown;
use crate::models::{CurrentSwitchModel, SwitchModel};
use num_complex::Complex64;

/// Resolution of a voltage switch in a transient (V): how far beyond its
/// threshold the control may be at the time point where the switch changes
/// state. The transient locates the threshold crossing to within it.
const VOLTAGE_RESOLUTION: f64 = 1e-6;

/// A current switch's resolution relative to its threshold scale
/// `|it| + |ih|`, so a milliampere-scale threshold resolves to the nanoampere
/// as a volt-scale one resolves to [`VOLTAGE_RESOLUTION`]. An absolute
/// resolution fit for microampere thresholds would lie below the rounding
/// noise of an ampere-scale branch current.
const CURRENT_RESOLUTION_SCALE: f64 = 1e-6;

/// Finest current-switch resolution (A): the default `abstol`, for
/// thresholds at or near 0 A.
const CURRENT_RESOLUTION_FLOOR: f64 = 1e-12;

/// A located retry must end at least this far (relative to the time)
/// before the step it shortens: two rounding units, so that the retried
/// step cannot round back to the same end.
const TIME_ROUNDING: f64 = 2.0 * f64::EPSILON;

/// A step of at most this many minimum timesteps is not shortened further
/// to locate a threshold crossing: the switch changes state where it ends,
/// at most one minimum step after the crossing (an ideal edge switches one
/// minimum step after the edge).
const LOCATE_FLOOR: f64 = 1.0;

/// Transient resolution of a current switch (A): [`CURRENT_RESOLUTION_SCALE`]
/// of `|it| + |ih|`, at least [`CURRENT_RESOLUTION_FLOOR`] (non-finite
/// parameters count as 0, as in [`SwitchParams::new`]).
fn current_resolution(model: &CurrentSwitchModel) -> f64 {
    let scale = finite_or(model.it, 0.0).abs() + finite_or(model.ih, 0.0).abs();
    (CURRENT_RESOLUTION_SCALE * scale).max(CURRENT_RESOLUTION_FLOOR)
}

/// Cubic smoothstep `3u² − 2u³` clamped to `[0, 1]`, with its derivative.
fn smoothstep(u: f64) -> (f64, f64) {
    if u <= 0.0 {
        (0.0, 0.0)
    } else if u >= 1.0 {
        (1.0, 0.0)
    } else {
        (u * u * (3.0 - 2.0 * u), 6.0 * u * (1.0 - u))
    }
}

/// How the conductance depends on the control (a voltage or a current).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// `vh ≥ 0`: on above `on_above`, off below `off_below`, otherwise
    /// unchanged.
    Latch { on_above: f64, off_below: f64 },
    /// `vh < 0`: smooth transition from `lower` to `lower + width`.
    Smooth { lower: f64, width: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SwitchParams {
    g_on: f64,
    g_off: f64,
    mode: Mode,
}

impl SwitchParams {
    /// Parameters from the on / off resistances and the threshold `vt` and
    /// hysteresis `vh` of the control (`it` and `ih` for a current switch).
    fn new(ron: f64, roff: f64, vt: f64, vh: f64) -> Self {
        let vt = finite_or(vt, 0.0);
        let vh = finite_or(vh, 0.0);
        let mode = if vh < 0.0 {
            Mode::Smooth {
                lower: vt + vh,
                width: -2.0 * vh,
            }
        } else {
            Mode::Latch {
                on_above: vt + vh,
                off_below: vt - vh,
            }
        };
        Self {
            g_on: 1.0 / ron,
            g_off: 1.0 / roff,
            mode,
        }
    }

    fn voltage(model: &SwitchModel) -> Self {
        Self::new(model.ron, model.roff, model.vt, model.vh)
    }

    fn current(model: &CurrentSwitchModel) -> Self {
        Self::new(model.ron, model.roff, model.it, model.ih)
    }

    /// State of a latch in state `on` after seeing control voltage `vc`
    /// (the smooth mode has no state and keeps `on`).
    fn next_state(&self, vc: f64, on: bool) -> bool {
        match self.mode {
            Mode::Latch {
                on_above,
                off_below,
            } => {
                if vc > on_above {
                    true
                } else if vc < off_below {
                    false
                } else {
                    on
                }
            }
            Mode::Smooth { .. } => on,
        }
    }

    /// How far control `vc` lies beyond the threshold at which a latch in
    /// state `on` switches (positive once it switches); `None` for the
    /// smooth mode.
    fn beyond(&self, vc: f64, on: bool) -> Option<f64> {
        match self.mode {
            Mode::Latch { off_below, .. } if on => Some(off_below - vc),
            Mode::Latch { on_above, .. } => Some(vc - on_above),
            Mode::Smooth { .. } => None,
        }
    }

    /// Conductance of a latch in state `on`.
    fn end_value(&self, on: bool) -> f64 {
        if on { self.g_on } else { self.g_off }
    }

    /// Conductance and `dG/dVc` of the smooth mode (`None` for a latch).
    fn smooth_conductance(&self, vc: f64) -> Option<(f64, f64)> {
        let Mode::Smooth { lower, width } = self.mode else {
            return None;
        };
        let (s, ds) = smoothstep((vc - lower) / width);
        Some(if s == 0.0 {
            (self.g_off, 0.0)
        } else if s == 1.0 {
            (self.g_on, 0.0)
        } else {
            let span = (self.g_on / self.g_off).ln();
            let g = self.g_off * (span * s).exp();
            (g, g * span * ds / width)
        })
    }
}

/// What a switch senses: its control.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sense {
    /// The voltage `v(control+) − v(control−)` across the voltage switch's
    /// control terminals (circuit nodes and their unknowns).
    Voltage {
        nodes: [NodeIndex; 2],
        unknowns: [Unknown; 2],
    },
    /// The branch current of the independent voltage source at element
    /// index `source` (current switch), positive into its positive terminal.
    /// `branch` is that current's unknown, found by [`DeviceInstance::link`];
    /// without a source (an unvalidated circuit) the control is 0 A.
    Current {
        source: Option<usize>,
        branch: Unknown,
    },
}

#[derive(Debug)]
pub(crate) struct Switch {
    /// `[positive, negative]` output terminals.
    nodes: [NodeIndex; 2],
    params: SwitchParams,
    /// `[positive, negative]` unknowns.
    unknowns: [Unknown; 2],
    sense: Sense,
    /// Transient resolution of the control: [`VOLTAGE_RESOLUTION`] or
    /// [`current_resolution`].
    resolution: f64,
    /// Committed state (changes only at accepted solutions).
    on: bool,
    /// State during the current DC solve (see the module documentation);
    /// the committed state during transient steps.
    trial: bool,
    /// Whether the last load was a DC solve rather than a transient step.
    dc: bool,
    /// Time and control of the last accepted solution.
    accepted_at: f64,
    accepted_ctl: f64,
    /// Time of the last transient load.
    loaded_at: f64,
    /// Time and control of the last trial that [`DeviceInstance::locate`]
    /// shortened since the last accepted solution.
    located: Option<(f64, f64)>,
    /// Switch voltage, control, conductance and `dG/dc` at the last load.
    v: f64,
    ctl: f64,
    g: f64,
    dg: f64,
}

impl Switch {
    fn new(nodes: [NodeIndex; 2], params: SwitchParams, sense: Sense, resolution: f64) -> Self {
        Self {
            nodes,
            params,
            unknowns: [None; 2],
            sense,
            resolution,
            on: false,
            trial: false,
            dc: true,
            accepted_at: 0.0,
            accepted_ctl: 0.0,
            loaded_at: f64::NEG_INFINITY,
            located: None,
            v: 0.0,
            ctl: 0.0,
            g: 0.0,
            dg: 0.0,
        }
    }

    /// A voltage switch on `[positive, negative, control+, control−]`.
    pub(crate) fn voltage(nodes: &[NodeIndex], model: &SwitchModel) -> Self {
        let sense = Sense::Voltage {
            nodes: [nodes[2], nodes[3]],
            unknowns: [None; 2],
        };
        Self::new(
            [nodes[0], nodes[1]],
            SwitchParams::voltage(model),
            sense,
            VOLTAGE_RESOLUTION,
        )
    }

    /// A current switch on `[positive, negative]` controlled by the branch
    /// current of the voltage source at element index `source`.
    pub(crate) fn current(
        nodes: &[NodeIndex],
        source: Option<usize>,
        model: &CurrentSwitchModel,
    ) -> Self {
        let sense = Sense::Current {
            source,
            branch: None,
        };
        Self::new(
            [nodes[0], nodes[1]],
            SwitchParams::current(model),
            sense,
            current_resolution(model),
        )
    }

    fn current_through(&self) -> f64 {
        self.g * self.v
    }

    /// Control (voltage or current) of a real or complex solution vector.
    fn control<T: Copy + Default + std::ops::Sub<Output = T>>(&self, x: &[T]) -> T {
        match self.sense {
            Sense::Voltage {
                unknowns: [cp, cn], ..
            } => solution_value(x, cp) - solution_value(x, cn),
            Sense::Current { branch, .. } => solution_value(x, branch),
        }
    }
}

impl DeviceInstance for Switch {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.unknowns = self.nodes.map(|n| ctx.node(n));
        if let Sense::Voltage { nodes, unknowns } = &mut self.sense {
            *unknowns = nodes.map(|n| ctx.node(n));
        }
    }

    fn link(&mut self, branches: &[Option<usize>]) {
        if let Sense::Current {
            source: Some(source),
            branch,
        } = &mut self.sense
        {
            *branch = branches.get(*source).copied().flatten();
        }
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [p, n] = self.unknowns;
        self.v = ctx.v(p) - ctx.v(n);
        let ctl = self.control(ctx.x);
        self.ctl = ctl;
        self.dc = ctx.mode == LoadMode::Dc;
        if !self.dc {
            // Within a timestep the latch keeps its committed state.
            self.trial = self.on;
            self.loaded_at = ctx.time;
        } else if ctx.init_junctions {
            // A fresh solve (or a continuation restarting from its last good
            // point) starts from the committed state. Its first iterate is
            // only the starting guess, all zero for an operating point, so
            // it must not move the latch: a band containing 0 V would
            // otherwise turn a switch on that LTspice keeps off.
            self.trial = self.on;
        } else {
            let next = self.params.next_state(ctl, self.trial);
            if next != self.trial {
                // As SPICE3: a change of state needs another iteration.
                self.trial = next;
                ctx.limited = true;
            }
        }
        let end = self.params.end_value(self.trial);
        (self.g, self.dg) = self.params.smooth_conductance(ctl).unwrap_or((end, 0.0));
        ctx.stamp_conductance(p, n, self.g);
        if self.dg != 0.0 {
            let gm = self.dg * self.v;
            match self.sense {
                Sense::Voltage {
                    unknowns: [cp, cn], ..
                } => ctx.stamp_transconductance(p, n, cp, cn, gm),
                Sense::Current { branch, .. } => ctx.stamp_current_controlled(p, n, branch, gm),
            }
            ctx.stamp_current(p, n, -gm * self.ctl);
        }
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [p, n] = self.unknowns;
        ctx.stamp_conductance(p, n, self.g);
        if self.dg != 0.0 {
            let gm = Complex64::from(self.dg * self.v);
            match self.sense {
                Sense::Voltage {
                    unknowns: [cp, cn], ..
                } => ctx.stamp_transadmittance(p, n, cp, cn, gm),
                Sense::Current { branch, .. } => ctx.stamp_current_controlled(p, n, branch, gm),
            }
        }
    }

    /// The switch current into the positive terminal and out of the
    /// negative one; nothing into a voltage switch's control terminals.
    fn currents(&self, out: &mut [f64]) {
        let i = self.current_through();
        for (slot, value) in out.iter_mut().zip([i, -i, 0.0, 0.0]) {
            *slot = value;
        }
    }

    /// `g·(vp − vn) + dG/dc·v·c` through the switch, with `c` the
    /// small-signal control; nothing into a voltage switch's control
    /// terminals.
    fn ac_currents(&self, x: &[Complex64], _omega: f64, out: &mut [Complex64]) -> bool {
        let [p, n] = self.unknowns;
        let v = solution_value(x, p) - solution_value(x, n);
        let i = v * self.g + self.control(x) * (self.dg * self.v);
        let zero = Complex64::new(0.0, 0.0);
        for (slot, value) in out.iter_mut().zip([i, -i, zero, zero]) {
            *slot = value;
        }
        true
    }

    fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
        let [p, n] = self.unknowns;
        let ctl = self.control(x);
        if self.dc && self.params.next_state(ctl, self.trial) != self.trial {
            // The iterate crosses a threshold: the next load switches.
            return false;
        }
        let v = solution_value(x, p) - solution_value(x, n);
        let delta = self.g * (v - self.v) + self.dg * self.v * (ctl - self.ctl);
        current_converged(self.current_through(), delta, options)
    }

    fn has_memory(&self) -> bool {
        true
    }

    /// Locates the latch's threshold crossing (see the module documentation).
    fn locate(&mut self, x: &[f64], time: f64, min_step: f64) -> Option<f64> {
        let ctl = self.control(x);
        let beyond = self.params.beyond(ctl, self.on)?;
        let start = self.accepted_at;
        let floor = start + LOCATE_FLOOR * min_step;
        // Not past the threshold (a NaN control never is), or located: the
        // state changes at the end of the step if the control is beyond the
        // threshold there.
        if beyond.is_nan() || beyond <= self.resolution || time <= floor {
            return None;
        }
        let jumped = self
            .located
            .is_some_and(|(at, previous)| time < at && (ctl - previous).abs() <= self.resolution);
        self.located = Some((time, ctl));
        if jumped {
            // The control is the same at the end of a shorter step: it
            // jumped right after the start of the step (an ideal edge).
            return Some(floor);
        }
        // The state is fixed within the step, so the control moved smoothly
        // from the accepted point: interpolate where it crossed the
        // threshold, and aim half a resolution past it.
        let travel = (ctl - self.accepted_ctl).abs();
        let fraction = (travel - beyond + 0.5 * self.resolution) / travel;
        let target = (start + fraction * (time - start)).max(floor);
        // A crossing that the time axis cannot resolve any closer (the
        // control moves more than a resolution within a few rounding units
        // of the time) is located where the step ends: retrying a step that
        // rounds to the same end would never finish.
        (target < time - TIME_ROUNDING * time.abs()).then_some(target)
    }

    fn accept(&mut self, x: &[f64], time: f64) -> bool {
        let ctl = self.control(x);
        let mut switched = false;
        if self.dc {
            self.on = self.params.next_state(ctl, self.trial);
        } else if time < self.loaded_at {
            // The midpoint of a step-doubled step, accepted after the later
            // end point was already solved with the committed state: the
            // end point's acceptance decides (the transient never accepts a
            // midpoint beyond the threshold, see `locate`).
        } else {
            let next = self.params.next_state(ctl, self.on);
            switched = next != self.on;
            self.on = next;
        }
        self.trial = self.on;
        self.accepted_at = time;
        self.accepted_ctl = ctl;
        self.located = None;
        switched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(vt: f64, vh: f64) -> SwitchModel {
        SwitchModel {
            name: "SW".into(),
            ron: 1.0,
            roff: 1e6,
            vt,
            vh,
        }
    }

    #[test]
    fn latch_switches_beyond_the_thresholds_and_keeps_its_state_at_them() {
        let p = SwitchParams::voltage(&model(1.0, 0.2));
        let (up, down) = (1.0 + 0.2, 1.0 - 0.2);
        // Rising: off up to and including vt + vh, on just above it.
        assert!(!p.next_state(1.1, false));
        assert!(!p.next_state(up, false));
        assert!(p.next_state(up + 1e-15, false));
        assert!(p.next_state(5.0, false));
        // Falling: on down to and including vt − vh, off just below it.
        assert!(p.next_state(0.9, true));
        assert!(p.next_state(down, true));
        assert!(!p.next_state(down - 1e-15, true));
        assert!(!p.next_state(-5.0, true));
        // Without hysteresis the state is kept only exactly at vt.
        let p = SwitchParams::voltage(&model(1.0, 0.0));
        assert!(!p.next_state(1.0, false) && p.next_state(1.0, true));
        assert!(p.next_state(1.0 + 1e-15, false));
        assert!(!p.next_state(1.0 - 1e-15, true));
    }

    #[test]
    fn the_distance_beyond_the_active_threshold_is_signed() {
        let p = SwitchParams::voltage(&model(1.0, 0.2));
        assert!((p.beyond(1.25, false).unwrap() - 0.05).abs() < 1e-12);
        assert!((p.beyond(1.1, false).unwrap() + 0.1).abs() < 1e-12);
        assert!((p.beyond(0.7, true).unwrap() - 0.1).abs() < 1e-12);
        assert!((p.beyond(1.1, true).unwrap() + 0.3).abs() < 1e-12);
        assert_eq!(
            SwitchParams::voltage(&model(1.0, -0.5)).beyond(5.0, false),
            None
        );
    }

    #[test]
    fn smooth_mode_has_no_state_and_a_continuous_transition() {
        let p = SwitchParams::voltage(&model(1.0, -0.5));
        assert_eq!(p.smooth_conductance(0.5).map(|g| g.0), Some(1e-6));
        assert_eq!(p.smooth_conductance(1.5).map(|g| g.0), Some(1.0));
        let mid = p.smooth_conductance(1.0).unwrap_or_default();
        assert!((mid.0 / 1e-3 - 1.0).abs() < 1e-12);
        assert!(!p.next_state(3.0, false) && p.next_state(-3.0, true));
        let g = |v: f64| p.smooth_conductance(v).unwrap_or_default();
        for &vc in &[0.55, 0.8, 1.0, 1.3, 1.45] {
            let h = 1e-7;
            let fd = (g(vc + h).0 - g(vc - h).0) / (2.0 * h);
            let dg = g(vc).1;
            assert!((dg - fd).abs() <= 1e-5 * dg.abs(), "vc={vc}: {dg} vs {fd}");
        }
        let latch = SwitchParams::voltage(&model(1.0, 0.0));
        assert_eq!(latch.smooth_conductance(1.0), None);
    }

    /// A switch from node `p` to ground controlled by node `c` (unknowns 0
    /// and 1), with the solution vectors and loads a transient would make.
    struct Driven {
        switch: Switch,
        options: SimOptions,
    }

    /// The minimum step the [`Driven`] transient locates with.
    const MIN_STEP: f64 = 1e-18;

    impl Driven {
        fn new(vt: f64, vh: f64) -> Self {
            use crate::circuit::{Circuit, Device, Element, GROUND};
            use crate::mna::Layout;
            let mut circuit = Circuit::new();
            let (p, c) = (circuit.node("p"), circuit.node("c"));
            let nodes = [p, GROUND, c, GROUND];
            let model = model(vt, vh);
            circuit.add(Element::new(
                "S1",
                nodes,
                Device::VoltageSwitch {
                    model: model.clone(),
                },
            ));
            let mut layout = Layout::new(&circuit);
            let mut states = 0;
            let mut switch = Switch::voltage(&nodes, &model);
            switch.setup(&mut SetupContext {
                layout: &mut layout,
                state_count: &mut states,
                element: "S1",
            });
            Self {
                switch,
                options: SimOptions::default(),
            }
        }

        /// Loads at control voltage `vc` (1 V across the switch) in `mode`
        /// at `time` and returns the conductance.
        fn load(&mut self, mode: LoadMode, time: f64, vc: f64) -> f64 {
            use crate::analysis::IntegrationMethod;
            use crate::integration::Integrator;
            use crate::solver::System;
            let x = [1.0, vc];
            let mut system = System::new(2);
            let mut integrator = Integrator::new(IntegrationMethod::Trapezoidal, 0);
            let mut ctx = LoadContext::new(&x, &mut system, &mut integrator, &self.options);
            ctx.mode = mode;
            ctx.time = time;
            self.switch.load(&mut ctx);
            self.switch.g
        }

        /// [`Self::load`], then accepts that point; returns the conductance
        /// of the step and whether the switch changed state.
        fn step(&mut self, mode: LoadMode, time: f64, vc: f64) -> (f64, bool) {
            let g = self.load(mode, time, vc);
            (g, self.switch.accept(&[1.0, vc], time))
        }

        /// Where a trial reaching `time` with control `vc` must end
        /// instead.
        fn locate(&mut self, time: f64, vc: f64) -> Option<f64> {
            self.load(LoadMode::Transient, time, vc);
            self.switch.locate(&[1.0, vc], time, MIN_STEP)
        }
    }

    #[test]
    fn the_latch_keeps_its_state_within_a_step_and_switches_when_accepted() {
        let mut s = Driven::new(2.5, 0.5);
        assert_eq!(s.step(LoadMode::Dc, 0.0, 2.9), (1e-6, false));
        // A trial far beyond 3 V is still solved with the off conductance.
        assert_eq!(s.load(LoadMode::Transient, 1e-6, 3.5), 1e-6);
        // Accepted just beyond the threshold: the switch turns on there, and
        // reports the discontinuity.
        assert_eq!(s.step(LoadMode::Transient, 1e-6, 3.0 + 1e-7), (1e-6, true));
        assert!(s.switch.on);
        assert_eq!(s.step(LoadMode::Transient, 2e-6, 2.5), (1.0, false));
        assert_eq!(s.step(LoadMode::Transient, 3e-6, 2.0 - 1e-7), (1.0, true));
        assert_eq!(s.step(LoadMode::Transient, 4e-6, 2.1), (1e-6, false));
    }

    #[test]
    fn a_step_crossing_the_threshold_is_retried_at_the_interpolated_crossing() {
        let mut s = Driven::new(2.5, 0.5);
        s.step(LoadMode::Dc, 0.0, 2.9);
        s.step(LoadMode::Transient, 1e-6, 2.95);
        // From 2.95 V at 1 µs to 3.1 V at 2 µs: the control crossed 3 V a
        // third of the way, and the retry aims half a resolution past it.
        let end = s.locate(2e-6, 3.1).expect("retried");
        let expected = 1e-6 + 1e-6 * (0.05 + 0.5 * VOLTAGE_RESOLUTION) / 0.15;
        assert!((end - expected).abs() < 1e-18, "{end} vs {expected}");
        // A trial at most a resolution beyond the threshold is accepted, and
        // so is one that does not reach it.
        assert_eq!(s.locate(end, 3.0 + 0.5 * VOLTAGE_RESOLUTION), None);
        assert_eq!(s.locate(end, 2.999), None);
        // Falling through vt − vh while on.
        s.step(LoadMode::Transient, end, 3.0 + 0.5 * VOLTAGE_RESOLUTION);
        s.step(LoadMode::Transient, 3e-6, 2.2);
        let end = s.locate(4e-6, 1.7).expect("retried");
        let expected = 3e-6 + 1e-6 * (0.2 + 0.5 * VOLTAGE_RESOLUTION) / 0.5;
        assert!((end - expected).abs() < 1e-18, "{end} vs {expected}");
    }

    #[test]
    fn an_intermediate_point_far_beyond_the_threshold_ends_the_step_before_it() {
        let mut s = Driven::new(2.5, 0.5);
        s.step(LoadMode::Dc, 0.0, 2.9);
        s.step(LoadMode::Transient, 1e-6, 2.95);
        // A stage (or midpoint) at 1.5 µs within the resolution needs no
        // retry: the end of the step decides.
        let stage = 1.5e-6;
        assert_eq!(s.locate(stage, 3.0 + 1e-7), None);
        // Further beyond: the step ends at the interpolated crossing.
        let end = s.locate(stage, 3.05).expect("retried");
        let expected = 1e-6 + 0.5e-6 * (0.05 + 0.5 * VOLTAGE_RESOLUTION) / 0.1;
        assert!((end - expected).abs() < 1e-18, "{end} vs {expected}");
        // Not beyond it: nothing to locate.
        assert_eq!(s.locate(stage, 2.99), None);
    }

    #[test]
    fn a_control_that_jumped_switches_right_after_the_start_of_the_step() {
        let mut s = Driven::new(2.5, 0.5);
        s.step(LoadMode::Dc, 0.0, 0.0);
        // An ideal 5 V edge at the accepted point: interpolation first
        // halves the step, then sees the same control and gives up.
        let first = s.locate(2e-6, 5.0).expect("retried");
        assert!((first - 1.2e-6).abs() < 1e-12, "{first}");
        let floor = LOCATE_FLOOR * MIN_STEP;
        assert_eq!(s.locate(first, 5.0), Some(floor));
        // At the floor the trial is accepted wherever it ends.
        assert_eq!(s.locate(floor, 5.0), None);
        assert_eq!(s.step(LoadMode::Transient, floor, 5.0), (1e-6, true));
    }

    #[test]
    fn current_resolutions_scale_with_the_threshold_currents() {
        let resolution = |it: f64, ih: f64| {
            current_resolution(&CurrentSwitchModel {
                it,
                ih,
                ..CurrentSwitchModel::default()
            })
        };
        assert!((resolution(1e-3, 0.2e-3) / 1.2e-9 - 1.0).abs() < 1e-12);
        assert!((resolution(-25e-6, -5e-6) / 30e-12 - 1.0).abs() < 1e-12);
        assert_eq!(resolution(0.0, 0.0), CURRENT_RESOLUTION_FLOOR);
        assert_eq!(resolution(1e-9, 0.0), CURRENT_RESOLUTION_FLOOR);
        assert_eq!(
            resolution(f64::NAN, f64::INFINITY),
            CURRENT_RESOLUTION_FLOOR
        );
        // Thresholds and the resolution are in amperes.
        let model = CurrentSwitchModel {
            it: 1e-3,
            ih: 0.2e-3,
            ..CurrentSwitchModel::default()
        };
        let p = SwitchParams::current(&model);
        assert!(!p.next_state(1.2e-3, false) && p.next_state(1.2e-3 + 1e-15, false));
        assert!(p.next_state(0.8e-3, true) && !p.next_state(0.8e-3 - 1e-15, true));
    }

    #[test]
    fn a_current_switch_senses_the_branch_it_is_linked_to() {
        let model = CurrentSwitchModel::default();
        let mut linked = Switch::current(&[1, 0], Some(2), &model);
        assert_eq!(linked.control(&[0.5, 0.25, 7.0]), 0.0, "not linked yet");
        linked.link(&[None, None, Some(2)]);
        assert_eq!(
            linked.sense,
            Sense::Current {
                source: Some(2),
                branch: Some(2)
            }
        );
        assert_eq!(linked.control(&[0.5, 0.25, 7.0]), 7.0);
        assert_eq!(linked.resolution, CURRENT_RESOLUTION_FLOOR);
        // Without a source (an unvalidated circuit) the control is 0 A.
        let mut unlinked = Switch::current(&[1, 0], None, &model);
        unlinked.link(&[Some(0), Some(1), Some(2)]);
        assert_eq!(unlinked.control(&[0.5, 0.25, 7.0]), 0.0);
    }

    #[test]
    fn non_finite_thresholds_fall_back_to_zero() {
        let p = SwitchParams::voltage(&model(f64::NAN, f64::INFINITY));
        assert_eq!(
            p.mode,
            Mode::Latch {
                on_above: 0.0,
                off_below: 0.0
            }
        );
    }
}
