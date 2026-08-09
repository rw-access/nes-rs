use std::io::Cursor;

use js_sys::{Float32Array, Uint8Array};
use nes::{cartridge, ines, Console, VideoBuffer, AUDIO_SAMPLE_RATE};
use wasm_bindgen::prelude::*;

/// A browser-owned emulator instance. The core remains responsible for
/// emulation; this wrapper owns the presentation buffers and the JS boundary.
#[wasm_bindgen]
pub struct NesWeb {
    console: Console,
    video: VideoBuffer,
    audio: Vec<f32>,
    rgba_js: Uint8Array,
    audio_js: Float32Array,
    controller_bits: u8,
    audio_discontinuity_pending: bool,
    last_frame: FrameMetadata,
}

/// Metadata for the most recently stepped frame. Video and audio payloads are
/// available through `NesWeb::rgba_buffer` and `NesWeb::audio_buffer`.
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct FrameMetadata {
    frame_number: u64,
    width: u32,
    height: u32,
    audio_sample_rate: u32,
    audio_samples: usize,
    audio_discontinuity: bool,
}

#[wasm_bindgen]
impl FrameMetadata {
    #[wasm_bindgen(getter)]
    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[wasm_bindgen(getter)]
    pub fn audio_sample_rate(&self) -> u32 {
        self.audio_sample_rate
    }

    #[wasm_bindgen(getter)]
    pub fn audio_samples(&self) -> usize {
        self.audio_samples
    }

    #[wasm_bindgen(getter)]
    pub fn audio_discontinuity(&self) -> bool {
        self.audio_discontinuity
    }
}

/// An opaque, cloneable point-in-time state for save/restore UI controls.
#[wasm_bindgen]
pub struct Snapshot {
    state: nes::ConsoleState,
    controller_bits: u8,
}

#[wasm_bindgen]
impl NesWeb {
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8]) -> Result<NesWeb, JsValue> {
        let (cartridge, mapper_number) = ines::load(&mut Cursor::new(rom))
            .ok_or_else(|| JsValue::from_str("invalid iNES ROM"))?;
        let mapper = cartridge::new(cartridge, mapper_number)
            .ok_or_else(|| JsValue::from_str("unsupported or invalid mapper"))?;

        Ok(Self {
            console: Console::new(mapper),
            video: VideoBuffer::new(),
            audio: Vec::with_capacity((AUDIO_SAMPLE_RATE / 55) as usize + 8),
            rgba_js: Uint8Array::new_with_length(nes::video::FRAME_RGBA_BYTES as u32),
            audio_js: Float32Array::new_with_length(4096),
            controller_bits: 0,
            audio_discontinuity_pending: false,
            last_frame: FrameMetadata {
                frame_number: 0,
                width: nes::video::FRAME_WIDTH as u32,
                height: nes::video::FRAME_HEIGHT as u32,
                audio_sample_rate: AUDIO_SAMPLE_RATE,
                audio_samples: 0,
                audio_discontinuity: false,
            },
        })
    }

    /// Alias for callers who prefer a named loader over the JS constructor.
    pub fn load_rom(rom: &[u8]) -> Result<NesWeb, JsValue> {
        Self::new(rom)
    }

    /// Emulate one frame and refresh the reusable RGBA/audio buffers.
    pub fn step_frame(&mut self) -> FrameMetadata {
        let frame = self.console.next_frame();
        frame.copy_to(&mut self.video);
        self.rgba_js.copy_from(self.video.rgba());

        self.audio.clear();
        self.audio.extend_from_slice(frame.audio_samples);
        if self.audio.len() > self.audio_js.length() as usize {
            self.audio_js = Float32Array::new_with_length(self.audio.len() as u32);
        }
        self.audio_js.copy_from(&self.audio);
        if self.audio.len() < self.audio_js.length() as usize {
            self.audio_js
                .subarray(self.audio.len() as u32, self.audio_js.length())
                .fill(0.0, 0, self.audio_js.length());
        }
        self.last_frame = FrameMetadata {
            frame_number: frame.frame_number,
            width: nes::video::FRAME_WIDTH as u32,
            height: nes::video::FRAME_HEIGHT as u32,
            audio_sample_rate: frame.audio_sample_rate,
            audio_samples: self.audio.len(),
            audio_discontinuity: frame.audio_discontinuity || self.audio_discontinuity_pending,
        };
        self.audio_discontinuity_pending = false;
        self.last_frame
    }

    pub fn frame_metadata(&self) -> FrameMetadata {
        self.last_frame
    }

    /// Return a JS-owned RGBA typed array, not a view into WASM memory.
    pub fn rgba_buffer(&self) -> Uint8Array {
        self.rgba_js.clone()
    }

    /// Return a JS-owned audio typed array. Use `FrameMetadata::audio_samples`
    /// as the valid prefix length; it is not a view into WASM memory.
    pub fn audio_buffer(&self) -> Float32Array {
        self.audio_js.clone()
    }

    pub fn set_controller(&mut self, bits: u8) {
        self.controller_bits = bits;
        self.console
            .update_buttons(nes::controller::ButtonState::from_bits(bits));
    }

    pub fn controller_bits(&self) -> u8 {
        self.controller_bits
    }

    pub fn rewind(&mut self) {
        self.console.rewind();
        self.console
            .update_buttons(nes::controller::ButtonState::from_bits(
                self.controller_bits,
            ));
    }

    pub fn save_snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.console.snapshot(),
            controller_bits: self.controller_bits,
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: &Snapshot) {
        self.console
            .restore_snapshot_and_reset_timeline(snapshot.state.clone());
        self.controller_bits = snapshot.controller_bits;
        self.console
            .update_buttons(nes::controller::ButtonState::from_bits(
                self.controller_bits,
            ));
        self.audio_discontinuity_pending = true;
    }

    pub fn width(&self) -> u32 {
        nes::video::FRAME_WIDTH as u32
    }

    pub fn height(&self) -> u32 {
        nes::video::FRAME_HEIGHT as u32
    }

    pub fn audio_sample_rate(&self) -> u32 {
        AUDIO_SAMPLE_RATE
    }
}
