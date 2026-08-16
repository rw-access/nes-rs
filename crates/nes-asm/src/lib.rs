//! A compact, intentionally limited ca65-like assembler for the official
//! NMOS 6502 instruction set.  The internal opcode boundary is kept in the
//! [`Isa`] trait so the planned workspace `nes-isa` crate can replace the
//! fallback table without changing parsing, layout, or object consumers.
mod diagnostics;
pub mod expr;
pub mod lexer;
pub mod parser;

pub use diagnostics::{Diagnostic, Span};
pub use parser::{DirectiveArg, IndexRegister, Operand, Parser, Statement, StatementKind};
use std::{collections::HashMap, fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressingMode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndexedIndirect,
    IndirectIndexed,
    Relative,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcodeInfo {
    pub byte: u8,
    pub mode: AddressingMode,
    pub width: u8,
    pub official: bool,
}
pub trait Isa {
    fn lookup(&self, mnemonic: &str, mode: AddressingMode) -> Option<OpcodeInfo>;
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Official6502;
impl Isa for Official6502 {
    fn lookup(&self, mnemonic: &str, mode: AddressingMode) -> Option<OpcodeInfo> {
        shared_opcode(mnemonic, mode).or_else(|| official_opcode(mnemonic, mode))
    }
}

fn shared_opcode(mnemonic: &str, mode: AddressingMode) -> Option<OpcodeInfo> {
    let isa_mode = match mode {
        AddressingMode::Implied => nes_isa::AddressingMode::Implied,
        AddressingMode::Accumulator => nes_isa::AddressingMode::Accumulator,
        AddressingMode::Immediate => nes_isa::AddressingMode::Immediate,
        AddressingMode::ZeroPage => nes_isa::AddressingMode::ZeroPage,
        AddressingMode::ZeroPageX => nes_isa::AddressingMode::ZeroPageIndexedX,
        AddressingMode::ZeroPageY => nes_isa::AddressingMode::ZeroPageIndexedY,
        AddressingMode::Absolute => nes_isa::AddressingMode::Absolute,
        AddressingMode::AbsoluteX => nes_isa::AddressingMode::AbsoluteIndexedX,
        AddressingMode::AbsoluteY => nes_isa::AddressingMode::AbsoluteIndexedY,
        AddressingMode::Indirect => nes_isa::AddressingMode::Indirect,
        AddressingMode::IndexedIndirect => nes_isa::AddressingMode::IndexedIndirect,
        AddressingMode::IndirectIndexed => nes_isa::AddressingMode::IndirectIndexed,
        AddressingMode::Relative => nes_isa::AddressingMode::Relative,
    };
    nes_isa::OPCODES
        .iter()
        .find(|entry| {
            entry.mnemonic.as_str() == mnemonic
                && entry.addressing_mode == isa_mode
                && entry.class == nes_isa::OpcodeClass::Official
        })
        .map(|entry| OpcodeInfo {
            byte: entry.byte,
            mode,
            width: entry.operand_width + 1,
            official: true,
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    Prg,
    Chr,
    Ram,
    Other,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub kind: SectionKind,
    pub vma: u16,
    pub alignment: u16,
    pub fill: u8,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelocationKind {
    Absolute8,
    Absolute16,
    Relative8,
    Low8,
    High8,
}
#[derive(Clone, Debug)]
pub struct Relocation {
    pub section: String,
    pub offset: u32,
    pub kind: RelocationKind,
    pub symbol: String,
    pub addend: i64,
}
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub value: u16,
    pub section: Option<String>,
    pub global: bool,
}
#[derive(Clone, Debug)]
pub struct SourceMapEntry {
    pub section: String,
    pub offset: u32,
    pub length: u32,
    pub span: Span,
}
#[derive(Clone, Debug, Default)]
pub struct ObjectFile {
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
    pub relocations: Vec<Relocation>,
    pub source_map: Vec<SourceMapEntry>,
}
impl ObjectFile {
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
    pub fn flat_binary(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for section in &self.sections {
            if section.kind == SectionKind::Ram {
                continue;
            }
            if out.is_empty() {
                out.resize(section.vma as usize, section.fill);
            }
            let end = section.vma as usize + section.data.len();
            if end > out.len() {
                out.resize(end, section.fill);
            }
            out[section.vma as usize..end].copy_from_slice(&section.data);
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct AssemblerOptions {
    pub allow_unofficial: bool,
    pub default_fill: u8,
    pub include_paths: Vec<std::path::PathBuf>,
}
impl Default for AssemblerOptions {
    fn default() -> Self {
        Self {
            allow_unofficial: false,
            default_fill: 0,
            include_paths: Vec::new(),
        }
    }
}
pub struct Assembler<I = Official6502> {
    pub isa: I,
    pub options: AssemblerOptions,
}
impl Default for Assembler<Official6502> {
    fn default() -> Self {
        Self {
            isa: Official6502,
            options: AssemblerOptions::default(),
        }
    }
}
impl<I: Isa + Copy> Assembler<I> {
    pub fn new(isa: I) -> Self {
        Self {
            isa,
            options: AssemblerOptions::default(),
        }
    }
    pub fn assemble_str(
        &self,
        name: impl Into<String>,
        source: &str,
    ) -> Result<ObjectFile, Diagnostic> {
        let mut parser = Parser::new();
        let statements = parser.parse_str(name, source)?;
        self.assemble_statements(&statements)
    }
    pub fn assemble_file(&self, path: impl AsRef<Path>) -> Result<ObjectFile, Diagnostic> {
        let mut parser = Parser::new();
        let statements = parser.parse_file(path)?;
        self.assemble_statements(&statements)
    }
    fn assemble_statements(&self, statements: &[Statement]) -> Result<ObjectFile, Diagnostic> {
        let mut constants = HashMap::new();
        collect_constants(statements, &mut constants)?;
        let mut prior = HashMap::new();
        let mut layout = Layout::default();
        for _ in 0..12 {
            layout = layout_statements(statements, &constants, &prior, &self.options, &self.isa)?;
            if layout.symbols == prior {
                break;
            }
            prior = layout.symbols.clone();
        }
        let layout = layout_statements(statements, &constants, &prior, &self.options, &self.isa)?;
        emit_object(statements, &constants, &layout, &self.options, &self.isa)
    }
}

#[derive(Default)]
struct Layout {
    symbols: HashMap<String, i64>,
    sections: Vec<SectionLayout>,
    locations: HashMap<usize, (usize, usize, usize)>,
    active: Vec<usize>,
}
#[derive(Clone)]
struct SectionLayout {
    name: String,
    kind: SectionKind,
    base: u16,
    end: u32,
    fill: u8,
    alignment: u16,
}
fn collect_constants(
    stmts: &[Statement],
    constants: &mut HashMap<String, i64>,
) -> Result<(), Diagnostic> {
    for stmt in stmts {
        match &stmt.kind {
            StatementKind::Assignment { name, value } => {
                let ctx = expr::EvalContext {
                    symbols: &HashMap::new(),
                    constants,
                    current: 0,
                };
                let v = value
                    .eval(&ctx)
                    .map_err(|e| Diagnostic::new(stmt.span.clone(), e))?;
                constants.insert(name.to_ascii_uppercase(), v);
            }
            StatementKind::Conditional {
                then_body,
                else_body,
                ..
            } => {
                collect_constants(then_body, constants)?;
                collect_constants(else_body, constants)?;
            }
            _ => {}
        }
    }
    Ok(())
}
fn context<'a>(
    symbols: &'a HashMap<String, i64>,
    constants: &'a HashMap<String, i64>,
    current: i64,
) -> expr::EvalContext<'a> {
    expr::EvalContext {
        symbols,
        constants,
        current,
    }
}

fn section_kind(name: &str) -> SectionKind {
    match name.to_ascii_uppercase().as_str() {
        "CHR" | "CHARS" => SectionKind::Chr,
        "BSS" | "RAM" | "ZEROPAGE" => SectionKind::Ram,
        "PRG" | "CODE" | "RODATA" | "VECTORS" => SectionKind::Prg,
        _ => SectionKind::Other,
    }
}
fn active<'a>(
    stmts: &'a [Statement],
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    out: &mut Vec<(usize, &'a Statement)>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StatementKind::Conditional {
                condition,
                then_body,
                else_body,
            } => {
                let yes = condition.eval(&context(symbols, constants, 0)).unwrap_or(0) != 0;
                active(
                    if yes { then_body } else { else_body },
                    symbols,
                    constants,
                    out,
                );
            }
            _ => out.push((out.len(), stmt)),
        }
    }
}
fn layout_statements<I: Isa>(
    statements: &[Statement],
    constants: &HashMap<String, i64>,
    symbols: &HashMap<String, i64>,
    options: &AssemblerOptions,
    isa: &I,
) -> Result<Layout, Diagnostic> {
    let mut flat = Vec::new();
    active(statements, symbols, constants, &mut flat);
    let mut layout = Layout::default();
    let mut current_section = 0usize;
    layout.sections.push(SectionLayout {
        name: "DEFAULT".into(),
        kind: SectionKind::Prg,
        base: 0,
        end: 0,
        fill: options.default_fill,
        alignment: 1,
    });
    for (idx, stmt) in flat {
        let sec = &mut layout.sections[current_section];
        let cursor = sec.end + sec.base as u32;
        layout
            .locations
            .insert(idx, (current_section, cursor as usize, stmt.span.line));
        match &stmt.kind {
            StatementKind::Label(name) => {
                layout
                    .symbols
                    .insert(name.to_ascii_uppercase(), cursor as i64);
            }
            StatementKind::Assignment { .. } => {}
            StatementKind::Directive { name, args } => match name.as_str() {
                ".segment" | ".section" => {
                    let section = match args.first() {
                        Some(DirectiveArg::String(s)) => s.clone(),
                        _ => {
                            return Err(Diagnostic::new(
                                stmt.span.clone(),
                                "expected quoted section name",
                            ))
                        }
                    };
                    if let Some(pos) = layout
                        .sections
                        .iter()
                        .position(|s| s.name.eq_ignore_ascii_case(&section))
                    {
                        current_section = pos;
                    } else {
                        layout.sections.push(SectionLayout {
                            name: section.clone(),
                            kind: section_kind(&section),
                            base: 0,
                            end: 0,
                            fill: options.default_fill,
                            alignment: 1,
                        });
                        current_section = layout.sections.len() - 1;
                    }
                }
                ".org" => {
                    let v =
                        eval_arg(args, 0, &layout.symbols, constants, cursor as i64, stmt)? as u32;
                    if sec.end == 0 {
                        sec.base = v as u16;
                    } else if v < cursor {
                        return Err(Diagnostic::new(
                            stmt.span.clone(),
                            ".org cannot move backwards",
                        ));
                    } else {
                        sec.end = v - sec.base as u32;
                    }
                }
                ".align" => {
                    let n =
                        eval_arg(args, 0, &layout.symbols, constants, cursor as i64, stmt)? as u32;
                    if n == 0 || !n.is_power_of_two() {
                        return Err(Diagnostic::new(
                            stmt.span.clone(),
                            "alignment must be a non-zero power of two",
                        ));
                    }
                    let aligned = (cursor + n - 1) & !(n - 1);
                    sec.end = aligned - sec.base as u32;
                }
                ".byte" | ".db" => {
                    sec.end += args
                        .iter()
                        .map(|a| match a {
                            DirectiveArg::String(s) => s.len() as u32,
                            _ => 1,
                        })
                        .sum::<u32>();
                }
                ".word" | ".dw" => sec.end += args.len() as u32 * 2,
                ".dword" | ".dd" => sec.end += args.len() as u32 * 4,
                ".res" | ".fill" => {
                    sec.end += eval_arg(args, 0, &layout.symbols, constants, cursor as i64, stmt)?
                        .max(0) as u32;
                }
                ".incbin" => {
                    let path = string_arg(args, 0, stmt)?;
                    let bytes = fs::read(path).map_err(|e| {
                        Diagnostic::new(stmt.span.clone(), format!("cannot read .incbin: {e}"))
                    })?;
                    sec.end += bytes.len() as u32;
                }
                ".tile2bpp" => {
                    sec.end += 16;
                }
                _ => {
                    return Err(Diagnostic::new(
                        stmt.span.clone(),
                        format!("unsupported directive `{name}`"),
                    ));
                }
            },
            StatementKind::Instruction { mnemonic, operand } => {
                let mode = choose_mode(
                    mnemonic,
                    operand,
                    &layout.symbols,
                    constants,
                    cursor as i64,
                    isa,
                    stmt,
                )?;
                let info = isa.lookup(mnemonic, mode).ok_or_else(|| {
                    Diagnostic::new(
                        stmt.span.clone(),
                        format!("unsupported {mnemonic} addressing mode {mode:?}"),
                    )
                })?;
                sec.end += info.width as u32;
            }
            StatementKind::Conditional { .. } => unreachable!(),
        }
    }
    Ok(layout)
}
fn eval_arg(
    args: &[DirectiveArg],
    index: usize,
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    current: i64,
    stmt: &Statement,
) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(DirectiveArg::Expr(e)) => e
            .eval(&context(symbols, constants, current))
            .map_err(|m| Diagnostic::new(stmt.span.clone(), m)),
        _ => Err(Diagnostic::new(
            stmt.span.clone(),
            "expected expression argument",
        )),
    }
}
fn string_arg<'a>(
    args: &'a [DirectiveArg],
    index: usize,
    stmt: &Statement,
) -> Result<&'a str, Diagnostic> {
    match args.get(index) {
        Some(DirectiveArg::String(s)) => Ok(s),
        _ => Err(Diagnostic::new(stmt.span.clone(), "expected quoted string")),
    }
}

fn choose_mode<I: Isa>(
    mnemonic: &str,
    operand: &Operand,
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    current: i64,
    isa: &I,
    stmt: &Statement,
) -> Result<AddressingMode, Diagnostic> {
    let raw = match operand {
        Operand::None => AddressingMode::Implied,
        Operand::Accumulator => AddressingMode::Accumulator,
        Operand::Immediate(_) => AddressingMode::Immediate,
        Operand::Indirect(_) => AddressingMode::Indirect,
        Operand::IndexedIndirect(_) => AddressingMode::IndexedIndirect,
        Operand::IndirectIndexed(_) => AddressingMode::IndirectIndexed,
        Operand::Indexed(_, IndexRegister::X) => AddressingMode::AbsoluteX,
        Operand::Indexed(_, IndexRegister::Y) => AddressingMode::AbsoluteY,
        Operand::Expr(e) => {
            if is_branch(mnemonic) {
                AddressingMode::Relative
            } else {
                AddressingMode::Absolute
            }
        }
    };
    let expr = match operand {
        Operand::Expr(e)
        | Operand::Immediate(e)
        | Operand::Indirect(e)
        | Operand::IndexedIndirect(e)
        | Operand::IndirectIndexed(e)
        | Operand::Indexed(e, _) => Some(e),
        _ => None,
    };
    if let Some(e) = expr {
        if let Ok(v) = e.eval(&context(symbols, constants, current)) {
            let zp = match operand {
                Operand::Indexed(_, IndexRegister::X) => AddressingMode::ZeroPageX,
                Operand::Indexed(_, IndexRegister::Y) => AddressingMode::ZeroPageY,
                _ => AddressingMode::ZeroPage,
            };
            // Only direct operands participate in zero-page relaxation.  An
            // operand already wrapped in parentheses has a distinct opcode
            // family (indexed-indirect or indirect-indexed), even when its
            // pointer byte happens to be below $0100.
            if matches!(operand, Operand::Expr(_) | Operand::Indexed(_, _))
                && (0..=255).contains(&v)
                && isa.lookup(mnemonic, zp).is_some()
            {
                return Ok(zp);
            }
        }
    }
    if isa.lookup(mnemonic, raw).is_some() {
        Ok(raw)
    } else {
        Err(Diagnostic::new(
            stmt.span.clone(),
            format!("no opcode for {mnemonic} with operand"),
        ))
    }
}

fn is_branch(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "BCC" | "BCS" | "BEQ" | "BMI" | "BNE" | "BPL" | "BVC" | "BVS"
    )
}

fn emit_object<I: Isa>(
    statements: &[Statement],
    constants: &HashMap<String, i64>,
    layout: &Layout,
    options: &AssemblerOptions,
    isa: &I,
) -> Result<ObjectFile, Diagnostic> {
    let mut object = ObjectFile::default();
    for s in &layout.sections {
        object.sections.push(Section {
            name: s.name.clone(),
            kind: s.kind,
            vma: s.base,
            alignment: s.alignment,
            fill: s.fill,
            data: vec![s.fill; s.end as usize],
        });
    }
    let mut flat = Vec::new();
    active(statements, &layout.symbols, constants, &mut flat);
    for (idx, stmt) in flat {
        let (sec_idx, loc, _) = *layout.locations.get(&idx).unwrap();
        if let StatementKind::Directive { name, args } = &stmt.kind {
            if name == ".segment" || name == ".section" {
                let _ = string_arg(args, 0, stmt)?;
                continue;
            }
        }
        let base = object.sections[sec_idx].vma as usize;
        let offset = loc.saturating_sub(base);
        emit_statement(
            stmt,
            &mut object,
            sec_idx,
            offset,
            constants,
            &layout.symbols,
            isa,
            options,
        )?;
        let current = object.sections[sec_idx].vma as i64 + offset as i64;
        let length = source_length(stmt, &layout.symbols, constants, current, isa)?;
        if length != 0 {
            object.source_map.push(SourceMapEntry {
                section: object.sections[sec_idx].name.clone(),
                offset: offset as u32,
                length: length as u32,
                span: stmt.span.clone(),
            });
        }
    }
    object.symbols = layout
        .symbols
        .iter()
        .map(|(name, value)| Symbol {
            name: name.clone(),
            value: *value as u16,
            section: object
                .sections
                .iter()
                .find(|s| {
                    (*value as u16) >= s.vma
                        && (*value as u16) < s.vma.wrapping_add(s.data.len() as u16)
                })
                .map(|s| s.name.clone()),
            global: true,
        })
        .collect();
    Ok(object)
}

fn source_length<I: Isa>(
    stmt: &Statement,
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    current: i64,
    isa: &I,
) -> Result<usize, Diagnostic> {
    match &stmt.kind {
        StatementKind::Instruction { mnemonic, operand } => {
            let mode = choose_mode(mnemonic, operand, symbols, constants, current, isa, stmt)?;
            Ok(isa
                .lookup(mnemonic, mode)
                .map(|info| info.width as usize)
                .unwrap_or(0))
        }
        StatementKind::Directive { name, args } => match name.as_str() {
            ".byte" | ".db" => Ok(args
                .iter()
                .map(|arg| match arg {
                    DirectiveArg::String(value) => value.len(),
                    _ => 1,
                })
                .sum()),
            ".word" | ".dw" => Ok(args.len() * 2),
            ".dword" | ".dd" => Ok(args.len() * 4),
            ".res" | ".fill" => {
                Ok(eval_arg(args, 0, symbols, constants, current, stmt)?.max(0) as usize)
            }
            ".incbin" => Ok(fs::metadata(string_arg(args, 0, stmt)?)
                .map_err(|e| {
                    Diagnostic::new(stmt.span.clone(), format!("cannot read .incbin: {e}"))
                })?
                .len() as usize),
            ".tile2bpp" => Ok(16),
            ".org" | ".align" | ".segment" | ".section" => Ok(0),
            _ => Ok(0),
        },
        _ => Ok(0),
    }
}
fn emit_statement<I: Isa>(
    stmt: &Statement,
    object: &mut ObjectFile,
    sec_idx: usize,
    offset: usize,
    constants: &HashMap<String, i64>,
    symbols: &HashMap<String, i64>,
    isa: &I,
    _options: &AssemblerOptions,
) -> Result<(), Diagnostic> {
    let section_name = object.sections[sec_idx].name.clone();
    let current = object.sections[sec_idx].vma as i64 + offset as i64;
    match &stmt.kind {
        StatementKind::Label(_) | StatementKind::Assignment { .. } => {}
        StatementKind::Directive { name, args } => match name.as_str() {
            ".org" | ".align" | ".segment" | ".section" => {}
            ".byte" | ".db" => {
                let mut at = offset;
                for arg in args {
                    match arg {
                        DirectiveArg::String(s) => {
                            for b in s.bytes() {
                                write(&mut object.sections[sec_idx].data, at, b);
                                at += 1;
                            }
                        }
                        DirectiveArg::Expr(e) => {
                            let v = value(e, symbols, constants, current, stmt)?;
                            write(&mut object.sections[sec_idx].data, at, v as u8);
                            add_reloc(
                                object,
                                &section_name,
                                at,
                                e,
                                symbols,
                                RelocationKind::Absolute8,
                                v,
                            );
                            at += 1;
                        }
                        DirectiveArg::Bytes(bytes) => {
                            for b in bytes {
                                write(&mut object.sections[sec_idx].data, at, *b);
                                at += 1;
                            }
                        }
                    }
                }
            }
            ".word" | ".dw" => emit_values(
                object,
                sec_idx,
                &section_name,
                offset,
                args,
                2,
                symbols,
                constants,
                stmt,
                current,
            )?,
            ".dword" | ".dd" => emit_values(
                object,
                sec_idx,
                &section_name,
                offset,
                args,
                4,
                symbols,
                constants,
                stmt,
                current,
            )?,
            ".res" | ".fill" => {
                let count = value_arg(args, 0, symbols, constants, current, stmt)?.max(0) as usize;
                let fill = args
                    .get(1)
                    .map(|_| value_arg(args, 1, symbols, constants, current, stmt))
                    .transpose()?
                    .unwrap_or(0) as u8;
                for i in 0..count {
                    write(&mut object.sections[sec_idx].data, offset + i, fill);
                }
            }
            ".incbin" => {
                let bytes = fs::read(string_arg(args, 0, stmt)?).map_err(|e| {
                    Diagnostic::new(stmt.span.clone(), format!("cannot read .incbin: {e}"))
                })?;
                for (i, b) in bytes.into_iter().enumerate() {
                    write(&mut object.sections[sec_idx].data, offset + i, b);
                }
            }
            ".tile2bpp" => {
                let rows: Vec<&str> = args
                    .iter()
                    .filter_map(|a| {
                        if let DirectiveArg::String(s) = a {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                if rows.len() != 8
                    || rows
                        .iter()
                        .any(|r| r.len() != 8 || r.bytes().any(|b| !(b'0'..=b'3').contains(&b)))
                {
                    return Err(Diagnostic::new(
                        stmt.span.clone(),
                        ".tile2bpp requires eight 8-pixel strings containing 0..3",
                    ));
                }
                let mut p0 = [0; 8];
                let mut p1 = [0; 8];
                for (y, row) in rows.iter().enumerate() {
                    for (x, b) in row.bytes().enumerate() {
                        p0[y] |= ((b - b'0') & 1) << (7 - x);
                        p1[y] |= (((b - b'0') >> 1) & 1) << (7 - x);
                    }
                }
                for i in 0..8 {
                    write(&mut object.sections[sec_idx].data, offset + i, p0[i]);
                    write(&mut object.sections[sec_idx].data, offset + 8 + i, p1[i]);
                }
            }
            _ => {
                return Err(Diagnostic::new(
                    stmt.span.clone(),
                    format!("unsupported directive `{name}`"),
                ));
            }
        },
        StatementKind::Instruction { mnemonic, operand } => {
            let mode = choose_mode(mnemonic, operand, symbols, constants, current, isa, stmt)?;
            let info = isa
                .lookup(mnemonic, mode)
                .ok_or_else(|| Diagnostic::new(stmt.span.clone(), "unsupported instruction"))?;
            write(&mut object.sections[sec_idx].data, offset, info.byte);
            let e = match operand {
                Operand::Expr(e)
                | Operand::Immediate(e)
                | Operand::Indirect(e)
                | Operand::IndexedIndirect(e)
                | Operand::IndirectIndexed(e)
                | Operand::Indexed(e, _) => Some(e),
                _ => None,
            };
            if let Some(e) = e {
                let v = value(e, symbols, constants, current, stmt)?;
                if mode == AddressingMode::Relative {
                    let target = v;
                    let next = current + 2;
                    let delta = target - next;
                    if !(-128..=127).contains(&delta) {
                        return Err(Diagnostic::new(
                            stmt.span.clone(),
                            "branch target is out of range",
                        ));
                    }
                    write(
                        &mut object.sections[sec_idx].data,
                        offset + 1,
                        delta as i8 as u8,
                    );
                    add_reloc(
                        object,
                        &section_name,
                        offset + 1,
                        e,
                        symbols,
                        RelocationKind::Relative8,
                        target,
                    );
                } else {
                    for n in 0..(info.width - 1) {
                        write(
                            &mut object.sections[sec_idx].data,
                            offset + 1 + n as usize,
                            (v >> (8 * n)) as u8,
                        );
                    }
                    let kind = if e.symbol().is_some() {
                        if mode == AddressingMode::Immediate && v < 0 {
                            RelocationKind::Low8
                        } else if info.width == 2 {
                            RelocationKind::Absolute8
                        } else {
                            RelocationKind::Absolute16
                        }
                    } else {
                        RelocationKind::Absolute16
                    };
                    add_reloc(object, &section_name, offset + 1, e, symbols, kind, v);
                }
            }
        }
        StatementKind::Conditional { .. } => unreachable!(),
    }
    Ok(())
}
fn value(
    e: &expr::Expr,
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    current: i64,
    stmt: &Statement,
) -> Result<i64, Diagnostic> {
    e.eval(&context(symbols, constants, current))
        .map_err(|m| Diagnostic::new(stmt.span.clone(), m))
}
fn value_arg(
    args: &[DirectiveArg],
    i: usize,
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    current: i64,
    stmt: &Statement,
) -> Result<i64, Diagnostic> {
    match args.get(i) {
        Some(DirectiveArg::Expr(e)) => value(e, symbols, constants, current, stmt),
        _ => Err(Diagnostic::new(stmt.span.clone(), "expected expression")),
    }
}
fn emit_values(
    object: &mut ObjectFile,
    sec_idx: usize,
    section: &str,
    offset: usize,
    args: &[DirectiveArg],
    width: u8,
    symbols: &HashMap<String, i64>,
    constants: &HashMap<String, i64>,
    stmt: &Statement,
    current: i64,
) -> Result<(), Diagnostic> {
    for (i, arg) in args.iter().enumerate() {
        let e = match arg {
            DirectiveArg::Expr(e) => e,
            _ => return Err(Diagnostic::new(stmt.span.clone(), "expected expression")),
        };
        let v = value(
            e,
            symbols,
            constants,
            current + (i * width as usize) as i64,
            stmt,
        )?;
        for n in 0..width {
            write(
                &mut object.sections[sec_idx].data,
                offset + i * width as usize + n as usize,
                (v >> (8 * n)) as u8,
            );
        }
        add_reloc(
            object,
            section,
            offset + i * width as usize,
            e,
            symbols,
            if width == 2 {
                RelocationKind::Absolute16
            } else {
                RelocationKind::Absolute8
            },
            v,
        );
    }
    Ok(())
}
fn add_reloc(
    object: &mut ObjectFile,
    section: &str,
    offset: usize,
    e: &expr::Expr,
    symbols: &HashMap<String, i64>,
    kind: RelocationKind,
    value: i64,
) {
    if let Some(name) = e.symbol() {
        if symbols.contains_key(&name.to_ascii_uppercase()) {
            object.relocations.push(Relocation {
                section: section.into(),
                offset: offset as u32,
                kind,
                symbol: name.to_string(),
                addend: value - symbols[&name.to_ascii_uppercase()],
            });
        }
    }
}
fn write(data: &mut Vec<u8>, offset: usize, value: u8) {
    if offset >= data.len() {
        data.resize(offset + 1, 0);
    }
    data[offset] = value;
}

fn official_opcode(m: &str, mode: AddressingMode) -> Option<OpcodeInfo> {
    let t: &[(&str, AddressingMode, u8)] = &[
        ("ADC", AddressingMode::Immediate, 0x69),
        ("ADC", AddressingMode::ZeroPage, 0x65),
        ("ADC", AddressingMode::ZeroPageX, 0x75),
        ("ADC", AddressingMode::Absolute, 0x6d),
        ("ADC", AddressingMode::AbsoluteX, 0x7d),
        ("ADC", AddressingMode::AbsoluteY, 0x79),
        ("ADC", AddressingMode::IndexedIndirect, 0x61),
        ("ADC", AddressingMode::IndirectIndexed, 0x71),
        ("AND", AddressingMode::Immediate, 0x29),
        ("AND", AddressingMode::ZeroPage, 0x25),
        ("AND", AddressingMode::ZeroPageX, 0x35),
        ("AND", AddressingMode::Absolute, 0x2d),
        ("AND", AddressingMode::AbsoluteX, 0x3d),
        ("AND", AddressingMode::AbsoluteY, 0x39),
        ("AND", AddressingMode::IndexedIndirect, 0x21),
        ("AND", AddressingMode::IndirectIndexed, 0x31),
        ("ASL", AddressingMode::Accumulator, 0x0a),
        ("ASL", AddressingMode::ZeroPage, 0x06),
        ("ASL", AddressingMode::ZeroPageX, 0x16),
        ("ASL", AddressingMode::Absolute, 0x0e),
        ("ASL", AddressingMode::AbsoluteX, 0x1e),
        ("BIT", AddressingMode::ZeroPage, 0x24),
        ("BIT", AddressingMode::Absolute, 0x2c),
        ("CMP", AddressingMode::Immediate, 0xc9),
        ("CMP", AddressingMode::ZeroPage, 0xc5),
        ("CMP", AddressingMode::ZeroPageX, 0xd5),
        ("CMP", AddressingMode::Absolute, 0xcd),
        ("CMP", AddressingMode::AbsoluteX, 0xdd),
        ("CMP", AddressingMode::AbsoluteY, 0xd9),
        ("CMP", AddressingMode::IndexedIndirect, 0xc1),
        ("CMP", AddressingMode::IndirectIndexed, 0xd1),
        ("CPX", AddressingMode::Immediate, 0xe0),
        ("CPX", AddressingMode::ZeroPage, 0xe4),
        ("CPX", AddressingMode::Absolute, 0xec),
        ("CPY", AddressingMode::Immediate, 0xc0),
        ("CPY", AddressingMode::ZeroPage, 0xc4),
        ("CPY", AddressingMode::Absolute, 0xcc),
        ("DEC", AddressingMode::ZeroPage, 0xc6),
        ("DEC", AddressingMode::ZeroPageX, 0xd6),
        ("DEC", AddressingMode::Absolute, 0xce),
        ("DEC", AddressingMode::AbsoluteX, 0xde),
        ("EOR", AddressingMode::Immediate, 0x49),
        ("EOR", AddressingMode::ZeroPage, 0x45),
        ("EOR", AddressingMode::ZeroPageX, 0x55),
        ("EOR", AddressingMode::Absolute, 0x4d),
        ("EOR", AddressingMode::AbsoluteX, 0x5d),
        ("EOR", AddressingMode::AbsoluteY, 0x59),
        ("EOR", AddressingMode::IndexedIndirect, 0x41),
        ("EOR", AddressingMode::IndirectIndexed, 0x51),
        ("INC", AddressingMode::ZeroPage, 0xe6),
        ("INC", AddressingMode::ZeroPageX, 0xf6),
        ("INC", AddressingMode::Absolute, 0xee),
        ("INC", AddressingMode::AbsoluteX, 0xfe),
        ("LDA", AddressingMode::Immediate, 0xa9),
        ("LDA", AddressingMode::ZeroPage, 0xa5),
        ("LDA", AddressingMode::ZeroPageX, 0xb5),
        ("LDA", AddressingMode::Absolute, 0xad),
        ("LDA", AddressingMode::AbsoluteX, 0xbd),
        ("LDA", AddressingMode::AbsoluteY, 0xb9),
        ("LDA", AddressingMode::IndexedIndirect, 0xa1),
        ("LDA", AddressingMode::IndirectIndexed, 0xb1),
        ("LDX", AddressingMode::Immediate, 0xa2),
        ("LDX", AddressingMode::ZeroPage, 0xa6),
        ("LDX", AddressingMode::ZeroPageY, 0xb6),
        ("LDX", AddressingMode::Absolute, 0xae),
        ("LDX", AddressingMode::AbsoluteY, 0xbe),
        ("LDY", AddressingMode::Immediate, 0xa0),
        ("LDY", AddressingMode::ZeroPage, 0xa4),
        ("LDY", AddressingMode::ZeroPageX, 0xb4),
        ("LDY", AddressingMode::Absolute, 0xac),
        ("LDY", AddressingMode::AbsoluteX, 0xbc),
        ("LSR", AddressingMode::Accumulator, 0x4a),
        ("LSR", AddressingMode::ZeroPage, 0x46),
        ("LSR", AddressingMode::ZeroPageX, 0x56),
        ("LSR", AddressingMode::Absolute, 0x4e),
        ("LSR", AddressingMode::AbsoluteX, 0x5e),
        ("ORA", AddressingMode::Immediate, 0x09),
        ("ORA", AddressingMode::ZeroPage, 0x05),
        ("ORA", AddressingMode::ZeroPageX, 0x15),
        ("ORA", AddressingMode::Absolute, 0x0d),
        ("ORA", AddressingMode::AbsoluteX, 0x1d),
        ("ORA", AddressingMode::AbsoluteY, 0x19),
        ("ORA", AddressingMode::IndexedIndirect, 0x01),
        ("ORA", AddressingMode::IndirectIndexed, 0x11),
        ("ROL", AddressingMode::Accumulator, 0x2a),
        ("ROL", AddressingMode::ZeroPage, 0x26),
        ("ROL", AddressingMode::ZeroPageX, 0x36),
        ("ROL", AddressingMode::Absolute, 0x2e),
        ("ROL", AddressingMode::AbsoluteX, 0x3e),
        ("ROR", AddressingMode::Accumulator, 0x6a),
        ("ROR", AddressingMode::ZeroPage, 0x66),
        ("ROR", AddressingMode::ZeroPageX, 0x76),
        ("ROR", AddressingMode::Absolute, 0x6e),
        ("ROR", AddressingMode::AbsoluteX, 0x7e),
        ("SBC", AddressingMode::Immediate, 0xe9),
        ("SBC", AddressingMode::ZeroPage, 0xe5),
        ("SBC", AddressingMode::ZeroPageX, 0xf5),
        ("SBC", AddressingMode::Absolute, 0xed),
        ("SBC", AddressingMode::AbsoluteX, 0xfd),
        ("SBC", AddressingMode::AbsoluteY, 0xf9),
        ("SBC", AddressingMode::IndexedIndirect, 0xe1),
        ("SBC", AddressingMode::IndirectIndexed, 0xf1),
        ("STA", AddressingMode::ZeroPage, 0x85),
        ("STA", AddressingMode::ZeroPageX, 0x95),
        ("STA", AddressingMode::Absolute, 0x8d),
        ("STA", AddressingMode::AbsoluteX, 0x9d),
        ("STA", AddressingMode::AbsoluteY, 0x99),
        ("STA", AddressingMode::IndexedIndirect, 0x81),
        ("STA", AddressingMode::IndirectIndexed, 0x91),
        ("STX", AddressingMode::ZeroPage, 0x86),
        ("STX", AddressingMode::ZeroPageY, 0x96),
        ("STX", AddressingMode::Absolute, 0x8e),
        ("STY", AddressingMode::ZeroPage, 0x84),
        ("STY", AddressingMode::ZeroPageX, 0x94),
        ("STY", AddressingMode::Absolute, 0x8c),
        ("JMP", AddressingMode::Absolute, 0x4c),
        ("JMP", AddressingMode::Indirect, 0x6c),
        ("JSR", AddressingMode::Absolute, 0x20),
    ];
    if let Some((_, mode, byte)) = t.iter().find(|(mn, mo, _)| *mn == m && *mo == mode) {
        return Some(OpcodeInfo {
            byte: *byte,
            mode: *mode,
            width: if matches!(mode, AddressingMode::Implied | AddressingMode::Accumulator) {
                1
            } else if matches!(
                mode,
                AddressingMode::Absolute
                    | AddressingMode::AbsoluteX
                    | AddressingMode::AbsoluteY
                    | AddressingMode::Indirect
            ) {
                3
            } else {
                2
            },
            official: true,
        });
    }
    let implied = [
        ("BRK", 0x00),
        ("CLC", 0x18),
        ("CLD", 0xd8),
        ("CLI", 0x58),
        ("CLV", 0xb8),
        ("DEX", 0xca),
        ("DEY", 0x88),
        ("INX", 0xe8),
        ("INY", 0xc8),
        ("NOP", 0xea),
        ("PHA", 0x48),
        ("PHP", 0x08),
        ("PLA", 0x68),
        ("PLP", 0x28),
        ("RTI", 0x40),
        ("RTS", 0x60),
        ("SEC", 0x38),
        ("SED", 0xf8),
        ("SEI", 0x78),
        ("TAX", 0xaa),
        ("TAY", 0xa8),
        ("TSX", 0xba),
        ("TXA", 0x8a),
        ("TXS", 0x9a),
        ("TYA", 0x98),
    ];
    implied
        .iter()
        .find(|(mn, b)| *mn == m && mode == AddressingMode::Implied)
        .map(|(_, b)| OpcodeInfo {
            byte: *b,
            mode,
            width: 1,
            official: true,
        })
        .or_else(|| {
            let branches = [
                ("BCC", 0x90),
                ("BCS", 0xb0),
                ("BEQ", 0xf0),
                ("BMI", 0x30),
                ("BNE", 0xd0),
                ("BPL", 0x10),
                ("BVC", 0x50),
                ("BVS", 0x70),
            ];
            branches
                .iter()
                .find(|(mn, _)| *mn == m && mode == AddressingMode::Relative)
                .map(|(_, b)| OpcodeInfo {
                    byte: *b,
                    mode,
                    width: 2,
                    official: true,
                })
        })
}

pub struct InesOptions {
    pub prg_banks: usize,
    pub chr_banks: usize,
    pub vertical_mirroring: bool,
    pub battery: bool,
}
impl Default for InesOptions {
    fn default() -> Self {
        Self {
            prg_banks: 2,
            chr_banks: 0,
            vertical_mirroring: false,
            battery: false,
        }
    }
}
pub fn write_ines(object: &ObjectFile, options: &InesOptions) -> Result<Vec<u8>, Diagnostic> {
    if !matches!(options.prg_banks, 1 | 2) {
        return Err(Diagnostic::new(
            Span::default(),
            "mapper-0 supports one or two 16 KiB PRG banks",
        ));
    }
    if options.chr_banks > 1 {
        return Err(Diagnostic::new(
            Span::default(),
            "first-pass mapper-0 output supports at most one 8 KiB CHR bank",
        ));
    }
    let mut prg = vec![0u8; options.prg_banks * 0x4000];
    let mut chr = vec![0u8; options.chr_banks * 0x2000];
    for section in &object.sections {
        match section.kind {
            SectionKind::Prg | SectionKind::Other => {
                for (i, b) in section.data.iter().enumerate() {
                    let address = section.vma as usize + i;
                    let offset = if options.prg_banks == 1 && address >= 0xc000 {
                        address - 0xc000
                    } else {
                        address.checked_sub(0x8000).ok_or_else(|| {
                            Diagnostic::new(
                                Span::default(),
                                format!("PRG section `{}` is below $8000", section.name),
                            )
                        })?
                    };
                    if offset >= prg.len() {
                        return Err(Diagnostic::new(
                            Span::default(),
                            format!("PRG section `{}` exceeds selected ROM size", section.name),
                        ));
                    }
                    prg[offset] = *b;
                }
            }
            SectionKind::Chr => {
                for (i, b) in section.data.iter().enumerate() {
                    if i >= chr.len() {
                        return Err(Diagnostic::new(
                            Span::default(),
                            "CHR data exceeds selected CHR size",
                        ));
                    }
                    chr[i] = *b;
                }
            }
            SectionKind::Ram => {}
        }
    }
    if options.chr_banks == 1
        && chr.iter().all(|b| *b == 0)
        && object
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::Chr && !s.data.is_empty())
    { /* zero-filled CHR is valid */
    }
    let mut out = Vec::with_capacity(16 + prg.len() + chr.len());
    out.extend_from_slice(b"NES\x1a");
    out.push(options.prg_banks as u8);
    out.push(options.chr_banks as u8);
    out.push((options.vertical_mirroring as u8) | ((options.battery as u8) << 1));
    out.push(0);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&prg);
    out.extend_from_slice(&chr);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_instructions_and_forward_labels() {
        let obj = Assembler::default()
            .assemble_str(
                "test.asm",
                ".org $8000\nstart: LDA #$01\n       STA $0200\n       BNE start\n",
            )
            .unwrap();
        assert_eq!(
            &obj.section("DEFAULT").unwrap().data[..],
            &[0xa9, 1, 0x8d, 0, 2, 0xd0, 0xf9]
        );
    }
    #[test]
    fn directives_and_tile() {
        let obj = Assembler::default().assemble_str("test.asm", ".segment \"CHR\"\n.tile2bpp\n \"00000000\"\n \"00000000\"\n \"00000000\"\n \"00000000\"\n \"00000000\"\n \"00000000\"\n \"00000000\"\n \"00000003\"\n").unwrap();
        assert_eq!(obj.section("CHR").unwrap().data.len(), 16);
        assert_eq!(obj.section("CHR").unwrap().data[15], 1);
    }
    #[test]
    fn writes_loadable_shape() {
        let obj = Assembler::default()
            .assemble_str("test.asm", ".org $8000\n NOP\n")
            .unwrap();
        let rom = write_ines(&obj, &InesOptions::default()).unwrap();
        assert_eq!(&rom[..4], b"NES\x1a");
        assert_eq!(rom.len(), 16 + 0x8000);
    }

    #[test]
    fn encodes_official_addressing_forms() {
        let obj = Assembler::default()
            .assemble_str(
                "modes.asm",
                ".org $8000\n\
                 LDA #$12\n\
                 LDA $12\n\
                 LDA $12,X\n\
                 LDA $1234\n\
                 LDA $1234,X\n\
                 LDA $1234,Y\n\
                 LDA ($12,X)\n\
                 LDA ($12),Y\n\
                 ORA ($12,X)\n\
                 JMP ($1234)\n\
                 BIT $1234\n\
                 ASL A\n",
            )
            .unwrap();
        assert_eq!(
            obj.section("DEFAULT").unwrap().data,
            vec![
                0xa9, 0x12, 0xa5, 0x12, 0xb5, 0x12, 0xad, 0x34, 0x12, 0xbd, 0x34, 0x12, 0xb9, 0x34,
                0x12, 0xa1, 0x12, 0xb1, 0x12, 0x01, 0x12, 0x6c, 0x34, 0x12, 0x2c, 0x34, 0x12, 0x0a,
            ]
        );
    }

    #[test]
    fn rejects_out_of_range_branches_and_unknown_directives() {
        let branch = Assembler::default()
            .assemble_str("branch.asm", ".org $8000\nBNE far\n.res 128\nfar:\nNOP\n");
        assert!(branch.is_err());

        let directive = Assembler::default().assemble_str("directive.asm", ".wat\n");
        assert!(directive
            .as_ref()
            .is_err_and(|e| e.message.contains("unsupported directive")));
    }

    #[test]
    fn preserves_section_switches_and_current_location_expressions() {
        let obj = Assembler::default()
            .assemble_str(
                "sections.asm",
                ".segment \"PRG\"\n.org $8000\n.word *\n.byte 5 % 2\n\
                 .segment \"CHR\"\n.byte $22\n\
                 .segment \"PRG\"\n.byte $33\n",
            )
            .unwrap();
        assert_eq!(
            obj.section("PRG").unwrap().data,
            vec![0x00, 0x80, 0x01, 0x33]
        );
        assert_eq!(obj.section("CHR").unwrap().data, vec![0x22]);
    }

    #[test]
    fn resolves_incbin_relative_to_source_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("nes-asm-incbin-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("asset.bin"), [0xde, 0xad, 0xbe, 0xef]).unwrap();
        let source = dir.join("main.asm");
        std::fs::write(&source, ".org $8000\n.incbin \"asset.bin\"\n").unwrap();
        let object = Assembler::default().assemble_file(&source).unwrap();
        assert_eq!(
            object.section("DEFAULT").unwrap().data,
            [0xde, 0xad, 0xbe, 0xef]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
