import { getSceneLightRevision, shadowLightProjection, surfaceShade } from "./scene-lighting.js";
import { getSceneCameraDepth, getSceneVanishingPoint, getSceneViewOffset, getSceneViewRevision } from "./scene-view.js";

export const MAX_DEPTH = 6;
const MAX_DPR = 2;
const MAX_CANVAS_WIDTH = 1536;
const IDENTITY_PROJECTION = Object.freeze({ scale: 1, x: 0, y: 0 });
const RELIEF_SHADOW_OPACITY = 0.22;
const CONTACT_GAP_FRACTION = 0.15;
const MAX_CONTACT_GAP = 0.15;
const RELIEF_MODES = new Set(["off", "shadows", "contact", "cutout"]);

/**
 * Collect the silhouette edges that can carry an extruded side face.
 * Coordinates are native pixel boundaries. Each face has start/end points,
 * its source boundary color, orientation, and a painter-order key.
 */
export function collectSideFaces(sourceRGBA, width, height) {
  validatePixels(sourceRGBA, width, height);
  const vanishingPoint = getSceneVanishingPoint(width, height);
  const faces = [];

  // A right face is the exposed right edge of one or more opaque pixels.
  for (let x = 0; x < width; x += 1) {
    let y = 0;
    while (y < height) {
      const offset = (y * width + x) * 4;
      if (!isOpaque(sourceRGBA, offset) || (x + 1 < width && isOpaque(sourceRGBA, offset + 4))) {
        y += 1;
        continue;
      }
      const color = [sourceRGBA[offset], sourceRGBA[offset + 1], sourceRGBA[offset + 2]];
      const edgeX = x + 1;
      if (edgeX >= vanishingPoint.x) {
        y += 1;
        continue;
      }
      const startY = y;
      y += 1;
      while (y < height) {
        const nextOffset = (y * width + x) * 4;
        if (!isOpaque(sourceRGBA, nextOffset) || !matchesRGB(sourceRGBA, nextOffset, color)
          || (x + 1 < width && isOpaque(sourceRGBA, nextOffset + 4))) break;
        y += 1;
      }
      faces.push(makeFace("right", edgeX, startY, edgeX, y, color, width, height, vanishingPoint));
    }
  }

  // The opposite-facing boundary is visible only on the right half of the
  // screen, where it recedes leftward toward the vanishing point.
  for (let x = 0; x < width; x += 1) {
    let y = 0;
    while (y < height) {
      const offset = (y * width + x) * 4;
      if (!isOpaque(sourceRGBA, offset) || (x > 0 && isOpaque(sourceRGBA, offset - 4)) || x <= vanishingPoint.x) {
        y += 1;
        continue;
      }
      const color = [sourceRGBA[offset], sourceRGBA[offset + 1], sourceRGBA[offset + 2]];
      const startY = y;
      y += 1;
      while (y < height) {
        const nextOffset = (y * width + x) * 4;
        if (!isOpaque(sourceRGBA, nextOffset) || !matchesRGB(sourceRGBA, nextOffset, color)
          || (x > 0 && isOpaque(sourceRGBA, nextOffset - 4))) break;
        y += 1;
      }
      faces.push(makeFace("left", x, startY, x, y, color, width, height, vanishingPoint));
    }
  }

  // A bottom face is the exposed lower edge of one or more opaque pixels.
  for (let y = 0; y < height; y += 1) {
    let x = 0;
    while (x < width) {
      const offset = (y * width + x) * 4;
      if (!isOpaque(sourceRGBA, offset) || (y + 1 < height && isOpaque(sourceRGBA, offset + width * 4))) {
        x += 1;
        continue;
      }
      const color = [sourceRGBA[offset], sourceRGBA[offset + 1], sourceRGBA[offset + 2]];
      const edgeY = y + 1;
      if (edgeY >= vanishingPoint.y) {
        x += 1;
        continue;
      }
      const startX = x;
      x += 1;
      while (x < width) {
        const nextOffset = (y * width + x) * 4;
        if (!isOpaque(sourceRGBA, nextOffset) || !matchesRGB(sourceRGBA, nextOffset, color)
          || (y + 1 < height && isOpaque(sourceRGBA, nextOffset + width * 4))) break;
        x += 1;
      }
      faces.push(makeFace("bottom", startX, edgeY, x, edgeY, color, width, height, vanishingPoint));
    }
  }

  // Top-facing boundaries recede upward on the lower half of the screen.
  for (let y = 0; y < height; y += 1) {
    let x = 0;
    while (x < width) {
      const offset = (y * width + x) * 4;
      if (!isOpaque(sourceRGBA, offset) || (y > 0 && isOpaque(sourceRGBA, offset - width * 4)) || y <= vanishingPoint.y) {
        x += 1;
        continue;
      }
      const color = [sourceRGBA[offset], sourceRGBA[offset + 1], sourceRGBA[offset + 2]];
      const startX = x;
      x += 1;
      while (x < width) {
        const nextOffset = (y * width + x) * 4;
        if (!isOpaque(sourceRGBA, nextOffset) || !matchesRGB(sourceRGBA, nextOffset, color)
          || (y > 0 && isOpaque(sourceRGBA, nextOffset - width * 4))) break;
        x += 1;
      }
      faces.push(makeFace("top", startX, y, x, y, color, width, height, vanishingPoint));
    }
  }

  // Faces begin at the outer silhouette and draw inward toward the center.
  faces.sort((a, b) => a.sortKey - b.sortKey || orientationOrder(a.orientation) - orientationOrder(b.orientation)
    || a.start.y - b.start.y || a.start.x - b.start.x);
  return faces;
}

/**
 * Return face runs whose original boundary pixels overlap nontransparent tiles.
 * Tile alpha is sampled at the inverse-projected center of each boundary pixel.
 */
export function collectTileSupportedFaces(faces, tileRGBA, width, height, occluderProjection = IDENTITY_PROJECTION) {
  if (!Array.isArray(faces)) throw new TypeError("Faces must be an array");
  validatePixels(tileRGBA, width, height);
  validateOccluderProjection(occluderProjection);
  const supported = [];
  const { scale, x: offsetX, y: offsetY } = occluderProjection;

  for (const face of faces) {
    const horizontal = face.orientation === "bottom" || face.orientation === "top";
    const vertical = face.orientation === "right" || face.orientation === "left";
    if (!horizontal && !vertical) throw new RangeError("Face orientation must be a rectangle edge");
    const start = horizontal ? face.start.x : face.start.y;
    const end = horizontal ? face.end.x : face.end.y;
    if (!Number.isInteger(start) || !Number.isInteger(end) || end < start) {
      throw new RangeError("Face coordinates must be ordered integer boundaries");
    }
    let runStart = null;
    for (let boundary = start; boundary < end; boundary += 1) {
      const sourcePixel = boundarySourcePixel(face, boundary);
      if (sourcePixel.x < 0 || sourcePixel.x >= width || sourcePixel.y < 0 || sourcePixel.y >= height) {
        if (runStart !== null) {
          supported.push(copyFaceSegment(face, horizontal, runStart, boundary));
          runStart = null;
        }
        continue;
      }
      const tileX = Math.floor((width / 2 + (sourcePixel.x + 0.5 - width / 2 - offsetX) / scale) + 1e-10);
      const tileY = Math.floor((height / 2 + (sourcePixel.y + 0.5 - height / 2 - offsetY) / scale) + 1e-10);
      const supportedPixel = tileX >= 0 && tileX < width && tileY >= 0 && tileY < height
        && isOpaque(tileRGBA, (tileY * width + tileX) * 4);
      if (supportedPixel) {
        if (runStart === null) runStart = boundary;
      } else if (runStart !== null) {
        supported.push(copyFaceSegment(face, horizontal, runStart, boundary));
        runStart = null;
      }
    }
    if (runStart !== null) supported.push(copyFaceSegment(face, horizontal, runStart, end));
  }
  return supported;
}

function boundarySourcePixel(face, boundary) {
  switch (face.orientation) {
    case "right": return { x: face.start.x - 1, y: boundary };
    case "left": return { x: face.start.x, y: boundary };
    case "bottom": return { x: boundary, y: face.start.y - 1 };
    case "top": return { x: boundary, y: face.start.y };
    default: throw new RangeError("Face orientation must be a rectangle edge");
  }
}

function copyFaceSegment(face, horizontal, start, end) {
  const segment = { ...face, start: { ...face.start }, end: { ...face.end } };
  if (Array.isArray(face.color)) segment.color = face.color.slice();
  if (Array.isArray(face.rgb)) segment.rgb = face.rgb.slice();
  if (horizontal) {
    segment.start.x = start;
    segment.end.x = end;
  } else {
    segment.start.y = start;
    segment.end.y = end;
  }
  return segment;
}

export const PERSPECTIVE_DEPTH_FACTOR = 0.015;

/** Camera scale at any physical plane, including the TV in front of the game. */
export function cameraDepthScale(depth) {
  const focalDepth = getSceneCameraDepth();
  if (!Number.isFinite(depth) || !Number.isFinite(focalDepth + depth) || focalDepth + depth <= 0) {
    throw new RangeError("Plane must remain in front of the camera");
  }
  // Preserve bit-for-bit results for the legacy neutral camera preset.
  if (focalDepth === 1 / PERSPECTIVE_DEPTH_FACTOR) return 1 / (1 + depth * PERSPECTIVE_DEPTH_FACTOR);
  return focalDepth / (focalDepth + depth);
}

/** Shared centered camera scale for sidewalls and the backdrop shadow. */
export function perspectiveDepthScale(depth) {
  validateDepth(depth);
  return cameraDepthScale(depth);
}

/** Project a native boundary point by the continuous depth vector. */
export function projectDepthPoint(point, depth, width = 256, height = 240) {
  validateDepth(depth);
  if (!Number.isFinite(point?.x) || !Number.isFinite(point?.y)) throw new TypeError("Point coordinates must be finite numbers");
  validateDimensions(width, height);
  if (depth === 0) return { x: point.x, y: point.y };
  const vanishingPoint = getSceneVanishingPoint(width, height);
  const scale = perspectiveDepthScale(depth);
  return {
    x: vanishingPoint.x + (point.x - vanishingPoint.x) * scale,
    y: vanishingPoint.y + (point.y - vanishingPoint.y) * scale,
  };
}

/** Paint continuous side quads, optional tile relief, then exact source artwork. */
export class CanvasDepthRenderer {
  constructor(canvas, width = 256, height = 240) {
    if (!canvas || typeof canvas.getContext !== "function") throw new TypeError("A canvas element is required");
    validateDimensions(width, height);
    this.canvas = canvas;
    this.width = width;
    this.height = height;
    this.context = canvas.getContext("2d", { alpha: true });
    if (!this.context) throw new Error("Canvas 2D context is unavailable");
    this.sourceCanvas = makeCanvas(width, height);
    this.sourceContext = this.sourceCanvas.getContext("2d", { alpha: true });
    if (!this.sourceContext) throw new Error("Canvas 2D context is unavailable");
    this.sourceImage = this.sourceContext.createImageData(width, height);
    this.occluderCanvas = null;
    this.occluderContext = null;
    this.occluderImage = null;
    this.hasOccluder = false;
    this.receiverCanvas = null;
    this.receiverContext = null;
    this.receiverImage = null;
    this.hasReceiver = false;
    this.reliefCanvas = null;
    this.reliefContext = null;
    this.lastPixels = this.sourceImage.data;
    this.hasPixels = false;
    this.faces = [];
    this.tileSupportedFaces = [];
    this.lastDepth = NaN;
    this.lastLightingRevision = -1;
    this.lastViewRevision = -1;
    this.lastProjectionScale = NaN;
    this.lastProjectionX = NaN;
    this.lastProjectionY = NaN;
    this.lastReliefMode = "off";
    this.lastReliefSoftness = NaN;
    this.lastOverlapFraction = NaN;
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
    return { width: pixelWidth, height: pixelHeight };
  }

  paint(sourceRGBA, depth, occluderRGBA = null, occluderProjection = IDENTITY_PROJECTION, reliefOptions = null) {
    validatePixels(sourceRGBA, this.width, this.height);
    validateDepth(depth);
    validateOccluderProjection(occluderProjection);
    const relief = normalizeReliefOptions(reliefOptions);
    if (occluderRGBA !== null) validatePixels(occluderRGBA, this.width, this.height);
    if (relief.receiverRGBA !== null) validatePixels(relief.receiverRGBA, this.width, this.height);
    const projectionScale = occluderProjection.scale;
    const projectionX = occluderProjection.x;
    const projectionY = occluderProjection.y;
    const lightingRevision = getSceneLightRevision();
    const viewRevision = getSceneViewRevision();
    const changedPixels = !this.hasPixels || !equalBytes(sourceRGBA, this.lastPixels);
    const changedOccluder = occluderAlphaChanged(occluderRGBA, this.hasOccluder, this.occluderImage);
    const changedReceiver = occluderAlphaChanged(relief.receiverRGBA, this.hasReceiver, this.receiverImage);
    const changedProjection = projectionScale !== this.lastProjectionScale
      || projectionX !== this.lastProjectionX || projectionY !== this.lastProjectionY;
    const changedLighting = lightingRevision !== this.lastLightingRevision;
    const changedView = viewRevision !== this.lastViewRevision;
    const changedReliefMode = relief.mode !== this.lastReliefMode || relief.softness !== this.lastReliefSoftness;
    const changedOverlapFraction = relief.overlapFraction !== this.lastOverlapFraction;
    const resized = this.lastCanvasWidth !== this.canvas.width || this.lastCanvasHeight !== this.canvas.height;
    if (!changedPixels && !changedOccluder && !changedReceiver && !changedProjection && !changedLighting && !changedView
      && !changedReliefMode && !changedOverlapFraction && depth === this.lastDepth && !resized) return false;

    if (changedPixels) {
      this.sourceImage.data.set(sourceRGBA);
      this.hasPixels = true;
      this.sourceContext.putImageData(this.sourceImage, 0, 0);
    }
    if (changedPixels || changedView) this.faces = collectSideFaces(sourceRGBA, this.width, this.height);
    if (changedOccluder) this.updateOccluder(occluderRGBA);
    if (changedReceiver) this.updateReceiver(relief.receiverRGBA);
    // Support depends on native coverage, not thickness. Keep it valid at zero
    // so raising thickness again doesn't reuse an empty classification.
    if (relief.mode === "contact" && this.hasOccluder) {
      if (changedPixels || changedView || changedOccluder || changedProjection || changedReliefMode || resized) {
        this.tileSupportedFaces = collectTileSupportedFaces(
          this.faces, this.occluderImage.data, this.width, this.height, occluderProjection,
        );
      }
    } else {
      this.tileSupportedFaces = [];
    }
    const reliefFaces = relief.mode === "contact" ? this.tileSupportedFaces
      : relief.mode === "cutout" ? this.faces : [];
    const useReceiverShadow = depth > 0 && relief.mode !== "off" && this.hasReceiver && relief.overlapFraction === 1;
    const capReceiverWalls = depth > 0 && relief.mode !== "off" && this.hasReceiver && relief.overlapFraction < 1;
    const useRelief = depth > 0 && relief.mode !== "off" && this.hasOccluder;
    // Only overlap geometry changes depth; the source face and backdrop caster stay fixed.
    const reliefDepth = depth * relief.overlapFraction;

    const context = this.context;
    const scaleX = this.canvas.width / this.width;
    const scaleY = this.canvas.height / this.height;
    context.setTransform(scaleX, 0, 0, scaleY, 0, 0);
    context.clearRect(0, 0, this.width, this.height);
    context.imageSmoothingEnabled = true;
    if (useReceiverShadow) {
      this.drawReceiverShadow(depth, scaleX, scaleY, occluderProjection, relief.softness);
      context.imageSmoothingEnabled = false;
      context.drawImage(this.reliefCanvas, 0, 0, this.width, this.height);
    }
    if (depth > 0) {
      for (const face of this.faces) paintFace(context, face, depth, this.width, this.height);

      if (this.hasOccluder) {
        applyAlphaMask(context, this.occluderCanvas, this.width, this.height, occluderProjection, "destination-out");
      }

      if (capReceiverWalls) {
        // Replace full-depth tile walls only where the rear sprite is visible.
        applyAlphaMask(context, this.receiverCanvas, this.width, this.height, occluderProjection, "destination-out");
        this.drawRelief(this.faces, reliefDepth, scaleX, scaleY,
          this.receiverCanvas, occluderProjection, relief.softness);
        context.imageSmoothingEnabled = false;
        context.drawImage(this.reliefCanvas, 0, 0, this.width, this.height);
      }

      if (useRelief) {
        this.drawRelief(reliefFaces, reliefDepth, scaleX, scaleY,
          this.occluderCanvas, occluderProjection, relief.softness);
        context.imageSmoothingEnabled = false;
        context.drawImage(this.reliefCanvas, 0, 0, this.width, this.height);
      }
    }

    // Restore the original, unshifted image last so its pixel colors and
    // aligned top surface stay exact over every side quad.
    context.imageSmoothingEnabled = false;
    context.drawImage(this.sourceCanvas, 0, 0, this.width, this.height);
    this.lastDepth = depth;
    this.lastLightingRevision = lightingRevision;
    this.lastViewRevision = viewRevision;
    this.lastProjectionScale = projectionScale;
    this.lastProjectionX = projectionX;
    this.lastProjectionY = projectionY;
    this.lastReliefMode = relief.mode;
    this.lastReliefSoftness = relief.softness;
    this.lastOverlapFraction = relief.overlapFraction;
    this.lastCanvasWidth = this.canvas.width;
    this.lastCanvasHeight = this.canvas.height;
    return true;
  }

  updateOccluder(occluderRGBA) {
    if (occluderRGBA === null) {
      this.hasOccluder = false;
      return;
    }
    if (!this.occluderCanvas) {
      this.occluderCanvas = makeCanvas(this.width, this.height);
      this.occluderContext = this.occluderCanvas.getContext("2d", { alpha: true });
      if (!this.occluderContext) throw new Error("Canvas 2D context is unavailable");
      this.occluderImage = this.occluderContext.createImageData(this.width, this.height);
    }
    const target = this.occluderImage.data;
    for (let offset = 0; offset < target.length; offset += 4) target[offset + 3] = occluderRGBA[offset + 3];
    this.occluderContext.putImageData(this.occluderImage, 0, 0);
    this.hasOccluder = true;
  }

  updateReceiver(receiverRGBA) {
    if (receiverRGBA === null) {
      this.hasReceiver = false;
      return;
    }
    if (!this.receiverCanvas) {
      this.receiverCanvas = makeCanvas(this.width, this.height);
      this.receiverContext = this.receiverCanvas.getContext("2d", { alpha: true });
      if (!this.receiverContext) throw new Error("Canvas 2D context is unavailable");
      this.receiverImage = this.receiverContext.createImageData(this.width, this.height);
    }
    const target = this.receiverImage.data;
    for (let offset = 0; offset < target.length; offset += 4) target[offset + 3] = receiverRGBA[offset + 3];
    this.receiverContext.putImageData(this.receiverImage, 0, 0);
    this.hasReceiver = true;
  }

  drawReceiverShadow(depth, scaleX, scaleY, projection, softness) {
    this.prepareReliefCanvas(depth, scaleX, scaleY, softness);
    const context = this.reliefContext;
    context.setTransform(scaleX, 0, 0, scaleY, 0, 0);
    applyAlphaMask(context, this.receiverCanvas, this.width, this.height, projection, "destination-in");
  }

  drawRelief(faces, depth, scaleX, scaleY, maskCanvas, maskProjection, softness) {
    this.prepareReliefCanvas(depth, scaleX, scaleY, softness);
    const context = this.reliefContext;
    context.setTransform(scaleX, 0, 0, scaleY, 0, 0);
    context.imageSmoothingEnabled = true;
    if (depth > 0) {
      for (const face of faces) paintFace(context, face, depth, this.width, this.height);
    }
    applyAlphaMask(context, maskCanvas, this.width, this.height, maskProjection, "destination-in");
  }

  prepareReliefCanvas(depth, scaleX, scaleY, softness) {
    if (!this.reliefCanvas) {
      this.reliefCanvas = makeCanvas(this.canvas.width, this.canvas.height);
      this.reliefContext = this.reliefCanvas.getContext("2d", { alpha: true });
      if (!this.reliefContext) throw new Error("Canvas 2D context is unavailable");
    }
    if (this.reliefCanvas.width !== this.canvas.width) this.reliefCanvas.width = this.canvas.width;
    if (this.reliefCanvas.height !== this.canvas.height) this.reliefCanvas.height = this.canvas.height;

    const context = this.reliefContext;
    const outputWidth = this.canvas.width;
    const outputHeight = this.canvas.height;
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, outputWidth, outputHeight);
    if (depth > 0) {
      paintReliefShadow(context, this.sourceCanvas, depth, this.width, this.height,
        outputWidth, outputHeight, scaleX, scaleY, softness);
    }
  }
}

function applyAlphaMask(context, maskCanvas, width, height, projection, operation) {
  context.save();
  context.imageSmoothingEnabled = false;
  context.globalCompositeOperation = operation;
  context.translate(width / 2 + projection.x, height / 2 + projection.y);
  context.scale(projection.scale, projection.scale);
  context.translate(-width / 2, -height / 2);
  context.drawImage(maskCanvas, 0, 0, width, height);
  context.restore();
}

function paintReliefShadow(context, sourceCanvas, depth, width, height,
  outputWidth, outputHeight, scaleX, scaleY, softness) {
  // Receiving artwork is almost touching the caster. Its optical contact gap
  // stays tiny even when the backdrop gap or cardboard thickness is large.
  const clearance = Math.min(MAX_CONTACT_GAP, depth * CONTACT_GAP_FRACTION);
  const cameraScale = perspectiveDepthScale(clearance);
  const light = shadowLightProjection(0, clearance, width, height);
  const viewOffset = getSceneViewOffset(width, height);
  const shadowScale = cameraScale * light.magnification;
  const blur = light.travel * softness * cameraScale * Math.max(scaleX, scaleY);
  context.save();
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.globalAlpha = RELIEF_SHADOW_OPACITY * (light.opacity ?? 1);
  context.imageSmoothingEnabled = false;
  context.filter = blur > 0 ? `blur(${blur}px)` : "none";
  context.translate(outputWidth / 2 + light.x * cameraScale * scaleX,
    outputHeight / 2 + light.y * cameraScale * scaleY);
  const viewShiftX = viewOffset.x * (1 - cameraScale) * scaleX;
  const viewShiftY = viewOffset.y * (1 - cameraScale) * scaleY;
  if (viewShiftX !== 0 || viewShiftY !== 0) context.translate(viewShiftX, viewShiftY);
  context.scale(shadowScale, shadowScale);
  context.translate(-outputWidth / 2, -outputHeight / 2);
  context.drawImage(sourceCanvas, 0, 0, outputWidth, outputHeight);
  context.restore();

  // Keep only the blurred source alpha and make it a translucent black mark.
  context.save();
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.globalCompositeOperation = "source-in";
  context.globalAlpha = 1;
  context.filter = "none";
  context.fillStyle = "#000";
  context.fillRect(0, 0, outputWidth, outputHeight);
  context.restore();
}

function paintFace(context, face, depth, width, height) {
  const startBack = projectDepthPoint(face.start, depth, width, height);
  const endBack = projectDepthPoint(face.end, depth, width, height);
  const shade = surfaceShade(face.orientation, false, {
    x: (face.start.x + face.end.x) / 2,
    y: (face.start.y + face.end.y) / 2,
    z: depth / 2,
  }, width, height);
  const [red, green, blue] = face.color;
  const start = face.start;
  const end = face.end;
  context.beginPath();
  context.moveTo(start.x, start.y);
  context.lineTo(end.x, end.y);
  context.lineTo(endBack.x, endBack.y);
  context.lineTo(startBack.x, startBack.y);
  context.closePath();
  context.fillStyle = `rgb(${Math.round(red * shade)} ${Math.round(green * shade)} ${Math.round(blue * shade)})`;
  context.fill();
}

function makeFace(orientation, startX, startY, endX, endY, color, width, height, vanishingPoint) {
  const start = { x: startX, y: startY };
  const end = { x: endX, y: endY };
  return {
    orientation,
    start,
    end,
    color,
    rgb: color,
    sortKey: -(((startX + endX) / 2 - vanishingPoint.x) ** 2
      + ((startY + endY) / 2 - vanishingPoint.y) ** 2),
  };
}

function orientationOrder(orientation) {
  return ({ bottom: 0, right: 1, top: 2, left: 3 })[orientation] ?? 4;
}

function matchesRGB(source, offset, color) {
  return source[offset] === color[0] && source[offset + 1] === color[1] && source[offset + 2] === color[2];
}

function isOpaque(source, offset) {
  return source[offset + 3] > 0;
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
    throw new RangeError("Width and height must be positive integers");
  }
}

function validateDepth(depth) {
  if (!Number.isFinite(depth) || depth < 0 || depth > MAX_DEPTH) {
    throw new RangeError(`Depth must be a number from 0 to ${MAX_DEPTH}`);
  }
}

function validateOccluderProjection(projection) {
  if (!projection || typeof projection !== "object"
    || !Number.isFinite(projection.scale) || projection.scale <= 0
    || !Number.isFinite(projection.x) || !Number.isFinite(projection.y)) {
    throw new RangeError("Occluder projection requires a positive finite scale and finite x/y offsets");
  }
}

function normalizeReliefOptions(options) {
  if (options === null) return { mode: "off", softness: 1, receiverRGBA: null, overlapFraction: 1 };
  if (typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("Sprite relief options must be an object or null");
  }
  const mode = options.mode;
  if (!RELIEF_MODES.has(mode)) throw new RangeError("Sprite relief mode must be off, shadows, contact, or cutout");
  const softness = options.softness === undefined ? 1 : options.softness;
  if (!Number.isFinite(softness) || softness < 0 || softness > 2) {
    throw new RangeError("Sprite relief softness must be between 0 and 2");
  }
  const receiverRGBA = options.receiverRGBA === undefined ? null : options.receiverRGBA;
  const overlapFraction = options.overlapFraction === undefined ? 1 : options.overlapFraction;
  if (!Number.isFinite(overlapFraction) || overlapFraction < 0 || overlapFraction > 1) {
    throw new RangeError("Sprite overlap fraction must be between 0 and 1");
  }
  return { mode, softness, receiverRGBA, overlapFraction };
}

function equalBytes(left, right) {
  if (left.length !== right.length) return false;
  // NES RGBA buffers are four-byte aligned. Compare a whole pixel at once;
  // byte-by-byte scans can cost more than rebuilding a simple silhouette.
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

function occluderAlphaChanged(mask, hasOccluder, image) {
  if (mask === null) return hasOccluder;
  if (!hasOccluder) return true;
  const owned = image.data;
  for (let offset = 3; offset < mask.length; offset += 4) {
    if (mask[offset] !== owned[offset]) return true;
  }
  return false;
}

function makeCanvas(width, height) {
  let canvas;
  if (typeof OffscreenCanvas === "function") canvas = new OffscreenCanvas(width, height);
  else if (typeof document !== "undefined") canvas = document.createElement("canvas");
  else throw new Error("CanvasDepthRenderer requires a browser canvas environment");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}
