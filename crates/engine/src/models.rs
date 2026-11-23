//! Semiconductor, switch and op-amp model parameters (voltage- and
//! current-controlled switches: [`SwitchModel`], [`CurrentSwitchModel`];
//! op-amps: [`OpAmpModel`]).
//!
//! Parameter names follow the classic Berkeley SPICE models so manufacturer
//! model cards translate one-to-one. Defaults are the SPICE defaults. All
//! values are SI; temperatures are in °C.
//!
//! ## Conventions shared by all models
//! * Corner currents and Early voltages (`ikf`, `ikr`, `vaf`, `var`, `irb`,
//!   `vtf`) use `f64::INFINITY` for "no effect". A value of `0.0`, which is
//!   how SPICE model cards spell "not given" for these, also means "no
//!   effect".
//! * Ohmic resistances of `0.0` mean the resistance is absent and no
//!   internal node is created.
//! * Parameters whose value would make the equations meaningless (a zero or
//!   negative emission coefficient or junction potential, a negative
//!   saturation current or resistance, NaN, …) are rejected by
//!   [`crate::Circuit::validate`], which every simulation runs first, with
//!   an error naming the element, the model and the parameter.
//! * Every model is characterised at `tnom` and adjusted to the simulation
//!   temperature with the SPICE3 temperature equations (the BJT's
//!   collector-substrate `cjs`/`vjs` stay constant, as in SPICE3 and
//!   LTspice).
//!
//! ## Serialization
//! Every parameter round-trips exactly through serde, JSON included.
//! Finite values are written as numbers; the non-finite values JSON cannot
//! hold are written as the strings `"inf"`, `"-inf"` and `"nan"` (so the
//! default `bv = f64::INFINITY` is `"bv": "inf"`) and read back from them.
//! Missing fields take their default values.

use serde::{Deserialize, Serialize};

/// Serde adapter for `f64` model parameters: finite values as numbers,
/// `±∞` and NaN as the strings `"inf"`, `"-inf"` and `"nan"`.
mod number {
    use serde::de::{Error, Unexpected, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub(super) fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        let value = *value;
        if value.is_finite() {
            serializer.serialize_f64(value)
        } else if value.is_nan() {
            serializer.serialize_str("nan")
        } else if value > 0.0 {
            serializer.serialize_str("inf")
        } else {
            serializer.serialize_str("-inf")
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        deserializer.deserialize_any(NumberVisitor)
    }

    struct NumberVisitor;

    impl Visitor<'_> for NumberVisitor {
        type Value = f64;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a number or one of \"inf\", \"-inf\", \"nan\"")
        }

        fn visit_f64<E: Error>(self, value: f64) -> Result<f64, E> {
            Ok(value)
        }

        fn visit_i64<E: Error>(self, value: i64) -> Result<f64, E> {
            Ok(value as f64)
        }

        fn visit_u64<E: Error>(self, value: u64) -> Result<f64, E> {
            Ok(value as f64)
        }

        fn visit_str<E: Error>(self, value: &str) -> Result<f64, E> {
            match value {
                "inf" => Ok(f64::INFINITY),
                "-inf" => Ok(f64::NEG_INFINITY),
                "nan" => Ok(f64::NAN),
                _ => Err(E::invalid_value(Unexpected::Str(value), &self)),
            }
        }
    }
}

/// Device polarity for BJTs (NPN/PNP) and JFETs (N/P channel).
///
/// Model parameters are always written for the N-type orientation: a PNP's
/// `is` and `vaf` are positive and a P-channel JFET's depletion-mode `vto`
/// is negative, exactly as in SPICE model cards. The simulator flips the
/// signs of terminal voltages and currents for P-type devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Polarity {
    /// NPN transistor or N-channel JFET.
    N,
    /// PNP transistor or P-channel JFET.
    P,
}

impl Polarity {
    /// `+1` for N-type and `−1` for P-type devices.
    pub(crate) fn sign(self) -> f64 {
        match self {
            Self::N => 1.0,
            Self::P => -1.0,
        }
    }
}

/// Junction diode (SPICE level 1) with series resistance, depletion and
/// diffusion capacitance, reverse breakdown and temperature dependence (Q83).
///
/// The forward current is `is·(e^(V/(n·Vt)) − 1)`. With a finite `bv` a
/// breakdown current `−is·e^(−(V + BV')/(nbv·Vt))` is added, where `BV'` is
/// chosen so the current at `V = −bv` is exactly `−ibv`. The diffusion charge
/// is `tt` times the forward current; the depletion charge follows `cjo`,
/// `vj`, `m` and `fc`. `is`, `ibv` and `cjo` scale with the instance area,
/// `rs` divides by it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiodeModel {
    pub name: String,
    /// Saturation current (A).
    #[serde(with = "number")]
    pub is: f64,
    /// Emission coefficient.
    #[serde(with = "number")]
    pub n: f64,
    /// Ohmic series resistance (Ω).
    #[serde(with = "number")]
    pub rs: f64,
    /// Zero-bias junction capacitance (F).
    #[serde(with = "number")]
    pub cjo: f64,
    /// Junction potential (V).
    #[serde(with = "number")]
    pub vj: f64,
    /// Grading coefficient.
    #[serde(with = "number")]
    pub m: f64,
    /// Transit time (s).
    #[serde(with = "number")]
    pub tt: f64,
    /// Reverse breakdown voltage (V, positive). Infinite means no breakdown.
    #[serde(with = "number")]
    pub bv: f64,
    /// Current at breakdown voltage (A).
    #[serde(with = "number")]
    pub ibv: f64,
    /// Breakdown emission coefficient.
    #[serde(with = "number")]
    pub nbv: f64,
    /// Activation energy (eV).
    #[serde(with = "number")]
    pub eg: f64,
    /// Saturation-current temperature exponent.
    #[serde(with = "number")]
    pub xti: f64,
    /// Forward-bias depletion capacitance coefficient (clamped to 0.95).
    #[serde(with = "number")]
    pub fc: f64,
    /// Flicker-noise coefficient and exponent (used by noise analysis).
    #[serde(with = "number")]
    pub kf: f64,
    #[serde(with = "number")]
    pub af: f64,
    /// Parameter measurement temperature (°C).
    #[serde(with = "number")]
    pub tnom: f64,
}

impl Default for DiodeModel {
    fn default() -> Self {
        Self {
            name: "D".into(),
            is: 1e-14,
            n: 1.0,
            rs: 0.0,
            cjo: 0.0,
            vj: 1.0,
            m: 0.5,
            tt: 0.0,
            bv: f64::INFINITY,
            ibv: 1e-10,
            nbv: 1.0,
            eg: 1.11,
            xti: 3.0,
            fc: 0.5,
            kf: 0.0,
            af: 1.0,
            tnom: 27.0,
        }
    }
}

/// Gummel–Poon bipolar transistor (SPICE BJT level 1).
///
/// The substrate terminal of the collector-substrate capacitance `cjs` is
/// ground. Currents, capacitances and `ikf`/`ikr`/`irb`/`itf` scale with the
/// instance area; resistances divide by it.
///
/// Base resistance: with `rb > 0` the base spreading resistance falls from
/// `rb` at low current towards `rbm` at high current, through the base
/// charge `qb` or, when `irb` is finite, through the SPICE `irb` formula.
/// SPICE model cards that omit `RBM` mean `RBM = RB` (constant base
/// resistance); converters must then set `rbm = rb` explicitly, because the
/// default `rbm = 0` lets the resistance fall with `qb`. Values of `rbm`
/// above `rb` are clamped to `rb`; with `rb = 0` there is no base resistance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BjtModel {
    pub name: String,
    pub polarity: Polarity,
    /// Transport saturation current (A).
    #[serde(with = "number")]
    pub is: f64,
    /// Ideal maximum forward / reverse beta.
    #[serde(with = "number")]
    pub bf: f64,
    #[serde(with = "number")]
    pub br: f64,
    /// Forward / reverse current emission coefficients.
    #[serde(with = "number")]
    pub nf: f64,
    #[serde(with = "number")]
    pub nr: f64,
    /// Forward / reverse Early voltages (V). Infinite = no Early effect.
    #[serde(with = "number")]
    pub vaf: f64,
    #[serde(with = "number")]
    pub var: f64,
    /// Forward / reverse beta high-current roll-off corners (A). Infinite = none.
    #[serde(with = "number")]
    pub ikf: f64,
    #[serde(with = "number")]
    pub ikr: f64,
    /// B-E / B-C leakage saturation currents (A) and emission coefficients.
    #[serde(with = "number")]
    pub ise: f64,
    #[serde(with = "number")]
    pub ne: f64,
    #[serde(with = "number")]
    pub isc: f64,
    #[serde(with = "number")]
    pub nc: f64,
    /// Zero-bias base resistance, current where it falls halfway, minimum (Ω, A, Ω).
    #[serde(with = "number")]
    pub rb: f64,
    #[serde(with = "number")]
    pub irb: f64,
    #[serde(with = "number")]
    pub rbm: f64,
    /// Emitter and collector resistances (Ω).
    #[serde(with = "number")]
    pub re: f64,
    #[serde(with = "number")]
    pub rc: f64,
    /// B-E depletion capacitance (F), built-in potential (V), grading.
    #[serde(with = "number")]
    pub cje: f64,
    #[serde(with = "number")]
    pub vje: f64,
    #[serde(with = "number")]
    pub mje: f64,
    /// B-C depletion capacitance (F), built-in potential (V), grading.
    #[serde(with = "number")]
    pub cjc: f64,
    #[serde(with = "number")]
    pub vjc: f64,
    #[serde(with = "number")]
    pub mjc: f64,
    /// Fraction of B-C capacitance connected to the internal base; the rest
    /// connects the external base to the internal collector.
    #[serde(with = "number")]
    pub xcjc: f64,
    /// Collector-substrate capacitance (F), potential (V), grading.
    #[serde(with = "number")]
    pub cjs: f64,
    #[serde(with = "number")]
    pub vjs: f64,
    #[serde(with = "number")]
    pub mjs: f64,
    /// Ideal forward transit time (s) and its bias dependence: `xtf`
    /// coefficient, `vtf` B-C voltage dependence (V, infinite = none) and
    /// `itf` high-current parameter (A, 0 = none).
    #[serde(with = "number")]
    pub tf: f64,
    #[serde(with = "number")]
    pub xtf: f64,
    #[serde(with = "number")]
    pub vtf: f64,
    #[serde(with = "number")]
    pub itf: f64,
    /// Excess phase at 1/(2π·tf) Hz (degrees). Applied in AC analysis;
    /// transient analysis ignores it.
    #[serde(with = "number")]
    pub ptf: f64,
    /// Ideal reverse transit time (s).
    #[serde(with = "number")]
    pub tr: f64,
    /// Energy gap (eV), beta temperature exponent, IS temperature exponent.
    #[serde(with = "number")]
    pub eg: f64,
    #[serde(with = "number")]
    pub xtb: f64,
    #[serde(with = "number")]
    pub xti: f64,
    /// Forward-bias depletion capacitance coefficient (clamped to 0.95).
    #[serde(with = "number")]
    pub fc: f64,
    #[serde(with = "number")]
    pub kf: f64,
    #[serde(with = "number")]
    pub af: f64,
    #[serde(with = "number")]
    pub tnom: f64,
}

impl Default for BjtModel {
    fn default() -> Self {
        Self {
            name: "NPN".into(),
            polarity: Polarity::N,
            is: 1e-16,
            bf: 100.0,
            br: 1.0,
            nf: 1.0,
            nr: 1.0,
            vaf: f64::INFINITY,
            var: f64::INFINITY,
            ikf: f64::INFINITY,
            ikr: f64::INFINITY,
            ise: 0.0,
            ne: 1.5,
            isc: 0.0,
            nc: 2.0,
            rb: 0.0,
            irb: f64::INFINITY,
            rbm: 0.0,
            re: 0.0,
            rc: 0.0,
            cje: 0.0,
            vje: 0.75,
            mje: 0.33,
            cjc: 0.0,
            vjc: 0.75,
            mjc: 0.33,
            xcjc: 1.0,
            cjs: 0.0,
            vjs: 0.75,
            mjs: 0.0,
            tf: 0.0,
            xtf: 0.0,
            vtf: f64::INFINITY,
            itf: 0.0,
            ptf: 0.0,
            tr: 0.0,
            eg: 1.11,
            xtb: 0.0,
            xti: 3.0,
            fc: 0.5,
            kf: 0.0,
            af: 1.0,
            tnom: 27.0,
        }
    }
}

/// Shichman–Hodges JFET (SPICE JFET level 1) with the Sydney University
/// doping-tail extension `b`.
///
/// The gate junctions are ideal diodes (emission coefficient 1) with
/// saturation current `is` and grading coefficient 0.5. `beta`, `is`, `cgs`
/// and `cgd` scale with the instance area; `rd` and `rs` divide by it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JfetModel {
    pub name: String,
    pub polarity: Polarity,
    /// Threshold (pinch-off) voltage (V). Negative for depletion-mode devices
    /// of either polarity (see [`Polarity`]).
    #[serde(with = "number")]
    pub vto: f64,
    /// Transconductance parameter (A/V²).
    #[serde(with = "number")]
    pub beta: f64,
    /// Channel-length modulation (1/V).
    #[serde(with = "number")]
    pub lambda: f64,
    /// Drain and source ohmic resistances (Ω).
    #[serde(with = "number")]
    pub rd: f64,
    #[serde(with = "number")]
    pub rs: f64,
    /// Zero-bias G-S and G-D junction capacitances (F).
    #[serde(with = "number")]
    pub cgs: f64,
    #[serde(with = "number")]
    pub cgd: f64,
    /// Gate junction potential (V).
    #[serde(with = "number")]
    pub pb: f64,
    /// Gate junction saturation current (A).
    #[serde(with = "number")]
    pub is: f64,
    /// Doping tail parameter (Sydney University model extension; 1 = classic).
    #[serde(with = "number")]
    pub b: f64,
    /// VTO temperature coefficient (V/°C) and BETA exponential temperature
    /// coefficient (%/°C): `beta(T) = beta·1.01^(betatce·(T − tnom))`.
    #[serde(with = "number")]
    pub vtotc: f64,
    #[serde(with = "number")]
    pub betatce: f64,
    /// Forward-bias depletion capacitance coefficient (clamped to 0.95).
    #[serde(with = "number")]
    pub fc: f64,
    #[serde(with = "number")]
    pub kf: f64,
    #[serde(with = "number")]
    pub af: f64,
    #[serde(with = "number")]
    pub tnom: f64,
}

impl Default for JfetModel {
    fn default() -> Self {
        Self {
            name: "NJF".into(),
            polarity: Polarity::N,
            vto: -2.0,
            beta: 1e-4,
            lambda: 0.0,
            rd: 0.0,
            rs: 0.0,
            cgs: 0.0,
            cgd: 0.0,
            pb: 1.0,
            is: 1e-14,
            b: 1.0,
            vtotc: 0.0,
            betatce: 0.0,
            fc: 0.5,
            kf: 0.0,
            af: 1.0,
            tnom: 27.0,
        }
    }
}

/// Voltage-controlled switch with hysteresis (Q87, D17), with the SPICE3 /
/// LTspice semantics: for `vh ≥ 0` the switch turns on when the control
/// voltage exceeds `vt + vh` and off when it falls below `vt − vh`; between
/// the thresholds, and exactly at either of them, it keeps its state (with
/// `vh = 0` both thresholds are `vt`). Switches start in the off state.
///
/// Operating points and DC sweeps use exactly `ron` or `roff`, and so does
/// every transient timestep: the switch changes state ideally, at a time
/// point the transient places within 1 µV of the control beyond the
/// threshold (it retries a step that crosses it further), and the
/// integration restarts there as after a source breakpoint. A switch whose
/// own current drives its control back across the thresholds (a relaxation
/// oscillator) oscillates and has no operating point. A negative `vh`
/// selects LTspice's smooth mode
/// instead: no hysteresis, and the resistance follows a log-cubic in the
/// control voltage from `vt − |vh|` to `vt + |vh|`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwitchModel {
    pub name: String,
    /// On / off resistance (Ω).
    #[serde(with = "number")]
    pub ron: f64,
    #[serde(with = "number")]
    pub roff: f64,
    /// Threshold voltage (V) and hysteresis half-width (V).
    #[serde(with = "number")]
    pub vt: f64,
    #[serde(with = "number")]
    pub vh: f64,
}

impl Default for SwitchModel {
    fn default() -> Self {
        Self {
            name: "SW".into(),
            ron: 1.0,
            roff: 1e9,
            vt: 0.0,
            vh: 0.0,
        }
    }
}

/// Current-controlled switch with hysteresis (Q87, D17): SPICE's `W`
/// element with an LTspice `CSW` model. The controlling quantity is the
/// branch current of a named independent voltage source
/// ([`crate::Device::CurrentSwitch`]), and the switch behaves exactly like
/// [`SwitchModel`] with that current in place of the control voltage: for
/// `ih ≥ 0` it turns on when the current exceeds `it + ih` and off when it
/// falls below `it − ih`, keeping its state between the thresholds and
/// exactly at either of them; switches start off; a negative `ih` selects
/// the smooth mode (log-cubic from `it − |ih|` to `it + |ih|`).
///
/// In a transient the switch changes state within `10⁻⁶·(|it| + |ih|)`, at
/// least 1 pA, of the control beyond the active threshold: to the control
/// current what the voltage switch's 1 µV is to a volt-scale control
/// voltage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CurrentSwitchModel {
    pub name: String,
    /// On / off resistance (Ω).
    #[serde(with = "number")]
    pub ron: f64,
    #[serde(with = "number")]
    pub roff: f64,
    /// Threshold current (A) and hysteresis half-width (A).
    #[serde(with = "number")]
    pub it: f64,
    #[serde(with = "number")]
    pub ih: f64,
}

impl Default for CurrentSwitchModel {
    fn default() -> Self {
        Self {
            name: "CSW".into(),
            ron: 1.0,
            roff: 1e9,
            it: 0.0,
            ih: 0.0,
        }
    }
}

/// Which op-amp model [`OpAmpModel`] describes (D26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum OpAmpKind {
    /// Very high gain and infinite bandwidth; the output is limited to the
    /// supply rails. Uses `aol` and `rout` only.
    Ideal,
    /// Single-pole macro-model: open-loop gain, gain-bandwidth pole, slew
    /// rate, output limited to the rails minus headroom, input and output
    /// resistance.
    #[default]
    Macro,
}

/// Op-amp model (D26) of [`crate::Device::OpAmp`]. The equations are in
/// the documentation of the device (`devices/opamp.rs`); in short, with
/// `vd = V(+) − V(−)`:
///
/// * **Ideal:** the output is `T(aol·vd)` behind `rout`, where `T` limits
///   smoothly to the supply rails exactly (no headroom). No input current;
///   `gbw`, `slew`, `rin` and the headrooms are ignored.
/// * **Macro:** an internal state `x` follows `τ·dx/dt = L·tanh(e/L)` with
///   `e = aol·vd − x` (plus a clamp that keeps `x` just beyond the output
///   range), `τ = aol/(2π·gbw)` and `L = slew·τ`: one pole at `gbw/aol`
///   in small signal and a slope of exactly `slew` in large signal. The
///   output is `T(x)` behind `rout`, limited to
///   `[V− + headroom_neg, V+ − headroom_pos]`; `rin` joins the inputs.
///
/// The output current is drawn from V+ while sourcing and returned to V−
/// while sinking; there is no quiescent current.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpAmpModel {
    pub name: String,
    pub kind: OpAmpKind,
    /// Open-loop DC gain (V/V).
    #[serde(with = "number")]
    pub aol: f64,
    /// Gain-bandwidth product (Hz); the open-loop pole is at `gbw/aol`.
    #[serde(with = "number")]
    pub gbw: f64,
    /// Slew rate (V/s).
    #[serde(with = "number")]
    pub slew: f64,
    /// Differential input resistance (Ω).
    #[serde(with = "number")]
    pub rin: f64,
    /// Output resistance (Ω).
    #[serde(with = "number")]
    pub rout: f64,
    /// How far below V+ and above V− the output limits (V).
    #[serde(with = "number")]
    pub headroom_pos: f64,
    #[serde(with = "number")]
    pub headroom_neg: f64,
}

impl Default for OpAmpModel {
    /// The general-purpose macro-model of the library (`OA_GP`).
    fn default() -> Self {
        Self {
            name: "OA_GP".into(),
            kind: OpAmpKind::Macro,
            aol: 1e5,
            gbw: 1e6,
            slew: 0.5e6,
            rin: 2e6,
            rout: 75.0,
            headroom_pos: 1.5,
            headroom_neg: 1.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polarity_sign_flips_p_type() {
        assert_eq!(Polarity::N.sign(), 1.0);
        assert_eq!(Polarity::P.sign(), -1.0);
    }
}
