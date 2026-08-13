"""Generic Gymnasium environment lifecycle for one nes-rs emulator."""

from __future__ import annotations

import hashlib
from collections.abc import Mapping, Sequence
from typing import Any

import gymnasium as gym
import numpy as np

from .core import NesCore, NesSnapshot
from .trace import CheckpointRef, EpisodeTrace, EpisodeTraceBuilder


class NesEnv(gym.Env[np.ndarray, int]):
    """RAM-only, one-emulator Gymnasium environment.

    Subclasses provide game semantics through ``metrics``, ``reward``, and
    terminal hooks.  The root snapshot is made once after ``init_sequence``;
    reset only restores that snapshot.
    """

    metadata = {"render_modes": []}

    def __init__(
        self,
        rom: str | bytes | bytearray | memoryview | None = None,
        *,
        init_sequence: Sequence[tuple[int, int]] = (),
        actions: Sequence[int] = (0,),
        frame_skip: int = 1,
        max_episode_frames: int = 18_000,
        core: NesCore | Any | None = None,
        library: str | None = None,
    ):
        super().__init__()
        if frame_skip <= 0:
            raise ValueError("frame_skip must be positive")
        if max_episode_frames <= 0:
            raise ValueError("max_episode_frames must be positive")
        if core is None and rom is None:
            raise ValueError("rom is required when core is not supplied")
        self.core = core if core is not None else NesCore(rom, library=library)
        self.init_sequence = tuple((int(controller), int(frames)) for controller, frames in init_sequence)
        self.frame_skip = int(frame_skip)
        self.max_episode_frames = int(max_episode_frames)
        self.action_mapping = tuple(int(value) for value in actions)
        if not self.action_mapping:
            raise ValueError("actions cannot be empty")
        if any(not 0 <= value <= 0xFF for value in self.action_mapping):
            raise ValueError("action controller states must fit in uint8_t")
        self.action_space = gym.spaces.Discrete(len(self.action_mapping))
        self.observation_space = gym.spaces.Box(
            low=0, high=255, shape=(2048,), dtype=np.uint8
        )
        for controller, frames in self.init_sequence:
            self.core.advance_frames(controller, frames)
        self._root_snapshot: NesSnapshot = self.core.snapshot()
        rom_hash = getattr(self.core, "rom_sha256", "unknown")
        init_bytes = repr(self.init_sequence).encode()
        checkpoint_id = hashlib.sha256(rom_hash.encode() + init_bytes + bytes(self.core.ram)).hexdigest()
        self.root_checkpoint = CheckpointRef.root(checkpoint_id)
        self._trace: EpisodeTraceBuilder | None = None
        self._elapsed_frames = 0
        self._episode_done = False
        self._episode_truncated = False
        self._previous_metrics: dict[str, Any] = {}
        self._last_trace: EpisodeTrace | None = None

    @property
    def ram(self) -> np.ndarray:
        return self.core.ram

    @property
    def elapsed_frames(self) -> int:
        return self._elapsed_frames

    @property
    def last_trace(self) -> EpisodeTrace | None:
        return self._last_trace

    def metrics(self) -> dict[str, Any]:
        return {}

    def reward(self, previous_metrics: Mapping[str, Any], current_metrics: Mapping[str, Any]) -> float:
        return 0.0

    def is_terminal(self) -> bool:
        return False

    def terminal_reason(self) -> str | None:
        return None

    def _info(self, *, controller: int | None = None, reason: str | None = None) -> dict[str, Any]:
        metrics = dict(self._previous_metrics)
        info: dict[str, Any] = {
            "episode_frames": self._elapsed_frames,
            "frame_skip": self.frame_skip,
            "checkpoint": self.root_checkpoint,
            "metrics": metrics,
        }
        info.update(metrics)
        if controller is not None:
            info["controller_state"] = controller
        if reason is not None:
            info["terminal_reason"] = reason
        return info

    def reset(self, *, seed: int | None = None, options: dict[str, Any] | None = None):
        super().reset(seed=seed)
        if options and options.get("checkpoint") is not None:
            raise NotImplementedError("custom checkpoint reset is reserved for derived checkpoints")
        self.core.restore(self._root_snapshot)
        self._elapsed_frames = 0
        self._episode_done = False
        self._episode_truncated = False
        self._trace = EpisodeTraceBuilder(self.root_checkpoint)
        self._previous_metrics = dict(self.metrics())
        self._last_trace = None
        return self.ram, self._info()

    def step(self, action: int):
        if self._episode_done:
            raise RuntimeError("step() called after episode ended; call reset() first")
        action = int(action)
        if not self.action_space.contains(action):
            raise ValueError(f"invalid action {action}")
        controller = self.action_mapping[action]
        previous = self._previous_metrics
        frames_to_advance = min(self.frame_skip, self.max_episode_frames - self._elapsed_frames)
        if frames_to_advance <= 0:
            raise RuntimeError("episode frame budget is exhausted; call reset() first")
        actual_frames = 0
        current = dict(previous)
        for _ in range(frames_to_advance):
            self.core.advance_frames(controller, 1)
            self._elapsed_frames += 1
            actual_frames += 1
            current = dict(self.metrics())
            if self.is_terminal():
                break
        reward = float(self.reward(previous, current))
        assert self._trace is not None
        self._trace.append(controller, actual_frames)
        self._trace.add_reward(reward)
        self._previous_metrics = current

        terminated = bool(self.is_terminal())
        reason = self.terminal_reason() if terminated else None
        truncated = not terminated and self._elapsed_frames >= self.max_episode_frames
        if truncated:
            reason = "max_frames"
        if terminated or truncated:
            self._episode_done = True
            self._episode_truncated = truncated
            self._last_trace = self._trace.finish(
                terminal_reason=reason,
                truncated=truncated,
                final_metrics=current,
            )
        return self.ram, reward, terminated, truncated, self._info(controller=controller, reason=reason)

    def current_trace(self) -> EpisodeTrace:
        if self._trace is None:
            raise RuntimeError("reset() must be called before requesting a trace")
        return self._trace.finish(
            terminal_reason=self.terminal_reason() if self._episode_done else None,
            truncated=self._episode_truncated,
            final_metrics=self._previous_metrics,
        )

    def replay_trace(self, trace: EpisodeTrace) -> np.ndarray:
        """Restore the root checkpoint and replay every recorded frame."""
        if trace.checkpoint.identifier != self.root_checkpoint.identifier:
            raise ValueError("trace checkpoint does not belong to this environment root")
        self.core.restore(self._root_snapshot)
        for controller, frames in trace.inputs_rle:
            self.core.advance_frames(controller, frames)
        return self.ram

    def close(self) -> None:
        root = getattr(self, "_root_snapshot", None)
        if root is not None:
            root.close()
            self._root_snapshot = None  # type: ignore[assignment]
        core = getattr(self, "core", None)
        if core is not None:
            core.close()

    def __enter__(self) -> "NesEnv":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()
