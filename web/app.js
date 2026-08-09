import init, * as wasm from "./pkg/nes_wasm.js";

const NesWeb = wasm.NesWasm ?? wasm.NesWeb;

const debugEnabled = new URLSearchParams(window.location.search).get("debug") === "1";
const debugEndpoint = debugEnabled ? new URL("__debug", document.baseURI).href : null;

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
const status = document.querySelector("#status");
const romInput = document.querySelector("#rom-input");
const audioButton = document.querySelector("#audio-button");
const snapshotButton = document.querySelector("#snapshot-button");
const restoreButton = document.querySelector("#restore-button");
const rewindButton = document.querySelector("#rewind-button");

let emulator = null;
let savedSnapshot = null;
let heldButtons = 0;
let previousTime = 0;
let accumulator = 0;
let rewinding = false;
let rewindFrames = 0;
let rewindStartedAt = 0;

class AudioScheduler {
  constructor() {
    this.context = null;
    this.nextTime = 0;
    this.sources = new Set();
    this.prefillSeconds = 0.05;
  }

  async enable() {
    if (!this.context) {
      this.context = new AudioContext();
      this.nextTime = this.context.currentTime + this.prefillSeconds;
    }
    await this.context.resume();
    audioButton.textContent = "Audio enabled";
  }

  flush() {
    if (!this.context) return;
    for (const source of this.sources) {
      try { source.stop(); } catch (_) { /* already stopped */ }
    }
    this.sources.clear();
    // Re-prefill after a discontinuity so the output device does not start
    // consuming the first post-rewind block immediately.
    this.nextTime = this.context.currentTime + this.prefillSeconds;
  }

  schedule(samples, sampleRate, discontinuity) {
    if (discontinuity) this.flush();
    if (!this.context || samples.length === 0) return;

    // The core emits 48 kHz, while a browser AudioContext may run at 44.1 kHz
    // or another device-selected rate. Resample explicitly so the scheduled
    // duration tracks the actual output clock on every browser.
    const outputSamples = sampleRate === this.context.sampleRate
      ? samples
      : resample(samples, sampleRate, this.context.sampleRate);
    const buffer = this.context.createBuffer(1, outputSamples.length, this.context.sampleRate);
    buffer.copyToChannel(outputSamples, 0);
    const source = this.context.createBufferSource();
    source.buffer = buffer;
    source.connect(this.context.destination);
    const start = Math.max(this.nextTime, this.context.currentTime + 0.01);
    source.start(start);
    this.nextTime = start + outputSamples.length / this.context.sampleRate;
    this.sources.add(source);
    source.addEventListener("ended", () => this.sources.delete(source), { once: true });
  }
}

function resample(samples, sourceRate, targetRate) {
  const length = Math.max(1, Math.round(samples.length * targetRate / sourceRate));
  const output = new Float32Array(length);
  const ratio = sourceRate / targetRate;
  for (let index = 0; index < length; index += 1) {
    const position = index * ratio;
    const left = Math.floor(position);
    const right = Math.min(left + 1, samples.length - 1);
    const fraction = position - left;
    output[index] = samples[left] * (1 - fraction) + samples[right] * fraction;
  }
  return output;
}

const audio = new AudioScheduler();

function setStatus(message) { status.textContent = message; }

function updateButtons() {
  if (emulator && !rewinding) emulator.set_controller(heldButtons);
}

function press(name) {
  heldButtons |= BUTTONS[name];
  updateButtons();
}

function release(name) {
  heldButtons &= ~BUTTONS[name];
  updateButtons();
}

function startRewind() {
  if (!emulator || rewinding) return;
  rewinding = true;
  rewindFrames = 0;
  rewindStartedAt = performance.now();
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
  reportDebug("rewind-stop");
}

function rewindFrame() {
  if (!emulator) return null;
  const available = emulator.rewind();
  const metadata = drawFrame(false);
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
  const rgba = emulator.rgba_buffer();
  image.data.set(rgba);
  context.putImageData(image, 0, 0);
  setStatus(`${rewinding ? "Rewinding · " : ""}Frame ${metadata.frame_number}`);
  if (scheduleAudio) {
    try {
      audio.schedule(emulator.audio_buffer().subarray(0, metadata.audio_samples), metadata.audio_sample_rate, metadata.audio_discontinuity);
    } catch (error) {
      reportDebug("audio-scheduling", error);
      console.error("Audio scheduling failed", error);
      setStatus(`Frame ${metadata.frame_number} · audio unavailable`);
    }
  }
  return metadata;
}

function tick(now) {
  try {
    if (!previousTime) previousTime = now;
    accumulator += Math.min(now - previousTime, 250);
    previousTime = now;

    if (emulator && rewinding) {
      let frames = 0;
      while (emulator && accumulator >= FRAME_MS && frames < 4) {
        rewindFrame();
        accumulator -= FRAME_MS;
        frames += 1;
      }
    } else {
      let frames = 0;
      while (emulator && accumulator >= FRAME_MS && frames < 4) {
        drawFrame();
        accumulator -= FRAME_MS;
        frames += 1;
      }
    }
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

async function loadRom(file) {
  try {
    setStatus(`Reading ${file.name}…`);
    audio.flush();
    const bytes = new Uint8Array(await file.arrayBuffer());
    emulator = NesWeb.load_rom(bytes);
    savedSnapshot = null;
    heldButtons = 0;
    snapshotButton.disabled = false;
    restoreButton.disabled = true;
    rewindButton.disabled = false;
    rewinding = false;
    previousTime = performance.now();
    accumulator = FRAME_MS;
    setStatus(`${file.name} loaded · press Enable audio to hear it`);
  } catch (error) {
    reportDebug("rom-load", error);
    emulator = null;
    setStatus(`Could not load ROM: ${error}`);
  }
}

romInput.addEventListener("change", () => {
  if (romInput.files?.[0]) void loadRom(romInput.files[0]);
});

// Allow selecting the same ROM again after an invalid load or a picker
// cancellation. Android file pickers otherwise may not emit `change`.
romInput.addEventListener("click", () => { romInput.value = ""; });

audioButton.addEventListener("click", async () => {
  try { await audio.enable(); }
  catch (error) {
    reportDebug("audio", error);
    setStatus(`Audio unavailable: ${error}`);
  }
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
for (const eventName of ["pointerup", "pointercancel", "lostpointercapture"]) {
  rewindButton.addEventListener(eventName, (event) => {
    event.preventDefault();
    stopRewind();
  });
}

const keyboard = new Map([
  ["z", "a"], ["x", "b"], ["Shift", "select"], ["Enter", "start"],
  ["ArrowUp", "up"], ["ArrowDown", "down"], ["ArrowLeft", "left"], ["ArrowRight", "right"],
]);
window.addEventListener("keydown", (event) => {
  reportDebug("keydown", event.key);
  if (event.key.toLowerCase() === "r") {
    event.preventDefault();
    startRewind();
    return;
  }
  const name = keyboard.get(event.key);
  if (!name) return;
  event.preventDefault();
  press(name);
});
window.addEventListener("keyup", (event) => {
  if (event.key.toLowerCase() === "r") {
    event.preventDefault();
    stopRewind();
    return;
  }
  const name = keyboard.get(event.key);
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

await init("./pkg/nes_wasm_bg.wasm?v=10");
requestAnimationFrame(tick);
