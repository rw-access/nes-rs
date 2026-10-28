import test from "node:test";
import assert from "node:assert/strict";
import { cameraDepthScale, PERSPECTIVE_DEPTH_FACTOR, projectDepthPoint } from "./depth-mesh.js";
import { getSceneCameraDepth, setSceneCamera } from "./scene-view.js";
import { computeViewingCamera, estimateGameWidthCm, resolveScreenProfile, SCREEN_PROFILES, computeStationaryView, DESKTOP_GAME_WIDTH_CM } from "./viewing-model.js";

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
