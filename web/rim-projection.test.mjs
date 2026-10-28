import test from "node:test";
import assert from "node:assert/strict";
import { calculateRimProjection, projectRimBackPoint } from "./rim-projection.js";
import { cameraDepthScale } from "./depth-mesh.js";
import { shadowLightProjection } from "./scene-lighting.js";
import { setSceneLightRotation } from "./scene-lighting.js";
import { getSceneVanishingPoint, getSceneViewOffset, setSceneView } from "./scene-view.js";

function projected(point, depth, width, height) {
  const scale = cameraDepthScale(depth);
  const vanishingPoint = getSceneVanishingPoint(width, height);
  return {
    x: vanishingPoint.x + (point.x - vanishingPoint.x) * scale,
    y: vanishingPoint.y + (point.y - vanishingPoint.y) * scale,
  };
}

function rotationXForLightDepth(depth) {
  const baseY = -168, baseZ = -921.6;
  const phase = Math.atan2(baseZ, baseY);
  const targetPhase = -Math.PI - Math.asin(depth * 19.2 / Math.hypot(baseY, baseZ));
  const angle = targetPhase - phase;
  const c = Math.cos(angle), s = Math.sin(angle);
  return [1, 0, 0, 0, c, -s, 0, s, c];
}

test("camera-only aperture projection stays centered and contact has no light spread", () => {
  for (const gap of [0, 1.5, 3, 6]) {
    const projection = calculateRimProjection(256, 240, gap);
    const scale = cameraDepthScale(gap);
    assert.equal(projection.receiverScale, scale);
    assert.equal(projection.apertureScale, projection.receiverScale * projection.lightMagnification);
    assert.ok(Math.abs(projection.farRect.x + projection.farRect.width / 2 - 128) < 1e-10);
    assert.ok(Math.abs(projection.farRect.y + projection.farRect.height / 2 - 120) < 1e-10);
    assert.ok(projection.farRect.width <= 256 && projection.farRect.height <= 240);
  }

  const gap = 3;
  const contact = calculateRimProjection(256, 240, gap, gap, gap, 0);
  assert.equal(contact.castsShadow, true);
  assert.equal(contact.rimBackDepth, gap);
  assert.equal(contact.travel, 0);
  assert.equal(contact.lightMagnification, 1);
  assert.equal(contact.blurDistance, 0);
  assert.deepEqual(contact.litOpening, contact.farRect);
});

test("rim back points always use perspective camera projection", () => {
  const point = { x: 0, y: 240 };
  for (const gap of [3, 6]) {
    for (const frontDepth of [0, gap / 4, gap]) {
      for (const thickness of [0, (gap - frontDepth) / 2, gap - frontDepth]) {
        const backDepth = frontDepth + thickness;
        const projection = calculateRimProjection(256, 240, gap, gap, thickness, frontDepth);
        assert.equal(projection.rimFrontDepth, frontDepth);
        assert.equal(projection.rimBackDepth, backDepth);
        assert.equal(projection.rimThickness, thickness);
        assert.deepEqual(projectRimBackPoint(point, gap, 256, 240, thickness, frontDepth),
          projected(point, backDepth, 256, 240));
      }
    }
  }
});

test("nearby point light enlarges the aperture before receiver camera projection", () => {
  const width = 256;
  const height = 240;
  const gap = 4;
  const frontDepth = 0.5;
  const thickness = 1.25;
  const receiverDepth = gap;
  const projection = calculateRimProjection(width, height, gap, receiverDepth, thickness, frontDepth);
  const rimBackDepth = frontDepth + thickness;
  const light = shadowLightProjection(rimBackDepth, receiverDepth, width, height);
  const receiverScale = cameraDepthScale(receiverDepth);
  const apertureScale = receiverScale * light.magnification;

  assert.equal(projection.castsShadow, true);
  assert.equal(projection.travel, light.travel);
  assert.equal(projection.lightMagnification, light.magnification);
  assert.equal(projection.apertureScale, apertureScale);
  assert.ok(projection.apertureScale > projection.receiverScale);
  assert.ok(projection.litOpening.width > projection.farRect.width);
  assert.ok(projection.litOpening.height > projection.farRect.height);
  assert.ok(Math.abs(projection.litOpening.x
    - (width / 2 + (projection.farRect.x - width / 2) * light.magnification + light.x * receiverScale)) < 1e-10);
  assert.ok(Math.abs(projection.litOpening.y
    - (height / 2 + (projection.farRect.y - height / 2) * light.magnification + light.y * receiverScale)) < 1e-10);
  assert.equal(projection.litOpening.width, width * apertureScale);
  assert.equal(projection.litOpening.height, height * apertureScale);
  assert.equal(projection.blurDistance, light.travel * receiverScale);
});

test("receivers before the rim back cast no shadow and avoid reversed light projection", () => {
  const gap = 5;
  const thickness = 2;
  const frontDepth = 0.5;
  const rimBackDepth = frontDepth + thickness;
  const before = calculateRimProjection(256, 240, gap, rimBackDepth - 0.25, thickness, frontDepth);
  const atContact = calculateRimProjection(256, 240, gap, rimBackDepth, thickness, frontDepth);
  const beyond = calculateRimProjection(256, 240, gap, gap, thickness, frontDepth);

  assert.equal(before.castsShadow, false);
  assert.equal(before.travel, 0);
  assert.equal(before.lightMagnification, 1);
  assert.equal(before.blurDistance, 0);
  assert.deepEqual(before.litOpening, before.farRect);
  assert.equal(atContact.castsShadow, true);
  assert.equal(atContact.travel, 0);
  assert.deepEqual(atContact.litOpening, atContact.farRect);
  assert.equal(beyond.castsShadow, true);
  assert.ok(beyond.travel > 0);
  assert.ok(beyond.blurDistance > 0);
});

test("projection rejects invalid dimensions, distances, thickness, and point coordinates", () => {
  assert.throws(() => calculateRimProjection(0, 240, 3), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 7), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 3, 4), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 3, 3, -0.1), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 3, 3, 3.1), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 3, 3, 0, -0.1), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 3, 3, 0, 3.1), RangeError);
  assert.throws(() => calculateRimProjection(256, 240, 3, 3, 1.1, 2), RangeError);
  assert.throws(() => projectRimBackPoint({ x: 1, y: Infinity }, 3), TypeError);
  assert.throws(() => projectRimBackPoint({ x: 1, y: 2 }, 3, 256, 240, 3.1), RangeError);
  assert.throws(() => projectRimBackPoint({ x: 1, y: 2 }, 3, 256, 240, 1.1, 2), RangeError);
});

test("off-axis camera moves the far aperture once while keeping the light spread in world space", () => {
  const width = 256, height = 240, gap = 4, receiverDepth = 4;
  const original = { x: 0, y: 0 };
  try {
    const centered = calculateRimProjection(width, height, gap, receiverDepth, 1.25, 0.5);
    setSceneView(0.24, -0.18);
    const offset = getSceneViewOffset(width, height);
    const vanishingPoint = getSceneVanishingPoint(width, height);
    const projection = calculateRimProjection(width, height, gap, receiverDepth, 1.25, 0.5);
    const scale = cameraDepthScale(receiverDepth);
    const light = shadowLightProjection(0.5 + 1.25, receiverDepth, width, height);
    const apertureScale = scale * light.magnification;

    assert.notDeepEqual(projection.farRect, centered.farRect);
    assert.ok(Math.abs(projection.farRect.x - vanishingPoint.x * (1 - scale)) < 1e-10);
    assert.ok(Math.abs(projection.farRect.y - vanishingPoint.y * (1 - scale)) < 1e-10);
    assert.ok(Math.abs(projection.litOpening.x - (width / 2 - width / 2 * apertureScale
      + light.x * scale + offset.x * (1 - scale))) < 1e-10);
    assert.ok(Math.abs(projection.litOpening.y - (height / 2 - height / 2 * apertureScale
      + light.y * scale + offset.y * (1 - scale))) < 1e-10);

    const contact = calculateRimProjection(width, height, gap, 2, 1.5, 0.5);
    assert.deepEqual(contact.litOpening, contact.farRect, "contact preserves the moved aperture without extra light spread");
    const sourcePoint = { x: 0, y: height };
    assert.deepEqual(projected(sourcePoint, 0, width, height), sourcePoint,
      "the source plane stays fixed while the off-axis vanishing point moves");

    setSceneView(...Object.values(original));
    const centeredAgain = calculateRimProjection(width, height, gap, receiverDepth, 1.25, 0.5);
    assert.deepEqual(centeredAgain, centered);
  } finally {
    setSceneView(0, 0);
  }
});

test("rim shadows fade at grazing incidence and disappear when the light is behind the rim", () => {
  const identity = [1, 0, 0, 0, 1, 0, 0, 0, 1];
  const behind = [-1, 0, 0, 0, 1, 0, 0, 0, -1];
  try {
    setSceneLightRotation(behind);
    const blocked = calculateRimProjection(256, 240, 6, 6, 3, 0);
    assert.equal(blocked.castsShadow, false);
    assert.equal(blocked.opacity, 0);
    assert.equal(blocked.travel, 0);
    assert.equal(blocked.blurDistance, 0);
    assert.deepEqual(blocked.litOpening, blocked.farRect);
    assert.ok(Object.values(blocked.farRect).every(Number.isFinite));

    // The full rim back is at z=3; z=2.5 gives a grazing ratio t=6 to the
    // receiver at z=6, which remains finite and visibly fades.
    setSceneLightRotation(rotationXForLightDepth(2.5));
    const grazing = calculateRimProjection(256, 240, 6, 6, 3, 0);
    assert.equal(grazing.castsShadow, true);
    assert.ok(grazing.opacity > 0 && grazing.opacity < 1);
    assert.ok(grazing.lightMagnification <= 9);
    assert.ok(Object.values(grazing.litOpening).every(Number.isFinite));
  } finally {
    setSceneLightRotation(identity);
  }
});
