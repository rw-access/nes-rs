# nes-web

This is the browser frontend crate and static harness for `nes-rs`.

Build from the repository root with:

```sh
wasm-pack build web/nes-web --target web --out-dir web/nes-web/pkg
python3 -m http.server 8000 --directory web/nes-web
```

Then open <http://localhost:8000/> and choose an iNES `.nes` ROM. The
generated `pkg/` directory is a build artifact and should not be committed.

The Rust wrapper exposes `NesWeb.load_rom(bytes)`, `step_frame()`, reusable
`rgba_buffer()` and `audio_buffer()` typed-array copies, controller bitmask
updates, rewind, and opaque save-state objects. `FrameMetadata` reports the
variable audio block length and marks discontinuities so the JavaScript audio
scheduler can flush before scheduling post-rewind audio.
