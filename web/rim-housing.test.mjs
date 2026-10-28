import test from 'node:test';
import assert from 'node:assert/strict';
import { RimHousing, RIM_MATERIALS } from './rim-housing.js';
import { cameraDepthScale, projectDepthPoint } from './depth-mesh.js';
import { RIM_CORNER_FRACTION } from './rim-outline.js';
import { getSceneLightRevision, setSceneLightDistance } from './scene-lighting.js';
import { getSceneViewRevision, setSceneView } from './scene-view.js';

function fixture(width = 311) {
  const context = {
    faces: [], materials: [], paths: [], current: [], arcs: [],
    createLinearGradient(...points) {
      return { points, stops: [], addColorStop(offset, color) { this.stops.push({ offset, color }); } };
    },
    setTransform(...values) { this.transform = values; },
    beginPath() { this.current = []; this.arcs = []; },
    moveTo(x,y) { this.current.push({x,y}); },
    lineTo(x,y) { this.current.push({x,y}); },
    ellipse(x, y, rx, ry, rotation, from, to, reverse = false) { this.arcs.push({ x, y, rx, ry, rotation, from, to, reverse }); },
    closePath() {},
    fill(rule) {
      this.faces.push(this.current); this.materials.push(this.fillStyle); this.lastFillRule = rule;
      this.paths.push({ points: this.current, arcs: this.arcs, material: this.fillStyle, rule });
    },
  };
  const canvas = { getContext: () => context };
  const stage = { clientWidth: width };
  return { context, canvas, stage, renderer: new RimHousing(canvas,stage) };
}

function assertWallProjection(context, recess, thickness) {
  const rx = 256 * RIM_CORNER_FRACTION, ry = 240 * RIM_CORNER_FRACTION;
  const corners = [{ x: rx, y: ry }, { x: 256 - rx, y: ry }, { x: 256 - rx, y: 240 - ry }, { x: rx, y: 240 - ry }];
  const walls = context.paths.filter(path => typeof path.material === 'object');
  assert.equal(walls.length, 4);
  for (let edge = 0; edge < 4; edge++) {
    const next = (edge + 1) % 4;
    assert.equal(walls[edge].arcs.length, 4);
    const expected = [[edge, recess], [next, recess], [next, recess + thickness], [edge, recess + thickness]];
    for (let arc = 0; arc < 4; arc++) {
      const [corner, depth] = expected[arc], curve = walls[edge].arcs[arc];
      assert.deepEqual({ x: curve.x, y: curve.y }, projectDepthPoint(corners[corner], depth));
      const scale = cameraDepthScale(depth);
      assert.equal(curve.rx, rx * scale);
      assert.equal(curve.ry, ry * scale);
    }
    // Neighboring strips meet at the SAME half-corner endpoints at both depths.
    for (const [a, b, endpoint] of [[walls[edge].arcs[1], walls[next].arcs[0], 'front'], [walls[edge].arcs[2], walls[next].arcs[3], 'back']]) {
      const aAngle = endpoint === 'front' ? a.to : a.from;
      const bAngle = endpoint === 'front' ? b.from : b.to;
      assert.ok(Math.hypot(a.x + a.rx * Math.cos(aAngle) - b.x - b.rx * Math.cos(bAngle),
        a.y + a.ry * Math.sin(aAngle) - b.y - b.ry * Math.sin(bAngle)) < 1e-10);
    }
  }
  const undercoat = context.paths[0];
  assert.equal(undercoat.arcs.length, 16, 'one joined undercoat seals antialiased corner seams');
}

test('rim walls share rounded corner joins at the foreground plane and recede with the game camera', () => {
  for (const thickness of [.75,1.5,3,6]) {
    const {renderer,context}=fixture();
    renderer.paint(6,thickness);
    assertWallProjection(context, 0, thickness);
  }
});

test('charcoal walls use directional light with narrow bevels and separate rear shading', () => {
    const { renderer, context } = fixture();
    renderer.paint(3, 1.5);
    const rgb = style => style.match(/\d+/g).map(Number);
    const materials = context.materials.filter(material => typeof material === 'object');
    const midpoints = [{x:128,y:0,nx:0,ny:1}, {x:256,y:120,nx:-1,ny:0},
      {x:128,y:240,nx:0,ny:-1}, {x:0,y:120,nx:1,ny:0}];
    for (const [index, point] of midpoints.entries()) {
      const dx=128-point.x,dy=-48-point.y,dz=(-48-.75)*19.2;
      const shade=.45+.32*Math.max(0,(point.nx*dx+point.ny*dy)/Math.hypot(dx,dy,dz));
      const expected=RIM_MATERIALS.charcoal.side.map(channel=>Math.round(channel*shade));
      assert.deepEqual(rgb(materials[index].stops[1].color),expected,'rim shading follows the frontal light at its physical midpoint');
      assert.ok(shade>=.45,'directional lighting never drops below ambient shade');
    }
    for (const { points, stops } of materials) {
      const [x0, y0, x1, y1] = points;
      assert.ok(x0 === x1 || y0 === y1, 'shade follows depth perpendicular to each edge');
      const span = Math.hypot(x1 - x0, y1 - y0);
      assert.ok(stops[1].offset * span <= 0.35 + 1e-12, 'bevel does not grow with wall thickness');
      assert.ok(stops[1].offset <= 0.22);
      assert.ok(stops[2].offset >= 0.8);
      assert.equal(stops[1].color, stops[2].color, 'main face stays uniformly lit');
      for (let channel = 0; channel < 3; channel++) {
        assert.ok(rgb(stops[0].color)[channel] > rgb(stops[1].color)[channel]);
        assert.ok(rgb(stops[3].color)[channel] < rgb(stops[2].color)[channel]);
        assert.ok(rgb(stops[1].color)[channel] < 90, 'side stays charcoal rather than gray');
      }
    }
});

test('edge shading stays finite and ordered for thin and deep walls', () => {
  for (const thickness of [0.0001, 0.1, 1.5, 6]) {
    const { renderer, context } = fixture();
    renderer.paint(6, thickness);
    for (const { points, stops } of context.materials.filter(material => typeof material === 'object')) {
      assert.ok(points.every(Number.isFinite));
      assert.ok(stops.every(({ offset }, index) => Number.isFinite(offset)
        && offset >= 0 && offset <= 1 && (!index || offset >= stops[index - 1].offset)));
    }
  }
});

test('recessed rims project front and back together and expose one continuous front face', () => {
  for (const recess of [0.1, 1.5, 4]) {
    const { renderer, context } = fixture();
    renderer.paint(6, 2, 1, recess, 12);
    assertWallProjection(context, recess, 2);
    const innerFace = context.paths.at(-2), cabinet = context.paths.at(-1);
    const scale = cameraDepthScale(recess);
    assert.equal(innerFace.arcs.length, 4);
    assert.equal(cabinet.arcs.length, 4);
    for (let corner = 0; corner < 4; corner++) {
      const outer = cabinet.arcs[corner], inner = innerFace.arcs[corner];
      assert.ok(Math.hypot(outer.x - inner.x, outer.y - inner.y) < 1e-10, 'inner and outer corners are concentric');
      assert.ok(Math.abs(outer.rx - inner.rx - 12 * scale) < 1e-10);
      assert.ok(Math.abs(outer.ry - inner.ry - 12 * scale) < 1e-10);
    }
    assert.equal(innerFace.material, '#24252a');
    assert.equal(cabinet.material, '#111217', 'page surround covers backdrop outside the recessed frame');
    assert.equal(context.lastFillRule, 'evenodd', 'the front opening remains transparent');
  }
});

test('paper rims can recede and recess/width changes invalidate the housing cache', () => {
  const { renderer, context, canvas } = fixture();
  renderer.paint(3, 0, 1, 1.5, 12);
  assert.equal(canvas.hidden, false);
  assert.equal(context.faces.length, 2, 'paper paints its recessed front and cabinet surround');
  renderer.paint(3, 0, 1, 1.5, 12);
  assert.equal(context.faces.length, 2);
  renderer.paint(3, 0, 1, 3, 12);
  assert.equal(context.faces.length, 4);
  renderer.paint(3, 0, 1, 3, 6);
  assert.equal(context.faces.length, 6);
  renderer.paint(0, 0, 1, 0, 6);
  assert.equal(canvas.hidden, true);
});

test('console edge replaces both front and side material without changing geometry', () => {
  const { renderer, context } = fixture();
  renderer.paint(3, 0.75, 1, 0.75, 12);
  const originalFaces = structuredClone(context.faces);
  renderer.paint(3, 0.75, 1, 0.75, 12, 'console');
  assert.deepEqual(context.faces.slice(7), originalFaces);
  assert.equal(context.materials[12], RIM_MATERIALS.console.front);
  assert.equal(context.materials[13], '#111217', 'shell-colored edge does not recolor the page surround');
  for (const { stops } of context.materials.slice(7).filter(material => typeof material === 'object')) {
    const [r, g, b] = stops[1].color.match(/\d+/g).map(Number);
    assert.ok(r > g && g > b, 'warm beige hue is retained on the shaded wall');
  }
  renderer.paint(3, 0.75, 1, 0.75, 12, 'console');
  assert.equal(context.faces.length, 14, 'material is part of the cache key');
  assert.throws(() => renderer.paint(3, 0.75, 1, 0.75, 12, 'unknown'), RangeError);
});

test('recessed rim exterior follows the page color independently of material and invalidates its cache', () => {
  const { renderer, context } = fixture();
  renderer.paint(3, .5, 1, .75, .1, 'charcoal', '#111217');
  assert.equal(context.materials.at(-2), RIM_MATERIALS.charcoal.front);
  assert.equal(context.materials.at(-1), '#111217');
  assert.ok(!context.materials.includes(RIM_MATERIALS.console.front), 'charcoal opening never paints a beige cabinet');
  const count = context.faces.length;
  renderer.paint(3, .5, 1, .75, .1, 'charcoal', '#111217');
  assert.equal(context.faces.length, count);
  renderer.paint(3, .5, 1, .75, .1, 'charcoal', '#171d24');
  assert.equal(context.faces.length, count * 2, 'surround-only change repaints');
  assert.equal(context.materials.at(-2), RIM_MATERIALS.charcoal.front);
  assert.equal(context.materials.at(-1), '#171d24');
  renderer.paint(3, .5, 1, .75, .1, 'console', '#171d24');
  assert.equal(context.materials.at(-2), RIM_MATERIALS.console.front, 'explicit shell edge retains its warm material');
  assert.equal(context.materials.at(-1), '#171d24');
});

test('rim uses game backing-pixel rounding, caches unchanged geometry, and hides when flat', () => {
  const {renderer,canvas,context,stage}=fixture();
  renderer.paint(3,1.5,2);
  assert.equal(canvas.width,622);
  assert.equal(canvas.height,583);
  assert.equal(canvas.hidden,false);
  renderer.paint(3,1.5,2);
  assert.equal(context.faces.length,5);
  renderer.paint(3,.75,2);
  assert.equal(context.faces.length,10);
  stage.clientWidth=312;
  renderer.paint(3,.75,2);
  assert.equal(canvas.width,624);
  assert.equal(canvas.height,585);
  renderer.paint(3,0,2);
  assert.equal(canvas.hidden,true);
  renderer.paint(0,0,2);
  assert.equal(canvas.hidden,true);
});

test('light-only changes invalidate rim materials while an unchanged light hits the cache', () => {
  setSceneLightDistance(2);
  try {
    const { renderer, context } = fixture();
    renderer.paint(3, 1.5);
    const originalFillCount = context.faces.length;
    const originalWallColors = context.materials.filter(material => typeof material === 'object')
      .map(material => material.stops[1].color);
    const originalRevision = getSceneLightRevision();

    renderer.paint(3, 1.5);
    assert.equal(context.faces.length, originalFillCount, 'unchanged key avoids redundant wall fills');
    setSceneLightDistance(3);
    assert.equal(getSceneLightRevision(), originalRevision + 1);
    renderer.paint(3, 1.5);
    assert.ok(context.faces.length > originalFillCount, 'light movement repaints the housing');
    const changedWallColors = context.materials.slice(-4).map(material => material.stops[1].color);
    assert.notDeepEqual(changedWallColors, originalWallColors, 'the repainted side shades reflect the moved light');
    const changedFillCount = context.faces.length;
    renderer.paint(3, 1.5);
    assert.equal(context.faces.length, changedFillCount, 'the new revision is cached');

    setSceneLightDistance(2);
    renderer.paint(3, 1.5);
    assert.ok(context.faces.length > changedFillCount, 'restoring the preset repaints the original lighting');
  } finally {
    setSceneLightDistance(2);
  }
});

test('view-only changes reproject coherent rim faces and invalidate the housing cache', () => {
  setSceneView(0, 0);
  try {
    const { renderer, context } = fixture();
    renderer.paint(3, 1.5);
    const centeredCount = context.faces.length;
    const centeredRevision = getSceneViewRevision();

    setSceneView(.2, -.16);
    const movedRevision = getSceneViewRevision();
    assert.equal(movedRevision, centeredRevision + 1);
    renderer.paint(3, 1.5);
    assert.ok(context.faces.length > centeredCount, 'view offset invalidates the previous housing geometry');
    const allPaths = context.paths;
    context.paths = allPaths.slice(-5);
    assertWallProjection(context, 0, 1.5);
    context.paths = allPaths;

    const movedCount = context.faces.length;
    renderer.paint(3, 1.5);
    assert.equal(context.faces.length, movedCount, 'unchanged view reuses its housing cache');
    setSceneView(0, 0);
    renderer.paint(3, 1.5);
    assert.ok(context.faces.length > movedCount, 'zero offset restores centered geometry and invalidates the cache');
    const restoredPaths = context.paths;
    context.paths = restoredPaths.slice(-5);
    assertWallProjection(context, 0, 1.5);
    context.paths = restoredPaths;
  } finally {
    setSceneView(0, 0);
  }
});
