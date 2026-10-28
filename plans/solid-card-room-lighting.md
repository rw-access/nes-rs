# Solid-card shadows and room-relative illumination

## Diagnosis

The calibrated Look pose and point-light rotation pass independent room-space
ray checks and actual browser roll checks. Those checks did not validate the
whole solid shadow caster: the app supplies original front-face silhouettes,
then projects them from the card's rear depth. It also displays an unlit
backdrop. A near-axis lamp and subpixel cut edges make the effect especially
hard to read at small tilts.

## Change contract

- Cast the complete extruded alpha silhouette: front cap, rear cap and the
  connecting boundary faces. Project all of them through the same point light
  onto the receiver and through the same camera onto the display. Union them
  before applying shadow opacity, preventing overlapping layers from stacking
  darkness. Keep printed foreground faces and native priority unchanged.
- Illuminate the receiving backboard with ambient plus Lambert diffuse light.
  Invert the camera projection at each display pixel before evaluating the
  receiver point. Its normal faces out of the box; transform the fixed room
  fixture through the measured phone pose. Flat mode remains the native image.
- Add an independent horizontal lamp angle. An 8-degree initial left offset makes
  the cast direction readable without moving the lamp with the eye or changing
  model depth. Distance stays 25 cm and elevation stays 7 degrees.
- Keep all dimensions in native game pixels and all controls at integer steps.
  Preserve the requested 1% rim width/recess defaults, optional advanced menu,
  ROM cache, save state, and tap-only motion permission.
- Keep thickness and camera response explicit. A 4-game-pixel card exposes only
  about 0.7 native pixels at a 10-degree calibrated view. Amplified view response
  is an artistic camera assumption; it must not be presented as measured head
  tracking or achieved by reversing the light. The experimental default is
  100% (twice the view slope); 50% remains the calibrated reference. Card
  thickness stays 4 pixels in both modes.

## Acceptance

Compare full-volume projected boundaries to independent room-space light rays,
including holes, opposing rotations, zero thickness/gap, and grazing/back light.
Check room-light reconstruction during calibrated roll and at arbitrary neutral
holds. Verify backdrop illumination using independent world-normal dot products
and ray/receiver intersections. Keep source RGBA unchanged, shadows translucent
once, all values finite, and flat output exact. Exercise the actual browser at
small angles, compare before/after crops, and measure the cached shading cost.
Hardware sensor feel remains subject to physical phone testing.

No new rendering library, sensor permission, emulation feature, or hosting
configuration is required.

## Validation result

- 142 browser-helper tests pass, including the lighting and full-solid shadow
  cases. An independent raster check covers 27 mask/angle/thickness combinations
  using ray intersections with opaque unit-pixel prisms. Differences are
  confined to antialiased boundaries; no tested full-volume region missing
  from the rear-cap-only shadow was lost.
- Actual Chromium at a 390×844 viewport and DPR 2 preserves printed-source
  hashes, ROM/save state, and tap-only permission behavior through reset.
  Calibrated roll at two initial holds keeps the reconstructed room light fixed.
  A 15-degree side peek exposes about 2 CSS pixels at strength 50 versus 3.5 at
  strength 100, while retaining 4 game pixels of material thickness.
- Receiver lighting is cached; the scalar lighting sampler validates its frame
  setup once and allocates no per-pixel objects. Warm browser refreshes take
  about 1.7–2.1 ms, including the output image write; unchanged cached paints
  take about 0.2–0.3 ms. Published-prefix browser checks match the local results.
  Extremely grazing/back-facing
  illumination intentionally fades the cast shadow rather than extrapolating
  unbounded geometry. Real-device sensor feel is not established by these tests.

## Follow-up

The later [shared-pose correction](shared-view-light-pose.md) addresses the
camera-only gain mismatch that the calibrated checks above did not cover,
and adds distance falloff to receiver and side lighting.
