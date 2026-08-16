//! Source lexer.  The parser deliberately keeps this lexer small: assembly
//! punctuation is represented as single-character tokens and expressions are
//! parsed by [`crate::expr`] from the same token stream.

use crate::{Diagnostic, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(i64),
    String(String),
    Char(u8),
    Punct(char),
    Operator(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

fn number(text: &str) -> Option<i64> {
    let (radix, digits) = if let Some(v) = text.strip_prefix('$') {
        (16, v)
    } else if let Some(v) = text.strip_prefix('%') {
        (2, v)
    } else if let Some(v) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        (16, v)
    } else if let Some(v) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        (2, v)
    } else {
        (10, text)
    };
    i64::from_str_radix(digits.replace('_', "").as_str(), radix).ok()
}

pub fn lex_line(file: &str, line: usize, text: &str) -> Result<Vec<Token>, Diagnostic> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i] == b';' {
            break;
        }
        let start = i;
        let ch = bytes[i] as char;
        if ch == '"' {
            i += 1;
            let mut value = String::new();
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    value.push(match bytes[i] as char {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '\\' => '\\',
                        '"' => '"',
                        other => other,
                    });
                } else {
                    value.push(bytes[i] as char);
                }
                i += 1;
            }
            if i >= bytes.len() {
                return Err(Diagnostic::new(
                    Span::new(file, line, start + 1, 1),
                    "unterminated string",
                ));
            }
            i += 1;
            out.push(Token {
                kind: TokenKind::String(value),
                span: Span::new(file, line, start + 1, i - start),
            });
            continue;
        }
        if ch == '\'' {
            i += 1;
            if i >= bytes.len() {
                return Err(Diagnostic::new(
                    Span::new(file, line, start + 1, 1),
                    "unterminated character",
                ));
            }
            let value = if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 1;
                bytes[i]
            } else {
                bytes[i]
            };
            i += 1;
            if i >= bytes.len() || bytes[i] != b'\'' {
                return Err(Diagnostic::new(
                    Span::new(file, line, start + 1, i - start),
                    "character literal must contain one byte",
                ));
            }
            i += 1;
            out.push(Token {
                kind: TokenKind::Char(value),
                span: Span::new(file, line, start + 1, i - start),
            });
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '.' {
            i += 1;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b'.')
            {
                i += 1;
            }
            let value = &text[start..i];
            out.push(Token {
                kind: TokenKind::Ident(value.to_string()),
                span: Span::new(file, line, start + 1, i - start),
            });
            continue;
        }
        // `%` is both ca65's binary-literal prefix and the modulo operator.
        // Treat it as a number only when it is followed by a binary digit
        // (or an underscore in a separated literal); otherwise it remains an
        // expression operator.
        let binary_literal =
            ch == '%' && i + 1 < bytes.len() && (matches!(bytes[i + 1], b'0' | b'1' | b'_'));
        if ch.is_ascii_digit() || ch == '$' || binary_literal {
            i += 1;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_hexdigit()
                    || bytes[i] == b'_'
                    || bytes[i] == b'x'
                    || bytes[i] == b'X'
                    || bytes[i] == b'b'
                    || bytes[i] == b'B')
            {
                i += 1;
            }
            let value = &text[start..i];
            let parsed = number(value).ok_or_else(|| {
                Diagnostic::new(
                    Span::new(file, line, start + 1, i - start),
                    format!("invalid number `{value}`"),
                )
            })?;
            out.push(Token {
                kind: TokenKind::Number(parsed),
                span: Span::new(file, line, start + 1, i - start),
            });
            continue;
        }
        let two = if i + 1 < bytes.len() {
            &text[i..i + 2]
        } else {
            ""
        };
        if matches!(two, "<<" | ">>" | "==" | "!=" | "<=" | ">=" | "&&" | "||") {
            i += 2;
            out.push(Token {
                kind: TokenKind::Operator(two.to_string()),
                span: Span::new(file, line, start + 1, 2),
            });
        } else if "+-*/%&|^~!<>#(),:[]=\\".contains(ch) {
            i += 1;
            let kind = if "+-*/%&|^~!<>#".contains(ch) {
                TokenKind::Operator(ch.to_string())
            } else {
                TokenKind::Punct(ch)
            };
            out.push(Token {
                kind,
                span: Span::new(file, line, start + 1, 1),
            });
        } else {
            return Err(Diagnostic::new(
                Span::new(file, line, start + 1, 1),
                format!("unexpected character `{ch}`"),
            ));
        }
    }
    Ok(out)
}
