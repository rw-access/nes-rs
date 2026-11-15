import test from "node:test";
import assert from "node:assert/strict";

const messages = [];
globalThis.self = { postMessage: value => messages.push(value) };
await import("./vr-mesh-worker.js");
const BYTES = 256 * 240 * 4;
const run = (pixels, thickness = 0.02, epoch = 1) => {
  self.onmessage({ data: { pixels, thickness, epoch } });
  return messages.pop();
};

test("worker errors clear partial caches; unchanged snapshots reuse meshes", () => {
  // Layer one is valid, but the next layer is truncated. Nothing from the
  // failed job may be treated as an applied mesh by the following request.
  assert.ok(run(new Uint8Array(BYTES * 2)).error);
  const pixels = new Uint8Array(BYTES * 4);
  let result = run(pixels);
  assert.deepEqual(result.meshes.map(mesh => mesh.index), [1, 2, 3]);
  assert.equal(result.epoch, 1);
  assert.equal(run(pixels).meshes.length, 0);
  pixels.set([200, 100, 40, 255], BYTES * 3 + 100 * 4);
  result = run(pixels, 0.02, 2);
  assert.deepEqual(result.meshes.map(mesh => mesh.index), [3]);
  assert.ok(result.meshes[0].positions.length > 0);
  assert.deepEqual(run(pixels, 0.03).meshes.map(mesh => mesh.index), [1, 2, 3]);
});
