#[macro_use]
extern crate lazy_static;

pub mod apu;
pub(crate) mod bus;
pub mod cartridge;
pub mod console;
pub mod controller;
pub mod cpu;
mod dsp;
#[cfg(feature = "layered-render")]
pub mod ghost;
pub mod ines;
mod instructions;
pub(crate) mod ppu;
pub mod snapshot;
pub mod video;

pub use console::{Console, ConsoleState, FrameOutput, VideoOutput, AUDIO_SAMPLE_RATE};
pub use video::{
    expand_rgba, FrameBuffer, VideoBuffer, FRAME_HEIGHT, FRAME_PIXELS, FRAME_RGBA_BYTES,
    FRAME_WIDTH, NES_PALETTE_RGB, NES_PALETTE_RGBA, PALETTE_ENTRIES,
};

#[cfg(feature = "layered-render")]
pub use ghost::{GhostFrame, GhostLayer, GhostSprite, GhostTimeline, GHOST_BASE_OPACITY};

#[cfg(feature = "layered-render")]
pub use video::{FrameLayers, FrameMask, FramePixels, RenderLayer, RenderMode};
