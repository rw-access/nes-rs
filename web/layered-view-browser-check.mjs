import assert from "node:assert/strict";
import { launch, base, range } from "./player-test-tools.mjs";
const browser = await launch();
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 700 } }), errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(base + "?debug=1&view=depth");
  await page.waitForFunction(() => window.nesVRDebug?.().framesRendered > 0 && !window.nesVRDebug().meshBusy);
  for (const [width, height] of [[1280, 700], [390, 844]]) {
    await page.setViewportSize({ width, height });
    await page.waitForTimeout(150);
    const framing = await page.evaluate(() => window.nesVRDebug());
    assert.ok(framing.cameraPosition.slice(0, 2).every(value => Math.abs(value) < 1e-10), "layered camera is head-on");
    assert.equal(framing.decorationsVisible, false, "layered game has no tabletop or box border");
    const [a, b] = framing.gameCorners;
    assert.ok(Math.abs(Math.max(Math.abs(a[0]), Math.abs(a[1])) - 1) < .00001, "game fits maximum available width or height");
    assert.ok(Math.abs(a[0]) <= 1.00001 && Math.abs(a[1]) <= 1.00001 && b[0] <= 1.00001 && b[1] <= 1.00001, "native game stays uncropped");
    await page.screenshot({ path: `/tmp/nes-shared-check/layered-fit-${width}.png` });
    // Sample the actual GPU framebuffer in the same callback as its next draw.
    const colors = await page.evaluate(() => new Promise(resolve => {
      const canvas = document.querySelector("#stage"), gl = canvas.getContext("webgl2");
      const draw = gl.drawElements.bind(gl);
      let scheduled = false;
      gl.drawElements = (...args) => {
        draw(...args);
        if (scheduled) return; scheduled = true;
        queueMicrotask(() => {
          gl.drawElements = draw;
          const pixel = (x, y) => { const out = new Uint8Array(4); gl.readPixels(Math.round(x), Math.round(y), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, out); return [...out]; };
          // At the wide viewport the side extension meets clear sky; on a
          // portrait viewport the top extension meets the same backdrop.
          const wide = canvas.width > canvas.height;
          resolve(wide ? [pixel(4, canvas.height * .78), pixel(canvas.width / 2, canvas.height * .78), pixel(canvas.width - 5, canvas.height * .78)]
            : [pixel(canvas.width / 2, canvas.height - 5), pixel(canvas.width / 2, canvas.height * .6)]);
        });
      };
      window.dispatchEvent(new Event("resize"));
    }));
    if (width > height) {
      for (const pixel of colors) assert.ok(pixel[3] === 255 && pixel.slice(0, 3).some(v => v > 60), "background extension is opaque game color");
      for (const pixel of colors.slice(1)) assert.ok(pixel.every((v, i) => Math.abs(v - colors[0][i]) <= 1), "extension uses the same lit backdrop without a color seam");
    } else {
      assert.ok(colors[0][3] === 255 && colors[0].slice(0, 3).every(v => v < 60), "portrait keeps clear space above the game");
      assert.ok(colors[1].slice(0, 3).some(v => v > 60), "native game backdrop remains lit");
    }
  }
  await page.locator("#effects-settings-button").click();
  await range(page, "size", 500); await range(page, "gap", 40);
  await page.locator("#close-settings-button").click();
  await page.waitForFunction(() => !window.nesVRDebug().meshBusy);
  const large = await page.evaluate(() => window.nesVRDebug());
  assert.ok(Math.abs(Math.max(...large.gameCorners[1].slice(0, 2)) - 1) < .00001);
  await page.locator("#orbit-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "orbit" && window.nesVRDebug().decorationsVisible);
  assert.deepEqual((await page.evaluate(() => window.nesVRDebug())).backdropBounds, [-.5, .46875]);
  await page.locator("#orbit-button").click();
  await page.waitForFunction(() => window.nesVRDebug().mode === "depth" && !window.nesVRDebug().decorationsVisible);
  assert.deepEqual(errors, []);
  console.log("Layered view: maximum uncropped head-on framing, seamless lit background extension, portrait/wide/room-size bounds and orbit restoration passed");
} finally { await browser.close(); }
