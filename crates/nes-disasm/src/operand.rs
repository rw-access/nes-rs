//! Decoded 6502 operands and deterministic assembly spelling.

use nes_isa::AddressingMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodedOperand {
    Implied,
    Accumulator,
    Immediate(u8),
    ZeroPage { value: u8, index: Option<char> },
    Absolute { address: u16, index: Option<char> },
    IndexedIndirect(u8),
    IndirectIndexed(u8),
    Indirect(u16),
    Relative { offset: i8, target: u16 },
}

impl DecodedOperand {
    pub fn target(&self) -> Option<u16> {
        match self {
            Self::Absolute { address, .. } => Some(*address),
            Self::Indirect(address) => Some(*address),
            Self::Relative { target, .. } => Some(*target),
            _ => None,
        }
    }

    pub fn mode(&self) -> AddressingMode {
        match self {
            Self::Implied => AddressingMode::Implied,
            Self::Accumulator => AddressingMode::Accumulator,
            Self::Immediate(_) => AddressingMode::Immediate,
            Self::ZeroPage { index: None, .. } => AddressingMode::ZeroPage,
            Self::ZeroPage {
                index: Some('X'), ..
            } => AddressingMode::ZeroPageIndexedX,
            Self::ZeroPage {
                index: Some('Y'), ..
            } => AddressingMode::ZeroPageIndexedY,
            Self::Absolute { index: None, .. } => AddressingMode::Absolute,
            Self::Absolute {
                index: Some('X'), ..
            } => AddressingMode::AbsoluteIndexedX,
            Self::Absolute {
                index: Some('Y'), ..
            } => AddressingMode::AbsoluteIndexedY,
            Self::IndexedIndirect(_) => AddressingMode::IndexedIndirect,
            Self::IndirectIndexed(_) => AddressingMode::IndirectIndexed,
            Self::Indirect(_) => AddressingMode::Indirect,
            Self::Relative { .. } => AddressingMode::Relative,
            Self::ZeroPage { index: Some(_), .. } | Self::Absolute { index: Some(_), .. } => {
                AddressingMode::Absolute
            }
        }
    }

    pub fn format(&self, labels: Option<&std::collections::BTreeMap<u16, String>>) -> String {
        let label = |address: u16| {
            labels
                .and_then(|map| map.get(&address).cloned())
                .unwrap_or_else(|| format!("${address:04X}"))
        };
        match self {
            Self::Implied => String::new(),
            Self::Accumulator => "A".into(),
            Self::Immediate(value) => format!("#${value:02X}"),
            Self::ZeroPage { value, index } => format!(
                "${value:02X}{}",
                index.map(|c| format!(",{c}")).unwrap_or_default()
            ),
            Self::Absolute { address, index } => format!(
                "{}{}",
                label(*address),
                index.map(|c| format!(",{c}")).unwrap_or_default()
            ),
            Self::IndexedIndirect(value) => format!("(${value:02X},X)"),
            Self::IndirectIndexed(value) => format!("(${value:02X}),Y"),
            Self::Indirect(address) => format!("({})", label(*address)),
            Self::Relative { target, .. } => label(*target),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_operands() {
        assert_eq!(DecodedOperand::Immediate(0xff).format(None), "#$FF");
        assert_eq!(
            DecodedOperand::IndirectIndexed(0x20).format(None),
            "($20),Y"
        );
    }
}
