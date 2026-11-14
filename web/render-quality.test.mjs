import test from "node:test";
import assert from "node:assert/strict";
import { RenderQuality, effectPixelRatio, frameBudget } from "./render-quality.js";

test("auto adapts after sustained cost and recovers with hysteresis", () => {
  const quality = new RenderQuality();
  for (let i=0; i<119; i++) assert.equal(quality.sample(16), false);
  assert.equal(quality.sample(16), true);
  assert.equal(quality.maxWidth, 512);
  for (let i=0; i<599; i++) assert.equal(quality.sample(3), false);
  assert.equal(quality.sample(3), true);
  assert.equal(quality.maxWidth, 768);
  quality.setMode("high");
  for (let i=0; i<180; i++) quality.sample(40);
  assert.equal(quality.maxWidth, 1536);
  quality.setMode("auto");
  assert.equal(quality.maxWidth, 768);
});

test("large TV and dense phone effect buffers respect the selected budget", () => {
  assert.equal(effectPixelRatio(3840, 1, 768) * 3840, 768);
  assert.equal(effectPixelRatio(400, 3, 768) * 400, 768);
  assert.equal(effectPixelRatio(400, 1, 1536), 1);
});

test("catch-up preserves all bounded simulation steps and discards excess backlog", () => {
  assert.deepEqual(frameBudget(95, 10), { frames: 4, remainder: 5 });
  assert.deepEqual(frameBudget(35, 10), { frames: 3, remainder: 5 });
  assert.deepEqual(frameBudget(5, 10), { frames: 0, remainder: 5 });
});
