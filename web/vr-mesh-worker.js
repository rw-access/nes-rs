import { buildPrismSides } from "./vr-geometry.js?v=2";

const BYTES = 256 * 240 * 4;
const previous = Array.from({ length: 4 }, () => null);
let lastThickness = -1, lastEpoch = null;

self.onmessage = ({ data: job }) => {
  const started = performance.now();
  try {
    if (!(job.pixels instanceof Uint8Array) || job.pixels.length !== BYTES * 4) throw new RangeError("Invalid scene buffer");
    const meshes = [];
    let textureMask = 0;
    const force = job.force || job.epoch !== lastEpoch;
    for (let index = 0; index < 4; index++) {
      const pixels = job.pixels.subarray(index * BYTES, (index + 1) * BYTES);
      const cached = previous[index];
      let changed = force || !cached;
      // Native RGBA layers are word-aligned. Compare the exact bits (including
      // transparent texels), with no hashing or dependence on byte order.
      if (!changed) {
        const nextWords = new Uint32Array(pixels.buffer, pixels.byteOffset, BYTES / 4);
        const previousWords = new Uint32Array(cached.buffer);
        for (let i = 0; i < nextWords.length; i++) if (nextWords[i] !== previousWords[i]) { changed = true; break; }
      }
      if (changed) {
        textureMask |= 1 << index;
        if (cached) cached.set(pixels);
        else previous[index] = pixels.slice();
      }
      if (index > 0 && (changed || job.thickness !== lastThickness)) {
        meshes.push({ index, ...buildPrismSides(pixels, 256, 240, job.thickness) });
      }
    }
    lastThickness = job.thickness; lastEpoch = job.epoch;
    const transfers = [job.pixels.buffer];
    for (const mesh of meshes) transfers.push(mesh.positions.buffer, mesh.normals.buffer, mesh.colors.buffer);
    self.postMessage({ epoch: job.epoch, pixels: job.pixels, textureMask, meshes, buildMs: performance.now() - started }, transfers);
  } catch (_) {
    previous.fill(null); lastThickness = -1; lastEpoch = null;
    self.postMessage({ epoch: job.epoch, error: "Could not build prism geometry" });
  }
};
