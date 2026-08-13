"""Run resumable PPO segments with an artifact per evaluation checkpoint."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom", type=Path)
    parser.add_argument("--workdir", type=Path, default=Path("../artifacts/staged-ppo"))
    parser.add_argument("--segments", type=int, default=12)
    parser.add_argument("--segment-timesteps", type=int, default=180_000)
    parser.add_argument("--initial-horizon", type=int, default=1_800)
    parser.add_argument("--frame-skip", type=int, default=1)
    parser.add_argument("--death-penalty", type=float, default=100.0)
    parser.add_argument("--completion-bonus", type=float, default=1_000.0)
    parser.add_argument("--ent-coef", type=float, default=0.01)
    parser.add_argument("--eval-episodes", type=int, default=8)
    parser.add_argument("--stochastic-eval", action="store_true", default=True)
    parser.add_argument("--resume-model", type=Path, default=None)
    parser.add_argument("--device", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    if args.segments <= 0 or args.segment_timesteps <= 0 or args.initial_horizon <= 0 or args.frame_skip <= 0 or args.ent_coef < 0 or args.eval_episodes <= 0:
        parser.error("segments, segment timesteps, initial horizon, and frame skip must be positive; entropy coefficient cannot be negative")

    args.workdir.mkdir(parents=True, exist_ok=True)
    manifest_path = args.workdir / "manifest.json"
    manifest: list[dict[str, object]] = []
    previous_model: Path | None = args.resume_model
    best_world_x = 0
    horizon = args.initial_horizon
    train_script = Path(__file__).with_name("train_ppo.py")

    for segment in range(1, args.segments + 1):
        segment_name = f"segment-{segment:03d}"
        model_path = args.workdir / f"{segment_name}.zip"
        trace_path = args.workdir / f"{segment_name}-eval.json"
        video_path = args.workdir / f"{segment_name}-eval.mp4"
        log_path = args.workdir / f"{segment_name}.log"
        command = [
            sys.executable,
            str(train_script),
            str(args.rom),
            "--total-timesteps", str(args.segment_timesteps),
            "--max-episode-frames", str(horizon),
            "--frame-skip", str(args.frame_skip),
            "--death-penalty", str(args.death_penalty),
            "--completion-bonus", str(args.completion_bonus),
            "--ent-coef", str(args.ent_coef),
            "--eval-episodes", str(args.eval_episodes),
            "--device", args.device,
            "--seed", str(args.seed),
            "--verbose", "0",
            "--model-output", str(model_path),
            "--trace-output", str(trace_path),
            "--video-output", str(video_path),
        ]
        if args.stochastic_eval:
            command.append("--stochastic-eval")
        if previous_model is not None:
            command.extend(("--resume-model", str(previous_model)))
        with log_path.open("w", encoding="utf-8") as log:
            result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=False)
        if result.returncode != 0:
            raise SystemExit(f"segment {segment} failed; see {log_path}")

        trace = json.loads(trace_path.read_text(encoding="utf-8"))
        metrics = trace.get("final_metrics", {})
        world_x = int(metrics.get("world_x", 0)) if isinstance(metrics, dict) else 0
        best_world_x = max(best_world_x, world_x)
        record = {
            "segment": segment,
            "model": str(model_path),
            "trace": str(trace_path),
            "video": str(video_path),
            "horizon": horizon,
            "episode_frames": trace.get("episode_frames"),
            "total_reward": trace.get("total_reward"),
            "terminal_reason": trace.get("terminal_reason"),
            "world_x": world_x,
            "best_world_x": best_world_x,
        }
        manifest.append(record)
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(record, flush=True)

        # Give a policy that has demonstrably advanced farther more runway.
        if best_world_x >= 800:
            horizon = max(horizon, 3_600)
        elif best_world_x >= 400:
            horizon = max(horizon, 2_400)
        previous_model = model_path


if __name__ == "__main__":
    main()
