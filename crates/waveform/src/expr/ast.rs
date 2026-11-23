//! Syntax tree of a trace expression.

/// Name of the ground node. Ground is the reference for every node voltage,
/// so the engine never records it as a signal.
pub(crate) const GROUND_NODE: &str = "0";

/// Prefixes of current probes, as they appear in recorded signal names:
/// two-terminal elements, BJT terminals, JFET terminals and op-amp
/// terminals (D26: inputs `Ip`/`In`, output `Io`, supplies `Isp`/`Isn`).
pub(crate) const CURRENT_PREFIXES: [&str; 12] = [
    "I", "Ic", "Ib", "Ie", "Id", "Ig", "Is", "Ip", "In", "Io", "Isp", "Isn",
];

/// Prefix of voltage probes.
pub(crate) const VOLTAGE_PREFIX: &str = "V";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Number(f64),
    /// A recorded quantity; `position` is the byte offset of its prefix.
    Probe {
        probe: Probe,
        position: usize,
    },
    /// The result's x axis; `position` is the byte offset of the name.
    Variable {
        variable: Variable,
        position: usize,
    },
    Negate(Box<Node>),
    Binary {
        op: BinaryOp,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    Call {
        function: Function,
        args: Vec<Node>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    /// `V(node)`, or the differential `V(node, reference)`.
    Voltage {
        node: String,
        reference: Option<String>,
    },
    /// `I(element)`, `Ic(element)`, …; `prefix` is one of [`CURRENT_PREFIXES`].
    Current {
        prefix: &'static str,
        element: String,
    },
}

impl Probe {
    /// Canonical names of the recorded signals this probe reads. Ground is
    /// omitted because it is never recorded.
    pub(crate) fn signal_names(&self) -> Vec<String> {
        match self {
            Self::Voltage { node, reference } => std::iter::once(node)
                .chain(reference)
                .filter(|name| name.as_str() != GROUND_NODE)
                .map(|name| voltage_signal_name(name))
                .collect(),
            Self::Current { prefix, element } => vec![format!("{prefix}({element})")],
        }
    }
}

/// Recorded signal name of the voltage of `node`.
pub(crate) fn voltage_signal_name(node: &str) -> String {
    format!("{VOLTAGE_PREFIX}({node})")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variable {
    Time,
    Freq,
}

impl Variable {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "time" => Some(Self::Time),
            "freq" => Some(Self::Freq),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Freq => "freq",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Function {
    Abs,
    Sqrt,
    Exp,
    Ln,
    Log10,
    Sin,
    Cos,
    Tan,
    Atan,
    Min,
    Max,
    Db,
    Mag,
    Ph,
    Re,
    Im,
    Deriv,
    Integ,
}

impl Function {
    const ALL: [Self; 18] = [
        Self::Abs,
        Self::Sqrt,
        Self::Exp,
        Self::Ln,
        Self::Log10,
        Self::Sin,
        Self::Cos,
        Self::Tan,
        Self::Atan,
        Self::Min,
        Self::Max,
        Self::Db,
        Self::Mag,
        Self::Ph,
        Self::Re,
        Self::Im,
        Self::Deriv,
        Self::Integ,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|function| function.name() == name)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Sqrt => "sqrt",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Log10 => "log10",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Atan => "atan",
            Self::Min => "min",
            Self::Max => "max",
            Self::Db => "db",
            Self::Mag => "mag",
            Self::Ph => "ph",
            Self::Re => "re",
            Self::Im => "im",
            Self::Deriv => "deriv",
            Self::Integ => "integ",
        }
    }

    /// Number of arguments the function takes.
    pub(crate) fn arity(self) -> usize {
        match self {
            Self::Min | Self::Max => 2,
            _ => 1,
        }
    }
}

impl Node {
    /// Appends the canonical names of every signal the tree reads.
    pub(crate) fn collect_signals(&self, names: &mut Vec<String>) {
        match self {
            Self::Number(_) | Self::Variable { .. } => {}
            Self::Probe { probe, .. } => names.extend(probe.signal_names()),
            Self::Negate(operand) => operand.collect_signals(names),
            Self::Binary { lhs, rhs, .. } => {
                lhs.collect_signals(names);
                rhs.collect_signals(names);
            }
            Self::Call { args, .. } => args.iter().for_each(|arg| arg.collect_signals(names)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_function_round_trips_through_its_name() {
        for function in Function::ALL {
            assert_eq!(Function::from_name(function.name()), Some(function));
        }
        assert_eq!(
            Function::from_name("Sqrt"),
            None,
            "function names are lowercase"
        );
    }

    #[test]
    fn differential_probe_names_both_nodes_but_never_ground() {
        let probe = Probe::Voltage {
            node: "out".into(),
            reference: Some("in".into()),
        };
        assert_eq!(probe.signal_names(), ["V(out)", "V(in)"]);

        let to_ground = Probe::Voltage {
            node: "out".into(),
            reference: Some(GROUND_NODE.into()),
        };
        assert_eq!(to_ground.signal_names(), ["V(out)"]);
    }

    #[test]
    fn current_probe_names_keep_their_prefix() {
        let probe = Probe::Current {
            prefix: "Ic",
            element: "Q1".into(),
        };
        assert_eq!(probe.signal_names(), ["Ic(Q1)"]);
    }
}
