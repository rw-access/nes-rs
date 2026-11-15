import assert from "node:assert/strict";
import { launch, base, range } from "./player-test-tools.mjs";
import { fixtureRom } from "./browser-fixture.mjs";
const browser = await launch();
try {
  for (const debug of [false, true]) {
    const page = await browser.newPage(), posts = [];
    page.on("request", req => { if (req.method() === "POST") posts.push(req.url()); });
    await page.goto(base + "?view=flat" + (debug ? "&debug=1" : ""));
    await page.locator("#rom-input").setInputFiles({ name: "palette.nes", mimeType: "application/octet-stream", buffer: fixtureRom() });
    await page.waitForFunction(() => document.querySelector(".app-shell").classList.contains("has-rom"));
    await page.waitForTimeout(500);
    await page.locator("#more-button").click(); await page.locator("#play").click();
    const pixels = () => page.locator("#screen").evaluate(c => Array.from(c.getContext("2d").getImageData(0, 0, 256, 240).data));
    const before = await pixels();
    await page.locator("#effects-settings-button").click();
    await page.locator("#material-finish-input").selectOption("satin");
    await range(page, "gap", 40); await range(page, "thickness", 30);
    await page.locator("#close-settings-button").click();
    assert.deepEqual(await pixels(), before, "flat pixels cannot be modified by 3D settings");
    if (debug) {
      await page.locator("#more-button").click(); await page.locator("#record-button").click();
      await page.locator("#play").click(); await page.waitForTimeout(150);
      assert.equal(await page.locator("#more-menu").isVisible(), false, "resuming play closes More");
      await page.locator("#more-button").click(); await page.locator("#play").click();
      const download = page.waitForEvent("download"); await page.locator("#stop-record-button").click(); await download;
      assert.ok(await page.evaluate(() => window.lastRecordingPayload.frames.length > 0));
    } else assert.equal(await page.locator("#record-button").isVisible(), false);
    assert.deepEqual(posts, [], "normal and debug replay recording remain local");
    await page.close();
  }
  console.log("Exact flat palette after material/geometry changes and local-only developer replay download passed");
} finally { await browser.close(); }
