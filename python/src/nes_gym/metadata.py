"""Reproducibility metadata kept outside the generic emulator state."""

from __future__ import annotations

import os
from typing import Any


def experiment_metadata(env: Any) -> dict[str, object]:
    """Collect stable configuration needed to identify an experiment."""
    core = env.core
    library = getattr(core, "library_path", None)
    return {
        "rom_sha256": getattr(core, "rom_sha256", None),
        "emulator_library": str(library) if library is not None else None,
        "emulator_git_sha": os.environ.get("NES_RS_GIT_SHA"),
        "environment": type(env).__name__,
        "environment_version": "0.1.0",
        "initialization_sequence": [list(run) for run in env.init_sequence],
        "root_checkpoint": env.root_checkpoint.identifier,
        "action_mapping": list(env.action_mapping),
        "frame_skip": env.frame_skip,
        "rewind_enabled": getattr(env, "rewind_enabled", False),
        "rewind_grace_frames": getattr(env, "rewind_grace_frames", None),
        "observation": {
            "kind": "cpu_ram",
            "shape": [2048],
            "dtype": "uint8",
        },
        "reward_configuration": f"{type(env).__module__}.{type(env).__name__}.reward",
    }
