import assert from "node:assert/strict";
import { launch, base } from "./player-test-tools.mjs";
import { fixtureRom } from "./browser-fixture.mjs";
const browser = await launch();
try {
  for (const [width, height] of [[360, 800], [390, 844], [844, 390], [1024, 768], [1920, 1080], [3840, 2160]]) {
    const context = await browser.newContext({ viewport: { width, height }, hasTouch: true, isMobile: width < 900 });
    const page = await context.newPage(), errors = [];
    page.on("pageerror", e => errors.push(e.message));
    await page.goto(base + "?view=flat&debug=1");
    await page.locator("#rom-input").setInputFiles({ name: "controls.nes", mimeType: "application/octet-stream", buffer: fixtureRom() });
    await page.waitForFunction(() => window.nesPlayerDebug()?.emulatorId === 1);
    await page.locator("#effects-settings-button").click();
    await page.locator("#touch-layout-input").selectOption("show"); await page.locator("#close-settings-button").click();
    await page.waitForFunction(() => document.activeElement.id === "screen");
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.ok(await page.locator(".view-controls #motion-button").isVisible(), "Tilt stays with the other 3D modes");
    const toolbar = await page.locator(".toolbar").boundingBox();
    assert.ok(toolbar.x >= -1 && toolbar.x + toolbar.width <= width + 1, "toolbar fits the viewport");
    const stage = await page.locator(".game-panel").boundingBox(), controls = await page.locator(".touch-controls").boundingBox();
    assert.ok(stage.width > 100 && stage.height > 100);
    if (height > width) assert.ok(controls.y >= stage.y + stage.height - 2, "portrait controls below play area");
    const button = page.locator('[data-button="a"]'), box = await button.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); await page.mouse.down();
    await page.waitForFunction(() => window.nesPlayerDebug().buttons === 1);
    assert.equal(await button.evaluate(el => el.dispatchEvent(new Event("contextmenu", { bubbles: true, cancelable: true }))), false);
    await page.mouse.up(); await page.waitForFunction(() => window.nesPlayerDebug().buttons === 0);
    await page.keyboard.down("-"); await page.waitForFunction(() => window.nesPlayerDebug().buttons === 4); await page.keyboard.up("-");
    await page.keyboard.down("="); await page.waitForFunction(() => window.nesPlayerDebug().buttons === 8); await page.keyboard.up("=");
    await page.screenshot({ path: `/tmp/nes-shared-check/viewport-${width}x${height}.png` });
    assert.deepEqual(errors, []); await context.close();
  }
  console.log("Shared player: six viewport sizes, portrait/landscape touch, long-hold context suppression and minus/plus controls passed");
} finally { await browser.close(); }
