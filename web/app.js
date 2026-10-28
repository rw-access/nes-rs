import init, * as wasm from "./pkg/nes_wasm.js";
import { SpatialAudio } from "./spatial-audio.js";
import { CanvasDepthRenderer, cameraDepthScale, projectDepthPoint } from "./depth-mesh.js";
import { DioramaFrame } from "./diorama-frame.js";
import { CanvasBackdropRenderer } from "./backdrop-lighting.js";
import { CanvasCardShadowRenderer } from "./card-shadow.js";
import { RimHousing, RIM_MATERIALS } from "./rim-housing.js";
import { RIM_CORNER_FRACTION } from "./rim-outline.js";
import { setSceneLightPlacement, setSceneLightRotation, getSceneLightRevision } from "./scene-lighting.js";
import { setSceneCamera, getSceneViewRevision } from "./scene-view.js";
import { resolveScreenProfile, estimateGameWidthCm, computeViewingCamera, SCENE_DEPTH_TO_NATIVE_PIXELS, computeStationaryView, resolveRenderedScenePose } from "./viewing-model.js";
import { DeviceTiltController } from "./device-tilt.js";
import { loadCachedRom, saveCachedRom, clearCachedRom } from "./rom-store.js";
import { resolveSceneDepth, resolveShadowSoftness, resolveRimThickness, resolveRimRecess, gameplayShadowProjection } from "./depth-model.js";

const NesWeb = wasm.NesWasm ?? wasm.NesWeb;

const debugEnabled = new URLSearchParams(window.location.search).get("debug") === "1";
const debugEndpoint = debugEnabled ? new URL("__debug", document.baseURI).href : null;
const recordEndpoint = new URL("__record", document.baseURI).href;

function debugValue(value) {
  if (value instanceof Error) {
    return { name: value.name, message: value.message, stack: value.stack };
  }
  if (typeof value === "string") return value;
  try { return JSON.parse(JSON.stringify(value)); }
  catch (_) { return String(value); }
}

function reportDebug(kind, ...values) {
  const record = {
    timestamp: new Date().toISOString(),
    kind,
    url: window.location.href,
    values: values.map(debugValue),
  };
  if (debugEnabled) {
    fetch(debugEndpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(record),
      keepalive: true,
    }).catch(() => {});
  }
  return record;
}

window.addEventListener("error", (event) => {
  reportDebug("window.error", event.error || event.message);
});
window.addEventListener("unhandledrejection", (event) => {
  reportDebug("unhandledrejection", event.reason);
});

if (debugEnabled) {
  for (const level of ["log", "info", "warn", "error"]) {
    const original = console[level].bind(console);
    console[level] = (...values) => {
      original(...values);
      reportDebug(`console.${level}`, ...values);
    };
  }
}

const WIDTH = 256;
const HEIGHT = 240;
const FRAME_MS = 1000 / 60;
const DISPLAY_MODE_LABELS = ["Background", "Sprites", "Both"];
const GHOST_RECORD_BYTES = 6;
const NEUTRAL_ROOM_ROTATION = Object.freeze([1, 0, 0, 0, 1, 0, 0, 0, 1]);

const BUTTONS = Object.freeze({
  a: 1 << 0,
  b: 1 << 1,
  select: 1 << 2,
  start: 1 << 3,
  up: 1 << 4,
  down: 1 << 5,
  left: 1 << 6,
  right: 1 << 7,
});

const canvas = document.querySelector("#screen");
const context = canvas.getContext("2d", { alpha: false });
const image = context.createImageData(WIDTH, HEIGHT);
const screenStage = document.querySelector("#screen-stage");
const screenWrap = document.querySelector(".screen-wrap");
const depthStack = document.querySelector("#depth-stack");
const depthCanvases = [...depthStack.querySelectorAll("canvas")];
const dioramaCanvas = document.createElement("canvas");
dioramaCanvas.className = "diorama-box";
dioramaCanvas.hidden = true;
dioramaCanvas.setAttribute("aria-hidden", "true");
depthStack.insertBefore(dioramaCanvas, depthCanvases[1]);
const dioramaFrame = new DioramaFrame(dioramaCanvas, WIDTH, HEIGHT);
// The complete gameplay surface casts one shadow onto the backdrop. NES
// drawing priority determines occlusion, not physical gaps between objects.
const backgroundShadow = document.createElement("canvas");
backgroundShadow.className = "layer-shadow";
backgroundShadow.setAttribute("aria-hidden", "true");
depthStack.insertBefore(backgroundShadow, depthCanvases[1]);
const cardShadowRenderer = new CanvasCardShadowRenderer(backgroundShadow, WIDTH, HEIGHT);
const shadowCasterCanvas = document.createElement("canvas");
shadowCasterCanvas.width = WIDTH;
shadowCasterCanvas.height = HEIGHT;
const shadowCasterContext = shadowCasterCanvas.getContext("2d");
const shadowCasterImage = shadowCasterContext.createImageData(WIDTH, HEIGHT);
// Like a surrounding tile, rim sides sit over backdrop and below all gameplay.
const rimCanvas = document.createElement("canvas");
rimCanvas.id = "tv-rim";
rimCanvas.hidden = true;
rimCanvas.setAttribute("aria-hidden", "true");
depthStack.insertBefore(rimCanvas, depthCanvases[1]);
const rimHousing = new RimHousing(rimCanvas, screenStage, WIDTH, HEIGHT);
let shadowsNeedPaint = true;
const backdropRenderer = new CanvasBackdropRenderer(depthCanvases[0], WIDTH, HEIGHT);
const depthRenderers = depthCanvases.map((plane, index) => index === 0 ? null : new CanvasDepthRenderer(plane, WIDTH, HEIGHT));
const ghostCanvas = document.querySelector("#depth-ghosts");
const ghostContext = ghostCanvas.getContext("2d");
const ghostImage = ghostContext.createImageData(WIDTH, HEIGHT);
const effectsButton = document.querySelector("#effects-button");
const effectsSettingsButton = document.querySelector("#effects-settings-button");
const effectsSettings = document.querySelector("#effects-settings");
const resetEffectsSettingsButton = document.querySelector("#reset-effects-settings-button");
const motionButton = document.querySelector("#motion-button");
const motionRecenterButton = document.querySelector("#motion-recenter-button");
const motionStatus = document.querySelector("#motion-status");
const motionTrackingInput = document.querySelector("#motion-tracking-input");
const motionStrengthInput = document.querySelector("#motion-strength-input");
const motionStrengthOutput = document.querySelector("#motion-strength-value");
const viewingDistanceInput = document.querySelector("#viewing-distance-input");
const viewingDistanceOutput = document.querySelector("#viewing-distance-value");
const screenProfileInput = document.querySelector("#screen-profile-input");
const screenProfileHint = document.querySelector("#screen-profile-hint");
const depthInput = document.querySelector("#depth-input");
const depthOutput = document.querySelector("#depth-value");
const softnessInput = document.querySelector("#softness-input");
const softnessOutput = document.querySelector("#softness-value");
const lightElevationInput = document.querySelector("#light-elevation-input");
const lightElevationOutput = document.querySelector("#light-elevation-value");
const lightAzimuthInput = document.querySelector("#light-azimuth-input");
const lightAzimuthOutput = document.querySelector("#light-azimuth-value");
const lightDistanceInput = document.querySelector("#light-distance-input");
const lightDistanceOutput = document.querySelector("#light-distance-value");
const thicknessInput = document.querySelector("#thickness-input");
const thicknessOutput = document.querySelector("#thickness-value");
const rimThicknessInput = document.querySelector("#rim-thickness-input");
const rimThicknessOutput = document.querySelector("#rim-thickness-value");
const rimRecessInput = document.querySelector("#rim-recess-input");
const rimRecessOutput = document.querySelector("#rim-recess-value");
const rimSoftnessInput = document.querySelector("#rim-softness-input");
const rimSoftnessOutput = document.querySelector("#rim-softness-value");
const consoleEdgeInput = document.querySelector("#console-edge-input");
const rimInput = document.querySelector("#rim-input");
const recessedOpeningInput = document.querySelector("#recessed-opening-input");
const rimWidthInput = document.querySelector("#rim-width-input");
const rimWidthOutput = document.querySelector("#rim-width-value");
const solidSidesInput = document.querySelector("#solid-sides-input");
const spriteReliefInput = document.querySelector("#sprite-relief-input");
const spriteReliefStyle = document.querySelector("#sprite-relief-style");
const thinOverlapsInput = document.querySelector("#thin-overlaps-input");
const dioramaInput = document.querySelector("#diorama-input");
const status = document.querySelector("#status");
const romInput = document.querySelector("#rom-input");
const audioButton = document.querySelector("#audio-button");
const audioLabel = audioButton.querySelector(".tool-label");
const snapshotButton = document.querySelector("#snapshot-button");
const restoreButton = document.querySelector("#restore-button");
const rewindButton = document.querySelector("#rewind-button");
const recordButton = document.querySelector("#record-button");
const stopRecordButton = document.querySelector("#stop-record-button");
const recordingStatus = document.querySelector("#recording-status");

let emulator = null;
let romName = null;
let savedSnapshot = null;
let heldButtons = 0;
let previousTime = 0;
let accumulator = 0;
let rewinding = false;
let rewindFrames = 0;
let rewindStartedAt = 0;
let recording = null;
let romLoadGeneration = 0;
let romCacheWrites = Promise.resolve();
let pendingMotionView = null;
let latestMotionView = { x: 0, y: 0 };
let viewingDistanceAdjusted = false;
let activeScreenProfile = null;
let motionState = "off";
const cardThicknessByMode = { stationary: 4, tilt: 4 };
const lightElevationByMode = { stationary: 7, tilt: 3 };
const suggestPhoneMotion = window.matchMedia("(pointer: coarse)").matches;

function updateMotionControls(state = motionState, message = motionStatus.textContent) {
  motionState = state;
  const enabled = ["requesting", "calibrating", "active", "paused"].includes(state);
  motionButton.setAttribute("aria-pressed", String(enabled));
  motionButton.disabled = state === "requesting" || (!wantsLayeredVideo() && !enabled);
  motionButton.title = enabled ? "Turn phone tilt off" : "Enable phone tilt";
  motionRecenterButton.hidden = !enabled;
  motionRecenterButton.disabled = !enabled || state === "requesting" || state === "paused";
  motionStrengthInput.disabled = !enabled || state === "requesting";
  motionStatus.hidden = state === "off" && !suggestPhoneMotion;
  motionStatus.textContent = state === "off" && suggestPhoneMotion
    ? "Tap Tilt while holding your phone comfortably, then move it to look inside."
    : message;
}

const motionController = new DeviceTiltController({
  trackingMode: motionTrackingInput.value,
  // Sensor callbacks only queue a camera change. The existing render loop
  // applies the latest one before drawing, avoiding a second video loop.
  onView: view => { pendingMotionView = view; },
  onStatus: ({ state, message }) => {
    const wasEnabled = motionButton.getAttribute("aria-pressed") === "true";
    updateMotionControls(state, message);
    // Starting/stopping can leave an already-neutral sensor pose unchanged.
    // Still render the switch between the static and calibrated cameras.
    const enabled = motionButton.getAttribute("aria-pressed") === "true";
    if (wasEnabled !== enabled) {
      cardThicknessByMode[wasEnabled ? "tilt" : "stationary"] = Number(thicknessInput.value);
      thicknessInput.defaultValue = "4";
      thicknessInput.value = String(cardThicknessByMode[enabled ? "tilt" : "stationary"]);
      lightElevationByMode[wasEnabled ? "tilt" : "stationary"] = Number(lightElevationInput.value);
      lightElevationInput.defaultValue = enabled ? "3" : "7";
      lightElevationInput.value = String(lightElevationByMode[enabled ? "tilt" : "stationary"]);
      pendingMotionView ??= { x: 0, y: 0 };
    }
  },
});
motionController.setStrength(Number(motionStrengthInput.value));

function applyPendingMotionView() {
  if (!pendingMotionView) return false;
  latestMotionView = pendingMotionView;
  pendingMotionView = null;
  const revision = getSceneViewRevision();
  const lightingRevision = getSceneLightRevision();
  updateDepthGeometry();
  return revision !== getSceneViewRevision() || lightingRevision !== getSceneLightRevision();
}

function updateViewingCamera() {
  const screenWidth = window.screen.width || window.innerWidth;
  const screenHeight = window.screen.height || window.innerHeight;
  const profile = resolveScreenProfile(screenProfileInput.value, {
    screenWidth, screenHeight,
    coarsePointer: window.matchMedia("(pointer: coarse)").matches,
  });
  if (!viewingDistanceAdjusted && activeScreenProfile !== profile.id) {
    viewingDistanceInput.value = String(profile.defaultDistanceCm);
  }
  activeScreenProfile = profile.id;
  const gameWidthCm = estimateGameWidthCm({
    profile, screenWidth, screenHeight,
    stageCssWidth: Math.max(1, screenStage.getBoundingClientRect().width),
    viewportScale: window.visualViewport?.scale || 1,
  });
  const distanceCm = Number(viewingDistanceInput.value);
  // The fixture is in the initial phone frame, independent of eye distance.
  setSceneLightPlacement({
    distanceCm: Number(lightDistanceInput.value),
    elevationDegrees: Number(lightElevationInput.value),
    azimuthDegrees: Number(lightAzimuthInput.value),
    gameWidthCm,
  });
  const view = resolveRenderedScenePose(!wantsLayeredVideo() ? { x: 0, y: 0 }
    : motionController.enabled ? latestMotionView : computeStationaryView({ gameWidthCm }));
  setSceneLightRotation(view.rotation ?? NEUTRAL_ROOM_ROTATION);
  const camera = computeViewingCamera({ distanceCm, gameWidthCm, motionX: view.x, motionY: view.y });
  setSceneCamera(camera.x, camera.y, camera.focalDepth);
  viewingDistanceOutput.textContent = `~${distanceCm} cm`;
  screenProfileHint.textContent = profile.shortWidthCm === null
    ? `Desktop estimate · Visible game width ~${gameWidthCm.toFixed(1)} cm`
    : `${profile.label} estimate · Visible game width ~${gameWidthCm.toFixed(1)} cm`;
}

function currentSceneDepth() {
  const enabled = effectsButton.getAttribute("aria-pressed") === "true";
  return resolveSceneDepth(enabled ? Number(depthInput.value) : 0, Number(thicknessInput.value), solidSidesInput.checked);
}

function currentRimGeometry(scene) {
  const enabled = (rimInput.checked || recessedOpeningInput.checked) && Number(rimWidthInput.value) > 0;
  const frontDepth = enabled ? resolveRimRecess(scene, Number(rimRecessInput.value)) : 0;
  return {
    enabled,
    frontDepth,
    material: !rimInput.checked || consoleEdgeInput.checked ? "console" : "charcoal",
    thickness: enabled ? resolveRimThickness(scene, Number(rimThicknessInput.value), frontDepth) : 0,
  };
}

function updateDepthGeometry() {
  updateViewingCamera();
  lightDistanceOutput.textContent = `${Number(lightDistanceInput.value)} cm`;
  lightElevationOutput.textContent = `${Number(lightElevationInput.value)}°`;
  const azimuth = Number(lightAzimuthInput.value);
  lightAzimuthOutput.textContent = azimuth === 0 ? "Centered" : `${Math.abs(azimuth)}° ${azimuth < 0 ? "left" : "right"}`;
  motionStrengthOutput.textContent = `${Number(motionStrengthInput.value)}%`;
  const scene = currentSceneDepth();
  const { separation: depth } = scene;
  const width = screenStage.clientWidth;
  const pixelScale = screenStage.getBoundingClientRect().width / WIDTH;
  depthOutput.textContent = Number(depthInput.value) === 0 ? "Off" : `${depthInput.value} game px`;
  softnessOutput.textContent = Number(softnessInput.value) === 0 ? "Crisp" : `${softnessInput.value}%`;
  const softness = resolveShadowSoftness(Number(softnessInput.value));
  const rim = currentRimGeometry(scene);
  screenWrap.dataset.rim = String(rim.enabled);
  screenWrap.style.setProperty("--rim-width", String(Number(rimWidthInput.value) / 100));
  rimWidthInput.disabled = !rimInput.checked && !recessedOpeningInput.checked;
  rimThicknessInput.disabled = !rim.enabled || !solidSidesInput.checked;
  rimRecessInput.disabled = !rim.enabled;
  consoleEdgeInput.disabled = !rim.enabled || !rimInput.checked;
  rimRecessOutput.textContent = Number(rimRecessInput.value) === 0 ? "Flush" : `${rimRecessInput.value}% · ${Number((rim.frontDepth * SCENE_DEPTH_TO_NATIVE_PIXELS).toFixed(2))} game px`;
  dioramaInput.disabled = !rim.enabled;
  rimSoftnessInput.disabled = !rim.enabled || !dioramaInput.checked;
  rimSoftnessOutput.textContent = Number(rimSoftnessInput.value) === 0 ? "Crisp" : `${rimSoftnessInput.value}%`;
  const rimCenter = projectDepthPoint({ x: WIDTH / 2, y: HEIGHT / 2 }, rim.frontDepth, WIDTH, HEIGHT);
  const rimScale = cameraDepthScale(rim.frontDepth);
  screenWrap.style.setProperty("--rim-transform", `translate(${(rimCenter.x - WIDTH / 2) * pixelScale}px, ${(rimCenter.y - HEIGHT / 2) * pixelScale}px) scale(${rimScale})`);
  screenWrap.style.setProperty("--rim-face-color", RIM_MATERIALS[rim.material].front);
  const faceWidth = pixelScale > 0 ? parseFloat(getComputedStyle(screenWrap).paddingLeft) * Number(rimWidthInput.value) / 100 / pixelScale : 0;
  rimWidthOutput.textContent = Number(rimWidthInput.value) === 0 ? "None"
    : `${rimWidthInput.value}% · ${Number(faceWidth.toFixed(2))} game px`;
  screenWrap.style.setProperty("--opening-radius-x", `${WIDTH * RIM_CORNER_FRACTION * pixelScale}px`);
  screenWrap.style.setProperty("--opening-radius-y", `${HEIGHT * RIM_CORNER_FRACTION * pixelScale}px`);
  screenWrap.style.setProperty("--rim-radius-x", `${(WIDTH * RIM_CORNER_FRACTION + faceWidth) * pixelScale}px`);
  screenWrap.style.setProperty("--rim-radius-y", `${(HEIGHT * RIM_CORNER_FRACTION + faceWidth) * pixelScale}px`);
  rimHousing.paint(depth, rim.thickness, window.devicePixelRatio || 1, rim.frontDepth, rim.enabled ? faceWidth : 0, rim.material, getComputedStyle(document.body).backgroundColor);
  thicknessInput.disabled = !solidSidesInput.checked;
  spriteReliefInput.disabled = !solidSidesInput.checked;
  spriteReliefStyle.disabled = !solidSidesInput.checked || !spriteReliefInput.checked;
  thinOverlapsInput.disabled = !solidSidesInput.checked || !spriteReliefInput.checked;
  const cardLimited = scene.thickness + 1e-10 < Number(thicknessInput.value) / SCENE_DEPTH_TO_NATIVE_PIXELS;
  thicknessOutput.textContent = solidSidesInput.checked
    ? `${thicknessInput.value} game px${cardLimited ? " (limited by gap)" : ""}`
    : "Paper";
  const rimLimited = rim.thickness + 1e-10 < scene.thickness * Number(rimThicknessInput.value) / 100;
  rimThicknessOutput.textContent = `${rimThicknessInput.value}% · ${Number((rim.thickness * SCENE_DEPTH_TO_NATIVE_PIXELS).toFixed(2))} game px${rim.enabled && rimLimited ? " (limited)" : ""}`;
  ghostCanvas.style.transform = "none";
  depthCanvases.forEach((plane, index) => {
    if (index !== 0 && width > 0) {
      depthRenderers[index].resize(width, width * HEIGHT / WIDTH, window.devicePixelRatio || 1);
    }
    plane.style.transform = "none";
    plane.style.filter = "none";
  });
  cardShadowRenderer.resize(width, width * HEIGHT / WIDTH, window.devicePixelRatio || 1);
  // The receiver is still at full separation, but the shadow's light ray
  // and penumbra cross only the air remaining behind the solid's rear face.
  const shadow = gameplayShadowProjection(scene, WIDTH, HEIGHT);
  backgroundShadow.style.transformOrigin = "50% 50%";
  // The shadow renderer already projects the entire solid into display space.
  backgroundShadow.style.transform = "none";
  backgroundShadow.style.filter = `brightness(0) opacity(0.32) blur(${shadow.blur * pixelScale * softness}px)`;
  backgroundShadow.style.visibility = depth > 0 ? "visible" : "hidden";
  // The backdrop is unbounded; the TV opening casts a translucent shadow
  // rather than clipping shadows or artwork to a smaller receiving card.
  backgroundShadow.style.clipPath = "none";
  shadowsNeedPaint = true;
  dioramaCanvas.hidden = !dioramaInput.checked || !rim.enabled;
  document.querySelector("#screen-scene").style.transform = "none";
  if (width > 0) dioramaFrame.resize(width, width * HEIGHT / WIDTH, window.devicePixelRatio || 1);
}

function wantsLayeredVideo() {
  return currentSceneDepth().separation > 0;
}

function updatePresentation() {
  motionController.setPaused(!wantsLayeredVideo());
  if (!wantsLayeredVideo()) {
    pendingMotionView = null;
    latestMotionView = { x: 0, y: 0 };
  }
  updateMotionControls();
  updateDepthGeometry();
  emulator?.set_depth_enabled(wantsLayeredVideo());
  if (emulator) renderVideo();
}

function setEffectsSettingsOpen(open) {
  effectsSettings.hidden = !open;
  effectsSettingsButton.setAttribute("aria-expanded", String(open));
}

function resetEffectsSettings() {
  cardThicknessByMode.stationary = 4;
  cardThicknessByMode.tilt = 4;
  lightElevationByMode.stationary = 7;
  lightElevationByMode.tilt = 3;
  lightElevationInput.defaultValue = motionController.enabled ? "3" : "7";
  for (const control of effectsSettings.querySelectorAll("input, select")) {
    if (control instanceof HTMLInputElement) {
      if (control.type === "checkbox" || control.type === "radio") control.checked = control.defaultChecked;
      else control.value = control.defaultValue;
    } else if (control.multiple) {
      for (const option of control.options) option.selected = option.defaultSelected;
    } else {
      const defaultIndex = [...control.options].findIndex(option => option.defaultSelected);
      control.selectedIndex = defaultIndex < 0 ? 0 : defaultIndex;
    }
  }

  viewingDistanceAdjusted = false;
  activeScreenProfile = null;
  motionController.setTrackingMode(motionTrackingInput.value);
  motionController.setStrength(Number(motionStrengthInput.value));
  if (pendingMotionView) {
    latestMotionView = pendingMotionView;
    pendingMotionView = null;
  }
  updatePresentation();
}

effectsSettingsButton.addEventListener("click", () => setEffectsSettingsOpen(effectsSettings.hidden));
resetEffectsSettingsButton.addEventListener("click", resetEffectsSettings);
effectsButton.addEventListener("click", () => {
  const enabled = effectsButton.getAttribute("aria-pressed") !== "true";
  effectsButton.setAttribute("aria-pressed", String(enabled));
  effectsSettingsButton.disabled = !enabled;
  if (!enabled) setEffectsSettingsOpen(false);
  updatePresentation();
});
depthInput.addEventListener("input", updatePresentation);
thicknessInput.addEventListener("input", updatePresentation);
rimThicknessInput.addEventListener("input", updatePresentation);
rimRecessInput.addEventListener("input", updatePresentation);
rimSoftnessInput.addEventListener("input", updatePresentation);
consoleEdgeInput.addEventListener("change", updatePresentation);
rimWidthInput.addEventListener("input", updatePresentation);
rimInput.addEventListener("change", updatePresentation);
recessedOpeningInput.addEventListener("change", updatePresentation);
softnessInput.addEventListener("input", updatePresentation);
lightDistanceInput.addEventListener("input", updatePresentation);
lightElevationInput.addEventListener("input", updatePresentation);
lightAzimuthInput.addEventListener("input", updatePresentation);
solidSidesInput.addEventListener("change", updatePresentation);
spriteReliefInput.addEventListener("change", updatePresentation);
spriteReliefStyle.addEventListener("change", updatePresentation);
thinOverlapsInput.addEventListener("change", updatePresentation);
dioramaInput.addEventListener("change", updatePresentation);
motionButton.addEventListener("click", () => {
  if (motionController.enabled) {
    motionController.stop();
    if (applyPendingMotionView() && emulator) renderVideo();
  } else {
    // start() invokes the iOS permission API during this user gesture.
    void motionController.start().then(() => {
      motionController.setPaused(!wantsLayeredVideo());
      if (applyPendingMotionView() && emulator) renderVideo();
    });
  }
});
motionRecenterButton.addEventListener("click", () => {
  motionController.recenter();
  if (applyPendingMotionView() && emulator) renderVideo();
});
motionTrackingInput.addEventListener("change", () => {
  motionController.setTrackingMode(motionTrackingInput.value);
  applyPendingMotionView();
  updatePresentation();
});
motionStrengthInput.addEventListener("input", () => {
  motionController.setStrength(Number(motionStrengthInput.value));
  motionStrengthOutput.textContent = `${Number(motionStrengthInput.value)}%`;
});
viewingDistanceInput.addEventListener("input", () => {
  viewingDistanceAdjusted = true;
  updatePresentation();
});
screenProfileInput.addEventListener("change", updatePresentation);
for (const input of [effectsButton, effectsSettingsButton, resetEffectsSettingsButton, motionButton, motionRecenterButton, motionTrackingInput, motionStrengthInput, viewingDistanceInput, screenProfileInput, depthInput, softnessInput, lightDistanceInput, lightElevationInput, lightAzimuthInput, rimSoftnessInput, thicknessInput, rimThicknessInput, rimRecessInput, consoleEdgeInput, rimInput, recessedOpeningInput, rimWidthInput, solidSidesInput, spriteReliefInput, spriteReliefStyle, thinOverlapsInput, dioramaInput]) {
  input.addEventListener("focus", () => {
    heldButtons = 0;
    stopRewind();
    updateButtons();
  });
}
new ResizeObserver(updateDepthGeometry).observe(screenStage);
window.addEventListener("resize", updateDepthGeometry);
window.visualViewport?.addEventListener("resize", updateDepthGeometry);
updateDepthGeometry();

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
  recordButton.disabled = !emulator || active;
  stopRecordButton.disabled = !active;
  recordButton.classList.toggle("recording", active);
}

function startRecording() {
  if (!emulator || recording) return;
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

async function stopRecording() {
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
  recordingStatus.textContent = "Uploading…";
  try {
    const response = await fetch(recordEndpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    const body = await response.text();
    let result = {};
    try { result = JSON.parse(body); } catch (_) {}
    if (!response.ok || !result.ok) throw new Error(result.error || body || `HTTP ${response.status}`);
    recordingStatus.textContent = `Saved · ${result.frames} frames`;
    setStatus(`Recording saved on server as ${result.filename}`);
  } catch (error) {
    reportDebug("record-upload", error);
    recordingStatus.textContent = `Upload failed — retry: ${error?.message ?? error}`;
    setStatus(`Could not upload recording: ${error}`);
  }
}

const audio = new SpatialAudio({
  onState: updateAudioControls,
  onError: (error) => {
    reportDebug("audio", error);
    setStatus(`Audio unavailable: ${error.message}`);
  },
});

function updateAudioControls({ state, muted }) {
  const running = state === "running";
  const label = muted ? "MUTE" : "AUDIO";
  const accessibleLabel = muted ? "Unmute audio" : running ? "Mute audio" : "Enable audio";
  audioButton.dataset.muted = muted ? "true" : "false";
  audioLabel.textContent = label;
  audioButton.title = accessibleLabel;
  audioButton.setAttribute("aria-label", accessibleLabel);
}

// Soundstage controls are shelved; use the original mono mix for playback.
audio.setWidth(0);

function setStatus(message) { status.textContent = message; }

function displayModeLabel() {
  return DISPLAY_MODE_LABELS[emulator?.display_mode?.() ?? 2] ?? "Both";
}

function cycleDisplayMode() {
  if (!emulator) return;
  emulator.cycle_display_mode();
  renderVideo();
  setStatus(`${rewinding ? "Rewinding · " : ""}Frame ${emulator.frame_metadata().frame_number} · ${displayModeLabel()}`);
}

function updateButtons() {
  if (emulator && !rewinding) emulator.set_controller(heldButtons);
}

function press(name) {
  heldButtons |= BUTTONS[name];
  recordInputEvent("button-down", { button: name });
  updateButtons();
}

function release(name) {
  heldButtons &= ~BUTTONS[name];
  recordInputEvent("button-up", { button: name });
  updateButtons();
}

function startRewind() {
  if (!emulator || rewinding) return;
  rewinding = true;
  rewindFrames = 0;
  rewindStartedAt = performance.now();
  recordInputEvent("rewind-start");
  audio.flush();
  accumulator = 0;
  setStatus("Rewinding…");
  reportDebug("rewind-start");
}

function stopRewind() {
  if (!rewinding) return;
  rewinding = false;
  updateButtons();
  previousTime = performance.now();
  accumulator = 0;
  recordInputEvent("rewind-stop");
  reportDebug("rewind-stop");
}

function rewindFrame() {
  if (!emulator) return null;
  const available = emulator.rewind();
  const metadata = drawFrame(false);
  recordFrame("rewind", metadata, { available });
  rewindFrames += 1;
  if (debugEnabled && (rewindFrames === 1 || rewindFrames % 30 === 0 || !available)) {
    reportDebug("rewind-frame", {
      available,
      rewind_frames: rewindFrames,
      elapsed_ms: performance.now() - rewindStartedAt,
      frame_number: metadata ? String(metadata.frame_number) : null,
      audio_samples: metadata?.audio_samples ?? null,
    });
  }
  return metadata;
}

function drawFrame(scheduleAudio = true) {
  if (!emulator) return null;
  const metadata = emulator.step_frame();
  renderVideo();
  setStatus(`${rewinding ? "Rewinding · " : ""}Frame ${metadata.frame_number} · ${displayModeLabel()}`);
  if (scheduleAudio) {
    try {
      audio.schedule(
        emulator.audio_buffer().subarray(0, metadata.audio_samples),
        emulator.audio_stem_buffer().subarray(0, metadata.audio_samples * 5),
        metadata.audio_sample_rate,
        metadata.audio_discontinuity,
      );
    } catch (error) {
      reportDebug("audio-scheduling", error);
      console.error("Audio scheduling failed", error);
      setStatus(`Frame ${metadata.frame_number} · audio unavailable`);
    }
  }
  recordFrame("frame", metadata);
  return metadata;
}

function renderVideo() {
  const showDepth = wantsLayeredVideo() && emulator.display_mode() === 2
    && emulator.frame_metadata().depth_available;
  screenStage.dataset.depth = String(showDepth);
  if (showDepth) {
    const planes = emulator.depth_buffer();
    const planeBytes = WIDTH * HEIGHT * 4;
    const rearSpritePixels = planes.subarray(planeBytes, 2 * planeBytes);
    const tilePixels = planes.subarray(2 * planeBytes, 3 * planeBytes);
    const scene = currentSceneDepth();
    const { separation, thickness: depth } = scene;
    const spriteRelief = spriteReliefInput.checked && solidSidesInput.checked
      ? { mode: spriteReliefStyle.value, softness: resolveShadowSoftness(Number(softnessInput.value)),
        overlapFraction: thinOverlapsInput.checked ? 1 / 3 : 1 }
      : null;
    const rim = currentRimGeometry(scene);
    backdropRenderer.paint(planes.subarray(0, planeBytes), separation);
    if (dioramaInput.checked && rim.enabled) {
      dioramaFrame.paint(backdropRenderer.outputRGBA, separation, resolveShadowSoftness(Number(rimSoftnessInput.value)), rim.thickness, rim.frontDepth);
    }
    for (let plane = 0; plane < 4; plane += 1) {
      const pixels = planes.subarray(plane * planeBytes, (plane + 1) * planeBytes);
      if (plane !== 0) {
        // Relief follows both priority relationships: front sprites over tiles,
        // and tiles over rear sprites. Caster artwork stays fixed and unmodified.
        const relief = plane === 3 ? spriteRelief
          : plane === 2 && spriteRelief ? { ...spriteRelief, receiverRGBA: rearSpritePixels } : null;
        const painted = depthRenderers[plane].paint(pixels, depth,
          plane === 3 ? tilePixels : null, undefined, relief);
        shadowsNeedPaint ||= painted;
      }
    }
    if (shadowsNeedPaint) {
      const alpha = shadowCasterImage.data;
      for (let offset = 3; offset < planeBytes; offset += 4) {
        alpha[offset] = planes[planeBytes + offset] || planes[2 * planeBytes + offset]
          || planes[3 * planeBytes + offset] ? 255 : 0;
      }
      shadowCasterContext.putImageData(shadowCasterImage, 0, 0);
      cardShadowRenderer.paint(shadowCasterCanvas, alpha, {
        frontDepth: 0, backDepth: depth, receiverDepth: separation,
      });
      backgroundShadow.style.opacity = String(cardShadowRenderer.opacity);
      backgroundShadow.style.visibility = separation > 0 && cardShadowRenderer.opacity > 0 ? "visible" : "hidden";
    }
    shadowsNeedPaint = false;
    renderDepthGhosts(rewinding ? new Uint8Array(0) : emulator.ghost_buffer());
    return;
  }
  const rgba = emulator.rgba_buffer();
  image.data.set(rgba);
  if (!rewinding && emulator.display_mode() !== 0) {
    blendGhosts(image.data, emulator.ghost_buffer());
  }
  context.putImageData(image, 0, 0);
}

function renderDepthGhosts(records) {
  // Keep historical ghosts as an overlay: they have no stored priority class.
  ghostImage.data.fill(0);
  for (let offset = 0; offset < records.length; offset += GHOST_RECORD_BYTES) {
    const pixel = (records[offset + 1] * WIDTH + records[offset]) * 4;
    const sourceAlpha = records[offset + 5] / 255;
    const oldAlpha = ghostImage.data[pixel + 3] / 255;
    const alpha = sourceAlpha + oldAlpha * (1 - sourceAlpha);
    if (!alpha) continue;
    for (let component = 0; component < 3; component += 1) {
      ghostImage.data[pixel + component] = Math.round((records[offset + 2 + component] * sourceAlpha
        + ghostImage.data[pixel + component] * oldAlpha * (1 - sourceAlpha)) / alpha);
    }
    ghostImage.data[pixel + 3] = Math.round(alpha * 255);
  }
  ghostContext.putImageData(ghostImage, 0, 0);
}

function blendGhosts(rgba, records) {
  if (records.length === 0) return;
  if (records.length % GHOST_RECORD_BYTES !== 0) {
    throw new Error("invalid ghost record length");
  }

  for (let offset = 0; offset < records.length; offset += GHOST_RECORD_BYTES) {
    const x = records[offset];
    const y = records[offset + 1];
    const opacity = records[offset + 5] / 255;
    const inverse = 1 - opacity;
    const pixel = (y * WIDTH + x) * 4;
    rgba[pixel] = Math.round(records[offset + 2] * opacity + rgba[pixel] * inverse);
    rgba[pixel + 1] = Math.round(records[offset + 3] * opacity + rgba[pixel + 1] * inverse);
    rgba[pixel + 2] = Math.round(records[offset + 4] * opacity + rgba[pixel + 2] * inverse);
  }
}

function tick(now) {
  try {
    if (document.visibilityState !== "visible") {
      previousTime = 0;
      accumulator = 0;
      requestAnimationFrame(tick);
      return;
    }
    const viewChanged = applyPendingMotionView();
    let drewVideo = false;
    if (!previousTime) previousTime = now;
    accumulator += Math.min(now - previousTime, 250);
    previousTime = now;

    if (emulator && rewinding) {
      let frames = 0;
      while (emulator && accumulator >= FRAME_MS && frames < 4) {
        rewindFrame();
        drewVideo = true;
        accumulator -= FRAME_MS;
        frames += 1;
      }
    } else {
      let frames = 0;
      while (emulator && accumulator >= FRAME_MS && frames < 4) {
        drawFrame();
        drewVideo = true;
        accumulator -= FRAME_MS;
        frames += 1;
      }
    }
    if (viewChanged && emulator && !drewVideo) renderVideo();
  } catch (error) {
    reportDebug("emulation-frame", error);
    console.error("Emulation frame failed", error);
    emulator = null;
    savedSnapshot = null;
    rewinding = false;
    snapshotButton.disabled = true;
    restoreButton.disabled = true;
    rewindButton.disabled = true;
    setStatus(`Emulation stopped: ${error?.message ?? error}`);
  }
  requestAnimationFrame(tick);
}

async function loadRom(file, { fromCache = false, expectedGeneration = null } = {}) {
  if (expectedGeneration !== null && expectedGeneration !== romLoadGeneration) return false;
  const generation = ++romLoadGeneration;
  let replacingEmulator = false;
  try {
    setStatus(`${fromCache ? "Restoring" : "Reading"} ${file.name}…`);
    const bytes = new Uint8Array(await file.arrayBuffer());
    await wasmReady;
    if (generation !== romLoadGeneration) return false;
    audio.flush();
    // Layered-render keeps several frame planes in the core. Release the
    // previous Rust allocation before asking WASM to allocate the next ROM;
    // otherwise repeated loads can exhaust the module heap before load_rom
    // gets a chance to return.
    const previousEmulator = emulator;
    replacingEmulator = true;
    emulator = null;
    previousEmulator?.free();
    emulator = NesWeb.load_rom(bytes);
    emulator.set_depth_enabled(wantsLayeredVideo());
    romName = file.name;
    savedSnapshot = null;
    heldButtons = 0;
    snapshotButton.disabled = false;
    restoreButton.disabled = true;
    rewindButton.disabled = false;
    recordButton.disabled = false;
    stopRecordButton.disabled = true;
    recordingStatus.textContent = "Recording is off";
    rewinding = false;
    previousTime = performance.now();
    accumulator = FRAME_MS;
    setStatus(`${file.name} loaded${fromCache ? " from this browser" : ""} · press Enable audio to hear it`);
    if (!fromCache) {
      // Serialize successful loads so a slower write cannot replace a newer
      // ROM. Invalid ROMs never replace the previous cached entry.
      romCacheWrites = romCacheWrites.catch(() => {}).then(() => saveCachedRom(file.name, bytes)).then(result => {
        if (result && !result.ok) reportDebug("rom-cache-write", result.reason);
      }).catch(error => reportDebug("rom-cache-write", error));
    }
    return true;
  } catch (error) {
    if (generation !== romLoadGeneration) return false;
    reportDebug("rom-load", error);
    audio.flush();
    if (replacingEmulator) {
      emulator?.free();
      emulator = null;
      snapshotButton.disabled = true;
      restoreButton.disabled = true;
      rewindButton.disabled = true;
      recordButton.disabled = true;
      stopRecordButton.disabled = true;
    }
    setStatus(`Could not load ROM: ${error}`);
    if (fromCache) {
      romCacheWrites = romCacheWrites.catch(() => {}).then(() => clearCachedRom());
    }
    return false;
  }
}

async function restoreLastRom() {
  // A manual selection may already be waiting for WASM initialization.
  if (romLoadGeneration !== 0) return;
  const generation = romLoadGeneration;
  const cached = await loadCachedRom();
  if (generation !== romLoadGeneration) return;
  if (!cached.ok) {
    reportDebug("rom-cache-read", cached.reason);
    return;
  }
  if (!cached.bytes) return;
  await loadRom({ name: cached.name, arrayBuffer: async () => cached.bytes.buffer }, {
    fromCache: true, expectedGeneration: generation,
  });
}

romInput.addEventListener("change", () => {
  if (romInput.files?.[0]) void loadRom(romInput.files[0]);
});

// Allow selecting the same ROM again after an invalid load or a picker
// cancellation. Android file pickers otherwise may not emit `change`.
romInput.addEventListener("click", () => { romInput.value = ""; });

audioButton.addEventListener("click", async () => {
  try {
    if (audio.context?.state === "running") audio.toggleMute();
    else await audio.enable();
  }
  catch (error) {
    reportDebug("audio", error);
    setStatus(`Audio unavailable: ${error}`);
  }
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState !== "visible") {
    audio.flush();
    previousTime = 0;
    accumulator = 0;
    return;
  }
  if (!audio.context) return;
  // iPad may interrupt/suspend Web Audio when the tab loses focus or the
  // device is locked. Re-resume the existing context when it returns.
  audio.enable().catch((error) => reportDebug("audio-resume", error));
});

snapshotButton.addEventListener("click", () => {
  if (!emulator) return;
  savedSnapshot = emulator.save_snapshot();
  restoreButton.disabled = false;
  setStatus("State saved");
});

restoreButton.addEventListener("click", () => {
  if (!emulator || !savedSnapshot) return;
  emulator.restore_snapshot(savedSnapshot);
  audio.flush();
  accumulator = FRAME_MS;
  setStatus("State restored");
});

rewindButton.addEventListener("pointerdown", (event) => {
  event.preventDefault();
  rewindButton.setPointerCapture(event.pointerId);
  startRewind();
});

recordButton.addEventListener("click", startRecording);
stopRecordButton.addEventListener("click", () => { void stopRecording(); });
for (const eventName of ["pointerup", "pointercancel", "lostpointercapture"]) {
  rewindButton.addEventListener(eventName, (event) => {
    event.preventDefault();
    stopRewind();
  });
}

const keyboard = new Map([
  ["w", "up"], ["a", "left"], ["s", "down"], ["d", "right"],
  ["j", "a"], ["k", "b"], ["z", "a"], ["x", "b"], ["Shift", "select"], ["Enter", "start"],
  ["ArrowUp", "up"], ["ArrowDown", "down"], ["ArrowLeft", "left"], ["ArrowRight", "right"],
]);
window.addEventListener("keydown", (event) => {
  if (event.target instanceof Element && (
    event.target.closest("input, select, textarea")
    || (["Enter", " "].includes(event.key) && event.target.closest("button, summary, a"))
  )) return;
  reportDebug("keydown", event.key);
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key === "r") {
    event.preventDefault();
    startRewind();
    return;
  }
  if (key === "m") {
    event.preventDefault();
    if (!event.repeat) audio.toggleMute();
    return;
  }
  if (key === "v") {
    event.preventDefault();
    if (!event.repeat) cycleDisplayMode();
    return;
  }
  const name = keyboard.get(key);
  if (!name) return;
  event.preventDefault();
  press(name);
});
window.addEventListener("keyup", (event) => {
  if (event.target instanceof Element && (
    event.target.closest("input, select, textarea")
    || (["Enter", " "].includes(event.key) && event.target.closest("button, summary, a"))
  )) return;
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key === "r") {
    event.preventDefault();
    stopRewind();
    return;
  }
  const name = keyboard.get(key);
  if (!name) return;
  event.preventDefault();
  release(name);
});
window.addEventListener("blur", () => {
  heldButtons = 0;
  stopRewind();
  updateButtons();
});

for (const button of document.querySelectorAll("[data-button]")) {
  const name = button.dataset.button;
  button.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    button.setPointerCapture(event.pointerId);
    press(name);
  });
  for (const eventName of ["pointerup", "pointercancel", "lostpointercapture"]) {
    button.addEventListener(eventName, (event) => {
      event.preventDefault();
      release(name);
    });
  }
}

const wasmReady = init("./pkg/nes_wasm_bg.wasm?v=13");
await wasmReady;
requestAnimationFrame(tick);
void restoreLastRom();
