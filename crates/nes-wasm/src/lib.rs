use std::io::Cursor;

use js_sys::{Float32Array, Uint8Array};
use nes_core::ghost::GhostLayer;
use nes_core::{
    cartridge, ines, Console, RenderMode, VideoBuffer, AUDIO_SAMPLE_RATE, NES_PALETTE_RGBA,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// A browser-owned emulator instance. The core remains responsible for
/// emulation; this wrapper owns the presentation buffers and the JS boundary.
#[wasm_bindgen]
pub struct NesWeb {
    console: Console,
    video: VideoBuffer,
    display_mode: RenderMode,
    audio: Vec<f32>,
    rgba_js: Uint8Array,
    audio_js: Float32Array,
    ghost_buffer: Vec<u8>,
    ghost_buffer_js: Uint8Array,
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
    state: nes_core::ConsoleState,
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
            display_mode: RenderMode::Both,
            audio: Vec::with_capacity((AUDIO_SAMPLE_RATE / 55) as usize + 8),
            rgba_js: Uint8Array::new_with_length(nes_core::video::FRAME_RGBA_BYTES as u32),
            audio_js: Float32Array::new_with_length(4096),
            ghost_buffer: Vec::new(),
            ghost_buffer_js: Uint8Array::new_with_length(0),
            controller_bits: 0,
            audio_discontinuity_pending: false,
            last_frame: FrameMetadata {
                frame_number: 0,
                width: nes_core::video::FRAME_WIDTH as u32,
                height: nes_core::video::FRAME_HEIGHT as u32,
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
        let (frame_number, audio_sample_rate, audio_discontinuity) = {
            let frame = self.console.next_frame();
            frame.copy_mode_to(self.display_mode, &mut self.video);
            self.audio.clear();
            self.audio.extend_from_slice(frame.audio_samples);
            collect_ghost_layers(frame.ghost_layers, &mut self.ghost_buffer);
            (
                frame.frame_number,
                frame.audio_sample_rate,
                frame.audio_discontinuity,
            )
        };
        self.rgba_js.copy_from(self.video.rgba());

        if self.audio.len() > self.audio_js.length() as usize {
            self.audio_js = Float32Array::new_with_length(self.audio.len() as u32);
        }
        self.audio_js
            .subarray(0, self.audio.len() as u32)
            .copy_from(&self.audio);
        if self.audio.len() < self.audio_js.length() as usize {
            self.audio_js
                .subarray(self.audio.len() as u32, self.audio_js.length())
                .fill(0.0, 0, self.audio_js.length());
        }
        self.ghost_buffer_js = Uint8Array::new_with_length(self.ghost_buffer.len() as u32);
        self.ghost_buffer_js.copy_from(&self.ghost_buffer);
        self.last_frame = FrameMetadata {
            frame_number,
            width: nes_core::video::FRAME_WIDTH as u32,
            height: nes_core::video::FRAME_HEIGHT as u32,
            audio_sample_rate,
            audio_samples: self.audio.len(),
            audio_discontinuity: audio_discontinuity || self.audio_discontinuity_pending,
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

    /// Return JS-owned sparse [x, y, red, green, blue, alpha] records for the
    /// current frame of each active replay ghost, ordered oldest to newest.
    pub fn ghost_buffer(&self) -> Uint8Array {
        self.ghost_buffer_js.clone()
    }

    /// Cycle the display through sprites, background/tiles, and the normal
    /// composite. Returns the numeric mode (0 background, 1 sprites, 2 both).
    pub fn cycle_display_mode(&mut self) -> u8 {
        self.display_mode = self.display_mode.next();
        self.console
            .copy_last_frame_to(self.display_mode, &mut self.video);
        self.rgba_js.copy_from(self.video.rgba());
        self.display_mode as u8
    }

    pub fn display_mode(&self) -> u8 {
        self.display_mode as u8
    }

    pub fn set_controller(&mut self, bits: u8) {
        self.controller_bits = bits;
        self.console
            .update_buttons(nes_core::controller::ButtonState::from_bits(bits));
    }

    pub fn controller_bits(&self) -> u8 {
        self.controller_bits
    }

    pub fn rewind(&mut self) -> bool {
        self.console.rewind()
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
            .update_buttons(nes_core::controller::ButtonState::from_bits(
                self.controller_bits,
            ));
        self.audio_discontinuity_pending = true;
    }

    pub fn width(&self) -> u32 {
        nes_core::video::FRAME_WIDTH as u32
    }

    pub fn height(&self) -> u32 {
        nes_core::video::FRAME_HEIGHT as u32
    }

    pub fn audio_sample_rate(&self) -> u32 {
        AUDIO_SAMPLE_RATE
    }
}

fn collect_ghost_layers(layers: &[GhostLayer], buffer: &mut Vec<u8>) {
    buffer.clear();
    for layer in layers {
        let Some(frame) = layer.current_frame() else {
            continue;
        };
        let alpha = (layer.opacity().clamp(0.0, 1.0) * 255.0).round() as u8;
        for sprite in frame.sprites() {
            let [red, green, blue, _] = NES_PALETTE_RGBA[sprite.palette_index as usize & 0x3f];
            buffer.extend_from_slice(&[sprite.x, sprite.y, red, green, blue, alpha]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::collect_ghost_layers;
    use nes_core::ghost::GhostTimeline;
    use nes_core::{RenderLayer, FRAME_HEIGHT, FRAME_WIDTH, NES_PALETTE_RGBA};

    #[test]
    fn ghost_buffer_serializes_current_sparse_layer_with_alpha() {
        let mut sprite_layer = RenderLayer {
            pixels: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
            coverage: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
        };
        sprite_layer.pixels[12][34] = 0x3f;
        sprite_layer.coverage[12][34] = 1;

        let mut timeline = GhostTimeline::default();
        timeline.capture_frame(&sprite_layer);
        assert!(timeline.finish_capture());
        timeline.begin_frame();

        let mut buffer = Vec::new();
        collect_ghost_layers(timeline.layers(), &mut buffer);

        assert_eq!(buffer.len(), 6);
        assert_eq!(&buffer[..2], &[34, 12]);
        assert_eq!(&buffer[2..5], &NES_PALETTE_RGBA[0x3f][..3]);
        assert_eq!(buffer[5], 128);
    }

    #[test]
    fn ghost_buffer_orders_layers_oldest_to_newest_with_independent_opacity() {
        let mut first = RenderLayer {
            pixels: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
            coverage: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
        };
        first.pixels[2][1] = 0x01;
        first.coverage[2][1] = 1;
        let mut first_next = first.clone();
        first_next.pixels[2][1] = 0x02;

        let mut second = first.clone();
        second.pixels[2][1] = 0x03;

        let mut timeline = GhostTimeline::default();
        timeline.capture_frame(&first);
        timeline.capture_frame(&first_next);
        assert!(timeline.finish_capture());
        timeline.begin_frame();
        timeline.capture_frame(&second);
        assert!(timeline.finish_capture());
        timeline.begin_frame();

        let mut buffer = Vec::new();
        collect_ghost_layers(timeline.layers(), &mut buffer);

        assert_eq!(buffer.len(), 12);
        assert_eq!(
            &buffer[..5],
            &[
                1,
                2,
                NES_PALETTE_RGBA[0x02][0],
                NES_PALETTE_RGBA[0x02][1],
                NES_PALETTE_RGBA[0x02][2]
            ]
        );
        assert_eq!(buffer[5], 64);
        assert_eq!(
            &buffer[6..11],
            &[
                1,
                2,
                NES_PALETTE_RGBA[0x03][0],
                NES_PALETTE_RGBA[0x03][1],
                NES_PALETTE_RGBA[0x03][2]
            ]
        );
        assert_eq!(buffer[11], 128);
    }
}
