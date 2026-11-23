//! Op-amps (D26): an ideal op-amp and a single-pole macro-model.
//!
//! Terminals `[in+, in−, out, V+, V−]`; `vd = V(in+) − V(in−)`. Every
//! function below is smooth (continuous first derivatives), so Newton's
//! method sees exact Jacobians everywhere.
//!
//! ## Output limit
//! `sp(v) = k·ln(1 + e^(v/k))` is a softplus with the knee `k = 10 mV`
//! ([`KNEE`]). The output limits are `hi = V(V+) − headroom_pos` and
//! `lo = V(V−) + headroom_neg` (both headrooms are zero for the ideal
//! op-amp). A span guard keeps them ordered: with `mid = (hi + lo)/2` and
//! `half = (hi − lo)/2` the limits used are `mid ± sp(half)`, which equal
//! `hi` and `lo` within `k·e^(−half/k)` (below 10⁻¹⁸ V once `hi − lo`
//! exceeds 0.75 V) and never cross when the supplies leave no room. The
//! limited output voltage is
//!
//! `T(x) = x − sp(x − hi) + sp(lo − x)`,
//!
//! which differs from `x` by at most `k·e^(−d/k)` at a distance `d` from
//! both limits (so the closed-loop gain is exact there) and approaches `hi`
//! or `lo` exponentially beyond them without ever crossing them.
//!
//! ## Ideal op-amp
//! `x = aol·vd`, with no internal state: infinite bandwidth, no slew limit,
//! no input current, the output limited to the rails exactly.
//!
//! ## Macro-model
//! An internal unknown `x` (named `<element>:x`) follows
//!
//! * `e = aol·vd − x − c(x)`, where the clamp
//!   `c(x) = G·(sp(x − hi − m) − sp(lo − m − x))` with `G = 10⁹` and
//!   `m = 0.5 V` ([`CLAMP_GAIN`], [`CLAMP_MARGIN`]) keeps `x` within about
//!   `m` beyond the output limits, so the output recovers from saturation
//!   after about `m/slew` instead of slewing back from `aol·vd`. Inside
//!   the output range `c(x)` is below `G·k·e^(−m/k)` ≈ 2·10⁻¹⁵ V.
//! * operating points, DC sweeps and the AC linearisation point: `e = 0`;
//! * transient: `τ·dx/dt = L·tanh(e/L)` with `τ = aol/(2π·gbw)` and
//!   `L = slew·τ`. In small signal this is `τ·dx/dt = aol·vd − x`: one pole
//!   at `gbw/aol`, so the AC open-loop gain is exactly
//!   `aol/(1 + j·f·aol/gbw)`. In large signal (`|e| ≫ L`) the slope of `x` is
//!   `±slew` exactly. The state is the "charge" `τ·x`, integrated like a
//!   capacitor's by every integration method.
//!
//! A transient that starts from rest holds `x` at 0 V at `t = 0`.
//!
//! ## Output stage and supply currents
//! The output current `i` (a branch unknown `<element>:out`, positive out
//! of the output pin) satisfies `V(out) = T(x) − rout·i`. The macro-model
//! adds `rin` between the inputs. The output current is drawn from V+ while
//! sourcing and returned to V− while sinking: `i_p = i·(1 + tanh(i/ε))/2`
//! flows into the V+ pin and `i_n = i − i_p` into the V− pin, with
//! `ε = 1 µA` ([`SPLIT_CURRENT`]); there is no quiescent current, and the
//! currents into the five terminals,
//! `[vd/rin, −vd/rin, −i, i_p, i_n]`, sum to zero.

use super::semiconductor::solution_value;
use super::{AcContext, InitialConstraints, LoadContext, LoadMode, SetupContext, StateSlot};
use crate::analysis::SimOptions;
use crate::circuit::NodeIndex;
use crate::devices::DeviceInstance;
use crate::mna::Unknown;
use crate::models::{OpAmpKind, OpAmpModel};
use num_complex::Complex64;
use std::f64::consts::PI;

/// Knee of every smooth limit (V).
pub(crate) const KNEE: f64 = 0.01;
/// How far beyond the output limits the macro-model's state is clamped (V).
pub(crate) const CLAMP_MARGIN: f64 = 0.5;
/// Gain of the macro-model's state clamp.
pub(crate) const CLAMP_GAIN: f64 = 1e9;
/// Output current over which the supply split changes from V− to V+ (A).
pub(crate) const SPLIT_CURRENT: f64 = 1e-6;

/// `sp(v) = k·ln(1 + e^(v/k))` and its slope `σ(v/k)`, without overflow
/// or cancellation.
fn softplus(v: f64) -> (f64, f64) {
    let e = (-v.abs() / KNEE).exp();
    let value = v.max(0.0) + KNEE * e.ln_1p();
    let slope = if v >= 0.0 {
        1.0 / (1.0 + e)
    } else {
        e / (1.0 + e)
    };
    (value, slope)
}

/// The output limits and their derivatives with respect to `V(V+)` (`_p`)
/// and `V(V−)` (`_n`).
#[derive(Debug, Clone, Copy, Default)]
struct Rails {
    hi: f64,
    lo: f64,
    hi_p: f64,
    hi_n: f64,
    lo_p: f64,
    lo_n: f64,
}

impl Rails {
    fn new(vp: f64, vn: f64, headroom_pos: f64, headroom_neg: f64) -> Self {
        let (top, bottom) = (vp - headroom_pos, vn + headroom_neg);
        let mid = 0.5 * (top + bottom);
        let (half, slope) = softplus(0.5 * (top - bottom));
        let (wide, narrow) = (0.5 + 0.5 * slope, 0.5 - 0.5 * slope);
        Self {
            hi: mid + half,
            lo: mid - half,
            hi_p: wide,
            hi_n: narrow,
            lo_p: narrow,
            lo_n: wide,
        }
    }

    /// `T(x)` with `dT/dx`, `dT/dV(V+)` and `dT/dV(V−)`. The two equal forms
    /// of `T` are evaluated on the side of `x` that avoids cancellation.
    fn limit(&self, x: f64) -> Smooth {
        let (value, dx, d_hi, d_lo) = if x >= 0.5 * (self.hi + self.lo) {
            let (a, ga) = softplus(self.hi - x);
            let (b, gb) = softplus(self.lo - x);
            (self.hi - a + b, ga - gb, 1.0 - ga, gb)
        } else {
            let (a, ga) = softplus(x - self.hi);
            let (b, gb) = softplus(x - self.lo);
            (self.lo - a + b, gb - ga, ga, 1.0 - gb)
        };
        self.chain(value, dx, d_hi, d_lo)
    }

    /// The macro-model's state clamp `c(x)` and its derivatives.
    fn clamp(&self, x: f64) -> Smooth {
        let (a, ga) = softplus(x - self.hi - CLAMP_MARGIN);
        let (b, gb) = softplus(self.lo - CLAMP_MARGIN - x);
        self.chain(
            CLAMP_GAIN * (a - b),
            CLAMP_GAIN * (ga + gb),
            -CLAMP_GAIN * ga,
            -CLAMP_GAIN * gb,
        )
    }

    fn chain(&self, value: f64, dx: f64, d_hi: f64, d_lo: f64) -> Smooth {
        Smooth {
            value,
            dx,
            dp: d_hi * self.hi_p + d_lo * self.lo_p,
            dn: d_hi * self.hi_n + d_lo * self.lo_n,
        }
    }
}

/// A function of `x`, `V(V+)` and `V(V−)` with its partial derivatives.
#[derive(Debug, Clone, Copy, Default)]
struct Smooth {
    value: f64,
    dx: f64,
    dp: f64,
    dn: f64,
}

impl Smooth {
    /// The linear prediction at `(x, vp, vn)` from the point `at`.
    fn predict(&self, at: &Point, x: f64, vp: f64, vn: f64) -> f64 {
        self.value + self.dx * (x - at.x) + self.dp * (vp - at.vp) + self.dn * (vn - at.vn)
    }
}

/// Current `i_p` into the V+ pin for the output current `i`, and `di_p/di`.
fn supply_split(i: f64) -> (f64, f64) {
    let t = (i / SPLIT_CURRENT).tanh();
    let share = 0.5 * (1.0 + t);
    (i * share, share + 0.5 * i * (1.0 - t * t) / SPLIT_CURRENT)
}

/// The controlling values of one evaluation.
#[derive(Debug, Clone, Copy, Default)]
struct Point {
    vd: f64,
    x: f64,
    vp: f64,
    vn: f64,
    i: f64,
}

/// Model evaluation at the last load, for currents, AC and convergence.
#[derive(Debug, Clone, Copy, Default)]
struct Linearisation {
    at: Point,
    output: Smooth,
    clamp: Smooth,
    i_p: f64,
    di_p: f64,
    /// `L·tanh(e/L)` and its slope `sech²(e/L)` (macro-model).
    drive: f64,
    slope: f64,
    /// True when the drive was part of the last load (a transient step).
    dynamic: bool,
}

#[derive(Debug)]
pub(crate) struct OpAmp {
    nodes: [NodeIndex; 5],
    ideal: bool,
    aol: f64,
    /// `τ = aol/(2π·gbw)` (s) and `L = slew·τ` (V), macro-model only.
    tau: f64,
    swing: f64,
    /// Input conductance `1/rin` (0 for the ideal op-amp).
    gin: f64,
    rout: f64,
    headroom: [f64; 2],
    pins: [Unknown; 5],
    output: Unknown,
    state: Unknown,
    slot: Option<StateSlot>,
    last: Linearisation,
}

impl OpAmp {
    pub(crate) fn new(nodes: &[NodeIndex], model: &OpAmpModel) -> Self {
        let ideal = model.kind == OpAmpKind::Ideal;
        let tau = if ideal {
            0.0
        } else {
            model.aol / (2.0 * PI * model.gbw)
        };
        Self {
            nodes: std::array::from_fn(|k| nodes[k]),
            ideal,
            aol: model.aol,
            tau,
            swing: model.slew * tau,
            gin: if ideal { 0.0 } else { 1.0 / model.rin },
            rout: model.rout,
            headroom: if ideal {
                [0.0, 0.0]
            } else {
                [model.headroom_pos, model.headroom_neg]
            },
            pins: [None; 5],
            output: None,
            state: None,
            slot: None,
            last: Linearisation::default(),
        }
    }

    /// The controlling values in the solution `x`.
    fn point(&self, x: &[f64]) -> Point {
        let [inp, inn, _, vp, vn] = self.pins;
        let vd = solution_value(x, inp) - solution_value(x, inn);
        Point {
            vd,
            x: if self.ideal {
                self.aol * vd
            } else {
                solution_value(x, self.state)
            },
            vp: solution_value(x, vp),
            vn: solution_value(x, vn),
            i: solution_value(x, self.output),
        }
    }

    fn rails(&self, at: &Point) -> Rails {
        Rails::new(at.vp, at.vn, self.headroom[0], self.headroom[1])
    }

    /// `e = aol·vd − x − c(x)` at `at`.
    fn error(&self, at: &Point, clamp: &Smooth) -> f64 {
        self.aol * at.vd - at.x - clamp.value
    }

    /// Stamps the row `x` of `u` with coefficient `g` and the derivative
    /// `g·dx/du` for the ideal op-amp, whose `x` is `aol·vd`.
    fn stamp_x(&self, ctx: &mut LoadContext, row: Unknown, g: f64) {
        let [inp, inn, ..] = self.pins;
        if self.ideal {
            ctx.system.add(row, inp, g * self.aol);
            ctx.system.add(row, inn, -g * self.aol);
        } else {
            ctx.system.add(row, self.state, g);
        }
    }

    /// The macro-model's state row; returns the drive, its slope and
    /// whether the drive was used.
    fn load_state(&self, ctx: &mut LoadContext, at: &Point, clamp: &Smooth) -> (f64, f64, bool) {
        let [inp, inn, _, vp, vn] = self.pins;
        let row = self.state;
        let q = self.tau * at.x;
        let (dq, geq) = match self.slot {
            Some(slot) => ctx.integrate(slot, q, self.tau),
            None => (0.0, 0.0),
        };
        let e = self.error(at, clamp);
        let held = ctx.initial_constraints() == InitialConstraints::All;
        let (rate, slope) = {
            let t = (e / self.swing).tanh();
            (self.swing * t, 1.0 - t * t)
        };
        if held {
            // At rest at t = 0: x = 0.
            ctx.system.add(row, row, 1.0);
            return (rate, slope, false);
        }
        // The residual r = w·(x + c(x) − aol·vd) + d with weight w and
        // remainder d: the static row `x + c − aol·vd = 0` (w = 1, d = 0)
        // or the transient row `τ·dx/dt − L·tanh(e/L) = 0`.
        let (w, residual, own) = match ctx.mode {
            LoadMode::Dc => (1.0, -e, 0.0),
            LoadMode::Transient => (slope, dq - rate, geq),
        };
        let dx = own + w * (1.0 + clamp.dx);
        ctx.system.add(row, row, dx);
        ctx.system.add(row, inp, -w * self.aol);
        ctx.system.add(row, inn, w * self.aol);
        ctx.system.add(row, vp, w * clamp.dp);
        ctx.system.add(row, vn, w * clamp.dn);
        let jacobian_at =
            dx * at.x - w * self.aol * at.vd + w * clamp.dp * at.vp + w * clamp.dn * at.vn;
        ctx.system.add_rhs(row, jacobian_at - residual);
        (rate, slope, ctx.mode == LoadMode::Transient)
    }
}

/// `|actual − predicted|` within the relative tolerance plus `floor`.
fn close(actual: f64, predicted: f64, reltol: f64, floor: f64) -> bool {
    let delta = actual - predicted;
    delta.is_finite() && delta.abs() <= reltol * actual.abs().max(predicted.abs()) + floor
}

impl DeviceInstance for OpAmp {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.pins = self.nodes.map(|n| ctx.node(n));
        self.output = Some(ctx.branch("out"));
        if !self.ideal {
            self.state = ctx.internal_node("x");
            self.slot = Some(ctx.state());
        }
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [inp, inn, out, vp, vn] = self.pins;
        let at = self.point(ctx.x);
        let rails = self.rails(&at);
        let output = rails.limit(at.x);
        let clamp = rails.clamp(at.x);
        let b = self.output;

        // V(out) + rout·i − T(x) = 0.
        ctx.system.add(b, out, 1.0);
        ctx.system.add(b, b, self.rout);
        self.stamp_x(ctx, b, -output.dx);
        ctx.system.add(b, vp, -output.dp);
        ctx.system.add(b, vn, -output.dn);
        ctx.system.add_rhs(
            b,
            output.value - output.dx * at.x - output.dp * at.vp - output.dn * at.vn,
        );

        // The output current leaves the output pin, i_p enters V+, i_n V−.
        let (i_p, di_p) = supply_split(at.i);
        ctx.system.add(out, b, -1.0);
        ctx.system.add(vp, b, di_p);
        ctx.system.add_rhs(vp, -(i_p - di_p * at.i));
        ctx.system.add(vn, b, 1.0 - di_p);
        ctx.system.add_rhs(vn, i_p - di_p * at.i);

        if self.gin > 0.0 {
            ctx.stamp_conductance(inp, inn, self.gin);
        }
        let (drive, slope, dynamic) = if self.ideal {
            (0.0, 1.0, false)
        } else {
            self.load_state(ctx, &at, &clamp)
        };
        self.last = Linearisation {
            at,
            output,
            clamp,
            i_p,
            di_p,
            drive,
            slope,
            dynamic,
        };
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [inp, inn, out, vp, vn] = self.pins;
        let one = Complex64::new(1.0, 0.0);
        let real = |v: f64| Complex64::new(v, 0.0);
        let l = &self.last;
        let b = self.output;
        ctx.system.add(b, out, one);
        ctx.system.add(b, b, real(self.rout));
        if self.ideal {
            ctx.system.add(b, inp, real(-l.output.dx * self.aol));
            ctx.system.add(b, inn, real(l.output.dx * self.aol));
        } else {
            ctx.system.add(b, self.state, real(-l.output.dx));
        }
        ctx.system.add(b, vp, real(-l.output.dp));
        ctx.system.add(b, vn, real(-l.output.dn));
        ctx.system.add(out, b, -one);
        ctx.system.add(vp, b, real(l.di_p));
        ctx.system.add(vn, b, real(1.0 - l.di_p));
        if self.gin > 0.0 {
            ctx.stamp_conductance(inp, inn, self.gin);
        }
        if !self.ideal {
            // jωτ·x = s·(aol·vd − x − c(x)), s = sech²(e/L) at the
            // operating point (1, since e = 0 there).
            let s = l.slope;
            let row = self.state;
            ctx.system.add(
                row,
                row,
                Complex64::new(s * (1.0 + l.clamp.dx), ctx.omega * self.tau),
            );
            ctx.system.add(row, inp, real(-s * self.aol));
            ctx.system.add(row, inn, real(s * self.aol));
            ctx.system.add(row, vp, real(s * l.clamp.dp));
            ctx.system.add(row, vn, real(s * l.clamp.dn));
        }
    }

    fn currents(&self, out: &mut [f64]) {
        let l = &self.last;
        let input = self.gin * l.at.vd;
        for (slot, value) in out
            .iter_mut()
            .zip([input, -input, -l.at.i, l.i_p, l.at.i - l.i_p])
        {
            *slot = value;
        }
    }

    fn ac_currents(&self, x: &[Complex64], _omega: f64, out: &mut [Complex64]) -> bool {
        let [inp, inn, ..] = self.pins;
        let input = (solution_value(x, inp) - solution_value(x, inn)) * self.gin;
        let i = solution_value(x, self.output);
        let di_p = self.last.di_p;
        for (slot, value) in out
            .iter_mut()
            .zip([input, -input, -i, i * di_p, i * (1.0 - di_p)])
        {
            *slot = value;
        }
        true
    }

    /// The limited output voltage, the supply split and (in a transient
    /// step) the slew-limited drive must match their linear predictions.
    fn converged(&self, x: &[f64], options: &SimOptions) -> bool {
        let l = &self.last;
        let at = self.point(x);
        let rails = self.rails(&at);
        let output = rails.limit(at.x);
        let predicted = l.output.predict(&l.at, at.x, at.vp, at.vn);
        if !close(output.value, predicted, options.reltol, options.vntol) {
            return false;
        }
        let (i_p, _) = supply_split(at.i);
        if !close(
            i_p,
            l.i_p + l.di_p * (at.i - l.at.i),
            options.reltol,
            options.abstol,
        ) {
            return false;
        }
        if !l.dynamic {
            return true;
        }
        let clamp = rails.clamp(at.x);
        let e = self.error(&at, &clamp);
        let e_last = self.error(&l.at, &l.clamp);
        let drive = self.swing * (e / self.swing).tanh();
        close(
            drive,
            l.drive + l.slope * (e - e_last),
            options.reltol,
            options.vntol,
        )
    }

    fn initial_conditions(
        &self,
        states: &mut Vec<(StateSlot, f64)>,
        unknowns: &mut Vec<(usize, f64)>,
    ) {
        let _ = unknowns;
        if let Some(slot) = self.slot {
            states.push((slot, 0.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of the smooth functions of the rails.
    type Evaluate = fn(&Rails, f64) -> Smooth;

    /// Central difference of `f` at `v`.
    fn derivative(f: impl Fn(f64) -> f64, v: f64) -> f64 {
        let h = 1e-7;
        (f(v + h) - f(v - h)) / (2.0 * h)
    }

    #[test]
    fn limit_is_the_identity_inside_and_the_rail_outside() {
        let rails = Rails::new(15.0, -15.0, 1.5, 1.5);
        assert!((rails.hi - 13.5).abs() < 1e-15 && (rails.lo + 13.5).abs() < 1e-15);
        for x in [-13.0, -1.0, 0.0, 2.5, 13.0] {
            let t = rails.limit(x);
            assert!((t.value - x).abs() < 1e-12, "{x}: {}", t.value);
            assert!((t.dx - 1.0).abs() < 1e-12);
        }
        for x in [14.0, 1e3, 1e12] {
            assert!((rails.limit(x).value - 13.5).abs() < 1e-12);
            assert!((rails.limit(-x).value + 13.5).abs() < 1e-12);
        }
        // Never beyond the limits, and monotonic.
        let mut previous = f64::NEG_INFINITY;
        for k in -2000..=2000 {
            let v = rails.limit(f64::from(k) * 0.01).value;
            assert!((-13.5..=13.5).contains(&v) && v >= previous);
            previous = v;
        }
    }

    #[test]
    fn derivatives_are_exact() {
        for (vp, vn) in [
            (15.0, -15.0),
            (5.0, 0.0),
            (0.01, 0.0),
            (0.0, 0.0),
            (-1.0, 1.0),
        ] {
            for x in [-20.0, -14.0, -13.49, 0.0, 0.004, 4.98, 13.52, 14.1, 20.0] {
                let r = Rails::new(vp, vn, 0.05, 0.05);
                let functions: [(Evaluate, &str); 2] =
                    [(|r, x| r.limit(x), "T"), (|r, x| r.clamp(x), "c")];
                for (f, what) in functions {
                    let s = f(&r, x);
                    let scale = s.dx.abs().max(1.0);
                    let tol = 1e-6 * scale;
                    let dx = derivative(|x| f(&r, x).value, x);
                    let dp = derivative(|v| f(&Rails::new(v, vn, 0.05, 0.05), x).value, vp);
                    let dn = derivative(|v| f(&Rails::new(vp, v, 0.05, 0.05), x).value, vn);
                    assert!((dx - s.dx).abs() <= tol, "{what} x {x} {vp} {vn}");
                    assert!((dp - s.dp).abs() <= tol, "{what} vp {x} {vp} {vn}");
                    assert!((dn - s.dn).abs() <= tol, "{what} vn {x} {vp} {vn}");
                }
            }
        }
        for i in [-1e-3, -2e-6, -1e-7, 0.0, 3e-7, 1e-6, 1e-3] {
            let (_, d) = supply_split(i);
            let numeric = (supply_split(i + 1e-12).0 - supply_split(i - 1e-12).0) / 2e-12;
            assert!((numeric - d).abs() < 1e-6, "{i}");
        }
    }

    #[test]
    fn supply_split_sends_sourcing_current_to_v_plus_and_sinking_to_v_minus() {
        assert_eq!(supply_split(0.0).0, 0.0);
        assert_eq!(supply_split(1e-3).0, 1e-3);
        assert_eq!(supply_split(-1e-3).0, 0.0);
    }
}
