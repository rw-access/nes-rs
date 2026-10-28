import { SCENE_DEPTH_TO_NATIVE_PIXELS } from "./viewing-model.js";

// A light above the scene, centered horizontally on the viewer's side.
export const SCENE_LIGHT = Object.freeze({ xFraction: 0.5, yFraction: 0.15, height: 24 });
export const MIN_SCENE_LIGHT_DISTANCE = 1;
export const MAX_SCENE_LIGHT_DISTANCE = 8;
const DEFAULT_SCENE_LIGHT_DISTANCE = 2;
const ROTATION_EPSILON = 1e-6;
const MAX_SHADOW_RAY_T = 8;
const FULL_SHADOW_RAY_T = MAX_SHADOW_RAY_T / 2;

let sceneLightDistance = DEFAULT_SCENE_LIGHT_DISTANCE;
let sceneLightPlacement = null;
let sceneLightPlacementPosition = null;
const IDENTITY_ROTATION = Object.freeze([1, 0, 0, 0, 1, 0, 0, 0, 1]);
let sceneLightRotation = IDENTITY_ROTATION;
let sceneLightRevision = 0;

export function getSceneLightDistance() {
  return sceneLightDistance;
}

export function getSceneLightRevision() {
  return sceneLightRevision;
}

/** Return the immutable phone-plane light placement, or null for the legacy fixture. */
export function getSceneLightPlacement() {
  return sceneLightPlacement;
}

/** Place the light out from the phone's initial plane at a fixed distance and elevation. */
export function setSceneLightPlacement(placement) {
  if (placement === null) {
    if (sceneLightPlacement === null) return false;
    sceneLightPlacement = null;
    sceneLightPlacementPosition = null;
    sceneLightRevision += 1;
    return true;
  }
  if (typeof placement !== "object" || Array.isArray(placement)) {
    throw new TypeError("Scene light placement must be an object or null");
  }

  const { distanceCm, elevationDegrees, gameWidthCm } = placement;
  const azimuthDegrees = placement.azimuthDegrees === undefined ? 0 : placement.azimuthDegrees;
  if (!Number.isFinite(distanceCm) || distanceCm < 5 || distanceCm > 300) {
    throw new RangeError("Light placement distance must be from 5 to 300 cm");
  }
  if (!Number.isFinite(elevationDegrees) || elevationDegrees < 0 || elevationDegrees > 80) {
    throw new RangeError("Light placement elevation must be from 0 to 80 degrees");
  }
  if (!Number.isFinite(azimuthDegrees) || azimuthDegrees < -60 || azimuthDegrees > 60) {
    throw new RangeError("Light placement azimuth must be from -60 to 60 degrees");
  }
  if (!Number.isFinite(gameWidthCm) || gameWidthCm <= 0) {
    throw new RangeError("Light placement game width must be a positive finite number");
  }
  const pixelsPerCm = 256 / gameWidthCm;
  const elevation = elevationDegrees * Math.PI / 180;
  const azimuth = azimuthDegrees * Math.PI / 180;
  const position = [
    distanceCm * Math.cos(elevation) * Math.sin(azimuth) * pixelsPerCm,
    -distanceCm * Math.sin(elevation) * pixelsPerCm,
    -distanceCm * Math.cos(elevation) * Math.cos(azimuth) * pixelsPerCm,
  ];
  if (!Number.isFinite(pixelsPerCm) || !position.every(Number.isFinite)
    || !Number.isFinite(Math.hypot(...position))) {
    throw new RangeError("Scene light placement produces a nonfinite position");
  }

  if (sceneLightPlacement && samePlacement(sceneLightPlacement, { distanceCm, elevationDegrees, azimuthDegrees, gameWidthCm })) {
    return false;
  }
  sceneLightPlacement = Object.freeze({
    distanceCm,
    elevationDegrees,
    azimuthDegrees,
    gameWidthCm,
  });
  sceneLightPlacementPosition = Object.freeze(position);
  sceneLightRevision += 1;
  return true;
}

/** Return the room-to-current-scene rotation as an immutable row-major matrix. */
export function getSceneLightRotation() {
  return sceneLightRotation;
}

/** Update the shared point-light distance, reporting whether it changed. */
export function setSceneLightDistance(distance) {
  if (!Number.isFinite(distance) || distance < MIN_SCENE_LIGHT_DISTANCE || distance > MAX_SCENE_LIGHT_DISTANCE) {
    throw new RangeError(`Scene light distance must be from ${MIN_SCENE_LIGHT_DISTANCE} to ${MAX_SCENE_LIGHT_DISTANCE}`);
  }
  if (distance === sceneLightDistance) return false;
  sceneLightDistance = distance;
  sceneLightRevision += 1;
  return true;
}

/** Update the room-relative light pose, reporting whether the matrix changed. */
export function setSceneLightRotation(rotation) {
  validateRotation(rotation);
  if (rotation.every((value, index) => value === sceneLightRotation[index])) return false;
  sceneLightRotation = isIdentityRotation(rotation) ? IDENTITY_ROTATION : Object.freeze(Array.from(rotation));
  sceneLightRevision += 1;
  return true;
}

/** Current light position after extending its base ray from screen center. */
export function getSceneLightPosition(width = 256, height = 240) {
  return lightPosition(width, height);
}

function lightPosition(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Projection dimensions must be positive integers");
  }
  const centerX = width / 2, centerY = height / 2;
  if (sceneLightPlacement !== null) {
    const m = sceneLightRotation;
    const [baseX, baseY, baseZ] = sceneLightPlacementPosition;
    const rotatedX = m[0] * baseX + m[1] * baseY + m[2] * baseZ;
    const rotatedY = m[3] * baseX + m[4] * baseY + m[5] * baseZ;
    const rotatedZ = m[6] * baseX + m[7] * baseY + m[8] * baseZ;
    const position = {
      x: centerX + rotatedX * width / 256,
      y: centerY + rotatedY * height / 240,
      z: rotatedZ / SCENE_DEPTH_TO_NATIVE_PIXELS,
    };
    if (!Object.values(position).every(Number.isFinite)) {
      throw new RangeError("Scene light placement produces a nonfinite position");
    }
    return position;
  }
  // Retain the legacy neutral path exactly; this is also the baseline fixture
  // against which every room-relative pose is measured.
  if (isIdentityRotation(sceneLightRotation)) {
    const baseX = width * SCENE_LIGHT.xFraction, baseY = height * SCENE_LIGHT.yFraction;
    return {
      x: centerX + (baseX - centerX) * sceneLightDistance,
      y: centerY + (baseY - centerY) * sceneLightDistance,
      z: -SCENE_LIGHT.height * sceneLightDistance,
    };
  }

  // Rotate in isotropic canonical native-pixel units. The physical ray is
  // extended by distance first, then mapped to the caller's viewport size.
  const canonicalX = 0;
  const canonicalY = (SCENE_LIGHT.yFraction - 0.5) * 240 * sceneLightDistance;
  const canonicalZ = -SCENE_LIGHT.height * sceneLightDistance * SCENE_DEPTH_TO_NATIVE_PIXELS;
  const m = sceneLightRotation;
  const rotatedX = m[0] * canonicalX + m[1] * canonicalY + m[2] * canonicalZ;
  const rotatedY = m[3] * canonicalX + m[4] * canonicalY + m[5] * canonicalZ;
  const rotatedZ = m[6] * canonicalX + m[7] * canonicalY + m[8] * canonicalZ;
  return {
    x: centerX + rotatedX * width / 256,
    y: centerY + rotatedY * height / 240,
    z: rotatedZ / SCENE_DEPTH_TO_NATIVE_PIXELS,
  };
}

function samePlacement(left, right) {
  return left.distanceCm === right.distanceCm
    && left.elevationDegrees === right.elevationDegrees
    && left.azimuthDegrees === right.azimuthDegrees
    && left.gameWidthCm === right.gameWidthCm
}

function isIdentityRotation(rotation) {
  return rotation.every((value, index) => value === IDENTITY_ROTATION[index]);
}

function validateRotation(rotation) {
  if ((!Array.isArray(rotation) && !ArrayBuffer.isView(rotation)) || rotation.length !== 9
    || rotation instanceof DataView || !Array.from(rotation).every(Number.isFinite)) {
    throw new TypeError("Scene light rotation must be a finite 3×3 row-major matrix");
  }
  const matrix = Array.from(rotation);
  for (let rowA = 0; rowA < 3; rowA += 1) {
    for (let rowB = rowA; rowB < 3; rowB += 1) {
      const dot = matrix[rowA * 3] * matrix[rowB * 3]
        + matrix[rowA * 3 + 1] * matrix[rowB * 3 + 1]
        + matrix[rowA * 3 + 2] * matrix[rowB * 3 + 2];
      const expected = rowA === rowB ? 1 : 0;
      if (Math.abs(dot - expected) > ROTATION_EPSILON) {
        throw new RangeError("Scene light rotation must be orthonormal");
      }
    }
  }
  const determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7])
    - matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6])
    + matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  if (Math.abs(determinant - 1) > ROTATION_EPSILON) {
    throw new RangeError("Scene light rotation must be a proper rotation with determinant +1");
  }
}

/** Project a point-light ray from a caster plane to a scene receiver. */
export function shadowLightProjection(casterDepth, receiverDepth, width = 256, height = 240) {
  const light = lightPosition(width, height);
  if (!Number.isFinite(casterDepth) || casterDepth < 0 || casterDepth > 6
    || !Number.isFinite(receiverDepth) || receiverDepth < 0 || receiverDepth > 6) {
    throw new RangeError("Caster and receiver depths must be from 0 to 6");
  }
  if (receiverDepth < casterDepth) throw new RangeError("Receiver depth must be at or behind the caster");

  const clearance = casterDepth - light.z;
  const gap = receiverDepth - casterDepth;
  const rawT = clearance > 0 ? gap / clearance : Infinity;
  if (!(clearance > 0) || !Number.isFinite(rawT)) {
    return { t: 0, magnification: 1, x: 0, y: 0, travel: gap * 1.5, opacity: 0 };
  }

  // Near-parallel rays can otherwise create unbounded canvas transforms.
  // Clamp the finite geometry and fade it out before it reaches that limit.
  const t = Math.min(rawT, MAX_SHADOW_RAY_T);
  const opacity = rawT <= FULL_SHADOW_RAY_T
    ? 1
    : rawT >= MAX_SHADOW_RAY_T
      ? 0
      : 1 - smoothstep((rawT - FULL_SHADOW_RAY_T) / (MAX_SHADOW_RAY_T - FULL_SHADOW_RAY_T));
  const projection = {
    t,
    magnification: 1 + t,
    x: (width / 2 - light.x) * t,
    y: (height / 2 - light.y) * t,
    travel: gap * 1.5,
  };
  if (opacity < 1) projection.opacity = opacity;
  return projection;
}

/** Diffuse irradiance of a backdrop point facing the viewer from the actual point light. */
export function backdropIrradiance(point, width = 256, height = 240) {
  const light = lightPosition(width, height);
  return backdropIrradianceFromLight(point, light, width, height);
}

/** Diffuse backdrop irradiance using a light position precomputed for the same canvas. */
export function backdropIrradianceFromLight(point, light, width = 256, height = 240) {
  validateProjectionDimensions(width, height);
  if (!point || typeof point !== "object"
    || !Number.isFinite(point.x) || !Number.isFinite(point.y) || !Number.isFinite(point.z)) {
    throw new TypeError("Backdrop point must have finite x, y, and z coordinates");
  }
  validateBackdropDepth(point.z);
  validateBackdropLight(light);
  const dx = (light.x - point.x) * 256 / width;
  const dy = (light.y - point.y) * 240 / height;
  const dz = (light.z - point.z) * SCENE_DEPTH_TO_NATIVE_PIXELS;
  const length = Math.hypot(dx, dy, dz);
  if (!Number.isFinite(length)) throw new RangeError("Backdrop light direction must be finite");
  if (length === 0) return 1;
  const cosine = Math.max(0, Math.min(1, -dz / length));
  const attenuation = inverseSquareAttenuation(sceneLightReferenceRadius(), length);
  return 0.35 + 0.65 * cosine * attenuation;
}

/** Build a low-allocation irradiance sampler for many x/y points on one receiver plane. */
export function createBackdropIrradianceSampler(light, depth, width = 256, height = 240) {
  validateProjectionDimensions(width, height);
  validateBackdropLight(light);
  validateBackdropDepth(depth);

  const xScale = 256 / width;
  const yScale = 240 / height;
  const lightX = light.x * xScale;
  const lightY = light.y * yScale;
  const dz = (light.z - depth) * SCENE_DEPTH_TO_NATIVE_PIXELS;
  const dzSquared = dz * dz;
  const referenceRadius = sceneLightReferenceRadius();
  if (![lightX, lightY, dz].every(Number.isFinite)) {
    throw new RangeError("Backdrop light direction must be finite");
  }

  return (x, y) => {
    const dx = lightX - x * xScale;
    const dy = lightY - y * yScale;
    const squaredLength = dx * dx + dy * dy + dzSquared;
    const length = Number.isFinite(squaredLength)
      ? Math.sqrt(squaredLength)
      : Math.hypot(dx, dy, dz);
    if (length === 0) return 1;
    const cosine = Math.max(0, Math.min(1, -dz / length));
    const attenuation = inverseSquareAttenuation(referenceRadius, length);
    return 0.35 + 0.65 * cosine * attenuation;
  };
}

function validateProjectionDimensions(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Projection dimensions must be positive integers");
  }
}

function validateBackdropLight(light) {
  if (!light || typeof light !== "object"
    || !Number.isFinite(light.x) || !Number.isFinite(light.y) || !Number.isFinite(light.z)) {
    throw new TypeError("Light position must have finite x, y, and z coordinates");
  }
}

function validateBackdropDepth(depth) {
  if (!Number.isFinite(depth)) throw new TypeError("Backdrop depth must be finite");
  if (depth < 0 || depth > 6) throw new RangeError("Backdrop depth must be from 0 to 6");
}

function sceneLightReferenceRadius() {
  const baseline = sceneLightPlacementPosition ?? [
    0,
    (SCENE_LIGHT.yFraction - 0.5) * 240 * sceneLightDistance,
    -SCENE_LIGHT.height * sceneLightDistance * SCENE_DEPTH_TO_NATIVE_PIXELS,
  ];
  const radius = Math.hypot(...baseline);
  if (!(radius > 0) || !Number.isFinite(radius)) {
    throw new RangeError("Reference light radius must be positive and finite");
  }
  return radius;
}

function inverseSquareAttenuation(referenceRadius, distance) {
  if (distance === 0) return 1;
  if (!Number.isFinite(distance)) return 0;
  const ratio = Math.min(1, referenceRadius / distance);
  return ratio * ratio;
}

function smoothstep(value) {
  const bounded = Math.max(0, Math.min(1, value));
  return bounded * bounded * (3 - 2 * bounded);
}

const AMBIENT = 0.45;
const DIFFUSE = 0.32;

const OUTWARD_NORMALS = Object.freeze({
  top: Object.freeze({ x: 0, y: -1 }),
  right: Object.freeze({ x: 1, y: 0 }),
  bottom: Object.freeze({ x: 0, y: 1 }),
  left: Object.freeze({ x: -1, y: 0 }),
});

/** Shade a physical side-face midpoint using the same light as its shadow. */
export function surfaceShade(orientation, inward = false, point = null, width = 256, height = 240) {
  const normal = OUTWARD_NORMALS[orientation];
  if (!normal) throw new RangeError("Orientation must be top, right, bottom, or left");
  if (typeof inward !== "boolean") throw new TypeError("Inward must be a boolean");

  const light = lightPosition(width, height);
  const position = point ?? { x: width / 2, y: height / 2, z: 0 };
  const depth = position.z ?? 0;
  if (!Number.isFinite(position.x) || !Number.isFinite(position.y)) {
    throw new TypeError("Surface coordinates must be finite numbers");
  }
  if (!Number.isFinite(depth) || depth < 0 || depth > 6) {
    throw new RangeError("Surface depth must be from 0 to 6");
  }
  const dx = (light.x - position.x) * 256 / width;
  const dy = (light.y - position.y) * 240 / height;
  const dz = (light.z - depth) * SCENE_DEPTH_TO_NATIVE_PIXELS;
  const length = Math.hypot(dx, dy, dz);
  if (!(length > 0) || !Number.isFinite(length)) return AMBIENT;

  const direction = inward ? -1 : 1;
  const diffuse = Math.max(0, direction * (normal.x * dx + normal.y * dy) / length);
  const attenuation = inverseSquareAttenuation(sceneLightReferenceRadius(), length);
  return AMBIENT + DIFFUSE * diffuse * attenuation;
}
