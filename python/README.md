# nes-gym Python integration

This package is a headless Gymnasium layer over the `nes-rs` C ABI. It
contains no emulator, reward discovery, pixels, or PPO implementation.

## Native ABI assumption

The current nes-rs library uses opaque `NesHandle` and `NesSnapshot` pointers
and exposes this status/out-parameter form:

```c
typedef struct NesHandle NesHandle;
typedef struct NesSnapshot NesSnapshot;
typedef unsigned int NesStatus;
typedef _Bool bool;
NesStatus nes_create(const uint8_t *rom_ptr, size_t rom_len, NesHandle **out_handle);
void nes_destroy(NesHandle *handle);
NesStatus nes_advance_frames(NesHandle *handle, uint8_t controller_bits, uint32_t frames);
NesStatus nes_snapshot(const NesHandle *handle, NesSnapshot **out_snapshot);
NesStatus nes_restore(NesHandle *handle, const NesSnapshot *snapshot);
void nes_snapshot_destroy(NesSnapshot *snapshot);
const uint8_t *nes_ram_view(const NesHandle *handle);
size_t nes_ram_view_len(void);
const char *nes_last_error(void);
NesStatus nes_set_video_output(NesHandle *handle, bool enabled);
const uint8_t *nes_framebuffer_view(const NesHandle *handle);
size_t nes_framebuffer_view_len(void);
```

Zero status is success. The Python wrapper keeps a NumPy `uint8` view over the
native reusable 2048-byte RAM buffer; it does not copy RAM on every frame.
Set `NES_FFI_LIBRARY` or pass `library=` to select the shared library. It is
searched for in `target/{release,debug}` by default.

## SMB World 1-1

`SuperMarioBros1_1Env` runs the verified initialization sequence
`NOOP * 60, START * 1, NOOP * 105`, then snapshots that state as its root.
Reset restores the snapshot. Actions are
`[NOOP, LEFT, RIGHT, A, LEFT|A, RIGHT|A]`; each action is held for
`frame_skip` complete frames (default 1).

Each episode records the actual controller state for every emulated frame in
RLE form. `env.last_trace` can be replayed with `env.replay_trace(trace)`.
Terminal reasons are `death` or `level_complete`; the external frame limit is
reported as truncation with reason `max_frames`.

## Tests and example

Native-independent tests use a fake core. Native discovery is skipped when no
shared library is present:

```powershell
uv run --directory python pytest
uv run --directory python python examples/random_episode.py "..\roms\Super Mario Bros. (World).nes"

Save and verify a trace, then export only its episode frames through the
in-process framebuffer and FFmpeg:

```powershell
uv run --directory python python examples/random_episode.py `
  "..\roms\Super Mario Bros. (World).nes" `
  --frames 600 `
  --trace "..\artifacts\episode.json" `
  --verify-replay `
  --video "..\artifacts\episode.mp4"
```

The PPO smoke entry point uses Stable-Baselines3 and supports both CPU and
CUDA devices:

```powershell
uv run --directory python python examples/train_ppo.py `
  "..\roms\Super Mario Bros. (World).nes" `
  --total-timesteps 2048 --device cpu
```

On this Windows host, the installed PyTorch wheel is currently blocked at
import time by Application Control (`torch_global_deps.dll`), so the training
command reports that issue clearly until the policy is adjusted.
```
