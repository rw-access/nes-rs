use std::collections::HashMap;

use crate::{
    lexer::{Token, TokenKind},
    Diagnostic, Span,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Value(i64),
    Symbol(String),
    Current,
    Unary {
        op: String,
        value: Box<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

impl Expr {
    pub fn symbol(&self) -> Option<&str> {
        match self {
            Expr::Symbol(s) => Some(s),
            Expr::Unary { value, .. } => value.symbol(),
            Expr::Binary { left, right, .. } => left.symbol().or_else(|| right.symbol()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EvalContext<'a> {
    pub symbols: &'a HashMap<String, i64>,
    pub constants: &'a HashMap<String, i64>,
    pub current: i64,
}

impl Expr {
    pub fn eval(&self, ctx: &EvalContext<'_>) -> Result<i64, String> {
        match self {
            Expr::Value(v) => Ok(*v),
            Expr::Current => Ok(ctx.current),
            Expr::Symbol(name) => {
                let canonical = name.to_ascii_uppercase();
                ctx.constants
                    .get(name)
                    .or_else(|| ctx.constants.get(&canonical))
                    .or_else(|| ctx.symbols.get(name))
                    .or_else(|| ctx.symbols.get(&canonical))
                    .copied()
                    .ok_or_else(|| format!("undefined symbol `{name}`"))
            }
            Expr::Unary { op, value } => {
                let v = value.eval(ctx)?;
                match op.as_str() {
                    "+" => Ok(v),
                    "-" => Ok(-v),
                    "~" => Ok(!v),
                    "!" => Ok((v == 0) as i64),
                    "<" => Ok(v & 0xff),
                    ">" => Ok((v >> 8) & 0xff),
                    _ => Err(format!("unknown unary operator `{op}`")),
                }
            }
            Expr::Binary { op, left, right } => {
                let a = left.eval(ctx)?;
                let b = right.eval(ctx)?;
                match op.as_str() {
                    "+" => Ok(a.wrapping_add(b)),
                    "-" => Ok(a.wrapping_sub(b)),
                    "*" => Ok(a.wrapping_mul(b)),
                    "/" => {
                        if b == 0 {
                            Err("division by zero".into())
                        } else {
                            Ok(a / b)
                        }
                    }
                    "%" => {
                        if b == 0 {
                            Err("modulo by zero".into())
                        } else {
                            Ok(a % b)
                        }
                    }
                    "<<" => Ok(a.wrapping_shl(b as u32)),
                    ">>" => Ok(a.wrapping_shr(b as u32)),
                    "&" => Ok(a & b),
                    "|" => Ok(a | b),
                    "^" => Ok(a ^ b),
                    "==" => Ok((a == b) as i64),
                    "!=" => Ok((a != b) as i64),
                    "<" => Ok((a < b) as i64),
                    "<=" => Ok((a <= b) as i64),
                    ">" => Ok((a > b) as i64),
                    ">=" => Ok((a >= b) as i64),
                    "&&" => Ok(((a != 0) && (b != 0)) as i64),
                    "||" => Ok(((a != 0) || (b != 0)) as i64),
                    _ => Err(format!("unknown binary operator `{op}`")),
                }
            }
        }
    }
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl<'a> ExprParser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, index: 0 }
    }
    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.index).map(|t| &t.kind)
    }
    fn take(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.index);
        self.index += usize::from(t.is_some());
        t
    }
    fn precedence(op: &str) -> u8 {
        match op {
            "||" => 1,
            "&&" => 2,
            "|" => 3,
            "^" => 4,
            "&" => 5,
            "==" | "!=" => 6,
            "<" | "<=" | ">" | ">=" => 7,
            "<<" | ">>" => 8,
            "+" | "-" => 9,
            "*" | "/" | "%" => 10,
            _ => 0,
        }
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self
            .take()
            .ok_or_else(|| Diagnostic::new(Span::default(), "expected expression"))?
            .clone();
        match token.kind {
            TokenKind::Number(v) => Ok(Expr::Value(v)),
            TokenKind::Char(v) => Ok(Expr::Value(v as i64)),
            TokenKind::Ident(s) => Ok(Expr::Symbol(s)),
            TokenKind::Operator(op) if op == "*" => Ok(Expr::Current),
            TokenKind::Operator(op) if matches!(op.as_str(), "+" | "-" | "~" | "!" | "<" | ">") => {
                Ok(Expr::Unary {
                    op,
                    value: Box::new(self.primary()?),
                })
            }
            TokenKind::Punct('(') => {
                let e = self.parse_bp(0)?;
                match self.take().map(|t| &t.kind) {
                    Some(TokenKind::Punct(')')) => Ok(e),
                    _ => Err(Diagnostic::new(token.span, "expected `)`")),
                }
            }
            _ => Err(Diagnostic::new(token.span, "expected expression")),
        }
    }
    fn parse_bp(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.primary()?;
        loop {
            let op = match self.peek() {
                Some(TokenKind::Operator(op))
                    if Self::precedence(op) >= min_bp && Self::precedence(op) != 0 =>
                {
                    op.clone()
                }
                _ => break,
            };
            let precedence = Self::precedence(&op);
            self.index += 1;
            let rhs = self.parse_bp(precedence + 1)?;
            lhs = Expr::Binary {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }
        Ok(lhs)
    }
}

pub fn parse(tokens: &[Token]) -> Result<Expr, Diagnostic> {
    if tokens.is_empty() {
        return Err(Diagnostic::new(Span::default(), "expected expression"));
    }
    let mut parser = ExprParser::new(tokens);
    let expr = parser.parse_bp(0)?;
    if parser.index != tokens.len() {
        return Err(Diagnostic::new(
            tokens[parser.index].span.clone(),
            "unexpected token in expression",
        ));
    }
    Ok(expr)
}
