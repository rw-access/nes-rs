use image::{write_buffer_with_format, GrayImage, ImageBuffer, Luma};
use nes::{cartridge, console::Console};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
// Construct a new RGB ImageBuffer with the specified width and height.

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

fn save_png(rom_path: &str, bmp_path: &str) {
    const TILES_PER_BANK: usize = 0x2000 / 16;

    let mut rom_file = std::fs::File::open(rom_path).unwrap();
    let mut bmp_file = std::fs::File::create(bmp_path).unwrap();

    let (c, _) = nes::ines::load(&mut rom_file).expect("failed to load cartridge");

    let num_tiles = c.chr.len() * TILES_PER_BANK;
    let tiles_x = 32 as usize;
    let tiles_y = num_tiles / tiles_x;

    let mut img: GrayImage = ImageBuffer::new((1 + tiles_x * 9) as u32, (1 + tiles_y * 9) as u32);

    for (bank_no, bank) in c.chr.iter().enumerate() {
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

fn play_rom(rom_path: &str) {
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
        .window("nes-rs", WIDTH, HEIGHT)
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

    let mut event_pump = sdl_context.event_pump().unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_target(PixelFormatEnum::RGB24, 256, 240)
        .unwrap();

    let mut raw_texture = [0 as u8; (WIDTH * HEIGHT * 3) as usize];

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
                _ => {}
            }
        }

        console.wait_vblank();

        let mut idx = 0;
        for row in console.screen().pixels {
            for column in row {
                // decode the palette
                let [_, r, g, b] = PALETTE_RGB[column as usize].to_be_bytes();
                raw_texture[idx] = r;
                raw_texture[idx + 1] = g;
                raw_texture[idx + 2] = b;
                idx += 3;
            }
        }

        canvas.clear();
        texture
            .update(None, &raw_texture, (WIDTH * 3) as usize)
            .unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        // sleep for 1/60th of a second
        let elapsed = pre_draw.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }
}

fn main() {
    let str_args: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = str_args.iter().map(|s| s.as_str()).collect();

    match args[1..] {
        ["chr-dump", rom_path, png_path] => save_png(rom_path, png_path),
        ["play", rom_path] => play_rom(rom_path),
        _ => {
            println!(
                "usage:
            chr-dump <in_file.nes> <out_file.png>
            play <in_file.nes> 
            "
            );
            exit(1);
        }
    }
}
