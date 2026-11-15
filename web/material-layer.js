import { createMaterialSampler, getMaterialFinish } from "./material-finish.js";
import { getSceneLightPosition, getSceneLightRevision, getSceneShadingStrength } from "./scene-lighting.js";
import { getSceneCameraDepth, getSceneVanishingPoint, getSceneViewRevision } from "./scene-view.js";

// All printed layers share a front plane. Reuse one smoothly interpolated
// highlight field instead of evaluating a BRDF at every pixel of every layer.
let cachedField = null;
function highlightField(width, height) {
  const lightRevision = getSceneLightRevision(), viewRevision = getSceneViewRevision();
  if (cachedField?.width === width && cachedField.height === height
    && cachedField.lightRevision === lightRevision && cachedField.viewRevision === viewRevision) return cachedField.pixels;
  const eye = { ...getSceneVanishingPoint(width, height), z: -getSceneCameraDepth() };
  const sample = createMaterialSampler({ light: getSceneLightPosition(width, height), eye, width, height, strength: getSceneShadingStrength() });
  const cols = Math.ceil((width - 1) / 8) + 1, rows = Math.ceil((height - 1) / 8) + 1;
  const grid = new Float32Array(cols * rows), pixels = new Float32Array(width * height);
  for (let row = 0; row < rows; row += 1) for (let col = 0; col < cols; col += 1) {
    grid[row * cols + col] = sample(col * (width - 1) / Math.max(1, cols - 1) + .5, row * (height - 1) / Math.max(1, rows - 1) + .5);
  }
  for (let y = 0; y < height; y += 1) {
    const gy = y * (rows - 1) / Math.max(1, height - 1), row = Math.floor(gy), ty = gy - row, nextRow = Math.min(row + 1, rows - 1);
    for (let x = 0; x < width; x += 1) {
      const gx = x * (cols - 1) / Math.max(1, width - 1), col = Math.floor(gx), tx = gx - col, nextCol = Math.min(col + 1, cols - 1);
      const top = grid[row * cols + col] * (1 - tx) + grid[row * cols + nextCol] * tx;
      const bottom = grid[nextRow * cols + col] * (1 - tx) + grid[nextRow * cols + nextCol] * tx;
      pixels[y * width + x] = top * (1 - ty) + bottom * ty;
    }
  }
  cachedField = { width, height, lightRevision, viewRevision, pixels };
  return pixels;
}

export class CanvasMaterialLayer {
  constructor(canvas, width, height) {
    this.width = width; this.height = height;
    this.context = canvas.getContext("2d", { alpha: true });
    this.image = this.context.createImageData(width, height);
    this.lightRevision = -1; this.viewRevision = -1;
  }

  paint(source, changedSource = true) {
    const lightRevision = getSceneLightRevision(), viewRevision = getSceneViewRevision();
    const hasHighlights = getMaterialFinish() !== "matte" && getSceneShadingStrength() > 0;
    if (!changedSource && !hasHighlights && this.hadHighlights === false) {
      this.lightRevision = lightRevision; this.viewRevision = viewRevision;
      return false;
    }
    if (!changedSource && this.lightRevision === lightRevision && this.viewRevision === viewRevision) return false;
    const output = this.image.data;
    output.set(source);
    if (hasHighlights) {
      const highlights = highlightField(this.width, this.height);
      for (let pixel = 0, offset = 0; offset < output.length; pixel += 1, offset += 4) {
        if (!output[offset + 3]) continue;
        const amount = highlights[pixel];
        // A bounded white direct-light overlay; input pigment and alpha remain
        // intact for geometry, masks and subsequent material changes.
        output[offset] += Math.round((255 - output[offset]) * amount);
        output[offset + 1] += Math.round((255 - output[offset + 1]) * amount);
        output[offset + 2] += Math.round((255 - output[offset + 2]) * amount);
      }
    }
    this.context.putImageData(this.image, 0, 0);
    this.lightRevision = lightRevision; this.viewRevision = viewRevision;
    this.hadHighlights = hasHighlights;
    return true;
  }
}
