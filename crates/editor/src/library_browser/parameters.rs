//! Key model parameters shown in the library preview, formatted with units.
//! GPUI-free.

use xsim_spice_engine::{BjtModel, DiodeModel, JfetModel, Polarity, units::format_value};

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
