// Exact real-scene invalidation with the original built-in sample and test ROM.
// Serve this checkout's web/; uses an isolated loopback-only Chromium context.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fixtureRom } from "./browser-fixture.mjs";
import { base, launch } from "./player-test-tools.mjs";

const original = await readFile(new URL("./vr.js", import.meta.url), "utf8");
const anchor = "    return {\n      refresh, recenter, exitVR,";
assert.equal(original.split(anchor).length, 2);
const post = "      mesher.postMessage({ epoch: meshEpoch, pixels, thickness, force: forceMesh }, [pixels.buffer]);";
assert.equal(original.split(post).length, 2);
const testSource = original
  .replace("    const vertexCounts = [];", "    const vertexCounts = [];\n    let detached = false;")
  .replace(post, post + "\n      detached ||= pixels.byteLength === 0;")
  .replace(anchor, `    let shadowUpdates = 0;
    const shadowRender = renderer.shadowMap.render.bind(renderer.shadowMap);
    renderer.shadowMap.render = (...args) => {
      if (renderer.shadowMap.enabled && (renderer.shadowMap.autoUpdate || renderer.shadowMap.needsUpdate)) shadowUpdates++;
      return shadowRender(...args);
    };
    const point = (layer, pixel) => BYTES * layer + pixel * 4;
    const set = (layer, pixel, rgba) => { source.set(rgba, point(layer, pixel)); geometryDirty = true; };
    window.nesInvalidationTest = {
      mutate: set,
      read: () => ({shadowUpdates, framesRendered, sceneFrames, meshEpoch, meshBusy, geometryDirty, detached,
        textureVersions: layers.map(layer => layer.texture.version),
        shadowsDirty, meshSpare: meshSpare?.byteLength, vertexCounts: [...vertexCounts], keyLightFar: key.shadow.camera.far,
        cameraX: camera.position.x}),
      pixel: (layer, pixel) => Array.from(layers[layer].pixels.subarray(pixel * 4, pixel * 4 + 4)),
      stale: (pixel, intermediate, final) => {
        set(3, pixel, intermediate);
        updateStage();
        set(3, pixel, final);
        meshEpoch++; forceMesh = true;
      }
    };\n` + anchor);

const browser = await launch();
try {
  const page = await browser.newPage({ viewport: {width: 800, height: 600}, deviceScaleFactor: 1 });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const origin = new URL(base).origin;
  await page.route("**/*", route => {
    const url = new URL(route.request().url());
    if (url.origin !== origin) return route.abort();
    if (url.pathname === "/vr.js") return route.fulfill({contentType: "text/javascript", body: testSource});
    return route.continue();
  });
  await page.goto(base + "?view=orbit&debug=1");
  const read = () => page.evaluate(() => window.nesInvalidationTest.read());
  const quiet = () => page.waitForFunction(() => {
    const s = window.nesInvalidationTest?.read();
    return s?.framesRendered > 0 && s.sceneFrames > 0 && s.meshSpare > 0
      && !s.meshBusy && !s.geometryDirty && !s.shadowsDirty;
  });
  const mutate = (...args) => page.evaluate(args => window.nesInvalidationTest.mutate(...args), args);
  const pixel = (layer, i) => page.evaluate(([layer, i]) => window.nesInvalidationTest.pixel(layer, i), [layer, i]);
  const completed = number => page.waitForFunction(number => window.nesInvalidationTest.read().sceneFrames > number, number);
  const drew = number => page.waitForFunction(number => window.nesInvalidationTest.read().framesRendered > number, number);
  const shadowed = number => page.waitForFunction(number => window.nesInvalidationTest.read().shadowUpdates > number, number);
  const corner = 9*256+9;

  await quiet();
  const first = await read();
  const oldBackdrop = await pixel(0, corner);
  await mutate(0, corner, oldBackdrop); // forced worker comparison; no change
  await completed(first.sceneFrames);
  await quiet();
  let now = await read();
  assert.equal(now.framesRendered, first.framesRendered, "unchanged reply must not schedule a desktop draw");
  assert.equal(now.shadowUpdates, first.shadowUpdates);
  assert.equal(now.meshSpare, 256*240*4*4);
  assert.equal(now.detached, true, "actual worker transfer detached the outgoing buffer");

  const newColor = [(oldBackdrop[0] + 127) % 256, 233, 7, 255];
  await mutate(0, corner, newColor);
  await drew(now.framesRendered);
  await quiet();
  assert.deepEqual(await pixel(0, corner),newColor);
  let next = await read();
  assert.equal(next.shadowUpdates, now.shadowUpdates, "receiving-only backdrop has no shadow caster");
  now = next;

  await mutate(3, corner, [187, 218, 96, 255]);
  await shadowed(now.shadowUpdates);
  await quiet();
  now = await read();
  await page.evaluate(i => {
    window.nesInvalidationTest.mutate(3, i, [0,0,0,0]);
    window.nesInvalidationTest.mutate(3, i + 1, [187,218,96,255]);
  }, corner);
  await shadowed(now.shadowUpdates);
  await quiet();
  assert.deepEqual(await pixel(3, corner), [0,0,0,0]);
  assert.deepEqual(await pixel(3, corner + 1), [187,218,96,255]);
  now = await read();
  await page.evaluate(i => window.nesInvalidationTest.stale(i, [255,11,41,255], [37,89,75,255]), corner+1);
  await shadowed(now.shadowUpdates);
  await quiet();
  assert.deepEqual(await pixel(3, corner + 1), [37,89,75,255], "old epoch's reply must never win");
  now = await read();

  const rect = await page.locator("#stage").boundingBox();
  await page.mouse.move(rect.x + rect.width * .6, rect.y + rect.height * .5);
  await page.mouse.down();
  await page.mouse.move(rect.x + rect.width * .72, rect.y + rect.height * .52);
  await page.mouse.up();
  await drew(now.framesRendered);
  await quiet();
  next = await read();
  assert.notEqual(next.cameraX, now.cameraX);
  assert.equal(next.shadowUpdates, now.shadowUpdates, "orbit camera update keeps world lighting cached");
  now = next;

  // The real size control changes the caster and light's frustum.
  await page.locator("#size").evaluate(input => { input.value = "95"; input.dispatchEvent(new Event("input", {bubbles: true})); });
  await shadowed(now.shadowUpdates);
  await quiet();
  assert.notEqual((await read()).keyLightFar, now.keyLightFar);

  // An actual ROM and rewind through the existing app should keep processing
  // frames even when the backdrop and gameplay pixels are static.
  await page.locator("#rom-input").setInputFiles({ name:"original-test.nes", mimeType:"application/octet-stream", buffer: fixtureRom() });
  await page.waitForFunction(() => window.nesVRDebug?.().playing && BigInt(window.nesVRDebug().frame ?? 0) > 24n);
  const before = await page.evaluate(() => ({frame:window.nesVRDebug().frame, shadow:window.nesInvalidationTest.read().shadowUpdates}));
  await page.waitForFunction(frame => BigInt(window.nesVRDebug().frame) >= BigInt(frame) + 9n, before.frame);
  const after = await page.evaluate(() => ({frame:window.nesVRDebug().frame, shadow:window.nesInvalidationTest.read().shadowUpdates}));
  assert.equal(after.shadow, before.shadow, "synthetic ROM's unchanged caster frames leave light cache alone");
  await page.locator("#stage").focus();
  await page.keyboard.down("r");
  await page.waitForFunction(frame => window.nesVRDebug().rewinding && BigInt(window.nesVRDebug().frame) < BigInt(frame), after.frame);
  await page.keyboard.up("r");
  await page.waitForFunction(() => !window.nesVRDebug().rewinding);
  await page.locator("#rom-input").setInputFiles({ name:"original-moving-sprite.nes", mimeType:"application/octet-stream", buffer: fixtureRom({ animateSprite: true }) });
  await page.waitForFunction(() => window.nesVRDebug?.().playing && BigInt(window.nesVRDebug().frame ?? 0) > 24n);
  const moving = await page.evaluate(() => ({frame:window.nesVRDebug().frame, ...window.nesInvalidationTest.read()}));
  await page.waitForFunction(base => {
    const s = window.nesInvalidationTest.read();
    return BigInt(window.nesVRDebug().frame) >= BigInt(base.frame) + 9n && s.shadowUpdates > base.shadowUpdates
      && s.textureVersions.slice(1).some((value, i) => value > base.textureVersions[i+1]);
  }, moving);
  const movingFrame = await page.evaluate(() => ({frame:window.nesVRDebug().frame, ...window.nesInvalidationTest.read()}));
  await page.locator("#stage").focus();
  await page.keyboard.down("r");
  await page.waitForFunction(base => {
    const s = window.nesInvalidationTest.read();
    return window.nesVRDebug().rewinding && BigInt(window.nesVRDebug().frame) < BigInt(base.frame)
      && s.shadowUpdates > base.shadowUpdates;
  }, movingFrame);
  await page.keyboard.up("r");
  assert.deepEqual(errors, []);
  console.log("Detached/reused transfer, stable/backdrop replies, moving casters, stale epochs, orbit, light volume and static/moving ROM rewind passed.");
} finally { await browser.close(); }
