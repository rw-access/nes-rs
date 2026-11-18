import test from "node:test";
import assert from "node:assert/strict";

const messages = [];
globalThis.self = { postMessage: (value, transfers) => messages.push({ ...value, transfers }) };
await import("./vr-mesh-worker.js");
const BYTES = 256 * 240 * 4;
const run = (pixels, thickness = 0.02, epoch = 1, force = false) => {
  self.onmessage({ data: { pixels, thickness, epoch, force } });
  return messages.pop();
};
const layerIDs = result => result.meshes.map(mesh => mesh.index);
const paint = (pixels, layer, point, rgba) => pixels.set(rgba, BYTES * layer + point * 4);

test("worker errors require a full resync; unchanged frames reuse meshes and textures", () => {
  const pixels = new Uint8Array(BYTES * 4);
  const first = run(pixels);
  assert.deepEqual(layerIDs(first), [1, 2, 3]);
  assert.equal(first.textureMask, 15);
  assert.equal(first.epoch, 1);
  assert.equal(run(pixels).textureMask, 0);
  assert.ok(run(pixels, .26).error);
  assert.ok(run(new Uint8Array(BYTES * 2)).error);
  assert.equal(run(pixels).textureMask, 15);
  assert.equal(run(pixels).meshes.length, 0);
});

test("exact texture flags stay valid after the round-trip buffer is reused", () => {
  const pixels = new Uint8Array(BYTES * 4);
  run(pixels, .02, 2);
  paint(pixels, 0, 30, [80, 60, 40, 255]);
  let result = run(pixels, .02, 2);
  assert.equal(result.textureMask, 1);
  assert.deepEqual(layerIDs(result), [], "backdrop receives only");
  assert.equal(result.pixels, pixels);
  assert.equal(result.transfers[0], pixels.buffer);
  paint(result.pixels, 3, 100, [200, 100, 40, 255]);
  result = run(result.pixels, .02, 2);
  assert.equal(result.textureMask, 8);
  assert.deepEqual(layerIDs(result), [3]);
  assert.ok(result.meshes[0].positions.length > 0);
  paint(result.pixels, 3, 100, [100, 200, 40, 255]);
  result = run(result.pixels, .02, 2);
  assert.equal(result.textureMask, 8);
  assert.deepEqual(layerIDs(result), [3], "RGB-only caster can retessellate");
  paint(result.pixels, 3, 100, [0, 0, 0, 0]);
  paint(result.pixels, 3, 101, [100, 200, 40, 255]);
  result = run(result.pixels, .02, 2);
  assert.equal(result.textureMask, 8);
  assert.deepEqual(layerIDs(result), [3]);
  const depth = run(result.pixels, .03, 2);
  assert.equal(depth.textureMask, 0);
  assert.deepEqual(layerIDs(depth), [1, 2, 3]);
  assert.equal(run(depth.pixels, .03, 2).meshes.length, 0);
});

test("epoch changes and explicit resets rematerialize all layers, including identical ones", () => {
  const pixels = new Uint8Array(BYTES * 4);
  run(pixels, .02, 3);
  assert.equal(run(pixels, .02, 3).textureMask, 0);
  const restored = run(pixels, .02, 4);
  assert.equal(restored.textureMask, 15, "main may have rejected the previous epoch's reply");
  assert.deepEqual(layerIDs(restored), [1, 2, 3]);
  assert.equal(run(pixels, .02, 4, true).textureMask, 15);
  assert.equal(run(pixels, .02, 4).textureMask, 0);
});
