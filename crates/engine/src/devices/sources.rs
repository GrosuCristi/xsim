//! Independent voltage and current sources.
//!
//! Which value a source applies depends on the solve ([`source_value`]):
//! its DC value for operating-point, DC-sweep and AC analyses, its waveform
//! at `t` for the transient's initial operating point, and the waveform's
//! left limit at the end of every transient step, so an ideal edge that a
//! step lands on takes effect in the step after it. Source stepping scales
//! every value by [`LoadContext::source_factor`].

use super::{AcContext, DeviceInstance, LoadContext, LoadMode, SetupContext};
use crate::circuit::NodeIndex;
use crate::mna::Unknown;
use crate::source::Source;
use num_complex::Complex64;

/// The value `source` applies in the solve described by `ctx`.
fn source_value(source: &Source, ctx: &LoadContext) -> f64 {
    let value = match (ctx.use_waveforms, ctx.mode) {
        (false, _) => source.dc,
        (true, LoadMode::Dc) => source.transient_value(ctx.time),
        (true, LoadMode::Transient) => source.transient_value_before(ctx.time),
    };
    ctx.source_factor * value
}

/// The small-signal stimulus of `source`.
fn ac_phasor(source: &Source) -> Complex64 {
    Complex64::from_polar(source.ac_magnitude, source.ac_phase.to_radians())
}

#[derive(Debug)]
pub(crate) struct VoltageSource {
    nodes: [NodeIndex; 2],
    source: Source,
    terminals: [Unknown; 2],
    /// Branch-current unknown; assigned in `setup`.
    branch: usize,
    /// Branch current (into the positive terminal) at the last load.
    current: f64,
}

impl VoltageSource {
    pub(crate) fn new(nodes: &[NodeIndex], source: Source) -> Self {
        Self {
            nodes: [nodes[0], nodes[1]],
            source,
            terminals: [None; 2],
            branch: 0,
            current: 0.0,
        }
    }
}

impl DeviceInstance for VoltageSource {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.terminals = self.nodes.map(|n| ctx.node(n));
        self.branch = ctx.branch("");
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [a, b] = self.terminals;
        let br = Some(self.branch);
        ctx.system.add(a, br, 1.0);
        ctx.system.add(b, br, -1.0);
        ctx.system.add(br, a, 1.0);
        ctx.system.add(br, b, -1.0);
        let value = source_value(&self.source, ctx);
        ctx.system.add_rhs(br, value);
        self.current = ctx.x[self.branch];
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [a, b] = self.terminals;
        let br = Some(self.branch);
        let one = Complex64::new(1.0, 0.0);
        ctx.system.add(a, br, one);
        ctx.system.add(b, br, -one);
        ctx.system.add(br, a, one);
        ctx.system.add(br, b, -one);
        ctx.system.add_rhs(br, ac_phasor(&self.source));
    }

    fn currents(&self, out: &mut [f64]) {
        out[0] = self.current;
        out[1] = -self.current;
    }

    fn is_linear(&self) -> bool {
        true
    }

    fn set_dc(&mut self, value: f64) {
        self.source.dc = value;
    }

    fn breakpoints(&self, stop: f64, out: &mut Vec<f64>) {
        if let Some(waveform) = &self.source.waveform {
            out.extend(waveform.breakpoints(stop));
        }
    }
}

#[derive(Debug)]
pub(crate) struct CurrentSource {
    nodes: [NodeIndex; 2],
    source: Source,
    terminals: [Unknown; 2],
    /// Applied current (through the source from + to −) at the last load.
    current: f64,
}

impl CurrentSource {
    pub(crate) fn new(nodes: &[NodeIndex], source: Source) -> Self {
        Self {
            nodes: [nodes[0], nodes[1]],
            source,
            terminals: [None; 2],
            current: 0.0,
        }
    }
}

impl DeviceInstance for CurrentSource {
    fn setup(&mut self, ctx: &mut SetupContext) {
        self.terminals = self.nodes.map(|n| ctx.node(n));
    }

    fn load(&mut self, ctx: &mut LoadContext) {
        let [a, b] = self.terminals;
        self.current = source_value(&self.source, ctx);
        ctx.stamp_current(a, b, self.current);
    }

    fn load_ac(&self, ctx: &mut AcContext) {
        let [a, b] = self.terminals;
        ctx.stamp_current(a, b, ac_phasor(&self.source));
    }

    fn currents(&self, out: &mut [f64]) {
        out[0] = self.current;
        out[1] = -self.current;
    }

    fn is_linear(&self) -> bool {
        true
    }

    fn set_dc(&mut self, value: f64) {
        self.source.dc = value;
    }

    fn breakpoints(&self, stop: f64, out: &mut Vec<f64>) {
        if let Some(waveform) = &self.source.waveform {
            out.extend(waveform.breakpoints(stop));
        }
    }
}
