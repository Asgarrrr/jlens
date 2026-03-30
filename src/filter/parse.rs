use super::FilterError;

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `.`
    Identity,
    /// `.foo`
    Field(String),
    /// `.[n]`
    Index(i64),
    /// `.[n:m]` — either bound may be absent
    Slice(Option<i64>, Option<i64>),
    /// `.[]`
    Iterate,
    /// `a | b`
    Pipe(Box<Expr>, Box<Expr>),
    /// `.foo.bar` — evaluates left then right (semantically like pipe but
    /// written without `|`)
    Chain(Box<Expr>, Box<Expr>),
    /// A builtin function applied to the current stream
    Builtin(BuiltinFn),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinFn {
    Length,
    Keys,
    Values,
    Type,
    Flatten,
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Dot,
    Pipe,
    LBracket,
    RBracket,
    Colon,
    Ident(String),
    Integer(i64),
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input[self.pos..].chars().next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.advance();
        }
    }

    fn read_ident_or_keyword(&mut self) -> String {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_integer(&mut self) -> Result<i64, FilterError> {
        let mut s = String::new();
        if self.peek() == Some('-') {
            s.push('-');
            self.advance();
        }
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        s.parse::<i64>()
            .map_err(|_| FilterError::Parse(format!("invalid integer: {}", s)))
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, FilterError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => break,
                Some('.') => {
                    self.advance();
                    tokens.push(Token::Dot);
                }
                Some('|') => {
                    self.advance();
                    tokens.push(Token::Pipe);
                }
                Some('[') => {
                    self.advance();
                    tokens.push(Token::LBracket);
                }
                Some(']') => {
                    self.advance();
                    tokens.push(Token::RBracket);
                }
                Some(':') => {
                    self.advance();
                    tokens.push(Token::Colon);
                }
                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    let ident = self.read_ident_or_keyword();
                    tokens.push(Token::Ident(ident));
                }
                Some(ch) if ch.is_ascii_digit() || ch == '-' => {
                    let n = self.read_integer()?;
                    tokens.push(Token::Integer(n));
                }
                Some(ch) => {
                    return Err(FilterError::Parse(format!(
                        "unexpected character: {:?}",
                        ch
                    )));
                }
            }
        }
        Ok(tokens)
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), FilterError> {
        match self.advance() {
            Some(tok) if tok == expected => Ok(()),
            Some(tok) => Err(FilterError::Parse(format!(
                "expected {:?}, got {:?}",
                expected, tok
            ))),
            None => Err(FilterError::Parse(format!(
                "expected {:?}, got end of input",
                expected
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Grammar (highest to lowest precedence):
    //
    //   pipe_expr   := chain_expr ( '|' pipe_rhs )*
    //   pipe_rhs    := builtin_expr | chain_expr
    //   chain_expr  := primary ( bracket_or_field )*
    //   primary     := '.' field_or_index?
    //   bracket_or_field := '[' index_or_slice ']'
    //                     | '.' ident
    //                     | '.' '[' index_or_slice ']'
    // -----------------------------------------------------------------------

    fn parse_expr(&mut self) -> Result<Expr, FilterError> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> Result<Expr, FilterError> {
        let left = self.parse_chain()?;

        if self.peek() == Some(&Token::Pipe) {
            self.advance(); // consume '|'
            let right = self.parse_pipe_rhs()?;
            Ok(Expr::Pipe(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    // The RHS of a pipe can be a builtin name or another chain/pipe expression.
    fn parse_pipe_rhs(&mut self) -> Result<Expr, FilterError> {
        // Try to parse as builtin
        if let Some(Token::Ident(name)) = self.peek() {
            let builtin = match name.as_str() {
                "length" => Some(BuiltinFn::Length),
                "keys" => Some(BuiltinFn::Keys),
                "values" => Some(BuiltinFn::Values),
                "type" => Some(BuiltinFn::Type),
                "flatten" => Some(BuiltinFn::Flatten),
                _ => None,
            };
            if let Some(b) = builtin {
                self.advance(); // consume ident
                // Allow chaining after a builtin via another pipe
                let builtin_expr = Expr::Builtin(b);
                if self.peek() == Some(&Token::Pipe) {
                    self.advance();
                    let right = self.parse_pipe_rhs()?;
                    return Ok(Expr::Pipe(Box::new(builtin_expr), Box::new(right)));
                }
                return Ok(builtin_expr);
            }
        }
        // Otherwise fall through to chain/pipe
        self.parse_pipe()
    }

    fn parse_chain(&mut self) -> Result<Expr, FilterError> {
        let mut expr = self.parse_primary()?;

        // After the primary dot expression, allow chaining via additional
        // bracket accesses that are NOT preceded by another dot (those are
        // handled inside parse_dot_chain already).
        // We re-enter here only for bare `[...]` suffixes attached directly.
        while let Some(Token::LBracket) = self.peek() {
            self.advance(); // consume '['
            let suffix = self.parse_index_or_slice()?;
            self.expect(&Token::RBracket)?;
            expr = Expr::Chain(Box::new(expr), Box::new(suffix));
        }

        Ok(expr)
    }

    /// Parse a primary expression: a leading `.` followed by optional
    /// chained field/index access.
    fn parse_primary(&mut self) -> Result<Expr, FilterError> {
        match self.peek() {
            Some(Token::Dot) => {
                self.advance(); // consume '.'
                self.parse_dot_chain(Expr::Identity)
            }
            Some(Token::Ident(name)) => {
                // Bare ident: check if it's a builtin used as identity pipeline
                let name = name.clone();
                match name.as_str() {
                    "length" | "keys" | "values" | "type" | "flatten" => {
                        Err(FilterError::Parse(format!(
                            "builtin '{}' must be used after a pipe (e.g. '. | {}')",
                            name, name
                        )))
                    }
                    _ => Err(FilterError::Parse(format!(
                        "unexpected identifier '{}'; did you mean '.{}'?",
                        name, name
                    ))),
                }
            }
            Some(tok) => Err(FilterError::Parse(format!(
                "unexpected token {:?}",
                tok.clone()
            ))),
            None => Err(FilterError::Parse("empty expression".to_string())),
        }
    }

    /// After consuming the leading `.`, parse the optional continuation:
    ///   - nothing → Identity
    ///   - `ident`  → Field, then recurse for further `.foo` or `[n]`
    ///   - `[...]`  → Index / Slice / Iterate, then recurse
    fn parse_dot_chain(&mut self, base: Expr) -> Result<Expr, FilterError> {
        match self.peek() {
            // `.foo` field access
            Some(Token::Ident(_)) => {
                let name = match self.advance() {
                    Some(Token::Ident(n)) => n.clone(),
                    _ => unreachable!(),
                };
                let field_expr = chain(base, Expr::Field(name));
                self.parse_dot_suffix(field_expr)
            }

            // `.[...]`
            Some(Token::LBracket) => {
                self.advance(); // consume '['
                let inner = self.parse_index_or_slice()?;
                self.expect(&Token::RBracket)?;
                let chained = chain(base, inner);
                self.parse_dot_suffix(chained)
            }

            // Just `.` on its own (or followed by `|` / end of input)
            _ => Ok(base),
        }
    }

    /// After a complete field/index expression, parse further `.foo` or `[n]`
    /// suffixes attached via `.`.
    fn parse_dot_suffix(&mut self, expr: Expr) -> Result<Expr, FilterError> {
        match self.peek() {
            Some(Token::Dot) => {
                self.advance(); // consume '.'
                self.parse_dot_chain(expr)
            }
            Some(Token::LBracket) => {
                self.advance(); // consume '['
                let inner = self.parse_index_or_slice()?;
                self.expect(&Token::RBracket)?;
                let chained = chain(expr, inner);
                self.parse_dot_suffix(chained)
            }
            _ => Ok(expr),
        }
    }

    /// Parse the content inside `[...]`: `n`, `n:m`, `:m`, `n:`, or empty (iterate).
    fn parse_index_or_slice(&mut self) -> Result<Expr, FilterError> {
        match self.peek() {
            // `[]` → iterate
            Some(Token::RBracket) => Ok(Expr::Iterate),

            // `[:m]` → slice with no lower bound
            Some(Token::Colon) => {
                self.advance(); // consume ':'
                let upper = self.parse_optional_int()?;
                Ok(Expr::Slice(None, upper))
            }

            // `[n]` or `[n:...]`
            Some(Token::Integer(_)) | Some(Token::Dot) => {
                // negative number is lexed as Integer(-n) already
                let n = self.parse_required_int()?;
                match self.peek() {
                    Some(Token::Colon) => {
                        self.advance(); // consume ':'
                        let upper = self.parse_optional_int()?;
                        Ok(Expr::Slice(Some(n), upper))
                    }
                    _ => Ok(Expr::Index(n)),
                }
            }

            Some(tok) => Err(FilterError::Parse(format!(
                "expected index, slice, or ']', got {:?}",
                tok.clone()
            ))),
            None => Err(FilterError::Parse(
                "unexpected end of input inside '[...]'".to_string(),
            )),
        }
    }

    fn parse_required_int(&mut self) -> Result<i64, FilterError> {
        match self.advance() {
            Some(Token::Integer(n)) => Ok(*n),
            Some(tok) => Err(FilterError::Parse(format!(
                "expected integer, got {:?}",
                tok.clone()
            ))),
            None => Err(FilterError::Parse("expected integer".to_string())),
        }
    }

    fn parse_optional_int(&mut self) -> Result<Option<i64>, FilterError> {
        match self.peek() {
            Some(Token::Integer(_)) => Ok(Some(self.parse_required_int()?)),
            _ => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Lift `base` and `next` into a Chain, but collapse Identity on the left so
/// that `.foo` becomes `Field("foo")` rather than `Chain(Identity, Field("foo"))`.
fn chain(base: Expr, next: Expr) -> Expr {
    if base == Expr::Identity {
        next
    } else {
        Expr::Chain(Box::new(base), Box::new(next))
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn parse(input: &str) -> Result<Expr, FilterError> {
    let tokens = Lexer::new(input).tokenize()?;
    if tokens.is_empty() {
        return Ok(Expr::Identity);
    }
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    if parser.pos != parser.tokens.len() {
        return Err(FilterError::Parse(format!(
            "unexpected token {:?} after expression",
            parser.tokens[parser.pos]
        )));
    }
    Ok(expr)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Expr {
        parse(s).unwrap_or_else(|e| panic!("parse failed for {:?}: {}", s, e))
    }

    #[test]
    fn identity() {
        assert_eq!(p("."), Expr::Identity);
    }

    #[test]
    fn field() {
        assert_eq!(p(".foo"), Expr::Field("foo".into()));
    }

    #[test]
    fn chained_fields() {
        assert_eq!(
            p(".foo.bar"),
            Expr::Chain(
                Box::new(Expr::Field("foo".into())),
                Box::new(Expr::Field("bar".into()))
            )
        );
    }

    #[test]
    fn index() {
        assert_eq!(p(".[0]"), Expr::Index(0));
        assert_eq!(p(".[-1]"), Expr::Index(-1));
    }

    #[test]
    fn slice() {
        assert_eq!(p(".[0:5]"), Expr::Slice(Some(0), Some(5)));
        assert_eq!(p(".[2:]"), Expr::Slice(Some(2), None));
        assert_eq!(p(".[:3]"), Expr::Slice(None, Some(3)));
    }

    #[test]
    fn iterate() {
        assert_eq!(p(".[]"), Expr::Iterate);
    }

    #[test]
    fn field_then_iterate() {
        assert_eq!(
            p(".items[]"),
            Expr::Chain(
                Box::new(Expr::Field("items".into())),
                Box::new(Expr::Iterate)
            )
        );
    }

    #[test]
    fn pipe_to_builtin() {
        assert_eq!(
            p(".users | length"),
            Expr::Pipe(
                Box::new(Expr::Field("users".into())),
                Box::new(Expr::Builtin(BuiltinFn::Length))
            )
        );
    }

    #[test]
    fn all_builtins() {
        assert_eq!(
            p(". | length"),
            Expr::Pipe(
                Box::new(Expr::Identity),
                Box::new(Expr::Builtin(BuiltinFn::Length))
            )
        );
        assert_eq!(
            p(". | keys"),
            Expr::Pipe(
                Box::new(Expr::Identity),
                Box::new(Expr::Builtin(BuiltinFn::Keys))
            )
        );
        assert_eq!(
            p(". | values"),
            Expr::Pipe(
                Box::new(Expr::Identity),
                Box::new(Expr::Builtin(BuiltinFn::Values))
            )
        );
        assert_eq!(
            p(". | type"),
            Expr::Pipe(
                Box::new(Expr::Identity),
                Box::new(Expr::Builtin(BuiltinFn::Type))
            )
        );
        assert_eq!(
            p(". | flatten"),
            Expr::Pipe(
                Box::new(Expr::Identity),
                Box::new(Expr::Builtin(BuiltinFn::Flatten))
            )
        );
    }

    #[test]
    fn identity_pipe_length() {
        let expr = p(". | length");
        assert_eq!(
            expr,
            Expr::Pipe(
                Box::new(Expr::Identity),
                Box::new(Expr::Builtin(BuiltinFn::Length))
            )
        );
    }

    #[test]
    fn field_index_chain() {
        // .users.[0]  →  Chain(Field("users"), Index(0))
        // also written without extra dot: .users[0]
        assert_eq!(
            p(".users[0]"),
            Expr::Chain(
                Box::new(Expr::Field("users".into())),
                Box::new(Expr::Index(0))
            )
        );
    }

    #[test]
    fn deep_chain() {
        // .a.b.c → Chain(Chain(Field("a"), Field("b")), Field("c"))
        let expr = p(".a.b.c");
        assert_eq!(
            expr,
            Expr::Chain(
                Box::new(Expr::Chain(
                    Box::new(Expr::Field("a".into())),
                    Box::new(Expr::Field("b".into()))
                )),
                Box::new(Expr::Field("c".into()))
            )
        );
    }

    #[test]
    fn empty_input_is_identity() {
        assert_eq!(parse("").unwrap(), Expr::Identity);
    }

    #[test]
    fn error_on_invalid() {
        assert!(parse("@foo").is_err());
    }

    #[test]
    fn negative_index() {
        assert_eq!(p(".[-1]"), Expr::Index(-1));
    }
}
