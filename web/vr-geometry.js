// Fixed palette conversion avoids repeating powers for every exposed edge.
const LINEAR_COLOR = Float32Array.from({ length: 256 }, (_, value) => {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
});

// Camera-independent, closed silhouette prisms. Coordinates use one scene unit
// per image width; callers choose the final size in meters with a group scale.
export function buildPrismSides(rgba, width, height, thickness) {
  if (!(rgba instanceof Uint8Array || rgba instanceof Uint8ClampedArray)
    || !Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1
    || width > 256 || height > 240 || rgba.length !== width * height * 4) throw new RangeError("Invalid native layer");
  if (!Number.isFinite(thickness) || thickness < 0 || thickness > 0.25) throw new RangeError("Invalid prism thickness");
  const opaque = (x, y) => x >= 0 && x < width && y >= 0 && y < height && rgba[(y * width + x) * 4 + 3] > 0;
  const sameColor = (a, b) => rgba[a] === rgba[b] && rgba[a + 1] === rgba[b + 1] && rgba[a + 2] === rgba[b + 2];
  // Count the start of each exposed same-color edge run in one linear pass.
  // The second pass writes those runs directly into exact-size output buffers.
  // There are no growing number arrays or per-face objects.
  let faceCount = 0;
  if (thickness > 0) {
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const offset = (y * width + x) * 4;
      if (rgba[offset + 3] === 0) continue;
      const rowBytes = width * 4;
      const left = x === 0 || rgba[offset - 1] === 0;
      const right = x + 1 === width || rgba[offset + 7] === 0;
      const top = y === 0 || rgba[offset - rowBytes + 3] === 0;
      const bottom = y + 1 === height || rgba[offset + rowBytes + 3] === 0;
      if (left || right) {
        const sameAbove = !top && sameColor(offset, offset - rowBytes);
        if (left && (!sameAbove || (x > 0 && rgba[offset - rowBytes - 1] > 0))) faceCount++;
        if (right && (!sameAbove || (x + 1 < width && rgba[offset - rowBytes + 7] > 0))) faceCount++;
      }
      if (top || bottom) {
        const sameLeft = !left && sameColor(offset, offset - 4);
        if (top && (!sameLeft || (y > 0 && rgba[offset - rowBytes - 1] > 0))) faceCount++;
        if (bottom && (!sameLeft || (y + 1 < height && rgba[offset + rowBytes - 1] > 0))) faceCount++;
      }
    }
  }
  function visitFaces(face) {
    if (faceCount === 0) return;
    for (let dx = -1; dx <= 1; dx += 2) {
      for (let x = 0; x < width; x++) {
        let y = 0;
        while (y < height) {
          if (!opaque(x, y) || opaque(x + dx, y)) { y++; continue; }
          const start = y, offset = (y * width + x) * 4;
          while (++y < height && opaque(x, y) && !opaque(x + dx, y) && sameColor(offset, (y * width + x) * 4)) {}
          face(x + (dx > 0 ? 1 : 0), start, x + (dx > 0 ? 1 : 0), y, dx, 0, offset);
        }
      }
    }
    for (let dy = -1; dy <= 1; dy += 2) {
      for (let y = 0; y < height; y++) {
        let x = 0;
        while (x < width) {
          if (!opaque(x, y) || opaque(x, y + dy)) { x++; continue; }
          const start = x, offset = (y * width + x) * 4;
          while (++x < width && opaque(x, y) && !opaque(x, y + dy) && sameColor(offset, (y * width + x) * 4)) {}
          face(start, y + (dy > 0 ? 1 : 0), x, y + (dy > 0 ? 1 : 0), 0, -dy, offset);
        }
      }
    }
  }
  const positions = new Float32Array(faceCount * 18);
  const normals = new Float32Array(positions.length);
  const colors = new Float32Array(positions.length);
  let cursor = 0;
  function vertex(x, y, z, nx, ny, r, g, b) {
    positions[cursor] = x; positions[cursor + 1] = y; positions[cursor + 2] = z;
    normals[cursor] = nx; normals[cursor + 1] = ny; normals[cursor + 2] = 0;
    colors[cursor] = r; colors[cursor + 1] = g; colors[cursor + 2] = b;
    cursor += 3;
  }
  visitFaces((x0, y0, x1, y1, nx, ny, offset) => {
    const ax = (x0 - width / 2) / width, ay = (height / 2 - y0) / width;
    const bx = (x1 - width / 2) / width, by = (height / 2 - y1) / width;
    const r = LINEAR_COLOR[rgba[offset]], g = LINEAR_COLOR[rgba[offset + 1]], b = LINEAR_COLOR[rgba[offset + 2]];
    const forward = -(by - ay) * thickness * nx + (bx - ax) * thickness * ny >= 0;
    vertex(ax, ay, 0, nx, ny, r, g, b);
    if (forward) {
      vertex(bx, by, 0, nx, ny, r, g, b);
      vertex(bx, by, -thickness, nx, ny, r, g, b);
      vertex(ax, ay, 0, nx, ny, r, g, b);
      vertex(bx, by, -thickness, nx, ny, r, g, b);
      vertex(ax, ay, -thickness, nx, ny, r, g, b);
    } else {
      vertex(bx, by, -thickness, nx, ny, r, g, b);
      vertex(bx, by, 0, nx, ny, r, g, b);
      vertex(ax, ay, 0, nx, ny, r, g, b);
      vertex(ax, ay, -thickness, nx, ny, r, g, b);
      vertex(bx, by, -thickness, nx, ny, r, g, b);
    }
  });
  return { positions, normals, colors };
}

export function readXRButtons(sources, buttons) {
  let mask = 0;
  for (const source of sources) {
    const pad = source.gamepad;
    if (!pad || pad.mapping !== "xr-standard") continue;
    const down = index => pad.buttons[index]?.pressed;
    if (source.handedness === "left") {
      if (pad.axes[2] < -0.4) mask |= buttons.left;
      if (pad.axes[2] > 0.4) mask |= buttons.right;
      if (pad.axes[3] < -0.4) mask |= buttons.up;
      if (pad.axes[3] > 0.4) mask |= buttons.down;
      if (down(4)) mask |= buttons.select;
      if (down(5)) mask |= buttons.start;
    } else if (source.handedness === "right") {
      if (down(0) || down(4)) mask |= buttons.a;
      if (down(1) || down(5)) mask |= buttons.b;
    }
  }
  return mask;
}
