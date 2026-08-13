import numpy as np

from nes_gym.smb import A, B, DOWN, INIT_SEQUENCE, LEFT, RIGHT, SELECT, START, UP, SuperMarioBros1_1Env


class FakeSnapshot:
    def __init__(self, state):
        self.state = state
    def close(self):
        pass


class FakeCore:
    rom_sha256 = "fake"
    def __init__(self):
        self.ram = np.zeros(2048, dtype=np.uint8)
    def advance_frames(self, controller, frames):
        self.ram[0x006D] = 1
        self.ram[0x0086] = (int(self.ram[0x0086]) + controller * frames) & 255
    def snapshot(self): return FakeSnapshot(bytes(self.ram))
    def restore(self, snapshot): self.ram[:] = np.frombuffer(snapshot.state, dtype=np.uint8)
    def close(self): pass


def test_smb_defaults_and_progress_reward():
    assert (A, B, SELECT, START, UP, DOWN, LEFT, RIGHT) == (
        0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80
    )
    assert INIT_SEQUENCE == ((0, 60), (START, 1), (0, 105))
    env = SuperMarioBros1_1Env(core=FakeCore())
    env.reset()
    _, reward, terminated, truncated, info = env.step(2)
    assert reward == 0x80
    assert not terminated and not truncated
    assert info["world_x"] == 0x188
    env.close()


def test_smb_terminal_ram_map():
    core = FakeCore()
    env = SuperMarioBros1_1Env(core=core)
    env.reset()
    core.ram[0x000E] = 0x06
    assert env.is_terminal()
    assert env.terminal_reason() == "death"
    core.ram[0x000E] = 0
    core.ram[0x0016] = 0x31
    core.ram[0x001D] = 3
    assert env.is_terminal()
    assert env.terminal_reason() == "level_complete"
    env.close()
