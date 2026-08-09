use crate::apu::ChannelMask;
use crate::{
    bus::MemoryBus,
    cartridge::{Cartridge, Mapper, MapperInstance},
    controller::ButtonState,
    cpu::CPU,
    ppu::Screen,
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
    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    scheduler_master_ticks: u64,
    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    ppu_master_ticks: u64,
}

impl ConsoleState {
    #[cfg(test)]
    pub(crate) fn frame_number(&self) -> u64 {
        self.frame_number
    }

    fn step_hardware_cycle<F: FnMut(f32)>(&mut self, screen: &mut Screen, process_sample: &mut F) {
        #[cfg(feature = "apu-disabled")]
        let _ = process_sample;
        self.bus.mapper.clock_cpu();
        #[cfg(not(feature = "apu-disabled"))]
        // DMC sample addresses are restricted to the cartridge range
        // ($8000-$ffff), so the callback never needs to access another bus
        // device. Borrowing the mapper separately keeps the APU in place and
        // avoids constructing a default APU on every hardware cycle.
        {
            let sample = {
                let apu = &mut self.bus.apu;
                let mapper = &self.bus.mapper;
                apu.step(|addr| mapper.read(addr))
            };
            if let Some(sample) = sample {
                process_sample(sample);
            }
        }
        self.bus.ppu.step_cpu_cycle(&mut self.bus.mapper, screen);
    }

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    fn step_timestamped_nrom<F: FnMut(f32)>(
        &mut self,
        screen: &mut Screen,
        process_sample: &mut F,
    ) {
        loop {
            let current_master_ticks = self.scheduler_master_ticks;
            let deadline = self.ppu_master_ticks + self.bus.ppu.next_scheduler_event_in_ticks();
            let may_access_ppu = self.cpu.prepare_timestamped(&self.bus);

            if may_access_ppu {
                self.scheduler_master_ticks = self.bus.ppu.catch_up_to(
                    self.ppu_master_ticks,
                    current_master_ticks,
                    &mut self.bus.mapper,
                    screen,
                );
                self.ppu_master_ticks = self.scheduler_master_ticks;
            }

            let cycles = self.cpu.step_timestamped(&mut self.bus);
            let target_master_ticks = current_master_ticks + cycles as u64 * 3;

            if may_access_ppu || target_master_ticks >= deadline {
                self.scheduler_master_ticks = self.bus.ppu.catch_up_to(
                    self.ppu_master_ticks,
                    target_master_ticks,
                    &mut self.bus.mapper,
                    screen,
                );
                self.ppu_master_ticks = self.scheduler_master_ticks;
                let _ = process_sample;
                return;
            }

            self.scheduler_master_ticks = target_master_ticks;
        }
    }

    fn step<F: FnMut(f32)>(&mut self, screen: &mut Screen, process_sample: &mut F) {
        #[cfg(not(feature = "apu-disabled"))]
        if self.bus.apu.dma_active() {
            // DMA stalls are observed between abstract CPU instructions. The
            // current CPU core has already dispatched an instruction before
            // this loop advances hardware cycles, so a request that becomes
            // due mid-instruction pauses at the next instruction boundary.
            self.step_hardware_cycle(screen, process_sample);
            return;
        }

        #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
        if matches!(&self.bus.mapper, MapperInstance::Nrom(_)) {
            self.step_timestamped_nrom(screen, process_sample);
            return;
        }

        let cycles = self.cpu.step(&mut self.bus, None); // Some(&mut stdout()));

        #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
        {
            for _ in 0..cycles {
                self.step_hardware_cycle(screen, process_sample);
            }
            self.scheduler_master_ticks += cycles as u64 * 3;
            self.ppu_master_ticks = self.scheduler_master_ticks;
        }

        #[cfg(not(all(feature = "timestamped-scheduler", feature = "apu-disabled")))]
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
            .map(|addr| self.state.bus.ppu.read_byte(&self.state.bus.mapper, *addr))
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
                    .write_byte(&mut self.state.bus.mapper, *addr, data);
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

    fn from_mapper(mapper: MapperInstance) -> Self {
        let mut console = Console {
            state: ConsoleState {
                bus: MemoryBus::new(mapper),
                cpu: CPU::default(),
                frame_number: 0,
                #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
                scheduler_master_ticks: 0,
                #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
                ppu_master_ticks: 0,
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

    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        Self::from_mapper(MapperInstance::Dynamic(mapper))
    }

    /// Construct a console with a cartridge using the mapper-0 fast path.
    pub fn new_nrom(cartridge: Cartridge) -> Self {
        Self::from_mapper(MapperInstance::new_nrom(cartridge))
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

        #[cfg(not(feature = "rewind-disabled"))]
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
    use crate::cartridge::{Cartridge, Mapper, MirroringMode, CHR, PRG};
    use crate::video::{VideoBuffer, FRAME_HEIGHT, FRAME_WIDTH};
    use std::{cell::Cell, rc::Rc};

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

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    #[derive(Clone)]
    struct CpuClockMapper {
        clocks: Rc<Cell<u32>>,
    }

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    impl Mapper for CpuClockMapper {
        fn mirror(&self) -> MirroringMode {
            MirroringMode::Horizontal
        }

        fn read(&self, _address: u16) -> u8 {
            0
        }

        fn write(&mut self, _address: u16, _data: u8) {}

        fn read_page(&self, _page: u8) -> Option<&[u8; 256]> {
            None
        }

        fn clock_cpu(&mut self) {
            self.clocks.set(self.clocks.get() + 1);
        }
    }

    #[derive(Clone)]
    struct MirrorChangingMapper {
        mode: Rc<Cell<MirroringMode>>,
    }

    impl Mapper for MirrorChangingMapper {
        fn mirror(&self) -> MirroringMode {
            self.mode.get()
        }

        fn read(&self, _address: u16) -> u8 {
            0xea
        }

        fn write(&mut self, address: u16, _data: u8) {
            if address == 0x8000 {
                self.mode.set(MirroringMode::Vertical);
            }
        }

        fn read_page(&self, _page: u8) -> Option<&[u8; 256]> {
            None
        }
    }

    fn patterned_nrom_cartridge() -> Cartridge {
        let mut prg = vec![[0; 0x4000]];
        let program = [
            0xa9, 0x08, 0x8d, 0x01, 0x20, // show background
            0xa9, 0x3f, 0x8d, 0x06, 0x20, // palette address high
            0xa9, 0x01, 0x8d, 0x06, 0x20, // palette address low
            0xa9, 0x01, 0x8d, 0x07, 0x20, // palette entry 1
            0xa9, 0x20, 0x8d, 0x06, 0x20, // nametable address high
            0xa9, 0x00, 0x8d, 0x06, 0x20, // nametable address low
            0xa9, 0x01, 0x8d, 0x07, 0x20, // tile 1 at the top-left
            0x4c, 0x00, 0x80, // loop
        ];
        prg[0][..program.len()].copy_from_slice(&program);
        prg[0][0x3ffc] = 0x00;
        prg[0][0x3ffd] = 0x80;

        let mut chr = [0; 0x2000];
        chr[0x10..0x18].fill(0xff);

        Cartridge {
            prg: Rc::new(PRG { banks: prg }),
            chr: CHR::ROM(Rc::new(vec![chr])),
            sram: vec![[0; 0x2000]],
            mirror: MirroringMode::Horizontal,
        }
    }

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    fn dma_and_midframe_register_nrom_cartridge() -> Cartridge {
        let mut prg = vec![[0; 0x4000]];
        let mut program = Vec::new();

        // Turn rendering on while OAM is still empty. The long NOP run puts
        // the DMA and following register writes in the visible portion of a
        // scanline, rather than hiding them in reset/vblank setup.
        program.extend_from_slice(&[
            0xa9, 0x1e, // LDA #$1e: show background and sprites
            0x8d, 0x01, 0x20, // STA $2001
        ]);
        program.extend(std::iter::repeat_n(0xea, 5_000)); // NOP

        // Install one visible sprite in CPU page $02, then start OAM DMA.
        // The timestamped scheduler must catch the PPU up before dispatching
        // this write, or earlier pixels can observe the new OAM contents.
        program.extend_from_slice(&[
            0xa9, 120, // LDA #$78: sprite Y
            0x8d, 0x00, 0x02, // STA $0200
            0xa9, 1, // LDA #$01: tile 1
            0x8d, 0x01, 0x02, // STA $0201
            0xa9, 0, // LDA #$00: attributes
            0x8d, 0x02, 0x02, // STA $0202
            0xa9, 40, // LDA #$28: sprite X
            0x8d, 0x03, 0x02, // STA $0203
            0xa9, 0, // LDA #$00: OAMADDR
            0x8d, 0x03, 0x20, // STA $2003
            0xa9, 2, // LDA #$02: DMA from page $02
            0x8d, 0x14, 0x40, // STA $4014
        ]);

        // Keep changing PPU-visible state after DMA. These writes exercise
        // synchronization at arbitrary instruction boundaries while the
        // renderer is active, including the scroll write toggle protocol.
        let loop_start = program.len();
        program.extend_from_slice(&[
            0xa9, 0x0e, // sprites off, background on
            0x8d, 0x01, 0x20, // STA $2001
            0xa9, 3, // PPUSCROLL X
            0x8d, 0x05, 0x20, // STA $2005
            0xa9, 0, // PPUSCROLL Y
            0x8d, 0x05, 0x20, // STA $2005
            0xa9, 0x1e, // sprites and background on
            0x8d, 0x01, 0x20, // STA $2001
            0xa9, 0, // PPUSCROLL X
            0x8d, 0x05, 0x20, // STA $2005
            0xa9, 0, // PPUSCROLL Y
            0x8d, 0x05, 0x20, // STA $2005
            0x4c, 0, 0, // JMP loop (patched below)
        ]);
        let loop_address = 0x8000 + loop_start as u16;
        let loop_operand = program.len() - 2;
        let [loop_lo, loop_hi] = loop_address.to_le_bytes();
        program[loop_operand] = loop_lo;
        program[loop_operand + 1] = loop_hi;

        prg[0][..program.len()].copy_from_slice(&program);
        prg[0][0x3ffc] = 0x00;
        prg[0][0x3ffd] = 0x80;

        let mut chr = [0; 0x2000];
        // Tile 1: solid low bitplane, giving the DMA-installed sprite a
        // distinctive nonzero footprint with palette entry 1.
        chr[0x10..0x18].fill(0xff);

        Cartridge {
            prg: Rc::new(PRG { banks: prg }),
            chr: CHR::ROM(Rc::new(vec![chr])),
            sram: vec![[0; 0x2000]],
            mirror: MirroringMode::Horizontal,
        }
    }

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    fn initialize_dma_differential_console(console: &mut Console) {
        // The source page and primary OAM start as $FF, so no sprite is
        // visible until the program's mid-frame DMA installs one.
        console.state.cpu.ram[0x200..0x300].fill(0xff);
        let source_page: &[u8; 256] = console.state.cpu.ram[0x200..0x300].try_into().unwrap();
        console.state.bus.ppu.write_dma(Some(source_page));
        console
            .state
            .bus
            .ppu
            .write_byte(&mut console.state.bus.mapper, 0x3f00, 0);
        console
            .state
            .bus
            .ppu
            .write_byte(&mut console.state.bus.mapper, 0x3f11, 1);
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
    fn nrom_static_path_matches_boxed_mapper_frames() {
        let cartridge = patterned_nrom_cartridge();
        let mut static_console = Console::new_nrom(cartridge.clone());
        let mut boxed_console = Console::new(crate::cartridge::new(cartridge, 0).unwrap());

        for _ in 0..10 {
            let static_frame = static_console.next_frame();
            let boxed_frame = boxed_console.next_frame();

            assert_eq!(static_frame.frame_number, boxed_frame.frame_number);
            assert_eq!(static_frame.pixels, boxed_frame.pixels);
            assert_eq!(static_frame.audio_samples, boxed_frame.audio_samples);
            assert_eq!(
                static_frame.audio_discontinuity,
                boxed_frame.audio_discontinuity
            );
            assert!(static_frame
                .pixels
                .iter()
                .flatten()
                .any(|&pixel| pixel != 0));
        }
    }

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    #[test]
    fn timestamped_nrom_matches_cycle_stepped_dma_and_midframe_registers() {
        let cartridge = dma_and_midframe_register_nrom_cartridge();
        let mut timestamped = Console::new_nrom(cartridge.clone());
        let mut cycle_stepped = Console::new(crate::cartridge::new(cartridge, 0).unwrap());
        initialize_dma_differential_console(&mut timestamped);
        initialize_dma_differential_console(&mut cycle_stepped);

        let mut saw_nonzero_pixel = false;
        for _ in 0..4 {
            let timestamped_frame = timestamped.next_frame();
            let cycle_stepped_frame = cycle_stepped.next_frame();

            assert_eq!(
                timestamped_frame.frame_number,
                cycle_stepped_frame.frame_number
            );
            assert_eq!(timestamped_frame.pixels, cycle_stepped_frame.pixels);
            saw_nonzero_pixel |= timestamped_frame
                .pixels
                .iter()
                .flatten()
                .any(|&pixel| pixel != 0);
        }
        assert!(saw_nonzero_pixel);
    }

    #[cfg(all(feature = "timestamped-scheduler", feature = "apu-disabled"))]
    #[test]
    fn timestamped_scheduler_preserves_dynamic_mapper_cpu_clocks() {
        let clocks = Rc::new(Cell::new(0));
        let mut console = Console::new(Box::new(CpuClockMapper {
            clocks: clocks.clone(),
        }));
        let mut screen = super::Screen::default();
        let mut process_sample = |_| {};

        console.state.step(&mut screen, &mut process_sample);

        assert_eq!(clocks.get(), 7);
    }

    #[test]
    fn nametable_mirroring_cache_refreshes_after_mapper_write() {
        let mode = Rc::new(Cell::new(MirroringMode::Horizontal));
        let mut console = Console::new(Box::new(MirrorChangingMapper { mode }));

        console
            .state
            .bus
            .ppu
            .write_byte(&mut console.state.bus.mapper, 0x2000, 0x11);
        console
            .state
            .cpu
            .write_byte(&mut console.state.bus, 0x8000, 0);
        console
            .state
            .bus
            .ppu
            .write_byte(&mut console.state.bus.mapper, 0x2800, 0x22);

        assert_eq!(
            console
                .state
                .bus
                .ppu
                .read_byte(&console.state.bus.mapper, 0x2000),
            0x22
        );
        assert_eq!(
            console
                .state
                .bus
                .ppu
                .read_byte(&console.state.bus.mapper, 0x2400),
            0
        );
    }

    #[cfg(all(not(feature = "apu-disabled"), not(feature = "rewind-disabled")))]
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

    #[cfg(not(feature = "rewind-disabled"))]
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

    #[cfg(all(not(feature = "apu-disabled"), not(feature = "rewind-disabled")))]
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

    #[cfg(not(feature = "apu-disabled"))]
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
