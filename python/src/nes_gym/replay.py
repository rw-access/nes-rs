"""Replay helpers shared by inspection tools and future video exporters."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
import subprocess
import tempfile
from typing import Any
import wave

from .core import NesCore, NesSnapshot
from .trace import REWIND_INPUT, EpisodeTrace


def replay_trace(
    core: NesCore,
    trace: EpisodeTrace,
    checkpoint: NesSnapshot,
    *,
    on_frame: Callable[[int, Any], None] | None = None,
) -> None:
    """Replay a trace from its materialized checkpoint, optionally per frame."""
    core.restore(checkpoint)
    frame = 0
    for controller, count in trace.inputs_rle:
        for _ in range(count):
            if controller == REWIND_INPUT:
                core.rewind()
            else:
                core.advance_frames(controller, 1)
            frame += 1
            if on_frame is not None:
                on_frame(frame, core.ram)


def export_trace_video(
    core: NesCore,
    trace: EpisodeTrace,
    checkpoint: NesSnapshot,
    output: str | Path,
    *,
    ffmpeg: str = "ffmpeg",
    fps: int = 60,
    scale: int = 1,
) -> Path:
    """Render exactly the trace frames from a materialized checkpoint.

    The emulator remains in-process. FFmpeg receives raw RGBA frames over its
    stdin; initialization frames are deliberately not included.
    """
    if fps <= 0 or scale <= 0:
        raise ValueError("fps and scale must be positive")
    output_path = Path(output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    core.set_video_output(True)
    core.restore(checkpoint)
    width, height = 256 * scale, 240 * scale
    temp_video_file = tempfile.NamedTemporaryFile(
        prefix="nes-replay-", suffix=".mp4", dir=output_path.parent, delete=False
    )
    temp_audio_file = tempfile.NamedTemporaryFile(
        prefix="nes-replay-", suffix=".wav", dir=output_path.parent, delete=False
    )
    temp_video = Path(temp_video_file.name)
    temp_audio = Path(temp_audio_file.name)
    temp_video_file.close()
    temp_audio_file.close()
    command = [
        ffmpeg,
        "-y",
        "-loglevel", "error",
        "-f", "rawvideo",
        "-vcodec", "rawvideo",
        "-pix_fmt", "rgba",
        "-s", f"{width}x{height}",
        "-r", str(fps),
        "-i", "-",
        "-an",
        "-c:v", "libx264",
        "-pix_fmt", "yuv420p",
        str(temp_video),
    ]
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    assert process.stdin is not None
    audio_samples: list[float] = []
    video_frames = 0
    try:
        for controller, count in trace.inputs_rle:
            for _ in range(count):
                if controller == REWIND_INPUT:
                    core.rewind()
                else:
                    core.advance_frames(controller, 1)
                frame = core.framebuffer
                if controller == REWIND_INPUT:
                    audio_samples.extend([0.0] * round(core.audio_sample_rate / fps))
                else:
                    audio_samples.extend(float(sample) for sample in core.audio)
                if scale == 1:
                    process.stdin.write(frame.tobytes())
                else:
                    rgba = frame.reshape(240, 256, 4)
                    scaled = rgba.repeat(scale, axis=0).repeat(scale, axis=1)
                    process.stdin.write(scaled.tobytes())
                video_frames += 1
        process.stdin.close()
        return_code = process.wait()
        if return_code != 0:
            raise RuntimeError(f"ffmpeg failed with exit code {return_code}")

        # The emulator's audio callback can return 799/800/801 samples at a
        # 60 Hz frame boundary.  Letting that small rounding error accumulate
        # makes the WAV shorter or longer than the raw video; `-shortest` then
        # silently drops a few video frames from the final replay.  Normalize
        # the complete track once so one replay frame always has the intended
        # duration, including rewind frames (which contribute silence).
        target_audio_samples = round(video_frames * core.audio_sample_rate / fps)
        if len(audio_samples) < target_audio_samples:
            audio_samples.extend([0.0] * (target_audio_samples - len(audio_samples)))
        else:
            del audio_samples[target_audio_samples:]

        pcm = bytearray()
        for sample in audio_samples:
            pcm.extend(int(max(-1.0, min(1.0, sample)) * 32767).to_bytes(2, "little", signed=True))
        with wave.open(str(temp_audio), "wb") as audio_file:
            audio_file.setnchannels(1)
            audio_file.setsampwidth(2)
            audio_file.setframerate(core.audio_sample_rate)
            audio_file.writeframes(pcm)

        mux_command = [
            ffmpeg,
            "-y",
            "-loglevel", "error",
            "-i", str(temp_video),
            "-i", str(temp_audio),
            "-map", "0:v:0",
            "-map", "1:a:0",
            "-c:v", "copy",
            "-c:a", "aac",
            "-shortest",
            str(output_path),
        ]
        mux_result = subprocess.run(
            mux_command,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if mux_result.returncode != 0:
            raise RuntimeError(f"ffmpeg audio mux failed with exit code {mux_result.returncode}")
        return output_path
    except Exception:
        process.kill()
        process.wait()
        raise
    finally:
        temp_video.unlink(missing_ok=True)
        temp_audio.unlink(missing_ok=True)
