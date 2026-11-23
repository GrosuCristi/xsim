//! Part measurements for the hover box (Q62).
//!
//! * Voltage across the part: pin 1 − pin 2 for two-terminal parts and
//!   switches (P − N), collector − emitter for BJTs, drain − source for
//!   JFETs, and the output to ground for an op-amp (D26).
//! * Current through the part: the current into its first terminal (`I`,
//!   `Ic`, `Id`; `I(S1)` and `I(W1)` into a switch's P), and into the
//!   output of an op-amp (`Io`, negative while it sources current).
//! * Power: `P = Σ V_k · I_k` over the terminals (current into terminal `k`),
//!   so a positive value is dissipated and a negative one delivered. The
//!   control inputs C+ and C− of a voltage switch draw no current. For an
//!   op-amp the sum runs over all five terminals: what it draws from its
//!   supplies less what it delivers at its output, i.e. its dissipation.
//!
//! Transient results report time-weighted RMS voltage and current and the
//! average power over the whole run; an operating point reports its values.
//! Sweeps have no meaningful single value, so they report nothing.
//!
//! Signals are looked up by name (`V(out)`, `I(R1)`, `Ic(Q1)`, …), as the
//! plot does, never by the node or element index recorded in the result:
//! a result restored with a project was recorded by an earlier netlist,
//! whose indices may differ from the one rebuilt from the schematic
//! (INT-2).

use xsim_project::{Component, Id, Netlist, PartKind};
use xsim_spice_engine::{AnalysisKind, GROUND, SimulationResult};
use xsim_waveform::measure::stats;

/// How the numbers of a [`Measurement`] were obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementKind {
    /// RMS voltage and current, average power, over a transient run.
    Transient,
    /// Values at the DC operating point.
    OperatingPoint,
}

/// Values shown in the hover box.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub kind: MeasurementKind,
    /// Short name of the voltage, e.g. `"V"` or `"V_CE"`.
    pub voltage_label: &'static str,
    /// Short name of the current, e.g. `"I"` or `"Ic"`.
    pub current_label: &'static str,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    pub power: Option<f64>,
}

/// Measures `component` in `result`. `None` for ground symbols, parts the
/// netlist does not know, and analyses without single values (sweeps).
pub fn measure(
    component: &Component,
    netlist: &Netlist,
    result: &SimulationResult,
) -> Option<Measurement> {
    let kind = match result.analysis {
        AnalysisKind::Transient => MeasurementKind::Transient,
        AnalysisKind::OperatingPoint => MeasurementKind::OperatingPoint,
        AnalysisKind::DcSweep | AnalysisKind::Ac => return None,
    };
    let element = netlist
        .circuit
        .elements
        .get(*netlist.elements.get(&component.id)?)?;
    let terminals = component.pins().len();
    let voltages: Vec<Option<Vec<f64>>> = (0..terminals)
        .map(|k| terminal_voltage(netlist, result, component.id, k))
        .collect();
    let prefixes = element.device.current_prefixes();
    let insulated = insulated_terminals(component.part.kind());
    let currents = terminal_currents(result, &element.name, prefixes, insulated, terminals);
    let (across, labels) = across_terminals(component.part.kind());
    let voltage = match across.1 {
        Some(minus) => difference(&voltages[across.0], &voltages[minus]),
        None => voltages[across.0].clone(),
    };
    let current = currents
        .get(current_terminal(component.part.kind()))
        .cloned()
        .flatten();
    // Insulated terminals add nothing to the power, whatever their voltage.
    let carrying = |values: &[Option<Vec<f64>>]| -> Vec<Option<Vec<f64>>> {
        values
            .iter()
            .enumerate()
            .filter(|(k, _)| !insulated.contains(k))
            .map(|(_, v)| v.clone())
            .collect()
    };
    let power = power(&carrying(&voltages), &carrying(&currents));
    let x = &result.axis.values;
    let summarize = |series: Option<Vec<f64>>, rms: bool| -> Option<f64> {
        let series = series?;
        match kind {
            MeasurementKind::OperatingPoint => series.first().copied(),
            MeasurementKind::Transient => {
                let s = stats(x, &series, None)?;
                Some(if rms { s.rms } else { s.average })
            }
        }
    };
    Some(Measurement {
        kind,
        voltage_label: labels.0,
        current_label: labels.1,
        voltage: summarize(voltage, true),
        current: summarize(current, true),
        power: summarize(power, false),
    })
}

/// Terminals the voltage is measured across (the second `None` for
/// ground) and the labels of the values. A switch, like any two-terminal
/// part, is measured across its switch path P–N; an op-amp at its output.
fn across_terminals(kind: PartKind) -> ((usize, Option<usize>), (&'static str, &'static str)) {
    match kind {
        PartKind::NpnBjt | PartKind::PnpBjt => ((0, Some(2)), ("V_CE", "Ic")),
        PartKind::NJfet | PartKind::PJfet => ((0, Some(2)), ("V_DS", "Id")),
        PartKind::Resistor
        | PartKind::Capacitor
        | PartKind::Inductor
        | PartKind::VoltageSource
        | PartKind::CurrentSource
        | PartKind::Diode
        | PartKind::VoltageSwitch
        | PartKind::CurrentSwitch
        | PartKind::Ground => ((0, Some(1)), ("V", "I")),
        PartKind::OpAmp => ((2, None), ("V_OUT", "Io")),
    }
}

/// The terminal whose current the card shows: the output of an op-amp,
/// the first terminal of every other part.
fn current_terminal(kind: PartKind) -> usize {
    match kind {
        PartKind::OpAmp => 2,
        _ => 0,
    }
}

/// Terminals that draw no current: the control inputs C+ and C− of a
/// voltage switch, which only sense a voltage. The switch's current then
/// enters at P and leaves at N.
fn insulated_terminals(kind: PartKind) -> &'static [usize] {
    match kind {
        PartKind::VoltageSwitch => &[2, 3],
        _ => &[],
    }
}

/// Real samples of the signal called `name`.
fn series(result: &SimulationResult, name: &str) -> Option<Vec<f64>> {
    result.signal(name)?.real().map(<[f64]>::to_vec)
}

/// Voltage samples of pin `pin`'s net (`V(node)`), zeros for ground.
fn terminal_voltage(
    netlist: &Netlist,
    result: &SimulationResult,
    component: Id,
    pin: usize,
) -> Option<Vec<f64>> {
    let net = netlist.connectivity.net_of_pin(component, pin)?;
    let node = *netlist.net_nodes.get(net)?;
    if node == GROUND {
        return Some(vec![0.0; result.len()]);
    }
    let name = netlist.circuit.nodes.get(node)?;
    series(result, &format!("V({name})"))
}

/// Current into each terminal of the element called `element`, recorded as
/// `{prefix}({element})` with the device's current prefixes in terminal
/// order (`I`, or `Ic`/`Ib`/`Ie`, …); the `insulated` terminals draw none,
/// and a single missing one follows from KCL.
fn terminal_currents(
    result: &SimulationResult,
    element: &str,
    prefixes: &[&str],
    insulated: &[usize],
    terminals: usize,
) -> Vec<Option<Vec<f64>>> {
    let mut currents: Vec<Option<Vec<f64>>> = (0..terminals)
        .map(|terminal| {
            if insulated.contains(&terminal) {
                return Some(vec![0.0; result.len()]);
            }
            let prefix = prefixes.get(terminal)?;
            series(result, &format!("{prefix}({element})"))
        })
        .collect();
    let missing: Vec<usize> = (0..terminals).filter(|&k| currents[k].is_none()).collect();
    if let [k] = missing[..] {
        let mut sum = vec![0.0; result.len()];
        for series in currents.iter().flatten() {
            for (acc, v) in sum.iter_mut().zip(series) {
                *acc -= v;
            }
        }
        currents[k] = Some(sum);
    }
    currents
}

fn difference(a: &Option<Vec<f64>>, b: &Option<Vec<f64>>) -> Option<Vec<f64>> {
    let (a, b) = (a.as_ref()?, b.as_ref()?);
    Some(a.iter().zip(b).map(|(x, y)| x - y).collect())
}

/// Instantaneous power `Σ V_k · I_k`.
fn power(voltages: &[Option<Vec<f64>>], currents: &[Option<Vec<f64>>]) -> Option<Vec<f64>> {
    let mut total: Option<Vec<f64>> = None;
    for (v, i) in voltages.iter().zip(currents) {
        let (v, i) = (v.as_ref()?, i.as_ref()?);
        let term = v.iter().zip(i).map(|(v, i)| v * i);
        total = Some(match total {
            None => term.collect(),
            Some(sum) => sum.iter().zip(term).map(|(s, t)| s + t).collect(),
        });
    }
    total
}

#[cfg(test)]
mod tests {
    use super::{measure, power, terminal_currents};
    use xsim_project::{
        Component, NetLabel, Orientation, Part, PartKind, Point, Schematic, netlist,
    };
    use xsim_spice_engine::{
        Analysis, AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimControl,
        SimOptions, SimulationResult, simulate,
    };

    fn result_with_current(samples: Vec<f64>) -> SimulationResult {
        SimulationResult {
            analysis: AnalysisKind::Transient,
            axis: Axis {
                name: "time".into(),
                unit: "s".into(),
                values: (0..samples.len()).map(|i| i as f64).collect(),
            },
            signals: vec![Signal {
                name: "I(R1)".into(),
                quantity: Quantity::Current,
                origin: SignalOrigin::TerminalCurrent {
                    element: 0,
                    terminal: 0,
                },
                data: SignalData::Real(samples),
            }],
        }
    }

    #[test]
    fn a_missing_terminal_current_follows_from_kcl() {
        let r = result_with_current(vec![1.0, -2.0]);
        let currents = terminal_currents(&r, "R1", &["I"], &[], 2);
        assert_eq!(currents[0], Some(vec![1.0, -2.0]), "found by name");
        assert_eq!(currents[1], Some(vec![-1.0, 2.0]));
        assert_eq!(
            terminal_currents(&r, "R2", &["I"], &[], 2),
            vec![None, None]
        );
    }

    /// A voltage switch records only `I(S1)`, into P: its control inputs
    /// draw nothing, so the current leaves at N.
    #[test]
    fn insulated_terminals_draw_no_current() {
        let mut r = result_with_current(vec![1.0, -2.0]);
        r.signals[0].name = "I(S1)".into();
        let currents = terminal_currents(&r, "S1", &["I"], &[2, 3], 4);
        assert_eq!(
            currents,
            vec![
                Some(vec![1.0, -2.0]),
                Some(vec![-1.0, 2.0]),
                Some(vec![0.0, 0.0]),
                Some(vec![0.0, 0.0]),
            ]
        );
    }

    /// A voltage divider: V1 (1 V) from `in` to ground, R1 from `in` to
    /// `out`, R2 from `out` to ground, all joined by labels on the pins.
    /// `order` lists the parts (0 = V1, 1 = R1, 2 = R2) in schematic order,
    /// which decides the netlist's element and node numbering.
    fn divider(order: [usize; 3]) -> Schematic {
        let parts = [
            (
                "V1",
                Part::default_for(PartKind::VoltageSource),
                ["in", "0"],
            ),
            ("R1", Part::Resistor { resistance: 1e3 }, ["in", "out"]),
            ("R2", Part::Resistor { resistance: 1e3 }, ["out", "0"]),
        ];
        let mut s = Schematic::default();
        for index in order {
            let (name, part, nets) = parts[index].clone();
            let id = index as u64 + 1;
            let c = Component {
                id,
                name: name.into(),
                position: Point::new(20 * index as i32, 0),
                orientation: Orientation::default(),
                part,
            };
            for (pin, net) in c.pins().into_iter().zip(nets) {
                s.labels.push(NetLabel {
                    id: 10 + s.labels.len() as u64,
                    name: net.into(),
                    position: pin,
                });
            }
            s.components.push(c);
        }
        s.next_id = 100;
        s
    }

    /// INT-2: a result restored with a project is measured against the
    /// netlist rebuilt from the (possibly reordered) schematic.
    #[test]
    fn a_reordered_schematic_measures_the_right_part_with_the_right_sign() {
        let recorded = divider([0, 1, 2]);
        let recorded_netlist = netlist(&recorded).expect("valid divider");
        let mut result = simulate(
            &recorded_netlist.circuit,
            &Analysis::OperatingPoint,
            &SimOptions::default(),
            &SimControl::new(),
        )
        .expect("the divider simulates");
        let reordered = divider([2, 1, 0]);
        let rebuilt = netlist(&reordered).expect("valid divider");
        assert_ne!(
            rebuilt.circuit.nodes, recorded_netlist.circuit.nodes,
            "the node numbering differs"
        );
        // Make every recorded index wrong but plausible: only names count.
        let mut origins: Vec<SignalOrigin> =
            result.signals.iter().map(|s| s.origin.clone()).collect();
        origins.reverse();
        for (signal, origin) in result.signals.iter_mut().zip(origins) {
            signal.origin = origin;
        }
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        let m = |id: u64| {
            let c = reordered.component(id).expect("part");
            measure(c, &rebuilt, &result).expect("operating point values")
        };
        let (v1, r1, r2) = (m(1), m(2), m(3));
        assert!(close(v1.voltage, 1.0), "{v1:?}");
        assert!(
            close(v1.current, -0.5e-3),
            "current into V1's + pin: {v1:?}"
        );
        assert!(close(v1.power, -0.5e-3), "the source delivers: {v1:?}");
        for r in [r1, r2] {
            assert!(close(r.voltage, 0.5), "{r:?}");
            assert!(close(r.current, 0.5e-3), "{r:?}");
            assert!(close(r.power, 0.25e-3), "resistors dissipate: {r:?}");
        }
    }

    /// INT-2 for a transient: RMS values and the average power's sign come
    /// from the signals named after the part, whatever their recorded index.
    #[test]
    fn a_reordered_schematic_measures_a_transient_by_name() {
        let recorded = netlist(&divider([0, 1, 2])).expect("valid divider");
        let op = simulate(
            &recorded.circuit,
            &Analysis::OperatingPoint,
            &SimOptions::default(),
            &SimControl::new(),
        )
        .expect("the divider simulates");
        // A steady transient: every signal holds its operating-point value.
        let mut result = SimulationResult {
            analysis: AnalysisKind::Transient,
            axis: Axis {
                name: "time".into(),
                unit: "s".into(),
                values: vec![0.0, 1e-3, 2e-3],
            },
            signals: op
                .signals
                .iter()
                .map(|s| Signal {
                    data: SignalData::Real(vec![s.real().map_or(f64::NAN, |v| v[0]); 3]),
                    ..s.clone()
                })
                .collect(),
        };
        result.signals.rotate_left(1);
        let mut origins: Vec<SignalOrigin> =
            result.signals.iter().map(|s| s.origin.clone()).collect();
        origins.rotate_left(2);
        for (signal, origin) in result.signals.iter_mut().zip(origins) {
            signal.origin = origin;
        }
        let reordered = divider([1, 2, 0]);
        let rebuilt = netlist(&reordered).expect("valid divider");
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        let m = |id: u64| {
            let c = reordered.component(id).expect("part");
            measure(c, &rebuilt, &result).expect("transient values")
        };
        let v1 = m(1);
        assert!(
            close(v1.voltage, 1.0) && close(v1.current, 0.5e-3),
            "{v1:?}"
        );
        assert!(close(v1.power, -0.5e-3), "the source delivers: {v1:?}");
        for r in [m(2), m(3)] {
            assert!(close(r.voltage, 0.5) && close(r.current, 0.5e-3), "{r:?}");
            assert!(close(r.power, 0.25e-3), "resistors dissipate: {r:?}");
        }
    }

    #[test]
    fn a_result_without_the_part_measures_nothing() {
        let s = divider([0, 1, 2]);
        let n = netlist(&s).expect("valid divider");
        let empty = SimulationResult {
            analysis: AnalysisKind::OperatingPoint,
            axis: Axis {
                name: "point".into(),
                unit: String::new(),
                values: vec![0.0],
            },
            signals: Vec::new(),
        };
        let r1 = s.component(2).expect("R1");
        let m = measure(r1, &n, &empty).expect("an operating point");
        assert_eq!((m.voltage, m.current, m.power), (None, None, None));
    }

    #[test]
    fn power_sums_every_terminal() {
        let v = [Some(vec![5.0, 4.0]), Some(vec![1.0, 0.0])];
        let i = [Some(vec![2.0, 1.0]), Some(vec![-2.0, -1.0])];
        assert_eq!(power(&v, &i), Some(vec![8.0, 4.0]));
        assert_eq!(power(&v, &[None, None]), None);
    }
}
