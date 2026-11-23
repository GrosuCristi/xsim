//! Trace expressions.
//!
//! ## Grammar
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/') unary)*
//! unary   := '-' unary | power
//! power   := atom ('^' unary)?
//! atom    := number | signal | variable | call | '(' expr ')'
//! number  := decimal with optional exponent and SI suffix (case-sensitive, see
//!            `xsim_spice_engine::units`), e.g. 2, 4.7k, 1e-3, 10m
//! signal  := V(node) | V(node, node) | I(name) | Ic(name) | Ib(name) | Ie(name)
//!          | Id(name) | Ig(name) | Is(name)
//! variable:= time | freq       (the result's x axis)
//! call    := func '(' expr (',' expr)* ')'
//! func    := abs | sqrt | exp | ln | log10 | sin | cos | tan | atan
//!          | min | max | db | mag | ph | re | im | deriv | integ
//! ```
//! `V(a, b)` is `V(a) − V(b)`. Node and element names are case-sensitive;
//! function names are lowercase. `db(x)` is `20·log10(|x|)`, `ph(x)` the
//! phase in degrees, `deriv`/`integ` differentiate/integrate with respect to
//! the x axis (trapezoidal). AC results evaluate in complex arithmetic;
//! `min`/`max`/comparisons on complex values use the real part.
//!
//! ## Details
//! * Probe arguments are raw names up to `,` or `)`, so `V(0)`, `V(N001)`
//!   and `V(net-1)` need no quoting. Ground (`0`) is never recorded; it
//!   evaluates as 0 V and is not listed by [`Expression::signals`].
//! * A number may carry one SI suffix and nothing else: `2meg` and `5V` are
//!   syntax errors rather than silently ignored text.
//! * A subexpression is complex only when it derives from complex (AC)
//!   signal data. Constants, axis variables and the results of `db`, `mag`,
//!   `abs`, `ph`, `re` and `im` are real, so `db(V(out)) - db(V(in))` is a
//!   real trace even in an AC result.
//! * `deriv` uses second-order differences (exact for quadratics on
//!   non-uniform grids); `integ` is the running trapezoidal integral from
//!   the first sample.
//! * `time` is only defined for transient results and `freq` for AC results;
//!   using them elsewhere is reported as a [`ExprError::Syntax`] error at the
//!   variable, as is a signal whose sample count does not match the axis.
//! * Nesting is limited to 128 levels so that no input can exhaust the stack.

mod ast;
mod dimension;
mod eval;
mod lexer;
mod parser;

use num_complex::Complex64;
use std::fmt;
use xsim_spice_engine::SimulationResult;

/// Evaluated trace samples, one per x-axis sample of the result.
#[derive(Debug, Clone, PartialEq)]
pub enum Series {
    Real(Vec<f64>),
    Complex(Vec<Complex64>),
}

impl Series {
    pub fn len(&self) -> usize {
        match self {
            Self::Real(v) => v.len(),
            Self::Complex(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// Syntax error at byte `position` of the expression text.
    Syntax {
        position: usize,
        message: String,
    },
    /// The expression names a signal the result does not contain.
    UnknownSignal(String),
    UnknownFunction(String),
    /// Wrong number of arguments for a function.
    Arity {
        function: String,
        expected: usize,
        found: usize,
    },
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { position, message } => {
                write!(f, "{message} at character {}", position + 1)
            }
            Self::UnknownSignal(s) => write!(f, "No signal named {s} in the result"),
            Self::UnknownFunction(s) => write!(f, "Unknown function {s}"),
            Self::Arity {
                function,
                expected,
                found,
            } => write!(f, "{function} takes {expected} argument(s), got {found}"),
        }
    }
}

impl std::error::Error for ExprError {}

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    text: String,
    tree: ast::Node,
}

impl Expression {
    /// Parses `text` per the [module grammar](self). Errors carry the byte
    /// position of the offending token.
    pub fn parse(text: &str) -> Result<Self, ExprError> {
        Ok(Self {
            text: text.to_string(),
            tree: parser::parse(text)?,
        })
    }

    /// The source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Canonical signal names referenced, e.g. `["V(in)", "V(out)"]`, sorted
    /// and deduplicated. `V(a, b)` contributes `V(a)` and `V(b)`.
    pub fn signals(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.tree.collect_signals(&mut names);
        names.sort();
        names.dedup();
        names
    }

    /// Unit of the result if it can be inferred (`"V"`, `"A"`, `"W"`, `"dB"`,
    /// `"°"`), else `""`. Ohm (`"Ω"`), siemens (`"S"`), second (`"s"`) and
    /// hertz (`"Hz"`) are inferred too.
    pub fn unit(&self) -> &'static str {
        dimension::unit_symbol(&self.tree)
    }

    /// Evaluates against `result`. Real results yield [`Series::Real`], AC
    /// results [`Series::Complex`] (except `db`, `mag`, `ph`, `re`, `im`,
    /// whose outer application yields real samples).
    ///
    /// Precisely: the series is complex when the expression depends on AC
    /// signal data other than through those real-valued functions (or
    /// `abs`), so `db(V(out)) - db(V(in))` and `freq` are real.
    pub fn evaluate(&self, result: &SimulationResult) -> Result<Series, ExprError> {
        eval::evaluate(&self.tree, result)
    }
}
