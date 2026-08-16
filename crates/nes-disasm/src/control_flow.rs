//! Linear sweep and conservative recursive traversal.

use crate::decode::{decode_at, DecodeOptions, DecodedInstruction};
use crate::diagnostics::{Diagnostic, DisasmError};
use crate::regions::{Classification, Provenance};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataBlock {
    pub address: u16,
    pub file_offset: Option<usize>,
    pub bytes: Vec<u8>,
    pub classification: Classification,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisItem {
    Instruction(DecodedInstruction),
    Data(DataBlock),
}

impl AnalysisItem {
    pub fn address(&self) -> u16 {
        match self {
            Self::Instruction(i) => i.address,
            Self::Data(d) => d.address,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Instruction(i) => &i.bytes,
            Self::Data(d) => &d.bytes,
        }
    }
    pub fn classification(&self) -> Classification {
        match self {
            Self::Instruction(_) => Classification::Code,
            Self::Data(d) => d.classification,
        }
    }
    pub fn provenance(&self) -> Provenance {
        match self {
            Self::Instruction(i) => i.provenance,
            Self::Data(d) => d.provenance,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    pub origin: u16,
    pub items: Vec<AnalysisItem>,
    pub labels: BTreeMap<u16, String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Analysis {
    pub fn instructions(&self) -> impl Iterator<Item = &DecodedInstruction> {
        self.items.iter().filter_map(|item| match item {
            AnalysisItem::Instruction(i) => Some(i),
            _ => None,
        })
    }
    pub fn data(&self) -> impl Iterator<Item = &DataBlock> {
        self.items.iter().filter_map(|item| match item {
            AnalysisItem::Data(d) => Some(d),
            _ => None,
        })
    }
    pub fn item_at(&self, address: u16) -> Option<&AnalysisItem> {
        self.items.iter().find(|item| item.address() == address)
    }
    pub fn synthesize_labels(&mut self) {
        let existing = self.labels.clone();
        for instruction in self.instructions().cloned().collect::<Vec<_>>() {
            let label_target = instruction.is_branch()
                || instruction.is_call()
                || (instruction.is_unconditional_jump()
                    && instruction.metadata.addressing_mode == nes_isa::AddressingMode::Absolute);
            if label_target {
                if let Some(target) = instruction.target {
                    if !existing.contains_key(&target) && !self.labels.contains_key(&target) {
                        let prefix = if instruction.is_call() { "sub" } else { "loc" };
                        self.labels.insert(target, format!("{prefix}_{target:04X}"));
                    }
                }
            }
        }
    }
    pub fn sort_items(&mut self) {
        self.items.sort_by_key(AnalysisItem::address);
    }

    /// Reassemble a contiguous raw analysis without changing any byte.
    pub fn reassemble(&self) -> Result<Vec<u8>, DisasmError> {
        let mut output = Vec::new();
        let mut expected = self.origin;
        for item in &self.items {
            if item.address() != expected {
                return Err(DisasmError::InvalidRange {
                    start: expected,
                    end: item.address(),
                });
            }
            output.extend_from_slice(item.bytes());
            expected = expected.wrapping_add(item.bytes().len() as u16);
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinearSweepOptions {
    pub decode: DecodeOptions,
    pub file_offset: Option<usize>,
    pub preserve_invalid: bool,
}
impl Default for LinearSweepOptions {
    fn default() -> Self {
        Self {
            decode: DecodeOptions::default(),
            file_offset: None,
            preserve_invalid: true,
        }
    }
}

pub fn linear_sweep(
    bytes: &[u8],
    origin: u16,
    options: LinearSweepOptions,
) -> Result<Analysis, DisasmError> {
    validate_span(origin, bytes.len())?;
    if bytes.is_empty() {
        return Err(DisasmError::EmptyInput);
    }
    let mut result = Analysis {
        origin,
        ..Analysis::default()
    };
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let address = origin.wrapping_add(cursor as u16);
        match decode_at(
            &bytes[cursor..],
            address,
            options.file_offset.map(|v| v + cursor),
            options.decode,
        ) {
            Ok(mut instruction) => {
                instruction.provenance = Provenance::LinearSweep;
                cursor += instruction.bytes.len();
                result.items.push(AnalysisItem::Instruction(instruction));
            }
            Err(error) => {
                result.diagnostics.push(Diagnostic::warning(
                    "decode",
                    error.to_string(),
                    Some(address),
                ));
                if !options.preserve_invalid {
                    return Err(error);
                }
                let start = cursor;
                cursor += 1;
                result.items.push(AnalysisItem::Data(DataBlock {
                    address,
                    file_offset: options.file_offset.map(|v| v + start),
                    bytes: bytes[start..cursor].to_vec(),
                    classification: Classification::Unknown,
                    provenance: Provenance::LinearSweep,
                }));
            }
        }
    }
    result.synthesize_labels();
    result.sort_items();
    Ok(result)
}

#[derive(Clone, Debug)]
pub struct TraversalOptions {
    pub decode: DecodeOptions,
    pub file_offset: Option<usize>,
    pub max_instructions: usize,
    pub preserve_unreachable: bool,
    pub indirect_targets: BTreeMap<u16, Vec<u16>>,
    pub user_labels: BTreeMap<u16, String>,
}

impl Default for TraversalOptions {
    fn default() -> Self {
        Self {
            decode: DecodeOptions::default(),
            file_offset: None,
            max_instructions: 100_000,
            preserve_unreachable: true,
            indirect_targets: BTreeMap::new(),
            user_labels: BTreeMap::new(),
        }
    }
}

pub fn recursive_traversal(
    bytes: &[u8],
    origin: u16,
    entry_points: &[u16],
    options: TraversalOptions,
) -> Result<Analysis, DisasmError> {
    validate_span(origin, bytes.len())?;
    if bytes.is_empty() {
        return Err(DisasmError::EmptyInput);
    }
    let mut result = Analysis {
        origin,
        labels: options.user_labels.clone(),
        ..Analysis::default()
    };
    let mut queue = VecDeque::from(entry_points.to_vec());
    let mut visited = BTreeSet::new();
    let mut starts = BTreeSet::new();
    let mut code_ranges = Vec::<(usize, usize)>::new();
    while let Some(address) = queue.pop_front() {
        if visited.contains(&address) {
            continue;
        }
        let Some(offset) = address.checked_sub(origin).map(usize::from) else {
            result.diagnostics.push(Diagnostic::warning(
                "unmapped-entry",
                format!("entry ${address:04X} is outside the analysed span"),
                Some(address),
            ));
            continue;
        };
        if offset >= bytes.len() {
            result.diagnostics.push(Diagnostic::warning(
                "unmapped-entry",
                format!("entry ${address:04X} is outside the analysed span"),
                Some(address),
            ));
            continue;
        }
        let mut cursor = offset;
        while cursor < bytes.len() && visited.insert(origin.wrapping_add(cursor as u16)) {
            starts.insert(origin.wrapping_add(cursor as u16));
            if visited.len() > options.max_instructions {
                result.diagnostics.push(Diagnostic::warning(
                    "analysis-limit",
                    "recursive traversal instruction limit reached",
                    Some(origin.wrapping_add(cursor as u16)),
                ));
                break;
            }
            let pc = origin.wrapping_add(cursor as u16);
            let mut instruction = match decode_at(
                &bytes[cursor..],
                pc,
                options.file_offset.map(|v| v + cursor),
                options.decode,
            ) {
                Ok(value) => value,
                Err(error) => {
                    result.diagnostics.push(Diagnostic::warning(
                        "decode",
                        error.to_string(),
                        Some(pc),
                    ));
                    break;
                }
            };
            instruction.provenance = Provenance::RecursiveTraversal;
            let next = cursor + instruction.bytes.len();
            code_ranges.push((cursor, next));
            let fallthrough = pc.wrapping_add(instruction.length() as u16);
            if instruction.is_branch() {
                if let Some(target) = instruction.target {
                    queue.push_back(target);
                }
                if next < bytes.len() {
                    cursor = next;
                    continue;
                }
                break;
            }
            if instruction.is_call() {
                if let Some(target) = instruction.target {
                    queue.push_back(target);
                }
                if next < bytes.len() {
                    cursor = next;
                    continue;
                }
                break;
            }
            if instruction.is_unconditional_jump() {
                if let Some(targets) = options.indirect_targets.get(&pc) {
                    queue.extend(targets.iter().copied());
                } else if instruction.metadata.addressing_mode == nes_isa::AddressingMode::Absolute
                {
                    if let Some(target) = instruction.target {
                        queue.push_back(target);
                    }
                }
                break;
            }
            if instruction.is_return() || instruction.metadata.mnemonic == nes_isa::Opcode::STP {
                break;
            }
            if let Some(targets) = options.indirect_targets.get(&pc) {
                queue.extend(targets.iter().copied());
            }
            let _ = fallthrough;
            if next >= bytes.len() {
                break;
            }
            cursor = next;
        }
    }
    let mut occupied = BTreeSet::new();
    for (start, end) in code_ranges {
        for offset in start..end {
            occupied.insert(offset);
        }
    }
    // Re-decode only the starts, so an instruction is emitted once even when
    // two entry paths converge on it.
    for address in starts.iter().copied() {
        let offset = usize::from(address - origin);
        if occupied.contains(&offset) && !result.items.iter().any(|item| item.address() == address)
        {
            if let Ok(mut instruction) = decode_at(
                &bytes[offset..],
                address,
                options.file_offset.map(|v| v + offset),
                options.decode,
            ) {
                instruction.provenance = Provenance::RecursiveTraversal;
                result.items.push(AnalysisItem::Instruction(instruction));
            }
        }
    }
    if options.preserve_unreachable {
        let mut cursor = 0usize;
        while cursor < bytes.len() {
            if occupied.contains(&cursor) {
                cursor += 1;
                continue;
            }
            let start = cursor;
            while cursor < bytes.len() && !occupied.contains(&cursor) {
                cursor += 1;
            }
            result.items.push(AnalysisItem::Data(DataBlock {
                address: origin.wrapping_add(start as u16),
                file_offset: options.file_offset.map(|v| v + start),
                bytes: bytes[start..cursor].to_vec(),
                classification: Classification::Unknown,
                provenance: Provenance::Unknown,
            }));
        }
    }
    result.synthesize_labels();
    result.sort_items();
    Ok(result)
}

fn validate_span(origin: u16, len: usize) -> Result<(), DisasmError> {
    if len == 0 {
        return Ok(());
    }
    if usize::from(origin) + len > 0x10000 {
        return Err(DisasmError::InvalidOrigin {
            origin: origin as u32,
            length: len,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_keeps_unreachable_bytes_as_data() {
        let bytes = [0x4c, 0x04, 0x80, 0xff, 0xa9, 0x01, 0x60];
        let result =
            recursive_traversal(&bytes, 0x8000, &[0x8000], TraversalOptions::default()).unwrap();
        assert!(result.instructions().any(|i| i.address == 0x8004));
        assert!(result.data().any(|d| d.bytes == [0xff]));
    }
}
