import test from "node:test";
import assert from "node:assert/strict";
import { ControllerInput, readGamepads } from "./play-controls.js";
const buttons = { a: 1, b: 2, select: 4, start: 8, up: 16, down: 32, left: 64, right: 128 };

test("independent keys and fingers retain shared buttons until every source releases", () => {
  const changes = [];
  const input = new ControllerInput(mask => changes.push(mask));
  input.set("finger-1", buttons.a);
  input.set("keyboard-z", buttons.a);
  input.set("finger-2", buttons.right | buttons.up);
  input.set("finger-1", 0);
  assert.equal(input.buttons, buttons.a | buttons.right | buttons.up);
  input.set("keyboard-z", 0);
  assert.equal(input.buttons, buttons.right | buttons.up);
  input.clear();
  assert.equal(input.buttons, 0);
  assert.deepEqual(changes, [1, 145, 144, 0]);
});

test("standard controllers combine buttons and stick, ignore unmapped pads and release on disconnect", () => {
  const pad = { connected: true, mapping: "standard", axes: [-0.8, 0.1], buttons: Array.from({length: 16}, (_, i) => ({pressed: [0, 9, 12].includes(i)})) };
  assert.deepEqual(readGamepads([null, pad], buttons), { connected: true, buttons: buttons.a | buttons.start | buttons.up | buttons.left });
  pad.connected = false;
  assert.deepEqual(readGamepads([pad], buttons), { connected: false, buttons: 0 });
  pad.connected = true; pad.mapping = "";
  assert.deepEqual(readGamepads([pad], buttons), { connected: false, buttons: 0 });
});
