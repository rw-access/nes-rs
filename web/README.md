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

The latest successfully loaded ROM is saved in this browser's IndexedDB and
restored automatically on the next visit. Storage failures leave manual loading
available. ROM bytes stay on the device; each site address has its own cache.

## Phone viewbox

The mobile layout uses an edge-to-edge game opening and simple touch controls.
There is no decorative television or controller skin. Desktop keeps keyboard
controls and a stationary perspective view. With Tilt off, a modest fixed
12° horizontal / 8° vertical viewing angle keeps 4-pixel card sides readable
on phones and larger screens. The vanishing point can lie outside the opening;
back-plane point spacing still shrinks with depth under perspective. Camera, lighting, rim and shadows
share the same box pose; the source artwork stays aligned. Turning Tilt on
switches to the calibrated phone view, and
turning it off restores the fixed view, including after recentering or a sensor
failure. Flat video remains neutral.

On an HTTPS connection, tap **Tilt** while holding your phone comfortably and
allow motion access if prompted. That initial pose becomes the neutral view.
The adjacent crosshair button recenters immediately; **Motion strength** in
the advanced settings controls the response. Screen rotation and returning
to the tab establish a fresh baseline. Disabling Tilt restores the static view.
Permission denial or missing sensor data leaves the ordinary view available.

**View tracking** separates two sensor assumptions. The default **Look around**
follows full relative gyro orientation, so bringing a left or right edge toward
you reveals the card sides even with an upright phone. It bounds the camera
response through large turns; use Recenter after turning your body. **Ground
stable** uses gravity only and ignores turning around the room, but cannot
recover sideways peeking about the gravity axis. Sensors alone cannot know
whether your head turned with the phone. Changing modes recenters the hold.

Both modes use an off-axis camera. Original game faces remain anchored to the
display while sides, rim and receiving shadows respond. The lamp starts at a
fixed distance/elevation from the phone's initial screen plane, then follows
the same relative pose in the renderer. Recenter re-establishes that reference.
It is not attached to the currently estimated eye position. Motion permission
is still requested only by Tilt; changing tracking mode does not request it.

Look around preserves the physical viewing ray in the supported forward range,
with a 60-degree bound at normal strength. Extreme/back-facing input remains
bounded and continuous rather than passing the virtual eye behind the screen.
Ground stable retains its 75-degree limit. This models lit cardboard cutouts,
without reconstructing level geometry, tracking your head, or inferring phone
translation. The actual phone feel still needs hardware testing.

**Viewing distance** estimates eye-to-screen distance from 20 to 100 cm, in
whole centimeters. Phone presets start at 35 cm and tablet presets at 50 cm.
It changes camera perspective and motion together, including the camera's
normal distance as the device rotates. **Motion strength** remains an artistic
gain: 50% uses the calibrated ray, while the experimental 100% default doubles
the view response to reveal thin edges with smaller movements. Lower values
soften movement. Camera and lighting share the amplified virtual pose, keeping
their relative placement consistent. At 50% the forward pose follows measured
phone rotation; amplified settings represent a virtual room. In-plane roll stays
one-to-one. This is not head tracking. Recenter keeps the selected viewing distance.

**Screen size** supplies an approximate physical display width. Auto uses the
screen dimensions and pointer type to choose a phone or tablet size; it does
not identify hardware. Override it for large phones or 11/13-inch tablets.
Display estimates are based on the current [iPhone Pro displays](https://www.apple.com/iphone-18-pro/specs/),
[Pixel displays](https://support.google.com/pixelphone/answer/7158570?hl=en), and
[iPad Pro displays](https://www.apple.com/ipad-pro/specs/). The visible game width,
orientation, layout, and pinch zoom determine the actual estimated opening size.
Desktop Auto estimates a 768 CSS-pixel game as 24 cm wide, scaling with the
visible stage width and viewport zoom. These are calibration
assumptions, not measurements of your eyes or head movement.

The virtual cardboard depth retains an artistic scale (19.2 native pixels per
depth unit) so a phone at 35 cm with a 7 cm opening preserves the established
static preset. This is not a reconstruction of physical game objects.

**Recessed opening** retains the aperture's width, recession, and shadow when
the black **Rim** is off. It uses the surrounding shell material; foreground
gameplay remains visible above the opening.

For LAN debugging, append `?debug=1` to the URL. The development server
collects browser errors and emulator/audio failures at `/__debug` and prints
them to its terminal.

## Picture depth and stereo sound

The compact **2.5D** toolbar button switches between the original flat picture
and the depth effect without changing your selected settings. It starts on.
The adjacent settings button opens the visual controls while effects are on;
turning effects off closes the panel and disables its settings button. The default
preset is Background separation 85 game pixels, Card thickness 4 game pixels in
both stationary and Tilt modes, Shadow softness
18%, with Solid sides, Thickness + shadows layer relief, Thin overlaps, and TV
rim shadow enabled. Light distance starts at 25 cm with 3° elevation in Tilt,
7° stationary, and a centered horizontal position. All depth uses center perspective;
there is no separate isometric mode or projection switch.
The soundstage control is hidden for now; playback uses the original mono mix.
Advanced settings separate **Scene geometry**, **Viewing setup**, and **Lighting**.
**Reset defaults** at the bottom restores every control in those groups while
keeping the panel open, the loaded ROM, saved state, audio and Tilt on/off state.
It resets both modes' thickness defaults; manual thickness adjustments otherwise
stay with their mode when switching Tilt on/off. It does not request motion
permission. Auto screen sizing selects its appropriate
viewing-distance default again after reset.
Geometry is measured in native game pixels: one unit is the width of one pixel
in the 256×240 NES frame, including depth along Z. These are model units, not
CSS/display pixels. Card thickness ranges from 0 to 8 game pixels in integer
steps, independent of backdrop separation, screen profile, viewing distance,
and zoom. A very shallow gap limits the card to fit; zero separation restores
flat video. Rim thickness/recess remain ratios and show their resolved game-pixel
sizes alongside the percentages. Viewing distance alone uses estimated physical
centimeters; screen size calibrates the camera, not the model dimensions.
Light distance, elevation, and the two shadow softness controls are in Lighting.

Cut edges inherit the exact RGB color of each adjoining sprite or tile pixel
and receive shading from the shared light. Adjacent edge pixels of the same
color merge into runs; color changes split the side material without adding
internal walls. The default four-pixel card is
roughly 1 mm on a phone-sized opening, but its physical display size scales
with the game, rather than depending on an estimated device profile.

One thin inner frame surrounds the game, without a decorative cabinet or a
second dark bezel. Only that frame has projected depth.
Its front edge starts slightly behind the foreground (1% recess). Its sides project
inward over exposed background, below every sprite and tile layer. A subtle
0.5% corner radius rounds the opening for both flat and 2.5D output. The front
rim, curved wall joins, and shadow share this outline; the outer corner radius
adds the actual face width so even hairline rims remain concentric. Source
resolution and aspect ratio stay unchanged.

**Rim** switches the entire frame off, including its face, sides, and shadow.
**Rim width** ranges from a full frame (100%) to none (0%), defaulting to a
hairline 1%, with 1% adjustment steps. It never changes game size or
position. **Rim thickness** controls
its solid depth separately: use 0% for a flat paper border, or a small percentage
for a thin surrounding tile. Turning the rim off retains these settings.
**Shell-colored edge** replaces the black insert with a pale material,
including its lit sides and bevel, so the opening feels cut into the console
itself. Uncheck it to compare with the charcoal rim; all depth controls still apply.

The **Background separation** slider lifts the complete gameplay surface above
the full-size NES backdrop through one combined silhouette shadow. It starts at
85 game pixels and ranges from 0 to 115, in integer steps. It is the master
distance: zero restores flat video regardless of other controls. Sprite and tile fronts retain their original
positions and scale, so Mario stays in contact with terrain. The light sits
in front of the scene, near the top and horizontally centered. The backdrop is the universal NES
background color; clouds, hills, and HUD drawn as tiles remain with gameplay.
NES drawing priority continues to determine occlusion, not physical distance.
A nearby frontal light spreads shadows outward before the camera projects
them onto their receiving surface. This can counter the camera's shrinking;
shadows, sides, and the rim still share one perspective. The caster is the complete extruded silhouette: front, rear, and connecting
cut edges. Their projections are combined before applying translucency once,
so solid thickness is neither added twice nor reduced to a rear-face shadow.
At contact, the rear silhouette meets the receiving board while the exposed
solid can still cast a shadow beyond that contact. Gameplay artwork stays aligned.

**Light distance** ranges from 5 to 300 cm, default 25 cm, measured from the
phone's initial screen center. **Light elevation** is 0–80°, above the initial
screen normal: 3° by default in Tilt for a subtle downward cast after Recenter,
and 7° in stationary mode. Manual elevation values are retained separately for
each mode during the page session; Recenter preserves them. Distance moves
the fixture along that ray, preserving
its angle while making illumination steadier as the source moves farther away.
Elevation changes the angle without changing the distance. **Light horizontal
angle** moves it left or right of the initial screen normal, from −60° to 60°,
with a centered 0° default, avoiding a preferred left/right cast direction. These controls affect side shading,
backboard illumination, gameplay shadows and rim shadows together; model
dimensions and source art remain fixed. Viewing distance does not move the lamp.

The backboard receives ambient and Lambert diffuse illumination from this same
room-fixed fixture. Each visible pixel is traced to its point on the farther
board before shading, so changing the view does not paint the light gradient
onto the front screen plane. The original printed gameplay faces retain their
colors. Flat video bypasses this shading.

Tilt / Recenter anchors the fixture in the initial phone frame. Rotation changes
its local coordinates; disabling Tilt restores the static position. The app
does not measure actual room lights or phone translation. Very oblique or
back-facing rays fade safely. This replaces the old abstract 1–8× multiplier.

**Rim thickness** is a percentage of the foreground thickness, defaulting to
25%. It follows changes to the foreground knob, limited by the distance left
before the backdrop. The output indicates when this limit reduces the thickness.

**Rim recess** moves the rim behind gameplay: 0% is flush, the default 1% is a
slight recess, and 100% places its front at the backdrop. Its front, sides, and shadow follow the
same camera as the game. Gameplay stays fixed and paints over the rim; even a
paper rim can recede. Its solid thickness is capped so the rear cannot pass the
backdrop. Zero separation or effects off restores the flush flat frame.
When recessed, the cabinet covers backdrop outside the projected frame; sprites
and tiles still paint above it. This prevents a second strip of sky outside the
rim without hiding any foreground artwork.

**TV rim shadow** casts onto the farther backdrop at 18% opacity. The
frame does not cast forward onto gameplay faces. **Rim shadow softness** controls
its diffusion separately from gameplay **Shadow softness**, on the same 0–100%
scale with a fully diffuse 100% default. It is disabled when Rim or Rim shadow is off, retaining
the selected value. Zero separation collapses every effect; paper mode
also flattens the frame's visible reveal.

The background remains full size. Rim sides cover only exposed background at
the edges; foreground tiles and sprites paint above them. There is no smaller
background-card crop, foreground mask, or scene inset. The original NES
scrolling crop remains unchanged.

Solid edges use Lambert light and an ambient term. Each side's position and
orientation determine its illumination from the same source that casts the
gameplay and rim shadows. Left and right parts of the scene can therefore cast
in different directions while retaining a downward lighting bias. The top light
reveals the cardboard thickness and a short backdrop shadow, with no uniform
sideways shadow offset.
The rim's exposed sides are charcoal, with a narrow lit bevel at the front and
subtle darkening at the rear edge. These transitions stay narrow as thickness
increases; the main side faces retain directional lighting.

**Layer relief** starts on with **Thickness + shadows** selected. It projects close,
translucent shadows over the receiving artwork while keeping source faces
aligned. Front sprites cast over tiles; tiles cast over sprites behind them.
Choose **Shadows only**, **Thickness + shadows**, or
**Forced cutouts + shadows**. Thickness and Shadow softness control these cues;
no detached layer is introduced. Shadows between pieces use a tiny contact
clearance, capped independently of the backdrop distance, so deeper scenery
does not make Mario's shadow float away from the tiles.

Thickness + shadows reveals a touching cardboard edge where the original
sprite boundary overlaps a tile. Unsupported solid edges, such as feet merely
touching the floor, keep their normal clipping. Forced cutouts instead lets
the whole projected sprite edge cover tile pixels it crosses—an intentional
optical illusion that may look better with thin cardboard. Shadows remain
translucent in every mode. Tile sides already draw above rear sprites; their
relief shadow draws underneath those sides and the original tile face. NES
occlusion remains intact. Mixed tile/sprite artwork, such as portions of a coin,
can look inconsistent because these options follow drawing priority without
guessing which pixels form an object. Solid sides off or zero thickness removes
the effect while retaining the selected controls.

**Thin overlaps (⅓ thickness)** starts checked and becomes available with Layer
relief. It caps depth over receiving artwork to one-third of Thickness in both
priority directions; edges against the backdrop keep full thickness. Contact
shadows use a small, capped fraction of that distance. Uncheck it to compare full
overlap thickness, or select Shadows only to compare shadows without new sprite
sidewalls over tiles. Tile walls already above rear sprites also become thinner
in their overlap region. Source faces and the combined backdrop shadow stay fixed.

**Shadow softness** ranges from 0% (crisp) to 100% (diffuse). Its normalized scale
reaches the former 200% blur multiplier at 100%. Its default is 18%.

**Solid sides** toggles paper versus solid edges. **Card thickness** ranges
from paper at 0 to 8 game pixels, defaulting to 4. The two scene sliders use
integer model-pixel steps. Changing separation leaves thickness fixed except
when the gap is shallower than the card. The full solid determines
the gameplay shadow silhouette; its diffusion uses the air behind the card.
Rim shadows use the rim's own clearance.
Turning Solid sides off preserves the selected material size. `V` still cycles Background,
Sprites, and Both diagnostic views.

Sidewalls join exposed pixel edges into continuous polygons with smooth edges
at display resolution; the source artwork stays crisp. Rear edges converge toward
the screen center. The camera does not tilt the picture, and Thickness never
moves the front artwork. Cut edges extend the adjoining artwork color down
the extrusion, including opaque black outlines, with directional lighting. Opaque tiles hide added front-sprite
sidewalls so the cosmetic thickness does not project feet over a floor or pipe.
During game loading transitions that temporarily stop drawing, retained picture
pixels keep their layer attribution and effects until the game replaces them.
These remain projected cutout surfaces, not a reconstructed 3D game world.

Press Enable audio to start playback. Spatial audio code remains available for
future tuning, but its soundstage control is currently shelved and playback uses
zero stereo width. The visual sliders support keyboard arrows without sending
those keys to the game. Settings currently last for the page session.
Room/reverb and headphone HRTF processing are planned follow-ups.

The core exposes channel contributions under `audio-stems`. They retain the
nonlinear mixer group totals and run on the same sample clock as mono output.
The WASM bridge returns `audio_stem_buffer()` in interleaved pulse 1, pulse 2,
triangle, noise, DMC order, with `audio_samples * 5` valid floats. Its optional
`depth_buffer()` contains four planar RGBA frames in backdrop/rear/tiles/front
order. Enable it with `set_depth_enabled(true)` and only consume it when frame
metadata reports `depth_available`.

Run the browser presentation helper tests with:

```sh
node --test web/*.test.mjs
```

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
