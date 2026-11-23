//! Schematic → engine circuit conversion and its error checks.

use super::graph::{self, Topology};
use super::{Netlist, NetlistError};
use crate::schematic::{Component, Part, PartKind, Schematic};
use crate::symbols;
use std::collections::{BTreeMap, BTreeSet};
use xsim_spice_engine::{Circuit, Device, Element, library};

pub(super) fn netlist(schematic: &Schematic) -> Result<Netlist, NetlistError> {
    // `(schematic index, component)` of every part that becomes an element.
    let parts: Vec<(usize, &Component)> = schematic
        .components
        .iter()
        .enumerate()
        .filter(|(_, c)| c.part.kind() != PartKind::Ground)
        .collect();
    if parts.is_empty() {
        return Err(NetlistError::Empty);
    }
    check_valid_names(&parts)?;
    check_unique_names(&parts)?;
    let topology = graph::analyse(schematic);
    check_pins_connected(schematic, &topology, &parts)?;
    check_ground(&topology, &parts)?;

    // Designators of the independent voltage sources, which current
    // switches may name as their control.
    let sources: BTreeSet<&str> = parts
        .iter()
        .filter(|(_, c)| c.part.kind() == PartKind::VoltageSource)
        .map(|(_, c)| c.name.as_str())
        .collect();
    let mut circuit = Circuit::new();
    let mut elements = BTreeMap::new();
    for &(index, component) in &parts {
        let Some(device) = device(component, &sources)? else {
            continue;
        };
        let nets = &topology.connectivity.nets;
        let nodes: Vec<_> = topology.pin_nets[index]
            .iter()
            .map(|&net| circuit.node(&nets[net].name))
            .collect();
        elements.insert(component.id, circuit.elements.len());
        circuit.add(Element::new(component.name.clone(), nodes, device));
    }
    // Every net holds a pin, so its node already exists (ground is "0").
    let net_nodes = topology
        .connectivity
        .nets
        .iter()
        .map(|net| circuit.node(&net.name))
        .collect();
    Ok(Netlist {
        circuit,
        connectivity: topology.connectivity,
        net_nodes,
        elements,
    })
}

/// Designators become probe arguments such as `I(R1)` (INT-4), so a file
/// edited by hand with `R(1)` or `R 1` fails here instead of producing a
/// current nobody can plot.
fn check_valid_names(parts: &[(usize, &Component)]) -> Result<(), NetlistError> {
    match parts
        .iter()
        .find(|(_, c)| !super::is_valid_part_name(&c.name))
    {
        Some((_, c)) => Err(NetlistError::InvalidName(c.name.clone())),
        None => Ok(()),
    }
}

fn check_unique_names(parts: &[(usize, &Component)]) -> Result<(), NetlistError> {
    let mut seen = BTreeSet::new();
    match parts.iter().find(|(_, c)| !seen.insert(c.name.as_str())) {
        Some((_, c)) => Err(NetlistError::DuplicateName(c.name.clone())),
        None => Ok(()),
    }
}

/// A pin "touches nothing" when its net is that pin alone: no wire, no
/// other pin and no label.
fn check_pins_connected(
    schematic: &Schematic,
    topology: &Topology,
    parts: &[(usize, &Component)],
) -> Result<(), NetlistError> {
    for &(index, component) in parts {
        for (pin, &net) in topology.pin_nets[index].iter().enumerate() {
            let net = &topology.connectivity.nets[net];
            let alone = net.pins.len() == 1
                && net.wires.is_empty()
                && !schematic
                    .labels
                    .iter()
                    .any(|l| net.points.contains(&l.position));
            if alone {
                // Symbols list one name per pin (checked by the symbol tests).
                let names = symbols::geometry(component.part.kind()).pin_names;
                return Err(NetlistError::UnconnectedPin {
                    component: component.name.clone(),
                    pin: names[pin].to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Some device terminal must be on the ground net; a ground symbol that
/// touches nothing does not count.
fn check_ground(topology: &Topology, parts: &[(usize, &Component)]) -> Result<(), NetlistError> {
    let nets = &topology.connectivity.nets;
    let grounded = parts
        .iter()
        .any(|&(index, _)| topology.pin_nets[index].iter().any(|&n| nets[n].is_ground));
    if grounded {
        Ok(())
    } else {
        Err(NetlistError::NoGround)
    }
}

/// The engine device of a part; `None` for ground symbols. Semiconductor
/// models are looked up by exact name and must match the part's polarity.
/// A current switch's control must be the exact designator of one of
/// `sources`, the schematic's voltage sources; the element of that
/// designator is its control in the circuit.
fn device(component: &Component, sources: &BTreeSet<&str>) -> Result<Option<Device>, NetlistError> {
    let unknown = |model: &str| NetlistError::UnknownModel {
        component: component.name.clone(),
        model: model.to_string(),
    };
    let device = match &component.part {
        Part::Resistor { resistance } => Device::resistor(*resistance),
        Part::Capacitor {
            capacitance,
            initial_voltage,
        } => Device::Capacitor {
            capacitance: *capacitance,
            initial_voltage: *initial_voltage,
        },
        Part::Inductor {
            inductance,
            initial_current,
        } => Device::Inductor {
            inductance: *inductance,
            initial_current: *initial_current,
        },
        Part::VoltageSource { source } => Device::VoltageSource(source.clone()),
        Part::CurrentSource { source } => Device::CurrentSource(source.clone()),
        Part::Diode { model } => {
            Device::diode(library::diode(model).ok_or_else(|| unknown(model))?)
        }
        Part::Bjt { polarity, model } => Device::bjt(
            library::bjt(model)
                .filter(|m| m.polarity == *polarity)
                .ok_or_else(|| unknown(model))?,
        ),
        Part::Jfet { polarity, model } => Device::jfet(
            library::jfet(model)
                .filter(|m| m.polarity == *polarity)
                .ok_or_else(|| unknown(model))?,
        ),
        Part::VoltageSwitch { model } => Device::VoltageSwitch {
            model: model.clone(),
        },
        Part::CurrentSwitch { control, model } => {
            if !sources.contains(control.as_str()) {
                return Err(NetlistError::UnknownControl {
                    component: component.name.clone(),
                    control: control.clone(),
                });
            }
            Device::CurrentSwitch {
                control: control.clone(),
                model: model.clone(),
            }
        }
        Part::Ground => return Ok(None),
    };
    Ok(Some(device))
}

#[cfg(test)]
mod tests {
    use crate::netlist::{NetlistError, is_valid_part_name, netlist};
    use crate::schematic::{Component, NetLabel, Orientation, Part, PartKind, Point, Schematic};

    /// A voltage source and a resistor in parallel, joined by labels (`a`
    /// and the ground label `0` on each pin), plus a ground symbol named
    /// `ground`.
    fn circuit(names: [&str; 2], ground: &str) -> Schematic {
        let mut s = Schematic::default();
        let parts = [
            Part::default_for(PartKind::VoltageSource),
            Part::default_for(PartKind::Resistor),
            Part::Ground,
        ];
        for (i, (name, part)) in names.into_iter().chain([ground]).zip(parts).enumerate() {
            let id = s.allocate_id();
            let c = Component {
                id,
                name: name.to_string(),
                position: Point::new(10 * i as i32, 0),
                orientation: Orientation::default(),
                part,
            };
            for (pin, net) in c.pins().into_iter().zip(["a", "0"]) {
                let id = s.allocate_id();
                s.labels.push(NetLabel {
                    id,
                    name: net.into(),
                    position: pin,
                });
            }
            s.components.push(c);
        }
        s
    }

    #[test]
    fn designators_are_letters_digits_and_underscores() {
        for name in ["R1", "Rload_2", "_x", "VIN", "q9"] {
            assert!(is_valid_part_name(name), "{name}");
        }
        for name in [
            "", "R(1)", "R1,2", "R1)", "R 1", "R\t1", "R-1", "R\u{e4}1", "R\"1",
        ] {
            assert!(!is_valid_part_name(name), "{name:?}");
        }
    }

    #[test]
    fn hand_edited_names_that_break_probes_fail_cleanly() {
        assert!(netlist(&circuit(["V1", "R_load"], "GND1")).is_ok());
        for bad in ["R(1)", "R1,2", "R1)", "R 1", ""] {
            let error = netlist(&circuit(["V1", bad], "GND1")).err();
            assert_eq!(
                error,
                Some(NetlistError::InvalidName(bad.into())),
                "{bad:?}"
            );
            let message = error.map(|e| e.to_string()).unwrap_or_default();
            assert!(
                message.contains("letters, digits and _") || message.contains("no name"),
                "{message}"
            );
        }
    }

    #[test]
    fn invalid_names_are_reported_before_duplicates() {
        assert_eq!(
            netlist(&circuit(["R 1", "R 1"], "GND1")).err(),
            Some(NetlistError::InvalidName("R 1".into()))
        );
        assert_eq!(
            netlist(&circuit(["V1", "V1"], "GND1")).err(),
            Some(NetlistError::DuplicateName("V1".into()))
        );
    }

    #[test]
    fn ground_symbols_are_not_elements_so_their_names_are_not_checked() {
        assert!(netlist(&circuit(["V1", "R1"], "GND (1)")).is_ok());
    }
}
