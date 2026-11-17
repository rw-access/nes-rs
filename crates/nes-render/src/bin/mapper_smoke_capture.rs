//! Isolated worker for scripts/mapper_smoke.py. ROM execution is bounded by
//! the parent process so even an emulator deadlock cannot stall the suite.
use image::{ImageBuffer, Rgba, RgbaImage};
use nes_core::{
    cartridge, controller::ButtonState, ines, Console, FRAME_HEIGHT, FRAME_WIDTH, NES_PALETTE_RGBA,
};
use std::{collections::BTreeSet, env, fs, path::Path};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--sheet") {
        return sheet(&args[1..]);
    }
    if args.len() != 7 {
        return Err(
            "usage: mapper_smoke_capture MAPPER ROM BOOT START END OUTPUT_DIR START_BUTTONS".into(),
        );
    }
    let expected: u8 = args[0].parse()?;
    let boot: u64 = args[2].parse()?;
    let start: u64 = args[3].parse()?;
    let end: u64 = args[4].parse()?;
    let buttons: u8 = args[6].parse()?;
    if !(0 < boot && boot < start && start < end && end - start > 1) {
        return Err("require 0 < BOOT < START and START + 1 < END".into());
    }
    let bytes = fs::read(&args[1])?;
    let (cart, mapper) = ines::load(&mut std::io::Cursor::new(&bytes)).ok_or("invalid ROM")?;
    if mapper != expected {
        return Err(format!("expected mapper {expected}, loaded {mapper}").into());
    }
    // The cartridge factory silently treats small images with these IDs as
    // NROM. Such a boot must not count as exercising the requested mapper.
    if matches!(mapper, 64 | 96 | 224)
        && bytes.starts_with(b"NES\x1a")
        && bytes[4] <= 2
        && bytes[5] <= 1
    {
        return Err("ROM uses the NROM compatibility fallback, not the requested mapper".into());
    }
    let mut console = if mapper == 0 {
        Console::new_nrom(cart)
    } else {
        Console::new(cartridge::new(cart, mapper).ok_or("unsupported mapper")?)
    };
    let out = Path::new(&args[5]);
    fs::create_dir_all(out)?;
    let mut sampled = BTreeSet::new();
    let mut stages = Vec::new();
    for number in 1..=end {
        console.update_buttons(ButtonState::from_bits(
            if number == start || number == start + 1 {
                buttons
            } else {
                0
            },
        ));
        let frame = console.next_frame();
        if number % 60 == 0 {
            sampled.insert(hash(frame.pixels));
        }
        let name = if number == boot {
            "boot"
        } else if number == end {
            "post-start"
        } else {
            continue;
        };
        let mut counts = [0usize; 64];
        let mut rgba = Vec::with_capacity(FRAME_WIDTH * FRAME_HEIGHT * 4);
        for row in frame.pixels {
            for &index in row {
                counts[usize::from(index & 63)] += 1;
                rgba.extend_from_slice(&NES_PALETTE_RGBA[usize::from(index & 63)]);
            }
        }
        let image: RgbaImage = ImageBuffer::from_raw(FRAME_WIDTH as u32, FRAME_HEIGHT as u32, rgba)
            .ok_or("invalid frame")?;
        image.save(out.join(format!("{name}.png")))?;
        stages.push(format!("{{\"stage\":\"{name}\",\"frame\":{number},\"hash\":\"{:016x}\",\"colors\":{},\"dominant_fraction\":{:.6}}}",
            hash(frame.pixels), counts.iter().filter(|&&n| n > 0).count(), *counts.iter().max().unwrap() as f64 / (FRAME_WIDTH * FRAME_HEIGHT) as f64));
    }
    fs::write(
        out.join("metrics.json"),
        format!(
            "{{\"sampled_unique_frames\":{},\"stages\":[{}]}}\n",
            sampled.len(),
            stages.join(",")
        ),
    )?;
    Ok(())
}

fn hash(pixels: &[[u8; FRAME_WIDTH]; FRAME_HEIGHT]) -> u64 {
    pixels.iter().flatten().fold(0xcbf29ce484222325, |h, &p| {
        (h ^ u64::from(p)).wrapping_mul(0x100000001b3)
    })
}

fn sheet(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        return Err("usage: --sheet OUTPUT CASE_DIR...".into());
    }
    let cases = &args[1..];
    let mut out = RgbaImage::from_pixel(
        1560,
        cases.len().div_ceil(3) as u32 * 264,
        Rgba([24, 24, 24, 255]),
    );
    let digits = [
        0b111101101101111u16,
        0b010110010010111,
        0b111001111100111,
        0b111001111001111,
        0b101101111001001,
        0b111100111001111,
        0b111100111101111,
        0b111001001001001,
        0b111101111101111,
        0b111101111001111,
    ];
    for (i, dir) in cases.iter().enumerate() {
        let x = (i % 3) as u32 * 520;
        let y = (i / 3) as u32 * 264;
        let label = Path::new(dir)
            .file_name()
            .ok_or("missing case name")?
            .to_string_lossy();
        for (n, digit) in label.chars().filter_map(|c| c.to_digit(10)).enumerate() {
            for bit in 0..15 {
                if digits[digit as usize] & (1 << (14 - bit)) != 0 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            out.put_pixel(
                                x + n as u32 * 8 + bit % 3 * 2 + dx,
                                y + bit / 3 * 2 + dy,
                                Rgba([240, 240, 240, 255]),
                            );
                        }
                    }
                }
            }
        }
        for (n, stage) in ["boot", "post-start"].iter().enumerate() {
            let path = Path::new(dir).join(format!("{stage}.png"));
            if path.exists() {
                image::imageops::overlay(
                    &mut out,
                    &image::open(path)?.to_rgba8(),
                    i64::from(x + n as u32 * 256),
                    i64::from(y + 16),
                );
            }
        }
    }
    out.save(&args[0])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_exactly_one_row_for_every_supported_mapper() {
        let mut bytes = vec![0u8; 16 + 8 * 0x4000 + 4 * 0x2000];
        bytes[..4].copy_from_slice(b"NES\x1a");
        bytes[4] = 8;
        bytes[5] = 4;
        let (cart, _) = ines::load(&mut std::io::Cursor::new(bytes)).unwrap();
        let supported: BTreeSet<_> = (0..=255)
            .filter(|&id| cartridge::new(cart.clone(), id).is_some())
            .collect();
        let rows: Vec<u8> = include_str!("../../../../tests/mapper-smoke/cases.tsv")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.split('\t').next().unwrap().parse().unwrap())
            .collect();
        let manifest: BTreeSet<_> = rows.iter().copied().collect();
        assert_eq!(manifest.len(), rows.len(), "duplicate mapper rows");
        assert_eq!(
            manifest, supported,
            "update smoke coverage when mapper support changes"
        );
    }
}
