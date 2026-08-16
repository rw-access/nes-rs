use image::{ImageBuffer, Rgba};
use nes_core::{
    cartridge, console::Console, controller::ButtonState, ines, FRAME_HEIGHT, FRAME_WIDTH,
    NES_PALETTE_RGBA,
};
use std::{collections::BTreeMap, env, fs::File, path::PathBuf};

fn main() {
    let mut args = env::args_os().skip(1);
    let frames: u64 = args
        .next()
        .expect("usage: capture_png FRAMES ROM OUT")
        .to_string_lossy()
        .parse()
        .expect("FRAMES must be an integer");
    let rom_path = PathBuf::from(args.next().expect("usage: capture_png FRAMES ROM OUT"));
    let output_path = PathBuf::from(args.next().expect("usage: capture_png FRAMES ROM OUT"));
    let mut input_path = None;
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--input" => {
                input_path = Some(PathBuf::from(
                    args.next()
                        .expect("usage: capture_png FRAMES ROM OUT [--input TRANSCRIPT]"),
                ))
            }
            _ => panic!("unknown argument: {arg:?}"),
        }
    }
    let input = input_path.map(|path| parse_input(&path));

    let mut rom = File::open(&rom_path).expect("failed to open ROM");
    let (cartridge, mapper_number) = ines::load(&mut rom).expect("failed to parse ROM");
    let mut console = match mapper_number {
        0 => Console::new_nrom(cartridge),
        _ => Console::new(cartridge::new(cartridge, mapper_number).expect("unsupported mapper")),
    };

    for frame_number in 1..=frames {
        if let Some(input) = &input {
            console.update_buttons(ButtonState::from_bits(
                *input.get(&frame_number).unwrap_or(&0),
            ));
        }
        console.next_frame();
    }

    let mut rgba = Vec::with_capacity(FRAME_WIDTH * FRAME_HEIGHT * 4);
    {
        let frame = console.next_frame();
        for row in frame.pixels {
            for &palette_index in row {
                rgba.extend_from_slice(&NES_PALETTE_RGBA[palette_index as usize & 0x3f]);
            }
        }
    }

    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(FRAME_WIDTH as u32, FRAME_HEIGHT as u32, rgba)
        .expect("invalid framebuffer dimensions");
    image.save(&output_path).expect("failed to save PNG");
}

fn parse_input(path: &std::path::Path) -> BTreeMap<u64, u8> {
    std::fs::read_to_string(path)
        .expect("failed to read input transcript")
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
