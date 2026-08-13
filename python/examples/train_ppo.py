"""Small Stable-Baselines3 PPO smoke run for SMB World 1-1."""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom", type=Path)
    parser.add_argument("--library", default=None)
    parser.add_argument("--total-timesteps", type=int, default=2048)
    parser.add_argument("--device", default="auto", choices=("auto", "cpu", "cuda"))
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--max-episode-frames", type=int, default=600)
    parser.add_argument("--model-output", type=Path, default=None)
    parser.add_argument("--trace-output", type=Path, default=None)
    parser.add_argument("--video-output", type=Path, default=None)
    args = parser.parse_args()
    if args.total_timesteps <= 0 or args.max_episode_frames <= 0:
        parser.error("timesteps and max episode frames must be positive")

    try:
        import torch
        from stable_baselines3 import PPO
    except (ImportError, OSError) as exc:
        raise SystemExit(
            "PyTorch/Stable-Baselines3 could not be imported. On Windows, "
            "check Application Control policy for torch_global_deps.dll, "
            f"then rerun this command. Original error: {exc}"
        ) from exc

    if args.device == "cuda" and not torch.cuda.is_available():
        raise SystemExit("--device cuda was requested, but torch.cuda.is_available() is false")
    resolved_device = "cuda" if args.device == "cuda" else args.device
    print({
        "torch": torch.__version__,
        "cuda_available": bool(torch.cuda.is_available()),
        "device": resolved_device,
        "cuda_device": torch.cuda.get_device_name(0) if torch.cuda.is_available() else None,
    })

    from nes_gym import SuperMarioBros1_1Env
    from nes_gym.replay import export_trace_video

    with SuperMarioBros1_1Env(
        args.rom,
        library=args.library,
        max_episode_frames=args.max_episode_frames,
    ) as env:
        model = PPO(
            "MlpPolicy",
            env,
            n_steps=128,
            batch_size=64,
            learning_rate=2.5e-4,
            seed=args.seed,
            device=resolved_device,
            verbose=1,
        )
        started = time.perf_counter()
        model.learn(total_timesteps=args.total_timesteps)
        elapsed = time.perf_counter() - started
        print({
            "total_timesteps": args.total_timesteps,
            "seconds": elapsed,
            "gym_steps_per_second": args.total_timesteps / max(elapsed, 1e-9),
        })

        if any(path is not None for path in (args.model_output, args.trace_output, args.video_output)):
            if args.model_output is not None:
                args.model_output.parent.mkdir(parents=True, exist_ok=True)
                model.save(str(args.model_output))

            observation, _ = env.reset(seed=args.seed)
            while True:
                action, _ = model.predict(observation, deterministic=True)
                action = int(action.item()) if hasattr(action, "item") else int(action)
                observation, _, terminated, truncated, info = env.step(action)
                if terminated or truncated:
                    break

            trace = env.last_trace
            assert trace is not None
            if args.trace_output is not None:
                args.trace_output.parent.mkdir(parents=True, exist_ok=True)
                args.trace_output.write_text(trace.to_json() + "\n", encoding="utf-8")
            if args.video_output is not None:
                export_trace_video(env.core, trace, env._root_snapshot, args.video_output)
            print({
                "evaluation": {
                    "episode_frames": trace.episode_frames,
                    "total_reward": trace.total_reward,
                    "terminal_reason": trace.terminal_reason,
                    "final_metrics": trace.final_metrics,
                },
                "model_output": str(args.model_output) if args.model_output is not None else None,
                "trace_output": str(args.trace_output) if args.trace_output is not None else None,
                "video_output": str(args.video_output) if args.video_output is not None else None,
                "info": info,
            })


if __name__ == "__main__":
    main()
