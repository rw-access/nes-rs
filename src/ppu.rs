use std::cell::Cell;

use crate::cartridge::{Mapper, MirroringMode};

struct PPUControl {
    base_nametable: u8, // two bits
    vram_increment: bool,
    sprite_pattern_table: bool,
    background_pattern_table: bool,
    tall_sprites: bool,
    master_select: bool,
    enable_nmi: bool,
}

impl From<u8> for PPUControl {
    fn from(raw: u8) -> Self {
        PPUControl {
            base_nametable: raw & 0b11,
            vram_increment: (raw & 0b100) != 0,
            sprite_pattern_table: (raw & 0b1000) != 0,
            background_pattern_table: (raw & 0b0001_0000) != 0,
            tall_sprites: (raw & 0b0010_0000) != 0,
            master_select: (raw & 0b0100_0000) != 0,
            enable_nmi: (raw & 0b1000_0000) != 0,
        }
    }
}

struct PPUMask {
    greyscale: bool,
    show_background_left8: bool,
    show_sprites_left8: bool,
    show_background: bool,
    show_sprites: bool,
    boost_red: bool,   // green on PAL
    boost_green: bool, // red on PAL
    boost_blue: bool,
}

impl From<u8> for PPUMask {
    fn from(raw: u8) -> Self {
        PPUMask {
            greyscale: (raw & 0b1) != 0,
            show_background_left8: (raw & 0b10) != 0,
            show_sprites_left8: (raw & 0b100) != 0,
            show_background: (raw & 0b1000) != 0,
            show_sprites: (raw & 0b0001_0000) != 0,
            boost_red: (raw & 0b0010_0000) != 0,
            boost_green: (raw & 0b0100_0000) != 0,
            boost_blue: (raw & 0b1000_0000) != 0,
        }
    }
}

struct PPUStatus {
    open_bus: u8, // five bits
    sprite_overflow: bool,
    sprite_zero_hit: bool,
    nmi_occurred: bool,
}

impl From<PPUStatus> for u8 {
    fn from(status: PPUStatus) -> u8 {
        (status.open_bus & 0b1_0000)
            | (status.sprite_overflow as u8) << 5
            | (status.sprite_zero_hit as u8) << 6
            | (status.nmi_occurred as u8) << 7
    }
}

struct VRAMAddress {
    coarse_x: u8,  //  0 ...  4
    coarse_y: u8,  //  5 ...  9
    nametable: u8, // 10 ... 11
    fine_y: u8,    // 12 ... 14
}

impl VRAMAddress {
    fn increment_x(&mut self) {
        // https://www.nesdev.org/wiki/PPU_scrolling#X_increment
        if self.coarse_x < 31 {
            self.coarse_x += 1;
        } else {
            self.nametable ^= 0b01;
            self.coarse_x = 0;
        }
    }
    fn increment_y(&mut self) {
        // https://www.nesdev.org/wiki/PPU_scrolling#Y_increment
        if self.fine_y < 7 {
            self.fine_y += 1;
        } else {
            self.fine_y = 0;
            if self.coarse_y < 29 {
                self.coarse_y += 1
            } else {
                self.coarse_y = 0;
                self.nametable ^= 0b10;
            }
        }
    }
    fn copy_x(&mut self, other: &VRAMAddress) {
        self.coarse_x = other.coarse_x;
        self.nametable = (self.nametable & 0b10) | (other.nametable & 0b01);
    }
    fn copy_y(&mut self, other: &VRAMAddress) {
        self.coarse_y = other.coarse_y;
        self.fine_y = other.fine_y;
        self.nametable = (self.nametable & 0b01) | (other.nametable & 0b10);
    }
}

impl From<VRAMAddress> for u16 {
    fn from(v: VRAMAddress) -> u16 {
        (v.coarse_x as u16)
            | ((v.coarse_y as u16) << 5)
            | ((v.nametable as u16) << 10)
            | ((v.fine_y as u16) << 12)
    }
}

impl From<u16> for VRAMAddress {
    fn from(raw: u16) -> Self {
        VRAMAddress {
            coarse_x: (raw & 0x1f) as u8,
            coarse_y: ((raw >> 5) & 0x1f) as u8,
            nametable: ((raw >> 10) & 0b11) as u8,
            fine_y: ((raw >> 12) & 0b111) as u8,
        }
    }
}

#[derive(Clone, Default, Debug)]
struct TileData {
    nametable_index: u8,
    palette: u8,
    pattern_low: u8,
    pattern_high: u8,
}

impl TileData {
    fn color(&self, x: u8) -> u8 {
        let shift = 7 - x;
        let lo = (self.pattern_low >> shift) & 0b1;
        let hi = (self.pattern_high >> shift) & 0b1;
        (hi << 1) | lo
    }
}

#[derive(Clone, Debug, Default)]
struct ParsedSprite {
    top_y: u8,
    tile_index: u8,
    palette: u8, // two bits
    behind_background: bool,
    flip_horizontal: bool,
    flip_vertical: bool,
    left_x: u8,
}

impl From<&[u8; 4]> for ParsedSprite {
    fn from(raw_sprite: &[u8; 4]) -> Self {
        ParsedSprite {
            top_y: raw_sprite[0],
            tile_index: raw_sprite[1],
            palette: raw_sprite[2] & 0b11,
            behind_background: (raw_sprite[2] & 0b0010_0000) != 0,
            flip_horizontal: (raw_sprite[2] & 0b0100_0000) != 0,
            flip_vertical: (raw_sprite[2] & 0b1000_0000) != 0,
            left_x: raw_sprite[3],
        }
    }
}

impl ParsedSprite {
    fn is_empty(&self) -> bool {
        self.top_y == 0xff && self.tile_index == 0xff && self.left_x == 0xff
    }
}

#[derive(Clone, Debug, Default)]
struct ProcessedSprite {
    sprite: ParsedSprite,
    tile: TileData,
}

impl ProcessedSprite {
    fn color(&self, x: u8) -> u8 {
        self.tile.color(if self.sprite.flip_horizontal {
            7 - x
        } else {
            x
        })
    }
}

#[derive(Clone)]
pub struct Screen {
    // indexes into the palette
    pub pixels: [[u8; 256]; 240],
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            pixels: [[0; 256]; 240],
        }
    }
}

#[derive(Clone)]
pub(crate) struct PPU {
    cycle_in_scanline: u16, // 0..=340
    scanline: u16,          // 0..=261
    frame: usize,
    control_reg: u8,
    status_reg: u8,
    mask_reg: u8,
    oam_addr: u8,
    buffered_ppu_data: Cell<u8>,
    v: u16,
    t: u16,
    w: bool,
    pub(crate) in_vblank: bool,
    fine_x: u8,
    pub(crate) pending_screen: Screen,
    oam: [u8; 256],
    secondary_oam: [u8; 32],
    palette_ram: [u8; 32],
    nametables: [u8; 2048],
    pending_nmi: bool,
    pending_tile: TileData,
    processed_tile: [TileData; 2],
    processed_sprites: [ProcessedSprite; 8],
    sprite_zero_in_line: bool,
    pub(crate) last_read: Cell<Option<u16>>,
}

impl Default for PPU {
    fn default() -> Self {
        Self {
            cycle_in_scanline: Default::default(),
            scanline: Default::default(),
            frame: Default::default(),
            control_reg: Default::default(),
            status_reg: Default::default(),
            mask_reg: Default::default(),
            oam_addr: Default::default(),
            buffered_ppu_data: Default::default(),
            v: Default::default(),
            t: Default::default(),
            w: Default::default(),
            pending_screen: Default::default(),
            oam: [0; 256],
            secondary_oam: Default::default(),
            palette_ram: [0; 32],
            nametables: [0; 2048],
            in_vblank: Default::default(),
            fine_x: Default::default(),
            pending_nmi: Default::default(),
            pending_tile: Default::default(),
            processed_tile: Default::default(),
            processed_sprites: Default::default(),
            sprite_zero_in_line: Default::default(),
            last_read: Default::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum MultiplexerDecision {
    DrawBackground = 0,
    DrawTile = 1,
    DrawSprite = 2,
}

impl PPU {
    pub(crate) fn reset(&mut self) {
        self.cycle_in_scanline = 0;
        self.scanline = 0;
        self.frame = 0;
        self.control_reg = 0;
        self.oam_addr = 0;
        self.mask_reg = 0;
        self.in_vblank = false;
        self.pending_nmi = false;
        self.last_read.set(None);
    }

    fn multiplex_colors(
        tile_palette: u8,
        tile_palette_offset: u8,
        sprite_palette: u8,
        sprite_palette_offset: u8,
        sprite_in_background: bool,
    ) -> (MultiplexerDecision, u8) {
        const decision_table: [MultiplexerDecision; 8] = [
            // bg==0, sp==0, priority==X
            MultiplexerDecision::DrawBackground,
            MultiplexerDecision::DrawBackground,
            // bg==0, sp!=0, priority==X
            MultiplexerDecision::DrawSprite,
            MultiplexerDecision::DrawSprite,
            // bg!=0, sp==0, priority==X
            MultiplexerDecision::DrawTile,
            MultiplexerDecision::DrawTile,
            // bg!=0, sp!=0, priority==foreground
            MultiplexerDecision::DrawSprite,
            // bg!=0, sp!=0, priority==background
            MultiplexerDecision::DrawTile,
        ];
        let decision_index: u8 = ((tile_palette != 0) as u8) << 2
            | ((sprite_palette != 0) as u8) << 1
            | (sprite_in_background as u8);
        let multiplexer_decision = decision_table[decision_index as usize];
        let colors: [u8; 3] = [
            // MultiplexerDecision::DrawBackground
            0,
            // MultiplexerDecision::DrawTile
            tile_palette_offset | tile_palette,
            // MultiplexerDecision::DrawSprite
            sprite_palette_offset | sprite_palette,
        ];

        (multiplexer_decision, colors[multiplexer_decision as usize])
    }

    fn rendering_enabled(&self) -> bool {
        let parsed_mask = PPUMask::from(self.mask_reg);
        return parsed_mask.show_background || parsed_mask.show_sprites;
    }

    pub(crate) fn step(&mut self, mapper: &dyn Mapper) {
        // change signals on the next cycle
        match self.last_read.get() {
            Some(0x2002) => {
                self.w = false;
                self.status_reg &= !0b1000_0000; // NMI occurred
            }
            Some(0x2007) => {
                self.v = self
                    .v
                    .wrapping_add(if PPUControl::from(self.control_reg).vram_increment {
                        32
                    } else {
                        1
                    })
            }
            _ => {}
        }

        self.last_read.set(None);

        match self.scanline {
            0..=239 => self.step_visible(mapper),
            240 => self.step_post_render(mapper),
            241..=260 => self.step_vblank(mapper),
            261 => self.step_pre_render(mapper),
            _ => unreachable!(),
        };

        self.update_cycle();
    }

    fn find_sprites_in_line(&mut self) {
        // Cycles 1-64: fill secondary OAM with 0xFF
        // Timing ultimately doesn't matter for accuracy because it's internal to sprite evaluation
        self.secondary_oam.fill(0xff);

        let sprite_height = if PPUControl::from(self.control_reg).tall_sprites {
            16
        } else {
            8
        };

        let mut overflow = false;
        let mut sprite_count: u8 = 0;
        let y = self.scanline;

        self.sprite_zero_in_line = false;

        // scan primary sprites, copying ones that are in range to the secondary OAM.
        // update overflow when > 8 are detected.
        // on a real NES, this is spread out from cycles 65-256, so hopefully
        // this approximation is accurate enough for most games
        for (idx, raw_sprite) in self.oam.chunks_exact(4).enumerate() {
            let raw_sprite: &[u8; 4] = raw_sprite.try_into().unwrap();
            let parsed_sprite = ParsedSprite::from(raw_sprite);

            let top_y = parsed_sprite.top_y as u16;

            if y >= top_y && y < top_y + sprite_height {
                if sprite_count == 8 {
                    overflow = true;
                    break;
                }

                self.sprite_zero_in_line |= idx == 0;
                self.secondary_oam[sprite_count as usize * 4..sprite_count as usize * 4 + 4]
                    .copy_from_slice(raw_sprite);
                sprite_count += 1;
            }
        }

        self.status_reg &= 1 << 5;
        self.status_reg |= (overflow as u8) << 5;
    }

    fn render_pixel(&mut self) {
        let x = self.cycle_in_scanline - 1;
        let y = self.scanline;

        // retrieve the background tile
        let fine_x = (x % 8) as u8 + self.fine_x;
        let tile = &self.processed_tile[(fine_x >= 8) as usize];
        let tile_palette = tile.color(fine_x % 8);
        let tile_palette_offset = (tile.palette & 0x3) << 2;

        // retrieve the matching sprite
        let mut sprite_palette: u8 = 0;
        let mut sprite_pos: u8 = 0;
        let mut sprite_palette_offset: u8 = 0;
        let mut sprite_in_background: bool = false;

        if PPUMask::from(self.control_reg).show_sprites {
            for (idx, processed_sprite) in self.processed_sprites.iter().enumerate() {
                if processed_sprite.sprite.is_empty() {
                    break;
                }

                let sprite_left: u16 = processed_sprite.sprite.left_x.into();
                if x >= sprite_left && x < sprite_left + 8 {
                    let sprite_x = x - (processed_sprite.sprite.left_x as u16);
                    sprite_palette = processed_sprite.color(sprite_x as u8);

                    if sprite_palette != 0 {
                        sprite_pos = idx as u8;
                        sprite_palette_offset = processed_sprite.sprite.palette << 2;
                        sprite_in_background = processed_sprite.sprite.behind_background;
                        break;
                    }
                }
            }
        }

        let (decision, color) = PPU::multiplex_colors(
            tile_palette,
            tile_palette_offset,
            sprite_palette,
            0x10 | sprite_palette_offset,
            sprite_in_background,
        );
        let zero_hit = self.sprite_zero_in_line
            && sprite_pos == 0
            && decision == MultiplexerDecision::DrawSprite;

        // set the sprite zero hit bit
        self.status_reg |= (zero_hit as u8) << 6;

        self.pending_screen.pixels[y as usize][x as usize] =
            self.palette_ram[PPU::mirror_palette(color) as usize];
    }

    fn step_visible(&mut self, mapper: &dyn Mapper) {
        if !self.rendering_enabled() {
            return;
        }

        match self.cycle_in_scanline {
            0 => {}
            1..=256 => {
                self.render_pixel();
                self.fetch_background_tile(mapper);
            }
            257 => {
                // Cycles 1-64: fill secondary OAM with 0xFF.
                // Cycles 65-256: Sprite evaluation
                self.find_sprites_in_line();
            }
            260 => {
                // TODO: mapper.on_scanline();
            }
            320 => {
                let ppu_control = PPUControl::from(self.control_reg);
                let sprite_height: u8 = if ppu_control.tall_sprites { 16 } else { 8 };
                let y = self.scanline;

                // Cycles 257-320: Sprite fetches (8 sprites total, 8 cycles per sprite).
                // Find the corresponding tiles for each sprite
                // 1-4: Read the Y-coordinate, tile number, attributes, and X-coordinate of the selected sprite from secondary OAM
                // 5-8: Read the X-coordinate of the selected sprite from secondary OAM 4 times (while the PPU fetches the sprite tile data)
                // For the first empty sprite slot, this will consist of sprite #63's Y-coordinate followed by 3 $FF bytes; for subsequent empty sprite slots, this will be four $FF bytes
                for (idx, raw_sprite) in self.secondary_oam.chunks_exact(4).enumerate() {
                    let raw_sprite: &[u8; 4] = raw_sprite.try_into().unwrap();
                    let processed_sprite = &mut self.processed_sprites[idx];
                    processed_sprite.sprite = ParsedSprite::from(raw_sprite);

                    // continue if the sprite is empty
                    if raw_sprite == &[0xff; 4] {
                        continue;
                    }

                    // retrieve the corresponding tile
                    let bank = if ppu_control.tall_sprites {
                        processed_sprite.sprite.tile_index & 0b1
                    } else {
                        ppu_control.sprite_pattern_table as u8
                    };

                    let pattern_table_address = (bank as u16) << 12;
                    let mut tile_index =
                        processed_sprite.sprite.tile_index & !(ppu_control.tall_sprites as u8);
                    let mut tile_y = (y - (processed_sprite.sprite.top_y as u16)) as u8;

                    tile_y = if processed_sprite.sprite.flip_vertical {
                        sprite_height - 1 - tile_y
                    } else {
                        tile_y
                    };

                    tile_index &= !(ppu_control.tall_sprites as u8);
                    tile_index += (tile_y >= 8) as u8;
                    tile_y &= 0x7;

                    let tile_address_lo =
                        pattern_table_address | (tile_index as u16) << 4 | (0 << 3) | tile_y as u16;
                    let tile_address_hi = tile_address_lo | (1 << 3);

                    processed_sprite.tile = TileData {
                        nametable_index: 0,
                        palette: processed_sprite.sprite.palette,
                        pattern_low: mapper.read(tile_address_lo),
                        pattern_high: mapper.read(tile_address_hi),
                    }
                }
            }
            321..=336 => {
                // Cycles 321-336: This is where the first two tiles for the next scanline are fetched,
                // and loaded into the shift registers. Again, each memory access takes 2 PPU cycles to
                // complete, and 4 are performed for the two tiles:
                self.fetch_background_tile(mapper);
            }
            _ => {}
        }

        self.update_vram_addr();
    }

    fn step_post_render(&mut self, mapper: &dyn Mapper) {}

    fn step_vblank(&mut self, mapper: &dyn Mapper) {
        if self.scanline == 241 && self.cycle_in_scanline == 1 {
            self.in_vblank = true;
            self.status_reg |= 0b1000_0000; // nmi occurred bit

            self.pending_nmi = PPUControl::from(self.control_reg).enable_nmi;
        }
    }

    fn step_pre_render(&mut self, mapper: &dyn Mapper) {
        // Pre-render scanline (-1 or 261)
        if self.cycle_in_scanline == 1 {
            // disable sprite zero hit + nmi occurred
            self.status_reg &= !0b1100_0000;
            self.in_vblank = false;
            self.pending_nmi = false;
        }

        if !self.rendering_enabled() {
            return;
        }

        match self.cycle_in_scanline {
            0 => {}                                          // idle
            1..=256 => self.fetch_background_tile(mapper),   // ignored tile fetch
            260 => {}                                        // notify mapper scanline
            321..=336 => self.fetch_background_tile(mapper), // tile for next line
            _ => {}                                          // nothing
        };

        self.update_vram_addr();
    }

    fn fetch_background_tile(&mut self, mapper: &dyn Mapper) {
        // https://www.nesdev.org/wiki/PPU_scrolling#Tile_and_attribute_fetching
        match self.cycle_in_scanline % 8 {
            0 => self.processed_tile = [self.processed_tile[1].clone(), self.pending_tile.clone()],
            1 => {
                let nametable_addr = 0x2000 | (self.v & 0x0FFF);
                self.pending_tile.nametable_index = self.read_byte(mapper, nametable_addr)
            }
            2 => {}
            3 => {
                // https://www.nesdev.org/wiki/PPU_scrolling#Tile_and_attribute_fetching
                // https://www.nesdev.org/wiki/PPU_attribute_tables
                let attr_address =
                    0x23C0 | (self.v & 0x0C00) | ((self.v >> 4) & 0x38) | ((self.v >> 2) & 0x07);
                let attr_data = self.read_byte(mapper, attr_address);
                let attr_shift = (self.v & 0x40) >> 4 | (self.v & 0x2);
                self.pending_tile.palette = (attr_data >> attr_shift) & 0b11;
            }
            4 => {}
            5 => {
                // two pattern tables: 0x0000 and 0x1000
                // xxxx xxxx xxxx xxxx
                //                 ^^^--- fine Y
                //      ^^^^ ^^^^ ------- tile
                //                0------ low byte
                //    ^ ---- ---- ------- foreground/background
                let pattern_table =
                    (PPUControl::from(self.control_reg).background_pattern_table as u16) << 12;
                let nametable_index = (self.pending_tile.nametable_index as u16) << 4;
                let lo_byte_offset = 0 << 3;
                let fine_y = VRAMAddress::from(self.v).fine_y as u16;
                let pattern_low_address = pattern_table | nametable_index | lo_byte_offset | fine_y;
                self.pending_tile.pattern_low = self.read_byte(mapper, pattern_low_address);
            }
            6 => {}
            7 => {
                //two pattern tables: 0x0000 and 0x1000
                // xxxx xxxx xxxx xxxx
                //                 ^^^--- fine Y
                //      ^^^^ ^^^^ ------- tile
                //                1------ high byte
                //    ^ ---- ---- ------- foreground/background
                let pattern_table =
                    (PPUControl::from(self.control_reg).background_pattern_table as u16) << 12;
                let nametable_index = (self.pending_tile.nametable_index as u16) << 4;
                let hi_byte_offset = 1 << 3;
                let fine_y = VRAMAddress::from(self.v).fine_y as u16;
                let pattern_high_address =
                    pattern_table | nametable_index | hi_byte_offset | fine_y;
                self.pending_tile.pattern_high = self.read_byte(mapper, pattern_high_address);
            }
            _ => unreachable!(),
        };
    }

    fn update_vram_addr(&mut self) {
        if !self.rendering_enabled() {
            return;
        }

        match (self.scanline, self.cycle_in_scanline) {
            (_, 256) => {
                // https://www.nesdev.org/wiki/PPU_scrolling#At_dot_256_of_each_scanline
                let mut parsed_addr = VRAMAddress::from(self.v);
                parsed_addr.increment_y();
                self.v = parsed_addr.into();
            }
            (_, 257) => {
                // https://www.nesdev.org/wiki/PPU_scrolling#At_dot_257_of_each_scanline
                // If rendering is enabled, the PPU copies all bits related to horizontal position from t to v:
                // v: ....A.. ...BCDEF <- t: ....A.. ...BCDEF
                let mut parsed_addr = VRAMAddress::from(self.v);
                parsed_addr.copy_x(&self.t.into());
                self.v = parsed_addr.into();
            }
            (261, 280..=304) => {
                // If rendering is enabled, at the end of vblank, shortly after the horizontal bits are copied from
                // t to v at dot 257, the PPU will repeatedly copy the vertical bits from t to v from dots 280 to 304,
                // completing the full initialization of v from t:
                // v: GHIA.BC DEF..... <- t: GHIA.BC DEF.....
                let mut parsed_addr = VRAMAddress::from(self.v);
                parsed_addr.copy_y(&self.t.into());
                self.v = parsed_addr.into();
            }
            (_, 1..=256 | 328..) if self.cycle_in_scanline % 8 == 0 => {
                // https://www.nesdev.org/wiki/PPU_scrolling#Between_dot_328_of_a_scanline,_and_256_of_the_next_scanline
                // If rendering is enabled, the PPU increments the horizontal position in v many times across the scanline,
                // it begins at dots 328 and 336, and will continue through the next scanline at 8, 16, 24... 240, 248, 256
                // (every 8 dots across the scanline until 256). Across the scanline the effective coarse X scroll coordinate
                // is incremented repeatedly, which will also wrap to the next nametable appropriately
                let mut parsed_addr = VRAMAddress::from(self.v);
                parsed_addr.increment_x();
                self.v = parsed_addr.into();
            }
            _ => {}
        }
    }

    fn update_cycle(&mut self) {
        if self.cycle_in_scanline < 340 {
            // advance in current scanline
            self.cycle_in_scanline += 1;
        } else if self.scanline < 261 {
            // advance to next scanline
            self.scanline += 1;
            self.cycle_in_scanline = 0;
        } else {
            // move to next frame
            self.frame = self.frame.wrapping_add(1);
            self.scanline = 0;

            // https://www.nesdev.org/wiki/PPU_frame_timing#Even/Odd_Frames
            // https://www.nesdev.org/wiki/File:Ntsc_timing.png
            // skip the first cycle of a frame when odd + rendering enabled
            self.cycle_in_scanline = (self.rendering_enabled() && (self.frame % 2 == 1)) as u16;
        }
    }

    fn mirror_nametable(addr: u16, mode: MirroringMode) -> u16 {
        let nametable_offset = addr % 0x400;

        // 0x2000, 0x2400, 0x2800, 0x2C00
        let mirroring: [u8; 4] = match mode {
            MirroringMode::Horizontal => [0, 0, 1, 1],
            MirroringMode::Vertical => [0, 1, 0, 1],
            MirroringMode::SingleScreenLowerBank => [0, 0, 0, 0],
            MirroringMode::FourScreen => [0, 1, 2, 3],
            MirroringMode::SingleScreenUpperBank => [0, 0, 0, 0],
        };

        let nametable_select = (addr >> 10) % 4;
        let nametable_bank = mirroring[nametable_select as usize];
        (nametable_bank as u16) << 10 | nametable_offset
    }

    fn mirror_palette(offset: u8) -> u8 {
        // Expected range [0x00, 0x1F]
        // Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
        //           $10/$14/$18/$1C are mirrors of $00/$04/$08/$0C
        // Perform with no branching logic
        let is_mirrored = (offset & 0x13) == 0x10;
        offset & !((is_mirrored as u8) << 4)
    }

    fn read_byte(&self, mapper: &dyn Mapper, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => mapper.read(addr),
            0x2000..=0x3eff => {
                self.nametables[PPU::mirror_nametable(addr, mapper.mirror()) as usize]
            }
            0x3f00.. => self.palette_ram[PPU::mirror_palette((addr % 0x20) as u8) as usize],
        }
    }

    pub(crate) fn write_dma(&mut self, page: Option<&[u8; 256]>) {
        match page {
            Some(page) => {
                if self.oam_addr == 0 {
                    self.oam.copy_from_slice(page);
                } else {
                    // not ideal but manageable:
                    // oam addr isn't perfectly aligned, perform two separate memcpys
                    let (before, after) = page.split_at(page.len() - (self.oam_addr as usize));
                    self.oam[self.oam_addr as usize..].copy_from_slice(before);
                    self.oam[..self.oam_addr as usize].copy_from_slice(after);
                }
            }
            None => self.oam.fill(0x00),
        }
    }

    fn write_byte(&mut self, mapper: &mut dyn Mapper, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1fff => mapper.write(addr, data),
            0x2000..=0x3eff => {
                self.nametables[PPU::mirror_nametable(addr, mapper.mirror()) as usize] = data;
            }
            0x3f00.. => self.palette_ram[PPU::mirror_palette((addr % 0x20) as u8) as usize] = data,
        }
    }

    // check the interrupt line and set it low
    pub(crate) fn read_nmi_line(&mut self) -> bool {
        let status = self.pending_nmi;
        self.pending_nmi = false;

        status
    }

    pub(crate) fn read_register(&self, mapper: &dyn Mapper, addr: u16) -> u8 {
        // change statuses signals on the next step()
        self.last_read.set(Some(0x2000 | (addr & 0xf)));

        match 0x2000 | (addr & 0xf) {
            0x2002 => {
                // PPUSTATUS: $2002
                self.status_reg
            }
            0x2004 => {
                // OAMDATA: $2004
                self.oam[self.oam_addr as usize]
            }
            0x2007 => {
                // PPUDATA: $2007
                let mut contents = self.read_byte(mapper, self.v);

                match self.v {
                    0x0000..=0x3eff => {
                        let latest_buffered = self.buffered_ppu_data.get();
                        self.buffered_ppu_data.set(contents);
                        contents = latest_buffered;
                    }
                    0x3f00..=0x3fff => {
                        self.buffered_ppu_data
                            .set(self.read_byte(mapper, self.v ^ 0x1000));
                    }
                    _ => {}
                };

                contents
            }
            _ => 0,
        }
    }

    pub(crate) fn write_register(&mut self, mapper: &mut dyn Mapper, addr: u16, data: u8) {
        match 0x2000 | (addr & 0xf) {
            0x2000 => {
                // PPUCTRL: $2000
                let parsed_prev_ctrl = PPUControl::from(self.control_reg);
                let parsed_next_ctrl = PPUControl::from(data);

                // detect if in vblank and a positive edge on enable_nmi, then send interrupt
                // https://www.nesdev.org/wiki/NMI
                if self.in_vblank && !parsed_prev_ctrl.enable_nmi && parsed_next_ctrl.enable_nmi {
                    self.pending_nmi = true;
                }

                self.control_reg = data;
                self.t = {
                    let mut t = VRAMAddress::from(self.t);
                    t.nametable = parsed_next_ctrl.base_nametable;
                    t.into()
                }
            }
            0x2001 => {
                // PPUMASK: $2001
                self.mask_reg = data;
            }
            0x2003 => {
                // OAMADDR: $2003
                self.oam_addr = data;
            }
            0x2004 => {
                // OAMDATA: $2004
                self.oam[self.oam_addr as usize] = data;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2005 => {
                // PPUSCROLL: $2005
                if !self.w {
                    // t: ....... ...ABCDE <- d: ABCDE...
                    // x:              FGH <- d: .....FGH
                    self.w = true;
                    self.t = {
                        let mut t = VRAMAddress::from(self.t);
                        t.coarse_x = data >> 3;
                        t.into()
                    };
                    self.fine_x = data & 0b111;
                } else {
                    // t: FGH..AB CDE..... <- d: ABCDEFGH
                    self.w = false;
                    self.t = {
                        let mut t = VRAMAddress::from(self.t);
                        t.coarse_y = data >> 3;
                        t.fine_y = data & 0b111;
                        t.into()
                    };
                }
            }
            0x2006 => {
                // PPUADDR: $2006
                if !self.w {
                    // t: .CDEFGH ........ <- d: ..CDEFGH
                    //        <unused>     <- d: AB......
                    // t: Z...... ........ <- 0 (bit Z is cleared)
                    let mask = 0x80ff;
                    self.w = true;
                    self.t = self.t & mask | (data as u16) << 8 & !mask;
                } else {
                    // t: ....... ABCDEFGH <- d: ABCDEFGH
                    // v: <...all bits...> <- t: <...all bits...>
                    self.t = (self.t & 0xff00) | (data as u16);
                    self.v = self.t;
                    self.w = false;
                }
            }
            0x2007 => {
                // PPUDATA: $2007
                self.write_byte(mapper, self.v, data);
                self.v = self.v.wrapping_add({
                    let vram_incr = PPUControl::from(self.control_reg).vram_increment;
                    if vram_incr {
                        32
                    } else {
                        1
                    }
                });
            }
            _ => unreachable!(),
        };
    }
}
