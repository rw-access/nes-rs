use clap::{Parser, ValueEnum};
use nes_render::{
    export::{export_movie, EncoderPreference, ExportOptions},
    fm2::{Fm2Movie, FrameRate},
};
use std::{fs, path::PathBuf, process::ExitCode};

#[derive(Parser, Debug)]
#[command(name = "fm2_to_mp4", about = "Replay an FCEUX FM2 movie into an MP4")]
struct Args {
    /// Plain-text FCEUX version-3 movie file.
    movie: PathBuf,

    /// ROM used to record the movie.
    rom: PathBuf,

    /// Destination MP4 path.
    #[arg(short, long)]
    output: PathBuf,

    /// Output timing: movie, 60, or 50 frames per second.
    #[arg(long, default_value = "movie")]
    fps: FpsArg,

    /// Integer nearest-neighbor scale factor.
    #[arg(long, default_value_t = 1)]
    scale: u32,

    /// Omit the emulator audio stream.
    #[arg(long)]
    video_only: bool,

    /// Encoder selection. Auto prefers available hardware encoders.
    #[arg(long, value_enum, default_value_t = EncoderArg::Auto)]
    encoder: EncoderArg,

    /// Permit exporting with a ROM whose checksum differs from the movie.
    #[arg(long)]
    ignore_rom_checksum: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum FpsArg {
    Movie,
    #[value(name = "60")]
    Fps60,
    #[value(name = "50")]
    Fps50,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum EncoderArg {
    Auto,
    #[value(name = "videotoolbox")]
    VideoToolbox,
    #[value(name = "nvenc")]
    Nvenc,
    Qsv,
    Amf,
    Libx264,
}

impl From<EncoderArg> for EncoderPreference {
    fn from(value: EncoderArg) -> Self {
        match value {
            EncoderArg::Auto => Self::Auto,
            EncoderArg::VideoToolbox => Self::VideoToolbox,
            EncoderArg::Nvenc => Self::Nvenc,
            EncoderArg::Qsv => Self::Qsv,
            EncoderArg::Amf => Self::Amf,
            EncoderArg::Libx264 => Self::Libx264,
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    if args.scale == 0 {
        return Err("--scale must be at least 1".to_owned());
    }
    let movie = Fm2Movie::read(&args.movie).map_err(|error| error.to_string())?;
    let rom = fs::read(&args.rom)
        .map_err(|error| format!("failed to read {}: {error}", args.rom.display()))?;
    if !args.ignore_rom_checksum {
        movie
            .verify_rom_checksum(&rom)
            .map_err(|error| format!("{}: {error}", args.movie.display()))?;
    }

    let frame_rate = match args.fps {
        FpsArg::Movie => movie.frame_rate,
        FpsArg::Fps60 => FrameRate::FPS_60,
        FpsArg::Fps50 => FrameRate::FPS_50,
    };
    let options = ExportOptions {
        output: args.output.clone(),
        frame_rate,
        scale: args.scale,
        video_only: args.video_only,
        encoder: args.encoder.into(),
    };
    let report = export_movie(&movie, &rom, &options).map_err(|error| error.to_string())?;
    println!(
        "wrote {} ({} frames, {:.1} export FPS, {})",
        args.output.display(),
        report.frames,
        report.export_fps,
        report.encoder
    );
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fm2_to_mp4: {error}");
            ExitCode::FAILURE
        }
    }
}
