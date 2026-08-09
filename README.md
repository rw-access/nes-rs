# nes-rs
NES emulator in Rust

[Try the WASM demo](https://rw-access.dev/nes-rs/)

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
cargo run -p nes-render --features headless-cli --bin headless_test -- \
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
NES_RUN_APU_ROM_TESTS=1 cargo test -p nes-render --test apu_roms
```

The test uses the same library execution and result-protocol code as the CLI.
Set `NES_TEST_ROM_ROOT` to use a different checkout of the ROM repository. The
test is skipped when `NES_RUN_APU_ROM_TESTS` is not set to `1`, so ordinary
`cargo test` does not require the submodule or execute ROMs.

## Frontends

The library and headless paths build without native presentation dependencies:

```text
cargo test --workspace
```

The first alternative desktop frontend uses winit, softbuffer, and cpal:

```text
cargo run -p nes-play --features winit-frontend --bin nes-winit -- path/to/game.nes
```

The original SDL2 frontend remains available as a migration fallback:

```text
cargo run -p nes-play --features sdl2-frontend --bin nes -- play --rom path/to/game.nes
```

### WASM/browser frontend

The browser shell lives under `web/`, while the Rust/WebAssembly wrapper is the
`crates/nes-wasm` workspace crate. Rust is compiled to WebAssembly, and the
HTML and JavaScript load the generated package, create the display, load ROMs,
and translate browser input into NES controller state.

Prerequisites:

- Rust and Cargo
- [`wasm-pack`](https://rustwasm.github.io/wasm-pack/installer/)
- A local HTTP server (browsers generally block WebAssembly modules and ROM
  resources when the page is opened directly with `file://`)

From the repository root, build the WebAssembly package into `web/pkg/`:

```text
wasm-pack build crates/nes-wasm --target web --out-dir ../../web/pkg
```

The `../../` output path is relative to `crates/nes-wasm`, placing generated
files in the static shell's `web/pkg/` directory.

Serve the frontend over HTTP, then open the URL shown by the server. For
example, with Python installed:

```text
python3 web/dev_server.py --port 8000
```

Open <http://localhost:8000/> in a browser. Use the frontend's ROM file picker
to load an iNES `.nes` ROM; the browser reads the selected file and passes its
bytes to the emulator. ROM files are not bundled into the WASM package.

#### GitHub Pages

The repository includes `.github/workflows/pages.yml`, which builds and
deploys the browser frontend automatically whenever `main` changes. To enable
it, open the repository's **Settings → Pages** page and set **Source** to
**GitHub Actions**. After the first successful run, the site will be available
at `https://<owner>.github.io/nes-rs/` (or the custom domain configured in
GitHub Pages).

The default keyboard mapping is:

| NES button | Keyboard |
| --- | --- |
| A | `Z` |
| B | `X` |
| Select | `Shift` |
| Start | `Enter` |
| D-pad | Arrow keys |

The browser frontend should prevent browser actions for keys used by the
emulator (especially the arrow keys and `Enter`) while the game has focus.
