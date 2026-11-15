// Explicit preview diagnostics only. Never includes game pixels, ROMs, filenames,
// controller events, page contents, cookies or URLs. Normal visits send nothing.
(() => {
  const enabled = new URLSearchParams(location.search).get("diagnostics") === "1";
  if (!enabled) return;
  const session = "vr-" + crypto.getRandomValues(new Uint32Array(1))[0].toString(16);
  const started = performance.now();
  let count = 0, lastTick = started, phase = "bootstrap", last = {};
  const note = document.getElementById("diagnostics-note");
  if (note) { note.hidden = false; note.textContent = "Preview diagnostics on · graphics timings and errors only · stops after 10 minutes"; }
  const report = (next, details = {}, failure = false) => {
    phase = next;
    if (count >= 60 || performance.now() - started > 600000) return;
    count++;
    const clean = { name: session, phase, library: "shared-scene-v4", frameMs: performance.now() - started };
    for (const key of ["frameMs", "framesRendered", "triangles", "drawCalls", "geometries", "textures"]) if (Number.isFinite(details[key])) clean[key] = details[key];
    for (const key of ["backend", "reason", "message", "webglVersion", "userAgent"]) if (typeof details[key] === "string") clean[key] = details[key].replace(/https?:\/\/\S+/g, "[url]").slice(0, 500);
    if (details.gpu && typeof details.gpu === "object") clean.gpu = details.gpu;
    if (details.screen) clean.screen = details.screen;
    if (details.geometryBuilds) clean.geometryBuilds = details.geometryBuilds.slice(0, 8);
    if (typeof details.contextLost === "boolean") clean.contextLost = details.contextLost;
    last = { ...last, ...clean };
    fetch(new URL("./__graphics-log", location.href), { method: "POST", credentials: "omit", mode: "same-origin", keepalive: true,
      headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind: failure ? "failure" : "ready", details: clean }) }).catch(() => {});
  };
  window.nesVRReport = report;
  report("bootstrap", { backend: "boot", userAgent: navigator.userAgent, screen: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio } });
  let lastError = "", lastErrorAt = -Infinity;
  const error = message => { const now = performance.now(); if (message === lastError && now - lastErrorAt < 5000) return; lastError = message; lastErrorAt = now; report(phase, { message }, true); };
  window.addEventListener("error", event => error(event.message || "Script resource error"));
  window.addEventListener("unhandledrejection", event => error(event.reason?.message || "Unhandled promise rejection"));
  const timer = setInterval(() => {
    const now = performance.now(), lag = now - lastTick - 15000; lastTick = now;
    if (now - started > 600000 || count >= 60) { clearInterval(timer); return; }
    report(phase, { framesRendered: last.framesRendered ?? 0, frameMs: lag, reason: document.hidden ? "background-heartbeat" : "heartbeat", ...window.nesVRRuntimeState?.() }, lag > 1000);
  }, 15000);
})();
