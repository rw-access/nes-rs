import test from "node:test";
import assert from "node:assert/strict";
import { CanvasBackdropRenderer, projectBackdropReceiverPoint } from "./backdrop-lighting.js";
import { getSceneLightPosition, setSceneLightDistance, setSceneLightPlacement, setSceneLightRotation } from "./scene-lighting.js";
import { getSceneCameraDepth, getSceneVanishingPoint, setSceneCamera } from "./scene-view.js";

const IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1];
const DEPTH_TO_NATIVE_PIXELS = 19.2;

class MockCanvas {
  constructor() {
    this.width = 0;
    this.height = 0;
    this.putCount = 0;
    this.context = {
      createImageData: (width, height) => ({ data: new Uint8ClampedArray(width * height * 4) }),
      putImageData: (image, x, y) => {
        this.lastImage = image;
        this.lastPut = { x, y };
        this.putCount += 1;
      },
    };
  }

  getContext() {
    return this.context;
  }
}

function solidRGBA(width, height, color = [180, 120, 60, 255]) {
  const data = new Uint8Array(width * height * 4);
  for (let offset = 0; offset < data.length; offset += 4) data.set(color, offset);
  return data;
}

function resetScene() {
  setSceneCamera(0, 0, 1 / 0.015);
  setSceneLightRotation(IDENTITY);
  setSceneLightPlacement(null);
  setSceneLightDistance(2);
}

function independentReceiverPoint(screenX, screenY, depth, width, height, focalDepth) {
  const vanishingPoint = getSceneVanishingPoint(width, height);
  const camera = { x: vanishingPoint.x, y: vanishingPoint.y, z: -focalDepth };
  const frontPlanePoint = { x: screenX, y: screenY, z: 0 };
  const distanceToReceiver = depth + focalDepth;
  const fraction = distanceToReceiver / focalDepth;
  return {
    x: camera.x + (frontPlanePoint.x - camera.x) * fraction,
    y: camera.y + (frontPlanePoint.y - camera.y) * fraction,
    z: depth,
  };
}

function independentIrradiance(point, light, width, height) {
  const ray = [
    (light.x - point.x) * 256 / width,
    (light.y - point.y) * 240 / height,
    (light.z - point.z) * DEPTH_TO_NATIVE_PIXELS,
  ];
  const magnitude = Math.hypot(...ray);
  const cosine = Math.max(0, Math.min(1, -ray[2] / magnitude));
  const referenceRadius = Math.hypot(
    (light.x - width / 2) * 256 / width,
    (light.y - height / 2) * 240 / height,
    light.z * DEPTH_TO_NATIVE_PIXELS,
  );
  const attenuation = Math.min(1, (referenceRadius / magnitude) ** 2);
  return 0.35 + 0.65 * cosine * attenuation;
}

function expectedPixel(source, irradiance) {
  return [
    Math.round(source[0] * irradiance),
    Math.round(source[1] * irradiance),
    Math.round(source[2] * irradiance),
    source[3],
  ];
}

function rotationY(degrees) {
  const angle = degrees * Math.PI / 180;
  const c = Math.cos(angle), s = Math.sin(angle);
  return [c, 0, s, 0, 1, 0, -s, 0, c];
}

test("backdrop receiver points equal independent camera-ray intersections at centered and off-axis views", () => {
  const width = 320, height = 200;
  const cases = [
    { view: [0, 0], focal: 1 / 0.015, point: [0.5, 0.5], depth: 0 },
    { view: [0.27, -0.18], focal: 42, point: [34.5, 172.75], depth: 4.25 },
    { view: [-0.31, 0.22], focal: 105, point: [287.5, 19.5], depth: 2.5 },
  ];
  try {
    for (const item of cases) {
      setSceneCamera(item.view[0], item.view[1], item.focal);
      const expected = independentReceiverPoint(...item.point, item.depth, width, height, item.focal);
      const actual = projectBackdropReceiverPoint(...item.point, item.depth, width, height);
      assert.ok(Math.abs(actual.x - expected.x) < 1e-12);
      assert.ok(Math.abs(actual.y - expected.y) < 1e-12);
      assert.equal(actual.z, item.depth);
    }
  } finally {
    resetScene();
  }
});

test("paint shades pixels by independently evaluated Lambert light and preserves alpha and source", () => {
  const width = 16, height = 12, depth = 4.25;
  const canvas = new MockCanvas();
  const renderer = new CanvasBackdropRenderer(canvas, width, height);
  const source = solidRGBA(width, height, [231, 151, 77, 255]);
  source.set([31, 178, 229, 73], (2 * width + 3) * 4);
  source.set([98, 56, 23, 0], (9 * width + 14) * 4);
  const original = source.slice();
  const angle = 27 * Math.PI / 180;
  const c = Math.cos(angle), s = Math.sin(angle);
  const pose = [c, -s, 0, s, c, 0, 0, 0, 1];

  try {
    setSceneCamera(0.18, -0.13, 48);
    setSceneLightDistance(3);
    setSceneLightPlacement({ distanceCm: 24, elevationDegrees: 17, azimuthDegrees: -13, gameWidthCm: 6.7 });
    setSceneLightRotation(pose);
    assert.equal(renderer.paint(source, depth), true);
    assert.equal(canvas.width, width);
    assert.equal(canvas.height, height);
    assert.deepEqual(source, original);
    assert.strictEqual(renderer.outputRGBA, canvas.lastImage.data);
    assert.deepEqual(canvas.lastPut, { x: 0, y: 0 });
    assert.ok(renderer.irradiance.every(value => value >= 0.35 && value <= 1));

    const light = getSceneLightPosition(width, height);
    const focalDepth = getSceneCameraDepth();
    for (const [x, y] of [[0, 0], [3, 2], [8, 6], [15, 11]]) {
      const sourceOffset = (y * width + x) * 4;
      const receiver = independentReceiverPoint(x + 0.5, y + 0.5, depth, width, height, focalDepth);
      const irradiance = independentIrradiance(receiver, light, width, height);
      assert.deepEqual([...renderer.outputRGBA.slice(sourceOffset, sourceOffset + 4)],
        expectedPixel(source.slice(sourceOffset, sourceOffset + 4), irradiance));
    }
    const transparentOffset = (9 * width + 14) * 4;
    assert.equal(renderer.outputRGBA[transparentOffset + 3], 0);
    const partiallyTransparentOffset = (2 * width + 3) * 4;
    assert.equal(renderer.outputRGBA[partiallyTransparentOffset + 3], 73);
  } finally {
    resetScene();
  }
});

test("irradiance cache invalidates on source, light, view, and scene-depth changes only", () => {
  const width = 24, height = 18, depth = 4.5;
  const canvas = new MockCanvas();
  const renderer = new CanvasBackdropRenderer(canvas, width, height);
  const source = solidRGBA(width, height, [255, 210, 160, 255]);
  const sample = (x = 2, y = 3) => [...renderer.outputRGBA.slice((y * width + x) * 4, (y * width + x) * 4 + 4)];
  const irradianceSample = (x = 2, y = 3) => renderer.irradiance[y * width + x];

  try {
    setSceneCamera(0.1, -0.08, 24);
    setSceneLightPlacement({ distanceCm: 20, elevationDegrees: 20, azimuthDegrees: 0, gameWidthCm: 6.7 });
    setSceneLightRotation(IDENTITY);
    renderer.paint(source, depth);
    const firstPixel = sample();
    const firstIrradiance = irradianceSample();
    const firstTable = renderer.irradiance.slice();
    assert.equal(canvas.putCount, 1);

    assert.equal(renderer.paint(source, depth), false, "unchanged pixels and geometry reuse the painted image");
    assert.equal(canvas.putCount, 1);

    const lampBeforeCameraChange = getSceneLightPosition(width, height);
    setSceneCamera(0.1, -0.08, 70);
    const lampAfterCameraChange = getSceneLightPosition(width, height);
    assert.deepEqual(lampAfterCameraChange, lampBeforeCameraChange, "camera distance does not move the light");
    assert.equal(renderer.paint(source, depth), true);
    assert.notEqual(irradianceSample(), firstIrradiance, "receiver point changes with camera distance");
    assert.notDeepEqual(renderer.irradiance, firstTable);
    assert.equal(sample()[3], firstPixel[3], "camera changes preserve backdrop alpha");

    const cameraTable = renderer.irradiance.slice();
    setSceneCamera(0.24, -0.16, 70);
    renderer.paint(source, depth);
    assert.notDeepEqual(renderer.irradiance, cameraTable, "view offset changes the receiver lookup");

    const beforeLight = renderer.irradiance.slice();
    setSceneLightRotation(rotationY(33));
    renderer.paint(source, depth);
    assert.notDeepEqual(renderer.irradiance, beforeLight, "room light rotation changes the irradiance field");

    const beforeDepth = renderer.irradiance.slice();
    renderer.paint(source, depth - 1);
    assert.notDeepEqual(renderer.irradiance, beforeDepth, "backdrop depth changes the receiving plane");

    const beforeSource = renderer.irradiance.slice();
    const editedSource = source.slice();
    editedSource.set([10, 20, 30, 121], (4 * width + 7) * 4);
    const putsBeforeSource = canvas.putCount;
    renderer.paint(editedSource, depth - 1);
    assert.equal(canvas.putCount, putsBeforeSource + 1, "changed source RGBA is repainted");
    assert.deepEqual(renderer.irradiance, beforeSource, "source edits reuse the cached light table");
    assert.equal(renderer.outputRGBA[(4 * width + 7) * 4 + 3], 121);
    assert.deepEqual(source, solidRGBA(width, height, [255, 210, 160, 255]), "caller source remains unchanged");
  } finally {
    resetScene();
  }
});

test("invalid canvas, dimensions, source arrays, and scene depths are rejected", () => {
  assert.throws(() => new CanvasBackdropRenderer(null), TypeError);
  assert.throws(() => new CanvasBackdropRenderer(new MockCanvas(), 0, 240), RangeError);
  const renderer = new CanvasBackdropRenderer(new MockCanvas(), 4, 3);
  assert.throws(() => renderer.paint(new Uint8Array(4 * 3 * 4 - 1), 2), RangeError);
  assert.throws(() => renderer.paint(new Float32Array(4 * 3 * 4), 2), TypeError);
  assert.throws(() => renderer.paint(new Uint8Array(4 * 3 * 4), -1), RangeError);
  assert.throws(() => renderer.paint(new Uint8Array(4 * 3 * 4), 6.1), RangeError);
  assert.throws(() => projectBackdropReceiverPoint(Infinity, 0, 1), TypeError);
});
