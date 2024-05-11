use crate::dsp::FirstOrderFilter;

const LENGTH_COUNTER_TABLE: [u8; 32] = [
    10, 254, 20, 2,  40, 4,  80, 6,  160, 8,  60, 10, 14, 12, 26, 14,
    12, 16,  24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254,380, 508, 762, 1016, 2034, 4068,
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


impl SweepUnit {
    fn step(&mut self) -> bool {
        if self.reload || self.delay == 0 {
            let should_sweep = self.enabled && self.delay == 0;
            self.delay = self.period;
            self.reload = false;
            should_sweep
        } else {
            self.delay -= 1;
            false
        }
    }
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

    fn step_sweep(&mut self) {
        if self.sweep.step() {
            let delta = self.timer_period >> self.sweep.shift_count;
            if self.sweep.negate {
                self.timer_period = !self.timer_period;
            }
            self.timer_period = self.timer_period.wrapping_add(delta).wrapping_add(self.sweep.carry as u16);
        }
    }

    fn sample(&self) -> u8 {
        let high = ((DUTY_TABLE[self.duty_type as usize] >> (7 - self.duty_offset)) & 1) != 0;
        let silenced = self.timer_period < 8 || (self.timer_period > 0x7ff);

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
        if self.enabled && self.timer_period > 0 && self.length_counter > 0 && self.linear_counter_offset > 0 {
            15u8.wrapping_sub(self.phase as u8) ^ 0u8.wrapping_sub((self.phase >= 16) as u8)
        } else {
            0
        }
    }
}

#[derive(Clone, Default)]
struct Noise {
    volume_envelope: VolumeEnvelope,
    length_counter: u16,
    shift_register: u16, // 15 bits
    enabled: bool,
    mode: bool,
    period: u16, // should this be u8?
    timer: u16, // should this be u8?
    feedback_bit: u8,
}

impl Noise {
    fn step_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.period;
            self.shift_register |= ((self.shift_register >> self.feedback_bit) ^ self.shift_register) & 1 << 15;
            self.shift_register >>= 1;
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

#[derive(Clone)]
pub(crate) struct APU {
    pulses: [Pulse; 2],
    triangle: Triangle,
    noise: Noise,

    cycles_x_frame_counter_freq: u32,
    cycles_x_sample_freq: u32,
    sample_freq: u32,
    frame_period: u8,

    on_frame_counter_edge: bool,
    on_sample_edge: bool,
    use_five_step: bool,
    enable_irq: bool,
    pending_irq: bool,
    on_apu_cycle: bool,

    first_order_filters: [FirstOrderFilter; 3],
}

impl Default for APU {
    fn default() -> Self {
        APU {
            pulses: [Pulse::default(), Pulse::default()],
            triangle: Triangle::default(),
            noise: Noise::default(),

            cycles_x_frame_counter_freq: 0,
            cycles_x_sample_freq: 0,
            sample_freq: 48_000,
            frame_period: 1,

            on_frame_counter_edge: false,
            on_sample_edge: false,
            use_five_step: false,
            enable_irq: false,
            pending_irq: false,
            on_apu_cycle: true,
            first_order_filters: [
                FirstOrderFilter::high_pass(48_000.0, 90.0),
                FirstOrderFilter::high_pass(48_000.0, 440.0),
                FirstOrderFilter::low_pass(48_000.0, 14_000.0),
            ]
        }
    }
}

impl APU {
    pub(crate) fn set_sample_freq(&mut self, sample_freq: u32) {
        self.sample_freq = sample_freq;
    }

    pub(crate) fn read_irq_line(&mut self) -> bool {
        let irq = self.pending_irq;
        self.pending_irq = false;
        irq
    }

    fn update_ticks(&mut self)  {
        // need to divide the CPU frequency into a non-integer amount.
        // this means that the positive edges won't consistently line up,
        // so need to detect positive edges that occur between CPU clock ticks
        const FRAME_COUNTER_FREQ: u32 = 240;
        const CPU_FREQ: u32 = 1_789_773;

        // calculate 240Hz ticks
        self.cycles_x_frame_counter_freq = self.cycles_x_frame_counter_freq.wrapping_add(FRAME_COUNTER_FREQ);
        if self.cycles_x_frame_counter_freq >= CPU_FREQ {
            self.cycles_x_frame_counter_freq -= CPU_FREQ;
            self.on_frame_counter_edge = true;
        } else {
            self.on_frame_counter_edge = false;
        }

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

    pub(crate) fn step(&mut self) -> Option<f32>{
        if self.on_apu_cycle {
            // pulse + noise + DMC
            self.pulses[0].step_timer();
            self.pulses[1].step_timer();
            self.noise.step_timer()

            // TODO: DMC
        }

        self.triangle.step_timer();

        if self.on_frame_counter_edge {
            self.step_frame_counter();
        }

        let sample = if self.on_sample_edge {
            Some(self.sample())
        } else {
            None
        };

        self.update_ticks();

        sample
    }

    fn step_frame_counter(&mut self) {
        // https://www.nesdev.org/wiki/APU_Frame_Counter
        let step_with_period: u8 = 0x40 | ((self.use_five_step as u8) << 4) | (self.frame_period & 0xf);
        match step_with_period {
            0x42 | 0x44 | 0x52 | 0x54 => {
                self.pulses[0].step_sweep();
                self.pulses[1].step_sweep();
                self.pulses[0].step_length();
                self.pulses[1].step_length();
                self.triangle.step_length();
                self.noise.step_length();

                // "fallthrough"
                self.pulses[0].volume_envelope.step();
                self.pulses[1].volume_envelope.step();
                self.noise.volume_envelope.step();
                self.triangle.step_linear_counter();
            },
            0x41 | 0x43 | 0x51 | 0x53 => {
                self.pulses[0].volume_envelope.step();
                self.pulses[1].volume_envelope.step();
                self.noise.volume_envelope.step();
                self.triangle.step_linear_counter();
            },
            _ => {},
        }

        // TODO: check interrupts
        if step_with_period == 0x44 && self.enable_irq {
            self.pending_irq = true;
        }

        // increment the frame timerPeriod and wrap around
        self.frame_period = match step_with_period {
            0x55 | 0x44 => 1,
            _ => self.frame_period.wrapping_add(1)
        };
    }


    fn sample(&mut self) -> f32 {
        let pulse_sample = PULSE_TABLE[self.pulses[0].sample() as usize + self.pulses[1].sample() as usize];
        let tnd_sample = TND_TABLE[3 * (self.triangle.sample() as usize) + 2 /* * (self.noise.sample() as usize) + 0 */];
        // let pulse_sample = 0.0;
        let tnd_sample = 0.0;
        let mut sampled = pulse_sample + tnd_sample;

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
                // TODO: Add DMC + noise
                (self.pulses[0].length_counter as u8) |
                    ((self.pulses[1].length_counter as u8) << 1) |
                    ((self.triangle.length_counter as u8) << 2)
            },
            _ => 0,
        }
    }

    pub(crate) fn write_register(&mut self, addr: u16, data: u8) {
        let pulse_reg = (addr as usize & 0x4) >> 2;

        match addr {
            0x4000 | 0x4004 => {
                self.pulses[pulse_reg].volume_envelope.period_or_constant_volume = data & 0xf;
                self.pulses[pulse_reg].volume_envelope.use_constant_volume = (data >> 4) & 0x1 != 0;
                self.pulses[pulse_reg].volume_envelope.loop_or_disabled = (data >> 5) & 0x1 != 0;
                self.pulses[pulse_reg].duty_type = (data >> 6) & 0x3;
                self.pulses[pulse_reg].volume_envelope.start = true;
            },
            0x4001 | 0x4005 => {
                self.pulses[pulse_reg].sweep.shift_count = data & 0x7;
                self.pulses[pulse_reg].sweep.negate = (data >> 3) & 1 != 0;
                self.pulses[pulse_reg].sweep.period = ((data >> 4) & 7) + 1;
                self.pulses[pulse_reg].sweep.enabled = (data >> 7) & 1 != 0;
                self.pulses[pulse_reg].sweep.reload = true;
            },
            0x4002 | 0x4006 => {
                self.pulses[pulse_reg].timer_period &= 0xff00;
                self.pulses[pulse_reg].timer_period |= data as u16;
            },
            0x4003 | 0x4007 => {
                self.pulses[pulse_reg].timer_period &= 0x00ff;
                self.pulses[pulse_reg].timer_period |= (data as u16 & 0x7) << 8;
                self.pulses[pulse_reg].length_counter = LENGTH_COUNTER_TABLE[(data >> 3) as usize] as u16;
                self.pulses[pulse_reg].volume_envelope.start = true;
                self.pulses[pulse_reg].duty_offset = 0;
            },
            0x4008 => {
                self.triangle.length_enabled = (data >> 7) & 1 != 0;
                self.triangle.linear_counter_period = (data as u16) & 0x7f;
            },
            0x4009 => {},
            0x400a => {
                self.triangle.timer_period &= 0xff00;
                self.triangle.timer_period |= data as u16;
            },
            0x400b => {
                self.triangle.timer_period &= 0x00ff;
                self.triangle.timer_period |= (data as u16 & 0x7) << 8;
                self.triangle.length_counter = LENGTH_COUNTER_TABLE[(data >> 3) as usize] as u16;
                self.triangle.reload = true;
                self.triangle.phase = 0;
            },
            0x400c => {
                self.noise.volume_envelope.period_or_constant_volume = data & 0xf;
                self.noise.volume_envelope.use_constant_volume = (data >> 4) & 0x1 != 0;
                self.noise.volume_envelope.loop_or_disabled = (data >> 5) & 0x1 != 0;
                self.noise.volume_envelope.start = true;
            },
            0x400d => {},
            0x400e => {
                self.noise.feedback_bit = if (data >> 7) & 1 != 0 { 6 } else { 1 };
                self.noise.period = NOISE_PERIOD_TABLE[(data & 0xf) as usize] as u16;
            },
            0x400f => {
                self.noise.length_counter = LENGTH_COUNTER_TABLE[(data >> 3) as usize] as u16;
                self.noise.volume_envelope.start = true;
            },
            0x4010 | 0x4011 | 0x4012 | 0x4013 => {},
            0x4015 => {
                self.pulses[0].enabled = data & 0x1 != 0;
                self.pulses[1].enabled = (data >> 1) & 0x1 != 0;
                self.triangle.enabled = (data >> 2) & 0x1 != 0;
                self.noise.enabled = (data >> 3) & 0x1 != 0;

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
            },
            0x4017 => {
                self.use_five_step = (data >> 7) & 1 != 0;
                self.enable_irq = (data >> 6) & 1 == 0;
                self.pending_irq = self.pending_irq && self.enable_irq;

                if self.use_five_step {
                    for i in 0..2 {
                        self.pulses[i].volume_envelope.step();
                        self.pulses[i].step_sweep();
                        self.pulses[i].step_length();
                    }

                    self.triangle.step_length();
                    self.noise.step_length();
                    self.noise.volume_envelope.step();
                }
            },
            _ => {}
        }
    }
}