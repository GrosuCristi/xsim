//! Which traces a result shows: the first plot after a run, schematic
//! probes (Q62, Q130) and whether a stored result still fits the drawing.

use xsim_editor::ProbeRequest;
use xsim_project::{Netlist, PlotPane, PlotSetup, Schematic, StoredResult, Trace};
use xsim_spice_engine::{Quantity, SignalOrigin, SimulationResult};

/// Most traces added automatically after the first run.
pub const MAX_DEFAULT_TRACES: usize = 4;

/// The trace expression for a probe: `V(out)`, `V(a,b)` or `I(R1)`.
pub fn probe_expression(probe: &ProbeRequest) -> String {
    match probe {
        ProbeRequest::Voltage { node } => format!("V({node})"),
        ProbeRequest::Differential { plus, minus } => format!("V({plus},{minus})"),
        ProbeRequest::Current { signal } => signal.clone(),
    }
}

/// `true` when no pane plots anything yet.
pub fn is_empty(setup: &PlotSetup) -> bool {
    setup.panes.iter().all(|pane| pane.traces.is_empty())
}

/// Traces for a circuit plotted for the first time: the voltages of the
/// labelled nets in label order (the nets the user named are the ones they
/// care about), otherwise the first node voltage.
pub fn default_traces(result: &SimulationResult, schematic: &Schematic) -> Vec<String> {
    let mut labelled: Vec<String> = Vec::new();
    for label in &schematic.labels {
        let expression = format!("V({})", label.name);
        if label.name != "0"
            && !labelled.contains(&expression)
            && result.signal(&expression).is_some()
        {
            labelled.push(expression);
        }
    }
    if !labelled.is_empty() {
        labelled.truncate(MAX_DEFAULT_TRACES);
        return labelled;
    }
    result
        .signals
        .iter()
        .find(|s| s.quantity == Quantity::Voltage)
        .map(|s| vec![s.name.clone()])
        .unwrap_or_default()
}

/// `setup` with `traces` added to its active pane (creating a pane when
/// there is none).
pub fn with_traces(mut setup: PlotSetup, traces: Vec<String>) -> PlotSetup {
    if setup.panes.is_empty() {
        setup.panes.push(PlotPane::default());
        setup.active_pane = 0;
    }
    let active = setup.active_pane.min(setup.panes.len() - 1);
    setup.active_pane = active;
    setup.panes[active]
        .traces
        .extend(traces.into_iter().map(Trace::new));
    setup
}

/// Whether a stored result still belongs to the schematic that netlists to
/// `netlist` (INT-1): its circuit fingerprint matches, and it records the
/// voltage of every node. A result from a file written before fingerprints
/// existed has only the second check, so a changed value goes unnoticed
/// there.
pub fn stored_result_is_current(stored: &StoredResult, netlist: &Netlist) -> bool {
    stored.computed_from(&netlist.circuit).unwrap_or(true)
        && result_matches(netlist, &stored.result)
}

/// Whether `result` was plausibly computed from `netlist`: every circuit
/// node has its voltage recorded and nothing else is.
pub fn result_matches(netlist: &Netlist, result: &SimulationResult) -> bool {
    let nodes = &netlist.circuit.nodes;
    let recorded: Vec<&str> = result
        .signals
        .iter()
        .filter(|s| matches!(s.origin, SignalOrigin::Node(_)))
        .map(|s| s.name.as_str())
        .collect();
    let expected = nodes.len().saturating_sub(1);
    recorded.len() == expected
        && nodes
            .iter()
            .skip(1)
            .all(|node| recorded.contains(&format!("V({node})").as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_project::{Connectivity, NetLabel, Point};
    use xsim_spice_engine::{
        Analysis, AnalysisKind, Axis, Circuit, Device, Element, Signal, SignalData,
    };

    fn voltage(name: &str, node: usize) -> Signal {
        Signal {
            name: format!("V({name})"),
            quantity: Quantity::Voltage,
            origin: SignalOrigin::Node(node),
            data: SignalData::Real(vec![0.0]),
        }
    }

    fn result(nodes: &[&str]) -> SimulationResult {
        SimulationResult {
            analysis: AnalysisKind::OperatingPoint,
            axis: Axis {
                name: "point".into(),
                unit: String::new(),
                values: vec![0.0],
            },
            signals: nodes
                .iter()
                .enumerate()
                .map(|(i, n)| voltage(n, i + 1))
                .collect(),
        }
    }

    fn label(id: u64, name: &str) -> NetLabel {
        NetLabel {
            id,
            name: name.into(),
            position: Point::new(0, 0),
        }
    }

    #[test]
    fn probes_become_expressions() {
        let voltage = ProbeRequest::Voltage { node: "out".into() };
        let differential = ProbeRequest::Differential {
            plus: "in".into(),
            minus: "out".into(),
        };
        let current = ProbeRequest::Current {
            signal: "I(R1)".into(),
        };
        assert_eq!(probe_expression(&voltage), "V(out)");
        assert_eq!(probe_expression(&differential), "V(in,out)");
        assert_eq!(probe_expression(&current), "I(R1)");
    }

    #[test]
    fn labelled_nets_are_plotted_first() {
        let result = result(&["N001", "out", "in"]);
        let mut schematic = Schematic {
            labels: vec![label(1, "out"), label(2, "in"), label(3, "out")],
            ..Schematic::default()
        };
        assert_eq!(default_traces(&result, &schematic), ["V(out)", "V(in)"]);
        schematic.labels.clear();
        assert_eq!(default_traces(&result, &schematic), ["V(N001)"]);
        assert!(default_traces(&self::result(&[]), &schematic).is_empty());
    }

    #[test]
    fn traces_land_in_the_active_pane() {
        let setup = with_traces(
            PlotSetup {
                panes: vec![],
                active_pane: 3,
            },
            vec!["V(out)".into()],
        );
        assert_eq!(setup.active_pane, 0);
        assert_eq!(setup.panes[0].traces[0].expression, "V(out)");
        assert!(!is_empty(&setup));
        assert!(is_empty(&PlotSetup::default()));
    }

    #[test]
    fn stored_results_must_match_the_netlist_nodes() {
        let mut circuit = Circuit::new();
        circuit.node("in");
        circuit.node("out");
        let netlist = Netlist {
            circuit,
            connectivity: Connectivity::default(),
            net_nodes: vec![],
            elements: Default::default(),
        };
        assert!(result_matches(&netlist, &result(&["in", "out"])));
        assert!(!result_matches(&netlist, &result(&["in"])));
        assert!(!result_matches(&netlist, &result(&["in", "x"])));
    }

    /// INT-1: the fingerprint decides when there is one; without one (older
    /// files) the node names do.
    #[test]
    fn stored_results_are_current_only_for_their_own_circuit() {
        let mut circuit = Circuit::new();
        circuit.node("in");
        circuit.node("out");
        let mut edited = circuit.clone();
        edited.add(Element::new("R1", [1, 2], Device::resistor(2e3)));
        let netlist = |circuit: &Circuit| Netlist {
            circuit: circuit.clone(),
            connectivity: Connectivity::default(),
            net_nodes: vec![],
            elements: Default::default(),
        };
        let stored = |fingerprint: Option<String>| StoredResult {
            analysis: Analysis::OperatingPoint,
            circuit_fingerprint: fingerprint,
            result: result(&["in", "out"]),
        };
        let own = Some(StoredResult::fingerprint(&circuit));
        assert!(stored_result_is_current(
            &stored(own.clone()),
            &netlist(&circuit)
        ));
        assert!(!stored_result_is_current(&stored(own), &netlist(&edited)));
        // Without a fingerprint the same node names pass, a value edit too.
        assert!(stored_result_is_current(&stored(None), &netlist(&edited)));
        let mut renamed = Circuit::new();
        renamed.node("in");
        renamed.node("x");
        assert!(!stored_result_is_current(&stored(None), &netlist(&renamed)));
    }
}
