use nes_core::{cartridge, console::Console, controller::ButtonState, ines, VideoOutput};
use std::{collections::BTreeMap, env, fs::File, hint::black_box, path::PathBuf, time::Instant};

fn main() {
    let mut args = env::args_os().skip(1);
    let frames: u64 = args
        .next()
        .expect("usage: perf_bench FRAMES ROM [--video-off] [--input TRANSCRIPT]")
        .to_string_lossy()
        .parse()
        .expect("FRAMES must be an integer");
    let path = PathBuf::from(args.next().expect("usage: perf_bench FRAMES ROM"));
    let mut video_off = false;
    let mut input_path = None;
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--video-off" => video_off = true,
            "--input" => {
                input_path = Some(PathBuf::from(args.next().expect(
                    "usage: perf_bench FRAMES ROM [--video-off] [--input TRANSCRIPT]",
                )));
            }
            _ => panic!("unknown argument: {arg:?}"),
        }
    }

    let input = input_path.map(|path| parse_input(&path));

    let mut file = File::open(&path).expect("failed to open ROM");
    let (cartridge, mapper_number) = ines::load(&mut file).expect("failed to parse ROM");
    let mut console = match mapper_number {
        0 => Console::new_nrom(cartridge),
        _ => Console::new(cartridge::new(cartridge, mapper_number).expect("unsupported mapper")),
    };
    if video_off {
        console.set_video_output(VideoOutput::Disabled);
    }

    let start = Instant::now();
    for frame_number in 1..=frames {
        if let Some(input) = &input {
            console.update_buttons(ButtonState::from_bits(
                *input.get(&frame_number).unwrap_or(&0),
            ));
        }
        black_box(console.next_frame().frame_number);
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "{frames} frames in {elapsed:.6}s ({:.2} FPS)",
        frames as f64 / elapsed
    );
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
