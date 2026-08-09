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

#[derive(Clone, Copy, Default, Debug)]
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

#[derive(Clone, Copy, Default)]
struct SpritePixel {
    palette: u8,
    position: u8,
    palette_offset: u8,
    behind_background: bool,
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
    oam: [u8; 256],
    secondary_oam: [u8; 32],
    palette_ram: [u8; 32],
    nametables: [u8; 4096],
    nametable_mirroring: MirroringMode,
    nametable_map: [u16; 4096],
    pending_nmi: bool,
    pending_tile: TileData,
    processed_tile: [TileData; 2],
    processed_sprites: [ProcessedSprite; 8],
    sprite_pixels: [SpritePixel; 256],
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
            oam: [0; 256],
            secondary_oam: Default::default(),
            palette_ram: [0; 32],
            nametables: [0; 4096],
            nametable_mirroring: MirroringMode::Horizontal,
            nametable_map: Self::build_nametable_map(MirroringMode::Horizontal),
            in_vblank: Default::default(),
            fine_x: Default::default(),
            pending_nmi: Default::default(),
            pending_tile: Default::default(),
            processed_tile: Default::default(),
            processed_sprites: Default::default(),
            sprite_pixels: [SpritePixel::default(); 256],
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
    fn can_batch_three_idle_ticks(&self) -> bool {
        if self.cycle_in_scanline > 337 {
            return false;
        }

        match self.scanline {
            240 | 242..=260 => true,
            241 => self.cycle_in_scanline >= 2,
            0..=239 => {
                if !self.rendering_enabled() {
                    true
                } else {
                    matches!(self.cycle_in_scanline, 261..=317 | 337)
                }
            }
            261 => {
                if !self.rendering_enabled() {
                    self.cycle_in_scanline >= 2
                } else {
                    matches!(self.cycle_in_scanline, 258..=277 | 305..=325 | 337)
                }
            }
            _ => false,
        }
    }

    pub(crate) fn step_cpu_cycle<M: Mapper + ?Sized>(
        &mut self,
        mapper: &mut M,
        screen: &mut Screen,
    ) {
        if self.scanline <= 239
            && self.rendering_enabled()
            && self.cycle_in_scanline <= 338
            && !matches!(self.cycle_in_scanline, 261..=317 | 337)
        {
            self.apply_deferred_read();
            self.last_read.set(None);
            self.step_visible(mapper, screen);
            self.update_cycle();
            self.step_visible(mapper, screen);
            self.update_cycle();
            self.step_visible(mapper, screen);
            self.update_cycle();
            return;
        }

        if self.can_batch_three_idle_ticks() {
            if self.last_read.get().is_some() {
                // Apply the deferred CPU-visible read effect on the first
                // tick, then advance the two remaining idle ticks directly.
                self.step(mapper, screen);
                self.cycle_in_scanline += 2;
            } else {
                self.cycle_in_scanline += 3;
            }
            return;
        }

        self.step(mapper, screen);
        self.step(mapper, screen);
        self.step(mapper, screen);
    }

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

    #[inline]
    pub(crate) fn refresh_nametable_mirroring<M: Mapper + ?Sized>(&mut self, mapper: &M) {
        self.nametable_mirroring = mapper.mirror();
        self.nametable_map = Self::build_nametable_map(self.nametable_mirroring);
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
        self.mask_reg & 0x18 != 0
    }

    fn apply_deferred_read(&mut self) {
        // change signals on the next cycle
        match self.last_read.get() {
            Some(0x2002) => {
                self.w = false;
                self.status_reg &= !0b1000_0000; // NMI occurred
            }
            Some(0x2007) => {
                self.v = self
                    .v
                    .wrapping_add(if self.control_reg & 4 != 0 { 32 } else { 1 })
            }
            _ => {}
        }
    }

    #[cfg(feature = "timestamped-scheduler")]
    fn idle_ticks_until_boundary(&self) -> u64 {
        let position = self.scanline as u64 * 341 + self.cycle_in_scanline as u64;
        match self.scanline {
            240 => 241 * 341 - position,
            241 if self.cycle_in_scanline >= 2 => 261 * 341 - position,
            242..=260 => 261 * 341 - position,
            _ => 0,
        }
    }

    #[cfg(feature = "timestamped-scheduler")]
    fn advance_idle_ticks(&mut self, ticks: u64) {
        debug_assert!(ticks > 0);
        debug_assert!(ticks <= self.idle_ticks_until_boundary());
        let position = self.cycle_in_scanline as u64 + ticks;
        self.scanline += (position / 341) as u16;
        self.cycle_in_scanline = (position % 341) as u16;
    }

    /// Return the number of PPU ticks until the next vblank-entry action is
    /// executed. The extra tick accounts for `step()` applying the action at
    /// the current dot before advancing to the following dot.
    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn next_vblank_in_ticks(&self) -> u64 {
        const VBLANK_EVENT: u64 = 241 * 341 + 1;
        let current = self.scanline as u64 * 341 + self.cycle_in_scanline as u64;

        if current <= VBLANK_EVENT {
            return VBLANK_EVENT - current + 1;
        }

        let ticks_to_next_frame = 262 * 341 - current;
        let next_frame_start = (self.rendering_enabled() && ((self.frame + 1) & 1 == 1)) as u64;
        ticks_to_next_frame + VBLANK_EVENT - next_frame_start + 1
    }

    /// Return the next PPU state transition that the timestamped console
    /// runner must expose to its frame loop. While in vblank this is the
    /// pre-render dot that clears vblank; otherwise it is the next vblank
    /// entry/NMI edge.
    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn next_scheduler_event_in_ticks(&self) -> u64 {
        if !self.in_vblank {
            return self.next_vblank_in_ticks();
        }

        const PRE_RENDER_CLEAR: u64 = 261 * 341 + 1;
        let current = self.scanline as u64 * 341 + self.cycle_in_scanline as u64;
        if current <= PRE_RENDER_CLEAR {
            PRE_RENDER_CLEAR - current + 1
        } else {
            // This state is not normally observable, but keep the deadline
            // finite if a caller starts from a manually constructed PPU.
            1
        }
    }

    /// Catch the PPU up to an absolute master-clock timestamp. The exact
    /// stepping path remains the authority at all eventful boundaries; only
    /// intervals with no rendering, mapper, or interrupt work are arithmetic.
    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn catch_up_to<M: Mapper + ?Sized>(
        &mut self,
        current_master_ticks: u64,
        target_master_ticks: u64,
        mapper: &mut M,
        screen: &mut Screen,
    ) -> u64 {
        debug_assert!(target_master_ticks >= current_master_ticks);
        let mut master_ticks = current_master_ticks;
        while master_ticks < target_master_ticks {
            if self.last_read.get().is_some() {
                self.step(mapper, screen);
                master_ticks += 1;
                continue;
            }

            if self.rendering_enabled()
                && (0..=239).contains(&self.scanline)
                && self.cycle_in_scanline == 0
                && target_master_ticks - master_ticks >= 341
            {
                self.catch_up_visible_scanline(mapper, screen);
                master_ticks += 341;
                continue;
            }

            let idle = self.idle_ticks_until_boundary();
            if idle != 0 {
                let ticks = idle.min(target_master_ticks - master_ticks);
                self.advance_idle_ticks(ticks);
                master_ticks += ticks;
                continue;
            }

            if target_master_ticks - master_ticks >= 3 {
                self.step_cpu_cycle(mapper, screen);
                master_ticks += 3;
            } else {
                self.step(mapper, screen);
                master_ticks += 1;
            }
        }
        master_ticks
    }

    /// Process the pixel/background portion of a visible scanline without
    /// re-running the generic cycle dispatcher for every dot. Eventful dots
    /// 257-340 remain on the exact path so sprite evaluation, mapper clocks,
    /// and sprite fetch preparation retain their existing timing.
    #[cfg(feature = "timestamped-scheduler")]
    fn catch_up_visible_scanline<M: Mapper + ?Sized>(
        &mut self,
        mapper: &mut M,
        screen: &mut Screen,
    ) {
        debug_assert!(self.rendering_enabled());
        debug_assert!((0..=239).contains(&self.scanline));
        debug_assert_eq!(self.cycle_in_scanline, 0);
        debug_assert!(self.last_read.get().is_none());

        self.cycle_in_scanline = 1;
        while self.cycle_in_scanline <= 256 {
            self.render_pixel(screen);
            self.fetch_background_tile(mapper);
            if self.cycle_in_scanline & 7 == 0 {
                self.update_vram_addr();
            }
            self.cycle_in_scanline += 1;
        }

        self.cycle_in_scanline = 257;
        self.find_sprites_in_line();
        self.update_vram_addr();

        self.cycle_in_scanline = 258;
        self.cycle_in_scanline = 260;
        mapper.clock_scanline();

        self.cycle_in_scanline = 261;
        self.cycle_in_scanline = 320;
        self.prepare_sprites_for_line(mapper);

        self.cycle_in_scanline = 321;
        while self.cycle_in_scanline <= 336 {
            self.fetch_background_tile(mapper);
            if self.cycle_in_scanline & 7 == 0 {
                self.update_vram_addr();
            }
            self.cycle_in_scanline += 1;
        }

        self.cycle_in_scanline = 0;
        self.scanline += 1;
    }

    pub(crate) fn step<M: Mapper + ?Sized>(&mut self, mapper: &mut M, screen: &mut Screen) {
        self.apply_deferred_read();
        self.last_read.set(None);

        // Scanlines 242-260 are entirely idle after the vblank edge. Keep
        // advancing dots, but avoid re-entering the full visible/vblank
        // dispatch for every PPU cycle. The read-latch transition above must
        // still run first because CPU-visible PPU reads can occur in vblank.
        if (242..=260).contains(&self.scanline) {
            self.update_cycle();
            return;
        }

        // With rendering enabled, these visible-line dots have no pixel,
        // sprite, mapper, or VRAM-address work. Keep them as single ticks so
        // CPU/PPU interleaving remains unchanged, but avoid the full visible
        // dispatch and its per-cycle event checks.
        if (0..=239).contains(&self.scanline)
            && matches!(
                self.cycle_in_scanline,
                0 | 258..=259 | 261..=319 | 337..=340
            )
            && self.rendering_enabled()
        {
            self.update_cycle();
            return;
        }

        match self.scanline {
            0..=239 => self.step_visible(mapper, screen),
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

        let sprite_height = if self.control_reg & 0x20 != 0 { 16 } else { 8 };

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

    fn render_pixel(&mut self, screen: &mut Screen) {
        let x = self.cycle_in_scanline - 1;
        let y = self.scanline;

        // retrieve the background tile
        let fine_x = (x as u8 & 7) + self.fine_x;
        let tile = &self.processed_tile[(fine_x >= 8) as usize];
        let tile_palette = tile.color(fine_x % 8);
        let tile_palette_offset = (tile.palette & 0x3) << 2;

        // Sprite pixels are prepared once per scanline, when the sprite tiles
        // are fetched, rather than scanning all eight sprites for every pixel.
        let sprite = if self.mask_reg & 0x10 != 0 {
            self.sprite_pixels[x as usize]
        } else {
            SpritePixel::default()
        };

        let (decision, color) = PPU::multiplex_colors(
            tile_palette,
            tile_palette_offset,
            sprite.palette,
            0x10 | sprite.palette_offset,
            sprite.behind_background,
        );
        let zero_hit = self.sprite_zero_in_line
            && sprite.position == 0
            && decision == MultiplexerDecision::DrawSprite;

        // set the sprite zero hit bit
        self.status_reg |= (zero_hit as u8) << 6;

        screen.pixels[y as usize][x as usize] =
            self.palette_ram[PPU::mirror_palette(color) as usize];
    }

    fn step_visible<M: Mapper + ?Sized>(&mut self, mapper: &mut M, screen: &mut Screen) {
        if !self.rendering_enabled() {
            return;
        }

        match self.cycle_in_scanline {
            0 => {}
            1..=256 => {
                self.render_pixel(screen);
                self.fetch_background_tile(mapper);
                if self.cycle_in_scanline & 7 == 0 {
                    self.update_vram_addr();
                }
            }
            257 => {
                // Cycles 1-64: fill secondary OAM with 0xFF.
                // Cycles 65-256: Sprite evaluation
                self.find_sprites_in_line();
                self.update_vram_addr();
            }
            260 => {
                mapper.clock_scanline();
            }
            320 => {
                self.prepare_sprites_for_line(mapper);
            }
            321..=336 => {
                // Cycles 321-336: This is where the first two tiles for the next scanline are fetched,
                // and loaded into the shift registers. Again, each memory access takes 2 PPU cycles to
                // complete, and 4 are performed for the two tiles:
                self.fetch_background_tile(mapper);
                if self.cycle_in_scanline & 7 == 0 {
                    self.update_vram_addr();
                }
            }
            _ => {}
        }
    }

    #[inline]
    fn prepare_sprites_for_line<M: Mapper + ?Sized>(&mut self, mapper: &M) {
        let tall_sprites = self.control_reg & 0x20 != 0;
        let sprite_height: u8 = if tall_sprites { 16 } else { 8 };
        let y = self.scanline;

        // Cycles 257-320: sprite fetches (8 sprites total, 8 cycles per sprite).
        for (idx, raw_sprite) in self.secondary_oam.chunks_exact(4).enumerate() {
            let raw_sprite: &[u8; 4] = raw_sprite.try_into().unwrap();
            let processed_sprite = &mut self.processed_sprites[idx];
            processed_sprite.sprite = ParsedSprite::from(raw_sprite);

            if raw_sprite == &[0xff; 4] {
                continue;
            }

            let bank = if tall_sprites {
                processed_sprite.sprite.tile_index & 0b1
            } else {
                (self.control_reg >> 3) & 1
            };

            let pattern_table_address = (bank as u16) << 12;
            let mut tile_index = processed_sprite.sprite.tile_index & !(tall_sprites as u8);
            let mut tile_y = (y - (processed_sprite.sprite.top_y as u16)) as u8;

            tile_y = if processed_sprite.sprite.flip_vertical {
                sprite_height - 1 - tile_y
            } else {
                tile_y
            };

            tile_index &= !(tall_sprites as u8);
            tile_index += (tile_y >= 8) as u8;
            tile_y &= 0x7;

            let tile_address_lo = pattern_table_address | (tile_index as u16) << 4 | tile_y as u16;
            let tile_address_hi = tile_address_lo | (1 << 3);
            let pattern_low = mapper
                .read_chr_page((tile_address_lo >> 8) as u8)
                .map_or_else(
                    || mapper.read(tile_address_lo),
                    |page| page[(tile_address_lo & 0xff) as usize],
                );
            let pattern_high = mapper
                .read_chr_page((tile_address_hi >> 8) as u8)
                .map_or_else(
                    || mapper.read(tile_address_hi),
                    |page| page[(tile_address_hi & 0xff) as usize],
                );

            processed_sprite.tile = TileData {
                nametable_index: 0,
                palette: processed_sprite.sprite.palette,
                pattern_low,
                pattern_high,
            };
        }
        self.prepare_sprite_pixels();
    }

    fn prepare_sprite_pixels(&mut self) {
        self.sprite_pixels.fill(SpritePixel::default());

        for (position, processed_sprite) in self.processed_sprites.iter().enumerate() {
            if processed_sprite.sprite.is_empty() {
                break;
            }

            let left = processed_sprite.sprite.left_x as usize;
            let end = (left + 8).min(self.sprite_pixels.len());
            for x in left..end {
                if self.sprite_pixels[x].palette == 0 {
                    let palette = processed_sprite.color((x - left) as u8);
                    if palette != 0 {
                        self.sprite_pixels[x] = SpritePixel {
                            palette,
                            position: position as u8,
                            palette_offset: processed_sprite.sprite.palette << 2,
                            behind_background: processed_sprite.sprite.behind_background,
                        };
                    }
                }
            }
        }
    }

    fn step_post_render<M: Mapper + ?Sized>(&mut self, _mapper: &M) {}

    fn step_vblank<M: Mapper + ?Sized>(&mut self, _mapper: &M) {
        if self.scanline == 241 && self.cycle_in_scanline == 1 {
            self.in_vblank = true;
            self.status_reg |= 0b1000_0000; // nmi occurred bit

            self.pending_nmi = self.control_reg & 0x80 != 0;
        }
    }

    fn step_pre_render<M: Mapper + ?Sized>(&mut self, mapper: &M) {
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

        self.update_vram_addr_if_needed();
    }

    fn fetch_background_tile<M: Mapper + ?Sized>(&mut self, mapper: &M) {
        // https://www.nesdev.org/wiki/PPU_scrolling#Tile_and_attribute_fetching
        match self.cycle_in_scanline & 7 {
            0 => self.processed_tile = [self.processed_tile[1], self.pending_tile],
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
                let pattern_table = ((self.control_reg as u16) & 0x10) << 8;
                let nametable_index = (self.pending_tile.nametable_index as u16) << 4;
                let lo_byte_offset = 0 << 3;
                let fine_y = self.v >> 12 & 0x7;
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
                let pattern_table = ((self.control_reg as u16) & 0x10) << 8;
                let nametable_index = (self.pending_tile.nametable_index as u16) << 4;
                let hi_byte_offset = 1 << 3;
                let fine_y = self.v >> 12 & 0x7;
                let pattern_high_address =
                    pattern_table | nametable_index | hi_byte_offset | fine_y;
                self.pending_tile.pattern_high = self.read_byte(mapper, pattern_high_address);
            }
            _ => unreachable!(),
        };
    }

    #[inline]
    fn update_vram_addr_if_needed(&mut self) {
        let cycle = self.cycle_in_scanline;
        let is_regular_increment = cycle != 0 && cycle & 7 == 0 && (cycle <= 256 || cycle >= 328);
        if cycle == 256
            || cycle == 257
            || is_regular_increment
            || (self.scanline == 261 && (280..=304).contains(&cycle))
        {
            self.update_vram_addr();
        }
    }

    fn update_vram_addr(&mut self) {
        match (self.scanline, self.cycle_in_scanline) {
            (_, 256) => {
                // https://www.nesdev.org/wiki/PPU_scrolling#At_dot_256_of_each_scanline
                let fine_y = self.v >> 12 & 0x7;
                if fine_y < 7 {
                    self.v += 0x1000;
                } else {
                    self.v &= !0x7000;
                    let coarse_y = self.v >> 5 & 0x1f;
                    if coarse_y < 29 {
                        self.v += 0x20;
                    } else {
                        self.v = (self.v & !0x03e0) ^ 0x0800;
                    }
                }
            }
            (_, 257) => {
                // https://www.nesdev.org/wiki/PPU_scrolling#At_dot_257_of_each_scanline
                // If rendering is enabled, the PPU copies all bits related to horizontal position from t to v:
                // v: ....A.. ...BCDEF <- t: ....A.. ...BCDEF
                self.v = (self.v & !0x041f) | (self.t & 0x041f);
            }
            (261, 280..=304) => {
                // If rendering is enabled, at the end of vblank, shortly after the horizontal bits are copied from
                // t to v at dot 257, the PPU will repeatedly copy the vertical bits from t to v from dots 280 to 304,
                // completing the full initialization of v from t:
                // v: GHIA.BC DEF..... <- t: GHIA.BC DEF.....
                self.v = (self.v & !0x7be0) | (self.t & 0x7be0);
            }
            (_, 1..=256 | 328..) if self.cycle_in_scanline & 7 == 0 => {
                // https://www.nesdev.org/wiki/PPU_scrolling#Between_dot_328_of_a_scanline,_and_256_of_the_next_scanline
                // If rendering is enabled, the PPU increments the horizontal position in v many times across the scanline,
                // it begins at dots 328 and 336, and will continue through the next scanline at 8, 16, 24... 240, 248, 256
                // (every 8 dots across the scanline until 256). Across the scanline the effective coarse X scroll coordinate
                // is incremented repeatedly, which will also wrap to the next nametable appropriately
                self.v = if self.v & 0x1f == 31 {
                    (self.v & !0x041f) ^ 0x0400
                } else {
                    self.v + 1
                };
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
            self.cycle_in_scanline = (self.rendering_enabled() && (self.frame & 1 == 1)) as u16;
        }
    }

    fn mirror_nametable(addr: u16, mode: MirroringMode) -> u16 {
        let nametable_offset = addr & 0x3ff;

        // 0x2000, 0x2400, 0x2800, 0x2C00
        let nametable_select = (addr >> 10) & 3;
        let nametable_bank = match mode {
            MirroringMode::Horizontal => nametable_select >> 1,
            MirroringMode::Vertical => nametable_select & 1,
            MirroringMode::SingleScreenLowerBank => 0,
            MirroringMode::FourScreen => nametable_select,
            MirroringMode::SingleScreenUpperBank => 1,
        };
        (nametable_bank as u16) << 10 | nametable_offset
    }

    fn build_nametable_map(mode: MirroringMode) -> [u16; 4096] {
        let mut map = [0; 4096];
        for (addr, mapped) in map.iter_mut().enumerate() {
            *mapped = Self::mirror_nametable(addr as u16, mode);
        }
        map
    }

    fn mirror_palette(offset: u8) -> u8 {
        // Expected range [0x00, 0x1F]
        // Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
        //           $10/$14/$18/$1C are mirrors of $00/$04/$08/$0C
        // Perform with no branching logic
        let is_mirrored = (offset & 0x13) == 0x10;
        offset & !((is_mirrored as u8) << 4)
    }

    pub(crate) fn read_byte<M: Mapper + ?Sized>(&self, mapper: &M, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => mapper
                .read_chr_page((addr >> 8) as u8)
                .map_or_else(|| mapper.read(addr), |page| page[(addr & 0xff) as usize]),
            0x2000..=0x3eff => {
                self.nametables[self.nametable_map[(addr & 0x0fff) as usize] as usize]
            }
            0x3f00.. => self.palette_ram[PPU::mirror_palette((addr & 0x1f) as u8) as usize],
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

    pub(crate) fn write_byte<M: Mapper + ?Sized>(&mut self, mapper: &mut M, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1fff => {
                mapper.write(addr, data);
                self.refresh_nametable_mirroring(mapper);
            }
            0x2000..=0x3eff => {
                let mapped = self.nametable_map[(addr & 0x0fff) as usize] as usize;
                self.nametables[mapped] = data;
            }
            0x3f00.. => self.palette_ram[PPU::mirror_palette((addr & 0x1f) as u8) as usize] = data,
        }
    }

    // check the interrupt line and set it low
    pub(crate) fn read_nmi_line(&mut self) -> bool {
        let status = self.pending_nmi;
        self.pending_nmi = false;

        status
    }

    pub(crate) fn nmi_pending(&self) -> bool {
        self.pending_nmi
    }

    pub(crate) fn read_register<M: Mapper + ?Sized>(&self, mapper: &M, addr: u16) -> u8 {
        // change statuses signals on the next step()
        // The eight PPU registers repeat throughout $2000-$3FFF.
        self.last_read.set(Some(0x2000 | (addr & 0x7)));

        match 0x2000 | (addr & 0x7) {
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

    pub(crate) fn write_register<M: Mapper + ?Sized>(
        &mut self,
        mapper: &mut M,
        addr: u16,
        data: u8,
    ) {
        match 0x2000 | (addr & 0x7) {
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
                    if self.control_reg & 4 != 0 {
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

#[cfg(all(test, feature = "timestamped-scheduler"))]
mod timestamped_tests {
    use super::{Screen, PPU};
    use crate::cartridge::{Mapper, MirroringMode};

    #[derive(Clone)]
    struct NoopMapper;

    impl Mapper for NoopMapper {
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
    }

    #[derive(Clone)]
    struct PatternMapper;

    impl Mapper for PatternMapper {
        fn mirror(&self) -> MirroringMode {
            MirroringMode::Horizontal
        }

        fn read(&self, address: u16) -> u8 {
            match address {
                0x0000..=0x0007 => 0xff,
                0x0008..=0x000f => 0,
                _ => 0,
            }
        }

        fn write(&mut self, _address: u16, _data: u8) {}

        fn read_page(&self, _page: u8) -> Option<&[u8; 256]> {
            None
        }
    }

    fn assert_same_state(exact: &PPU, caught_up: &PPU) {
        assert_eq!(exact.cycle_in_scanline, caught_up.cycle_in_scanline);
        assert_eq!(exact.scanline, caught_up.scanline);
        assert_eq!(exact.frame, caught_up.frame);
        assert_eq!(exact.control_reg, caught_up.control_reg);
        assert_eq!(exact.status_reg, caught_up.status_reg);
        assert_eq!(exact.mask_reg, caught_up.mask_reg);
        assert_eq!(exact.v, caught_up.v);
        assert_eq!(exact.t, caught_up.t);
        assert_eq!(exact.w, caught_up.w);
        assert_eq!(exact.in_vblank, caught_up.in_vblank);
        assert_eq!(exact.pending_nmi, caught_up.pending_nmi);
        assert_eq!(exact.last_read.get(), caught_up.last_read.get());
    }

    #[test]
    fn catch_up_preserves_vblank_nmi_edge() {
        let mut exact = PPU::default();
        exact.control_reg = 0x80;
        exact.scanline = 240;
        let mut caught_up = exact.clone();
        let mut exact_mapper = NoopMapper;
        let mut caught_up_mapper = NoopMapper;
        let mut exact_screen = Screen::default();
        let mut caught_up_screen = Screen::default();

        for _ in 0..343 {
            exact.step(&mut exact_mapper, &mut exact_screen);
        }
        assert_eq!(
            caught_up.catch_up_to(0, 343, &mut caught_up_mapper, &mut caught_up_screen),
            343
        );

        assert_same_state(&exact, &caught_up);
        assert!(caught_up.in_vblank);
        assert!(caught_up.pending_nmi);
    }

    #[test]
    fn catch_up_preserves_pre_render_clear_edge() {
        let mut exact = PPU::default();
        exact.scanline = 241;
        exact.cycle_in_scanline = 2;
        exact.in_vblank = true;
        exact.status_reg = 0b1100_0000;
        exact.pending_nmi = true;
        let mut caught_up = exact.clone();
        let mut exact_mapper = NoopMapper;
        let mut caught_up_mapper = NoopMapper;
        let mut exact_screen = Screen::default();
        let mut caught_up_screen = Screen::default();
        let ticks = 20 * 341;

        for _ in 0..ticks {
            exact.step(&mut exact_mapper, &mut exact_screen);
        }
        caught_up.catch_up_to(0, ticks, &mut caught_up_mapper, &mut caught_up_screen);

        assert_same_state(&exact, &caught_up);
        assert_eq!(caught_up.scanline, 261);
        assert_eq!(caught_up.cycle_in_scanline, 2);
        assert!(!caught_up.in_vblank);
        assert!(!caught_up.pending_nmi);
    }

    #[test]
    fn catch_up_applies_deferred_status_read_before_idle_skip() {
        let mut exact = PPU::default();
        exact.scanline = 242;
        exact.last_read.set(Some(0x2002));
        exact.status_reg = 0b1000_0000;
        let mut caught_up = exact.clone();
        let mut exact_mapper = NoopMapper;
        let mut caught_up_mapper = NoopMapper;
        let mut exact_screen = Screen::default();
        let mut caught_up_screen = Screen::default();
        let ticks = 1000;

        for _ in 0..ticks {
            exact.step(&mut exact_mapper, &mut exact_screen);
        }
        caught_up.catch_up_to(0, ticks, &mut caught_up_mapper, &mut caught_up_screen);

        assert_same_state(&exact, &caught_up);
        assert_eq!(caught_up.status_reg & 0x80, 0);
    }

    #[test]
    fn catch_up_preserves_odd_frame_rollover() {
        let mut exact = PPU::default();
        exact.mask_reg = 0x18;
        exact.scanline = 261;
        exact.cycle_in_scanline = 338;
        let mut caught_up = exact.clone();
        let mut exact_mapper = NoopMapper;
        let mut caught_up_mapper = NoopMapper;
        let mut exact_screen = Screen::default();
        let mut caught_up_screen = Screen::default();

        for _ in 0..5 {
            exact.step(&mut exact_mapper, &mut exact_screen);
        }
        caught_up.catch_up_to(0, 5, &mut caught_up_mapper, &mut caught_up_screen);

        assert_same_state(&exact, &caught_up);
        assert_eq!(caught_up.frame, 1);
        assert_eq!(caught_up.scanline, 0);
        assert_eq!(caught_up.cycle_in_scanline, 3);
    }

    #[test]
    fn catch_up_matches_rendered_pixels() {
        let mut exact = PPU::default();
        exact.mask_reg = 0x08;
        exact.palette_ram[0] = 0x09;
        exact.palette_ram[1] = 0x2a;
        let mut caught_up = exact.clone();
        let mut exact_mapper = PatternMapper;
        let mut caught_up_mapper = PatternMapper;
        let mut exact_screen = Screen::default();
        let mut caught_up_screen = Screen::default();
        let ticks = 240 * 341;

        for _ in 0..ticks {
            exact.step(&mut exact_mapper, &mut exact_screen);
        }
        caught_up.catch_up_to(0, ticks, &mut caught_up_mapper, &mut caught_up_screen);

        assert_same_state(&exact, &caught_up);
        assert_eq!(exact_screen.pixels, caught_up_screen.pixels);
        assert!(exact_screen
            .pixels
            .iter()
            .flatten()
            .any(|&pixel| pixel != 0));
    }

    #[test]
    fn next_vblank_in_ticks_handles_vblank_entry_boundary() {
        let mut ppu = PPU::default();

        ppu.scanline = 240;
        ppu.cycle_in_scanline = 340;
        assert_eq!(ppu.next_vblank_in_ticks(), 3);

        ppu.scanline = 241;
        ppu.cycle_in_scanline = 0;
        assert_eq!(ppu.next_vblank_in_ticks(), 2);

        ppu.cycle_in_scanline = 1;
        assert_eq!(ppu.next_vblank_in_ticks(), 1);
    }

    #[test]
    fn next_vblank_in_ticks_handles_pre_render_dot_one_and_rollover() {
        let mut ppu = PPU::default();
        ppu.mask_reg = 0x18;
        ppu.scanline = 261;

        ppu.cycle_in_scanline = 0;
        assert_eq!(ppu.next_vblank_in_ticks(), 82523);

        ppu.cycle_in_scanline = 1;
        assert_eq!(ppu.next_vblank_in_ticks(), 82522);

        ppu.cycle_in_scanline = 340;
        assert_eq!(ppu.next_vblank_in_ticks(), 82183);
    }

    #[test]
    fn next_vblank_in_ticks_accounts_for_odd_frame_skip() {
        let mut ppu = PPU::default();
        ppu.mask_reg = 0x18;
        ppu.scanline = 261;
        ppu.cycle_in_scanline = 340;

        ppu.frame = 0;
        assert_eq!(ppu.next_vblank_in_ticks(), 82183);

        ppu.frame = 1;
        assert_eq!(ppu.next_vblank_in_ticks(), 82184);
    }

    #[test]
    fn next_scheduler_event_exposes_vblank_exit() {
        let mut ppu = PPU::default();
        ppu.in_vblank = true;
        ppu.scanline = 241;
        ppu.cycle_in_scanline = 7;

        assert_eq!(
            ppu.next_scheduler_event_in_ticks(),
            (261 * 341 + 1) - (241 * 341 + 7) + 1
        );

        ppu.scanline = 261;
        ppu.cycle_in_scanline = 1;
        assert_eq!(ppu.next_scheduler_event_in_ticks(), 1);
    }
}
