//! Fast, headless FM2 replay export through an external FFmpeg process.

use crate::fm2::{Fm2Movie, FrameRate};
use nes_core::{
    cartridge, console::Console, ines, video::NES_PALETTE_RGB, VideoOutput, FRAME_HEIGHT,
    FRAME_WIDTH,
};
use std::{
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderPreference {
    Auto,
    VideoToolbox,
    Nvenc,
    Qsv,
    Amf,
    Libx264,
}

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub output: PathBuf,
    pub frame_rate: FrameRate,
    pub scale: u32,
    pub video_only: bool,
    pub encoder: EncoderPreference,
}

#[derive(Clone, Debug)]
pub struct ExportReport {
    pub frames: u64,
    pub export_fps: f64,
    pub encoder: String,
}

#[derive(Debug)]
pub struct ExportError {
    message: String,
}

impl ExportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(context: &str, error: impl fmt::Display) -> Self {
        Self::new(format!("{context}: {error}"))
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for ExportError {}

pub fn export_movie(
    movie: &Fm2Movie,
    rom: &[u8],
    options: &ExportOptions,
) -> Result<ExportReport, ExportError> {
    if movie.frames.is_empty() {
        return Err(ExportError::new("FM2 movie contains no frames"));
    }
    if options.scale == 0 || options.scale > 32 {
        return Err(ExportError::new("scale must be between 1 and 32"));
    }
    if let Some(parent) = options.output.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(ExportError::new(format!(
                "output directory does not exist: {}",
                parent.display()
            )));
        }
    }

    let ffmpeg = locate_ffmpeg()?;
    let mut encoder = choose_encoder(&ffmpeg, options.encoder)?;
    let video_temp = TempPath::new("video.mp4")?;
    let final_temp = TempPath::in_directory(
        options
            .output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
        "output.mp4",
    )?;
    let audio_temp = if options.video_only {
        None
    } else {
        Some(TempPath::new("audio.wav")?)
    };

    let started = Instant::now();
    let mut console = new_console(rom)?;
    if let Err(error) = encode_video(
        &ffmpeg,
        &encoder,
        options,
        &mut console,
        movie,
        &video_temp.path,
        audio_temp.as_ref().map(|path| path.path.as_path()),
    ) {
        if options.encoder != EncoderPreference::Auto || encoder == "libx264" {
            return Err(error);
        }
        eprintln!("{encoder} was unavailable at runtime; replaying with libx264 ({error})");
        encoder = "libx264".to_owned();
        console = new_console(rom)?;
        encode_video(
            &ffmpeg,
            &encoder,
            options,
            &mut console,
            movie,
            &video_temp.path,
            audio_temp.as_ref().map(|path| path.path.as_path()),
        )?;
    }

    if options.video_only {
        finalize_output(&video_temp.path, &final_temp.path, &options.output)?;
    } else {
        mux_audio(
            &ffmpeg,
            &video_temp.path,
            audio_temp
                .as_ref()
                .expect("audio temp exists when audio is enabled"),
            &final_temp.path,
        )?;
        finalize_output(&final_temp.path, &final_temp.path, &options.output)?;
    }

    let elapsed = started.elapsed().as_secs_f64();
    Ok(ExportReport {
        frames: movie.frames.len() as u64,
        export_fps: movie.frames.len() as f64 / elapsed.max(f64::MIN_POSITIVE),
        encoder,
    })
}

fn new_console(rom: &[u8]) -> Result<Console, ExportError> {
    let mut rom_reader = io::Cursor::new(rom);
    let (cartridge_data, mapper_number) =
        ines::load(&mut rom_reader).ok_or_else(|| ExportError::new("invalid iNES ROM"))?;
    let mut console = if mapper_number == 0 {
        Console::new_nrom(cartridge_data)
    } else {
        let mapper = cartridge::new(cartridge_data, mapper_number)
            .ok_or_else(|| ExportError::new(format!("unsupported mapper {mapper_number}")))?;
        Console::new(mapper)
    };
    console.set_video_output(VideoOutput::Enabled);
    Ok(console)
}

fn encode_video(
    ffmpeg: &Path,
    encoder: &str,
    options: &ExportOptions,
    console: &mut Console,
    movie: &Fm2Movie,
    video_path: &Path,
    audio_path: Option<&Path>,
) -> Result<(), ExportError> {
    let width = FRAME_WIDTH as u32 * options.scale;
    let height = FRAME_HEIGHT as u32 * options.scale;
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("-s:v")
        .arg(format!("{width}x{height}"))
        .arg("-framerate")
        .arg(options.frame_rate.as_ffmpeg_value())
        .arg("-i")
        .arg("pipe:0")
        .arg("-an")
        .arg("-c:v")
        .arg(encoder)
        .arg("-pix_fmt")
        .arg("yuv420p");
    if encoder == "libx264" {
        command
            .arg("-preset")
            .arg("ultrafast")
            .arg("-crf")
            .arg("18")
            // At NES resolution x264's automatic choice is slightly slower
            // than a small fixed worker pool. Avoid oversubscribing the
            // emulator/pipe path while still using other cores for encoding.
            .arg("-threads")
            .arg(
                std::thread::available_parallelism()
                    .map(|parallelism| parallelism.get().min(4))
                    .unwrap_or(1)
                    .to_string(),
            );
    } else {
        command.arg("-b:v").arg("8M");
    }
    if audio_path.is_none() {
        command.arg("-movflags").arg("+faststart");
    }
    command
        .arg(video_path)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| ExportError::io("failed to start FFmpeg", error))?;
    let video_input = child
        .stdin
        .take()
        .ok_or_else(|| ExportError::new("FFmpeg video input pipe was not available"))?;
    // Keep the FFmpeg pipe buffered; the output worker writes each converted
    // frame as one contiguous block.
    let video_input = BufWriter::with_capacity(1024 * 1024, video_input);
    let audio = audio_path.map(WavWriter::create).transpose()?;
    let (jobs_tx, jobs_rx) = sync_channel(2);
    let (free_tx, free_rx) = sync_channel(2);
    let worker_free_tx = free_tx.clone();
    let frame_bytes = FRAME_WIDTH
        .checked_mul(options.scale as usize)
        .and_then(|width| {
            FRAME_HEIGHT
                .checked_mul(options.scale as usize)
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| ExportError::new("scaled video frame is too large"))?;
    let scale = options.scale;
    let worker_error = Arc::new(Mutex::new(None));
    let worker_error_slot = Arc::clone(&worker_error);
    let worker = thread::spawn(move || {
        let result = encode_video_worker(
            child,
            video_input,
            audio,
            jobs_rx,
            worker_free_tx,
            scale,
            frame_bytes,
        );
        if let Err(error) = &result {
            *worker_error_slot
                .lock()
                .expect("worker error mutex poisoned") = Some(error.to_string());
        }
        result
    });

    for _ in 0..2 {
        free_tx
            .send(FrameBuffer::new())
            .map_err(|_| ExportError::new("video worker stopped before encoding began"))?;
    }

    for frame in &movie.frames {
        let mut buffer = receive_frame_buffer(&free_rx, &worker_error)?;
        console.update_buttons(frame.buttons);
        {
            let output = console.next_frame();
            buffer.pixels.copy_from_slice(output.pixels);
        }
        if audio_path.is_some() {
            console.take_audio_samples_into(&mut buffer.audio);
        } else {
            buffer.audio.clear();
        }
        if jobs_tx.send(buffer).is_err() {
            let result = worker
                .join()
                .map_err(|_| ExportError::new("video worker thread panicked"))?;
            if let Err(error) = result {
                return Err(error);
            }
            if let Some(error) = worker_error
                .lock()
                .map_err(|_| ExportError::new("video worker error mutex poisoned"))?
                .clone()
            {
                return Err(ExportError::new(error));
            }
            return Err(ExportError::new("video worker stopped while encoding"));
        }
    }
    drop(jobs_tx);
    worker
        .join()
        .map_err(|_| ExportError::new("video worker thread panicked"))??;
    Ok(())
}

struct FrameBuffer {
    pixels: Box<[[u8; FRAME_WIDTH]; FRAME_HEIGHT]>,
    audio: Vec<f32>,
}

impl FrameBuffer {
    fn new() -> Self {
        Self {
            pixels: Box::new([[0; FRAME_WIDTH]; FRAME_HEIGHT]),
            audio: Vec::with_capacity(1024),
        }
    }
}

fn receive_frame_buffer(
    free_rx: &Receiver<FrameBuffer>,
    worker_error: &Arc<Mutex<Option<String>>>,
) -> Result<FrameBuffer, ExportError> {
    loop {
        match free_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(buffer) => return Ok(buffer),
            Err(RecvTimeoutError::Timeout) => {
                if let Some(error) = worker_error
                    .lock()
                    .map_err(|_| ExportError::new("video worker error mutex poisoned"))?
                    .clone()
                {
                    return Err(ExportError::new(error));
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ExportError::new("video worker stopped while encoding"));
            }
        }
    }
}

fn encode_video_worker(
    mut child: Child,
    mut video_input: BufWriter<ChildStdin>,
    mut audio: Option<WavWriter>,
    jobs_rx: Receiver<FrameBuffer>,
    free_tx: SyncSender<FrameBuffer>,
    scale: u32,
    frame_bytes: usize,
) -> Result<(), ExportError> {
    let mut video = Vec::with_capacity(frame_bytes);
    for mut buffer in jobs_rx {
        render_rgb24(&mut video, &buffer.pixels, scale);
        if let Err(error) = video_input.write_all(&video) {
            drop(video_input);
            let ffmpeg_output = child
                .wait_with_output()
                .map_err(|wait_error| ExportError::io("failed waiting for FFmpeg", wait_error))?;
            let detail = String::from_utf8_lossy(&ffmpeg_output.stderr)
                .trim()
                .to_owned();
            return Err(ExportError::new(if detail.is_empty() {
                format!("failed to write video frame: {error}")
            } else {
                format!("failed to write video frame: {error}; FFmpeg: {detail}")
            }));
        }
        if let Some(audio) = audio.as_mut() {
            if let Err(error) = audio.write_samples(&buffer.audio) {
                drop(video_input);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        buffer.audio.clear();
        if free_tx.send(buffer).is_err() {
            return Err(ExportError::new("video producer stopped while encoding"));
        }
    }

    video_input
        .flush()
        .map_err(|error| ExportError::io("flush video frames", error))?;
    drop(video_input);
    if let Some(audio) = audio {
        audio.finish()?;
    }

    let output = child
        .wait_with_output()
        .map_err(|error| ExportError::io("failed waiting for FFmpeg", error))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ExportError::new(if detail.is_empty() {
            format!("FFmpeg video encoding failed ({})", output.status)
        } else {
            format!("FFmpeg video encoding failed: {detail}")
        }));
    }
    Ok(())
}

fn mux_audio(
    ffmpeg: &Path,
    video_path: &Path,
    audio_path: &TempPath,
    output_path: &Path,
) -> Result<(), ExportError> {
    let output = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(video_path)
        .arg("-i")
        .arg(&audio_path.path)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:a:0")
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("192k")
        .arg("-af")
        .arg("apad")
        .arg("-shortest")
        .arg("-movflags")
        .arg("+faststart")
        .arg(output_path)
        .output()
        .map_err(|error| ExportError::io("failed to start FFmpeg audio mux", error))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ExportError::new(if detail.is_empty() {
            format!("FFmpeg audio mux failed ({})", output.status)
        } else {
            format!("FFmpeg audio mux failed: {detail}")
        }));
    }
    Ok(())
}

fn write_rgb24<W: Write>(
    writer: &mut W,
    pixels: &[[u8; FRAME_WIDTH]; FRAME_HEIGHT],
    scale: u32,
) -> io::Result<()> {
    let mut frame = Vec::new();
    render_rgb24(&mut frame, pixels, scale);
    writer.write_all(&frame)
}

fn render_rgb24(frame: &mut Vec<u8>, pixels: &[[u8; FRAME_WIDTH]; FRAME_HEIGHT], scale: u32) {
    let scale = scale as usize;
    frame.clear();
    frame.reserve(FRAME_WIDTH * FRAME_HEIGHT * scale * scale * 3);
    if scale == 1 {
        for source_row in pixels {
            for &palette_index in source_row {
                let [_, red, green, blue] =
                    NES_PALETTE_RGB[palette_index as usize & 0x3f].to_be_bytes();
                frame.extend_from_slice(&[red, green, blue]);
            }
        }
        return;
    }

    let mut row = Vec::with_capacity(FRAME_WIDTH * scale * 3);
    for source_row in pixels {
        row.clear();
        for &palette_index in source_row {
            let [_, red, green, blue] =
                NES_PALETTE_RGB[palette_index as usize & 0x3f].to_be_bytes();
            for _ in 0..scale {
                row.extend_from_slice(&[red, green, blue]);
            }
        }
        for _ in 0..scale {
            frame.extend_from_slice(&row);
        }
    }
}

fn locate_ffmpeg() -> Result<PathBuf, ExportError> {
    let path = PathBuf::from("ffmpeg");
    let output = Command::new(&path)
        .arg("-version")
        .output()
        .map_err(|error| ExportError::io("FFmpeg was not found in PATH", error))?;
    if !output.status.success() {
        return Err(ExportError::new(
            "FFmpeg is installed but cannot be executed",
        ));
    }
    Ok(path)
}

fn choose_encoder(ffmpeg: &Path, preference: EncoderPreference) -> Result<String, ExportError> {
    let output = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-encoders")
        .output()
        .map_err(|error| ExportError::io("failed to inspect FFmpeg encoders", error))?;
    if !output.status.success() {
        return Err(ExportError::new("FFmpeg could not list its encoders"));
    }
    let listing = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates: &[&str] = match preference {
        EncoderPreference::Auto => &[
            "h264_videotoolbox",
            "h264_nvenc",
            "h264_qsv",
            "h264_amf",
            "libx264",
        ],
        EncoderPreference::VideoToolbox => &["h264_videotoolbox"],
        EncoderPreference::Nvenc => &["h264_nvenc"],
        EncoderPreference::Qsv => &["h264_qsv"],
        EncoderPreference::Amf => &["h264_amf"],
        EncoderPreference::Libx264 => &["libx264"],
    };
    candidates
        .iter()
        .find(|candidate| encoder_is_listed(&listing, candidate))
        .map(|candidate| (*candidate).to_owned())
        .ok_or_else(|| {
            ExportError::new(format!(
                "none of the requested FFmpeg encoders are available: {}",
                candidates.join(", ")
            ))
        })
}

fn encoder_is_listed(listing: &str, encoder: &str) -> bool {
    listing.lines().any(|line| {
        line.split_whitespace()
            .nth(1)
            .is_some_and(|name| name == encoder)
    })
}

struct WavWriter {
    file: BufWriter<File>,
    data_bytes: u64,
}

impl WavWriter {
    fn create(path: &Path) -> Result<Self, ExportError> {
        let file = File::create(path).map_err(|error| ExportError::io("create WAV", error))?;
        let mut file = BufWriter::with_capacity(64 * 1024, file);
        file.write_all(&[0; 44])
            .map_err(|error| ExportError::io("reserve WAV header", error))?;
        Ok(Self {
            file,
            data_bytes: 0,
        })
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<(), ExportError> {
        #[cfg(target_endian = "little")]
        {
            // WAV PCM data is little-endian IEEE-754. The supported native
            // targets are little-endian, so write the owned sample buffer
            // directly instead of repacking every f32 one at a time.
            let bytes = unsafe {
                std::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), samples.len() * 4)
            };
            self.file
                .write_all(bytes)
                .map_err(|error| ExportError::io("write WAV samples", error))?;
        }
        #[cfg(target_endian = "big")]
        {
            let mut sample_bytes = Vec::with_capacity(samples.len() * 4);
            for &sample in samples {
                sample_bytes.extend_from_slice(&sample.to_le_bytes());
            }
            self.file
                .write_all(&sample_bytes)
                .map_err(|error| ExportError::io("write WAV samples", error))?;
        }
        self.data_bytes = self
            .data_bytes
            .checked_add(samples.len() as u64 * 4)
            .ok_or_else(|| ExportError::new("WAV audio stream is too large"))?;
        Ok(())
    }

    fn finish(mut self) -> Result<(), ExportError> {
        if self.data_bytes > u32::MAX as u64 {
            return Err(ExportError::new("WAV audio stream exceeds the 4 GiB limit"));
        }
        let data_size = self.data_bytes as u32;
        let riff_size = 36u32
            .checked_add(data_size)
            .ok_or_else(|| ExportError::new("WAV RIFF is too large"))?;
        let mut header = Vec::with_capacity(44);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&riff_size.to_le_bytes());
        header.extend_from_slice(b"WAVEfmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&3u16.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&48_000u32.to_le_bytes());
        header.extend_from_slice(&192_000u32.to_le_bytes());
        header.extend_from_slice(&4u16.to_le_bytes());
        header.extend_from_slice(&32u16.to_le_bytes());
        header.extend_from_slice(b"data");
        header.extend_from_slice(&data_size.to_le_bytes());
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(&header))
            .and_then(|_| self.file.flush())
            .map_err(|error| ExportError::io("finish WAV header", error))
    }
}

struct TempPath {
    path: PathBuf,
}

impl TempPath {
    fn new(suffix: &str) -> Result<Self, ExportError> {
        Self::in_directory(&std::env::temp_dir(), suffix)
    }

    fn in_directory(directory: &Path, suffix: &str) -> Result<Self, ExportError> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0..100u32 {
            let path = directory.join(format!(
                "nes-rs-{}-{stamp}-{attempt}.{suffix}",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(ExportError::io("create temporary export file", error)),
            }
        }
        Err(ExportError::new(
            "could not create a unique temporary export file",
        ))
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn finalize_output(
    source: &Path,
    temporary_output: &Path,
    output: &Path,
) -> Result<(), ExportError> {
    if source != temporary_output {
        fs::rename(source, temporary_output)
            .map_err(|error| ExportError::io("stage encoded MP4", error))?;
    }
    match fs::rename(temporary_output, output) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(output)
                .map_err(|remove_error| ExportError::io("replace existing MP4", remove_error))?;
            fs::rename(temporary_output, output)
                .map_err(|rename_error| ExportError::io("finalize MP4", rename_error))
        }
        Err(error) => Err(ExportError::io("finalize MP4", error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nes_core::{FRAME_HEIGHT, FRAME_WIDTH};

    #[test]
    fn writes_native_rgb_frames_with_nearest_neighbor_scaling() {
        let pixels = [[0u8; FRAME_WIDTH]; FRAME_HEIGHT];
        let mut output = Vec::new();
        write_rgb24(&mut output, &pixels, 2).unwrap();
        assert_eq!(output.len(), FRAME_WIDTH * 2 * FRAME_HEIGHT * 2 * 3);
        assert_eq!(
            &output[..12],
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66]
        );
    }

    #[test]
    fn recognizes_ffmpeg_encoder_listing() {
        let listing =
            " V..... h264_videotoolbox VideoToolbox H.264 Encoder\n V..... libx264 libx264 H.264\n";
        assert!(encoder_is_listed(listing, "h264_videotoolbox"));
        assert!(encoder_is_listed(listing, "libx264"));
        assert!(!encoder_is_listed(listing, "h264_nvenc"));
    }
}
