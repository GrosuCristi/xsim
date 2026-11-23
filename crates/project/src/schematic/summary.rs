//! The short value text drawn next to a part symbol.
//!
//! Plain values use engineering notation with units (`1 kΩ`, `1 µF IC=0 V`).
//! Source waveforms use SPICE syntax without units, so the text reads like
//! the netlist line a SPICE user expects: `PULSE(0 1 0 1n 1n 1m 2m)`,
//! `SINE(0 1 1k)`, `AC 1`. Switch models read like the parameters of a
//! SPICE model card, in xsim's case-sensitive notation (`M` is mega, so
//! ROFF = 1 MΩ shows as `ROFF=1M`, not SPICE's `1Meg`): `RON=1 ROFF=1M
//! VT=500m` for a voltage switch, the control source and threshold for a
//! current switch (`V1 IT=1m`), each with `VH=` / `IH=` when the hysteresis
//! is not zero.

use crate::schematic::Part;
use xsim_spice_engine::units::format_value;
use xsim_spice_engine::{CurrentSwitchModel, Source, SwitchModel, Waveform};

/// PWL points shown before the list is abbreviated with `…`.
const PWL_POINTS_SHOWN: usize = 3;

pub(super) fn summary(part: &Part) -> String {
    match part {
        Part::Resistor { resistance } => format_value(*resistance, "Ω"),
        Part::Capacitor {
            capacitance,
            initial_voltage,
        } => with_initial(format_value(*capacitance, "F"), *initial_voltage, "V"),
        Part::Inductor {
            inductance,
            initial_current,
        } => with_initial(format_value(*inductance, "H"), *initial_current, "A"),
        Part::VoltageSource { source } => source_summary(source, "V"),
        Part::CurrentSource { source } => source_summary(source, "A"),
        Part::Diode { model } | Part::Bjt { model, .. } | Part::Jfet { model, .. } => model.clone(),
        Part::VoltageSwitch { model } => voltage_switch_summary(model),
        Part::CurrentSwitch { control, model } => current_switch_summary(control, model),
        Part::Ground => String::new(),
    }
}

/// `RON=1 ROFF=1M VT=500m`, then `VH=…` unless it is zero.
fn voltage_switch_summary(model: &SwitchModel) -> String {
    let mut text = format!(
        "RON={} ROFF={} VT={}",
        number(model.ron),
        number(model.roff),
        number(model.vt)
    );
    if model.vh != 0.0 {
        text = format!("{text} VH={}", number(model.vh));
    }
    text
}

/// `V1 IT=1m`, then `IH=…` unless it is zero; the control is left out while
/// it is empty.
fn current_switch_summary(control: &str, model: &CurrentSwitchModel) -> String {
    let mut parts = Vec::new();
    if !control.is_empty() {
        parts.push(control.to_string());
    }
    parts.push(format!("IT={}", number(model.it)));
    if model.ih != 0.0 {
        parts.push(format!("IH={}", number(model.ih)));
    }
    parts.join(" ")
}

fn with_initial(value: String, initial: Option<f64>, unit: &str) -> String {
    match initial {
        Some(v) => format!("{value} IC={}", format_value(v, unit)),
        None => value,
    }
}

/// The transient waveform if any, else the DC value (left out when it is
/// zero and the source is only an AC stimulus), then the AC stimulus.
fn source_summary(source: &Source, unit: &str) -> String {
    let ac = source.ac_magnitude != 0.0;
    let mut parts = Vec::new();
    match &source.waveform {
        Some(waveform) => parts.push(waveform_summary(waveform)),
        None if source.dc != 0.0 || !ac => parts.push(format_value(source.dc, unit)),
        None => {}
    }
    if ac {
        let mut text = format!("AC {}", number(source.ac_magnitude));
        if source.ac_phase != 0.0 {
            text = format!("{text} {}", number(source.ac_phase));
        }
        parts.push(text);
    }
    parts.join(" ")
}

fn waveform_summary(waveform: &Waveform) -> String {
    match waveform {
        Waveform::Pulse {
            initial,
            pulsed,
            delay,
            rise,
            fall,
            width,
            period,
            cycles,
        } => {
            let mut args: Vec<String> = [initial, pulsed, delay, rise, fall, width, period]
                .into_iter()
                .map(|&v| number(v))
                .collect();
            args.extend(cycles.map(|n| n.to_string()));
            call("PULSE", &args)
        }
        Waveform::Sine {
            offset,
            amplitude,
            frequency,
            delay,
            damping,
            phase,
            cycles,
        } => {
            // Trailing parameters at their defaults are left out, as SPICE
            // users write them, but the first three are always shown.
            let mut args: Vec<Option<f64>> = vec![
                Some(*offset),
                Some(*amplitude),
                Some(*frequency),
                Some(*delay),
                Some(*damping),
                Some(*phase),
                *cycles,
            ];
            while args.len() > 3 && args.last().is_some_and(|v| v.is_none_or(|x| x == 0.0)) {
                args.pop();
            }
            let args: Vec<String> = args.into_iter().map(|v| number(v.unwrap_or(0.0))).collect();
            call("SINE", &args)
        }
        Waveform::Exp {
            initial,
            pulsed,
            rise_delay,
            rise_tau,
            fall_delay,
            fall_tau,
        } => call(
            "EXP",
            &[initial, pulsed, rise_delay, rise_tau, fall_delay, fall_tau].map(|&v| number(v)),
        ),
        Waveform::Pwl { points, repeat } => {
            let mut args: Vec<String> = points
                .iter()
                .take(PWL_POINTS_SHOWN)
                .flat_map(|&(t, v)| [number(t), number(v)])
                .collect();
            if points.len() > PWL_POINTS_SHOWN {
                args.push("…".to_string());
            }
            call(if *repeat { "PWL REPEAT" } else { "PWL" }, &args)
        }
        Waveform::Sffm {
            offset,
            amplitude,
            carrier,
            index,
            signal,
        } => call(
            "SFFM",
            &[offset, amplitude, carrier, index, signal].map(|&v| number(v)),
        ),
    }
}

/// A unitless engineering-notation number such as `1k` or `2.2u`.
fn number(value: f64) -> String {
    format_value(value, "")
}

fn call(name: &str, args: &[String]) -> String {
    format!("{name}({})", args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_spice_engine::Polarity;

    fn voltage(source: Source) -> String {
        Part::VoltageSource { source }.summary()
    }

    fn with(waveform: Waveform) -> Source {
        Source {
            dc: 0.0,
            ac_magnitude: 0.0,
            ac_phase: 0.0,
            waveform: Some(waveform),
        }
    }

    fn pulse(cycles: Option<u32>) -> Waveform {
        Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay: 0.0,
            rise: 1e-9,
            fall: 1e-9,
            width: 1e-3,
            period: 2e-3,
            cycles,
        }
    }

    fn sine(phase: f64, cycles: Option<f64>) -> Waveform {
        Waveform::Sine {
            offset: 0.0,
            amplitude: 1.0,
            frequency: 1e3,
            delay: 0.0,
            damping: 0.0,
            phase,
            cycles,
        }
    }

    #[test]
    fn passive_values_use_engineering_notation_and_units() {
        assert_eq!(Part::Resistor { resistance: 1e3 }.summary(), "1 kΩ");
        assert_eq!(Part::Resistor { resistance: 4.7e3 }.summary(), "4.7 kΩ");
        let capacitor = Part::Capacitor {
            capacitance: 1e-6,
            initial_voltage: Some(0.5),
        };
        let microfarad = format_value(1e-6, "F");
        assert_eq!(capacitor.summary(), format!("{microfarad} IC=500 mV"));
        let inductor = Part::Inductor {
            inductance: 1e-3,
            initial_current: None,
        };
        assert_eq!(inductor.summary(), "1 mH");
        let inductor = Part::Inductor {
            inductance: 1e-3,
            initial_current: Some(-1e-3),
        };
        assert_eq!(inductor.summary(), "1 mH IC=-1 mA");
    }

    #[test]
    fn dc_and_ac_sources() {
        assert_eq!(voltage(Source::dc(1.0)), "1 V");
        assert_eq!(voltage(Source::dc(0.0)), "0 V");
        assert_eq!(voltage(Source::ac(0.0, 1.0, 0.0)), "AC 1");
        assert_eq!(voltage(Source::ac(2.0, 1.0, 90.0)), "2 V AC 1 90");
        let current = Part::CurrentSource {
            source: Source::dc(1e-3),
        };
        assert_eq!(current.summary(), "1 mA");
    }

    #[test]
    fn waveforms_use_spice_syntax() {
        assert_eq!(voltage(with(pulse(None))), "PULSE(0 1 0 1n 1n 1m 2m)");
        assert_eq!(voltage(with(pulse(Some(5)))), "PULSE(0 1 0 1n 1n 1m 2m 5)");
        assert_eq!(voltage(with(sine(0.0, None))), "SINE(0 1 1k)");
        assert_eq!(voltage(with(sine(90.0, None))), "SINE(0 1 1k 0 0 90)");
        assert_eq!(voltage(with(sine(0.0, Some(3.0)))), "SINE(0 1 1k 0 0 0 3)");
        let exp = Waveform::Exp {
            initial: 0.0,
            pulsed: 5.0,
            rise_delay: 1e-3,
            rise_tau: 10e-3,
            fall_delay: 20e-3,
            fall_tau: 5e-3,
        };
        assert_eq!(voltage(with(exp)), "EXP(0 5 1m 10m 20m 5m)");
        let sffm = Waveform::Sffm {
            offset: 0.0,
            amplitude: 1.0,
            carrier: 1e6,
            index: 5.0,
            signal: 1e3,
        };
        assert_eq!(voltage(with(sffm)), "SFFM(0 1 1M 5 1k)");
    }

    #[test]
    fn long_pwl_lists_are_abbreviated() {
        let pwl = |points: Vec<(f64, f64)>, repeat| Waveform::Pwl { points, repeat };
        let four = vec![(0.0, 0.0), (1e-3, 1.0), (2e-3, 0.0), (3e-3, 1.0)];
        assert_eq!(voltage(with(pwl(four, false))), "PWL(0 0 1m 1 2m 0 …)");
        assert_eq!(
            voltage(with(pwl(vec![(0.0, 1.0)], true))),
            "PWL REPEAT(0 1)"
        );
    }

    #[test]
    fn a_waveform_replaces_the_dc_value_but_not_the_ac_stimulus() {
        let source = Source {
            dc: 3.0,
            ac_magnitude: 1.0,
            ac_phase: 0.0,
            waveform: Some(pulse(None)),
        };
        assert_eq!(voltage(source), "PULSE(0 1 0 1n 1n 1m 2m) AC 1");
    }

    #[test]
    fn switches_show_their_model_parameters() {
        use crate::schematic::PartKind;
        assert_eq!(
            Part::default_for(PartKind::VoltageSwitch).summary(),
            "RON=1 ROFF=1M VT=500m"
        );
        assert_eq!(
            Part::default_for(PartKind::CurrentSwitch).summary(),
            "V1 IT=1m"
        );
        let voltage = Part::VoltageSwitch {
            model: SwitchModel {
                ron: 0.01,
                roff: 1e9,
                vt: 2.5,
                vh: -0.5,
                ..SwitchModel::default()
            },
        };
        assert_eq!(voltage.summary(), "RON=10m ROFF=1G VT=2.5 VH=-500m");
        let current = |control: &str| Part::CurrentSwitch {
            control: control.into(),
            model: CurrentSwitchModel {
                it: 25e-6,
                ih: 5e-6,
                ..CurrentSwitchModel::default()
            },
        };
        assert_eq!(current("VSENSE").summary(), "VSENSE IT=25µ IH=5µ");
        assert_eq!(current("").summary(), "IT=25µ IH=5µ");
    }

    #[test]
    fn semiconductors_show_their_model_and_ground_nothing() {
        assert_eq!(
            Part::Diode {
                model: "D_SIGNAL".into()
            }
            .summary(),
            "D_SIGNAL"
        );
        let bjt = Part::Bjt {
            polarity: Polarity::P,
            model: "PNP_GP".into(),
        };
        assert_eq!(bjt.summary(), "PNP_GP");
        assert_eq!(Part::Ground.summary(), "");
    }
}
