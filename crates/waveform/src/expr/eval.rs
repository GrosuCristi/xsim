//! Evaluation of an expression tree against a simulation result.
//!
//! Every subexpression evaluates to one sample per x-axis point. A value is
//! complex only when it derives from complex (AC) signal data, so constants,
//! axis variables and the outputs of `db`, `mag`, `abs`, `ph`, `re` and `im`
//! are real, and `db(V(out)) - db(V(in))` stays a real dB trace in an AC
//! result. Real and complex operands combine in complex arithmetic.

use super::ast::{BinaryOp, Function, GROUND_NODE, Node, Probe, Variable, voltage_signal_name};
use super::lexer::syntax;
use super::{ExprError, Series};
use num_complex::Complex64;
use std::ops::{Add, Mul, Sub};
use xsim_spice_engine::{AnalysisKind, SignalData, SimulationResult};

pub(crate) fn evaluate(tree: &Node, result: &SimulationResult) -> Result<Series, ExprError> {
    let evaluator = Evaluator { result };
    Ok(match evaluator.eval(tree)? {
        Samples::Real(values) => Series::Real(values),
        Samples::Complex(values) => Series::Complex(values),
    })
}

/// Samples of an evaluated subexpression, one per x-axis point.
enum Samples {
    Real(Vec<f64>),
    Complex(Vec<Complex64>),
}

impl Samples {
    /// Applies a function that preserves the real/complex kind.
    fn map(self, real: impl Fn(f64) -> f64, complex: impl Fn(Complex64) -> Complex64) -> Self {
        match self {
            Self::Real(values) => Self::Real(values.into_iter().map(real).collect()),
            Self::Complex(values) => Self::Complex(values.into_iter().map(complex).collect()),
        }
    }

    /// Applies a function whose result is always real.
    fn map_to_real(self, real: impl Fn(f64) -> f64, complex: impl Fn(Complex64) -> f64) -> Self {
        Self::Real(match self {
            Self::Real(values) => values.into_iter().map(real).collect(),
            Self::Complex(values) => values.into_iter().map(complex).collect(),
        })
    }

    fn into_complex(self) -> Vec<Complex64> {
        match self {
            Self::Real(values) => values.into_iter().map(Complex64::from).collect(),
            Self::Complex(values) => values,
        }
    }
}

struct Evaluator<'a> {
    result: &'a SimulationResult,
}

impl Evaluator<'_> {
    fn len(&self) -> usize {
        self.result.len()
    }

    fn x(&self) -> &[f64] {
        &self.result.axis.values
    }

    fn eval(&self, node: &Node) -> Result<Samples, ExprError> {
        match node {
            Node::Number(value) => Ok(Samples::Real(vec![*value; self.len()])),
            Node::Probe { probe, position } => self.probe(probe, *position),
            Node::Variable { variable, position } => self.variable(*variable, *position),
            Node::Negate(operand) => Ok(self.eval(operand)?.map(|v| -v, |z| -z)),
            Node::Binary { op, lhs, rhs } => Ok(binary(*op, self.eval(lhs)?, self.eval(rhs)?)),
            Node::Call { function, args } => self.call(*function, args),
        }
    }

    fn probe(&self, probe: &Probe, position: usize) -> Result<Samples, ExprError> {
        match probe {
            Probe::Voltage { node, reference } => {
                let voltage = self.node_voltage(node, position)?;
                match reference {
                    None => Ok(voltage),
                    Some(reference) => {
                        let reference = self.node_voltage(reference, position)?;
                        Ok(binary(BinaryOp::Sub, voltage, reference))
                    }
                }
            }
            Probe::Current { prefix, element } => {
                self.signal(&format!("{prefix}({element})"), position)
            }
        }
    }

    fn node_voltage(&self, node: &str, position: usize) -> Result<Samples, ExprError> {
        let name = voltage_signal_name(node);
        if node == GROUND_NODE && self.result.signal(&name).is_none() {
            return Ok(Samples::Real(vec![0.0; self.len()]));
        }
        self.signal(&name, position)
    }

    fn signal(&self, name: &str, position: usize) -> Result<Samples, ExprError> {
        let signal = self
            .result
            .signal(name)
            .ok_or_else(|| ExprError::UnknownSignal(name.to_string()))?;
        let n = self.len();
        let samples = match &signal.data {
            SignalData::Real(values) if values.len() == n => Some(Samples::Real(values.clone())),
            SignalData::Complex { re, im } if re.len() == n && im.len() == n => {
                Some(Samples::Complex(
                    re.iter()
                        .zip(im)
                        .map(|(&re, &im)| Complex64::new(re, im))
                        .collect(),
                ))
            }
            _ => None,
        };
        samples.ok_or_else(|| {
            syntax(
                position,
                format!(
                    "{name} does not have one sample per {} point",
                    self.result.axis.name
                ),
            )
        })
    }

    fn variable(&self, variable: Variable, position: usize) -> Result<Samples, ExprError> {
        let (required, analysis) = match variable {
            Variable::Time => (AnalysisKind::Transient, "transient"),
            Variable::Freq => (AnalysisKind::Ac, "AC"),
        };
        if self.result.analysis != required {
            return Err(syntax(
                position,
                format!(
                    "'{}' is only available in {analysis} results",
                    variable.name()
                ),
            ));
        }
        Ok(Samples::Real(self.x().to_vec()))
    }

    fn call(&self, function: Function, args: &[Node]) -> Result<Samples, ExprError> {
        use Function::*;
        let nan = f64::NAN;
        let complex_nan = Complex64::new(nan, nan);
        Ok(match (function, args) {
            (Min, [a, b]) => select(self.eval(a)?, self.eval(b)?, |a, b| b < a),
            (Max, [a, b]) => select(self.eval(a)?, self.eval(b)?, |a, b| b > a),
            (Abs | Mag, [arg]) => self.eval(arg)?.map_to_real(f64::abs, |z| z.norm()),
            (Sqrt, [arg]) => self.eval(arg)?.map(f64::sqrt, |z| z.sqrt()),
            (Exp, [arg]) => self.eval(arg)?.map(f64::exp, |z| z.exp()),
            (Ln, [arg]) => self.eval(arg)?.map(f64::ln, |z| z.ln()),
            (Log10, [arg]) => self.eval(arg)?.map(f64::log10, |z| z.log10()),
            (Sin, [arg]) => self.eval(arg)?.map(f64::sin, |z| z.sin()),
            (Cos, [arg]) => self.eval(arg)?.map(f64::cos, |z| z.cos()),
            (Tan, [arg]) => self.eval(arg)?.map(f64::tan, |z| z.tan()),
            (Atan, [arg]) => self.eval(arg)?.map(f64::atan, |z| z.atan()),
            (Db, [arg]) => self
                .eval(arg)?
                .map_to_real(|v| 20.0 * v.abs().log10(), |z| 20.0 * z.norm().log10()),
            (Ph, [arg]) => self.eval(arg)?.map_to_real(
                |v| Complex64::from(v).arg().to_degrees(),
                |z| z.arg().to_degrees(),
            ),
            (Re, [arg]) => self.eval(arg)?.map_to_real(|v| v, |z| z.re),
            (Im, [arg]) => self.eval(arg)?.map_to_real(|_| 0.0, |z| z.im),
            (Deriv, [arg]) => match self.eval(arg)? {
                Samples::Real(y) => Samples::Real(derivative(self.x(), &y, nan)),
                Samples::Complex(y) => Samples::Complex(derivative(self.x(), &y, complex_nan)),
            },
            (Integ, [arg]) => match self.eval(arg)? {
                Samples::Real(y) => Samples::Real(cumulative_integral(self.x(), &y, 0.0)),
                Samples::Complex(y) => {
                    Samples::Complex(cumulative_integral(self.x(), &y, Complex64::from(0.0)))
                }
            },
            // The parser checks arity, so only a hand-built tree gets here.
            _ => {
                return Err(ExprError::Arity {
                    function: function.name().to_string(),
                    expected: function.arity(),
                    found: args.len(),
                });
            }
        })
    }
}

fn binary(op: BinaryOp, lhs: Samples, rhs: Samples) -> Samples {
    match (lhs, rhs) {
        (Samples::Real(a), Samples::Real(b)) => Samples::Real(zip_with(a, b, real_operator(op))),
        (a, b) => Samples::Complex(zip_with(
            a.into_complex(),
            b.into_complex(),
            complex_operator(op),
        )),
    }
}

fn zip_with<T: Copy>(a: Vec<T>, b: Vec<T>, f: fn(T, T) -> T) -> Vec<T> {
    a.into_iter().zip(b).map(|(a, b)| f(a, b)).collect()
}

fn real_operator(op: BinaryOp) -> fn(f64, f64) -> f64 {
    match op {
        BinaryOp::Add => |a, b| a + b,
        BinaryOp::Sub => |a, b| a - b,
        BinaryOp::Mul => |a, b| a * b,
        BinaryOp::Div => |a, b| a / b,
        BinaryOp::Pow => f64::powf,
    }
}

fn complex_operator(op: BinaryOp) -> fn(Complex64, Complex64) -> Complex64 {
    match op {
        BinaryOp::Add => |a, b| a + b,
        BinaryOp::Sub => |a, b| a - b,
        BinaryOp::Mul => |a, b| a * b,
        BinaryOp::Div => |a, b| a / b,
        BinaryOp::Pow => complex_power,
    }
}

/// `base ^ exponent`. Integer exponents use repeated multiplication, which
/// is exact for small powers where `exp(w·ln z)` would round, and a zero
/// base is special-cased because `ln 0` is infinite.
fn complex_power(base: Complex64, exponent: Complex64) -> Complex64 {
    let is_integer = exponent.im == 0.0 && exponent.re.fract() == 0.0;
    if is_integer && exponent.re.abs() <= f64::from(i32::MAX) {
        // The range check makes the conversion exact.
        base.powi(exponent.re as i32)
    } else if base == Complex64::from(0.0) && exponent.re > 0.0 {
        Complex64::from(0.0)
    } else {
        base.powc(exponent)
    }
}

/// Element-wise choice between `a` and `b`; `prefer_b(a, b)` compares real
/// parts (the documented ordering of complex values). Ties keep `a`.
fn select(a: Samples, b: Samples, prefer_b: fn(f64, f64) -> bool) -> Samples {
    match (a, b) {
        (Samples::Real(a), Samples::Real(b)) => Samples::Real(
            a.into_iter()
                .zip(b)
                .map(|(a, b)| if prefer_b(a, b) { b } else { a })
                .collect(),
        ),
        (a, b) => Samples::Complex(
            a.into_complex()
                .into_iter()
                .zip(b.into_complex())
                .map(|(a, b)| if prefer_b(a.re, b.re) { b } else { a })
                .collect(),
        ),
    }
}

/// Derivative of `y` with respect to `x` from the quadratic through each
/// sample and its neighbours: second-order accurate on non-uniform grids
/// (exact for quadratics). Two samples give their common slope; a single
/// sample has no derivative (`undefined`).
///
/// With slopes `s0`, `s1` over steps `h0`, `h1` and curvature
/// `a = (s1 − s0)/(h0 + h1)`, the quadratic's derivative is `s0 − a·h0` at
/// the first point, `s0 + a·h0` at the middle one and `s1 + a·h1` at the
/// last. Working from slopes avoids cancellation between absolute x values.
fn derivative<T>(x: &[f64], y: &[T], undefined: T) -> Vec<T>
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f64, Output = T>,
{
    let n = x.len().min(y.len());
    let slope = |i: usize| (y[i + 1] - y[i]) * (1.0 / (x[i + 1] - x[i]));
    match n {
        0 => Vec::new(),
        1 => vec![undefined],
        2 => vec![slope(0); 2],
        _ => (0..n)
            .map(|i| {
                let centre = i.clamp(1, n - 2);
                let (h0, h1) = (x[centre] - x[centre - 1], x[centre + 1] - x[centre]);
                let (s0, s1) = (slope(centre - 1), slope(centre));
                let curvature = (s1 - s0) * (1.0 / (h0 + h1));
                if i < centre {
                    s0 - curvature * h0
                } else if i == centre {
                    s0 + curvature * h0
                } else {
                    s1 + curvature * h1
                }
            })
            .collect(),
    }
}

/// Running trapezoidal integral of `y` over `x`, starting from `zero`.
fn cumulative_integral<T>(x: &[f64], y: &[T], zero: T) -> Vec<T>
where
    T: Copy + Add<Output = T> + Mul<f64, Output = T>,
{
    let n = x.len().min(y.len());
    let mut total = zero;
    (0..n)
        .map(|i| {
            if i > 0 {
                total = total + (y[i - 1] + y[i]) * (0.5 * (x[i] - x[i - 1]));
            }
            total
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivative_is_exact_for_quadratics_on_uneven_grids() {
        let x = [0.0, 0.1, 0.35, 0.4, 1.0, 1.7];
        let y: Vec<f64> = x.iter().map(|&t| 3.0 * t * t - 2.0 * t + 0.5).collect();
        for (t, d) in x.iter().zip(derivative(&x, &y, f64::NAN)) {
            let expected = 6.0 * t - 2.0;
            assert!((d - expected).abs() < 1e-12, "at {t}: {d} vs {expected}");
        }
    }

    #[test]
    fn derivative_of_short_series() {
        assert!(derivative::<f64>(&[], &[], f64::NAN).is_empty());
        assert!(derivative(&[1.0], &[2.0], f64::NAN)[0].is_nan());
        assert_eq!(derivative(&[1.0, 3.0], &[2.0, 6.0], f64::NAN), [2.0, 2.0]);
    }

    #[test]
    fn complex_derivative_differentiates_both_parts() {
        let x = [1.0, 2.0, 4.0];
        let y: Vec<Complex64> = x.iter().map(|&f| Complex64::new(f * f, -3.0 * f)).collect();
        let d = derivative(&x, &y, Complex64::new(f64::NAN, f64::NAN));
        for (f, d) in x.iter().zip(d) {
            assert!((d - Complex64::new(2.0 * f, -3.0)).norm() < 1e-12);
        }
    }

    #[test]
    fn cumulative_integral_is_exact_for_linear_data() {
        let x = [0.0, 0.5, 2.0, 2.25];
        let y: Vec<f64> = x.iter().map(|&t| 4.0 * t + 1.0).collect();
        for (t, integral) in x.iter().zip(cumulative_integral(&x, &y, 0.0)) {
            assert!((integral - (2.0 * t * t + t)).abs() < 1e-12);
        }
    }

    #[test]
    fn integer_complex_powers_are_exact() {
        let z = Complex64::new(1.0, 1.0);
        assert_eq!(
            complex_power(z, Complex64::from(2.0)),
            Complex64::new(0.0, 2.0)
        );
        assert_eq!(
            complex_power(z, Complex64::from(-2.0)),
            Complex64::new(0.0, -0.5)
        );
        assert_eq!(
            complex_power(Complex64::from(0.0), Complex64::from(0.5)),
            Complex64::from(0.0)
        );
        let root = complex_power(Complex64::from(-4.0), Complex64::from(0.5));
        assert!((root - Complex64::new(0.0, 2.0)).norm() < 1e-12);
    }
}
