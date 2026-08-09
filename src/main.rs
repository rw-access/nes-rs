#[cfg(feature = "sdl2-frontend")]
mod sdl2_frontend {
    use clap::Parser;
    use image::{write_buffer_with_format, GrayImage, ImageBuffer, Luma};
    use nes::controller::ButtonState;
    use nes::{
        apu::ChannelMask, cartridge, console::Console, controller::Button, video::NES_PALETTE_RGB,
    };
    use sdl2::event::Event;
    use sdl2::keyboard::Keycode;
    use sdl2::pixels::{Color, PixelFormatEnum};
    // Construct a new RGB ImageBuffer with the specified width and height.

    use sdl2::audio::AudioSpecDesired;
    use std::time::Duration;

    const AUDIO_SAMPLE_RATE: usize = 48_000;
    const AUDIO_FRAME_SAMPLES: usize = AUDIO_SAMPLE_RATE / 60;

    /// Samples produced while emulating one video frame for the SDL queue.
    struct AudioBlock {
        samples: Vec<f32>,
    }

    impl AudioBlock {
        fn with_capacity(capacity: usize) -> Self {
            Self {
                samples: Vec::with_capacity(capacity),
            }
        }

        fn push(&mut self, sample: f32) {
            self.samples.push(sample);
        }

        fn clear(&mut self) {
            self.samples.clear();
        }

        fn is_empty(&self) -> bool {
            self.samples.is_empty()
        }

        fn as_slice(&self) -> &[f32] {
            &self.samples
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum AudioQueueAction {
        WaitForPrefill,
        Resume,
        Continue,
        Pace,
    }

    /// Keeps the SDL queue within a small latency window without tying emulation to wall-clock time.
    struct AudioQueuePacer {
        prefill_samples: usize,
        target_samples: usize,
        max_samples: usize,
        started: bool,
    }

    impl AudioQueuePacer {
        fn new(block_samples: usize) -> Self {
            assert!(block_samples > 0);
            Self {
                prefill_samples: block_samples * 3,
                target_samples: block_samples * 3,
                max_samples: block_samples * 5,
                started: false,
            }
        }

        fn observe(&mut self, queued_samples: usize) -> AudioQueueAction {
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

        fn target_samples(&self) -> usize {
            self.target_samples
        }

        fn reset(&mut self) {
            self.started = false;
        }
    }

    fn get_button(keycode: Keycode) -> Option<Button> {
        match keycode {
            Keycode::W => Some(Button::Up),
            Keycode::A => Some(Button::Left),
            Keycode::S => Some(Button::Down),
            Keycode::D => Some(Button::Right),
            Keycode::J => Some(Button::B),
            Keycode::K => Some(Button::A),
            Keycode::Period => Some(Button::Start),
            Keycode::Comma => Some(Button::Select),
            _ => None,
        }
    }

    fn save_png(rom_path: &str, bmp_path: &str) {
        const TILES_PER_BANK: usize = 0x2000 / 16;

        let mut rom_file = std::fs::File::open(rom_path).unwrap();
        let mut bmp_file = std::fs::File::create(bmp_path).unwrap();

        let (c, _) = nes::ines::load(&mut rom_file).expect("failed to load cartridge");

        let num_tiles = c.chr.get_banks().len() * TILES_PER_BANK;
        let tiles_x = 32 as usize;
        let tiles_y = num_tiles / tiles_x;

        let mut img: GrayImage =
            ImageBuffer::new((1 + tiles_x * 9) as u32, (1 + tiles_y * 9) as u32);

        for (bank_no, bank) in c.chr.get_banks().iter().enumerate() {
            for (tile_no, tile) in bank.chunks_exact(16).enumerate() {
                // 16 bytes per tile
                // planeOne = offset + [0 ... 7]
                // planeTwo = planeOne + 8
                let tile_no = bank_no * TILES_PER_BANK + tile_no;
                let left_x = 1 + 9 * (tile_no % tiles_x);
                let top_y = 1 + 9 * (tile_no / tiles_x);

                for tile_y in 0..8usize {
                    let lo_y = tile[tile_y];
                    let hi_y = tile[tile_y + 8];

                    for tile_x in 0..8usize {
                        let lo_px = (lo_y >> (7 - tile_x)) & 0b1;
                        let hi_px = (hi_y >> (7 - tile_x)) & 0b1;
                        let px = hi_px << 1 | lo_px;

                        img.put_pixel(
                            (left_x + tile_x) as u32,
                            (top_y + tile_y) as u32,
                            Luma([px << 6]),
                        );
                    }
                }
            }
        }

        write_buffer_with_format(
            &mut bmp_file,
            &img,
            img.width(),
            img.height(),
            image::ColorType::L8,
            image::ImageOutputFormat::Png,
        )
        .expect("failed to save image")
    }

    fn play_rom(rom_path: &str, cpu_ignore_rewind: Vec<u16>, ppu_ignore_rewind: Vec<u16>) {
        const SCALING: u32 = 2;
        const WIDTH: u32 = 256;
        const HEIGHT: u32 = 240;
        let mut rom_file = std::fs::File::open(rom_path).unwrap();

        let (c, m) = nes::ines::load(&mut rom_file).expect("failed to load cartridge");
        let mapper = cartridge::new(c, m).unwrap();
        let mut console = Console::new(mapper);

        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();
        let audio_subsystem = sdl_context.audio().unwrap();

        // draw the screen, for now make a function
        let window = video_subsystem
            .window("nes-rs", WIDTH * SCALING, HEIGHT * SCALING)
            .position_centered()
            .build()
            .expect("could not initialize video subsystem");

        let mut canvas = window
            .into_canvas()
            .build()
            .expect("could not make a canvas");

        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        canvas.present();

        // create audio device, start it, and open buffer

        let audio_device = audio_subsystem
            .open_queue::<f32, _>(
                None,
                &AudioSpecDesired {
                    freq: Some(48_000),
                    channels: Some(1),
                    samples: Some(AUDIO_FRAME_SAMPLES as u16),
                },
            )
            .unwrap();

        if audio_device.spec().freq != AUDIO_SAMPLE_RATE as i32 {
            panic!("expected 48 KHz sample rate")
        }

        // Keep the device paused until several complete frame blocks are queued.
        audio_device.pause();
        let mut audio_pacer = AudioQueuePacer::new(AUDIO_FRAME_SAMPLES);
        let mut audio_block = AudioBlock::with_capacity(AUDIO_FRAME_SAMPLES);

        let mut event_pump = sdl_context.event_pump().unwrap();

        let creator = canvas.texture_creator();
        let mut texture = creator
            .create_texture_target(PixelFormatEnum::RGB24, WIDTH * SCALING, HEIGHT * SCALING)
            .unwrap();

        let mut raw_texture = [0u8; (WIDTH * HEIGHT * SCALING * SCALING * 3) as usize];

        let mut rewind = false;
        let mut button_state = ButtonState::default();
        'run_loop: loop {
            for event in event_pump.poll_iter() {
                match event {
                    Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => {
                        break 'run_loop;
                    }
                    Event::KeyDown {
                        keycode: Some(k), ..
                    } => {
                        if k == Keycode::I {
                            rewind = true;
                        }

                        if let Some(button) = get_button(k) {
                            button_state.set(button);
                            if !rewind {
                                console.update_buttons(button_state);
                            }
                        }
                    }
                    Event::KeyUp {
                        keycode: Some(k), ..
                    } => {
                        if k == Keycode::I {
                            rewind = false;
                            console.update_buttons(button_state);
                        }

                        if let Some(button) = get_button(k) {
                            button_state.unset(button);
                            if !rewind {
                                console.update_buttons(button_state);
                            }
                        }

                        console.update_channel_mask(ChannelMask {
                            pulse1: k == Keycode::Num1,
                            pulse2: k == Keycode::Num2,
                            triangle: k == Keycode::Num3,
                            noise: k == Keycode::Num4,
                        })
                    }
                    _ => {}
                }
            }

            if rewind {
                console.rewind();
            }

            audio_block.clear();
            let frame = console.next_frame();

            if frame.audio_discontinuity {
                audio_device.pause();
                audio_device.clear();
                audio_pacer.reset();
                audio_block.clear();
            }

            for &sample in frame.audio_samples {
                audio_block.push(sample);
            }

            if !audio_block.is_empty() && !audio_device.queue(audio_block.as_slice()) {
                panic!("failed to queue audio block");
            }

            let queued_samples = || audio_device.size() as usize / std::mem::size_of::<f32>();
            match audio_pacer.observe(queued_samples()) {
                AudioQueueAction::Resume => audio_device.resume(),
                AudioQueueAction::Pace => {
                    while queued_samples() > audio_pacer.target_samples() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                AudioQueueAction::WaitForPrefill | AudioQueueAction::Continue => {}
            }

            for (y, row) in frame.pixels.iter().enumerate() {
                for (x, palette_color) in row.iter().enumerate() {
                    // decode the palette
                    let [_, r, g, b] =
                        NES_PALETTE_RGB[*palette_color as usize & 0x3f].to_be_bytes();

                    for y_off in 0..SCALING {
                        let row_start =
                            (y * SCALING as usize + y_off as usize) * (WIDTH * SCALING) as usize;
                        for x_off in 0..SCALING {
                            let column_offset = x * SCALING as usize + x_off as usize;
                            let px_offset = (row_start + column_offset) * 3;

                            raw_texture[px_offset] = r;
                            raw_texture[px_offset + 1] = g;
                            raw_texture[px_offset + 2] = b;
                        }
                    }
                }
            }

            texture
                .update(None, &raw_texture, (SCALING * WIDTH * 3) as usize)
                .unwrap();
            canvas.copy(&texture, None, None).unwrap();
            canvas.present();
        }
    }

    #[derive(clap::Parser)]
    enum CLI {
        Play {
            #[arg(short, long)]
            rom: String,
            #[arg(short, long)]
            cpu_ignore_rewind: Vec<u16>,
            #[arg(short, long)]
            ppu_ignore_rewind: Vec<u16>,
        },
        CHRDump {
            #[arg(long)]
            rom: String,
            #[arg(long)]
            out: String,
        },
    }

    pub fn run() {
        let args = CLI::parse();

        match args {
            CLI::CHRDump { rom, out } => save_png(&rom, &out),
            CLI::Play {
                rom,
                cpu_ignore_rewind,
                ppu_ignore_rewind,
            } => play_rom(&rom, cpu_ignore_rewind, ppu_ignore_rewind),
        };
    }

    #[cfg(test)]
    mod tests {
        use super::{AudioBlock, AudioQueueAction, AudioQueuePacer};

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
    }
}

#[cfg(feature = "sdl2-frontend")]
fn main() {
    sdl2_frontend::run();
}

#[cfg(not(feature = "sdl2-frontend"))]
fn main() {
    eprintln!("the SDL2 frontend is disabled; rebuild with --features sdl2-frontend");
}
