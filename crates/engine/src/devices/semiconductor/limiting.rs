//! Newton step limiting for junction and FET voltages (SPICE3 / ngspice
//! `DEVpnjlim` and `DEVfetlim`).
//!
//! Limiting replaces a proposed junction voltage by one closer to the previous
//! iterate when the exponential would otherwise overshoot by orders of
//! magnitude. The device then linearises at the limited voltage; the Newton
//! loop must keep iterating whenever any voltage was limited.

/// Limits the pn-junction voltage `vnew` given the previous iterate `vold`.
///
/// `vt` is the (emission-scaled) thermal voltage and `vcrit` the critical
/// voltage from [`super::critical_voltage`]. Returns the voltage to evaluate
/// at and whether it differs from `vnew`.
///
/// * Forward bias above `vcrit`: the step is compressed logarithmically.
/// * Reverse bias: a single step may at most double the reverse voltage
///   (plus 1 V), which keeps breakdown regions from being jumped over
///   (ngspice extension of the SPICE3 algorithm).
pub(crate) fn pnjlim(vnew: f64, vold: f64, vt: f64, vcrit: f64) -> (f64, bool) {
    if vnew > vcrit && (vnew - vold).abs() > 2.0 * vt {
        let limited = if vold > 0.0 {
            let arg = 1.0 + (vnew - vold) / vt;
            if arg > 0.0 {
                vold + vt * arg.ln()
            } else {
                vcrit
            }
        } else {
            vt * (vnew / vt).ln()
        };
        (limited, true)
    } else if vnew < 0.0 {
        let floor = if vold > 0.0 {
            -vold - 1.0
        } else {
            2.0 * vold - 1.0
        };
        if vnew < floor {
            (floor, true)
        } else {
            (vnew, false)
        }
    } else {
        (vnew, false)
    }
}

/// Limits a FET control voltage `vnew` around the threshold `vto` given the
/// previous iterate `vold`. Returns the voltage to evaluate at and whether it
/// differs from `vnew`.
///
/// Steps that would cross from deep cut-off into strong conduction (or the
/// reverse) in one iteration are shortened so the quadratic channel law is
/// approached gradually.
pub(crate) fn fetlim(vnew: f64, vold: f64, vto: f64) -> (f64, bool) {
    let vtsthi = (2.0 * (vold - vto)).abs() + 2.0;
    let vtstlo = (vold - vto).abs() + 1.0;
    let vtox = vto + 3.5;
    let delv = vnew - vold;

    let limited = if vold >= vto {
        if vold >= vtox {
            if delv <= 0.0 {
                // Turning off from strong conduction.
                if vnew >= vtox {
                    if -delv > vtstlo { vold - vtstlo } else { vnew }
                } else {
                    vnew.max(vto + 2.0)
                }
            } else if delv >= vtsthi {
                // Staying on.
                vold + vtsthi
            } else {
                vnew
            }
        } else if delv <= 0.0 {
            // Middle region, decreasing.
            vnew.max(vto - 0.5)
        } else {
            // Middle region, increasing.
            vnew.min(vto + 4.0)
        }
    } else if delv <= 0.0 {
        // Off and staying off.
        if -delv > vtsthi { vold - vtsthi } else { vnew }
    } else {
        // Off and turning on.
        let vtemp = vto + 0.5;
        if vnew <= vtemp {
            if delv > vtstlo { vold + vtstlo } else { vnew }
        } else {
            vtemp
        }
    };
    (limited, limited != vnew)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VT: f64 = 0.025_864_7;

    #[test]
    fn pnjlim_leaves_small_steps_alone() {
        assert_eq!(pnjlim(0.62, 0.6, VT, 0.65), (0.62, false));
        assert_eq!(pnjlim(0.7, 0.69, VT, 0.65), (0.7, false));
        assert_eq!(pnjlim(-3.0, -2.0, VT, 0.65), (-3.0, false));
    }

    #[test]
    fn pnjlim_compresses_large_forward_steps() {
        let (v, limited) = pnjlim(5.0, 0.6, VT, 0.65);
        assert!(limited);
        assert_eq!(v, 0.6 + VT * (1.0 + 4.4 / VT).ln());
        // From reverse bias the new voltage is mapped logarithmically.
        let (v, limited) = pnjlim(5.0, -1.0, VT, 0.65);
        assert!(limited);
        assert_eq!(v, VT * (5.0 / VT).ln());
    }

    #[test]
    fn pnjlim_bounds_reverse_steps() {
        assert_eq!(pnjlim(-100.0, 0.0, VT, 0.65), (-1.0, true));
        assert_eq!(pnjlim(-100.0, -1.0, VT, 0.65), (-3.0, true));
        assert_eq!(pnjlim(-100.0, 0.5, VT, 0.65), (-1.5, true));
    }

    #[test]
    fn pnjlim_falls_back_to_vcrit_when_log_argument_is_negative() {
        // A large forward-bias decrease makes 1 + Δv/vt negative.
        assert_eq!(pnjlim(0.7, 0.9, VT, 0.65), (0.65, true));
    }

    #[test]
    fn fetlim_limits_turn_on_from_cutoff() {
        // Off (vold < vto) and jumping far above vto + 0.5.
        assert_eq!(fetlim(3.0, -4.0, -2.0), (-1.5, true));
        // Off, small increase stays.
        assert_eq!(fetlim(-3.5, -4.0, -2.0), (-3.5, false));
        // Off, a large decrease is bounded by vtsthi.
        let vtsthi = (2.0f64 * (-4.0 + 2.0)).abs() + 2.0;
        assert_eq!(fetlim(-20.0, -4.0, -2.0), (-4.0 - vtsthi, true));
    }

    #[test]
    fn fetlim_limits_middle_and_strong_regions() {
        // Middle region: increase capped at vto + 4, decrease at vto − 0.5.
        assert_eq!(fetlim(10.0, -1.0, -2.0), (2.0, true));
        assert_eq!(fetlim(-10.0, -1.0, -2.0), (-2.5, true));
        // Strongly on, turning off below vtox is held at vto + 2.
        assert_eq!(fetlim(-5.0, 2.0, -2.0), (0.0, true));
        // Strongly on, increasing by more than vtsthi.
        let vtsthi = (2.0f64 * (2.0 + 2.0)).abs() + 2.0;
        assert_eq!(fetlim(50.0, 2.0, -2.0), (2.0 + vtsthi, true));
        assert_eq!(fetlim(3.0, 2.0, -2.0), (3.0, false));
    }
}
