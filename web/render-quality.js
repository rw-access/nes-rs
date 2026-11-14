export const RENDER_WIDTHS = Object.freeze({ high: 1536, balanced: 768, performance: 512 });

// Resolution changes only the effect surfaces. Geometry, source NES pixels,
// simulation speed and audio retain the same values at every level.
export class RenderQuality {
  constructor() { this.mode = "auto"; this.level = "balanced"; this.resetSamples(); }
  resetSamples() { this.cost = 0; this.samples = 0; this.slow = 0; this.fast = 0; }
  setMode(mode) {
    if (mode !== "auto" && !(mode in RENDER_WIDTHS)) throw new RangeError("Unknown render quality");
    this.mode = mode;
    this.level = mode === "auto" ? "balanced" : mode;
    this.resetSamples();
  }
  get maxWidth() { return RENDER_WIDTHS[this.level]; }
  sample(milliseconds) {
    if (this.mode !== "auto" || !Number.isFinite(milliseconds) || milliseconds < 0) return false;
    this.cost += milliseconds;
    this.samples += 1;
    if (this.samples < 60) return false;
    const average = this.cost / this.samples;
    this.cost = 0; this.samples = 0;
    this.slow = average > 12 ? this.slow + 1 : 0;
    this.fast = average < 5 ? this.fast + 1 : 0;
    // Two sustained slow windows lower the pixel budget; ten fast windows
    // restore balanced edges without switching resolution on every spike.
    if (this.level === "balanced" && this.slow >= 2) {
      this.level = "performance"; this.resetSamples(); return true;
    }
    if (this.level === "performance" && this.fast >= 10) {
      this.level = "balanced"; this.resetSamples(); return true;
    }
    return false;
  }
}

export function effectPixelRatio(cssWidth, devicePixelRatio, maxWidth) {
  return Math.min(devicePixelRatio || 1, 2, maxWidth / Math.max(1, cssWidth));
}

// Advance every simulation frame, present only the latest state. Discard old
// wall-clock backlog after a bounded catch-up instead of spiralling indefinitely.
export function frameBudget(accumulator, frameMs, limit = 4) {
  const available = Math.max(0, Math.floor(accumulator / frameMs));
  return { frames: Math.min(limit, available), remainder: accumulator % frameMs };
}
