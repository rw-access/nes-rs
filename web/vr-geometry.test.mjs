import test from "node:test";
import assert from "node:assert/strict";
import { buildPrismSides, readXRButtons } from "./vr-geometry.js";

const image = (width, height, draw) => {
  const pixels = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const color = draw(x, y); if (color) pixels.set([...color, 255], (y * width + x) * 4);
  }
  return pixels;
};

test("closed rectangle has all four side orientations with outward winding and physical depth", () => {
  const pixels = image(8, 8, (x, y) => x >= 2 && x < 6 && y >= 2 && y < 6 ? [120, 80, 20] : null);
  const before = pixels.slice(), mesh = buildPrismSides(pixels, 8, 8, 0.03);
  assert.equal(mesh.positions.length, 4 * 6 * 3);
  const directions = new Set();
  for (let i = 0; i < mesh.positions.length; i += 9) {
    const a = mesh.positions.slice(i, i + 3), b = mesh.positions.slice(i + 3, i + 6), c = mesh.positions.slice(i + 6, i + 9);
    const u = b.map((v, j) => v - a[j]), v = c.map((value, j) => value - a[j]);
    const cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    const normal = mesh.normals.slice(i, i + 3);
    assert.ok(cross.reduce((sum, value, j) => sum + value * normal[j], 0) > 0);
    directions.add([...normal].join(","));
  }
  assert.equal(directions.size, 4);
  for (let i = 2; i < mesh.positions.length; i += 3) assert.ok(Math.abs(mesh.positions[i]) < 1e-8 || Math.abs(mesh.positions[i] + 0.03) < 1e-8);
  assert.deepEqual(pixels, before);
});

test("holes remain open; interior color changes do not add walls; zero depth adds no sides", () => {
  const full = image(5, 5, () => [100, 150, 200]);
  const colored = full.slice(); colored.set([255, 0, 0, 255], (2 * 5 + 2) * 4);
  assert.deepEqual(buildPrismSides(full, 5, 5, 0.02).positions, buildPrismSides(colored, 5, 5, 0.02).positions);
  const hole = full.slice(); hole[(2 * 5 + 2) * 4 + 3] = 0;
  assert.equal(buildPrismSides(hole, 5, 5, 0.02).positions.length, 8 * 6 * 3);
  assert.equal(buildPrismSides(hole, 5, 5, 0).positions.length, 0);
  assert.equal(buildPrismSides(new Uint8Array(100), 5, 5, 0.02).positions.length, 0);
  assert.throws(() => buildPrismSides(full, 5, 5, NaN));
});

test("boundary pigments stay finite and black pixels remain opaque geometry", () => {
  const black = image(1, 1, () => [0, 0, 0]);
  const mesh = buildPrismSides(black, 1, 1, 0.01);
  assert.equal(mesh.positions.length, 72);
  assert.ok(mesh.colors.every(value => value === 0));
  const white = buildPrismSides(image(1, 1, () => [255, 255, 255]), 1, 1, 0.01);
  assert.ok(white.colors.every(value => value === 1));
});

test("maximum native checkerboard fills exact buffers without dropping exposed faces", () => {
  const width = 256, height = 240;
  const pixels = image(width, height, (x, y) => (x + y) % 2 ? [100, 150, 200] : null);
  const mesh = buildPrismSides(pixels, width, height, 0.018);
  const vertices = width * height / 2 * 4 * 6;
  for (const buffer of Object.values(mesh)) {
    assert.ok(buffer instanceof Float32Array);
    assert.equal(buffer.length, vertices * 3);
    assert.equal(buffer.byteLength, buffer.buffer.byteLength, "no retained spare capacity");
    assert.ok(buffer.every(Number.isFinite));
  }
  for (let i = 0; i < mesh.normals.length; i += 3) {
    assert.equal(Math.abs(mesh.normals[i]) + Math.abs(mesh.normals[i + 1]), 1, "every vertex was written");
    assert.equal(mesh.normals[i + 2], 0);
  }
  // The final bottom edge retains its original triangle order and color.
  assert.deepEqual(mesh.positions.slice(-18), new Float32Array([
    126 / 256, -120 / 256, 0, 127 / 256, -120 / 256, -0.018, 127 / 256, -120 / 256, 0,
    126 / 256, -120 / 256, 0, 126 / 256, -120 / 256, -0.018, 127 / 256, -120 / 256, -0.018,
  ]));
  const linear = value => Math.fround(((value / 255 + 0.055) / 1.055) ** 2.4);
  assert.deepEqual(mesh.colors.slice(-3), new Float32Array([linear(100), linear(150), linear(200)]));
});

test("XR-standard sources combine both hands without treating trigger axes as a D-pad", () => {
  const buttons = { a: 1, b: 2, select: 4, start: 8, up: 16, down: 32, left: 64, right: 128 };
  const pad = (axes, pressed) => ({ mapping: "xr-standard", axes, buttons: Array.from({ length: 6 }, (_, i) => ({ pressed: pressed.includes(i) })) });
  const sources = [{ handedness: "left", gamepad: pad([1, 1, -0.7, -0.8], [5]) }, { handedness: "right", gamepad: pad([0, 0, 0, 0], [0, 5]) }];
  assert.equal(readXRButtons(sources, buttons), buttons.left | buttons.up | buttons.start | buttons.a | buttons.b);
  assert.equal(readXRButtons([{ handedness: "left", gamepad: pad([1, 1, 0, 0], []) }], buttons), 0);
  assert.equal(readXRButtons([{ handedness: "right", gamepad: { mapping: "unknown" } }], buttons), 0);
});
