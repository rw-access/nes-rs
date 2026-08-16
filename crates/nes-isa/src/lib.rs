//! Canonical 6502/Ricoh 2A03 opcode metadata.
//!
//! The table is deliberately a direct 256-entry array. CPU decode paths can
//! continue to index it with the fetched opcode without an extra lookup.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    ADC,
    AHX,
    ALR,
    ANC,
    AND,
    ARR,
    ASL,
    AXS,
    BCC,
    BCS,
    BEQ,
    BIT,
    BMI,
    BNE,
    BPL,
    BRK,
    BVC,
    BVS,
    CLC,
    CLD,
    CLI,
    CLV,
    CMP,
    CPX,
    CPY,
    DCP,
    DEC,
    DEX,
    DEY,
    EOR,
    INC,
    INX,
    INY,
    ISB,
    JMP,
    JSR,
    LAS,
    LAX,
    LDA,
    LDX,
    LDY,
    LSR,
    NOP,
    ORA,
    PHA,
    PHP,
    PLA,
    PLP,
    RLA,
    ROL,
    ROR,
    RRA,
    RTI,
    RTS,
    SAX,
    SBC,
    SEC,
    SED,
    SEI,
    SHX,
    SHY,
    SLO,
    SRE,
    STA,
    STP,
    STX,
    STY,
    TAS,
    TAX,
    TAY,
    TSX,
    TXA,
    TXS,
    TYA,
    XAA,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressingMode {
    Implied,
    Accumulator,      // A
    Absolute,         // $xxxx       a
    AbsoluteIndexedX, // $xxxx,Y     a,x ?? $xx,PC
    AbsoluteIndexedY, // $xxxx,Y     a,y $xx,PC
    Immediate,        // #$xx:       #i
    IndexedIndirect,  // ($xx,X)     (d,x) $xx
    Indirect,         // ($xxxx)     (a)
    IndirectIndexed,  // ($xx),Y     (d),y $xx,X
    Relative,         // $xx,PC      *+d
    ZeroPage,         // $xx         d
    ZeroPageIndexedX, // $xx,X       d,x
    ZeroPageIndexedY, // $xx,X       d,y
}

impl AddressingMode {
    /// Number of operand bytes encoded after the opcode byte.
    pub const fn operand_width(self) -> u8 {
        match self {
            Self::Implied | Self::Accumulator => 0,
            Self::Relative
            | Self::Immediate
            | Self::IndexedIndirect
            | Self::IndirectIndexed
            | Self::ZeroPage
            | Self::ZeroPageIndexedX
            | Self::ZeroPageIndexedY => 1,
            Self::Absolute | Self::AbsoluteIndexedX | Self::AbsoluteIndexedY | Self::Indirect => 2,
        }
    }
}

/// Whether an opcode is part of the documented 6502 instruction set or is an
/// undocumented NMOS opcode with known hardware behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpcodeClass {
    Official,
    UnofficialStable,
    UnofficialUnstable,
    Halt,
}

/// CPU profiles understood by the metadata table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cpu {
    Nmos6502,
    Ricoh2A03,
}

/// Assembly dialect policy for selecting from the complete opcode map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// Documented 6502 instructions only.
    Official6502,
    /// Ricoh 2A03/NMOS-compatible dialect with undocumented opcodes enabled.
    Ricoh2A03,
}

impl Opcode {
    /// Conventional uppercase assembler spelling for this mnemonic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BRK => "BRK",
            Self::ORA => "ORA",
            Self::STP => "STP",
            Self::SLO => "SLO",
            Self::NOP => "NOP",
            Self::ASL => "ASL",
            Self::PHP => "PHP",
            Self::ANC => "ANC",
            Self::BPL => "BPL",
            Self::CLC => "CLC",
            Self::JSR => "JSR",
            Self::AND => "AND",
            Self::RLA => "RLA",
            Self::BIT => "BIT",
            Self::ROL => "ROL",
            Self::PLP => "PLP",
            Self::BMI => "BMI",
            Self::SEC => "SEC",
            Self::RTI => "RTI",
            Self::EOR => "EOR",
            Self::SRE => "SRE",
            Self::LSR => "LSR",
            Self::PHA => "PHA",
            Self::ALR => "ALR",
            Self::JMP => "JMP",
            Self::BVC => "BVC",
            Self::CLI => "CLI",
            Self::RTS => "RTS",
            Self::ADC => "ADC",
            Self::RRA => "RRA",
            Self::ROR => "ROR",
            Self::PLA => "PLA",
            Self::ARR => "ARR",
            Self::BVS => "BVS",
            Self::SEI => "SEI",
            Self::STA => "STA",
            Self::SAX => "SAX",
            Self::STY => "STY",
            Self::STX => "STX",
            Self::DEY => "DEY",
            Self::TXA => "TXA",
            Self::XAA => "XAA",
            Self::BCC => "BCC",
            Self::AHX => "AHX",
            Self::TYA => "TYA",
            Self::TXS => "TXS",
            Self::TAS => "TAS",
            Self::SHY => "SHY",
            Self::SHX => "SHX",
            Self::LDY => "LDY",
            Self::LDA => "LDA",
            Self::LDX => "LDX",
            Self::LAX => "LAX",
            Self::TAY => "TAY",
            Self::TAX => "TAX",
            Self::BCS => "BCS",
            Self::CLV => "CLV",
            Self::TSX => "TSX",
            Self::LAS => "LAS",
            Self::CPY => "CPY",
            Self::CMP => "CMP",
            Self::DCP => "DCP",
            Self::DEC => "DEC",
            Self::INY => "INY",
            Self::DEX => "DEX",
            Self::AXS => "AXS",
            Self::BNE => "BNE",
            Self::CLD => "CLD",
            Self::CPX => "CPX",
            Self::SBC => "SBC",
            Self::ISB => "ISB",
            Self::INC => "INC",
            Self::INX => "INX",
            Self::BEQ => "BEQ",
            Self::SED => "SED",
        }
    }
}

/// Metadata for one byte in the 6502 opcode space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcodeMetadata {
    /// The byte used to index this entry.
    pub byte: u8,
    /// Instruction mnemonic.
    pub mnemonic: Opcode,
    pub addressing_mode: AddressingMode,
    /// Number of operand bytes following the opcode.
    pub operand_width: u8,
    pub min_cycles: u8,
    pub page_boundary_penalty: bool,
    pub class: OpcodeClass,
}

/// The subset of opcode metadata needed by the emulator's instruction
/// decoder. Keep this record compact: it is indexed for every CPU
/// instruction, while the richer [`OpcodeMetadata`] is primarily for tools
/// such as the assembler and disassembler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuOpcodeMetadata {
    pub opcode: Opcode,
    pub addressing_mode: AddressingMode,
    pub min_cycles: u8,
    pub page_boundary_penalty: bool,
}

impl CpuOpcodeMetadata {
    const fn from_metadata(metadata: OpcodeMetadata) -> Self {
        Self {
            opcode: metadata.mnemonic,
            addressing_mode: metadata.addressing_mode,
            min_cycles: metadata.min_cycles,
            page_boundary_penalty: metadata.page_boundary_penalty,
        }
    }
}

impl OpcodeMetadata {
    const fn class_for(byte: usize, mnemonic: Opcode) -> OpcodeClass {
        if matches!(mnemonic, Opcode::STP) {
            return OpcodeClass::Halt;
        }
        if matches!(byte, 0x8B | 0x93 | 0x9B | 0x9C | 0x9E | 0x9F) {
            return OpcodeClass::UnofficialUnstable;
        }
        if matches!(
            byte,
            0x00 | 0x01
                | 0x05
                | 0x06
                | 0x08
                | 0x09
                | 0x0A
                | 0x0D
                | 0x0E
                | 0x10
                | 0x11
                | 0x15
                | 0x16
                | 0x18
                | 0x19
                | 0x1D
                | 0x1E
                | 0x20
                | 0x21
                | 0x24
                | 0x25
                | 0x26
                | 0x28
                | 0x29
                | 0x2A
                | 0x2C
                | 0x2D
                | 0x2E
                | 0x30
                | 0x31
                | 0x35
                | 0x36
                | 0x38
                | 0x39
                | 0x3D
                | 0x3E
                | 0x40
                | 0x41
                | 0x45
                | 0x46
                | 0x48
                | 0x49
                | 0x4A
                | 0x4C
                | 0x4D
                | 0x4E
                | 0x50
                | 0x51
                | 0x55
                | 0x56
                | 0x58
                | 0x59
                | 0x5D
                | 0x5E
                | 0x60
                | 0x61
                | 0x65
                | 0x66
                | 0x68
                | 0x69
                | 0x6A
                | 0x6C
                | 0x6D
                | 0x6E
                | 0x70
                | 0x71
                | 0x75
                | 0x76
                | 0x78
                | 0x79
                | 0x7D
                | 0x7E
                | 0x81
                | 0x84
                | 0x85
                | 0x86
                | 0x88
                | 0x89
                | 0x8A
                | 0x8C
                | 0x8D
                | 0x8E
                | 0x90
                | 0x91
                | 0x94
                | 0x95
                | 0x98
                | 0x99
                | 0x9A
                | 0x9D
                | 0xA0
                | 0xA1
                | 0xA2
                | 0xA4
                | 0xA5
                | 0xA6
                | 0xA8
                | 0xA9
                | 0xAA
                | 0xAC
                | 0xAD
                | 0xAE
                | 0xB0
                | 0xB1
                | 0xB4
                | 0xB5
                | 0xB6
                | 0xB8
                | 0xB9
                | 0xBA
                | 0xBC
                | 0xBD
                | 0xBE
                | 0xC0
                | 0xC1
                | 0xC4
                | 0xC5
                | 0xC6
                | 0xC8
                | 0xC9
                | 0xCA
                | 0xCC
                | 0xCD
                | 0xCE
                | 0xD0
                | 0xD1
                | 0xD5
                | 0xD6
                | 0xD8
                | 0xD9
                | 0xDD
                | 0xDE
                | 0xE0
                | 0xE1
                | 0xE4
                | 0xE5
                | 0xE6
                | 0xE8
                | 0xE9
                | 0xEA
                | 0xEC
                | 0xED
                | 0xEE
                | 0xF0
                | 0xF1
                | 0xF5
                | 0xF6
                | 0xF8
                | 0xF9
                | 0xFD
                | 0xFE
        ) {
            OpcodeClass::Official
        } else {
            OpcodeClass::UnofficialStable
        }
    }

    const fn new(
        byte: usize,
        mnemonic: Opcode,
        addressing_mode: AddressingMode,
        min_cycles: u8,
        page_boundary_penalty: bool,
    ) -> Self {
        Self {
            byte: byte as u8,
            mnemonic,
            addressing_mode,
            operand_width: addressing_mode.operand_width(),
            min_cycles,
            page_boundary_penalty,
            class: Self::class_for(byte, mnemonic),
        }
    }

    pub const fn is_official(self) -> bool {
        matches!(self.class, OpcodeClass::Official)
    }

    /// The NMOS 6502 and Ricoh 2A03 share this opcode map. Decimal arithmetic
    /// differences are CPU execution semantics, not opcode availability.
    pub const fn supports_cpu(self, _cpu: Cpu) -> bool {
        true
    }

    pub const fn supports_dialect(self, dialect: Dialect) -> bool {
        match dialect {
            Dialect::Official6502 => self.is_official(),
            Dialect::Ricoh2A03 => true,
        }
    }
}

/// Compatibility name for callers that used the original core terminology.
pub type ExtendedOpcode = OpcodeMetadata;

pub const EXTENDED_OPCODES: [OpcodeMetadata; 256] = [
    OpcodeMetadata::new(0, Opcode::BRK, AddressingMode::Implied, 7, false),
    OpcodeMetadata::new(1, Opcode::ORA, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(2, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(3, Opcode::SLO, AddressingMode::IndexedIndirect, 8, false),
    OpcodeMetadata::new(4, Opcode::NOP, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(5, Opcode::ORA, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(6, Opcode::ASL, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(7, Opcode::SLO, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(8, Opcode::PHP, AddressingMode::Implied, 3, false),
    OpcodeMetadata::new(9, Opcode::ORA, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(10, Opcode::ASL, AddressingMode::Accumulator, 2, false),
    OpcodeMetadata::new(11, Opcode::ANC, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(12, Opcode::NOP, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(13, Opcode::ORA, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(14, Opcode::ASL, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(15, Opcode::SLO, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(16, Opcode::BPL, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(17, Opcode::ORA, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(18, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(19, Opcode::SLO, AddressingMode::IndirectIndexed, 8, false),
    OpcodeMetadata::new(20, Opcode::NOP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(21, Opcode::ORA, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(22, Opcode::ASL, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(23, Opcode::SLO, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(24, Opcode::CLC, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(25, Opcode::ORA, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(26, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(27, Opcode::SLO, AddressingMode::AbsoluteIndexedY, 7, false),
    OpcodeMetadata::new(28, Opcode::NOP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(29, Opcode::ORA, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(30, Opcode::ASL, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(31, Opcode::SLO, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(32, Opcode::JSR, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(33, Opcode::AND, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(34, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(35, Opcode::RLA, AddressingMode::IndexedIndirect, 8, false),
    OpcodeMetadata::new(36, Opcode::BIT, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(37, Opcode::AND, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(38, Opcode::ROL, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(39, Opcode::RLA, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(40, Opcode::PLP, AddressingMode::Implied, 4, false),
    OpcodeMetadata::new(41, Opcode::AND, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(42, Opcode::ROL, AddressingMode::Accumulator, 2, false),
    OpcodeMetadata::new(43, Opcode::ANC, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(44, Opcode::BIT, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(45, Opcode::AND, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(46, Opcode::ROL, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(47, Opcode::RLA, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(48, Opcode::BMI, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(49, Opcode::AND, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(50, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(51, Opcode::RLA, AddressingMode::IndirectIndexed, 8, false),
    OpcodeMetadata::new(52, Opcode::NOP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(53, Opcode::AND, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(54, Opcode::ROL, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(55, Opcode::RLA, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(56, Opcode::SEC, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(57, Opcode::AND, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(58, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(59, Opcode::RLA, AddressingMode::AbsoluteIndexedY, 7, false),
    OpcodeMetadata::new(60, Opcode::NOP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(61, Opcode::AND, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(62, Opcode::ROL, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(63, Opcode::RLA, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(64, Opcode::RTI, AddressingMode::Implied, 6, false),
    OpcodeMetadata::new(65, Opcode::EOR, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(66, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(67, Opcode::SRE, AddressingMode::IndexedIndirect, 8, false),
    OpcodeMetadata::new(68, Opcode::NOP, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(69, Opcode::EOR, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(70, Opcode::LSR, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(71, Opcode::SRE, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(72, Opcode::PHA, AddressingMode::Implied, 3, false),
    OpcodeMetadata::new(73, Opcode::EOR, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(74, Opcode::LSR, AddressingMode::Accumulator, 2, false),
    OpcodeMetadata::new(75, Opcode::ALR, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(76, Opcode::JMP, AddressingMode::Absolute, 3, false),
    OpcodeMetadata::new(77, Opcode::EOR, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(78, Opcode::LSR, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(79, Opcode::SRE, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(80, Opcode::BVC, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(81, Opcode::EOR, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(82, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(83, Opcode::SRE, AddressingMode::IndirectIndexed, 8, false),
    OpcodeMetadata::new(84, Opcode::NOP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(85, Opcode::EOR, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(86, Opcode::LSR, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(87, Opcode::SRE, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(88, Opcode::CLI, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(89, Opcode::EOR, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(90, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(91, Opcode::SRE, AddressingMode::AbsoluteIndexedY, 7, false),
    OpcodeMetadata::new(92, Opcode::NOP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(93, Opcode::EOR, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(94, Opcode::LSR, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(95, Opcode::SRE, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(96, Opcode::RTS, AddressingMode::Implied, 6, false),
    OpcodeMetadata::new(97, Opcode::ADC, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(98, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(99, Opcode::RRA, AddressingMode::IndexedIndirect, 8, false),
    OpcodeMetadata::new(100, Opcode::NOP, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(101, Opcode::ADC, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(102, Opcode::ROR, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(103, Opcode::RRA, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(104, Opcode::PLA, AddressingMode::Implied, 4, false),
    OpcodeMetadata::new(105, Opcode::ADC, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(106, Opcode::ROR, AddressingMode::Accumulator, 2, false),
    OpcodeMetadata::new(107, Opcode::ARR, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(108, Opcode::JMP, AddressingMode::Indirect, 5, false),
    OpcodeMetadata::new(109, Opcode::ADC, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(110, Opcode::ROR, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(111, Opcode::RRA, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(112, Opcode::BVS, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(113, Opcode::ADC, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(114, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(115, Opcode::RRA, AddressingMode::IndirectIndexed, 8, false),
    OpcodeMetadata::new(116, Opcode::NOP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(117, Opcode::ADC, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(118, Opcode::ROR, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(119, Opcode::RRA, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(120, Opcode::SEI, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(121, Opcode::ADC, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(122, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(123, Opcode::RRA, AddressingMode::AbsoluteIndexedY, 7, false),
    OpcodeMetadata::new(124, Opcode::NOP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(125, Opcode::ADC, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(126, Opcode::ROR, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(127, Opcode::RRA, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(128, Opcode::NOP, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(129, Opcode::STA, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(130, Opcode::NOP, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(131, Opcode::SAX, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(132, Opcode::STY, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(133, Opcode::STA, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(134, Opcode::STX, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(135, Opcode::SAX, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(136, Opcode::DEY, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(137, Opcode::NOP, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(138, Opcode::TXA, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(139, Opcode::XAA, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(140, Opcode::STY, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(141, Opcode::STA, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(142, Opcode::STX, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(143, Opcode::SAX, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(144, Opcode::BCC, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(145, Opcode::STA, AddressingMode::IndirectIndexed, 6, false),
    OpcodeMetadata::new(146, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(147, Opcode::AHX, AddressingMode::IndirectIndexed, 6, false),
    OpcodeMetadata::new(148, Opcode::STY, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(149, Opcode::STA, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(150, Opcode::STX, AddressingMode::ZeroPageIndexedY, 4, false),
    OpcodeMetadata::new(151, Opcode::SAX, AddressingMode::ZeroPageIndexedY, 4, false),
    OpcodeMetadata::new(152, Opcode::TYA, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(153, Opcode::STA, AddressingMode::AbsoluteIndexedY, 5, false),
    OpcodeMetadata::new(154, Opcode::TXS, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(155, Opcode::TAS, AddressingMode::AbsoluteIndexedY, 5, false),
    OpcodeMetadata::new(156, Opcode::SHY, AddressingMode::AbsoluteIndexedX, 5, false),
    OpcodeMetadata::new(157, Opcode::STA, AddressingMode::AbsoluteIndexedX, 5, false),
    OpcodeMetadata::new(158, Opcode::SHX, AddressingMode::AbsoluteIndexedY, 5, false),
    OpcodeMetadata::new(159, Opcode::AHX, AddressingMode::AbsoluteIndexedY, 5, false),
    OpcodeMetadata::new(160, Opcode::LDY, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(161, Opcode::LDA, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(162, Opcode::LDX, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(163, Opcode::LAX, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(164, Opcode::LDY, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(165, Opcode::LDA, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(166, Opcode::LDX, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(167, Opcode::LAX, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(168, Opcode::TAY, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(169, Opcode::LDA, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(170, Opcode::TAX, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(171, Opcode::LAX, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(172, Opcode::LDY, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(173, Opcode::LDA, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(174, Opcode::LDX, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(175, Opcode::LAX, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(176, Opcode::BCS, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(177, Opcode::LDA, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(178, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(179, Opcode::LAX, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(180, Opcode::LDY, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(181, Opcode::LDA, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(182, Opcode::LDX, AddressingMode::ZeroPageIndexedY, 4, false),
    OpcodeMetadata::new(183, Opcode::LAX, AddressingMode::ZeroPageIndexedY, 4, false),
    OpcodeMetadata::new(184, Opcode::CLV, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(185, Opcode::LDA, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(186, Opcode::TSX, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(187, Opcode::LAS, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(188, Opcode::LDY, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(189, Opcode::LDA, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(190, Opcode::LDX, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(191, Opcode::LAX, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(192, Opcode::CPY, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(193, Opcode::CMP, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(194, Opcode::NOP, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(195, Opcode::DCP, AddressingMode::IndexedIndirect, 8, false),
    OpcodeMetadata::new(196, Opcode::CPY, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(197, Opcode::CMP, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(198, Opcode::DEC, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(199, Opcode::DCP, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(200, Opcode::INY, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(201, Opcode::CMP, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(202, Opcode::DEX, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(203, Opcode::AXS, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(204, Opcode::CPY, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(205, Opcode::CMP, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(206, Opcode::DEC, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(207, Opcode::DCP, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(208, Opcode::BNE, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(209, Opcode::CMP, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(210, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(211, Opcode::DCP, AddressingMode::IndirectIndexed, 8, false),
    OpcodeMetadata::new(212, Opcode::NOP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(213, Opcode::CMP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(214, Opcode::DEC, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(215, Opcode::DCP, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(216, Opcode::CLD, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(217, Opcode::CMP, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(218, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(219, Opcode::DCP, AddressingMode::AbsoluteIndexedY, 7, false),
    OpcodeMetadata::new(220, Opcode::NOP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(221, Opcode::CMP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(222, Opcode::DEC, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(223, Opcode::DCP, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(224, Opcode::CPX, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(225, Opcode::SBC, AddressingMode::IndexedIndirect, 6, false),
    OpcodeMetadata::new(226, Opcode::NOP, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(227, Opcode::ISB, AddressingMode::IndexedIndirect, 8, false),
    OpcodeMetadata::new(228, Opcode::CPX, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(229, Opcode::SBC, AddressingMode::ZeroPage, 3, false),
    OpcodeMetadata::new(230, Opcode::INC, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(231, Opcode::ISB, AddressingMode::ZeroPage, 5, false),
    OpcodeMetadata::new(232, Opcode::INX, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(233, Opcode::SBC, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(234, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(235, Opcode::SBC, AddressingMode::Immediate, 2, false),
    OpcodeMetadata::new(236, Opcode::CPX, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(237, Opcode::SBC, AddressingMode::Absolute, 4, false),
    OpcodeMetadata::new(238, Opcode::INC, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(239, Opcode::ISB, AddressingMode::Absolute, 6, false),
    OpcodeMetadata::new(240, Opcode::BEQ, AddressingMode::Relative, 2, true),
    OpcodeMetadata::new(241, Opcode::SBC, AddressingMode::IndirectIndexed, 5, true),
    OpcodeMetadata::new(242, Opcode::STP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(243, Opcode::ISB, AddressingMode::IndirectIndexed, 8, false),
    OpcodeMetadata::new(244, Opcode::NOP, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(245, Opcode::SBC, AddressingMode::ZeroPageIndexedX, 4, false),
    OpcodeMetadata::new(246, Opcode::INC, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(247, Opcode::ISB, AddressingMode::ZeroPageIndexedX, 6, false),
    OpcodeMetadata::new(248, Opcode::SED, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(249, Opcode::SBC, AddressingMode::AbsoluteIndexedY, 4, true),
    OpcodeMetadata::new(250, Opcode::NOP, AddressingMode::Implied, 2, false),
    OpcodeMetadata::new(251, Opcode::ISB, AddressingMode::AbsoluteIndexedY, 7, false),
    OpcodeMetadata::new(252, Opcode::NOP, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(253, Opcode::SBC, AddressingMode::AbsoluteIndexedX, 4, true),
    OpcodeMetadata::new(254, Opcode::INC, AddressingMode::AbsoluteIndexedX, 7, false),
    OpcodeMetadata::new(255, Opcode::ISB, AddressingMode::AbsoluteIndexedX, 7, false),
];

/// Canonical public name for the 256-entry opcode table.
pub use EXTENDED_OPCODES as OPCODES;

/// Compact direct-indexed table for the CPU hot path.
///
/// This is derived from the canonical tool-facing table at compile time, so
/// the assembler and emulator still share one source of opcode truth without
/// making every decode load assembler-only fields.
pub const CPU_OPCODES: [CpuOpcodeMetadata; 256] = {
    let mut compact = [CpuOpcodeMetadata {
        opcode: Opcode::NOP,
        addressing_mode: AddressingMode::Implied,
        min_cycles: 2,
        page_boundary_penalty: false,
    }; 256];
    let mut index = 0;
    while index < compact.len() {
        compact[index] = CpuOpcodeMetadata::from_metadata(EXTENDED_OPCODES[index]);
        index += 1;
    }
    compact
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_table_covers_every_byte_in_order() {
        assert_eq!(OPCODES.len(), 256);
        for (byte, metadata) in OPCODES.iter().enumerate() {
            assert_eq!(metadata.byte, byte as u8);
            assert_eq!(
                metadata.operand_width,
                metadata.addressing_mode.operand_width()
            );
            assert!(metadata.supports_cpu(Cpu::Nmos6502));
            assert!(metadata.supports_cpu(Cpu::Ricoh2A03));
            assert!(metadata.supports_dialect(Dialect::Ricoh2A03));
        }
    }

    #[test]
    fn representative_metadata_is_canonical() {
        let reset = &OPCODES[0xA9];
        assert_eq!(reset.mnemonic, Opcode::LDA);
        assert_eq!(reset.addressing_mode, AddressingMode::Immediate);
        assert_eq!(reset.operand_width, 1);
        assert_eq!(reset.min_cycles, 2);
        assert_eq!(reset.class, OpcodeClass::Official);

        let branch = &OPCODES[0xD0];
        assert_eq!(branch.mnemonic, Opcode::BNE);
        assert_eq!(branch.addressing_mode, AddressingMode::Relative);
        assert_eq!(branch.operand_width, 1);
        assert!(branch.page_boundary_penalty);
        assert_eq!(OPCODES[0x01].class, OpcodeClass::Official);

        assert_eq!(OPCODES[0x02].class, OpcodeClass::Halt);
        assert_eq!(OPCODES[0x0B].class, OpcodeClass::UnofficialStable);
        assert_eq!(OPCODES[0x8B].class, OpcodeClass::UnofficialUnstable);
        assert!(!OPCODES[0x8B].supports_dialect(Dialect::Official6502));
        assert_eq!(Opcode::LDA.as_str(), "LDA");
    }

    #[test]
    fn cpu_table_keeps_only_decode_metadata() {
        assert_eq!(core::mem::size_of::<CpuOpcodeMetadata>(), 4);
        assert_eq!(core::mem::size_of::<OpcodeMetadata>(), 7);
        for index in 0..256 {
            let full = OPCODES[index];
            let compact = CPU_OPCODES[index];
            assert_eq!(compact.opcode, full.mnemonic);
            assert_eq!(compact.addressing_mode, full.addressing_mode);
            assert_eq!(compact.min_cycles, full.min_cycles);
            assert_eq!(compact.page_boundary_penalty, full.page_boundary_penalty);
        }
    }
}
