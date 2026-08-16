use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    expr::{parse, Expr},
    lexer::{lex_line, Token, TokenKind},
    Diagnostic, Span,
};

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub enum Operand {
    None,
    Accumulator,
    Expr(Expr),
    Immediate(Expr),
    Indirect(Expr),
    IndexedIndirect(Expr),
    IndirectIndexed(Expr),
    Indexed(Expr, IndexRegister),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexRegister {
    X,
    Y,
}

#[derive(Clone, Debug)]
pub enum StatementKind {
    Instruction {
        mnemonic: String,
        operand: Operand,
    },
    Label(String),
    Assignment {
        name: String,
        value: Expr,
    },
    Directive {
        name: String,
        args: Vec<DirectiveArg>,
    },
    Conditional {
        condition: Expr,
        then_body: Vec<Statement>,
        else_body: Vec<Statement>,
    },
}
#[derive(Clone, Debug)]
pub enum DirectiveArg {
    Expr(Expr),
    String(String),
    Bytes(Vec<u8>),
}
#[derive(Clone, Debug)]
pub struct Statement {
    pub span: Span,
    pub kind: StatementKind,
}

pub struct Parser {
    include_stack: Vec<PathBuf>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            include_stack: Vec::new(),
        }
    }
    pub fn parse_str(
        &mut self,
        name: impl Into<String>,
        text: &str,
    ) -> Result<Vec<Statement>, Diagnostic> {
        let source = SourceFile {
            name: name.into(),
            text: text.to_string(),
        };
        self.parse_source(&source, None)
    }
    pub fn parse_file(&mut self, path: impl AsRef<Path>) -> Result<Vec<Statement>, Diagnostic> {
        let requested = path.as_ref().to_path_buf();
        let path = path.as_ref().canonicalize().map_err(|e| {
            Diagnostic::new(
                Span::new(requested.to_string_lossy(), 1, 1, 1),
                format!("cannot read source: {e}"),
            )
        })?;
        if self.include_stack.contains(&path) {
            return Err(Diagnostic::new(
                Span::new(path.to_string_lossy(), 1, 1, 1),
                "recursive .include",
            ));
        }
        let text = fs::read_to_string(&path).map_err(|e| {
            Diagnostic::new(
                Span::new(path.to_string_lossy(), 1, 1, 1),
                format!("cannot read source: {e}"),
            )
        })?;
        self.include_stack.push(path.clone());
        let result = self.parse_source(
            &SourceFile {
                name: path.to_string_lossy().into_owned(),
                text,
            },
            path.parent(),
        );
        self.include_stack.pop();
        result
    }
    fn parse_source(
        &mut self,
        source: &SourceFile,
        base: Option<&Path>,
    ) -> Result<Vec<Statement>, Diagnostic> {
        let lines: Vec<&str> = source.text.lines().collect();
        let mut index = 0;
        self.parse_block(source, base, &lines, &mut index, false)
    }
    fn parse_block(
        &mut self,
        source: &SourceFile,
        base: Option<&Path>,
        lines: &[&str],
        index: &mut usize,
        nested: bool,
    ) -> Result<Vec<Statement>, Diagnostic> {
        let mut statements = Vec::new();
        while *index < lines.len() {
            let line_no = *index + 1;
            let tokens = lex_line(&source.name, line_no, lines[*index])?;
            *index += 1;
            if tokens.is_empty() {
                continue;
            }
            let first_ident = match &tokens[0].kind {
                TokenKind::Ident(v) => Some(v.as_str()),
                _ => None,
            };
            if let Some(name) = first_ident {
                if name.eq_ignore_ascii_case(".else") || name.eq_ignore_ascii_case(".endif") {
                    if !nested {
                        return Err(Diagnostic::new(
                            tokens[0].span.clone(),
                            format!("unexpected `{name}`"),
                        ));
                    }
                    *index -= 1;
                    break;
                }
            }
            let (span, mut rest) = (tokens[0].span.clone(), tokens.as_slice());
            if rest.len() >= 2 && matches!(rest[1].kind, TokenKind::Punct(':')) {
                if let TokenKind::Ident(label) = &rest[0].kind {
                    statements.push(Statement {
                        span: rest[0].span.clone(),
                        kind: StatementKind::Label(label.clone()),
                    });
                    rest = &rest[2..];
                }
            } else if rest.len() == 1
                && matches!(rest[0].kind, TokenKind::Ident(_))
                && !first_ident.unwrap_or("").starts_with('.')
            {
                if let TokenKind::Ident(label) = &rest[0].kind {
                    statements.push(Statement {
                        span,
                        kind: StatementKind::Label(label.clone()),
                    });
                    continue;
                }
            }
            if rest.is_empty() {
                continue;
            }
            let name = match &rest[0].kind {
                TokenKind::Ident(v) => v.clone(),
                _ => {
                    return Err(Diagnostic::new(
                        rest[0].span.clone(),
                        "expected instruction, directive, or label",
                    ))
                }
            };
            if rest.len() >= 2 && matches!(rest[1].kind, TokenKind::Punct('=')) {
                let value = parse(&rest[2..]).map_err(|e| {
                    if e.span.file.is_empty() {
                        Diagnostic::new(rest[0].span.clone(), e.message)
                    } else {
                        e
                    }
                })?;
                statements.push(Statement {
                    span,
                    kind: StatementKind::Assignment { name, value },
                });
                continue;
            }
            if name.eq_ignore_ascii_case(".if") {
                let condition = parse(&rest[1..])?;
                let then_body = self.parse_block(source, base, lines, index, true)?;
                let mut else_body = Vec::new();
                if *index < lines.len() {
                    let peek = lex_line(&source.name, *index + 1, lines[*index])?;
                    if peek.first().is_some_and(|t| matches!(&t.kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(".else"))) { *index += 1; else_body = self.parse_block(source, base, lines, index, true)?; }
                }
                if *index >= lines.len() {
                    return Err(Diagnostic::new(span, "missing `.endif`"));
                }
                let end = lex_line(&source.name, *index + 1, lines[*index])?;
                if !end.first().is_some_and(
                    |t| matches!(&t.kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(".endif")),
                ) {
                    return Err(Diagnostic::new(span, "missing `.endif`"));
                }
                *index += 1;
                statements.push(Statement {
                    span,
                    kind: StatementKind::Conditional {
                        condition,
                        then_body,
                        else_body,
                    },
                });
                continue;
            }
            if name.eq_ignore_ascii_case(".else") || name.eq_ignore_ascii_case(".endif") {
                return Err(Diagnostic::new(span, "unexpected conditional terminator"));
            }
            if name.eq_ignore_ascii_case(".include") {
                let path = one_string(&rest[1..], span)?;
                let full = base.unwrap_or(Path::new(".")).join(path);
                statements.extend(self.parse_file(full)?);
                continue;
            }
            let kind = if name.starts_with('.') {
                let directive_name = name.to_ascii_lowercase();
                let mut args = directive_args(&rest[1..])?;
                if directive_name == ".incbin" {
                    if let Some(DirectiveArg::String(path)) = args.first_mut() {
                        let resolved = base.unwrap_or(Path::new(".")).join(&*path);
                        *path = resolved.to_string_lossy().into_owned();
                    }
                }
                // ca65-style tile blocks put one quoted row on each following
                // line. Consume those rows as arguments of the directive so
                // they do not get parsed as standalone statements.
                if directive_name == ".tile2bpp" && args.is_empty() {
                    while *index < lines.len() && args.len() < 8 {
                        let row_tokens = lex_line(&source.name, *index + 1, lines[*index])?;
                        if row_tokens.is_empty() {
                            *index += 1;
                            continue;
                        }
                        if row_tokens.len() != 1 {
                            break;
                        }
                        if let TokenKind::String(row) = &row_tokens[0].kind {
                            args.push(DirectiveArg::String(row.clone()));
                            *index += 1;
                        } else {
                            break;
                        }
                    }
                }
                StatementKind::Directive {
                    name: directive_name,
                    args,
                }
            } else {
                StatementKind::Instruction {
                    mnemonic: name.to_ascii_uppercase(),
                    operand: operand(&rest[1..])?,
                }
            };
            statements.push(Statement { span, kind });
        }
        if nested && *index < lines.len() {
            let tokens = lex_line(&source.name, *index + 1, lines[*index])?;
            if tokens.first().is_some_and(|t| matches!(&t.kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(".endif") || s.eq_ignore_ascii_case(".else"))) { return Ok(statements); }
        }
        Ok(statements)
    }
}

fn one_string(tokens: &[Token], span: Span) -> Result<String, Diagnostic> {
    match tokens {
        [Token {
            kind: TokenKind::String(s),
            ..
        }] => Ok(s.clone()),
        _ => Err(Diagnostic::new(span, "expected a quoted path")),
    }
}
fn directive_args(tokens: &[Token]) -> Result<Vec<DirectiveArg>, Diagnostic> {
    let mut args = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for i in 0..=tokens.len() {
        let split =
            i == tokens.len() || (depth == 0 && matches!(tokens[i].kind, TokenKind::Punct(',')));
        if split {
            if start < i {
                let part = &tokens[start..i];
                if part.len() == 1 {
                    match &part[0].kind {
                        TokenKind::String(s) => args.push(DirectiveArg::String(s.clone())),
                        TokenKind::Char(v) => args.push(DirectiveArg::Expr(Expr::Value(*v as i64))),
                        _ => args.push(DirectiveArg::Expr(parse(part)?)),
                    }
                } else {
                    args.push(DirectiveArg::Expr(parse(part)?));
                }
            }
            start = i + 1;
        } else if matches!(tokens[i].kind, TokenKind::Punct('(')) {
            depth += 1;
        } else if matches!(tokens[i].kind, TokenKind::Punct(')')) {
            depth -= 1;
        }
    }
    Ok(args)
}
fn operand(tokens: &[Token]) -> Result<Operand, Diagnostic> {
    if tokens.is_empty() {
        return Ok(Operand::None);
    }
    if tokens.len() == 1
        && matches!(tokens[0].kind, TokenKind::Ident(ref s) if s.eq_ignore_ascii_case("a"))
    {
        return Ok(Operand::Accumulator);
    }
    if matches!(tokens[0].kind, TokenKind::Operator(ref s) if s == "#") {
        if tokens.len() == 1 {
            return Err(Diagnostic::new(
                tokens[0].span.clone(),
                "expected immediate expression",
            ));
        }
        return Ok(Operand::Immediate(parse(&tokens[1..])?));
    }
    if matches!(tokens.first().map(|t| &t.kind), Some(TokenKind::Punct('('))) {
        // (expr,X), (expr),Y, and (expr) are deliberately recognized by
        // their complete punctuation shape.  This avoids accidentally
        // treating the parentheses as part of an absolute expression.
        if tokens.len() >= 5
            && matches!(
                tokens.last(),
                Some(Token {
                    kind: TokenKind::Punct(')'),
                    ..
                })
            )
            && matches!(tokens.get(tokens.len() - 2), Some(Token { kind: TokenKind::Ident(s), .. }) if s.eq_ignore_ascii_case("x"))
            && matches!(
                tokens.get(tokens.len() - 3),
                Some(Token {
                    kind: TokenKind::Punct(','),
                    ..
                })
            )
        {
            return Ok(Operand::IndexedIndirect(parse(
                &tokens[1..tokens.len() - 3],
            )?));
        }
        if tokens.len() >= 5
            && matches!(tokens.get(tokens.len() - 1), Some(Token { kind: TokenKind::Ident(s), .. }) if s.eq_ignore_ascii_case("y"))
            && matches!(
                tokens.get(tokens.len() - 2),
                Some(Token {
                    kind: TokenKind::Punct(','),
                    ..
                })
            )
            && matches!(
                tokens.get(tokens.len() - 3),
                Some(Token {
                    kind: TokenKind::Punct(')'),
                    ..
                })
            )
        {
            return Ok(Operand::IndirectIndexed(parse(
                &tokens[1..tokens.len() - 3],
            )?));
        }
        if tokens.len() >= 3
            && matches!(
                tokens.last(),
                Some(Token {
                    kind: TokenKind::Punct(')'),
                    ..
                })
            )
        {
            return Ok(Operand::Indirect(parse(&tokens[1..tokens.len() - 1])?));
        }
    }
    if tokens.len() >= 3 && matches!(tokens[tokens.len() - 2].kind, TokenKind::Punct(',')) {
        let reg = match &tokens[tokens.len() - 1].kind {
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("x") => IndexRegister::X,
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("y") => IndexRegister::Y,
            _ => {
                return Err(Diagnostic::new(
                    tokens[tokens.len() - 1].span.clone(),
                    "expected X or Y index",
                ))
            }
        };
        return Ok(Operand::Indexed(parse(&tokens[..tokens.len() - 2])?, reg));
    }
    Ok(Operand::Expr(parse(tokens)?))
}
