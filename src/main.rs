use image::{write_buffer_with_format, GrayImage, ImageBuffer, Luma};

// Construct a new RGB ImageBuffer with the specified width and height.


use std::process::exit;

fn save_png(rom_path: &str, bmp_path: &str) {
    const TILES_PER_BANK: usize = 0x2000 / 16;

    let mut rom_file = std::fs::File::open(rom_path).unwrap();
    let mut bmp_file = std::fs::File::create(bmp_path).unwrap();

    let c = nes::ines::load(&mut rom_file).expect("failed to load cartridge");

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
                let hi_y = tile[tile_y+8];

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

    write_buffer_with_format(&mut bmp_file, &img, img.width(), img.height(), image::ColorType::L8, image::ImageOutputFormat::Png).expect("failed to save image")
}

fn main() {
    let str_args: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = str_args.iter().map(|s| s.as_str()).collect();

    match args[1..] {
        ["chr-dump", rom_path, png_path] => save_png(rom_path, png_path),
        _ => {
            println!(
                "usage:
            chr-dump <in_file.nes> <out_file.png>
            "
            );
            exit(1);
        }
    }
}
