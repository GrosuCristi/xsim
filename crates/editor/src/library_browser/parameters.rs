//! Key model parameters shown in the library preview, formatted with units.
//! GPUI-free.

use xsim_spice_engine::{
    BjtModel, DiodeModel, JfetModel, OpAmpKind, OpAmpModel, Polarity, units::format_value,
};

/// A labelled parameter value, e.g. `("BF", "200")`.
pub(crate) type Parameter = (&'static str, String);

/// `value` in engineering notation with `unit`; infinity (an absent
/// effect, such as no breakdown) shows as `∞`.
fn quantity(value: f64, unit: &str) -> String {
    if value == f64::INFINITY {
        "∞".into()
    } else {
        format_value(value, unit)
    }
}

/// Saturation current, emission coefficient, series resistance, junction
/// capacitance, transit time and breakdown of a diode.
pub(crate) fn diode(model: &DiodeModel) -> Vec<Parameter> {
    vec![
        ("IS", quantity(model.is, "A")),
        ("N", quantity(model.n, "")),
        ("RS", quantity(model.rs, "Ω")),
        ("CJO", quantity(model.cjo, "F")),
        ("TT", quantity(model.tt, "s")),
        ("BV", quantity(model.bv, "V")),
        ("IBV", quantity(model.ibv, "A")),
    ]
}

/// Polarity, gain, Early voltage, currents, capacitances and transit time of
/// a bipolar transistor.
pub(crate) fn bjt(model: &BjtModel) -> Vec<Parameter> {
    let polarity = match model.polarity {
        Polarity::N => "NPN",
        Polarity::P => "PNP",
    };
    vec![
        ("Polarity", polarity.into()),
        ("BF", quantity(model.bf, "")),
        ("VAF", quantity(model.vaf, "V")),
        ("IS", quantity(model.is, "A")),
        ("IKF", quantity(model.ikf, "A")),
        ("BR", quantity(model.br, "")),
        ("CJE", quantity(model.cje, "F")),
        ("CJC", quantity(model.cjc, "F")),
        ("TF", quantity(model.tf, "s")),
    ]
}

/// Polarity, threshold, transconductance, channel-length modulation, gate
/// current and capacitances of a JFET.
pub(crate) fn jfet(model: &JfetModel) -> Vec<Parameter> {
    let polarity = match model.polarity {
        Polarity::N => "N-channel",
        Polarity::P => "P-channel",
    };
    vec![
        ("Polarity", polarity.into()),
        ("VTO", quantity(model.vto, "V")),
        ("BETA", quantity(model.beta, "A/V²")),
        ("LAMBDA", format!("{} V⁻¹", quantity(model.lambda, ""))),
        ("IS", quantity(model.is, "A")),
        ("CGS", quantity(model.cgs, "F")),
        ("CGD", quantity(model.cgd, "F")),
    ]
}

/// `value` with at most four significant digits and no trailing zeros,
/// e.g. `0.5`, `10`, `3.16`.
fn plain(value: f64) -> String {
    let magnitude = if value == 0.0 {
        0
    } else {
        value.abs().log10().floor() as i32
    };
    let digits = (3 - magnitude).clamp(0, 12) as usize;
    let text = format!("{value:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').into()
    } else {
        text
    }
}

/// Model type, open-loop gain, gain-bandwidth, slew rate, output headroom,
/// input and output resistance of an op-amp (D26). What the ideal kind
/// ignores (bandwidth, slew rate, input resistance, headroom) shows as
/// unlimited, as it behaves.
pub(crate) fn opamp(model: &OpAmpModel) -> Vec<Parameter> {
    let ideal = model.kind == OpAmpKind::Ideal;
    let kind = if ideal { "Ideal" } else { "Macro-model" };
    let gain = format!(
        "{} ({} dB)",
        quantity(model.aol, ""),
        plain((20.0 * model.aol.log10()).round())
    );
    let (gbw, slew, rin) = if ideal {
        (f64::INFINITY, f64::INFINITY, f64::INFINITY)
    } else {
        (model.gbw, model.slew, model.rin)
    };
    let slew = if slew == f64::INFINITY {
        "∞".into()
    } else {
        format!("{} V/µs", plain(slew / 1e6))
    };
    let headroom = if ideal {
        "None (to the rails)".into()
    } else if model.headroom_pos == model.headroom_neg {
        format!("{} from each rail", quantity(model.headroom_pos, "V"))
    } else {
        format!(
            "{} from V+, {} from V\u{2212}",
            quantity(model.headroom_pos, "V"),
            quantity(model.headroom_neg, "V")
        )
    };
    vec![
        ("Type", kind.into()),
        ("AOL", gain),
        ("GBW", quantity(gbw, "Hz")),
        ("Slew rate", slew),
        ("Headroom", headroom),
        ("RIN", quantity(rin, "Ω")),
        ("ROUT", quantity(model.rout, "Ω")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(parameters: &[Parameter]) -> Vec<&'static str> {
        parameters.iter().map(|p| p.0).collect()
    }

    #[test]
    fn each_family_lists_its_key_parameters() {
        let diode = diode(&DiodeModel::default());
        assert!(["IS", "N", "BV"].iter().all(|l| labels(&diode).contains(l)));
        let bjt = bjt(&BjtModel::default());
        assert!(
            ["Polarity", "BF", "VAF", "IS"]
                .iter()
                .all(|l| labels(&bjt).contains(l))
        );
        let jfet = jfet(&JfetModel::default());
        assert!(
            ["Polarity", "VTO", "BETA"]
                .iter()
                .all(|l| labels(&jfet).contains(l))
        );
    }

    /// D26: every library op-amp lists the same seven parameters, so the
    /// model card keeps its height whichever model is chosen; the ideal one
    /// shows what it ignores as unlimited.
    #[test]
    fn opamps_list_their_parameters_with_units() {
        let value = |model: &str, label: &str| {
            let model = xsim_spice_engine::library::opamp(model).expect("a library op-amp");
            opamp(&model)
                .into_iter()
                .find(|p| p.0 == label)
                .map(|p| p.1)
                .unwrap_or_default()
        };
        for entry in xsim_spice_engine::library::opamps() {
            assert_eq!(
                labels(&opamp(&entry.model)),
                ["Type", "AOL", "GBW", "Slew rate", "Headroom", "RIN", "ROUT"]
            );
        }
        assert_eq!(value("OA_GP", "Type"), "Macro-model");
        assert_eq!(value("OA_GP", "AOL"), "100k (100 dB)");
        assert_eq!(value("OA_GP", "GBW"), "1 MHz");
        assert_eq!(value("OA_GP", "Slew rate"), "0.5 V/µs");
        assert_eq!(value("OA_GP", "Headroom"), "1.5 V from each rail");
        assert_eq!(value("OA_GP", "RIN"), "2 MΩ");
        assert_eq!(value("OA_GP", "ROUT"), "75 Ω");
        assert_eq!(value("OA_FAST", "Slew rate"), "10 V/µs");
        assert_eq!(value("OA_RRIO", "AOL"), "316k (110 dB)");
        assert_eq!(value("OA_RRIO", "Headroom"), "50 mV from each rail");
        assert_eq!(value("OA_IDEAL", "Type"), "Ideal");
        assert_eq!(value("OA_IDEAL", "AOL"), "1M (120 dB)");
        for label in ["GBW", "Slew rate", "RIN"] {
            assert_eq!(value("OA_IDEAL", label), "∞", "{label}");
        }
        assert_eq!(value("OA_IDEAL", "Headroom"), "None (to the rails)");
        assert_eq!(value("OA_IDEAL", "ROUT"), "1 mΩ");
        let asymmetric = OpAmpModel {
            headroom_neg: 0.2,
            ..OpAmpModel::default()
        };
        assert_eq!(
            opamp(&asymmetric)[4].1,
            "1.5 V from V+, 200 mV from V\u{2212}"
        );
        assert_eq!(plain(0.0), "0");
        assert_eq!(plain(3.16), "3.16");
    }

    #[test]
    fn values_carry_units_and_infinity_reads_as_absent() {
        let model = DiodeModel {
            is: 4.75e-9,
            ..DiodeModel::default()
        };
        let parameters = diode(&model);
        assert_eq!(parameters[0], ("IS", "4.75 nA".to_string()));
        let bv = parameters
            .iter()
            .find(|p| p.0 == "BV")
            .map(|p| p.1.as_str());
        assert_eq!(bv, Some("∞"), "the default diode has no breakdown");
        let bjt = bjt(&BjtModel {
            polarity: Polarity::P,
            ..BjtModel::default()
        });
        assert_eq!(bjt[0].1, "PNP");
        assert_eq!(bjt[2].1, "∞", "no Early effect by default");
    }
}
