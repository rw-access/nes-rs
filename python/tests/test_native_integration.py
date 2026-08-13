import os
from pathlib import Path

import pytest

from nes_gym import SuperMarioBros1_1Env
from nes_gym.ffi import find_library


def _native_inputs():
    try:
        library = find_library()
    except FileNotFoundError:
        pytest.skip("native nes-rs FFI library is not built")
    rom = os.environ.get("NES_SMB_ROM")
    if rom is None:
        rom = str(Path(__file__).parents[2] / "roms" / "Super Mario Bros. (World).nes")
    if not Path(rom).is_file():
        pytest.skip("SMB ROM is not available; set NES_SMB_ROM")
    return rom, library


def test_native_reset_and_replay_are_deterministic():
    rom, library = _native_inputs()
    with SuperMarioBros1_1Env(rom, library=str(library), max_episode_frames=30) as env:
        observation, _ = env.reset(seed=0)
        view_id = id(observation)
        for action in (2, 2, 0, 5):
            _, _, terminated, truncated, _ = env.step(action)
            if terminated or truncated:
                break
        trace = env.current_trace()
        first = bytes(env.replay_trace(trace))
        assert id(env.ram) == view_id
        second = bytes(env.replay_trace(trace))
        assert first == second

        env.core.set_video_output(True)
        env.core.restore(env._root_snapshot)
        for controller, frames in trace.inputs_rle:
            env.core.advance_frames(controller, frames)
        first_framebuffer = bytes(env.core.framebuffer)
        env.core.restore(env._root_snapshot)
        for controller, frames in trace.inputs_rle:
            env.core.advance_frames(controller, frames)
        assert first_framebuffer == bytes(env.core.framebuffer)


def test_native_init_reaches_world_1_1_and_view_mutation_is_isolated():
    rom, library = _native_inputs()
    with SuperMarioBros1_1Env(rom, library=str(library)) as env:
        observation, _ = env.reset(seed=0)
        assert observation.shape == (2048,)
        # The calibrated root is normal gameplay, not title/pre-level state.
        # These values are stable for the supplied World ROM and distinguish
        # the playable 1-1 root reached by the init sequence.
        assert int(env.ram[0x000E]) == 0x08
        assert int(env.ram[0x0770]) == 0x01
        assert int(env.ram[0x075D]) == 0x01
        assert int(env.ram[0x0761]) == 0x02
        assert env.metrics()["world"] == 1
        assert env.metrics()["stage"] == 1

        snapshot = env.core.snapshot()
        try:
            env.core.advance_frames(0, 1)
            expected_after_frame = bytes(env.ram)
            env.core.restore(snapshot)
            env.ram[:] = 0xA5
            env.core.advance_frames(0, 1)
            assert bytes(env.ram) == expected_after_frame
        finally:
            snapshot.close()
