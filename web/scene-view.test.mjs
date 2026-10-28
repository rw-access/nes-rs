import test from "node:test";
import assert from "node:assert/strict";
import {
  getSceneCameraDepth,
  getSceneVanishingPoint,
  getSceneView,
  getSceneViewOffset,
  getSceneViewRevision,
  setSceneCamera,
  setSceneView,
} from "./scene-view.js";

test("scene view is immutable, bounded, revisioned, and expressed in viewport pixels", () => {
  assert.deepEqual(getSceneView(), { x: 0, y: 0 });
  assert.equal(getSceneViewRevision(), 0);
  assert.equal(setSceneView(0, 0), false);
  assert.equal(getSceneViewRevision(), 0);

  assert.equal(setSceneView(0.25, -0.4), true);
  assert.deepEqual(getSceneView(), { x: 0.25, y: -0.4 });
  assert.ok(Object.isFrozen(getSceneView()));
  assert.equal(getSceneViewRevision(), 1);
  const vanishingPoint = getSceneVanishingPoint();
  assert.equal(vanishingPoint.x, 192);
  assert.ok(Math.abs(vanishingPoint.y - 24) < 1e-12);
  assert.deepEqual(getSceneViewOffset(), { x: 64, y: -96 });
  const widerVanishingPoint = getSceneVanishingPoint(320, 200);
  assert.equal(widerVanishingPoint.x, 240);
  assert.ok(Math.abs(widerVanishingPoint.y - 20) < 1e-12);
  assert.deepEqual(getSceneViewOffset(320, 200), { x: 80, y: -80 });
  assert.throws(() => { getSceneView().x = 0; }, TypeError);

  assert.equal(setSceneView(0.25, -0.4), false);
  assert.equal(getSceneViewRevision(), 1);
  assert.equal(setSceneView(-64, 64), true);
  assert.equal(getSceneViewRevision(), 2);
  const boundaryVanishingPoint = getSceneVanishingPoint(10, 20);
  assert.equal(boundaryVanishingPoint.x, -635);
  assert.equal(boundaryVanishingPoint.y, 1290);

  for (const [x, y] of [[-64.0001, 0], [64.0001, 0], [0, -64.0001], [0, 64.0001],
    [NaN, 0], [0, Infinity], ["0", 0]]) {
    const view = getSceneView();
    const revision = getSceneViewRevision();
    assert.throws(() => setSceneView(x, y), RangeError);
    assert.strictEqual(getSceneView(), view);
    assert.equal(getSceneViewRevision(), revision);
  }
  for (const dimensions of [[0, 1], [1, 0], [1.5, 2], [2, Infinity]]) {
    assert.throws(() => getSceneVanishingPoint(...dimensions), RangeError);
    assert.throws(() => getSceneViewOffset(...dimensions), RangeError);
  }

  assert.equal(setSceneView(0, 0), true);
  assert.deepEqual(getSceneView(), { x: 0, y: 0 });
  assert.equal(getSceneViewRevision(), 3);
  assert.equal(getSceneCameraDepth(), 1 / 0.015);
});

test("camera updates are atomic and view-only changes preserve calibrated focal depth", () => {
  const revision = getSceneViewRevision();
  const focalDepth = getSceneCameraDepth();
  assert.equal(setSceneCamera(0.2, -0.3, 42), true);
  assert.deepEqual(getSceneView(), { x: 0.2, y: -0.3 });
  assert.equal(getSceneCameraDepth(), 42);
  assert.equal(getSceneViewRevision(), revision + 1, "one camera update increments one revision");
  assert.equal(setSceneCamera(0.2, -0.3, 42), false);
  assert.equal(getSceneViewRevision(), revision + 1);

  assert.equal(setSceneView(0.4, 0.1), true);
  assert.equal(getSceneCameraDepth(), 42, "motion preserves physical viewing distance");
  assert.equal(getSceneViewRevision(), revision + 2);

  for (const [x, y, depth] of [[65, 0, 42], [0, -65, 42], [0.1, 0.2, 0.099],
    [0.1, 0.2, 10_000.1], [0.1, 0.2, NaN], [0.1, 0.2, Infinity]]) {
    const priorView = getSceneView();
    const priorDepth = getSceneCameraDepth();
    const priorRevision = getSceneViewRevision();
    assert.throws(() => setSceneCamera(x, y, depth), RangeError);
    assert.strictEqual(getSceneView(), priorView, "failed camera updates leave the view untouched");
    assert.equal(getSceneCameraDepth(), priorDepth);
    assert.equal(getSceneViewRevision(), priorRevision);
  }

  assert.equal(setSceneCamera(0, 0, focalDepth), true);
  assert.equal(getSceneCameraDepth(), focalDepth);
});
