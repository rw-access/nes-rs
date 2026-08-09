use nes_core::{cartridge, console::Console, ines, VideoOutput};
use std::{env, fs::File, hint::black_box, path::PathBuf, time::Instant};

fn main() {
    let mut args = env::args_os().skip(1);
    let frames: u64 = args
        .next()
        .expect("usage: perf_bench FRAMES ROM [--video-off]")
        .to_string_lossy()
        .parse()
        .expect("FRAMES must be an integer");
    let path = PathBuf::from(args.next().expect("usage: perf_bench FRAMES ROM"));
    let video_off = args.next().is_some_and(|arg| arg == "--video-off");

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
    for _ in 0..frames {
        black_box(console.next_frame().frame_number);
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "{frames} frames in {elapsed:.6}s ({:.2} FPS)",
        frames as f64 / elapsed
    );
}
