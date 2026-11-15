import assert from "node:assert/strict";
import { launch, base } from "./player-test-tools.mjs";
const browser = await launch();
try {
  const page = await browser.newPage(), errors = [];
  await page.addInitScript(() => {
    window.motionPermissionCalls = 0;
    window.DeviceOrientationEvent = class extends Event {
      constructor(type, data) { super(type); Object.assign(this, data); }
      static requestPermission() { window.motionPermissionCalls++; return Promise.resolve("granted"); }
    };
  });
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(base + "?view=depth&debug=1");
  await page.waitForFunction(() => window.nesVRDebug?.().framesRendered > 0 && !window.nesVRDebug().meshBusy);
  const before = await page.evaluate(() => window.nesVRDebug().cameraPosition);
  const started = await page.evaluate(async () => {
    document.querySelector("#orbit-button").click();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    return window.nesVRDebug();
  });
  assert.equal(started.mode, "orbit"); assert.equal(started.transitioning, true);
  assert.ok(started.cameraPosition.every((value, i) => Math.abs(value - before[i]) < 1e-9), "orbit starts at the exact existing camera pose");
  await page.waitForFunction(() => !window.nesVRDebug().transitioning);
  const settled = await page.evaluate(() => window.nesVRDebug());
  assert.ok(Math.abs(settled.cameraPosition[0]) > .05, "orbit eases into a useful perspective angle");
  assert.ok(settled.framesRendered > started.framesRendered);
  await page.locator("#orbit-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "depth" && !window.nesVRDebug().transitioning);
  const restored = await page.evaluate(() => window.nesVRDebug().cameraPosition);
  assert.ok(restored.every((value, i) => Math.abs(value - before[i]) < 1e-9), "layered view returns to its exact fitted camera");
  const faded = await page.evaluate(async () => {
    document.querySelector("#effects-button").click();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    return { mode: window.nesPlayerDebug().mode, visible: !document.querySelector(".view-transition").hidden };
  });
  assert.equal(faded.mode, "flat"); assert.equal(faded.visible, true, "GPU to flat crossfades");
  await page.waitForFunction(() => document.querySelector(".view-transition").hidden);
  const fromFlat = await page.evaluate(async () => {
    document.querySelector("#orbit-button").click();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    return { mode: window.nesPlayerDebug().mode, visible: !document.querySelector(".view-transition").hidden };
  });
  assert.equal(fromFlat.mode, "orbit"); assert.equal(fromFlat.visible, true, "flat can crossfade directly to Orbit");
  await page.waitForFunction(() => !window.nesVRDebug().transitioning && document.querySelector(".view-transition").hidden);
  await page.locator("#motion-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "depth" && document.querySelector("#motion-button").getAttribute("aria-pressed") === "true");
  assert.equal(await page.evaluate(() => window.motionPermissionCalls), 1);
  await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent("deviceorientation", { alpha: 0, beta: 45, gamma: 0 })));
  await page.waitForTimeout(50);
  await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent("deviceorientation", { alpha: 0, beta: 60, gamma: 12 })));
  await page.waitForFunction(() => !window.nesVRDebug().transitioning && Math.abs(window.nesVRDebug().cameraPosition[0]) > .005);
  const tiltOff = await page.evaluate(() => { document.querySelector("#motion-button").click(); return window.nesVRDebug().transitioning; });
  assert.equal(tiltOff, true, "turning Tilt off eases back to neutral");
  await page.waitForFunction(() => !window.nesVRDebug().transitioning);
  assert.ok((await page.evaluate(() => window.nesVRDebug().cameraPosition)).slice(0, 2).every(v => Math.abs(v) < 1e-9));
  await page.locator("#motion-button").click();
  await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent("deviceorientation", { alpha: 0, beta: 45, gamma: 0 })));
  await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent("deviceorientation", { alpha: 0, beta: 60, gamma: 12 })));
  await page.waitForFunction(() => Math.abs(window.nesVRDebug().cameraPosition[0]) > .005);
  await page.locator("#orbit-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "orbit" && !window.nesVRDebug().transitioning);
  await page.locator("#motion-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "depth");
  // Recalibrate once during the return tween, then leave the device still.
  await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent("deviceorientation", { alpha: 0, beta: 60, gamma: 12 })));
  await page.waitForFunction(() => !window.nesVRDebug().transitioning);
  assert.ok((await page.evaluate(() => window.nesVRDebug().cameraPosition)).slice(0, 2).every(v => Math.abs(v) < 1e-9), "stationary recalibration retargets the pending camera transition");
  await page.locator("#motion-button").click();
  await page.emulateMedia({ reducedMotion: "reduce" });
  const reduced = await page.evaluate(async () => {
    document.querySelector("#orbit-button").click();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    return window.nesVRDebug();
  });
  assert.equal(reduced.mode, "orbit"); assert.equal(reduced.transitioning, false, "reduced motion skips the animation");
  assert.deepEqual(errors, []);
  console.log("Camera transitions: exact pose, eased orbit/layered, flat crossfades, direct Tilt activation/neutral return and reduced motion passed");
} finally { await browser.close(); }
