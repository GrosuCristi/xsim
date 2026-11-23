//! Unit inference for trace expressions.
//!
//! Units are tracked as exponents of volt, ampere and second, so `V*A` is a
//! watt and `V/A` an ohm. Number literals adopt the unit of whatever they
//! are added to (`V(out) + 1` is in volts). `db` and `ph` produce the
//! labels dB and °. Anything without a conventional symbol, and `deriv`/`integ`
//! (whose x axis is unknown until evaluation), infer as `""`.

use super::ast::{BinaryOp, Function, Node, Probe, Variable};

/// The unit symbol of `tree`, or `""` when dimensionless or unknown.
pub(crate) fn unit_symbol(tree: &Node) -> &'static str {
    match infer(tree) {
        Unit::Number | Unit::Unknown => "",
        Unit::Decibel => "dB",
        Unit::Degree => "°",
        Unit::Dimension(dimension) => NAMED_DIMENSIONS
            .iter()
            .find(|(named, _)| *named == dimension)
            .map_or("", |(_, symbol)| symbol),
    }
}

/// Exponents of volt, ampere and second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dimension {
    volt: i32,
    ampere: i32,
    second: i32,
}

impl Dimension {
    const fn new(volt: i32, ampere: i32, second: i32) -> Self {
        Self {
            volt,
            ampere,
            second,
        }
    }

    fn combine(self, other: Self, op: fn(i32, i32) -> Option<i32>) -> Option<Self> {
        Some(Self::new(
            op(self.volt, other.volt)?,
            op(self.ampere, other.ampere)?,
            op(self.second, other.second)?,
        ))
    }

    fn scale(self, factor: i32) -> Option<Self> {
        self.combine(Self::new(factor, factor, factor), i32::checked_mul)
    }
}

const DIMENSIONLESS: Dimension = Dimension::new(0, 0, 0);
const VOLT: Dimension = Dimension::new(1, 0, 0);
const AMPERE: Dimension = Dimension::new(0, 1, 0);
const SECOND: Dimension = Dimension::new(0, 0, 1);
const HERTZ: Dimension = Dimension::new(0, 0, -1);

const NAMED_DIMENSIONS: [(Dimension, &str); 8] = [
    (DIMENSIONLESS, ""),
    (VOLT, "V"),
    (AMPERE, "A"),
    (Dimension::new(1, 1, 0), "W"),
    (Dimension::new(1, -1, 0), "Ω"),
    (Dimension::new(-1, 1, 0), "S"),
    (SECOND, "s"),
    (HERTZ, "Hz"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// A pure number, which takes on the unit it is combined with.
    Number,
    Dimension(Dimension),
    Decibel,
    Degree,
    Unknown,
}

impl From<Option<Dimension>> for Unit {
    fn from(dimension: Option<Dimension>) -> Self {
        dimension.map_or(Self::Unknown, Self::Dimension)
    }
}

fn infer(node: &Node) -> Unit {
    match node {
        Node::Number(_) => Unit::Number,
        Node::Probe { probe, .. } => Unit::Dimension(match probe {
            Probe::Voltage { .. } => VOLT,
            Probe::Current { .. } => AMPERE,
        }),
        Node::Variable { variable, .. } => Unit::Dimension(match variable {
            Variable::Time => SECOND,
            Variable::Freq => HERTZ,
        }),
        Node::Negate(operand) => infer(operand),
        Node::Binary { op, lhs, rhs } => {
            let lhs_unit = infer(lhs);
            match op {
                BinaryOp::Add | BinaryOp::Sub => common(lhs_unit, infer(rhs)),
                BinaryOp::Mul => product(lhs_unit, infer(rhs)),
                BinaryOp::Div => quotient(lhs_unit, infer(rhs)),
                BinaryOp::Pow => power(lhs_unit, rhs),
            }
        }
        Node::Call { function, args } => call(*function, args),
    }
}

fn call(function: Function, args: &[Node]) -> Unit {
    let mut units = args.iter().map(infer);
    let Some(first) = units.next() else {
        return Unit::Unknown;
    };
    match function {
        Function::Min | Function::Max => units.fold(first, common),
        Function::Abs | Function::Mag | Function::Re | Function::Im => first,
        Function::Sqrt => match first {
            Unit::Number => Unit::Number,
            Unit::Dimension(d) if [d.volt, d.ampere, d.second].iter().all(|e| e % 2 == 0) => {
                Unit::Dimension(Dimension::new(d.volt / 2, d.ampere / 2, d.second / 2))
            }
            _ => Unit::Unknown,
        },
        Function::Exp
        | Function::Ln
        | Function::Log10
        | Function::Sin
        | Function::Cos
        | Function::Tan
        | Function::Atan => match first {
            Unit::Number => Unit::Number,
            _ => Unit::Dimension(DIMENSIONLESS),
        },
        Function::Db => Unit::Decibel,
        Function::Ph => Unit::Degree,
        Function::Deriv | Function::Integ => Unit::Unknown,
    }
}

/// Unit of a sum, difference, `min` or `max`.
fn common(a: Unit, b: Unit) -> Unit {
    match (a, b) {
        (Unit::Number, other) | (other, Unit::Number) => other,
        (a, b) if a == b => a,
        _ => Unit::Unknown,
    }
}

fn product(a: Unit, b: Unit) -> Unit {
    match (a, b) {
        (Unit::Number, other) | (other, Unit::Number) => other,
        (Unit::Dimension(a), Unit::Dimension(b)) => a.combine(b, i32::checked_add).into(),
        _ => Unit::Unknown,
    }
}

fn quotient(a: Unit, b: Unit) -> Unit {
    match (a, b) {
        (other, Unit::Number) => other,
        (Unit::Number, Unit::Dimension(b)) => b.scale(-1).into(),
        (Unit::Dimension(a), Unit::Dimension(b)) => a.combine(b, i32::checked_sub).into(),
        _ => Unit::Unknown,
    }
}

fn power(base: Unit, exponent: &Node) -> Unit {
    if let Some(k) = integer_literal(exponent) {
        return match base {
            Unit::Number => Unit::Number,
            Unit::Dimension(d) => d.scale(k).into(),
            _ => Unit::Unknown,
        };
    }
    let is_dimensionless = |unit| matches!(unit, Unit::Number | Unit::Dimension(DIMENSIONLESS));
    match (base, infer(exponent)) {
        (Unit::Number, Unit::Number) => Unit::Number,
        (base, exponent) if is_dimensionless(base) && is_dimensionless(exponent) => {
            Unit::Dimension(DIMENSIONLESS)
        }
        _ => Unit::Unknown,
    }
}

/// The value of an integer literal such as `2` or `-1`.
fn integer_literal(node: &Node) -> Option<i32> {
    match node {
        Node::Number(value) if value.fract() == 0.0 && value.abs() <= f64::from(i32::MAX) => {
            // The range check makes the conversion exact.
            Some(*value as i32)
        }
        Node::Negate(operand) => integer_literal(operand)?.checked_neg(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    fn unit(source: &str) -> &'static str {
        unit_symbol(&parse(source).expect("valid expression"))
    }

    #[test]
    fn probes_and_axis_variables() {
        assert_eq!(unit("V(out)"), "V");
        assert_eq!(unit("V(out, in)"), "V");
        assert_eq!(unit("Ic(Q1)"), "A");
        assert_eq!(unit("time"), "s");
        assert_eq!(unit("freq"), "Hz");
        assert_eq!(unit("2.5"), "");
    }

    #[test]
    fn products_and_quotients_combine_dimensions() {
        assert_eq!(unit("V(out)*I(R1)"), "W");
        assert_eq!(unit("I(R1)*V(out)"), "W");
        assert_eq!(unit("V(out)/I(R1)"), "Ω");
        assert_eq!(unit("I(R1)/V(out)"), "S");
        assert_eq!(unit("V(out)/V(in)"), "");
        assert_eq!(unit("1/time"), "Hz");
        assert_eq!(unit("V(a)*I(R1)/I(R1)"), "V");
        assert_eq!(unit("V(a)*V(b)"), "", "V² has no symbol");
    }

    #[test]
    fn numbers_adopt_the_other_operand_unit() {
        assert_eq!(unit("V(out) + 1"), "V");
        assert_eq!(unit("2 * I(R1)"), "A");
        assert_eq!(unit("-V(out) / 2"), "V");
        assert_eq!(unit("V(out) - V(in)"), "V");
        assert_eq!(unit("V(out) + I(R1)"), "");
        assert_eq!(unit("V(out)/V(in) + V(in)"), "", "a gain is not a voltage");
    }

    #[test]
    fn powers_and_roots() {
        assert_eq!(unit("sqrt(V(a)^2)"), "V");
        assert_eq!(unit("(V(a)*I(R1))^1"), "W");
        assert_eq!(unit("I(R1)^-1*V(a)"), "Ω");
        assert_eq!(unit("V(a)^0.5"), "");
        assert_eq!(unit("2^3"), "");
        assert_eq!(unit("sqrt(V(a))"), "");
    }

    #[test]
    fn functions() {
        assert_eq!(unit("db(V(out)/V(in))"), "dB");
        assert_eq!(unit("db(V(out)) - db(V(in))"), "dB");
        assert_eq!(unit("ph(V(out))"), "°");
        assert_eq!(unit("mag(V(out))"), "V");
        assert_eq!(unit("re(I(R1))"), "A");
        assert_eq!(unit("abs(V(a)*I(R1))"), "W");
        assert_eq!(unit("max(V(a), 0)"), "V");
        assert_eq!(unit("min(V(a), I(R1))"), "");
        assert_eq!(unit("exp(V(a))"), "");
        assert_eq!(unit("deriv(V(a))"), "");
        assert_eq!(unit("integ(I(C1))"), "");
        assert_eq!(unit("db(V(a)) * V(a)"), "");
    }

    #[test]
    fn exponent_overflow_is_unknown_not_a_panic() {
        assert_eq!(unit("V(a)^2147483647*V(a)"), "");
        assert_eq!(unit("(V(a)^65536)^65536"), "");
        assert_eq!(unit("V(a)^-2147483648"), "");
    }
}
