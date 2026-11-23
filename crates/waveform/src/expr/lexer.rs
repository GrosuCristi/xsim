//! Tokenizer for trace expressions.
//!
//! The lexer is pulled by the parser one token at a time. That lets the
//! parser read the arguments of `V(...)` and `I(...)` as raw names instead of
//! tokens, so node names such as `0`, `N001` or `net-1` need no quoting.
//!
//! Positions are byte offsets into the source text; the lexer only ever
//! advances by whole characters, so they always fall on character
//! boundaries.

use super::ExprError;
use xsim_spice_engine::units;

/// SI suffixes accepted after a number literal. The factors themselves come
/// from [`units::parse_value`] so expression constants match component values.
const SI_SUFFIXES: [char; 11] = ['f', 'p', 'n', 'u', 'µ', 'μ', 'm', 'k', 'M', 'G', 'T'];

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TokenKind<'a> {
    Number(f64),
    Identifier(&'a str),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    OpenParen,
    CloseParen,
    Comma,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Token<'a> {
    pub(crate) kind: TokenKind<'a>,
    /// Byte offset of the first character.
    pub(crate) position: usize,
    /// The token's source text (empty for [`TokenKind::End`]).
    pub(crate) text: &'a str,
}

pub(crate) struct Lexer<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    /// Lexes the next token, skipping leading whitespace.
    pub(crate) fn next_token(&mut self) -> Result<Token<'a>, ExprError> {
        self.skip_whitespace();
        let start = self.position;
        let Some(c) = self.peek() else {
            return Ok(Token {
                kind: TokenKind::End,
                position: start,
                text: "",
            });
        };
        let kind = match c {
            '0'..='9' | '.' => TokenKind::Number(self.number()?),
            c if is_identifier_start(c) => {
                self.advance_while(is_identifier_continue);
                TokenKind::Identifier(&self.source[start..self.position])
            }
            _ => {
                let kind = match c {
                    '+' => TokenKind::Plus,
                    '-' => TokenKind::Minus,
                    '*' => TokenKind::Star,
                    '/' => TokenKind::Slash,
                    '^' => TokenKind::Caret,
                    '(' => TokenKind::OpenParen,
                    ')' => TokenKind::CloseParen,
                    ',' => TokenKind::Comma,
                    _ => {
                        return Err(syntax(start, format!("Unexpected character '{c}'")));
                    }
                };
                self.position += c.len_utf8();
                kind
            }
        };
        Ok(Token {
            kind,
            position: start,
            text: &self.source[start..self.position],
        })
    }

    /// Reads a raw node or element name: everything up to whitespace, a
    /// comma or a parenthesis. Returns the (possibly empty) name and its
    /// byte offset.
    pub(crate) fn raw_name(&mut self) -> (&'a str, usize) {
        self.skip_whitespace();
        let start = self.position;
        self.advance_while(|c| !c.is_whitespace() && !matches!(c, ',' | '(' | ')'));
        (&self.source[start..self.position], start)
    }

    /// Lexes a number literal: digits with an optional fraction, an optional
    /// exponent and an optional single SI suffix. Anything alphanumeric glued
    /// to the literal is an error rather than a silently ignored unit, so
    /// `2meg` or `5V` are reported instead of being misread.
    fn number(&mut self) -> Result<f64, ExprError> {
        let start = self.position;
        let integer_digits = self.advance_while(|c| c.is_ascii_digit());
        let mut fraction_digits = 0;
        if self.peek() == Some('.') {
            self.position += 1;
            fraction_digits = self.advance_while(|c| c.is_ascii_digit());
        }
        if integer_digits + fraction_digits == 0 {
            return Err(syntax(start, "Expected a digit".to_string()));
        }
        if self.exponent_follows() {
            self.position += 1; // 'e' or 'E'
            if matches!(self.peek(), Some('+' | '-')) {
                self.position += 1;
            }
            self.advance_while(|c| c.is_ascii_digit());
        }
        let mantissa_end = self.position;
        if let Some(suffix) = self.peek().filter(|c| SI_SUFFIXES.contains(c))
            && !self.peek_second().is_some_and(is_number_continue)
        {
            self.position += suffix.len_utf8();
        }
        if self.peek().is_some_and(is_number_continue) {
            self.advance_while(is_number_continue);
            let literal = &self.source[start..self.position];
            return Err(syntax(
                start,
                format!(
                    "Invalid number '{literal}' (SI suffixes are f p n u m k M G T and are \
                     case-sensitive: m is milli, M is mega)"
                ),
            ));
        }

        let literal = &self.source[start..self.position];
        let value = if self.position == mantissa_end {
            literal.parse::<f64>().ok()
        } else {
            units::parse_value(literal).ok()
        };
        match value {
            Some(value) if value.is_finite() => Ok(value),
            Some(_) => Err(syntax(start, format!("Number '{literal}' is out of range"))),
            None => Err(syntax(start, format!("Invalid number '{literal}'"))),
        }
    }

    /// Whether an exponent (`e` or `E`, an optional sign, then a digit)
    /// starts at the current position.
    fn exponent_follows(&self) -> bool {
        let mut rest = self.source[self.position..].chars();
        if !matches!(rest.next(), Some('e' | 'E')) {
            return false;
        }
        match rest.next() {
            Some('+' | '-') => rest.next().is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    fn skip_whitespace(&mut self) {
        self.advance_while(char::is_whitespace);
    }

    /// Advances over characters matching `predicate`; returns how many.
    fn advance_while(&mut self, predicate: impl Fn(char) -> bool) -> usize {
        let mut count = 0;
        while let Some(c) = self.peek().filter(|&c| predicate(c)) {
            self.position += c.len_utf8();
            count += 1;
        }
        count
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.source[self.position..].chars().nth(1)
    }
}

fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_identifier_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Characters that may not directly follow a complete number literal.
fn is_number_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

pub(crate) fn syntax(position: usize, message: String) -> ExprError {
    ExprError::Syntax { position, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind<'_>> {
        let mut lexer = Lexer::new(source);
        let mut kinds = Vec::new();
        loop {
            let token = lexer.next_token().expect("valid input");
            kinds.push(token.kind);
            if token.kind == TokenKind::End {
                return kinds;
            }
        }
    }

    fn error_position(source: &str) -> usize {
        let mut lexer = Lexer::new(source);
        loop {
            match lexer.next_token() {
                Ok(token) if token.kind == TokenKind::End => panic!("{source:?} lexed cleanly"),
                Ok(_) => {}
                Err(ExprError::Syntax { position, .. }) => return position,
                Err(other) => panic!("unexpected error {other:?}"),
            }
        }
    }

    #[test]
    fn operators_and_identifiers() {
        use TokenKind::*;
        assert_eq!(
            kinds(" a_1+b-c*d/e^f(,) "),
            [
                Identifier("a_1"),
                Plus,
                Identifier("b"),
                Minus,
                Identifier("c"),
                Star,
                Identifier("d"),
                Slash,
                Identifier("e"),
                Caret,
                Identifier("f"),
                OpenParen,
                Comma,
                CloseParen,
                End
            ]
        );
    }

    #[test]
    fn plain_numbers_parse_exactly() {
        for (text, expected) in [
            ("2", 2.0),
            ("0.5", 0.5),
            (".5", 0.5),
            ("5.", 5.0),
            ("1e-3", 1e-3),
            ("1E+3", 1e3),
            ("2.5e2", 250.0),
        ] {
            assert_eq!(
                kinds(text),
                [TokenKind::Number(expected), TokenKind::End],
                "{text}"
            );
        }
    }

    #[test]
    fn number_followed_by_identifier_character_is_rejected() {
        for text in ["2e", "2x", "5V", "2meg", "1.2.3", "3_", "2mm"] {
            assert_eq!(error_position(text), 0, "{text}");
        }
    }

    #[test]
    fn exponent_needs_digits() {
        // "1e+" is not an exponent, so the 'e' is glued to the number.
        assert_eq!(error_position("1e+"), 0);
    }

    #[test]
    fn lone_dot_is_not_a_number() {
        assert_eq!(error_position("  ."), 2);
    }

    #[test]
    fn unexpected_characters_report_byte_positions() {
        assert_eq!(error_position("1 + #"), 4);
        assert_eq!(error_position("(µ"), 1);

        // Raw names may contain any non-separator character...
        let mut lexer = Lexer::new("(µ$");
        lexer.next_token().unwrap();
        assert_eq!(lexer.raw_name(), ("µ$", 1));
        // ...and positions count bytes: 'µ' is two, so '$' starts at byte 4.
        let mut lexer = Lexer::new("(µ $");
        lexer.next_token().unwrap();
        lexer.raw_name();
        assert_eq!(
            lexer.next_token(),
            Err(syntax(4, "Unexpected character '$'".to_string()))
        );
    }

    #[test]
    fn number_out_of_range_is_an_error() {
        assert_eq!(error_position("1e400"), 0);
    }

    #[test]
    fn raw_names_stop_at_separators() {
        let mut lexer = Lexer::new("( net-1 , 0)");
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::OpenParen);
        assert_eq!(lexer.raw_name(), ("net-1", 2));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Comma);
        assert_eq!(lexer.raw_name(), ("0", 10));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::CloseParen);
        assert_eq!(lexer.raw_name(), ("", 12));
    }
}
