# Browser frontend

This static player loads the Rust/WebAssembly wrapper from `crates/nes-wasm`.
Build and serve it from the repository root:

```sh
wasm-pack build crates/nes-wasm --target web --out-dir ../../web/pkg
python3 web/dev_server.py --port 8000
```

Open <http://localhost:8000/> and choose an iNES `.nes` ROM. `wasm-pack` resolves
its output relative to the crate directory, so the generated package belongs in
`web/pkg/`. Generated packages and ROMs are not committed. The latest valid ROM
is cached in browser-local IndexedDB and restored on the next visit; manual
loading remains available if storage fails. Different site addresses have
separate caches.

## One player, several views

The same running game, audio, input, saved state and rewind history are retained
when switching views:

- **Original 2D** displays the untouched native RGBA picture.
- **Layered** views the shared 3D scene head-on, fitted without stretching or
  cropping its 256×240 aspect ratio. The lit backdrop extends sideways; portrait
  retains clear space above and below the game.
- **Tilt** steers that scene using a calibrated phone orientation. Select it
  from the toolbar, hold the phone comfortably and grant motion access if asked.
  Recenter establishes a new neutral pose. Missing permission or sensor data
  leaves ordinary views available.
- **Orbit** allows one-finger dragging or mouse dragging to rotate the diorama.
  Pinch with two fingers or scroll to zoom. Releasing one finger after a pinch
  resumes rotation from its current position.
- **VR** renders the same geometry stereoscopically with native head tracking.

Layered, Tilt, Orbit and VR controls sit together in the toolbar. Desktop view
changes ease the current camera or crossfade, respecting reduced-motion
preferences. Returning to play clears toolbar focus and sticky touch hover;
mode indicators retain their icon color. On small screens, fullscreen is in
More; desktop also has a toolbar fullscreen button. Legacy `vr.html` links
redirect into this player.

The 3D model extrudes native pixel silhouettes into lit prisms with original
front/back artwork, outward-facing sides and real shadows. It does not infer
unseen artwork or reconstruct semantic game objects. Layered, Orbit and VR use
the same geometry, materials and lights; Orbit and VR include a tabletop.
Three.js 0.180.0 is vendored with its MIT license; no runtime CDN is needed.
One latest-only worker job builds edges while head tracking renders separately.
Frozen desktop scenes redraw on changes rather than continuously.

## Settings and controls

Settings renders the current frozen scene in three starting points:

- **Miniature relief:** 70 cm stage, 4 cm backdrop depth, 30 mm cards.
- **Pressed layers:** 140 cm stage, no air gap, 30 mm cards.
- **Paper theatre:** 90 cm stage, 24 cm backdrop depth, 16 mm cards.

Stage size, backdrop depth, thickness, matte/eggshell/satin finish, shadows and
VR resolution apply to the shared scene. Saved defaults and changed-setting
notes are local to the browser. Mobile preset cards are one per row. Stage
width ranges from 50 cm to 5 m; thickness and backdrop depth retain their physical
dimensions when it changes.

FPS starts hidden. **Show FPS counter** in Settings remembers an independent
choice for the page and headset. Render counts render-loop submissions (one
stereo pair is one frame), Game counts emulated frames, and Scene counts applied
worker snapshots. These counters do not measure compositor scanout. A frozen
desktop scene can report zero Render FPS.

Touch controls sit below the game in portrait and split across side rails in
landscape. They support separate fingers, diagonals and sliding on the D-pad,
without text selection or long-press callouts. Standard gamepads are discovered
when a button is pressed; Settings can show or hide touch controls explicitly.
Keyboard mappings are WASD/arrows, J/K or Z/X for A/B, minus/Shift for Select and
plus/Enter for Start. Hold L1 or R to rewind; M mutes, F toggles fullscreen and
V cycles the displayed emulator planes. More contains Pause/Play, Recenter,
Save state, Restore state and held Rewind. Focus loss clears held inputs.

## Headset use

VR needs HTTPS or a trusted localhost origin, a compatible browser and a
connected headset runtime. The button checks immersive WebXR availability and
requests a session only when clicked. PSVR2 uses a Windows PC with its PC adapter
and VR runtime; this is not a PS5 browser application.

The diorama stays anchored while you look around. Left stick moves; right
trigger/lower face button is A; right grip/upper face button is B. Left lower
and upper face buttons are Select and Start. Hold left grip to rewind. Click
both sticks to recenter; right stick up/down resizes the anchored stage.
The headset system menu or the page's Exit VR button ends the session.

The classic `XRWebGLLayer` path is the default; `?xr=layers` enables projection
layers for comparison. Shader preparation precedes entry. Balanced resolution
uses 65% of recommended dimensions without multisampling; Performance and High
are also available. Headset visibility controls drawing independently of HTML
document visibility. Blurred sessions keep rendering while game input pauses.
VR entry reveals the scene without animating the tracked camera. A desktop
view selection cancels a pending VR request or leaves an active session.
Actual headset comfort, compatibility and performance require hardware checks.

## Diagnostics and testing

Ordinary visits upload no capture or diagnostics. `?debug=1` exposes developer
replay capture as a local JSON download and keeps bounded debugging events in
`window.nesDebugEvents`. `?diagnostics=1` additionally enables bounded graphics
reports to an optional same-origin `__graphics-log` collector. Static hosting
does not supply that collector. Reports exclude ROMs, images, filenames, input,
cookies and page URLs, and stop after ten minutes or 60 reports.

Run helper tests from the repository root:

```sh
node --test web/*.test.mjs
```

With a local server and generated WASM available, the `*-browser-check.mjs`
files exercise shared game state, exact flat colors, local capture privacy,
mobile input, settings, pinch zoom, transitions, GPU rendering and simulated
WebXR. They accept `NES_PREVIEW_URL`, `PLAYWRIGHT_MODULE` and `CHROMIUM_PATH`.
For example:

```sh
NES_PREVIEW_URL=http://127.0.0.1:8000/ node web/shared-player-browser-check.mjs
```

The WebXR suite simulates stereo views, tracking, visibility, XR input, failure
cleanup and canceled permission/setup. Simulation does not establish physical
headset performance. Browser fixtures generate original synthetic ROMs.
