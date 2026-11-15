import assert from "node:assert/strict";
import { launch, base } from "./player-test-tools.mjs";
const browser = await launch();
try {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  const page = await context.newPage(), errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(base + "?debug=1&view=orbit");
  await page.waitForFunction(() => window.nesVRDebug?.().framesRendered > 0 && !window.nesVRDebug().meshBusy);
  const cdp = await context.newCDPSession(page), stage = await page.locator("#stage").boundingBox();
  const center = { x: stage.x + stage.width / 2, y: stage.y + stage.height / 2 };
  const touch = (type, points) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points.map(([id, dx, dy]) => ({ id, x: center.x + dx, y: center.y + dy, radiusX: 4, radiusY: 4 })) });
  const state = () => page.evaluate(() => window.nesVRDebug().orbit);
  const before = await state();
  await touch("touchStart", [[1, -35, 0], [2, 35, 0]]);
  await touch("touchMove", [[1, -70, 0], [2, 70, 0]]);
  await page.waitForFunction(distance => window.nesVRDebug().orbit.distance < distance * .65, before.distance);
  const zoomed = await state();
  assert.equal(zoomed.yaw, before.yaw); assert.equal(zoomed.pitch, before.pitch, "pinching cannot rotate the model");
  assert.equal(await page.evaluate(() => visualViewport.scale), 1, "pinching the stage cannot zoom the browser page");
  await touch("touchMove", [[1, -18, 0], [2, 18, 0]]);
  await page.waitForFunction(distance => window.nesVRDebug().orbit.distance > distance * 1.8, zoomed.distance);
  const pinched = await state();
  // Keep one finger down; its next small movement rotates smoothly from its
  // current position rather than using the old pre-pinch drag origin.
  await touch("touchEnd", [[2, 18, 0]]);
  await touch("touchMove", [[1, -16, 0]]);
  await page.waitForFunction(yaw => window.nesVRDebug().orbit.yaw !== yaw, pinched.yaw);
  const rotated = await state();
  assert.ok(Math.abs(rotated.yaw - pinched.yaw) < .03); assert.equal(rotated.distance, pinched.distance);
  await touch("touchEnd", []);
  await touch("touchStart", [[1, -25, 0], [2, 25, 0]]);
  await touch("touchCancel", []);
  const canceled = await state();
  // A new drag after cancellation must not inherit a stuck pinch.
  await touch("touchStart", [[3, 0, 0]]); await touch("touchMove", [[3, 8, 0]]); await touch("touchEnd", []);
  await page.waitForFunction(yaw => window.nesVRDebug().orbit.yaw !== yaw, canceled.yaw);
  assert.equal((await state()).distance, canceled.distance);
  await page.locator("#orbit-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "depth" && !window.nesVRDebug().transitioning);
  const fixed = await page.evaluate(() => window.nesVRDebug().cameraPosition);
  await touch("touchStart", [[1, -25, 0], [2, 25, 0]]); await touch("touchMove", [[1, -60, 0], [2, 60, 0]]); await touch("touchEnd", []);
  assert.deepEqual(await page.evaluate(() => window.nesVRDebug().cameraPosition), fixed, "layered framing stays fixed");
  assert.deepEqual(errors, []);
  console.log("Orbit touch: real two-finger pinch in/out, rotation-free zoom, single-finger handoff, cancellation, no page zoom and fixed layered framing passed");
} finally { await browser.close(); }
