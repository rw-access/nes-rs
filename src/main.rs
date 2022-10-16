use clap::builder::Str;
use clap::Parser;
use image::{write_buffer_with_format, GrayImage, ImageBuffer, Luma};
use nes::controller::ButtonState;
use nes::snapshot::Snapshot;
use nes::{cartridge, console::Console, controller::Button};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::sys::KeyCode;
// Construct a new RGB ImageBuffer with the specified width and height.

use std::collections::VecDeque;
use std::process::exit;
use std::time::Duration;

const PALETTE_RGB: [u32; 64] = [
    0x666666, 0x002A88, 0x1412A7, 0x3B00A4, 0x5C007E, 0x6E0040, 0x6C0600, 0x561D00, 0x333500,
    0x0B4800, 0x005200, 0x004F08, 0x00404D, 0x000000, 0x000000, 0x000000, 0xADADAD, 0x155FD9,
    0x4240FF, 0x7527FE, 0xA01ACC, 0xB71E7B, 0xB53120, 0x994E00, 0x6B6D00, 0x388700, 0x0C9300,
    0x008F32, 0x007C8D, 0x000000, 0x000000, 0x000000, 0xFFFEFF, 0x64B0FF, 0x9290FF, 0xC676FF,
    0xF36AFF, 0xFE6ECC, 0xFE8170, 0xEA9E22, 0xBCBE00, 0x88D800, 0x5CE430, 0x45E082, 0x48CDDE,
    0x4F4F4F, 0x000000, 0x000000, 0xFFFEFF, 0xC0DFFF, 0xD3D2FF, 0xE8C8FF, 0xFBC2FF, 0xFEC4EA,
    0xFECCC5, 0xF7D8A5, 0xE4E594, 0xCFEF96, 0xBDF4AB, 0xB3F3CC, 0xB5EBF2, 0xB8B8B8, 0x000000,
    0x000000,
];

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

    let mut img: GrayImage = ImageBuffer::new((1 + tiles_x * 9) as u32, (1 + tiles_y * 9) as u32);

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
    let frame_duration = Duration::from_secs(1) / 60;

    let mut rom_file = std::fs::File::open(rom_path).unwrap();

    let (c, m) = nes::ines::load(&mut rom_file).expect("failed to load cartridge");
    let mapper = cartridge::new(c, m).unwrap();
    let mut console = Console::new(mapper);

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

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

    let mut history: VecDeque<Snapshot> = VecDeque::new();

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();

    let mut event_pump = sdl_context.event_pump().unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_target(PixelFormatEnum::RGB24, WIDTH * SCALING, HEIGHT * SCALING)
        .unwrap();

    let mut raw_texture = [0 as u8; (WIDTH * HEIGHT * SCALING * SCALING * 3) as usize];

    let mut rewind = false;
    let mut button_state = ButtonState::default();

    'run_loop: loop {
        let pre_draw = std::time::Instant::now();
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
                        console.update_buttons(button_state);
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
                        console.update_buttons(button_state);
                    }
                }
                _ => {}
            }
        }

        if rewind {
            match history.pop_back() {
                Some(snapshot) => {
                    console.restore_snapshot(snapshot, &cpu_ignore_rewind, &ppu_ignore_rewind)
                }
                None => {
                    console.update_buttons(button_state);
                    rewind = false;
                }
            }
        }

        console.wait_vblank();

        for (y, row) in console.screen().pixels.iter().enumerate() {
            for (x, palette_color) in row.iter().enumerate() {
                // decode the palette
                let [_, r, g, b] = PALETTE_RGB[*palette_color as usize].to_be_bytes();

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

        canvas.clear();
        texture
            .update(None, &raw_texture, (SCALING * WIDTH * 3) as usize)
            .unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        if !rewind {
            history.push_back(console.take_snapshot());
        }

        // sleep for 1/60th of a second
        let elapsed = pre_draw.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
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

fn main() {
    let args = CLI::parse();
    println!("size of snapshot = {}", std::mem::size_of::<Snapshot>());

    match args {
        CLI::CHRDump { rom, out } => save_png(&rom, &out),
        CLI::Play {
            rom,
            cpu_ignore_rewind,
            ppu_ignore_rewind,
        } => play_rom(&rom, cpu_ignore_rewind, ppu_ignore_rewind),
    };
}
