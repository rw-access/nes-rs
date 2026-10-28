# Unified cardboard scene

**Status: design only.** This plan does not change the renderer or declare the new scene implemented. The user asked whether the effect should use a real 3D model and light. Recommendation: prototype a WebGL scene with actual alpha-cutout card meshes, a depth buffer, and one shared shadow map. Use a very thin stack of the three existing foreground priority planes; this gives real, predictable inter-layer contacts and shadows without guessing object depths. It trades exact all-angle pixel placement for small, physically caused parallax.

## Goals and constraints

- Use one camera, one room-relative light, explicit mesh geometry, and depth testing for scene surfaces, shadows, and the rim.
- Preserve the four existing Wasm planes and their NES priority: backdrop, rear sprites, tiles, and front sprites. Do not infer objects, heights, or collision shapes from pixels, and do not use machine learning.
- Keep the native 256×240 opening and source colors exact wherever the selected model promises pixel identity. Keep the rim as an opening around the game, not a cabinet or box that can cover it.
- Express model X/Y/Z, card thickness, and backdrop separation in native game pixels. Keep physical viewing calibration (centimeters and estimated screen width) separate from model dimensions, and illumination controls separate from both.
- Use the selected calibrated phone tracking mode: full relative orientation for sideways peeking, or gravity-only for ignoring room turns. Do not estimate phone translation. With Tilt off, use the shared stationary perspective pose described in visual-depth-and-spatial-audio.md. Request motion access only through the existing Tilt gesture.
- Preserve a 2D fallback if WebGL2 is unavailable or its context is lost. This is a display enhancement; it must not block emulation, input, ROM loading, or audio.

## Renderer choice

| Option | What it provides | Why it does or does not fit |
| --- | --- | --- |
| Incremental Canvas2D | Existing pixel upload, source-last composition, and fast fallback | It can improve face drawing, but still has no scene depth buffer or shared shadow pass. Occlusion and shadows remain separate 2D approximations. Keep it as the fallback. |
| Direct WebGL2 | Real triangles, depth testing, alpha-tested textures, and shadow-map control with no new library | Viable, but this project would own camera matrices, mesh buffers, shader variants, framebuffers, shadow filtering, and context recovery. That is a sizeable rendering subsystem to maintain. |
| Three.js over WebGL2 | Camera, mesh, depth, material, and shadow-map management; custom geometry and materials remain possible | Recommended for the prototype. It still needs project-specific cutout boundary geometry and render-order rules, but avoids building a general GL engine. Vendor a pinned ESM module locally; do not use a runtime CDN. Keep Three objects behind one application adapter. |

If adding a local library is unacceptable, use direct WebGL2 behind the same adapter. Do not expand Canvas2D and call the result a unified physical scene.

## Scene and units

Use screen-plane coordinates with positive Z extending away from the viewer. The front-sprite sheet is the `z=0` reference plane. Use one model unit per native game pixel for X/Y/Z, sheet thickness, and backdrop separation. Convert physical eye distance into these model units using estimated visible game width only at the camera boundary; changing a screen profile must not rebuild or resize the model. Stack the tile sheet one sheet thickness behind the front-sprite sheet and the rear-sprite sheet one more thickness behind it. Keep the backdrop gap independent of sheet thickness and clamp the stack before it reaches the backdrop. This order comes from the three existing PPU priority planes, not image analysis.

The phone view consumes the selected tracking mode in the calibrated screen axes. Full relative orientation preserves sideways peeking; gravity-only ignores room turns but loses that axis at upright holds. Neither estimates phone translation. Camera, light, mesh transforms, rim, and shadow pass consume one immutable `ScenePose` revision so they cannot drift apart. Stationary 2.5D uses the shared stationary camera/lamp pose described in [the implemented presentation contract](visual-depth-and-spatial-audio.md). Flat mode remains neutral. The current Canvas implementation and phone-frame lamp contract are recorded in [the implemented presentation contract](visual-depth-and-spatial-audio.md).

Keep the screen opening as the clipping boundary. Render the backdrop as an overscanned plane that covers the full view frustum at the supported tilt range; extend its edge color or fill the remainder from the scene background. Do not add side faces at the overscan boundary. There is no surrounding enclosure mesh.

The existing four RGBA planes remain the input contract. `renderVideo()` should hand their views and one scene description to a `CardboardSceneRenderer`; it should not separately scale planes or place CSS shadow canvases:

```text
CardboardSceneRenderer.renderFrame({
  planes: { backdrop, rearSprites, tiles, frontSprites },
  scene: { gameWidthCm, backdropGap, cardThicknessPixels, view, light, rim, reliefMode }
})
```

The adapter owns texture uploads, alpha-boundary geometry, mesh caches, scene rendering, and the native-pixel composition pass. It reads the existing app state; the Wasm buffer format does not change.

## Cutout geometry and light

Foreground textures are alpha-tested surfaces, not opaque rectangles. Build a closed cutout volume from each selected alpha shape: the front uses the exact RGBA texture; the rear uses its alpha coverage and corresponding source colors; both discard transparent pixels, while every exposed outer and inner-hole boundary contributes a side quad. The shadow pass uses the same alpha test and side geometry. Generate all boundaries independently of camera direction; let the GPU cull back faces and resolve occlusion. Merge contiguous edge runs of the same source boundary RGB for manageable geometry. Split runs at source color changes, including opaque black outlines, and apply shared lighting to those inherited colors.

Keep unshadowed front artwork nearest-sampled with exact base colors, without exposure or tone-mapping changes. A shadow-receiving front material must explicitly combine the exact printed color with the shared geometric shadow term; an unlit material alone does not receive light shadows. Side faces receive a restrained diffuse response from the one point light and a small ambient term. The light uses the same calibrated pose and model-space units as the camera. Use the library point-light shadow pass with verified coverage as the fixture moves; do not assume one perspective shadow camera can cover every relevant direction. Use a small shadow map with tightly controlled, slope-aware bias and a modest PCF radius. Do not move the source to hide acne or detach contact shadows. The softness control adjusts filtering, not the geometry.

The rim becomes a rounded annular mesh with front lip and reveal walls. It uses the same outline radius, camera, edge material, light, and shadow pass as the cards. The reveal ends at the backdrop plane and only covers pixels inside the opening that the existing rim is meant to occupy. Its shadow receives on the backdrop; it cannot paint over foreground gameplay.

## Unavoidable layer tradeoff

Flush, coplanar sprite and tile faces cannot cast physical shadows onto one another: their surfaces have zero separation. A depth test cannot make a rear sprite's thickness appear over an opaque tile that is physically in front. The current forced-cutout/overlap-relief behavior is a deliberate Penrose-style optical overlay, not a result a physically ordered mesh can reproduce without actual Z separation.

Likewise, a finite-Z stack cannot preserve the complete composited source image at identical screen coordinates under every oblique camera pose. A zero-depth reference face can remain fixed, but lower priority planes at nonzero depth must move and scale with parallax. Priority ordering alone does not provide real object depth.

The recommended priority stack is not a claim that each sprite has been reconstructed as an object. It treats each existing PPU priority plane as one thin printed sheet. Calibrate the front sheet at `z=0` against the native opening and keep the same physical width for the sheets behind it. Under tilt, lower sheets show small parallax; even at neutral view, perspective causes a small scale difference for the rear sheets. Record both effects rather than silently stretching sheets or altering their thickness.

An alternative **flush mode** composites the three foreground planes using NES priority into one exact alpha-cutout card at `z=0`, then extrudes its visible silhouette. It preserves foreground source pixels at every camera pose and gives real lighting plus backdrop/rim shadows, but coplanar sprite and tile faces cannot cast physical shadows onto one another. Keep the existing Layer relief modes only as explicitly labeled, opt-in 2D optical overlays in that mode. A forced overlap cutout is a Penrose-style visual effect, not physical depth. A depth buffer cannot provide both strict coplanar pixel identity and physical inter-layer shadows; choose the stated priority-stack default when physical cardboard is the goal, and use flush mode when exact pixels take precedence. Do not infer per-sprite height to evade this tradeoff.

## Fallback and migration

Keep the current Canvas2D renderer operational behind the adapter. If WebGL2 context creation, shader setup, or context restoration fails, show the static Canvas2D scene and keep the existing emulator controls active. Motion permission remains user-triggered; denied or unavailable sensors select the neutral desktop-style view. Flat mode and frames without depth data continue to use the original RGBA path.

Build the WebGL prototype beside the existing renderer and compare it on the same Wasm planes before switching the app over. Cache textures and mesh boundaries by alpha-mask revision; update color textures independently. Track camera, light, material, and rim revisions separately. Rollback is restoring the Canvas2D adapter as the active renderer; no ROM, PPU, or Wasm format migration is required.

## Acceptance criteria

- A deterministic rectangle, ring-with-hole, and irregular alpha mask produce closed meshes with correct winding and no cap or boundary leaks. Compare projected vertices against an independent ray/plane oracle.
- In priority-stack mode, source arrays remain unchanged; reference-view sheet registration and any scale delta are measured, while nonzero-view parallax and occlusion match independent plane intersections. Flush mode, if retained, must match the exact priority-composed source bytes at every pose.
- Test all four source planes, transparent pixels, opaque black pixels, equal-depth priority ties, cutout holes, tile/sprite overlap, and known priority transitions. No object or height inference is permitted.
- For a known light, compare mesh shadow boundaries against analytic point-light/receiver intersections. Test self-shadow acne and contact gaps at 0, 2, and 8 game-pixel material thickness; no shadow is emitted when the light is behind the relevant surface.
- The oversized backdrop covers the entire stage frustum at the supported extreme tilt without exposing a rectangular card edge or adding box-like occlusion. The rounded rim shares the game opening outline and never covers native gameplay pixels.
- Browser pixel tests cover neutral, pitch, roll, combined gravity tilts, portrait/landscape, DPR 1/2, shadow softness, layer mode, flat mode, missing depth, and WebGL2 context fallback/loss. The desktop default remains static and no sensor permission is requested on load.
- Simulated browser poses and geometry oracles do not establish real-device sensor calibration or GPU performance. A physical phone test is still required before calling the physical-camera feel or WebGL performance verified. The scene is a lit cutout-card approximation; it does not reconstruct SMB object geometry or promise photorealism.

## Settings boundary

The current settings use three groups: Scene geometry (game-pixel dimensions and
explicit ratios), Viewing setup (estimated physical centimeters, screen profile,
and motion gain), and Lighting (fixture distance and softness). Preserve this
boundary in the unified scene; avoid a camera calibration slider changing card
thickness. Future light positions should also be expressed in model-space
units inside Lighting, rather than borrowing the eye-distance parameter.

Primary implementation references: [Three WebGLRenderer](https://threejs.org/docs/pages/WebGLRenderer.html)
and [PointLightShadow](https://threejs.org/docs/pages/PointLightShadow.html).
No library version has been selected or installed by this design.
