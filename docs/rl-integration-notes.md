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

## Runtime rewind variant

The native rewind tape is compiled into the FFI library, but rewind is exposed
to Python only when `rewind_enabled=True` (or `--rewind-enabled` in the PPO
examples). The SMB action mapping then appends the sentinel `REWIND_INPUT =
0x100`, which is outside the NES controller byte range and cannot be combined
accidentally with buttons. A rewind action rewinds one completed frame per
frame in the configured `frame_skip` budget and receives zero reward.

When enabled, a raw SMB death is held pending for 60 forward frames by default
(configurable as `rewind_grace_frames`). Rewinding clears that pending death if
the emulator returns to a non-death state. The sentinel is stored in the RLE
episode trace, and the Python replay helpers understand it.

## Trace video export, audio, and rewind ghosts

`python/src/nes_gym/replay.py:export_trace_video` restores the materialized
checkpoint, enables native video output, replays the exact RLE input trace,
and pipes one RGBA frame per emulated frame to FFmpeg. The FFI now exposes a
reusable per-frame audio view at 48 kHz; the exporter writes a temporary WAV
and muxes it into the final AAC MP4. Rewind frames contribute silence because
they are state navigation rather than newly emulated audio.

The FFI build enables `layered-render`. The existing core rewind timeline
captures rewound sprite frames and fades them over subsequent normal frames;
the FFI compositor overlays those sparse ghost sprites in the exported RGBA
frame, so rewind-triggered traces retain the visual tape effect.

## Evenly split rewind/no-rewind PPO runs

`python/examples/staged_ppo.py` is a single resumable training chain, not a
combined A/B runner. For an even comparison, run two independent chains with
identical segment counts, per-segment timesteps, seed, horizon/reward options,
and evaluation settings. Give each chain its own repository-local workdir and
allocate half of the total timestep budget to each. For example, this gives
1,080,000 training steps to each variant and 2,160,000 combined:

```powershell
$rom = "roms\Super Mario Bros. (World).nes"

uv run --directory python python examples/staged_ppo.py $rom `
  --workdir "artifacts\staged-ppo-no-rewind" `
  --segments 6 --segment-timesteps 180000 `
  --seed 0 --device cpu

uv run --directory python python examples/staged_ppo.py $rom `
  --workdir "artifacts\staged-ppo-rewind" `
  --segments 6 --segment-timesteps 180000 `
  --seed 0 --device cpu `
  --rewind-enabled --rewind-grace-frames 60
```

Do not resume the rewind chain from a non-rewind checkpoint, or vice versa:
the policies have different action heads (7 actions versus 6). The staged
runner's default `--workdir ../artifacts/staged-ppo` is relative to the
process working directory, so explicit paths are recommended. Its current
`--stochastic-eval` declaration is effectively enabled by default; use the
same evaluation mode for both runs, and treat the resulting sampled evaluation
trace/video as comparison artifacts rather than deterministic policy scores.

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
