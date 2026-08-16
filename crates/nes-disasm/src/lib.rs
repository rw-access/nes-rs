//! Conservative, lossless 6502/NES disassembly primitives.
//!
//! The crate deliberately keeps bytes on every output item. A valid opcode
//! value is not evidence that a byte is code; use [`linear_sweep`] for known
//! code ranges and [`recursive_traversal`] for entry-point driven analysis.

pub mod chr;
pub mod control_flow;
pub mod decode;
pub mod diagnostics;
pub mod format;
pub mod operand;
pub mod regions;

pub use control_flow::{
    linear_sweep, recursive_traversal, Analysis, AnalysisItem, DataBlock, LinearSweepOptions,
    TraversalOptions,
};
pub use decode::{decode_at, DecodeOptions, DecodedInstruction};
pub use diagnostics::{Diagnostic, DiagnosticSeverity, DisasmError};
pub use format::{format, format_annotated, format_ca65, format_json, FormatOptions, OutputFormat};
pub use operand::DecodedOperand;
pub use regions::{
    Classification, InesFormat, InesHeader, InesImage, NromMapping, Provenance, Region, RegionKind,
};

pub use nes_isa::{AddressingMode, Cpu, Dialect, Opcode, OpcodeClass, OpcodeMetadata};

/// Decode a raw CPU range with an explicit origin and optional file offset.
pub fn disassemble(
    bytes: &[u8],
    origin: u16,
    options: LinearSweepOptions,
) -> Result<Analysis, DisasmError> {
    linear_sweep(bytes, origin, options)
}

/// Parse and map a mapper-0 image, retaining the original bytes for exact
/// reconstruction by callers.
pub fn parse_nrom(bytes: &[u8]) -> Result<InesImage, DisasmError> {
    let image = InesImage::parse(bytes)?;
    image.nrom()?;
    Ok(image)
}
