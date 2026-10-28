import test from "node:test";
import assert from "node:assert/strict";
import { CanvasDepthRenderer, collectSideFaces, collectTileSupportedFaces, projectDepthPoint } from "./depth-mesh.js";
import {
  getSceneLightRevision,
  getSceneLightRotation,
  setSceneLightDistance,
  setSceneLightRotation,
  shadowLightProjection,
} from "./scene-lighting.js";
import { getSceneVanishingPoint, getSceneViewOffset, getSceneViewRevision, setSceneView } from "./scene-view.js";

function rgba(width, height, pixels = []) {
  const data = new Uint8Array(width * height * 4);
  for (const [x, y, red, green, blue, alpha = 255] of pixels) {
    data.set([red, green, blue, alpha], (y * width + x) * 4);
  }
  return data;
}

function expectedShadowTransform(depth, width, height, outputWidth, outputHeight) {
  const clearance = Math.min(0.15, depth * 0.15);
  const cameraScale = 1 / (1 + clearance * 0.015);
  const light = shadowLightProjection(0, clearance, width, height);
  const viewOffset = getSceneViewOffset(width, height);
  const scaleX = outputWidth / width;
  const scaleY = outputHeight / height;
  const transforms = [
    ["setTransform", 1, 0, 0, 1, 0, 0],
    ["translate", outputWidth / 2 + light.x * cameraScale * scaleX,
      outputHeight / 2 + light.y * cameraScale * scaleY],
  ];
  const viewShiftX = viewOffset.x * (1 - cameraScale) * scaleX;
  const viewShiftY = viewOffset.y * (1 - cameraScale) * scaleY;
  if (viewShiftX !== 0 || viewShiftY !== 0) transforms.push(["translate", viewShiftX, viewShiftY]);
  transforms.push(["scale", cameraScale * light.magnification, cameraScale * light.magnification]);
  transforms.push(["translate", -outputWidth / 2, -outputHeight / 2]);
  return transforms;
}

function expectedShadowBlur(depth, softness, width, height, outputWidth, outputHeight) {
  const clearance = Math.min(0.15, depth * 0.15);
  const cameraScale = 1 / (1 + clearance * 0.015);
  const maxOutputScale = Math.max(outputWidth / width, outputHeight / height);
  return clearance * 1.5 * softness * cameraScale * maxOutputScale;
}

test("solid rectangle produces its inward-facing left and top runs despite interior colors", () => {
  const pixels = [];
  for (let y = 6; y <= 8; y += 1) {
    for (let x = 6; x <= 8; x += 1) {
      const interior = x === 7 && y === 7;
      pixels.push([x, y, ...(interior ? [0, 255, 0] : [100, 120, 140])]);
    }
  }
  const faces = collectSideFaces(rgba(10, 10, pixels), 10, 10);
  assert.equal(faces.length, 2);
  assert.deepEqual(faces.map((face) => face.orientation).sort(), ["left", "top"]);
  assert.ok(faces.every((face) => face.color.join(",") === "100,120,140"));
  assert.ok(faces.every((face) => face.start && face.end && Number.isFinite(face.sortKey)));
});

test("transparent holes create inward-facing exposed boundaries", () => {
  const pixels = [];
  for (let y = 6; y <= 8; y += 1) {
    for (let x = 6; x <= 8; x += 1) {
      if (x !== 7 || y !== 7) pixels.push([x, y, 80, 40, 20]);
    }
  }
  const faces = collectSideFaces(rgba(10, 10, pixels), 10, 10);
  assert.ok(faces.some((face) => face.orientation === "left"
    && face.start.x === 8 && face.end.x === 8 && face.start.y === 7 && face.end.y === 8));
  assert.ok(faces.some((face) => face.orientation === "top"
    && face.start.y === 8 && face.end.y === 8 && face.start.x === 7 && face.end.x === 8));
});

test("opaque black pixels remain in the silhouette", () => {
  const faces = collectSideFaces(rgba(10, 10, [[7, 7, 0, 0, 0]]), 10, 10);
  assert.equal(faces.length, 2);
  assert.ok(faces.every((face) => face.color.join(",") === "0,0,0"));
});

test("projection keeps fractional depth continuous without pixel rounding", () => {
  const point = { x: 7, y: 11 };
  const scale = 1 / (1 + 0.375 * 0.015);
  assert.deepEqual(projectDepthPoint(point, 0.375), {
    x: 128 + (point.x - 128) * scale,
    y: 120 + (point.y - 120) * scale,
  });
  assert.deepEqual(point, { x: 7, y: 11 });
});

test("face coordinates are contiguous native boundaries and collection preserves input", () => {
  const source = rgba(10, 10, [
    [6, 6, 12, 34, 56], [7, 6, 12, 34, 56], [8, 6, 12, 34, 56],
    [6, 7, 12, 34, 56], [7, 7, 12, 34, 56], [8, 7, 12, 34, 56],
    [6, 8, 12, 34, 56], [7, 8, 12, 34, 56], [8, 8, 12, 34, 56],
  ]);
  const original = source.slice();
  const faces = collectSideFaces(source, 10, 10);
  assert.deepEqual(faces.find((face) => face.orientation === "left").start, { x: 6, y: 6 });
  assert.deepEqual(faces.find((face) => face.orientation === "left").end, { x: 6, y: 9 });
  assert.deepEqual(faces.find((face) => face.orientation === "top").start, { x: 6, y: 6 });
  assert.deepEqual(faces.find((face) => face.orientation === "top").end, { x: 9, y: 6 });
  assert.deepEqual(source, original);
});

test("perspective collects inward-facing left, right, top, and bottom silhouette runs", () => {
  const source = rgba(10, 10, [
    [2, 2, 20, 40, 60], [7, 2, 20, 40, 60],
    [2, 7, 20, 40, 60], [7, 7, 20, 40, 60],
  ]);
  const faces = collectSideFaces(source, 10, 10);
  assert.deepEqual([...new Set(faces.map((face) => face.orientation))].sort(), ["bottom", "left", "right", "top"]);
  assert.equal(faces.length, 8);
});

test("perspective suppresses sides on a filled rectangle centered on the vanishing point", () => {
  const pixels = [];
  for (let y = 3; y <= 6; y += 1) {
    for (let x = 3; x <= 6; x += 1) pixels.push([x, y, 90, 100, 110]);
  }
  assert.deepEqual(collectSideFaces(rgba(10, 10, pixels), 10, 10), []);
});

test("perspective projects boundaries inward toward the center and preserves depth zero", () => {
  const depth = 0.75;
  const center = { x: 5, y: 5 };
  const upperLeft = projectDepthPoint({ x: 0, y: 0 }, depth, 10, 10);
  const upperRight = projectDepthPoint({ x: 10, y: 0 }, depth, 10, 10);
  const lowerLeft = projectDepthPoint({ x: 0, y: 10 }, depth, 10, 10);
  const lowerRight = projectDepthPoint({ x: 10, y: 10 }, depth, 10, 10);
  assert.ok(upperLeft.x > 0 && upperLeft.y > 0);
  assert.ok(upperRight.x < 10 && upperRight.y > 0);
  assert.ok(lowerLeft.x > 0 && lowerLeft.y < 10);
  assert.ok(lowerRight.x < 10 && lowerRight.y < 10);
  assert.deepEqual(projectDepthPoint(center, depth, 10, 10), center);
  const point = { x: 2.25, y: 8.5 };
  assert.deepEqual(projectDepthPoint(point, 0, 10, 10), point);
  const pointScale = 1 / (1 + depth * 0.015);
  assert.deepEqual(projectDepthPoint(point, depth, 10, 10), {
    x: 5 + (point.x - 5) * pointScale,
    y: 5 + (point.y - 5) * pointScale,
  });
});

test("off-axis depth projection matches independent camera-ray intersections and keeps the foreground fixed", () => {
  setSceneView(0, 0);
  try {
    assert.equal(setSceneView(0.27, -0.18), true);
    const width = 320, height = 200, depth = 6;
    const vanishingPoint = getSceneVanishingPoint(width, height);
    const viewOffset = getSceneViewOffset(width, height);
    assert.deepEqual(vanishingPoint, { x: 246.4, y: 64 });
    assert.deepEqual(viewOffset, { x: 86.4, y: -36 });

    const worldPoint = { x: 34.5, y: 172.75 };
    const focalDepth = 1 / 0.015;
    const camera = { x: vanishingPoint.x, y: vanishingPoint.y, z: -focalDepth };
    const target = { x: worldPoint.x, y: worldPoint.y, z: depth };
    const rayFraction = -camera.z / (target.z - camera.z);
    const rayPlaneIntersection = {
      x: camera.x + (target.x - camera.x) * rayFraction,
      y: camera.y + (target.y - camera.y) * rayFraction,
    };
    const projected = projectDepthPoint(worldPoint, depth, width, height);
    assert.ok(Math.abs(projected.x - rayPlaneIntersection.x) < 1e-12);
    assert.ok(Math.abs(projected.y - rayPlaneIntersection.y) < 1e-12);
    assert.deepEqual(projectDepthPoint(worldPoint, 0, width, height), worldPoint,
      "the foreground plane remains exactly at its original coordinates");
    assert.notStrictEqual(projectDepthPoint(worldPoint, 0, width, height), worldPoint);
    assert.deepEqual(projectDepthPoint(vanishingPoint, depth, width, height), vanishingPoint);
  } finally {
    setSceneView(0, 0);
  }
});

test("off-axis vanishing point changes visible boundaries and painter ordering", () => {
  const source = rgba(10, 10, [[7, 7, 100, 120, 140]]);
  setSceneView(0, 0);
  try {
    const centered = collectSideFaces(source, 10, 10);
    assert.deepEqual(centered.map(face => face.orientation).sort(), ["left", "top"]);

    setSceneView(0.4, -0.2);
    const offAxis = collectSideFaces(source, 10, 10);
    assert.deepEqual(offAxis.map(face => face.orientation).sort(), ["right", "top"]);
    const vanishingPoint = getSceneVanishingPoint(10, 10);
    assert.deepEqual(vanishingPoint, { x: 9, y: 3 });
    assert.ok(offAxis.every((face, index) => index === 0 || face.sortKey >= offAxis[index - 1].sortKey));
    for (const face of offAxis) {
      const centerX = (face.start.x + face.end.x) / 2;
      const centerY = (face.start.y + face.end.y) / 2;
      assert.equal(face.sortKey, -((centerX - vanishingPoint.x) ** 2 + (centerY - vanishingPoint.y) ** 2));
    }
  } finally {
    setSceneView(0, 0);
  }
});

test("tile-supported faces split mixed edge runs by alpha and preserve painter order", () => {
  const faces = [
    { orientation: "right", start: { x: 3, y: 0 }, end: { x: 3, y: 4 }, color: [1, 2, 3], sortKey: 4 },
    { orientation: "bottom", start: { x: 0, y: 4 }, end: { x: 4, y: 4 }, color: [4, 5, 6], sortKey: 3 },
    { orientation: "left", start: { x: 3, y: 0 }, end: { x: 3, y: 4 }, color: [7, 8, 9], sortKey: 2 },
    { orientation: "top", start: { x: 0, y: 3 }, end: { x: 4, y: 3 }, color: [10, 11, 12], sortKey: 1 },
  ];
  const originalFaces = structuredClone(faces);
  const tiles = rgba(6, 6, [
    [2, 0, 0, 0, 0, 255], [2, 1, 255, 255, 255, 0], [2, 2, 0, 0, 0, 1],
    [1, 3, 0, 0, 0, 255], [3, 3, 50, 60, 70, 1],
    [3, 1, 80, 90, 100, 1],
  ]);

  const supported = collectTileSupportedFaces(faces, tiles, 6, 6);
  assert.deepEqual(supported.map(({ orientation, start, end }) => ({ orientation, start, end })), [
    { orientation: "right", start: { x: 3, y: 0 }, end: { x: 3, y: 1 } },
    { orientation: "right", start: { x: 3, y: 2 }, end: { x: 3, y: 3 } },
    { orientation: "bottom", start: { x: 1, y: 4 }, end: { x: 2, y: 4 } },
    { orientation: "bottom", start: { x: 3, y: 4 }, end: { x: 4, y: 4 } },
    { orientation: "left", start: { x: 3, y: 1 }, end: { x: 3, y: 2 } },
    { orientation: "left", start: { x: 3, y: 3 }, end: { x: 3, y: 4 } },
    { orientation: "top", start: { x: 1, y: 3 }, end: { x: 2, y: 3 } },
    { orientation: "top", start: { x: 3, y: 3 }, end: { x: 4, y: 3 } },
  ]);
  assert.deepEqual(faces, originalFaces);
  assert.notEqual(supported[0], faces[0]);
  assert.notEqual(supported[0].start, faces[0].start);
  assert.notEqual(supported[0].color, faces[0].color);
  assert.equal(supported[0].sortKey, faces[0].sortKey);
});

test("tile-supported sampling follows inverse mask projection and perspective edge orientations", () => {
  const right = { orientation: "right", start: { x: 1, y: 1 }, end: { x: 1, y: 2 }, color: [1, 2, 3], sortKey: 0 };
  const projectedTile = rgba(4, 4, [[1, 1, 0, 0, 0, 255]]);
  assert.deepEqual(collectTileSupportedFaces([right], projectedTile, 4, 4), []);
  assert.deepEqual(collectTileSupportedFaces([right], projectedTile, 4, 4, { scale: 2, x: -0.5, y: 0.5 }), [right]);

  const source = rgba(10, 10, [
    [2, 2, 20, 40, 60], [7, 2, 20, 40, 60],
    [2, 7, 20, 40, 60], [7, 7, 20, 40, 60],
  ]);
  const projectedFaces = collectSideFaces(source, 10, 10);
  assert.deepEqual(collectTileSupportedFaces(projectedFaces, source, 10, 10), projectedFaces);
});

test("perspective uses the refined quarter-depth scale continuously", () => {
  const depth = 0.4;
  const scale = 1 / (1 + depth * 0.25 * 0.06);
  assert.deepEqual(projectDepthPoint({ x: 0, y: 0 }, depth, 10, 10), {
    x: 5 - 5 * scale,
    y: 5 - 5 * scale,
  });
  const below = projectDepthPoint({ x: 0, y: 0 }, depth - 0.0001, 10, 10);
  const above = projectDepthPoint({ x: 0, y: 0 }, depth + 0.0001, 10, 10);
  assert.ok(above.x > below.x && above.y > below.y);
});

test("source color changes split exposed edge runs without adding internal walls", () => {
  const pixels = [];
  for (let y = 7; y <= 11; y++) for (let x = 7; x <= 11; x++) {
    pixels.push([x, y, ...(y % 2 ? [0, 0, 0] : [210, 120, 60])]);
  }
  const source = rgba(12, 12, pixels), original = source.slice();
  const wall = collectSideFaces(source, 12, 12).filter(face => face.orientation === "left")
    .sort((a, b) => a.start.y - b.start.y);
  assert.equal(wall.length, 5);
  for (let index = 0; index < wall.length; index++) {
    assert.deepEqual(wall[index].start, { x: 7, y: 7 + index });
    assert.deepEqual(wall[index].end, { x: 7, y: 8 + index });
    assert.deepEqual(wall[index].color, index % 2 ? [210, 120, 60] : [0, 0, 0]);
  }
  assert.deepEqual(source, original);
});

test("all four side orientations inherit each boundary pixel and merge only equal colors", () => {
  const pixels = [];
  for (let y = 2; y < 6; y++) for (let x = 2; x < 6; x++) {
    pixels.push([x, y, x * 31, y * 37, (x + y) * 13]);
  }
  const source = rgba(8, 8, pixels), original = source.slice();
  try {
    const faces = [];
    for (const offset of [-1, 1]) {
      setSceneView(offset, offset);
      faces.push(...collectSideFaces(source, 8, 8));
    }
    assert.equal(faces.length, 16, "only the rectangle's exposed perimeter contributes walls");
    assert.deepEqual([...new Set(faces.map(face => face.orientation))].sort(), ["bottom", "left", "right", "top"]);
    for (const face of faces) {
      const x = face.start.x - (face.orientation === "right" ? 1 : 0);
      const y = face.start.y - (face.orientation === "bottom" ? 1 : 0);
      assert.deepEqual(face.color, [...source.slice((y * 8 + x) * 4, (y * 8 + x) * 4 + 3)]);
    }
    setSceneView(1, 1);
    const band = rgba(6, 1, [[0, 0, 200, 30, 10], [1, 0, 200, 30, 10],
      [2, 0, 0, 0, 0], [3, 0, 0, 0, 0], [4, 0, 20, 180, 50], [5, 0, 20, 180, 50]]);
    const bottom = collectSideFaces(band, 6, 1).filter(face => face.orientation === "bottom")
      .sort((a, b) => a.start.x - b.start.x);
    assert.deepEqual(bottom.map(face => [face.start.x, face.end.x, face.color]),
      [[0, 2, [200, 30, 10]], [2, 4, [0, 0, 0]], [4, 6, [20, 180, 50]]]);
    assert.deepEqual(source, original);
  } finally {
    setSceneView(0, 0);
  }
});

test("transparent gaps keep differently colored edges separate", () => {
  const source = rgba(8, 1, [[5, 0, 0, 0, 0], [7, 0, 220, 40, 20]]);
  const edges = collectSideFaces(source, 8, 1).filter(face => face.orientation === "left");
  assert.deepEqual(edges.map(face => face.start.x), [7, 5]);
  assert.deepEqual(edges.map(face => face.color), [[220, 40, 20], [0, 0, 0]]);
});

test("renderer redraws changed occluders, removes them on null, and draws source art last", () => {
  class MockContext {
    constructor() {
      this.drawCalls = [];
      this.putCount = 0;
      this.fillCount = 0;
      this.globalCompositeOperation = "source-over";
      this.imageSmoothingEnabled = false;
      this.transformOps = [];
      this.stack = [];
    }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData(image) { this.putCount += 1; this.lastPixels = new Uint8ClampedArray(image.data); }
    setTransform(...values) { this.transformOps = [["setTransform", ...values]]; }
    clearRect() { this.drawCalls = []; }
    save() {
      this.stack.push({
        transformOps: this.transformOps.slice(),
        globalCompositeOperation: this.globalCompositeOperation,
        imageSmoothingEnabled: this.imageSmoothingEnabled,
      });
    }
    restore() {
      const state = this.stack.pop();
      this.transformOps = state.transformOps;
      this.globalCompositeOperation = state.globalCompositeOperation;
      this.imageSmoothingEnabled = state.imageSmoothingEnabled;
    }
    translate(x, y) { this.transformOps.push(["translate", x, y]); }
    scale(x, y) { this.transformOps.push(["scale", x, y]); }
    beginPath() {}
    moveTo() {}
    lineTo() {}
    closePath() {}
    fill() { this.fillCount += 1; }
    drawImage(image) {
      this.drawCalls.push({ image, operation: this.globalCompositeOperation, transforms: this.transformOps.slice() });
    }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }

  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    const canvas = new MockCanvas(4, 3);
    const renderer = new CanvasDepthRenderer(canvas, 4, 3);
    const source = rgba(4, 3, [[1, 1, 0, 0, 0]]);
    const original = source.slice();
    const maskA = rgba(4, 3, [[0, 0, 255, 255, 255]]);
    const maskColorOnly = rgba(4, 3, [[0, 0, 10, 20, 30]]);
    const maskB = rgba(4, 3, [[1, 0, 255, 255, 255]]);

    assert.equal(renderer.paint(source, 0.5), true);
    assert.equal(canvas.context.drawCalls.at(-1).image, renderer.sourceCanvas);
    assert.equal(canvas.context.drawCalls.at(-1).operation, "source-over");
    assert.deepEqual(Array.from(renderer.sourceContext.lastPixels), Array.from(original));

    assert.equal(renderer.paint(source, 0.5, maskA), true);
    const cachedMaskCanvas = renderer.occluderCanvas;
    assert.equal(renderer.occluderContext.putCount, 1);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: cachedMaskCanvas, operation: "destination-out" },
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
    assert.deepEqual(canvas.context.drawCalls[0].transforms, [
      ["setTransform", 1, 0, 0, 1, 0, 0],
      ["translate", 2, 1.5],
      ["scale", 1, 1],
      ["translate", -2, -1.5],
    ]);

    assert.equal(renderer.paint(source, 0.5, maskColorOnly), false);
    assert.equal(renderer.occluderContext.putCount, 1);
    assert.deepEqual(Array.from(renderer.sourceContext.lastPixels), Array.from(original));

    const projection = { scale: 0.8, x: 2, y: -1 };
    assert.equal(renderer.paint(source, 0.5, maskA, projection), true);
    assert.equal(renderer.occluderContext.putCount, 1);
    assert.deepEqual(canvas.context.drawCalls[0].transforms, [
      ["setTransform", 1, 0, 0, 1, 0, 0],
      ["translate", 4, 0.5],
      ["scale", 0.8, 0.8],
      ["translate", -2, -1.5],
    ]);
    assert.deepEqual(canvas.context.drawCalls[1].transforms, [["setTransform", 1, 0, 0, 1, 0, 0]]);
    assert.equal(renderer.paint(source, 0.5, maskA, projection), false);

    assert.equal(renderer.paint(source, 0.5, maskB, projection), true);
    assert.equal(renderer.occluderCanvas, cachedMaskCanvas);
    assert.equal(renderer.occluderContext.putCount, 2);
    assert.deepEqual(Array.from(renderer.occluderContext.lastPixels), Array.from(maskB.map((value, offset) => offset % 4 === 3 ? value : 0)));

    const fillCount = canvas.context.fillCount;
    assert.equal(renderer.paint(source, 0, maskB, projection), true);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
    assert.equal(canvas.context.fillCount, fillCount);

    assert.equal(renderer.paint(source, 0, null, projection), true);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
    assert.equal(renderer.paint(source, 0, null, projection), false);
    assert.deepEqual(source, original);
    assert.throws(() => renderer.paint(source, 0, null, { scale: 0, x: 0, y: 0 }), RangeError);
    assert.throws(() => renderer.paint(source, 0, null, { scale: 1, x: Infinity, y: 0 }), RangeError);
  } finally {
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("light-only changes redraw depth relief without rebuilding source, masks, or side geometry", () => {
  class MockContext {
    constructor() {
      this.draws = [];
      this.fillCount = 0;
      this.putCount = 0;
      this.globalCompositeOperation = "source-over";
      this.imageSmoothingEnabled = false;
      this.stack = [];
    }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData() { this.putCount += 1; }
    setTransform() {}
    clearRect() {}
    save() { this.stack.push({ globalCompositeOperation: this.globalCompositeOperation }); }
    restore() { Object.assign(this, this.stack.pop()); }
    translate() {}
    scale() {}
    beginPath() {}
    moveTo() {}
    lineTo() {}
    closePath() {}
    fill() { this.fillCount += 1; }
    fillRect() {}
    drawImage(image) { this.draws.push(image); }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }

  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  const originalRotation = [...getSceneLightRotation()];
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    setSceneLightDistance(2);
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    const canvas = new MockCanvas(10, 10);
    const renderer = new CanvasDepthRenderer(canvas, 10, 10);
    const source = rgba(10, 10, [[1, 1, 180, 120, 60]]);
    const tiles = rgba(10, 10, [[1, 1, 0, 0, 0, 255]]);
    const options = { mode: "contact", softness: 1 };
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), false);

    const faces = renderer.faces;
    const supportedFaces = renderer.tileSupportedFaces;
    const sourceWrites = renderer.sourceContext.putCount;
    const maskWrites = renderer.occluderContext.putCount;
    const wallFills = canvas.context.fillCount;
    const shadowDraws = renderer.reliefContext.draws.filter(image => image === renderer.sourceCanvas).length;
    const revision = getSceneLightRevision();

    setSceneLightDistance(3);
    assert.equal(getSceneLightRevision(), revision + 1);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true);
    assert.strictEqual(renderer.faces, faces);
    assert.strictEqual(renderer.tileSupportedFaces, supportedFaces);
    assert.equal(renderer.sourceContext.putCount, sourceWrites);
    assert.equal(renderer.occluderContext.putCount, maskWrites);
    assert.ok(canvas.context.fillCount > wallFills, "side faces are repainted with the moved light");
    assert.ok(renderer.reliefContext.draws.filter(image => image === renderer.sourceCanvas).length > shadowDraws,
      "contact shadow is projected again");
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), false,
      "the new light revision is cached after one redraw");

    setSceneLightDistance(2);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true,
      "restoring the preset invalidates light-dependent output");
    assert.equal(renderer.sourceContext.putCount, sourceWrites);
    assert.equal(renderer.occluderContext.putCount, maskWrites);

    const facesBeforeRotation = renderer.faces;
    const supportBeforeRotation = renderer.tileSupportedFaces;
    const fillsBeforeRotation = canvas.context.fillCount;
    const shadowDrawsBeforeRotation = renderer.reliefContext.draws.filter(image => image === renderer.sourceCanvas).length;
    const revisionBeforeRotation = getSceneLightRevision();
    setSceneLightRotation([0, -1, 0, 1, 0, 0, 0, 0, 1]);
    assert.equal(getSceneLightRevision(), revisionBeforeRotation + 1);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true,
      "room rotation invalidates light-dependent renderer output");
    assert.strictEqual(renderer.faces, facesBeforeRotation);
    assert.strictEqual(renderer.tileSupportedFaces, supportBeforeRotation);
    assert.equal(renderer.sourceContext.putCount, sourceWrites);
    assert.equal(renderer.occluderContext.putCount, maskWrites);
    assert.ok(canvas.context.fillCount > fillsBeforeRotation);
    assert.ok(renderer.reliefContext.draws.filter(image => image === renderer.sourceCanvas).length > shadowDrawsBeforeRotation);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), false,
      "the rotated-light revision is cached after repaint");
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true,
      "restoring room orientation invalidates shading once");
  } finally {
    setSceneLightDistance(2);
    setSceneLightRotation(originalRotation);
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("view-only changes recull and reorder geometry, offset contact shadows once, and preserve source buffers", () => {
  class MockContext {
    constructor() {
      this.draws = [];
      this.fillCount = 0;
      this.putCount = 0;
      this.globalCompositeOperation = "source-over";
      this.imageSmoothingEnabled = false;
      this.transformOps = [];
      this.stack = [];
    }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData() { this.putCount += 1; }
    setTransform(...values) { this.transformOps = [["setTransform", ...values]]; }
    clearRect() {}
    save() { this.stack.push({ transformOps: this.transformOps.slice(), globalCompositeOperation: this.globalCompositeOperation }); }
    restore() { Object.assign(this, this.stack.pop()); }
    translate(x, y) { this.transformOps.push(["translate", x, y]); }
    scale(x, y) { this.transformOps.push(["scale", x, y]); }
    beginPath() {}
    moveTo() {}
    lineTo() {}
    closePath() {}
    fill() { this.fillCount += 1; }
    fillRect() {}
    drawImage(image) { this.draws.push({ image, transforms: this.transformOps.slice() }); }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }

  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    setSceneLightDistance(2);
    setSceneView(0, 0);
    const canvas = new MockCanvas(10, 10);
    const renderer = new CanvasDepthRenderer(canvas, 10, 10);
    const source = rgba(10, 10, [[7, 7, 180, 120, 60]]);
    const tiles = rgba(10, 10, [[7, 7, 0, 0, 0, 255]]);
    const options = { mode: "contact", softness: 1 };
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true);
    assert.deepEqual(renderer.faces.map(face => face.orientation).sort(), ["left", "top"]);
    assert.equal(renderer.tileSupportedFaces.length, 2);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), false);

    const initialFaces = renderer.faces;
    const initialSupportedFaces = renderer.tileSupportedFaces;
    const sourceWrites = renderer.sourceContext.putCount;
    const maskWrites = renderer.occluderContext.putCount;
    const priorRevision = getSceneViewRevision();
    setSceneView(0.25, -0.2);
    assert.equal(getSceneViewRevision(), priorRevision + 1);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true);
    assert.notStrictEqual(renderer.faces, initialFaces);
    assert.notStrictEqual(renderer.tileSupportedFaces, initialSupportedFaces);
    assert.deepEqual(renderer.faces.map(face => face.orientation).sort(), ["top"]);
    assert.equal(renderer.tileSupportedFaces.length, 1, "contact support follows the newly visible boundary set");
    assert.equal(renderer.sourceContext.putCount, sourceWrites);
    assert.equal(renderer.occluderContext.putCount, maskWrites);

    const shadowDraw = renderer.reliefContext.draws.filter(({ image }) => image === renderer.sourceCanvas).at(-1);
    assert.deepEqual(shadowDraw.transforms, expectedShadowTransform(0.5, 10, 10, 10, 10));
    const viewTranslations = shadowDraw.transforms.filter(([name]) => name === "translate");
    assert.equal(viewTranslations.length, 3, "one view translation follows the point-light spread");
    const viewOffset = getSceneViewOffset(10, 10);
    const cameraScale = 1 / (1 + Math.min(0.15, 0.5 * 0.15) * 0.015);
    assert.deepEqual(viewTranslations[1], ["translate", viewOffset.x * (1 - cameraScale), viewOffset.y * (1 - cameraScale)]);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), false,
      "repeated paint reuses the view-specific result");

    setSceneView(0, 0);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), true,
      "returning to the centered view rebuilds the baseline geometry");
    assert.deepEqual(renderer.faces.map(face => face.orientation).sort(), ["left", "top"]);
    assert.equal(renderer.tileSupportedFaces.length, 2);
    assert.equal(renderer.sourceContext.putCount, sourceWrites);
    assert.equal(renderer.occluderContext.putCount, maskWrites);
    assert.equal(renderer.paint(source, 0.5, tiles, undefined, options), false);
  } finally {
    setSceneView(0, 0);
    setSceneLightDistance(2);
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("sprite relief modes add a clipped rear-projected shadow and opt-in tile faces before exact source art", () => {
  class MockContext {
    constructor() {
      this.events = [];
      this.drawCalls = [];
      this.fillCount = 0;
      this.transformOps = [];
      this.stack = [];
      this.globalAlpha = 1;
      this.globalCompositeOperation = "source-over";
      this.imageSmoothingEnabled = false;
      this.filter = "none";
      this.fillStyle = "#000";
    }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData(image) { this.events.push(["putImageData", image]); }
    setTransform(...values) {
      this.transformOps = [["setTransform", ...values]];
      this.events.push(["setTransform", ...values]);
    }
    clearRect(...values) {
      this.drawCalls = [];
      this.events.push(["clearRect", ...values]);
    }
    save() {
      this.stack.push({
        transformOps: this.transformOps.slice(),
        globalCompositeOperation: this.globalCompositeOperation,
        globalAlpha: this.globalAlpha,
        imageSmoothingEnabled: this.imageSmoothingEnabled,
        filter: this.filter,
        fillStyle: this.fillStyle,
      });
      this.events.push(["save"]);
    }
    restore() {
      Object.assign(this, this.stack.pop());
      this.events.push(["restore"]);
    }
    translate(x, y) { this.transformOps.push(["translate", x, y]); this.events.push(["translate", x, y]); }
    scale(x, y) { this.transformOps.push(["scale", x, y]); this.events.push(["scale", x, y]); }
    beginPath() { this.events.push(["beginPath"]); }
    moveTo(...values) { this.events.push(["moveTo", ...values]); }
    lineTo(...values) { this.events.push(["lineTo", ...values]); }
    closePath() { this.events.push(["closePath"]); }
    fill() {
      this.fillCount += 1;
      this.events.push(["fill", this.fillStyle, this.globalCompositeOperation]);
    }
    fillRect(...values) {
      this.events.push(["fillRect", this.fillStyle, this.globalCompositeOperation, ...values]);
    }
    drawImage(image, ...values) {
      const call = {
        image,
        operation: this.globalCompositeOperation,
        alpha: this.globalAlpha,
        filter: this.filter,
        transforms: this.transformOps.slice(),
        values,
      };
      this.drawCalls.push(call);
      this.events.push(["drawImage", call]);
    }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }

  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    const canvas = new MockCanvas(12, 12);
    const renderer = new CanvasDepthRenderer(canvas, 10, 10);
    const source = rgba(10, 10, [
      [1, 1, 150, 80, 40], [2, 1, 150, 80, 40], [3, 1, 150, 80, 40],
      [1, 2, 150, 80, 40], [2, 2, 150, 80, 40], [3, 2, 150, 80, 40],
      [1, 3, 150, 80, 40], [2, 3, 150, 80, 40], [3, 3, 150, 80, 40],
    ]);
    const original = source.slice();
    const tilePixels = rgba(10, 10, [
      [1, 3, 0, 0, 0, 255], [1, 4, 30, 40, 50, 255],
      [4, 1, 80, 90, 100, 255], [4, 2, 80, 90, 100, 255],
    ]);
    const projection = { scale: 1, x: 0, y: 0 };

    assert.equal(renderer.paint(source, 0.5, tilePixels, projection), true);
    assert.equal(renderer.reliefCanvas, null);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.occluderCanvas, operation: "destination-out" },
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);

    assert.equal(renderer.paint(source, 0.5, tilePixels, projection, { mode: "shadows", softness: 1 }), true);
    const reliefCanvas = renderer.reliefCanvas;
    const reliefContext = renderer.reliefContext;
    assert.ok(reliefCanvas);
    assert.equal(reliefCanvas.width, canvas.width);
    assert.equal(reliefCanvas.height, canvas.height);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.occluderCanvas, operation: "destination-out" },
      { image: reliefCanvas, operation: "source-over" },
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
    const shadowDraw = reliefContext.events.find(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas)[1];
    assert.equal(shadowDraw.alpha, 0.22);
    assert.equal(shadowDraw.filter, `blur(${expectedShadowBlur(0.5, 1, 10, 10, 12, 12)}px)`);
    assert.deepEqual(shadowDraw.transforms, expectedShadowTransform(0.5, 10, 10, 12, 12));
    const shadowFill = reliefContext.events.find(([name]) => name === "fillRect");
    assert.deepEqual(shadowFill, ["fillRect", "#000", "source-in", 0, 0, 12, 12]);
    assert.equal(reliefContext.fillCount, 0, "shadow-only mode adds no side faces over tiles");
    assert.equal(reliefContext.drawCalls.at(-1).image, renderer.occluderCanvas);
    assert.equal(reliefContext.drawCalls.at(-1).operation, "destination-in");
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection, { mode: "shadows", softness: 1 }), false);
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection,
      { mode: "shadows", softness: 1, overlapFraction: 1 / 3 }), true);
    const thinShadow = reliefContext.events.filter(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas).at(-1)[1];
    assert.deepEqual(thinShadow.transforms, expectedShadowTransform(0.5 / 3, 10, 10, 12, 12));
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection,
      { mode: "shadows", softness: 1, overlapFraction: 1 / 3 }), false);
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection,
      { mode: "shadows", softness: 1, overlapFraction: 1 }), true, "fraction changes back to full thickness");

    const contactStart = reliefContext.events.length;
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection,
      { mode: "contact", softness: 1, overlapFraction: 1 / 3 }), true);
    assert.equal(renderer.tileSupportedFaces.length, 1);
    assert.equal(renderer.tileSupportedFaces[0].orientation, "bottom");
    assert.deepEqual(renderer.tileSupportedFaces[0].start, { x: 1, y: 4 });
    assert.deepEqual(renderer.tileSupportedFaces[0].end, { x: 2, y: 4 });
    assert.equal(reliefContext.fillCount, 1, "contact mode restores only the boundary-supported segment");
    const contactRear = projectDepthPoint({ x: 2, y: 4 }, 0.5 / 3, 10, 10);
    assert.ok(reliefContext.events.slice(contactStart).some(([name, ...args]) => name === "lineTo"
      && args[0] === contactRear.x && args[1] === contactRear.y), "contact wall geometry uses the reduced extrusion depth");
    const contactShadow = reliefContext.events.slice(contactStart)
      .filter(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas).at(-1)[1];
    assert.deepEqual(contactShadow.transforms, expectedShadowTransform(0.5 / 3, 10, 10, 12, 12));
    assert.equal(canvas.context.drawCalls.at(-1).image, renderer.sourceCanvas);
    assert.deepEqual(source, original);
    assert.deepEqual(Array.from(renderer.sourceImage.data), Array.from(original));

    assert.equal(renderer.paint(source, 0.5, tilePixels, projection, { mode: "cutout", softness: 1 }), true);
    assert.equal(reliefContext.fillCount, 3, "cutout mode restores both complete side faces");
    assert.equal(canvas.context.drawCalls.at(-2).operation, "source-over", "relief remains translucent source-over, not a cutout");
    const recoloredSource = source.slice();
    recoloredSource[(3 * 10 + 1) * 4] = 222;
    assert.equal(renderer.paint(recoloredSource, 0.5, tilePixels, projection, { mode: "contact", softness: 1 }), true);
    assert.deepEqual(renderer.tileSupportedFaces[0].color, [222, 80, 40], "boundary color changes also update the supported side material");
    assert.equal(renderer.sourceImage.data[(3 * 10 + 1) * 4], 222, "printed face still updates");
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection, { mode: "cutout", softness: 0.5 }), true);
    const softerShadow = reliefContext.events.filter(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas).at(-1)[1];
    assert.equal(softerShadow.filter, `blur(${expectedShadowBlur(0.5, 0.5, 10, 10, 12, 12)}px)`);

    const beforeZeroOverlapFills = reliefContext.fillCount;
    const zeroOverlapStart = reliefContext.events.length;
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection,
      { mode: "cutout", softness: 1, overlapFraction: 0 }), true);
    assert.equal(reliefContext.fillCount, beforeZeroOverlapFills, "zero overlap removes only the added tile faces");
    assert.equal(reliefContext.events.slice(zeroOverlapStart)
      .some(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas), false,
    "zero overlap collapses the relief shadow");
    assert.equal(renderer.paint(source, 0.5, tilePixels, projection,
      { mode: "cutout", softness: 1, overlapFraction: 1 }), true, "restoring the fraction redraws full depth");

    const tileColorOnly = tilePixels.slice();
    tileColorOnly[0] = 200;
    assert.equal(renderer.paint(source, 0.5, tileColorOnly, projection, { mode: "cutout", softness: 1 }), false);
    const tileAlphaChanged = tileColorOnly.slice();
    tileAlphaChanged[(2 * 10 + 3) * 4 + 3] = 255;
    const beforeAlphaUpdate = renderer.occluderContext.events.length;
    assert.equal(renderer.paint(source, 0.5, tileAlphaChanged, projection, { mode: "contact", softness: 1 }), true);
    assert.ok(renderer.occluderContext.events.length > beforeAlphaUpdate);
    assert.equal(renderer.tileSupportedFaces.length, 2, "alpha changes rebuild contact segments");
    assert.equal(renderer.paint(source, 0.5, tileAlphaChanged, { scale: 0.8, x: 0.2, y: -0.1 }, { mode: "contact", softness: 1 }), true);
    assert.equal(renderer.lastProjectionScale, 0.8);

    const reusedRelief = renderer.reliefCanvas;
    renderer.resize(6, 6, 1);
    assert.equal(renderer.paint(source, 0.5, tileAlphaChanged, { scale: 0.8, x: 0.2, y: -0.1 }, { mode: "contact", softness: 1 }), true);
    assert.equal(renderer.reliefCanvas, reusedRelief);
    assert.equal(renderer.reliefCanvas.width, 6);
    assert.equal(renderer.reliefCanvas.height, 6);
    assert.equal(renderer.paint(source, 0, tileAlphaChanged, projection, { mode: "cutout", softness: 1 }), true);
    assert.equal(renderer.reliefCanvas.width, 6, "zero depth reuses, but does not grow, relief buffers");
    assert.deepEqual(source, original);

    const zeroCanvas = new MockCanvas(12, 12);
    const zeroRenderer = new CanvasDepthRenderer(zeroCanvas, 10, 10);
    assert.equal(zeroRenderer.paint(source, 0, tilePixels, projection, { mode: "contact", softness: 1 }), true);
    assert.equal(zeroRenderer.reliefCanvas, null, "zero depth collapses relief and does not allocate scratch buffers");
    assert.deepEqual(zeroCanvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: zeroRenderer.sourceCanvas, operation: "source-over" },
    ]);
    zeroRenderer.paint(source, 0.5, tilePixels, projection, { mode: "contact", softness: 1 });
    assert.equal(zeroRenderer.tileSupportedFaces.length, 1, "contact geometry returns when thickness rises from zero");
    zeroRenderer.paint(source, 0, tilePixels, projection, { mode: "contact", softness: 1 });
    zeroRenderer.paint(source, 0.5, tilePixels, projection, { mode: "contact", softness: 1 });
    assert.equal(zeroRenderer.tileSupportedFaces.length, 1, "zero thickness preserves reusable contact classification");

    assert.throws(() => renderer.paint(source, 0.5, tilePixels, projection, { mode: "unknown", softness: 1 }), RangeError);
    assert.throws(() => renderer.paint(source, 0.5, tilePixels, projection, { mode: "contact", softness: 2.01 }), RangeError);
    assert.throws(() => renderer.paint(source, 0.5, tilePixels, projection, { mode: "contact", overlapFraction: -0.01 }), RangeError);
    assert.throws(() => renderer.paint(source, 0.5, tilePixels, projection, { mode: "contact", overlapFraction: 1.01 }), RangeError);
    assert.throws(() => renderer.paint(source, 0.5, tilePixels, projection, { mode: "contact", overlapFraction: Infinity }), RangeError);
    assert.throws(() => renderer.paint(source, 0.5, tilePixels, projection, true), TypeError);
  } finally {
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("relief shadows stay near contact even as cardboard depth increases", () => {
  class MockContext {
    constructor() {
      this.events = [];
      this.drawCalls = [];
      this.transformOps = [];
      this.stack = [];
      this.globalAlpha = 1;
      this.globalCompositeOperation = "source-over";
      this.imageSmoothingEnabled = false;
      this.filter = "none";
    }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData() {}
    setTransform(...values) { this.transformOps = [["setTransform", ...values]]; this.events.push(["setTransform", ...values]); }
    clearRect(...values) { this.drawCalls = []; this.events.push(["clearRect", ...values]); }
    save() {
      this.stack.push({ transformOps: this.transformOps.slice(), globalAlpha: this.globalAlpha,
        globalCompositeOperation: this.globalCompositeOperation, imageSmoothingEnabled: this.imageSmoothingEnabled, filter: this.filter });
    }
    restore() { Object.assign(this, this.stack.pop()); }
    translate(x, y) { this.transformOps.push(["translate", x, y]); }
    scale(x, y) { this.transformOps.push(["scale", x, y]); }
    beginPath() {}
    moveTo(...values) { this.events.push(["moveTo", ...values]); }
    lineTo(...values) { this.events.push(["lineTo", ...values]); }
    closePath() {}
    fill() {}
    fillRect() {}
    drawImage(image, ...values) {
      this.drawCalls.push({ image, operation: this.globalCompositeOperation, alpha: this.globalAlpha,
        filter: this.filter, transforms: this.transformOps.slice(), values });
      this.events.push(["drawImage", this.drawCalls.at(-1)]);
    }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }

  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  const originalRotation = [...getSceneLightRotation()];
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    const canvas = new MockCanvas(20, 16);
    const renderer = new CanvasDepthRenderer(canvas, 10, 8);
    const source = rgba(10, 8, [[1, 2, 200, 100, 50]]);
    const tiles = rgba(10, 8, [[1, 2, 0, 0, 0, 255]]);
    const depth = 2;
    assert.equal(renderer.paint(source, depth, tiles, { scale: 0.9, x: 0.4, y: -0.2 }, { mode: "shadows", softness: 1 }), true);

    const shadowDraw = renderer.reliefContext.events.find(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas)[1];
    assert.deepEqual(shadowDraw.transforms, expectedShadowTransform(depth, 10, 8, 20, 16));
    assert.equal(shadowDraw.filter, `blur(${expectedShadowBlur(depth, 1, 10, 8, 20, 16)}px)`);
    const maskDraw = renderer.reliefContext.drawCalls.at(-1);
    assert.equal(maskDraw.image, renderer.occluderCanvas);
    assert.equal(maskDraw.operation, "destination-in");
    assert.deepEqual(maskDraw.transforms, [
      ["setTransform", 2, 0, 0, 2, 0, 0], ["translate", 5.4, 3.8], ["scale", 0.9, 0.9], ["translate", -5, -4],
    ]);
    assert.equal(renderer.reliefContext.events.filter(([name]) => name === "fill").length, 0);

    renderer.paint(source, 6, tiles, { scale: 0.9, x: 0.4, y: -0.2 }, { mode: "shadows", softness: 1 });
    const deeperShadow = renderer.reliefContext.events.filter(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas).at(-1)[1];
    assert.deepEqual(deeperShadow.transforms, shadowDraw.transforms, "backdrop-scale thickness cannot stretch the contact shadow");
    assert.equal(deeperShadow.filter, shadowDraw.filter, "contact diffusion also has a finite range");
    const cameraAndLightScale = deeperShadow.transforms[2][1];
    assert.ok(Math.abs(cameraAndLightScale - 1) < 0.005, "contact shadow stays within half a percent of the source silhouette size");
    const offsetX = deeperShadow.transforms[1][1] - 10;
    const offsetY = deeperShadow.transforms[1][2] - 8;
    assert.equal(offsetX, 0, "the centered light adds no uniform sideways offset");
    assert.ok(offsetY > 0 && offsetY < 0.1,
      "nearby pieces have only a subpixel downward shadow offset");

    setSceneLightRotation([-1, 0, 0, 0, 1, 0, 0, 0, -1]);
    renderer.paint(source, depth, tiles, { scale: 0.9, x: 0.4, y: -0.2 }, { mode: "shadows", softness: 1 });
    const blockedShadow = renderer.reliefContext.events
      .filter(([name, event]) => name === "drawImage" && event.image === renderer.sourceCanvas).at(-1)[1];
    assert.equal(blockedShadow.alpha, 0, "a light behind the caster contributes no contact shadow");
    assert.ok(blockedShadow.transforms.flat().every(value => typeof value !== "number" || Number.isFinite(value)),
      "blocked contact geometry remains finite");
  } finally {
    setSceneLightRotation(originalRotation);
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("tile relief casts onto the alpha-masked rear sprite before tile walls and source", () => {
  class MockContext {
    constructor() {
      this.events = [];
      this.drawCalls = [];
      this.fillCount = 0;
      this.transformOps = [];
      this.stack = [];
      this.globalAlpha = 1;
      this.globalCompositeOperation = "source-over";
      this.imageSmoothingEnabled = false;
      this.filter = "none";
      this.fillStyle = "#000";
      this.putCount = 0;
    }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData() { this.putCount += 1; }
    setTransform(...values) { this.transformOps = [["setTransform", ...values]]; this.events.push(["setTransform", ...values]); }
    clearRect(...values) { this.drawCalls = []; this.events.push(["clearRect", ...values]); }
    save() {
      this.stack.push({ transformOps: this.transformOps.slice(), globalAlpha: this.globalAlpha,
        globalCompositeOperation: this.globalCompositeOperation, imageSmoothingEnabled: this.imageSmoothingEnabled,
        filter: this.filter, fillStyle: this.fillStyle });
    }
    restore() { Object.assign(this, this.stack.pop()); }
    translate(x, y) { this.transformOps.push(["translate", x, y]); }
    scale(x, y) { this.transformOps.push(["scale", x, y]); }
    beginPath() {}
    moveTo(...values) { this.events.push(["moveTo", ...values]); }
    lineTo(...values) { this.events.push(["lineTo", ...values]); }
    closePath() {}
    fill() { this.fillCount += 1; this.events.push(["fill", this.globalCompositeOperation]); }
    fillRect(...values) { this.events.push(["fillRect", this.globalCompositeOperation, ...values]); }
    drawImage(image, ...values) {
      const call = { image, operation: this.globalCompositeOperation, alpha: this.globalAlpha,
        filter: this.filter, transforms: this.transformOps.slice(), values };
      this.drawCalls.push(call);
      this.events.push(["drawImage", call]);
    }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }

  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    const canvas = new MockCanvas(8, 6);
    const renderer = new CanvasDepthRenderer(canvas, 4, 3);
    const tileSource = rgba(4, 3, [[0, 0, 180, 120, 60]]);
    const rearSprites = rgba(4, 3, [[0, 0, 0, 0, 0, 255], [1, 0, 40, 50, 60, 127]]);
    const options = { mode: "contact", softness: 0.5, receiverRGBA: rearSprites };

    assert.equal(renderer.paint(tileSource, 0.5, null, undefined, options), true);
    assert.equal(renderer.occluderCanvas, null, "receiver alpha is separate from wall-occlusion alpha");
    assert.ok(renderer.receiverCanvas);
    assert.deepEqual(Array.from(renderer.receiverImage.data.slice(0, 4)), [0, 0, 0, 255]);
    assert.deepEqual(Array.from(renderer.receiverImage.data.slice(4, 8)), [0, 0, 0, 127]);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.reliefCanvas, operation: "source-over" },
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
    const shadowMask = renderer.reliefContext.drawCalls.at(-1);
    assert.equal(shadowMask.image, renderer.receiverCanvas);
    assert.equal(shadowMask.operation, "destination-in");
    const mainShadow = canvas.context.events.find(([name, call]) => name === "drawImage" && call.image === renderer.reliefCanvas);
    const firstWall = canvas.context.events.findIndex(([name]) => name === "fill");
    const sourceArt = canvas.context.events.findIndex(([name, call]) => name === "drawImage" && call.image === renderer.sourceCanvas);
    assert.ok(canvas.context.events.indexOf(mainShadow) < firstWall && firstWall < sourceArt,
      "receiver shadow is composited before the caster's walls and exact source art");
    assert.equal(renderer.reliefContext.events.filter(([name]) => name === "fill").length, 0,
      "auxiliary receiver mode does not redraw the tile's existing sidewalls");

    const colorOnlyReceiver = rearSprites.slice();
    colorOnlyReceiver[0] = 220;
    assert.equal(renderer.paint(tileSource, 0.5, null, undefined,
      { ...options, receiverRGBA: colorOnlyReceiver }), false, "receiver RGB changes do not rebuild its alpha mask");
    const alphaChangedReceiver = colorOnlyReceiver.slice();
    alphaChangedReceiver[7] = 0;
    const receiverPutCount = renderer.receiverContext.putCount;
    assert.equal(renderer.paint(tileSource, 0.5, null, undefined,
      { ...options, receiverRGBA: alphaChangedReceiver }), true);
    assert.equal(renderer.receiverContext.putCount, receiverPutCount + 1);

    const capMainStart = canvas.context.events.length;
    const capReliefStart = renderer.reliefContext.events.length;
    const beforeCapFaces = renderer.reliefContext.fillCount;
    assert.equal(renderer.paint(tileSource, 0.5, null, undefined,
      { mode: "shadows", softness: 0.5, overlapFraction: 1 / 3, receiverRGBA: rearSprites }), true);
    const cappedEvents = canvas.context.events.slice(capMainStart);
    const fullWall = cappedEvents.findIndex(([name]) => name === "fill");
    const eraseFullWall = cappedEvents.findIndex(([name, call]) => name === "drawImage"
      && call.image === renderer.receiverCanvas && call.operation === "destination-out");
    const localOverlay = cappedEvents.findIndex(([name, call]) => name === "drawImage"
      && call.image === renderer.reliefCanvas && call.operation === "source-over");
    const cappedSource = cappedEvents.findIndex(([name, call]) => name === "drawImage" && call.image === renderer.sourceCanvas);
    assert.ok(fullWall >= 0 && fullWall < eraseFullWall && eraseFullWall < localOverlay && localOverlay < cappedSource,
      "reduced receiver thickness replaces full walls only inside receiver alpha before source art");
    assert.ok(renderer.reliefContext.fillCount > beforeCapFaces,
      "shadows-only still caps the tile's existing walls over the receiver");
    const localShadow = renderer.reliefContext.events.slice(capReliefStart)
      .find(([name, call]) => name === "drawImage" && call.image === renderer.sourceCanvas)[1];
    const localDepth = 0.5 / 3;
    assert.deepEqual(localShadow.transforms, expectedShadowTransform(localDepth, 4, 3, 8, 6));
    const wallRear = projectDepthPoint({ x: 1, y: 0 }, localDepth, 4, 3);
    assert.ok(renderer.reliefContext.events.slice(capReliefStart).some(([name, ...args]) => name === "lineTo"
      && args[0] === wallRear.x && args[1] === wallRear.y),
    "receiver walls retain reduced cardboard depth independently of the tiny contact gap");
    assert.equal(renderer.reliefContext.drawCalls.at(-1).image, renderer.receiverCanvas);
    assert.equal(renderer.reliefContext.drawCalls.at(-1).operation, "destination-in");

    const zeroCapMainStart = canvas.context.events.length;
    const zeroCapReliefStart = renderer.reliefContext.events.length;
    const beforeZeroCapFaces = renderer.reliefContext.fillCount;
    assert.equal(renderer.paint(tileSource, 0.5, null, undefined,
      { mode: "shadows", softness: 0.5, overlapFraction: 0, receiverRGBA: rearSprites }), true);
    const zeroCapEvents = canvas.context.events.slice(zeroCapMainStart);
    assert.ok(zeroCapEvents.some(([name, call]) => name === "drawImage"
      && call.image === renderer.receiverCanvas && call.operation === "destination-out"));
    assert.equal(renderer.reliefContext.fillCount, beforeZeroCapFaces,
      "zero overlap removes local walls as well as the local shadow");
    assert.equal(renderer.reliefContext.events.slice(zeroCapReliefStart)
      .some(([name, call]) => name === "drawImage" && call.image === renderer.sourceCanvas), false,
    "zero overlap does not project a receiver shadow");

    assert.equal(renderer.paint(tileSource, 0.5, null, undefined,
      { mode: "off", softness: 0.5, overlapFraction: 0, receiverRGBA: alphaChangedReceiver }), true);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
    assert.equal(renderer.paint(tileSource, 0.5, null, undefined,
      { mode: "contact", softness: 0.5, overlapFraction: 0.5, receiverRGBA: null }), true);
    assert.equal(renderer.hasReceiver, false);
    assert.deepEqual(canvas.context.drawCalls.map(({ image, operation }) => ({ image, operation })), [
      { image: renderer.sourceCanvas, operation: "source-over" },
    ]);
  } finally {
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});
