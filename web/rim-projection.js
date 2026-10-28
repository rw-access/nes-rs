import { MAX_DEPTH, cameraDepthScale, projectDepthPoint } from "./depth-mesh.js";
import { shadowLightProjection } from "./scene-lighting.js";
import { getSceneViewOffset, getSceneVanishingPoint } from "./scene-view.js";

const DEPTH_EPSILON = 1e-12;

/** Project the camera-only rim aperture and its nearby-light shadow. */
export function calculateRimProjection(width, height, gap, receiverDepth = gap, thickness = gap / 2, frontDepth = 0) {
  validateProjection(width, height, gap);
  if (!Number.isFinite(receiverDepth) || receiverDepth < 0 || receiverDepth > gap) {
    throw new RangeError("Receiver must be between game and backdrop");
  }
  const rim = validateRimDepth(gap, thickness, frontDepth);
  const topLeft = projectDepthPoint({ x: 0, y: 0 }, receiverDepth, width, height);
  const bottomRight = projectDepthPoint({ x: width, y: height }, receiverDepth, width, height);
  const receiverScale = cameraDepthScale(receiverDepth);
  const rimBackDepth = rim.frontDepth + rim.thickness;
  const farRect = {
    x: topLeft.x,
    y: topLeft.y,
    width: bottomRight.x - topLeft.x,
    height: bottomRight.y - topLeft.y,
  };
  const eligibleForShadow = gap > 0 && receiverDepth >= rimBackDepth;
  const light = eligibleForShadow
    ? shadowLightProjection(rimBackDepth, receiverDepth, width, height)
    : { t: 0, magnification: 1, x: 0, y: 0, travel: 0 };
  const castsShadow = eligibleForShadow && light.opacity !== 0;
  const effectiveMagnification = castsShadow ? light.magnification : 1;
  const apertureScale = receiverScale * effectiveMagnification;
  const viewOffset = getSceneViewOffset(width, height);
  const vanishingPoint = getSceneVanishingPoint(width, height);
  const cameraOffset = {
    x: viewOffset.x * (1 - receiverScale),
    y: viewOffset.y * (1 - receiverScale),
  };
  const litOpening = effectiveMagnification === 1 && (!castsShadow || (light.x === 0 && light.y === 0))
    ? { ...farRect }
    : {
      x: width / 2 - width / 2 * apertureScale + light.x * receiverScale + cameraOffset.x,
      y: height / 2 - height / 2 * apertureScale + light.y * receiverScale + cameraOffset.y,
      width: width * apertureScale,
      height: height * apertureScale,
    };
  const clearance = castsShadow ? receiverDepth - rimBackDepth : 0;

  const projection = {
    rimDepth: rim.frontDepth,
    rimFrontDepth: rim.frontDepth,
    rimThickness: rim.thickness,
    rimBackDepth,
    receiverDepth,
    castsShadow,
    clearance,
    travel: castsShadow ? light.travel : 0,
    lightMagnification: effectiveMagnification,
    receiverScale,
    cameraScale: receiverScale,
    vanishingPoint,
    cameraOffset,
    apertureScale,
    farRect,
    litOpening,
    blurDistance: castsShadow ? light.travel * receiverScale : 0,
  };
  if (light.opacity !== undefined) projection.opacity = light.opacity;
  return projection;
}

/** Project a point from the rim front to its back with the shared camera. */
export function projectRimBackPoint(point, gap, width = 256, height = 240, thickness = gap / 2, frontDepth = 0) {
  validateProjection(width, height, gap);
  const rim = validateRimDepth(gap, thickness, frontDepth);
  return projectDepthPoint(point, rim.frontDepth + rim.thickness, width, height);
}

function validateProjection(width, height, gap) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Positive integer dimensions required");
  }
  if (!Number.isFinite(gap) || gap < 0 || gap > MAX_DEPTH) {
    throw new RangeError(`Gap must be from 0 to ${MAX_DEPTH}`);
  }
}

function validateRimDepth(gap, thickness, frontDepth) {
  if (!Number.isFinite(frontDepth) || frontDepth < 0 || frontDepth > gap) {
    throw new RangeError("Rim front depth must be within the backdrop gap");
  }
  if (!Number.isFinite(thickness) || thickness < 0) throw new RangeError("Rim thickness must be nonnegative and finite");
  const availableDepth = gap - frontDepth;
  if (thickness > availableDepth + DEPTH_EPSILON) {
    throw new RangeError("Rim front depth plus thickness must stay within the backdrop gap");
  }
  return { frontDepth, thickness: Math.min(thickness, availableDepth) };
}
