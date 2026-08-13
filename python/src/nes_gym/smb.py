"""Super Mario Bros. World 1-1 RAM-only Gymnasium environment."""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from .env import NesEnv
from .trace import REWIND_INPUT

# Keep this in lockstep with nes_core::Button.
NOOP = 0x00
A = 0x01
B = 0x02
SELECT = 0x04
START = 0x08
UP = 0x10
DOWN = 0x20
LEFT = 0x40
RIGHT = 0x80

ACTIONS = (NOOP, LEFT, RIGHT, A, LEFT | A, RIGHT | A)
INIT_SEQUENCE = ((NOOP, 60), (START, 1), (NOOP, 105))


class SuperMarioBros1_1Env(NesEnv):
    """One-level SMB environment with a deliberately simple progress reward."""

    def __init__(
        self,
        rom: str | bytes | bytearray | memoryview | None = None,
        *,
        death_penalty: float = 0.0,
        completion_bonus: float = 0.0,
        score_coef: float = 0.0,
        score_delta_clip: float = 1000.0,
        rewind_enabled: bool = False,
        rewind_grace_frames: int = 60,
        **kwargs: Any,
    ):
        kwargs.setdefault("init_sequence", INIT_SEQUENCE)
        kwargs.setdefault("actions", ACTIONS + ((REWIND_INPUT,) if rewind_enabled else ()))
        self.death_penalty = float(death_penalty)
        self.completion_bonus = float(completion_bonus)
        self.score_coef = float(score_coef)
        self.score_delta_clip = float(score_delta_clip)
        if self.score_delta_clip <= 0:
            raise ValueError("score_delta_clip must be positive")
        super().__init__(
            rom,
            rewind_enabled=rewind_enabled,
            rewind_grace_frames=rewind_grace_frames,
            **kwargs,
        )

    @staticmethod
    def _bcd(data: bytes | bytearray) -> int:
        result = 0
        for value in data:
            result = result * 100 + ((value >> 4) & 0xF) * 10 + (value & 0xF)
        return result

    def metrics(self) -> dict[str, Any]:
        ram = self.ram
        return {
            "score": self._bcd(bytes(ram[0x07DE:0x07E3])),
            "lives": int(ram[0x075A]),
            "world_x": (int(ram[0x006D]) << 8) | int(ram[0x0086]),
            "level_state": int(ram[0x000E]),
            "y_viewport": int(ram[0x00B5]),
            "flag_state": int(ram[0x001D]),
            "world": int(ram[0x075F]) + 1,
            "stage": int(ram[0x075C]) + 1,
        }

    def reward(self, previous_metrics: Mapping[str, Any], current_metrics: Mapping[str, Any]) -> float:
        reward = float(current_metrics["world_x"] - previous_metrics.get("world_x", current_metrics["world_x"]))
        current_score = float(current_metrics.get("score", previous_metrics.get("score", 0.0)))
        previous_score = float(previous_metrics.get("score", current_score))
        score_delta = max(0.0, current_score - previous_score)
        reward += self.score_coef * min(score_delta, self.score_delta_clip)
        if self.is_terminal():
            if self._is_level_complete():
                reward += self.completion_bonus
            elif self._is_dying_or_dead():
                reward -= self.death_penalty
        return reward

    def _is_dying_or_dead(self) -> bool:
        ram = self.ram
        return int(ram[0x000E]) in (0x06, 0x0B) or int(ram[0x00B5]) > 1

    def _is_level_complete(self) -> bool:
        ram = self.ram
        flagpole_marker = any(int(ram[address]) in (0x2D, 0x31) for address in range(0x16, 0x1B))
        return flagpole_marker and int(ram[0x001D]) == 0x03

    def is_terminal(self) -> bool:
        return self._is_dying_or_dead() or self._is_level_complete()

    def terminal_reason(self) -> str | None:
        if self._is_dying_or_dead():
            return "death"
        if self._is_level_complete():
            return "level_complete"
        return None
