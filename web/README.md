# Browser frontend

This is the static browser frontend for `nes-rs`. The Rust/WebAssembly
wrapper lives in `crates/nes-wasm`; this directory contains the HTML, CSS, and
JavaScript shell that loads its generated package.

Build from the repository root with:

```sh
wasm-pack build crates/nes-wasm --target web --out-dir ../../web/pkg
python3 web/dev_server.py --port 8000
```

`wasm-pack` resolves `--out-dir` from the crate directory, so `../../web/pkg`
places the generated package beside this static shell.

Then open <http://localhost:8000/> and choose an iNES `.nes` ROM. The
generated `web/pkg/` directory is a build artifact and should not be
committed.

For LAN debugging, append `?debug=1` to the URL. The development server
collects browser errors and emulator/audio failures at `/__debug` and prints
them to its terminal.

## Capturing a play sequence

Load the ROM, press `● Record`, play normally, and include any rewind with `R`
or the on-screen Rewind button. Press `Stop & upload` when finished. The server
saves one JSON transcript outside the repository, by default under
`/tmp/nes-rs-captures/`. Each transcript contains the controller state for each
emulated frame plus button and rewind events, so it can be replayed later and
rendered into a video.

The generated wrapper exposes ROM loading, frame stepping, reusable video and
audio buffers, controller updates, rewind, save-state objects, and a compact
`ghost_buffer()` containing the shared core timeline's active sprite ghosts.
Frame metadata reports the variable audio block length and marks
discontinuities so the JavaScript audio scheduler can flush before scheduling
post-rewind audio. Ghost capture/replay state is owned by `nes-core`; the
browser only composites the current buffer over Sprites/Both output.
