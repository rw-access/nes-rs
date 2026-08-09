# nes-web

This is the browser frontend crate and static harness for `nes-rs`.

Build from the repository root with:

```sh
wasm-pack build web/nes-web --target web --out-dir web/nes-web/pkg
python3 web/nes-web/dev_server.py --port 8000
```

Then open <http://localhost:8000/> and choose an iNES `.nes` ROM. The
generated `pkg/` directory is a build artifact and should not be committed.

For LAN debugging, append `?debug=1` to the URL. The development server
collects browser errors and emulator/audio failures at `/__debug` and prints
them to its terminal.

The Rust wrapper exposes `NesWeb.load_rom(bytes)`, `step_frame()`, reusable
`rgba_buffer()` and `audio_buffer()` typed-array copies, controller bitmask
updates, rewind, and opaque save-state objects. `FrameMetadata` reports the
variable audio block length and marks discontinuities so the JavaScript audio
scheduler can flush before scheduling post-rewind audio.
