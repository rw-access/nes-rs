"""Small Stable-Baselines3 PPO smoke run for SMB World 1-1."""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

import gymnasium as gym
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))


class NormalizeRamObservation(gym.ObservationWrapper):
    """Keep the uint8 environment contract but feed PPO float RAM in [0, 1]."""

    def __init__(self, env: gym.Env):
        super().__init__(env)
        shape = env.observation_space.shape
        assert shape is not None
        self.observation_space = gym.spaces.Box(
            low=0.0, high=1.0, shape=shape, dtype=np.float32
        )

    def observation(self, observation: np.ndarray) -> np.ndarray:
        return np.asarray(observation, dtype=np.float32) / 255.0


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom", type=Path)
    parser.add_argument("--library", default=None)
    parser.add_argument("--total-timesteps", type=int, default=2048)
    parser.add_argument("--device", default="auto", choices=("auto", "cpu", "cuda"))
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--max-episode-frames", type=int, default=600)
    parser.add_argument("--frame-skip", type=int, default=1)
    parser.add_argument("--death-penalty", type=float, default=0.0)
    parser.add_argument("--completion-bonus", type=float, default=0.0)
    parser.add_argument("--ent-coef", type=float, default=0.0)
    parser.add_argument("--model-output", type=Path, default=None)
    parser.add_argument("--resume-model", type=Path, default=None)
    parser.add_argument("--trace-output", type=Path, default=None)
    parser.add_argument("--video-output", type=Path, default=None)
    parser.add_argument("--eval-episodes", type=int, default=1)
    parser.add_argument("--stochastic-eval", action="store_true")
    parser.add_argument("--verbose", type=int, choices=(0, 1), default=1)
    args = parser.parse_args()
    if args.total_timesteps <= 0 or args.max_episode_frames <= 0 or args.frame_skip <= 0 or args.ent_coef < 0 or args.eval_episodes <= 0:
        parser.error("timesteps, max episode frames, frame skip, and eval episodes must be positive; entropy coefficient cannot be negative")

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
        frame_skip=args.frame_skip,
        max_episode_frames=args.max_episode_frames,
        death_penalty=args.death_penalty,
        completion_bonus=args.completion_bonus,
    ) as base_env:
        env = NormalizeRamObservation(base_env)
        if args.resume_model is not None:
            model = PPO.load(str(args.resume_model), env=env, device=resolved_device)
        else:
            model = PPO(
                "MlpPolicy",
                env,
                n_steps=128,
                batch_size=64,
                learning_rate=2.5e-4,
                ent_coef=args.ent_coef,
                seed=args.seed,
                device=resolved_device,
                verbose=args.verbose,
            )
        started = time.perf_counter()
        model.learn(
            total_timesteps=args.total_timesteps,
            reset_num_timesteps=args.resume_model is None,
        )
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

            evaluations = []
            best_trace = None
            best_key = None
            for episode in range(args.eval_episodes):
                observation, _ = env.reset(seed=args.seed + episode)
                while True:
                    action, _ = model.predict(
                        observation, deterministic=not args.stochastic_eval
                    )
                    action = int(action.item()) if hasattr(action, "item") else int(action)
                    observation, _, terminated, truncated, info = env.step(action)
                    if terminated or truncated:
                        break
                trace = base_env.last_trace
                assert trace is not None
                evaluation = {
                    "episode": episode,
                    "episode_frames": trace.episode_frames,
                    "total_reward": trace.total_reward,
                    "terminal_reason": trace.terminal_reason,
                    "final_metrics": trace.final_metrics,
                }
                evaluations.append(evaluation)
                world_x = int(trace.final_metrics.get("world_x", 0))
                key = (world_x, trace.total_reward, trace.episode_frames)
                if best_key is None or key > best_key:
                    best_key = key
                    best_trace = trace

            assert best_trace is not None
            if args.trace_output is not None:
                args.trace_output.parent.mkdir(parents=True, exist_ok=True)
                args.trace_output.write_text(best_trace.to_json() + "\n", encoding="utf-8")
            if args.video_output is not None:
                export_trace_video(base_env.core, best_trace, base_env._root_snapshot, args.video_output)
            print({
                "evaluation": evaluations,
                "selected_evaluation": best_trace.to_dict(),
                "stochastic_evaluation": args.stochastic_eval,
                "model_output": str(args.model_output) if args.model_output is not None else None,
                "trace_output": str(args.trace_output) if args.trace_output is not None else None,
                "video_output": str(args.video_output) if args.video_output is not None else None,
                "info": info,
            })


if __name__ == "__main__":
    main()
