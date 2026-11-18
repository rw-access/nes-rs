import init, * as wasm from "./pkg/nes_wasm.js";
import { SpatialAudio } from "./spatial-audio.js";
import { ControllerInput, readGamepads, bindTouchControls } from "./play-controls.js";
import { loadCachedRom, saveCachedRom, MAX_ROM_BYTES } from "./rom-store.js";
import { frameBudget } from "./render-quality.js";
import { DeviceTiltController } from "./device-tilt.js";

const $ = id => document.getElementById(id);
const params = new URLSearchParams(location.search), debugEnabled = params.get("debug") === "1";
const WIDTH = 256, HEIGHT = 240, FRAME_MS = 1000 / 60;
const BUTTONS = { a: 1, b: 2, select: 4, start: 8, up: 16, down: 32, left: 64, right: 128 };
const NesWeb = wasm.NesWasm ?? wasm.NesWeb;
const appShell = document.querySelector(".app-shell"), gamePanel = document.querySelector(".game-panel");
const screenWrap = document.querySelector(".screen-wrap"), canvas = $("screen");
const context = canvas.getContext("2d", { alpha: false }), image = context.createImageData(WIDTH, HEIGHT);
const effectsSettings = $("effects-settings"), effectsSettingsButton = $("effects-settings-button");
const rewindButton = $("rewind-button"), recordButton = $("record-button"), stopRecordButton = $("stop-record-button");
const recordingStatus = $("recording-status"), audioButton = $("audio-button"), status = $("status");
const touchLayoutInput = $("touch-layout-input"), romInput = $("rom-input");
const snapshotButton = $("snapshot-button"), restoreButton = $("restore-button");
let emulator = null, emulatorId = 0, romName = null, savedSnapshot = null, recording = null;
let mode = "flat", viewer = null, viewerPromise = null, modeGeneration = 0;
let paused = false, heldButtons = 0, rewinding = false, rewindFrames = 0, rewindStartedAt = 0;
let previousTime = 0, accumulator = 0, romLoadGeneration = 0, cacheWrites = Promise.resolve();
let gamepadConnected = false, statusTimeout, chromeTimeout, presetGeneration = 0;
let flatFrames = 0, gameFrames = 0, flatStatsAt = performance.now(), lastFlatFrames = 0, lastGameFrames = 0;
const rewindSources = new Set();
const transitionCanvas = document.createElement("canvas");
transitionCanvas.className = "view-transition"; transitionCanvas.hidden = true; transitionCanvas.setAttribute("aria-hidden", "true");
gamePanel.append(transitionCanvas);
let viewFade = null, xrPageFade = null, tiltEnabled = false;
const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
function cancelViewFade() {
  const animation = viewFade; viewFade = null; animation?.cancel();
  transitionCanvas.hidden = true; transitionCanvas.width = transitionCanvas.height = 1;
}
function captureOutgoingView() {
  if (reducedMotion() || !transitionCanvas.animate || viewer?.presenting) return false;
  const previousOpacity = transitionCanvas.hidden ? 0 : Number(getComputedStyle(transitionCanvas).opacity);
  const previous = document.createElement("canvas");
  if (previousOpacity > 0) { previous.width = transitionCanvas.width; previous.height = transitionCanvas.height; previous.getContext("2d").drawImage(transitionCanvas, 0, 0); }
  cancelViewFade();
  const ratio = Math.min(devicePixelRatio || 1, 1.25);
  transitionCanvas.width = Math.max(1, Math.round(gamePanel.clientWidth * ratio));
  transitionCanvas.height = Math.max(1, Math.round(gamePanel.clientHeight * ratio));
  const ctx = transitionCanvas.getContext("2d");
  ctx.fillStyle = "#111217"; ctx.fillRect(0, 0, transitionCanvas.width, transitionCanvas.height);
  if (mode === "flat") {
    const panel = gamePanel.getBoundingClientRect(), picture = canvas.getBoundingClientRect();
    ctx.imageSmoothingEnabled = false;
    if (emulator) ctx.drawImage(canvas, (picture.x - panel.x) * ratio, (picture.y - panel.y) * ratio, picture.width * ratio, picture.height * ratio);
  } else if (!viewer?.captureFrame(transitionCanvas)) return false;
  if (previousOpacity > 0) { ctx.globalAlpha = previousOpacity; ctx.drawImage(previous, 0, 0, transitionCanvas.width, transitionCanvas.height); ctx.globalAlpha = 1; }
  return true;
}
function fadeOutgoingView() {
  transitionCanvas.hidden = false;
  const animation = transitionCanvas.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 260, easing: "ease-out" });
  viewFade = animation;
  animation.finished.then(() => { if (viewFade === animation) cancelViewFade(); }).catch(() => {});
}
function updateMotionButton() {
  const selected = tiltEnabled && mode === "depth" && !viewer?.presenting;
  $("motion-button").setAttribute("aria-pressed", String(selected));
  const label = selected ? "Turn tilt off" : "Enable phone tilt";
  $("motion-button").setAttribute("aria-label", label); $("motion-button").title = label;
}

recordButton.hidden = stopRecordButton.hidden = recordingStatus.hidden = !debugEnabled;
const showFPSInput = $("show-fps-input"), FPS_STORAGE_KEY = "nes.show-fps.v1";
try { showFPSInput.checked = localStorage.getItem(FPS_STORAGE_KEY) === "true"; } catch (_) { /* Storage is optional. */ }
function updateFPSVisibility(persist = false) {
  $("fps").hidden = !showFPSInput.checked;
  viewer?.setFPSVisible(showFPSInput.checked);
  if (persist) { try { localStorage.setItem(FPS_STORAGE_KEY, String(showFPSInput.checked)); } catch (_) { /* Keep the choice for this visit. */ } }
}
showFPSInput.addEventListener("change", () => updateFPSVisibility(true));
updateFPSVisibility();


function reportDebug(kind, ...values) {
  if (!debugEnabled) return;
  const events = window.nesDebugEvents ??= [];
  events.push({ timestamp: new Date().toISOString(), kind, values: values.map(v => v instanceof Error ? { name: v.name, message: v.message } : v) });
  if (events.length > 200) events.shift();
}
window.addEventListener("error", event => reportDebug("window.error", event.message));
window.addEventListener("unhandledrejection", event => reportDebug("unhandledrejection", event.reason?.message));
function setStatus(message) {
  status.textContent = message; status.classList.add("is-visible");
  clearTimeout(statusTimeout); statusTimeout = setTimeout(() => status.classList.remove("is-visible"), 3500);
}
const audio = new SpatialAudio({
  onState: ({ state, muted }) => {
    const label = muted ? "Unmute audio" : state === "running" ? "Mute audio" : "Enable audio";
    audioButton.dataset.muted = String(muted || state !== "running");
    audioButton.title = label; audioButton.setAttribute("aria-label", label);
  },
  onError: error => setStatus(`Audio unavailable: ${error.message}`),
});
audio.setWidth(0);
function resetClock() { previousTime = 0; accumulator = 0; }
function returnToGame() {
  if (effectsSettings.open) return;
  if (!$("more-menu").hidden) setMoreOpen(false);
  const active = document.activeElement;
  if (active instanceof HTMLElement && active.closest(".toolbar, #more-menu")) active.blur();
  const surface = mode === "flat" ? canvas : $("stage");
  if (document.activeElement !== surface) surface.focus({ preventScroll: true });
}
gamePanel.addEventListener("pointerdown", returnToGame);
document.querySelector(".touch-controls").addEventListener("pointerdown", returnToGame);

function isPlaying() { return Boolean(emulator && !paused && !effectsSettings.open); }
function updateButtons() { if (emulator && !rewinding) emulator.set_controller(heldButtons); }
const controllerInput = new ControllerInput((buttons, previous) => {
  heldButtons = buttons;
  for (const [name, mask] of Object.entries(BUTTONS)) {
    if ((buttons & mask) !== (previous & mask)) recordInputEvent(buttons & mask ? "button-down" : "button-up", { button: name });
  }
  for (const button of document.querySelectorAll("[data-button]")) button.classList.toggle("is-held", Boolean(buttons & BUTTONS[button.dataset.button]));
  updateButtons();
});
const clearTouchPointers = bindTouchControls(document.querySelector(".touch-controls"), controllerInput, BUTTONS);
function setRewind(source, active) {
  if (active) rewindSources.add(source); else rewindSources.delete(source);
  const next = Boolean(isPlaying() && rewindSources.size);
  if (next === rewinding) return;
  rewinding = next; audio.flush(); resetClock();
  rewindButton.classList.toggle("is-held", next);
  if (next) {
    rewindFrames = 0; rewindStartedAt = performance.now(); recordInputEvent("rewind-start"); setStatus("Rewinding · release L1 / R to resume");
  } else { updateButtons(); recordInputEvent("rewind-stop"); }
}
function stopRewind() { rewindSources.clear(); setRewind("reset", false); }
function clearInput() { controllerInput.clear(); clearTouchPointers(); stopRewind(); resetClock(); }
function setPaused(value) {
  paused = Boolean(value); clearInput(); audio.flush();
  const label = paused ? "Play game" : "Pause game";
  $("play").setAttribute("aria-label", label); $("play").title = label;
  $("play").setAttribute("aria-pressed", String(!paused));
  $("play").querySelector("path").setAttribute("d", paused ? "m7 3 14 9-14 9z" : "M6 4h4v16H6zm8 0h4v16h-4z");
  if (emulator) setStatus(paused ? "Game frozen · look around to inspect it" : "Playing");
}
function updateTouchLayout() {
  const show = touchLayoutInput.value === "show" || (touchLayoutInput.value === "auto" && matchMedia("(any-pointer: coarse)").matches && !gamepadConnected);
  appShell.dataset.touch = String(show); clearInput(); resize();
}
function pollGamepads(presenting = false, visible = true) {
  const pads = navigator.getGamepads?.() ?? [], state = readGamepads(pads, BUTTONS);
  if (state.connected !== gamepadConnected) {
    gamepadConnected = state.connected; updateTouchLayout();
    if (state.connected) setStatus("Controller connected · hold L1 to rewind");
  }
  const enabled = isPlaying() && visible && (presenting || document.hasFocus());
  controllerInput.set("gamepad", enabled ? state.buttons : 0);
  if (enabled && state.buttons) returnToGame();
  setRewind("gamepad", enabled && Array.from(pads).some(p => p?.connected && p.mapping === "standard" && p.buttons[4]?.pressed));
}
function drawFrame(scheduleAudio = true) {
  const metadata = emulator.step_frame(); gameFrames++;
  if (scheduleAudio) {
    try { audio.schedule(emulator.audio_buffer().subarray(0, metadata.audio_samples), emulator.audio_stem_buffer().subarray(0, metadata.audio_samples * 5), metadata.audio_sample_rate, metadata.audio_discontinuity); }
    catch (error) { reportDebug("audio-scheduling", error); audio.flush(); }
  }
  recordFrame("frame", metadata);
  return metadata;
}
function renderVideo(reset = false) {
  if (mode !== "flat") { viewer?.refresh(reset); return; }
  if (!emulator) return;
  image.data.set(emulator.rgba_buffer()); context.putImageData(image, 0, 0); flatFrames++;
}
// Exactly one scheduler advances the core: window RAF in flat mode; the shared
// renderer's animation loop in both desktop depth modes and headset sessions.
function advance(now, { visible = !document.hidden, presenting = false } = {}) {
  try {
    pollGamepads(presenting, visible);
    if (!visible || !isPlaying()) { resetClock(); return 0; }
    if (!previousTime) previousTime = now;
    accumulator += Math.min(100, Math.max(0, now - previousTime)); previousTime = now;
    const budget = frameBudget(accumulator, FRAME_MS); accumulator = budget.remainder;
    for (let n = 0; n < budget.frames; n++) {
      const available = rewinding ? emulator.rewind() : false;
      const metadata = drawFrame(!rewinding);
      if (rewinding) { recordFrame("rewind", metadata, { available }); rewindFrames++; }
      metadata.free?.();
    }
    if (budget.frames) renderVideo();
    return budget.frames;
  } catch (error) {
    reportDebug("emulation-frame", error); setPaused(true); setStatus(`Emulation stopped: ${error.message}`); return 0;
  }
}
function tick(now) {
  if (mode === "flat") {
    advance(now);
    if (now - flatStatsAt >= 1000) {
      const seconds = (now - flatStatsAt) / 1000;
      $("fps").textContent = `Render ${Math.round((flatFrames - lastFlatFrames) / seconds)} fps · Game ${Math.round((gameFrames - lastGameFrames) / seconds)} fps · Scene — fps`;
      flatStatsAt = now; lastFlatFrames = flatFrames; lastGameFrames = gameFrames;
    }
  }
  requestAnimationFrame(tick);
}
async function ensureViewer() {
  viewerPromise ??= import("./vr.js?v=9").then(({ createDioramaView }) => {
    viewer = createDioramaView({ buttons: BUTTONS, input: controllerInput, audio, getEmulator: () => emulator,
      isPlaying, isRewinding: () => rewinding, setRewind, clearInput, resetClock, pause: () => setPaused(true),
      tick: (now, state) => mode === "flat" ? 0 : advance(now, state),
      activateForVR: () => {
        const ready = mode === "flat" ? setMode("orbit") : Promise.resolve();
        const generation = modeGeneration;
        return { ready, isCurrent: () => generation === modeGeneration && mode !== "flat" };
      },
      sessionChanged: presenting => {
        cancelViewFade(); xrPageFade?.cancel();
        if (!presenting && !reducedMotion()) xrPageFade = $("stage").animate([{ opacity: 0 }, { opacity: 1 }], { duration: 180 });
        if (presenting && effectsSettings.open) setSettingsOpen(false);
        motionController.setPaused(presenting || mode !== "depth"); updateMotionButton(); clearInput();
        if (!presenting) returnToGame();
      },
    });
    viewer.setFPSVisible(showFPSInput.checked);
    if (preferred && !defaultsApplied) { viewer.applySettings(preferred); defaultsApplied = true; }
    updateSettings();
    return viewer;
  }).catch(error => { viewerPromise = null; throw error; });
  return viewerPromise;
}
async function setMode(next) {
  if (!["flat", "depth", "orbit"].includes(next)) next = "depth";
  const generation = ++modeGeneration;
  try {
    const scene = next === "flat" ? viewer : await ensureViewer();
    if (generation !== modeGeneration) return;
    if (scene?.presenting) await scene.exitVR();
    if (generation !== modeGeneration) return;
    const crossfade = mode !== next && (mode === "flat" || next === "flat") && captureOutgoingView();
    clearInput(); audio.flush(); mode = next;
    await scene?.setMode(next);
    if (generation !== modeGeneration) return;
    // Keep native scene layers available across camera switches, including
    // while paused in flat mode. Only the active view performs rendering.
    emulator?.set_depth_enabled(true);
    appShell.dataset.view = next;
    $("view-mode").value = next;
    $("effects-button").setAttribute("aria-pressed", String(next !== "flat"));
    $("orbit-button").setAttribute("aria-pressed", String(next === "orbit"));
    $("enter-vr").hidden = false;
    motionController.setPaused(next !== "depth" || Boolean(scene?.presenting)); updateMotionButton();
    const url = new URL(location.href); url.searchParams.set("view", next); history.replaceState(null, "", url);
    renderVideo(true); resize();
    if (crossfade) fadeOutgoingView();
    $("look-picker-hint").textContent = next === "flat" ? "Picture settings affect the 3D views. Original 2D colors stay unchanged." : "The same frozen scene, rendered with the same 3D model in every view.";
  } catch (error) { setStatus(`3D unavailable: ${error.message}`); $("fatal").hidden = false; if (next !== "flat") void setMode("flat"); }
}
const motionController = new DeviceTiltController({
  onView: view => viewer?.setTilt(view, !motionController.enabled),
  onStatus: ({ state, message }) => {
    tiltEnabled = ["requesting", "calibrating", "active", "paused"].includes(state); updateMotionButton();
    $("motion-status").hidden = state === "off" || state === "active";
    $("motion-status").textContent = message;
  },
});
$("motion-button").addEventListener("click", () => {
  modeGeneration++;
  if (motionController.enabled && mode === "depth" && !viewer?.presenting) motionController.stop();
  else {
    // Motion permission must be requested directly in this user gesture.
    if (!motionController.enabled) void motionController.start();
    void setMode("depth");
  }
});
$("motion-recenter-button").remove();
$("recenter").addEventListener("click", () => { viewer?.recenter(); motionController.recenter(); });
$("effects-button").addEventListener("click", () => void setMode(mode === "flat" ? "depth" : "flat"));
$("orbit-button").addEventListener("click", () => void setMode(mode === "orbit" ? "depth" : "orbit"));
$("view-mode").addEventListener("change", event => void setMode(event.target.value));
$("play").addEventListener("click", () => { setPaused(!paused); if (!paused) returnToGame(); });

function resize() {
  const width = Math.max(1, Math.min(gamePanel.clientWidth, gamePanel.clientHeight * WIDTH / HEIGHT));
  screenWrap.style.width = `${width}px`;
}
new ResizeObserver(resize).observe(gamePanel); window.addEventListener("resize", () => { cancelViewFade(); resize(); });
touchLayoutInput.addEventListener("change", updateTouchLayout);
matchMedia("(any-pointer: coarse)").addEventListener("change", updateTouchLayout);
function setMoreOpen(open) {
  $("more-menu").hidden = !open; $("more-button").setAttribute("aria-expanded", String(open));
  if (!open) setRewind("pointer", false);
}
$("more-button").addEventListener("click", () => setMoreOpen($("more-menu").hidden));
document.addEventListener("pointerdown", event => { if (!event.target.closest("#more-menu, #more-button")) setMoreOpen(false); });
for (const name of ["contextmenu", "selectstart", "dragstart"]) $("more-menu").addEventListener(name, event => event.preventDefault());
function showChrome() {
  appShell.classList.add("chrome-visible"); clearTimeout(chromeTimeout);
  chromeTimeout = setTimeout(() => { if (!effectsSettings.open && $("more-menu").hidden && !viewer?.presenting) appShell.classList.remove("chrome-visible"); }, 2400);
}
async function toggleFullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else if (appShell.requestFullscreen) await appShell.requestFullscreen();
    else appShell.classList.toggle("is-immersive");
    updateFullscreenControls();
    showChrome(); returnToGame();
  } catch (error) { setStatus(`Fullscreen unavailable: ${error.message}`); }
}
function updateFullscreenControls() {
  const active = Boolean(document.fullscreenElement) || appShell.classList.contains("is-immersive");
  const label = active ? "Exit fullscreen" : "Enter fullscreen";
  for (const id of ["fullscreen-button", "fullscreen-more-button"]) {
    $(id).setAttribute("aria-pressed", String(active)); $(id).setAttribute("aria-label", label); $(id).title = label;
  }
}
$("fullscreen-button").addEventListener("click", toggleFullscreen);
$("fullscreen-more-button").addEventListener("click", () => { setMoreOpen(false); void toggleFullscreen(); });
document.addEventListener("fullscreenchange", () => {
  appShell.classList.toggle("is-immersive", Boolean(document.fullscreenElement));
  updateFullscreenControls(); resize();
});
for (const name of ["pointermove", "pointerdown", "keydown"]) window.addEventListener(name, showChrome, { passive: true });

const PRESETS = [
  { id: "miniature", name: "Miniature relief", description: "70 cm · close layers, thick paper edges", values: { size: "70", gap: "4", thickness: "30" } },
  { id: "pressed", name: "Pressed layers", description: "140 cm · sandwiched layers, no air gap", values: { size: "140", gap: "0", thickness: "30" } },
  { id: "airy", name: "Paper theatre", description: "90 cm · a deeper backdrop, thin cards", values: { size: "90", gap: "24", thickness: "16" } },
];
const settingsControls = ["size", "gap", "thickness", "material-finish-input", "vr-resolution", "shadows"];
const factoryDefaults = Object.fromEntries(settingsControls.map(id => [id, $(id).type === "checkbox" ? $(id).defaultChecked : $(id).tagName === "SELECT" ? [...$(id).options].find(o => o.defaultSelected)?.value ?? $(id).options[0].value : $(id).defaultValue]));
const DEFAULT_KEY = "nes.scene-defaults.v1";
let preferred = null, defaultsApplied = false;
const valueOf = id => $(id).type === "checkbox" ? $(id).checked : $(id).value;
function validSetting(id, value) {
  const c = $(id);
  if (c.type === "checkbox") return typeof value === "boolean";
  if (typeof value !== "string" || value === "") return false;
  if (c.tagName === "SELECT") return [...c.options].some(o => o.value === value);
  return Number.isFinite(Number(value)) && Number(value) >= Number(c.min) && Number(value) <= Number(c.max) && (Number(value) - Number(c.min)) % Number(c.step) === 0;
}
try {
  const saved = JSON.parse(localStorage.getItem(DEFAULT_KEY));
  if (saved?.version === 1 && saved.values && settingsControls.every(id => validSetting(id, saved.values[id]))) preferred = saved.values;
} catch (_) { /* Optional local preferences. */ }
const presetCards = PRESETS.map(preset => {
  const button = document.createElement("button"); button.type = "button"; button.className = "look-card";
  const preview = document.createElement("img"); preview.alt = `${preset.name} preview`; preview.hidden = true;
  const title = document.createElement("strong"); title.textContent = preset.name;
  const hint = document.createElement("small"); hint.textContent = preset.description;
  button.append(preview, title, hint); $("look-presets").append(button);
  button.addEventListener("click", async () => {
    try { const scene = await ensureViewer(); scene.applySettings(preset.values); updateSettings(); }
    catch (error) { setStatus(error.message); }
  });
  return { preset, button, preview };
});
for (const id of settingsControls) {
  const control = $(id), note = document.createElement("small"); note.className = "setting-default"; note.hidden = true;
  control.closest("label").append(note); control.dataset.note = `${id}-default`; note.id = control.dataset.note;
  control.setAttribute("aria-describedby", [control.getAttribute("aria-describedby"), note.id].filter(Boolean).join(" "));
}
function updateSettings() {
  const defaults = preferred ?? factoryDefaults; let changed = 0;
  for (const id of settingsControls) {
    const modified = valueOf(id) !== defaults[id], note = $($(id).dataset.note);
    if (modified) changed++;
    $(id).dataset.modified = String(modified); $(id).closest("label").classList.toggle("is-modified", modified); note.hidden = !modified;
    const text = $(id).type === "checkbox" ? defaults[id] ? "On" : "Off" : $(id).tagName === "SELECT" ? [...$(id).options].find(o => o.value === defaults[id])?.textContent : defaults[id] + (id === "thickness" ? " mm" : " cm");
    note.textContent = `${preferred ? "Your default" : "Default"}: ${text}`;
  }
  $("settings-change-summary").textContent = changed ? `${changed} settings changed` : preferred ? "At your preferred default" : "All settings at default";
  $("reset-effects-settings-button").disabled = changed === 0; $("factory-defaults-button").hidden = !preferred;
  for (const { preset, button } of presetCards) button.setAttribute("aria-pressed", String(Object.entries(preset.values).every(([id, value]) => valueOf(id) === value)));
}
async function buildPreviews(generation) {
  try {
    const scene = await ensureViewer();
    // Wait for the shared shader prewarm and first scene, yielding to the UI.
    for (let attempt = 0; attempt < 80; attempt++) {
      if (generation !== presetGeneration || !effectsSettings.open || scene.presenting) return;
      const images = await scene.capturePresets(PRESETS);
      if (images.length) { if (generation === presetGeneration) images.forEach((src, i) => { presetCards[i].preview.src = src; presetCards[i].preview.hidden = false; }); return; }
      await new Promise(resolve => setTimeout(resolve, 50));
    }
  } catch (error) { reportDebug("preset-preview", error); }
}
function setSettingsOpen(open) {
  clearInput(); audio.flush();
  if (open) {
    setMoreOpen(false); effectsSettings.hidden = false; effectsSettings.showModal();
    updateSettings(); $("settings-action-status").textContent = ""; void buildPreviews(++presetGeneration);
  } else { viewer?.cancelPreviews(); presetGeneration++; effectsSettings.close(); effectsSettings.hidden = true; resetClock(); }
  effectsSettingsButton.setAttribute("aria-expanded", String(open));
}
effectsSettingsButton.addEventListener("click", () => setSettingsOpen(!effectsSettings.open));
$("close-settings-button").addEventListener("click", () => setSettingsOpen(false));
effectsSettings.addEventListener("close", () => { viewer?.cancelPreviews(); presetGeneration++; effectsSettings.hidden = true; effectsSettingsButton.setAttribute("aria-expanded", "false"); clearInput(); returnToGame(); });
for (const name of ["input", "change"]) effectsSettings.addEventListener(name, updateSettings);
$("save-defaults-button").addEventListener("click", () => {
  preferred = Object.fromEntries(settingsControls.map(id => [id, valueOf(id)])); updateSettings();
  try { localStorage.setItem(DEFAULT_KEY, JSON.stringify({ version: 1, values: preferred })); $("settings-action-status").textContent = "Preferred default saved on this device"; }
  catch (_) { $("settings-action-status").textContent = "Preferred default set for this visit; browser storage is unavailable"; }
});
$("reset-effects-settings-button").addEventListener("click", async () => { try { (await ensureViewer()).applySettings(preferred ?? factoryDefaults); updateSettings(); } catch (error) { setStatus(error.message); } });
$("factory-defaults-button").addEventListener("click", async () => {
  preferred = null; try { localStorage.removeItem(DEFAULT_KEY); } catch (_) { /* Storage is optional. */ }
  try { (await ensureViewer()).applySettings(factoryDefaults); updateSettings(); } catch (error) { setStatus(error.message); }
});

async function loadRom(file, { fromCache = false, expectedGeneration = null } = {}) {
  if (expectedGeneration !== null && expectedGeneration !== romLoadGeneration) return false;
  const generation = ++romLoadGeneration;
  let candidate = null;
  try {
    if (file.size > MAX_ROM_BYTES) throw new Error("Choose a ROM under 64 MiB");
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (bytes.length > MAX_ROM_BYTES) throw new Error("Choose a ROM under 64 MiB");
    await wasmReady;
    if (generation !== romLoadGeneration) return false;
    // Validate before replacing the running game, including its rewind history.
    candidate = NesWeb.load_rom(bytes); candidate.set_depth_enabled(true);
    if (generation !== romLoadGeneration) { candidate.free(); return false; }
    clearInput(); audio.flush(); emulator?.free(); emulator = candidate; candidate = null; emulatorId++;
    romName = file.name; savedSnapshot?.free?.(); savedSnapshot = null; setPaused(false);
    appShell.classList.add("has-rom"); snapshotButton.disabled = rewindButton.disabled = $("play").disabled = false;
    restoreButton.disabled = true; recordButton.disabled = !debugEnabled;
    renderVideo(true); accumulator = FRAME_MS;
    setStatus(`${file.name} loaded${fromCache ? " from this browser" : ""}`);
    if (!fromCache) cacheWrites = cacheWrites.catch(() => {}).then(() => saveCachedRom(file.name, bytes));
    return true;
  } catch (error) { candidate?.free(); if (generation === romLoadGeneration) setStatus(`Could not load ROM: ${error.message}`); return false; }
}
async function restoreLastRom() {
  if (romLoadGeneration) return;
  const generation = romLoadGeneration, cached = await loadCachedRom();
  if (generation !== romLoadGeneration || !cached.ok || !cached.bytes) return;
  await loadRom({ name: cached.name, arrayBuffer: async () => cached.bytes.buffer }, { fromCache: true, expectedGeneration: generation });
}
romInput.addEventListener("change", () => { if (romInput.files?.[0]) void loadRom(romInput.files[0]); });
romInput.addEventListener("click", () => { romInput.value = ""; });
$("welcome-load-button").addEventListener("click", () => romInput.click());
audioButton.addEventListener("click", async () => {
  try { if (audio.context?.state === "running") audio.toggleMute(); else await audio.enable(); }
  catch (error) { setStatus(`Audio unavailable: ${error.message}`); }
});
snapshotButton.addEventListener("click", () => {
  if (!emulator) return; savedSnapshot?.free?.(); savedSnapshot = emulator.save_snapshot(); restoreButton.disabled = false; setStatus("State saved");
});
restoreButton.addEventListener("click", () => {
  if (!emulator || !savedSnapshot) return;
  clearInput(); audio.flush(); emulator.restore_snapshot(savedSnapshot); updateButtons();
  // The core refreshes its exported picture/metadata on the next frame.
  const metadata = drawFrame(false); metadata.free?.(); renderVideo(true); setStatus("State restored");
});
rewindButton.addEventListener("pointerdown", event => { event.preventDefault(); rewindButton.setPointerCapture(event.pointerId); setRewind("pointer", true); });
for (const name of ["pointerup", "pointercancel", "lostpointercapture"]) rewindButton.addEventListener(name, event => { event.preventDefault(); setRewind("pointer", false); });
const keyboard = new Map(Object.entries({ w: "up", a: "left", s: "down", d: "right", j: "a", k: "b", z: "a", x: "b", Shift: "select", Enter: "start", "+": "start", "=": "start", "-": "select", ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" }));
window.addEventListener("keydown", event => {
  if (event.defaultPrevented || effectsSettings.open || event.target.closest?.("input, select, textarea") || (["Enter", " "].includes(event.key) && event.target.closest?.("button, summary, a"))) return;
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key === "Escape") { if (!$("more-menu").hidden) { event.preventDefault(); setMoreOpen(false); } return; }
  if (key === "r") { event.preventDefault(); returnToGame(); setRewind("keyboard", true); return; }
  if (["m", "f", "v"].includes(key)) {
    event.preventDefault(); if (event.repeat) return;
    if (key === "m") audio.toggleMute();
    if (key === "f") void toggleFullscreen();
    if (key === "v" && emulator) { emulator.cycle_display_mode(); renderVideo(true); }
    return;
  }
  const name = keyboard.get(key); if (!name) return;
  event.preventDefault(); returnToGame(); controllerInput.set(`key-${event.code || key}`, isPlaying() ? BUTTONS[name] : 0);
});
window.addEventListener("keyup", event => {
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key === "r") { event.preventDefault(); setRewind("keyboard", false); }
  if (keyboard.has(key)) { event.preventDefault(); controllerInput.set(`key-${event.code || key}`, 0); }
});
window.addEventListener("blur", () => { clearInput(); audio.flush(); });
document.addEventListener("visibilitychange", () => { clearInput(); audio.flush(); });

function recordingTime() {
  return recording ? Math.max(0, Math.round(performance.now() - recording.started_perf_at)) : 0;
}

function jsonNumber(value) {
  if (value == null) return null;
  return typeof value === "bigint" ? Number(value) : value;
}

function recordInputEvent(type, details = {}) {
  if (!recording) return;
  recording.input_events.push({
    t_ms: recordingTime(),
    type,
    buttons: heldButtons,
    ...details,
  });
}

function recordFrame(kind, metadata, details = {}) {
  if (!recording) return;
  recording.frames.push({
    index: recording.frames.length,
    t_ms: recordingTime(),
    kind,
    buttons: heldButtons,
    emulator_frame: jsonNumber(metadata?.frame_number),
    ...details,
  });
  const seconds = (recordingTime() / 1000).toFixed(1);
  recordingStatus.textContent = `Recording · ${seconds}s · ${recording.frames.length} frames`;
}

function setRecordingButtons(active) {
  recordButton.disabled = !debugEnabled || !emulator || active;
  stopRecordButton.disabled = !active;
  recordButton.classList.toggle("recording", active);
}

function startRecording() {
  if (!debugEnabled || !emulator || recording) return;
  recording = {
    version: 1,
    source: "nes-rs-browser",
    rom_name: romName,
    frame_rate: 60,
    started_at: new Date().toISOString(),
    started_perf_at: performance.now(),
    started_frame: jsonNumber(emulator.frame_metadata().frame_number),
    frames: [],
    input_events: [],
  };
  recordInputEvent("record-start");
  setRecordingButtons(true);
  recordingStatus.textContent = "Recording · 0.0s · 0 frames";
}

function stopRecording() {
  if (!recording) return;
  if (rewinding) stopRewind();
  const payload = {
    ...recording,
    stopped_at: new Date().toISOString(),
    stopped_frame: jsonNumber(emulator?.frame_metadata().frame_number),
  };
  window.lastRecordingPayload = payload;
  recording = null;
  setRecordingButtons(false);
  try {
    const url = URL.createObjectURL(new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = `nes-replay-${new Date().toISOString().replace(/[:.]/g, "-")}.json`;
    link.hidden = true;
    document.body.append(link);
    link.click(); link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    recordingStatus.textContent = `Saved locally · ${payload.frames.length} frames`;
    setStatus("Replay capture saved locally");
  } catch (error) {
    reportDebug("capture-save", error);
    recordingStatus.textContent = "Local save failed; capture remains available in this tab";
    setStatus("Could not save replay capture locally");
  }
}

recordButton.addEventListener("click", startRecording);
stopRecordButton.addEventListener("click", stopRecording);
const wasmReady = init("./pkg/nes_wasm_bg.wasm?v=13");
updateTouchLayout(); updateSettings(); resize();
if (debugEnabled) window.nesPlayerDebug = () => {
  const metadata = emulator?.frame_metadata();
  const frame = metadata ? String(metadata.frame_number) : null; metadata?.free?.();
  return { mode, emulatorId, frame, paused, playing: isPlaying(), rewinding, buttons: heldButtons, gameFrames,
    audioState: audio.context?.state ?? "uninitialized", savedState: Boolean(savedSnapshot) };
};
try {
  await wasmReady; requestAnimationFrame(tick);
  const initial = params.get("view") ?? "depth";
  await setMode(initial);
  void restoreLastRom();
  // Prepare a connected headset even when the player starts in original 2D.
  if (mode === "flat") {
    const unavailable = () => {
      if (mode !== "flat" || viewer) return;
      $("enter-vr").setAttribute("aria-label", "VR headset unavailable"); $("enter-vr").title = "VR headset unavailable";
    };
    if (navigator.xr) void navigator.xr.isSessionSupported("immersive-vr").then(supported => supported ? ensureViewer() : unavailable()).catch(unavailable);
    else unavailable();
  }
} catch (error) { setStatus(`Could not start the player: ${error.message}`); }
