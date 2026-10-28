import { projectRimBackPoint } from "./rim-projection.js";
import { cameraDepthScale, projectDepthPoint } from "./depth-mesh.js";
import { getSceneLightRevision, surfaceShade } from "./scene-lighting.js";
import { getSceneViewRevision } from "./scene-view.js";
import { RIM_CORNER_FRACTION, traceRoundedRectangle } from "./rim-outline.js";

export const RIM_MATERIALS = Object.freeze({
  charcoal: Object.freeze({ front: "#24252a", side: Object.freeze([104, 108, 116]) }),
  // Same front color as the console shell in tv.svg, without a black insert.
  console: Object.freeze({ front: "#ded8cd", side: Object.freeze([222, 216, 205]) }),
});
const EDGE_TRANSITION_WIDTH = 0.35; // NES pixels, independent of wall depth.

function openingCorners(width, height) {
  const rx = width * RIM_CORNER_FRACTION, ry = height * RIM_CORNER_FRACTION;
  return [
    { x: rx, y: ry }, { x: width - rx, y: ry },
    { x: width - rx, y: height - ry }, { x: rx, y: height - ry },
  ];
}

function cornerPoint(center, rx, ry, angle) {
  return { x: center.x + rx * Math.cos(angle), y: center.y + ry * Math.sin(angle) };
}

/** Half of each corner belongs to each adjacent wall, sharing exact endpoints. */
function wallBoundary(context, corners, edge, depth, width, height, reverse) {
  const rx = width * RIM_CORNER_FRACTION, ry = height * RIM_CORNER_FRACTION;
  const scale = cameraDepthScale(depth);
  const project = point => projectDepthPoint(point, depth, width, height);
  const first = { center: corners[edge], from: Math.PI * (1.25 + edge / 2), to: Math.PI * (1.5 + edge / 2) };
  const second = { center: corners[(edge + 1) % 4], from: first.to, to: first.to + Math.PI / 4 };
  const arcs = reverse ? [second, first] : [first, second];
  const start = arcs[0];
  const point = project(cornerPoint(start.center, rx, ry, reverse ? start.to : start.from));
  if (reverse) context.lineTo(point.x, point.y);
  else context.moveTo(point.x, point.y);
  for (const arc of arcs) {
    const center = project(arc.center);
    const from = reverse ? arc.to : arc.from;
    const to = reverse ? arc.from : arc.to;
    const point = project(cornerPoint(arc.center, rx, ry, from));
    context.lineTo(point.x, point.y);
    context.ellipse(center.x, center.y, rx * scale, ry * scale, 0, from, to, reverse);
  }
}

function edgeColor(material, shade) {
  return `rgb(${material.side.map(channel => Math.round(channel * shade)).join(",")})`;
}

function wallMaterial(context, front, back, orientation, material, shade) {
  // Keep the depth-shading gradient perpendicular to the projected edge.
  const horizontal = orientation === "top" || orientation === "bottom";
  const end = horizontal ? { x: front.x, y: back.y } : { x: back.x, y: front.y };
  const span = Math.hypot(end.x - front.x, end.y - front.y);
  const bevel = Math.min(0.22, EDGE_TRANSITION_WIDTH / span);
  const rear = Math.min(0.2, EDGE_TRANSITION_WIDTH / span);
  const gradient = context.createLinearGradient(front.x, front.y, end.x, end.y);
  // The small chamfer faces partly toward the light/viewer. Keep its highlight
  // narrow; most of the side remains a flat, directionally lit charcoal face.
  gradient.addColorStop(0, edgeColor(material, 0.4 + shade * 0.65));
  gradient.addColorStop(bevel, edgeColor(material, shade));
  gradient.addColorStop(1 - rear, edgeColor(material, shade));
  gradient.addColorStop(1, edgeColor(material, shade * 0.82));
  return gradient;
}

/** A surrounding tile, optionally recessed behind the original gameplay face. */
export class RimHousing {
  constructor(canvas, stage, width = 256, height = 240) {
    this.canvas = canvas;
    this.stage = stage;
    this.width = width;
    this.height = height;
    this.context = canvas.getContext("2d");
    this.lastKey = null;
  }

  paint(gap, thickness, dpr = 1, frontDepth = 0, faceWidth = 0, materialName = "charcoal", surroundColor = "#111217") {
    const material = RIM_MATERIALS[materialName];
    if (!material) throw new RangeError("Unknown rim material");
    const width = this.stage.clientWidth;
    if (width <= 0) return;
    const height = width * this.height / this.width;
    const ratio = Math.min(2, dpr, 1536 / width);
    const key = [width, height, ratio, gap, thickness, frontDepth, faceWidth, materialName, surroundColor,
      getSceneLightRevision(), getSceneViewRevision()].join(",");
    if (key === this.lastKey) return;
    this.lastKey = key;
    this.canvas.width = Math.max(1, Math.floor(width * ratio));
    this.canvas.height = Math.max(1, Math.floor(height * ratio));
    this.canvas.hidden = gap === 0 || (thickness === 0 && (frontDepth === 0 || faceWidth === 0));
    if (this.canvas.hidden) return;

    const opening = [
      { x: 0, y: 0 }, { x: this.width, y: 0 },
      { x: this.width, y: this.height }, { x: 0, y: this.height },
    ];
    const front = opening.map(point => projectDepthPoint(point, frontDepth, this.width, this.height));
    const back = opening.map(point => projectRimBackPoint(point, gap, this.width, this.height, thickness, frontDepth));
    const corners = openingCorners(this.width, this.height);
    const context = this.context;
    context.setTransform(this.canvas.width / this.width, 0, 0, this.canvas.height / this.height, 0, 0);
    const orientations = ["top", "right", "bottom", "left"];
    const shades = orientations.map((orientation, index) => {
      const next = (index + 1) % 4;
      // Lighting uses native world coordinates, before camera projection.
      return surfaceShade(orientation, true, {
        x: (opening[index].x + opening[next].x) / 2,
        y: (opening[index].y + opening[next].y) / 2,
        z: frontDepth + thickness / 2,
      }, this.width, this.height);
    });
    if (thickness > 0) {
      // Underpaint the joined strips once so antialiasing at shared corner
      // seams cannot reveal the backdrop between independently shaded walls.
      context.fillStyle = edgeColor(material, Math.min(...shades));
      context.beginPath();
      for (let edge = 0; edge < 4; edge++) {
        wallBoundary(context, corners, edge, frontDepth, this.width, this.height, false);
        wallBoundary(context, corners, edge, frontDepth + thickness, this.width, this.height, true);
        context.closePath();
      }
      context.fill();
    }
    for (const [index, orientation] of orientations.entries()) {
      if (thickness === 0) break;
      const next = (index + 1) % 4;
      const midpoint = points => ({
        x: (points[index].x + points[next].x) / 2,
        y: (points[index].y + points[next].y) / 2,
      });
      context.fillStyle = wallMaterial(context, midpoint(front), midpoint(back), orientation, material, shades[index]);
      context.beginPath();
      wallBoundary(context, corners, index, frontDepth, this.width, this.height, false);
      wallBoundary(context, corners, index, frontDepth + thickness, this.width, this.height, true);
      context.closePath();
      context.fill();
    }
    if (frontDepth > 0 && faceWidth > 0) {
      // The exterior belongs to the page surround, independently of the rim's
      // material. Opaque page color masks the backdrop below this canvas;
      // transparent fill would expose sky outside the projected outer contour.
      // Gameplay paints above, preserving every native foreground pixel.
      const scale = cameraDepthScale(frontDepth);
      const project = point => projectDepthPoint(point, frontDepth, this.width, this.height);
      for (const [margin, color] of [[0, material.front], [faceWidth, surroundColor]]) {
        context.fillStyle = color;
        context.beginPath();
        context.moveTo(0, 0);
        context.lineTo(this.width, 0);
        context.lineTo(this.width, this.height);
        context.lineTo(0, this.height);
        context.closePath();
        traceRoundedRectangle(context, project({ x: -margin, y: -margin }),
          project({ x: this.width + margin, y: this.height + margin }),
          (this.width * RIM_CORNER_FRACTION + margin) * scale,
          (this.height * RIM_CORNER_FRACTION + margin) * scale);
        context.fill("evenodd");
      }
    }
  }
}
