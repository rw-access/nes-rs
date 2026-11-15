import { CanvasBackdropRenderer } from "./backdrop-lighting.js";
import { CanvasCardShadowRenderer } from "./card-shadow.js";
import { CanvasDepthRenderer } from "./depth-mesh.js";
import { DioramaFrame } from "./diorama-frame.js";
import { RimHousing } from "./rim-housing.js";
import { resolveSceneDepth, resolveShadowSoftness, resolveRimRecess, resolveRimThickness, gameplayShadowProjection } from "./depth-model.js";
import {
  getSceneLightPlacement, setSceneLightPlacement, getSceneLightDistance, setSceneLightDistance,
  getSceneLightRotation, setSceneLightRotation, getSceneShadingStrength, setSceneShadingStrength,
  sceneShadowOpacity,
} from "./scene-lighting.js";
import { getSceneView, getSceneCameraDepth, setSceneCamera } from "./scene-view.js";
import { getMaterialFinish, setMaterialFinish } from "./material-finish.js";

const WIDTH = 256, HEIGHT = 240, PLANE_BYTES = WIDTH * HEIGHT * 4;

/** Freeze one camera/light pose so lazily rendered cards share the same view. */
export function capturePresetView() {
  return Object.freeze({
    view: getSceneView(), focalDepth: getSceneCameraDepth(), rotation: getSceneLightRotation(),
    gameWidthCm: getSceneLightPlacement()?.gameWidthCm ?? 24,
  });
}

/**
 * Render a frozen four-plane NES frame using a candidate set of control values.
 * An optional captured view fixes camera, room rotation and physical game width
 * across lazy cards while preserving the live pose active at each invocation.
 * This call is synchronous: candidate state never survives a return or throw.
 * Revision counters remain monotonic, invalidating the live renderer's caches
 * after the original values are restored. No controls or emulator are touched.
 */
export function renderPresetPreview(canvas, planes, values = {}, viewContext = null) {
  if (!canvas || typeof canvas.getContext !== "function") throw new TypeError("A preview canvas is required");
  if (!(planes instanceof Uint8Array || planes instanceof Uint8ClampedArray) || planes.length !== PLANE_BYTES * 4) {
    throw new RangeError("Preview pixels must contain four native RGBA planes");
  }
  const context = canvas.getContext("2d", { alpha: true });
  if (!context) throw new Error("Canvas 2D context is unavailable");
  const snapshot = {
    placement: getSceneLightPlacement(), distance: getSceneLightDistance(), rotation: getSceneLightRotation(),
    view: getSceneView(), focalDepth: getSceneCameraDepth(), shading: getSceneShadingStrength(), finish: getMaterialFinish(),
  };
  const number = (id, fallback) => Number(values[id] ?? fallback);
  const checked = (id, fallback) => {
    const value = values[id] ?? fallback;
    return value === true || value === "true";
  };
  try {
    if (viewContext) {
      setSceneCamera(viewContext.view.x, viewContext.view.y, viewContext.focalDepth);
      setSceneLightRotation(viewContext.rotation);
    }
    setSceneLightPlacement({
      distanceCm: number("light-distance-input", 85),
      elevationDegrees: number("light-elevation-input", 3),
      azimuthDegrees: number("light-azimuth-input", 0),
      gameWidthCm: viewContext?.gameWidthCm ?? snapshot.placement?.gameWidthCm ?? 24,
    });
    setSceneShadingStrength(number("shading-strength-input", 35) / 100);
    setMaterialFinish(values["material-finish-input"] ?? "matte");

    const scene = resolveSceneDepth(number("depth-input", 100), number("thickness-input", 5), checked("solid-sides-input", true));
    canvas.width = WIDTH; canvas.height = HEIGHT;
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, WIDTH, HEIGHT);
    context.imageSmoothingEnabled = false;
    const native = makeCanvas(canvas);
    const nativeContext = native.getContext("2d", { alpha: true });
    if (!nativeContext) throw new Error("Canvas 2D context is unavailable");
    const image = nativeContext.createImageData(WIDTH, HEIGHT);
    const pixels = index => planes.subarray(index * PLANE_BYTES, (index + 1) * PLANE_BYTES);
    if (scene.separation === 0) {
      for (let index = 0; index < 4; index += 1) {
        image.data.set(pixels(index));
        nativeContext.putImageData(image, 0, 0);
        context.drawImage(native, 0, 0);
      }
      return canvas;
    }

    const backdropCanvas = makeCanvas(canvas);
    const backdrop = new CanvasBackdropRenderer(backdropCanvas, WIDTH, HEIGHT);
    backdrop.paint(pixels(0), scene.separation);
    context.drawImage(backdropCanvas, 0, 0);

    const rimEnabled = (checked("rim-input", false) || checked("recessed-opening-input", false)) && number("rim-width-input", 1) > 0;
    const rimFront = rimEnabled ? resolveRimRecess(scene, number("rim-recess-input", 1)) : 0;
    const rimThickness = rimEnabled ? resolveRimThickness(scene, number("rim-thickness-input", 25), rimFront) : 0;
    if (rimEnabled && checked("diorama-input", true)) {
      const rimShadowCanvas = makeCanvas(canvas);
      const rimShadow = new DioramaFrame(rimShadowCanvas, WIDTH, HEIGHT);
      rimShadow.resize(WIDTH, HEIGHT, 1);
      rimShadow.paint(backdrop.outputRGBA, scene.separation, resolveShadowSoftness(number("rim-softness-input", 100)), rimThickness, rimFront);
      context.drawImage(rimShadowCanvas, 0, 0);
    }

    // Match the live merged extrusion caster and CSS filter in screen space.
    image.data.fill(0);
    for (let offset = 3; offset < PLANE_BYTES; offset += 4) {
      image.data[offset] = planes[PLANE_BYTES + offset] || planes[PLANE_BYTES * 2 + offset]
        || planes[PLANE_BYTES * 3 + offset] ? 255 : 0;
    }
    nativeContext.putImageData(image, 0, 0);
    const shadowCanvas = makeCanvas(canvas);
    const shadow = new CanvasCardShadowRenderer(shadowCanvas, WIDTH, HEIGHT);
    shadow.resize(WIDTH, HEIGHT, 1);
    shadow.paint(native, image.data, { frontDepth: 0, backDepth: scene.thickness, receiverDepth: scene.separation });
    const softness = resolveShadowSoftness(number("softness-input", 9));
    const projection = gameplayShadowProjection(scene, WIDTH, HEIGHT);
    context.save();
    context.filter = `brightness(0) opacity(${sceneShadowOpacity(0.32)}) blur(${projection.blur * softness}px)`;
    context.globalAlpha = shadow.opacity;
    context.drawImage(shadowCanvas, 0, 0);
    context.restore();

    if (rimEnabled) {
      const rimCanvas = makeCanvas(canvas);
      const rim = new RimHousing(rimCanvas, { clientWidth: WIDTH }, WIDTH, HEIGHT);
      const material = !checked("rim-input", false) || checked("console-edge-input", false) ? "console" : "charcoal";
      // Three native pixels correspond to the live opening's padding at 1x.
      const faceWidth = 3 * number("rim-width-input", 1) / 100;
      rim.paint(scene.separation, rimThickness, 1, rimFront, faceWidth, material, "#111217");
      if (!rimCanvas.hidden) context.drawImage(rimCanvas, 0, 0);
    }

    const relief = checked("sprite-relief-input", true) && checked("solid-sides-input", true)
      ? { mode: values["sprite-relief-style"] ?? "contact", softness, overlapFraction: checked("thin-overlaps-input", true) ? 1 / 3 : 1 }
      : null;
    for (let index = 1; index < 4; index += 1) {
      const layerCanvas = makeCanvas(canvas);
      const renderer = new CanvasDepthRenderer(layerCanvas, WIDTH, HEIGHT);
      renderer.resize(WIDTH, HEIGHT, 1);
      const layerRelief = index === 3 ? relief : index === 2 && relief ? { ...relief, receiverRGBA: pixels(1) } : null;
      renderer.paint(pixels(index), scene.thickness, index === 3 ? pixels(2) : null, undefined, layerRelief);
      context.drawImage(layerCanvas, 0, 0);
    }
    return canvas;
  } finally {
    setSceneLightPlacement(snapshot.placement);
    setSceneLightDistance(snapshot.distance);
    setSceneLightRotation(snapshot.rotation);
    setSceneCamera(snapshot.view.x, snapshot.view.y, snapshot.focalDepth);
    setSceneShadingStrength(snapshot.shading);
    setMaterialFinish(snapshot.finish);
  }
}

function makeCanvas(reference) {
  const canvas = typeof OffscreenCanvas === "function" ? new OffscreenCanvas(WIDTH, HEIGHT)
    : (reference.ownerDocument ?? document).createElement("canvas");
  canvas.width = WIDTH; canvas.height = HEIGHT;
  return canvas;
}
