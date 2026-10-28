import { cameraDepthScale } from "./depth-mesh.js";
import { getSceneLightPosition, getSceneLightRevision, shadowLightProjection } from "./scene-lighting.js";
import { getSceneVanishingPoint, getSceneViewRevision, getSceneViewOffset } from "./scene-view.js";

const MAX_DEPTH = 6;
const MAX_DPR = 2;
const MAX_CANVAS_WIDTH = 1536;

/**
 * Trace every pixel-boundary edge of an alpha silhouette, including holes.
 * Runs are returned in native pixel-boundary coordinates without camera culling.
 */
export function collectCardSilhouetteEdges(sourceRGBA, width = 256, height = 240) {
  validatePixels(sourceRGBA, width, height);
  const opaque = (x, y) => x >= 0 && x < width && y >= 0 && y < height
    && sourceRGBA[(y * width + x) * 4 + 3] > 0;
  const edges = [];

  for (let x = 0; x < width; x += 1) {
    collectVerticalEdges(edges, opaque, x, height, "left", x, -1);
    collectVerticalEdges(edges, opaque, x, height, "right", x + 1, 1);
  }
  for (let y = 0; y < height; y += 1) {
    collectHorizontalEdges(edges, opaque, y, width, height, "top", y, -1);
    collectHorizontalEdges(edges, opaque, y, width, height, "bottom", y + 1, 1);
  }
  return edges;
}

function collectVerticalEdges(edges, opaque, x, height, orientation, edgeX, neighborDelta) {
  let y = 0;
  while (y < height) {
    if (!opaque(x, y) || opaque(x + neighborDelta, y)) {
      y += 1;
      continue;
    }
    const startY = y;
    y += 1;
    while (y < height && opaque(x, y) && !opaque(x + neighborDelta, y)) y += 1;
    edges.push({
      orientation,
      start: { x: edgeX, y: startY },
      end: { x: edgeX, y },
    });
  }
}

function collectHorizontalEdges(edges, opaque, y, width, height, orientation, edgeY, neighborDelta) {
  let x = 0;
  while (x < width) {
    if (!opaque(x, y) || opaque(x, y + neighborDelta)) {
      x += 1;
      continue;
    }
    const startX = x;
    x += 1;
    while (x < width && opaque(x, y) && !opaque(x, y + neighborDelta)) x += 1;
    edges.push({
      orientation,
      start: { x: startX, y: edgeY },
      end: { x, y: edgeY },
    });
  }
}

/** Project one 3D silhouette point through a point-light ray and the shared camera. */
export function projectCardShadowPoint(point, sourceDepth, receiverDepth, width = 256, height = 240) {
  if (!Number.isFinite(point?.x) || !Number.isFinite(point?.y)) {
    throw new TypeError("Point coordinates must be finite numbers");
  }
  validateDimensions(width, height);
  validateDepthPair(sourceDepth, receiverDepth);

  const light = getSceneLightPosition(width, height);
  const lightProjection = shadowLightProjection(sourceDepth, receiverDepth, width, height);
  const clearance = sourceDepth - light.z;
  if (!(clearance > 0)) return null;

  const t = lightProjection.t;
  const receiverPoint = {
    x: point.x + t * (point.x - light.x),
    y: point.y + t * (point.y - light.y),
  };
  const receiverScale = cameraDepthScale(receiverDepth);
  const vanishingPoint = getSceneVanishingPoint(width, height);
  const projected = {
    x: vanishingPoint.x + (receiverPoint.x - vanishingPoint.x) * receiverScale,
    y: vanishingPoint.y + (receiverPoint.y - vanishingPoint.y) * receiverScale,
    opacity: lightProjection.opacity ?? 1,
  };
  if (!Number.isFinite(projected.x) || !Number.isFinite(projected.y)) {
    throw new RangeError("Projected card shadow point must remain finite");
  }
  return projected;
}

/**
 * Render the complete opaque extrusion of one merged sprite/tile silhouette
 * onto its receiver. The output canvas is already in final screen coordinates.
 */
export class CanvasCardShadowRenderer {
  constructor(canvas, width = 256, height = 240) {
    if (!canvas || typeof canvas.getContext !== "function") throw new TypeError("A canvas element is required");
    validateDimensions(width, height);
    this.canvas = canvas;
    this.width = width;
    this.height = height;
    this.context = canvas.getContext("2d", { alpha: true });
    if (!this.context) throw new Error("Canvas 2D context is unavailable");
    this.maskCanvas = makeCanvas(width, height);
    this.maskContext = this.maskCanvas.getContext("2d", { alpha: true });
    if (!this.maskContext) throw new Error("Canvas 2D context is unavailable");
    this.maskImage = this.maskContext.createImageData(width, height);
    this.lastAlpha = new Uint8Array(width * height);
    this.hasAlpha = false;
    this.lastFrontDepth = NaN;
    this.lastBackDepth = NaN;
    this.lastReceiverDepth = NaN;
    this.lastLightingRevision = -1;
    this.lastViewRevision = -1;
    this.lastCanvasWidth = -1;
    this.lastCanvasHeight = -1;
    this.edges = [];
    this.opacity = 0;
  }

  resize(cssWidth, cssHeight, dpr = 1) {
    if (!Number.isFinite(cssWidth) || cssWidth <= 0 || !Number.isFinite(cssHeight) || cssHeight <= 0
      || !Number.isFinite(dpr) || dpr <= 0) {
      throw new RangeError("CSS dimensions and device pixel ratio must be positive finite numbers");
    }
    const scale = Math.min(dpr, MAX_DPR, MAX_CANVAS_WIDTH / cssWidth);
    const width = Math.max(1, Math.floor(cssWidth * scale));
    const height = Math.max(1, Math.floor(cssHeight * scale));
    if (this.canvas.width !== width) this.canvas.width = width;
    if (this.canvas.height !== height) this.canvas.height = height;
    return { width, height };
  }

  /** Paint a merged source alpha silhouette through a physical cardboard slab. */
  paint(sourceCanvas, sourceRGBA, { frontDepth = 0, backDepth, receiverDepth } = {}) {
    validatePixels(sourceRGBA, this.width, this.height);
    if (!sourceCanvas || typeof sourceCanvas.getContext !== "function"
      || sourceCanvas.width !== this.width || sourceCanvas.height !== this.height) {
      throw new RangeError("Source canvas must match the native silhouette dimensions");
    }
    validateDepthVolume(frontDepth, backDepth, receiverDepth);

    const alphaChanged = this.updateSilhouette(sourceRGBA);
    const lightingRevision = getSceneLightRevision();
    const viewRevision = getSceneViewRevision();
    const resized = this.canvas.width !== this.lastCanvasWidth || this.canvas.height !== this.lastCanvasHeight;
    const changed = alphaChanged || frontDepth !== this.lastFrontDepth || backDepth !== this.lastBackDepth
      || receiverDepth !== this.lastReceiverDepth || lightingRevision !== this.lastLightingRevision
      || viewRevision !== this.lastViewRevision || resized;
    if (!changed) return false;

    const frontProjection = shadowLightProjection(frontDepth, receiverDepth, this.width, this.height);
    const light = getSceneLightPosition(this.width, this.height);
    const hasForwardLight = light.z < frontDepth && receiverDepth > frontDepth;
    this.opacity = hasForwardLight ? (frontProjection.opacity ?? 1) : 0;
    const context = this.context;
    const scaleX = this.canvas.width / this.width;
    const scaleY = this.canvas.height / this.height;
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, this.canvas.width, this.canvas.height);

    if (this.opacity > 0 && this.edges.length > 0) {
      context.setTransform(scaleX, 0, 0, scaleY, 0, 0);
      context.imageSmoothingEnabled = true;
      paintCap(context, this.maskCanvas, frontDepth, receiverDepth, this.width, this.height);
      if (backDepth > frontDepth) {
        paintCap(context, this.maskCanvas, backDepth, receiverDepth, this.width, this.height);
        paintSideWalls(context, this.edges, frontDepth, backDepth, receiverDepth, this.width, this.height);
      }
    }

    this.lastFrontDepth = frontDepth;
    this.lastBackDepth = backDepth;
    this.lastReceiverDepth = receiverDepth;
    this.lastLightingRevision = lightingRevision;
    this.lastViewRevision = viewRevision;
    this.lastCanvasWidth = this.canvas.width;
    this.lastCanvasHeight = this.canvas.height;
    return true;
  }

  updateSilhouette(sourceRGBA) {
    let changed = !this.hasAlpha;
    const data = this.maskImage.data;
    let alphaIndex = 0;
    for (let offset = 0; offset < sourceRGBA.length; offset += 4) {
      const alpha = sourceRGBA[offset + 3] > 0 ? 255 : 0;
      if (!changed && this.lastAlpha[alphaIndex] !== alpha) changed = true;
      this.lastAlpha[alphaIndex] = alpha;
      const pixelOffset = alphaIndex * 4;
      data[pixelOffset] = 255;
      data[pixelOffset + 1] = 255;
      data[pixelOffset + 2] = 255;
      data[pixelOffset + 3] = alpha;
      alphaIndex += 1;
    }
    if (changed) {
      this.maskContext.putImageData(this.maskImage, 0, 0);
      this.edges = collectCardSilhouetteEdges(sourceRGBA, this.width, this.height);
      this.hasAlpha = true;
    }
    return changed;
  }
}

function paintCap(context, maskCanvas, sourceDepth, receiverDepth, width, height) {
  const projection = capTransform(sourceDepth, receiverDepth, width, height);
  context.save();
  context.globalCompositeOperation = "source-over";
  context.translate(width / 2 + projection.x, height / 2 + projection.y);
  context.scale(projection.scale, projection.scale);
  context.translate(-width / 2, -height / 2);
  context.drawImage(maskCanvas, 0, 0, width, height);
  context.restore();
}

function capTransform(sourceDepth, receiverDepth, width, height) {
  const lightProjection = shadowLightProjection(sourceDepth, receiverDepth, width, height);
  const cameraScale = cameraDepthScale(receiverDepth);
  const viewOffset = getSceneViewOffset(width, height);
  return {
    scale: cameraScale * lightProjection.magnification,
    x: cameraScale * lightProjection.x + viewOffset.x * (1 - cameraScale),
    y: cameraScale * lightProjection.y + viewOffset.y * (1 - cameraScale),
  };
}

function paintSideWalls(context, edges, frontDepth, backDepth, receiverDepth, width, height) {
  const sides = edges.map(edge => ({
    edge,
    projected: [
      projectCardShadowPoint(edge.start, frontDepth, receiverDepth, width, height),
      projectCardShadowPoint(edge.end, frontDepth, receiverDepth, width, height),
      projectCardShadowPoint(edge.end, backDepth, receiverDepth, width, height),
      projectCardShadowPoint(edge.start, backDepth, receiverDepth, width, height),
    ],
  }));
  context.fillStyle = "#fff";
  for (const side of sides) {
    if (side.projected.some(point => point === null || point.opacity === 0)) continue;
    context.beginPath();
    context.moveTo(side.projected[0].x, side.projected[0].y);
    for (let index = 1; index < side.projected.length; index += 1) {
      context.lineTo(side.projected[index].x, side.projected[index].y);
    }
    context.closePath();
    context.fill();
  }
}

function validateDepthVolume(frontDepth, backDepth, receiverDepth) {
  if (!Number.isFinite(frontDepth) || frontDepth < 0 || frontDepth > MAX_DEPTH
    || !Number.isFinite(backDepth) || backDepth < frontDepth || backDepth > MAX_DEPTH
    || !Number.isFinite(receiverDepth) || receiverDepth < backDepth || receiverDepth > MAX_DEPTH) {
    throw new RangeError("Card shadow depths must satisfy 0 ≤ front ≤ back ≤ receiver ≤ 6");
  }
}

function validateDepthPair(sourceDepth, receiverDepth) {
  if (!Number.isFinite(sourceDepth) || sourceDepth < 0 || sourceDepth > MAX_DEPTH
    || !Number.isFinite(receiverDepth) || receiverDepth < sourceDepth || receiverDepth > MAX_DEPTH) {
    throw new RangeError("Shadow depths must satisfy 0 ≤ source ≤ receiver ≤ 6");
  }
}

function validatePixels(sourceRGBA, width, height) {
  if (!(sourceRGBA instanceof Uint8Array || sourceRGBA instanceof Uint8ClampedArray)) {
    throw new TypeError("Source must be a Uint8Array or Uint8ClampedArray");
  }
  validateDimensions(width, height);
  if (sourceRGBA.length !== width * height * 4) throw new RangeError("Source must match the RGBA dimensions");
}

function validateDimensions(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Dimensions must be positive integers");
  }
}

function makeCanvas(width, height) {
  const canvas = typeof OffscreenCanvas === "function"
    ? new OffscreenCanvas(width, height)
    : globalThis.document?.createElement?.("canvas");
  if (!canvas) throw new Error("Canvas creation is unavailable");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}
