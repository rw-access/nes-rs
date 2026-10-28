const CHANNELS = 6;
const PAN_POSITIONS = [-0.65, 0.65, 0, 0.3, -0.25];
const CROSSFADE_SECONDS = 0.015;
const MAX_BACKLOG_SECONDS = 0.2;

/** Stateful planar linear resampler. All channels share one sample cursor. */
export class MultiChannelResampler {
  constructor(channelCount = CHANNELS) {
    this.channelCount = channelCount;
    this.reset();
  }

  reset() {
    this.rate = 0;
    this.step = 1;
    this.position = 0;
    this.pending = Array.from({ length: this.channelCount }, () => new Float32Array(0));
  }

  process(channels, sourceRate, targetRate) {
    if (!Number.isFinite(sourceRate) || sourceRate <= 0 || !Number.isFinite(targetRate) || targetRate <= 0) {
      throw new RangeError("Sample rates must be positive finite numbers");
    }
    if (!Array.isArray(channels) || channels.length !== this.channelCount || channels.some((c) => !(c instanceof Float32Array))) {
      throw new TypeError(`Expected ${this.channelCount} Float32Array channels`);
    }
    if (this.rate && (this.rate.source !== sourceRate || this.rate.target !== targetRate)) this.reset();
    this.rate = { source: sourceRate, target: targetRate };
    this.step = sourceRate / targetRate;
    const count = channels[0].length;
    if (channels.some((c) => c.length !== count)) throw new RangeError("All channels must have the same sample count");

    const combined = channels.map((channel, index) => {
      const old = this.pending[index];
      const joined = new Float32Array(old.length + count);
      joined.set(old);
      joined.set(channel, old.length);
      return joined;
    });
    const output = Array.from({ length: this.channelCount }, () => []);
    const length = combined[0].length;
    while (this.position + 1 < length) {
      const left = Math.floor(this.position);
      const fraction = this.position - left;
      for (let ch = 0; ch < this.channelCount; ch += 1) {
        const data = combined[ch];
        output[ch].push(data[left] * (1 - fraction) + data[left + 1] * fraction);
      }
      this.position += this.step;
    }

    // Keep the sample at floor(position) so the next block can interpolate
    // across the boundary. The cursor is always common to every channel.
    const consumed = Math.min(Math.floor(this.position), length);
    this.pending = combined.map((data) => data.slice(consumed));
    this.position -= consumed;
    return output.map((samples) => Float32Array.from(samples));
  }

  /** Emit the one or two boundary samples whose right neighbor never arrived. */
  finish() {
    const output = Array.from({ length: this.channelCount }, () => []);
    const length = this.pending[0]?.length ?? 0;
    if (length) {
      while (this.position < length) {
        const left = Math.min(Math.floor(this.position), length - 1);
        const right = Math.min(left + 1, length - 1);
        const fraction = this.position - Math.floor(this.position);
        for (let ch = 0; ch < this.channelCount; ch += 1) {
          const data = this.pending[ch];
          output[ch].push(data[left] * (1 - fraction) + data[right] * fraction);
        }
        this.position += this.step;
      }
    }
    this.reset();
    return output.map((samples) => Float32Array.from(samples));
  }
}

export class SpatialAudio {
  constructor({ onState = () => {}, onError = () => {} } = {}) {
    this.onState = onState;
    this.onError = onError;
    this.context = null;
    this.contextConstructor = globalThis.AudioContext || globalThis.webkitAudioContext || null;
    this.muted = false;
    this.width = 0;
    this.supported = false;
    this.nextTime = 0;
    this.prefillSeconds = 0.05;
    this.sources = new Set();
    this.resampler = new MultiChannelResampler(CHANNELS);
    this.graph = null;
    this.pendingFadeIn = true;
    this.lastContextState = null;
  }

  _notify() {
    this.onState({
      context: this.context,
      state: this.context?.state ?? "unavailable",
      muted: this.muted,
      width: this.width,
      supported: this.supported,
      sampleRate: this.context?.sampleRate ?? null,
    });
  }

  _param(param, value, duration = CROSSFADE_SECONDS) {
    if (!param) return;
    const now = this.context.currentTime;
    try {
      if (typeof param.cancelAndHoldAtTime === "function") {
        param.cancelAndHoldAtTime(now);
      } else {
        param.cancelScheduledValues(now);
        param.setValueAtTime(param.value, now);
      }
      param.linearRampToValueAtTime(value, now + duration);
    } catch (_) { param.value = value; }
  }

  _buildGraph() {
    const ctx = this.context;
    const splitter = ctx.createChannelSplitter(CHANNELS);
    splitter.channelCount = CHANNELS;
    splitter.channelCountMode = "explicit";
    splitter.channelInterpretation = "discrete";
    const monoGain = ctx.createGain();
    const stemGain = ctx.createGain();
    const fadeGain = ctx.createGain();
    const outputGain = ctx.createGain();
    const panners = [];
    monoGain.connect(fadeGain);
    stemGain.connect(fadeGain);
    fadeGain.connect(outputGain);
    outputGain.connect(ctx.destination);
    splitter.connect(monoGain, 0, 0);
    for (let i = 0; i < 5; i += 1) {
      if (typeof ctx.createStereoPanner === "function") {
        const panner = ctx.createStereoPanner();
        const calibrationGain = ctx.createGain();
        panner.pan.value = PAN_POSITIONS[i] * this.width;
        splitter.connect(panner, i + 1, 0);
        // A centered equal-power panner emits 1/sqrt(2) per output channel.
        calibrationGain.gain.value = Math.SQRT2;
        panner.connect(calibrationGain);
        calibrationGain.connect(stemGain);
        panners.push(panner);
      } else {
        splitter.connect(stemGain, i + 1, 0);
      }
    }
    this.supported = panners.length === 5;
    outputGain.gain.value = this.muted ? 0 : 1;
    fadeGain.gain.value = 1;
    const spatial = this.supported && this.width > 0;
    monoGain.gain.value = spatial ? 0 : 1;
    stemGain.gain.value = spatial ? 1 : 0;
    this.graph = { splitter, monoGain, stemGain, fadeGain, outputGain, panners };
  }

  async enable() {
    if (!this.contextConstructor) throw new Error("Web Audio is not supported by this browser");
    // iPadOS may otherwise use the ambient/ringer audio session.
    if (globalThis.navigator?.audioSession) {
      try { globalThis.navigator.audioSession.type = "playback"; }
      catch (error) { this.onError(error); }
    }
    if (!this.context) {
      this.context = new this.contextConstructor();
      this._buildGraph();
      this.lastContextState = this.context.state;
      if (this.context.addEventListener) {
        this.context.addEventListener("statechange", () => {
          const state = this.context.state;
          if (state !== "running" && this.lastContextState === "running") this.flush();
          this.lastContextState = state;
        });
      }
      this.nextTime = this.context.currentTime + this.prefillSeconds;
      // Start silence during the user gesture to unlock older iPad WebKit.
      const unlockBuffer = this.context.createBuffer(1, 1, this.context.sampleRate);
      const unlockSource = this.context.createBufferSource();
      unlockSource.buffer = unlockBuffer;
      unlockSource.connect(this.context.destination);
      unlockSource.start(0);
    }
    if (this.context.state !== "running") {
      this.flush();
      await this.context.resume();
    }
    if (this.context.state !== "running") throw new Error(`AudioContext is ${this.context.state}`);
    this._notify();
  }

  toggleMute() {
    this.muted = !this.muted;
    if (this.muted) this.flush();
    if (this.graph) this._param(this.graph.outputGain.gain, this.muted ? 0 : 1);
    this._notify();
    return this.muted;
  }

  setWidth(width) {
    if (!Number.isFinite(width) || width < 0 || width > 1) throw new RangeError("Width must be a number from 0 to 1");
    this.width = width;
    if (this.graph) {
      const spatial = this.supported && width > 0;
      this._param(this.graph.monoGain.gain, spatial ? 0 : 1);
      this._param(this.graph.stemGain.gain, spatial ? 1 : 0);
      for (let i = 0; i < this.graph.panners.length; i += 1) {
        this._param(this.graph.panners[i].pan, PAN_POSITIONS[i] * width);
      }
    }
    this._notify();
  }

  flush() {
    if (this.context) {
      for (const source of this.sources) {
        try { source.stop(); } catch (_) { /* already ended */ }
      }
      this.sources.clear();
      this.nextTime = this.context.currentTime + this.prefillSeconds;
    }
    this.resampler.reset();
    this.pendingFadeIn = true;
  }

  schedule(mono, stems, sampleRate, discontinuity = false) {
    if (discontinuity) this.flush();
    if (!(mono instanceof Float32Array) || !Number.isFinite(sampleRate) || sampleRate <= 0) {
      this.onError(new TypeError("Expected mono Float32Array and a positive sample rate"));
      return;
    }
    if (stems instanceof Float32Array && stems.length !== mono.length * 5) {
      this.onError(new RangeError(`Expected ${mono.length * 5} interleaved stem samples; received ${stems.length}`));
      return;
    }
    if (stems != null && !(stems instanceof Float32Array)) {
      this.onError(new TypeError("Expected interleaved stem Float32Array"));
      return;
    }
    if (!this.context || mono.length === 0) return;
    if (this.muted || this.context.state !== "running") {
      this.flush();
      return;
    }
    if (this.width > 0 && !(stems instanceof Float32Array)) {
      this.onError(new TypeError("Stereo width requires interleaved five-channel stems"));
      return;
    }

    // Discard stale queued audio before converting this block so the resampler
    // starts the replacement block with a clean shared sample phase.
    if (this.context.currentTime > this.nextTime + 0.002 || this.nextTime - this.context.currentTime > MAX_BACKLOG_SECONDS) {
      this.flush();
    }

    // Convert [p1,p2,tri,noise,dmc] interleaved frames to the common planar cursor.
    const channels = [mono];
    if (stems) {
      const parts = Array.from({ length: 5 }, () => new Float32Array(mono.length));
      for (let frame = 0, offset = 0; frame < mono.length; frame += 1, offset += 5) {
        for (let ch = 0; ch < 5; ch += 1) parts[ch][frame] = stems[offset + ch];
      }
      channels.push(...parts);
    } else {
      for (let ch = 0; ch < 5; ch += 1) channels.push(new Float32Array(mono.length));
    }

    let output;
    if (sampleRate === this.context.sampleRate) {
      this.resampler.reset();
      output = channels;
    } else {
      try { output = this.resampler.process(channels, sampleRate, this.context.sampleRate); }
      catch (error) { this.onError(error); return; }
    }
    const frames = output[0].length;
    if (!frames) return;

    const start = Math.max(this.nextTime, this.context.currentTime + 0.01);
    const buffer = this.context.createBuffer(CHANNELS, frames, this.context.sampleRate);
    for (let ch = 0; ch < CHANNELS; ch += 1) {
      if (buffer.copyToChannel) buffer.copyToChannel(output[ch], ch);
      else buffer.getChannelData(ch).set(output[ch]);
    }
    const source = this.context.createBufferSource();
    source.buffer = buffer;
    source.channelCount = CHANNELS;
    source.channelCountMode = "explicit";
    source.channelInterpretation = "discrete";
    source.connect(this.graph.splitter);
    source.start(start);
    if (this.pendingFadeIn) {
      const fade = this.graph.fadeGain.gain;
      try {
        fade.cancelScheduledValues(start);
        fade.setValueAtTime(0, start);
        fade.linearRampToValueAtTime(1, start + CROSSFADE_SECONDS);
      } catch (_) { fade.value = 1; }
      this.pendingFadeIn = false;
    }
    this.nextTime = start + frames / this.context.sampleRate;
    this.sources.add(source);
    const cleanup = () => this.sources.delete(source);
    if (source.addEventListener) source.addEventListener("ended", cleanup, { once: true });
    else source.onended = cleanup;
  }
}
