// Exercise the real renderer's WebXR lifecycle using a deterministic fake device.
// This validates integration, not physical headset compatibility or frame timing.
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { fixtureRom } from "./browser-fixture.mjs";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.NES_PREVIEW_URL || "http://127.0.0.1:8765/";
const output = process.env.NES_SCREENSHOT_DIR || "/tmp/nes-vr-xr-check";
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || "/usr/bin/chromium", headless: true,
  args: ["--no-sandbox", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });

function fakeDevice({ failReferenceSpace = false, failCompatibility = false, deferRequest = false, deferCompatibility = false } = {}) {
  const raf = window.requestAnimationFrame.bind(window), cancel = window.cancelAnimationFrame.bind(window);
  const head = { x: 0, y: 0, z: 0 }, identity = (x, y, z) => new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1]);
  const buttons = () => Array.from({ length: 6 }, () => ({ pressed: false, value: 0 }));
  const sources = ["left", "right"].map(handedness => ({ handedness, gamepad: { mapping: "xr-standard", buttons: buttons(), axes: [0, 0, 0, 0] } }));
  WebGL2RenderingContext.prototype.makeXRCompatible = async () => { if (deferCompatibility && !window.fakeXR.contextGranted) await new Promise(resolve => { window.fakeXR.finishContext = () => { window.fakeXR.contextGranted = true; resolve(); }; }); if (failCompatibility) throw new Error("Synthetic XR context failure"); };
  // Expose the newer API while granting no projection layers: compatibility
  // mode must deliberately choose the classic base layer even on this browser.
  Object.defineProperty(window, "XRWebGLBinding", { configurable: true, value: class {
    createProjectionLayer() { throw new Error("Compatibility mode must use the classic base layer"); }
  } });
  Object.defineProperty(window, "XRWebGLLayer", { configurable: true, value: class {
    constructor() { this.framebufferWidth = 1024; this.framebufferHeight = 512; this.framebuffer = null; this.ignoreDepthValues = true; }
    getViewport(view) { return { x: view.eye === "left" ? 0 : 512, y: 0, width: 512, height: 512 }; }
  } });
  class Session extends EventTarget {
    constructor() {
      super(); this.inputSources = sources; this.visibilityState = "visible";
      this.environmentBlendMode = "opaque"; this.enabledFeatures = ["local"];
      this.renderState = { depthNear: 0.025, depthFar: 30 }; this.pending = new Set(); this.ended = false;
    }
    updateRenderState(value) { Object.assign(this.renderState, value); }
    async requestReferenceSpace() { if (failReferenceSpace) throw new Error("Synthetic reference-space failure"); return new EventTarget(); }
    requestAnimationFrame(callback) {
      const handle = raf(time => {
        this.pending.delete(handle); if (this.ended) return;
        const near = this.renderState.depthNear, far = this.renderState.depthFar;
        const projectionMatrix = new Float32Array([1.15, 0, 0, 0, 0, 1.15, 0, 0, 0, 0, -(far + near) / (far - near), -1, 0, 0, -2 * far * near / (far - near), 0]);
        const orientation = { x: 0, y: 0, z: 0, w: 1 };
        const pose = { transform: { position: { ...head }, orientation }, views: ["left", "right"].map(eye => ({ eye, projectionMatrix,
          transform: { matrix: identity(head.x + (eye === "left" ? -0.032 : 0.032), head.y, head.z) } })) };
        callback(time, { session: this, getViewerPose: () => pose });
      });
      this.pending.add(handle); return handle;
    }
    cancelAnimationFrame(handle) { cancel(handle); this.pending.delete(handle); }
    async end() {
      if (this.ended) return; this.ended = true;
      for (const handle of this.pending) cancel(handle); this.pending.clear(); this.dispatchEvent(new Event("end"));
    }
  }
  const xr = Object.assign(new EventTarget(), {
    isSessionSupported: async () => true,
    requestSession: async (mode, options) => {
      if (mode !== "immersive-vr" || options.optionalFeatures.includes("layers")) throw new Error("Compatibility mode should not request projection layers");
      if (deferRequest && !window.fakeXR.granted) await new Promise(resolve => { window.fakeXR.grant = () => { window.fakeXR.granted = true; resolve(); }; });
      window.fakeXR.session = new Session(); return window.fakeXR.session;
    },
  });
  window.fakeXR = { head, sources, session: null };
  Object.defineProperty(navigator, "xr", { configurable: true, value: xr });
}

try {
  const context = await browser.newContext(); await context.addInitScript(fakeDevice);
  const page = await context.newPage(), errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(base + "vr.html?debug=1");
  await page.waitForFunction(() => window.nesVRDebug?.().triangles > 100);
  await page.locator("#rom-input").setInputFiles({ name: "xr-test.nes", mimeType: "application/octet-stream", buffer: fixtureRom() });
  await page.waitForFunction(() => document.querySelector("#stage").dataset.source === "rom");
  await page.locator("#orbit-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "depth" && !window.nesVRDebug().decorationsVisible);
  // Enter through a real click, the same gesture path as a headset session.
  await page.getByRole("button", { name: "Enter VR", exact: true }).click();
  await page.waitForFunction(() => window.nesVRDebug().xr && window.nesVRDebug().eyes.length === 2 && window.nesVRDebug().placement[2] < -1);
  const first = await page.evaluate(() => window.nesVRDebug());
  assert.equal(first.fpsVisible, false, "VR FPS starts hidden too");
  await page.locator("#show-fps-input").evaluate(input => { input.checked = true; input.dispatchEvent(new Event("change", { bubbles: true })); });
  assert.equal((await page.evaluate(() => window.nesVRDebug())).fpsVisible, true);
  await page.locator("#show-fps-input").evaluate(input => { input.checked = false; input.dispatchEvent(new Event("change", { bubbles: true })); });
  assert.equal((await page.evaluate(() => window.nesVRDebug())).fpsVisible, false);
  assert.equal(first.decorationsVisible, true, "VR restores the tabletop presentation from layered mode");
  assert.deepEqual(first.backdropBounds, [-.5, .46875]);
  assert.ok(await page.getByRole("button", { name: "Exit VR", exact: true }).isVisible());
  assert.ok(Math.abs(first.eyes[1].position[0] - first.eyes[0].position[0] - 0.064) < 1e-5);
  await page.evaluate(() => { window.fakeXR.head.x = 0.2; });
  await page.waitForFunction(() => window.nesVRDebug().eyes[0].position[0] > 0.15);
  assert.deepEqual((await page.evaluate(() => window.nesVRDebug())).placement, first.placement, "head motion must not drag the stage");
  const originalSize = Number(await page.locator("#size").inputValue());
  await page.evaluate(() => { window.fakeXR.sources[1].gamepad.axes[3] = -0.9; });
  await page.waitForFunction(size => Number(document.querySelector("#size").value) >= size + 20, originalSize);
  await page.evaluate(() => { window.fakeXR.sources[1].gamepad.axes[3] = 0; });
  await page.waitForTimeout(150);
  const largerSize = Number(await page.locator("#size").inputValue());
  assert.deepEqual((await page.evaluate(() => window.nesVRDebug())).placement, first.placement, "resizing cannot move the room anchor");
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("nes.vr-stage.v1") || "null")?.size === document.querySelector("#size").value);
  assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem("nes.vr-stage.v1")).size), String(largerSize));
  // A tethered headset can stay visible while the HTML document is hidden.
  await page.evaluate(() => { Object.defineProperty(document, "hidden", { configurable: true, value: true }); });
  const hiddenDocumentFrame = (await page.evaluate(() => window.nesVRDebug())).framesRendered;
  await page.waitForFunction(count => window.nesVRDebug().framesRendered > count + 2, hiddenDocumentFrame);
  await page.evaluate(() => { delete document.hidden; window.fakeXR.session.visibilityState = "visible-blurred"; window.fakeXR.session.dispatchEvent(new Event("visibilitychange")); });
  const blurredFrame = (await page.evaluate(() => window.nesVRDebug())).framesRendered;
  await page.waitForFunction(count => window.nesVRDebug().framesRendered > count + 2, blurredFrame);
  await page.evaluate(() => { window.fakeXR.session.visibilityState = "visible"; window.fakeXR.session.dispatchEvent(new Event("visibilitychange")); });
  await page.evaluate(() => {
    window.fakeXR.sources[0].gamepad.axes[2] = -0.8;
    window.fakeXR.sources[1].gamepad.buttons[0].pressed = true;
  });
  await page.waitForFunction(() => window.nesVRDebug().buttons === (64 | 1));
  const rewindFrom = (await page.evaluate(() => window.nesVRDebug())).frame;
  await page.evaluate(() => { window.fakeXR.sources[0].gamepad.buttons[1].pressed = true; });
  await page.waitForFunction(frame => window.nesVRDebug().rewinding && BigInt(window.nesVRDebug().frame) < BigInt(frame) - 2n, rewindFrom);
  assert.deepEqual((await page.evaluate(() => window.nesVRDebug())).placement, first.placement, "rewind cannot move the stage");
  await page.evaluate(() => { window.fakeXR.sources[0].gamepad.buttons[1].pressed = false; });
  await page.waitForFunction(() => !window.nesVRDebug().rewinding);
  await page.evaluate(() => { window.fakeXR.session.visibilityState = "hidden"; window.fakeXR.session.dispatchEvent(new Event("visibilitychange")); });
  await page.waitForFunction(() => window.nesVRDebug().buttons === 0);
  const frozen = (await page.evaluate(() => window.nesVRDebug())).frame;
  await page.waitForTimeout(100); assert.equal((await page.evaluate(() => window.nesVRDebug())).frame, frozen);
  await page.evaluate(() => {
    window.fakeXR.sources[0].gamepad.axes[2] = 0; window.fakeXR.sources[1].gamepad.buttons[0].pressed = false;
    window.fakeXR.session.visibilityState = "visible"; window.fakeXR.session.dispatchEvent(new Event("visibilitychange"));
    for (const source of window.fakeXR.sources) source.gamepad.buttons[3].pressed = true;
  });
  await page.waitForFunction(() => window.nesVRDebug().placement[0] > 0.19);
  await page.screenshot({ path: output + "/stereo.png" });
  await page.evaluate(() => window.fakeXR.session.end());
  await page.waitForFunction(() => !window.nesVRDebug().xr && !document.body.classList.contains("in-vr"));
  assert.equal((await page.evaluate(() => window.nesVRDebug())).playing, true, "exiting VR continues the same running game");
  assert.deepEqual((await page.evaluate(() => window.nesVRDebug())).placement, [0, 0, 0]);
  assert.equal((await page.evaluate(() => window.nesVRDebug())).decorationsVisible, false, "returning to layered mode removes tabletop decorations");
  await page.getByRole("button", { name: "Enter VR", exact: true }).click();
  await page.waitForFunction(() => window.nesVRDebug().xr && window.nesVRDebug().eyes.length === 2);
  // A delayed session end must not let an older Flat request disable a newer
  // Orbit selection and strand the sole emulator scheduler.
  await page.evaluate(() => {
    const session = window.fakeXR.session, end = session.end.bind(session);
    session.end = () => new Promise(resolve => setTimeout(async () => { await end(); resolve(); }, 75));
    const select = document.querySelector("#view-mode");
    select.value = "flat"; select.dispatchEvent(new Event("change"));
    select.value = "orbit"; select.dispatchEvent(new Event("change"));
  });
  await page.waitForFunction(() => !window.nesVRDebug().xr && window.nesVRDebug().mode === "orbit" && window.nesPlayerDebug().mode === "orbit");
  const resumed = (await page.evaluate(() => window.nesPlayerDebug())).frame;
  await page.waitForFunction(frame => BigInt(window.nesPlayerDebug().frame) > BigInt(frame) + 2n, resumed);
  assert.deepEqual(errors, []);
  await context.close();

  const direct = await browser.newContext(); await direct.addInitScript(fakeDevice);
  const directPage = await direct.newPage(); await directPage.goto(base + "?view=flat&debug=1");
  await directPage.getByRole("button", { name: "Enter VR", exact: true }).click();
  await directPage.waitForFunction(() => window.nesVRDebug().xr && window.nesVRDebug().appXRFrames > 2);
  assert.equal(await directPage.evaluate(() => window.nesPlayerDebug().mode), "orbit", "2D can enter VR directly");
  await directPage.locator("#motion-button").click();
  await directPage.waitForFunction(() => !window.nesVRDebug().xr && window.nesPlayerDebug().mode === "depth");
  await direct.close();

  const stale = await browser.newContext(); await stale.addInitScript(fakeDevice, { deferRequest: true });
  const stalePage = await stale.newPage(), staleErrors = []; stalePage.on("pageerror", e => staleErrors.push(e.message));
  await stalePage.goto(base + "?view=orbit&debug=1");
  await stalePage.getByRole("button", { name: "Enter VR", exact: true }).click();
  await stalePage.waitForFunction(() => typeof window.fakeXR.grant === "function");
  await stalePage.locator("#effects-button").click();
  await stalePage.waitForFunction(() => window.nesPlayerDebug().mode === "flat");
  await stalePage.evaluate(() => window.fakeXR.grant());
  await stalePage.waitForFunction(() => window.fakeXR.session?.ended);
  assert.equal(await stalePage.evaluate(() => window.nesVRDebug().xr), false, "stale VR grant cannot start an inactive renderer");
  assert.deepEqual(staleErrors, []); await stale.close();

  const setupRace = await browser.newContext(); await setupRace.addInitScript(fakeDevice, { deferCompatibility: true });
  const setupPage = await setupRace.newPage(), setupErrors = []; setupPage.on("pageerror", e => setupErrors.push(e.message));
  await setupPage.goto(base + "?view=orbit&debug=1");
  await setupPage.getByRole("button", { name: "Enter VR", exact: true }).click();
  await setupPage.waitForFunction(() => typeof window.fakeXR.finishContext === "function");
  await setupPage.locator("#effects-button").click();
  await setupPage.evaluate(() => window.fakeXR.finishContext());
  await setupPage.waitForFunction(() => window.fakeXR.session.ended && window.nesPlayerDebug().mode === "flat" && !window.nesVRDebug().xr);
  assert.deepEqual(setupErrors, []); await setupRace.close();

  for (const mode of ["reference", "compatibility"]) {
  const failure = await browser.newContext(); await failure.addInitScript(fakeDevice, { failReferenceSpace: mode === "reference", failCompatibility: mode === "compatibility" });
  const failedPage = await failure.newPage(), failedErrors = [];
  failedPage.on("pageerror", error => failedErrors.push(error.message));
  await failedPage.goto(base + "vr.html?debug=1");
  await failedPage.getByRole("button", { name: "Enter VR", exact: true }).click();
  await failedPage.waitForFunction(() => document.querySelector("#diorama-status").textContent.includes("Could not enter VR"));
  assert.equal(await failedPage.evaluate(() => window.fakeXR.session.ended), true);
  assert.equal(await failedPage.locator("#enter-vr").isEnabled(), true);
  assert.ok(await failedPage.locator("#stage").evaluate(canvas => canvas.width > 0 && canvas.height > 0));
  assert.deepEqual(failedErrors, []);
  await failure.close();
  }
  console.log("Simulated WebXR: stereo views, tracked head motion, anchored stage, recenter, XR controls, visibility pause, exit/reentry and failed initialization cleanup passed");
} finally { await browser.close(); }
