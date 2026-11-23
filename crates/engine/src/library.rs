//! Built-in generic model library (Q141, D13).
//!
//! Every model records where its parameters come from in [`ModelEntry::source`].
//! Model names are case-sensitive and unique within each device family.
//!
//! ## Provenance rules (D13)
//! * Every model is a generic model authored for xsim. None describes a
//!   specific part, and no parameter is copied from a manufacturer's model
//!   card or from another simulator's bundled library. Part-numbered models
//!   come from manufacturer libraries that users import themselves (Q142).
//! * Each model starts from stated design targets, for example "VF ≈ 0.62 V
//!   at 1 mA". The parameters are solved from those targets with the device
//!   equations xsim implements (SPICE level-1 diode, Gummel–Poon BJT,
//!   Shichman–Hodges JFET) at 27 °C and then rounded. The source string
//!   lists the targets and how each parameter follows from them, and
//!   `tests/library.rs` checks every target by simulation.
//! * Names are upper-case identifiers that start with the device type
//!   (`D_`, `DZ_`, `LED_`, `NPN_`, `PNP_`, `NJF_`, `PJF_`), never a part
//!   number. The manufacturer is always "Generic".
//! * Where `RB` is set, `rbm = rb` (a constant base resistance), because
//!   the default `rbm = 0` would let the resistance fall with the base charge.
//! * BJT breakdown is not modelled, so voltage classes such as "high
//!   voltage" describe the intended use, not a simulated limit.
//! * Parameters a target does not involve keep the SPICE defaults: `EG =
//!   1.11 eV` and `XTI = 3` for silicon junctions, `FC = 0.5`, `TNOM = 27 °C`.
//!   The LEDs' `EG` is solved from a forward-voltage temperature coefficient
//!   target instead.

use crate::models::{BjtModel, DiodeModel, JfetModel, Polarity};

/// Metadata shared by all library entries (shown by the model browser, Q144).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry<M> {
    pub model: M,
    /// Library category path, e.g. `["Diodes", "Switching"]`.
    pub category: &'static [&'static str],
    /// One-line human description, e.g. "Generic small-signal switching diode".
    pub description: &'static str,
    /// Manufacturer or "Generic".
    pub manufacturer: &'static str,
    /// Provenance of the parameter values (document, URL or derivation).
    pub source: &'static str,
}

const SWITCHING: &[&str] = &["Diodes", "Switching"];
const RECTIFIER: &[&str] = &["Diodes", "Rectifiers"];
const SCHOTTKY: &[&str] = &["Diodes", "Schottky"];
const ZENER: &[&str] = &["Diodes", "Zener"];
const LED: &[&str] = &["Diodes", "LED"];
const NPN_SMALL: &[&str] = &["Transistors", "Bipolar", "NPN", "Small signal"];
const NPN_POWER: &[&str] = &["Transistors", "Bipolar", "NPN", "Power"];
const PNP_SMALL: &[&str] = &["Transistors", "Bipolar", "PNP", "Small signal"];
const PNP_POWER: &[&str] = &["Transistors", "Bipolar", "PNP", "Power"];
const NJF: &[&str] = &["Transistors", "JFET", "N-channel"];
const PJF: &[&str] = &["Transistors", "JFET", "P-channel"];

/// Manufacturer of every built-in model.
const GENERIC: &str = "Generic";

fn entry<M>(
    model: M,
    category: &'static [&'static str],
    description: &'static str,
    source: &'static str,
) -> ModelEntry<M> {
    ModelEntry {
        model,
        category,
        description,
        manufacturer: GENERIC,
        source,
    }
}

/// A zener diode: silicon forward junction (`IS = 10 fA`, `N = 1`) and a
/// breakdown with `IBV` equal to the test current, so the terminal voltage
/// at that current is `BV + IBV·RS`. The level-1 diode's single `RS` acts
/// in both directions, so it stays at 1 Ω (forward ≈ 0.87 V at 100 mA) and
/// the breakdown slope `NBV` sets the dynamic resistance.
fn zener(name: &str, bv: f64, rs: f64, nbv: f64, cjo: f64) -> DiodeModel {
    DiodeModel {
        name: name.into(),
        is: 1e-14,
        n: 1.0,
        rs,
        cjo,
        vj: 0.8,
        m: 0.4,
        tt: 50e-9,
        bv,
        ibv: 5e-3,
        nbv,
        ..DiodeModel::default()
    }
}

/// All built-in diode models.
pub fn diodes() -> Vec<ModelEntry<DiodeModel>> {
    vec![
        entry(
            DiodeModel {
                name: "D_SIGNAL".into(),
                is: 3e-10,
                n: 1.6,
                rs: 0.4,
                cjo: 2e-12,
                vj: 0.7,
                m: 0.25,
                tt: 5e-9,
                bv: 100.0,
                ibv: 100e-6,
                ..DiodeModel::default()
            },
            SWITCHING,
            "Generic small-signal switching diode, 100 V, 4 ns class",
            "xsim generic model (D13): parameters chosen for VF ≈ 0.62 V at 1 mA, 0.72 V at \
             10 mA and 0.85 V at 100 mA (IS, N and RS solved from the three points of \
             V = N·Vt·ln(I/IS) + I·RS, giving 3.9e-10 A, 1.62 and 0.37 Ω; N and RS rounded to \
             1.6 and 0.4 Ω and IS refitted to 3e-10 A for them); BV = 100 V at IBV = 100 µA; CJ0 = 2 pF falling to ≈ 1 pF at 10 V \
             reverse (VJ = 0.7 V, M = 0.25); TT = 5 ns, a charge-control storage time \
             TT·ln 2 ≈ 3.5 ns at IF = IR (4 ns trr class); not a model of any specific part",
        ),
        entry(
            DiodeModel {
                name: "D_RECT_1A".into(),
                is: 2.4e-8,
                n: 1.85,
                rs: 0.11,
                cjo: 20e-12,
                vj: 0.7,
                m: 0.4,
                tt: 3e-6,
                bv: 1000.0,
                ibv: 10e-6,
                ..DiodeModel::default()
            },
            RECTIFIER,
            "Generic 1 A general-purpose rectifier, 1000 V, slow recovery",
            "xsim generic model (D13): parameters chosen for VF ≈ 0.62 V at 10 mA, 0.74 V at \
             100 mA and 0.95 V at 1 A (IS, N and RS solved from the three points of \
             V = N·Vt·ln(I/IS) + I·RS and rounded to 2.4e-8 A, 1.85 and 0.11 Ω); \
             BV = 1000 V at IBV = 10 µA; CJ0 = 20 pF falling to ≈ 9 pF at 4 V reverse \
             (VJ = 0.7 V, M = 0.4); TT = 3 µs, a storage time TT·ln 2 ≈ 2 µs at IF = IR \
             (slow mains rectifier); not a model of any specific part",
        ),
        entry(
            DiodeModel {
                name: "D_FAST_1A".into(),
                is: 2.1e-8,
                n: 1.95,
                rs: 0.16,
                cjo: 15e-12,
                vj: 0.7,
                m: 0.4,
                tt: 200e-9,
                bv: 600.0,
                ibv: 10e-6,
                ..DiodeModel::default()
            },
            RECTIFIER,
            "Generic 1 A fast-recovery rectifier, 600 V, 150 ns class",
            "xsim generic model (D13): parameters chosen for VF ≈ 0.66 V at 10 mA, 0.79 V at \
             100 mA and 1.05 V at 1 A (IS, N and RS solved from the three points of \
             V = N·Vt·ln(I/IS) + I·RS and rounded to 2.1e-8 A, 1.95 and 0.16 Ω); \
             BV = 600 V at IBV = 10 µA; CJ0 = 15 pF falling to ≈ 7 pF at 4 V reverse \
             (VJ = 0.7 V, M = 0.4); TT = 200 ns, a storage time TT·ln 2 ≈ 140 ns at IF = IR; \
             not a model of any specific part",
        ),
        entry(
            DiodeModel {
                name: "D_SCHOTTKY_1A".into(),
                is: 2.3e-6,
                n: 1.05,
                rs: 0.1,
                cjo: 150e-12,
                vj: 0.5,
                m: 0.5,
                tt: 0.0,
                bv: 40.0,
                ibv: 0.5e-3,
                eg: 0.69,
                xti: 2.0,
                ..DiodeModel::default()
            },
            SCHOTTKY,
            "Generic 1 A Schottky barrier rectifier, 40 V, low forward drop",
            "xsim generic model (D13): parameters chosen for VF ≈ 0.30 V at 100 mA and 0.45 V \
             at 1 A with the near-ideal slope N = 1.05 of a metal-semiconductor barrier (IS and \
             RS solved from the two points of V = N·Vt·ln(I/IS) + I·RS and rounded to 2.3 µA and \
             0.1 Ω, which gives ≈ 0.23 V at 10 mA); BV = 40 V at IBV = 0.5 mA; CJ0 = 150 pF of \
             an abrupt barrier, ≈ 50 pF at 4 V reverse (VJ = 0.5 V, M = 0.5); TT = 0, \
             majority-carrier conduction stores no charge; EG = 0.69 eV and XTI = 2, the usual \
             Schottky barrier temperature parameters; not a model of any specific part",
        ),
        entry(
            zener("DZ_3V3", 3.295, 1.0, 5.5, 150e-12),
            ZENER,
            "Generic 3.3 V zener diode, 5 mA test current",
            "xsim generic model (D13): parameters chosen for VZ = 3.3 V at IZT = 5 mA with the \
             soft knee of a low-voltage zener (IBV = IZT, BV = VZ − IZT·RS = 3.295 V) and the \
             largest dynamic resistance the level-1 breakdown allows without conducting at \
             0 V: rz = NBV·Vt/IZT + RS ≈ 30 Ω with NBV = 5.5 and RS = 1 Ω, where the breakdown \
             term carries IBV·e^(−BV/(NBV·Vt)) ≈ 0.4 pA at 0 V (a typical 40 Ω would need \
             NBV ≈ 7.5 and 0.2 nA, or an RS that also adds to the forward drop); about \
             0.5 mA at 0.9·VZ; forward IS = 10 fA, N = 1 (VF ≈ 0.72 V at 10 mA, 0.87 V at \
             100 mA); CJ0 = 150 pF (VJ = 0.8 V, M = 0.4); TT = 50 ns; BV has no temperature \
             coefficient; not a model of any specific part",
        ),
        entry(
            zener("DZ_5V1", 5.095, 1.0, 2.7, 100e-12),
            ZENER,
            "Generic 5.1 V zener diode, 5 mA test current",
            "xsim generic model (D13): parameters chosen for VZ = 5.1 V at IZT = 5 mA and \
             ≈ 15 Ω dynamic resistance (IBV = IZT, BV = VZ − IZT·RS = 5.095 V, \
             rz = NBV·Vt/IZT + RS with NBV = 2.7 and RS = 1 Ω); forward IS = 10 fA, N = 1 \
             (VF ≈ 0.72 V at 10 mA, 0.87 V at 100 mA); CJ0 = 100 pF (VJ = 0.8 V, M = 0.4); \
             TT = 50 ns; BV has no temperature coefficient; not a model of any specific part",
        ),
        entry(
            zener("DZ_12V", 11.995, 1.0, 2.7, 40e-12),
            ZENER,
            "Generic 12 V zener diode, 5 mA test current",
            "xsim generic model (D13): parameters chosen for VZ = 12 V at IZT = 5 mA with the \
             sharp avalanche knee (≈ 0.2 nA at 0.9·VZ) and ≈ 15 Ω dynamic resistance of a \
             higher-voltage zener (IBV = IZT, BV = VZ − IZT·RS = 11.995 V, \
             rz = NBV·Vt/IZT + RS with NBV = 2.7 and RS = 1 Ω); forward IS = 10 fA, N = 1 \
             (VF ≈ 0.72 V at 10 mA, 0.87 V at 100 mA); CJ0 = 40 pF (VJ = 0.8 V, M = 0.4); \
             TT = 50 ns; BV has no temperature coefficient; not a model of any specific part",
        ),
        entry(
            DiodeModel {
                name: "LED_RED".into(),
                is: 8e-19,
                n: 2.0,
                rs: 2.4,
                cjo: 20e-12,
                vj: 1.6,
                m: 0.4,
                tt: 20e-9,
                bv: 5.0,
                ibv: 10e-6,
                eg: 2.44,
                ..DiodeModel::default()
            },
            LED,
            "Generic red LED, about 1.9 V at 10 mA",
            "xsim generic model (D13): parameters chosen for VF ≈ 1.80 V at 1 mA and 2.00 V at \
             20 mA with the recombination-dominated slope N = 2 of a heterojunction LED (IS and \
             RS solved from the two points of V = N·Vt·ln(I/IS) + I·RS and rounded to 8e-19 A \
             and 2.4 Ω, which gives ≈ 1.94 V at 10 mA); dVF/dT ≈ −2.0 mV/K at 10 mA, the \
             usual value for red LEDs: at constant current dVF/dT = (VJ − EG − XTI·Vt)/T with \
             the junction voltage VJ = 1.917 V, so with XTI = 3 the activation energy is \
             EG = 2.44 eV (an effective value; the photon energy of red light, 1.9 eV, would \
             give only −0.2 mV/K); reverse BV = 5 V at IBV = 10 µA; CJ0 = 20 pF (VJ = 1.6 V, \
             M = 0.4); TT = 20 ns; not a model of any specific part",
        ),
        entry(
            DiodeModel {
                name: "LED_GREEN".into(),
                is: 5.6e-19,
                n: 2.2,
                rs: 1.6,
                cjo: 15e-12,
                vj: 2.0,
                m: 0.4,
                tt: 20e-9,
                bv: 5.0,
                ibv: 10e-6,
                eg: 2.8,
                ..DiodeModel::default()
            },
            LED,
            "Generic green LED, about 2.1 V at 10 mA",
            "xsim generic model (D13): parameters chosen for VF ≈ 2.00 V at 1 mA and 2.20 V at \
             20 mA with N = 2.2 (IS and RS solved from the two points of \
             V = N·Vt·ln(I/IS) + I·RS and rounded to 5.6e-19 A and 1.6 Ω, which gives ≈ 2.15 V \
             at 10 mA); dVF/dT ≈ −2.5 mV/K at 10 mA, within the usual −1.5 to −3 mV/K of green \
             LEDs: dVF/dT = (VJ − EG − XTI·Vt)/T with VJ = 2.129 V and XTI = 3 gives the \
             effective activation energy EG = 2.8 eV (the photon energy of green light, \
             2.3 eV, would give −0.8 mV/K); reverse BV = 5 V at IBV = 10 µA; CJ0 = 15 pF \
             (VJ = 2.0 V, M = 0.4); TT = 20 ns; not a model of any specific part",
        ),
    ]
}

/// The parameters every bundled BJT shares: emission coefficients of 1, the
/// SPICE B-E leakage slope `NE = 1.5`, a constant base resistance and
/// `XTB = 1.5` (β rising about 0.5 %/K).
fn gummel_poon(
    name: &str,
    polarity: Polarity,
    dc: [f64; 4],
    early_and_reverse: [f64; 2],
    resistances: [f64; 3],
    capacitances: [f64; 2],
    transit: [f64; 2],
) -> BjtModel {
    let [is, bf, ise, ikf] = dc;
    let [vaf, br] = early_and_reverse;
    let [rb, re, rc] = resistances;
    let [cje, cjc] = capacitances;
    let [tf, tr] = transit;
    BjtModel {
        name: name.into(),
        polarity,
        is,
        bf,
        nf: 1.0,
        vaf,
        ikf,
        ise,
        ne: 1.5,
        br,
        nr: 1.0,
        rb,
        rbm: rb,
        re,
        rc,
        cje,
        vje: 0.75,
        mje: 0.33,
        cjc,
        vjc: 0.75,
        mjc: 0.33,
        tf,
        tr,
        xtb: 1.5,
        ..BjtModel::default()
    }
}

/// All built-in bipolar transistor models (NPN and PNP).
pub fn bjts() -> Vec<ModelEntry<BjtModel>> {
    use Polarity::{N, P};
    vec![
        entry(
            gummel_poon(
                "NPN_GP",
                N,
                [1.2e-14, 210.0, 2.5e-14, 0.082],
                [100.0, 5.0],
                [40.0, 0.5, 2.0],
                [10e-12, 4e-12],
                [580e-12, 50e-9],
            ),
            NPN_SMALL,
            "Generic general-purpose small-signal NPN, beta 200, 250 MHz",
            "xsim generic model (D13): parameters chosen for VBE ≈ 0.65 V at IC = 1 mA; \
             β ≈ 200 at 2 mA, ≈ 150 at 10 µA and ≈ 100 at 100 mA (VCE = 5 V); VAF = 100 V; \
             fT ≈ 250 MHz at 10 mA; CJE/CJC = 10/4 pF (VJ = 0.75 V, M = 0.33). IS, BF, ISE \
             (NE = 1.5) and IKF are solved together from the four Gummel–Poon DC targets and \
             rounded; TF from fT = gm/(2π(CD + CJE + CJC)) at 10 mA, CD being the diffusion \
             capacitance d(TF·IF/qb)/dVBE; BR = 5, RB = RBM = 40 Ω, RE = 0.5 Ω, RC = 2 Ω \
             (VCE(sat) ≈ 60 mV at 10 mA/1 mA); TR = 50 ns; not a model of any specific part",
        ),
        entry(
            gummel_poon(
                "NPN_LOWNOISE",
                N,
                [2.6e-14, 460.0, 1.1e-14, 0.052],
                [80.0, 8.0],
                [15.0, 0.5, 3.0],
                [12e-12, 5e-12],
                [730e-12, 50e-9],
            ),
            NPN_SMALL,
            "Generic low-noise high-gain NPN, beta 450, low base resistance",
            "xsim generic model (D13): parameters chosen for VBE ≈ 0.63 V at IC = 1 mA; \
             β ≈ 450 at 1 mA, ≈ 380 at 10 µA (low B-E leakage keeps β flat at low current) and \
             ≈ 250 at 50 mA (VCE = 5 V); VAF = 80 V; fT ≈ 200 MHz at 10 mA; CJE/CJC = 12/5 pF. \
             IS, BF, ISE (NE = 1.5) and IKF are solved together from the four Gummel–Poon DC \
             targets and rounded; TF from fT = gm/(2π(CD + CJE + CJC)) at 10 mA; RB = RBM = \
             15 Ω, the low base spreading resistance that sets thermal noise; BR = 8, \
             RE = 0.5 Ω, RC = 3 Ω (VCE(sat) ≈ 75 mV at 10 mA/0.5 mA); TR = 50 ns; not a model \
             of any specific part",
        ),
        entry(
            gummel_poon(
                "NPN_SWITCH",
                N,
                [7.9e-15, 120.0, 1.2e-13, 0.14],
                [60.0, 2.0],
                [20.0, 0.3, 1.0],
                [5e-12, 2.5e-12],
                [290e-12, 8e-9],
            ),
            NPN_SMALL,
            "Generic fast switching NPN, 500 MHz, short storage time",
            "xsim generic model (D13): parameters chosen for VBE ≈ 0.66 V at IC = 1 mA; \
             β ≈ 100 at 10 mA, ≈ 70 at 100 µA and ≈ 50 at 200 mA (VCE = 5 V); VAF = 60 V; \
             fT ≈ 500 MHz at 10 mA; small CJE/CJC = 5/2.5 pF. IS, BF, ISE (NE = 1.5) and IKF \
             are solved together from the four Gummel–Poon DC targets and rounded; TF from \
             fT = gm/(2π(CD + CJE + CJC)) at 10 mA; BR = 2 and TR = 8 ns for a short \
             saturation storage time; RB = RBM = 20 Ω, RE = 0.3 Ω, RC = 1 Ω (VCE(sat) \
             ≈ 0.2 V at 100 mA/10 mA); not a model of any specific part",
        ),
        entry(
            BjtModel {
                vjc: 0.6,
                mjc: 0.5,
                ..gummel_poon(
                    "NPN_HV",
                    N,
                    [7.6e-14, 120.0, 9.8e-13, 0.057],
                    [500.0, 3.0],
                    [30.0, 1.0, 15.0],
                    [30e-12, 3e-12],
                    [2.5e-9, 300e-9],
                )
            },
            NPN_SMALL,
            "Generic high-voltage small-signal NPN, low feedback capacitance",
            "xsim generic model (D13): a 300 V VCEO-class device (breakdown is not modelled) \
             with parameters chosen for VBE ≈ 0.68 V at IC = 10 mA; β ≈ 80 at 10 mA, ≈ 50 at \
             100 µA and ≈ 40 at 100 mA (VCE = 5 V); the large VAF = 500 V and low CJC = 3 pF \
             (VJC = 0.6 V, MJC = 0.5: ≈ 0.7 pF at VCB = 10 V) of a thick, lightly doped \
             collector, which also gives RC = 15 Ω; fT ≈ 60 MHz at 10 mA; CJE = 30 pF. IS, \
             BF, ISE (NE = 1.5) and IKF are solved together from the four Gummel–Poon DC \
             targets and rounded; TF from fT = gm/(2π(CD + CJE + CJC)) at 10 mA; BR = 3, \
             RB = RBM = 30 Ω, RE = 1 Ω (VCE(sat) ≈ 0.37 V at 20 mA/2 mA); TR = 300 ns; not a \
             model of any specific part",
        ),
        entry(
            gummel_poon(
                "NPN_1A",
                N,
                [3.5e-13, 340.0, 6.2e-12, 0.4],
                [100.0, 10.0],
                [5.0, 0.1, 0.2],
                [60e-12, 20e-12],
                [1e-9, 80e-9],
            ),
            NPN_POWER,
            "Generic 1 A medium-power NPN, beta 150",
            "xsim generic model (D13): parameters chosen for VBE ≈ 0.70 V at IC = 100 mA; \
             β ≈ 150 at 150 mA, ≈ 100 at 5 mA and ≈ 80 at 1 A (VCE = 5 V); VAF = 100 V; \
             fT ≈ 150 MHz at 50 mA; CJE/CJC = 60/20 pF. IS, BF, ISE (NE = 1.5) and IKF are \
             solved together from the four Gummel–Poon DC targets and rounded; TF from \
             fT = gm/(2π(CD + CJE + CJC)) at 50 mA; BR = 10, RB = RBM = 5 Ω, RE = 0.1 Ω, \
             RC = 0.2 Ω (VCE(sat) ≈ 0.36 V at 1 A/100 mA); TR = 80 ns; not a model of any \
             specific part",
        ),
        entry(
            gummel_poon(
                "NPN_POWER",
                N,
                [5.8e-13, 270.0, 4.2e-11, 1.4],
                [100.0, 5.0],
                [2.0, 0.03, 0.05],
                [600e-12, 250e-12],
                [53e-9, 500e-9],
            ),
            NPN_POWER,
            "Generic multi-amp power NPN, beta 70 at 1 A, 3 MHz",
            "xsim generic model (D13): parameters chosen for VBE ≈ 0.80 V at IC = 1 A; β ≈ 70 \
             at 1 A, ≈ 50 at 50 mA and ≈ 40 at 5 A (VCE = 5 V); VAF = 100 V; fT ≈ 3 MHz at \
             500 mA; CJE/CJC = 600/250 pF. IS, BF, ISE (NE = 1.5) and IKF are solved together \
             from the four Gummel–Poon DC targets and rounded, IKF = 1.4 A setting the \
             high-current roll-off; TF from fT = gm/(2π(CD + CJE + CJC)) at 500 mA; BR = 5, \
             RB = RBM = 2 Ω, RE = 30 mΩ, RC = 50 mΩ (VCE(sat) ≈ 0.5 V at 5 A/500 mA); \
             TR = 500 ns; not a model of any specific part",
        ),
        entry(
            gummel_poon(
                "PNP_GP",
                P,
                [1.2e-14, 190.0, 3.2e-14, 0.08],
                [80.0, 4.0],
                [40.0, 0.5, 2.0],
                [12e-12, 6e-12],
                [730e-12, 60e-9],
            ),
            PNP_SMALL,
            "Generic general-purpose small-signal PNP, beta 180, 200 MHz",
            "xsim generic model (D13): parameters chosen for VEB ≈ 0.65 V at IC = −1 mA; \
             β ≈ 180 at 2 mA, ≈ 130 at 10 µA and ≈ 90 at 100 mA (VEC = 5 V); VAF = 80 V; \
             fT ≈ 200 MHz at 10 mA; CJE/CJC = 12/6 pF (lower hole mobility: lower fT, larger \
             junctions than the NPN of the same class). IS, BF, ISE (NE = 1.5) and IKF are \
             solved together from the four Gummel–Poon DC targets and rounded; TF from \
             fT = gm/(2π(CD + CJE + CJC)) at 10 mA; BR = 4, RB = RBM = 40 Ω, RE = 0.5 Ω, \
             RC = 2 Ω (VEC(sat) ≈ 65 mV at 10 mA/1 mA); TR = 60 ns; not a model of any \
             specific part",
        ),
        entry(
            gummel_poon(
                "PNP_LOWNOISE",
                P,
                [2.6e-14, 410.0, 1.5e-14, 0.05],
                [60.0, 6.0],
                [20.0, 0.5, 3.0],
                [12e-12, 7e-12],
                [1e-9, 60e-9],
            ),
            PNP_SMALL,
            "Generic low-noise high-gain PNP, beta 400, low base resistance",
            "xsim generic model (D13): parameters chosen for VEB ≈ 0.63 V at IC = −1 mA; \
             β ≈ 400 at 1 mA, ≈ 330 at 10 µA and ≈ 220 at 50 mA (VEC = 5 V); VAF = 60 V; \
             fT ≈ 150 MHz at 10 mA; CJE/CJC = 12/7 pF. IS, BF, ISE (NE = 1.5) and IKF are \
             solved together from the four Gummel–Poon DC targets and rounded; TF from \
             fT = gm/(2π(CD + CJE + CJC)) at 10 mA; RB = RBM = 20 Ω for low thermal noise; \
             BR = 6, RE = 0.5 Ω, RC = 3 Ω (VEC(sat) ≈ 80 mV at 10 mA/0.5 mA); TR = 60 ns; not \
             a model of any specific part",
        ),
        entry(
            gummel_poon(
                "PNP_SWITCH",
                P,
                [7.8e-15, 110.0, 1.5e-13, 0.13],
                [50.0, 2.0],
                [20.0, 0.3, 1.0],
                [6e-12, 3e-12],
                [370e-12, 12e-9],
            ),
            PNP_SMALL,
            "Generic fast switching PNP, 400 MHz, short storage time",
            "xsim generic model (D13): parameters chosen for VEB ≈ 0.66 V at IC = −1 mA; \
             β ≈ 90 at 10 mA, ≈ 60 at 100 µA and ≈ 45 at 200 mA (VEC = 5 V); VAF = 50 V; \
             fT ≈ 400 MHz at 10 mA; small CJE/CJC = 6/3 pF. IS, BF, ISE (NE = 1.5) and IKF \
             are solved together from the four Gummel–Poon DC targets and rounded; TF from \
             fT = gm/(2π(CD + CJE + CJC)) at 10 mA; BR = 2 and TR = 12 ns for a short \
             saturation storage time; RB = RBM = 20 Ω, RE = 0.3 Ω, RC = 1 Ω (VEC(sat) \
             ≈ 0.2 V at 100 mA/10 mA); not a model of any specific part",
        ),
        entry(
            gummel_poon(
                "PNP_1A",
                P,
                [3.5e-13, 260.0, 6.4e-12, 0.44],
                [80.0, 8.0],
                [5.0, 0.1, 0.25],
                [70e-12, 30e-12],
                [1.25e-9, 120e-9],
            ),
            PNP_POWER,
            "Generic 1 A medium-power PNP, beta 130",
            "xsim generic model (D13): parameters chosen for VEB ≈ 0.70 V at IC = −100 mA; \
             β ≈ 130 at 150 mA, ≈ 90 at 5 mA and ≈ 70 at 1 A (VEC = 5 V); VAF = 80 V; \
             fT ≈ 120 MHz at 50 mA; CJE/CJC = 70/30 pF. IS, BF, ISE (NE = 1.5) and IKF are \
             solved together from the four Gummel–Poon DC targets and rounded; TF from \
             fT = gm/(2π(CD + CJE + CJC)) at 50 mA; BR = 8, RB = RBM = 5 Ω, RE = 0.1 Ω, \
             RC = 0.25 Ω (VEC(sat) ≈ 0.4 V at 1 A/100 mA); TR = 120 ns; not a model of any \
             specific part",
        ),
        entry(
            gummel_poon(
                "PNP_POWER",
                P,
                [6e-13, 150.0, 4.2e-11, 2.1],
                [80.0, 4.0],
                [2.0, 0.03, 0.06],
                [700e-12, 350e-12],
                [53e-9, 600e-9],
            ),
            PNP_POWER,
            "Generic multi-amp power PNP, beta 60 at 1 A, 3 MHz",
            "xsim generic model (D13): parameters chosen for VEB ≈ 0.80 V at IC = −1 A; β ≈ 60 \
             at 1 A, ≈ 45 at 50 mA and ≈ 35 at 5 A (VEC = 5 V); VAF = 80 V; fT ≈ 3 MHz at \
             500 mA; CJE/CJC = 700/350 pF. IS, BF, ISE (NE = 1.5) and IKF are solved together \
             from the four Gummel–Poon DC targets and rounded, IKF = 2.1 A setting the \
             high-current roll-off; TF from fT = gm/(2π(CD + CJE + CJC)) at 500 mA; BR = 4, \
             RB = RBM = 2 Ω, RE = 30 mΩ, RC = 60 mΩ (VEC(sat) ≈ 0.55 V at 5 A/500 mA); \
             TR = 600 ns; not a model of any specific part",
        ),
    ]
}

/// A depletion-mode JFET with the gate junction potential `PB = 0.8 V` of a
/// silicon p-n gate and the typical silicon drifts `VTOTC = −2 mV/K` and
/// `BETATCE = −0.5 %/K`.
fn shichman_hodges(
    name: &str,
    polarity: Polarity,
    [vto, beta, lambda]: [f64; 3],
    series: f64,
    [cgs, cgd]: [f64; 2],
    is: f64,
) -> JfetModel {
    JfetModel {
        name: name.into(),
        polarity,
        vto,
        beta,
        lambda,
        rd: series,
        rs: series,
        cgs,
        cgd,
        pb: 0.8,
        is,
        vtotc: -2e-3,
        betatce: -0.5,
        ..JfetModel::default()
    }
}

/// All built-in JFET models (N and P channel).
pub fn jfets() -> Vec<ModelEntry<JfetModel>> {
    use Polarity::{N, P};
    vec![
        entry(
            shichman_hodges(
                "NJF_LOW",
                N,
                [-0.8, 1.4e-3, 0.01],
                0.0,
                [3e-12, 1.5e-12],
                1e-14,
            ),
            NJF,
            "Generic N-channel JFET, IDSS 1 mA, VP -0.8 V",
            "xsim generic model (D13): parameters chosen for IDSS ≈ 1 mA at VDS = 10 V, \
             VP ≈ −0.8 V and λ ≈ 0.01/V (VTO = VP, LAMBDA = λ, BETA = IDSS/(VP²·(1 + λ·10 V)) \
             = 1.42 mA/V² rounded to 1.4 mA/V², gfs ≈ 2.5 mS at IDSS); CGS/CGD = 3/1.5 pF; \
             gate IS = 10 fA; PB = 0.8 V; VTOTC = −2 mV/K and BETATCE = −0.5 %/K; not a \
             model of any specific part",
        ),
        entry(
            shichman_hodges(
                "NJF_MID",
                N,
                [-2.0, 1.05e-3, 0.02],
                0.0,
                [4e-12, 1.5e-12],
                1e-14,
            ),
            NJF,
            "Generic N-channel JFET, IDSS 5 mA, VP -2 V",
            "xsim generic model (D13): parameters chosen for IDSS ≈ 5 mA at VDS = 10 V, \
             VP ≈ −2 V and λ ≈ 0.02/V (VTO = VP, LAMBDA = λ, BETA = IDSS/(VP²·(1 + λ·10 V)) \
             = 1.042 mA/V² rounded up to 1.05 mA/V², gfs ≈ 5 mS at IDSS); CGS/CGD = 4/1.5 pF; \
             gate IS = 10 fA; \
             PB = 0.8 V; VTOTC = −2 mV/K and BETATCE = −0.5 %/K; not a model of any specific \
             part",
        ),
        entry(
            shichman_hodges(
                "NJF_HIGH",
                N,
                [-3.0, 1.9e-3, 0.025],
                5.0,
                [6e-12, 2.5e-12],
                5e-14,
            ),
            NJF,
            "Generic N-channel JFET, IDSS 20 mA, VP -3 V",
            "xsim generic model (D13): parameters chosen for IDSS ≈ 20 mA at VDS = 10 V, \
             VP ≈ −3 V and λ ≈ 0.025/V with RD = RS = 5 Ω (VTO = VP, LAMBDA = λ, BETA solved \
             from IDSS = BETA·(VTO + IDSS·RS)²·(1 + λ·(10 V − 2·IDSS·RS)) = 1.91 mA/V² rounded \
             to 1.9 mA/V², gfs ≈ 13 mS at IDSS); CGS/CGD = 6/2.5 pF; gate IS = 50 fA; \
             PB = 0.8 V; VTOTC = −2 mV/K and BETATCE = −0.5 %/K; not a model of any specific \
             part",
        ),
        entry(
            shichman_hodges(
                "PJF_MID",
                P,
                [-2.0, 1.05e-3, 0.02],
                0.0,
                [6e-12, 2.5e-12],
                2e-14,
            ),
            PJF,
            "Generic P-channel JFET, IDSS 5 mA, VP 2 V",
            "xsim generic model (D13): parameters chosen for IDSS ≈ 5 mA at VDS = −10 V, \
             VP ≈ 2 V and λ ≈ 0.02/V (VTO = −VP in the N-type orientation of the model, \
             LAMBDA = λ, BETA = IDSS/(VP²·(1 + λ·10 V)) = 1.042 mA/V² rounded up to \
             1.05 mA/V², gfs ≈ 5 mS at IDSS); CGS/CGD = 6/2.5 pF, larger than the N-channel \
             device of the same IDSS because holes are less mobile; gate IS = 20 fA; \
             PB = 0.8 V; VTOTC = −2 mV/K and BETATCE = −0.5 %/K; not a model of any specific \
             part",
        ),
        entry(
            shichman_hodges(
                "PJF_HIGH",
                P,
                [-3.0, 1.45e-3, 0.025],
                8.0,
                [10e-12, 4e-12],
                1e-13,
            ),
            PJF,
            "Generic P-channel JFET, IDSS 15 mA, VP 3 V",
            "xsim generic model (D13): parameters chosen for IDSS ≈ 15 mA at VDS = −10 V, \
             VP ≈ 3 V and λ ≈ 0.025/V with RD = RS = 8 Ω (VTO = −VP in the N-type orientation \
             of the model, LAMBDA = λ, BETA solved from IDSS = BETA·(VTO + IDSS·RS)²·(1 + \
             λ·(10 V − 2·IDSS·RS)) = 1.45 mA/V², gfs ≈ 9.5 mS at IDSS); CGS/CGD = 10/4 pF; \
             gate IS = 100 fA; \
             PB = 0.8 V; VTOTC = −2 mV/K and BETATCE = −0.5 %/K; not a model of any specific \
             part",
        ),
    ]
}

/// Diode model by exact name.
pub fn diode(name: &str) -> Option<DiodeModel> {
    diodes()
        .into_iter()
        .find(|e| e.model.name == name)
        .map(|e| e.model)
}

/// BJT model by exact name.
pub fn bjt(name: &str) -> Option<BjtModel> {
    bjts()
        .into_iter()
        .find(|e| e.model.name == name)
        .map(|e| e.model)
}

/// JFET model by exact name.
pub fn jfet(name: &str) -> Option<JfetModel> {
    jfets()
        .into_iter()
        .find(|e| e.model.name == name)
        .map(|e| e.model)
}

/// Name of the model a newly placed part uses by default.
pub fn default_diode() -> &'static str {
    "D_SIGNAL"
}

/// Default BJT model for each polarity.
pub fn default_bjt(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::N => "NPN_GP",
        Polarity::P => "PNP_GP",
    }
}

/// Default JFET model for each polarity.
pub fn default_jfet(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::N => "NJF_MID",
        Polarity::P => "PJF_MID",
    }
}
