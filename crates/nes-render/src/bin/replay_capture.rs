use nes_core::{console::Console, controller::ButtonState, ines};
use std::{env, fs::File, io::Write, path::PathBuf};

fn main() {
    let mut args = env::args_os().skip(1);
    let transcript = PathBuf::from(
        args.next()
            .expect("usage: replay_capture TRANSCRIPT ROM OUT_DIR START END STEP"),
    );
    let rom = PathBuf::from(
        args.next()
            .expect("usage: replay_capture TRANSCRIPT ROM OUT_DIR START END STEP"),
    );
    let out_dir = PathBuf::from(
        args.next()
            .expect("usage: replay_capture TRANSCRIPT ROM OUT_DIR START END STEP"),
    );
    let start: u64 = args
        .next()
        .expect("missing START")
        .to_string_lossy()
        .parse()
        .unwrap();
    let end: u64 = args
        .next()
        .expect("missing END")
        .to_string_lossy()
        .parse()
        .unwrap();
    let step: u64 = args
        .next()
        .expect("missing STEP")
        .to_string_lossy()
        .parse()
        .unwrap();

    let input =
        parse_input(&std::fs::read_to_string(transcript).expect("failed to read transcript"));
    let mut file = File::open(rom).expect("failed to open ROM");
    let (cartridge, mapper_number) = ines::load(&mut file).expect("failed to parse ROM");
    assert_eq!(
        mapper_number, 0,
        "replay_capture currently supports NROM only"
    );
    let mut console = Console::new_nrom(cartridge);
    std::fs::create_dir_all(&out_dir).expect("failed to create output directory");

    for frame_number in 1..=end {
        console.update_buttons(ButtonState::from_bits(
            *input.get(&frame_number).unwrap_or(&0),
        ));
        let frame = console.next_frame();
        if frame_number >= start && (frame_number - start) % step == 0 {
            write_ppm(
                &out_dir.join(format!("frame-{frame_number:08}.ppm")),
                frame.pixels,
            );
        }
    }
}

fn parse_input(transcript: &str) -> std::collections::BTreeMap<u64, u8> {
    transcript
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != "frame" {
                return None;
            }
            let frame = fields.next()?.parse().ok()?;
            let buttons = fields.next()?.strip_prefix("buttons=0x")?;
            Some((frame, u8::from_str_radix(buttons, 16).ok()?))
        })
        .collect()
}

fn write_ppm(
    path: &std::path::Path,
    pixels: &[[u8; nes_core::FRAME_WIDTH]; nes_core::FRAME_HEIGHT],
) {
    let mut output = format!(
        "P6\n{} {}\n255\n",
        nes_core::FRAME_WIDTH,
        nes_core::FRAME_HEIGHT
    )
    .into_bytes();
    for row in pixels {
        for &palette_index in row {
            let [_, r, g, b] =
                nes_core::video::NES_PALETTE_RGB[palette_index as usize & 0x3f].to_be_bytes();
            output.extend_from_slice(&[r, g, b]);
        }
    }
    File::create(path)
        .expect("failed to create frame")
        .write_all(&output)
        .expect("failed to write frame");
}
