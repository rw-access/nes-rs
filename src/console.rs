use crate::apu::ChannelMask;
use crate::{
    apu::APU,
    bus::MemoryBus,
    cartridge::Mapper,
    controller::{ButtonState, Controller},
    cpu::CPU,
    ppu::{Screen, PPU},
    snapshot::RewindTape,
    video::VideoBuffer,
};

pub use crate::video::{FRAME_HEIGHT, FRAME_WIDTH};

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;

/// A borrowed output view for one completed emulated video frame.
///
/// Pixels are NES palette indices, not RGB values. Audio is an ordered stream
/// collected during this frame; its length may vary slightly from frame to
/// frame and must not be assumed to be exactly sample_rate / 60.
pub struct FrameOutput<'a> {
    pub frame_number: u64,
    pub pixels: &'a [[u8; FRAME_WIDTH]; FRAME_HEIGHT],
    pub audio_samples: &'a [f32],
    pub audio_sample_rate: u32,
    pub audio_discontinuity: bool,
}

impl<'a> FrameOutput<'a> {
    /// Copy this borrowed frame into an owned, reusable flat video buffer.
    ///
    /// The destination owns both allocations and remains usable after the
    /// `FrameOutput` borrow ends. The copy also refreshes its RGBA view.
    pub fn copy_to(&self, buffer: &mut VideoBuffer) {
        buffer.set_screen(self.pixels);
    }
}

#[derive(Clone)]
pub struct ConsoleState {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
    frame_number: u64,
}

impl ConsoleState {
    #[cfg(test)]
    pub(crate) fn frame_number(&self) -> u64 {
        self.frame_number
    }

    fn step_hardware_cycle<F: FnMut(f32)>(&mut self, screen: &mut Screen, process_sample: &mut F) {
        self.bus.mapper.clock_cpu();
        // Temporarily take the APU so its DMC memory callback can read the
        // CPU address space without aliasing the mutable APU borrow.
        let mut apu = std::mem::take(&mut self.bus.apu);
        let sample = apu.step(|addr| self.cpu.read_byte(&self.bus, addr));
        self.bus.apu = apu;
        if let Some(sample) = sample {
            process_sample(sample);
        }
        for _ in 0..3 {
            self.bus.ppu.step(self.bus.mapper.as_mut(), screen);
        }
    }

    fn step<F: FnMut(f32)>(&mut self, screen: &mut Screen, process_sample: &mut F) {
        if self.bus.apu.dma_active() {
            // DMA stalls are observed between abstract CPU instructions. The
            // current CPU core has already dispatched an instruction before
            // this loop advances hardware cycles, so a request that becomes
            // due mid-instruction pauses at the next instruction boundary.
            self.step_hardware_cycle(screen, process_sample);
            return;
        }

        let cycles = self.cpu.step(&mut self.bus, None); // Some(&mut stdout()));
        for _ in 0..cycles {
            self.step_hardware_cycle(screen, process_sample);
        }
    }

    pub(crate) fn wait_vblank<F: FnMut(f32)>(
        &mut self,
        screen: &mut Screen,
        mut process_sample: F,
    ) {
        // only return on a positive edge
        while self.bus.ppu.in_vblank {
            self.step(screen, &mut process_sample);
        }

        while !self.bus.ppu.in_vblank {
            self.step(screen, &mut process_sample);
        }
    }

    pub(crate) fn advance_frame_number(&mut self) {
        self.frame_number += 1;
    }
}

#[derive(Default)]
struct AudioResetSignal {
    pending: bool,
}

impl AudioResetSignal {
    fn mark(&mut self) {
        self.pending = true;
    }

    fn take(&mut self) -> bool {
        let pending = self.pending;
        self.pending = false;
        pending
    }
}

pub struct Console {
    state: ConsoleState,
    tape: RewindTape,
    screen: Screen,
    audio_samples: Vec<f32>,
    in_rewind: bool,
    rewind_history_active: bool,
    rewind_exhausted: bool,
    audio_reset: AudioResetSignal,
}

impl Console {
    const INITIAL_TAPE_STEP: usize = 60;

    pub fn snapshot(&self) -> ConsoleState {
        self.state.clone()
    }

    /// Reads a byte through the CPU memory map without exposing mutable state.
    pub fn read_memory(&self, address: u16) -> u8 {
        self.state.cpu.read_byte(&self.state.bus, address)
    }

    pub fn restore_snapshot(
        &mut self,
        snapshot: ConsoleState,
        cpu_ignore: &[u16],
        ppu_ignore: &[u16],
    ) {
        // read preserved addresses
        let cpu_backup_contents: Vec<u8> = cpu_ignore
            .iter()
            .map(|addr| self.state.cpu.read_byte(&self.state.bus, *addr))
            .collect();
        let ppu_backup_contents: Vec<u8> = ppu_ignore
            .iter()
            .map(|addr| {
                self.state
                    .bus
                    .ppu
                    .read_byte(self.state.bus.mapper.as_ref(), *addr)
            })
            .collect();

        self.state = snapshot;

        // restore preserved addresses
        cpu_ignore
            .iter()
            .zip(cpu_backup_contents)
            .for_each(|(addr, data)| {
                self.state.cpu.write_byte(&mut self.state.bus, *addr, data);
            });
        ppu_ignore
            .iter()
            .zip(ppu_backup_contents)
            .for_each(|(addr, data)| {
                self.state
                    .bus
                    .ppu
                    .write_byte(self.state.bus.mapper.as_mut(), *addr, data);
            });

        self.reset_timeline();
    }

    /// Restore a snapshot as a new timeline, clearing rewind history and
    /// marking the next frame's audio as discontinuous.
    ///
    /// This is the safe restore operation for frontends that expose save
    /// states. Older callers that need memory-preservation exceptions can use
    /// [`restore_snapshot`](Self::restore_snapshot), which also resets the
    /// timeline after applying those exceptions.
    pub fn restore_snapshot_and_reset_timeline(&mut self, snapshot: ConsoleState) {
        self.state = snapshot;
        self.reset_timeline();
    }

    fn reset_timeline(&mut self) {
        self.tape = RewindTape::new(Self::INITIAL_TAPE_STEP);
        self.in_rewind = false;
        self.rewind_history_active = false;
        self.rewind_exhausted = false;
        self.audio_samples.clear();
        self.audio_reset.mark();
    }

    pub fn rewind(&mut self) -> bool {
        if let Some(prev_state) = self.tape.pop_back(&mut self.screen) {
            self.state = prev_state;
            self.in_rewind = true;
            self.rewind_history_active = true;
            self.rewind_exhausted = false;
            self.audio_reset.mark();
            true
        } else if self.rewind_history_active {
            self.in_rewind = true;
            self.rewind_exhausted = true;
            false
        } else {
            false
        }
    }

    /// Returns and clears the pending audio discontinuity notification.
    ///
    /// Most frontends should consume `FrameOutput::audio_discontinuity` from
    /// [`next_frame`](Self::next_frame) instead. This method remains available
    /// for older integrations that manage the notification separately.
    pub fn take_audio_reset(&mut self) -> bool {
        self.audio_reset.take()
    }

    pub fn update_buttons(&mut self, state: ButtonState) {
        self.state.bus.controller.update_buttons(state);
    }

    pub fn update_channel_mask(&mut self, toggle_mask: ChannelMask) {
        self.state.bus.apu.toggle_channel_mask(toggle_mask);
    }

    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        let mut console = Console {
            state: ConsoleState {
                bus: MemoryBus {
                    mapper,
                    ppu: PPU::default(),
                    apu: APU::default(),
                    controller: Controller::default(),
                },
                cpu: CPU::default(),
                frame_number: 0,
            },
            screen: Screen::default(),
            audio_samples: Vec::with_capacity((AUDIO_SAMPLE_RATE / 60) as usize + 1),
            tape: RewindTape::new(Self::INITIAL_TAPE_STEP),
            in_rewind: false,
            rewind_history_active: false,
            rewind_exhausted: false,
            audio_reset: AudioResetSignal::default(),
        };

        console.state.bus.ppu.reset();
        console.state.cpu.reset(&mut console.state.bus);
        console
    }

    /// Emulate one frame and return its video/audio output.
    pub fn next_frame(&mut self) -> FrameOutput<'_> {
        let audio_discontinuity = self.audio_reset.take();
        self.audio_samples.clear();

        if self.rewind_exhausted {
            self.in_rewind = false;
            self.rewind_exhausted = false;
            return FrameOutput {
                frame_number: self.state.frame_number,
                pixels: &self.screen.pixels,
                audio_samples: &self.audio_samples,
                audio_sample_rate: AUDIO_SAMPLE_RATE,
                audio_discontinuity,
            };
        }

        {
            let state = &mut self.state;
            let screen = &mut self.screen;
            let audio_samples = &mut self.audio_samples;
            state.wait_vblank(screen, |sample| audio_samples.push(sample));
        }

        if !self.in_rewind {
            self.tape.push_back(self.state.clone());
            self.rewind_history_active = false;
        }

        self.in_rewind = false;
        self.state.frame_number += 1;
        FrameOutput {
            frame_number: self.state.frame_number,
            pixels: &self.screen.pixels,
            audio_samples: &self.audio_samples,
            audio_sample_rate: AUDIO_SAMPLE_RATE,
            audio_discontinuity,
        }
    }

    /// Compatibility wrapper for callers that consume audio through a callback.
    pub fn next_screen<F: FnMut(f32)>(&mut self, mut process_sample: F) -> &Screen {
        let frame = self.next_frame();
        for &sample in frame.audio_samples {
            process_sample(sample);
        }
        &self.screen
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioResetSignal, Console};
    use crate::cartridge::{Mapper, MirroringMode};
    use crate::video::{VideoBuffer, FRAME_HEIGHT, FRAME_WIDTH};

    #[derive(Clone)]
    struct TestMapper;

    impl Mapper for TestMapper {
        fn mirror(&self) -> MirroringMode {
            MirroringMode::Horizontal
        }

        fn read(&self, _address: u16) -> u8 {
            0xea
        }

        fn write(&mut self, _address: u16, _data: u8) {}

        fn read_page(&self, _page: u8) -> Option<&[u8; 256]> {
            None
        }
    }

    #[test]
    fn audio_reset_signal_is_one_shot() {
        let mut signal = AudioResetSignal::default();
        assert!(!signal.take());

        signal.mark();
        assert!(signal.take());
        assert!(!signal.take());
    }

    #[test]
    fn frame_output_exposes_dimensions_rate_and_monotonic_sequence() {
        let mut console = Console::new(Box::new(TestMapper));

        let first = console.next_frame();
        assert_eq!(first.frame_number, 1);
        assert_eq!(first.pixels.len(), FRAME_HEIGHT);
        assert_eq!(first.pixels[0].len(), FRAME_WIDTH);
        assert_eq!(first.audio_sample_rate, super::AUDIO_SAMPLE_RATE);
        assert!(!first.audio_samples.is_empty());

        let second = console.next_frame();
        assert_eq!(second.frame_number, 2);
        assert!(!second.audio_samples.is_empty());

        console.rewind();
        let rewound = console.next_frame();
        assert!(rewound.audio_discontinuity);
        assert!(!rewound.audio_samples.is_empty());
    }

    #[test]
    fn rewind_frame_numbers_cross_checkpoints_one_at_a_time() {
        let mut console = Console::new(Box::new(TestMapper));
        for _ in 0..180 {
            let _ = console.next_frame();
        }

        for expected in (1..=180).rev() {
            assert!(console.rewind());
            let frame = console.next_frame();
            assert_eq!(frame.frame_number, expected);
        }
    }

    #[test]
    fn rewind_holds_at_the_oldest_available_frame() {
        let mut console = Console::new(Box::new(TestMapper));
        for _ in 0..3 {
            let _ = console.next_frame();
        }

        for _ in 0..3 {
            console.rewind();
            let _ = console.next_frame();
        }

        console.rewind();
        let held = console.next_frame();
        assert!(held.audio_samples.is_empty());

        let resumed = console.next_frame();
        assert!(!resumed.audio_samples.is_empty());
    }

    #[test]
    fn frame_output_copies_into_reusable_video_buffer() {
        let mut console = Console::new(Box::new(TestMapper));
        let output = console.next_frame();
        let mut buffer = VideoBuffer::new();

        output.copy_to(&mut buffer);

        assert_eq!(buffer.palette_indices().len(), FRAME_WIDTH * FRAME_HEIGHT);
        assert_eq!(buffer.rgba().len(), FRAME_WIDTH * FRAME_HEIGHT * 4);
        assert_eq!(buffer.palette_indices()[0], output.pixels[0][0]);
    }

    #[test]
    fn restoring_snapshot_starts_a_new_timeline_and_reports_audio_discontinuity() {
        let mut console = Console::new(Box::new(TestMapper));
        let snapshot = console.snapshot();

        let _ = console.next_frame();
        console.restore_snapshot_and_reset_timeline(snapshot);
        console.rewind();

        let frame = console.next_frame();
        assert!(frame.audio_discontinuity);
        assert_eq!(frame.frame_number, 1);
    }

    #[test]
    fn dmc_dma_pauses_cpu_progress_while_hardware_cycles_continue() {
        let console = Console::new(Box::new(TestMapper));
        let mut state = console.snapshot();
        let mut screen = super::Screen::default();
        let mut process_sample = |_| {};

        state.cpu.write_byte(&mut state.bus, 0x4012, 0x20);
        state.cpu.write_byte(&mut state.bus, 0x4013, 0x00);
        state.cpu.write_byte(&mut state.bus, 0x4015, 0x10);

        for _ in 0..3 {
            state.step_hardware_cycle(&mut screen, &mut process_sample);
        }
        assert!(state.bus.apu.dma_active());

        let cpu_before_dma = format!("{:?}", state.cpu);
        for _ in 0..3 {
            state.step(&mut screen, &mut process_sample);
            assert_eq!(format!("{:?}", state.cpu), cpu_before_dma);
        }
        assert!(!state.bus.apu.dma_active());
    }
}
