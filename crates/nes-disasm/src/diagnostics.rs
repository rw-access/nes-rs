//! Diagnostics emitted while decoding or analysing a binary.

use std::{error::Error, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub message: String,
    pub address: Option<u16>,
    pub file_offset: Option<usize>,
}

impl Diagnostic {
    pub fn warning(code: &'static str, message: impl Into<String>, address: Option<u16>) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            code,
            message: message.into(),
            address,
            file_offset: None,
        }
    }

    pub fn error(code: &'static str, message: impl Into<String>, address: Option<u16>) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code,
            message: message.into(),
            address,
            file_offset: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisasmError {
    EmptyInput,
    InvalidOrigin {
        origin: u32,
        length: usize,
    },
    AddressOverflow {
        address: u16,
        length: usize,
    },
    TruncatedInstruction {
        address: u16,
        opcode: u8,
        needed: usize,
        available: usize,
    },
    UnsupportedOpcode {
        address: u16,
        opcode: u8,
    },
    InvalidInesHeader {
        reason: String,
    },
    TruncatedRegion {
        region: &'static str,
        needed: usize,
        available: usize,
    },
    UnsupportedMapper {
        mapper: u16,
    },
    MappingOutOfRange {
        address: u16,
    },
    InvalidRange {
        start: u16,
        end: u16,
    },
}

impl fmt::Display for DisasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "cannot analyse an empty byte stream"),
            Self::InvalidOrigin { origin, length } => write!(f, "origin ${origin:04X} with {length} bytes exceeds the 16-bit CPU address space"),
            Self::AddressOverflow { address, length } => write!(f, "instruction at ${address:04X} with length {length} crosses the CPU address space"),
            Self::TruncatedInstruction { address, opcode, needed, available } => write!(f, "truncated instruction ${opcode:02X} at ${address:04X}: need {needed} bytes, have {available}"),
            Self::UnsupportedOpcode { address, opcode } => write!(f, "opcode ${opcode:02X} at ${address:04X} is disabled by the selected dialect"),
            Self::InvalidInesHeader { reason } => write!(f, "invalid iNES image: {reason}"),
            Self::TruncatedRegion { region, needed, available } => write!(f, "truncated {region}: need {needed} bytes, have {available}"),
            Self::UnsupportedMapper { mapper } => write!(f, "mapper {mapper} is not supported by the NROM mapping"),
            Self::MappingOutOfRange { address } => write!(f, "CPU address ${address:04X} is not mapped by this image"),
            Self::InvalidRange { start, end } => write!(f, "invalid range ${start:04X}:${end:04X}"),
        }
    }
}

impl Error for DisasmError {}
