// Physical focal length is expressed in the canonical 256-wide NES scene.
const NATIVE_WIDTH = 256;
// Approximate optical calibration preserving the established 35 cm / 7 cm view.
export const SCENE_DEPTH_TO_NATIVE_PIXELS = 19.2;
const MAX_VIEW_OFFSET = 64;
const MIN_FOCAL_DEPTH = 0.1;
const MAX_FOCAL_DEPTH = 10_000;
const MIN_DISTANCE_CM = 20;
const MAX_DISTANCE_CM = 100;
// Controller output is normalized against 25°, not limited to that angle.
const REFERENCE_TILT_RADIANS = 25 * Math.PI / 180;
// Desktop reference: a 768 CSS-pixel game is approximately 24 cm wide.
export const DESKTOP_GAME_WIDTH_CM = 24;
const DESKTOP_REFERENCE_CSS_WIDTH = 768;
const MOTION_SLOPE_SCALE = 0.7 / (2 * Math.tan(REFERENCE_TILT_RADIANS));
const IDENTITY_ROTATION = Object.freeze([1, 0, 0, 0, 1, 0, 0, 0, 1]);
const POSE_ROTATION_EPSILON = 1e-6;
const POSE_NORMAL_EPSILON = 1e-12;
const POSE_FULL_CORRECTION_FORWARD_Z = 0.1;

/**
 * Reconcile a measured room pose with the gain-scaled camera ray, returning
 * one proper rotation shared by the rendered camera and room-fixed lighting.
 */
export function resolveRenderedScenePose(view) {
  if (!view || typeof view !== "object") throw new TypeError("Rendered scene view must be an object");
  const { x, y } = view;
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    throw new RangeError("Rendered scene view offsets must be finite numbers");
  }
  const rawRotation = view.rotation ?? IDENTITY_ROTATION;
  validatePoseRotation(rawRotation);
  const rotation = Array.from(rawRotation);
  const result = { x, y };

  // Preserve the established safe handling of folded/back-facing sensor poses.
  if (rotation[8] <= 0) {
    result.rotation = Object.freeze(rotation);
    return Object.freeze(result);
  }

  const eyeMeasured = normalizePoseVector([-rotation[2], -rotation[5], -rotation[8]]);
  const eyeRendered = normalizePoseVector([
    x / MOTION_SLOPE_SCALE,
    y / MOTION_SLOPE_SCALE,
    -1,
  ]);
  const difference = Math.hypot(
    eyeMeasured[0] - eyeRendered[0],
    eyeMeasured[1] - eyeRendered[1],
    eyeMeasured[2] - eyeRendered[2],
  );
  if (difference <= POSE_NORMAL_EPSILON) {
    result.rotation = Object.freeze(rotation);
    return Object.freeze(result);
  }

  const cross = [
    eyeMeasured[1] * eyeRendered[2] - eyeMeasured[2] * eyeRendered[1],
    eyeMeasured[2] * eyeRendered[0] - eyeMeasured[0] * eyeRendered[2],
    eyeMeasured[0] * eyeRendered[1] - eyeMeasured[1] * eyeRendered[0],
  ];
  const dot = Math.max(-1, Math.min(1,
    eyeMeasured[0] * eyeRendered[0]
      + eyeMeasured[1] * eyeRendered[1]
      + eyeMeasured[2] * eyeRendered[2]));
  const crossLength = Math.hypot(...cross);
  const correctionWeight = smoothstep(Math.max(0, Math.min(1,
    rotation[8] / POSE_FULL_CORRECTION_FORWARD_Z)));
  if (correctionWeight === 0) {
    result.rotation = Object.freeze(rotation);
    return Object.freeze(result);
  }
  const correction = shortestArcCorrection(cross, crossLength, dot, correctionWeight);

  const renderedRotation = Array(9);
  for (let row = 0; row < 3; row += 1) {
    for (let column = 0; column < 3; column += 1) {
      renderedRotation[row * 3 + column] = correction[row * 3] * rotation[column]
        + correction[row * 3 + 1] * rotation[3 + column]
        + correction[row * 3 + 2] * rotation[6 + column];
    }
  }
  if (!renderedRotation.every(Number.isFinite)) {
    throw new RangeError("Rendered scene rotation must remain finite");
  }
  result.rotation = Object.freeze(renderedRotation);
  return Object.freeze(result);
}

function shortestArcCorrection(cross, crossLength, dot, weight) {
  if (!(crossLength > 0) || !Number.isFinite(crossLength)) {
    throw new RangeError("Rendered camera direction cannot be aligned to the measured pose");
  }
  const axis = cross.map(component => component / crossLength);
  const angle = Math.atan2(crossLength, dot) * weight;
  const sine = Math.sin(angle);
  const oneMinusCosine = 1 - Math.cos(angle);
  const skew = [
    0, -axis[2], axis[1],
    axis[2], 0, -axis[0],
    -axis[1], axis[0], 0,
  ];
  const correction = Array(9);
  for (let row = 0; row < 3; row += 1) {
    for (let column = 0; column < 3; column += 1) {
      let square = 0;
      for (let inner = 0; inner < 3; inner += 1) {
        square += skew[row * 3 + inner] * skew[inner * 3 + column];
      }
      correction[row * 3 + column] = (row === column ? 1 : 0)
        + sine * skew[row * 3 + column] + oneMinusCosine * square;
    }
  }
  return correction;
}

function smoothstep(value) {
  return value * value * (3 - 2 * value);
}

function normalizePoseVector(vector) {
  const length = Math.hypot(...vector);
  if (!(length > 0) || !Number.isFinite(length)) {
    throw new RangeError("Rendered scene direction must be finite and nonzero");
  }
  return vector.map(component => component / length);
}

function validatePoseRotation(rotation) {
  if ((!Array.isArray(rotation) && !ArrayBuffer.isView(rotation)) || rotation.length !== 9
    || rotation instanceof DataView || !Array.from(rotation).every(Number.isFinite)) {
    throw new TypeError("Rendered scene rotation must be a finite 3×3 row-major matrix");
  }
  const matrix = Array.from(rotation);
  for (let rowA = 0; rowA < 3; rowA += 1) {
    for (let rowB = rowA; rowB < 3; rowB += 1) {
      const dot = matrix[rowA * 3] * matrix[rowB * 3]
        + matrix[rowA * 3 + 1] * matrix[rowB * 3 + 1]
        + matrix[rowA * 3 + 2] * matrix[rowB * 3 + 2];
      const expected = rowA === rowB ? 1 : 0;
      if (Math.abs(dot - expected) > POSE_ROTATION_EPSILON) {
        throw new RangeError("Rendered scene rotation must be orthonormal");
      }
    }
  }
  const determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7])
    - matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6])
    + matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  if (Math.abs(determinant - 1) > POSE_ROTATION_EPSILON) {
    throw new RangeError("Rendered scene rotation must be a proper rotation with determinant +1");
  }
}

/** A fixed box pose whose vanishing point stays inside the aperture at supported viewing distances. */
export function computeStationaryView({ gameWidthCm, width = NATIVE_WIDTH, height = 240 } = {}) {
  // The pose is fixed at the 35 cm reference. Estimating a different eye
  // distance changes camera perspective, never the box orientation or lamp.
  const centered = computeViewingCamera({ distanceCm: 35, gameWidthCm, width, height });
  const radius = centered.focalDepth * SCENE_DEPTH_TO_NATIVE_PIXELS;
  const eyeX = width * 0.15, eyeY = height * 0.10;
  const forwardSquared = radius * radius - eyeX * eyeX - eyeY * eyeY;
  if (forwardSquared <= 0) throw new RangeError("Stationary camera is outside the supported scene bounds");
  const eyeZ = Math.sqrt(forwardSquared);
  const slopeX = eyeX / eyeZ, slopeY = eyeY / eyeZ;
  const yaw = Math.atan(slopeX), pitch = Math.atan(slopeY * Math.cos(yaw));
  const ch = Math.cos(yaw), sh = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch);
  return Object.freeze({
    x: slopeX * MOTION_SLOPE_SCALE,
    y: slopeY * MOTION_SLOPE_SCALE,
    rotation: Object.freeze([ch, -sh * sp, -sh * cp, 0, cp, -sp, sh, ch * sp, ch * cp]),
  });
}

const profile = (id, label, shortWidthCm, defaultDistanceCm) => Object.freeze({
  id, label, shortWidthCm, defaultDistanceCm,
});

export const SCREEN_PROFILES = Object.freeze({
  phone: profile("phone", "Phone", 6.7, 35),
  "large-phone": profile("large-phone", "Large phone", 7.3, 35),
  "tablet-11": profile("tablet-11", "11-inch tablet", 16.05, 50),
  "tablet-13": profile("tablet-13", "13-inch tablet", 19.86, 50),
  desktop: profile("desktop", "Desktop reference", null, 35),
});

/**
 * Resolve an explicit profile or estimate a broad device class for Auto.
 * Auto uses only coarse screen-size bands; it does not identify device models.
 */
export function resolveScreenProfile(selection = "auto", { screenWidth, screenHeight, coarsePointer = false } = {}) {
  if (typeof selection !== "string") throw new TypeError("Screen profile selection must be a string");
  if (typeof coarsePointer !== "boolean") throw new TypeError("coarsePointer must be a boolean");
  const normalized = selection.toLowerCase();
  if (normalized !== "auto") {
    const selected = SCREEN_PROFILES[selection];
    if (!selected) throw new RangeError(`Unknown screen profile: ${selection}`);
    return selected;
  }

  if (!coarsePointer || screenWidth === undefined || screenHeight === undefined) return SCREEN_PROFILES.desktop;
  validateScreenDimensions(screenWidth, screenHeight);
  const shortSide = Math.min(screenWidth, screenHeight);
  if (shortSide < 600) return SCREEN_PROFILES.phone;
  if (shortSide < 950) return SCREEN_PROFILES["tablet-11"];
  return SCREEN_PROFILES["tablet-13"];
}

/** Estimate the physical width of the visible game stage from device width. */
export function estimateGameWidthCm({ profile: selectedProfile, stageCssWidth, screenWidth, screenHeight, viewportScale = 1 } = {}) {
  if (!selectedProfile || typeof selectedProfile !== "object") {
    throw new TypeError("A resolved screen profile is required");
  }

  if (selectedProfile.shortWidthCm !== null && (!Number.isFinite(selectedProfile.shortWidthCm) || selectedProfile.shortWidthCm <= 0)) {
    throw new RangeError("Profile short width must be positive and finite");
  }
  if (!Number.isFinite(stageCssWidth) || stageCssWidth <= 0) {
    throw new RangeError("Stage CSS width must be positive and finite");
  }
  validateScreenDimensions(screenWidth, screenHeight);
  if (!Number.isFinite(viewportScale) || viewportScale <= 0) {
    throw new RangeError("Viewport scale must be positive and finite");
  }
  if (selectedProfile.shortWidthCm === null) {
    return DESKTOP_GAME_WIDTH_CM * stageCssWidth / DESKTOP_REFERENCE_CSS_WIDTH * viewportScale;
  }
  return selectedProfile.shortWidthCm * stageCssWidth / Math.min(screenWidth, screenHeight) * viewportScale;
}

/** Convert viewing distance, visible width, and phone motion to camera state. */
export function computeViewingCamera({ distanceCm, gameWidthCm, motionX = 0, motionY = 0, width = NATIVE_WIDTH, height = 240 } = {}) {
  if (!Number.isFinite(distanceCm) || distanceCm < MIN_DISTANCE_CM || distanceCm > MAX_DISTANCE_CM) {
    throw new RangeError(`Viewing distance must be from ${MIN_DISTANCE_CM} to ${MAX_DISTANCE_CM} cm`);
  }
  if (!Number.isFinite(gameWidthCm) || gameWidthCm <= 0) {
    throw new RangeError("Visible game width must be positive and finite");
  }
  if (!Number.isFinite(motionX) || !Number.isFinite(motionY)) {
    throw new RangeError("Motion offsets must be finite numbers");
  }
  validateNativeDimensions(width, height);

  const focalLengthNative = distanceCm / gameWidthCm * NATIVE_WIDTH;
  const motionScale = 2 * Math.tan(REFERENCE_TILT_RADIANS) / 0.7;
  const slopeX = motionX * motionScale;
  const slopeY = motionY * motionScale;
  const normalLength = Math.hypot(slopeX, slopeY, 1);
  const nx = slopeX / normalLength;
  const ny = slopeY / normalLength;
  const nz = 1 / normalLength;
  const camera = {
    x: focalLengthNative * nx / width,
    y: focalLengthNative * ny / height,
    focalDepth: focalLengthNative * nz / SCENE_DEPTH_TO_NATIVE_PIXELS,
  };
  if (!Number.isFinite(camera.x) || !Number.isFinite(camera.y)
    || Math.abs(camera.x) > MAX_VIEW_OFFSET || Math.abs(camera.y) > MAX_VIEW_OFFSET
    || !Number.isFinite(camera.focalDepth) || camera.focalDepth < MIN_FOCAL_DEPTH
    || camera.focalDepth > MAX_FOCAL_DEPTH) {
    throw new RangeError("Viewing camera is outside the supported scene bounds");
  }
  return camera;
}

function validateScreenDimensions(width, height) {
  if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0) {
    throw new RangeError("Screen width and height must be positive finite numbers");
  }
}

function validateNativeDimensions(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("Native dimensions must be positive integers");
  }
}
