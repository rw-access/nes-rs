// Optional integration check. Requires built web/pkg and Playwright + Chromium.
// PLAYWRIGHT_MODULE may point to a preinstalled Playwright package's index.mjs.
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.NES_PREVIEW_URL || "http://127.0.0.1:8765/";
const output = process.env.NES_SCREENSHOT_DIR || "/tmp/nes-browser-check";
await mkdir(output, { recursive: true });

// Original synthetic NROM scene; no commercial ROM or user data is needed.
function fixtureRom() {
  const prg = Buffer.alloc(16384, 0xea), chr = Buffer.alloc(8192);
  const code = [], labels = {}, fixups = [];
  const emit = (...bytes) => code.push(...bytes);
  const label = name => labels[name] = 0x8000 + code.length;
  const address = (opcode, name) => { emit(opcode,0,0); fixups.push([code.length-2,name,false]); };
  const branch = (opcode, name) => { emit(opcode,0); fixups.push([code.length-1,name,true]); };
  emit(0x78,0xd8,0xa2,0xff,0x9a,0xe8,0x8e,0,0x20,0x8e,1,0x20);
  label("wait"); emit(0x2c,2,0x20); branch(0x10,"wait");
  emit(0xa9,0x3f,0x8d,6,0x20,0xa9,0,0x8d,6,0x20,0xa2,0);
  label("paletteLoop"); address(0xbd,"palette"); emit(0x8d,7,0x20,0xe8,0xe0,32); branch(0xd0,"paletteLoop");
  emit(0xa9,0x20,0x8d,6,0x20,0xa9,0,0x8d,6,0x20,0xa2,0);
  label("tilesLoop");
  for (const name of ["tiles0","tiles1","tiles2","tiles3"]) { address(0xbd,name); emit(0x8d,7,0x20); }
  // Four interleaved pages are fine for a synthetic scene; all bytes are valid tiles.
  emit(0xe8); branch(0xd0,"tilesLoop");
  emit(0xa9,0,0x8d,5,0x20,0x8d,5,0x20,0xa9,0x0a,0x8d,1,0x20);
  label("forever"); address(0x4c,"forever");
  label("palette"); emit(...Array.from({length:32},(_,i)=>[0x21,0x19,0x29,0x30][i%4]));
  for (let n=0;n<4;n++) { label(`tiles${n}`); emit(...Array.from({length:256},(_,i)=> (i+n*256)>832 ? 2 : (i%32<4 || i%32>27) ? 1 : 0)); }
  for (const [offset,name,relative] of fixups) {
    if (relative) code[offset]=(labels[name]-(0x8000+offset+1))&255;
    else {code[offset]=labels[name]&255;code[offset+1]=labels[name]>>8;}
  }
  prg.set(code); for (const offset of [0x3ffa,0x3ffc,0x3ffe]) {prg[offset]=0;prg[offset+1]=0x80;}
  for(let y=0;y<8;y++){chr[16+y]=0xff;chr[24+y]=0;chr[32+y]=y===0?0xff:0x81;chr[40+y]=0xff;}
  return Buffer.concat([Buffer.from([0x4e,0x45,0x53,0x1a,1,1,0,0,0,0,0,0,0,0,0,0]),prg,chr]);
}
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || "/usr/bin/chromium", headless: true, args: ["--no-sandbox"] });
try {
  for (const [name,width,height,mobile] of [["phone-portrait",390,844,true],["phone-landscape",844,390,true],["small-phone",320,568,true],["tablet",1024,768,true],["desktop",1440,900,false],["tv",3840,2160,false]]) {
    if (process.env.NES_TEST_LAYOUT && name !== process.env.NES_TEST_LAYOUT) continue;
    const context = await browser.newContext({viewport:{width,height},isMobile:mobile,hasTouch:mobile,deviceScaleFactor:mobile?3:1});
    const page = await context.newPage(); const errors=[];
    page.on("pageerror",error=>errors.push(error.message));
    // Observe real bridge calls, retaining the actual WASM implementation.
    await page.route("**/pkg/nes_wasm.js", async route => {
      const response = await route.fetch();
      const body=(await response.text()).replace(/set_controller\((\w+)\) \{/, 'set_controller($1) { window.__testButtons = $1;');
      await route.fulfill({response,body});
    });
    await page.addInitScript(() => {
      window.__testPads = [];
      Object.defineProperty(navigator, "getGamepads", { value: () => window.__testPads });
    });
    await page.goto(`${base}?debug=1`);
    await page.locator("#welcome-load-button").waitFor();
    await page.screenshot({path:`${output}/${name}-welcome.png`});
    await page.locator("#rom-input").setInputFiles({name:"synthetic.nes",mimeType:"application/octet-stream",buffer:fixtureRom()});
    await page.waitForFunction(()=>document.querySelector(".app-shell").classList.contains("has-rom"));
    await page.waitForFunction(()=>document.querySelector("#screen-stage").dataset.depth==="true");
    await page.waitForTimeout(200);
    const layout = await page.evaluate(()=>{
      const rect=selector=>{const r=document.querySelector(selector).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};};
      return {stage:rect("#screen-stage"),panel:rect(".game-panel"),dpad:rect(".dpad-hitbox"),face:rect(".face-hitbox"),touch:document.querySelector(".app-shell").dataset.touch,scroll:document.documentElement.scrollHeight,quality:window.nesRenderStats};
    });
    assert.equal(layout.touch,String(mobile));
    const {stage,panel}=layout;
    assert.ok(stage.width>0 && stage.height>0 && stage.width<=panel.width+1 && stage.height<=panel.height+1,JSON.stringify(layout));
    assert.ok(Math.abs(stage.width/stage.height-256/240)<.01);
    assert.ok(stage.y>=0 && stage.y+stage.height<=height+1);
    assert.equal(layout.scroll,height);
    if (mobile && width>height) {
      assert.ok(layout.dpad.x+layout.dpad.width<=stage.x+1);
      assert.ok(layout.face.x>=stage.x+stage.width-1);
    }
    if (mobile) {
      const cdp=await context.newCDPSession(page);
      const a=await page.locator('[data-button="a"]').boundingBox();
      const d=layout.dpad;
      const point=(id,x,y)=>({id,x,y,radiusX:2,radiusY:2});
      const right=point(1,d.x+d.width*.85,d.y+d.height*.5), action=point(2,a.x+a.width/2,a.y+a.height/2);
      await cdp.send("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[right,action]});
      await page.waitForTimeout(700);
      assert.equal(await page.evaluate(()=>window.__testButtons),129,"hold right + A");
      await cdp.send("Input.dispatchTouchEvent",{type:"touchMove",touchPoints:[point(1,d.x+d.width*.85,d.y+d.height*.15),action]});
      assert.equal(await page.evaluate(()=>window.__testButtons),145,"slide into diagonal + A");
      await cdp.send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[point(1,d.x+d.width*.85,d.y+d.height*.15)]});
      assert.equal(await page.evaluate(()=>window.__testButtons),1,"release direction, keep A");
      await cdp.send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});
      assert.equal(await page.evaluate(()=>window.__testButtons),0);
      assert.equal(await page.evaluate(()=>String(getSelection())),"");
      assert.equal(await page.locator('[data-button="a"]').evaluate(el=>el.dispatchEvent(new MouseEvent("contextmenu",{bubbles:true,cancelable:true}))),false);
    }
    if (name === "desktop") {
      await page.keyboard.down("z"); await page.keyboard.down("j");
      await page.keyboard.up("z");
      assert.equal(await page.evaluate(()=>window.__testButtons),1,"alias key still holds A");
      await page.keyboard.up("j");
      await page.evaluate(() => { window.__testPads = [{connected:true,mapping:"standard",axes:[.8,0],buttons:Array.from({length:16},(_,i)=>({pressed:i===0}))}]; });
      await page.waitForFunction(()=>window.__testButtons===129);
      await page.evaluate(()=>{window.__testPads=[];});
      await page.waitForFunction(()=>window.__testButtons===0);
    }
    await page.locator("#effects-settings-button").click();
    assert.equal(await page.locator("#effects-settings").evaluate(el=>el.open),true);
    await page.locator("#close-settings-button").focus();
    await page.keyboard.press("r");
    assert.equal(await page.locator("#rewind-button").evaluate(el=>el.classList.contains("is-held")),false,"settings blocks rewind shortcut");
    await page.locator("#render-quality-input").selectOption("performance");
    await page.waitForTimeout(100);
    await page.waitForFunction(()=>[...document.querySelectorAll("#depth-stack canvas")].every(node=>node.width<=512));
    await page.locator("#touch-layout-input").selectOption(mobile?"hide":"show");
    await page.keyboard.press("Escape");
    await page.waitForTimeout(100);
    assert.equal(await page.locator(".app-shell").getAttribute("data-touch"),String(!mobile));
    await page.locator("#effects-settings-button").click();
    await page.locator("#touch-layout-input").selectOption("auto");
    await page.locator("#render-quality-input").selectOption("balanced");
    await page.locator("#close-settings-button").click();
    await page.screenshot({path:`${output}/${name}-play.png`});
    await page.locator("#more-button").click();
    await page.locator("#snapshot-button").click();
    assert.equal(await page.locator("#restore-button").isEnabled(),true);
    await page.locator("#restore-button").click();
    await page.keyboard.press("Escape");
    await page.keyboard.press("f");
    await page.waitForFunction(()=>document.querySelector(".app-shell").classList.contains("is-immersive"));
    await page.evaluate(()=>document.fullscreenElement && document.exitFullscreen());
    assert.deepEqual(errors,[]);
    console.log(name,JSON.stringify(layout));
    await context.close();
  }
} finally { await browser.close(); }
