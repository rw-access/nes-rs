use std::{env, fs, process::ExitCode};

use nes_asm::{write_ines, Assembler, InesOptions};

fn usage() {
    eprintln!("usage: nes-asm <input.asm> -o <output> [--format bin|ines] [--prg-banks 1|2] [--chr-banks 0|1]");
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
        return ExitCode::from(2);
    }
    let input = args[1].clone();
    let mut output = None;
    let mut format = "bin";
    let mut options = InesOptions::default();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                output = args.get(i).cloned();
            }
            "--format" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    format = v;
                }
            }
            "--prg-banks" => {
                i += 1;
                options.prg_banks = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "--chr-banks" => {
                i += 1;
                options.chr_banks = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(99);
            }
            "--vertical" => options.vertical_mirroring = true,
            "--battery" => options.battery = true,
            _ => {
                usage();
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let Some(output) = output else {
        usage();
        return ExitCode::from(2);
    };
    let object = match Assembler::default().assemble_file(&input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let bytes = match format {
        "bin" => object.flat_binary(),
        "ines" | "nes" => match write_ines(&object, &options) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(1);
            }
        },
        _ => {
            usage();
            return ExitCode::from(2);
        }
    };
    if let Err(e) = fs::write(&output, bytes) {
        eprintln!("cannot write {output}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
