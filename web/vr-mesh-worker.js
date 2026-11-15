import { buildPrismSides } from "./vr-geometry.js?v=2";

const BYTES = 256 * 240 * 4;
const previous = Array.from({ length: 4 }, () => null);
let lastThickness = -1;

self.onmessage = ({ data: job }) => {
  const started = performance.now();
  try {
    const meshes = [];
    for (let index = 1; index < 4; index++) {
      const pixels = job.pixels.subarray(index * BYTES, (index + 1) * BYTES);
      let changed = job.force || job.thickness !== lastThickness || !previous[index];
      if (!changed) for (let i = 0; i < BYTES; i++) if (pixels[i] !== previous[index][i]) { changed = true; break; }
      if (!changed) continue;
      previous[index] = pixels.slice();
      meshes.push({ index, ...buildPrismSides(pixels, 256, 240, job.thickness) });
    }
    lastThickness = job.thickness;
    const transfers = [job.pixels.buffer];
    for (const mesh of meshes) transfers.push(mesh.positions.buffer, mesh.normals.buffer, mesh.colors.buffer);
    self.postMessage({ epoch: job.epoch, pixels: job.pixels, meshes, buildMs: performance.now() - started }, transfers);
  } catch (_) {
    previous.fill(null); lastThickness = -1;
    self.postMessage({ epoch: job.epoch, error: "Could not build prism geometry" });
  }
};
