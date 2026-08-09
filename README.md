# nes-rs
NES emulator in Rust

## Headless test ROM runner

The optional `headless_test` binary runs mapper-0/NROM iNES test ROMs through
the normal headless `Console` path, without initializing SDL. It polls the
Blargg test signature (`$6001-$6003 = DE B0 61`), status at `$6000`, and the
zero-terminated diagnostic text at `$6004`. Exit status 0 means every ROM
reported status 0; status 1 indicates a test failure, missing signature, or
frame-limit timeout. Invalid arguments use exit status 2.

The ROM submodule is optional for normal builds and tests. After initializing
it with `git submodule update --init tests/nes-test-roms`, run the selected
NROM APU tests, for example:

```text
cargo run --features headless-cli --bin headless_test -- \
  --frames 600 \
  tests/nes-test-roms/apu_test/rom_singles/7-dmc_basics.nes \
  tests/nes-test-roms/apu_test/rom_singles/8-dmc_rates.nes
```

The runner deliberately scopes execution to mapper 0 and does not compile or
execute ROMs automatically during `cargo test`; the existing submodule can be
absent without affecting the normal test suite.

The compatible APU ROM subset can also be run as a Rust integration test when
the submodule is present:

```text
NES_RUN_APU_ROM_TESTS=1 cargo test --test apu_roms
```

The test uses the same library execution and result-protocol code as the CLI.
Set `NES_TEST_ROM_ROOT` to use a different checkout of the ROM repository. The
test is skipped when `NES_RUN_APU_ROM_TESTS` is not set to `1`, so ordinary
`cargo test` does not require the submodule or execute ROMs.

## Frontends

The library and headless paths build without native presentation dependencies:

```text
cargo test --no-default-features
```

The first alternative desktop frontend uses winit, softbuffer, and cpal:

```text
cargo run --features winit-frontend --bin nes-winit -- path/to/game.nes
```

The original SDL2 frontend remains available as a migration fallback:

```text
cargo run --features sdl2-frontend --bin nes -- play --rom path/to/game.nes
```
