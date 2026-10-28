import { cameraDepthScale } from "./depth-mesh.js";
import { createBackdropIrradianceSampler, getSceneLightPosition, getSceneLightRevision } from "./scene-lighting.js";
import { getSceneCameraDepth, getSceneVanishingPoint, getSceneViewRevision } from "./scene-view.js";

const MAX_DEPTH = 6;

/** Invert the shared camera projection to find the receiving-plane point below a screen pixel. */
export function projectBackdropReceiverPoint(screenX, screenY, sceneDepth, width = 256, height = 240) {
  validateDimensions(width, height);
  validateDepth(sceneDepth);
  if (!Number.isFinite(screenX) || !Number.isFinite(screenY)) {
    throw new TypeError("Screen coordinates must be finite numbers");
  }

  const vanishingPoint = getSceneVanishingPoint(width, height);
  const scale = cameraDepthScale(sceneDepth);
  return {
    x: vanishingPoint.x + (screenX - vanishingPoint.x) / scale,
    y: vanishingPoint.y + (screenY - vanishingPoint.y) / scale,
    z: sceneDepth,
  };
}

/**
 * Shade the backdrop image at native resolution while keeping its RGBA source intact.
 * Irradiance is sampled at the scene-space receiver point under each visible screen pixel.
 */
export class CanvasBackdropRenderer {
  constructor(canvas, width = 256, height = 240) {
    if (!canvas || typeof canvas.getContext !== "function") throw new TypeError("A canvas element is required");
    validateDimensions(width, height);
    this.canvas = canvas;
    this.width = width;
    this.height = height;
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    this.context = canvas.getContext("2d", { alpha: true });
    if (!this.context) throw new Error("Canvas 2D context is unavailable");

    this.image = this.context.createImageData(width, height);
    this.sourceRGBA = new Uint8ClampedArray(width * height * 4);
    this.irradiance = new Float32Array(width * height);
    this.hasSource = false;
    this.lastDepth = NaN;
    this.lastLightRevision = -1;
    this.lastViewRevision = -1;
    this.lastCanvasWidth = canvas.width;
    this.lastCanvasHeight = canvas.height;
  }

  /** The shaded native RGBA image, suitable for downstream backdrop receivers. */
  get outputRGBA() {
    return this.image.data;
  }

  /** Shade and draw the source image. Returns false when the pixels and scene are unchanged. */
  paint(sourceRGBA, sceneDepth) {
    validatePixels(sourceRGBA, this.width, this.height);
    validateDepth(sceneDepth);

    const resized = this.canvas.width !== this.lastCanvasWidth || this.canvas.height !== this.lastCanvasHeight;
    if (resized) {
      this.canvas.width = this.width;
      this.canvas.height = this.height;
      this.lastCanvasWidth = this.canvas.width;
      this.lastCanvasHeight = this.canvas.height;
    }

    const lightRevision = getSceneLightRevision();
    const viewRevision = getSceneViewRevision();
    const changedSource = !this.hasSource || !equalBytes(sourceRGBA, this.sourceRGBA);
    const changedIrradiance = sceneDepth !== this.lastDepth
      || lightRevision !== this.lastLightRevision
      || viewRevision !== this.lastViewRevision;
    if (!changedSource && !changedIrradiance && !resized) return false;

    if (changedSource) {
      this.sourceRGBA.set(sourceRGBA);
      this.hasSource = true;
    }
    if (changedIrradiance) this.updateIrradiance(sceneDepth);

    const source = this.sourceRGBA;
    const output = this.image.data;
    for (let pixel = 0, offset = 0; pixel < this.irradiance.length; pixel += 1, offset += 4) {
      const amount = this.irradiance[pixel];
      output[offset] = Math.round(source[offset] * amount);
      output[offset + 1] = Math.round(source[offset + 1] * amount);
      output[offset + 2] = Math.round(source[offset + 2] * amount);
      output[offset + 3] = source[offset + 3];
    }
    this.context.putImageData(this.image, 0, 0);

    this.lastDepth = sceneDepth;
    this.lastLightRevision = lightRevision;
    this.lastViewRevision = viewRevision;
    return true;
  }

  updateIrradiance(sceneDepth) {
    const vanishingPoint = getSceneVanishingPoint(this.width, this.height);
    const inverseScale = 1 / cameraDepthScale(sceneDepth);
    const light = getSceneLightPosition(this.width, this.height);
    const sample = createBackdropIrradianceSampler(light, sceneDepth, this.width, this.height);
    for (let y = 0, pixel = 0; y < this.height; y += 1) {
      const receiverY = vanishingPoint.y + (y + 0.5 - vanishingPoint.y) * inverseScale;
      for (let x = 0; x < this.width; x += 1, pixel += 1) {
        const receiverX = vanishingPoint.x + (x + 0.5 - vanishingPoint.x) * inverseScale;
        this.irradiance[pixel] = sample(receiverX, receiverY);
      }
    }
  }
}

function validatePixels(sourceRGBA, width, height) {
  if (!(sourceRGBA instanceof Uint8Array || sourceRGBA instanceof Uint8ClampedArray)) {
    throw new TypeError("Source must be a Uint8Array or Uint8ClampedArray");
  }
  if (sourceRGBA.length !== width * height * 4) throw new RangeError("Source must match the RGBA dimensions");
}

function validateDimensions(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Width and height must be positive integers");
  }
}

function validateDepth(depth) {
  if (!Number.isFinite(depth) || depth < 0 || depth > MAX_DEPTH) {
    throw new RangeError(`Backdrop depth must be a number from 0 to ${MAX_DEPTH}`);
  }
}

function equalBytes(left, right) {
  if (left.length !== right.length) return false;
  if ((left.byteOffset & 3) === 0 && (right.byteOffset & 3) === 0 && (left.length & 3) === 0) {
    const leftPixels = new Uint32Array(left.buffer, left.byteOffset, left.length / 4);
    const rightPixels = new Uint32Array(right.buffer, right.byteOffset, right.length / 4);
    for (let index = 0; index < leftPixels.length; index += 1) {
      if (leftPixels[index] !== rightPixels[index]) return false;
    }
    return true;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) return false;
  }
  return true;
}
