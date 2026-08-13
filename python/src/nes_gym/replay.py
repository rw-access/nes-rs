"""Replay helpers shared by inspection tools and future video exporters."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
import subprocess
from typing import Any

from .core import NesCore, NesSnapshot
from .trace import EpisodeTrace


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
        str(output_path),
    ]
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    assert process.stdin is not None
    try:
        for controller, count in trace.inputs_rle:
            for _ in range(count):
                core.advance_frames(controller, 1)
                frame = core.framebuffer
                if scale == 1:
                    process.stdin.write(frame.tobytes())
                else:
                    rgba = frame.reshape(240, 256, 4)
                    scaled = rgba.repeat(scale, axis=0).repeat(scale, axis=1)
                    process.stdin.write(scaled.tobytes())
        process.stdin.close()
        return_code = process.wait()
    except Exception:
        process.kill()
        process.wait()
        raise
    if return_code != 0:
        raise RuntimeError(f"ffmpeg failed with exit code {return_code}")
    return output_path
