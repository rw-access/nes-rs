use image::{ImageBuffer, Rgba};
use nes_core::{
    cartridge, console::Console, controller::ButtonState, ines, FRAME_HEIGHT, FRAME_WIDTH,
    NES_PALETTE_RGBA,
};
use std::{collections::HashSet, env, fs, io::Cursor, path::PathBuf};

const SEQUENCES: &[(&str, &[(u64, u64, u8)])] = &[
    ("start", &[(90, 95, 0x08)]),
    ("a", &[(90, 95, 0x01)]),
    ("b", &[(90, 95, 0x02)]),
    ("select", &[(90, 95, 0x04)]),
    ("start-a", &[(90, 95, 0x09)]),
    ("a-then-start", &[(90, 95, 0x01), (150, 155, 0x08)]),
    ("start-then-a", &[(90, 95, 0x08), (150, 155, 0x01)]),
    ("start-then-start", &[(90, 95, 0x08), (150, 155, 0x08)]),
    ("a-then-a", &[(90, 95, 0x01), (150, 155, 0x01)]),
    ("select-then-a", &[(90, 95, 0x04), (150, 155, 0x01)]),
    ("down-a", &[(90, 95, 0x20), (150, 155, 0x01)]),
    ("up-a", &[(90, 95, 0x10), (150, 155, 0x01)]),
    ("left-a", &[(90, 95, 0x40), (150, 155, 0x01)]),
    ("right-a", &[(90, 95, 0x80), (150, 155, 0x01)]),
    ("down-start", &[(90, 95, 0x20), (150, 155, 0x08)]),
    ("up-start", &[(90, 95, 0x10), (150, 155, 0x08)]),
    ("left-start", &[(90, 95, 0x40), (150, 155, 0x08)]),
    ("right-start", &[(90, 95, 0x80), (150, 155, 0x08)]),
    ("select-then-start", &[(90, 95, 0x04), (150, 155, 0x08)]),
];

fn main() {
    let mut args = env::args_os().skip(1);
    let frames: u64 = args
        .next()
        .expect("usage: probe_inputs FRAMES ROM OUT_DIR")
        .to_string_lossy()
        .parse()
        .expect("FRAMES must be an integer");
    let rom_path = PathBuf::from(args.next().expect("usage: probe_inputs FRAMES ROM OUT_DIR"));
    let out_dir = PathBuf::from(args.next().expect("usage: probe_inputs FRAMES ROM OUT_DIR"));
    let only = args.next().map(|value| {
        value
            .to_string_lossy()
            .split(',')
            .map(str::to_owned)
            .collect::<HashSet<_>>()
    });

    let bytes = fs::read(&rom_path).expect("failed to read ROM");
    let stem = rom_path
        .file_stem()
        .expect("ROM has no filename")
        .to_string_lossy()
        .into_owned();
    let mut baseline = run(&bytes, frames, &[]);
    let baseline_flat = is_flat(&baseline);

    println!("ROM\t{}", rom_path.display());
    println!(
        "BASELINE\tflat={}\tunique={}\tsequence=none",
        baseline_flat,
        unique(&baseline)
    );
    for &(name, events) in SEQUENCES {
        if only.as_ref().is_some_and(|names| !names.contains(name)) {
            continue;
        }
        let pixels = run(&bytes, frames, events);
        let (different, mean_abs) = difference(&baseline, &pixels);
        let flat = is_flat(&pixels);
        let sequence_dir = out_dir.join(name);
        fs::create_dir_all(&sequence_dir).expect("failed to create sequence directory");
        save_png(&sequence_dir.join(format!("{stem}.png")), &pixels);
        println!(
            "RESULT\t{}\tdifferent_pixels={}\tchanged_ratio={:.4}\tmean_abs={:.3}\tflat={}\tunique={}",
            name,
            different,
            different as f64 / (FRAME_WIDTH * FRAME_HEIGHT) as f64,
            mean_abs,
            flat,
            unique(&pixels)
        );
    }

    baseline.clear();
}

fn run(bytes: &[u8], frames: u64, events: &[(u64, u64, u8)]) -> Vec<u8> {
    let mut file = Cursor::new(bytes);
    let (cartridge, mapper_number) = ines::load(&mut file).expect("failed to parse ROM");
    let mut console = match mapper_number {
        0 => Console::new_nrom(cartridge),
        _ => Console::new(cartridge::new(cartridge, mapper_number).expect("unsupported mapper")),
    };

    let mut pixels = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    for frame_number in 1..=frames {
        let buttons = events
            .iter()
            .find(|&&(start, end, _)| (start..=end).contains(&frame_number))
            .map_or(0, |&(_, _, buttons)| buttons);
        console.update_buttons(ButtonState::from_bits(buttons));
        let frame = console.next_frame();
        for (destination, row) in pixels.chunks_exact_mut(FRAME_WIDTH).zip(frame.pixels) {
            destination.copy_from_slice(row);
        }
    }
    pixels
}

fn difference(left: &[u8], right: &[u8]) -> (usize, f64) {
    let mut different = 0;
    let mut total = 0u64;
    for (&left, &right) in left.iter().zip(right) {
        if left != right {
            different += 1;
        }
        total += u64::from(left.abs_diff(right));
    }
    (different, total as f64 / left.len() as f64)
}

fn is_flat(pixels: &[u8]) -> bool {
    pixels.iter().all(|&pixel| pixel == pixels[0])
}

fn unique(pixels: &[u8]) -> usize {
    let mut seen = [false; 64];
    for &pixel in pixels {
        seen[(pixel & 0x3f) as usize] = true;
    }
    seen.into_iter().filter(|&present| present).count()
}

fn save_png(path: &std::path::Path, pixels: &[u8]) {
    let mut rgba = Vec::with_capacity(FRAME_WIDTH * FRAME_HEIGHT * 4);
    for &palette_index in pixels {
        rgba.extend_from_slice(&NES_PALETTE_RGBA[palette_index as usize & 0x3f]);
    }
    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(FRAME_WIDTH as u32, FRAME_HEIGHT as u32, rgba)
        .expect("invalid framebuffer dimensions");
    image.save(path).expect("failed to save PNG");
}
