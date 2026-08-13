import numpy as np

from nes_gym.env import NesEnv


class FakeSnapshot:
    def __init__(self, state):
        self.state = state
        self._closed = False

    def close(self):
        self._closed = True


class FakeCore:
    rom_sha256 = "fake-rom"

    def __init__(self):
        self.ram = np.zeros(2048, dtype=np.uint8)
        self.state = 0
        self.closed = False

    def advance_frames(self, controller, frames):
        self.state += controller * frames
        self.ram[:] = self.state & 0xFF

    def snapshot(self):
        return FakeSnapshot(self.state)

    def restore(self, snapshot):
        self.state = snapshot.state
        self.ram[:] = self.state & 0xFF

    def close(self):
        self.closed = True


def test_reset_step_truncation_and_exact_trace():
    core = FakeCore()
    env = NesEnv(core=core, init_sequence=((4, 2),), actions=(0, 3), max_episode_frames=3)
    initial, _ = env.reset()
    assert initial.shape == (2048,)
    assert initial.dtype == np.uint8
    _, reward, terminated, truncated, info = env.step(1)
    assert reward == 0
    assert not terminated and not truncated
    _, _, _, truncated, _ = env.step(0)
    if not truncated:
        _, _, _, truncated, _ = env.step(0)
    assert truncated
    assert env.last_trace.inputs_rle == ((3, 1), (0, 2))
    env.close()
    assert core.closed


def test_replay_uses_root_and_preserves_view_identity():
    core = FakeCore()
    env = NesEnv(core=core, actions=(2,), max_episode_frames=10)
    observation, _ = env.reset()
    view_id = id(observation)
    env.step(0)
    trace = env.current_trace()
    replayed = env.replay_trace(trace)
    assert id(replayed) == view_id
    assert int(replayed[0]) == 2
    env.close()


def test_semantic_terminal_at_frame_limit_is_not_truncation():
    class TerminalEnv(NesEnv):
        def is_terminal(self):
            return self.elapsed_frames >= 1

        def terminal_reason(self):
            return "test_terminal" if self.is_terminal() else None

    env = TerminalEnv(core=FakeCore(), actions=(0,), max_episode_frames=1)
    try:
        env.reset()
        _, _, terminated, truncated, info = env.step(0)
        assert terminated
        assert not truncated
        assert info["terminal_reason"] == "test_terminal"
        assert env.current_trace().terminal_reason == "test_terminal"
        assert not env.current_trace().truncated
    finally:
        env.close()
