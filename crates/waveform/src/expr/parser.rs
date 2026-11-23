//! Recursive-descent parser producing the expression tree (grammar in the
//! [parent module](super)).

use super::ExprError;
use super::ast::{BinaryOp, CURRENT_PREFIXES, Function, Node, Probe, VOLTAGE_PREFIX, Variable};
use super::lexer::{Lexer, Token, TokenKind, syntax};

/// Limit on both parser recursion and tree height. Evaluation and unit
/// inference recurse over the tree, so without a bound a pathological input
/// such as 100 000 opening parentheses would overflow the stack (an abort,
/// not a recoverable error). Hand-written expressions never come close.
const MAX_DEPTH: usize = 128;

pub(crate) fn parse(source: &str) -> Result<Node, ExprError> {
    let mut parser = Parser::new(source)?;
    if parser.current.kind == TokenKind::End {
        return Err(syntax(0, "Expression is empty".to_string()));
    }
    let tree = parser.expression()?;
    let token = parser.current;
    match token.kind {
        TokenKind::End => Ok(tree.node),
        TokenKind::CloseParen => Err(syntax(token.position, "Unmatched ')'".to_string())),
        _ => Err(syntax(
            token.position,
            format!("Expected an operator before '{}'", token.text),
        )),
    }
}

/// A parsed subtree and its height (1 for a leaf).
struct Subtree {
    node: Node,
    height: usize,
}

impl Subtree {
    fn leaf(node: Node) -> Self {
        Self { node, height: 1 }
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    /// One token of lookahead.
    current: Token<'a>,
    /// Recursion depth of [`Parser::unary`], which every nested construct
    /// (parentheses, call arguments, negation, exponents) passes through.
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Result<Self, ExprError> {
        let mut lexer = Lexer::new(source);
        let current = lexer.next_token()?;
        Ok(Self {
            lexer,
            current,
            depth: 0,
        })
    }

    /// Consumes the current token and returns it.
    fn advance(&mut self) -> Result<Token<'a>, ExprError> {
        let next = self.lexer.next_token()?;
        Ok(std::mem::replace(&mut self.current, next))
    }

    /// `expr := term (('+' | '-') term)*`
    fn expression(&mut self) -> Result<Subtree, ExprError> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.current.kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => return Ok(lhs),
            };
            let operator = self.advance()?;
            let rhs = self.term()?;
            lhs = binary(op, lhs, rhs, operator.position)?;
        }
    }

    /// `term := unary (('*' | '/') unary)*`
    fn term(&mut self) -> Result<Subtree, ExprError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.current.kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => return Ok(lhs),
            };
            let operator = self.advance()?;
            let rhs = self.unary()?;
            lhs = binary(op, lhs, rhs, operator.position)?;
        }
    }

    /// `unary := '-' unary | power`
    fn unary(&mut self) -> Result<Subtree, ExprError> {
        if self.depth >= MAX_DEPTH {
            return Err(too_deep(self.current.position));
        }
        self.depth += 1;
        let subtree = self.negation_or_power();
        self.depth -= 1;
        subtree
    }

    fn negation_or_power(&mut self) -> Result<Subtree, ExprError> {
        if self.current.kind != TokenKind::Minus {
            return self.power();
        }
        let minus = self.advance()?;
        let operand = self.unary()?;
        grow(
            Node::Negate(Box::new(operand.node)),
            operand.height,
            minus.position,
        )
    }

    /// `power := atom ('^' unary)?`
    fn power(&mut self) -> Result<Subtree, ExprError> {
        let base = self.atom()?;
        if self.current.kind != TokenKind::Caret {
            return Ok(base);
        }
        let caret = self.advance()?;
        let exponent = self.unary()?;
        binary(BinaryOp::Pow, base, exponent, caret.position)
    }

    /// `atom := number | signal | variable | call | '(' expr ')'`
    fn atom(&mut self) -> Result<Subtree, ExprError> {
        let token = self.current;
        match token.kind {
            TokenKind::Number(value) => {
                self.advance()?;
                Ok(Subtree::leaf(Node::Number(value)))
            }
            TokenKind::OpenParen => {
                self.advance()?;
                let inner = self.expression()?;
                self.expect_close(token.position)?;
                Ok(inner)
            }
            TokenKind::Identifier(name) => self.named(name, token.position),
            TokenKind::End => Err(syntax(
                token.position,
                "Expression ends early: expected a value".to_string(),
            )),
            _ => Err(syntax(
                token.position,
                format!("Expected a value before '{}'", token.text),
            )),
        }
    }

    /// An atom starting with an identifier: a probe, an axis variable or a
    /// function call.
    fn named(&mut self, name: &'a str, position: usize) -> Result<Subtree, ExprError> {
        if name == VOLTAGE_PREFIX {
            return self.probe(None, position);
        }
        if let Some(&prefix) = CURRENT_PREFIXES.iter().find(|&&prefix| prefix == name) {
            return self.probe(Some(prefix), position);
        }
        self.advance()?;
        if let Some(variable) = Variable::from_name(name) {
            return Ok(Subtree::leaf(Node::Variable { variable, position }));
        }
        let function = Function::from_name(name);
        if self.current.kind != TokenKind::OpenParen {
            let message = match function {
                Some(_) => format!("Expected '(' after {name}"),
                None => format!(
                    "Unknown name '{name}': write V({name}) for a node voltage or I({name}) \
                     for a current"
                ),
            };
            return Err(syntax(position, message));
        }
        let function = function.ok_or_else(|| ExprError::UnknownFunction(name.to_string()))?;
        self.call(function, position)
    }

    /// `call := func '(' expr (',' expr)* ')'`, with the current token on the
    /// opening parenthesis.
    fn call(&mut self, function: Function, position: usize) -> Result<Subtree, ExprError> {
        let open = self.advance()?;
        let mut args = Vec::new();
        let mut height = 0;
        if self.current.kind != TokenKind::CloseParen {
            loop {
                let arg = self.expression()?;
                height = height.max(arg.height);
                args.push(arg.node);
                if self.current.kind != TokenKind::Comma {
                    break;
                }
                self.advance()?;
            }
        }
        self.expect_close(open.position)?;
        if args.len() != function.arity() {
            return Err(ExprError::Arity {
                function: function.name().to_string(),
                expected: function.arity(),
                found: args.len(),
            });
        }
        grow(Node::Call { function, args }, height, position)
    }

    /// `V(node)`, `V(node, node)` or `<current prefix>(element)`, with the
    /// current token on the prefix. `current_prefix` is `None` for voltages.
    fn probe(
        &mut self,
        current_prefix: Option<&'static str>,
        position: usize,
    ) -> Result<Subtree, ExprError> {
        let prefix = self.advance()?.text;
        if self.current.kind != TokenKind::OpenParen {
            return Err(syntax(
                self.current.position,
                format!("Expected '(' after {prefix}"),
            ));
        }
        let what = if current_prefix.is_some() {
            "an element name"
        } else {
            "a node name"
        };
        // The lexer stands just past the '(': read the names raw, then
        // resume tokenizing at the separator that follows each one.
        let (first, _) = self.raw_name(what)?;
        let mut others = Vec::new();
        self.current = self.lexer.next_token()?;
        while self.current.kind == TokenKind::Comma {
            others.push(self.raw_name(what)?);
            self.current = self.lexer.next_token()?;
        }
        match self.current.kind {
            TokenKind::CloseParen => {}
            TokenKind::End => {
                return Err(syntax(position, format!("Missing ')' after {prefix}(")));
            }
            _ => {
                return Err(syntax(
                    self.current.position,
                    format!("Expected ',' or ')' before '{}'", self.current.text),
                ));
            }
        }
        self.advance()?;

        let probe = match (current_prefix, others.as_slice()) {
            (None, []) => Probe::Voltage {
                node: first.to_string(),
                reference: None,
            },
            (None, [(reference, _)]) => Probe::Voltage {
                node: first.to_string(),
                reference: Some(reference.to_string()),
            },
            (None, [_, (_, extra), ..]) => {
                return Err(syntax(
                    *extra,
                    "V(...) takes one or two node names".to_string(),
                ));
            }
            (Some(prefix), []) => Probe::Current {
                prefix,
                element: first.to_string(),
            },
            (Some(prefix), [(_, extra), ..]) => {
                return Err(syntax(
                    *extra,
                    format!("{prefix}(...) takes one element name"),
                ));
            }
        };
        Ok(Subtree::leaf(Node::Probe { probe, position }))
    }

    /// Reads one raw probe argument; `what` names it for the error message.
    fn raw_name(&mut self, what: &str) -> Result<(&'a str, usize), ExprError> {
        let (name, position) = self.lexer.raw_name();
        if name.is_empty() {
            return Err(syntax(position, format!("Expected {what}")));
        }
        Ok((name, position))
    }

    fn expect_close(&mut self, open_position: usize) -> Result<(), ExprError> {
        match self.current.kind {
            TokenKind::CloseParen => {
                self.advance()?;
                Ok(())
            }
            TokenKind::End => Err(syntax(
                open_position,
                "Missing ')' for this '('".to_string(),
            )),
            _ => Err(syntax(
                self.current.position,
                format!("Expected ')' before '{}'", self.current.text),
            )),
        }
    }
}

fn binary(op: BinaryOp, lhs: Subtree, rhs: Subtree, position: usize) -> Result<Subtree, ExprError> {
    let height = lhs.height.max(rhs.height);
    let node = Node::Binary {
        op,
        lhs: Box::new(lhs.node),
        rhs: Box::new(rhs.node),
    };
    grow(node, height, position)
}

/// Wraps `node`, whose tallest child has `child_height`, enforcing
/// [`MAX_DEPTH`].
fn grow(node: Node, child_height: usize, position: usize) -> Result<Subtree, ExprError> {
    let height = child_height + 1;
    if height > MAX_DEPTH {
        return Err(too_deep(position));
    }
    Ok(Subtree { node, height })
}

fn too_deep(position: usize) -> ExprError {
    syntax(
        position,
        format!("Expression is nested too deeply (the limit is {MAX_DEPTH} levels)"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: f64) -> Box<Node> {
        Box::new(Node::Number(value))
    }

    fn voltage(node: &str, position: usize) -> Node {
        Node::Probe {
            probe: Probe::Voltage {
                node: node.to_string(),
                reference: None,
            },
            position,
        }
    }

    fn syntax_position(source: &str) -> usize {
        match parse(source) {
            Err(ExprError::Syntax { position, .. }) => position,
            other => panic!("{source:?}: expected a syntax error, got {other:?}"),
        }
    }

    #[test]
    fn precedence_and_associativity_follow_the_grammar() {
        // 1 - 2 - 3 is (1 - 2) - 3; 2 * 3 binds tighter than +.
        let tree = parse("1 - 2 - 3 + 2 * 3").unwrap();
        let left = Node::Binary {
            op: BinaryOp::Sub,
            lhs: Box::new(Node::Binary {
                op: BinaryOp::Sub,
                lhs: number(1.0),
                rhs: number(2.0),
            }),
            rhs: number(3.0),
        };
        let product = Node::Binary {
            op: BinaryOp::Mul,
            lhs: number(2.0),
            rhs: number(3.0),
        };
        assert_eq!(
            tree,
            Node::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(left),
                rhs: Box::new(product),
            }
        );
    }

    #[test]
    fn power_is_right_associative_and_binds_tighter_than_negation() {
        // -2^3^2 is -(2^(3^2)).
        let tree = parse("-2^3^2").unwrap();
        let inner = Node::Binary {
            op: BinaryOp::Pow,
            lhs: number(3.0),
            rhs: number(2.0),
        };
        let power = Node::Binary {
            op: BinaryOp::Pow,
            lhs: number(2.0),
            rhs: Box::new(inner),
        };
        assert_eq!(tree, Node::Negate(Box::new(power)));
    }

    #[test]
    fn exponent_may_be_negative() {
        let tree = parse("2^-1").unwrap();
        assert_eq!(
            tree,
            Node::Binary {
                op: BinaryOp::Pow,
                lhs: number(2.0),
                rhs: Box::new(Node::Negate(number(1.0))),
            }
        );
    }

    #[test]
    fn probes_read_raw_names() {
        assert_eq!(parse(" V( N001 ) ").unwrap(), voltage("N001", 1));
        assert_eq!(parse("V(0)").unwrap(), voltage("0", 0));
        assert_eq!(parse("V(net-1)").unwrap(), voltage("net-1", 0));
        assert_eq!(
            parse("V(out,in)").unwrap(),
            Node::Probe {
                probe: Probe::Voltage {
                    node: "out".into(),
                    reference: Some("in".into()),
                },
                position: 0,
            }
        );
        assert_eq!(
            parse("Ib(Q1)").unwrap(),
            Node::Probe {
                probe: Probe::Current {
                    prefix: "Ib",
                    element: "Q1".into(),
                },
                position: 0,
            }
        );
    }

    #[test]
    fn variables_and_calls() {
        assert_eq!(
            parse("time").unwrap(),
            Node::Variable {
                variable: Variable::Time,
                position: 0,
            }
        );
        assert_eq!(
            parse("max(1, freq)").unwrap(),
            Node::Call {
                function: Function::Max,
                args: vec![
                    Node::Number(1.0),
                    Node::Variable {
                        variable: Variable::Freq,
                        position: 7,
                    },
                ],
            }
        );
    }

    #[test]
    fn syntax_errors_point_at_the_offending_token() {
        assert_eq!(syntax_position(""), 0);
        assert_eq!(syntax_position("   "), 0);
        assert_eq!(syntax_position("1 +"), 3);
        assert_eq!(syntax_position("1 2"), 2);
        assert_eq!(syntax_position("(1 + 2"), 0);
        assert_eq!(syntax_position("(1 2)"), 3);
        assert_eq!(syntax_position("1 + 2)"), 5);
        assert_eq!(syntax_position("* 2"), 0);
        assert_eq!(syntax_position("out"), 0);
        assert_eq!(syntax_position("1 + sqrt"), 4);
        assert_eq!(syntax_position("V out"), 2);
        assert_eq!(syntax_position("V()"), 2);
        assert_eq!(syntax_position("V(a,)"), 4);
        assert_eq!(syntax_position("V(a b)"), 4);
        assert_eq!(syntax_position("V(a"), 0);
        assert_eq!(syntax_position("V(a, b, c)"), 8);
        assert_eq!(syntax_position("I(R1, R2)"), 6);
        assert_eq!(syntax_position("time(1)"), 4);
    }

    #[test]
    fn unknown_functions_and_wrong_arity_have_dedicated_errors() {
        assert_eq!(
            parse("foo(1)"),
            Err(ExprError::UnknownFunction("foo".into()))
        );
        assert_eq!(parse("v(out)"), Err(ExprError::UnknownFunction("v".into())));
        assert_eq!(
            parse("sqrt(1, 2)"),
            Err(ExprError::Arity {
                function: "sqrt".into(),
                expected: 1,
                found: 2,
            })
        );
        assert_eq!(
            parse("min()"),
            Err(ExprError::Arity {
                function: "min".into(),
                expected: 2,
                found: 0,
            })
        );
    }

    #[test]
    fn nesting_up_to_the_limit_is_accepted() {
        let depth = MAX_DEPTH - 1;
        let nested = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(parse(&nested).unwrap(), Node::Number(1.0));
        let chain = vec!["1"; MAX_DEPTH].join("+");
        assert!(parse(&chain).is_ok());
    }

    #[test]
    fn pathological_nesting_is_an_error_not_a_stack_overflow() {
        for source in [
            "(".repeat(100_000),
            format!("{}1", "-".repeat(100_000)),
            vec!["2"; 100_000].join("^"),
            vec!["1"; 100_000].join("+"),
            vec!["1"; 100_000].join("*"),
            format!("{}1{}", "abs(".repeat(100_000), ")".repeat(100_000)),
        ] {
            match parse(&source) {
                Err(ExprError::Syntax { message, .. }) => {
                    assert!(message.contains("nested too deeply"), "{message}");
                }
                other => panic!("expected a depth error, got {other:?}"),
            }
        }
    }
}
