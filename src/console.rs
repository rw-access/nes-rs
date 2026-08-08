use crate::apu::ChannelMask;
use crate::{
    apu::APU,
    bus::MemoryBus,
    cartridge::Mapper,
    controller::{ButtonState, Controller},
    cpu::CPU,
    ppu::{Screen, PPU},
    snapshot::RewindTape,
};

#[derive(Clone)]
pub struct ConsoleState {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
}

impl ConsoleState {
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
}

/// Samples produced while emulating one video frame.
pub struct AudioBlock {
    samples: Vec<f32>,
}

impl AudioBlock {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.samples.push(sample);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.samples
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AudioQueueAction {
    WaitForPrefill,
    Resume,
    Continue,
    Pace,
}

/// Keeps the SDL queue within a small latency window without tying emulation to wall-clock time.
pub struct AudioQueuePacer {
    prefill_samples: usize,
    target_samples: usize,
    max_samples: usize,
    started: bool,
}

impl AudioQueuePacer {
    pub fn new(block_samples: usize) -> Self {
        assert!(block_samples > 0);
        Self {
            prefill_samples: block_samples * 3,
            target_samples: block_samples * 3,
            max_samples: block_samples * 5,
            started: false,
        }
    }

    pub fn observe(&mut self, queued_samples: usize) -> AudioQueueAction {
        if !self.started {
            if queued_samples < self.prefill_samples {
                AudioQueueAction::WaitForPrefill
            } else {
                self.started = true;
                AudioQueueAction::Resume
            }
        } else if queued_samples >= self.max_samples {
            AudioQueueAction::Pace
        } else {
            AudioQueueAction::Continue
        }
    }

    pub fn target_samples(&self) -> usize {
        self.target_samples
    }

    pub fn reset(&mut self) {
        self.started = false;
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
    in_rewind: bool,
    audio_reset: AudioResetSignal,
}

impl Console {
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
        cpu_ignore: &Vec<u16>,
        ppu_ignore: &Vec<u16>,
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
    }

    pub fn rewind(&mut self) {
        if let Some(prev_state) = self.tape.pop_back(&mut self.screen) {
            self.state = prev_state;
            self.in_rewind = true;
            self.audio_reset.mark();
        }
    }

    /// Returns and clears the audio reset notification generated by a successful rewind.
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
        const INITIAL_TAPE_STEP: usize = 60; // 1 second buffered

        let mut console = Console {
            state: ConsoleState {
                bus: MemoryBus {
                    mapper,
                    ppu: PPU::default(),
                    apu: APU::default(),
                    controller: Controller::default(),
                },
                cpu: CPU::default(),
            },
            screen: Screen::default(),
            tape: RewindTape::new(INITIAL_TAPE_STEP),
            in_rewind: false,
            audio_reset: AudioResetSignal::default(),
        };

        console.state.bus.ppu.reset();
        console.state.cpu.reset(&mut console.state.bus);
        console
    }

    pub fn next_screen<F: FnMut(f32)>(&mut self, process_sample: F) -> &Screen {
        self.state.wait_vblank(&mut self.screen, process_sample);

        if !self.in_rewind {
            self.tape.push_back(self.state.clone());
        }

        self.in_rewind = false;
        &self.screen
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioBlock, AudioQueueAction, AudioQueuePacer, AudioResetSignal, Console};
    use crate::cartridge::{Mapper, MirroringMode};

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
    fn audio_block_is_reused_as_one_frame_sized_buffer() {
        let mut block = AudioBlock::with_capacity(3);
        block.push(0.25);
        block.push(-0.5);
        assert_eq!(block.as_slice(), &[0.25, -0.5]);

        block.clear();
        assert!(block.is_empty());
        block.push(1.0);
        assert_eq!(block.as_slice(), &[1.0]);
    }

    #[test]
    fn audio_queue_pacer_prefills_and_limits_queue_depth() {
        let mut pacer = AudioQueuePacer::new(800);
        assert_eq!(pacer.observe(799), AudioQueueAction::WaitForPrefill);
        assert_eq!(pacer.observe(2_400), AudioQueueAction::Resume);
        assert_eq!(pacer.observe(3_999), AudioQueueAction::Continue);
        assert_eq!(pacer.observe(4_000), AudioQueueAction::Pace);
        assert_eq!(pacer.target_samples(), 2_400);

        pacer.reset();
        assert_eq!(pacer.observe(2_399), AudioQueueAction::WaitForPrefill);
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
