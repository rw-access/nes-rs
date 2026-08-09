#[macro_use]
extern crate lazy_static;

pub mod apu;
pub(crate) mod bus;
pub mod cartridge;
pub mod console;
pub mod controller;
pub mod cpu;
mod dsp;
pub mod headless;
pub mod ines;
mod instructions;
pub(crate) mod ppu;
pub mod snapshot;

pub use console::{
    AudioBlock, AudioQueueAction, AudioQueuePacer, Console, FrameOutput, AUDIO_SAMPLE_RATE,
    FRAME_HEIGHT, FRAME_WIDTH,
};
