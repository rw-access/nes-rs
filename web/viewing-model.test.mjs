import test from "node:test";
import assert from "node:assert/strict";
import { relativeLookPose, relativeTiltPose } from "./device-tilt.js";
import { cameraDepthScale, PERSPECTIVE_DEPTH_FACTOR, projectDepthPoint } from "./depth-mesh.js";
import { getSceneLightPlacement, getSceneLightPosition, getSceneLightRotation, setSceneLightPlacement, setSceneLightRotation } from "./scene-lighting.js";
import { getSceneCameraDepth, setSceneCamera } from "./scene-view.js";
import { computeViewingCamera, estimateGameWidthCm, resolveRenderedScenePose, resolveScreenProfile, SCREEN_PROFILES, computeStationaryView, DESKTOP_GAME_WIDTH_CM, SCENE_DEPTH_TO_NATIVE_PIXELS } from "./viewing-model.js";

function assertNear(actual, expected, tolerance = 1e-11) {
  assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} differs from ${expected}`);
}

function slopes(motionX, motionY) {
  const scale = 2 * Math.tan(25 * Math.PI / 180) / 0.7;
  const x = motionX * scale;
  const y = motionY * scale;
  const length = Math.hypot(x, y, 1);
  return { x: x / length, y: y / length, z: 1 / length };
}

function multiplyVector(matrix, vector) {
  return [
    matrix[0] * vector[0] + matrix[1] * vector[1] + matrix[2] * vector[2],
    matrix[3] * vector[0] + matrix[4] * vector[1] + matrix[5] * vector[2],
    matrix[6] * vector[0] + matrix[7] * vector[1] + matrix[8] * vector[2],
  ];
}

function assertProperRotation(matrix, tolerance = 1e-8) {
  for (let row = 0; row < 3; row += 1) for (let other = 0; other < 3; other += 1) {
    const dot = matrix.slice(row * 3, row * 3 + 3)
      .reduce((sum, value, index) => sum + value * matrix[other * 3 + index], 0);
    assertNear(dot, row === other ? 1 : 0, tolerance);
  }
  const determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7])
    - matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6])
    + matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  assertNear(determinant, 1, tolerance);
}

function rotationDifference(left, right) {
  const trace = [0, 1, 2].reduce((sum, row) => sum + [0, 1, 2].reduce((dot, column) =>
    dot + left[column * 3 + row] * right[column * 3 + row], 0), 0);
  return Math.acos(Math.max(-1, Math.min(1, (trace - 1) / 2)));
}

function assertRenderedEye(pose, distanceCm = 35, gameWidthCm = 7) {
  const camera = computeViewingCamera({
    distanceCm, gameWidthCm, motionX: pose.x, motionY: pose.y,
  });
  const focalNative = distanceCm / gameWidthCm * 256;
  const renderedEye = [camera.x * 256, camera.y * 240, -camera.focalDepth * SCENE_DEPTH_TO_NATIVE_PIXELS];
  const transformedInitialEye = multiplyVector(pose.rotation, [0, 0, -focalNative]);
  transformedInitialEye.forEach((value, index) => assertNear(value, renderedEye[index], 1e-8));
  assertProperRotation(pose.rotation, 1e-8);
  return { camera, renderedEye, focalNative };
}

test("screen profiles expose calibrated active widths and broad Auto estimates", () => {
  assert.deepEqual(SCREEN_PROFILES.phone, {
    id: "phone", label: "Phone", shortWidthCm: 6.7, defaultDistanceCm: 35,
  });
  assert.deepEqual(SCREEN_PROFILES["large-phone"], {
    id: "large-phone", label: "Large phone", shortWidthCm: 7.3, defaultDistanceCm: 35,
  });
  assert.deepEqual(SCREEN_PROFILES["tablet-11"], {
    id: "tablet-11", label: "11-inch tablet", shortWidthCm: 16.05, defaultDistanceCm: 50,
  });
  assert.deepEqual(SCREEN_PROFILES["tablet-13"], {
    id: "tablet-13", label: "13-inch tablet", shortWidthCm: 19.86, defaultDistanceCm: 50,
  });
  assert.equal(SCREEN_PROFILES.desktop.shortWidthCm, null);
  assert.ok(Object.isFrozen(SCREEN_PROFILES) && Object.isFrozen(SCREEN_PROFILES.phone));

  assert.strictEqual(resolveScreenProfile("phone"), SCREEN_PROFILES.phone);
  assert.strictEqual(resolveScreenProfile("large-phone"), SCREEN_PROFILES["large-phone"]);
  assert.strictEqual(resolveScreenProfile("auto", { screenWidth: 390, screenHeight: 844, coarsePointer: true }), SCREEN_PROFILES.phone);
  assert.strictEqual(resolveScreenProfile("auto", { screenWidth: 820, screenHeight: 1180, coarsePointer: true }), SCREEN_PROFILES["tablet-11"]);
  assert.strictEqual(resolveScreenProfile("auto", { screenWidth: 1024, screenHeight: 1366, coarsePointer: true }), SCREEN_PROFILES["tablet-13"]);
  assert.strictEqual(resolveScreenProfile("auto", { screenWidth: 390, screenHeight: 844, coarsePointer: false }), SCREEN_PROFILES.desktop);
  assert.strictEqual(resolveScreenProfile("auto"), SCREEN_PROFILES.desktop);
  assert.throws(() => resolveScreenProfile("unknown"), RangeError);
  assert.throws(() => resolveScreenProfile("auto", { screenWidth: 0, screenHeight: 500, coarsePointer: true }), RangeError);
});

test("visible physical game width follows the stage-to-screen ratio and viewport scale", () => {
  const phone = resolveScreenProfile("phone");
  assertNear(estimateGameWidthCm({
    profile: phone,
    stageCssWidth: 320,
    screenWidth: 390,
    screenHeight: 844,
    viewportScale: 1.25,
  }), 6.7 * 320 / 390 * 1.25);
  assert.equal(estimateGameWidthCm({
    profile: SCREEN_PROFILES.desktop,
    stageCssWidth: 640,
    screenWidth: 1920,
    screenHeight: 1080,
  }), 20, "desktop estimate scales its 24 cm reference with the visible stage width");
  assert.throws(() => estimateGameWidthCm({ profile: phone, stageCssWidth: 0, screenWidth: 390, screenHeight: 844 }), RangeError);
  assert.throws(() => estimateGameWidthCm({ profile: phone, stageCssWidth: 320, screenWidth: NaN, screenHeight: 844 }), RangeError);
  assert.throws(() => estimateGameWidthCm({ profile: phone, stageCssWidth: 320, screenWidth: 390, screenHeight: 844, viewportScale: 0 }), RangeError);
});

test("35 cm at a 7 cm game width preserves the legacy neutral camera", () => {
  const camera = computeViewingCamera({ distanceCm: 35, gameWidthCm: 7 });
  assert.deepEqual(camera, { x: 0, y: 0, focalDepth: (35 / 7 * 256) / 19.2 });
  assertNear(cameraDepthScale(1), 1 / (1 + PERSPECTIVE_DEPTH_FACTOR));
  assert.equal(getSceneCameraDepth(), 1 / PERSPECTIVE_DEPTH_FACTOR,
    "computing a profile does not mutate the global camera");
});

test("stationary perspective converges inside the game and keeps front artwork and physical units exact", () => {
  const previousDepth = getSceneCameraDepth();
  try {
    for (const gameWidthCm of [6.7, DESKTOP_GAME_WIDTH_CM]) {
      const pose = computeStationaryView({ gameWidthCm });
      const camera = computeViewingCamera({ distanceCm: 35, gameWidthCm, motionX: pose.x, motionY: pose.y });
      setSceneCamera(camera.x, camera.y, camera.focalDepth);
      assertNear(camera.x, .15);
      assertNear(camera.y, .10);
      const vp = { x: 256 * .65, y: 240 * .60 };
      const m = pose.rotation;
      assertNear(-m[2] / m[8], camera.x * 256 / (camera.focalDepth * 19.2));
      assertNear(-m[5] / m[8], camera.y * 240 / (camera.focalDepth * 19.2));
      for (let row = 0; row < 3; row += 1) for (let other = 0; other < 3; other += 1) {
        assertNear(m.slice(row * 3, row * 3 + 3).reduce((sum, v, i) => sum + v * m[other * 3 + i], 0), row === other ? 1 : 0);
      }
      for (const point of [{ x: 32, y: 32 }, { x: 128, y: 120 }, { x: 224, y: 208 }]) {
        assert.deepEqual(projectDepthPoint(point, 0), point);
        for (const depthPixels of [2, 8, 85]) {
          const back = projectDepthPoint(point, depthPixels / 19.2);
          const t = depthPixels / (camera.focalDepth * 19.2 + depthPixels);
          assertNear(back.x, point.x + (vp.x - point.x) * t);
          assertNear(back.y, point.y + (vp.y - point.y) * t);
        }
      }
      const left = projectDepthPoint({ x: 32, y: 120 }, 8 / 19.2);
      const right = projectDepthPoint({ x: 224, y: 120 }, 8 / 19.2);
      assert.ok(left.x > 32 && right.x < 224, "opposite sides converge, rather than sharing an isometric offset");
      if (gameWidthCm === DESKTOP_GAME_WIDTH_CM) {
        const edge = projectDepthPoint({ x: 32, y: 32 }, 2 / 19.2);
        assert.ok(edge.x - 32 > .7 && edge.y - 32 > .5, "desktop two-pixel card edges remain visible");
      }
      assert.ok(Object.isFrozen(pose) && Object.isFrozen(m));
    }
    assert.throws(() => computeStationaryView({ gameWidthCm: 1000 }), RangeError);
  } finally {
    setSceneCamera(0, 0, previousDepth);
  }
});

test("stationary pose and lamp rotation are independent of viewing distance while perspective remains finite", () => {
  const pose = computeStationaryView({ gameWidthCm: DESKTOP_GAME_WIDTH_CM });
  const rotatedZ = pose.rotation.slice(2).filter((_, index) => index % 3 === 0);
  for (const distanceCm of [20, 35, 50, 100]) {
    const camera = computeViewingCamera({ distanceCm, gameWidthCm: DESKTOP_GAME_WIDTH_CM, motionX: pose.x, motionY: pose.y });
    assertNear(camera.x, .15 * distanceCm / 35);
    assertNear(camera.y, .10 * distanceCm / 35);
    assert.ok(camera.x < .5 && camera.y < .5, "the vanishing point stays inside the game");
    assertNear(-rotatedZ[0] / rotatedZ[2], camera.x * 256 / (camera.focalDepth * 19.2));
  }
});

test("distance, physical width, tilt, and aspect produce coherent camera rays", () => {
  const cameraDefault = 1 / PERSPECTIVE_DEPTH_FACTOR;
  const cases = [
    { distanceCm: 35, gameWidthCm: 6.7, motionX: 0.3, motionY: -0.18, width: 256, height: 240 },
    { distanceCm: 50, gameWidthCm: 16.05, motionX: -0.2, motionY: 0.25, width: 512, height: 288 },
  ];

  try {
    for (const input of cases) {
      const camera = computeViewingCamera(input);
      const normal = slopes(input.motionX, input.motionY);
      const focalNative = input.distanceCm / input.gameWidthCm * 256;
      assertNear(camera.x, focalNative * normal.x / input.width);
      assertNear(camera.y, focalNative * normal.y / input.height);
      assertNear(camera.focalDepth, focalNative * normal.z / 19.2);
      assert.equal(setSceneCamera(camera.x, camera.y, camera.focalDepth), true);

      const vanishing = { x: input.width / 2 + focalNative * normal.x, y: input.height / 2 + focalNative * normal.y };
      const cameraPoint = {
        x: vanishing.x,
        y: vanishing.y,
        z: -focalNative * normal.z,
      };
      const worldPoint = { x: input.width * 0.18, y: input.height * 0.77 };
      assert.deepEqual(projectDepthPoint(worldPoint, 0, input.width, input.height), worldPoint);

      for (const depth of [0.1, 1.5, 6]) {
        const targetZ = depth * 19.2;
        const rayFraction = -cameraPoint.z / (targetZ - cameraPoint.z);
        const intersection = {
          x: cameraPoint.x + (worldPoint.x - cameraPoint.x) * rayFraction,
          y: cameraPoint.y + (worldPoint.y - cameraPoint.y) * rayFraction,
        };
        const projected = projectDepthPoint(worldPoint, depth, input.width, input.height);
        assertNear(projected.x, intersection.x);
        assertNear(projected.y, intersection.y);
        assertNear(cameraDepthScale(depth), rayFraction);
      }
    }
  } finally {
    setSceneCamera(0, 0, cameraDefault);
  }
});

test("viewing camera validates physical distance, width, motion, focal depth, and viewport bounds", () => {
  for (const distanceCm of [19.9, 100.1, NaN, Infinity]) {
    assert.throws(() => computeViewingCamera({ distanceCm, gameWidthCm: 7 }), RangeError);
  }
  for (const gameWidthCm of [0, -1, NaN, Infinity]) {
    assert.throws(() => computeViewingCamera({ distanceCm: 35, gameWidthCm }), RangeError);
  }
  for (const motionX of [NaN, Infinity]) {
    assert.throws(() => computeViewingCamera({ distanceCm: 35, gameWidthCm: 7, motionX }), RangeError);
  }
  assert.throws(() => computeViewingCamera({ distanceCm: 35, gameWidthCm: 7, width: 0 }), RangeError);
  assert.throws(() => computeViewingCamera({ distanceCm: 100, gameWidthCm: 0.01 }), RangeError,
    "the computed view must fit the shared scene-camera bounds");
});

test("rendered pose keeps camera and measured eye coherent across arbitrary look tilts and strengths", () => {
  const motions = [
    [{ alpha: 83, beta: 37, gamma: -22 }, { alpha: 83, beta: 48, gamma: -8 }],
    [{ alpha: 0, beta: 90, gamma: 0 }, { alpha: 85, beta: 90, gamma: 0 }],
    [{ alpha: 217, beta: 71, gamma: -18 }, { alpha: 249, beta: 83, gamma: -5 }],
    [{ alpha: 31, beta: 50, gamma: 12 }, { alpha: 80, beta: 62, gamma: 29 }],
  ];
  for (const strength of [0, 25, 50, 75, 100]) {
    for (const [baseline, current] of motions) {
      const measured = relativeLookPose(baseline, current, 0, 0, strength);
      const rendered = resolveRenderedScenePose(measured);
      assert.equal(rendered.x, measured.x);
      assert.equal(rendered.y, measured.y);
      assertProperRotation(rendered.rotation, 1e-8);
      if (measured.rotation[8] >= 0.1 || measured.rotation[8] <= 0) assertRenderedEye(rendered);
    }
  }
});

test("rendered pose preserves forward strength-50, stationary, and pure-roll matrices exactly", () => {
  const baseline = { alpha: 217, beta: 71, gamma: -18 };
  const current = { alpha: 249, beta: 83, gamma: -5 };
  const calibrated = relativeLookPose(baseline, current, 0, 0, 50);
  assert.deepEqual(resolveRenderedScenePose(calibrated).rotation, calibrated.rotation);

  const stationary = computeStationaryView({ gameWidthCm: DESKTOP_GAME_WIDTH_CM });
  assert.deepEqual(resolveRenderedScenePose(stationary).rotation, stationary.rotation);

  const roll = relativeLookPose(
    { alpha: 0, beta: 0, gamma: 0 },
    { alpha: 45, beta: 0, gamma: 0 },
  );
  assertNear(roll.x, 0);
  assertNear(roll.y, 0);
  assert.deepEqual(resolveRenderedScenePose(roll).rotation, roll.rotation);
});

test("rendered pose preserves a safe back-facing raw rotation without introducing a camera flip", () => {
  const measured = relativeLookPose(
    { alpha: 0, beta: 0, gamma: 0 },
    { alpha: 0, beta: 180, gamma: 0 },
  );
  assert.ok(measured.rotation[8] <= 0);
  const rendered = resolveRenderedScenePose(measured);
  assert.deepEqual(rendered.rotation, measured.rotation);
  assert.equal(rendered.x, measured.x);
  assert.equal(rendered.y, measured.y);
});

test("rendered correction tapers continuously through the forward-to-backward boundary", () => {
  const baseline = { alpha: 0, beta: 0, gamma: 0 };
  for (const strength of [0, 25, 50, 75, 100]) {
    for (const tracking of [relativeLookPose, relativeTiltPose]) {
      for (const direction of [1, -1]) {
        let previous = null;
        for (let step = 0; step <= 120; step += 1) {
          const angle = direction > 0 ? 84 + step * 0.1 : 96 - step * 0.1;
          const measured = tracking(baseline, { alpha: 0, beta: angle, gamma: 0 }, 0, 0, strength);
          const rendered = resolveRenderedScenePose(measured);
          assertProperRotation(rendered.rotation, 1e-8);
          if (previous) {
            assert.ok(rotationDifference(previous, rendered.rotation) < 3 * Math.PI / 180,
              `${tracking.name} strength ${strength} had a boundary jump near ${angle}°`);
          }
          previous = rendered.rotation;
        }
      }
    }
  }
});

test("the shared rendered rotation preserves eye-lamp distance and inverse-recovers both points", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const placement = { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: 14, gameWidthCm: 7 };
  const initialOrientation = { alpha: 83, beta: 37, gamma: -22 };
  const currentOrientation = { alpha: 121, beta: 56, gamma: -4 };
  try {
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    setSceneLightPlacement(placement);
    const baselineLight = getSceneLightPosition();
    const lamp0 = [baselineLight.x - 128, baselineLight.y - 120, baselineLight.z * SCENE_DEPTH_TO_NATIVE_PIXELS];
    const raw = relativeLookPose(initialOrientation, currentOrientation, 0, 0, 100);
    const rendered = resolveRenderedScenePose(raw);
    const { renderedEye, focalNative } = assertRenderedEye(rendered);
    const eye0 = [0, 0, -focalNative];
    const transformedLamp = multiplyVector(rendered.rotation, lamp0);
    const measuredEye = multiplyVector(rendered.rotation, eye0);
    const initialDistance = Math.hypot(...eye0.map((value, index) => value - lamp0[index]));
    const renderedDistance = Math.hypot(...measuredEye.map((value, index) => value - transformedLamp[index]));
    assertNear(renderedDistance, initialDistance, 1e-8);
    measuredEye.forEach((value, index) => assertNear(value, renderedEye[index], 1e-8));

    const inverse = [
      rendered.rotation[0], rendered.rotation[3], rendered.rotation[6],
      rendered.rotation[1], rendered.rotation[4], rendered.rotation[7],
      rendered.rotation[2], rendered.rotation[5], rendered.rotation[8],
    ];
    multiplyVector(inverse, measuredEye).forEach((value, index) => assertNear(value, eye0[index], 1e-8));
    multiplyVector(inverse, transformedLamp).forEach((value, index) => assertNear(value, lamp0[index], 1e-8));
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("rendered-pose resolver validates finite offsets and proper input rotations", () => {
  const implicitIdentity = resolveRenderedScenePose({ x: 0, y: 0 });
  assert.deepEqual(implicitIdentity.rotation, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
  assert.ok(Object.isFrozen(implicitIdentity) && Object.isFrozen(implicitIdentity.rotation));
  assertRenderedEye(resolveRenderedScenePose({ x: 0.2, y: -0.1 }));
  for (const invalid of [null, {}, { x: NaN, y: 0 }, { x: 0, y: Infinity }]) {
    assert.throws(() => resolveRenderedScenePose(invalid));
  }
  assert.throws(() => resolveRenderedScenePose({ x: Number.MAX_VALUE, y: 0 }), RangeError);
  for (const rotation of [
    [1, 0, 0, 0, 1, 0, 0, 0],
    [1, 0, 0, 0, 1, 0, 0, 0, Infinity],
    [2, 0, 0, 0, 1, 0, 0, 0, 1],
    [1, 0, 0, 0, 1, 0, 0, 0, -1],
  ]) {
    assert.throws(() => resolveRenderedScenePose({ x: 0, y: 0, rotation }));
  }
});
