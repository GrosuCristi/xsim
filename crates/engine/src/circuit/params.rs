//! Model-parameter checks run by [`super::Circuit::validate`].
//!
//! The device constructors cannot report errors, so they replace values
//! that would make the equations meaningless by the SPICE default. These
//! checks reject exactly those values first, naming the element, the model
//! and the parameter, so a simulation never runs with a silently substituted
//! parameter. Values with a documented meaning stay legal: `+∞` or `0` for
//! the "no effect" corners and Early voltages, `+∞` for no breakdown, zero
//! for absent resistances, capacitances and leakage currents, `rbm > rb`
//! (clamped to `rb`) and `fc ≥ 1` (clamped to 0.95).

use crate::error::CircuitError;
use crate::models::{BjtModel, CurrentSwitchModel, DiodeModel, JfetModel, SwitchModel};

/// Absolute zero in °C.
const ABSOLUTE_ZERO: f64 = -273.15;

/// Checks the parameters of one element's model.
struct Checker<'a> {
    element: &'a str,
    model: &'a str,
}

impl Checker<'_> {
    fn ensure(&self, ok: bool, param: &str, rule: &str, v: f64) -> Result<(), CircuitError> {
        if ok {
            Ok(())
        } else {
            Err(CircuitError::new(format!(
                "{}: model {} parameter {param} must be {rule}, got {v}",
                self.element, self.model
            )))
        }
    }

    fn finite(&self, values: &[(&str, f64)]) -> Result<(), CircuitError> {
        for &(param, v) in values {
            self.ensure(v.is_finite(), param, "finite", v)?;
        }
        Ok(())
    }

    fn positive(&self, values: &[(&str, f64)]) -> Result<(), CircuitError> {
        for &(param, v) in values {
            self.ensure(v.is_finite() && v > 0.0, param, "positive and finite", v)?;
        }
        Ok(())
    }

    fn non_negative(&self, values: &[(&str, f64)]) -> Result<(), CircuitError> {
        for &(param, v) in values {
            self.ensure(
                v.is_finite() && v >= 0.0,
                param,
                "finite and not negative",
                v,
            )?;
        }
        Ok(())
    }

    /// Corner currents and Early voltages: `0` and `+∞` both mean "no effect".
    fn corner(&self, values: &[(&str, f64)]) -> Result<(), CircuitError> {
        for &(param, v) in values {
            // `v >= 0.0` is false for NaN.
            self.ensure(v >= 0.0, param, "positive, 0 or infinite (no effect)", v)?;
        }
        Ok(())
    }

    fn tnom(&self, v: f64) -> Result<(), CircuitError> {
        self.ensure(
            v.is_finite() && v > ABSOLUTE_ZERO,
            "TNOM",
            "a finite temperature above absolute zero (°C)",
            v,
        )
    }
}

pub(super) fn diode(element: &str, m: &DiodeModel) -> Result<(), CircuitError> {
    let c = Checker {
        element,
        model: &m.name,
    };
    c.positive(&[
        ("N", m.n),
        ("VJ", m.vj),
        ("IBV", m.ibv),
        ("NBV", m.nbv),
        ("EG", m.eg),
    ])?;
    c.non_negative(&[
        ("IS", m.is),
        ("RS", m.rs),
        ("CJO", m.cjo),
        ("M", m.m),
        ("TT", m.tt),
        ("XTI", m.xti),
        ("FC", m.fc),
    ])?;
    // `v > 0.0` is false for NaN; +∞ means no breakdown.
    c.ensure(
        m.bv > 0.0,
        "BV",
        "positive (or infinite: no breakdown)",
        m.bv,
    )?;
    c.finite(&[("KF", m.kf), ("AF", m.af)])?;
    c.tnom(m.tnom)
}

pub(super) fn bjt(element: &str, m: &BjtModel) -> Result<(), CircuitError> {
    let c = Checker {
        element,
        model: &m.name,
    };
    c.positive(&[
        ("BF", m.bf),
        ("BR", m.br),
        ("NF", m.nf),
        ("NR", m.nr),
        ("NE", m.ne),
        ("NC", m.nc),
        ("VJE", m.vje),
        ("VJC", m.vjc),
        ("VJS", m.vjs),
        ("EG", m.eg),
    ])?;
    c.non_negative(&[
        ("IS", m.is),
        ("ISE", m.ise),
        ("ISC", m.isc),
        ("RB", m.rb),
        ("RBM", m.rbm),
        ("RE", m.re),
        ("RC", m.rc),
        ("CJE", m.cje),
        ("MJE", m.mje),
        ("CJC", m.cjc),
        ("MJC", m.mjc),
        ("CJS", m.cjs),
        ("MJS", m.mjs),
        ("TF", m.tf),
        ("XTF", m.xtf),
        ("ITF", m.itf),
        ("TR", m.tr),
        ("XTB", m.xtb),
        ("XTI", m.xti),
        ("FC", m.fc),
    ])?;
    c.corner(&[
        ("VAF", m.vaf),
        ("VAR", m.var),
        ("IKF", m.ikf),
        ("IKR", m.ikr),
        ("IRB", m.irb),
        ("VTF", m.vtf),
    ])?;
    c.ensure(
        (0.0..=1.0).contains(&m.xcjc),
        "XCJC",
        "a fraction between 0 and 1",
        m.xcjc,
    )?;
    c.finite(&[("PTF", m.ptf), ("KF", m.kf), ("AF", m.af)])?;
    c.tnom(m.tnom)
}

pub(super) fn jfet(element: &str, m: &JfetModel) -> Result<(), CircuitError> {
    let c = Checker {
        element,
        model: &m.name,
    };
    c.positive(&[("BETA", m.beta), ("PB", m.pb)])?;
    c.non_negative(&[
        ("RD", m.rd),
        ("RS", m.rs),
        ("CGS", m.cgs),
        ("CGD", m.cgd),
        ("IS", m.is),
        ("B", m.b),
        ("FC", m.fc),
    ])?;
    c.finite(&[
        ("VTO", m.vto),
        ("LAMBDA", m.lambda),
        ("VTOTC", m.vtotc),
        ("BETATCE", m.betatce),
        ("KF", m.kf),
        ("AF", m.af),
    ])?;
    c.tnom(m.tnom)
}

pub(super) fn switch(element: &str, m: &SwitchModel) -> Result<(), CircuitError> {
    let c = Checker {
        element,
        model: &m.name,
    };
    c.positive(&[("RON", m.ron), ("ROFF", m.roff)])?;
    c.finite(&[("VT", m.vt), ("VH", m.vh)])
}

pub(super) fn current_switch(element: &str, m: &CurrentSwitchModel) -> Result<(), CircuitError> {
    let c = Checker {
        element,
        model: &m.name,
    };
    c.positive(&[("RON", m.ron), ("ROFF", m.roff)])?;
    c.finite(&[("IT", m.it), ("IH", m.ih)])
}
