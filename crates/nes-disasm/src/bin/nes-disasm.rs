use nes_disasm::chr::{inspect_chr, render_contact_sheet_ppm};
use nes_disasm::{
    format, linear_sweep, recursive_traversal, Cpu, DecodeOptions, Dialect, FormatOptions,
    InesImage, LinearSweepOptions, OutputFormat, TraversalOptions,
};
use std::{env, fs, path::PathBuf};

fn parse_number(value: &str) -> Result<u16, String> {
    let value = value.trim();
    let (radix, digits) = if let Some(rest) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = value.strip_prefix('$') {
        (16, rest)
    } else {
        (10, value)
    };
    u16::from_str_radix(digits, radix).map_err(|_| format!("invalid number `{value}`"))
}

fn parse_range(value: &str) -> Result<(u16, u16), String> {
    let (start, end) = value
        .split_once(':')
        .ok_or_else(|| "range must be START:END".to_string())?;
    Ok((parse_number(start)?, parse_number(end)?))
}

fn usage() -> &'static str {
    "usage: nes-disasm INPUT [--format asm|annotated|json] [--origin ADDR] [--entry ADDR]\n\
     [--range START:END] [--linear] [--unofficial] [--reassemble OUTPUT]\n\
     [--chr-sheet OUTPUT] [--columns N]\n\
     `inspect INPUT --chr-sheet OUTPUT` renders the image CHR as a PPM sheet."
}

fn main() {
    if let Err(error) = run() {
        eprintln!("nes-disasm: {error}");
        eprintln!("{}", usage());
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let inspect = args.first().is_some_and(|arg| arg == "inspect");
    if inspect {
        args.remove(0);
    }
    let input = args.first().cloned().ok_or("missing input")?;
    args.remove(0);
    let mut format_kind = OutputFormat::Ca65;
    let mut origin = None;
    let mut entries = Vec::new();
    let mut range = None;
    let mut linear = inspect;
    let mut unofficial = false;
    let mut reassemble = None::<PathBuf>;
    let mut chr_sheet = None::<PathBuf>;
    let mut columns = 16usize;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--format" => {
                format_kind = match value(&mut index)?.as_str() {
                    "asm" | "ca65" => OutputFormat::Ca65,
                    "annotated" => OutputFormat::Annotated,
                    "json" => OutputFormat::Json,
                    other => return Err(format!("unknown format `{other}`").into()),
                };
            }
            "--origin" => origin = Some(parse_number(&value(&mut index)?)?),
            "--entry" => entries.push(parse_number(&value(&mut index)?)?),
            "--range" => range = Some(parse_range(&value(&mut index)?)?),
            "--linear" => linear = true,
            "--unofficial" => unofficial = true,
            "--reassemble" => reassemble = Some(PathBuf::from(value(&mut index)?)),
            "--chr-sheet" => chr_sheet = Some(PathBuf::from(value(&mut index)?)),
            "--columns" => columns = value(&mut index)?.parse()?,
            other => return Err(format!("unknown option `{other}`").into()),
        }
        index += 1;
    }

    let source = fs::read(&input)?;
    if let Ok(image) = InesImage::parse(&source) {
        eprintln!(
            "format={:?} PRG={} CHR={} mapper={} trainer={} mode={}",
            image.header.format,
            image.header.prg_rom_size,
            image.header.chr_rom_size,
            image.header.mapper,
            image.header.has_trainer,
            if linear { "linear" } else { "recursive" },
        );
        if let Some(path) = reassemble {
            fs::write(path, image.reassemble())?;
        }
        if let Some(path) = chr_sheet {
            let chr = image
                .chr
                .as_ref()
                .map(|range| &image.bytes[range.clone()])
                .unwrap_or(&[]);
            if chr.is_empty() {
                return Err("image has no CHR-ROM to render".into());
            }
            let inspection = inspect_chr(chr);
            let tiles = inspection
                .tiles()
                .iter()
                .map(|record| record.tile)
                .collect::<Vec<_>>();
            fs::write(
                path,
                render_contact_sheet_ppm(
                    &tiles,
                    columns,
                    2,
                    [[0, 0, 0], [85, 85, 85], [170, 170, 170], [255, 255, 255]],
                )?,
            )?;
        }
        if inspect {
            return Ok(());
        }
        let mapping = image.nrom()?;
        let default_origin = if image.header.prg_rom_size == 0x4000 {
            0xc000
        } else {
            0x8000
        };
        let cpu_origin = origin.unwrap_or(default_origin);
        let prg = &image.bytes[image.prg.clone()];
        let bytes = if cpu_origin == default_origin {
            prg.to_vec()
        } else {
            mapping.read(cpu_origin, prg.len())?
        };
        let decode = DecodeOptions {
            cpu: Cpu::Ricoh2A03,
            dialect: if unofficial {
                Dialect::Ricoh2A03
            } else {
                Dialect::Official6502
            },
        };
        let analysis = if linear {
            linear_sweep(
                &bytes,
                cpu_origin,
                LinearSweepOptions {
                    decode,
                    file_offset: Some(image.prg.start),
                    preserve_invalid: true,
                },
            )?
        } else {
            let vectors = mapping.vectors()?;
            let vector_entry = vectors[1];
            if entries.is_empty() {
                entries.push(vector_entry);
            }
            recursive_traversal(
                &bytes,
                cpu_origin,
                &entries,
                TraversalOptions {
                    decode,
                    file_offset: Some(image.prg.start),
                    ..TraversalOptions::default()
                },
            )?
        };
        print!(
            "{}",
            format(&analysis, format_kind, FormatOptions::default())
        );
    } else {
        let cpu_origin = origin.ok_or("raw binaries require --origin")?;
        let (start, end) =
            range.unwrap_or((cpu_origin, cpu_origin.wrapping_add(source.len() as u16)));
        if end < start || usize::from(end - start) > source.len() {
            return Err("range is outside the raw input".into());
        }
        let offset = usize::from(start - cpu_origin);
        let bytes = &source[offset..offset + usize::from(end - start)];
        let decode = DecodeOptions {
            cpu: Cpu::Ricoh2A03,
            dialect: if unofficial {
                Dialect::Ricoh2A03
            } else {
                Dialect::Official6502
            },
        };
        let analysis = if linear || entries.is_empty() {
            linear_sweep(
                bytes,
                start,
                LinearSweepOptions {
                    decode,
                    file_offset: Some(offset),
                    preserve_invalid: true,
                },
            )?
        } else {
            recursive_traversal(
                bytes,
                start,
                &entries,
                TraversalOptions {
                    decode,
                    file_offset: Some(offset),
                    ..TraversalOptions::default()
                },
            )?
        };
        if let Some(path) = reassemble {
            fs::write(path, analysis.reassemble()?)?;
        }
        print!(
            "{}",
            format(&analysis, format_kind, FormatOptions::default())
        );
    }
    Ok(())
}
