import * as THREE from "./vendor/three/three.module.min.js?v=3";
import { readXRButtons } from "./vr-geometry.js?v=2";
import { createPreviewScene } from "./look-presets.js";

// One GPU scene serves the fixed-camera, orbiting and headset views. The host
// owns the sole emulator, audio context, input combiner and frame scheduler.
export function createDioramaView(host) {
  const $ = id => document.getElementById(id === "status" ? "diorama-status" : id);
  const WIDTH = 256, HEIGHT = 240, BYTES = WIDTH * HEIGHT * 4;
  const BUTTONS = host.buttons;
  const stage = $("stage"), status = $("status"), vrButton = $("enter-vr");
  const report = (...args) => window.nesVRReport?.(...args);
  const STAGE_STORAGE_KEY = "nes.vr-stage.v1";
  const stageControls = ["size", "gap", "thickness", "vr-resolution", "shadows", "material-finish-input"];
  let previewValues = null, previewRevision = 0, modeRevision = 0;
  const stageValue = id => previewValues?.[id] ?? $(id).value;
  function pressedLayers() { $("size").value = "140"; $("gap").value = "0"; $("thickness").value = "30"; }
  function miniatureRelief() { $("size").value = "70"; $("gap").value = "4"; $("thickness").value = "30"; }
  function saveStageSettings() {
    try { localStorage.setItem(STAGE_STORAGE_KEY, JSON.stringify(Object.fromEntries(stageControls.map(id => [id, $(id).type === "checkbox" ? $(id).checked : $(id).value])))); }
    catch (_) { /* Settings remain usable when browser storage is unavailable. */ }
    // A starting-point URL must not override later manual edits on reload.
    const url = new URL(location.href);
    if (url.searchParams.has("look")) { url.searchParams.delete("look"); history.replaceState(null, "", url); }
  }
  try {
    const saved = JSON.parse(localStorage.getItem(STAGE_STORAGE_KEY));
    if (saved && typeof saved === "object") for (const id of stageControls) {
      const control = $(id), value = saved[id];
      if (control.type === "checkbox") { if (typeof value === "boolean") control.checked = value; }
      else if (typeof value === "string" && value !== "" && (control.tagName === "SELECT"
        ? [...control.options].some(option => option.value === value)
        : Number.isFinite(Number(value)) && Number(value) >= Number(control.min) && Number(value) <= Number(control.max)
          && (Number(value) - Number(control.min)) % Number(control.step) === 0)) control.value = value;
    }
  } catch (_) { /* Ignore malformed or unavailable preferences. */ }
  if (new URLSearchParams(location.search).get("look") === "pressed") pressedLayers();
  if (new URLSearchParams(location.search).get("look") === "miniature") miniatureRelief();
  let renderer;
  report("webgl-create-start");
  try { renderer = new THREE.WebGLRenderer({ canvas: stage, antialias: false, alpha: false, powerPreference: "high-performance" }); }
  catch (error) { $("fatal").hidden = false; $("fatal").textContent = "The 3D views need WebGL 2. Try a browser with hardware acceleration enabled. " + error.message; }
  if (!renderer) throw new Error($("fatal").textContent);
  return start();

  function start() {
    renderer.setPixelRatio(Math.min(devicePixelRatio, 1.25));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.NoToneMapping;
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    renderer.shadowMap.autoUpdate = false;
    renderer.xr.enabled = true;
    // Explicit compatibility policy: the classic WebXR base layer is the default.
    // Projection layers remain available for controlled comparison via ?xr=layers.
    renderer.xr.useProjectionLayers = new URLSearchParams(location.search).get("xr") === "layers";
    renderer.xr.setReferenceSpaceType("local");
    renderer.xr.setFramebufferScaleFactor(Number($("vr-resolution").value));
    const gl = renderer.getContext(), gpuInfo = gl.getExtension("WEBGL_debug_renderer_info");
    report("webgl-created", { gpu: { renderer: gl.getParameter(gpuInfo?.UNMASKED_RENDERER_WEBGL ?? gl.RENDERER),
      vendor: gl.getParameter(gpuInfo?.UNMASKED_VENDOR_WEBGL ?? gl.VENDOR), version: gl.getParameter(gl.VERSION) },
      screen: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio } });
    if (window.nesVRReport) {
      if (typeof gl.makeXRCompatible === "function") {
        const makeCompatible = gl.makeXRCompatible.bind(gl);
        gl.makeXRCompatible = async () => { report("xr-context-start"); await makeCompatible(); report("xr-context-ready"); };
      }
      renderer.debug.onShaderError = (context, program) => report("shader-error", { message: context.getProgramInfoLog(program) || "Shader error" }, true);
    }

    const scene = new THREE.Scene();
    scene.background = new THREE.Color("#202a2c");
    const camera = new THREE.PerspectiveCamera(42, 1, 0.025, 30);
    const world = new THREE.Group();
    scene.add(world);
    const diorama = new THREE.Group();
    world.add(diorama);
    const fill = new THREE.HemisphereLight(0xf3f5ef, 0xa4b6b0, 1.9);
    scene.add(fill);
    const key = new THREE.DirectionalLight(0xfff3df, 1.15);
    key.position.set(-0.6, 0.6, 3);
    key.castShadow = true;
    key.shadow.mapSize.set(512, 512);
    key.shadow.camera.left = key.shadow.camera.bottom = -0.9;
    key.shadow.camera.right = key.shadow.camera.top = 0.9;
    key.shadow.camera.near = 0.1; key.shadow.camera.far = 5;
    key.shadow.bias = -0.0003; key.shadow.normalBias = 0.003;
    key.shadow.intensity = $("shadows").checked ? 0.55 : 0;
    diorama.add(key, key.target);
    const plane = new THREE.PlaneGeometry(1, HEIGHT / WIDTH);
    const backdropPlane = plane.clone();
    const layers = [];
    const bodyMaterial = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 1, metalness: 0 });
    for (let index = 0; index < 4; index++) {
      const pixels = new Uint8Array(BYTES);
      const texture = new THREE.DataTexture(pixels, WIDTH, HEIGHT, THREE.RGBAFormat);
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.magFilter = texture.minFilter = THREE.NearestFilter;
      texture.generateMipmaps = false; texture.flipY = true;
      const material = new THREE.MeshStandardMaterial({ map: texture, alphaTest: 0.05, roughness: 1, metalness: 0, side: THREE.DoubleSide });
      const front = new THREE.Mesh(index === 0 ? backdropPlane : plane, material), back = new THREE.Mesh(plane, material);
      const sides = new THREE.Mesh(new THREE.BufferGeometry(), bodyMaterial);
      for (const mesh of [front, back, sides]) { mesh.castShadow = index !== 0; mesh.receiveShadow = true; diorama.add(mesh); }
      layers.push({ pixels, texture, front, back, sides, initialized: false });
    }
    const paper = new THREE.MeshStandardMaterial({ color: 0xe1d2b5, roughness: 1 });
    const backing = new THREE.Mesh(new THREE.BoxGeometry(1.025, HEIGHT / WIDTH + 0.025, 0.008), paper);
    const plinth = new THREE.Mesh(new THREE.BoxGeometry(1.13, 0.035, 0.58), new THREE.MeshStandardMaterial({ color: 0xb7ad91, roughness: 1 }));
    backing.receiveShadow = plinth.receiveShadow = true;
    diorama.add(backing, plinth);
    const fpsCanvas = document.createElement("canvas"); fpsCanvas.width = 768; fpsCanvas.height = 64;
    const fpsContext = fpsCanvas.getContext("2d");
    const fpsTexture = new THREE.CanvasTexture(fpsCanvas); fpsTexture.colorSpace = THREE.SRGBColorSpace;
    const fpsHud = new THREE.Mesh(new THREE.PlaneGeometry(0.7, 0.7 / 12), new THREE.MeshBasicMaterial({ map: fpsTexture, transparent: true, depthTest: false, toneMapped: false }));
    fpsHud.position.set(0, -HEIGHT / WIDTH / 2 - 0.09, 0.04); fpsHud.visible = false;
    diorama.add(fpsHud);

    let source = createPreviewScene();
    let active = false, mode = "depth", previewing = false;
    let pendingPlacement = false, sessionBusy = false;
    let geometryDirty = true, xrRecenterHeld = false, orbit = { yaw: -0.28, pitch: 0.12, distance: 1.65, lookZ: -0.08 };
    let renderRequested = true, shadowsDirty = true, shadersReady = false, framesRendered = 0;
    let meshBusy = false, meshEpoch = 0, forceMesh = true, meshSpare = null, lastMeshMs = 0, lastRenderMs = 0;
    let layoutKey = "", framesEmulated = 0, sceneFrames = 0;
    let statsStarted = performance.now(), statsRendered = 0, statsEmulated = 0, statsScene = 0, lastStatsReport = 0;
    let xrRevealStarted = null, xrRevealing = false;
    let showFPS = false;
    let fps = { rendered: 0, emulated: 0, scene: 0 };
    let nativeXRRequests = 0, nativeXRFrames = 0, appXRFrames = 0;
    let xrWaitTimer = null, exitSessionPromise = null, setupSessionPromise = null;
    function exitVR() {
      if (exitSessionPromise) return exitSessionPromise;
      if (!setupSessionPromise && !renderer.xr.getSession()) return Promise.resolve();
      const setup = setupSessionPromise;
      exitSessionPromise = (async () => {
        // Let Three finish compatibility/reference-space setup before ending
        // its native session; otherwise late setup can reactivate a stale view.
        await setup?.catch(() => {});
        const session = renderer.xr.getSession();
        if (session) await session.end();
      })().finally(() => { exitSessionPromise = null; });
      return exitSessionPromise;
    }
    let sizingHeld = false, lastSizeStep = -Infinity;
    let previousOrbit = null;
    const audio = host.audio, input = host.input;
    const vertexCounts = [];
    function runtimeState() {
      const session = renderer.xr.getSession();
      return { reason: `doc:${document.visibilityState} hidden:${document.hidden} xr:${session?.visibilityState ?? "none"} native:${nativeXRFrames} app:${appXRFrames} requests:${nativeXRRequests} base:${Boolean(session?.renderState.baseLayer)} layers:${Array.from(session?.renderState.layers ?? []).map(layer => layer.constructor.name).join(",")}`,
        backend: renderer.xr.useProjectionLayers ? "projection-layers" : "classic-base-layer", framesRendered };
    }
    if (window.nesVRReport) window.nesVRRuntimeState = runtimeState;
    const mesher = new Worker(new URL("./vr-mesh-worker.js?v=3", import.meta.url), { type: "module" });
    mesher.onmessage = ({ data }) => {
      meshBusy = false;
      if (data.pixels?.length === source.length && data.pixels.byteOffset === 0
          && data.pixels.byteLength === data.pixels.buffer.byteLength) meshSpare = data.pixels;
      if (data.epoch !== meshEpoch) { geometryDirty = true; return; }
      if (data.error) { status.textContent = data.error; report("mesh-failed", { message: data.error }, true); return; }
      lastMeshMs = data.buildMs;
      const firstScene = !layers[0].initialized;
      // Cached older workers may not have the mask. Keep the old exact
      // byte compare as a correctness fallback across mixed asset versions.
      const textureMask = Number.isInteger(data.textureMask) ? data.textureMask : null;
      let textureChanged = false;
      for (let index = 0; index < 4; index++) {
        const layer = layers[index], pixels = data.pixels.subarray(index * BYTES, (index + 1) * BYTES);
        let changed = !layer.initialized || (textureMask !== null && (textureMask & (1 << index)) !== 0);
        if (textureMask === null && !changed) for (let i = 0; i < BYTES; i++) if (pixels[i] !== layer.pixels[i]) { changed = true; break; }
        if (changed) { layer.pixels.set(pixels); layer.texture.needsUpdate = true; layer.initialized = textureChanged = true; }
      }
      for (const mesh of data.meshes) {
        const geometry = new THREE.BufferGeometry();
        for (const [attribute, values] of [["position", mesh.positions], ["normal", mesh.normals], ["color", mesh.colors]]) geometry.setAttribute(attribute, new THREE.BufferAttribute(values, 3));
        // All native silhouettes fit this fixed bound. Avoid rescanning hundreds
        // of thousands of vertices on the UI thread after worker completion.
        geometry.boundingSphere = new THREE.Sphere(new THREE.Vector3(0, 0, -0.125), 0.8);
        layers[mesh.index].sides.geometry.dispose(); layers[mesh.index].sides.geometry = geometry;
        vertexCounts[mesh.index] = mesh.positions.length / 3;
      }
      stage.dataset.vertices = vertexCounts.reduce((sum, n) => sum + n, 0);
      stage.dataset.ready = "true";
      sceneFrames++;
      if (textureChanged || data.meshes.length) renderRequested = true;
      // Layer zero only receives shadows. A changed caster geometry may
      // include color-only tessellation changes; conservatively redraw its
      // shadow as before. Exact no-change replies need neither kind of draw.
      if (data.meshes.length) shadowsDirty = true;
      if (firstScene && shadersReady) checkVR();
    };
    mesher.onerror = event => { meshBusy = false; report("mesh-worker-error", { message: event.message }, true); status.textContent = "Could not build edges. Reload the player to try again."; };

    function updateStage(force = false) {
      if (force) { meshEpoch++; forceMesh = true; }
      const size = Number(stageValue("size")) / 100;
      const gap = Number(stageValue("gap")) / 100 / size;
      const thickness = Number(stageValue("thickness")) / 1000 / size;
      // Zero air gap must still leave the receiver behind every rear cap.
      const receiverDepth = Math.max(gap, thickness + 0.003 / size);
      diorama.scale.setScalar(size);
      key.shadow.camera.left = key.shadow.camera.bottom = -0.9 * size;
      key.shadow.camera.right = key.shadow.camera.top = 0.9 * size;
      key.shadow.camera.near = 0.1 * size; key.shadow.camera.far = 5 * size;
      key.shadow.camera.updateProjectionMatrix();
      key.shadow.bias = -0.0003 * 0.9 / size;
      // Keep the counter readable when the stage fills the room.
      fpsHud.scale.setScalar(1 / size);
      fpsHud.position.set(0, -Math.min(HEIGHT / WIDTH / 2 * size + 0.09, 0.55) / size, 0.04 / size);
      if (!previewing) {
        $("size-value").value = `${$("size").value} cm`;
        $("gap-value").value = `${$("gap").value} cm`;
        $("thickness-value").value = `${$("thickness").value} mm`;
      }
      backing.position.z = -receiverDepth - 0.005;
      plinth.position.set(0, -HEIGHT / WIDTH / 2 - 0.027, -receiverDepth / 2);
      plinth.scale.z = Math.max(0.2, (receiverDepth + 0.16) / 0.58);
      for (let index = 0; index < layers.length; index++) {
        const layer = layers[index];
        const z = index === 0 ? -receiverDepth : (index - 2) * 0.0015 / size;
        layer.front.position.z = z;
        layer.back.position.z = z - (index === 0 ? 0.001 : thickness);
        layer.sides.position.z = z;
        layer.back.visible = index === 0 || thickness > 0;
        layer.sides.visible = index !== 0 && thickness > 0;
      }
      const nextLayout = [size, gap, thickness].join(":");
      if (nextLayout !== layoutKey) { updateViewPresentation(); renderRequested = shadowsDirty = true; layoutKey = nextLayout; }
      if (meshBusy) return;
      const pixels = meshSpare ?? new Uint8Array(source.length);
      pixels.set(source); meshSpare = null;
      mesher.postMessage({ epoch: meshEpoch, pixels, thickness, force: forceMesh }, [pixels.buffer]);
      meshBusy = true; forceMesh = false; geometryDirty = false;
    }

    function refresh(reset = false) {
      const emulator = host.getEmulator();
      if (reset) { meshEpoch++; forceMesh = true; }
      if (emulator) {
        const metadata = emulator.frame_metadata();
        if (metadata.depth_available && emulator.display_mode() === 2) source.set(emulator.depth_buffer());
        else { source.fill(0); source.set(emulator.rgba_buffer()); }
        metadata.free?.();
      } else source = createPreviewScene();
      stage.dataset.source = emulator ? "rom" : "sample";
      geometryDirty = true;
    }
    function updateStageSettings(persist = true) { if (persist) { previewRevision++; saveStageSettings(); } geometryDirty = true; updateStage(true); if (!renderer.xr.isPresenting) desktopCamera(); }
    for (const id of ["size", "gap", "thickness"]) $(id).addEventListener("input", updateStageSettings);
    $("vr-resolution").addEventListener("change", () => { saveStageSettings(); if (!renderer.xr.isPresenting) renderer.xr.setFramebufferScaleFactor(Number($("vr-resolution").value)); });
    $("shadows").addEventListener("change", () => { saveStageSettings(); key.shadow.intensity = $("shadows").checked ? 0.55 : 0; renderRequested = shadowsDirty = true; });

    function updateMaterial() {
      const finish = $("material-finish-input").value;
      const roughness = { matte: 1, eggshell: 0.82, satin: 0.58 }[finish] ?? 1;
      bodyMaterial.roughness = roughness;
      for (const layer of layers) layer.front.material.roughness = roughness;
      renderRequested = true;
    }
    $("material-finish-input").addEventListener("change", () => { previewRevision++; saveStageSettings(); updateMaterial(); });
    updateMaterial();
    let tilt = { x: 0, y: 0 }, cameraTransition = null, cameraLookZ = 0;
    const receiverPlane = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
    const backgroundRay = new THREE.Raycaster(), corner = new THREE.Vector2(), intersection = new THREE.Vector3();
    let presentationKey = "";
    function updateViewPresentation() {
      const layered = mode === "depth" && !previewing && !renderer.xr.isPresenting;
      backing.visible = plinth.visible = !layered;
      let halfWidth = 0.5, halfHeight = HEIGHT / WIDTH / 2;
      if (layered) {
        // Extend the existing receiver, with clamped edge texels. Its native
        // central UVs, material, lighting and shadows remain the same, so no
        // sampled clear-color seam appears around the game rectangle.
        camera.updateMatrixWorld();
        receiverPlane.constant = -layers[0].front.position.z * diorama.scale.x;
        // Portrait keeps the empty space above/below the native game. Only
        // extend sideways; project the gameplay height onto this receiver.
        let gameTop = -1, gameBottom = 1;
        for (const x of [-0.5, 0.5]) for (const y of [-HEIGHT / WIDTH / 2, HEIGHT / WIDTH / 2]) {
          intersection.set(x * diorama.scale.x, y * diorama.scale.x, 0.0015).project(camera);
          gameTop = Math.max(gameTop, intersection.y); gameBottom = Math.min(gameBottom, intersection.y);
        }
        for (const x of [-1, 1]) for (const y of [Math.max(-1, gameBottom), Math.min(1, gameTop)]) {
          corner.set(x, y); backgroundRay.setFromCamera(corner, camera);
          if (backgroundRay.ray.intersectPlane(receiverPlane, intersection)) {
            halfWidth = Math.max(halfWidth, Math.abs(intersection.x) / diorama.scale.x);
            halfHeight = Math.max(halfHeight, Math.abs(intersection.y) / diorama.scale.x);
          }
        }
      }
      const nextKey = [layered, halfWidth, halfHeight].join(":");
      if (presentationKey === nextKey) return;
      presentationKey = nextKey;
      scene.background.set(layered ? "#111217" : "#202a2c");
      const positions = backdropPlane.attributes.position, uvs = backdropPlane.attributes.uv;
      for (let i = 0; i < positions.count; i++) {
        const x = Math.sign(positions.getX(i)) * halfWidth, y = Math.sign(positions.getY(i)) * halfHeight;
        positions.setXYZ(i, x, y, 0); uvs.setXY(i, x + 0.5, y / (HEIGHT / WIDTH) + 0.5);
      }
      positions.needsUpdate = uvs.needsUpdate = true;
      backdropPlane.computeBoundingSphere();
      renderRequested = shadowsDirty = true;
    }
    function currentCameraPose() {
      const distance = camera.position.length();
      return { yaw: Math.atan2(camera.position.x, camera.position.z), pitch: distance ? Math.asin(Math.max(-1, Math.min(1, camera.position.y / distance))) : 0, distance, lookZ: cameraLookZ };
    }
    function applyCameraPose(pose) {
      camera.position.set(Math.sin(pose.yaw) * Math.cos(pose.pitch) * pose.distance,
        Math.sin(pose.pitch) * pose.distance, Math.cos(pose.yaw) * Math.cos(pose.pitch) * pose.distance);
      cameraLookZ = pose.lookZ; camera.lookAt(0, 0, pose.lookZ);
      renderRequested = true; updateViewPresentation();
    }
    function animateCamera(time) {
      if (!cameraTransition) return;
      const progress = Math.max(0, Math.min(1, (time - cameraTransition.started) / 420));
      const eased = progress * progress * (3 - 2 * progress);
      const pose = {};
      for (const key of ["yaw", "pitch", "distance", "lookZ"]) pose[key] = cameraTransition.from[key] + (cameraTransition.to[key] - cameraTransition.from[key]) * eased;
      if (progress === 1) cameraTransition = null;
      applyCameraPose(pose);
    }
    function desktopCamera(animate = false) {
      const size = Number(stageValue("size")) / 100;
      const layered = mode === "depth" && !previewing;
      const fitDistance = layered
        // Fit the closest gameplay face exactly, keeping the NES aspect ratio.
        ? 0.0015 + size * Math.max(HEIGHT / WIDTH, 1 / camera.aspect) / (2 * Math.tan(camera.fov * Math.PI / 360))
        : size / (2 * Math.tan(camera.fov * Math.PI / 360) * Math.min(1, camera.aspect) * 0.82);
      const distance = fitDistance * (layered || previewing ? 1 : orbit.distance / 1.65);
      const view = previewing ? { yaw: -0.28, pitch: 0.12, distance: 1.65 } : orbit;
      const yaw = previewing || mode === "orbit" ? view.yaw : tilt.x * 0.5;
      const pitch = previewing || mode === "orbit" ? view.pitch : -tilt.y * 0.5;
      const target = { yaw, pitch, distance, lookZ: layered ? 0 : (orbit.lookZ ?? -0.08) };
      if (animate && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
        cameraTransition = { from: currentCameraPose(), to: target, started: performance.now() };
        applyCameraPose(cameraTransition.from);
      } else { cameraTransition = null; applyCameraPose(target); }
    }
    function adoptCurrentOrbit() {
      if (!cameraTransition || mode !== "orbit") return;
      const pose = currentCameraPose(); cameraTransition = null;
      const fit = Number(stageValue("size")) / 100 / (2 * Math.tan(camera.fov * Math.PI / 360) * Math.min(1, camera.aspect) * 0.82);
      orbit = { ...pose, distance: pose.distance / fit * 1.65 };
    }
    function animateXRReveal(time) {
      if (!xrRevealing) return;
      xrRevealStarted ??= time;
      const progress = Math.max(0, Math.min(1, (time - xrRevealStarted) / 180));
      fill.intensity = 1.9 * progress; key.intensity = 1.15 * progress;
      scene.background.set("#202a2c").multiplyScalar(progress); fpsHud.material.opacity = progress;
      if (progress === 1) xrRevealing = false;
    }
    function recenter() {
      resetOrbitGesture();
      if (renderer.xr.isPresenting) pendingPlacement = true;
      else { orbit = { yaw: 0, pitch: 0, distance: 1.65, lookZ: -0.08 }; desktopCamera(true); }
    }
    $("recenter").addEventListener("click", recenter);
    const orbitPointers = new Map();
    let drag = null, pinch = null;
    const zoomDistance = value => Math.max(0.8, Math.min(3.5, value));
    function resetOrbitGesture() { orbitPointers.clear(); drag = pinch = null; }
    function rebaseOrbitGesture() {
      const points = [...orbitPointers.values()];
      drag = points.length === 1 ? { ...points[0] } : null;
      pinch = points.length >= 2 ? { span: Math.max(1, Math.hypot(points[1].x - points[0].x, points[1].y - points[0].y)), distance: orbit.distance } : null;
    }
    stage.addEventListener("pointerdown", event => {
      if (event.button !== 0 || mode !== "orbit" || renderer.xr.isPresenting || previewing) return;
      event.preventDefault(); adoptCurrentOrbit();
      orbitPointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      stage.setPointerCapture(event.pointerId); rebaseOrbitGesture();
    });
    stage.addEventListener("pointermove", event => {
      if (!orbitPointers.has(event.pointerId) || mode !== "orbit" || renderer.xr.isPresenting || previewing) return;
      event.preventDefault();
      orbitPointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (pinch) {
        const [a, b] = orbitPointers.values();
        const span = Math.max(1, Math.hypot(b.x - a.x, b.y - a.y));
        orbit.distance = zoomDistance(pinch.distance * pinch.span / span);
      } else if (drag) {
        orbit.yaw = Math.max(-1.45, Math.min(1.45, orbit.yaw - (event.clientX - drag.x) * 0.007));
        orbit.pitch = Math.max(-0.75, Math.min(0.9, orbit.pitch + (event.clientY - drag.y) * 0.007));
        drag = { x: event.clientX, y: event.clientY };
      }
      desktopCamera();
    });
    for (const name of ["pointerup", "pointercancel", "lostpointercapture"]) stage.addEventListener(name, event => {
      if (!orbitPointers.delete(event.pointerId)) return;
      rebaseOrbitGesture();
    });
    stage.addEventListener("wheel", event => {
      if (mode !== "orbit" || renderer.xr.isPresenting || previewing) return;
      event.preventDefault(); adoptCurrentOrbit(); orbit.distance = zoomDistance(orbit.distance + event.deltaY * 0.001); desktopCamera();
    }, { passive: false });
    stage.addEventListener("contextmenu", event => event.preventDefault());
    function clearInput() { resetOrbitGesture(); host.clearInput(); if (sizingHeld) { sizingHeld = false; saveStageSettings(); } }
    window.addEventListener("blur", clearInput);
    document.addEventListener("visibilitychange", () => { if (active) clearInput(); });
    function setVRLabel(text) { vrButton.setAttribute("aria-label", text); vrButton.title = text; }

    async function checkVR() {
      if (renderer.xr.isPresenting) { setVRLabel("Exit VR"); vrButton.disabled = false; return; }
      if (sessionBusy || previewing) return;
      vrButton.disabled = true;
      if (!isSecureContext) { setVRLabel("VR needs HTTPS"); return; }
      try {
        const supported = Boolean(navigator.xr && await navigator.xr.isSessionSupported("immersive-vr"));
        if (renderer.xr.isPresenting) { setVRLabel("Exit VR"); vrButton.disabled = false; return; }
        if (sessionBusy || previewing) return;
        const ready = shadersReady && stage.dataset.ready === "true";
        setVRLabel(supported ? ready ? "Enter VR" : "Preparing 3D…" : "VR headset unavailable");
        vrButton.disabled = !supported || !ready;
        vrButton.title = supported ? "Open this diorama in your headset" : "Connect your headset and start its VR runtime, then reload. Desktop orbit still works.";
      } catch (_) { setVRLabel("VR unavailable"); }
    }
    navigator.xr?.addEventListener("devicechange", checkVR);
    vrButton.addEventListener("click", async () => {
      if (sessionBusy) return;
      const existing = renderer.xr.getSession();
      if (existing) { await exitVR(); return; }
      sessionBusy = true; vrButton.disabled = true;
      let session;
      try {
        const activation = host.activateForVR();
        renderer.xr.setFramebufferScaleFactor(Number($("vr-resolution").value));
        report("vr-request-start", { backend: $("vr-resolution").value });
        // Request directly within the user gesture; no auto-entry or sensor prompts.
        const granted = navigator.xr.requestSession("immersive-vr", { optionalFeatures: ["local-floor", ...(renderer.xr.useProjectionLayers ? ["layers"] : [])] });
        [, session] = await Promise.all([activation.ready, granted]);
        if (!activation.isCurrent() || !active) {
          await session.end(); status.textContent = "VR entry canceled because the view changed."; await checkVR(); return;
        }
        nativeXRRequests = nativeXRFrames = appXRFrames = 0;
        report("vr-request-ready", { reason: `visibility:${session.visibilityState} features:${Array.from(session.enabledFeatures ?? []).join(",")}` });
        const requestFrame = session.requestAnimationFrame.bind(session);
        let reportedFrameError = false;
        session.requestAnimationFrame = callback => {
          nativeXRRequests++;
          if (nativeXRRequests === 1) report("xr-native-frame-requested", runtimeState());
          return requestFrame((time, frame) => {
            nativeXRFrames++;
            const first = nativeXRFrames === 1;
            if (first) report("xr-native-frame-enter", runtimeState());
            try { callback(time, frame); }
            catch (error) { if (!reportedFrameError) { reportedFrameError = true; report("xr-native-frame-error", { ...runtimeState(), message: error.message }, true); } throw error; }
            finally { if (first) report("xr-native-frame-exit", runtimeState()); }
          });
        };
        session.addEventListener("visibilitychange", () => {
          clearInput(); audio.flush(); host.resetClock();
          report("xr-visibility", runtimeState());
        });
        report("xr-renderer-setup");
        setupSessionPromise = renderer.xr.setSession(session);
        try { await setupSessionPromise; } finally { setupSessionPromise = null; }
        if (!activation.isCurrent() || !active) { await exitVR(); await checkVR(); return; }
        const layer = renderer.xr.getBaseLayer();
        report("xr-renderer-ready", { ...runtimeState(), backend: layer?.constructor.name ?? "unknown-layer",
          screen: { width: layer?.framebufferWidth ?? layer?.textureWidth, height: layer?.framebufferHeight ?? layer?.textureHeight, dpr: 1 } });
        clearTimeout(xrWaitTimer);
        xrWaitTimer = setTimeout(() => {
          if (renderer.xr.getSession() !== session || appXRFrames > 0) return;
          status.textContent = session.visibilityState === "hidden" ? "VR is waiting for headset focus. Close the VR dashboard or use Exit VR to return."
            : "VR started but no frames arrived. Exit VR is available on this page; the diagnostic report records the runtime state.";
          report("xr-no-frames", runtimeState(), true);
        }, 5000);
      } catch (error) {
        if (session) await session.end().catch(() => {});
        report("vr-entry-failed", { message: error.message }, true);
        status.textContent = `Could not enter VR: ${error.message}. Desktop inspection remains available.`;
        await checkVR();
      } finally { sessionBusy = false; void checkVR(); }
    });
    renderer.xr.addEventListener("sessionstart", () => {
      cameraTransition = null; previousOrbit = { ...orbit }; camera.position.set(0, 0, 0); camera.quaternion.identity();
      document.body.classList.add("in-vr"); pendingPlacement = true;
      fpsHud.visible = showFPS; updateViewPresentation();
      xrRevealStarted = null; xrRevealing = !matchMedia("(prefers-reduced-motion: reduce)").matches;
      if (xrRevealing) { fill.intensity = key.intensity = fpsHud.material.opacity = 0; scene.background.set(0x000000); }
      renderRequested = shadowsDirty = true;
      report("vr-session-started");
      setVRLabel("Exit VR"); vrButton.disabled = false;
      host.resetClock(); clearInput(); host.sessionChanged(true);
    });
    renderer.xr.addEventListener("sessionend", () => {
      clearTimeout(xrWaitTimer);
      clearInput();
      document.body.classList.remove("in-vr"); world.position.set(0, 0, 0); world.rotation.set(0, 0, 0);
      fpsHud.visible = false; xrRevealing = false; xrRevealStarted = null;
      fill.intensity = 1.9; key.intensity = 1.15; fpsHud.material.opacity = 1;
      scene.background.set(mode === "depth" ? "#111217" : "#202a2c");
      pendingPlacement = false; xrRecenterHeld = false; host.resetClock(); host.sessionChanged(false);
      renderer.setPixelRatio(Math.min(devicePixelRatio, 1.25));
      renderRequested = shadowsDirty = true;
      report("vr-session-ended");
      camera.fov = 42;
      orbit = previousOrbit ?? orbit; desktopCamera(); resize(); checkVR();
    });
    const headPosition = new THREE.Vector3(), headDirection = new THREE.Vector3();
    function placeInFront(frame) {
      const pose = frame.getViewerPose(renderer.xr.getReferenceSpace());
      if (!pose) return;
      const p = pose.transform.position, q = pose.transform.orientation;
      headPosition.set(p.x, p.y, p.z);
      headDirection.set(0, 0, -1).applyQuaternion(new THREE.Quaternion(q.x, q.y, q.z, q.w));
      headDirection.y = 0; if (headDirection.lengthSq() < 0.001) return;
      headDirection.normalize();
      world.position.copy(headPosition).addScaledVector(headDirection, 1.35); world.position.y -= 0.07;
      world.rotation.set(0, Math.atan2(-headDirection.x, -headDirection.z), 0);
      renderRequested = shadowsDirty = true;
      pendingPlacement = false;
    }

    function resize() {
      if (renderer.xr.isPresenting) return;
      const width = Math.max(1, stage.clientWidth), height = Math.max(1, stage.clientHeight);
      renderer.setSize(width, height, false); camera.aspect = width / height; camera.updateProjectionMatrix();
      desktopCamera();
    }
    new ResizeObserver(() => { if (active) resize(); }).observe(stage);
    window.addEventListener("resize", () => { if (active) resize(); });
    stage.addEventListener("webglcontextlost", event => {
      event.preventDefault(); host.pause();
      status.textContent = "Graphics context lost. Reload the player to restore the stage.";
      report("context-lost", { contextLost: true }, true);
      exitVR().catch(() => {});
    });
    desktopCamera(); resize(); updateStage(true); checkVR();
    report("shader-warmup-start");
    // Allow the loading state to paint before asking the driver to compile.
    setTimeout(() => renderer.compileAsync(scene, camera).then(() => {
      shadersReady = true; renderRequested = shadowsDirty = true; report("shader-warmup-ready"); checkVR();
    }).catch(error => { status.textContent = "Could not prepare graphics. Reload or try another browser."; report("shader-warmup-failed", { message: error.message }, true); }), 0);
    status.textContent = "Sample paper stage · drag or lean to look around"; stage.dataset.source = "sample";

    renderer.setAnimationLoop((time, frame) => {
      if (!active || previewing) return;
      const session = renderer.xr.getSession();
      if (!session) animateCamera(time);
      else animateXRReveal(time);
      if (frame && ++appXRFrames === 1) report("xr-app-frame", runtimeState());
      if (frame && pendingPlacement) placeInFront(frame);
      const visible = session ? session.visibilityState === "visible" : !document.hidden;
      const canDraw = session ? session.visibilityState !== "hidden" : !document.hidden;
      const playing = host.isPlaying();
      const pads = navigator.getGamepads?.() ?? [];
      let sizeAxis = 0;
      if (session && visible) {
        for (const pad of pads) if (pad?.connected && pad.mapping === "standard" && Math.abs(pad.axes[3] ?? 0) > Math.abs(sizeAxis)) sizeAxis = pad.axes[3];
        for (const source of session.inputSources) if (source.handedness === "right" && source.gamepad?.mapping === "xr-standard"
          && Math.abs(source.gamepad.axes[3] ?? 0) > Math.abs(sizeAxis)) sizeAxis = source.gamepad.axes[3];
      }
      const sizeDirection = sizeAxis < -0.6 ? 1 : sizeAxis > 0.6 ? -1 : 0;
      if (sizeDirection && time - lastSizeStep >= 100) {
        lastSizeStep = time;
        const nextSize = Math.max(50, Math.min(500, Number(stageValue("size")) + sizeDirection * 10));
        if (nextSize !== Number(stageValue("size"))) { $("size").value = String(nextSize); sizingHeld = true; updateStageSettings(false); }
      } else if (!sizeDirection && sizingHeld) { sizingHeld = false; saveStageSettings(); }
      input.set("xr", playing && visible && session ? readXRButtons(session.inputSources, BUTTONS) : 0);
      let leftClick = false, rightClick = false, leftGrip = false;
      if (session) for (const source of session.inputSources) {
        if (source.gamepad?.mapping !== "xr-standard") continue;
        if (source.handedness === "left") { leftClick = Boolean(source.gamepad.buttons[3]?.pressed); leftGrip = Boolean(source.gamepad.buttons[1]?.pressed); }
        if (source.handedness === "right") rightClick = Boolean(source.gamepad.buttons[3]?.pressed);
      }
      const recenterHeld = leftClick && rightClick;
      if (recenterHeld && !xrRecenterHeld) pendingPlacement = true;
      xrRecenterHeld = recenterHeld;
      host.setRewind("xr", Boolean(playing && visible && leftGrip));
      framesEmulated += host.tick(time, { visible, presenting: Boolean(session) });
      if (geometryDirty) updateStage();
      const statsNow = performance.now(), statsElapsed = statsNow - statsStarted;
      if (statsElapsed >= 1000) {
        fps = { rendered: (framesRendered - statsRendered) * 1000 / statsElapsed,
          emulated: (framesEmulated - statsEmulated) * 1000 / statsElapsed,
          scene: (sceneFrames - statsScene) * 1000 / statsElapsed };
        const text = `Render ${Math.round(fps.rendered)} fps · Game ${Math.round(fps.emulated)} fps · Scene ${Math.round(fps.scene)} fps`;
        $("fps").textContent = text;
        if (fpsHud.visible) {
          fpsContext.clearRect(0, 0, 768, 64); fpsContext.fillStyle = "#172322e8"; fpsContext.fillRect(0, 0, 768, 64);
          fpsContext.fillStyle = "#e0f0e5"; fpsContext.font = "27px system-ui"; fpsContext.textAlign = "center";
          fpsContext.fillText(text, 384, 42); fpsTexture.needsUpdate = true;
        }
        statsStarted = statsNow; statsRendered = framesRendered; statsEmulated = framesEmulated; statsScene = sceneFrames;
        if (statsNow - lastStatsReport > 5000) {
          report("frame-stats", { framesRendered, frameMs: lastRenderMs, geometryBuilds: [lastMeshMs],
            reason: `render:${fps.rendered.toFixed(1)} game:${fps.emulated.toFixed(1)} scene:${fps.scene.toFixed(1)}` });
          lastStatsReport = statsNow;
        }
      }
      if (shadersReady && canDraw && (renderer.xr.isPresenting || renderRequested)) {
        const started = performance.now();
        const firstXRRender = frame && appXRFrames === 1;
        if (firstXRRender) report("xr-render-enter", runtimeState());
        renderer.shadowMap.needsUpdate = shadowsDirty;
        renderer.render(scene, camera);
        if (firstXRRender) report("xr-render-exit", runtimeState());
        lastRenderMs = performance.now() - started;
        framesRendered++; renderRequested = shadowsDirty = false;
        if (framesRendered === 1 || renderer.xr.isPresenting && framesRendered % 300 === 0) report("rendered", { frameMs: lastRenderMs,
          framesRendered, triangles: renderer.info.render.triangles, drawCalls: renderer.info.render.calls,
          geometryBuilds: [lastMeshMs], screen: { width: stage.width, height: stage.height, dpr: renderer.getPixelRatio() } });
      }
      stage.dataset.xr = String(renderer.xr.isPresenting);
      stage.dataset.playing = String(host.isPlaying());
    });
    // Read-only counters for explicit local testing, no remote telemetry.
    if (new URLSearchParams(location.search).get("debug") === "1") {
      window.nesVRDebug = () => ({ triangles: renderer.info.render.triangles, calls: renderer.info.render.calls,
        geometries: renderer.info.memory.geometries, textures: renderer.info.memory.textures,
        framesRendered, framesEmulated, sceneFrames, fps, lastRenderMs, lastMeshMs, meshBusy, nativeXRFrames, appXRFrames,
        xr: renderer.xr.isPresenting, playing: host.isPlaying(), rewinding: host.isRewinding(), buttons: input.buttons, frame: (() => { const metadata = host.getEmulator()?.frame_metadata(); const number = metadata ? String(metadata.frame_number) : null; metadata?.free?.(); return number; })(),
        sides: vertexCounts.reduce((sum, n) => sum + n, 0), placement: world.position.toArray(),
        stageSizeCm: Number(stageValue("size")), shadowBounds: [key.shadow.camera.left, key.shadow.camera.right, key.shadow.camera.far],
        receiverZ: layers[0].front.position.z, rearCapZ: layers[1].back.position.z,
        model: "shared-prisms", mode, fpsVisible: fpsHud.visible, transitioning: Boolean(cameraTransition),
        orbit: { ...orbit }, cameraPosition: camera.position.toArray(), decorationsVisible: backing.visible || plinth.visible,
        gameCorners: renderer.xr.isPresenting ? [] : [[-0.5, -HEIGHT / WIDTH / 2], [0.5, HEIGHT / WIDTH / 2]].map(([x, y]) => new THREE.Vector3(x * diorama.scale.x, y * diorama.scale.x, 0.0015).project(camera).toArray()),
        backdropBounds: [backdropPlane.attributes.position.getX(0), backdropPlane.attributes.position.getY(0)],
        eyes: renderer.xr.isPresenting ? renderer.xr.getCamera().cameras.map(eye => ({ position: eye.position.toArray(), projection: eye.projectionMatrix.toArray() })) : [] });
    }
    return {
      refresh, recenter, exitVR,
      setFPSVisible(value) { showFPS = Boolean(value); fpsHud.visible = showFPS && renderer.xr.isPresenting; renderRequested = true; },
      get presenting() { return renderer.xr.isPresenting || Boolean(setupSessionPromise); },
      async setMode(value) {
        previewRevision++; resetOrbitGesture();
        const revision = ++modeRevision;
        if (value === "flat" && (setupSessionPromise || renderer.xr.getSession())) await exitVR();
        if (revision !== modeRevision) return;
        const wasActive = active, previousMode = mode;
        mode = value; active = value !== "flat";
        if (active) {
          refresh(true);
          if (wasActive && previousMode !== mode && !renderer.xr.isPresenting) desktopCamera(true);
          else {
            resize();
            if (previousMode === "flat" && mode === "orbit" && !renderer.xr.isPresenting) {
              mode = "depth"; desktopCamera(); mode = value; desktopCamera(true);
            }
          }
          renderRequested = true;
        } else { cameraTransition = null; clearInput(); }
      },
      setTilt(view, animate = false) {
        tilt = view;
        if (mode !== "depth" || renderer.xr.isPresenting) return;
        if (cameraTransition && !animate) { cameraTransition.to.yaw = tilt.x * 0.5; cameraTransition.to.pitch = -tilt.y * 0.5; }
        else desktopCamera(animate);
      },
      captureFrame(target) {
        if (!active || !shadersReady || previewing || renderer.xr.isPresenting) return false;
        renderer.render(scene, camera);
        target.getContext("2d").drawImage(stage, 0, 0, target.width, target.height); return true;
      },
      cancelPreviews() { previewRevision++; },
      async capturePresets(presets) {
        if (previewing || renderer.xr.isPresenting || !shadersReady) return [];
        resetOrbitGesture(); refresh(true);
        previewing = true;
        const revision = previewRevision;
        vrButton.disabled = true;
        renderer.setSize(512, 480, false); camera.aspect = 256 / 240; camera.updateProjectionMatrix();
        const images = [];
        try {
          for (const preset of presets) {
            if (revision !== previewRevision) return [];
            previewValues = preset.values;
            updateStageSettings(false);
            const until = performance.now() + 5000;
            while (meshBusy || geometryDirty) {
              if (revision !== previewRevision) return [];
              if (performance.now() > until) throw new Error("Preview generation timed out");
              if (!meshBusy && geometryDirty) updateStage();
              await new Promise(resolve => setTimeout(resolve, 16));
            }
            renderer.shadowMap.needsUpdate = true;
            renderer.render(scene, camera);
            images.push(stage.toDataURL("image/png"));
          }
        } finally {
          previewValues = null; previewing = false;
          resize(); updateStageSettings(false); checkVR(); host.resetClock();
        }
        return images;
      },
      applySettings(values) {
        for (const [id, value] of Object.entries(values)) { if ($(id).type === "checkbox") $(id).checked = value; else $(id).value = value; }
        key.shadow.intensity = $("shadows").checked ? 0.55 : 0;
        if (!renderer.xr.isPresenting) renderer.xr.setFramebufferScaleFactor(Number($("vr-resolution").value));
        updateMaterial(); updateStageSettings();
      },
    };
  }
}
