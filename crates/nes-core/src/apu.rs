use std::cell::Cell;

use crate::dsp::FirstOrderFilter;

const LENGTH_COUNTER_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

// The DMC rate table is specified in CPU cycles.  APU timers are clocked on
// every other CPU cycle, so the unit stores these periods divided by two.
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

lazy_static! {
    static ref PULSE_TABLE: [f32; 31] = {
        let mut table = [0.0; 31];
        for index in 0..31 {
            table[index] = 95.52 / (8128.0 / (index as f32) + 100.0);
        }

        table
    };
    static ref TND_TABLE: [f32; 203] = {
        let mut table = [0.0; 203];
        for index in 0..203 {
            table[index] = 163.67 / (24329.0 / (index as f32) + 100.0);
        }

        table
    };
}

// read in the order 0, 7, 6, 5, 4, 3, 2, 1.
const DUTY_TABLE: [u8; 4] = [
    0b01000000, // 12.5%
    0b01100000, // 25%
    0b01111000, // 50%
    0b10011111, // 75%
];

#[derive(Clone, Default)]
struct SweepUnit {
    enabled: bool,
    negate: bool,
    reload: bool,
    carry: bool,
    period: u8,
    shift_count: u8,
    delay: u8,
}

#[derive(Clone, Default)]
struct VolumeEnvelope {
    start: bool,
    loop_or_disabled: bool,
    use_constant_volume: bool,
    decay_level_counter: u8,
    divider_counter: u8,
    period_or_constant_volume: u8,
}

impl VolumeEnvelope {
    fn step(&mut self) {
        if self.start {
            self.decay_level_counter = 15;
            self.divider_counter = self.period_or_constant_volume;
            self.start = false;
        } else if self.divider_counter > 0 {
            self.divider_counter -= 1;
        } else {
            self.divider_counter = self.period_or_constant_volume;
            if self.decay_level_counter > 0 {
                self.decay_level_counter -= 1;
            } else if self.loop_or_disabled {
                self.decay_level_counter = 15;
            }
        }
    }

    fn volume(&self) -> u8 {
        if self.use_constant_volume {
            self.period_or_constant_volume
        } else {
            self.decay_level_counter
        }
    }
}

#[derive(Clone, Default)]
struct Pulse {
    sweep: SweepUnit,
    volume_envelope: VolumeEnvelope,
    duty_type: u8, // 12.5%, 25%, 50%, 75%
    duty_offset: u8,
    timer: u16, // 11 bits
    timer_period: u16,
    length_counter: u16, // should this be a u8?
    enabled: bool,
    reload: bool,
}

impl Pulse {
    fn step_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.timer_period;
            self.duty_offset = (self.duty_offset + 1) % 8;
        }
    }

    fn step_length(&mut self) {
        if !self.volume_envelope.loop_or_disabled && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn sweep_target_period(&self) -> u16 {
        let period = self.timer_period as i32;
        let change = (self.timer_period >> self.sweep.shift_count) as i32;

        if self.sweep.negate {
            // Pulse 1 has a ones-complement adder; pulse 2 has a
            // twos-complement adder. `carry` identifies pulse 1.
            period
                .saturating_sub(change)
                .saturating_sub(self.sweep.carry as i32)
                .max(0) as u16
        } else {
            period.saturating_add(change) as u16
        }
    }

    fn step_sweep(&mut self) {
        let divider_zero = self.sweep.delay == 0;
        let target_period = self.sweep_target_period();
        let muted = self.timer_period < 8 || target_period > 0x7ff;

        if divider_zero && self.sweep.enabled && self.sweep.shift_count != 0 && !muted {
            self.timer_period = target_period;
        }

        if divider_zero || self.sweep.reload {
            self.sweep.delay = self.sweep.period;
            self.sweep.reload = false;
        } else {
            self.sweep.delay -= 1;
        }
    }

    fn sample(&self) -> u8 {
        let high = ((DUTY_TABLE[self.duty_type as usize] >> (7 - self.duty_offset)) & 1) != 0;
        let silenced = self.timer_period < 8 || self.sweep_target_period() > 0x7ff;

        if !self.enabled || self.length_counter == 0 || !high || silenced {
            0
        } else {
            self.volume_envelope.volume()
        }
    }
}

#[derive(Clone, Default)]
struct Triangle {
    timer: u16, // 11 bits
    timer_period: u16,
    length_counter: u16, // should this be a u8?
    linear_counter_period: u16,
    linear_counter_offset: u16,
    phase: u16,
    length_enabled: bool,
    enabled: bool,
    reload: bool,
}

impl Triangle {
    fn step_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.timer_period;
            if self.length_counter > 0 && self.linear_counter_offset > 0 {
                self.phase = (self.phase + 1) % 32;
            }
        }
    }

    fn step_linear_counter(&mut self) {
        if self.reload {
            self.linear_counter_offset = self.linear_counter_period;
        } else if self.linear_counter_offset > 0 {
            self.linear_counter_offset -= 1;
        }

        if !self.length_enabled {
            self.reload = false;
        }
    }

    fn step_length(&mut self) {
        if !self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn sample(&self) -> u8 {
        if self.enabled
            && self.timer_period > 0
            && self.length_counter > 0
            && self.linear_counter_offset > 0
        {
            15u8.wrapping_sub(self.phase as u8) ^ 0u8.wrapping_sub((self.phase >= 16) as u8)
        } else {
            0
        }
    }
}

#[derive(Clone)]
struct Noise {
    volume_envelope: VolumeEnvelope,
    length_counter: u16,
    shift_register: u16, // 15 bits
    enabled: bool,
    mode: bool,
    period: u16, // should this be u8?
    timer: u16,  // should this be u8?
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            volume_envelope: VolumeEnvelope::default(),
            length_counter: 0,
            shift_register: 1,
            enabled: false,
            mode: false,
            period: 0,
            timer: 0,
        }
    }
}

impl Noise {
    fn step_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.period;
            let tap = if self.mode { 6 } else { 1 };
            let feedback = (self.shift_register ^ (self.shift_register >> tap)) & 1;
            self.shift_register = (self.shift_register >> 1) | (feedback << 14);
        }
    }

    fn step_length(&mut self) {
        if !self.volume_envelope.loop_or_disabled && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn sample(&self) -> u8 {
        let silenced = self.shift_register & 1 == 1;
        if !self.enabled || self.length_counter == 0 || silenced {
            0
        } else {
            self.volume_envelope.volume()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmcDmaKind {
    Load,
    Reload,
}

#[derive(Clone)]
struct Dmc {
    irq_enabled: bool,
    loop_flag: bool,
    rate_index: u8,
    timer: u16,
    timer_period: u16,

    output_level: u8,
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,

    sample_buffer: Option<u8>,
    shift_register: u8,
    bits_remaining: u8,
    silence: bool,
    irq_pending: bool,

    dma_kind: Option<DmcDmaKind>,
    dma_cycles_remaining: u8,
    load_dma_delay: u8,
}

impl Default for Dmc {
    fn default() -> Self {
        Self {
            irq_enabled: false,
            loop_flag: false,
            rate_index: 0,
            timer: 0,
            timer_period: DMC_RATE_TABLE[0] / 2,
            output_level: 0,
            sample_address: 0xc000,
            sample_length: 1,
            current_address: 0xc000,
            bytes_remaining: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: 0,
            silence: true,
            irq_pending: false,
            dma_kind: None,
            dma_cycles_remaining: 0,
            load_dma_delay: 0,
        }
    }
}

impl Dmc {
    fn write_control(&mut self, data: u8) {
        self.irq_enabled = data & 0x80 != 0;
        self.loop_flag = data & 0x40 != 0;
        self.rate_index = data & 0x0f;
        self.timer_period = DMC_RATE_TABLE[self.rate_index as usize] / 2;

        if !self.irq_enabled {
            self.irq_pending = false;
        }
    }

    fn write_direct_load(&mut self, data: u8) {
        self.output_level = data & 0x7f;
    }

    fn write_sample_address(&mut self, data: u8) {
        self.sample_address = 0xc000 | ((data as u16) << 6);
    }

    fn write_sample_length(&mut self, data: u8) {
        self.sample_length = ((data as u16) << 4) | 1;
    }

    fn restart(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.bytes_remaining = 0;
            self.load_dma_delay = 0;
            self.dma_kind = None;
            self.dma_cycles_remaining = 0;
        } else if self.bytes_remaining == 0 {
            self.restart();
            if self.sample_buffer.is_none() {
                self.load_dma_delay = 2;
            }
        }
    }

    fn read_sample_byte<F: FnMut(u16) -> u8>(&mut self, read_memory: &mut F) {
        if self.sample_buffer.is_some() || self.bytes_remaining == 0 {
            return;
        }

        self.sample_buffer = Some(read_memory(self.current_address));
        self.current_address = if self.current_address == 0xffff {
            0x8000
        } else {
            self.current_address + 1
        };
        self.bytes_remaining -= 1;

        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart();
            } else if self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    fn dma_cycles(kind: DmcDmaKind, scheduled_on_get: bool) -> u8 {
        match (kind, scheduled_on_get) {
            // Load DMA normally halts on a get cycle; reload DMA normally
            // halts on a put cycle. A phase mismatch adds/removes alignment.
            (DmcDmaKind::Load, true) => 3,
            (DmcDmaKind::Load, false) => 4,
            (DmcDmaKind::Reload, true) => 3,
            (DmcDmaKind::Reload, false) => 4,
        }
    }

    fn schedule_dma(&mut self, kind: DmcDmaKind, scheduled_on_get: bool) {
        if self.sample_buffer.is_some()
            || self.bytes_remaining == 0
            || self.dma_cycles_remaining != 0
        {
            return;
        }

        self.dma_kind = Some(kind);
        self.dma_cycles_remaining = Self::dma_cycles(kind, scheduled_on_get);
    }

    fn dma_active(&self) -> bool {
        self.dma_cycles_remaining != 0
    }

    fn step_load_schedule(&mut self, apu_phase_is_get: bool) {
        if self.load_dma_delay == 0 {
            return;
        }

        self.load_dma_delay -= 1;
        if self.load_dma_delay == 0 {
            self.schedule_dma(DmcDmaKind::Load, apu_phase_is_get);
        }
    }

    fn step_dma<F: FnMut(u16) -> u8>(&mut self, read_memory: &mut F) {
        if self.dma_cycles_remaining == 0 {
            return;
        }

        self.dma_cycles_remaining -= 1;
        if self.dma_cycles_remaining == 0 {
            debug_assert!(self.dma_kind.take().is_some());
            self.read_sample_byte(read_memory);
        }
    }

    fn clock_output(&mut self) {
        if self.bits_remaining == 0 {
            if let Some(sample) = self.sample_buffer.take() {
                self.shift_register = sample;
                self.bits_remaining = 8;
                self.silence = false;
            } else {
                self.silence = true;
                self.bits_remaining = 8;
            }
        }

        if !self.silence {
            if self.shift_register & 1 != 0 {
                self.output_level = self.output_level.saturating_add(2).min(127);
            } else {
                self.output_level = self.output_level.saturating_sub(2);
            }
        }

        self.shift_register >>= 1;
        self.bits_remaining -= 1;
    }

    fn step_timer(&mut self, apu_phase_is_get: bool) {
        let output_cycle_ended = self.bits_remaining == 0;
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.timer_period.saturating_sub(1);
            self.clock_output();

            if output_cycle_ended
                && self.sample_buffer.is_none()
                && self.bytes_remaining > 0
                && self.load_dma_delay == 0
                && !self.dma_active()
            {
                // Reload DMA attempts to halt on a put cycle.
                self.schedule_dma(DmcDmaKind::Reload, !apu_phase_is_get);
            }
        }
    }
}

#[derive(Clone)]
pub struct ChannelMask {
    pub pulse1: bool,
    pub pulse2: bool,
    pub triangle: bool,
    pub noise: bool,
}

impl Default for ChannelMask {
    fn default() -> Self {
        Self {
            pulse1: true,
            pulse2: true,
            triangle: true,
            noise: true,
        }
    }
}

#[derive(Clone)]
pub(crate) struct APU {
    pulses: [Pulse; 2],
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,
    channel_mask: ChannelMask,

    cycles_x_sample_freq: u32,
    sample_freq: u32,
    frame_counter_cycle: u32,
    frame_counter_step: usize,
    frame_counter_reset_delay: u8,
    pending_five_step: bool,
    register_write_timing: Option<(bool, u8)>,

    on_sample_edge: bool,
    use_five_step: bool,
    enable_irq: bool,
    pending_irq: Cell<bool>,
    on_apu_cycle: bool,

    first_order_filters: [FirstOrderFilter; 3],
}

impl Default for APU {
    fn default() -> Self {
        let mut pulses = [Pulse::default(), Pulse::default()];
        // The first pulse's negate adder uses the ones' complement form.
        pulses[0].sweep.carry = true;

        APU {
            pulses,
            triangle: Triangle::default(),
            noise: Noise::default(),
            dmc: Dmc::default(),
            channel_mask: ChannelMask::default(),

            cycles_x_sample_freq: 0,
            sample_freq: 48_000,
            frame_counter_cycle: 0,
            frame_counter_step: 0,
            frame_counter_reset_delay: 0,
            pending_five_step: false,
            register_write_timing: None,

            on_sample_edge: false,
            use_five_step: false,
            enable_irq: false,
            pending_irq: Cell::new(false),
            on_apu_cycle: true,
            first_order_filters: [
                FirstOrderFilter::high_pass(48_000.0, 90.0),
                FirstOrderFilter::high_pass(48_000.0, 440.0),
                FirstOrderFilter::low_pass(48_000.0, 14_000.0),
            ],
        }
    }
}

impl APU {
    pub(crate) fn toggle_channel_mask(&mut self, toggle_mask: ChannelMask) {
        self.channel_mask = ChannelMask {
            pulse1: self.channel_mask.pulse1 ^ toggle_mask.pulse1,
            pulse2: self.channel_mask.pulse2 ^ toggle_mask.pulse2,
            triangle: self.channel_mask.triangle ^ toggle_mask.triangle,
            noise: self.channel_mask.noise ^ toggle_mask.noise,
        }
    }

    pub(crate) fn set_sample_freq(&mut self, sample_freq: u32) {
        self.sample_freq = sample_freq;
    }

    pub(crate) fn irq_line(&self) -> bool {
        self.pending_irq.get() || self.dmc.irq_pending
    }

    pub(crate) fn phase_after_cpu_cycles(&self, cycles: u8) -> bool {
        self.on_apu_cycle ^ (cycles & 1 != 0)
    }

    pub(crate) fn set_register_write_timing(&mut self, write_phase: bool, instruction_cycles: u8) {
        self.register_write_timing = Some((write_phase, instruction_cycles));
    }

    pub(crate) fn clear_register_write_timing(&mut self) {
        self.register_write_timing = None;
    }

    pub(crate) fn dma_active(&self) -> bool {
        self.dmc.dma_active()
    }

    fn update_ticks(&mut self) {
        const CPU_FREQ: u32 = 1_789_773;

        // calculate sample rate
        self.cycles_x_sample_freq = self.cycles_x_sample_freq.wrapping_add(self.sample_freq);
        if self.cycles_x_sample_freq >= CPU_FREQ {
            self.cycles_x_sample_freq -= CPU_FREQ;
            self.on_sample_edge = true;
        } else {
            self.on_sample_edge = false;
        }

        self.on_apu_cycle = !self.on_apu_cycle;
    }

    pub(crate) fn step<F: FnMut(u16) -> u8>(&mut self, mut read_memory: F) -> Option<f32> {
        self.dmc.step_dma(&mut read_memory);

        if self.on_apu_cycle {
            // pulse + noise + DMC
            self.pulses[0].step_timer();
            self.pulses[1].step_timer();
            self.noise.step_timer();
            self.dmc.step_load_schedule(self.on_apu_cycle);
            self.dmc.step_timer(self.on_apu_cycle);
        }

        self.triangle.step_timer();

        self.step_frame_counter();

        let sample = if self.on_sample_edge {
            Some(self.sample())
        } else {
            None
        };

        self.update_ticks();

        sample
    }

    fn step_frame_counter(&mut self) {
        if self.frame_counter_reset_delay > 0 {
            self.frame_counter_reset_delay -= 1;
            if self.frame_counter_reset_delay == 0 {
                self.use_five_step = self.pending_five_step;
                self.frame_counter_cycle = 0;
                self.frame_counter_step = 0;

                if self.use_five_step {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
            }
            return;
        }

        // NESdev's frame sequencer timings, expressed in CPU cycles from the
        // reset of the sequence. The underlying sequencer advances on every
        // other CPU clock; these are the resulting observable CPU-cycle
        // positions of its quarter/half-frame events.
        const FOUR_STEP_CYCLES: [u32; 4] = [7457, 14913, 22371, 29829];
        const FIVE_STEP_CYCLES: [u32; 5] = [7457, 14913, 22371, 29829, 37281];

        self.frame_counter_cycle += 1;
        let schedule = if self.use_five_step {
            &FIVE_STEP_CYCLES[..]
        } else {
            &FOUR_STEP_CYCLES[..]
        };

        if self.frame_counter_cycle != schedule[self.frame_counter_step] {
            return;
        }

        match self.frame_counter_step {
            0 | 2 => self.clock_quarter_frame(),
            1 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            3 if self.use_five_step => {}
            3 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
                if self.enable_irq {
                    self.pending_irq.set(true);
                }
            }
            4 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            _ => unreachable!(),
        }

        self.frame_counter_step += 1;
        if self.frame_counter_step == schedule.len() {
            self.frame_counter_cycle = 0;
            self.frame_counter_step = 0;
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.pulses[0].volume_envelope.step();
        self.pulses[1].volume_envelope.step();
        self.noise.volume_envelope.step();
        self.triangle.step_linear_counter();
    }

    fn clock_half_frame(&mut self) {
        self.pulses[0].step_sweep();
        self.pulses[1].step_sweep();
        self.pulses[0].step_length();
        self.pulses[1].step_length();
        self.triangle.step_length();
        self.noise.step_length();
    }

    fn mixed_sample(&self) -> f32 {
        let pulse_1 = if self.channel_mask.pulse1 {
            self.pulses[0].sample()
        } else {
            0
        };
        let pulse_2 = if self.channel_mask.pulse2 {
            self.pulses[1].sample()
        } else {
            0
        };
        let triangle = if self.channel_mask.triangle {
            self.triangle.sample()
        } else {
            0
        };
        let noise = if self.channel_mask.noise {
            self.noise.sample()
        } else {
            0
        };

        let pulse_sample = PULSE_TABLE[(pulse_1 + pulse_2) as usize];
        let tnd_sample = TND_TABLE[(3 * triangle + 2 * noise + self.dmc.output_level) as usize];
        pulse_sample + tnd_sample
    }

    fn sample(&mut self) -> f32 {
        let mut sampled = self.mixed_sample();

        // TODO: add low- and high-pass filters
        for filter in self.first_order_filters.iter_mut() {
            sampled = filter.filter(sampled);
        }

        sampled
    }

    // Only supports 0x4015
    pub(crate) fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                let status = (self.pulses[0].length_counter > 0) as u8
                    | (((self.pulses[1].length_counter > 0) as u8) << 1)
                    | (((self.triangle.length_counter > 0) as u8) << 2)
                    | (((self.noise.length_counter > 0) as u8) << 3)
                    | ((self.dmc.bytes_remaining > 0) as u8) << 4;
                let frame_irq = self.pending_irq.replace(false) as u8;
                status | (frame_irq << 6) | ((self.dmc.irq_pending as u8) << 7)
            }
            _ => 0,
        }
    }

    pub(crate) fn write_register(&mut self, addr: u16, data: u8) {
        let pulse_reg = (addr as usize & 0x4) >> 2;

        match addr {
            0x4000 | 0x4004 => {
                self.pulses[pulse_reg]
                    .volume_envelope
                    .period_or_constant_volume = data & 0xf;
                self.pulses[pulse_reg].volume_envelope.use_constant_volume = (data >> 4) & 0x1 != 0;
                self.pulses[pulse_reg].volume_envelope.loop_or_disabled = (data >> 5) & 0x1 != 0;
                self.pulses[pulse_reg].duty_type = (data >> 6) & 0x3;
                self.pulses[pulse_reg].volume_envelope.start = true;
            }
            0x4001 | 0x4005 => {
                self.pulses[pulse_reg].sweep.shift_count = data & 0x7;
                self.pulses[pulse_reg].sweep.negate = (data >> 3) & 1 != 0;
                self.pulses[pulse_reg].sweep.period = ((data >> 4) & 7) + 1;
                self.pulses[pulse_reg].sweep.enabled = (data >> 7) & 1 != 0;
                self.pulses[pulse_reg].sweep.reload = true;
            }
            0x4002 | 0x4006 => {
                self.pulses[pulse_reg].timer_period &= 0xff00;
                self.pulses[pulse_reg].timer_period |= data as u16;
            }
            0x4003 | 0x4007 => {
                self.pulses[pulse_reg].timer_period &= 0x00ff;
                self.pulses[pulse_reg].timer_period |= (data as u16 & 0x7) << 8;
                if self.pulses[pulse_reg].enabled {
                    self.pulses[pulse_reg].length_counter =
                        LENGTH_COUNTER_TABLE[(data >> 3) as usize] as u16;
                }
                self.pulses[pulse_reg].volume_envelope.start = true;
                self.pulses[pulse_reg].duty_offset = 0;
            }
            0x4008 => {
                self.triangle.length_enabled = (data >> 7) & 1 != 0;
                self.triangle.linear_counter_period = (data as u16) & 0x7f;
            }
            0x4009 => {}
            0x400a => {
                self.triangle.timer_period &= 0xff00;
                self.triangle.timer_period |= data as u16;
            }
            0x400b => {
                self.triangle.timer_period &= 0x00ff;
                self.triangle.timer_period |= (data as u16 & 0x7) << 8;
                if self.triangle.enabled {
                    self.triangle.length_counter =
                        LENGTH_COUNTER_TABLE[(data >> 3) as usize] as u16;
                }
                self.triangle.reload = true;
                self.triangle.phase = 0;
            }
            0x400c => {
                self.noise.volume_envelope.period_or_constant_volume = data & 0xf;
                self.noise.volume_envelope.use_constant_volume = (data >> 4) & 0x1 != 0;
                self.noise.volume_envelope.loop_or_disabled = (data >> 5) & 0x1 != 0;
                self.noise.volume_envelope.start = true;
            }
            0x400d => {}
            0x400e => {
                self.noise.mode = data & 0x80 != 0;
                self.noise.period = NOISE_PERIOD_TABLE[(data & 0xf) as usize] as u16;
            }
            0x400f => {
                if self.noise.enabled {
                    self.noise.length_counter = LENGTH_COUNTER_TABLE[(data >> 3) as usize] as u16;
                }
                self.noise.volume_envelope.start = true;
            }
            0x4010 => self.dmc.write_control(data),
            0x4011 => self.dmc.write_direct_load(data),
            0x4012 => self.dmc.write_sample_address(data),
            0x4013 => self.dmc.write_sample_length(data),
            0x4015 => {
                self.pulses[0].enabled = data & 0x1 != 0;
                self.pulses[1].enabled = (data >> 1) & 0x1 != 0;
                self.triangle.enabled = (data >> 2) & 0x1 != 0;
                self.noise.enabled = (data >> 3) & 0x1 != 0;
                self.dmc.set_enabled(data & 0x10 != 0);
                self.dmc.irq_pending = false;

                if !self.pulses[0].enabled {
                    self.pulses[0].length_counter = 0;
                }

                if !self.pulses[1].enabled {
                    self.pulses[1].length_counter = 0;
                }

                if !self.triangle.enabled {
                    self.triangle.length_counter = 0;
                }

                if !self.noise.enabled {
                    self.noise.length_counter = 0;
                }
            }
            0x4017 => {
                self.pending_five_step = (data >> 7) & 1 != 0;
                self.enable_irq = (data >> 6) & 1 == 0;
                if !self.enable_irq {
                    self.pending_irq.set(false);
                }

                // The restart takes effect on the next eligible odd APU
                // phase: two or three CPU cycles after the write depending on
                // whether the write arrived between APU phases or during one.
                let (write_phase, instruction_cycles) = self
                    .register_write_timing
                    .take()
                    .unwrap_or((self.on_apu_cycle, 0));
                let phase_delay = if write_phase { 3 } else { 2 };
                self.frame_counter_reset_delay = instruction_cycles + phase_delay;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_sweep_uses_channel_specific_negate_math() {
        let mut pulse_1 = Pulse::default();
        pulse_1.timer_period = 100;
        pulse_1.sweep = SweepUnit {
            enabled: true,
            negate: true,
            period: 1,
            shift_count: 2,
            delay: 0,
            carry: true,
            reload: false,
        };
        pulse_1.step_sweep();
        assert_eq!(pulse_1.timer_period, 74); // 100 - 25 - 1

        let mut pulse_2 = Pulse::default();
        pulse_2.timer_period = 100;
        pulse_2.sweep = SweepUnit {
            enabled: true,
            negate: true,
            period: 1,
            shift_count: 2,
            delay: 0,
            carry: false,
            reload: false,
        };
        pulse_2.step_sweep();
        assert_eq!(pulse_2.timer_period, 75); // 100 - 25

        let mut overflow = Pulse::default();
        overflow.timer_period = 0x400;
        overflow.enabled = true;
        overflow.length_counter = 1;
        overflow.duty_offset = 1;
        overflow.sweep.shift_count = 0;
        assert_eq!(overflow.sweep_target_period(), 0x800);
        assert_eq!(overflow.sample(), 0); // target overflow mutes even if sweep is disabled
    }

    #[test]
    fn noise_lfsr_is_seeded_and_clocks_feedback_into_bit_14() {
        let mut noise = Noise::default();
        assert_eq!(noise.shift_register, 1);

        noise.step_timer();
        assert_eq!(noise.shift_register, 0x4000);
        noise.step_timer();
        assert_eq!(noise.shift_register, 0x2000);
    }

    #[test]
    fn tnd_mixer_uses_triangle_and_noise_contributions() {
        let mut apu = APU::default();
        apu.channel_mask = ChannelMask {
            pulse1: false,
            pulse2: false,
            triangle: true,
            noise: true,
        };
        apu.triangle.enabled = true;
        apu.triangle.timer_period = 1;
        apu.triangle.length_counter = 1;
        apu.triangle.linear_counter_offset = 1;
        apu.triangle.phase = 0;
        apu.noise.enabled = true;
        apu.noise.length_counter = 1;
        apu.noise.shift_register = 0;
        apu.noise.volume_envelope.use_constant_volume = true;
        apu.noise.volume_envelope.period_or_constant_volume = 15;

        assert_eq!(apu.mixed_sample(), TND_TABLE[3 * 15 + 2 * 15]);
    }

    #[test]
    fn status_reports_active_length_counters_as_bits() {
        let mut apu = APU::default();
        apu.write_register(0x4003, 0); // disabled channels cannot load length
        assert_eq!(apu.read_register(0x4015), 0);

        apu.write_register(0x4015, 0x0f);
        apu.write_register(0x4003, 0);
        apu.write_register(0x4007, 0);
        apu.write_register(0x400b, 0);
        apu.write_register(0x400f, 0);
        assert_eq!(apu.read_register(0x4015), 0x0f);

        apu.write_register(0x4015, 0);
        assert_eq!(apu.read_register(0x4015), 0);
    }

    #[test]
    fn reading_status_reports_and_clears_frame_irq() {
        let mut apu = APU::default();
        apu.pulses[0].length_counter = 1;
        apu.noise.length_counter = 1;
        apu.pending_irq.set(true);

        assert_eq!(apu.read_register(0x4015), 0x49);
        assert!(!apu.pending_irq.get());
        assert!(!apu.irq_line());

        apu.pending_irq.set(true);
        assert!(apu.irq_line());
        assert!(apu.irq_line());
        assert_eq!(apu.read_register(0x4015), 0x49);
        assert!(!apu.irq_line());
    }

    #[test]
    fn dmc_registers_decode_control_and_sample_parameters() {
        let mut apu = APU::default();

        apu.write_register(0x4010, 0xcf);
        apu.write_register(0x4011, 0xff);
        apu.write_register(0x4012, 0x12);
        apu.write_register(0x4013, 0x02);

        assert!(apu.dmc.irq_enabled);
        assert!(apu.dmc.loop_flag);
        assert_eq!(apu.dmc.rate_index, 0x0f);
        assert_eq!(apu.dmc.timer_period, 27);
        assert_eq!(apu.dmc.output_level, 127);
        assert_eq!(apu.dmc.sample_address, 0xc480);
        assert_eq!(apu.dmc.sample_length, 33);
    }

    #[test]
    fn dmc_output_updates_on_timer_and_clamps_to_seven_bits() {
        let mut dmc = Dmc::default();
        dmc.timer_period = 2;
        dmc.output_level = 126;
        dmc.sample_buffer = Some(0xff);

        dmc.step_timer(true);
        assert_eq!(dmc.output_level, 127);
        dmc.step_timer(true);
        assert_eq!(dmc.output_level, 127);
        dmc.step_timer(true);
        assert_eq!(dmc.output_level, 127);

        dmc.output_level = 1;
        dmc.sample_buffer = Some(0x00);
        dmc.bits_remaining = 0;
        dmc.silence = true;
        dmc.timer = 0;
        dmc.step_timer(true);
        assert_eq!(dmc.output_level, 0);
    }

    #[test]
    fn dmc_rate_period_is_measured_in_apu_cycles() {
        let mut dmc = Dmc::default();
        dmc.write_control(0x00);
        dmc.sample_buffer = Some(0xff);

        dmc.step_timer(true);
        assert_eq!(dmc.output_level, 2);

        for _ in 0..(dmc.timer_period - 1) {
            dmc.step_timer(true);
        }
        assert_eq!(dmc.output_level, 2);

        dmc.step_timer(true);
        assert_eq!(dmc.output_level, 4);
    }

    #[test]
    fn dmc_reader_advances_and_wraps_memory_address() {
        let mut dmc = Dmc::default();
        dmc.current_address = 0xffff;
        dmc.bytes_remaining = 2;
        let mut addresses = Vec::new();

        dmc.read_sample_byte(&mut |address| {
            addresses.push(address);
            address as u8
        });
        dmc.sample_buffer = None;
        dmc.read_sample_byte(&mut |address| {
            addresses.push(address);
            address as u8
        });

        assert_eq!(addresses, [0xffff, 0x8000]);
        assert_eq!(dmc.current_address, 0x8001);
        assert_eq!(dmc.bytes_remaining, 0);
    }

    #[test]
    fn dmc_enable_restart_disable_loop_and_irq_follow_status_rules() {
        let mut apu = APU::default();
        apu.write_register(0x4010, 0x80);
        apu.write_register(0x4012, 0x20);
        apu.write_register(0x4013, 0x00);
        apu.write_register(0x4015, 0x10);
        assert_eq!(apu.dmc.bytes_remaining, 1);
        assert_eq!(apu.read_register(0x4015) & 0x10, 0x10);

        apu.dmc.read_sample_byte(&mut |_| 0x80);
        assert_eq!(apu.dmc.bytes_remaining, 0);
        assert_eq!(apu.read_register(0x4015) & 0x80, 0x80);
        assert!(apu.irq_line());
        assert_eq!(apu.read_register(0x4015) & 0x80, 0x80);

        apu.write_register(0x4010, 0);
        assert!(!apu.dmc.irq_pending);
        assert!(!apu.irq_line());

        apu.write_register(0x4015, 0);
        assert_eq!(apu.dmc.bytes_remaining, 0);
        assert!(!apu.dmc.irq_pending);
        assert!(!apu.irq_line());

        apu.write_register(0x4010, 0xc0);
        apu.write_register(0x4015, 0x10);
        apu.dmc.read_sample_byte(&mut |_| 0x80);
        assert_eq!(apu.dmc.bytes_remaining, 1);
        assert!(!apu.dmc.irq_pending);
        assert_eq!(apu.dmc.current_address, apu.dmc.sample_address);

        // An active sample is not restarted by a second enable write.
        apu.dmc.current_address = 0xc123;
        apu.write_register(0x4015, 0x10);
        assert_eq!(apu.dmc.current_address, 0xc123);
    }

    #[test]
    fn dmc_load_dma_is_scheduled_after_enable_not_fetched_immediately() {
        let mut apu = APU::default();
        apu.write_register(0x4012, 0x20);
        apu.write_register(0x4013, 0x00);
        apu.write_register(0x4015, 0x10);

        assert_eq!(apu.dmc.load_dma_delay, 2);
        assert!(!apu.dma_active());

        let mut reads = 0;
        apu.step(|_| {
            reads += 1;
            0xa5
        });
        apu.step(|_| {
            reads += 1;
            0xa5
        });
        assert_eq!(reads, 0);
        assert!(!apu.dma_active());

        apu.step(|_| {
            reads += 1;
            0xa5
        });
        assert_eq!(reads, 0);
        assert_eq!(apu.dmc.dma_kind, Some(DmcDmaKind::Load));
        assert_eq!(apu.dmc.dma_cycles_remaining, 3);

        for _ in 0..2 {
            apu.step(|_| {
                reads += 1;
                0xa5
            });
        }
        assert_eq!(reads, 0);
        apu.step(|_| {
            reads += 1;
            0xa5
        });
        assert_eq!(reads, 1);
        assert_eq!(apu.dmc.sample_buffer, Some(0xa5));
    }

    #[test]
    fn dmc_reload_dma_is_scheduled_when_sample_buffer_is_exhausted() {
        let mut apu = APU::default();
        apu.dmc.sample_buffer = Some(0x5a);
        apu.dmc.bytes_remaining = 1;
        apu.dmc.timer = 0;

        apu.step(|_| 0xa5);

        assert!(apu.dmc.sample_buffer.is_none());
        assert_eq!(apu.dmc.dma_kind, Some(DmcDmaKind::Reload));
        assert_eq!(apu.dmc.dma_cycles_remaining, 4);
    }

    #[test]
    fn dmc_dma_cost_changes_with_alignment_phase() {
        assert_eq!(Dmc::dma_cycles(DmcDmaKind::Load, true), 3);
        assert_eq!(Dmc::dma_cycles(DmcDmaKind::Load, false), 4);
        assert_eq!(Dmc::dma_cycles(DmcDmaKind::Reload, false), 4);
        assert_eq!(Dmc::dma_cycles(DmcDmaKind::Reload, true), 3);
    }

    #[test]
    fn dmc_dma_does_not_start_without_data_or_when_disabled() {
        let mut apu = APU::default();
        apu.write_register(0x4015, 0x10);
        apu.write_register(0x4015, 0x00);
        for _ in 0..8 {
            apu.step(|_| panic!("disabled DMC must not read memory"));
        }
        assert!(!apu.dma_active());

        apu.dmc.load_dma_delay = 0;
        apu.dmc.bytes_remaining = 0;
        apu.dmc.sample_buffer = None;
        for _ in 0..8 {
            apu.step(|_| panic!("empty DMC must not read memory"));
        }
        assert!(!apu.dma_active());
    }

    #[test]
    fn frame_counter_follows_four_and_five_step_schedules() {
        let mut apu = APU::default();
        apu.enable_irq = true;
        apu.pulses[0].volume_envelope.decay_level_counter = 15;
        apu.pulses[0].volume_envelope.period_or_constant_volume = 0;
        apu.pulses[0].length_counter = 3;

        for _ in 0..7456 {
            apu.step_frame_counter();
        }
        assert_eq!(apu.pulses[0].volume_envelope.decay_level_counter, 15);
        assert_eq!(apu.pulses[0].length_counter, 3);
        apu.step_frame_counter(); // 7457: quarter frame
        assert_eq!(apu.pulses[0].volume_envelope.decay_level_counter, 14);
        assert_eq!(apu.pulses[0].length_counter, 3);

        for _ in 0..7456 {
            apu.step_frame_counter();
        }
        apu.step_frame_counter(); // 14913: quarter + half frame
        assert_eq!(apu.pulses[0].volume_envelope.decay_level_counter, 13);
        assert_eq!(apu.pulses[0].length_counter, 2);

        for _ in 0..14916 {
            apu.step_frame_counter();
        }
        assert!(apu.pending_irq.get()); // 29829: final 4-step clock

        let mut five_step = APU::default();
        five_step.pulses[0].volume_envelope.decay_level_counter = 1;
        five_step.pulses[0].length_counter = 2;
        five_step.write_register(0x4017, 0x80);
        assert_eq!(five_step.frame_counter_cycle, 0);
        assert_eq!(five_step.frame_counter_step, 0);
        assert_eq!(five_step.pulses[0].volume_envelope.decay_level_counter, 1);
        assert_eq!(five_step.pulses[0].length_counter, 2);
        for _ in 0..(five_step.frame_counter_reset_delay - 1) {
            five_step.step_frame_counter();
        }
        assert_eq!(five_step.pulses[0].volume_envelope.decay_level_counter, 1);
        assert_eq!(five_step.pulses[0].length_counter, 2);
        five_step.step_frame_counter();
        assert_eq!(five_step.pulses[0].volume_envelope.decay_level_counter, 0);
        assert_eq!(five_step.pulses[0].length_counter, 1);
        for _ in 0..37280 {
            five_step.step_frame_counter();
        }
        assert_eq!(five_step.frame_counter_step, 4);
        five_step.step_frame_counter(); // 37281: final 5-step clock
        assert_eq!(five_step.frame_counter_cycle, 0);
        assert_eq!(five_step.frame_counter_step, 0);
        assert!(!five_step.pending_irq.get());
    }

    #[test]
    fn frame_counter_write_delay_depends_on_apu_phase() {
        let mut during_apu_phase = APU::default();
        during_apu_phase.on_apu_cycle = true;
        during_apu_phase.write_register(0x4017, 0x80);
        assert_eq!(during_apu_phase.frame_counter_reset_delay, 3);

        let mut between_apu_phases = APU::default();
        between_apu_phases.on_apu_cycle = false;
        between_apu_phases.write_register(0x4017, 0x80);
        assert_eq!(between_apu_phases.frame_counter_reset_delay, 2);

        for _ in 0..2 {
            during_apu_phase.step_frame_counter();
        }
        assert_eq!(during_apu_phase.frame_counter_reset_delay, 1);
        assert!(!during_apu_phase.use_five_step);

        during_apu_phase.step_frame_counter();
        assert_eq!(during_apu_phase.frame_counter_reset_delay, 0);
        assert!(during_apu_phase.use_five_step);

        for _ in 0..2 {
            between_apu_phases.step_frame_counter();
        }
        assert_eq!(between_apu_phases.frame_counter_reset_delay, 0);
        assert!(between_apu_phases.use_five_step);
    }

    #[test]
    fn frame_irq_latches_until_status_read_or_inhibit() {
        let mut apu = APU::default();
        apu.enable_irq = true;

        for _ in 0..29829 {
            apu.step_frame_counter();
        }
        assert!(apu.pending_irq.get());
        assert!(apu.irq_line());
        assert!(apu.irq_line());

        assert_eq!(apu.read_register(0x4015) & 0x40, 0x40);
        assert!(!apu.irq_line());

        apu.pending_irq.set(true);
        apu.write_register(0x4017, 0x00);
        assert!(apu.irq_line());

        apu.write_register(0x4017, 0x40);
        assert!(!apu.irq_line());
    }
}
