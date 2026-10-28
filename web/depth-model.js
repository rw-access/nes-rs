import { MAX_DEPTH, cameraDepthScale } from "./depth-mesh.js";
import { shadowLightProjection } from "./scene-lighting.js";
import { getSceneViewOffset } from "./scene-view.js";
import { SCENE_DEPTH_TO_NATIVE_PIXELS } from "./viewing-model.js";

export const MAX_SCENE_DISTANCE_PIXELS = MAX_DEPTH * SCENE_DEPTH_TO_NATIVE_PIXELS;
export const MAX_CARD_THICKNESS_PIXELS = 8;

/** Convert scene controls expressed in native game pixels into shared scene Z units. */
export function resolveSceneDepth(separationPixels, thicknessPixels, solidSides = true) {
  if (!Number.isFinite(separationPixels) || separationPixels < 0 || separationPixels > MAX_SCENE_DISTANCE_PIXELS) {
    throw new RangeError(`Background separation must be from 0 to ${MAX_SCENE_DISTANCE_PIXELS} native pixels`);
  }
  if (!Number.isFinite(thicknessPixels) || thicknessPixels < 0 || thicknessPixels > MAX_CARD_THICKNESS_PIXELS) {
    throw new RangeError(`Card thickness must be from 0 to ${MAX_CARD_THICKNESS_PIXELS} native pixels`);
  }
  if (typeof solidSides !== "boolean") throw new TypeError("Solid sides must be a boolean");
  const separation = separationPixels / SCENE_DEPTH_TO_NATIVE_PIXELS;
  const requestedThickness = thicknessPixels / SCENE_DEPTH_TO_NATIVE_PIXELS;
  const thickness = solidSides ? Math.min(separation, requestedThickness) : 0;
  return { separation, thickness, clearance: Math.max(0, separation - thickness) };
}

/** Normalized softness: 25% preserves the former 50%, 100% reaches 2x blur. */
export function resolveShadowSoftness(percent) {
  if (!Number.isFinite(percent) || percent < 0 || percent > 100) {
    throw new RangeError("Shadow softness must be from 0 to 100 percent");
  }
  return percent / 50;
}

/** Position the rim behind the game plane as a fraction of the backdrop gap. */
export function resolveRimRecess(scene, percent = 0) {
  if (!Number.isFinite(percent) || percent < 0 || percent > 100) {
    throw new RangeError("Rim recess must be from 0 to 100 percent of the gap");
  }
  const gap = sceneGap(scene);
  return gap * (percent / 100);
}

/** Scale rim thickness against the foreground and keep it inside the backdrop. */
export function resolveRimThickness(scene, percent = 25, frontDepth = 0) {
  if (!Number.isFinite(percent) || percent < 0 || percent > 100) {
    throw new RangeError("Rim thickness must be from 0 to 100 percent of the foreground");
  }
  const gap = sceneGap(scene);
  if (!Number.isFinite(scene.thickness) || scene.thickness < 0 || scene.thickness > gap) {
    throw new RangeError("Scene thickness must be within the backdrop gap");
  }
  if (!Number.isFinite(frontDepth) || frontDepth < 0 || frontDepth > gap) {
    throw new RangeError("Rim front depth must be within the backdrop gap");
  }
  const availableDepth = Math.max(0, gap - frontDepth);
  return Math.min(scene.thickness * (percent / 100), availableDepth);
}

function sceneGap(scene) {
  if (!Number.isFinite(scene?.separation) || scene.separation < 0 || scene.separation > MAX_DEPTH) {
    throw new RangeError("Scene separation must be within the supported depth range");
  }
  return scene.separation;
}

/** Project a foreground silhouette onto the backdrop with the shared point light. */
export function gameplayShadowProjection(scene, width = 256, height = 240) {
  const light = shadowLightProjection(scene.thickness, scene.separation, width, height);
  const cameraScale = cameraDepthScale(scene.separation);
  const viewOffset = getSceneViewOffset(width, height);
  const scale = cameraScale * light.magnification;
  const projection = {
    scale,
    x: light.x * cameraScale + viewOffset.x * (1 - cameraScale),
    y: light.y * cameraScale + viewOffset.y * (1 - cameraScale),
    blur: light.travel / light.magnification,
  };
  if (light.opacity !== undefined) projection.opacity = light.opacity;
  return projection;
}
