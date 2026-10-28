import test from "node:test";
import assert from "node:assert/strict";
import { MultiChannelResampler, SpatialAudio } from "./spatial-audio.js";

function join(chunks) {
  const channels = chunks[0].length;
  return Array.from({ length: channels }, (_, ch) => {
    const length = chunks.reduce((total, block) => total + block[ch].length, 0);
    const output = new Float32Array(length);
    let offset = 0;
    for (const block of chunks) {
      output.set(block[ch], offset);
      offset += block[ch].length;
    }
    return output;
  });
}

class FakeParam {
  constructor(value = 0) { this.value = value; this.events = []; }
  cancelScheduledValues(time) { this.events.push(["cancel", time]); }
  cancelAndHoldAtTime(time) { this.events.push(["hold", time]); }
  setValueAtTime(value, time) { this.value = value; this.events.push(["set", value, time]); }
  linearRampToValueAtTime(value, time) { this.value = value; this.events.push(["ramp", value, time]); }
}

class FakeNode {
  constructor(kind) {
    this.kind = kind;
    this.connections = [];
    this.gain = new FakeParam(1);
    this.pan = new FakeParam(0);
    this.events = {};
  }
  connect(destination, output, input) { this.connections.push({ destination, output, input }); }
  addEventListener(name, callback) { this.events[name] = callback; }
  start(time) { this.startedAt = time; }
  stop() { this.stopped = true; }
}

class FakeBuffer {
  constructor(channels, frames, sampleRate) {
    this.numberOfChannels = channels;
    this.length = frames;
    this.sampleRate = sampleRate;
    this.data = Array.from({ length: channels }, () => new Float32Array(frames));
  }
  copyToChannel(data, channel) { this.data[channel].set(data); }
  getChannelData(channel) { return this.data[channel]; }
}

class FakeAudioContext {
  constructor() {
    this.currentTime = 0;
    this.sampleRate = 48000;
    this.state = "suspended";
    this.destination = new FakeNode("destination");
    this.nodes = [];
    this.buffers = [];
    this.sources = [];
    this.listeners = {};
  }
  createGain() { const node = new FakeNode("gain"); this.nodes.push(node); return node; }
  createStereoPanner() { const node = new FakeNode("panner"); this.nodes.push(node); return node; }
  createChannelSplitter(count) { const node = new FakeNode("splitter"); node.numberOfOutputs = count; this.nodes.push(node); return node; }
  createBuffer(channels, frames, sampleRate) { const buffer = new FakeBuffer(channels, frames, sampleRate); this.buffers.push(buffer); return buffer; }
  createBufferSource() { const source = new FakeNode("source"); this.sources.push(source); return source; }
  addEventListener(name, callback) { this.listeners[name] = callback; }
  setState(state) { this.state = state; this.listeners.statechange?.(); }
  async resume() { this.setState("running"); }
}

test("stateful 48-to-44.1 kHz resampling matches one whole block and variable chunks", () => {
  const samples = 997;
  const input = Array.from({ length: 6 }, (_, ch) => Float32Array.from(
    { length: samples }, (_, i) => Math.sin((i + ch * 13) * 0.037) * (ch + 1),
  ));
  const whole = new MultiChannelResampler(6);
  const expected = join([whole.process(input, 48000, 44100), whole.finish()]);

  const chunked = new MultiChannelResampler(6);
  const blocks = [];
  let offset = 0;
  for (const size of [1, 29, 7, 128, 3, 411, 2, 416]) {
    if (offset >= samples) break;
    const end = Math.min(samples, offset + size);
    blocks.push(chunked.process(input.map((channel) => channel.slice(offset, end)), 48000, 44100));
    offset = end;
  }
  if (offset < samples) blocks.push(chunked.process(input.map((channel) => channel.slice(offset)), 48000, 44100));
  blocks.push(chunked.finish());
  const actual = join(blocks);
  const expectedCount = Math.ceil(samples * 44100 / 48000);
  for (let ch = 0; ch < 6; ch += 1) {
    assert.equal(actual[ch].length, expectedCount);
    assert.equal(expected[ch].length, expectedCount);
    for (let i = 0; i < expectedCount; i += 1) assert.ok(Math.abs(actual[ch][i] - expected[ch][i]) < 1e-6);
  }
});

test("resampling keeps phase and output count through tiny chunks at 16 kHz", () => {
  const samples = 103;
  const input = Array.from({ length: 6 }, (_, ch) => Float32Array.from(
    { length: samples }, (_, i) => i * 0.01 + ch,
  ));
  const resampler = new MultiChannelResampler(6);
  const blocks = [];
  for (let i = 0; i < samples; i += 1) {
    blocks.push(resampler.process(input.map((channel) => channel.slice(i, i + 1)), 48000, 16000));
  }
  blocks.push(resampler.finish());
  const output = join(blocks);
  assert.equal(output[0].length, Math.ceil(samples / 3));
  for (let ch = 1; ch < 6; ch += 1) assert.equal(output[ch].length, output[0].length);
  for (let i = 0; i < output[0].length; i += 1) assert.ok(Math.abs(output[0][i] - Math.min(i * 3, samples - 1) * 0.01) < 1e-6);
});

test("long irregular 48-to-44.1 kHz stream preserves total duration", () => {
  const samples = 100_003;
  const input = [Float32Array.from({ length: samples }, (_, i) => Math.sin(i * 0.01))];
  const resampler = new MultiChannelResampler(1);
  const outputs = [];
  let offset = 0;
  let size = 17;
  while (offset < samples) {
    const end = Math.min(samples, offset + size);
    outputs.push(resampler.process([input[0].slice(offset, end)], 48000, 44100));
    offset = end;
    size = (size * 37) % 1009 + 1;
  }
  outputs.push(resampler.finish());
  assert.equal(join(outputs)[0].length, Math.ceil(samples * 44100 / 48000));
});

test("dry graph schedules six aligned channels on one continuous clock", async () => {
  const audio = new SpatialAudio();
  audio.contextConstructor = FakeAudioContext;
  await audio.enable();
  const ctx = audio.context;
  const mono = Float32Array.from([0.1, 0.2, 0.3, 0.4]);
  const stems = Float32Array.from({ length: mono.length * 5 }, (_, i) => i / 100);
  audio.schedule(mono, stems, 48000);
  audio.schedule(mono, stems, 48000);
  const scheduled = ctx.sources.filter((source) => source.buffer?.numberOfChannels === 6);
  assert.equal(scheduled.length, 2);
  assert.equal(scheduled[0].buffer.numberOfChannels, 6);
  assert.equal(scheduled[0].startedAt, 0.05);
  assert.ok(Math.abs(scheduled[1].startedAt - (scheduled[0].startedAt + 4 / 48000)) < 1e-9);
  assert.deepEqual([...scheduled[0].buffer.data[0]], [...mono]);
  for (let i = 0; i < mono.length; i += 1) {
    assert.ok(Math.abs(scheduled[0].buffer.data[1][i] - i * 0.05) < 1e-7);
  }
  assert.equal(scheduled[0].connections[0].destination, audio.graph.splitter);
  const ramps = audio.graph.fadeGain.gain.events.filter((event) => event[0] === "ramp");
  assert.equal(ramps.length, 1);
  assert.ok(Math.abs(ramps[0][2] - (scheduled[0].startedAt + 0.015)) < 1e-9);
  assert.equal(audio.pendingFadeIn, false);
});

test("width uses the mono bypass at zero and crossfades to panned stems", async () => {
  const audio = new SpatialAudio();
  audio.contextConstructor = FakeAudioContext;
  await audio.enable();
  assert.equal(audio.supported, true);
  assert.equal(audio.graph.monoGain.gain.value, 1);
  assert.equal(audio.graph.stemGain.gain.value, 0);
  audio.setWidth(0.5);
  assert.equal(audio.graph.monoGain.gain.value, 0);
  assert.equal(audio.graph.stemGain.gain.value, 1);
  assert.equal(audio.graph.panners[0].pan.value, -0.325);
  assert.equal(audio.graph.panners[1].pan.value, 0.325);
  assert.ok(audio.graph.monoGain.gain.events.some((event) => event[0] === "ramp"));
  assert.throws(() => audio.setWidth(1.1), RangeError);
});

test("flush, mute, suspension, and malformed stems cannot accumulate queued output", async () => {
  const errors = [];
  const audio = new SpatialAudio({ onError: (error) => errors.push(error) });
  audio.contextConstructor = FakeAudioContext;
  await audio.enable();
  const mono = new Float32Array(32).fill(0.25);
  audio.schedule(mono, new Float32Array(31), 48000);
  assert.equal(errors.length, 1);
  assert.equal(audio.context.sources.filter((source) => source.buffer?.numberOfChannels === 6).length, 0);
  audio.schedule(mono, new Float32Array(160), 48000);
  const scheduled = audio.context.sources.find((source) => source.buffer?.numberOfChannels === 6);
  audio.flush();
  assert.equal(scheduled.stopped, true);
  assert.equal(audio.sources.size, 0);
  audio.schedule(mono, new Float32Array(160), 48000);
  audio.toggleMute();
  assert.equal(audio.muted, true);
  assert.equal(audio.sources.size, 0);
  assert.equal(audio.pendingFadeIn, true);
  const sourceCount = audio.context.sources.length;
  audio.schedule(mono, new Float32Array(160), 48000);
  assert.equal(audio.context.sources.length, sourceCount);
  audio.toggleMute();
  audio.schedule(mono, new Float32Array(160), 48000);
  assert.equal(audio.pendingFadeIn, false);
  audio.context.state = "suspended";
  audio.context.listeners.statechange();
  assert.equal(audio.pendingFadeIn, true);
  const preSuspend = audio.context.sources.find((source) => source.buffer?.numberOfChannels === 6);
  assert.equal(preSuspend.stopped, true);
  audio.schedule(mono, new Float32Array(160), 48000);
  assert.equal(audio.sources.size, 0);
  await audio.enable();
  audio.schedule(mono, new Float32Array(160), 48000);
  const resumed = [...audio.sources][0];
  assert.ok(resumed.startedAt >= audio.context.currentTime + audio.prefillSeconds);
  assert.equal(audio.pendingFadeIn, false);
});
