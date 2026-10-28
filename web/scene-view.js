const MAX_VIEW_OFFSET = 64;
const DEFAULT_FOCAL_DEPTH = 1 / 0.015;
const MIN_FOCAL_DEPTH = 0.1;
const MAX_FOCAL_DEPTH = 10_000;

let sceneView = Object.freeze({ x: 0, y: 0 });
let sceneCameraDepth = DEFAULT_FOCAL_DEPTH;
let sceneViewRevision = 0;

/** Set the off-axis camera center and scene focal depth atomically. */
export function setSceneCamera(x, y, focalDepth) {
  validateOffset(x, "Horizontal");
  validateOffset(y, "Vertical");
  validateFocalDepth(focalDepth);
  if (sceneView.x === x && sceneView.y === y && sceneCameraDepth === focalDepth) return false;
  sceneView = Object.freeze({ x, y });
  sceneCameraDepth = focalDepth;
  sceneViewRevision += 1;
  return true;
}

/** Set the off-axis camera center while preserving its calibrated focal depth. */
export function setSceneView(x, y) {
  return setSceneCamera(x, y, sceneCameraDepth);
}

/** Return the immutable normalized off-axis view. */
export function getSceneView() {
  return sceneView;
}

/** Return the monotonic view revision used by render caches. */
export function getSceneViewRevision() {
  return sceneViewRevision;
}

/** Return the scene-space distance from the view plane to the camera center. */
export function getSceneCameraDepth() {
  return sceneCameraDepth;
}

/** Convert the normalized view into its pixel-space vanishing point. */
export function getSceneVanishingPoint(width = 256, height = 240) {
  validateDimensions(width, height);
  return { x: width * (0.5 + sceneView.x), y: height * (0.5 + sceneView.y) };
}

/** Return the pixel-space offset from the centered view. */
export function getSceneViewOffset(width = 256, height = 240) {
  validateDimensions(width, height);
  return { x: width * sceneView.x, y: height * sceneView.y };
}

function validateOffset(value, axis) {
  if (!Number.isFinite(value) || value < -MAX_VIEW_OFFSET || value > MAX_VIEW_OFFSET) {
    throw new RangeError(`${axis} scene view offset must be finite and from -${MAX_VIEW_OFFSET} to ${MAX_VIEW_OFFSET}`);
  }
}

function validateFocalDepth(value) {
  if (!Number.isFinite(value) || value < MIN_FOCAL_DEPTH || value > MAX_FOCAL_DEPTH) {
    throw new RangeError(`Scene camera focal depth must be from ${MIN_FOCAL_DEPTH} to ${MAX_FOCAL_DEPTH}`);
  }
}

function validateDimensions(width, height) {
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
    throw new RangeError("View dimensions must be positive integers");
  }
}
