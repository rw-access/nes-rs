//! Pure byte-to-instruction decoding built on the shared `nes-isa` table.

use crate::diagnostics::DisasmError;
use crate::operand::DecodedOperand;
use crate::regions::Provenance;
use nes_isa::{AddressingMode, Cpu, Dialect, Opcode, OpcodeClass, OpcodeMetadata, OPCODES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeOptions {
    pub cpu: Cpu,
    pub dialect: Dialect,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            cpu: Cpu::Ricoh2A03,
            dialect: Dialect::Official6502,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub address: u16,
    pub file_offset: Option<usize>,
    pub bytes: Vec<u8>,
    pub opcode: u8,
    pub metadata: OpcodeMetadata,
    pub operand: DecodedOperand,
    pub target: Option<u16>,
    pub provenance: Provenance,
}

impl DecodedInstruction {
    pub fn length(&self) -> u8 {
        self.bytes.len() as u8
    }
    pub fn mnemonic(&self) -> &'static str {
        self.metadata.mnemonic.as_str()
    }
    pub fn is_branch(&self) -> bool {
        matches!(
            self.metadata.mnemonic,
            Opcode::BCC
                | Opcode::BCS
                | Opcode::BEQ
                | Opcode::BMI
                | Opcode::BNE
                | Opcode::BPL
                | Opcode::BVC
                | Opcode::BVS
        )
    }
    pub fn is_call(&self) -> bool {
        self.metadata.mnemonic == Opcode::JSR
    }
    pub fn is_return(&self) -> bool {
        matches!(self.metadata.mnemonic, Opcode::RTI | Opcode::RTS)
    }
    pub fn is_unconditional_jump(&self) -> bool {
        self.metadata.mnemonic == Opcode::JMP
    }
    pub fn terminates_fallthrough(&self) -> bool {
        self.is_return() || self.metadata.mnemonic == Opcode::STP || self.is_unconditional_jump()
    }
}

pub fn metadata(opcode: u8) -> OpcodeMetadata {
    OPCODES[opcode as usize]
}

pub fn decode_at(
    bytes: &[u8],
    address: u16,
    file_offset: Option<usize>,
    options: DecodeOptions,
) -> Result<DecodedInstruction, DisasmError> {
    if bytes.is_empty() {
        return Err(DisasmError::EmptyInput);
    }
    let meta = metadata(bytes[0]);
    if !meta.supports_cpu(options.cpu) || !meta.supports_dialect(options.dialect) {
        return Err(DisasmError::UnsupportedOpcode {
            address,
            opcode: bytes[0],
        });
    }
    // BRK has an implied addressing mode but consumes a signature byte.
    let needed = 1 + if meta.mnemonic == Opcode::BRK {
        1
    } else {
        meta.operand_width as usize
    };
    if bytes.len() < needed {
        return Err(DisasmError::TruncatedInstruction {
            address,
            opcode: bytes[0],
            needed,
            available: bytes.len(),
        });
    }
    if (address as usize) + needed > 0x10000 {
        return Err(DisasmError::AddressOverflow {
            address,
            length: needed,
        });
    }
    let raw = &bytes[..needed];
    let operand = decode_operand(meta.addressing_mode, &raw[1..], address, needed as u16);
    let target = operand.target();
    Ok(DecodedInstruction {
        address,
        file_offset,
        bytes: raw.to_vec(),
        opcode: raw[0],
        metadata: meta,
        operand,
        target,
        provenance: Provenance::Explicit,
    })
}

fn decode_operand(mode: AddressingMode, bytes: &[u8], address: u16, length: u16) -> DecodedOperand {
    let word = || u16::from_le_bytes([bytes[0], bytes[1]]);
    match mode {
        AddressingMode::Implied => DecodedOperand::Implied,
        AddressingMode::Accumulator => DecodedOperand::Accumulator,
        AddressingMode::Immediate => DecodedOperand::Immediate(bytes[0]),
        AddressingMode::ZeroPage => DecodedOperand::ZeroPage {
            value: bytes[0],
            index: None,
        },
        AddressingMode::ZeroPageIndexedX => DecodedOperand::ZeroPage {
            value: bytes[0],
            index: Some('X'),
        },
        AddressingMode::ZeroPageIndexedY => DecodedOperand::ZeroPage {
            value: bytes[0],
            index: Some('Y'),
        },
        AddressingMode::Absolute => DecodedOperand::Absolute {
            address: word(),
            index: None,
        },
        AddressingMode::AbsoluteIndexedX => DecodedOperand::Absolute {
            address: word(),
            index: Some('X'),
        },
        AddressingMode::AbsoluteIndexedY => DecodedOperand::Absolute {
            address: word(),
            index: Some('Y'),
        },
        AddressingMode::IndexedIndirect => DecodedOperand::IndexedIndirect(bytes[0]),
        AddressingMode::IndirectIndexed => DecodedOperand::IndirectIndexed(bytes[0]),
        AddressingMode::Indirect => DecodedOperand::Indirect(word()),
        AddressingMode::Relative => {
            let target = address
                .wrapping_add(length)
                .wrapping_add((bytes[0] as i8) as i16 as u16);
            DecodedOperand::Relative {
                offset: bytes[0] as i8,
                target,
            }
        }
    }
}

pub fn is_control_flow(meta: OpcodeMetadata) -> bool {
    matches!(
        meta.mnemonic,
        Opcode::BCC
            | Opcode::BCS
            | Opcode::BEQ
            | Opcode::BMI
            | Opcode::BNE
            | Opcode::BPL
            | Opcode::BVC
            | Opcode::BVS
            | Opcode::JMP
            | Opcode::JSR
            | Opcode::RTI
            | Opcode::RTS
            | Opcode::STP
    )
}

pub fn is_official(meta: OpcodeMetadata) -> bool {
    meta.class == OpcodeClass::Official
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_little_endian_and_relative_targets() {
        let i = decode_at(
            &[0x4c, 0x34, 0x12],
            0x8000,
            Some(3),
            DecodeOptions {
                dialect: Dialect::Ricoh2A03,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(i.target, Some(0x1234));
        let b = decode_at(&[0xd0, 0xfc], 0x8000, None, DecodeOptions::default()).unwrap();
        assert_eq!(b.target, Some(0x7ffe));
        assert_eq!(
            b.operand,
            DecodedOperand::Relative {
                offset: -4,
                target: 0x7ffe
            }
        );
    }
}
