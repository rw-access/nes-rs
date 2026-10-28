import test from "node:test";
import assert from "node:assert/strict";
import {
  calculateDioramaProjection,
  DioramaFrame,
  FAR_SHADOW_OPACITY,
  NEAR_SHADOW_OPACITY,
} from "./diorama-frame.js";
import { calculateRimProjection } from "./rim-projection.js";
import { RIM_CORNER_FRACTION } from "./rim-outline.js";
import {
  getSceneLightRevision,
  getSceneLightRotation,
  setSceneLightDistance,
  setSceneLightRotation,
} from "./scene-lighting.js";
import { getSceneViewRevision, setSceneView } from "./scene-view.js";

function rgba(width, height, color = [10, 20, 30, 255]) {
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let offset = 0; offset < pixels.length; offset += 4) pixels.set(color, offset);
  return pixels;
}

function rotationXForLightDepth(depth, distance = 2) {
  const baseY = -84 * distance;
  const baseZ = -24 * distance * 19.2;
  const phase = Math.atan2(baseZ, baseY);
  const targetPhase = -Math.PI - Math.asin(depth * 19.2 / Math.hypot(baseY, baseZ));
  const angle = targetPhase - phase;
  const c = Math.cos(angle), s = Math.sin(angle);
  return [1, 0, 0, 0, c, -s, 0, s, c];
}

function rotationZ(angle) {
  const c = Math.cos(angle), s = Math.sin(angle);
  return [c, -s, 0, s, c, 0, 0, 0, 1];
}

function mockCanvasTypes() {
  class MockContext {
    constructor() {
      this.events = [];
      this.putCount = 0;
      this.fillCount = 0;
      this.stateStack = [];
      this._filter = "none";
      this.globalAlpha = 1;
      this.globalCompositeOperation = "source-over";
    }
    set filter(value) { this._filter = value; this.events.push(["filter", value]); }
    get filter() { return this._filter; }
    createImageData(width, height) { return { data: new Uint8ClampedArray(width * height * 4) }; }
    putImageData(image) {
      this.image = new Uint8ClampedArray(image.data);
      this.putCount += 1;
      this.events.push(["putImageData"]);
    }
    setTransform(...args) { this.events.push(["setTransform", ...args]); }
    clearRect(...args) { this.events.push(["clearRect", ...args]); }
    fillRect(...args) { this.fillCount += 1; this.events.push(["fillRect", ...args]); }
    save() {
      this.stateStack.push({ globalCompositeOperation: this.globalCompositeOperation, globalAlpha: this.globalAlpha });
      this.events.push(["save"]);
    }
    restore() {
      const state = this.stateStack.pop();
      if (state) Object.assign(this, state);
      this.events.push(["restore"]);
    }
    translate(...args) { this.events.push(["translate", ...args]); }
    scale(...args) { this.events.push(["scale", ...args]); }
    drawImage(...args) {
      this.events.push(["drawImage", this.globalAlpha, this.globalCompositeOperation, ...args]);
    }
    beginPath() { this.events.push(["beginPath"]); }
    moveTo(...args) { this.events.push(["moveTo", ...args]); }
    lineTo(...args) { this.events.push(["lineTo", ...args]); }
    ellipse(...args) { this.events.push(["ellipse", ...args]); }
    closePath() { this.events.push(["closePath"]); }
    fill() { this.events.push(["fill", this.globalCompositeOperation]); }
    rect(...args) { this.events.push(["rect", ...args]); }
    clip() { this.events.push(["clip"]); }
  }
  class MockCanvas {
    constructor(width, height) { this.width = width; this.height = height; this.context = new MockContext(); }
    getContext() { return this.context; }
  }
  return { MockContext, MockCanvas };
}

test("diorama projection shares the perspective near-rim geometry", () => {
  const expected = calculateRimProjection(256, 240, 2, 1.5, 1, 0.25);
  assert.deepEqual(calculateDioramaProjection(256, 240, 2, 1.5, 1, 0.25), expected);
  const far = calculateDioramaProjection(256, 240, 2);
  const near = calculateDioramaProjection(256, 240, 2, 0);
  assert.equal(far.blurDistance, far.travel * far.receiverScale);
  assert.equal(near.blurDistance, near.travel * near.receiverScale);
  assert.ok(far.apertureScale > far.receiverScale, "nearby-light magnification scales the shadow aperture");
  assert.ok(near.blurDistance < far.blurDistance);
  assert.ok(far.litOpening.width > 0 && far.litOpening.height > 0);
  assert.ok(near.litOpening.width > 0 && near.litOpening.height > 0);
});

test("zero gap draws the original full backdrop and emits no shadow", () => {
  const { MockCanvas } = mockCanvasTypes();
  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    const canvas = new MockCanvas(400, 300);
    const renderer = new DioramaFrame(canvas, 4, 3);
    renderer.resize(400, 300, 1);
    const backdrop = rgba(4, 3, [80, 70, 60, 255]);
    const original = backdrop.slice();
    assert.equal(renderer.paint(backdrop, 0), true);
    assert.equal(renderer.shadowCanvas, null);
    assert.deepEqual(renderer.backdropContext.image, original);
    const draws = canvas.context.events.filter(([name]) => name === "drawImage");
    assert.equal(draws.length, 1);
    assert.equal(draws[0][1], 1);
    assert.equal(draws[0][2], "source-over");
    assert.equal(draws[0][3], renderer.backdropCanvas);
    assert.deepEqual(draws[0].slice(-4), [0, 0, 4, 3]);
    assert.equal(renderer.paint(backdrop, 0), false);
    assert.deepEqual(backdrop, original);
  } finally {
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("far and near masks stay separate, blur by rim travel, and composite through source alpha", () => {
  const { MockCanvas } = mockCanvasTypes();
  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    const canvas = new MockCanvas(256, 192);
    const renderer = new DioramaFrame(canvas, 4, 3);
    renderer.resize(256, 192, 1);
    const backdrop = rgba(4, 3);
    assert.equal(renderer.paint(backdrop, 2, 0.5), true);

    const backdropDraws = canvas.context.events.filter(([name]) => name === "drawImage");
    assert.equal(backdropDraws.length, 2);
    assert.equal(backdropDraws[0][1], 1);
    assert.equal(backdropDraws[0][2], "source-over");
    assert.equal(backdropDraws[0][3], renderer.backdropCanvas);
    assert.deepEqual(backdropDraws[0].slice(-4), [0, 0, 4, 3]);
    assert.equal(backdropDraws[1][1], FAR_SHADOW_OPACITY);
    assert.equal(backdropDraws[1][2], "source-over");
    assert.equal(backdropDraws[1][3], renderer.shadowCanvas);
    assert.equal(canvas.context.events.some(([name]) => name === "fillRect"), false);

    const farLayer = renderer.shadowLayers.far;
    const farFillCount = farLayer.maskContext.fillCount;
    const farCutoutCount = farLayer.maskContext.events.filter(([name]) => name === "fill").length;
    const farMaskDraw = farLayer.shadowContext.events.find(([name]) => name === "drawImage");
    assert.equal(farMaskDraw[3], farLayer.maskCanvas);
    const projection = calculateRimProjection(4, 3, 2, 2);
    assert.ok(farLayer.shadowContext.events.some(([name, ...args]) => name === "rect"
      && args[0] === 0 && args[1] === 0 && args[2] === canvas.width && args[3] === canvas.height));
    const pad = Math.ceil(0.5 * projection.blurDistance * 64 * 3 + 1);
    const clearCalls = farLayer.maskContext.events.filter(([name]) => name === "clearRect");
    assert.deepEqual(clearCalls, [["clearRect", 0, 0, farLayer.maskCanvas.width, farLayer.maskCanvas.height]]);
    const cutoutFills = farLayer.maskContext.events.filter(([name]) => name === "fill");
    assert.deepEqual(cutoutFills, [["fill", "destination-out"]]);
    const expectedRx = 4 * RIM_CORNER_FRACTION * projection.apertureScale * 64;
    const expectedRy = 3 * RIM_CORNER_FRACTION * projection.apertureScale * 64;
    const topLeft = {
      x: pad + projection.litOpening.x * 64,
      y: pad + projection.litOpening.y * 64,
    };
    const bottomRight = {
      x: pad + (projection.litOpening.x + projection.litOpening.width) * 64,
      y: pad + (projection.litOpening.y + projection.litOpening.height) * 64,
    };
    const ellipseCalls = farLayer.maskContext.events.filter(([name]) => name === "ellipse");
    assert.equal(ellipseCalls.length, 4);
    for (const [, , , rx, ry] of ellipseCalls) {
      assert.equal(rx, expectedRx);
      assert.equal(ry, expectedRy);
    }
    assert.deepEqual(farLayer.maskContext.events.find(([name]) => name === "moveTo"), [
      "moveTo", topLeft.x + expectedRx, topLeft.y,
    ]);
    assert.deepEqual(farLayer.maskContext.events.find(([name]) => name === "lineTo"), [
      "lineTo", bottomRight.x - expectedRx, topLeft.y,
    ]);
    assert.equal(farLayer.maskContext.globalCompositeOperation, "source-over");

    const source = new MockCanvas(canvas.width, canvas.height);
    const target = new MockCanvas(canvas.width, canvas.height);
    assert.equal(renderer.paintForeground(source, target, 2, 0.5), true);
    const nearLayer = renderer.shadowLayers.near;
    assert.notEqual(nearLayer.shadowCanvas, farLayer.shadowCanvas);
    assert.equal(nearLayer.maskContext.fillCount, 0);
    const nearMaskDraw = nearLayer.shadowContext.events.find(([name]) => name === "drawImage");
    assert.equal(nearMaskDraw[3], nearLayer.maskCanvas);
    assert.notEqual(nearMaskDraw[3], farMaskDraw[3]);
    const foregroundDraws = target.context.events.filter(([name]) => name === "drawImage");
    assert.equal(foregroundDraws.length, 2);
    assert.equal(foregroundDraws[0][1], NEAR_SHADOW_OPACITY);
    assert.equal(foregroundDraws[0][2], "source-over");
    assert.equal(foregroundDraws[0][3], nearLayer.shadowCanvas);
    assert.equal(foregroundDraws[1][1], 1);
    assert.equal(foregroundDraws[1][2], "destination-in");
    assert.equal(foregroundDraws[1][3], source);
    const nearFillCount = nearLayer.maskContext.fillCount;
    assert.equal(renderer.paintForeground(source, target, 2, 0.5), true);
    assert.equal(nearLayer.maskContext.fillCount, nearFillCount);

    backdrop[0] = 90;
    assert.equal(renderer.paint(backdrop, 2, 0.5), true);
    assert.equal(farLayer.maskContext.fillCount, farFillCount);
    assert.equal(renderer.backdropContext.image[0], 90);
    assert.equal(renderer.paint(backdrop, 2, 0.75), true);
    assert.ok(farLayer.maskContext.fillCount > farFillCount);
    assert.ok(farLayer.maskContext.events.filter(([name]) => name === "fill").length > farCutoutCount);
    assert.equal(farLayer.shadowContext.events.filter(([name]) => name === "drawImage").at(-1)[3], farLayer.maskCanvas);
    const afterFarSoftness = farLayer.maskContext.fillCount;
    renderer.paintForeground(source, target, 2, 0.75);
    assert.equal(nearLayer.maskContext.fillCount, 0);
    assert.equal(nearLayer.key.softness, .75);
    assert.equal(nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").at(-1)[3], nearLayer.maskCanvas);
    assert.equal(renderer.paint(backdrop, 2, 0.75), false);
    renderer.paintForeground(source, target, 2, 2);
    assert.throws(() => renderer.paint(backdrop, 2, 2.01), RangeError);
    assert.throws(() => renderer.paintForeground(source, target, 2, 2.01), RangeError);
    assert.equal(renderer.resize(1000, 750, 3).width, 1536);
    assert.equal(renderer.paint(backdrop, 2, 0.75), true);
    renderer.paintForeground(source, target, 2, 0.75);
    assert.ok(farLayer.maskContext.fillCount > afterFarSoftness);
    const beforeThickness = farLayer.maskContext.fillCount;
    assert.equal(renderer.paint(backdrop, 2, .75, 2), true);
    renderer.paintForeground(source, target, 2, .75, 2);
    assert.ok(farLayer.maskContext.fillCount > beforeThickness);
    assert.equal(nearLayer.maskContext.fillCount, 0);
    assert.equal(farLayer.key.thickness, 2);
    assert.equal(nearLayer.key.thickness, 2);

    assert.equal(renderer.paint(backdrop, 2, .75, 1.5, 0), true);
    renderer.paintForeground(source, target, 2, .75, 1.5, 0);
    const beforeFrontDepth = farLayer.maskContext.fillCount;
    assert.equal(renderer.paint(backdrop, 2, .75, 1.5, .25), true);
    renderer.paintForeground(source, target, 2, .75, 1.5, .25);
    assert.ok(farLayer.maskContext.fillCount > beforeFrontDepth);
    assert.equal(farLayer.key.frontDepth, .25);
    assert.equal(nearLayer.key.frontDepth, .25);
    const afterFrontDepth = farLayer.maskContext.fillCount;
    assert.equal(renderer.paint(backdrop, 2, .75, 1.5, .25), false);
    renderer.paintForeground(source, target, 2, .75, 1.5, .25);
    assert.equal(farLayer.maskContext.fillCount, afterFrontDepth);
    assert.throws(() => renderer.paint(backdrop, 2, .75, 1.8, .25), RangeError);
    assert.throws(() => renderer.paintForeground(source, target, 2, .75, 1.8, .25), RangeError);
  } finally {
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("light-only changes invalidate far and near shadows without rewriting backdrop pixels", () => {
  const { MockCanvas } = mockCanvasTypes();
  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    setSceneLightDistance(2);
    const canvas = new MockCanvas(24, 18);
    const renderer = new DioramaFrame(canvas, 4, 3);
    renderer.resize(24, 18, 1);
    const backdrop = rgba(4, 3);
    assert.equal(renderer.paint(backdrop, 2), true);
    const source = new MockCanvas(canvas.width, canvas.height);
    const target = new MockCanvas(canvas.width, canvas.height);
    renderer.paintForeground(source, target, 2);

    const farLayer = renderer.shadowLayers.far;
    const nearLayer = renderer.shadowLayers.near;
    const backdropWrites = renderer.backdropContext.putCount;
    const farFills = farLayer.maskContext.fillCount;
    const farDraws = farLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    const nearDraws = nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    const priorRevision = getSceneLightRevision();
    assert.equal(renderer.paint(backdrop, 2), false);

    setSceneLightDistance(3);
    const changedRevision = getSceneLightRevision();
    assert.equal(changedRevision, priorRevision + 1);
    assert.equal(renderer.paint(backdrop, 2), true);
    assert.equal(renderer.backdropContext.putCount, backdropWrites, "light changes do not rewrite backdrop pixels");
    assert.ok(farLayer.maskContext.fillCount > farFills, "the far aperture mask is rebuilt");
    assert.equal(farLayer.key.lightingRevision, changedRevision);
    assert.ok(farLayer.shadowContext.events.filter(([name]) => name === "drawImage").length > farDraws);

    renderer.paintForeground(source, target, 2);
    assert.equal(nearLayer.key.lightingRevision, changedRevision);
    assert.ok(nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length > nearDraws,
      "the near shadow layer also rebuilds");
    const stableFarFills = farLayer.maskContext.fillCount;
    const stableFarDraws = farLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    const stableNearDraws = nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    assert.equal(renderer.paint(backdrop, 2), false);
    renderer.paintForeground(source, target, 2);
    assert.equal(farLayer.maskContext.fillCount, stableFarFills);
    assert.equal(farLayer.shadowContext.events.filter(([name]) => name === "drawImage").length, stableFarDraws);
    assert.equal(nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length, stableNearDraws);

    setSceneLightDistance(2);
    assert.equal(renderer.paint(backdrop, 2), true, "restoring the preset rebuilds far shadow output");
    renderer.paintForeground(source, target, 2);
    assert.equal(farLayer.key.lightingRevision, getSceneLightRevision());
    assert.equal(nearLayer.key.lightingRevision, getSceneLightRevision());
    assert.equal(renderer.backdropContext.putCount, backdropWrites);
  } finally {
    setSceneLightDistance(2);
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("view-only changes invalidate both shadow masks without rewriting backdrop pixels", () => {
  const { MockCanvas } = mockCanvasTypes();
  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = MockCanvas;
  setSceneView(0, 0);
  try {
    const canvas = new MockCanvas(24, 18);
    const renderer = new DioramaFrame(canvas, 4, 3);
    renderer.resize(24, 18, 1);
    const backdrop = rgba(4, 3);
    assert.equal(renderer.paint(backdrop, 2), true);
    const source = new MockCanvas(canvas.width, canvas.height);
    const target = new MockCanvas(canvas.width, canvas.height);
    renderer.paintForeground(source, target, 2);

    const farLayer = renderer.shadowLayers.far;
    const nearLayer = renderer.shadowLayers.near;
    const backdropWrites = renderer.backdropContext.putCount;
    const farFills = farLayer.maskContext.fillCount;
    const nearFills = nearLayer.maskContext.fillCount;
    const nearDraws = nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    const oldRevision = getSceneViewRevision();
    setSceneView(.18, -.12);
    const newRevision = getSceneViewRevision();
    assert.equal(newRevision, oldRevision + 1);

    assert.equal(renderer.paint(backdrop, 2), true);
    assert.equal(renderer.backdropContext.putCount, backdropWrites, "camera movement leaves source backdrop pixels untouched");
    assert.ok(farLayer.maskContext.fillCount > farFills, "far aperture mask follows the moved vanishing point");
    assert.equal(farLayer.key.viewRevision, newRevision);
    renderer.paintForeground(source, target, 2);
    assert.equal(nearLayer.maskContext.fillCount, nearFills, "a receiver in front of the rim does not get a shadow mask fill");
    assert.ok(nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length > nearDraws,
      "near layer cache follows the same view even when its receiver has no cast shadow");
    assert.equal(nearLayer.key.viewRevision, newRevision);

    const stableFarFills = farLayer.maskContext.fillCount;
    const stableNearFills = nearLayer.maskContext.fillCount;
    assert.equal(renderer.paint(backdrop, 2), false);
    renderer.paintForeground(source, target, 2);
    assert.equal(farLayer.maskContext.fillCount, stableFarFills);
    assert.equal(nearLayer.maskContext.fillCount, stableNearFills);

    setSceneView(0, 0);
    assert.equal(renderer.paint(backdrop, 2), true, "restoring centered view rebuilds the far mask");
    renderer.paintForeground(source, target, 2);
    assert.equal(renderer.backdropContext.putCount, backdropWrites);
    assert.equal(farLayer.key.viewRevision, getSceneViewRevision());
    assert.equal(nearLayer.key.viewRevision, getSceneViewRevision());
  } finally {
    setSceneView(0, 0);
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});

test("room-light rotation invalidates both layers and shadow opacity fades at grazing or behind incidence", () => {
  const { MockCanvas } = mockCanvasTypes();
  const originalOffscreenCanvas = globalThis.OffscreenCanvas;
  const originalRotation = [...getSceneLightRotation()];
  globalThis.OffscreenCanvas = MockCanvas;
  try {
    setSceneLightDistance(2);
    setSceneLightRotation([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    const canvas = new MockCanvas(24, 18);
    const renderer = new DioramaFrame(canvas, 4, 3);
    renderer.resize(24, 18, 1);
    const backdrop = rgba(4, 3);
    assert.equal(renderer.paint(backdrop, 6, 0, 3), true);
    const source = new MockCanvas(canvas.width, canvas.height);
    const target = new MockCanvas(canvas.width, canvas.height);
    renderer.paintForeground(source, target, 6, 0, 3);

    const farLayer = renderer.shadowLayers.far;
    const nearLayer = renderer.shadowLayers.near;
    const backdropWrites = renderer.backdropContext.putCount;
    const farDraws = farLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    const nearDraws = nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length;
    const priorRevision = getSceneLightRevision();
    setSceneLightRotation(rotationZ(Math.PI / 2));
    assert.equal(getSceneLightRevision(), priorRevision + 1);
    assert.equal(renderer.paint(backdrop, 6, 0, 3), true);
    renderer.paintForeground(source, target, 6, 0, 3);
    assert.equal(renderer.backdropContext.putCount, backdropWrites);
    assert.equal(farLayer.key.lightingRevision, getSceneLightRevision());
    assert.equal(nearLayer.key.lightingRevision, getSceneLightRevision());
    assert.ok(farLayer.shadowContext.events.filter(([name]) => name === "drawImage").length > farDraws);
    assert.ok(nearLayer.shadowContext.events.filter(([name]) => name === "drawImage").length > nearDraws);
    assert.equal(renderer.paint(backdrop, 6, 0, 3), false, "identical pose reuses the rebuilt far layer");

    // For a back plane at z=3, this rotation places the light at z=2.5,
    // yielding t=6 and a finite half-strength projected shadow.
    setSceneLightRotation(rotationXForLightDepth(2.5));
    assert.equal(renderer.paint(backdrop, 6, 0, 3), true);
    const grazingProjection = calculateRimProjection(4, 3, 6, 6, 3);
    assert.ok(grazingProjection.opacity > 0 && grazingProjection.opacity < 1);
    assert.equal(farLayer.opacity, grazingProjection.opacity);
    const grazingShadowDraw = canvas.context.events.filter(([name]) => name === "drawImage").at(-1);
    assert.equal(grazingShadowDraw[1], FAR_SHADOW_OPACITY * grazingProjection.opacity);
    assert.ok(Number.isFinite(grazingProjection.litOpening.x));

    setSceneLightRotation([-1, 0, 0, 0, 1, 0, 0, 0, -1]);
    const priorMaskFills = farLayer.maskContext.fillCount;
    assert.equal(renderer.paint(backdrop, 6, 0, 3), true);
    assert.equal(farLayer.opacity, 0);
    assert.equal(farLayer.maskContext.fillCount, priorMaskFills,
      "a back-facing light never fills a fake shadow aperture");
    const blockedShadowDraw = canvas.context.events.filter(([name]) => name === "drawImage").at(-1);
    assert.equal(blockedShadowDraw[1], 0);
    assert.equal(renderer.paint(backdrop, 6, 0, 3), false);
  } finally {
    setSceneLightRotation(originalRotation);
    setSceneLightDistance(2);
    if (originalOffscreenCanvas === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = originalOffscreenCanvas;
  }
});
