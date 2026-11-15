import test from "node:test";
import assert from "node:assert/strict";
import { CanvasMaterialLayer } from "./material-layer.js";
import { getMaterialFinish, setMaterialFinish } from "./material-finish.js";
import { getSceneShadingStrength, setSceneShadingStrength, getSceneLightDistance, setSceneLightDistance } from "./scene-lighting.js";

test("matte stays exact and cached; reflective finishes preserve alpha/source and clear on return to matte", () => {
  const originalFinish = getMaterialFinish(), originalStrength = getSceneShadingStrength(), originalDistance = getSceneLightDistance();
  const source = new Uint8ClampedArray(32*30*4);
  for (let offset=0; offset<source.length; offset+=4) source.set([50,100,150,offset%16?255:0],offset);
  const original = source.slice(); let uploads = 0;
  const context = {createImageData:(w,h)=>({data:new Uint8ClampedArray(w*h*4)}),putImageData:()=>{uploads+=1;}};
  const layer = new CanvasMaterialLayer({getContext:()=>context},32,30);
  try {
    setMaterialFinish("matte"); setSceneShadingStrength(1);
    layer.paint(source);
    assert.deepEqual(layer.image.data,source);
    setSceneLightDistance(originalDistance===2?3:2);
    assert.equal(layer.paint(source,false),false,"matte does not re-upload source artwork for light changes");
    assert.equal(uploads,1);
    setMaterialFinish("satin");
    assert.equal(layer.paint(source,false),true);
    assert.notDeepEqual(layer.image.data,source,"satin adds a light response");
    assert.deepEqual(source,original);
    for (let offset=3;offset<source.length;offset+=4) assert.equal(layer.image.data[offset],source[offset]);
    assert.equal(layer.paint(source,false),false);
    setMaterialFinish("matte");
    assert.equal(layer.paint(source,false),true);
    assert.deepEqual(layer.image.data,source,"returning to matte removes all reflective highlights");
    setMaterialFinish("eggshell"); setSceneShadingStrength(0);
    layer.paint(source,false);
    assert.deepEqual(layer.image.data,source,"zero shading also disables sheen");
  } finally {setMaterialFinish(originalFinish);setSceneShadingStrength(originalStrength);setSceneLightDistance(originalDistance);}
});
