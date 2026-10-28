# One rendered pose for the camera and room light

## Broken invariant

The measured phone transform already maps the initial room into current phone
coordinates using the inverse relative orientation. However, Motion strength
scales only the camera offsets, while the lamp uses the unscaled transform.
At the experimental 100% default, two supposedly room-fixed points—the virtual
eye and fixture—therefore change their separation during tilt. The previous
room-lamp-only reconstruction check cannot detect that mismatch.

## Contract and implementation

Keep the raw calibrated pose and sensor lifecycle unchanged. At the presentation
boundary, derive one proper rendered rotation for the existing camera ray:

1. The measured eye direction is the raw rotation applied to (0, 0, -1).
2. The displayed eye direction comes from the existing strength-adjusted view
   slopes, normalized with a negative forward Z.
3. Apply the shortest rotation aligning those directions to the raw matrix.
4. Use that common rendered matrix for the fixture, backboard and side lighting,
   and keep the camera offsets that define exactly the same displayed direction.

Calibrated forward views at strength 50 and the desktop stationary pose are
unchanged. Pure in-plane roll is preserved one-to-one: its view normal does
not change, so there is no extra correction. Near-zero normal differences keep
the exact original matrix. Back-facing sensor input retains the existing safe
fade/fold fallback and is outside the physical forward-view contract. Correction
smoothly tapers to identity over the last 0.1 of the measured forward normal,
so crossing a grazing orientation cannot abruptly jump the lamp.

At non-unity gain this is a coherent amplified virtual box, not a measurement
of physical head tracking. Strength 50 remains the literal calibrated mapping.
Keep 4-game-pixel cards, the 100% experimental default, the full-solid caster,
receiving-board illumination, lamp placement controls, and all unrelated state.

## Receiver lighting

Side and backboard diffuse lighting include inverse-square distance falloff,
normalized against the initial fixture radius. Reference source power does not
change during rotation. Ambient remains constant and diffuse gain is capped
at one. Evaluate distances at actual scene points before camera projection;
printed gameplay faces remain unchanged.

Parallel card caps and the parallel receiving board produce a scale and
translation in box coordinates under a point light. Full-volume boundaries
also include projected side faces. Do not add an arbitrary shear or separately
rotate the board normal: physical screen foreshortening already supplies the
view-dependent retinal skew.

## Acceptance

For arbitrary neutral holds, yaw, pitch, roll and combined forward motions,
at strengths 0, 25, 50, 75 and 100, verify that the displayed camera equals the
common rotation applied to its initial eye point. The same rotation applied
to the initial lamp must preserve the eye–lamp distance and recover both fixed
points under its inverse. Check actual rendered shadow coordinates against
independent room-space rays. Pure roll and strength-50 results must remain
unchanged, as must desktop, source artwork, ROM/save state and permission timing.

Compare actual browser output at small opposing tilts, including the exact
100% default. Release only the reviewed static assets through the existing
recoverable preview path after unit and browser checks pass.

## Verification

All 150 JavaScript helper tests pass. Local and published-prefix browser
results agree for all 14 captures; all served runtime assets match the exact
release candidate. An independent rigid-board calculation
checks that raising either side toward the fixture reverses the near/far
irradiance gradient, with fixed source power. Actual Chromium at 390×844 and
DPR 2 checks seven opposing/combined poses at each of strengths 50 and 100.
Eye–lamp separation remains constant to floating-point tolerance. All 9,122 independently
projected caster samples land within the rendered shadow; clipped outline
bounds differ by at most 1.73 output pixels at antialiased edges. Printed
source hashes remain exact, permission occurs only after Tilt, and reset
preserves the ROM and save state.

These checks establish internal geometric consistency, not the user's reported
on-device directional feel. The strength-50 forward mapping retains its
previous measured direction; this correction specifically removes the
amplified camera/lamp mismatch and adds distance-dependent illumination.
Physical-phone testing remains necessary.
