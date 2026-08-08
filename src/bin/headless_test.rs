use clap::Parser;
use nes::{cartridge, console::Console, ines};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    process::ExitCode,
};

const SIGNATURE: [u8; 3] = [0xde, 0xb0, 0x61];
const STATUS_ADDRESS: u16 = 0x6000;
const SIGNATURE_ADDRESS: u16 = 0x6001;
const TEXT_ADDRESS: u16 = 0x6004;
const TEXT_END: u16 = 0x8000;

#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(
    name = "headless_test",
    about = "Run NROM test ROMs without SDL and report their $6000 result"
)]
struct Arguments {
    /// Maximum number of video frames to execute for each ROM.
    #[arg(long, default_value_t = 600, value_name = "FRAMES")]
    frames: u64,

    /// One or more iNES ROM files. The runner is intended for mapper-0/NROM ROMs.
    #[arg(required = true, value_name = "ROM")]
    roms: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Running,
    NeedsReset,
    Complete(u8),
}

impl Status {
    fn decode(value: u8) -> Self {
        match value {
            0x80 => Self::Running,
            0x81 => Self::NeedsReset,
            value => Self::Complete(value),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TestResult {
    path: PathBuf,
    signature_valid: bool,
    status: Option<Status>,
    text: String,
    frames: u64,
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

fn run_rom(path: &Path, frame_limit: u64) -> io::Result<TestResult> {
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
        console.next_screen(|_| {});
        let result = read_result(&console, path, frame + 1);
        if result.signature_valid && !matches!(result.status, Some(Status::Running)) {
            return Ok(result);
        }
    }

    Ok(read_result(&console, path, frame_limit))
}

fn print_result(result: &TestResult, frame_limit: u64) -> bool {
    println!("{} ({} frames)", result.path.display(), result.frames);
    if !result.signature_valid {
        println!("  no test signature at $6001-$6003 (expected DE B0 61)");
        println!("  stopped after {} frame limit", frame_limit);
        return false;
    }

    println!("  status: ${:02X}", status_byte(result.status));
    if !result.text.is_empty() {
        println!("  {}", result.text);
    }

    if matches!(result.status, Some(Status::Running)) {
        println!("  stopped after {} frame limit", frame_limit);
        return false;
    }

    matches!(result.status, Some(Status::Complete(0)))
}

fn status_byte(status: Option<Status>) -> u8 {
    match status {
        Some(Status::Running) => 0x80,
        Some(Status::NeedsReset) => 0x81,
        Some(Status::Complete(value)) => value,
        None => 0,
    }
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    if arguments.frames == 0 {
        eprintln!("--frames must be greater than zero");
        return ExitCode::from(2);
    }

    let mut all_passed = true;
    for path in &arguments.roms {
        match run_rom(path, arguments.frames) {
            Ok(result) => {
                all_passed &= print_result(&result, arguments.frames);
            }
            Err(error) => {
                eprintln!("{}: {}", path.display(), error);
                all_passed = false;
            }
        }
    }

    if all_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
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

    #[test]
    fn arguments_accept_multiple_roms_and_frame_limit() {
        let arguments = Arguments::try_parse_from([
            "headless_test",
            "--frames",
            "42",
            "first.nes",
            "second.nes",
        ])
        .unwrap();
        assert_eq!(arguments.frames, 42);
        assert_eq!(
            arguments.roms,
            [PathBuf::from("first.nes"), PathBuf::from("second.nes")]
        );
    }
}
