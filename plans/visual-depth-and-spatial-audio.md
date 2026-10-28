# Cardboard depth and synchronized audio presentation

## Implemented scope

The browser adds an optional cardboard-style 2.5D presentation to the existing
NES output. It uses Canvas2D projected cutout geometry, a shared perspective
camera, one point light, and translucent receiving shadows. It does not infer
objects, reconstruct game geometry, or use a GPU scene/depth buffer.

A compact toolbar toggle enables the effect; advanced controls are optional.
Mobile uses an edge-to-edge viewbox and simple touch controls. Desktop retains
keyboard input and a stationary perspective view. Flat video and original mono
audio remain available without motion access.

## Core and WASM contracts

The `layered-render` feature captures four RGBA planes in backdrop, rear sprite,
tile, front sprite order. The backdrop is the universal NES background color;
clouds, scenery, and HUD drawn as tiles remain on the tile plane. NES priority,
including transparency and sprite priority rules, determines composition.
Recombining the planes must reproduce the canonical framebuffer. Retained frames
during forced blanking retain their attribution; disabled video invalidates it.
Capture must not change hardware-visible behavior or the ordinary framebuffer.

The `audio-stems` feature attributes five synchronized contributions in pulse 1,
pulse 2, triangle, noise, DMC order. Contributions retain the nonlinear mixer
group totals, use the mono sample clock, and share block lengths and
scheduler-discontinuity metadata. Stem filter histories participate in snapshots
and rewind while canonical mono output remains unchanged.

The WASM wrapper exposes reusable planar RGBA and interleaved stem buffers.
Consumers check frame availability and sample counts before using them. These
presentation features do not change native mono exports or require changes to
ROMs, save-state controls, input, or the emulation timeline.

## Scene geometry

Model dimensions use native NES game pixels (256×240), including the Z axis.
Card thickness is independent of backdrop separation. Shallow gaps cap solid
thickness before it reaches the backdrop; zero separation restores flat video.
The front artwork retains its original registration and colors. Side polygons
use an unprinted board material with shared Lambert illumination rather than
stretching the artwork down the extrusion.

There is one thin rounded rim around the opening. Its face, side joins and
receiving shadow use the same outline and camera. Rim thickness is relative to
the foreground card and limited by the remaining gap. Recession moves the rim
behind the foreground; source gameplay paints above it. The exterior uses the
page surround color, avoiding a second cabinet or bezel. An optional shell edge
or recessed opening can retain the aperture cues independently of the dark rim.

Layer relief adds close shadows over receiving artwork in the existing drawing
priority order. Optional overlap thickness and forced cutouts are optical
presentation effects, not recovered object depth. Thin overlaps limit their
thickness to one third of the card; contact shadow clearance is capped
independently of the backdrop gap. Coplanar source faces cannot have physically
separated contact shadows, so these effects deliberately approximate that cue.

## Viewing and motion

Stationary 2.5D uses a shared fixed camera/lamp pose with the depth vanishing
point inside the game. Perspective depth rays converge; there is no isometric
mode. Changing viewing distance adjusts camera perspective without changing
model dimensions or the lamp placement.

Tilt is opt-in and requests motion permission only through its button. The
initial hold becomes the neutral reference, and Recenter re-establishes it.
Look around uses full calibrated relative orientation for sideways peeking;
Ground stable uses gravity and ignores turns about the gravity axis. Neither
mode measures the viewer's head, phone translation, or actual room lighting.
Camera response is bounded and smoothed, with lifecycle handling for absent
sensors, denial, tab suspension, screen rotation, and re-enabling the control.

Physical screen size and eye distance are estimates used at the camera boundary.
Device profiles can be overridden. Screen dimensions, stage size, orientation,
and viewport zoom determine the estimated visible game width. These assumptions
are separate from model dimensions and illumination controls. Simulated sensor
tests do not establish calibration on real hardware.

## Illumination

The point light is placed relative to the initial phone screen plane. Tilt and
Recenter establish its reference frame; subsequent relative rotation changes
its local coordinates with the same scene pose used by the camera. Eye distance
does not move it. Stationary viewing restores its fixed reference placement.

Shadow placement intersects light rays with receiving planes and then applies
the shared camera projection. The solid card consumes part of the backdrop gap;
only the remaining air contributes to cast-shadow clearance. Nearby sources can
expand shadows before camera projection. Side shading and rim shadows use the
same source. Independent softness controls govern gameplay and rim diffusion.

## Default preset

| Control | Default |
| --- | --- |
| Background separation | 85 game pixels |
| Card thickness, stationary and Tilt | 4 game pixels |
| Rim width | 2% |
| Rim thickness | 25% of foreground |
| Rim recess | 5% of gap |
| Gameplay shadow softness | 18% |
| Rim shadow softness | 100% |
| Light distance / elevation | 25 cm / 7° |
| Viewing distance | 35 cm phone/desktop; 50 cm tablet |
| Solid sides, layer relief, thin overlaps, rim, rim shadow | On |
| Shell-colored edge, recessed opening | Off |

Sliders use integer intervals. Reset defaults restores scene geometry, viewing,
and lighting without clearing the ROM, save state, audio state, or Tilt state
and without requesting permission. Manual card thickness can be retained
separately for stationary and Tilt modes during the page session.

## Browser state and audio

The most recently loaded valid ROM is cached in local IndexedDB and restored on
revisit. Invalid replacements do not overwrite it. A manual selection supersedes
a pending restore; storage failure preserves manual loading. Caching does not
upload ROM bytes. Save-state and rewind behavior remain independent of the cache.

Spatial audio helpers provide synchronized resampling and channel separation,
but their soundstage control is hidden pending listening/tuning. Current playback
uses the original mono mix. Convolution, room effects, HRTF, native stereo, and
enhanced offline exports are future work, not delivered features.

## Validation and future work

Browser unit tests exercise perspective and shadow geometry, rim joins, physical
unit conversions, calibrated motion and lifecycle, reset-related defaults, ROM
storage, and audio alignment. Core tests exercise priority capture, forced-blank
retention, feature-disabled behavior, mixer contributions, filtering, snapshots,
and both scheduling paths. Build validation includes the WASM feature set.

Run the relevant suites from the repository root:

```sh
node --test web/*.test.mjs
cargo test -p nes-core
cargo test -p nes-core --features audio-stems,layered-render,timestamped-scheduler
cargo check -p nes-wasm --target wasm32-unknown-unknown
```

The proposed [unified GPU scene](unified-cardboard-scene.md) is design only. It
would use real cutout meshes, depth testing and a shared shadow pass, with an
explicit tradeoff between strict source registration and separated sheet
parallax. The current renderer remains the Canvas2D presentation described here.
