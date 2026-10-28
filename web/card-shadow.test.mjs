import test from "node:test";
import assert from "node:assert/strict";
import { CanvasCardShadowRenderer, collectCardSilhouetteEdges, projectCardShadowPoint } from "./card-shadow.js";
import { getSceneLightPosition, setSceneLightPlacement, setSceneLightRotation } from "./scene-lighting.js";
import { getSceneVanishingPoint, getSceneViewOffset, setSceneCamera } from "./scene-view.js";
import { cameraDepthScale } from "./depth-mesh.js";
import { SCENE_DEPTH_TO_NATIVE_PIXELS } from "./viewing-model.js";

const IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1];
const WIDTH = 256;
const HEIGHT = 240;
const identityCamera = { x: 0, y: 0, focalDepth: 1 / 0.015 };

function rgba(width, height) {
  return new Uint8ClampedArray(width * height * 4);
}

function fillRect(source, width, x0, y0, x1, y1, alpha = 255) {
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) source[(y * width + x) * 4 + 3] = alpha;
  }
}

function setYaw(degrees) {
  const angle = degrees * Math.PI / 180;
  const c = Math.cos(angle), s = Math.sin(angle);
  setSceneLightRotation([c, 0, s, 0, 1, 0, -s, 0, c]);
}

function near(actual, expected, tolerance = 1e-9) {
  assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} not within ${tolerance} of ${expected}`);
}

function rayProjection(point, sourceDepth, receiverDepth, width, height) {
  const light = getSceneLightPosition(width, height);
  const lambda = (receiverDepth - light.z) / (sourceDepth - light.z);
  const receiverX = light.x + lambda * (point.x - light.x);
  const receiverY = light.y + lambda * (point.y - light.y);
  const scale = cameraDepthScale(receiverDepth);
  const vanishingPoint = getSceneVanishingPoint(width, height);
  return {
    x: vanishingPoint.x + (receiverX - vanishingPoint.x) * scale,
    y: vanishingPoint.y + (receiverY - vanishingPoint.y) * scale,
  };
}

function withNeutralScene(callback) {
  const priorDocument = globalThis.document;
  globalThis.document = { createElement: () => new MockCanvas() };
  setSceneLightPlacement({ distanceCm: 25, elevationDegrees: 7, gameWidthCm: 6.7 });
  setSceneLightRotation(IDENTITY);
  setSceneCamera(identityCamera.x, identityCamera.y, identityCamera.focalDepth);
  try {
    callback();
  } finally {
    setSceneLightRotation(IDENTITY);
    setSceneCamera(identityCamera.x, identityCamera.y, identityCamera.focalDepth);
    if (priorDocument === undefined) delete globalThis.document;
    else globalThis.document = priorDocument;
  }
}

class MockContext {
  constructor() {
    this.events = [];
    this.imageSmoothingEnabled = false;
    this.globalCompositeOperation = "source-over";
  }

  createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
  putImageData(image, x, y) { this.events.push(["putImageData", image, x, y]); }
  setTransform(...args) { this.events.push(["setTransform", ...args]); }
  clearRect(...args) { this.events.push(["clearRect", ...args]); }
  save() { this.events.push(["save"]); }
  restore() { this.events.push(["restore"]); }
  translate(...args) { this.events.push(["translate", ...args]); }
  scale(...args) { this.events.push(["scale", ...args]); }
  drawImage(...args) { this.events.push(["drawImage", ...args]); }
  beginPath() { this.events.push(["beginPath"]); }
  moveTo(...args) { this.events.push(["moveTo", ...args]); }
  lineTo(...args) { this.events.push(["lineTo", ...args]); }
  closePath() { this.events.push(["closePath"]); }
  fill() { this.events.push(["fill"]); }
}

class MockCanvas {
  constructor(width = 256, height = 240) {
    this.width = width;
    this.height = height;
    this.context = new MockContext();
  }

  getContext(type) { return type === "2d" ? this.context : null; }
}

test("unculled silhouette edges include both outer boundaries and cutout holes", () => {
  const source = rgba(7, 7);
  fillRect(source, 7, 1, 1, 6, 6);
  source[(3 * 7 + 3) * 4 + 3] = 0;
  const edges = collectCardSilhouetteEdges(source, 7, 7);
  assert.equal(edges.length, 8);
  assert.ok(edges.some(edge => edge.start.x === 3 && edge.end.x === 3 && edge.start.y === 3 && edge.end.y === 4));
  assert.ok(edges.some(edge => edge.start.y === 3 && edge.end.y === 3 && edge.start.x === 3 && edge.end.x === 4));

  const empty = collectCardSilhouetteEdges(rgba(7, 7), 7, 7);
  assert.deepEqual(empty, []);
  const full = rgba(7, 7);
  fillRect(full, 7, 0, 0, 7, 7);
  assert.equal(collectCardSilhouetteEdges(full, 7, 7).length, 4);
});

test("point projections match independent room-space ray hits for cap and side points", () => {
  withNeutralScene(() => {
    const width = 256, height = 240, receiver = 5.5;
    for (const angle of [-30, 0, 30]) {
      setYaw(angle);
      for (const thicknessPx of [0, 4, 8]) {
        const back = thicknessPx / SCENE_DEPTH_TO_NATIVE_PIXELS;
        for (const sourceDepth of [0, back / 2, back]) {
          for (const point of [{ x: 18.5, y: 27.25 }, { x: 129, y: 119 }, { x: 233.5, y: 210 }]) {
            const actual = projectCardShadowPoint(point, sourceDepth, receiver, width, height);
            const expected = rayProjection(point, sourceDepth, receiver, width, height);
            near(actual.x, expected.x, 1e-9);
            near(actual.y, expected.y, 1e-9);
            assert.equal(actual.opacity, 1);
          }
        }
      }
    }
  });
});

test("rectangular, holed, and asymmetric extrusion vertices follow independent ray hits", () => {
  withNeutralScene(() => {
    const fixtures = [];
    const rectangle = rgba(WIDTH, HEIGHT);
    fillRect(rectangle, WIDTH, 30, 42, 56, 71);
    fixtures.push(rectangle);

    const hole = rgba(WIDTH, HEIGHT);
    fillRect(hole, WIDTH, 40, 35, 63, 64);
    fillRect(hole, WIDTH, 48, 44, 55, 55, 0);
    fixtures.push(hole);

    const asymmetric = rgba(WIDTH, HEIGHT);
    fillRect(asymmetric, WIDTH, 92, 45, 114, 53);
    fillRect(asymmetric, WIDTH, 92, 53, 99, 84);
    fillRect(asymmetric, WIDTH, 99, 73, 128, 84);
    fixtures.push(asymmetric);

    for (const angle of [-30, 0, 30]) {
      setYaw(angle);
      for (const thicknessPixels of [0, 4, 8]) {
        const frontDepth = 0;
        const backDepth = thicknessPixels / SCENE_DEPTH_TO_NATIVE_PIXELS;
        const receiverDepth = 6;
        for (const source of fixtures) {
          const edges = collectCardSilhouetteEdges(source, WIDTH, HEIGHT);
          assert.ok(edges.length >= 4);
          for (const edge of edges) {
            for (const [point, depth] of [
              [edge.start, frontDepth], [edge.end, frontDepth],
              [edge.start, backDepth], [edge.end, backDepth],
            ]) {
              const actual = projectCardShadowPoint(point, depth, receiverDepth, WIDTH, HEIGHT);
              const expected = rayProjection(point, depth, receiverDepth, WIDTH, HEIGHT);
              near(actual.x, expected.x, 1e-9);
              near(actual.y, expected.y, 1e-9);
            }
          }
        }
      }
    }
  });
});

test("projected cap transforms include off-axis camera projection exactly once", () => {
  withNeutralScene(() => {
    setYaw(22);
    setSceneCamera(-0.3, 0.12, 54);
    const point = { x: 42, y: 211 };
    const actual = projectCardShadowPoint(point, 0, 4.2);
    const expected = rayProjection(point, 0, 4.2, WIDTH, HEIGHT);
    near(actual.x, expected.x, 1e-9);
    near(actual.y, expected.y, 1e-9);
    const offset = getSceneViewOffset(WIDTH, HEIGHT);
    assert.ok(Math.abs(offset.x) > 1);
  });
});

test("renderer unions front/back caps with unculled quads and preserves alpha caching", () => {
  withNeutralScene(() => {
    const canvas = new MockCanvas(512, 480);
    const sourceCanvas = new MockCanvas();
    const source = rgba(256, 240);
    fillRect(source, 256, 30, 31, 36, 38);
    source[(34 * 256 + 33) * 4 + 3] = 0;
    const renderer = new CanvasCardShadowRenderer(canvas);

    assert.equal(renderer.paint(sourceCanvas, source, { frontDepth: 0, backDepth: 4 / 19.2, receiverDepth: 4.5 }), true);
    assert.equal(renderer.opacity, 1);
    assert.equal(canvas.context.events.filter(([name]) => name === "drawImage").length, 2,
      "both cap silhouettes are projected");
    assert.equal(canvas.context.events.filter(([name]) => name === "fill").length, renderer.edges.length,
      "each uncullled outer or hole boundary contributes a side quad");

    canvas.context.events.length = 0;
    assert.equal(renderer.paint(sourceCanvas, source, { frontDepth: 0, backDepth: 4 / 19.2, receiverDepth: 4.5 }), false,
      "stable source, pose and depth reuse the painted shadow");

    const colorOnly = source.slice();
    colorOnly[0] = 99;
    canvas.context.events.length = 0;
    assert.equal(renderer.paint(sourceCanvas, colorOnly, { frontDepth: 0, backDepth: 4 / 19.2, receiverDepth: 4.5 }), false,
      "RGB-only changes do not rebuild a silhouette mask");

    const changedAlpha = source.slice();
    changedAlpha[(30 * 256 + 30) * 4 + 3] = 255;
    assert.equal(renderer.paint(sourceCanvas, changedAlpha,
      { frontDepth: 0, backDepth: 4 / 19.2, receiverDepth: 4.5 }), true,
    "alpha changes rebuild the cap mask and uncullled wall contours");

    canvas.context.events.length = 0;
    assert.equal(renderer.paint(sourceCanvas, source, { frontDepth: 0, backDepth: 0, receiverDepth: 4.5 }), true);
    assert.equal(canvas.context.events.filter(([name]) => name === "drawImage").length, 1,
      "zero thickness collapses to a single cap");
    assert.equal(canvas.context.events.filter(([name]) => name === "fill").length, 0);
  });
});

test("renderer redraws on camera/light changes and respects output resize", () => {
  withNeutralScene(() => {
    const canvas = new MockCanvas();
    const sourceCanvas = new MockCanvas();
    const source = rgba(256, 240);
    fillRect(source, 256, 44, 32, 61, 53);
    const renderer = new CanvasCardShadowRenderer(canvas);
    const scene = { frontDepth: 0, backDepth: 8 / 19.2, receiverDepth: 5 };

    assert.equal(renderer.paint(sourceCanvas, source, scene), true);
    assert.equal(renderer.paint(sourceCanvas, source, scene), false);
    setSceneCamera(0.1, -0.05, 50);
    assert.equal(renderer.paint(sourceCanvas, source, scene), true);
    assert.equal(renderer.paint(sourceCanvas, source, scene), false);
    setYaw(10);
    assert.equal(renderer.paint(sourceCanvas, source, scene), true);
    assert.equal(renderer.paint(sourceCanvas, source, scene), false);
    renderer.resize(256, 240, 2);
    assert.deepEqual({ width: canvas.width, height: canvas.height }, { width: 512, height: 480 });
    assert.equal(renderer.paint(sourceCanvas, source, scene), true);
  });
});

test("grazing and behind-card lights remain bounded and disable the shadow safely", () => {
  withNeutralScene(() => {
    const canvas = new MockCanvas();
    const sourceCanvas = new MockCanvas();
    const source = rgba(256, 240);
    fillRect(source, 256, 100, 100, 120, 120);
    const renderer = new CanvasCardShadowRenderer(canvas);
    const scene = { frontDepth: 0, backDepth: 8 / 19.2, receiverDepth: 6 };

    const nearGrazing = Math.acos(1 / 49.380405554294626) * 180 / Math.PI;
    setYaw(nearGrazing);
    assert.equal(renderer.paint(sourceCanvas, source, scene), true);
    assert.ok(renderer.opacity > 0 && renderer.opacity < 1);
    const point = projectCardShadowPoint({ x: 100, y: 100 }, 0, 6);
    assert.ok(Number.isFinite(point.x) && Number.isFinite(point.y));

    setYaw(180);
    assert.equal(renderer.paint(sourceCanvas, source, scene), true);
    assert.equal(renderer.opacity, 0);
    assert.equal(projectCardShadowPoint({ x: 100, y: 100 }, 0, 6), null);
    assert.throws(() => renderer.paint(sourceCanvas, source, { frontDepth: -0.1, backDepth: 1, receiverDepth: 3 }), RangeError);
    assert.throws(() => renderer.paint(sourceCanvas, source, { frontDepth: 0, backDepth: 3, receiverDepth: 2 }), RangeError);
  });
});
