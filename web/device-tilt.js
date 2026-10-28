const DEGREES_TO_RADIANS = Math.PI / 180;
const REFERENCE_TILT_DEGREES = 25;
const MAX_CAMERA_TILT_DEGREES = 75;
const MAX_LOOK_POLAR_DEGREES = 60;
const MAX_VIEW_OFFSET = 0.7;
const SMOOTHING_MS = 90;
const DEADZONE = 0.001;
const MAX_EMIT_RATE = 30;
const MIN_EMIT_INTERVAL_MS = 1000 / MAX_EMIT_RATE;
const RECENTER_FRESH_MS = 250;
const NO_DATA_TIMEOUT_MS = 3000;
const ANTIPODAL_DOT_THRESHOLD = 1e-6;
const PARALLEL_VECTOR_EPSILON = 1e-12;
const ANTIPODAL_ENTER_DEGREES = 1;
const ANTIPODAL_EXIT_DEGREES = 2;
const IDENTITY_ROTATION = Object.freeze([1, 0, 0, 0, 1, 0, 0, 0, 1]);

const STATUS_MESSAGES = Object.freeze({
  off: "Tilt is off.",
  requesting: "Allow motion access to use tilt.",
  calibrating: "Hold the device in your preferred neutral position.",
  active: "Tilt control is active.",
  paused: "Tilt control is paused.",
  insecure: "Tilt control requires a secure connection.",
  unsupported: "Device tilt is not available in this browser.",
  denied: "Motion access was denied.",
  unavailable: "No motion data received. Check the device sensor.",
  error: "Could not start tilt control.",
});

/** W3C device-orientation Z-X-Y rotation matrix, or null for incomplete data. */
export function orientationMatrix(orientation) {
  const { alpha, beta, gamma } = orientation ?? {};
  if (![alpha, beta, gamma].every(value => Number.isFinite(value))) return null;

  const betaRadians = beta * DEGREES_TO_RADIANS;
  const gammaRadians = gamma * DEGREES_TO_RADIANS;
  const alphaRadians = alpha * DEGREES_TO_RADIANS;
  const cX = Math.cos(betaRadians);
  const cY = Math.cos(gammaRadians);
  const cZ = Math.cos(alphaRadians);
  const sX = Math.sin(betaRadians);
  const sY = Math.sin(gammaRadians);
  const sZ = Math.sin(alphaRadians);

  return [
    cZ * cY - sZ * sX * sY, -cX * sZ, cY * sZ * sX + cZ * sY,
    cY * sZ + cZ * sX * sY, cZ * cX, sZ * sY - cZ * cY * sX,
    -cX * sY, sX, cX * cY,
  ];
}

/** Relative gravity-frame pose and camera offsets in the calibrated screen axes. */
export function relativeTiltPose(
  baseline,
  current,
  baselineScreenAngle = 0,
  currentScreenAngle = baselineScreenAngle,
  strength = 50,
) {
  validateStrength(strength);
  if (!Number.isFinite(baselineScreenAngle) || !Number.isFinite(currentScreenAngle)) {
    throw new TypeError("Screen angles must be finite");
  }
  const baselineGravity = gravityDeviceVector(baseline);
  const currentGravity = gravityDeviceVector(current);
  if (!baselineGravity || !currentGravity) return null;
  return relativeTiltPoseFromGravity(
    baselineGravity,
    currentGravity,
    baselineScreenAngle,
    currentScreenAngle,
    strength,
  );
}

/** Full look-relative pose; missing alpha on either sample falls back to gravity tracking. */
export function relativeLookPose(
  baseline,
  current,
  baselineScreenAngle = 0,
  currentScreenAngle = baselineScreenAngle,
  strength = 50,
) {
  validateStrength(strength);
  if (!Number.isFinite(baselineScreenAngle) || !Number.isFinite(currentScreenAngle)) {
    throw new TypeError("Screen angles must be finite");
  }
  const baselineMatrix = orientationMatrix(baseline);
  const currentMatrix = orientationMatrix(current);
  if (baselineMatrix && currentMatrix) {
    return relativeLookPoseFromMatrices(
      baselineMatrix,
      currentMatrix,
      baselineScreenAngle,
      currentScreenAngle,
      strength,
    );
  }
  const baselineGravity = gravityDeviceVector(baseline);
  const currentGravity = gravityDeviceVector(current);
  if (!baselineGravity || !currentGravity) return null;
  return relativeTiltPoseFromGravity(
    baselineGravity,
    currentGravity,
    baselineScreenAngle,
    currentScreenAngle,
    strength,
  );
}

/** Compatibility helper returning only camera offsets. */
export function relativeTiltView(baseline, current, screenAngle = 0, strength = 50) {
  const pose = relativeTiltPose(baseline, current, screenAngle, screenAngle, strength);
  return pose ? { x: pose.x, y: pose.y } : null;
}

function gravityDeviceVector(orientation) {
  const { alpha, beta, gamma } = orientation ?? {};
  if (!Number.isFinite(beta) || !Number.isFinite(gamma)) return null;
  // The third row of R (equivalently R^T * world-up) is independent of alpha.
  // Use alpha=0 only when absent; orientationMatrix remains strict for callers needing full R.
  const matrix = orientationMatrix({ alpha: Number.isFinite(alpha) ? alpha : 0, beta, gamma });
  return matrix ? normalizeVector(matrix.slice(6, 9)) : null;
}

function relativeTiltPoseFromGravity(baselineGravityDevice, currentGravityDevice, baselineScreenAngle, currentScreenAngle, strength) {
  const baselineAxis = screenToDeviceRotation(baselineScreenAngle);
  const currentAxis = screenToDeviceRotation(currentScreenAngle);
  const baselineGravity = normalizeVector(multiplyVector(baselineAxis, baselineGravityDevice));
  const currentGravity = normalizeVector(multiplyVector(currentAxis, currentGravityDevice));
  const rotation = shortestGravityRotation(baselineGravity, currentGravity);
  const view = cameraViewFromRotation(rotation, strength);
  return { ...view, rotation: Object.freeze(rotation) };
}

function relativeLookPoseFromMatrices(baselineMatrix, currentMatrix, baselineScreenAngle, currentScreenAngle, strength) {
  const baselineAxis = screenToDeviceRotation(baselineScreenAngle);
  const currentAxis = screenToDeviceRotation(currentScreenAngle);
  const relativeDevice = multiplyMatrices(transposeMatrix(currentMatrix), baselineMatrix);
  const rotation = multiplyMatrices(currentAxis, multiplyMatrices(relativeDevice, baselineAxis));
  const view = cameraViewFromRotation(rotation, strength, "look");
  return { ...view, rotation: Object.freeze(rotation) };
}

function shortestGravityRotation(from, to) {
  const dot = clamp(from[0] * to[0] + from[1] * to[1] + from[2] * to[2], -1, 1);
  const cross = [
    from[1] * to[2] - from[2] * to[1],
    from[2] * to[0] - from[0] * to[2],
    from[0] * to[1] - from[1] * to[0],
  ];
  const crossLength = Math.hypot(...cross);
  if (dot > 0 && crossLength < PARALLEL_VECTOR_EPSILON) return [...IDENTITY_ROTATION];

  // There is no unique swing axis at exactly opposite gravity. Use a stable
  // baseline-derived perpendicular axis throughout a small antipodal cone so
  // sensor noise cannot flip the fixture by 180 degrees between samples.
  if (dot < -1 + ANTIPODAL_DOT_THRESHOLD) {
    const axis = deterministicPerpendicular(from);
    return quaternionToMatrix([axis[0], axis[1], axis[2], 0]);
  }

  return quaternionToMatrix(normalizeQuaternion([cross[0], cross[1], cross[2], 1 + dot]));
}

function deterministicPerpendicular(vector) {
  const absolute = vector.map(Math.abs);
  const basis = absolute[0] <= absolute[1] && absolute[0] <= absolute[2]
    ? [1, 0, 0]
    : absolute[1] <= absolute[2] ? [0, 1, 0] : [0, 0, 1];
  return normalizeVector([
    vector[1] * basis[2] - vector[2] * basis[1],
    vector[2] * basis[0] - vector[0] * basis[2],
    vector[0] * basis[1] - vector[1] * basis[0],
  ]);
}

function normalizeVector(vector) {
  const magnitude = Math.hypot(...vector);
  if (!(magnitude > 0) || !Number.isFinite(magnitude)) return [0, 0, 1];
  return vector.map(value => value / magnitude);
}

function multiplyVector(matrix, vector) {
  return [
    matrix[0] * vector[0] + matrix[1] * vector[1] + matrix[2] * vector[2],
    matrix[3] * vector[0] + matrix[4] * vector[1] + matrix[5] * vector[2],
    matrix[6] * vector[0] + matrix[7] * vector[1] + matrix[8] * vector[2],
  ];
}

function transposeMatrix(matrix) {
  return [matrix[0], matrix[3], matrix[6], matrix[1], matrix[4], matrix[7], matrix[2], matrix[5], matrix[8]];
}

function multiplyMatrices(left, right) {
  const result = new Array(9);
  for (let row = 0; row < 3; row += 1) {
    for (let column = 0; column < 3; column += 1) {
      result[row * 3 + column] = left[row * 3] * right[column]
        + left[row * 3 + 1] * right[3 + column]
        + left[row * 3 + 2] * right[6 + column];
    }
  }
  return result;
}

function cameraViewFromRotation(rotation, strength, trackingMode = "gravity") {
  if (trackingMode === "look") return cameraViewFromLookRotation(rotation, strength);
  // The camera looks along -z. Express its horizontal and vertical slopes from
  // the very same room pose that is sent to the scene-lighting consumer.
  const angleLimit = MAX_CAMERA_TILT_DEGREES * DEGREES_TO_RADIANS;
  const angleX = clamp(Math.atan2(-rotation[2], rotation[8]), -angleLimit, angleLimit);
  const angleY = clamp(Math.atan2(-rotation[5], rotation[8]), -angleLimit, angleLimit);
  const maxOffset = MAX_VIEW_OFFSET * strength / 100;
  const referenceAngle = REFERENCE_TILT_DEGREES * DEGREES_TO_RADIANS;
  return {
    x: Math.tan(angleX) / Math.tan(referenceAngle) * maxOffset,
    y: Math.tan(angleY) / Math.tan(referenceAngle) * maxOffset,
  };
}

function cameraViewFromLookRotation(rotation, strength) {
  // Map the look normal to a finite forward camera ray. Front-facing rays up
  // to 60 degrees are exact; the back hemisphere folds smoothly to neutral at
  // the back pole while preserving lateral azimuth. M itself remains intact.
  const normalX = -rotation[2];
  const normalY = -rotation[5];
  const normalZ = rotation[8];
  const lateralLength = Math.hypot(normalX, normalY);
  const maxPolarAngle = MAX_LOOK_POLAR_DEGREES * DEGREES_TO_RADIANS;
  const denominator = Math.max(Math.abs(normalZ), lateralLength / Math.tan(maxPolarAngle));
  const slopeX = normalX / denominator;
  const slopeY = normalY / denominator;
  const maxOffset = MAX_VIEW_OFFSET * strength / 100;
  const referenceAngle = REFERENCE_TILT_DEGREES * DEGREES_TO_RADIANS;
  return {
    x: slopeX / Math.tan(referenceAngle) * maxOffset,
    y: slopeY / Math.tan(referenceAngle) * maxOffset,
  };
}

function screenToDeviceRotation(screenAngle) {
  const angle = normalizeAngle(screenAngle) * DEGREES_TO_RADIANS;
  const cosAngle = Math.cos(angle);
  const sinAngle = Math.sin(angle);
  return [cosAngle, sinAngle, 0, sinAngle, -cosAngle, 0, 0, 0, -1];
}

function matrixToQuaternion(matrix) {
  const trace = matrix[0] + matrix[4] + matrix[8];
  let x, y, z, w;
  if (trace > 0) {
    const scale = Math.sqrt(trace + 1) * 2;
    w = 0.25 * scale;
    x = (matrix[7] - matrix[5]) / scale;
    y = (matrix[2] - matrix[6]) / scale;
    z = (matrix[3] - matrix[1]) / scale;
  } else if (matrix[0] > matrix[4] && matrix[0] > matrix[8]) {
    const scale = Math.sqrt(1 + matrix[0] - matrix[4] - matrix[8]) * 2;
    w = (matrix[7] - matrix[5]) / scale;
    x = 0.25 * scale;
    y = (matrix[1] + matrix[3]) / scale;
    z = (matrix[2] + matrix[6]) / scale;
  } else if (matrix[4] > matrix[8]) {
    const scale = Math.sqrt(1 + matrix[4] - matrix[0] - matrix[8]) * 2;
    w = (matrix[2] - matrix[6]) / scale;
    x = (matrix[1] + matrix[3]) / scale;
    y = 0.25 * scale;
    z = (matrix[5] + matrix[7]) / scale;
  } else {
    const scale = Math.sqrt(1 + matrix[8] - matrix[0] - matrix[4]) * 2;
    w = (matrix[3] - matrix[1]) / scale;
    x = (matrix[2] + matrix[6]) / scale;
    y = (matrix[5] + matrix[7]) / scale;
    z = 0.25 * scale;
  }
  return normalizeQuaternion([x, y, z, w]);
}

function quaternionToMatrix([x, y, z, w]) {
  const xx = x * x, yy = y * y, zz = z * z;
  const xy = x * y, xz = x * z, yz = y * z;
  const wx = w * x, wy = w * y, wz = w * z;
  return [
    1 - 2 * (yy + zz), 2 * (xy - wz), 2 * (xz + wy),
    2 * (xy + wz), 1 - 2 * (xx + zz), 2 * (yz - wx),
    2 * (xz - wy), 2 * (yz + wx), 1 - 2 * (xx + yy),
  ];
}

function normalizeQuaternion(quaternion) {
  const magnitude = Math.hypot(...quaternion);
  if (!(magnitude > 0)) return [0, 0, 0, 1];
  return quaternion.map(value => value / magnitude);
}

function slerpQuaternion(from, to, amount) {
  let target = to;
  let dot = from.reduce((sum, value, index) => sum + value * target[index], 0);
  if (dot < 0) {
    target = target.map(value => -value);
    dot = -dot;
  }
  if (dot > 0.9995) {
    return normalizeQuaternion(from.map((value, index) => value + (target[index] - value) * amount));
  }
  const angle = Math.acos(clamp(dot, -1, 1));
  const sinAngle = Math.sin(angle);
  const fromWeight = Math.sin((1 - amount) * angle) / sinAngle;
  const toWeight = Math.sin(amount * angle) / sinAngle;
  return normalizeQuaternion(from.map((value, index) => value * fromWeight + target[index] * toWeight));
}

function quaternionDistance(left, right) {
  const dot = Math.abs(left.reduce((sum, value, index) => sum + value * right[index], 0));
  return 2 * Math.acos(clamp(dot, -1, 1));
}

function rotationDistance(left, right) {
  return quaternionDistance(matrixToQuaternion(left), matrixToQuaternion(right));
}

/** Opt-in tilt with gravity-stable or full look-relative tracking. */
export class DeviceTiltController {
  constructor({ onView = () => {}, onStatus = () => {}, window = globalThis.window, document = globalThis.document, trackingMode = "gravity" } = {}) {
    this._trackingMode = validateTrackingMode(trackingMode);
    this.window = window ?? null;
    this.document = document ?? null;
    this.onView = typeof onView === "function" ? onView : () => {};
    this.onStatus = typeof onStatus === "function" ? onStatus : () => {};

    this._enabled = false;
    this._disposed = false;
    this._userPaused = false;
    this._hidden = false;
    this._strength = 50;
    this._generation = 0;
    this._startPromise = null;
    this._state = null;
    this._baselineGravity = null;
    this._baselineMatrix = null;
    this._baselineScreenAngle = null;
    this._nearAntipode = false;
    this._needsCalibration = false;
    this._latestReading = null;
    this._latestReadingAt = -Infinity;
    this._view = neutralPose(this._trackingMode);
    this._target = neutralPose(this._trackingMode);
    this._lastEmitted = neutralPose();
    this._lastEmitAt = -Infinity;
    this._lastFrameAt = null;
    this._frameHandle = null;
    this._noDataTimer = null;
    this._listenersAttached = false;

    this._handleOrientation = event => this._receiveOrientation(event);
    this._handleVisibility = () => this._receiveVisibility();
    this._handleScreenOrientation = () => this._receiveScreenOrientation();
    this._publishStatus("off");
  }

  get enabled() {
    return this._enabled;
  }

  get trackingMode() {
    return this._trackingMode;
  }

  /** Call directly from a user gesture so a browser permission prompt can open. */
  start() {
    if (this._disposed) return Promise.resolve(false);
    if (this._enabled) return this._startPromise ?? Promise.resolve(true);

    const host = this.window;
    if (!host || host.isSecureContext !== true) {
      this._publishStatus("insecure");
      return Promise.resolve(false);
    }
    const orientationEvent = host.DeviceOrientationEvent;
    if (!orientationEvent || typeof host.addEventListener !== "function") {
      this._publishStatus("unsupported");
      return Promise.resolve(false);
    }

    this._enabled = true;
    this._userPaused = false;
    this._hidden = this._isDocumentHidden();
    this._needsCalibration = true;
    this._baselineGravity = null;
    this._baselineMatrix = null;
    this._baselineScreenAngle = null;
    this._nearAntipode = false;
    this._latestReading = null;
    this._latestReadingAt = -Infinity;
    const generation = ++this._generation;
    this._publishStatus("requesting");

    // This call stays synchronous inside start(), before any promise continuation.
    let permission;
    try {
      permission = typeof orientationEvent.requestPermission === "function"
        ? orientationEvent.requestPermission.call(orientationEvent)
        : "granted";
    } catch (error) {
      this._failStart(isPermissionDenial(error) ? "denied" : "error");
      return Promise.resolve(false);
    }

    this._startPromise = Promise.resolve(permission).then(result => {
      if (!this._isCurrentStart(generation)) return false;
      if (result !== "granted") {
        this._failStart("denied");
        return false;
      }

      this._attachListeners();
      this._hidden = this._isDocumentHidden();
      if (this._isPaused()) {
        this._publishStatus("paused");
      } else {
        this._publishStatus("calibrating");
        this._armNoDataTimeout(generation);
      }
      return true;
    }).catch(error => {
      if (!this._isCurrentStart(generation)) return false;
      this._failStart(isPermissionDenial(error) ? "denied" : "error");
      return false;
    });
    return this._startPromise;
  }

  stop() {
    this._generation += 1;
    const shouldNeutralize = !isNeutralPose(this._lastEmitted);
    this._enabled = false;
    this._userPaused = false;
    this._hidden = false;
    this._startPromise = null;
    this._removeListeners();
    this._clearNoDataTimeout();
    this._cancelFrame();
    this._baselineGravity = null;
    this._baselineMatrix = null;
    this._baselineScreenAngle = null;
    this._nearAntipode = false;
    this._needsCalibration = false;
    this._latestReading = null;
    this._latestReadingAt = -Infinity;
    this._view = neutralPose(this._trackingMode);
    this._target = neutralPose(this._trackingMode);
    this._lastFrameAt = null;
    if (shouldNeutralize) this._publishView(this._view, this._now(), true);
    this._publishStatus("off");
  }

  /** Recalibrate from the latest fresh reading, or from the next valid sample. */
  recenter() {
    if (!this._enabled || this._isPaused()) return false;
    const now = this._now();
    if (this._latestReading && now - this._latestReadingAt <= RECENTER_FRESH_MS) {
      if (this._captureCalibration(this._latestReading)) {
        this._clearNoDataTimeout();
        this._resetView(neutralPose(this._trackingMode), now, true);
        this._publishStatus("active");
        return true;
      }
    }
    this._resetView(neutralPose(this._trackingMode), now, true);
    this._beginCalibration(true);
    return false;
  }

  setStrength(percent) {
    validateStrength(percent);
    this._strength = percent;
    if (!this._enabled || this._isPaused() || !this._baselineGravity || !this._latestReading) return;
    this._updateTarget(this._latestReading);
  }

  setTrackingMode(mode) {
    const validatedMode = validateTrackingMode(mode);
    if (validatedMode === this._trackingMode) return true;
    this._trackingMode = validatedMode;
    if (this._enabled && this._listenersAttached && !this._isPaused()) this.recenter();
    return true;
  }

  setPaused(paused) {
    if (typeof paused !== "boolean") throw new TypeError("Paused must be a boolean");
    if (!this._enabled) return false;
    this._userPaused = paused;
    if (!this._listenersAttached) {
      this._publishStatus(this._isPaused() ? "paused" : "requesting");
      return true;
    }
    if (this._isPaused()) {
      this._enterPausedState();
      return true;
    }
    if (this._state === "paused") this._beginCalibration(true);
    return true;
  }

  dispose() {
    if (this._disposed) return;
    this.stop();
    this._disposed = true;
  }

  _receiveOrientation(event) {
    if (!this._enabled || this._isPaused()) return;
    const reading = validReading(event);
    if (!reading) return;
    this._latestReading = reading;
    this._latestReadingAt = this._now();

    if (this._needsCalibration || !this._baselineGravity) {
      if (!this._captureCalibration(reading)) return;
      this._clearNoDataTimeout();
      this._resetView(neutralPose(this._trackingMode), this._latestReadingAt, true);
      this._publishStatus("active");
      return;
    }
    this._updateTarget(reading);
  }

  _captureCalibration(reading) {
    const gravity = gravityDeviceVector(reading);
    if (!gravity) return false;
    const screenAngle = readScreenAngle(this.window);
    this._baselineGravity = gravity;
    this._baselineMatrix = orientationMatrix(reading);
    this._baselineScreenAngle = screenAngle;
    this._nearAntipode = false;
    this._needsCalibration = false;
    return true;
  }

  _updateTarget(reading) {
    const currentGravity = gravityDeviceVector(reading);
    if (!currentGravity) return;
    const currentMatrix = orientationMatrix(reading);
    const useLook = this._trackingMode === "look" && this._baselineMatrix && currentMatrix;
    const currentScreenAngle = readScreenAngle(this.window);
    if (!useLook && this._isAntipodalAmbiguous(currentGravity, currentScreenAngle)) return;
    const next = useLook
      ? relativeLookPoseFromMatrices(
        this._baselineMatrix,
        currentMatrix,
        this._baselineScreenAngle,
        currentScreenAngle,
        this._strength,
      )
      : relativeTiltPoseFromGravity(
        this._baselineGravity,
        currentGravity,
        this._baselineScreenAngle,
        currentScreenAngle,
        this._strength,
      );
    if (!next) return;
    const nextRotation = [...next.rotation];
    const cameraMode = useLook ? "look" : "gravity";
    if (Math.abs(next.x - this._target.x) < DEADZONE
      && Math.abs(next.y - this._target.y) < DEADZONE
      && rotationDistance(nextRotation, this._target.rotation) < DEADZONE) {
      this._target.cameraMode = cameraMode;
      return;
    }
    this._target = { x: next.x, y: next.y, rotation: nextRotation, cameraMode };
    this._scheduleFrame();
  }

  _isAntipodalAmbiguous(currentGravity, currentScreenAngle) {
    const baselineScene = normalizeVector(multiplyVector(
      screenToDeviceRotation(this._baselineScreenAngle),
      this._baselineGravity,
    ));
    const currentScene = normalizeVector(multiplyVector(
      screenToDeviceRotation(currentScreenAngle),
      currentGravity,
    ));
    const dot = clamp(baselineScene.reduce((sum, value, index) => sum + value * currentScene[index], 0), -1, 1);
    const antipodalDistance = Math.acos(clamp(-dot, -1, 1)) / DEGREES_TO_RADIANS;

    if (this._nearAntipode) {
      if (antipodalDistance < ANTIPODAL_EXIT_DEGREES) return true;
      this._nearAntipode = false;
      return false;
    }
    if (antipodalDistance <= ANTIPODAL_ENTER_DEGREES) {
      // Upright direction is ambiguous when gravity is inverted relative to
      // calibration. Hold the last valid target instead of following noisy axes.
      this._nearAntipode = true;
      return true;
    }
    return false;
  }

  _scheduleFrame() {
    if (this._frameHandle !== null || !this._enabled || this._isPaused()) return;
    const host = this.window;
    if (typeof host.requestAnimationFrame === "function") {
      this._frameHandle = { kind: "raf", id: host.requestAnimationFrame(time => this._animate(time)) };
    } else {
      this._frameHandle = { kind: "timeout", id: host.setTimeout(() => this._animate(this._now()), MIN_EMIT_INTERVAL_MS) };
    }
  }

  _animate(timestamp) {
    this._frameHandle = null;
    if (!this._enabled || this._isPaused()) return;
    const now = Number.isFinite(timestamp) ? timestamp : this._now();
    const delta = this._lastFrameAt === null ? 16.67 : Math.max(0, now - this._lastFrameAt);
    this._lastFrameAt = now;
    const blend = 1 - Math.exp(-delta / SMOOTHING_MS);
    const currentQuaternion = matrixToQuaternion(this._view.rotation);
    const targetQuaternion = matrixToQuaternion(this._target.rotation);
    const nextQuaternion = slerpQuaternion(currentQuaternion, targetQuaternion, blend);
    const nextRotation = quaternionToMatrix(nextQuaternion);
    const settled = quaternionDistance(nextQuaternion, targetQuaternion) < DEADZONE;
    const rotation = settled ? [...this._target.rotation] : nextRotation;
    const cameraMode = this._target.cameraMode ?? "gravity";
    const camera = cameraViewFromRotation(rotation, this._strength, cameraMode);
    this._view = { ...camera, rotation, cameraMode };
    const changed = Math.abs(this._view.x - this._lastEmitted.x) >= DEADZONE
      || Math.abs(this._view.y - this._lastEmitted.y) >= DEADZONE
      || rotationDistance(this._view.rotation, this._lastEmitted.rotation) >= DEADZONE;
    if (changed && now - this._lastEmitAt >= MIN_EMIT_INTERVAL_MS) {
      this._publishView(this._view, now);
    }
    if (!settled || (changed && now - this._lastEmitAt < MIN_EMIT_INTERVAL_MS)) this._scheduleFrame();
    else this._lastFrameAt = null;
  }

  _resetView(pose, now, emit) {
    this._cancelFrame();
    this._view = clonePose(pose);
    this._target = clonePose(pose);
    this._lastFrameAt = null;
    const changed = !samePose(pose, this._lastEmitted) || !Number.isFinite(this._lastEmitAt);
    if (emit && changed) this._publishView(this._view, now, true);
  }

  _publishView(view, now, immediate = false) {
    // Sensor motion is rate-limited; explicit calibration and stop resets stay immediate.
    if (!immediate && now - this._lastEmitAt < MIN_EMIT_INTERVAL_MS) return;
    this._lastEmitAt = now;
    this._lastEmitted = clonePose(view);
    try {
      this.onView(Object.freeze({ x: view.x, y: view.y, rotation: Object.freeze([...view.rotation]) }));
    } catch {
      // A consumer callback must not stop sensor cleanup or animation.
    }
  }

  _receiveScreenOrientation() {
    if (this._enabled && !this._isPaused()) this._beginCalibration(true);
  }

  _receiveVisibility() {
    if (!this._enabled) return;
    this._hidden = this._isDocumentHidden();
    if (this._isPaused()) {
      this._enterPausedState();
    } else if (this._state === "paused") {
      this._beginCalibration(true);
    }
  }

  _beginCalibration(rearmTimeout) {
    this._baselineGravity = null;
    this._baselineMatrix = null;
    this._baselineScreenAngle = null;
    this._nearAntipode = false;
    this._needsCalibration = true;
    this._latestReading = null;
    this._latestReadingAt = -Infinity;
    this._target = clonePose(this._view);
    this._cancelFrame();
    this._lastFrameAt = null;
    this._publishStatus("calibrating");
    if (rearmTimeout) this._armNoDataTimeout(this._generation);
  }

  _enterPausedState() {
    this._clearNoDataTimeout();
    this._baselineGravity = null;
    this._baselineMatrix = null;
    this._baselineScreenAngle = null;
    this._nearAntipode = false;
    this._needsCalibration = true;
    this._latestReading = null;
    this._latestReadingAt = -Infinity;
    this._target = clonePose(this._view);
    this._cancelFrame();
    this._lastFrameAt = null;
    this._publishStatus("paused");
  }

  _armNoDataTimeout(generation) {
    this._clearNoDataTimeout();
    if (!this._enabled || this._isPaused()) return;
    this._noDataTimer = this.window.setTimeout(() => {
      this._noDataTimer = null;
      if (this._isCurrentStart(generation) && this._needsCalibration) this._failStart("unavailable");
    }, NO_DATA_TIMEOUT_MS);
  }

  _clearNoDataTimeout() {
    if (this._noDataTimer === null) return;
    this.window.clearTimeout(this._noDataTimer);
    this._noDataTimer = null;
  }

  _attachListeners() {
    if (this._listenersAttached) return;
    this.window.addEventListener("deviceorientation", this._handleOrientation);
    this.window.addEventListener("orientationchange", this._handleScreenOrientation);
    this.document?.addEventListener?.("visibilitychange", this._handleVisibility);
    this.window.screen?.orientation?.addEventListener?.("change", this._handleScreenOrientation);
    this._listenersAttached = true;
  }

  _removeListeners() {
    if (!this._listenersAttached) return;
    this.window.removeEventListener("deviceorientation", this._handleOrientation);
    this.window.removeEventListener("orientationchange", this._handleScreenOrientation);
    this.document?.removeEventListener?.("visibilitychange", this._handleVisibility);
    this.window.screen?.orientation?.removeEventListener?.("change", this._handleScreenOrientation);
    this._listenersAttached = false;
  }

  _cancelFrame() {
    if (this._frameHandle === null) return;
    const { kind, id } = this._frameHandle;
    if (kind === "raf") this.window.cancelAnimationFrame?.(id);
    else this.window.clearTimeout?.(id);
    this._frameHandle = null;
  }

  _failStart(state) {
    this._generation += 1;
    const shouldNeutralize = state === "unavailable" && !isNeutralPose(this._lastEmitted);
    this._enabled = false;
    this._userPaused = false;
    this._hidden = false;
    this._startPromise = null;
    this._removeListeners();
    this._clearNoDataTimeout();
    this._cancelFrame();
    this._baselineGravity = null;
    this._baselineMatrix = null;
    this._baselineScreenAngle = null;
    this._nearAntipode = false;
    this._needsCalibration = false;
    this._latestReading = null;
    this._latestReadingAt = -Infinity;
    if (shouldNeutralize) {
      this._view = neutralPose(this._trackingMode);
      this._target = neutralPose(this._trackingMode);
      this._publishView(this._view, this._now(), true);
    } else {
      this._target = clonePose(this._view);
    }
    this._lastFrameAt = null;
    this._publishStatus(state);
  }

  _isCurrentStart(generation) {
    return this._enabled && generation === this._generation;
  }

  _isPaused() {
    return this._userPaused || this._hidden;
  }

  _isDocumentHidden() {
    return this.document?.visibilityState === "hidden" || this.document?.hidden === true;
  }

  _now() {
    const now = this.window?.performance?.now?.();
    return Number.isFinite(now) ? now : Date.now();
  }

  _publishStatus(state) {
    if (state === this._state) return;
    this._state = state;
    try {
      this.onStatus({ state, message: STATUS_MESSAGES[state] });
    } catch {
      // Status presentation is separate from sensor lifecycle.
    }
  }
}

function neutralPose(cameraMode = "gravity") {
  return { x: 0, y: 0, rotation: [...IDENTITY_ROTATION], cameraMode };
}

function clonePose(pose) {
  return { x: pose.x, y: pose.y, rotation: [...pose.rotation], cameraMode: pose.cameraMode ?? "gravity" };
}

function samePose(left, right) {
  return left.x === right.x
    && left.y === right.y
    && left.rotation.every((value, index) => value === right.rotation[index]);
}

function isNeutralPose(pose) {
  return pose.x === 0
    && pose.y === 0
    && pose.rotation.every((value, index) => value === IDENTITY_ROTATION[index]);
}

function validReading(event) {
  const { alpha, beta, gamma } = event ?? {};
  if (!Number.isFinite(beta) || !Number.isFinite(gamma)) return null;
  return { alpha: Number.isFinite(alpha) ? alpha : null, beta, gamma };
}

function readScreenAngle(host) {
  const standardAngle = host?.screen?.orientation?.angle;
  if (Number.isFinite(standardAngle)) return normalizeAngle(standardAngle);
  const legacyAngle = host?.orientation;
  return Number.isFinite(legacyAngle) ? normalizeAngle(legacyAngle) : 0;
}

function normalizeAngle(angle) {
  return ((angle % 360) + 360) % 360;
}

function clamp(value, minimum, maximum) {
  return Math.max(minimum, Math.min(maximum, value));
}

function validateStrength(percent) {
  if (!Number.isInteger(percent) || percent < 0 || percent > 100) {
    throw new RangeError("Tilt strength must be an integer from 0 to 100");
  }
}

function validateTrackingMode(mode) {
  if (mode !== "gravity" && mode !== "look") throw new RangeError("Tracking mode must be 'gravity' or 'look'");
  return mode;
}

function isPermissionDenial(error) {
  return error?.name === "NotAllowedError" || error?.name === "PermissionDeniedError";
}
