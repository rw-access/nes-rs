import test from "node:test";
import assert from "node:assert/strict";
import {
  MATTE, EGGSHELL, SATIN, MATERIAL_FINISHES,
  getMaterialFinish, setMaterialFinish, getMaterialRevision, createMaterialSampler,
} from "./material-finish.js";

const centered = { light: { x: 128, y: 120, z: -10 }, eye: { x: 128, y: 120, z: -10 } };

function withFinish(name, run) {
  const previous = getMaterialFinish();
  try { setMaterialFinish(name); return run(); }
  finally { setMaterialFinish(previous); }
}

test("the dry matte default has exactly zero glint and immutable profiles", () => {
  assert.equal(getMaterialFinish(), "matte");
  assert.equal(getMaterialRevision(), 0);
  assert.equal(MATERIAL_FINISHES.matte, MATTE);
  assert.equal(MATERIAL_FINISHES.eggshell, EGGSHELL);
  assert.equal(MATERIAL_FINISHES.satin, SATIN);
  assert.ok([MATERIAL_FINISHES, MATTE, EGGSHELL, SATIN].every(Object.isFrozen));
  const noPositionWork = { get x() { throw new Error("matte must not inspect rays"); } };
  const sample = createMaterialSampler({ light: noPositionWork, eye: noPositionWork });
  assert.equal(sample(128, 120), 0);
  assert.equal(sample(NaN, Infinity, undefined, null), 0);
  assert.equal(createMaterialSampler()(), 0);
});

test("only real valid finish changes increment the material cache revision", () => {
  const original = getMaterialFinish();
  const revision = getMaterialRevision();
  try {
    assert.equal(setMaterialFinish(original), false);
    assert.equal(getMaterialRevision(), revision);
    assert.equal(setMaterialFinish("eggshell"), true);
    assert.equal(getMaterialRevision(), revision + 1);
    assert.equal(setMaterialFinish("eggshell"), false);
    assert.equal(getMaterialRevision(), revision + 1);
    assert.equal(setMaterialFinish("satin"), true);
    assert.equal(getMaterialRevision(), revision + 2);
    for (const value of ["gloss", "Satin", "__proto__", "toString", "", null, 0, {}]) {
      assert.throws(() => setMaterialFinish(value), RangeError);
    }
    assert.equal(getMaterialRevision(), revision + 2);
    assert.equal(getMaterialFinish(), "satin");
  } finally { setMaterialFinish(original); }
});

test("zero strength skips specular work and nonzero strength scales coverage linearly", () => withFinish("satin", () => {
  assert.equal(createMaterialSampler({ strength: 0 })(NaN, Infinity), 0);
  const full = createMaterialSampler(centered)(128, 120);
  const half = createMaterialSampler({ ...centered, strength: 0.5 })(128, 120);
  assert.ok(full > 0 && full <= SATIN.maxGlint);
  assert.ok(Math.abs(half - full * 0.5) < 1e-12);
  for (const strength of [-0.1, 1.1, Infinity, NaN]) {
    assert.throws(() => createMaterialSampler({ ...centered, strength }), RangeError);
  }
}));

test("the highlight follows both the light and the eye, not a painted screen gradient", () => withFinish("satin", () => {
  const light = { x: 0, y: 120, z: -10 }, eye = { x: 256, y: 120, z: -10 };
  const initial = createMaterialSampler({ light, eye });
  assert.ok(initial(128, 120) > initial(64, 120));
  assert.ok(initial(128, 120) > initial(192, 120));
  const eyeMoved = createMaterialSampler({ light, eye: { ...eye, x: 384 } });
  assert.ok(eyeMoved(192, 120) > eyeMoved(128, 120), "moving the eye shifts the reflection right");
  const lightMoved = createMaterialSampler({ light: { ...light, x: -128 }, eye });
  assert.ok(lightMoved(64, 120) > lightMoved(128, 120), "moving the lamp shifts the reflection left");
}));

test("eggshell is broader and subtler than satin", () => {
  assert.ok(EGGSHELL.roughness > SATIN.roughness);
  assert.ok(EGGSHELL.maxGlint < SATIN.maxGlint);
  const shell = withFinish("eggshell", () => createMaterialSampler(centered));
  const satin = withFinish("satin", () => createMaterialSampler(centered));
  const shellPeak = shell(128, 120), satinPeak = satin(128, 120);
  assert.ok(shellPeak > 0 && shellPeak < satinPeak);
  assert.ok(shell(208, 120) / shellPeak > satin(208, 120) / satinPeak, "rougher eggshell retains a wider lobe");
});

test("front, back, grazing, degenerate and invalid rays always give bounded finite coverage", () => withFinish("satin", () => {
  for (const x of [-1e6, -100, 128, 256, 1e6]) {
    for (const lightZ of [-20, -1e-8, 0, 1e-8, 20]) {
      for (const eyeZ of [-20, -1e-8, 0, 1e-8, 20]) {
        const sample = createMaterialSampler({ light: { x, y: 120, z: lightZ }, eye: { x: 256 - x, y: 120, z: eyeZ } });
        for (const normal of [undefined, { x: 1, y: 0, z: 0 }, { x: 0, y: 0, z: 1 }, { x: 0, y: 0, z: 0 }, null]) {
          const value = sample(128, 120, 0, normal);
          assert.ok(Number.isFinite(value) && value >= 0 && value <= 0.18);
        }
        if (lightZ >= 0 || eyeZ >= 0) assert.equal(sample(128, 120), 0, "backlight or backview cannot reflect off the front");
      }
    }
  }
  const sample = createMaterialSampler(centered);
  for (const coordinate of [NaN, Infinity, -Infinity]) assert.equal(sample(coordinate, 120), 0);
  const grazing = createMaterialSampler({ light: { x: 128, y: 120, z: -10 }, eye: { x: 1e6, y: 120, z: -1e-8 } });
  assert.ok(grazing(128, 120) < 1e-10, "reflection fades continuously before the eye passes behind the surface");
}));

test("position scaling and inverse-transpose normals preserve reflection across canvas dimensions", () => withFinish("satin", () => {
  const light = { x: 180, y: 160, z: -10 }, eye = { x: 140, y: 110, z: -12 };
  const regular = createMaterialSampler({ light, eye });
  const resized = createMaterialSampler({
    light: { x: light.x * 2, y: light.y / 2, z: light.z },
    eye: { x: eye.x * 2, y: eye.y / 2, z: eye.z }, width: 512, height: 120,
  });
  const normal = { x: 0.01, y: 0.02, z: -1 };
  const a = regular(130, 125, 2, normal);
  const b = resized(260, 62.5, 2, { x: normal.x / 2, y: normal.y * 2, z: normal.z });
  assert.ok(a > 0);
  assert.ok(Math.abs(a - b) < 1e-12);
  const translated = createMaterialSampler({ light: { ...light, z: light.z + 3 }, eye: { ...eye, z: eye.z + 3 } });
  assert.ok(Math.abs(a - translated(130, 125, 5, normal)) < 1e-12);
}));

test("samplers snapshot material and ray constants until their owner rebuilds them", () => withFinish("satin", () => {
  const light = { ...centered.light }, eye = { ...centered.eye };
  const sample = createMaterialSampler({ light, eye });
  const original = sample(128, 120);
  light.z = 10; eye.z = 10;
  setMaterialFinish("matte");
  assert.equal(sample(128, 120), original);
  assert.equal(createMaterialSampler()(), 0);
}));

test("active finishes reject malformed sampler configuration", () => withFinish("eggshell", () => {
  for (const dimensions of [{ width: 0 }, { width: Infinity }, { height: -1 }, { height: NaN }]) {
    assert.throws(() => createMaterialSampler({ ...centered, ...dimensions }), RangeError);
  }
  for (const light of [null, {}, { x: 0, y: 0, z: NaN }]) assert.throws(() => createMaterialSampler({ ...centered, light }), TypeError);
  assert.throws(() => createMaterialSampler({ light: centered.light }), TypeError);
}));
