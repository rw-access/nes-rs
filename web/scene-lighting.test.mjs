import test from "node:test";
import assert from "node:assert/strict";
import {
  SCENE_LIGHT,
  backdropIrradiance,
  backdropIrradianceFromLight,
  createBackdropIrradianceSampler,
  getSceneLightPlacement,
  getSceneLightDistance,
  getSceneLightPosition,
  getSceneLightRevision,
  getSceneLightRotation,
  shadowLightProjection,
  setSceneLightPlacement,
  setSceneLightDistance,
  setSceneLightRotation,
  surfaceShade,
} from "./scene-lighting.js";
import { SCENE_DEPTH_TO_NATIVE_PIXELS } from "./viewing-model.js";

const orientations = ["top", "right", "bottom", "left"];

function rotationX(angle) {
  const c = Math.cos(angle), s = Math.sin(angle);
  return [1, 0, 0, 0, c, -s, 0, s, c];
}

function rotationY(angle) {
  const c = Math.cos(angle), s = Math.sin(angle);
  return [c, 0, s, 0, 1, 0, -s, 0, c];
}

function rotationZ(angle) {
  const c = Math.cos(angle), s = Math.sin(angle);
  return [c, -s, 0, s, c, 0, 0, 0, 1];
}

function rotationXForLightDepth(depth, distance = 2) {
  const y = -84 * distance;
  const z = -24 * distance * 19.2;
  const radius = Math.hypot(y, z);
  const phase = Math.atan2(z, y);
  const target = depth * 19.2;
  const targetPhase = -Math.PI - Math.asin(target / radius);
  return rotationX(targetPhase - phase);
}

test("the frontal light sits near the top and centered horizontally", () => {
  assert.deepEqual(SCENE_LIGHT, { xFraction: 0.5, yFraction: 0.15, height: 24 });
  assert.equal(getSceneLightDistance(), 2);
  assert.equal(getSceneLightRevision(), 0);
  assert.deepEqual(getSceneLightPosition(), { x: 128, y: -48, z: -48 });
  for (const [width, height] of [[256, 240], [512, 480], [160, 144]]) {
    const projection = shadowLightProjection(0, 3, width, height);
    assert.equal(projection.x, 0);
    assert.ok(projection.y > 0 && projection.y < height * 0.05);
    const project = point => ({
      x: width / 2 + (point.x - width / 2) * projection.magnification + projection.x,
      y: height / 2 + (point.y - height / 2) * projection.magnification + projection.y,
    });
    assert.ok(project({ x: 0, y: 0 }).x < 0, "left-hand shadows spread left");
    assert.ok(project({ x: width, y: height }).x > width, "right-hand shadows spread right");
    assert.ok(project({ x: 0, y: 0 }).y > 0, "the centered ray moves the upper edge down");
    assert.ok(project({ x: width, y: height }).y > height, "shadows below the light spread down");
  }
});

test("contact has no ray spread, while farther receivers enlarge and travel farther", () => {
  const contact = shadowLightProjection(3, 3);
  const near = shadowLightProjection(1.5, 3);
  const far = shadowLightProjection(0, 3);
  assert.deepEqual(contact, { t: 0, magnification: 1, x: 0, y: 0, travel: 0 });
  assert.ok(near.magnification > 1 && far.magnification > near.magnification);
  assert.equal(near.x, 0);
  assert.equal(far.x, 0);
  assert.ok(near.y > 0 && far.y > near.y);
  assert.ok(near.travel > 0 && far.travel > near.travel);
});

test("increasing caster depth reduces magnification for a fixed receiver", () => {
  const thin = shadowLightProjection(0.5, 4);
  const thick = shadowLightProjection(2, 4);
  assert.ok(thin.magnification > thick.magnification);
  assert.equal(thin.x, thick.x);
  assert.ok(thin.y > thick.y);
});

test("distance changes preserve the center ray while reducing shadow variation across the screen", () => {
  const originalDistance = getSceneLightDistance();
  try {
    const projections = new Map();
    const topFaceShades = new Map();
    for (const distance of [1, 2, 8]) {
      setSceneLightDistance(distance);
      const light = getSceneLightPosition();
      topFaceShades.set(distance, surfaceShade("top", false, { x: 128, y: 0, z: 0 }));
      const center = { x: 128, y: 120, z: 0 };
      const centerRay = { x: center.x - light.x, y: center.y - light.y, z: center.z - light.z };
      assert.ok(Math.abs(centerRay.x) < 1e-12);
      assert.ok(Math.abs(centerRay.y / centerRay.z - 3.5) < 1e-12);

      const caster = 0.5, receiver = 5;
      const projection = shadowLightProjection(caster, receiver);
      projections.set(distance, projection);
      const t = (receiver - caster) / (caster - light.z);
      for (const point of [{ x: 0, y: 0 }, { x: 128, y: 120 }, { x: 240, y: 220 }]) {
        const shadow = {
          x: 128 + (point.x - 128) * projection.magnification + projection.x,
          y: 120 + (point.y - 120) * projection.magnification + projection.y,
          z: receiver,
        };
        const ray = { x: point.x - light.x, y: point.y - light.y, z: caster - light.z };
        const distanceRatio = (shadow.z - light.z) / ray.z;
        assert.ok(Math.abs(shadow.x - light.x - ray.x * distanceRatio) < 1e-10);
        assert.ok(Math.abs(shadow.y - light.y - ray.y * distanceRatio) < 1e-10);
      }
    }
    assert.ok(projections.get(1).magnification > projections.get(2).magnification);
    assert.ok(projections.get(2).magnification > projections.get(8).magnification);
    assert.ok(projections.get(1).t > projections.get(2).t && projections.get(2).t > projections.get(8).t);
    assert.ok(topFaceShades.get(8) > topFaceShades.get(1), "surface lighting follows the moved light");
  } finally {
    if (getSceneLightDistance() !== originalDistance) setSceneLightDistance(originalDistance);
  }
});

test("distance setter validates, reports changes, and revisions only real updates", () => {
  const originalDistance = getSceneLightDistance();
  const originalRevision = getSceneLightRevision();
  try {
    assert.equal(setSceneLightDistance(originalDistance), false);
    assert.equal(getSceneLightRevision(), originalRevision);
    assert.equal(setSceneLightDistance(1), originalDistance !== 1);
    if (originalDistance !== 1) assert.equal(getSceneLightRevision(), originalRevision + 1);
    assert.equal(setSceneLightDistance(1), false);
    const afterNoop = getSceneLightRevision();
    assert.equal(setSceneLightDistance(8), true);
    assert.equal(getSceneLightRevision(), afterNoop + 1);
    for (const invalid of [0.99, 8.01, NaN, Infinity, -Infinity]) {
      const distance = getSceneLightDistance();
      const revision = getSceneLightRevision();
      assert.throws(() => setSceneLightDistance(invalid), RangeError);
      assert.equal(getSceneLightDistance(), distance);
      assert.equal(getSceneLightRevision(), revision);
    }
  } finally {
    if (getSceneLightDistance() !== originalDistance) setSceneLightDistance(originalDistance);
  }
});

test("wall shade remains shared and bounded by ambient plus diffuse light", () => {
  assert.equal(surfaceShade("top"), surfaceShade("bottom", true));
  assert.equal(surfaceShade("right"), surfaceShade("left", true));
  assert.equal(surfaceShade("bottom"), surfaceShade("top", true));
  assert.equal(surfaceShade("left"), surfaceShade("right", true));
  for (const orientation of orientations) {
    for (const inward of [false, true]) {
      const shade = surfaceShade(orientation, inward);
      assert.ok(Number.isFinite(shade));
      assert.ok(shade >= 0.45 && shade <= 0.77);
    }
  }

  const normals = {
    top: [0, -1], right: [1, 0], bottom: [0, 1], left: [-1, 0],
  };
  for (const [width, height, point] of [
    [256, 240, { x: 37, y: 193, z: 1.25 }],
    [512, 480, { x: 418, y: 61, z: 3.5 }],
    [160, 144, { x: 24, y: 112, z: 0 }],
  ]) {
    const light = getSceneLightPosition(width, height);
    const referenceRadius = Math.hypot(
      (light.x - width / 2) * 256 / width,
      (light.y - height / 2) * 240 / height,
      light.z * SCENE_DEPTH_TO_NATIVE_PIXELS,
    );
    const dx = (light.x - point.x) * 256 / width;
    const dy = (light.y - point.y) * 240 / height;
    const dz = (light.z - point.z) * 19.2;
    const length = Math.hypot(dx, dy, dz);
    for (const orientation of orientations) {
      for (const inward of [false, true]) {
        const direction = inward ? -1 : 1;
        const [nx, ny] = normals[orientation];
        const cosine = Math.max(0, direction * (nx * dx + ny * dy) / length);
        const attenuation = Math.min(1, (referenceRadius / length) ** 2);
        const expected = 0.45 + 0.32 * cosine * attenuation;
        assert.ok(Math.abs(surfaceShade(orientation, inward, point, width, height) - expected) < 1e-12,
          "diffuse shading uses the isotropic point-light vector");
      }
    }
  }
});

test("side illumination follows position relative to the same frontal light", () => {
  const left = { x: 32, y: -48, z: 1 };
  const right = { x: 224, y: -48, z: 1 };
  assert.ok(surfaceShade("right", false, left) > surfaceShade("right", false, right));
  assert.ok(surfaceShade("left", false, right) > surfaceShade("left", false, left));
  assert.equal(surfaceShade("right", false, right), 0.45);
  assert.equal(surfaceShade("left", false, left), 0.45);
  assert.ok(surfaceShade("bottom", false, { x: 128, y: -60 }) > 0.45);
  assert.ok(surfaceShade("top", false, { x: 128, y: 216 }) > 0.45);
  assert.ok(surfaceShade("right", false, left) > surfaceShade("right", false, { ...left, z: 6 }),
    "the full three-dimensional light vector determines illumination");
  for (const point of [left, right]) {
    assert.equal(surfaceShade("left", true, point), surfaceShade("right", false, point));
  }
});

test("rejects invalid light projections and wall orientations", () => {
  for (const [width, height] of [[0, 240], [256, 0], [2.5, 240]]) {
    assert.throws(() => shadowLightProjection(0, 3, width, height), RangeError);
  }
  for (const depths of [[-0.1, 1], [0, 6.1], [3, 2], [NaN, 1], [0, Infinity]]) {
    assert.throws(() => shadowLightProjection(...depths), RangeError);
  }
  assert.throws(() => surfaceShade("diagonal"), RangeError);
  assert.throws(() => surfaceShade("top", 1), TypeError);
  assert.throws(() => surfaceShade("top", false, { x: NaN, y: 1 }), TypeError);
  assert.throws(() => surfaceShade("top", false, { x: 1, y: Infinity }), TypeError);
  assert.throws(() => surfaceShade("top", false, { x: 1, y: 1, z: -1 }), RangeError);
  assert.throws(() => surfaceShade("top", false, { x: 1, y: 1, z: 7 }), RangeError);
  assert.throws(() => surfaceShade("top", false, null, 0, 240), RangeError);
});

test("room rotation maps the baseline fixture through proper rotations and scales canonical axes", () => {
  const originalDistance = getSceneLightDistance();
  const originalRotation = [...getSceneLightRotation()];
  try {
    setSceneLightDistance(3.25);
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    assert.deepEqual(getSceneLightPosition(), {
      x: 128,
      y: 120 + (36 - 120) * 3.25,
      z: -24 * 3.25,
    }, "identity preserves the prior fixture exactly");

    const roll = rotationZ(Math.PI / 2);
    const beforeRollRevision = getSceneLightRevision();
    assert.equal(setSceneLightRotation(roll), true);
    assert.equal(getSceneLightRevision(), beforeRollRevision + 1);
    const rotated = getSceneLightPosition();
    const baseY = -84 * 3.25;
    assert.ok(Math.abs(rotated.x - (128 - baseY)) < 1e-10);
    assert.ok(Math.abs(rotated.y - 120) < 1e-10);
    assert.ok(Math.abs(rotated.z + 24 * 3.25) < 1e-10);
    assert.equal(setSceneLightRotation(roll), false);
    assert.equal(getSceneLightRevision(), beforeRollRevision + 1, "no-op rotations do not invalidate caches");
    assert.ok(Object.isFrozen(getSceneLightRotation()));
    const capturedRoll = [...getSceneLightRotation()];
    roll[0] = 99;
    assert.deepEqual(getSceneLightRotation(), capturedRoll, "the setter snapshots caller-owned arrays");

    const pose = rotationZ(0.57);
    const yaw = rotationY(-0.43);
    const composed = Array.from({ length: 9 }, (_, index) => {
      const row = Math.floor(index / 3), column = index % 3;
      return yaw[row * 3] * pose[column]
        + yaw[row * 3 + 1] * pose[3 + column]
        + yaw[row * 3 + 2] * pose[6 + column];
    });
    setSceneLightRotation(composed);
    const [width, height] = [512, 480];
    const light = getSceneLightPosition(width, height);
    const currentCanonical = [
      (light.x - width / 2) * 256 / width,
      (light.y - height / 2) * 240 / height,
      light.z * 19.2,
    ];
    const baseline = [0, -84 * 3.25, -24 * 3.25 * 19.2];
    const recovered = [0, 0, 0].map((_, column) =>
      composed[column] * currentCanonical[0]
        + composed[3 + column] * currentCanonical[1]
        + composed[6 + column] * currentCanonical[2]);
    recovered.forEach((value, index) => assert.ok(Math.abs(value - baseline[index]) < 1e-9,
      "inverse room rotation recovers the fixed baseline light"));
    assert.ok(Number.isFinite(light.x) && Number.isFinite(light.y) && Number.isFinite(light.z));
  } finally {
    setSceneLightRotation(originalRotation);
    setSceneLightDistance(originalDistance);
  }
});

test("rotation setter rejects nonfinite, nonorthogonal, and reflected matrices without revisions", () => {
  const rotation = [...getSceneLightRotation()];
  const revision = getSceneLightRevision();
  for (const invalid of [
    null,
    [1, 0, 0, 0, 1, 0, 0, 0],
    [1, 0, 0, 0, 1, 0, 0, 0, Infinity],
    [2, 0, 0, 0, 1, 0, 0, 0, 1],
    [1, 0, 0, 0, 1, 0, 0, 0, -1],
  ]) {
    assert.throws(() => setSceneLightRotation(invalid));
    assert.equal(getSceneLightRevision(), revision);
    assert.deepEqual(getSceneLightRotation(), rotation);
  }
});

test("near-parallel light rays fade safely and a light behind the caster casts no shadow", () => {
  const originalDistance = getSceneLightDistance();
  const originalRotation = [...getSceneLightRotation()];
  try {
    setSceneLightDistance(2);
    setSceneLightRotation(rotationXForLightDepth(-1));
    const grazing = shadowLightProjection(0, 6);
    assert.ok(Math.abs(grazing.t - 6) < 1e-10);
    assert.ok(grazing.opacity > 0 && grazing.opacity < 1);
    assert.ok(grazing.magnification <= 9);
    for (const value of Object.values(grazing)) assert.ok(Number.isFinite(value));
    const light = getSceneLightPosition();
    assert.ok(Number.isFinite(surfaceShade("top", false, { x: light.x, y: light.y, z: 0 })));

    setSceneLightRotation(rotationY(Math.PI));
    const behind = shadowLightProjection(0, 6);
    assert.equal(behind.opacity, 0);
    assert.equal(behind.t, 0);
    assert.equal(behind.magnification, 1);
    assert.equal(behind.x, 0);
    assert.equal(behind.y, 0);
    assert.ok(Object.values(behind).every(Number.isFinite));
  } finally {
    setSceneLightRotation(originalRotation);
    setSceneLightDistance(originalDistance);
  }
});

test("phone-plane placement round-trips physical distance and elevation across device sizes", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  try {
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    for (const [gameWidthCm, width, height] of [[6.7, 390, 366], [16.05, 844, 792]]) {
      for (const [distanceCm, elevationDegrees, azimuthDegrees] of [
        [25, 10, 0], [5, 0, -60], [300, 80, 60], [42, 35, 23],
      ]) {
        setSceneLightPlacement({ distanceCm, elevationDegrees, azimuthDegrees, gameWidthCm });
        const light = getSceneLightPosition(width, height);
        const pixelsPerCm = 256 / gameWidthCm;
        const canonicalX = (light.x - width / 2) * 256 / width;
        const canonicalY = (light.y - height / 2) * 240 / height;
        const canonicalZ = light.z * SCENE_DEPTH_TO_NATIVE_PIXELS;
        const recoveredDistance = Math.hypot(canonicalX, canonicalY, canonicalZ) / pixelsPerCm;
        const recoveredElevation = Math.atan2(-canonicalY, Math.hypot(canonicalX, canonicalZ)) * 180 / Math.PI;
        const recoveredAzimuth = Math.atan2(canonicalX, -canonicalZ) * 180 / Math.PI;
        assert.ok(Math.abs(recoveredDistance - distanceCm) < 1e-10);
        assert.ok(Math.abs(recoveredElevation - elevationDegrees) < 1e-10);
        assert.ok(Math.abs(recoveredAzimuth - azimuthDegrees) < 1e-10);
      }
    }
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("phone-plane placement rotates as one canonical point and preserves ray collinearity", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const placement = { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: -15, gameWidthCm: 7 };
  const first = rotationX(0.3), second = rotationY(-0.2);
  const composed = Array.from({ length: 9 }, (_, index) => {
    const row = Math.floor(index / 3), column = index % 3;
    return first[row * 3] * second[column]
      + first[row * 3 + 1] * second[3 + column]
      + first[row * 3 + 2] * second[6 + column];
  });
  try {
    setSceneLightPlacement(placement);
    setSceneLightRotation(composed);
    const canonical = (width, height) => {
      const light = getSceneLightPosition(width, height);
      return [(light.x - width / 2) * 256 / width,
        (light.y - height / 2) * 240 / height,
        light.z * SCENE_DEPTH_TO_NATIVE_PIXELS];
    };
    const atNative = canonical(256, 240);
    const atDouble = canonical(512, 480);
    atNative.forEach((value, index) => assert.ok(Math.abs(value - atDouble[index]) < 1e-10));

    const pixelsPerCm = 256 / placement.gameWidthCm;
    const elevation = placement.elevationDegrees * Math.PI / 180;
    const azimuth = placement.azimuthDegrees * Math.PI / 180;
    const baseline = [
      placement.distanceCm * Math.cos(elevation) * Math.sin(azimuth) * pixelsPerCm,
      -placement.distanceCm * Math.sin(elevation) * pixelsPerCm,
      -placement.distanceCm * Math.cos(elevation) * Math.cos(azimuth) * pixelsPerCm];
    const recovered = [0, 1, 2].map(column =>
      composed[column] * atNative[0]
        + composed[3 + column] * atNative[1]
        + composed[6 + column] * atNative[2]);
    recovered.forEach((value, index) => assert.ok(Math.abs(value - baseline[index]) < 1e-9));

    const casterDepth = 0.25, receiverDepth = 2.75;
    const projection = shadowLightProjection(casterDepth, receiverDepth);
    assert.ok(projection.t > 0 && projection.opacity === undefined,
      "the placed source remains on the viewer side for this nongrazing ray");
    const caster = { x: 37, y: 51 };
    const shadow = {
      x: 128 + (caster.x - 128) * projection.magnification + projection.x,
      y: 120 + (caster.y - 120) * projection.magnification + projection.y,
    };
    const light = getSceneLightPosition();
    const lineScale = (receiverDepth - light.z) / (casterDepth - light.z);
    assert.ok(Math.abs(shadow.x - light.x - (caster.x - light.x) * lineScale) < 1e-9);
    assert.ok(Math.abs(shadow.y - light.y - (caster.y - light.y) * lineScale) < 1e-9);
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("farther phone-plane placement reduces shadow magnification and ignores legacy distance", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const originalDistance = getSceneLightDistance();
  try {
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    setSceneLightPlacement({ distanceCm: 5, elevationDegrees: 10, gameWidthCm: 7 });
    const near = shadowLightProjection(0, 4).magnification;
    const positioned = getSceneLightPosition();
    setSceneLightDistance(originalDistance === 8 ? 7 : 8);
    assert.deepEqual(getSceneLightPosition(), positioned,
      "legacy distance controls do not move an active phone-plane placement");
    setSceneLightPlacement({ distanceCm: 300, elevationDegrees: 10, gameWidthCm: 7 });
    const far = shadowLightProjection(0, 4).magnification;
    assert.ok(near > far && far > 1);
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
    setSceneLightDistance(originalDistance);
  }
});

test("placement configuration is immutable and revisions change only for effective updates", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const originalRevision = getSceneLightRevision();
  try {
    assert.equal(setSceneLightPlacement(null), false);
    assert.equal(setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 10, gameWidthCm: 7 }), true);
    const revision = getSceneLightRevision();
    const placement = getSceneLightPlacement();
    assert.deepEqual(placement, { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: 0, gameWidthCm: 7 });
    assert.ok(Object.isFrozen(placement));
    assert.equal(setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 10, gameWidthCm: 7 }), false);
    assert.equal(getSceneLightRevision(), revision, "unchanged placement does not invalidate caches");
    assert.equal(setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 11, gameWidthCm: 7 }), true);
    assert.equal(getSceneLightRevision(), revision + 1);
    assert.equal(setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 11, azimuthDegrees: 5, gameWidthCm: 7 }), true);
    assert.equal(getSceneLightRevision(), revision + 2, "azimuth changes invalidate dependent caches");
    assert.equal(setSceneLightPlacement(null), true);
    assert.equal(getSceneLightPlacement(), null);
    assert.equal(getSceneLightRevision(), revision + 3);
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    assert.deepEqual(getSceneLightPosition(), {
      x: 128,
      y: 120 + (36 - 120) * getSceneLightDistance(),
      z: -24 * getSceneLightDistance(),
    }, "clearing placement restores the exact standalone fixture");

    const state = getSceneLightRevision();
    for (const invalid of [
      undefined,
      { distanceCm: 4.99, elevationDegrees: 10, gameWidthCm: 7 },
      { distanceCm: 300.01, elevationDegrees: 10, gameWidthCm: 7 },
      { distanceCm: NaN, elevationDegrees: 10, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: -0.01, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: 80.01, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: Infinity, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: -60.01, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: 60.01, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: NaN, gameWidthCm: 7 },
      { distanceCm: 25, elevationDegrees: 10, gameWidthCm: 0 },
      { distanceCm: 25, elevationDegrees: 10, gameWidthCm: Infinity },
      { distanceCm: 25, elevationDegrees: 10, gameWidthCm: Number.MIN_VALUE },
    ]) {
      assert.throws(() => setSceneLightPlacement(invalid));
      assert.equal(getSceneLightPlacement(), null);
      assert.equal(getSceneLightRevision(), state);
    }
    setSceneLightPlacement({ distanceCm: 300, elevationDegrees: 10, gameWidthCm: 7 });
    assert.throws(() => getSceneLightPosition(Number.MAX_VALUE, Number.MAX_VALUE), RangeError);
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
    assert.ok(getSceneLightRevision() >= originalRevision);
  }
});

test("backdrop irradiance follows the same point-light ray through phone rotation", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const placement = { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: 18, gameWidthCm: 7 };
  const rotations = [
    [1, 0, 0, 0, 1, 0, 0, 0, 1],
    rotationZ(0.7),
    [
      Math.cos(0.3), 0, Math.sin(0.3),
      Math.sin(0.2) * Math.sin(0.3), Math.cos(0.2), -Math.sin(0.2) * Math.cos(0.3),
      -Math.cos(0.2) * Math.sin(0.3), Math.sin(0.2), Math.cos(0.2) * Math.cos(0.3),
    ],
  ];
  try {
    setSceneLightPlacement(placement);
    for (const rotation of rotations) {
      setSceneLightRotation(rotation);
      for (const [width, height, point] of [
        [256, 240, { x: 22, y: 180, z: 0 }],
        [512, 480, { x: 390, y: 40, z: 1.25 }],
        [844, 792, { x: 173, y: 655, z: 4.5 }],
      ]) {
        const light = getSceneLightPosition(width, height);
        const dx = (light.x - point.x) * 256 / width;
        const dy = (light.y - point.y) * 240 / height;
        const dz = (light.z - point.z) * SCENE_DEPTH_TO_NATIVE_PIXELS;
        const length = Math.hypot(dx, dy, dz);
        const referenceRadius = placement.distanceCm * 256 / placement.gameWidthCm;
        const attenuation = Math.min(1, (referenceRadius / length) ** 2);
        const expected = 0.35 + 0.65 * Math.max(0, -dz / length) * attenuation;
        assert.ok(Math.abs(backdropIrradiance(point, width, height) - expected) < 1e-12,
          "irradiance matches an independent front-normal dot product");
        assert.equal(backdropIrradianceFromLight(point, light, width, height),
          backdropIrradiance(point, width, height), "precomputed-light helper matches the oracle");
        const sample = createBackdropIrradianceSampler(light, point.z, width, height);
        assert.ok(Math.abs(sample(point.x, point.y) - backdropIrradiance(point, width, height)) < 1e-12,
          "prevalidated scalar sampler matches the oracle");
      }
    }
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("backdrop irradiance is bright for a front light and ambient-only for a back light", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const point = { x: 128, y: 120, z: 0 };
  try {
    setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 10, gameWidthCm: 7 });
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    const front = backdropIrradiance(point);
    assert.ok(front > 0.35 && front <= 1);
    setSceneLightRotation(rotationY(Math.PI));
    assert.ok(Math.abs(backdropIrradiance(point) - 0.35) < 1e-12);
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("distance attenuation preserves fixture power under rotation and dims a farther backboard", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const placement = { distanceCm: 25, elevationDegrees: 10, azimuthDegrees: 0, gameWidthCm: 7 };
  const referenceRadius = placement.distanceCm * 256 / placement.gameWidthCm;
  try {
    setSceneLightPlacement(placement);
    const results = [];
    for (const rotation of [rotationY(Math.PI / 6), rotationY(-Math.PI / 6),
      rotationZ(Math.PI / 4), rotationZ(-Math.PI / 4)]) {
      setSceneLightRotation(rotation);
      const light = getSceneLightPosition();
      const centerRadius = Math.hypot(
        (light.x - 128), (light.y - 120), light.z * SCENE_DEPTH_TO_NATIVE_PIXELS,
      );
      assert.ok(Math.abs(centerRadius - referenceRadius) < 1e-10,
        "room rotation preserves source radius to the initial phone center");
      const near = backdropIrradiance({ x: 128, y: 120, z: 0 });
      const far = backdropIrradiance({ x: 128, y: 120, z: 3 });
      const nearSide = surfaceShade("top", false, { x: 128, y: 200, z: 0 });
      const farSide = surfaceShade("top", false, { x: 128, y: 200, z: 3 });
      assert.ok(near <= 1 && near >= 0.35);
      assert.ok(far < near && far >= 0.35,
        "the same fixed light loses diffuse contribution across the physical gap");
      assert.ok(nearSide <= 0.77 && nearSide >= 0.45);
      assert.ok(farSide < nearSide && farSide >= 0.45,
        "side-face diffuse light also falls with physical distance");
      results.push({ near, far, nearSide, farSide });
    }
    for (const [positive, negative] of [[results[0], results[1]], [results[2], results[3]]]) {
      assert.ok(Math.abs(positive.near - negative.near) < 1e-12,
        "opposing yaw and roll preserve the center-point irradiance");
      assert.ok(Math.abs(positive.far - negative.far) < 1e-12);
      assert.ok(Math.abs(positive.nearSide - negative.nearSide) < 1e-12);
      assert.ok(Math.abs(positive.farSide - negative.farSide) < 1e-12);
    }
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("raising either side toward a fixed room lamp reverses the receiving-board light gradient", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  const radians = 10 * Math.PI / 180;
  const depth = 85 / SCENE_DEPTH_TO_NATIVE_PIXELS;
  const pixelsPerCm = 256 / 6.7;
  const roomLamp = [0, -25 * Math.sin(7 * Math.PI / 180) * pixelsPerCm,
    -25 * Math.cos(7 * Math.PI / 180) * pixelsPerCm];
  try {
    setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 7, gameWidthCm: 6.7 });
    for (const angle of [-radians, radians]) {
      // Inverse phone rotation, with positive angle bringing its left edge toward the lamp.
      setSceneLightRotation(rotationY(angle));
      const values = [];
      for (const x of [-64, 64]) {
        // Independently rotate the receiving point into the room, leaving the lamp fixed.
        const z = depth * SCENE_DEPTH_TO_NATIVE_PIXELS;
        const roomPoint = [Math.cos(angle) * x - Math.sin(angle) * z, 0,
          Math.sin(angle) * x + Math.cos(angle) * z];
        const ray = roomLamp.map((value, index) => value - roomPoint[index]);
        const length = Math.hypot(...ray);
        const outwardNormal = [Math.sin(angle), 0, -Math.cos(angle)];
        const cosine = Math.max(0, ray.reduce((sum, value, index) =>
          sum + value * outwardNormal[index], 0) / length);
        const expected = 0.35 + 0.65 * cosine * Math.min(1,
          (25 * pixelsPerCm / length) ** 2);
        const actual = backdropIrradiance({ x: 128 + x, y: 120, z: depth });
        assert.ok(Math.abs(actual - expected) < 1e-12);
        values.push(actual);
      }
      assert.equal(values[0] > values[1], angle > 0,
        "the raised side is nearer and receives more light; reversing tilt reverses the gradient");
    }
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});

test("backdrop irradiance validates coordinates, depth, dimensions, and coincident rays", () => {
  const originalPlacement = getSceneLightPlacement();
  const originalRotation = [...getSceneLightRotation()];
  try {
    setSceneLightPlacement({ distanceCm: 5, elevationDegrees: 0, gameWidthCm: 256 });
    setSceneLightRotation(rotationY(Math.PI));
    const light = getSceneLightPosition();
    const coincidentPoint = {
      x: light.x,
      y: light.y,
      z: light.z,
    };
    assert.equal(backdropIrradiance(coincidentPoint), 1);
    for (const point of [null, {}, { x: NaN, y: 1, z: 0 }, { x: 0, y: Infinity, z: 0 }, { x: 1, y: 2 }]) {
      assert.throws(() => backdropIrradiance(point));
    }
    for (const invalidLight of [null, {}, { x: 1, y: 2, z: NaN }]) {
      assert.throws(() => backdropIrradianceFromLight({ x: 0, y: 0, z: 0 }, invalidLight));
    }
    for (const depth of [-0.01, 6.01]) {
      assert.throws(() => backdropIrradiance({ x: 1, y: 2, z: depth }), RangeError);
    }
    assert.throws(() => backdropIrradiance({ x: 1, y: 2, z: 0 }, 0, 240), RangeError);
    assert.throws(() => backdropIrradiance({ x: Number.MAX_VALUE, y: Number.MAX_VALUE, z: 0 }), RangeError);
    assert.equal(createBackdropIrradianceSampler(light, light.z, 256, 240)(light.x, light.y), 1);
    const huge = createBackdropIrradianceSampler({ x: Number.MAX_VALUE, y: 0, z: 0 }, 0);
    assert.ok(Number.isFinite(huge(0, 0)), "overflowed squared length falls back to hypot");
    for (const args of [
      [null, 0, 256, 240],
      [{ x: 0, y: NaN, z: 0 }, 0, 256, 240],
      [light, NaN, 256, 240],
      [light, -0.01, 256, 240],
      [light, 0, 0, 240],
      [{ x: Number.MAX_VALUE, y: 0, z: 0 }, 0, 0.5, 240],
    ]) {
      assert.throws(() => createBackdropIrradianceSampler(...args));
    }
  } finally {
    setSceneLightPlacement(originalPlacement);
    setSceneLightRotation(originalRotation);
  }
});
