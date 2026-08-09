use std::collections::VecDeque;
use std::env;
use std::error::Error;
use std::fs::File;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use nes_core::apu::ChannelMask;
use nes_core::cartridge;
use nes_core::console::Console;
use nes_core::controller::{Button, ButtonState};
use nes_core::ines;
use nes_core::video::{FRAME_HEIGHT, FRAME_WIDTH, NES_PALETTE_RGB};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const SCALE: usize = 2;
const WINDOW_WIDTH: u32 = (FRAME_WIDTH * SCALE) as u32;
const WINDOW_HEIGHT: u32 = (FRAME_HEIGHT * SCALE) as u32;
const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);
const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_QUEUE_CAPACITY: usize = AUDIO_SAMPLE_RATE as usize;

struct AudioQueue {
    samples: Mutex<VecDeque<f32>>,
    capacity: usize,
}

impl AudioQueue {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    fn enqueue(&self, samples: &[f32]) {
        let Ok(mut queue) = self.samples.lock() else {
            return;
        };

        let overflow = queue
            .len()
            .saturating_add(samples.len())
            .saturating_sub(self.capacity);
        for _ in 0..overflow {
            queue.pop_front();
        }
        let remaining = self.capacity - queue.len();
        queue.extend(samples.iter().copied().take(remaining));
    }

    fn flush(&self) {
        if let Ok(mut queue) = self.samples.lock() {
            queue.clear();
        }
    }

    fn fill(&self, output: &mut [f32], channels: usize) {
        if let Ok(mut queue) = self.samples.lock() {
            for frame in output.chunks_mut(channels) {
                let sample = queue.pop_front().unwrap_or(0.0);
                frame.fill(sample);
            }
        } else {
            output.fill(0.0);
        }
    }

    fn len(&self) -> usize {
        self.samples.lock().map(|queue| queue.len()).unwrap_or(0)
    }
}

struct AudioOutput {
    queue: Arc<AudioQueue>,
    _stream: cpal::Stream,
    sample_rate: u32,
}

impl AudioOutput {
    fn new() -> Result<Self, Box<dyn Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let requested_rate = SampleRate(AUDIO_SAMPLE_RATE);
        let supported_config = device
            .supported_output_configs()?
            .find(|config| {
                config.channels() >= 1
                    && config.sample_format() == SampleFormat::F32
                    && config.min_sample_rate() <= requested_rate
                    && requested_rate <= config.max_sample_rate()
            })
            .ok_or("no supported 48 kHz f32 output configuration")?
            .with_sample_rate(requested_rate);

        let config: StreamConfig = supported_config.config();
        let channels = config.channels as usize;
        let queue = Arc::new(AudioQueue::new(AUDIO_QUEUE_CAPACITY));
        let callback_queue = Arc::clone(&queue);
        let error_callback = |error| eprintln!("audio stream error: {error}");
        let stream = device.build_output_stream::<f32, _, _>(
            &config,
            move |output, _| callback_queue.fill(output, channels),
            error_callback,
            None,
        )?;
        Ok(Self {
            queue,
            _stream: stream,
            sample_rate: config.sample_rate.0,
        })
    }

    fn enqueue(&self, samples: &[f32]) {
        self.queue.enqueue(samples);
    }

    fn flush(&self) {
        self.queue.flush();
    }

    fn queued_samples(&self) -> usize {
        self.queue.len()
    }

    fn start(&self) -> bool {
        match self._stream.play() {
            Ok(()) => true,
            Err(error) => {
                eprintln!("failed to start audio stream: {error}");
                false
            }
        }
    }
}

struct RunningWindow {
    window: Rc<Window>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
}

struct App {
    context: Context<OwnedDisplayHandle>,
    audio: AudioOutput,
    console: Console,
    buttons: ButtonState,
    rewinding: bool,
    window: Option<RunningWindow>,
    pixels: [[u8; FRAME_WIDTH]; FRAME_HEIGHT],
    have_frame: bool,
    audio_started: bool,
    next_frame_at: Instant,
}

impl App {
    fn new(context: Context<OwnedDisplayHandle>, console: Console, audio: AudioOutput) -> Self {
        Self {
            context,
            audio,
            console,
            buttons: ButtonState::default(),
            rewinding: false,
            window: None,
            pixels: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
            have_frame: false,
            audio_started: false,
            next_frame_at: Instant::now(),
        }
    }

    fn update_buttons(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.buttons.set(button);
        } else {
            self.buttons.unset(button);
        }
        if !self.rewinding {
            self.console.update_buttons(self.buttons);
        }
    }

    fn advance_frame(&mut self) {
        if self.rewinding {
            self.console.rewind();
            self.console.update_buttons(self.buttons);
        }

        let frame = self.console.next_frame();
        if frame.audio_discontinuity {
            self.audio.flush();
        }
        debug_assert_eq!(frame.audio_sample_rate, self.audio.sample_rate);
        self.audio.enqueue(frame.audio_samples);
        if !self.audio_started && self.audio.queued_samples() >= (AUDIO_SAMPLE_RATE / 20) as usize {
            self.audio_started = self.audio.start();
        }
        self.pixels = *frame.pixels;
        self.have_frame = true;

        let now = Instant::now();
        self.next_frame_at = self
            .next_frame_at
            .checked_add(FRAME_DURATION)
            .unwrap_or(now);
        if self.next_frame_at < now {
            self.next_frame_at = now;
        }
    }

    fn render(&mut self) {
        let Some(running) = self.window.as_mut() else {
            return;
        };
        let Some(surface) = running.surface.as_mut() else {
            return;
        };
        let size = running.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };

        if let Err(error) = surface.resize(width, height) {
            eprintln!("failed to resize software surface: {error}");
            return;
        }

        let Ok(mut buffer) = surface.buffer_mut() else {
            eprintln!("failed to acquire software framebuffer");
            return;
        };
        let buffer_width = buffer.width().get() as usize;
        let buffer_height = buffer.height().get() as usize;
        buffer.fill(0);

        for (y, row) in self.pixels.iter().enumerate() {
            for (x, &palette_index) in row.iter().enumerate() {
                let color = NES_PALETTE_RGB[palette_index as usize & 0x3f];
                for y_offset in 0..SCALE {
                    let output_y = y * SCALE + y_offset;
                    if output_y >= buffer_height {
                        continue;
                    }
                    for x_offset in 0..SCALE {
                        let output_x = x * SCALE + x_offset;
                        if output_x < buffer_width {
                            buffer[output_y * buffer_width + output_x] = color;
                        }
                    }
                }
            }
        }

        if let Err(error) = buffer.present() {
            eprintln!("failed to present software framebuffer: {error}");
        }
    }

    fn redraw(&mut self) {
        if !self.have_frame || Instant::now() >= self.next_frame_at {
            self.advance_frame();
        }
        self.render();
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, state: ElementState) {
        let pressed = state == ElementState::Pressed;
        if pressed && code == KeyCode::Escape {
            event_loop.exit();
            return;
        }

        if code == KeyCode::KeyI {
            self.rewinding = pressed;
            if !pressed {
                self.console.update_buttons(self.buttons);
            }
            return;
        }

        let button = button_for_key(code);
        if let Some(button) = button {
            self.update_buttons(button, pressed);
        }

        if !pressed {
            let toggle_mask = match code {
                KeyCode::Digit1 => ChannelMask {
                    pulse1: true,
                    pulse2: false,
                    triangle: false,
                    noise: false,
                },
                KeyCode::Digit2 => ChannelMask {
                    pulse1: false,
                    pulse2: true,
                    triangle: false,
                    noise: false,
                },
                KeyCode::Digit3 => ChannelMask {
                    pulse1: false,
                    pulse2: false,
                    triangle: true,
                    noise: false,
                },
                KeyCode::Digit4 => ChannelMask {
                    pulse1: false,
                    pulse2: false,
                    triangle: false,
                    noise: true,
                },
                _ => return,
            };
            self.console.update_channel_mask(toggle_mask);
        }
    }
}

fn button_for_key(code: KeyCode) -> Option<Button> {
    match code {
        KeyCode::KeyW => Some(Button::Up),
        KeyCode::KeyA => Some(Button::Left),
        KeyCode::KeyS => Some(Button::Down),
        KeyCode::KeyD => Some(Button::Right),
        KeyCode::KeyJ => Some(Button::B),
        KeyCode::KeyK => Some(Button::A),
        KeyCode::Comma => Some(Button::Select),
        KeyCode::Period => Some(Button::Start),
        _ => None,
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init) {
            self.next_frame_at = Instant::now();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes()
                .with_title("nes-rs")
                .with_inner_size(winit::dpi::PhysicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
                .with_min_inner_size(winit::dpi::PhysicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
                .with_resizable(false);
            let window = Rc::new(
                event_loop
                    .create_window(attributes)
                    .expect("failed to create nes-rs window"),
            );
            self.window = Some(RunningWindow {
                surface: Some(
                    Surface::new(&self.context, Rc::clone(&window))
                        .expect("failed to create software surface"),
                ),
                window,
            });
        } else {
            let window = Rc::clone(&self.window.as_ref().unwrap().window);
            let surface =
                Surface::new(&self.context, window).expect("failed to recreate software surface");
            self.window.as_mut().unwrap().surface = Some(surface);
        }

        if let Some(running) = &self.window {
            running.window.request_redraw();
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(running) = self.window.as_mut() {
            running.surface = None;
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if !matches!(self.window.as_ref(), Some(running) if running.window.id() == window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.handle_key(event_loop, code, event.state);
                }
            }
            WindowEvent::Focused(false) => {
                self.buttons = ButtonState::default();
                self.rewinding = false;
                self.console.update_buttons(self.buttons);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame_at));
        if Instant::now() >= self.next_frame_at {
            if let Some(running) = &self.window {
                running.window.request_redraw();
            }
        }
    }
}

fn load_console(path: &PathBuf) -> Result<Console, Box<dyn Error>> {
    let mut rom = File::open(path)?;
    let (cartridge, mapper_number) = ines::load(&mut rom).ok_or("failed to parse iNES ROM")?;
    let mapper = cartridge::new(cartridge, mapper_number).ok_or("unsupported NES mapper")?;
    Ok(Console::new(mapper))
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_else(|| "nes-winit".into());
    let Some(rom_path) = args.next() else {
        return Err(format!("usage: {} ROM", PathBuf::from(program).display()).into());
    };
    if args.next().is_some() {
        return Err(format!("usage: {} ROM", PathBuf::from(program).display()).into());
    }

    let rom_path = PathBuf::from(rom_path);
    let console = load_console(&rom_path)?;
    let audio = AudioOutput::new()?;
    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())?;
    let mut app = App::new(context, console, audio);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("nes-winit: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::button_for_key;
    use nes_core::controller::Button;
    use winit::keyboard::KeyCode;

    #[test]
    fn keyboard_mapping_matches_nes_controller_bits() {
        let mappings = [
            (KeyCode::KeyW, Button::Up),
            (KeyCode::KeyA, Button::Left),
            (KeyCode::KeyS, Button::Down),
            (KeyCode::KeyD, Button::Right),
            (KeyCode::KeyJ, Button::B),
            (KeyCode::KeyK, Button::A),
            (KeyCode::Comma, Button::Select),
            (KeyCode::Period, Button::Start),
        ];

        for (key, button) in mappings {
            assert_eq!(button_for_key(key), Some(button));
        }
        assert_eq!(button_for_key(KeyCode::Escape), None);
    }
}
