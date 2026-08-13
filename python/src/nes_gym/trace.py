"""Exact, compact controller traces and checkpoint provenance."""

from __future__ import annotations

from dataclasses import dataclass, field
import json
from typing import Iterable, Iterator, Sequence

# A value above the NES uint8 controller range represents a framework input.
# REWIND_INPUT is mutually exclusive with all controller buttons.
REWIND_INPUT = 0x100
InputRLE = tuple[tuple[int, int], ...]


def normalize_rle(runs: Iterable[tuple[int, int]]) -> InputRLE:
    normalized: list[tuple[int, int]] = []
    for controller, count in runs:
        controller, count = int(controller), int(count)
        if not 0 <= controller <= REWIND_INPUT:
            raise ValueError("input must be a NES uint8 controller state or REWIND_INPUT")
        if count <= 0:
            raise ValueError("RLE counts must be positive")
        if normalized and normalized[-1][0] == controller:
            normalized[-1] = (controller, normalized[-1][1] + count)
        else:
            normalized.append((controller, count))
    return tuple(normalized)


def append_rle(runs: list[tuple[int, int]], controller: int, count: int = 1) -> None:
    merged = normalize_rle(((controller, count),))
    if runs and runs[-1][0] == merged[0][0]:
        runs[-1] = (runs[-1][0], runs[-1][1] + merged[0][1])
    else:
        runs.append(merged[0])


def expand_rle(runs: Iterable[tuple[int, int]]) -> Iterator[int]:
    for controller, count in normalize_rle(runs):
        yield from (controller for _ in range(count))


@dataclass(frozen=True)
class CheckpointRef:
    """Logical identity of an in-memory checkpoint and its provenance."""

    identifier: str
    parent: str | None = None
    input_prefix: InputRLE = ()

    @classmethod
    def root(cls, identifier: str) -> "CheckpointRef":
        return cls(identifier=identifier)

    @classmethod
    def derived(cls, parent: "CheckpointRef", prefix: Iterable[tuple[int, int]]) -> "CheckpointRef":
        return cls(identifier=f"{parent.identifier}+prefix", parent=parent.identifier,
                   input_prefix=normalize_rle(prefix))


@dataclass(frozen=True)
class EpisodeTrace:
    checkpoint: CheckpointRef
    inputs_rle: InputRLE = ()
    total_reward: float = 0.0
    terminal_reason: str | None = None
    truncated: bool = False
    final_metrics: dict[str, object] = field(default_factory=dict)

    def __post_init__(self) -> None:
        object.__setattr__(self, "inputs_rle", normalize_rle(self.inputs_rle))

    @property
    def episode_frames(self) -> int:
        return sum(count for _, count in self.inputs_rle)

    def inputs(self) -> Iterator[int]:
        return expand_rle(self.inputs_rle)

    def to_dict(self) -> dict[str, object]:
        return {
            "checkpoint": {
                "identifier": self.checkpoint.identifier,
                "parent": self.checkpoint.parent,
                "input_prefix": [list(run) for run in self.checkpoint.input_prefix],
            },
            "inputs_rle": [list(run) for run in self.inputs_rle],
            "total_reward": self.total_reward,
            "terminal_reason": self.terminal_reason,
            "truncated": self.truncated,
            "episode_frames": self.episode_frames,
            "final_metrics": self.final_metrics,
        }

    @classmethod
    def from_dict(cls, data: dict[str, object]) -> "EpisodeTrace":
        checkpoint_data = data["checkpoint"]
        if not isinstance(checkpoint_data, dict):
            raise ValueError("trace checkpoint must be an object")
        checkpoint = CheckpointRef(
            identifier=str(checkpoint_data["identifier"]),
            parent=(str(checkpoint_data["parent"]) if checkpoint_data.get("parent") is not None else None),
            input_prefix=normalize_rle(
                tuple(tuple(int(value) for value in run) for run in checkpoint_data.get("input_prefix", ()))
            ),
        )
        return cls(
            checkpoint=checkpoint,
            inputs_rle=normalize_rle(
                tuple(tuple(int(value) for value in run) for run in data.get("inputs_rle", ()))
            ),
            total_reward=float(data.get("total_reward", 0.0)),
            terminal_reason=(str(data["terminal_reason"]) if data.get("terminal_reason") is not None else None),
            truncated=bool(data.get("truncated", False)),
            final_metrics=dict(data.get("final_metrics", {})),
        )

    def to_json(self) -> str:
        return json.dumps(self.to_dict(), sort_keys=True, indent=2)

    @classmethod
    def from_json(cls, text: str) -> "EpisodeTrace":
        data = json.loads(text)
        if not isinstance(data, dict):
            raise ValueError("episode trace JSON must contain an object")
        return cls.from_dict(data)


class EpisodeTraceBuilder:
    def __init__(self, checkpoint: CheckpointRef):
        self.checkpoint = checkpoint
        self._runs: list[tuple[int, int]] = []
        self.total_reward = 0.0

    def append(self, controller: int, frames: int = 1) -> None:
        append_rle(self._runs, controller, frames)

    def add_reward(self, reward: float) -> None:
        self.total_reward += float(reward)

    def finish(
        self,
        *,
        terminal_reason: str | None = None,
        truncated: bool = False,
        final_metrics: dict[str, object] | None = None,
    ) -> EpisodeTrace:
        return EpisodeTrace(
            checkpoint=self.checkpoint,
            inputs_rle=tuple(self._runs),
            total_reward=self.total_reward,
            terminal_reason=terminal_reason,
            truncated=truncated,
            final_metrics=final_metrics or {},
        )
