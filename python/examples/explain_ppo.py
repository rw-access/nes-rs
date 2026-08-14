"""Render a replay with PPO action probabilities and value estimates."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from nes_gym import SuperMarioBros1_1Env
from nes_gym.replay import export_trace_video
from nes_gym.trace import EpisodeTrace, REWIND_INPUT


ACTION_LABELS = ("NOOP", "LEFT", "RIGHT", "A", "LEFT+A", "RIGHT+A", "REWIND")


def ass_time(seconds: float) -> str:
    hours = int(seconds // 3600)
    minutes = int(seconds // 60) % 60
    centiseconds = int(round((seconds % 60) * 100))
    if centiseconds >= 6000:
        minutes += 1
        centiseconds = 0
    return f"{hours}:{minutes:02d}:{centiseconds // 100:02d}.{centiseconds % 100:02d}"


def write_ass(
    path: Path,
    rows: list[tuple[int, str]],
    *,
    fps: int,
) -> None:
    header = """[Script Info]
ScriptType: v4.00+
PlayResX: 256
PlayResY: 240
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Overlay,Arial,9,&H00FFFFFF,&H00FFFFFF,&H90000000,&H90000000,0,0,0,0,100,100,0,0,3,1,0,7,3,3,3,0

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
    with path.open("w", encoding="utf-8", newline="\n") as output:
        output.write(header)
        for frame, text in rows:
            start = ass_time((frame - 1) / fps)
            end = ass_time(frame / fps)
            output.write(f"Dialogue: 0,{start},{end},Overlay,,0,0,0,,{text}\n")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("trace", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--library", default=None)
    parser.add_argument("--fps", type=int, default=60)
    parser.add_argument("--scale", type=int, default=2)
    parser.add_argument("--max-episode-frames", type=int, default=3600)
    args = parser.parse_args()
    if args.fps <= 0 or args.scale <= 0:
        parser.error("fps and scale must be positive")

    try:
        import torch
        from stable_baselines3 import PPO
        from stable_baselines3.common.utils import obs_as_tensor
    except (ImportError, OSError) as exc:
        raise SystemExit(f"PyTorch/Stable-Baselines3 could not be imported: {exc}") from exc

    # Import the same observation wrapper used by train_ppo.py without making
    # this visualization tool depend on the training script's CLI.
    from train_ppo import NormalizeRamObservation

    trace = EpisodeTrace.from_json(args.trace.read_text(encoding="utf-8"))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    base_video = args.output.with_suffix(".base.mp4")
    ass_path = args.output.with_suffix(".ass")
    args.output.unlink(missing_ok=True)
    base_video.unlink(missing_ok=True)
    ass_path.unlink(missing_ok=True)

    rows: list[tuple[int, str]] = []
    with SuperMarioBros1_1Env(
        args.rom,
        library=args.library,
        max_episode_frames=args.max_episode_frames,
        death_penalty=100.0,
        completion_bonus=1000.0,
        score_coef=0.1,
    ) as base_env:
        env = NormalizeRamObservation(base_env)
        model = PPO.load(str(args.model), env=env, device="cpu")
        model.policy.set_training_mode(False)
        base_env.reset(seed=0)
        action_mapping = tuple(base_env.action_mapping)

        with torch.no_grad():
            frame = 0
            previous_metrics = base_env.metrics()
            for controller, count in trace.inputs_rle:
                for _ in range(count):
                    observation = (np.asarray(base_env.ram, dtype=np.float32) / 255.0)[None, :]
                    tensor = obs_as_tensor(observation, model.device)
                    distribution = model.policy.get_distribution(tensor).distribution
                    probabilities = distribution.probs[0].cpu().numpy()
                    value = float(model.policy.predict_values(tensor).item())

                    if controller == REWIND_INPUT:
                        actual_label = "REWIND"
                        base_env.core.rewind()
                    else:
                        actual_index = action_mapping.index(controller)
                        actual_label = ACTION_LABELS[actual_index]
                        base_env.core.advance_frames(controller, 1)

                    current_metrics = base_env.metrics()
                    top_index = int(np.argmax(probabilities))
                    top_label = ACTION_LABELS[top_index]
                    top_probability = float(probabilities[top_index]) * 100.0
                    top_indices = np.argsort(probabilities)[::-1][:3]
                    probability_text = " ".join(
                        f"{ACTION_LABELS[index]} {float(probabilities[index]) * 100.0:.0f}%"
                        for index in top_indices
                    )
                    delta_x = int(current_metrics["world_x"]) - int(previous_metrics["world_x"])
                    score = int(current_metrics["score"])
                    world_x = int(current_metrics["world_x"])
                    text = (
                        f"INPUT {actual_label} | TOP {top_label} {top_probability:.0f}% | V {value:.1f}\\N"
                        f"{probability_text}\\N"
                        f"WORLD X {world_x}  SCORE {score}  DX {delta_x:+d}"
                    )
                    frame += 1
                    rows.append((frame, text))
                    previous_metrics = current_metrics

        export_trace_video(base_env.core, trace, base_env._root_snapshot, base_video, fps=args.fps)

    write_ass(ass_path, rows, fps=args.fps)
    command = [
        "ffmpeg", "-y", "-loglevel", "error",
        "-i", base_video.name,
        "-vf", f"subtitles={ass_path.name},scale={256 * args.scale}:{240 * args.scale}:flags=neighbor",
        "-map", "0:v:0", "-map", "0:a?",
        "-c:v", "libx264", "-crf", "18", "-preset", "medium",
        "-c:a", "copy", args.output.name,
    ]
    result = subprocess.run(command, cwd=args.output.parent, check=False)
    if result.returncode != 0:
        raise SystemExit(f"ffmpeg annotation failed with exit code {result.returncode}")
    base_video.unlink(missing_ok=True)
    ass_path.unlink(missing_ok=True)
    print(args.output)


if __name__ == "__main__":
    main()
