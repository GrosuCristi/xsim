//! What a probe click asks for (Q62, Q130).

use crate::ProbeRequest;
use xsim_project::{Component, PartKind};

/// Name of the ground node, which has no voltage trace.
const GROUND: &str = "0";

/// The current trace of a part: `I(R1)`, `Ic(Q1)` for transistors,
/// `Id(J1)` for JFETs, `I(S1)` / `I(W1)` for switches, the current into
/// P, and `Io(U1)` for op-amps, the current into the output (D26).
/// Ground symbols have none.
pub fn current_signal(component: &Component) -> Option<String> {
    let name = &component.name;
    match component.part.kind() {
        PartKind::Ground => None,
        PartKind::NpnBjt | PartKind::PnpBjt => Some(format!("Ic({name})")),
        PartKind::NJfet | PartKind::PJfet => Some(format!("Id({name})")),
        PartKind::Resistor
        | PartKind::Capacitor
        | PartKind::Inductor
        | PartKind::VoltageSource
        | PartKind::CurrentSource
        | PartKind::Diode
        | PartKind::VoltageSwitch
        | PartKind::CurrentSwitch => Some(format!("I({name})")),
        PartKind::OpAmp => Some(format!("Io({name})")),
    }
}

/// The voltage of `node` to ground; nothing for the ground node itself.
pub fn voltage(node: &str) -> Option<ProbeRequest> {
    (node != GROUND).then(|| ProbeRequest::Voltage {
        node: node.to_string(),
    })
}

/// The voltage between the node the probe was pressed on (`plus`) and the
/// node it was released on (`minus`). Releasing on the same node or on
/// ground measures `plus` to ground.
pub fn differential(plus: &str, minus: &str) -> Option<ProbeRequest> {
    if plus == minus || minus == GROUND {
        return voltage(plus);
    }
    Some(ProbeRequest::Differential {
        plus: plus.to_string(),
        minus: minus.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{current_signal, differential, voltage};
    use crate::ProbeRequest;
    use xsim_project::{Component, Orientation, Part, PartKind, Point};

    fn named(kind: PartKind, name: &str) -> Component {
        Component {
            id: 1,
            name: name.into(),
            position: Point::default(),
            orientation: Orientation::default(),
            part: match kind {
                PartKind::NpnBjt => Part::Bjt {
                    polarity: xsim_spice_engine::Polarity::N,
                    model: "M".into(),
                },
                PartKind::NJfet => Part::Jfet {
                    polarity: xsim_spice_engine::Polarity::N,
                    model: "M".into(),
                },
                PartKind::Ground => Part::Ground,
                PartKind::VoltageSwitch | PartKind::CurrentSwitch | PartKind::OpAmp => {
                    Part::default_for(kind)
                }
                _ => Part::Resistor { resistance: 1.0 },
            },
        }
    }

    #[test]
    fn current_traces_use_the_terminal_conventions() {
        assert_eq!(
            current_signal(&named(PartKind::Resistor, "R1")).as_deref(),
            Some("I(R1)")
        );
        assert_eq!(
            current_signal(&named(PartKind::NpnBjt, "Q1")).as_deref(),
            Some("Ic(Q1)")
        );
        assert_eq!(
            current_signal(&named(PartKind::NJfet, "J2")).as_deref(),
            Some("Id(J2)")
        );
        assert_eq!(current_signal(&named(PartKind::Ground, "GND1")), None);
        assert_eq!(
            current_signal(&named(PartKind::VoltageSwitch, "S1")).as_deref(),
            Some("I(S1)")
        );
        assert_eq!(
            current_signal(&named(PartKind::CurrentSwitch, "W1")).as_deref(),
            Some("I(W1)")
        );
        assert_eq!(
            current_signal(&named(PartKind::OpAmp, "U1")).as_deref(),
            Some("Io(U1)")
        );
    }

    #[test]
    fn differential_probes_collapse_to_plain_voltages_where_meaningful() {
        let v = |n: &str| Some(ProbeRequest::Voltage { node: n.into() });
        assert_eq!(voltage("out"), v("out"));
        assert_eq!(voltage("0"), None);
        assert_eq!(differential("out", "out"), v("out"));
        assert_eq!(differential("out", "0"), v("out"));
        assert_eq!(
            differential("in", "out"),
            Some(ProbeRequest::Differential {
                plus: "in".into(),
                minus: "out".into()
            })
        );
    }
}
