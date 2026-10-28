import { calculateRimProjection } from "./rim-projection.js";
import { RIM_CORNER_FRACTION, traceRoundedRectangle } from "./rim-outline.js";
import { MAX_DEPTH } from "./depth-mesh.js";
import { getSceneLightRevision } from "./scene-lighting.js";
import { getSceneViewRevision } from "./scene-view.js";

const MAX_DPR = 2;
const MAX_CANVAS_WIDTH = 1536;
export const FAR_SHADOW_OPACITY = 0.18;
export const NEAR_SHADOW_OPACITY = 0.12;
const BLUR_PADDING_FACTOR = 3;

/** Shared perspective projection for the backdrop and its shadow receiver. */
export function calculateDioramaProjection(width, height, gap, receiverDepth = gap, thickness = gap / 2, frontDepth = 0) {
  return calculateRimProjection(width, height, gap, receiverDepth, thickness, frontDepth);
}

/** Paint the full infinite backdrop and translucent projected TV-aperture shadow. */
export class DioramaFrame {
  constructor(canvas, width = 256, height = 240) {
    if (!canvas || typeof canvas.getContext !== "function") throw new TypeError("A canvas element is required");
    validateDimensions(width, height);
    this.canvas = canvas;
    this.width = width;
    this.height = height;
    this.context = canvas.getContext("2d", { alpha: true });
    if (!this.context) throw new Error("Canvas 2D context is unavailable");
    this.backdropCanvas = makeCanvas(canvas, width, height);
    this.backdropContext = this.backdropCanvas.getContext("2d", { alpha: true });
    if (!this.backdropContext) throw new Error("Canvas 2D context is unavailable");
    this.backdropImage = this.backdropContext.createImageData(width, height);
    this.shadowLayers = { far: null, near: null };
    this.shadowMaskCanvas = null;
    this.shadowMaskContext = null;
    this.shadowCanvas = null;
    this.shadowContext = null;
    this.hasBackdrop = false;
    this.lastDepth = NaN;
    this.lastSoftness = NaN;
    this.lastThickness = NaN;
    this.lastFrontDepth = NaN;
    this.lastLightingRevision = -1;
    this.lastViewRevision = -1;
    this.lastCanvasWidth = -1;
    this.lastCanvasHeight = -1;
  }

  resize(cssWidth, cssHeight, dpr = 1) {
    if (!Number.isFinite(cssWidth) || cssWidth <= 0 || !Number.isFinite(cssHeight) || cssHeight <= 0
      || !Number.isFinite(dpr) || dpr <= 0) {
      throw new RangeError("CSS dimensions and device pixel ratio must be positive finite numbers");
    }
    const scale = Math.min(dpr, MAX_DPR, MAX_CANVAS_WIDTH / cssWidth);
    const pixelWidth = Math.max(1, Math.floor(cssWidth * scale));
    const pixelHeight = Math.max(1, Math.floor(cssHeight * scale));
    if (this.canvas.width !== pixelWidth) this.canvas.width = pixelWidth;
    if (this.canvas.height !== pixelHeight) this.canvas.height = pixelHeight;
    if (this.lastCanvasWidth !== pixelWidth || this.lastCanvasHeight !== pixelHeight) {
      for (const layer of Object.values(this.shadowLayers)) if (layer) layer.key = null;
    }
    return { width: pixelWidth, height: pixelHeight };
  }

  paint(backdropRGBA, depth, softness = 1, thickness = depth / 2, frontDepth = 0) {
    validatePixels(backdropRGBA, this.width, this.height, "Backdrop image");
    validateDepth(depth);
    validateSoftness(softness);
    calculateDioramaProjection(this.width, this.height, depth, depth, thickness, frontDepth);

    const changedBackdrop = !this.hasBackdrop || !equalBytes(backdropRGBA, this.backdropImage.data);
    const lightingRevision = getSceneLightRevision();
    const viewRevision = getSceneViewRevision();
    const resized = this.lastCanvasWidth !== this.canvas.width || this.lastCanvasHeight !== this.canvas.height;
    const changedProjection = depth !== this.lastDepth || softness !== this.lastSoftness
      || thickness !== this.lastThickness || frontDepth !== this.lastFrontDepth
      || lightingRevision !== this.lastLightingRevision || viewRevision !== this.lastViewRevision;
    if (!changedBackdrop && !resized && !changedProjection) return false;

    if (changedBackdrop) {
      // The emulator reuses its frame buffer, so keep the backdrop independent of it.
      this.backdropImage.data.set(backdropRGBA);
      this.backdropContext.putImageData(this.backdropImage, 0, 0);
      this.hasBackdrop = true;
    }
    if (depth > 0) this.updateShadowMask(depth, softness, depth, "far", thickness, frontDepth);

    const context = this.context;
    const scaleX = this.canvas.width / this.width;
    const scaleY = this.canvas.height / this.height;
    context.setTransform(scaleX, 0, 0, scaleY, 0, 0);
    context.clearRect(0, 0, this.width, this.height);
    context.globalAlpha = 1;
    context.globalCompositeOperation = "source-over";
    context.imageSmoothingEnabled = false;
    context.drawImage(this.backdropCanvas, 0, 0, this.width, this.height);
    if (depth > 0) {
      // Shadow alpha was blurred in a padded cache, then limited to the visible viewport.
      context.imageSmoothingEnabled = false;
      context.globalAlpha = FAR_SHADOW_OPACITY * (this.shadowLayers.far?.opacity ?? 1);
      context.drawImage(this.shadowCanvas, 0, 0, this.canvas.width, this.canvas.height, 0, 0, this.width, this.height);
      context.globalAlpha = 1;
    }

    this.lastDepth = depth;
    this.lastSoftness = softness;
    this.lastThickness = thickness;
    this.lastFrontDepth = frontDepth;
    this.lastLightingRevision = lightingRevision;
    this.lastViewRevision = viewRevision;
    this.lastCanvasWidth = this.canvas.width;
    this.lastCanvasHeight = this.canvas.height;
    return true;
  }

  paintForeground(sourceCanvas, targetCanvas, gap, softness = 1, thickness = gap / 2, frontDepth = 0) {
    if (!sourceCanvas || !targetCanvas || typeof targetCanvas.getContext !== "function") {
      throw new TypeError("Source and target canvases are required");
    }
    validateDepth(gap);
    validateSoftness(softness);
    const layer = this.updateShadowMask(gap, softness, 0, "near", thickness, frontDepth);
    const context = targetCanvas.getContext("2d", { alpha: true });
    if (!context) throw new Error("Canvas 2D context is unavailable");
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, targetCanvas.width, targetCanvas.height);
    if (gap > 0) {
      context.save();
      context.globalCompositeOperation = "source-over";
      context.globalAlpha = NEAR_SHADOW_OPACITY * (layer.opacity ?? 1);
      context.imageSmoothingEnabled = false;
      context.drawImage(layer.shadowCanvas, 0, 0, this.canvas.width, this.canvas.height,
        0, 0, targetCanvas.width, targetCanvas.height);
      context.globalAlpha = 1;
      context.globalCompositeOperation = "destination-in";
      context.drawImage(sourceCanvas, 0, 0, targetCanvas.width, targetCanvas.height);
      context.restore();
    }
    return true;
  }

  updateShadowMask(gap, softness, receiverDepth = gap, layerName = "far", thickness = gap / 2, frontDepth = 0) {
    validateSoftness(softness);
    const outputWidth = this.canvas.width;
    const outputHeight = this.canvas.height;
    const scaleX = outputWidth / this.width;
    const scaleY = outputHeight / this.height;
    const projection = calculateDioramaProjection(this.width, this.height, gap, receiverDepth, thickness, frontDepth);
    const blurRadius = softness * projection.blurDistance * Math.max(scaleX, scaleY);
    const padding = Math.ceil(blurRadius * BLUR_PADDING_FACTOR + 1);
    const key = {
      gap, thickness, frontDepth, receiverDepth, softness, outputWidth, outputHeight, padding,
      lightingRevision: getSceneLightRevision(),
      viewRevision: getSceneViewRevision(),
    };
    let layer = this.shadowLayers[layerName];
    if (!layer) {
      layer = makeShadowLayer(this.canvas, outputWidth, outputHeight);
      this.shadowLayers[layerName] = layer;
    }
    layer.opacity = projection.opacity ?? 1;
    if (sameKey(layer.key, key)) return layer;
    const maskWidth = outputWidth + padding * 2;
    const maskHeight = outputHeight + padding * 2;
    layer.maskCanvas.width = maskWidth;
    layer.maskCanvas.height = maskHeight;
    layer.shadowCanvas.width = outputWidth;
    layer.shadowCanvas.height = outputHeight;

    const mask = layer.maskContext;
    mask.setTransform(1, 0, 0, 1, 0, 0);
    mask.clearRect(0, 0, maskWidth, maskHeight);
    mask.fillStyle = "#000";
    if (projection.castsShadow) {
      mask.fillRect(0, 0, maskWidth, maskHeight);
      const lit = projection.litOpening;
      const topLeft = { x: padding + lit.x * scaleX, y: padding + lit.y * scaleY };
      const bottomRight = {
        x: padding + (lit.x + lit.width) * scaleX,
        y: padding + (lit.y + lit.height) * scaleY,
      };
      const rx = this.width * RIM_CORNER_FRACTION * projection.apertureScale * scaleX;
      const ry = this.height * RIM_CORNER_FRACTION * projection.apertureScale * scaleY;
      mask.save();
      mask.globalCompositeOperation = "destination-out";
      mask.beginPath();
      traceRoundedRectangle(mask, topLeft, bottomRight, rx, ry);
      mask.fill();
      mask.restore();
    }

    const shadow = layer.shadowContext;
    shadow.setTransform(1, 0, 0, 1, 0, 0);
    shadow.clearRect(0, 0, outputWidth, outputHeight);
    shadow.save();
    shadow.beginPath();
    shadow.rect(0, 0, outputWidth, outputHeight);
    shadow.clip();
    shadow.filter = blurRadius > 0 ? `blur(${blurRadius}px)` : "none";
    shadow.drawImage(layer.maskCanvas, -padding, -padding, maskWidth, maskHeight);
    shadow.restore();
    shadow.filter = "none";
    layer.key = key;
    if (layerName === "far") {
      this.shadowMaskCanvas = layer.maskCanvas;
      this.shadowMaskContext = layer.maskContext;
      this.shadowCanvas = layer.shadowCanvas;
      this.shadowContext = layer.shadowContext;
    }
    return layer;
  }
}

function makeShadowLayer(referenceCanvas, width, height) {
  const maskCanvas = makeCanvas(referenceCanvas, width, height);
  const maskContext = maskCanvas.getContext("2d", { alpha: true });
  const shadowCanvas = makeCanvas(referenceCanvas, width, height);
  const shadowContext = shadowCanvas.getContext("2d", { alpha: true });
  if (!maskContext || !shadowContext) throw new Error("Canvas 2D context is unavailable");
  return { maskCanvas, maskContext, shadowCanvas, shadowContext, key: null };
}

function makeCanvas(referenceCanvas, width, height) {
  let canvas;
  if (typeof OffscreenCanvas === "function") canvas = new OffscreenCanvas(width, height);
  else {
    const document = referenceCanvas.ownerDocument;
    if (!document?.createElement) throw new Error("Canvas creation is unavailable");
    canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
  }
  return canvas;
}

function validatePixels(pixels, width, height, label) {
  if (!(pixels instanceof Uint8Array || pixels instanceof Uint8ClampedArray)) {
    throw new TypeError(`${label} must be a Uint8Array or Uint8ClampedArray`);
  }
  validateDimensions(width, height);
  if (pixels.length !== width * height * 4) throw new RangeError(`${label} must match the RGBA dimensions`);
}

function validateDimensions(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Width and height must be positive integers");
  }
}

function validateDepth(depth) {
  if (!Number.isFinite(depth) || depth < 0 || depth > MAX_DEPTH) throw new RangeError(`Depth must be a number from 0 to ${MAX_DEPTH}`);
}

function validateSoftness(softness) {
  if (!Number.isFinite(softness) || softness < 0 || softness > 2) throw new RangeError("Softness must be between 0 and 2");
}

function equalBytes(left, right) {
  if (!right || left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) if (left[index] !== right[index]) return false;
  return true;
}

function sameKey(left, right) {
  return left && left.gap === right.gap && left.thickness === right.thickness && left.frontDepth === right.frontDepth
    && left.receiverDepth === right.receiverDepth
    && left.softness === right.softness && left.outputWidth === right.outputWidth
    && left.outputHeight === right.outputHeight && left.padding === right.padding
    && left.lightingRevision === right.lightingRevision && left.viewRevision === right.viewRevision;
}
