"""Run a short headless random episode and print its replayable trace."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from nes_gym import SuperMarioBros1_1Env, experiment_metadata
from nes_gym.replay import export_trace_video


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom")
    parser.add_argument("--frames", type=int, default=600)
    parser.add_argument("--library", default=None)
    parser.add_argument("--trace", type=Path, default=None)
    parser.add_argument("--video", type=Path, default=None)
    parser.add_argument("--verify-replay", action="store_true")
    parser.add_argument("--metadata", type=Path, default=None)
    args = parser.parse_args()
    with SuperMarioBros1_1Env(args.rom, library=args.library, max_episode_frames=args.frames) as env:
        observation, info = env.reset(seed=0)
        del observation
        while True:
            action = env.action_space.sample()
            _, _, terminated, truncated, info = env.step(action)
            if terminated or truncated:
                break
        trace = env.last_trace
        assert trace is not None
        if args.verify_replay:
            first = bytes(env.ram)
            replayed = bytes(env.replay_trace(trace))
            if first != replayed:
                raise RuntimeError("trace replay did not reproduce the final RAM view")
        if args.trace is not None:
            args.trace.parent.mkdir(parents=True, exist_ok=True)
            args.trace.write_text(trace.to_json() + "\n", encoding="utf-8")
        if args.metadata is not None:
            args.metadata.parent.mkdir(parents=True, exist_ok=True)
            args.metadata.write_text(
                json.dumps(experiment_metadata(env), sort_keys=True, indent=2) + "\n",
                encoding="utf-8",
            )
        if args.video is not None:
            export_trace_video(env.core, trace, env._root_snapshot, args.video)
        print({"trace": trace.to_dict(), "info": info})


if __name__ == "__main__":
    main()
