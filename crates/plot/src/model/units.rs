//! Which y axis each trace of a pane is drawn against (D9).
//!
//! A linear or logarithmic pane has up to two y axes. The left axis carries
//! the unit of the first trace (in pane order) whose unit is known, and every
//! trace with that unit; traces whose unit is unknown (`""`) share it. The
//! second distinct unit gets its own, independently scaled axis on the right
//! (LTspice style). A trace with a third unit does not fit:
//! [`super::setup::add_trace`] puts it in another pane.
//!
//! Every trace of the pane counts, visible or not and whether or not the
//! result has its signals, so hiding a trace never moves another one to the
//! other axis. Units depend only on the expression text
//! ([`xsim_waveform::Expression::unit`]).
//!
//! Decibel and phase panes convert every trace to one unit (dB, °): they
//! have a single axis and accept any trace.

use xsim_project::{PlotPane, YScale};
use xsim_waveform::Expression;

/// One of a pane's y axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YAxis {
    Left,
    Right,
}

/// The unit of trace expression `text` (`"V"`, `"A"`, `"W"`, …), or `""`
/// when it is unknown or the expression does not parse.
pub fn expression_unit(text: &str) -> &'static str {
    Expression::parse(text).map_or("", |expression| expression.unit())
}

/// The unit every trace of a pane with y scale `scale` is shown in, when
/// the scale fixes one (decibel and phase panes).
pub fn fixed_unit(scale: YScale) -> Option<&'static str> {
    match scale {
        YScale::Decibel => Some("dB"),
        YScale::Phase => Some("°"),
        YScale::Linear | YScale::Log => None,
    }
}

/// The units of a pane's y axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneUnits {
    /// Unit of the left axis: that of the first trace with a known unit, or
    /// `""` while there is none.
    pub left: &'static str,
    /// The second distinct unit, shown on the right axis.
    pub right: Option<&'static str>,
    /// A decibel or phase pane: one axis that every trace fits.
    pub single: bool,
}

impl PaneUnits {
    /// The axes of `pane`, whose trace expressions have the units `unit`
    /// gives ([`expression_unit`], or a cache of it).
    pub fn of(pane: &PlotPane, unit: impl Fn(&str) -> &'static str) -> Self {
        if let Some(fixed) = fixed_unit(pane.y_scale) {
            return Self {
                left: fixed,
                right: None,
                single: true,
            };
        }
        let mut units = Self {
            left: "",
            right: None,
            single: false,
        };
        for trace in &pane.traces {
            units.claim(unit(&trace.expression));
        }
        units
    }

    /// The axis a trace with unit `unit` uses in this pane, or would use if
    /// it were added; `None` when the pane already shows two other units.
    pub fn axis(&self, unit: &str) -> Option<YAxis> {
        if self.single || unit.is_empty() || self.left.is_empty() || unit == self.left {
            Some(YAxis::Left)
        } else if self.right.is_none_or(|right| right == unit) {
            Some(YAxis::Right)
        } else {
            None
        }
    }

    /// Whether a trace with unit `unit` fits this pane.
    pub fn accepts(&self, unit: &str) -> bool {
        self.axis(unit).is_some()
    }

    /// Whether one of the pane's axes already shows the known unit `unit`.
    pub fn carries(&self, unit: &str) -> bool {
        !unit.is_empty() && (self.left == unit || self.right == Some(unit))
    }

    /// Records a trace with unit `unit`, giving it a free axis if its unit
    /// is new.
    fn claim(&mut self, unit: &'static str) {
        match self.axis(unit) {
            Some(YAxis::Left) if self.left.is_empty() => self.left = unit,
            Some(YAxis::Right) if self.right.is_none() => self.right = Some(unit),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_project::Trace;

    fn pane(scale: YScale, expressions: &[&str]) -> PlotPane {
        PlotPane {
            traces: expressions.iter().map(|e| Trace::new(*e)).collect(),
            y_scale: scale,
            ..PlotPane::default()
        }
    }

    fn units(expressions: &[&str]) -> PaneUnits {
        PaneUnits::of(&pane(YScale::Linear, expressions), expression_unit)
    }

    #[test]
    fn expression_units_come_from_the_parser() {
        assert_eq!(expression_unit("V(out)"), "V");
        assert_eq!(expression_unit("I(R1)"), "A");
        assert_eq!(expression_unit("V(out)*I(R1)"), "W");
        assert_eq!(expression_unit("V(out"), "", "unparsable");
    }

    #[test]
    fn the_first_unit_is_left_and_the_second_right() {
        let mixed = units(&["V(out)", "I(R1)", "V(in)"]);
        assert_eq!((mixed.left, mixed.right), ("V", Some("A")));
        assert_eq!(mixed.axis("V"), Some(YAxis::Left));
        assert_eq!(mixed.axis("A"), Some(YAxis::Right));
        assert_eq!(mixed.axis("W"), None, "a third unit does not fit");
        assert!(!mixed.accepts("W") && mixed.carries("A") && !mixed.carries("W"));

        let currents_first = units(&["I(R1)", "V(out)"]);
        assert_eq!(
            (currents_first.left, currents_first.right),
            ("A", Some("V"))
        );
    }

    #[test]
    fn unknown_units_share_the_left_axis() {
        // `V(out` does not parse, so its unit is unknown.
        let units = units(&["V(out", "I(R1)", "V(in)"]);
        assert_eq!((units.left, units.right), ("A", Some("V")));
        assert_eq!(units.axis(""), Some(YAxis::Left));
        let empty = self::units(&[]);
        assert_eq!((empty.left, empty.right), ("", None));
        assert_eq!(empty.axis("W"), Some(YAxis::Left));
    }

    #[test]
    fn bode_panes_have_one_axis_for_every_trace() {
        for (scale, unit) in [(YScale::Decibel, "dB"), (YScale::Phase, "°")] {
            let units = PaneUnits::of(&pane(scale, &["V(out)", "I(R1)"]), expression_unit);
            assert_eq!((units.left, units.right, units.single), (unit, None, true));
            assert_eq!(units.axis("W"), Some(YAxis::Left));
        }
    }

    #[test]
    fn hidden_traces_keep_their_axis() {
        let mut pane = pane(YScale::Linear, &["V(out)", "I(R1)"]);
        pane.traces[0].visible = false;
        let units = PaneUnits::of(&pane, expression_unit);
        assert_eq!((units.left, units.right), ("V", Some("A")));
    }
}
