use super::FilterError;

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Identity,
    Field(String),
    Index(i64),
    Slice(Option<i64>, Option<i64>),
    Iterate,
    Pipe(Box<Expr>, Box<Expr>),
    Chain(Box<Expr>, Box<Expr>),
    Builtin(BuiltinFn),
    // Literals
    StringLit(String),
    NumberLit(f64),
    BoolLit(bool),
    NullLit,
    // Comparison
    Compare(Box<Expr>, CmpOp, Box<Expr>),
    // Boolean logic
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    // Arithmetic
    Arith(Box<Expr>, ArithOp, Box<Expr>),
    // Higher-order
    Select(Box<Expr>),
    Map(Box<Expr>),
    SortBy(Box<Expr>),
    // Parenthesized (transparent — just for grouping)
    Paren(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinFn {
    Length,
    Keys,
    Values,
    Type,
    Flatten,
    First,
    Last,
    Reverse,
    Unique,
    Sort,
    Min,
    Max,
    Not,
    ToNumber,
    ToString,
    AsciiDowncase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
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
    LParen,
    RParen,
    Colon,
    Comma,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    // Values
    Ident(String),
    Integer(i64),
    Float(f64),
    Str(String),
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

    fn read_ident(&mut self) -> String {
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

    fn read_number(&mut self) -> Result<Token, FilterError> {
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
        if self.peek() == Some('.') {
            // Check it's a float, not a dot operator
            let next = self.input[self.pos + 1..].chars().next();
            if matches!(next, Some(c) if c.is_ascii_digit()) {
                s.push('.');
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        s.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }
                let f: f64 = s
                    .parse()
                    .map_err(|_| FilterError::Parse(format!("invalid float: {s}")))?;
                return Ok(Token::Float(f));
            }
        }
        let n: i64 = s
            .parse()
            .map_err(|_| FilterError::Parse(format!("invalid integer: {s}")))?;
        Ok(Token::Integer(n))
    }

    fn read_string(&mut self) -> Result<String, FilterError> {
        self.advance(); // consume opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => return Ok(s),
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => return Err(FilterError::Parse("unterminated string".into())),
                },
                Some(c) => s.push(c),
                None => return Err(FilterError::Parse("unterminated string".into())),
            }
        }
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
                Some('(') => {
                    self.advance();
                    tokens.push(Token::LParen);
                }
                Some(')') => {
                    self.advance();
                    tokens.push(Token::RParen);
                }
                Some(',') => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                Some(':') => {
                    self.advance();
                    tokens.push(Token::Colon);
                }
                Some('=') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Eq);
                    } else {
                        return Err(FilterError::Parse("expected '==' not '='".into()));
                    }
                }
                Some('!') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Ne);
                    } else {
                        return Err(FilterError::Parse("expected '!=' not '!'".into()));
                    }
                }
                Some('<') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Le);
                    } else {
                        tokens.push(Token::Lt);
                    }
                }
                Some('>') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Ge);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                Some('+') => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                Some('*') => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                Some('/') => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                Some('"') => {
                    let s = self.read_string()?;
                    tokens.push(Token::Str(s));
                }
                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    let ident = self.read_ident();
                    tokens.push(Token::Ident(ident));
                }
                Some(ch) if ch.is_ascii_digit() => {
                    tokens.push(self.read_number()?);
                }
                Some('-') => {
                    // Could be negative number or minus operator
                    // If previous token is a value-producing token, it's minus
                    let is_unary = tokens.is_empty()
                        || matches!(
                            tokens.last(),
                            Some(
                                Token::Pipe
                                    | Token::LParen
                                    | Token::LBracket
                                    | Token::Comma
                                    | Token::Eq
                                    | Token::Ne
                                    | Token::Lt
                                    | Token::Le
                                    | Token::Gt
                                    | Token::Ge
                                    | Token::Plus
                                    | Token::Minus
                                    | Token::Star
                                    | Token::Slash
                            )
                        );
                    if is_unary {
                        tokens.push(self.read_number()?);
                    } else {
                        self.advance();
                        tokens.push(Token::Minus);
                    }
                }
                Some(ch) => {
                    return Err(FilterError::Parse(format!("unexpected character: {ch:?}")));
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
                "expected {expected:?}, got {tok:?}"
            ))),
            None => Err(FilterError::Parse(format!(
                "expected {expected:?}, got end of input"
            ))),
        }
    }

    // -------------------------------------------------------------------
    // Grammar (lowest to highest precedence):
    //
    //   expr          := pipe_expr
    //   pipe_expr     := or_expr ( '|' or_expr )*
    //   or_expr       := and_expr ( 'or' and_expr )*
    //   and_expr      := compare_expr ( 'and' compare_expr )*
    //   compare_expr  := add_expr ( cmp_op add_expr )?
    //   add_expr      := mul_expr ( ('+' | '-') mul_expr )*
    //   mul_expr      := unary_expr ( ('*' | '/') unary_expr )*
    //   unary_expr    := 'not' unary_expr | postfix_expr
    //   postfix_expr  := primary ( '.' ident | '[' ... ']' )*
    //   primary       := '.' chain? | '(' expr ')' | literal | builtin_call | ident
    //   builtin_call  := ident '(' expr ')'
    //   literal       := string | number | 'true' | 'false' | 'null'
    // -------------------------------------------------------------------

    fn parse_expr(&mut self) -> Result<Expr, FilterError> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_or()?;
        while self.peek() == Some(&Token::Pipe) {
            self.advance();
            let right = self.parse_or()?;
            left = Expr::Pipe(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Ident(s)) if s == "or") {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_compare()?;
        while matches!(self.peek(), Some(Token::Ident(s)) if s == "and") {
            self.advance();
            let right = self.parse_compare()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_compare(&mut self) -> Result<Expr, FilterError> {
        let left = self.parse_add()?;
        let op = match self.peek() {
            Some(Token::Eq) => Some(CmpOp::Eq),
            Some(Token::Ne) => Some(CmpOp::Ne),
            Some(Token::Lt) => Some(CmpOp::Lt),
            Some(Token::Le) => Some(CmpOp::Le),
            Some(Token::Gt) => Some(CmpOp::Gt),
            Some(Token::Ge) => Some(CmpOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_add()?;
            Ok(Expr::Compare(Box::new(left), op, Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_add(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => ArithOp::Add,
                Some(Token::Minus) => ArithOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_mul()?;
            left = Expr::Arith(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star) => ArithOp::Mul,
                Some(Token::Slash) => ArithOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Arith(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, FilterError> {
        if matches!(self.peek(), Some(Token::Ident(s)) if s == "not") {
            self.advance();
            let inner = self.parse_unary()?;
            Ok(Expr::Not(Box::new(inner)))
        } else {
            self.parse_postfix()
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, FilterError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Some(Token::Dot) => {
                    self.advance();
                    if let Some(Token::Ident(_)) = self.peek() {
                        let name = match self.advance() {
                            Some(Token::Ident(n)) => n.clone(),
                            _ => unreachable!(),
                        };
                        expr = Expr::Chain(Box::new(expr), Box::new(Expr::Field(name)));
                    } else if self.peek() == Some(&Token::LBracket) {
                        self.advance();
                        let inner = self.parse_index_or_slice()?;
                        self.expect(&Token::RBracket)?;
                        expr = Expr::Chain(Box::new(expr), Box::new(inner));
                    }
                }
                Some(Token::LBracket) => {
                    self.advance();
                    let inner = self.parse_index_or_slice()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::Chain(Box::new(expr), Box::new(inner));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, FilterError> {
        match self.peek() {
            Some(Token::Dot) => {
                self.advance();
                match self.peek() {
                    Some(Token::Ident(_)) => {
                        let name = match self.advance() {
                            Some(Token::Ident(n)) => n.clone(),
                            _ => unreachable!(),
                        };
                        Ok(Expr::Field(name))
                    }
                    Some(Token::LBracket) => {
                        self.advance();
                        let inner = self.parse_index_or_slice()?;
                        self.expect(&Token::RBracket)?;
                        Ok(inner)
                    }
                    _ => Ok(Expr::Identity),
                }
            }
            Some(Token::LParen) => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Paren(Box::new(inner)))
            }
            Some(Token::Str(_)) => {
                let s = match self.advance() {
                    Some(Token::Str(s)) => s.clone(),
                    _ => unreachable!(),
                };
                Ok(Expr::StringLit(s))
            }
            Some(Token::Integer(n)) => {
                let n = *n;
                self.advance();
                Ok(Expr::NumberLit(n as f64))
            }
            Some(Token::Float(f)) => {
                let f = *f;
                self.advance();
                Ok(Expr::NumberLit(f))
            }
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "true" => Ok(Expr::BoolLit(true)),
                    "false" => Ok(Expr::BoolLit(false)),
                    "null" => Ok(Expr::NullLit),
                    // Higher-order builtins with argument
                    "select" | "map" | "sort_by" => {
                        self.expect(&Token::LParen)?;
                        let arg = self.parse_expr()?;
                        self.expect(&Token::RParen)?;
                        match name.as_str() {
                            "select" => Ok(Expr::Select(Box::new(arg))),
                            "map" => Ok(Expr::Map(Box::new(arg))),
                            "sort_by" => Ok(Expr::SortBy(Box::new(arg))),
                            _ => unreachable!(),
                        }
                    }
                    // No-arg builtins
                    _ => {
                        if let Some(b) = parse_builtin(&name) {
                            Ok(Expr::Builtin(b))
                        } else {
                            Err(FilterError::Parse(format!(
                                "unknown function '{name}'; did you mean '.{name}'?"
                            )))
                        }
                    }
                }
            }
            Some(tok) => Err(FilterError::Parse(format!("unexpected token {tok:?}"))),
            None => Err(FilterError::Parse("unexpected end of input".into())),
        }
    }

    fn parse_index_or_slice(&mut self) -> Result<Expr, FilterError> {
        match self.peek() {
            Some(Token::RBracket) => Ok(Expr::Iterate),
            Some(Token::Colon) => {
                self.advance();
                let upper = self.parse_optional_int()?;
                Ok(Expr::Slice(None, upper))
            }
            Some(Token::Integer(_)) => {
                let n = self.parse_required_int()?;
                if self.peek() == Some(&Token::Colon) {
                    self.advance();
                    let upper = self.parse_optional_int()?;
                    Ok(Expr::Slice(Some(n), upper))
                } else {
                    Ok(Expr::Index(n))
                }
            }
            Some(tok) => Err(FilterError::Parse(format!(
                "expected index or ']', got {tok:?}"
            ))),
            None => Err(FilterError::Parse(
                "unexpected end of input inside '[...]'".into(),
            )),
        }
    }

    fn parse_required_int(&mut self) -> Result<i64, FilterError> {
        match self.advance() {
            Some(Token::Integer(n)) => Ok(*n),
            Some(tok) => Err(FilterError::Parse(format!(
                "expected integer, got {tok:?}"
            ))),
            None => Err(FilterError::Parse("expected integer".into())),
        }
    }

    fn parse_optional_int(&mut self) -> Result<Option<i64>, FilterError> {
        if matches!(self.peek(), Some(Token::Integer(_))) {
            Ok(Some(self.parse_required_int()?))
        } else {
            Ok(None)
        }
    }
}

fn parse_builtin(name: &str) -> Option<BuiltinFn> {
    Some(match name {
        "length" => BuiltinFn::Length,
        "keys" => BuiltinFn::Keys,
        "values" => BuiltinFn::Values,
        "type" => BuiltinFn::Type,
        "flatten" => BuiltinFn::Flatten,
        "first" => BuiltinFn::First,
        "last" => BuiltinFn::Last,
        "reverse" => BuiltinFn::Reverse,
        "unique" => BuiltinFn::Unique,
        "sort" => BuiltinFn::Sort,
        "min" => BuiltinFn::Min,
        "max" => BuiltinFn::Max,
        "not" => BuiltinFn::Not,
        "to_number" => BuiltinFn::ToNumber,
        "to_string" => BuiltinFn::ToString,
        "ascii_downcase" => BuiltinFn::AsciiDowncase,
        _ => return None,
    })
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
        parse(s).unwrap_or_else(|e| panic!("parse failed for {s:?}: {e}"))
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
        assert!(matches!(p(". | length"), Expr::Pipe(_, _)));
        assert!(matches!(p(". | keys"), Expr::Pipe(_, _)));
        assert!(matches!(p(". | values"), Expr::Pipe(_, _)));
        assert!(matches!(p(". | type"), Expr::Pipe(_, _)));
        assert!(matches!(p(". | flatten"), Expr::Pipe(_, _)));
    }

    #[test]
    fn identity_pipe_length() {
        assert!(matches!(p(". | length"), Expr::Pipe(_, _)));
    }

    #[test]
    fn field_index_chain() {
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
        let expr = p(".a.b.c");
        assert!(matches!(expr, Expr::Chain(_, _)));
    }

    #[test]
    fn empty_input_is_identity() {
        assert_eq!(parse("").unwrap(), Expr::Identity);
    }

    #[test]
    fn error_on_invalid() {
        assert!(parse("@@foo").is_err());
    }

    #[test]
    fn negative_index() {
        assert_eq!(p(".[-1]"), Expr::Index(-1));
    }

    // New feature tests
    #[test]
    fn string_literal() {
        assert_eq!(p("\"hello\""), Expr::StringLit("hello".into()));
    }

    #[test]
    fn number_literal() {
        assert_eq!(p("42"), Expr::NumberLit(42.0));
        assert_eq!(p("3.14"), Expr::NumberLit(3.14));
    }

    #[test]
    fn bool_and_null_literals() {
        assert_eq!(p("true"), Expr::BoolLit(true));
        assert_eq!(p("false"), Expr::BoolLit(false));
        assert_eq!(p("null"), Expr::NullLit);
    }

    #[test]
    fn comparison() {
        assert!(matches!(p(".age > 30"), Expr::Compare(_, CmpOp::Gt, _)));
        assert!(matches!(p(".x == 1"), Expr::Compare(_, CmpOp::Eq, _)));
        assert!(matches!(p(".x != 1"), Expr::Compare(_, CmpOp::Ne, _)));
        assert!(matches!(p(".x <= 10"), Expr::Compare(_, CmpOp::Le, _)));
    }

    #[test]
    fn boolean_logic() {
        assert!(matches!(p(".a > 1 and .b < 5"), Expr::And(_, _)));
        assert!(matches!(p(".a > 1 or .b < 5"), Expr::Or(_, _)));
        assert!(matches!(p("not .active"), Expr::Not(_)));
    }

    #[test]
    fn arithmetic() {
        assert!(matches!(p(".price * .qty"), Expr::Arith(_, ArithOp::Mul, _)));
        assert!(matches!(p(".a + .b"), Expr::Arith(_, ArithOp::Add, _)));
    }

    #[test]
    fn select_expr() {
        assert!(matches!(p("select(.age > 30)"), Expr::Select(_)));
    }

    #[test]
    fn map_expr() {
        assert!(matches!(p("map(.name)"), Expr::Map(_)));
    }

    #[test]
    fn sort_by_expr() {
        assert!(matches!(p("sort_by(.age)"), Expr::SortBy(_)));
    }

    #[test]
    fn parenthesized() {
        assert!(matches!(p("(.a + .b) * 2"), Expr::Arith(_, ArithOp::Mul, _)));
    }

    #[test]
    fn complex_pipeline() {
        // .users[] | select(.age >= 30) | .name
        let expr = p(".users[] | select(.age >= 30) | .name");
        assert!(matches!(expr, Expr::Pipe(_, _)));
    }
}
