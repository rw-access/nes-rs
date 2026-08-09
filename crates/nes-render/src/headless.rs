//! Headless iNES test-ROM execution and the Blargg result protocol.

use nes_core::{cartridge, console::Console, ines};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

pub const SIGNATURE: [u8; 3] = [0xde, 0xb0, 0x61];
pub const STATUS_ADDRESS: u16 = 0x6000;
pub const SIGNATURE_ADDRESS: u16 = 0x6001;
pub const TEXT_ADDRESS: u16 = 0x6004;
const TEXT_END: u16 = 0x8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    NeedsReset,
    Complete(u8),
}

impl Status {
    pub fn decode(value: u8) -> Self {
        match value {
            0x80 => Self::Running,
            0x81 => Self::NeedsReset,
            value => Self::Complete(value),
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Self::Running => 0x80,
            Self::NeedsReset => 0x81,
            Self::Complete(value) => value,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct TestResult {
    pub path: PathBuf,
    pub signature_valid: bool,
    pub status: Option<Status>,
    pub text: String,
    pub frames: u64,
}

fn extract_text(read: impl Fn(u16) -> u8) -> String {
    let mut text = Vec::new();
    for address in TEXT_ADDRESS..TEXT_END {
        let byte = read(address);
        if byte == 0 {
            break;
        }
        text.push(byte);
    }
    String::from_utf8_lossy(&text).into_owned()
}

fn read_result(console: &Console, path: &Path, frames: u64) -> TestResult {
    let signature_valid = (0..3)
        .map(|offset| console.read_memory(SIGNATURE_ADDRESS + offset))
        .eq(SIGNATURE);
    let status = if signature_valid {
        Some(Status::decode(console.read_memory(STATUS_ADDRESS)))
    } else {
        None
    };
    let text = extract_text(|address| console.read_memory(address));

    TestResult {
        path: path.to_path_buf(),
        signature_valid,
        status,
        text,
        frames,
    }
}

/// Run an NROM test ROM until it reports a result or the frame limit expires.
pub fn run_rom(path: &Path, frame_limit: u64) -> io::Result<TestResult> {
    let mut file = File::open(path)?;
    let (cartridge, mapper_number) = ines::load(&mut file)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid iNES ROM"))?;
    if mapper_number != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} uses mapper {}, expected NROM/mapper 0",
                path.display(),
                mapper_number
            ),
        ));
    }

    let mut console = Console::new(cartridge::new(cartridge, mapper_number).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported mapper or cartridge",
        )
    })?);

    for frame in 0..frame_limit {
        let _output = console.next_frame();
        let result = read_result(&console, path, frame + 1);
        if result.signature_valid && !matches!(result.status, Some(Status::Running)) {
            return Ok(result);
        }
    }

    Ok(read_result(&console, path, frame_limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn status_values_match_documented_protocol() {
        assert_eq!(Status::decode(0x80), Status::Running);
        assert_eq!(Status::decode(0x81), Status::NeedsReset);
        assert_eq!(Status::decode(0), Status::Complete(0));
        assert_eq!(Status::decode(7), Status::Complete(7));
        assert_eq!(Status::Complete(7).byte(), 7);
    }

    #[test]
    fn text_extraction_stops_at_zero_and_handles_ascii() {
        let mut memory = BTreeMap::new();
        memory.insert(TEXT_ADDRESS, b'P');
        memory.insert(TEXT_ADDRESS + 1, b'a');
        memory.insert(TEXT_ADDRESS + 2, b's');
        memory.insert(TEXT_ADDRESS + 3, b's');
        assert_eq!(
            extract_text(|address| memory.get(&address).copied().unwrap_or(0)),
            "Pass"
        );
    }
}
