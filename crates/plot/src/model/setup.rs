//! Edits of a [`PlotSetup`]: traces, panes, axis modes and ranges.
//!
//! Panes share one x axis (Q128), so every function that changes the x range
//! or the x scale writes it to all panes. Each function returns whether the
//! setup changed, so the viewer emits `SetupChanged` only for real edits.
//!
//! Mixed units (D9): a linear or logarithmic pane shows up to two units,
//! the second on a right y axis (see [`super::units`]). A trace whose unit
//! would be a third one in the active pane goes to the first pane that
//! already shows that unit, otherwise to a new pane appended below the
//! others. The active pane does not change: it stays the user's choice, so
//! the next voltage probe still joins the other voltages, while further
//! traces of the new unit join the new pane.
//!
//! Pinned colours (D11): every trace carries a palette slot, the lowest one
//! no other trace of its pane uses when it is added. Slots are never
//! reassigned, so removing a trace never recolours another; the same trace
//! in the Bode magnitude and phase panes shares one slot.
//!
//! Moving a trace to another pane (D14, [`move_trace`]) keeps its slot
//! unless the target pane already uses it, and is refused when the target
//! already plots the expression or would get a third unit (D9).

use super::axis::AxisRange;
use super::units::{PaneUnits, expression_unit};
use xsim_project::{PlotPane, PlotSetup, Trace, YScale};
use xsim_spice_engine::{AnalysisKind, SimulationResult};

/// Whether two expressions are the same trace (whitespace is insignificant).
pub fn same_expression(a: &str, b: &str) -> bool {
    a.chars()
        .filter(|c| !c.is_whitespace())
        .eq(b.chars().filter(|c| !c.is_whitespace()))
}

/// Index of the pane new traces go to, clamped to the existing panes.
pub fn active_pane(setup: &PlotSetup) -> usize {
    setup.active_pane.min(setup.panes.len().saturating_sub(1))
}

/// The first decibel pane and the first phase pane, if both exist.
pub fn bode_pair(setup: &PlotSetup) -> Option<(usize, usize)> {
    let magnitude = setup
        .panes
        .iter()
        .position(|p| p.y_scale == YScale::Decibel)?;
    let phase = setup
        .panes
        .iter()
        .position(|p| p.y_scale == YScale::Phase)?;
    Some((magnitude, phase))
}

/// Adds `expression` to the active pane unless it is already there, with
/// the lowest palette slot that pane does not use (D11). On an AC result
/// whose active pane belongs to the Bode pair, the trace goes to both the
/// magnitude and the phase pane (Q134), with one slot free in both. A trace
/// whose unit does not fit the active pane goes elsewhere (D9, see
/// [`target_pane`]).
pub fn add_trace(setup: &mut PlotSetup, expression: &str, ac: bool) -> bool {
    let expression = expression.trim();
    if expression.is_empty() {
        return false;
    }
    ensure_pane(setup);
    let active = active_pane(setup);
    let targets = match bode_pair(setup) {
        Some((magnitude, phase)) if ac && (active == magnitude || active == phase) => {
            vec![magnitude, phase]
        }
        _ => vec![target_pane(setup, active, expression)],
    };
    let existing = targets
        .iter()
        .find_map(|&index| find(&setup.panes[index], expression))
        .and_then(|trace| trace.slot);
    let missing: Vec<usize> = targets
        .into_iter()
        .filter(|&index| find(&setup.panes[index], expression).is_none())
        .collect();
    if missing.is_empty() {
        return false;
    }
    let slot = existing
        .filter(|&slot| missing.iter().all(|&i| !uses_slot(&setup.panes[i], slot)))
        .unwrap_or_else(|| free_slot(&setup.panes, &missing));
    for index in missing {
        let pane = &mut setup.panes[index];
        let before = units(pane);
        pane.traces.push(Trace {
            slot: Some(slot),
            ..Trace::new(expression)
        });
        retarget_ranges(pane, before);
    }
    true
}

/// The pane a new trace `expression` goes to while pane `active` is active
/// (D9): the active pane when the trace is already there or its unit fits
/// (it is the first or second unit there, or unknown); otherwise the first
/// pane that already holds the trace or shows its unit; otherwise the first
/// linear or logarithmic pane without any known unit yet (an empty pane the
/// user added); otherwise a new pane appended below the others, sharing
/// their x axis. The active pane stays.
pub fn target_pane(setup: &mut PlotSetup, active: usize, expression: &str) -> usize {
    let unit = expression_unit(expression);
    let fits = |pane: &PlotPane| find(pane, expression).is_some() || units(pane).accepts(unit);
    if setup.panes.get(active).is_none_or(fits) {
        return active;
    }
    let shown = setup
        .panes
        .iter()
        .position(|pane| find(pane, expression).is_some() || units(pane).carries(unit));
    let unused = || {
        setup.panes.iter().position(|pane| {
            let units = units(pane);
            !units.single && units.left.is_empty()
        })
    };
    shown.or_else(unused).unwrap_or_else(|| push_pane(setup))
}

/// The trace of `pane` with expression `expression`.
fn find<'a>(pane: &'a PlotPane, expression: &str) -> Option<&'a Trace> {
    pane.traces
        .iter()
        .find(|t| same_expression(&t.expression, expression))
}

/// The y-axis units of `pane` (D9).
pub fn units(pane: &PlotPane) -> PaneUnits {
    PaneUnits::of(pane, expression_unit)
}

fn uses_slot(pane: &PlotPane, slot: u32) -> bool {
    pane.traces.iter().any(|t| t.slot == Some(slot))
}

/// The lowest palette slot no trace of the panes `indices` uses.
fn free_slot(panes: &[PlotPane], indices: &[usize]) -> u32 {
    (0..u32::MAX)
        .find(|&slot| {
            !indices
                .iter()
                .filter_map(|&i| panes.get(i))
                .any(|pane| uses_slot(pane, slot))
        })
        .unwrap_or(0)
}

/// Removes trace `trace` of pane `pane`. Every other trace keeps its slot
/// and so its colour (D11); stored y ranges stay with their unit when the
/// right axis's unit moves to the left axis.
pub fn remove_trace(setup: &mut PlotSetup, pane: usize, trace: usize) -> bool {
    match setup.panes.get_mut(pane) {
        Some(p) if trace < p.traces.len() => {
            let before = units(p);
            p.traces.remove(trace);
            retarget_ranges(p, before);
            true
        }
        _ => false,
    }
}

/// Keeps stored y ranges with the unit they were set for after the pane's
/// axis units changed from `before`: when the right unit becomes the left
/// one its range moves along, a left axis that loses its known unit (to
/// another one, or because no trace with a known unit is left) returns to
/// auto, and so does a right axis whose unit changes. A pane emptied by
/// removing or moving its traces therefore keeps no range that the next
/// trace, of any unit, would inherit.
fn retarget_ranges(pane: &mut PlotPane, before: PaneUnits) {
    let after = units(pane);
    if after.left != before.left {
        if !after.left.is_empty() && before.right == Some(after.left) {
            pane.y_range = pane.y2_range.take();
        } else if !before.left.is_empty() {
            pane.y_range = None;
        }
    }
    if after.right != before.right || after.right.is_none() {
        pane.y2_range = None;
    }
}

/// Why a trace cannot move to another pane ([`move_trace`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRefusal {
    /// The trace or one of the panes does not exist.
    Missing,
    /// The trace is already in that pane.
    SamePane,
    /// The target pane already plots the same expression.
    AlreadyThere,
    /// The target pane already shows two other units (D9).
    ThirdUnit,
}

impl MoveRefusal {
    /// A short explanation for the drop target.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Missing | Self::SamePane => "",
            Self::AlreadyThere => "This pane already shows this trace",
            Self::ThirdUnit => "This pane already shows two other units",
        }
    }
}

/// Whether trace `trace` of pane `from` may move to pane `to`: the panes
/// differ, `to` does not plot the expression yet, and its unit fits there
/// (one of its two units, or a free axis; D9).
pub fn can_move_trace(
    setup: &PlotSetup,
    from: usize,
    trace: usize,
    to: usize,
) -> Result<(), MoveRefusal> {
    let source = setup.panes.get(from).ok_or(MoveRefusal::Missing)?;
    let moved = source.traces.get(trace).ok_or(MoveRefusal::Missing)?;
    let target = setup.panes.get(to).ok_or(MoveRefusal::Missing)?;
    if from == to {
        Err(MoveRefusal::SamePane)
    } else if find(target, &moved.expression).is_some() {
        Err(MoveRefusal::AlreadyThere)
    } else if !units(target).accepts(expression_unit(&moved.expression)) {
        Err(MoveRefusal::ThirdUnit)
    } else {
        Ok(())
    }
}

/// Moves trace `trace` of pane `from` to the end of pane `to` when
/// [`can_move_trace`] allows it, and returns its index there. The trace
/// keeps its palette slot, and so its colour (D11), unless `to` already
/// uses that slot; then it takes the lowest slot free in `to`. Stored y
/// ranges of both panes follow their units, as when a trace is added or
/// removed.
pub fn move_trace(
    setup: &mut PlotSetup,
    from: usize,
    trace: usize,
    to: usize,
) -> Result<usize, MoveRefusal> {
    can_move_trace(setup, from, trace, to)?;
    let source = &mut setup.panes[from];
    let before = units(source);
    let moved = source.traces.remove(trace);
    retarget_ranges(source, before);
    let slot = moved
        .slot
        .filter(|&slot| !uses_slot(&setup.panes[to], slot))
        .unwrap_or_else(|| free_slot(&setup.panes, &[to]));
    let target = &mut setup.panes[to];
    let before = units(target);
    target.traces.push(Trace {
        slot: Some(slot),
        ..moved
    });
    retarget_ranges(target, before);
    Ok(target.traces.len() - 1)
}

/// Shows or hides trace `trace` of pane `pane`.
pub fn toggle_visible(setup: &mut PlotSetup, pane: usize, trace: usize) -> bool {
    let Some(t) = setup
        .panes
        .get_mut(pane)
        .and_then(|p| p.traces.get_mut(trace))
    else {
        return false;
    };
    t.visible = !t.visible;
    true
}

/// Appends an empty pane sharing the x axis and makes it active.
pub fn add_pane(setup: &mut PlotSetup) -> usize {
    ensure_pane(setup);
    setup.active_pane = push_pane(setup);
    setup.active_pane
}

/// Appends an empty pane sharing the x axis; returns its index.
fn push_pane(setup: &mut PlotSetup) -> usize {
    let pane = match setup.panes.first() {
        Some(first) => PlotPane {
            x_range: first.x_range,
            x_log: first.x_log,
            ..PlotPane::default()
        },
        None => PlotPane::default(),
    };
    setup.panes.push(pane);
    setup.panes.len() - 1
}

/// Removes pane `index`; the last remaining pane cannot be removed.
pub fn remove_pane(setup: &mut PlotSetup, index: usize) -> bool {
    if setup.panes.len() <= 1 || index >= setup.panes.len() {
        return false;
    }
    setup.panes.remove(index);
    if setup.active_pane >= index && setup.active_pane > 0 {
        setup.active_pane -= 1;
    }
    true
}

/// Makes pane `index` the one new traces go to.
pub fn set_active(setup: &mut PlotSetup, index: usize) -> bool {
    if index >= setup.panes.len() || setup.active_pane == index {
        return false;
    }
    setup.active_pane = index;
    true
}

/// Switches the y scale of pane `index`; its y ranges return to auto. A
/// pane switched from decibels or phase to a linear or logarithmic axis may
/// hold more than two units; the extra ones move to other panes (D9).
pub fn set_y_scale(setup: &mut PlotSetup, index: usize, scale: YScale) -> bool {
    match setup.panes.get_mut(index) {
        Some(pane) if pane.y_scale != scale => {
            pane.y_scale = scale;
            pane.y_range = None;
            pane.y2_range = None;
            separate_units(setup);
            true
        }
        _ => false,
    }
}

/// Switches every pane's x axis between linear and logarithmic (`None` =
/// automatic); the x range returns to auto.
pub fn set_x_log(setup: &mut PlotSetup, log: Option<bool>) -> bool {
    let changed = setup.panes.iter().any(|p| p.x_log != log);
    for pane in &mut setup.panes {
        pane.x_log = log;
        pane.x_range = None;
    }
    changed
}

/// Sets the shared x range of every pane (`None` = auto).
pub fn set_x_range(setup: &mut PlotSetup, range: Option<[f64; 2]>) -> bool {
    let changed = setup.panes.iter().any(|p| p.x_range != range);
    for pane in &mut setup.panes {
        pane.x_range = range;
    }
    changed
}

/// Sets the y range of pane `index` (`None` = auto).
pub fn set_y_range(setup: &mut PlotSetup, index: usize, range: Option<[f64; 2]>) -> bool {
    match setup.panes.get_mut(index) {
        Some(pane) if pane.y_range != range => {
            pane.y_range = range;
            true
        }
        _ => false,
    }
}

/// Sets the range of the right y axis of pane `index` (`None` = auto). A
/// pane without a right axis stores none.
pub fn set_y2_range(setup: &mut PlotSetup, index: usize, range: Option<[f64; 2]>) -> bool {
    match setup.panes.get_mut(index) {
        Some(pane)
            if pane.y2_range != range && (range.is_none() || units(pane).right.is_some()) =>
        {
            pane.y2_range = range;
            true
        }
        _ => false,
    }
}

/// Returns every axis to its automatic range.
pub fn reset_ranges(setup: &mut PlotSetup) -> bool {
    let mut changed = set_x_range(setup, None);
    for index in 0..setup.panes.len() {
        changed |= set_y_range(setup, index, None);
        changed |= set_y2_range(setup, index, None);
    }
    changed
}

/// Whether the shared x axis is logarithmic for a result of kind `kind`.
pub fn x_log(setup: &PlotSetup, kind: AnalysisKind) -> bool {
    setup
        .panes
        .first()
        .and_then(|p| p.x_log)
        .unwrap_or(kind == AnalysisKind::Ac)
}

/// Adapts a stored setup to a newly arrived result, idempotently:
/// * there is at least one pane and the active pane exists;
/// * every pane shares pane 0's x range and x scale;
/// * an x range that misses the new data, or is not positive on a log
///   axis, returns to auto (a transient range reused on an AC run);
/// * an AC result gets the Bode layout (magnitude in dB above phase) and a
///   non-AC result collapses an untouched Bode layout back into one pane;
/// * no linear or logarithmic pane shows more than two units (D9), and a
///   pane without a right axis stores no range for it;
/// * every trace has a palette slot ([`assign_slots`]).
pub fn adapt_to_result(setup: &mut PlotSetup, result: &SimulationResult) -> bool {
    let before = setup.clone();
    ensure_pane(setup);
    setup.active_pane = active_pane(setup);
    if result.analysis == AnalysisKind::Ac {
        make_bode(setup);
    } else {
        collapse_bode(setup);
    }
    separate_units(setup);
    share_x_axis(setup);
    drop_stale_x_range(setup, result);
    for pane in &mut setup.panes {
        if units(pane).right.is_none() {
            pane.y2_range = None;
        }
    }
    assign_slots(setup);
    *setup != before
}

/// Gives every trace without a palette slot (files written before D11,
/// traces created by the shell) the lowest slot its pane does not use yet,
/// in pane order. A trace in the Bode magnitude and phase panes gets one
/// slot free in both, or takes its counterpart's. Existing slots are kept,
/// so no colour on screen changes. Returns whether any slot was assigned.
pub fn assign_slots(setup: &mut PlotSetup) -> bool {
    let pair = bode_pair(setup);
    let mut changed = false;
    for index in 0..setup.panes.len() {
        for trace in 0..setup.panes[index].traces.len() {
            if setup.panes[index].traces[trace].slot.is_some() {
                continue;
            }
            let partner = match pair {
                Some((magnitude, phase)) if index == magnitude => Some(phase),
                Some((magnitude, phase)) if index == phase => Some(magnitude),
                _ => None,
            };
            let expression = &setup.panes[index].traces[trace].expression;
            let counterpart = partner.and_then(|p| {
                let position = setup.panes[p]
                    .traces
                    .iter()
                    .position(|t| same_expression(&t.expression, expression))?;
                Some((p, position))
            });
            let shared = counterpart
                .and_then(|(p, t)| setup.panes[p].traces[t].slot)
                .filter(|&slot| !uses_slot(&setup.panes[index], slot));
            let slot = shared.unwrap_or_else(|| {
                let mut panes = vec![index];
                panes.extend(counterpart.map(|(p, _)| p));
                free_slot(&setup.panes, &panes)
            });
            setup.panes[index].traces[trace].slot = Some(slot);
            if let Some((p, t)) = counterpart {
                setup.panes[p].traces[t].slot.get_or_insert(slot);
            }
            changed = true;
        }
    }
    changed
}

/// Moves every trace whose unit would be a third one in its linear or
/// logarithmic pane (a merged Bode layout, a pane switched from decibels,
/// a hand-written file) to the pane [`target_pane`] picks for it. A moved
/// trace keeps its slot unless its new pane already uses it.
fn separate_units(setup: &mut PlotSetup) {
    let mut index = 0;
    while index < setup.panes.len() {
        let pane = &mut setup.panes[index];
        let fitting = units(pane);
        let (kept, extra): (Vec<Trace>, Vec<Trace>) = std::mem::take(&mut pane.traces)
            .into_iter()
            .partition(|t| fitting.accepts(expression_unit(&t.expression)));
        pane.traces = kept;
        for trace in extra {
            let target = target_pane(setup, index, &trace.expression);
            let pane = &mut setup.panes[target];
            if find(pane, &trace.expression).is_some() {
                continue;
            }
            let free = trace.slot.filter(|&slot| !uses_slot(pane, slot));
            pane.traces.push(Trace {
                slot: free,
                ..trace
            });
        }
        index += 1;
    }
    assign_slots(setup);
}

fn ensure_pane(setup: &mut PlotSetup) {
    if setup.panes.is_empty() {
        setup.panes.push(PlotPane::default());
        setup.active_pane = 0;
    }
}

fn share_x_axis(setup: &mut PlotSetup) {
    let (range, log) = (setup.panes[0].x_range, setup.panes[0].x_log);
    for pane in &mut setup.panes[1..] {
        pane.x_range = range;
        pane.x_log = log;
    }
}

fn drop_stale_x_range(setup: &mut PlotSetup, result: &SimulationResult) {
    let Some([min, max]) = setup.panes[0].x_range else {
        return;
    };
    let log = x_log(setup, result.analysis);
    let data = super::traces::extent(&result.axis.values);
    let overlaps = data.is_some_and(|(lo, hi)| min <= hi && max >= lo);
    if !overlaps || AxisRange::new(min, max, log).is_none() {
        set_x_range(setup, None);
    }
}

/// Turns a setup without decibel or phase panes into the Bode layout, moving
/// every trace into both panes.
fn make_bode(setup: &mut PlotSetup) {
    let has_bode_scale = setup
        .panes
        .iter()
        .any(|p| matches!(p.y_scale, YScale::Decibel | YScale::Phase));
    if has_bode_scale {
        return;
    }
    let mut traces: Vec<Trace> = Vec::new();
    for trace in setup.panes.iter().flat_map(|p| p.traces.iter()) {
        if !traces
            .iter()
            .any(|t| same_expression(&t.expression, &trace.expression))
        {
            // Traces of different panes may share a slot; the later one
            // gets a free slot (see `assign_slots`).
            let clash = traces
                .iter()
                .any(|t| t.slot.is_some() && t.slot == trace.slot);
            traces.push(Trace {
                slot: if clash { None } else { trace.slot },
                ..trace.clone()
            });
        }
    }
    let pane = |y_scale| PlotPane {
        traces: traces.clone(),
        y_scale,
        ..PlotPane::default()
    };
    setup.panes = vec![pane(YScale::Decibel), pane(YScale::Phase)];
    setup.active_pane = 0;
}

/// Collapses exactly the automatic Bode layout (a decibel pane above a phase
/// pane with the same traces) into one linear pane for real-valued results.
fn collapse_bode(setup: &mut PlotSetup) {
    let [magnitude, phase] = setup.panes.as_slice() else {
        return;
    };
    let automatic = magnitude.y_scale == YScale::Decibel
        && phase.y_scale == YScale::Phase
        && magnitude.traces == phase.traces;
    if automatic {
        let traces = magnitude.traces.clone();
        setup.panes = vec![PlotPane {
            traces,
            ..PlotPane::default()
        }];
        setup.active_pane = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_spice_engine::result::Axis;

    fn result(kind: AnalysisKind, axis: Vec<f64>) -> SimulationResult {
        SimulationResult {
            analysis: kind,
            axis: Axis {
                name: "time".into(),
                unit: "s".into(),
                values: axis,
            },
            signals: Vec::new(),
        }
    }

    fn traces(setup: &PlotSetup, pane: usize) -> Vec<&str> {
        setup.panes[pane]
            .traces
            .iter()
            .map(|t| t.expression.as_str())
            .collect()
    }

    #[test]
    fn add_trace_deduplicates_ignoring_whitespace() {
        let mut setup = PlotSetup::default();
        assert!(add_trace(&mut setup, "V(out) - V(in)", false));
        assert!(!add_trace(&mut setup, "V(out)-V(in)", false));
        assert!(!add_trace(&mut setup, "   ", false));
        assert_eq!(traces(&setup, 0), ["V(out) - V(in)"]);
    }

    #[test]
    fn add_trace_targets_the_active_pane() {
        let mut setup = PlotSetup::default();
        add_pane(&mut setup);
        add_trace(&mut setup, "V(out)", false);
        assert!(setup.panes[0].traces.is_empty());
        assert_eq!(traces(&setup, 1), ["V(out)"]);
        setup.active_pane = 7;
        add_trace(&mut setup, "I(R1)", false);
        assert_eq!(traces(&setup, 1), ["V(out)", "I(R1)"]);
    }

    #[test]
    fn ac_probe_adds_to_both_bode_panes() {
        let mut setup = PlotSetup::default();
        adapt_to_result(&mut setup, &result(AnalysisKind::Ac, vec![1.0, 10.0]));
        assert_eq!(bode_pair(&setup), Some((0, 1)));
        add_trace(&mut setup, "V(out)", true);
        assert_eq!(traces(&setup, 0), ["V(out)"]);
        assert_eq!(traces(&setup, 1), ["V(out)"]);
    }

    #[test]
    fn bode_layout_is_idempotent_and_reversible() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        let ac = result(AnalysisKind::Ac, vec![1.0, 1e6]);
        assert!(adapt_to_result(&mut setup, &ac));
        assert_eq!(setup.panes.len(), 2);
        assert_eq!(setup.panes[0].y_scale, YScale::Decibel);
        assert_eq!(setup.panes[1].y_scale, YScale::Phase);
        assert!(!adapt_to_result(&mut setup, &ac));
        let tran = result(AnalysisKind::Transient, vec![0.0, 1.0]);
        assert!(adapt_to_result(&mut setup, &tran));
        assert_eq!(setup.panes.len(), 1);
        assert_eq!(traces(&setup, 0), ["V(out)"]);
    }

    #[test]
    fn stale_or_invalid_x_ranges_return_to_auto() {
        let mut setup = PlotSetup::default();
        set_x_range(&mut setup, Some([0.0, 1e-3]));
        adapt_to_result(
            &mut setup,
            &result(AnalysisKind::Transient, vec![0.0, 5e-3]),
        );
        assert_eq!(setup.panes[0].x_range, Some([0.0, 1e-3]));
        adapt_to_result(&mut setup, &result(AnalysisKind::Transient, vec![1.0, 2.0]));
        assert_eq!(setup.panes[0].x_range, None);
        setup.panes[0].x_range = Some([0.0, 10.0]);
        adapt_to_result(&mut setup, &result(AnalysisKind::Ac, vec![1.0, 100.0]));
        assert!(setup.panes.iter().all(|p| p.x_range.is_none()));
    }

    #[test]
    fn panes_share_the_x_axis() {
        let mut setup = PlotSetup::default();
        add_pane(&mut setup);
        set_x_range(&mut setup, Some([1.0, 2.0]));
        assert!(setup.panes.iter().all(|p| p.x_range == Some([1.0, 2.0])));
        setup.panes[1].x_range = None;
        adapt_to_result(&mut setup, &result(AnalysisKind::DcSweep, vec![0.0, 5.0]));
        assert_eq!(setup.panes[1].x_range, Some([1.0, 2.0]));
        assert!(set_x_log(&mut setup, Some(true)));
        assert!(
            setup
                .panes
                .iter()
                .all(|p| p.x_log == Some(true) && p.x_range.is_none())
        );
    }

    #[test]
    fn pane_removal_keeps_one_pane_and_a_valid_active_index() {
        let mut setup = PlotSetup::default();
        add_pane(&mut setup);
        add_pane(&mut setup);
        assert_eq!(setup.active_pane, 2);
        assert!(remove_pane(&mut setup, 2));
        assert_eq!(setup.active_pane, 1);
        assert!(remove_pane(&mut setup, 0));
        assert_eq!(setup.active_pane, 0);
        assert!(!remove_pane(&mut setup, 0));
    }

    #[test]
    fn trace_and_scale_edits_report_changes() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(a)", false);
        assert!(toggle_visible(&mut setup, 0, 0));
        assert!(!setup.panes[0].traces[0].visible);
        assert!(!toggle_visible(&mut setup, 0, 3));
        setup.panes[0].y_range = Some([0.0, 1.0]);
        assert!(set_y_scale(&mut setup, 0, YScale::Log));
        assert_eq!(setup.panes[0].y_range, None);
        assert!(!set_y_scale(&mut setup, 0, YScale::Log));
        assert!(remove_trace(&mut setup, 0, 0));
        assert!(!remove_trace(&mut setup, 0, 0));
        assert!(!set_active(&mut setup, 0));
        assert!(!reset_ranges(&mut setup));
    }

    fn slots(setup: &PlotSetup, pane: usize) -> Vec<Option<u32>> {
        setup.panes[pane].traces.iter().map(|t| t.slot).collect()
    }

    fn pane_with(expressions: &[&str]) -> PlotPane {
        PlotPane {
            traces: expressions.iter().map(|e| Trace::new(*e)).collect(),
            ..PlotPane::default()
        }
    }

    #[test]
    fn a_second_unit_shares_the_pane_and_a_third_gets_a_new_one() {
        let mut setup = PlotSetup::default();
        set_x_range(&mut setup, Some([1e-3, 2e-3]));
        for expression in ["V(out)", "I(R1)", "V(in)", "I(C1)"] {
            assert!(add_trace(&mut setup, expression, false));
        }
        assert_eq!(setup.panes.len(), 1);
        let axes = units(&setup.panes[0]);
        assert_eq!((axes.left, axes.right), ("V", Some("A")));

        // Power is a third unit: a new pane below, sharing the x axis. The
        // active pane stays, so the next voltage joins the other voltages.
        assert!(add_trace(&mut setup, "V(out)*I(R1)", false));
        assert_eq!(setup.panes.len(), 2);
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)"]);
        assert_eq!(setup.panes[1].x_range, Some([1e-3, 2e-3]));
        assert_eq!(setup.active_pane, 0);
        add_trace(&mut setup, "V(n1)", false);
        assert_eq!(
            traces(&setup, 0),
            ["V(out)", "I(R1)", "V(in)", "I(C1)", "V(n1)"]
        );

        // More power traces join the power pane; a duplicate changes nothing.
        assert!(add_trace(&mut setup, "V(in)*I(R1)", false));
        assert!(!add_trace(&mut setup, "V(out) * I(R1)", false));
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)", "V(in)*I(R1)"]);
        assert_eq!(setup.panes.len(), 2);

        // Unknown units fit any pane, on its left axis.
        assert!(add_trace(&mut setup, "V(out)/V(in)", false));
        assert_eq!(setup.panes[0].traces.len(), 6);
    }

    #[test]
    fn a_third_unit_fills_an_empty_pane_before_adding_one() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        add_trace(&mut setup, "I(R1)", false);
        add_pane(&mut setup);
        set_active(&mut setup, 0);
        assert!(add_trace(&mut setup, "V(out)*I(R1)", false));
        assert_eq!(setup.panes.len(), 2);
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)"]);
        assert_eq!(setup.active_pane, 0);
    }

    #[test]
    fn unparsable_expressions_stay_in_the_active_pane() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        add_trace(&mut setup, "I(R1)", false);
        assert!(add_trace(&mut setup, "V(out", false));
        assert_eq!(setup.panes.len(), 1);
    }

    #[test]
    fn panes_with_three_units_are_split_when_a_result_arrives() {
        let mut setup = PlotSetup {
            panes: vec![pane_with(&[
                "V(out)",
                "I(R1)",
                "V(out)*I(R1)",
                "V(in)",
                "V(in)*I(R1)",
            ])],
            active_pane: 0,
        };
        let tran = result(AnalysisKind::Transient, vec![0.0, 1.0]);
        assert!(adapt_to_result(&mut setup, &tran));
        assert_eq!(traces(&setup, 0), ["V(out)", "I(R1)", "V(in)"]);
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)", "V(in)*I(R1)"]);
        assert_eq!(setup.active_pane, 0);
        assert!(!adapt_to_result(&mut setup, &tran), "idempotent");

        // Collapsing a Bode layout of three units splits it the same way.
        let mut setup = PlotSetup::default();
        for expression in ["V(out)", "I(R1)", "V(out)*I(R1)"] {
            setup.panes[0].traces.push(Trace::new(expression));
        }
        adapt_to_result(&mut setup, &result(AnalysisKind::Ac, vec![1.0, 10.0]));
        assert_eq!(traces(&setup, 0), ["V(out)", "I(R1)", "V(out)*I(R1)"]);
        assert!(adapt_to_result(&mut setup, &tran));
        assert_eq!(traces(&setup, 0), ["V(out)", "I(R1)"]);
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)"]);
    }

    #[test]
    fn switching_a_decibel_pane_to_linear_moves_a_third_unit_out() {
        let mut setup = PlotSetup {
            panes: vec![PlotPane {
                y_scale: YScale::Decibel,
                ..pane_with(&["V(out)", "I(R1)", "V(out)*I(R1)"])
            }],
            active_pane: 0,
        };
        assign_slots(&mut setup);
        assert!(set_y_scale(&mut setup, 0, YScale::Linear));
        assert_eq!(traces(&setup, 0), ["V(out)", "I(R1)"]);
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)"]);
        assert_eq!(slots(&setup, 1), [Some(2)], "a moved trace keeps its slot");
    }

    #[test]
    fn right_axis_ranges_follow_their_unit() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        assert!(
            !set_y2_range(&mut setup, 0, Some([0.0, 1e-3])),
            "no right axis"
        );
        add_trace(&mut setup, "I(R1)", false);
        set_y_range(&mut setup, 0, Some([0.0, 2.0]));
        assert!(set_y2_range(&mut setup, 0, Some([0.0, 1e-3])));

        // Removing the only voltage moves the current, and its range, left.
        let mut moved = setup.clone();
        remove_trace(&mut moved, 0, 0);
        assert_eq!(moved.panes[0].y_range, Some([0.0, 1e-3]));
        assert_eq!(moved.panes[0].y2_range, None);

        // Removing the current drops the right axis and its range.
        let mut dropped = setup.clone();
        remove_trace(&mut dropped, 0, 1);
        assert_eq!(dropped.panes[0].y_range, Some([0.0, 2.0]));
        assert_eq!(dropped.panes[0].y2_range, None);

        // Removing the last trace leaves no range for the next one, which
        // may have another unit.
        remove_trace(&mut dropped, 0, 0);
        assert_eq!(dropped.panes[0].y_range, None);
        add_trace(&mut dropped, "I(R1)", false);
        assert_eq!(dropped.panes[0].y_range, None);

        assert!(reset_ranges(&mut setup));
        assert_eq!(
            (setup.panes[0].y_range, setup.panes[0].y2_range),
            (None, None)
        );
        set_y2_range(&mut setup, 0, Some([0.0, 1e-3]));
        assert!(set_y_scale(&mut setup, 0, YScale::Log));
        assert_eq!(setup.panes[0].y2_range, None);
    }

    #[test]
    fn stale_right_ranges_are_dropped_when_a_result_arrives() {
        let mut setup = PlotSetup {
            panes: vec![PlotPane {
                y2_range: Some([0.0, 1.0]),
                ..pane_with(&["V(out)"])
            }],
            active_pane: 0,
        };
        adapt_to_result(&mut setup, &result(AnalysisKind::Transient, vec![0.0, 1.0]));
        assert_eq!(setup.panes[0].y2_range, None);
    }

    #[test]
    fn traces_pin_the_lowest_free_slot_and_removal_recolours_nothing() {
        let mut setup = PlotSetup::default();
        for expression in ["V(a)", "V(b)", "V(c)"] {
            add_trace(&mut setup, expression, false);
        }
        assert_eq!(slots(&setup, 0), [Some(0), Some(1), Some(2)]);
        remove_trace(&mut setup, 0, 1);
        assert_eq!(slots(&setup, 0), [Some(0), Some(2)], "others keep theirs");
        add_trace(&mut setup, "V(d)", false);
        add_trace(&mut setup, "V(e)", false);
        assert_eq!(slots(&setup, 0), [Some(0), Some(2), Some(1), Some(3)]);
        // Slots are per pane: a new pane starts from the first colour.
        add_pane(&mut setup);
        add_trace(&mut setup, "V(f)", false);
        assert_eq!(slots(&setup, 1), [Some(0)]);
    }

    #[test]
    fn unslotted_traces_get_slots_once_in_pane_order() {
        let mut setup = PlotSetup::default();
        setup.panes[0].traces = vec![
            Trace::new("V(a)"),
            Trace {
                slot: Some(0),
                ..Trace::new("V(b)")
            },
            Trace::new("V(c)"),
        ];
        assert!(assign_slots(&mut setup));
        assert_eq!(slots(&setup, 0), [Some(1), Some(0), Some(2)]);
        assert!(!assign_slots(&mut setup));
        let tran = result(AnalysisKind::Transient, vec![0.0, 1.0]);
        let mut fresh = PlotSetup::default();
        fresh.panes[0].traces = vec![Trace::new("V(a)"), Trace::new("V(b)")];
        assert!(
            adapt_to_result(&mut fresh, &tran),
            "assigning slots is a change"
        );
        assert_eq!(slots(&fresh, 0), [Some(0), Some(1)]);
        assert!(!adapt_to_result(&mut fresh, &tran));
    }

    #[test]
    fn bode_panes_share_one_slot_per_trace() {
        let mut setup = PlotSetup::default();
        let ac = result(AnalysisKind::Ac, vec![1.0, 10.0]);
        adapt_to_result(&mut setup, &ac);
        add_trace(&mut setup, "V(out)", true);
        add_trace(&mut setup, "V(in)", true);
        assert_eq!(slots(&setup, 0), [Some(0), Some(1)]);
        assert_eq!(slots(&setup, 1), [Some(0), Some(1)]);
        remove_trace(&mut setup, 1, 0);
        add_trace(&mut setup, "V(n1)", true);
        // Slot 0 is still taken in the magnitude pane, so both get slot 2.
        assert_eq!(slots(&setup, 0), [Some(0), Some(1), Some(2)]);
        assert_eq!(slots(&setup, 1), [Some(1), Some(2)]);

        // Merging panes into the Bode layout resolves slot clashes.
        let mut setup = PlotSetup {
            panes: vec![pane_with(&["V(a)"]), pane_with(&["V(b)"])],
            active_pane: 0,
        };
        assign_slots(&mut setup);
        assert_eq!(
            (slots(&setup, 0), slots(&setup, 1)),
            (vec![Some(0)], vec![Some(0)])
        );
        adapt_to_result(&mut setup, &ac);
        assert_eq!(slots(&setup, 0), [Some(0), Some(1)]);
        assert_eq!(slots(&setup, 1), [Some(0), Some(1)]);
        assert!(!adapt_to_result(&mut setup, &ac));
        // An untouched Bode layout collapses back with the same slots.
        adapt_to_result(&mut setup, &result(AnalysisKind::Transient, vec![0.0, 1.0]));
        assert_eq!(setup.panes.len(), 1);
        assert_eq!(slots(&setup, 0), [Some(0), Some(1)]);
    }

    #[test]
    fn moving_a_trace_keeps_its_colour_unless_the_slot_is_taken() {
        let mut setup = PlotSetup::default();
        for expression in ["V(a)", "V(b)", "V(c)"] {
            add_trace(&mut setup, expression, false);
        }
        add_pane(&mut setup);
        add_trace(&mut setup, "V(d)", false);
        assert_eq!(slots(&setup, 1), [Some(0)]);
        // V(b) keeps slot 1: the lower pane does not use it.
        assert_eq!(move_trace(&mut setup, 0, 1, 1), Ok(1));
        assert_eq!(traces(&setup, 0), ["V(a)", "V(c)"]);
        assert_eq!(traces(&setup, 1), ["V(d)", "V(b)"]);
        assert_eq!(slots(&setup, 1), [Some(0), Some(1)]);
        assert_eq!(slots(&setup, 0), [Some(0), Some(2)], "others keep theirs");
        // V(a)'s slot 0 is taken below, so it gets the lowest free one.
        assert_eq!(move_trace(&mut setup, 0, 0, 1), Ok(2));
        assert_eq!(slots(&setup, 1), [Some(0), Some(1), Some(2)]);
        // Visibility travels with the trace.
        setup.panes[1].traces[0].visible = false;
        assert_eq!(move_trace(&mut setup, 1, 0, 0), Ok(1));
        assert!(!setup.panes[0].traces[1].visible);
    }

    #[test]
    fn moves_are_refused_by_the_unit_rule_and_duplicates() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        add_trace(&mut setup, "I(R1)", false);
        add_pane(&mut setup);
        add_trace(&mut setup, "V(out)*I(R1)", false);
        add_trace(&mut setup, "I(R1)", false);
        assert_eq!(traces(&setup, 1), ["V(out)*I(R1)", "I(R1)"]);
        let before = setup.clone();
        // Power would be a third unit next to volts and amperes (D9), and
        // so would volts next to watts and amperes.
        assert_eq!(move_trace(&mut setup, 1, 0, 0), Err(MoveRefusal::ThirdUnit));
        assert_eq!(move_trace(&mut setup, 0, 0, 1), Err(MoveRefusal::ThirdUnit));
        assert_eq!(
            move_trace(&mut setup, 0, 1, 1),
            Err(MoveRefusal::AlreadyThere)
        );
        assert_eq!(move_trace(&mut setup, 0, 0, 0), Err(MoveRefusal::SamePane));
        assert_eq!(move_trace(&mut setup, 0, 5, 1), Err(MoveRefusal::Missing));
        assert_eq!(move_trace(&mut setup, 0, 0, 7), Err(MoveRefusal::Missing));
        assert_eq!(setup, before, "refused moves change nothing");
        // An empty pane takes anything.
        add_pane(&mut setup);
        assert_eq!(can_move_trace(&setup, 0, 0, 2), Ok(()));
        assert_eq!(move_trace(&mut setup, 0, 0, 2), Ok(0));
        assert_eq!(traces(&setup, 2), ["V(out)"]);
        let left = units(&setup.panes[0]);
        assert_eq!((left.left, left.right), ("A", None));
        assert!(MoveRefusal::ThirdUnit.reason().contains("two"));
    }

    #[test]
    fn moving_the_right_axis_unit_keeps_ranges_with_their_unit() {
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        add_trace(&mut setup, "I(R1)", false);
        set_y_range(&mut setup, 0, Some([0.0, 2.0]));
        set_y2_range(&mut setup, 0, Some([0.0, 1e-3]));
        add_pane(&mut setup);
        assert_eq!(move_trace(&mut setup, 0, 1, 1), Ok(0));
        assert_eq!(setup.panes[0].y_range, Some([0.0, 2.0]));
        assert_eq!(setup.panes[0].y2_range, None, "the right axis is gone");
        // Moving the voltage away leaves the current alone on the left.
        let mut setup = PlotSetup::default();
        add_trace(&mut setup, "V(out)", false);
        add_trace(&mut setup, "I(R1)", false);
        set_y2_range(&mut setup, 0, Some([0.0, 1e-3]));
        add_pane(&mut setup);
        assert_eq!(move_trace(&mut setup, 0, 0, 1), Ok(0));
        assert_eq!(setup.panes[0].y_range, Some([0.0, 1e-3]));
        // Moving the current away too empties the pane: its range goes, so
        // a trace dropped there later, of any unit, starts fitted.
        assert_eq!(move_trace(&mut setup, 0, 0, 1), Ok(1));
        assert_eq!(setup.panes[0].y_range, None);
    }

    #[test]
    fn x_scale_defaults_to_log_for_ac() {
        let mut setup = PlotSetup::default();
        assert!(x_log(&setup, AnalysisKind::Ac));
        assert!(!x_log(&setup, AnalysisKind::Transient));
        setup.panes[0].x_log = Some(false);
        assert!(!x_log(&setup, AnalysisKind::Ac));
    }
}
