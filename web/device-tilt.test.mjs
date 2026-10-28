import test from "node:test";
import assert from "node:assert/strict";
import { DeviceTiltController, orientationMatrix, relativeLookPose, relativeTiltPose, relativeTiltView } from "./device-tilt.js";
import { computeViewingCamera } from "./viewing-model.js";

class FakeEventTarget {
  constructor() { this.listeners = new Map(); }
  addEventListener(type, callback) {
    let callbacks = this.listeners.get(type);
    if (!callbacks) this.listeners.set(type, callbacks = new Set());
    callbacks.add(callback);
  }
  removeEventListener(type, callback) { this.listeners.get(type)?.delete(callback); }
  dispatch(type, event = {}) {
    for (const callback of [...(this.listeners.get(type) ?? [])]) callback({ type, ...event });
  }
  listenerCount(type) { return this.listeners.get(type)?.size ?? 0; }
}

function fakeBrowser({ secure = true, supported = true, permission = () => "granted" } = {}) {
  const window = new FakeEventTarget();
  const document = new FakeEventTarget();
  const clock = { now: 0, nextId: 1, timers: new Map(), frames: new Map() };
  document.visibilityState = "visible";
  window.isSecureContext = secure;
  window.orientation = 0;
  window.screen = { orientation: new FakeEventTarget() };
  window.screen.orientation.angle = 0;
  window.performance = { now: () => clock.now };
  if (supported) {
    window.DeviceOrientationEvent = class DeviceOrientationEvent {};
    window.DeviceOrientationEvent.requestPermission = permission;
  }
  window.setTimeout = (callback, delay) => {
    const id = clock.nextId++;
    clock.timers.set(id, { callback, due: clock.now + delay });
    return id;
  };
  window.clearTimeout = id => clock.timers.delete(id);
  window.requestAnimationFrame = callback => {
    const id = clock.nextId++;
    clock.frames.set(id, callback);
    return id;
  };
  window.cancelAnimationFrame = id => clock.frames.delete(id);

  function advance(milliseconds) {
    const end = clock.now + milliseconds;
    while (true) {
      const due = [...clock.timers.entries()]
        .filter(([, timer]) => timer.due <= end)
        .sort((left, right) => left[1].due - right[1].due)[0];
      if (!due) break;
      const [id, timer] = due;
      clock.timers.delete(id);
      clock.now = timer.due;
      timer.callback();
    }
    clock.now = end;
  }

  function frame(milliseconds = 1000 / 60) {
    advance(milliseconds);
    const callbacks = [...clock.frames.values()];
    clock.frames.clear();
    for (const callback of callbacks) callback(clock.now);
  }

  return { window, document, clock, advance, frame };
}

function near(actual, expected, tolerance = 1e-9) {
  assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} not within ${tolerance} of ${expected}`);
}

function expectedOffset(angleDegrees, strength = 50) {
  const maxOffset = 0.7 * strength / 100;
  const angle = Math.max(-75, Math.min(75, angleDegrees)) * Math.PI / 180;
  return Math.tan(angle) / Math.tan(25 * Math.PI / 180) * maxOffset;
}

function assertMatrixNear(actual, expected, tolerance = 1e-9) {
  assert.equal(actual.length, 9);
  for (let index = 0; index < 9; index += 1) near(actual[index], expected[index], tolerance);
}

function multiply3(left, right) {
  return Array.from({ length: 9 }, (_, index) => {
    const row = Math.floor(index / 3), column = index % 3;
    return left[row * 3] * right[column]
      + left[row * 3 + 1] * right[3 + column]
      + left[row * 3 + 2] * right[6 + column];
  });
}

function transpose3(matrix) {
  return [matrix[0], matrix[3], matrix[6], matrix[1], matrix[4], matrix[7], matrix[2], matrix[5], matrix[8]];
}

function worldYawMatrix(angleDegrees) {
  const angle = angleDegrees * Math.PI / 180;
  const c = Math.cos(angle), s = Math.sin(angle);
  return [c, -s, 0, s, c, 0, 0, 0, 1];
}

function decomposeZxy(matrix) {
  const beta = Math.asin(Math.max(-1, Math.min(1, matrix[7])));
  const cosBeta = Math.cos(beta);
  if (Math.abs(cosBeta) < 1e-8) throw new Error("Test orientation reached the Euler singularity");
  const alpha = Math.atan2(-matrix[1], matrix[4]);
  const gamma = Math.atan2(-matrix[6], matrix[8]);
  return {
    alpha: alpha * 180 / Math.PI,
    beta: beta * 180 / Math.PI,
    gamma: gamma * 180 / Math.PI,
  };
}

function screenAxis(angleDegrees) {
  const angle = angleDegrees * Math.PI / 180;
  const c = Math.cos(angle), s = Math.sin(angle);
  return [c, s, 0, s, -c, 0, 0, 0, -1];
}

function gravityScene(orientation, screenAngle) {
  const deviceUp = orientationMatrix({ ...orientation, alpha: Number.isFinite(orientation.alpha) ? orientation.alpha : 0 }).slice(6, 9);
  const axis = screenAxis(screenAngle);
  const sceneUp = [
    axis[0] * deviceUp[0] + axis[1] * deviceUp[1] + axis[2] * deviceUp[2],
    axis[3] * deviceUp[0] + axis[4] * deviceUp[1] + axis[5] * deviceUp[2],
    axis[6] * deviceUp[0] + axis[7] * deviceUp[1] + axis[8] * deviceUp[2],
  ];
  const magnitude = Math.hypot(...sceneUp);
  return sceneUp.map(value => value / magnitude);
}

function shortestRotation(from, to) {
  const dot = from.reduce((sum, value, index) => sum + value * to[index], 0);
  const cross = [
    from[1] * to[2] - from[2] * to[1],
    from[2] * to[0] - from[0] * to[2],
    from[0] * to[1] - from[1] * to[0],
  ];
  const sine = Math.hypot(...cross);
  if (sine < 1e-12) return dot > 0 ? [1, 0, 0, 0, 1, 0, 0, 0, 1] : null;
  const [x, y, z] = cross.map(value => value / sine);
  const cosine = Math.max(-1, Math.min(1, dot));
  const oneMinusCosine = 1 - cosine;
  return [
    cosine + x * x * oneMinusCosine, x * y * oneMinusCosine - z * sine, x * z * oneMinusCosine + y * sine,
    y * x * oneMinusCosine + z * sine, cosine + y * y * oneMinusCosine, y * z * oneMinusCosine - x * sine,
    z * x * oneMinusCosine - y * sine, z * y * oneMinusCosine + x * sine, cosine + z * z * oneMinusCosine,
  ];
}

function assertRotationMatrix(matrix, tolerance = 1e-9) {
  for (let row = 0; row < 3; row += 1) {
    for (let column = 0; column < 3; column += 1) {
      let dot = 0;
      for (let axis = 0; axis < 3; axis += 1) dot += matrix[row * 3 + axis] * matrix[column * 3 + axis];
      near(dot, row === column ? 1 : 0, tolerance);
    }
  }
  const determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7])
    - matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6])
    + matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  near(determinant, 1, tolerance);
}

function expectedCameraFromRotation(rotation, strength = 50) {
  const maxAngle = 25 * Math.PI / 180;
  const maxOffset = 0.7 * strength / 100;
  const angleX = Math.max(-maxAngle, Math.min(maxAngle, Math.atan2(-rotation[2], rotation[8])));
  const angleY = Math.max(-maxAngle, Math.min(maxAngle, Math.atan2(-rotation[5], rotation[8])));
  return {
    x: Math.tan(angleX) / Math.tan(maxAngle) * maxOffset,
    y: Math.tan(angleY) / Math.tan(maxAngle) * maxOffset,
  };
}

function expectedLookSlopes(rotation) {
  const normalX = -rotation[2];
  const normalY = -rotation[5];
  const normalZ = rotation[8];
  const lateralLength = Math.hypot(normalX, normalY);
  const denominator = Math.max(Math.abs(normalZ), lateralLength / Math.tan(60 * Math.PI / 180));
  return { x: normalX / denominator, y: normalY / denominator };
}

function motionToCameraSlope(offset, strength = 50) {
  return offset * Math.tan(25 * Math.PI / 180) / (0.7 * strength / 100);
}

function rotationAngle(matrix) {
  return Math.acos(Math.max(-1, Math.min(1, (matrix[0] + matrix[4] + matrix[8] - 1) / 2)));
}

function rotationDifference(left, right) {
  return rotationAngle(multiply3(transpose3(left), right));
}

test("orientation matrices reject incomplete samples and preserve the identity posture", () => {
  assert.equal(orientationMatrix({ alpha: 0, beta: 0, gamma: null }), null);
  assert.equal(orientationMatrix({ alpha: 0, beta: 0 }), null);
  const identity = orientationMatrix({ alpha: 0, beta: 0, gamma: 0 });
  for (const [index, value] of [1, 0, 0, 0, 1, 0, 0, 0, 1].entries()) near(identity[index], value);
});

test("arbitrary initial posture is neutral and heading-only changes do not move the view", () => {
  const neutral = { alpha: 217, beta: 71, gamma: -18 };
  for (const screenAngle of [0, 90, 180, 270]) {
    const view = relativeTiltView(neutral, neutral, screenAngle);
    near(view.x, 0);
    near(view.y, 0);
  }

  const wrapped = relativeTiltView(
    { alpha: 359, beta: 71, gamma: -18 },
    { alpha: 1, beta: 71, gamma: -18 },
  );
  near(wrapped.x, 0);
  near(wrapped.y, 0);
  assert.equal(relativeTiltView(neutral, { alpha: null, beta: null, gamma: 2 }), null);
  const alphaFree = relativeTiltPose(
    { alpha: null, beta: 71, gamma: -18 },
    { alpha: 240, beta: 71, gamma: -18 },
  );
  assertMatrixNear(alphaFree.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
});

test("heading sweep at portrait beta 70 and 90 leaves the calibrated fixture unchanged", () => {
  for (const beta of [70, 90]) {
    for (const gamma of [-20, 20]) {
      const baseline = { alpha: 0, beta, gamma };
      for (let alpha = 0; alpha <= 360; alpha += 30) {
        const pose = relativeTiltPose(baseline, { alpha, beta, gamma });
        near(pose.x, 0);
        near(pose.y, 0);
        assertMatrixNear(pose.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
      }
    }
  }
});

test("world-up orbit of an arbitrary Z-X-Y posture preserves the gravity pose", () => {
  const baseline = { alpha: 83, beta: 37, gamma: -22 };
  const orbitMatrix = multiply3(worldYawMatrix(137), orientationMatrix(baseline));
  const orbited = decomposeZxy(orbitMatrix);
  const pose = relativeTiltPose(baseline, orbited);
  assertMatrixNear(pose.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
  near(pose.x, 0);
  near(pose.y, 0);
});

test("gravity remains continuous across the beta 89/91 Euler gimbal neighborhood", () => {
  const baseline = { alpha: 359, beta: 89, gamma: 0 };
  const current = { alpha: 1, beta: 91, gamma: 0 };
  const pose = relativeTiltPose(baseline, current);
  near(rotationAngle(pose.rotation), 2 * Math.PI / 180, 1e-8);
  near(pose.y, expectedOffset(-2), 1e-8);
  assertRotationMatrix(pose.rotation, 1e-8);

  const equivalentGravity = relativeTiltPose(
    { alpha: 0, beta: 90, gamma: -20 },
    { alpha: 210, beta: 90, gamma: 20 },
  );
  assertMatrixNear(equivalentGravity.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
});

test("antipodal gravity and nearby sensor noise use a stable finite half-turn", () => {
  const baseline = { alpha: 0, beta: 0, gamma: 0 };
  const opposite = relativeTiltPose(baseline, { alpha: 0, beta: 180, gamma: 0 });
  const noisyPositive = relativeTiltPose(baseline, { alpha: 180, beta: 180, gamma: 0.05 });
  const noisyNegative = relativeTiltPose(baseline, { alpha: 90, beta: 180, gamma: -0.05 });
  const justOutsideStableCone = relativeTiltPose(baseline, { alpha: 270, beta: 180, gamma: 0.1 });
  assertMatrixNear(noisyPositive.rotation, opposite.rotation, 1e-10);
  assertMatrixNear(noisyNegative.rotation, opposite.rotation, 1e-10);
  assert.ok(rotationDifference(justOutsideStableCone.rotation, opposite.rotation) < 0.2 * Math.PI / 180);
  for (const pose of [opposite, noisyPositive, noisyNegative, justOutsideStableCone]) {
    assertRotationMatrix(pose.rotation, 1e-8);
    assert.ok(Number.isFinite(pose.x) && Number.isFinite(pose.y));
    assert.ok(Math.abs(pose.x) < 10 && Math.abs(pose.y) < 10);
  }
});

test("controller holds its last valid target in the upside-down ambiguity cone", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push({ time: browser.clock.now, ...view }),
  });
  await controller.start();
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 0, gamma: 0 });
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 180, gamma: 3 });
  for (let frame = 0; frame < 48; frame += 1) browser.frame();
  const validTarget = [...controller._target.rotation];
  assert.ok(rotationDifference(controller._view.rotation, validTarget) < 0.1 * Math.PI / 180);
  const settledView = views.at(-1);
  const settledViewCount = views.length;

  for (const gamma of [0.5, -0.05, 0.05, -0.5, 1.5]) {
    browser.window.dispatch("deviceorientation", { alpha: gamma * 80, beta: 180, gamma });
    assert.deepEqual(controller._target.rotation, validTarget);
    assert.equal(controller._nearAntipode, true);
    assert.equal(views.at(-1), settledView, "the emitted pose stays held across the ambiguous cone");
    assert.equal(views.length, settledViewCount);
    assert.ok(browser.clock.frames.size <= 1, "ambiguous samples retain the existing single animation chain");
  }

  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 180, gamma: 2.1 });
  assert.equal(controller._nearAntipode, false, "the guard exits only beyond its wider 2 degree boundary");
  assert.ok(rotationDifference(validTarget, controller._target.rotation) < 1.1 * Math.PI / 180);
  const beforeFrame = [...controller._view.rotation];
  browser.frame();
  assert.ok(rotationDifference(beforeFrame, controller._view.rotation) < 1 * Math.PI / 180,
    "the existing SLERP chain eases the small same-side exit change");
  assert.equal(controller.recenter(), true);
  assert.equal(controller._nearAntipode, false, "recenter clears the inverted-gravity guard");
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
  controller.dispose();
});

test("relative room pose is neutral at an arbitrary anchor and follows screen-axis rotation", () => {
  const anchor = { alpha: 217, beta: 71, gamma: -18 };
  for (const angle of [0, 90, 180, 270]) {
    const pose = relativeTiltPose(anchor, anchor, angle, angle);
    near(pose.x, 0);
    near(pose.y, 0);
    assertMatrixNear(pose.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
    assertRotationMatrix(pose.rotation, 1e-8);
  }

  const upright = { alpha: 0, beta: 90, gamma: 0 };
  const quarterTurn = relativeTiltPose(upright, upright, 0, 90);
  assertMatrixNear(quarterTurn.rotation, [0, -1, 0, 1, 0, 0, 0, 0, 1], 1e-8);
  near(quarterTurn.x, 0);
  near(quarterTurn.y, 0);
  const oppositeQuarterTurn = relativeTiltPose(upright, upright, 0, 270);
  assertMatrixNear(oppositeQuarterTurn.rotation, [0, 1, 0, -1, 0, 0, 0, 0, 1], 1e-8);
});

test("noncommuting arbitrary orientations use the unique shortest gravity swing", () => {
  const baseline = { alpha: 217, beta: 71, gamma: -18 };
  const current = { alpha: 249, beta: 83, gamma: -5 };
  const baselineAngle = 90, currentAngle = 270;
  const expected = shortestRotation(
    gravityScene(baseline, baselineAngle),
    gravityScene(current, currentAngle),
  );
  const pose = relativeTiltPose(baseline, current, baselineAngle, currentAngle);
  assertMatrixNear(pose.rotation, expected, 1e-9);
  assertRotationMatrix(pose.rotation, 1e-8);
  assert.ok(Object.isFrozen(pose.rotation));
});

test("look mode composes the full calibrated Z-X-Y matrices independently of gravity swing", () => {
  const baseline = { alpha: 217, beta: 71, gamma: -18 };
  const current = { alpha: 249, beta: 83, gamma: -5 };
  const baselineAngle = 90, currentAngle = 270;
  const expected = multiply3(
    screenAxis(currentAngle),
    multiply3(
      transpose3(orientationMatrix(current)),
      multiply3(orientationMatrix(baseline), screenAxis(baselineAngle)),
    ),
  );
  const lookPose = relativeLookPose(baseline, current, baselineAngle, currentAngle);
  const gravityPose = relativeTiltPose(baseline, current, baselineAngle, currentAngle);
  assertMatrixNear(lookPose.rotation, expected, 1e-9);
  assertRotationMatrix(lookPose.rotation, 1e-8);
  assert.ok(rotationDifference(lookPose.rotation, gravityPose.rotation) > 0.1);
  assert.ok(Object.isFrozen(lookPose.rotation));
});

test("look tracking brings the side edge toward the eye at portrait beta 70 and 90", () => {
  const gammaChangeDegrees = 25;
  for (const beta of [70, 90]) {
    const baseline = { alpha: 0, beta, gamma: 0 };
    const current = { alpha: 0, beta, gamma: gammaChangeDegrees };
    const lookPose = relativeLookPose(baseline, current);
    const gravityPose = relativeTiltPose(baseline, current);
    near(lookPose.x, expectedOffset(-gammaChangeDegrees), 1e-9);
    near(lookPose.y, 0, 1e-9);
    assert.ok(Math.hypot(gravityPose.x, gravityPose.y) < Math.abs(lookPose.x) * 0.15,
      "gravity mode keeps the earth-stable response smaller or zero");
  }
});

test("look camera preserves front-facing rays through 60 degrees and folds back-facing rays safely", () => {
  const baseline = { alpha: 0, beta: 0, gamma: 0 };
  for (const current of [
    { alpha: 0, beta: 40, gamma: 0 },
    { alpha: 0, beta: 60, gamma: 0 },
    { alpha: 0, beta: 90, gamma: 0 },
    { alpha: 0, beta: 179, gamma: 0 },
    { alpha: 0, beta: 180, gamma: 0 },
    { alpha: 0, beta: 181, gamma: 0 },
    { alpha: 0, beta: 270, gamma: 0 },
    { alpha: 0, beta: 70, gamma: 45 },
  ]) {
    const pose = relativeLookPose(baseline, current, 0, 0, 50);
    const expected = expectedLookSlopes(pose.rotation);
    const actual = { x: motionToCameraSlope(pose.x), y: motionToCameraSlope(pose.y) };
    near(actual.x, expected.x, 1e-8);
    near(actual.y, expected.y, 1e-8);
    assert.ok(Math.hypot(actual.x, actual.y) <= Math.tan(60 * Math.PI / 180) + 1e-8);
    assert.ok(Number.isFinite(pose.x) && Number.isFinite(pose.y));
    assertRotationMatrix(pose.rotation, 1e-8);
  }

  const frontForty = relativeLookPose(baseline, { alpha: 0, beta: 40, gamma: 0 }, 0, 0, 50);
  near(motionToCameraSlope(frontForty.y), -Math.tan(40 * Math.PI / 180), 1e-8);
  const frontSixty = relativeLookPose(baseline, { alpha: 0, beta: 60, gamma: 0 }, 0, 0, 50);
  near(motionToCameraSlope(frontSixty.y), -Math.tan(60 * Math.PI / 180), 1e-8);
});

test("look mode follows heading, preserves full roll in the lamp matrix, and stays continuous through 180", () => {
  const upright = { alpha: 0, beta: 90, gamma: 0 };
  let previousSlope = null;
  for (let alpha = 0; alpha <= 360; alpha += 1) {
    const current = { alpha, beta: 90, gamma: 0 };
    const lookPose = relativeLookPose(upright, current);
    const gravityPose = relativeTiltPose(upright, current);
    const slope = motionToCameraSlope(lookPose.x);
    const expected = expectedLookSlopes(lookPose.rotation);
    near(slope, expected.x, 1e-8);
    near(motionToCameraSlope(lookPose.y), expected.y, 1e-8);
    near(gravityPose.x, 0, 1e-8);
    near(gravityPose.y, 0, 1e-8);
    assert.ok(Number.isFinite(lookPose.x) && Number.isFinite(lookPose.y));
    assert.ok(Math.hypot(slope, expected.y) <= Math.tan(60 * Math.PI / 180) + 1e-8);
    if (previousSlope !== null) assert.ok(Math.abs(slope - previousSlope) < 0.08);
    previousSlope = slope;
  }

  for (const alpha of [179, 180, 181]) {
    const pose = relativeLookPose(upright, { alpha, beta: 90, gamma: 0 });
    assert.ok(Number.isFinite(pose.x) && Number.isFinite(pose.y));
    assert.ok(Math.max(Math.abs(pose.x), Math.abs(pose.y)) < 0.05,
      "the camera ray folds to neutral near the back pole instead of flipping azimuth");
  }

  const flat = { alpha: 0, beta: 0, gamma: 0 };
  const rollPose = relativeLookPose(flat, { alpha: 45, beta: 0, gamma: 0 });
  assertMatrixNear(rollPose.rotation, [
    Math.cos(45 * Math.PI / 180), -Math.sin(45 * Math.PI / 180), 0,
    Math.sin(45 * Math.PI / 180), Math.cos(45 * Math.PI / 180), 0,
    0, 0, 1,
  ], 1e-8);
  near(rollPose.x, 0);
  near(rollPose.y, 0);

  const wrapPose = relativeLookPose(
    { alpha: 359, beta: 90, gamma: 0 },
    { alpha: 1, beta: 90, gamma: 0 },
  );
  near(rotationAngle(wrapPose.rotation), 2 * Math.PI / 180, 1e-8);
});

test("look mode tracks world-up heading rotations that gravity mode intentionally ignores", () => {
  const baseline = { alpha: 83, beta: 37, gamma: -22 };
  const orbit = decomposeZxy(multiply3(worldYawMatrix(137), orientationMatrix(baseline)));
  const look = relativeLookPose(baseline, orbit);
  const groundStable = relativeTiltPose(baseline, orbit);
  near(rotationAngle(look.rotation), 137 * Math.PI / 180, 1e-8);
  assertMatrixNear(groundStable.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
});

test("look helper falls back to gravity when alpha is unavailable", () => {
  const baseline = { alpha: 23, beta: 70, gamma: -12 };
  const current = { alpha: null, beta: 78, gamma: -4 };
  const lookPose = relativeLookPose(baseline, current, 90, 90);
  const gravityPose = relativeTiltPose(baseline, current, 90, 90);
  assertMatrixNear(lookPose.rotation, gravityPose.rotation, 1e-9);
  near(lookPose.x, gravityPose.x, 1e-9);
  near(lookPose.y, gravityPose.y, 1e-9);
});

test("gravity-plane roll remains in the room pose while camera offsets stay centered", () => {
  const rollDegrees = 34;
  const roll = rollDegrees * Math.PI / 180;
  const pose = relativeTiltPose(
    { alpha: 0, beta: 0, gamma: 90 },
    { alpha: 180, beta: rollDegrees, gamma: 90 },
  );
  assertMatrixNear(pose.rotation, [
    Math.cos(roll), -Math.sin(roll), 0,
    Math.sin(roll), Math.cos(roll), 0,
    0, 0, 1,
  ]);
  near(pose.x, 0);
  near(pose.y, 0);
  assertRotationMatrix(pose.rotation);

  const weaker = relativeTiltPose({ alpha: 0, beta: 0, gamma: 0 }, { alpha: 0, beta: 14, gamma: 0 }, 0, 0, 25);
  const stronger = relativeTiltPose({ alpha: 0, beta: 0, gamma: 0 }, { alpha: 0, beta: 14, gamma: 0 }, 0, 0, 100);
  assertMatrixNear(weaker.rotation, stronger.rotation);
  near(stronger.y, weaker.y * 4);
});

test("relative room rotation crosses alpha wrap without a large pose jump", () => {
  const baseline = { alpha: 359, beta: 42, gamma: -11 };
  const current = { alpha: 1, beta: 42, gamma: -11 };
  const pose = relativeTiltPose(baseline, current);
  assertRotationMatrix(pose.rotation, 1e-8);
  assertMatrixNear(pose.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
});

test("relative local-axis rotations produce analytic view slopes around arbitrary baselines", () => {
  const xBaseline = { alpha: 43, beta: 37, gamma: 0 };
  const xCurrent = { ...xBaseline, beta: xBaseline.beta + 12 };
  const xView = relativeTiltView(xBaseline, xCurrent);
  near(xView.x, 0);
  near(xView.y, expectedOffset(-12));

  const yBaseline = { alpha: 113, beta: 0, gamma: -14 };
  const yCurrent = { ...yBaseline, gamma: yBaseline.gamma + 12 };
  const yView = relativeTiltView(yBaseline, yCurrent);
  near(yView.x, expectedOffset(-12));
  near(yView.y, 0);
});

test("screen rotation maps the same relative ray into portrait and landscape axes", () => {
  const baseline = { alpha: 43, beta: 0, gamma: 0 };
  const current = { ...baseline, gamma: 12 };
  const portrait = relativeTiltView(baseline, current, 0);
  const landscapeRight = relativeTiltView(baseline, current, 90);
  const landscapeLeft = relativeTiltView(baseline, current, 270);
  near(landscapeRight.x, 0);
  near(landscapeRight.y, portrait.x);
  near(landscapeLeft.x, 0);
  near(landscapeLeft.y, -portrait.x);
  near(relativeTiltView(baseline, current, 180).x, -portrait.x);
  near(relativeTiltView(baseline, current, 180).y, -portrait.y);
});

test("controller reads standardized and legacy screen orientation angles", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push(view),
  });
  await controller.start();
  const baseline = { alpha: 43, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);

  browser.window.screen.orientation.angle = 90;
  browser.window.screen.orientation.dispatch("change");
  browser.window.dispatch("deviceorientation", baseline);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 12 });
  for (let frame = 0; frame < 24; frame += 1) browser.frame();
  near(views.at(-1).x, 0, 1e-6);
  near(views.at(-1).y, expectedOffset(-12), 0.005);

  browser.window.screen.orientation.angle = NaN;
  browser.window.orientation = 270;
  browser.window.dispatch("orientationchange");
  browser.window.dispatch("deviceorientation", baseline);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 12 });
  for (let frame = 0; frame < 24; frame += 1) browser.frame();
  near(views.at(-1).x, 0, 1e-6);
  near(views.at(-1).y, expectedOffset(12), 0.005);
  controller.stop();
});

test("controller ignores heading sweeps but follows gravity tilt and accepts missing alpha", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push(view),
  });
  await controller.start();
  browser.window.dispatch("deviceorientation", { alpha: null, beta: 70, gamma: 20 });
  const calibratedViewCount = views.length;
  for (let alpha = 0; alpha <= 360; alpha += 15) {
    browser.window.dispatch("deviceorientation", { alpha, beta: 70, gamma: 20 });
    assert.equal(browser.clock.frames.size, 0);
  }
  assert.equal(views.length, calibratedViewCount, "heading-only samples never perturb camera or fixture light");

  browser.window.dispatch("deviceorientation", { alpha: null, beta: 82, gamma: 20 });
  for (let frame = 0; frame < 24; frame += 1) browser.frame();
  assert.ok(Math.abs(views.at(-1).y) > 0.1, "gravity tilt still moves the view");
  assert.ok(views.at(-1).rotation.some((value, index) => Math.abs(value - [1, 0, 0, 0, 1, 0, 0, 0, 1][index]) > 0.01));
  controller.dispose();
});

test("controller smooths one gravity pose and publishes screen-plane roll with matching camera offsets", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push({ time: browser.clock.now, ...view }),
  });
  await controller.start();
  const baseline = { alpha: 0, beta: 0, gamma: 90 };
  const rolled = { alpha: 34, beta: 34, gamma: 90 };
  browser.window.dispatch("deviceorientation", baseline);
  browser.window.dispatch("deviceorientation", rolled);
  for (let frame = 0; frame < 36; frame += 1) browser.frame();

  assert.ok(views.length > 2, "gravity-plane rotation publishes even when the camera ray stays centered");
  assertMatrixNear(views.at(-1).rotation, relativeTiltPose(baseline, rolled).rotation, 0.003);
  assert.ok(Math.abs(views.at(-1).rotation[1]) > 0.1, "the final pose retains its gravity-plane roll");
  for (const view of views) {
    const expected = expectedCameraFromRotation(view.rotation, 50);
    near(view.x, expected.x, 1e-9);
    near(view.y, expected.y, 1e-9);
    assertRotationMatrix(view.rotation, 1e-8);
  }
  for (let index = 1; index < views.length; index += 1) {
    assert.ok(views[index].time - views[index - 1].time >= 1000 / 30 - 1e-6);
  }

  controller.stop();
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
  controller.dispose();
});

test("tracking mode defaults to gravity, changes off without motion, and recenters when changed active", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push(view),
  });
  assert.equal(controller.trackingMode, "gravity");
  assert.equal(controller.setTrackingMode("look"), true);
  assert.equal(controller.trackingMode, "look");
  assert.equal(views.length, 0, "changing mode while off only selects the mode");
  assert.throws(() => controller.setTrackingMode("compass"), RangeError);

  await controller.start();
  const baseline = { alpha: 0, beta: 90, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  const firstLookViewIndex = views.length;
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 25 });
  for (let frame = 0; frame < 30; frame += 1) browser.frame();
  near(views.at(-1).x, expectedOffset(-25), 0.005);
  for (const view of views.slice(firstLookViewIndex)) {
    const expected = expectedLookSlopes(view.rotation);
    near(motionToCameraSlope(view.x), expected.x, 1e-8);
    near(motionToCameraSlope(view.y), expected.y, 1e-8);
  }

  controller.setTrackingMode("gravity");
  assert.equal(controller.trackingMode, "gravity");
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
  near(views.at(-1).x, 0);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 30 });
  for (let frame = 0; frame < 3; frame += 1) browser.frame();
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);

  controller.setTrackingMode("look");
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 55 });
  for (let frame = 0; frame < 30; frame += 1) browser.frame();
  near(views.at(-1).x, expectedOffset(-25), 0.005);
  assert.deepEqual(Object.keys(views.at(-1)).sort(), ["rotation", "x", "y"]);
  controller.dispose();
});

test("look mode uses full valid rotations through gravity antipodes and gravity fallback for missing alpha", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    trackingMode: "look",
    onView: view => views.push(view),
  });
  await controller.start();
  const baseline = { alpha: 0, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  const inverted = { alpha: 0, beta: 180, gamma: 0 };
  browser.window.dispatch("deviceorientation", inverted);
  assert.equal(controller._nearAntipode, false, "full valid orientations do not use the gravity ambiguity guard");
  assert.ok(rotationDifference(controller._target.rotation, relativeLookPose(baseline, inverted).rotation) < 1e-8);
  for (let frame = 0; frame < 36; frame += 1) browser.frame();
  assertRotationMatrix(views.at(-1).rotation, 1e-8);

  const missingAlpha = { alpha: null, beta: 70, gamma: 0 };
  browser.window.dispatch("deviceorientation", missingAlpha);
  const gravityFallback = relativeTiltPose(baseline, missingAlpha);
  assert.equal(controller._target.cameraMode, "gravity");
  for (let frame = 0; frame < 60; frame += 1) browser.frame();
  assertMatrixNear(views.at(-1).rotation, gravityFallback.rotation, 0.003);
  near(views.at(-1).x, gravityFallback.x, 0.005);
  near(views.at(-1).y, gravityFallback.y, 0.005);
  controller.dispose();
});

test("view strength follows tangent slope and clamps large rotations per axis", () => {
  const baseline = { alpha: 43, beta: 10, gamma: 0 };
  const small = relativeTiltView(baseline, { ...baseline, beta: 22 });
  near(small.y, expectedOffset(-12));
  near(relativeTiltView(baseline, { ...baseline, beta: 22 }, 0, 100).y, expectedOffset(-12, 100));
  near(relativeTiltView(baseline, { ...baseline, beta: 22 }, 0, 0).y, 0);

  const large = relativeTiltView(baseline, { ...baseline, beta: 160 }, 0, 100);
  const maximumOffset = 0.7 * Math.tan(75 * Math.PI / 180) / Math.tan(25 * Math.PI / 180);
  near(large.y, -maximumOffset);
  assert.ok(Math.abs(large.x) <= maximumOffset + 1e-9);
  assert.throws(() => relativeTiltView(baseline, baseline, 0, 50.5), RangeError);
});

test("40 and 60 degree room tilts invert through the physical viewing camera at strength 50", () => {
  const baseline = { alpha: 0, beta: 0, gamma: 0 };
  for (const angleDegrees of [40, 60]) {
    const radians = angleDegrees * Math.PI / 180;
    const pitchPose = relativeTiltPose(baseline, { alpha: 0, beta: angleDegrees, gamma: 0 }, 0, 0, 50);
    const pitchCamera = computeViewingCamera({
      distanceCm: 35,
      gameWidthCm: 7,
      motionX: pitchPose.x,
      motionY: pitchPose.y,
    });
    const pitchSlope = pitchCamera.y * 240 / (19.2 * pitchCamera.focalDepth);
    near(pitchSlope, -Math.tan(radians), 1e-9);
    near(pitchSlope, -pitchPose.rotation[5] / pitchPose.rotation[8], 1e-9);

    const yawPose = relativeTiltPose(baseline, { alpha: 0, beta: 0, gamma: angleDegrees }, 0, 0, 50);
    const yawCamera = computeViewingCamera({
      distanceCm: 35,
      gameWidthCm: 7,
      motionX: yawPose.x,
      motionY: yawPose.y,
    });
    const yawSlope = yawCamera.x * 256 / (19.2 * yawCamera.focalDepth);
    near(yawSlope, -Math.tan(radians), 1e-9);
    near(yawSlope, -yawPose.rotation[2] / yawPose.rotation[8], 1e-9);
    assert.ok(Number.isFinite(pitchPose.x) && Number.isFinite(pitchPose.y));
    assert.ok(Number.isFinite(yawPose.x) && Number.isFinite(yawPose.y));
  }
});

test("start requests permission synchronously and calibrates from the first valid sample", async () => {
  const order = [];
  const browser = fakeBrowser({ permission: () => { order.push("permission"); return Promise.resolve("granted"); } });
  const statuses = [], views = [], published = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onStatus: status => { statuses.push(status.state); order.push(`status:${status.state}`); },
    onView: view => {
      published.push(view);
      views.push({ time: browser.clock.now, ...view });
    },
  });

  const starting = controller.start();
  assert.equal(order[0], "status:off");
  assert.equal(order[1], "status:requesting");
  assert.equal(order[2], "permission");
  assert.equal(controller.enabled, true);
  assert.equal(await starting, true);
  assert.equal(statuses.at(-1), "calibrating");
  browser.window.dispatch("deviceorientation", { alpha: null, beta: null, gamma: 0 });
  assert.equal(statuses.at(-1), "calibrating", "invalid samples do not establish a baseline");
  browser.window.dispatch("deviceorientation", { alpha: null, beta: 90, gamma: -4 });
  assert.equal(statuses.at(-1), "active");
  assert.deepEqual(views.at(-1), { time: 0, x: 0, y: 0, rotation: [1, 0, 0, 0, 1, 0, 0, 0, 1] });
  assert.ok(Object.isFrozen(published.at(-1)));
  assert.ok(Object.isFrozen(published.at(-1).rotation));
  controller.dispose();
  assert.equal(browser.window.listenerCount("deviceorientation"), 0);
  assert.equal(browser.document.listenerCount("visibilitychange"), 0);
  assert.equal(controller.enabled, false);
});

test("insecure, unsupported, and denied starts report their state without sensor listeners", async () => {
  const insecure = fakeBrowser({ secure: false });
  const insecureStatuses = [];
  const insecureController = new DeviceTiltController({ window: insecure.window, document: insecure.document, onStatus: s => insecureStatuses.push(s.state) });
  assert.equal(await insecureController.start(), false);
  assert.deepEqual(insecureStatuses, ["off", "insecure"]);
  assert.equal(insecureController.enabled, false);

  const unsupported = fakeBrowser({ supported: false });
  const unsupportedController = new DeviceTiltController({ window: unsupported.window, document: unsupported.document });
  assert.equal(await unsupportedController.start(), false);
  assert.equal(unsupportedController.enabled, false);

  const denied = fakeBrowser({ permission: () => Promise.resolve("denied") });
  const deniedController = new DeviceTiltController({ window: denied.window, document: denied.document });
  assert.equal(await deniedController.start(), false);
  assert.equal(deniedController.enabled, false);
  assert.equal(deniedController._state, "denied");
  assert.equal(denied.window.listenerCount("deviceorientation"), 0);

  const rejected = fakeBrowser({
    permission: () => Promise.reject(Object.assign(new Error("blocked"), { name: "NotAllowedError" })),
  });
  const rejectedController = new DeviceTiltController({ window: rejected.window, document: rejected.document });
  assert.equal(await rejectedController.start(), false);
  assert.equal(rejectedController._state, "denied");

  const failed = fakeBrowser({ permission: () => Promise.reject(new Error("sensor service failed")) });
  const failedController = new DeviceTiltController({ window: failed.window, document: failed.document });
  assert.equal(await failedController.start(), false);
  assert.equal(failedController._state, "error");
});

test("stopping while permission is pending cancels the late grant", async () => {
  let grant;
  const browser = fakeBrowser({ permission: () => new Promise(resolve => { grant = resolve; }) });
  const controller = new DeviceTiltController({ window: browser.window, document: browser.document });
  const starting = controller.start();
  controller.stop();
  grant("granted");
  assert.equal(await starting, false);
  assert.equal(controller.enabled, false);
  assert.equal(browser.window.listenerCount("deviceorientation"), 0);
  assert.equal(controller._state, "off");
});

test("pausing during a permission prompt preserves enabled intent and calibrates on resume", async () => {
  let grant;
  const browser = fakeBrowser({ permission: () => new Promise(resolve => { grant = resolve; }) });
  const controller = new DeviceTiltController({ window: browser.window, document: browser.document });
  const starting = controller.start();
  controller.setPaused(true);
  assert.equal(controller.enabled, true);
  grant("granted");
  assert.equal(await starting, true);
  assert.equal(controller._state, "paused");
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 90, gamma: 0 });
  assert.equal(controller._state, "paused");
  controller.setPaused(false);
  assert.equal(controller._state, "calibrating");
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 90, gamma: 0 });
  assert.equal(controller._state, "active");
  controller.stop();
});

test("missing sensor data times out, while null samples do not calibrate", async () => {
  const browser = fakeBrowser();
  const controller = new DeviceTiltController({ window: browser.window, document: browser.document });
  assert.equal(await controller.start(), true);
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: null, gamma: 0 });
  browser.advance(2999);
  assert.equal(controller.enabled, true);
  browser.advance(1);
  assert.equal(controller.enabled, false);
  assert.equal(controller._state, "unavailable");
  assert.equal(browser.window.listenerCount("deviceorientation"), 0);
});

test("recenter, pause, resume, visibility, and rotation changes rebaseline safely", async () => {
  const browser = fakeBrowser();
  const views = [];
  const statuses = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push({ time: browser.clock.now, ...view }),
    onStatus: status => statuses.push(status.state),
  });
  await controller.start();
  const baseline = { alpha: 77, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 18 });
  for (let frame = 0; frame < 12; frame += 1) browser.frame();
  assert.notEqual(views.at(-1).x, 0);
  assert.equal(controller.recenter(), true, "a fresh sample recenters immediately");
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, { x: 0, y: 0 });
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);

  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 28 });
  for (let frame = 0; frame < 5; frame += 1) browser.frame();
  const countBeforePause = views.length;
  const retained = { x: views.at(-1).x, y: views.at(-1).y };
  assert.notEqual(retained.x, 0);
  controller.setPaused(true);
  assert.equal(controller.enabled, true, "paused keeps the requested tilt intent enabled");
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: -20 });
  for (let frame = 0; frame < 4; frame += 1) browser.frame();
  assert.equal(views.length, countBeforePause);
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, retained);
  controller.setPaused(false);
  assert.equal(statuses.at(-1), "calibrating");
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: -20 });
  assert.equal(statuses.at(-1), "active");
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, { x: 0, y: 0 });

  browser.window.screen.orientation.angle = 90;
  browser.window.screen.orientation.dispatch("change");
  assert.equal(statuses.at(-1), "calibrating");
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: -20 });
  assert.equal(statuses.at(-1), "active");
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, { x: 0, y: 0 });

  const countBeforeHide = views.length;
  browser.document.visibilityState = "hidden";
  browser.document.dispatch("visibilitychange");
  assert.equal(statuses.at(-1), "paused");
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 22 });
  assert.equal(views.length, countBeforeHide);
  browser.document.visibilityState = "visible";
  browser.document.dispatch("visibilitychange");
  assert.equal(statuses.at(-1), "calibrating");
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 22 });
  assert.equal(statuses.at(-1), "active");
  controller.stop();
});

test("stale recenter neutralizes immediately and calibrates from the next sample", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push({ time: browser.clock.now, ...view }),
  });
  await controller.start();
  const baseline = { alpha: 42, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 18 });
  for (let frame = 0; frame < 12; frame += 1) browser.frame();
  const before = views.at(-1);
  browser.advance(RECENTER_STALE_INTERVAL);
  assert.equal(controller.recenter(), false);
  assert.notEqual(views.at(-1), before);
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, { x: 0, y: 0 }, "stale recenter still resets the view immediately");
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1], 1e-8);
  assert.equal(controller._state, "calibrating");
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 18 });
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, { x: 0, y: 0 });
  controller.stop();
});

test("recenter and stop publish exact identity after a sub-deadzone roll was emitted", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push(view),
  });
  await controller.start();
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 0, gamma: 90 });

  const anchorAndRoll = async baselineBeta => {
    browser.window.dispatch("deviceorientation", { alpha: 0, beta: baselineBeta, gamma: 90 });
    browser.window.dispatch("deviceorientation", { alpha: 0, beta: baselineBeta + 0.2, gamma: 90 });
    for (let frame = 0; frame < 36; frame += 1) browser.frame();
    assert.ok(rotationAngle(views.at(-1).rotation) > 0.002);
    browser.window.dispatch("deviceorientation", {
      alpha: 0,
      beta: baselineBeta + 0.02864788975654116,
      gamma: 90,
    });
    for (let frame = 0; frame < 30; frame += 1) {
      browser.frame();
      const angle = rotationAngle(views.at(-1).rotation);
      if (angle > 0 && angle < 0.001) return angle;
    }
    assert.fail("expected an emitted nonzero gravity-plane roll below the animation deadzone");
  };

  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 0, gamma: 90 });
  const nearNeutralAngle = await anchorAndRoll(0);
  assert.ok(nearNeutralAngle > 0 && nearNeutralAngle < 0.001);
  assert.equal(controller.recenter(), true);
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);

  const secondNearNeutralAngle = await anchorAndRoll(0.02864788975654116);
  assert.ok(secondNearNeutralAngle > 0 && secondNearNeutralAngle < 0.001);
  controller.stop();
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
});

test("a resumed sensor timeout neutralizes the retained hidden view", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push({ time: browser.clock.now, ...view }),
  });
  await controller.start();
  const baseline = { alpha: 42, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 18 });
  for (let frame = 0; frame < 12; frame += 1) browser.frame();
  assert.notEqual(views.at(-1).x, 0);

  browser.document.visibilityState = "hidden";
  browser.document.dispatch("visibilitychange");
  const retained = views.at(-1);
  browser.document.visibilityState = "visible";
  browser.document.dispatch("visibilitychange");
  assert.equal(controller._state, "calibrating");
  assert.equal(views.at(-1), retained, "resume waits for a fresh baseline before changing the view");
  browser.advance(3000);
  assert.equal(controller.enabled, false);
  assert.equal(controller._state, "unavailable");
  assert.deepEqual({ x: views.at(-1).x, y: views.at(-1).y }, { x: 0, y: 0 });
});

test("no-data timeout neutralizes a pure room roll even when camera offsets are zero", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push(view),
  });
  await controller.start();
  browser.window.dispatch("deviceorientation", { alpha: 0, beta: 0, gamma: 90 });
  browser.window.dispatch("deviceorientation", { alpha: 45, beta: 30, gamma: 90 });
  for (let frame = 0; frame < 18; frame += 1) browser.frame();
  near(views.at(-1).x, 0);
  near(views.at(-1).y, 0);
  assert.ok(Math.abs(views.at(-1).rotation[1]) > 0.1);

  browser.document.visibilityState = "hidden";
  browser.document.dispatch("visibilitychange");
  browser.document.visibilityState = "visible";
  browser.document.dispatch("visibilitychange");
  browser.advance(3000);
  assert.equal(controller.enabled, false);
  assertMatrixNear(views.at(-1).rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
});

test("sensor bursts use one RAF chain, smooth over time, and emit at no more than 30 Hz", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({
    window: browser.window,
    document: browser.document,
    onView: view => views.push({ time: browser.clock.now, ...view }),
  });
  await controller.start();
  const baseline = { alpha: 58, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  for (let gamma = 2; gamma <= 24; gamma += 2) {
    browser.window.dispatch("deviceorientation", { ...baseline, gamma });
    assert.ok(browser.clock.frames.size <= 1);
  }
  assert.equal(browser.clock.frames.size, 1);
  for (let frame = 0; frame < 24; frame += 1) browser.frame();
  const target = expectedOffset(-24);
  const motionViews = views.slice(1);
  assert.ok(motionViews.length >= 4);
  assert.ok(Math.abs(motionViews[0].x) < Math.abs(target), "the first update is smoothed toward the target");
  assert.ok(Math.abs(motionViews.at(-1).x - target) < 0.01, "the low-pass filter approaches the target");
  for (let index = 1; index < views.length; index += 1) {
    assert.ok(views[index].time - views[index - 1].time >= 1000 / 30 - 1e-6);
  }
  controller.dispose();
});

test("strength changes are validated and take effect on the next sample", async () => {
  const browser = fakeBrowser();
  const views = [];
  const controller = new DeviceTiltController({ window: browser.window, document: browser.document, onView: view => views.push(view) });
  await controller.start();
  const baseline = { alpha: 58, beta: 0, gamma: 0 };
  browser.window.dispatch("deviceorientation", baseline);
  controller.setStrength(100);
  browser.window.dispatch("deviceorientation", { ...baseline, gamma: 12 });
  for (let frame = 0; frame < 12; frame += 1) browser.frame();
  assert.ok(views.at(-1).x < -0.2 && views.at(-1).x >= -0.7);
  assert.throws(() => controller.setStrength(-1), RangeError);
  assert.throws(() => controller.setStrength(50.5), RangeError);
  controller.stop();
});

const RECENTER_STALE_INTERVAL = 251;
