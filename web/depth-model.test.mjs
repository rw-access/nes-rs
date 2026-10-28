import test from 'node:test';
import assert from 'node:assert/strict';
import {
  gameplayShadowProjection,
  MAX_CARD_THICKNESS_PIXELS,
  MAX_SCENE_DISTANCE_PIXELS,
  resolveRimRecess,
  resolveRimThickness,
  resolveSceneDepth,
  resolveShadowSoftness,
} from './depth-model.js';
import { MAX_DEPTH, projectDepthPoint } from './depth-mesh.js';
import { projectRimBackPoint } from './rim-projection.js';
import { getSceneCameraDepth, getSceneView, getSceneViewOffset, setSceneCamera, setSceneView } from './scene-view.js';
import { setSceneLightRotation } from './scene-lighting.js';
import { SCENE_DEPTH_TO_NATIVE_PIXELS } from './viewing-model.js';

const nativePx = (sceneZ) => sceneZ * SCENE_DEPTH_TO_NATIVE_PIXELS;
const gapPercentPx = (percent) => MAX_SCENE_DISTANCE_PIXELS * percent / 100;

test('scene depth controls use native game-pixel units and convert once to shared Z', () => {
  assert.equal(MAX_SCENE_DISTANCE_PIXELS, MAX_DEPTH * SCENE_DEPTH_TO_NATIVE_PIXELS);
  assert.equal(MAX_CARD_THICKNESS_PIXELS, 8);
  for (const thicknessPx of [0, 0.5, 2, MAX_CARD_THICKNESS_PIXELS]) {
    for (const solid of [false, true]) {
      const scene = resolveSceneDepth(85, thicknessPx, solid);
      const expectedThickness = solid ? Math.min(85, thicknessPx) / SCENE_DEPTH_TO_NATIVE_PIXELS : 0;
      assert.ok(Math.abs(scene.separation - 85 / SCENE_DEPTH_TO_NATIVE_PIXELS) < 1e-12);
      assert.ok(Math.abs(scene.thickness - expectedThickness) < 1e-12);
      assert.ok(Math.abs(scene.clearance - (85 / SCENE_DEPTH_TO_NATIVE_PIXELS - expectedThickness)) < 1e-12);
    }
  }
});

test('native-pixel depth conversion is independent of camera pose and viewing profile', () => {
  const before = resolveSceneDepth(85, 2);
  const previousCameraDepth = getSceneCameraDepth();
  const previousView = getSceneView();
  try {
    setSceneView(0.2, -0.15);
    setSceneCamera(0.2, -0.15, previousCameraDepth * 1.4);
    assert.deepEqual(resolveSceneDepth(85, 2), before);
    assert.deepEqual(resolveSceneDepth(85, 2, true), before);
  } finally {
    setSceneCamera(previousView.x, previousView.y, previousCameraDepth);
  }
});

test('converted native-pixel thickness reaches geometry as the same shared scene Z', () => {
  const point = { x: 47.25, y: 83.5 };
  const scene = resolveSceneDepth(85, 2);
  const projected = projectDepthPoint(point, scene.thickness, 256, 240);
  const cameraScale = 1 / (1 + (2 / SCENE_DEPTH_TO_NATIVE_PIXELS) * 0.015);
  assert.ok(Math.abs(projected.x - (128 + (point.x - 128) * cameraScale)) < 1e-12);
  assert.ok(Math.abs(projected.y - (120 + (point.y - 120) * cameraScale)) < 1e-12);
  assert.equal(scene.thickness, 2 / SCENE_DEPTH_TO_NATIVE_PIXELS);
  assert.equal(scene.separation, 85 / SCENE_DEPTH_TO_NATIVE_PIXELS);
});

test('native-pixel gap/thickness bounds, zero endpoints, and backdrop clamping', () => {
  const zero = resolveSceneDepth(0, MAX_CARD_THICKNESS_PIXELS);
  assert.deepEqual(zero, { separation: 0, thickness: 0, clearance: 0 });
  const maximum = resolveSceneDepth(MAX_SCENE_DISTANCE_PIXELS, MAX_CARD_THICKNESS_PIXELS);
  assert.equal(maximum.separation, MAX_DEPTH);
  assert.equal(maximum.thickness, MAX_CARD_THICKNESS_PIXELS / SCENE_DEPTH_TO_NATIVE_PIXELS);
  assert.ok(maximum.clearance > 0);

  const ordinary = resolveSceneDepth(85, 2, true);
  assert.ok(ordinary.thickness < ordinary.separation);
  const tinyGap = resolveSceneDepth(1, MAX_CARD_THICKNESS_PIXELS, true);
  assert.equal(tinyGap.thickness, tinyGap.separation);
  assert.equal(tinyGap.clearance, 0);
  assert.equal(resolveSceneDepth(1, MAX_CARD_THICKNESS_PIXELS, false).thickness, 0);
  for (const invalidSeparation of [-1, MAX_SCENE_DISTANCE_PIXELS + 0.01, NaN, Infinity]) {
    assert.throws(() => resolveSceneDepth(invalidSeparation, 2), RangeError);
  }
  for (const invalidThickness of [-1, MAX_CARD_THICKNESS_PIXELS + 0.01, NaN, Infinity]) {
    assert.throws(() => resolveSceneDepth(85, invalidThickness), RangeError);
  }
  assert.throws(() => resolveSceneDepth(85, 2, 1), TypeError);
});

test('normalized softness preserves former 50 percent at 25 and reaches former 200 at 100',()=>{
  assert.equal(resolveShadowSoftness(0),0);
  assert.equal(resolveShadowSoftness(25),.5);
  assert.equal(resolveShadowSoftness(100),2);
  for(const invalid of [-1,101,NaN,Infinity]) assert.throws(()=>resolveShadowSoftness(invalid),RangeError);
});

test('the flush rim scales foreground thickness and remains within the backdrop gap',()=>{
  for(const separationPercent of [0,50,100]){
    const separation=gapPercentPx(separationPercent);
    for(const fillPx of [0,0.5,2,MAX_CARD_THICKNESS_PIXELS])for(const ratio of [0,25,50,100]){
      const scene=resolveSceneDepth(separation,fillPx);
      assert.ok(Math.abs(resolveRimThickness(scene,ratio)-scene.thickness*ratio/100)<1e-12);
      assert.ok(resolveRimThickness(scene,ratio)<=scene.separation+1e-12);
    }
    assert.equal(resolveRimThickness(resolveSceneDepth(separation,MAX_CARD_THICKNESS_PIXELS)),
      resolveSceneDepth(separation,MAX_CARD_THICKNESS_PIXELS).thickness / 4);
    assert.equal(resolveRimThickness(resolveSceneDepth(separation,MAX_CARD_THICKNESS_PIXELS,false)),0);
  }
  for(const invalid of [-1,101,NaN,Infinity])assert.throws(()=>resolveRimThickness(resolveSceneDepth(gapPercentPx(50),2),invalid),RangeError);
});

test('rim recess spans the backdrop gap and thickness clamps behind its front plane', () => {
  for (const separationPercent of [0, 25, 100]) {
    const scene = resolveSceneDepth(gapPercentPx(separationPercent), MAX_CARD_THICKNESS_PIXELS);
    for (const percent of [0, 25, 50, 75, 100]) {
      const frontDepth = resolveRimRecess(scene, percent);
      const rimThickness = resolveRimThickness(scene, 100, frontDepth);
      assert.equal(frontDepth, scene.separation * percent / 100);
      assert.equal(rimThickness, Math.min(scene.thickness, Math.max(0, scene.separation - frontDepth)));
      assert.ok(frontDepth >= 0);
      assert.ok(frontDepth + rimThickness <= scene.separation + 1e-12);
    }
  }

  const scene = resolveSceneDepth(gapPercentPx(75), MAX_CARD_THICKNESS_PIXELS);
  assert.equal(resolveRimRecess(scene), 0);
  assert.equal(resolveRimThickness(scene, 50, scene.separation), 0);
  for (const invalid of [-1, 101, NaN, Infinity]) {
    assert.throws(() => resolveRimRecess(scene, invalid), RangeError);
  }
  for (const invalid of [-0.1, scene.separation + 0.1, NaN, Infinity]) {
    assert.throws(() => resolveRimThickness(scene, 50, invalid), RangeError);
  }
  assert.throws(() => resolveRimRecess({ separation: Infinity }, 50), RangeError);
});

test('full rim recess reaches the exact backdrop at every separation slider value', () => {
  for (let separation = 0; separation <= 100; separation++) {
    for (const fill of [0, 2, 5.8, MAX_CARD_THICKNESS_PIXELS]) {
      const scene = resolveSceneDepth(gapPercentPx(separation), fill);
      assert.equal(resolveRimThickness(scene, 100), scene.thickness);
      const front = resolveRimRecess(scene, 100);
      assert.equal(front, scene.separation);
      const thickness = resolveRimThickness(scene, 100, front);
      assert.equal(thickness, 0);
      assert.doesNotThrow(() => projectRimBackPoint({ x: 0, y: 0 }, scene.separation, 256, 240, thickness, front));
    }
  }
});

test('nearby point-light projection spreads before camera scaling and collapses at contact',()=>{
 const paperScene=resolveSceneDepth(nativePx(3),0);
 const halfScene=resolveSceneDepth(nativePx(3),2);
 const contactGap=resolveSceneDepth(nativePx(3),0).separation;
 const contactScene={separation:contactGap,thickness:contactGap,clearance:0};
 const independentlyProjected=(scene,width=256,height=240)=>{
  const caster=scene.thickness,receiver=scene.separation;
  const t=(receiver-caster)/(48+caster);
  const magnification=1+t;
  const cameraScale=1/(1+receiver*.015);
  return {
   scale:cameraScale*magnification,
   x:0,
   y:height*.7*t*cameraScale,
   blur:(receiver-caster)*1.5/magnification,
  };
 };
 const paper=gameplayShadowProjection(paperScene);
 const half=gameplayShadowProjection(halfScene);
 const contact=gameplayShadowProjection(contactScene);
 for(const [scene,result] of [[paperScene,paper],[halfScene,half],[contactScene,contact]]){
  const expected=independentlyProjected(scene);
  for(const key of Object.keys(expected))assert.ok(Math.abs(result[key]-expected[key])<1e-12,`${key} matches point-light ray projection`);
 }
 assert.equal(contact.blur,0);
 assert.equal(contact.x,0);
 assert.equal(contact.y,0);
 assert.equal(contact.scale,1/(1+contactScene.separation*.015));
 assert.ok(paper.scale>half.scale && half.scale>contact.scale);
 assert.equal(paper.x,0);
 assert.equal(half.x,0);
 assert.ok(paper.y>half.y && half.y>contact.y);
 assert.ok(paper.blur>half.blur && half.blur>contact.blur);
 const wide=gameplayShadowProjection(paperScene,512,480);
 const wideExpected=independentlyProjected(paperScene,512,480);
 assert.ok(Math.abs(wide.x-wideExpected.x)<1e-12);
 assert.ok(Math.abs(wide.y-wideExpected.y)<1e-12);
 const shadowPoint=(point)=>({
  x:128+(point.x-128)*paper.scale+paper.x,
  y:120+(point.y-120)*paper.scale+paper.y,
 });
 assert.ok(shadowPoint({x:24,y:12}).x<24,'camera-projected shadows on the left spread left');
 assert.ok(shadowPoint({x:232,y:228}).x>232,'camera-projected shadows on the right spread right');
 assert.ok(shadowPoint({x:24,y:12}).y>12,'near-top lighting gives a readable downward shadow');
 assert.ok(shadowPoint({x:232,y:228}).y>228,'bottom shadows spread downward after camera projection');
 assert.ok(half.x===0 && half.y>0,
  'a thin physical sheet retains a directional backdrop shadow');
});

test('off-axis backdrop shadows add the camera view offset once without changing light spread or blur', () => {
  setSceneView(0, 0);
  try {
    const scene = resolveSceneDepth(gapPercentPx(70), 2);
    const centered = gameplayShadowProjection(scene, 320, 200);
    setSceneView(0.2, -0.15);
    const offAxis = gameplayShadowProjection(scene, 320, 200);
    const cameraScale = 1 / (1 + scene.separation * 0.015);
    const viewOffset = getSceneViewOffset(320, 200);
    assert.equal(offAxis.scale, centered.scale, 'view movement preserves point-light magnification');
    assert.equal(offAxis.blur, centered.blur, 'view movement preserves shadow softness');
    assert.ok(Math.abs(offAxis.x - centered.x - viewOffset.x * (1 - cameraScale)) < 1e-12);
    assert.ok(Math.abs(offAxis.y - centered.y - viewOffset.y * (1 - cameraScale)) < 1e-12);

    const contactGap = resolveSceneDepth(gapPercentPx(70), 0).separation;
    const contactScene = { separation: contactGap, thickness: contactGap, clearance: 0 };
    setSceneView(0, 0);
    const centeredContact = gameplayShadowProjection(contactScene, 320, 200);
    setSceneView(0.2, -0.15);
    const offAxisContact = gameplayShadowProjection(contactScene, 320, 200);
    assert.equal(offAxisContact.scale, centeredContact.scale);
    assert.equal(offAxisContact.blur, 0);
    assert.ok(Math.abs(offAxisContact.x - centeredContact.x - 64 * (1 - cameraScale)) < 1e-12,
      'camera projection still moves the coincident receiver when the view is off-axis');
  } finally {
    setSceneView(0, 0);
  }
});

test('a room-rotated light behind the foreground disables backdrop shadow output safely', () => {
  const identity = [1, 0, 0, 0, 1, 0, 0, 0, 1];
  const behind = [-1, 0, 0, 0, 1, 0, 0, 0, -1];
  try {
    setSceneLightRotation(behind);
    const shadow = gameplayShadowProjection(resolveSceneDepth(MAX_SCENE_DISTANCE_PIXELS, 0));
    assert.equal(shadow.opacity, 0);
    assert.ok(Object.values(shadow).every(Number.isFinite));
  } finally {
    setSceneLightRotation(identity);
  }
});
