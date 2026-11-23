//! Newton-Raphson iteration and DC convergence recovery (Q103):
//! direct Newton, then GMIN stepping, then source stepping, then
//! pseudo-transient continuation.
//!
//! ## Convergence test
//! An iterate is converged when every unknown moved by at most
//! `reltol·max(|new|, |old|)` plus `vntol` (voltages) or `abstol` (branch
//! currents), every device accepts the new iterate
//! ([`crate::devices::DeviceInstance::converged`]), no device limited its
//! update, and the iteration was not the junction-initialisation pass. A
//! circuit whose devices are all linear is solved exactly by one iteration.
//! After convergence the devices are loaded once more at the solution, so
//! their stored currents and the integrator's pending states describe it.
//!
//! ### Rounding floor
//! Some unknowns cannot be resolved to `reltol` in double precision: a
//! cluster of nodes joined by a large conductance `G` (a diode's series
//! resistance) but held to the rest of the circuit only by junction
//! conductances `g ≪ G` is known only to about `ε·G/g` of its voltage,
//! because `G + g` loses the digits of `g` when the matrix is assembled
//! (series diode strings at low forward bias, DCAC-3). Their Newton updates
//! then stall at that noise instead of shrinking. When an iterate fails the
//! update test but nothing was limited and every device accepts it, the
//! rounding noise of each solution component is estimated
//! ([`Mna::solution_noise`]) and an update within a few times that noise
//! counts as converged, unless the noise exceeds 1 % of the value. When the
//! noise is well above the tolerance the run warns, naming the unknown and
//! its estimated resolution: that solution is only as precise as double
//! precision allows (LTspice resolves such circuits no better). Well-
//! conditioned circuits never reach this path with a noise that matters:
//! there it is about `ε·|x|`, far below `reltol`.
//!
//! ## DC convergence ladder
//! [`operating_point`] first checks the circuit's connectivity: a node
//! without a DC path to ground that is fed by a current source or connected
//! to nothing, or a loop of voltage sources and inductors, has no operating
//! point, and the failure names it without iterating. Nodes reached only
//! through capacitors are tied to ground through gmin by [`Mna::load`]
//! (D15); [`operating_point`] raises a warning naming each one. It then
//! tries each strategy in turn, every one starting from the caller's
//! initial guess, and stops at the first success:
//!
//! 1. **Direct Newton**, junctions initialised on the first iteration when
//!    the solve is fresh.
//! 2. **GMIN stepping**: a conductance from every node to ground (and across
//!    every junction) starts at 10 mS and is divided by 10 after each
//!    converged solve, by a smaller factor after a failure, down to
//!    `options.gmin`; the unaided circuit is then solved from the result.
//! 3. **Source stepping**: all independent sources start at zero and ramp up
//!    with an adaptive increment, each solve starting from the previous one.
//! 4. **Pseudo-transient continuation**: backward-Euler steps of an
//!    artificial capacitance from every node to ground, with a growing
//!    pseudo timestep, until the unaided circuit converges from the settled
//!    state.
//!
//! When direct Newton meets a matrix that is singular to double precision
//! although every node has a DC path (a micro-ohm resistance next to a node
//! held only by junction gmin), and every source is zero there, the
//! operating point is all zeros and is accepted "by inspection", as LTspice
//! does, without factoring the matrix.
//!
//! When everything fails the error lists how far each strategy got, e.g.
//! the percentage of source strength source stepping reached (Q162), and
//! names the unknown whose update was largest relative to its tolerance in
//! the last direct Newton iteration (D7).

use crate::control::SimControl;
use crate::devices::LoadMode;
use crate::error::SimError;
use crate::mna::{Conditions, Mna};
use crate::solver::SolveFailure;

/// An update within this many times the estimated rounding noise of its
/// solution component counts as converged (see the module docs).
const NOISE_MARGIN: f64 = 4.0;

/// An iterate is not accepted at the rounding floor when an unknown's
/// estimated rounding noise exceeds this fraction of its value: such a
/// value would carry no meaningful digits.
const FLOOR_LIMIT: f64 = 1e-2;

/// A solution accepted at the rounding floor raises a warning when an
/// unknown's estimated noise exceeds its tolerance by this factor. (Near
/// zero, branch currents through large companion conductances routinely
/// carry noise of the order of `abstol`, which is not worth a warning.)
///
/// In a transient step only node voltages are reported. A branch current's
/// absolute rounding noise grows with the conductance·voltage products
/// around it (about `ε·G·|v|`: picoamperes next to a 1 mΩ resistor at 10 V),
/// so whenever the current crosses zero it exceeds `abstol` many times over
/// without any loss of meaningful precision, while an ill-conditioned
/// cluster shows up in its node voltages. Operating points report currents
/// too: there a current resolved only to its floor is a fixed property of
/// the answer (1 V → 1 µΩ → 1 MΩ gives `I(V1)` to about 1e-4 of itself).
const COARSE_FLOOR: f64 = 10.0;

/// GMIN stepping starts with this conductance (S) from every node to ground.
const GMIN_START: f64 = 1e-2;
/// GMIN stepping stops reducing the conductance here when `options.gmin` is
/// smaller (or zero) and continues with the unaided circuit.
const GMIN_FLOOR: f64 = 1e-12;
/// Initial reduction factor of the GMIN conductance per converged solve.
const GMIN_FACTOR: f64 = 10.0;
/// GMIN stepping gives up once failures have shrunk the factor below this.
const GMIN_MIN_FACTOR: f64 = 1.001;

/// First source-strength increment of source stepping.
const SOURCE_STEP_START: f64 = 1e-3;
/// Source stepping gives up when failures shrink the increment below this.
const SOURCE_STEP_MIN: f64 = 1e-9;

/// Initial pseudo conductance (S): a 1 F capacitor over a 1 s pseudo step.
const PSEUDO_START: f64 = 1.0;
/// Below this pseudo conductance the unaided circuit is tried after every
/// pseudo step.
const PSEUDO_RELEASE: f64 = 1e-6;
/// Pseudo-transient continuation gives up beyond these conductances.
const PSEUDO_MIN: f64 = 1e-18;
const PSEUDO_MAX: f64 = 1e6;
/// Pseudo timestep growth after a converged pseudo step.
const PSEUDO_GROWTH: f64 = 4.0;

/// Upper bound on the solves of one continuation strategy, so that even a
/// pathological circuit cannot keep a strategy busy forever.
const MAX_CONTINUATION_SOLVES: usize = 1000;

/// One Newton–Raphson solve.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Request<'a> {
    pub(crate) conditions: Conditions,
    /// Evaluate junctions at their initial voltages on the first iteration.
    pub(crate) init_junctions: bool,
    pub(crate) max_iterations: usize,
    /// Solution the pseudo-transient conductance pulls towards; may be empty
    /// when that conductance is zero.
    pub(crate) anchor: &'a [f64],
}

/// Why a Newton solve stopped without a solution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NewtonError {
    /// The linearised system is singular at this unknown.
    Singular(usize),
    /// The iteration limit was reached.
    NoConvergence,
    /// The run was cancelled.
    Interrupted(SimError),
}

/// Iterates from `x` until convergence; `x` then holds the solution and the
/// devices have been loaded at it. Returns the number of iterations. On
/// failure `x` holds the last iterate.
pub(crate) fn solve(
    mna: &mut Mna,
    x: &mut [f64],
    request: &Request,
    control: &SimControl,
) -> Result<usize, NewtonError> {
    let mut next = mna.take_iterate();
    let outcome = iterate(mna, x, request, control, &mut next);
    mna.return_iterate(next);
    outcome
}

/// The iterations of [`solve`], with `next` as the storage of each update.
fn iterate(
    mna: &mut Mna,
    x: &mut [f64],
    request: &Request,
    control: &SimControl,
    next: &mut Vec<f64>,
) -> Result<usize, NewtonError> {
    let mut init_junctions = request.init_junctions;
    mna.set_precision_floor(None);
    // See `COARSE_FLOOR`: in a transient step only node voltages are
    // reported as resolved to their rounding floor.
    let report_currents = request.conditions.mode == LoadMode::Dc;
    for iteration in 1..=request.max_iterations {
        control.checkpoint().map_err(NewtonError::Interrupted)?;
        let limited = mna.load(x, &request.conditions, init_junctions, request.anchor);
        mna.solve_into(next)
            .map_err(|e| NewtonError::Singular(e.row))?;
        let next = next.as_slice();
        let (small, worst) = measure_update(mna, x, next);
        mna.set_worst(worst);
        let at_floor = !mna.is_linear()
            && !init_junctions
            && !small
            && !limited
            && mna.devices_converged(next)
            && within_rounding_noise(mna, x, next, report_currents);
        let settled = mna.is_linear() || (!init_junctions && small) || at_floor;
        let converged = settled && !limited && mna.devices_converged(next);
        x.copy_from_slice(next);
        init_junctions = false;
        if converged {
            // Continuation solves are intermediate; only the circuit's own
            // equations are worth a warning.
            let c = &request.conditions;
            if at_floor
                && c.node_gmin == 0.0
                && c.pseudo_conductance == 0.0
                && c.source_factor == 1.0
            {
                warn_about_precision(mna, control);
            }
            mna.load(x, &request.conditions, false, request.anchor);
            return Ok(iteration);
        }
    }
    Err(NewtonError::NoConvergence)
}

/// The update tolerance of unknown `i` moving from `old` to `new`.
fn tolerance(mna: &Mna, i: usize, old: f64, new: f64) -> f64 {
    let options = mna.options();
    let absolute = if mna.layout().is_voltage(i) {
        options.vntol
    } else {
        options.abstol
    };
    options.reltol * old.abs().max(new.abs()) + absolute
}

/// SPICE's iterate test (every unknown moved by less than its tolerance),
/// and the unknown whose update is largest relative to its tolerance.
fn measure_update(mna: &Mna, old: &[f64], new: &[f64]) -> (bool, Option<usize>) {
    let mut small = true;
    let mut worst: Option<(usize, f64)> = None;
    for (i, (&a, &b)) in old.iter().zip(new).enumerate() {
        let ratio = (b - a).abs() / tolerance(mna, i, a, b);
        small &= ratio <= 1.0;
        if worst.is_none_or(|(_, w)| ratio > w) {
            worst = Some((i, ratio));
        }
    }
    (small, worst.map(|(i, _)| i))
}

/// Whether every unknown moved by at most its tolerance plus
/// [`NOISE_MARGIN`] times the rounding noise of the solve just done, and no
/// unknown is so noisy that its value would be meaningless
/// ([`FLOOR_LIMIT`]). Records the unknown whose noise exceeds its tolerance
/// the most, if by more than [`COARSE_FLOOR`] ([`Mna::precision_floor`]);
/// branch currents only when `report_currents`.
fn within_rounding_noise(mna: &mut Mna, old: &[f64], new: &[f64], report_currents: bool) -> bool {
    let Some(noise) = mna.solution_noise(new) else {
        return false;
    };
    let mut accept = true;
    let mut coarsest: Option<(usize, f64, f64)> = None;
    for (i, ((&a, &b), &n)) in old.iter().zip(new).zip(&noise).enumerate() {
        let tol = tolerance(mna, i, a, b);
        accept &= (b - a).abs() <= tol + NOISE_MARGIN * n && n <= FLOOR_LIMIT * b.abs() + tol;
        let ratio = n / tol;
        let reportable = report_currents || mna.layout().is_voltage(i);
        if reportable && ratio > COARSE_FLOOR && coarsest.is_none_or(|(_, _, r)| ratio > r) {
            coarsest = Some((i, n, ratio));
        }
    }
    mna.set_precision_floor(coarsest.map(|(i, n, _)| (i, n)));
    accept
}

/// Estimates the rounding noise of the operating point `x` just found and
/// warns (once per unknown) where it is much coarser than the tolerance.
/// An iterate can pass the update test by chance although its noise is
/// large, so this check does not rely on how Newton accepted it.
fn check_precision(mna: &mut Mna, x: &[f64], control: &SimControl) {
    let Some(noise) = mna.solution_noise(x) else {
        return;
    };
    let coarsest = noise
        .iter()
        .enumerate()
        .map(|(i, &n)| (i, n, n / tolerance(mna, i, x[i], x[i])))
        .filter(|&(_, _, ratio)| ratio > COARSE_FLOOR)
        .max_by(|a, b| a.2.total_cmp(&b.2));
    mna.set_precision_floor(coarsest.map(|(i, n, _)| (i, n)));
    warn_about_precision(mna, control);
}

/// Warns once per unknown that a converged solution is resolved only to its
/// rounding floor, coarser than the requested tolerance.
fn warn_about_precision(mna: &mut Mna, control: &SimControl) {
    let Some((unknown, noise)) = mna.precision_floor() else {
        return;
    };
    if !mna.mark_precision_warned(unknown) {
        return;
    }
    let unit = if mna.layout().is_voltage(unknown) {
        "V"
    } else {
        "A"
    };
    control.warn(format!(
        "{} could be resolved only to about ±{} (estimated), less precisely than reltol, because \
         element values around it span too many decades for double precision (for example a \
         series resistance next to junction-level conductances)",
        mna.layout().name(unknown),
        crate::units::format_value(noise, unit)
    ));
}

/// Why the DC ladder produced no operating point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DcFailure {
    /// The matrix is singular, or its solution not finite, at this unknown
    /// (and no strategy helped), or the circuit's connectivity rules out an
    /// operating point (see [`Mna::dc_obstacle`]).
    Singular(usize, SolveFailure),
    /// Every strategy failed; `detail` says how far each one got and
    /// `worst` is the unknown with the largest normalised update in the
    /// last direct Newton iteration.
    NoConvergence {
        detail: String,
        worst: Option<usize>,
    },
    Interrupted(SimError),
}

/// How a continuation strategy ended without a solution.
enum Stall {
    /// A note on how far the strategy got.
    At(String),
    Interrupted(SimError),
}

impl From<SimError> for Stall {
    fn from(error: SimError) -> Self {
        Stall::Interrupted(error)
    }
}

type Strategy = fn(&mut Mna, &mut [f64], Conditions, &SimControl) -> Result<(), Stall>;

/// Solves the DC operating point under `base` conditions with the full
/// convergence ladder, starting from the guess in `x`. `fresh` marks a solve
/// without a nearby previous solution (junctions are initialised). On success
/// `x` holds the solution and the devices are loaded at it.
pub(crate) fn operating_point(
    mna: &mut Mna,
    x: &mut [f64],
    base: Conditions,
    fresh: bool,
    control: &SimControl,
) -> Result<(), DcFailure> {
    if let Some(row) = mna.dc_obstacle(&base) {
        return Err(DcFailure::Singular(row, SolveFailure::Singular));
    }
    warn_about_tied_nodes(mna, &base, control);
    let guess = x.to_vec();
    let iterations = mna.options().dc_iterations;
    let request = Request {
        conditions: base,
        init_junctions: fresh,
        max_iterations: iterations,
        anchor: &[],
    };
    let mut notes = Vec::new();
    let mut singular = None;
    let mut worst = None;
    match solve(mna, x, &request, control) {
        Ok(_) => {
            check_precision(mna, x, control);
            return Ok(());
        }
        Err(NewtonError::Interrupted(e)) => return Err(DcFailure::Interrupted(e)),
        Err(NewtonError::Singular(row)) => {
            let kind = mna.last_failure().unwrap_or(SolveFailure::Singular);
            if kind == SolveFailure::Singular && solved_by_inspection(mna, x, &base) {
                return Ok(());
            }
            // A linear circuit's matrix does not depend on the iterate, so
            // no continuation can make it regular.
            if mna.is_linear() {
                return Err(DcFailure::Singular(row, kind));
            }
            singular = Some((row, kind));
            notes.push(format!(
                "Newton–Raphson met a singular matrix at {}",
                mna.layout().name(row)
            ));
        }
        Err(NewtonError::NoConvergence) => {
            worst = mna.worst();
            let plural = if iterations == 1 { "" } else { "s" };
            notes.push(format!(
                "Newton–Raphson did not converge in {iterations} iteration{plural}"
            ));
            if let Some((i, noise)) = mna.precision_floor()
                && noise > FLOOR_LIMIT * x[i].abs()
            {
                let unit = if mna.layout().is_voltage(i) { "V" } else { "A" };
                notes.push(format!(
                    "double precision resolves {} only to about ±{} because element values \
                     around it span too many decades",
                    mna.layout().name(i),
                    crate::units::format_value(noise, unit)
                ));
            }
        }
    }
    let strategies: [Strategy; 3] = [gmin_stepping, source_stepping, pseudo_transient];
    for strategy in strategies {
        x.copy_from_slice(&guess);
        match strategy(mna, x, base, control) {
            Ok(()) => {
                check_precision(mna, x, control);
                return Ok(());
            }
            Err(Stall::Interrupted(e)) => return Err(DcFailure::Interrupted(e)),
            Err(Stall::At(note)) => notes.push(note),
        }
    }
    x.copy_from_slice(&guess);
    Err(match singular {
        Some((row, kind)) => DcFailure::Singular(row, kind),
        None => DcFailure::NoConvergence {
            detail: notes.join("; "),
            worst,
        },
    })
}

/// Whether `x = 0` solves the operating point under `conditions` exactly:
/// every source is 0 and no device carries current at zero bias, so the
/// right-hand side assembled at `x = 0` vanishes (the residual of the
/// circuit equations there is exactly zero), no device limits and every
/// device accepts it. LTspice reports such an operating point as "found by
/// inspection" without factoring the matrix; xsim falls back on it when the
/// matrix is singular to double precision (a micro-ohm resistance next to a
/// node held only by junction gmin, SR-1), where solving fails although the
/// answer is known. On success `x` is zero and the devices are loaded at it.
fn solved_by_inspection(mna: &mut Mna, x: &mut [f64], conditions: &Conditions) -> bool {
    let zero = vec![0.0; x.len()];
    let limited = mna.load(&zero, conditions, false, &[]);
    if limited || !mna.excitation_is_zero() || !mna.devices_converged(&zero) {
        return false;
    }
    x.fill(0.0);
    true
}

/// Warns about every node that an operating-point solve under `conditions`
/// ties to ground through gmin (D15).
fn warn_about_tied_nodes(mna: &Mna, conditions: &Conditions, control: &SimControl) {
    let (nodes, conductance) = mna.tied_nodes(conditions);
    for node in nodes {
        control.warn(format!(
            "Node {node} has no DC path to ground (it is reached only through capacitors); it \
             was tied to ground through gmin = {} for the operating point",
            crate::units::format_value(conductance, "S")
        ));
    }
}

/// Runs one Newton solve of a continuation strategy, turning cancellation
/// into an early return and every other failure into `false`.
fn attempt(
    mna: &mut Mna,
    x: &mut [f64],
    conditions: Conditions,
    init_junctions: bool,
    anchor: &[f64],
    control: &SimControl,
) -> Result<bool, Stall> {
    let request = Request {
        conditions,
        init_junctions,
        max_iterations: mna.options().dc_iterations,
        anchor,
    };
    match solve(mna, x, &request, control) {
        Ok(_) => Ok(true),
        Err(NewtonError::Interrupted(e)) => Err(Stall::Interrupted(e)),
        Err(NewtonError::Singular(_) | NewtonError::NoConvergence) => Ok(false),
    }
}

fn gmin_stepping(
    mna: &mut Mna,
    x: &mut [f64],
    base: Conditions,
    control: &SimControl,
) -> Result<(), Stall> {
    let floor = base.gmin.max(GMIN_FLOOR);
    let mut gmin = GMIN_START.max(floor);
    let mut factor = GMIN_FACTOR;
    let mut good = x.to_vec();
    let mut reached: Option<f64> = None;
    for _ in 0..MAX_CONTINUATION_SOLVES {
        let conditions = Conditions {
            gmin: gmin.max(base.gmin),
            node_gmin: gmin,
            ..base
        };
        x.copy_from_slice(&good);
        if attempt(mna, x, conditions, reached.is_none(), &[], control)? {
            good.copy_from_slice(x);
            reached = Some(gmin);
            if gmin <= floor {
                break;
            }
            gmin = (gmin / factor).max(floor);
        } else {
            let Some(last) = reached else {
                return Err(Stall::At(format!(
                    "GMIN stepping failed at its starting conductance of {}",
                    crate::units::format_value(gmin, "S")
                )));
            };
            factor = factor.sqrt();
            if factor < GMIN_MIN_FACTOR {
                return Err(Stall::At(format!(
                    "GMIN stepping stalled at {}",
                    crate::units::format_value(last, "S")
                )));
            }
            gmin = last / factor;
        }
    }
    let reached = reached.unwrap_or(gmin);
    x.copy_from_slice(&good);
    if attempt(mna, x, base, false, &[], control)? {
        Ok(())
    } else {
        Err(Stall::At(format!(
            "GMIN stepping reached {} but the circuit did not converge without it",
            crate::units::format_value(reached, "S")
        )))
    }
}

fn source_stepping(
    mna: &mut Mna,
    x: &mut [f64],
    base: Conditions,
    control: &SimControl,
) -> Result<(), Stall> {
    x.fill(0.0);
    let off = Conditions {
        source_factor: 0.0,
        ..base
    };
    if !attempt(mna, x, off, true, &[], control)? {
        return Err(Stall::At(
            "source stepping failed with every source at zero".into(),
        ));
    }
    let mut good = x.to_vec();
    let mut strength = 0.0;
    let mut step = SOURCE_STEP_START;
    for _ in 0..MAX_CONTINUATION_SOLVES {
        let target = (strength + step).min(1.0);
        let conditions = Conditions {
            source_factor: target,
            ..base
        };
        x.copy_from_slice(&good);
        if attempt(mna, x, conditions, false, &[], control)? {
            // At full strength these are exactly the base conditions.
            if target >= 1.0 {
                return Ok(());
            }
            good.copy_from_slice(x);
            strength = target;
            step *= 2.0;
        } else {
            step /= 4.0;
            if step < SOURCE_STEP_MIN {
                break;
            }
        }
    }
    Err(Stall::At(format!(
        "source stepping stalled at {:.1}% of the source values",
        strength * 100.0
    )))
}

fn pseudo_transient(
    mna: &mut Mna,
    x: &mut [f64],
    base: Conditions,
    control: &SimControl,
) -> Result<(), Stall> {
    let mut anchor = x.to_vec();
    let mut conductance = PSEUDO_START;
    let mut settled = false;
    for _ in 0..MAX_CONTINUATION_SOLVES {
        let conditions = Conditions {
            pseudo_conductance: conductance,
            ..base
        };
        x.copy_from_slice(&anchor);
        if attempt(mna, x, conditions, !settled, &anchor, control)? {
            anchor.copy_from_slice(x);
            settled = true;
            if conductance <= PSEUDO_RELEASE && attempt(mna, x, base, false, &[], control)? {
                return Ok(());
            }
            conductance /= PSEUDO_GROWTH;
            if conductance < PSEUDO_MIN {
                break;
            }
        } else {
            conductance *= 8.0;
            if conductance > PSEUDO_MAX {
                break;
            }
        }
    }
    Err(Stall::At(format!(
        "pseudo-transient continuation stalled at a pseudo conductance of {}",
        crate::units::format_value(conductance, "S")
    )))
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A minimal exponential junction for exercising the nonlinear machinery
    //! (limiting, junction initialisation, the convergence ladder) without
    //! depending on the production semiconductor models.

    use crate::analysis::SimOptions;
    use crate::circuit::NodeIndex;
    use crate::devices::{AcContext, DeviceInstance, LoadContext, SetupContext};
    use crate::mna::Unknown;
    use std::sync::{Arc, Mutex};

    pub(crate) const SATURATION: f64 = 1e-14;
    pub(crate) const THERMAL: f64 = 0.025;

    /// The `(gmin, source_factor)` of every load, shared with the test.
    pub(crate) type Log = Arc<Mutex<Vec<(f64, f64)>>>;

    /// `i = Is·(e^(v/Vt) − 1) + gmin·v` from anode to cathode.
    #[derive(Debug)]
    pub(crate) struct Junction {
        nodes: [NodeIndex; 2],
        terminals: [Unknown; 2],
        voltage: f64,
        current: f64,
        conductance: f64,
        gmin: f64,
        log: Option<Log>,
    }

    impl Junction {
        pub(crate) fn new(anode: NodeIndex, cathode: NodeIndex, log: Option<Log>) -> Self {
            Self {
                nodes: [anode, cathode],
                terminals: [None; 2],
                voltage: 0.0,
                current: 0.0,
                conductance: 0.0,
                gmin: 0.0,
                log,
            }
        }

        pub(crate) fn current_at(v: f64, gmin: f64) -> f64 {
            SATURATION * (v / THERMAL).exp_m1() + gmin * v
        }
    }

    fn critical_voltage() -> f64 {
        THERMAL * (THERMAL / (std::f64::consts::SQRT_2 * SATURATION)).ln()
    }

    /// SPICE `pnjlim`: limits forward-bias updates to a logarithmic step.
    fn limit(new: f64, old: f64) -> (f64, bool) {
        let critical = critical_voltage();
        if new <= critical || (new - old).abs() <= 2.0 * THERMAL {
            return (new, false);
        }
        let limited = if old > 0.0 {
            let arg = 1.0 + (new - old) / THERMAL;
            if arg > 0.0 {
                old + THERMAL * arg.ln()
            } else {
                critical
            }
        } else {
            THERMAL * (new / THERMAL).ln()
        };
        (limited, true)
    }

    fn value(x: &[f64], u: Unknown) -> f64 {
        u.map_or(0.0, |i| x[i])
    }

    impl DeviceInstance for Junction {
        fn setup(&mut self, ctx: &mut SetupContext) {
            self.terminals = self.nodes.map(|n| ctx.node(n));
        }

        fn load(&mut self, ctx: &mut LoadContext) {
            let [a, b] = self.terminals;
            let (v, limited) = if ctx.init_junctions {
                (critical_voltage(), false)
            } else {
                limit(ctx.v(a) - ctx.v(b), self.voltage)
            };
            ctx.limited |= limited;
            let e = (v / THERMAL).exp();
            self.current = Self::current_at(v, ctx.gmin);
            self.conductance = SATURATION / THERMAL * e + ctx.gmin;
            self.voltage = v;
            self.gmin = ctx.gmin;
            ctx.stamp_nonlinear(a, b, self.current, self.conductance, v);
            if let Some(log) = &self.log {
                log.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((ctx.gmin, ctx.source_factor));
            }
        }

        fn load_ac(&self, ctx: &mut AcContext) {
            let [a, b] = self.terminals;
            ctx.stamp_conductance(a, b, self.conductance);
        }

        fn currents(&self, out: &mut [f64]) {
            out[0] = self.current;
            out[1] = -self.current;
        }

        /// SPICE's current-prediction test.
        fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
            let [a, b] = self.terminals;
            let v = value(x, a) - value(x, b);
            let predicted = self.current + self.conductance * (v - self.voltage);
            let actual = Self::current_at(v, self.gmin);
            (actual - predicted).abs()
                <= options.reltol * actual.abs().max(predicted.abs()) + options.abstol
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{Junction, Log};
    use super::*;
    use crate::analysis::SimOptions;
    use crate::circuit::{Circuit, Device, Element, GROUND};
    use crate::devices;
    use crate::source::Source;
    use std::sync::Arc;

    /// `V1 → R1 → junction to ground`; the junction's place in the circuit
    /// description is held by a resistor of the same terminal count.
    fn junction_circuit(volts: f64, ohms: f64) -> Circuit {
        let mut c = Circuit::new();
        let (a, d) = (c.node("a"), c.node("d"));
        c.add(Element::new(
            "V1",
            [a, GROUND],
            Device::VoltageSource(Source::dc(volts)),
        ));
        c.add(Element::new("R1", [a, d], Device::resistor(ohms)));
        c.add(Element::new("D1", [d, GROUND], Device::resistor(1.0)));
        c
    }

    fn prepare(circuit: &Circuit, options: &SimOptions, log: Option<Log>) -> Mna {
        let mut instances: Vec<_> = (0..2)
            .map(|i| devices::build(circuit, i, options))
            .collect();
        instances.push(Box::new(Junction::new(2, GROUND, log)));
        Mna::with_devices(circuit, instances, options)
    }

    /// The junction voltage solving `(V − v)/R = i(v)`, by bisection.
    fn reference(volts: f64, ohms: f64, gmin: f64) -> f64 {
        let (mut low, mut high) = (0.0, volts);
        for _ in 0..200 {
            let mid = 0.5 * (low + high);
            if (volts - mid) / ohms > Junction::current_at(mid, gmin) {
                low = mid;
            } else {
                high = mid;
            }
        }
        0.5 * (low + high)
    }

    fn solve_dc(mna: &mut Mna, control: &SimControl) -> (Vec<f64>, Result<(), DcFailure>) {
        let mut x = vec![0.0; mna.size()];
        let conditions = Conditions::dc(mna.options(), false);
        let outcome = operating_point(mna, &mut x, conditions, true, control);
        (x, outcome)
    }

    #[test]
    fn direct_newton_solves_a_junction_without_continuation() {
        let options = SimOptions::default();
        let log = Log::default();
        let mut mna = prepare(
            &junction_circuit(5.0, 1e3),
            &options,
            Some(Arc::clone(&log)),
        );
        let (x, outcome) = solve_dc(&mut mna, &SimControl::new());
        outcome.unwrap();
        assert!((x[1] - reference(5.0, 1e3, options.gmin)).abs() < 1e-9);
        let log = log.lock().unwrap();
        assert!(
            log.iter()
                .all(|&(gmin, factor)| gmin == options.gmin && factor == 1.0)
        );
        // Junction initialisation, limited steps and a confirming iteration.
        assert!(log.len() > 3);
    }

    #[test]
    fn every_strategy_alone_finds_the_operating_point() {
        let options = SimOptions::default();
        let (volts, ohms) = (10.0, 1.0);
        let expected = reference(volts, ohms, options.gmin);
        let strategies: [(&str, Strategy); 3] = [
            ("gmin", gmin_stepping),
            ("source", source_stepping),
            ("pseudo", pseudo_transient),
        ];
        for (name, strategy) in strategies {
            let mut mna = prepare(&junction_circuit(volts, ohms), &options, None);
            let mut x = vec![0.0; mna.size()];
            let base = Conditions::dc(&options, false);
            assert!(
                strategy(&mut mna, &mut x, base, &SimControl::new()).is_ok(),
                "{name}"
            );
            assert!(
                (x[1] - expected).abs() < 1e-9,
                "{name}: {} vs {expected}",
                x[1]
            );
        }
    }

    #[test]
    fn the_ladder_continues_when_direct_newton_runs_out_of_iterations() {
        // Limiting needs many iterations to bring 10 V across 1 Ω down to
        // the junction voltage; three are not enough for direct Newton.
        let options = SimOptions {
            dc_iterations: 3,
            ..SimOptions::default()
        };
        let log = Log::default();
        let mut mna = prepare(
            &junction_circuit(10.0, 1.0),
            &options,
            Some(Arc::clone(&log)),
        );
        let (x, outcome) = solve_dc(&mut mna, &SimControl::new());
        outcome.unwrap();
        assert!((x[1] - reference(10.0, 1.0, options.gmin)).abs() < 1e-9);
        let log = log.lock().unwrap();
        let continued = log
            .iter()
            .any(|&(gmin, factor)| gmin > options.gmin || factor < 1.0);
        assert!(continued, "a continuation strategy must have run");
    }

    #[test]
    fn an_exhausted_ladder_explains_each_strategy() {
        // One iteration never confirms convergence of a nonlinear circuit.
        let options = SimOptions {
            dc_iterations: 1,
            ..SimOptions::default()
        };
        let mut mna = prepare(&junction_circuit(5.0, 1e3), &options, None);
        let (x, outcome) = solve_dc(&mut mna, &SimControl::new());
        let Err(DcFailure::NoConvergence { detail, worst }) = outcome else {
            panic!("expected non-convergence, got {outcome:?}");
        };
        // The only direct Newton step moved the source node furthest.
        assert_eq!(worst.map(|i| mna.layout().name(i)), Some("V(a)"));
        for strategy in [
            "in 1 iteration;",
            "GMIN stepping",
            "source stepping",
            "pseudo-transient",
        ] {
            assert!(detail.contains(strategy), "{detail}");
        }
        // The caller's guess is restored.
        assert!(x.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn a_numerically_singular_nonlinear_circuit_is_reported_after_the_ladder() {
        // A 1 µΩ resistor into a node held only by a junction: G + g rounds
        // to G, so the matrix is singular to double precision although
        // every node has a DC path to ground.
        let options = SimOptions::default();
        let mut circuit = junction_circuit(5.0, 1e3);
        let (m, k) = (circuit.node("m"), circuit.node("k"));
        circuit.add(Element::new("R2", [m, k], Device::resistor(1e-6)));
        circuit.add(Element::new("D2", [k, GROUND], Device::resistor(1.0)));
        let mut instances: Vec<_> = (0..2)
            .map(|i| devices::build(&circuit, i, &options))
            .collect();
        instances.push(Box::new(Junction::new(2, GROUND, None)));
        instances.push(devices::build(&circuit, 3, &options));
        instances.push(Box::new(Junction::new(k, GROUND, None)));
        let mut mna = Mna::with_devices(&circuit, instances, &options);
        let (_, outcome) = solve_dc(&mut mna, &SimControl::new());
        let Err(DcFailure::Singular(row, SolveFailure::Singular)) = outcome else {
            panic!("expected a singular matrix, got {outcome:?}");
        };
        assert!(["V(m)", "V(k)"].contains(&mna.layout().name(row)));
        let reason = mna.singular_reason(row, None);
        assert!(reason.contains("double precision"), "{reason}");
    }

    #[test]
    fn connectivity_obstacles_are_reported_without_iterating() {
        // I1 feeds an island with no return path: no operating point exists.
        let options = SimOptions::default();
        let mut circuit = junction_circuit(5.0, 1e3);
        let island = circuit.node("island");
        circuit.add(Element::new(
            "I1",
            [GROUND, island],
            Device::CurrentSource(Source::dc(1e-3)),
        ));
        let log = Log::default();
        let mut instances: Vec<_> = (0..2)
            .map(|i| devices::build(&circuit, i, &options))
            .collect();
        instances.push(Box::new(Junction::new(2, GROUND, Some(Arc::clone(&log)))));
        instances.push(devices::build(&circuit, 3, &options));
        let mut mna = Mna::with_devices(&circuit, instances, &options);
        let (_, outcome) = solve_dc(&mut mna, &SimControl::new());
        assert_eq!(outcome, Err(DcFailure::Singular(2, SolveFailure::Singular)));
        assert_eq!(mna.layout().name(2), "V(island)");
        assert!(log.lock().unwrap().is_empty(), "nothing was loaded");
        assert_eq!(
            mna.singular_reason(2, None),
            "node island has no DC path to ground (only current source I1 feeds it, so its \
             voltage is undefined)"
        );
    }

    #[test]
    fn capacitor_only_nodes_are_tied_to_ground_with_a_warning() {
        let options = SimOptions::default();
        let mut circuit = junction_circuit(5.0, 1e3);
        let island = circuit.node("island");
        circuit.add(Element::new("C1", [2, island], Device::capacitor(1e-9)));
        let mut instances: Vec<_> = (0..2)
            .map(|i| devices::build(&circuit, i, &options))
            .collect();
        instances.push(Box::new(Junction::new(2, GROUND, None)));
        instances.push(devices::build(&circuit, 3, &options));
        let mut mna = Mna::with_devices(&circuit, instances, &options);
        let control = SimControl::new();
        let (x, outcome) = solve_dc(&mut mna, &control);
        outcome.unwrap();
        assert_eq!(x[2], 0.0);
        assert!((x[1] - reference(5.0, 1e3, options.gmin)).abs() < 1e-9);
        assert_eq!(
            control.warnings(),
            [
                "Node island has no DC path to ground (it is reached only through capacitors); it \
              was tied to ground through gmin = 1 pS for the operating point"
            ]
        );
    }

    #[test]
    fn cancellation_interrupts_the_ladder() {
        let options = SimOptions::default();
        let mut mna = prepare(&junction_circuit(5.0, 1e3), &options, None);
        let control = SimControl::new();
        control.cancel();
        let (_, outcome) = solve_dc(&mut mna, &control);
        assert_eq!(
            outcome,
            Err(DcFailure::Interrupted(SimError::Cancelled {
                progress: 0.0
            }))
        );
    }

    #[test]
    fn a_limited_iteration_is_never_accepted() {
        // From a 0.9 V guess the first update is limited; even with a huge
        // tolerance the solve must take another iteration.
        let options = SimOptions {
            reltol: 1.0,
            vntol: 1e3,
            abstol: 1e3,
            ..SimOptions::default()
        };
        let log = Log::default();
        let mut mna = prepare(
            &junction_circuit(10.0, 1.0),
            &options,
            Some(Arc::clone(&log)),
        );
        let mut x = vec![10.0, 0.9, -1.0];
        let request = Request {
            conditions: Conditions::dc(&options, false),
            init_junctions: false,
            max_iterations: 50,
            anchor: &[],
        };
        let iterations = solve(&mut mna, &mut x, &request, &SimControl::new()).unwrap();
        assert!(iterations >= 2);
    }
}
