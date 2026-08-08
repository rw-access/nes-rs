use clap::Parser;
use nes::headless::{run_rom, Status};
use std::{path::PathBuf, process::ExitCode};

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

fn print_result(result: &nes::headless::TestResult, frame_limit: u64) -> bool {
    println!("{} ({} frames)", result.path.display(), result.frames);
    if !result.signature_valid {
        println!("  no test signature at $6001-$6003 (expected DE B0 61)");
        println!("  stopped after {} frame limit", frame_limit);
        return false;
    }

    println!(
        "  status: ${:02X}",
        result.status.map(Status::byte).unwrap_or(0)
    );
    if !result.text.is_empty() {
        println!("  {}", result.text);
    }

    if matches!(result.status, Some(Status::Running)) {
        println!("  stopped after {} frame limit", frame_limit);
        return false;
    }

    matches!(result.status, Some(Status::Complete(0)))
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
