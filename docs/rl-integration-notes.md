# NES Gymnasium / RL Integration Notes

This note records the current empirical SMB World 1-1 calibration and the
initial game-specific terminal-state interpretation. It is intentionally
outside the emulator core: reward, metrics, and episode semantics belong to
the Python environment.

## World 1-1 initialization

For `roms/Super Mario Bros. (World).nes`, the verified RLE initialization
sequence is:

```python
[
    (NOOP, 60),
    (START, 1),
    (NOOP, 105),
]
```

This reaches emulator frame 166, with playable World 1-1, Mario at the start,
and the in-game timer at `400`. `NOOP x 30` also reaches the title screen, but
`NOOP x 60` is retained as a safer startup margin.

The sequence was replayed twice and produced identical final rendered-frame
hashes. The initialization sequence is run once when constructing the game
environment; the resulting emulator state is cloned as the canonical root
checkpoint. Normal episode reset restores that checkpoint.

## Initial SMB terminal signals

These RAM locations are interpreted from the CPU RAM observation and should be
kept in the World 1-1 environment, not in generic `NesEnv`:

```python
is_dying = ram[0x000E] == 0x0B or ram[0x00B5] > 1
is_dead = ram[0x000E] == 0x06

is_stage_over = (
    any(ram[address] in (0x2D, 0x31)
        for address in (0x0016, 0x0017, 0x0018, 0x0019, 0x001A))
    and ram[0x001D] == 0x03
)

is_world_over = ram[0x0770] == 0x02
```

For the single-level v0 environment:

```python
terminated = is_dying or is_dead or is_stage_over
truncated = elapsed_frames >= max_episode_frames
```

Suggested terminal reasons are `"death"`, `"level_complete"`, and
`"max_frames"`. Death should normally terminate at the beginning of the death
animation rather than spending rollout frames on the full animation.

The RAM interpretation is consistent with the established
`gym-super-mario-bros` implementation:

<https://raw.githubusercontent.com/Kautenja/gym-super-mario-bros/master/gym_super_mario_bros/smb_env.py>

## Related design constraints

- Snapshots contain deterministic emulator state only.
- Observation/view buffers, Gym state, rewards, traces, and experiment
  metadata are not part of snapshots.
- Episode traces record actual controller state for every emulated frame using
  RLE.
- Replay is `starting checkpoint + exact controller trace`.
- Root and derived-checkpoint replay must be covered by deterministic tests.
