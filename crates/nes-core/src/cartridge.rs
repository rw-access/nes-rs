use std::{cell::Cell, rc::Rc};

use dyn_clone::DynClone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirroringMode {
    Horizontal = 0,
    Vertical = 1,
    SingleScreenLowerBank = 2,
    FourScreen = 3,
    SingleScreenUpperBank = 4,
}

pub type ProgBank = [u8; 0x4000];
pub type ChrBank = [u8; 0x2000];
pub type SaveRamBank = [u8; 0x2000];

#[derive(Clone)]
pub enum CHR {
    ROM(Rc<Vec<ChrBank>>),
    RAM(Vec<ChrBank>),
}

impl CHR {
    pub fn get_banks(&self) -> &Vec<ChrBank> {
        match self {
            CHR::ROM(banks) => banks,
            CHR::RAM(banks) => banks,
        }
    }

    pub fn get_banks_mut(&mut self) -> Option<&mut Vec<ChrBank>> {
        match self {
            CHR::ROM(_) => None,
            CHR::RAM(banks) => Some(banks),
        }
    }
}

#[derive(Clone)]
pub struct PRG {
    pub(crate) banks: Vec<ProgBank>,
}

#[derive(Clone)]
pub struct Cartridge {
    /// NES 2.0 board variant; None denotes an ambiguous legacy iNES header.
    pub submapper: Option<u8>,
    pub prg: Rc<PRG>,           // 0x4000 aligned
    pub chr: CHR,               // 0x2000 aligned
    pub sram: Vec<SaveRamBank>, // 0x2000 aligned
    pub mirror: MirroringMode,
}

pub trait Mapper: DynClone {
    fn mirror(&self) -> MirroringMode;
    fn read(&self, address: u16) -> u8;
    fn write(&mut self, address: u16, data: u8);
    fn read_page(&self, page: u8) -> Option<&[u8; 256]>;

    /// The adjacent write cycles of a 6502 read-modify-write instruction.
    /// MMC1 accepts the first write and ignores the second serial bit (but
    /// never ignores a reset). The CPU currently executes instructions atomically.
    fn write_rmw(&mut self, address: u16, original: u8, modified: u8) {
        self.write(address, original);
        self.write(address, modified);
    }

    /// Optional board-specific PPU memory routing. CIRAM remains owned by the
    /// PPU, so clones/snapshots retain a single authoritative copy.
    fn read_ppu_override(&self, _address: u16, _ciram: &[u8; 4096]) -> Option<u8> {
        None
    }
    fn write_ppu_override(&mut self, _address: u16, _data: u8, _ciram: &mut [u8; 4096]) -> bool {
        false
    }
    /// Address-only transitions also occur through PPUADDR/PPUDATA, without
    /// a memory strobe. Used by discrete CHR latches such as Oeka Kids.
    fn notify_ppu_address(&self, _address: u16) {}

    /// Return a context-independent CHR page when the mapper can expose one.
    /// The PPU otherwise uses the background/sprite fetch callbacks for banked
    /// or context-dependent CHR mappings.
    fn read_chr_page(&self, _page: u8) -> Option<&[u8; 256]> {
        None
    }

    /// Pattern fetches carry rendering context for boards such as MMC5.
    /// CPU PPUDATA accesses continue to use `read` and the board's last bank set.
    fn read_background_chr(&self, address: u16) -> u8 {
        self.read(address)
    }

    fn read_sprite_chr(&self, address: u16) -> u8 {
        self.read(address)
    }

    /// Optional RGB-PPU color permutation, normalized to the shared NES palette.
    fn palette_map(&self) -> Option<&'static [u8; 64]> {
        None
    }

    /// Fully decoded PPUCTRL writes, for mapper hardware that monitors sprite size.
    fn notify_ppu_control(&mut self, _data: u8) {}

    /// End of the visible frame, before VBlank NMI delivery.
    fn end_frame(&mut self) {}

    /// Clock mapper hardware at the end of a rendered scanline.
    /// Most mappers do nothing here; scanline IRQ mappers use it as an
    /// approximation of their PPU-bus clock source.
    fn clock_scanline(&mut self) {}

    /// Clock mapper hardware once per CPU cycle.
    fn clock_cpu(&mut self) {}

    /// Whether the mapper is currently asserting its CPU IRQ line.
    fn irq_pending(&self) -> bool {
        false
    }
}

dyn_clone::clone_trait_object!(Mapper);

fn bank_index(bank: usize, count: usize) -> usize {
    bank % count
}

fn prg_16k(cartridge: &Cartridge, bank: usize, offset: usize) -> u8 {
    let bank = bank_index(bank, cartridge.prg.banks.len());
    cartridge.prg.banks[bank][offset & 0x3fff]
}

fn prg_8k(cartridge: &Cartridge, bank: usize, offset: usize) -> u8 {
    let bank = bank_index(bank, cartridge.prg.banks.len() * 2);
    prg_16k(cartridge, bank / 2, (bank & 1) * 0x2000 + (offset & 0x1fff))
}

fn prg_page_8k(cartridge: &Cartridge, bank: usize, page: u8) -> Option<&[u8; 256]> {
    let bank = bank_index(bank, cartridge.prg.banks.len() * 2);
    prg_page(
        cartridge,
        bank / 2,
        ((bank & 1) * 0x20) as u8 + (page & 0x1f),
    )
}

fn prg_page(cartridge: &Cartridge, bank: usize, page: u8) -> Option<&[u8; 256]> {
    let bank = bank_index(bank, cartridge.prg.banks.len());
    let offset = (page as usize & 0x3f) << 8;
    cartridge.prg.banks[bank][offset..offset + 0x100]
        .try_into()
        .ok()
}

fn read_sram(cartridge: &Cartridge, address: u16) -> u8 {
    if cartridge.sram.is_empty() {
        return 0;
    }
    let offset = address as usize - 0x6000;
    cartridge.sram[(offset / 0x2000) % cartridge.sram.len()][offset & 0x1fff]
}

fn write_sram(cartridge: &mut Cartridge, address: u16, data: u8) {
    if cartridge.sram.is_empty() {
        return;
    }
    let offset = address as usize - 0x6000;
    let bank = (offset / 0x2000) % cartridge.sram.len();
    cartridge.sram[bank][offset & 0x1fff] = data;
}

// `bank` selects a physical 1 KiB page; only the low ten offset bits
// address bytes inside it. Larger windows must include their subpage in bank.
fn read_chr_1k(cartridge: &Cartridge, bank: usize, offset: usize) -> u8 {
    let bank_count = cartridge.chr.get_banks().len() * 8;
    let bank = bank_index(bank, bank_count);
    cartridge.chr.get_banks()[bank / 8][(bank & 7) * 0x400 + (offset & 0x3ff)]
}

fn chr_page(cartridge: &Cartridge, page: u8) -> Option<&[u8; 256]> {
    let offset = (page as usize & 0x1f) << 8;
    cartridge
        .chr
        .get_banks()
        .first()?
        .get(offset..offset + 0x100)?
        .try_into()
        .ok()
}

fn write_chr_1k(cartridge: &mut Cartridge, bank: usize, offset: usize, data: u8) {
    let bank_count = cartridge.chr.get_banks().len() * 8;
    let bank = bank_index(bank, bank_count);
    if let Some(banks) = cartridge.chr.get_banks_mut() {
        banks[bank / 8][(bank & 7) * 0x400 + (offset & 0x3ff)] = data;
    }
}

#[derive(Clone)]
pub(crate) struct NROM {
    cartridge: Cartridge,
}

impl NROM {
    fn new(cartridge: Cartridge) -> Self {
        Self { cartridge }
    }

    fn prg_bank(&self, address: u16) -> (usize, usize) {
        let offset = address as usize - 0x8000;
        (offset / 0x4000, offset & 0x3fff)
    }
}

impl Mapper for NROM {
    #[inline]
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    #[inline]
    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, address as usize / 0x400, address as usize)
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => {
                let (bank, offset) = self.prg_bank(address);
                prg_16k(&self.cartridge, bank, offset)
            }
            _ => 0,
        }
    }

    #[inline]
    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                address as usize / 0x400,
                address as usize,
                data,
            ),
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            _ => {}
        }
    }

    #[inline]
    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(&self.cartridge, (page as usize - 0x80) / 0x40, page),
            _ => None,
        }
    }

    #[inline]
    fn read_chr_page(&self, page: u8) -> Option<&[u8; 256]> {
        (page < 0x20)
            .then(|| chr_page(&self.cartridge, page))
            .flatten()
    }
}

#[derive(Clone)]
pub(crate) enum MapperInstance {
    Nrom(NROM),
    Dynamic(Box<dyn Mapper>),
}

impl MapperInstance {
    pub(crate) fn new_nrom(cartridge: Cartridge) -> Self {
        Self::Nrom(NROM::new(cartridge))
    }
}

impl Mapper for MapperInstance {
    #[inline]
    fn mirror(&self) -> MirroringMode {
        match self {
            Self::Nrom(mapper) => mapper.mirror(),
            Self::Dynamic(mapper) => mapper.mirror(),
        }
    }

    #[inline]
    fn read(&self, address: u16) -> u8 {
        match self {
            Self::Nrom(mapper) => mapper.read(address),
            Self::Dynamic(mapper) => mapper.read(address),
        }
    }

    #[inline]
    fn write(&mut self, address: u16, data: u8) {
        match self {
            Self::Nrom(mapper) => mapper.write(address, data),
            Self::Dynamic(mapper) => mapper.write(address, data),
        }
    }

    fn write_rmw(&mut self, address: u16, original: u8, modified: u8) {
        match self {
            Self::Nrom(mapper) => mapper.write_rmw(address, original, modified),
            Self::Dynamic(mapper) => mapper.write_rmw(address, original, modified),
        }
    }

    #[inline]
    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match self {
            Self::Nrom(mapper) => mapper.read_page(page),
            Self::Dynamic(mapper) => mapper.read_page(page),
        }
    }

    fn read_ppu_override(&self, address: u16, ciram: &[u8; 4096]) -> Option<u8> {
        match self {
            Self::Nrom(mapper) => mapper.read_ppu_override(address, ciram),
            Self::Dynamic(mapper) => mapper.read_ppu_override(address, ciram),
        }
    }
    fn write_ppu_override(&mut self, address: u16, data: u8, ciram: &mut [u8; 4096]) -> bool {
        match self {
            Self::Nrom(mapper) => mapper.write_ppu_override(address, data, ciram),
            Self::Dynamic(mapper) => mapper.write_ppu_override(address, data, ciram),
        }
    }
    fn notify_ppu_address(&self, address: u16) {
        match self {
            Self::Nrom(mapper) => mapper.notify_ppu_address(address),
            Self::Dynamic(mapper) => mapper.notify_ppu_address(address),
        }
    }

    #[inline]
    fn read_chr_page(&self, page: u8) -> Option<&[u8; 256]> {
        match self {
            Self::Nrom(mapper) => mapper.read_chr_page(page),
            Self::Dynamic(mapper) => mapper.read_chr_page(page),
        }
    }

    #[inline]
    fn read_background_chr(&self, address: u16) -> u8 {
        match self {
            Self::Nrom(mapper) => mapper.read_background_chr(address),
            Self::Dynamic(mapper) => mapper.read_background_chr(address),
        }
    }

    #[inline]
    fn read_sprite_chr(&self, address: u16) -> u8 {
        match self {
            Self::Nrom(mapper) => mapper.read_sprite_chr(address),
            Self::Dynamic(mapper) => mapper.read_sprite_chr(address),
        }
    }

    fn palette_map(&self) -> Option<&'static [u8; 64]> {
        match self {
            Self::Nrom(mapper) => mapper.palette_map(),
            Self::Dynamic(mapper) => mapper.palette_map(),
        }
    }

    fn notify_ppu_control(&mut self, data: u8) {
        match self {
            Self::Nrom(mapper) => mapper.notify_ppu_control(data),
            Self::Dynamic(mapper) => mapper.notify_ppu_control(data),
        }
    }

    fn end_frame(&mut self) {
        match self {
            Self::Nrom(mapper) => mapper.end_frame(),
            Self::Dynamic(mapper) => mapper.end_frame(),
        }
    }

    #[inline]
    fn clock_scanline(&mut self) {
        match self {
            Self::Nrom(_) => {}
            Self::Dynamic(mapper) => mapper.clock_scanline(),
        }
    }

    #[inline]
    fn clock_cpu(&mut self) {
        match self {
            Self::Nrom(_) => {}
            Self::Dynamic(mapper) => mapper.clock_cpu(),
        }
    }

    #[inline]
    fn irq_pending(&self) -> bool {
        match self {
            Self::Nrom(mapper) => mapper.irq_pending(),
            Self::Dynamic(mapper) => mapper.irq_pending(),
        }
    }
}

#[derive(Clone)]
struct UxROM {
    cartridge: Cartridge,
    selected_bank: usize,
}

impl UxROM {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            selected_bank: 0,
        }
    }
}

impl Mapper for UxROM {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, address as usize / 0x400, address as usize)
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xbfff => prg_16k(
                &self.cartridge,
                self.selected_bank,
                address as usize - 0x8000,
            ),
            0xc000..=0xffff => prg_16k(
                &self.cartridge,
                self.cartridge.prg.banks.len() - 1,
                address as usize - 0xc000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                address as usize / 0x400,
                address as usize,
                data,
            ),
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => self.selected_bank = data as usize,
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xbf => prg_page(&self.cartridge, self.selected_bank, page),
            0xc0..=0xff => prg_page(&self.cartridge, self.cartridge.prg.banks.len() - 1, page),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct CNROM {
    cartridge: Cartridge,
    selected_bank: usize,
}

#[derive(Clone)]
struct Mapper15 {
    cartridge: Cartridge,
    banks: [usize; 4],
    mirror: MirroringMode,
}

impl Mapper15 {
    fn new(cartridge: Cartridge) -> Self {
        let mirror = cartridge.mirror;
        Self {
            cartridge,
            banks: [0, 1, 2, 3],
            mirror,
        }
    }
}

impl Mapper for Mapper15 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, address as usize / 0x400, address as usize)
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_8k(
                &self.cartridge,
                self.banks[(address as usize - 0x8000) / 0x2000],
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                address as usize / 0x400,
                address as usize,
                data,
            ),
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => {
                let mode = (address & 3) as usize;
                let bank = (data & 0x3f) as usize * 2;
                let swap = usize::from(data & 0x80 != 0);
                self.banks = match mode {
                    // NROM-256: CPU A14 replaces the register's low 16K bit.
                    0 => {
                        let base = bank & !2;
                        [base, base + 1, base + 2, base + 3]
                    }
                    // UNROM: fix A14..A16 high in the selected 128K region.
                    1 => [bank, bank + 1, bank | 0x0e, bank | 0x0f],
                    2 => [bank + swap; 4],
                    3 => [bank, bank + 1, bank, bank + 1],
                    _ => unreachable!(),
                };
                self.mirror = if data & 0x40 != 0 {
                    MirroringMode::Horizontal
                } else {
                    MirroringMode::Vertical
                };
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page_8k(
                &self.cartridge,
                self.banks[(page as usize - 0x80) / 0x20],
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct Mapper50 {
    cartridge: Cartridge,
    prg_bank: usize,
    irq_enabled: bool,
    irq_counter: u16,
    irq_pending: bool,
}

impl Mapper50 {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            prg_bank: 0,
            irq_enabled: false,
            irq_counter: 0,
            irq_pending: false,
        }
    }

    fn fixed_bank(&self, slot: usize) -> usize {
        match slot {
            0 => 0x0f,
            1 => 8,
            2 => 9,
            3 => self.prg_bank,
            _ => 0x0b,
        }
    }
}

impl Mapper for Mapper50 {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, address as usize / 0x400, address as usize)
            }
            0x6000..=0x7fff => prg_8k(
                &self.cartridge,
                self.fixed_bank(0),
                address as usize - 0x6000,
            ),
            0x8000..=0xffff => prg_8k(
                &self.cartridge,
                self.fixed_bank((address as usize - 0x8000) / 0x2000 + 1),
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x4020..=0x5fff if address & 0x60 == 0x20 => {
                if address & 0x100 != 0 {
                    if data & 1 != 0 {
                        self.irq_enabled = true;
                    } else {
                        self.irq_enabled = false;
                        self.irq_counter = 0;
                        self.irq_pending = false;
                    }
                } else {
                    self.prg_bank = usize::from(
                        (data & 0x08)
                            | ((data & 0x04) >> 1)
                            | ((data & 0x02) >> 1)
                            | ((data & 0x01) << 2),
                    );
                }
            }
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                address as usize / 0x400,
                address as usize,
                data,
            ),
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x60..=0xff => prg_page_8k(
                &self.cartridge,
                self.fixed_bank((page as usize - 0x60) / 0x20),
                page,
            ),
            _ => None,
        }
    }

    fn clock_cpu(&mut self) {
        if self.irq_enabled {
            self.irq_counter = self.irq_counter.wrapping_add(1);
            if self.irq_counter & 0x1000 != 0 {
                self.irq_enabled = false;
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

// RP2C04-0004 hardware color permutation, mapped to the closest RP2C03
// color index. RGB PPUs have a few additional colors; these retain the shared
// indexed output's approximate hues rather than claiming exact RGB DAC output.
// https://www.nesdev.org/wiki/PPU_palettes#RP2C04-0004
const VS_2C04_0004_TO_NES: [u8; 64] = [
    0x18, 0x03, 0x1c, 0x28, 0x0d, 0x35, 0x01, 0x17, 0x10, 0x08, 0x2a, 0x01, 0x36, 0x28, 0x0b, 0x39,
    0x25, 0x0d, 0x12, 0x34, 0x0d, 0x00, 0x06, 0x26, 0x36, 0x1b, 0x22, 0x19, 0x04, 0x0d, 0x3a, 0x21,
    0x05, 0x0a, 0x07, 0x02, 0x13, 0x14, 0x00, 0x15, 0x0c, 0x31, 0x11, 0x09, 0x09, 0x38, 0x00, 0x24,
    0x33, 0x20, 0x08, 0x16, 0x27, 0x2b, 0x20, 0x3c, 0x0d, 0x27, 0x23, 0x31, 0x29, 0x32, 0x2c, 0x09,
];

fn vs_palette_for_prg_crc(crc: u32) -> Option<&'static [u8; 64]> {
    match crc {
        0x8192_c804 => Some(&VS_2C04_0004_TO_NES),
        _ => None,
    }
}

fn crc32(bytes: impl IntoIterator<Item = u8>) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[derive(Clone)]
struct VsSystem {
    cartridge: Cartridge,
    bank: usize,
    palette_map: Option<&'static [u8; 64]>,
}

impl VsSystem {
    fn new(cartridge: Cartridge) -> Self {
        // Legacy iNES has no RGB PPU subtype. Identify the known unmodified
        // VS SMB PRG by checksum; do not apply its permutation to other VS games.
        let palette_map =
            vs_palette_for_prg_crc(crc32(cartridge.prg.banks.iter().flatten().copied()));
        Self {
            cartridge,
            bank: 0,
            palette_map,
        }
    }
}

impl Mapper for VsSystem {
    fn palette_map(&self) -> Option<&'static [u8; 64]> {
        self.palette_map
    }

    fn mirror(&self) -> MirroringMode {
        MirroringMode::FourScreen
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.bank * 8 + address as usize / 0x400,
                address as usize,
            ),
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => {
                let slot = (address as usize - 0x8000) / 0x2000;
                let bank = if slot == 0 { self.bank * 4 } else { slot };
                prg_8k(&self.cartridge, bank, address as usize - 0x8000)
            }
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x4016 => self.bank = usize::from(data & 4 != 0),
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                self.bank * 8 + address as usize / 0x400,
                address as usize,
                data,
            ),
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => {
                let slot = (page as usize - 0x80) / 0x20;
                let bank = if slot == 0 { self.bank * 4 } else { slot };
                prg_page_8k(&self.cartridge, bank, page)
            }
            _ => None,
        }
    }
}

impl CNROM {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            selected_bank: 0,
        }
    }
}

impl Mapper for CNROM {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.selected_bank * 8 + address as usize / 0x400,
                address as usize,
            ),
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                (address as usize - 0x8000) / 0x4000,
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => self.selected_bank = data as usize,
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(&self.cartridge, (page as usize - 0x80) / 0x40, page),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct MMC1 {
    cartridge: Cartridge,
    shift: u8,
    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
}

impl MMC1 {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            shift: 0x10,
            control: 0x0c,
            chr_bank0: 0,
            chr_bank1: 0,
            prg_bank: 0,
        }
    }

    fn mirror_mode(&self) -> MirroringMode {
        match self.control & 3 {
            0 => MirroringMode::SingleScreenLowerBank,
            1 => MirroringMode::SingleScreenUpperBank,
            2 => MirroringMode::Vertical,
            _ => MirroringMode::Horizontal,
        }
    }

    fn prg_bank_for(&self, address: u16) -> usize {
        // CHR A16 is repurposed as PRG A18 on SUROM/SXROM CHR-RAM boards.
        let outer =
            if matches!(self.cartridge.chr, CHR::RAM(_)) && self.cartridge.prg.banks.len() > 16 {
                self.chr_bank0 as usize & 0x10 // in 16 KiB units
            } else {
                0
            };
        let bank = self.prg_bank as usize & 0x0f;
        let last = (self.cartridge.prg.banks.len() - 1).min(15);
        outer
            | match (self.control >> 2) & 3 {
                0 | 1 => (bank & !1) + usize::from(address >= 0xc000),
                2 => {
                    if address < 0xc000 {
                        0
                    } else {
                        bank
                    }
                }
                _ => {
                    if address < 0xc000 {
                        bank
                    } else {
                        last
                    }
                }
            }
    }

    fn chr_bank_for(&self, address: u16) -> usize {
        let bank_4k = if self.control & 0x10 == 0 {
            (self.chr_bank0 as usize & !1) + address as usize / 0x1000
        } else if address < 0x1000 {
            self.chr_bank0 as usize
        } else {
            self.chr_bank1 as usize
        };
        bank_4k * 4 + (address as usize & 0x0fff) / 0x400
    }

    fn write_register(&mut self, address: u16, value: u8) {
        match address & 0x6000 {
            0x0000 => self.control = value & 0x1f,
            0x2000 => self.chr_bank0 = value & 0x1f,
            0x4000 => self.chr_bank1 = value & 0x1f,
            _ => self.prg_bank = value & 0x1f,
        }
    }
}

impl Mapper for MMC1 {
    fn mirror(&self) -> MirroringMode {
        self.mirror_mode()
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr_bank_for(address),
                address as usize,
            ),
            0x6000..=0x7fff => {
                if self.cartridge.sram.is_empty() || self.prg_bank & 0x10 != 0 {
                    0
                } else {
                    read_sram(&self.cartridge, address)
                }
            }
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                self.prg_bank_for(address),
                (address as usize - 0x8000) & 0x3fff,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr_bank_for(address);
                write_chr_1k(&mut self.cartridge, bank, address as usize, data)
            }
            0x6000..=0x7fff if self.prg_bank & 0x10 == 0 => {
                write_sram(&mut self.cartridge, address, data)
            }
            0x8000..=0xffff if data & 0x80 != 0 => {
                self.shift = 0x10;
                self.control |= 0x0c;
            }
            0x8000..=0xffff => {
                let complete = self.shift & 1 != 0;
                self.shift = (self.shift >> 1) | ((data & 1) << 4);
                if complete {
                    self.write_register(address, self.shift);
                    self.shift = 0x10;
                }
            }
            _ => {}
        }
    }

    fn write_rmw(&mut self, address: u16, original: u8, modified: u8) {
        self.write(address, original);
        if address < 0x8000 || modified & 0x80 != 0 {
            self.write(address, modified);
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(&self.cartridge, self.prg_bank_for((page as u16) << 8), page),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct AxROM {
    cartridge: Cartridge,
    selected_bank: usize,
    upper_nametable: bool,
}

impl AxROM {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            selected_bank: 0,
            upper_nametable: false,
        }
    }
}

impl Mapper for AxROM {
    fn mirror(&self) -> MirroringMode {
        if self.upper_nametable {
            MirroringMode::SingleScreenUpperBank
        } else {
            MirroringMode::SingleScreenLowerBank
        }
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, address as usize / 0x400, address as usize)
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                self.selected_bank * 2 + (address >= 0xc000) as usize,
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                address as usize / 0x400,
                address as usize,
                data,
            ),
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => {
                self.selected_bank = (data & 7) as usize;
                self.upper_nametable = data & 0x10 != 0;
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(
                &self.cartridge,
                self.selected_bank * 2 + (page >= 0xc0) as usize,
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct BNROM {
    cartridge: Cartridge,
    selected_bank: usize,
}

impl BNROM {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            selected_bank: 0,
        }
    }
}

impl Mapper for BNROM {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, address as usize / 0x400, address as usize)
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                self.selected_bank * 2 + (address >= 0xc000) as usize,
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => write_chr_1k(
                &mut self.cartridge,
                address as usize / 0x400,
                address as usize,
                data,
            ),
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => self.selected_bank = data as usize,
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(
                &self.cartridge,
                self.selected_bank * 2 + (page >= 0xc0) as usize,
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct OekaKids {
    cartridge: Cartridge,
    control: u8,
    inner: Cell<usize>,
    in_nametable: Cell<bool>,
}
impl OekaKids {
    fn chr_bank(&self, address: u16) -> usize {
        let bank = (self.control as usize & 4)
            | if address < 0x1000 {
                self.inner.get()
            } else {
                3
            };
        bank * 4 + (address as usize & 0xfff) / 0x400
    }
}
impl Mapper for OekaKids {
    fn mirror(&self) -> MirroringMode {
        MirroringMode::Vertical
    }
    fn notify_ppu_address(&self, address: u16) {
        let in_nametable = address & 0x3000 == 0x2000;
        if in_nametable && !self.in_nametable.get() {
            self.inner.set((address as usize >> 8) & 3);
        }
        self.in_nametable.set(in_nametable);
    }
    fn read(&self, address: u16) -> u8 {
        match address {
            0..=0x1fff => read_chr_1k(&self.cartridge, self.chr_bank(address), address as usize),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                (self.control as usize & 3) * 2 + usize::from(address >= 0xc000),
                address as usize,
            ),
            _ => 0,
        }
    }
    fn write(&mut self, address: u16, data: u8) {
        match address {
            0..=0x1fff => {
                let bank = self.chr_bank(address);
                write_chr_1k(&mut self.cartridge, bank, address as usize, data);
            }
            0x8000..=0xffff => self.control = data & self.read(address) & 7,
            _ => {}
        }
    }
    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(
                &self.cartridge,
                (self.control as usize & 3) * 2 + usize::from(page >= 0xc0),
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct Nina001 {
    cartridge: Cartridge,
    prg_bank: usize,
    chr: [usize; 2],
}

impl Mapper for Nina001 {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }
    fn read(&self, address: u16) -> u8 {
        match address {
            0..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr[address as usize / 0x1000] * 4 + (address as usize % 0x1000) / 0x400,
                address as usize,
            ),
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                self.prg_bank * 2 + usize::from(address >= 0xc000),
                address as usize,
            ),
            _ => 0,
        }
    }
    fn write(&mut self, address: u16, data: u8) {
        if (0x6000..=0x7fff).contains(&address) {
            write_sram(&mut self.cartridge, address, data);
            match address {
                0x7ffd => self.prg_bank = (data & 1) as usize,
                0x7ffe => self.chr[0] = (data & 0x0f) as usize,
                0x7fff => self.chr[1] = (data & 0x0f) as usize,
                _ => {}
            }
        }
    }
    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(
                &self.cartridge,
                self.prg_bank * 2 + usize::from(page >= 0xc0),
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct ColorDreams {
    cartridge: Cartridge,
    prg_bank: usize,
    chr_bank: usize,
}

impl ColorDreams {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            prg_bank: 0,
            chr_bank: 0,
        }
    }
}

impl Mapper for ColorDreams {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr_bank * 8 + address as usize / 0x400,
                address as usize,
            ),
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                self.prg_bank * 2 + (address >= 0xc000) as usize,
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => {
                self.prg_bank = (data & 3) as usize;
                self.chr_bank = (data >> 4) as usize;
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(
                &self.cartridge,
                self.prg_bank * 2 + (page >= 0xc0) as usize,
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct GxROM {
    cartridge: Cartridge,
    prg_bank: usize,
    chr_bank: usize,
}

impl GxROM {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            prg_bank: 0,
            chr_bank: 0,
        }
    }
}

impl Mapper for GxROM {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr_bank * 8 + address as usize / 0x400,
                address as usize,
            ),
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_16k(
                &self.cartridge,
                self.prg_bank * 2 + (address >= 0xc000) as usize,
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0xffff => {
                self.prg_bank = ((data >> 4) & 3) as usize;
                self.chr_bank = (data & 3) as usize;
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page(
                &self.cartridge,
                self.prg_bank * 2 + (page >= 0xc0) as usize,
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct MMC3 {
    cartridge: Cartridge,
    bank_select: u8,
    banks: [u8; 8],
    prg_mode: bool,
    chr_invert: bool,
    mirror: MirroringMode,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    ram_control: u8,
}

impl MMC3 {
    fn new(cartridge: Cartridge) -> Self {
        let mirror = cartridge.mirror;
        Self {
            cartridge,
            bank_select: 0,
            banks: [0; 8],
            prg_mode: false,
            chr_invert: false,
            mirror,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            ram_control: 0x80,
        }
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let last = self.cartridge.prg.banks.len() * 2 - 1;
        let second_last = last - 1;
        match (self.prg_mode, slot) {
            (false, 0) => self.banks[6] as usize,
            (false, 1) => self.banks[7] as usize,
            (false, 2) => second_last,
            (false, _) => last,
            (true, 0) => second_last,
            (true, 1) => self.banks[7] as usize,
            (true, 2) => self.banks[6] as usize,
            (true, _) => last,
        }
    }

    fn chr_bank(&self, slot: usize) -> usize {
        if !self.chr_invert {
            match slot {
                0 => self.banks[0] as usize & !1,
                1 => (self.banks[0] as usize & !1) + 1,
                2 => self.banks[1] as usize & !1,
                3 => (self.banks[1] as usize & !1) + 1,
                4 => self.banks[2] as usize,
                5 => self.banks[3] as usize,
                6 => self.banks[4] as usize,
                _ => self.banks[5] as usize,
            }
        } else {
            match slot {
                0 => self.banks[2] as usize,
                1 => self.banks[3] as usize,
                2 => self.banks[4] as usize,
                3 => self.banks[5] as usize,
                4 => self.banks[0] as usize & !1,
                5 => (self.banks[0] as usize & !1) + 1,
                6 => self.banks[1] as usize & !1,
                _ => (self.banks[1] as usize & !1) + 1,
            }
        }
    }
}

impl Mapper for MMC3 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                let slot = address as usize / 0x400;
                read_chr_1k(&self.cartridge, self.chr_bank(slot), address as usize)
            }
            0x6000..=0x7fff if self.ram_control & 0x80 != 0 => read_sram(&self.cartridge, address),
            0x8000..=0xffff => {
                let slot = (address as usize - 0x8000) / 0x2000;
                let bank = self.prg_bank(slot);
                let bank16 = bank / 2;
                prg_16k(
                    &self.cartridge,
                    bank16,
                    (bank & 1) * 0x2000 + ((address as usize - 0x8000) & 0x1fff),
                )
            }
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr_bank(address as usize / 0x400);
                write_chr_1k(&mut self.cartridge, bank, address as usize, data)
            }
            0x6000..=0x7fff if self.ram_control & 0xc0 == 0x80 => {
                write_sram(&mut self.cartridge, address, data)
            }
            0x8000..=0x9fff if address & 1 == 0 => {
                self.bank_select = data & 7;
                self.prg_mode = data & 0x40 != 0;
                self.chr_invert = data & 0x80 != 0;
            }
            0x8000..=0x9fff => self.banks[self.bank_select as usize] = data,
            0xa000..=0xbfff
                if address & 1 == 0 && self.cartridge.mirror != MirroringMode::FourScreen =>
            {
                self.mirror = if data & 1 == 0 {
                    MirroringMode::Vertical
                } else {
                    MirroringMode::Horizontal
                }
            }
            0xa000..=0xbfff if address & 1 != 0 => self.ram_control = data,
            0xc000..=0xdfff if address & 1 == 0 => self.irq_latch = data,
            0xc000..=0xdfff => self.irq_reload = true,
            0xe000..=0xffff if address & 1 == 0 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0xe000..=0xffff => self.irq_enabled = true,
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => {
                let slot = (page as usize - 0x80) / 0x20;
                let bank = self.prg_bank(slot);
                let bank16 = bank / 2;
                let offset_page = ((bank & 1) * 0x2000 / 0x100) + ((page as usize - 0x80) & 0x1f);
                prg_page(&self.cartridge, bank16, 0x80 + offset_page as u8)
            }
            _ => None,
        }
    }

    fn clock_scanline(&mut self) {
        if self.irq_reload || self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

// RAMBO-1 resembles MMC3, but has a third switchable PRG window and
// independent 1 KiB CHR registers. Keep those differences out of MMC3.
#[derive(Clone)]
struct RAMBO1 {
    cartridge: Cartridge,
    registers: [u8; 16],
    control: u8,
    mirror: MirroringMode,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    cpu_irq_mode: bool,
    cpu_divider: u8,
    irq_delay: u8,
    final_cpu_clock: bool,
}

impl RAMBO1 {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            mirror: cartridge.mirror,
            cartridge,
            registers: [0; 16],
            control: 0,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            cpu_irq_mode: false,
            cpu_divider: 0,
            irq_delay: 0,
            final_cpu_clock: false,
        }
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let register = if self.control & 0x40 == 0 {
            [6, 7, 15]
        } else {
            [15, 6, 7]
        };
        if slot == 3 {
            self.cartridge.prg.banks.len() * 2 - 1
        } else {
            self.registers[register[slot]] as usize
        }
    }

    fn chr_bank(&self, slot: usize) -> usize {
        let slot = slot ^ if self.control & 0x80 != 0 { 4 } else { 0 };
        match slot {
            0 => self.registers[0] as usize,
            1 if self.control & 0x20 != 0 => self.registers[8] as usize,
            1 => self.registers[0].wrapping_add(1) as usize,
            2 => self.registers[1] as usize,
            3 if self.control & 0x20 != 0 => self.registers[9] as usize,
            3 => self.registers[1].wrapping_add(1) as usize,
            _ => self.registers[slot - 2] as usize,
        }
    }

    fn clock_irq(&mut self, delay: u8) {
        if self.irq_reload {
            self.irq_counter = self
                .irq_latch
                .wrapping_add(if self.irq_latch <= 1 { 1 } else { 2 });
            self.irq_reload = false;
        } else if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch.wrapping_add(1);
        }
        self.irq_counter = self.irq_counter.wrapping_sub(1);
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_delay = delay;
        }
    }
}

impl Mapper for RAMBO1 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr_bank(address as usize / 0x400),
                address as usize,
            ),
            0x8000..=0xffff => prg_8k(
                &self.cartridge,
                self.prg_bank((address as usize - 0x8000) / 0x2000),
                address as usize,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        if address < 0x2000 {
            let bank = self.chr_bank(address as usize / 0x400);
            write_chr_1k(&mut self.cartridge, bank, address as usize, data);
            return;
        }
        match address & 0xe001 {
            0x8000 => self.control = data,
            0x8001 => self.registers[(self.control & 0x0f) as usize] = data,
            0xa000 => {
                self.mirror = if data & 1 == 0 {
                    MirroringMode::Vertical
                } else {
                    MirroringMode::Horizontal
                }
            }
            0xc000 => self.irq_latch = data,
            0xc001 => {
                self.final_cpu_clock |= self.cpu_irq_mode && data & 1 == 0;
                self.cpu_irq_mode = data & 1 != 0;
                if self.cpu_irq_mode {
                    self.cpu_divider = 0;
                }
                self.irq_reload = true;
            }
            0xe000 => {
                self.irq_enabled = false;
                self.irq_pending = false;
                self.irq_delay = 0;
            }
            0xe001 => self.irq_enabled = true,
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        if page < 0x80 {
            return None;
        }
        prg_page_8k(
            &self.cartridge,
            self.prg_bank((page as usize - 0x80) / 0x20),
            page,
        )
    }

    fn clock_cpu(&mut self) {
        if self.irq_delay != 0 {
            self.irq_delay -= 1;
            if self.irq_delay == 0 && self.irq_enabled {
                self.irq_pending = true;
            }
        }
        if self.cpu_irq_mode || self.final_cpu_clock {
            self.cpu_divider = (self.cpu_divider + 1) & 3;
            if self.cpu_divider == 0 {
                self.clock_irq(1);
                self.final_cpu_clock = false;
            }
        }
    }

    fn clock_scanline(&mut self) {
        // The PPU currently provides scanline clocks rather than filtered A12
        // edges, the same approximation used for MMC3.
        if !self.cpu_irq_mode {
            self.clock_irq(2);
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

#[derive(Clone)]
struct MMC2 {
    cartridge: Cartridge,
    prg_bank: usize,
    left_fd: usize,
    left_fe: usize,
    right_fd: usize,
    right_fe: usize,
    left_latch: Cell<bool>,
    right_latch: Cell<bool>,
    mirror: MirroringMode,
    mmc4: bool,
}

impl MMC2 {
    fn new(cartridge: Cartridge, mmc4: bool) -> Self {
        let mirror = cartridge.mirror;
        Self {
            cartridge,
            prg_bank: 0,
            left_fd: 0,
            left_fe: 0,
            right_fd: 0,
            right_fe: 0,
            left_latch: Cell::new(false),
            right_latch: Cell::new(false),
            mirror,
            mmc4,
        }
    }

    fn latch_chr(&self, address: u16) {
        match address {
            0x0fd8..=0x0fdf if self.mmc4 || address == 0x0fd8 => self.left_latch.set(false),
            0x0fe8..=0x0fef if self.mmc4 || address == 0x0fe8 => self.left_latch.set(true),
            0x1fd8..=0x1fdf => self.right_latch.set(false),
            0x1fe8..=0x1fef => self.right_latch.set(true),
            _ => {}
        }
    }

    fn chr_bank(&self, address: u16) -> usize {
        if address < 0x1000 {
            if self.left_latch.get() {
                self.left_fe
            } else {
                self.left_fd
            }
        } else if self.right_latch.get() {
            self.right_fe
        } else {
            self.right_fd
        }
    }
}

impl Mapper for MMC2 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                let value = read_chr_1k(
                    &self.cartridge,
                    self.chr_bank(address) * 4 + (address as usize & 0x0fff) / 0x400,
                    address as usize,
                );
                self.latch_chr(address);
                value
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xbfff if self.mmc4 => {
                prg_16k(&self.cartridge, self.prg_bank, address as usize - 0x8000)
            }
            0x8000..=0x9fff => prg_8k(&self.cartridge, self.prg_bank, address as usize - 0x8000),
            0xc000..=0xffff if self.mmc4 => prg_16k(
                &self.cartridge,
                self.cartridge.prg.banks.len() - 1,
                address as usize - 0xc000,
            ),
            0xa000..=0xffff => prg_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 3 + ((address as usize - 0xa000) / 0x2000),
                address as usize - 0xa000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr_bank(address);
                write_chr_1k(
                    &mut self.cartridge,
                    bank * 4 + (address as usize & 0x0fff) / 0x400,
                    address as usize,
                    data,
                )
            }
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0xa000..=0xafff => self.prg_bank = (data & 0x0f) as usize,
            0xb000..=0xbfff => self.left_fd = (data & 0x1f) as usize,
            0xc000..=0xcfff => self.left_fe = (data & 0x1f) as usize,
            0xd000..=0xdfff => self.right_fd = (data & 0x1f) as usize,
            0xe000..=0xefff => self.right_fe = (data & 0x1f) as usize,
            0xf000..=0xffff => {
                self.mirror = match data & 1 {
                    0 => MirroringMode::Vertical,
                    _ => MirroringMode::Horizontal,
                }
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xbf if self.mmc4 => prg_page(&self.cartridge, self.prg_bank, page),
            0x80..=0x9f => prg_page_8k(&self.cartridge, self.prg_bank, page),
            0xc0..=0xff if self.mmc4 => {
                prg_page(&self.cartridge, self.cartridge.prg.banks.len() - 1, page)
            }
            0xa0..=0xff => prg_page_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 3 + (page as usize - 0xa0) / 0x20,
                page,
            ),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct VRC {
    cartridge: Cartridge,
    prg: [usize; 2],
    chr: [usize; 8],
    mirror: MirroringMode,
    swap: bool,
    vrc2: bool,
    chr_shift: bool,
    vrc6: bool,
    address_mode: u8,
    address_pins: Option<(u8, u8)>,
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_mode_cpu: bool,
    irq_pending: bool,
    irq_after_ack: bool,
    irq_prescaler: i16,
}

impl VRC {
    fn new(cartridge: Cartridge, mapper: u8) -> Self {
        Self {
            mirror: cartridge.mirror,
            address_pins: match (mapper, cartridge.submapper) {
                (21, Some(1)) => Some((1, 2)),
                (21, Some(2)) => Some((6, 7)),
                (23, Some(1 | 3)) => Some((0, 1)),
                (23, Some(2)) => Some((2, 3)),
                (25, Some(1 | 3)) => Some((1, 0)),
                (25, Some(2)) => Some((3, 2)),
                _ => None,
            },
            vrc2: mapper == 22 || (matches!(mapper, 23 | 25) && cartridge.submapper == Some(3)),
            cartridge,
            prg: [0, 0],
            chr: [0; 8],
            swap: false,
            chr_shift: mapper == 22,
            vrc6: mapper == 24 || mapper == 26,
            address_mode: match mapper {
                21 => 0,
                22 => 1,
                23 => 2,
                24 => 3,
                25 => 4,
                26 => 1,
                _ => 3,
            },
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_mode_cpu: false,
            irq_pending: false,
            irq_after_ack: false,
            irq_prescaler: 341,
        }
    }

    fn port(&self, address: u16) -> u8 {
        if let Some((a0, a1)) = self.address_pins {
            return (((address >> a0) & 1) | (((address >> a1) & 1) << 1)) as u8;
        }
        let low = (address & 0x0f) as u8;
        match self.address_mode {
            0 => (((address >> 1) | (address >> 6)) & 3) as u8,
            4 => ((low >> 1 | low >> 3) & 1) | ((low << 1 | low >> 1) & 2),
            1 => ((low & 1) << 1) | ((low >> 1) & 1),
            2 => {
                if low & 0x0c != 0 {
                    (low >> 2) & 3
                } else {
                    low & 3
                }
            }
            _ => low & 3,
        }
    }

    fn irq_control(&mut self, data: u8) {
        self.irq_pending = false;
        self.irq_after_ack = data & 1 != 0;
        self.irq_enabled = data & 2 != 0;
        self.irq_mode_cpu = data & 4 != 0;
        self.irq_prescaler = 341;
        if self.irq_enabled {
            self.irq_counter = self.irq_latch;
        }
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
        self.irq_enabled = self.irq_after_ack;
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xff {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter += 1;
        }
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let last = self.cartridge.prg.banks.len() * 2 - 1;
        let second_last = last - 1;
        match slot {
            0 if self.swap && !self.vrc2 => second_last,
            0 => self.prg[0],
            1 => self.prg[1],
            2 if self.swap && !self.vrc2 => self.prg[0],
            2 => second_last,
            _ => last,
        }
    }
}

impl Mapper for VRC {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr[address as usize / 0x400] >> usize::from(self.chr_shift),
                address as usize,
            ),
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xbfff if self.vrc6 => {
                prg_16k(&self.cartridge, self.prg[0], address as usize - 0x8000)
            }
            0xc000..=0xdfff if self.vrc6 => {
                prg_8k(&self.cartridge, self.prg[1], address as usize - 0xc000)
            }
            0xe000..=0xffff if self.vrc6 => prg_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 1,
                address as usize - 0xe000,
            ),
            0x8000..=0xffff => prg_8k(
                &self.cartridge,
                self.prg_bank((address as usize - 0x8000) / 0x2000),
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr[address as usize / 0x400] >> usize::from(self.chr_shift);
                write_chr_1k(&mut self.cartridge, bank, address as usize, data)
            }
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0x8fff => self.prg[0] = data as usize,
            0x9000..=0x9fff if self.vrc6 => {}
            0x9000..=0x9fff => match self.port(address) {
                _ if self.vrc2 => {
                    self.mirror = if data & 1 == 0 {
                        MirroringMode::Vertical
                    } else {
                        MirroringMode::Horizontal
                    };
                }
                0 => {
                    self.mirror = match data & 3 {
                        0 => MirroringMode::Vertical,
                        1 => MirroringMode::Horizontal,
                        2 => MirroringMode::SingleScreenLowerBank,
                        _ => MirroringMode::SingleScreenUpperBank,
                    }
                }
                2 if !self.vrc2 => {
                    self.swap = data & 2 != 0;
                }
                _ => {}
            },
            0xa000..=0xafff if !self.vrc6 => self.prg[1] = data as usize,
            0xb000..=0xbfff if self.vrc6 && self.port(address) == 3 => {
                self.mirror = match (data >> 2) & 3 {
                    0 => MirroringMode::Vertical,
                    1 => MirroringMode::Horizontal,
                    2 => MirroringMode::SingleScreenLowerBank,
                    _ => MirroringMode::SingleScreenUpperBank,
                }
            }
            0xc000..=0xcfff if self.vrc6 => self.prg[1] = data as usize,
            0xd000..=0xefff if self.vrc6 => {
                let index =
                    ((address as usize - 0xd000) / 0x1000) * 4 + self.port(address) as usize;
                if index < 8 {
                    self.chr[index] = data as usize;
                }
            }
            0xb000..=0xefff if !self.vrc6 => {
                let block = ((address as usize - 0xb000) / 0x1000) * 2;
                let port = self.port(address) as usize;
                let index = block + port / 2;
                if index < 8 {
                    if port & 1 == 0 {
                        self.chr[index] = (self.chr[index] & 0x1f0) | (data as usize & 0x0f);
                    } else {
                        self.chr[index] = (self.chr[index] & 0x0f)
                            | ((data as usize & if self.vrc2 { 0x0f } else { 0x1f }) << 4);
                    }
                }
            }
            0xf000..=0xffff if !self.vrc2 => {
                let port = self.port(address);
                if self.vrc6 {
                    match port {
                        0 => self.irq_latch = data,
                        1 => self.irq_control(data),
                        2 => self.irq_acknowledge(),
                        _ => {}
                    }
                } else {
                    match port {
                        0 => self.irq_latch = (self.irq_latch & 0xf0) | (data & 0x0f),
                        1 => self.irq_latch = (self.irq_latch & 0x0f) | ((data & 0x0f) << 4),
                        2 => self.irq_control(data),
                        _ => self.irq_acknowledge(),
                    }
                }
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xbf if self.vrc6 => prg_page(&self.cartridge, self.prg[0], page),
            0xc0..=0xdf if self.vrc6 => prg_page_8k(&self.cartridge, self.prg[1], page),
            0xe0..=0xff if self.vrc6 => prg_page_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 1,
                page,
            ),
            0x80..=0xff => prg_page_8k(
                &self.cartridge,
                self.prg_bank((page as usize - 0x80) / 0x20),
                page,
            ),
            _ => None,
        }
    }

    fn clock_cpu(&mut self) {
        if !self.irq_enabled || self.vrc2 {
            return;
        }
        if self.irq_mode_cpu {
            self.clock_irq_counter();
        } else {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

#[derive(Clone)]
struct FME7 {
    cartridge: Cartridge,
    command: u8,
    chr: [usize; 8],
    prg: [usize; 4],
    mirror: MirroringMode,
    irq_counter: u16,
    irq_counting: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl FME7 {
    fn new(cartridge: Cartridge) -> Self {
        let mirror = cartridge.mirror;
        Self {
            cartridge,
            command: 0,
            chr: [0; 8],
            prg: [0; 4],
            mirror,
            irq_counter: 0,
            irq_counting: false,
            irq_enabled: false,
            irq_pending: false,
        }
    }
}

impl Mapper for FME7 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => read_chr_1k(
                &self.cartridge,
                self.chr[address as usize / 0x400],
                address as usize,
            ),
            0x6000..=0x7fff if self.prg[0] & 0x40 == 0 => prg_8k(
                &self.cartridge,
                self.prg[0] & 0x3f,
                address as usize - 0x6000,
            ),
            0x6000..=0x7fff if self.prg[0] & 0x80 != 0 => read_sram(&self.cartridge, address),
            0x8000..=0xdfff => prg_8k(
                &self.cartridge,
                self.prg[(address as usize - 0x8000) / 0x2000 + 1],
                address as usize - 0x8000,
            ),
            0xe000..=0xffff => prg_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 1,
                address as usize - 0xe000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr[address as usize / 0x400];
                write_chr_1k(&mut self.cartridge, bank, address as usize, data);
            }
            0x6000..=0x7fff if self.prg[0] & 0xc0 == 0xc0 => {
                write_sram(&mut self.cartridge, address, data)
            }
            0x8000..=0x9fff => self.command = data & 0x0f,
            0xa000..=0xbfff => match self.command {
                0..=7 => self.chr[self.command as usize] = data as usize,
                8..=11 => self.prg[(self.command - 8) as usize] = data as usize,
                12 => {
                    self.mirror = match data & 3 {
                        0 => MirroringMode::Vertical,
                        1 => MirroringMode::Horizontal,
                        2 => MirroringMode::SingleScreenLowerBank,
                        _ => MirroringMode::SingleScreenUpperBank,
                    }
                }
                13 => {
                    self.irq_counting = data & 0x80 != 0;
                    self.irq_enabled = data & 1 != 0;
                    self.irq_pending = false;
                }
                14 => self.irq_counter = (self.irq_counter & 0xff00) | data as u16,
                15 => self.irq_counter = (self.irq_counter & 0x00ff) | ((data as u16) << 8),
                _ => {}
            },
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x60..=0x7f if self.prg[0] & 0x40 == 0 => {
                prg_page_8k(&self.cartridge, self.prg[0] & 0x3f, page)
            }
            0x80..=0xdf => prg_page_8k(
                &self.cartridge,
                self.prg[(page as usize - 0x80) / 0x20 + 1],
                page,
            ),
            0xe0..=0xff => prg_page_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 1,
                page,
            ),
            _ => None,
        }
    }

    fn clock_cpu(&mut self) {
        if self.irq_counting {
            if self.irq_counter == 0 {
                self.irq_counter = 0xffff;
            } else {
                self.irq_counter -= 1;
            }
            if self.irq_counter == 0xffff && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

#[derive(Clone)]
struct MMC5 {
    cartridge: Cartridge,
    prg_mode: u8,
    nametable_select: u8,
    exram: [u8; 1024],
    exram_mode: u8,
    fill_tile: u8,
    fill_attribute: u8,
    chr_mode: u8,
    prg: [u8; 5],
    chr: [u16; 12],
    chr_upper: u8,
    chr_background_selected: bool,
    mirror: MirroringMode,
    ram_protect1: u8,
    ram_protect2: u8,
    irq_line: u8,
    irq_enabled: bool,
    irq_pending: Cell<bool>,
    in_frame: bool,
    tall_sprites: bool,
    scanline: u8,
    multiplier_a: u8,
    multiplier_b: u8,
}

impl MMC5 {
    fn new(cartridge: Cartridge) -> Self {
        let mirror = cartridge.mirror;
        Self {
            cartridge,
            prg_mode: 3,
            nametable_select: match mirror {
                MirroringMode::Vertical => 0x44,
                MirroringMode::Horizontal => 0x50,
                MirroringMode::SingleScreenUpperBank => 0x55,
                _ => 0,
            },
            exram: [0; 1024],
            exram_mode: 3,
            fill_tile: 0,
            fill_attribute: 0,
            chr_mode: 0,
            prg: [0, 0, 0, 0, 0xff],
            chr: [0; 12],
            chr_upper: 0,
            chr_background_selected: false,
            mirror,
            ram_protect1: 0,
            ram_protect2: 0,
            irq_line: 0,
            irq_enabled: false,
            irq_pending: Cell::new(false),
            in_frame: false,
            tall_sprites: false,
            scanline: 0,
            multiplier_a: 0,
            multiplier_b: 0,
        }
    }

    fn prg_bank(&self, address: u16) -> usize {
        let slot = (address as usize - 0x8000) / 0x2000;
        match self.prg_mode & 3 {
            0 => (self.prg[4] as usize & !3) + slot,
            1 => {
                if slot < 2 {
                    (self.prg[2] as usize & !1) + slot
                } else {
                    (self.prg[4] as usize & !1) + slot - 2
                }
            }
            2 => {
                if slot < 2 {
                    (self.prg[2] as usize & !1) + slot
                } else if slot == 2 {
                    self.prg[3] as usize
                } else {
                    self.prg[4] as usize
                }
            }
            _ => self.prg[slot + 1] as usize,
        }
    }

    fn prg_is_rom(&self, address: u16) -> bool {
        let slot = (address as usize - 0x8000) / 0x2000;
        let reg = match self.prg_mode {
            0 => 4,
            1 => {
                if slot < 2 {
                    2
                } else {
                    4
                }
            }
            2 => {
                if slot < 2 {
                    2
                } else {
                    slot + 1
                }
            }
            _ => slot + 1,
        };
        reg == 4 || self.prg[reg] & 0x80 != 0
    }

    fn read_ram(&self, bank: usize, address: u16) -> u8 {
        if self.cartridge.sram.is_empty() {
            return 0;
        }
        self.cartridge.sram[bank % self.cartridge.sram.len()][address as usize & 0x1fff]
    }
    fn write_ram(&mut self, bank: usize, address: u16, data: u8) {
        if !self.cartridge.sram.is_empty() && self.ram_protect1 == 2 && self.ram_protect2 == 1 {
            let bank = bank % self.cartridge.sram.len();
            self.cartridge.sram[bank][address as usize & 0x1fff] = data;
        }
    }

    fn chr_bank(&self, address: u16) -> usize {
        self.chr_bank_for_set(address, self.tall_sprites && self.chr_background_selected)
    }

    fn chr_bank_for_set(&self, address: u16, background: bool) -> usize {
        let slot = address as usize / 0x400;
        if background {
            match self.chr_mode & 3 {
                0 => self.chr[11] as usize * 8 + slot,
                1 => self.chr[11] as usize * 4 + slot % 4,
                2 => self.chr[9 + (slot / 2 % 2) * 2] as usize * 2 + slot % 2,
                _ => self.chr[8 + slot % 4] as usize,
            }
        } else {
            match self.chr_mode & 3 {
                0 => self.chr[7] as usize * 8 + slot,
                1 => self.chr[if slot < 4 { 3 } else { 7 }] as usize * 4 + slot % 4,
                2 => self.chr[slot / 2 * 2 + 1] as usize * 2 + slot % 2,
                _ => self.chr[slot] as usize,
            }
        }
    }
}

impl Mapper for MMC5 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read_ppu_override(&self, address: u16, ciram: &[u8; 4096]) -> Option<u8> {
        if !(0x2000..=0x3eff).contains(&address) {
            return None;
        }
        let offset = address as usize & 0x3ff;
        Some(
            match (self.nametable_select >> (((address >> 10) & 3) * 2)) & 3 {
                page @ 0..=1 => ciram[page as usize * 0x400 + offset],
                2 => {
                    if self.exram_mode < 2 {
                        self.exram[offset]
                    } else {
                        0
                    }
                }
                _ => {
                    if offset < 0x3c0 {
                        self.fill_tile
                    } else {
                        self.fill_attribute
                    }
                }
            },
        )
    }
    fn write_ppu_override(&mut self, address: u16, data: u8, ciram: &mut [u8; 4096]) -> bool {
        if !(0x2000..=0x3eff).contains(&address) {
            return false;
        }
        let offset = address as usize & 0x3ff;
        match (self.nametable_select >> (((address >> 10) & 3) * 2)) & 3 {
            page @ 0..=1 => ciram[page as usize * 0x400 + offset] = data,
            2 if self.exram_mode < 2 => self.exram[offset] = data,
            _ => {}
        }
        true
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, self.chr_bank(address), address as usize)
            }
            0x5205 => (self.multiplier_a as u16 * self.multiplier_b as u16) as u8,
            0x5206 => ((self.multiplier_a as u16 * self.multiplier_b as u16) >> 8) as u8,
            0x5204 => ((self.irq_pending.replace(false) as u8) << 7) | ((self.in_frame as u8) << 6),
            0x5c00..=0x5fff if self.exram_mode >= 2 => self.exram[address as usize & 0x3ff],
            0x6000..=0x7fff => self.read_ram((self.prg[0] & 0x0f) as usize, address),
            0x8000..=0xffff if !self.prg_is_rom(address) => {
                self.read_ram(self.prg_bank(address) & 0x0f, address)
            }
            0x8000..=0xffff => prg_8k(
                &self.cartridge,
                self.prg_bank(address) & 0x7f,
                address as usize - 0x8000,
            ),
            _ => 0,
        }
    }

    fn read_background_chr(&self, address: u16) -> u8 {
        let background = self.tall_sprites;
        read_chr_1k(
            &self.cartridge,
            self.chr_bank_for_set(address, background),
            address as usize,
        )
    }

    fn read_sprite_chr(&self, address: u16) -> u8 {
        let background = false;
        read_chr_1k(
            &self.cartridge,
            self.chr_bank_for_set(address, background),
            address as usize,
        )
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr_bank(address);
                write_chr_1k(&mut self.cartridge, bank, address as usize, data);
            }
            0x5100 => self.prg_mode = data & 3,
            0x5101 => self.chr_mode = data & 3,
            0x5102 => self.ram_protect1 = data & 3,
            0x5103 => self.ram_protect2 = data & 3,
            0x5104 => self.exram_mode = data & 3,
            0x5105 => self.nametable_select = data,
            0x5106 => self.fill_tile = data,
            0x5107 => self.fill_attribute = (data & 3) * 0x55,
            0x5113..=0x5117 => self.prg[(address - 0x5113) as usize] = data,
            0x5120..=0x512b => {
                self.chr[(address - 0x5120) as usize] =
                    u16::from(data) | (u16::from(self.chr_upper) << 8);
                self.chr_background_selected = address >= 0x5128;
            }
            0x5130 => self.chr_upper = data & 3,
            0x5203 => self.irq_line = data,
            0x5204 => {
                self.irq_enabled = data & 0x80 != 0;
            }
            0x5205 => self.multiplier_a = data,
            0x5206 => self.multiplier_b = data,
            0x5c00..=0x5fff => match self.exram_mode {
                0 | 1 => {
                    self.exram[address as usize & 0x3ff] = if self.in_frame { data } else { 0 }
                }
                2 => self.exram[address as usize & 0x3ff] = data,
                _ => {}
            },
            0x6000..=0x7fff => self.write_ram((self.prg[0] & 0x0f) as usize, address, data),
            0x8000..=0xdfff if !self.prg_is_rom(address) => {
                self.write_ram(self.prg_bank(address) & 0x0f, address, data)
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff if !self.prg_is_rom(u16::from(page) << 8) => {
                if self.cartridge.sram.is_empty() {
                    return None;
                }
                let bank = (self.prg_bank(u16::from(page) << 8) & 0x0f) % self.cartridge.sram.len();
                let offset = (page as usize & 0x1f) << 8;
                self.cartridge.sram[bank][offset..offset + 256]
                    .try_into()
                    .ok()
            }
            0x80..=0xff => prg_page_8k(
                &self.cartridge,
                self.prg_bank((page as u16) << 8) & 0x7f,
                page,
            ),
            _ => None,
        }
    }

    fn notify_ppu_control(&mut self, data: u8) {
        self.tall_sprites = data & 0x20 != 0;
    }

    fn end_frame(&mut self) {
        self.in_frame = false;
        self.scanline = 0;
    }

    fn clock_scanline(&mut self) {
        if self.in_frame {
            self.scanline = self.scanline.wrapping_add(1);
        } else {
            self.in_frame = true;
            self.scanline = 0;
        }
        if self.scanline != 0 && self.scanline == self.irq_line {
            self.irq_pending.set(true);
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_enabled && self.irq_pending.get()
    }
}

#[derive(Clone)]
struct Namco163 {
    cartridge: Cartridge,
    chr: [usize; 12],
    prg: [usize; 3],
    internal_ram: [u8; 128],
    ram_address: Cell<u8>,
    ram_auto_increment: bool,
    chr_ram_enable: u8,
    ram_protect: u8,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Namco163 {
    fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            chr: [0; 12],
            prg: [0; 3],
            internal_ram: [0; 128],
            ram_address: Cell::new(0),
            ram_auto_increment: false,
            chr_ram_enable: 0,
            ram_protect: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn chr_bank(&self, address: u16) -> usize {
        self.chr[address as usize / 0x400]
    }

    fn ppu_bank(&self, address: u16) -> Option<(usize, bool)> {
        match address {
            0..=0x1fff => {
                let enabled = self.chr_ram_enable & (0x40 << (address / 0x1000)) == 0;
                Some((self.chr_bank(address), enabled))
            }
            0x2000..=0x3eff => Some((self.chr[8 + (address as usize & 0xfff) / 0x400], true)),
            _ => None,
        }
    }

    fn ram_window_writable(&self, address: u16) -> bool {
        let bit = ((address as usize - 0x6000) / 0x800) as u8;
        (self.ram_protect & (1 << bit)) == 0 && (self.ram_protect & 0xf0) == 0x40
    }

    fn ram_data_read(&self) -> u8 {
        let value = self.internal_ram[self.ram_address.get() as usize];
        self.advance_ram_address();
        value
    }

    fn advance_ram_address(&self) {
        if self.ram_auto_increment {
            self.ram_address
                .set(self.ram_address.get().saturating_add(1).min(0x7f));
        }
    }

    fn ram_data_write(&mut self, data: u8) {
        self.internal_ram[(self.ram_address.get() & 0x7f) as usize] = data;
        self.advance_ram_address();
    }
}

impl Mapper for Namco163 {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read_ppu_override(&self, address: u16, ciram: &[u8; 4096]) -> Option<u8> {
        let (bank, ciram_enabled) = self.ppu_bank(address)?;
        if ciram_enabled && bank >= 0xe0 {
            Some(ciram[(bank & 1) * 0x400 + (address as usize & 0x3ff)])
        } else if address >= 0x2000 {
            Some(read_chr_1k(&self.cartridge, bank, address as usize))
        } else {
            None
        }
    }
    fn write_ppu_override(&mut self, address: u16, data: u8, ciram: &mut [u8; 4096]) -> bool {
        let Some((bank, enabled)) = self.ppu_bank(address) else {
            return false;
        };
        if enabled && bank >= 0xe0 {
            ciram[(bank & 1) * 0x400 + (address as usize & 0x3ff)] = data;
            true
        } else if address >= 0x2000 {
            write_chr_1k(&mut self.cartridge, bank, address as usize, data);
            true
        } else {
            false
        }
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, self.chr_bank(address), address as usize)
            }
            0x4800..=0x4fff => self.ram_data_read(),
            0x5000..=0x57ff => self.irq_counter as u8,
            0x5800..=0x5fff => (self.irq_counter >> 8) as u8 | (self.irq_enabled as u8) << 7,
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0x9fff => prg_8k(&self.cartridge, self.prg[0], address as usize - 0x8000),
            0xa000..=0xbfff => prg_8k(&self.cartridge, self.prg[1], address as usize - 0xa000),
            0xc000..=0xdfff => prg_8k(&self.cartridge, self.prg[2], address as usize - 0xc000),
            0xe000..=0xffff => prg_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 1,
                address as usize - 0xe000,
            ),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                let bank = self.chr_bank(address);
                write_chr_1k(&mut self.cartridge, bank, address as usize, data);
            }
            0x4800..=0x4fff => self.ram_data_write(data),
            0x5000..=0x57ff => {
                self.irq_counter = (self.irq_counter & 0x7f00) | data as u16;
                self.irq_pending = false;
            }
            0x5800..=0x5fff => {
                self.irq_counter = (self.irq_counter & 0x00ff) | ((data as u16 & 0x7f) << 8);
                self.irq_enabled = data & 0x80 != 0;
                self.irq_pending = false;
            }
            0x6000..=0x7fff if self.ram_window_writable(address) => {
                write_sram(&mut self.cartridge, address, data)
            }
            0x8000..=0xdfff => {
                let index = (address as usize - 0x8000) / 0x800;
                if index < 12 {
                    self.chr[index] = data as usize;
                }
            }
            0xe000..=0xe7ff => self.prg[0] = data as usize & 0x3f,
            0xe800..=0xefff => {
                self.prg[1] = data as usize & 0x3f;
                self.chr_ram_enable = data & 0xc0;
            }
            0xf000..=0xf7ff => self.prg[2] = data as usize & 0x3f,
            0xf800..=0xffff => {
                self.ram_address.set(data & 0x7f);
                self.ram_auto_increment = data & 0x80 != 0;
                self.ram_protect = data;
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0x9f => prg_page_8k(&self.cartridge, self.prg[0], page),
            0xa0..=0xbf => prg_page_8k(&self.cartridge, self.prg[1], page),
            0xc0..=0xdf => prg_page_8k(&self.cartridge, self.prg[2], page),
            0xe0..=0xff => prg_page_8k(
                &self.cartridge,
                self.cartridge.prg.banks.len() * 2 - 1,
                page,
            ),
            _ => None,
        }
    }

    fn clock_cpu(&mut self) {
        if self.irq_enabled && self.irq_counter < 0x7fff {
            self.irq_counter += 1;
            if self.irq_counter == 0x7fff {
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

pub fn new(cartridge: Cartridge, mapper: u8) -> Option<Box<dyn Mapper>> {
    if cartridge.prg.banks.is_empty() || cartridge.chr.get_banks().is_empty() {
        return None;
    }

    // A number of bad iNES dumps in the wild carry a mapper number in the
    // header even though their 32 KiB payload is a plain NROM image.  Keep
    // those images usable while still selecting the real mapper for images
    // large enough to need banking.
    if matches!(mapper, 64 | 96 | 224)
        && cartridge.prg.banks.len() <= 2
        && cartridge.chr.get_banks().len() <= 1
    {
        return Some(Box::new(NROM::new(cartridge)));
    }

    Some(match mapper {
        0 => Box::new(NROM::new(cartridge)) as Box<dyn Mapper>,
        1 => Box::new(MMC1::new(cartridge)),
        2 => Box::new(UxROM::new(cartridge)),
        3 => Box::new(CNROM::new(cartridge)),
        4 => Box::new(MMC3::new(cartridge)),
        5 => Box::new(MMC5::new(cartridge)),
        7 => Box::new(AxROM::new(cartridge)),
        9 => Box::new(MMC2::new(cartridge, false)),
        10 => Box::new(MMC2::new(cartridge, true)),
        11 => Box::new(ColorDreams::new(cartridge)),
        15 => Box::new(Mapper15::new(cartridge)),
        // Mapper 20 is an FDS image convention in iNES. The current core
        // has no disk-side I/O, but treating its loaded PRG payload as a
        // cartridge keeps the image loadable for tools that only inspect or
        // disassemble it.
        20 => Box::new(NROM::new(cartridge)),
        50 => Box::new(Mapper50::new(cartridge)),
        21 | 22 | 23 | 25 => Box::new(VRC::new(cartridge, mapper)),
        24 | 26 => Box::new(VRC::new(cartridge, mapper)),
        64 => Box::new(RAMBO1::new(cartridge)),
        34 if cartridge.submapper == Some(1)
            || (cartridge.submapper.unwrap_or(0) == 0
                && matches!(cartridge.chr, CHR::ROM(_))
                && cartridge.chr.get_banks().len() > 1) =>
        {
            Box::new(Nina001 {
                cartridge,
                prg_bank: 0,
                chr: [0; 2],
            })
        }
        34 => Box::new(BNROM::new(cartridge)),
        66 => Box::new(GxROM::new(cartridge)),
        69 => Box::new(FME7::new(cartridge)),
        19 => Box::new(Namco163::new(cartridge)),
        96 => Box::new(OekaKids {
            cartridge,
            control: 0,
            inner: Cell::new(0),
            in_nametable: Cell::new(false),
        }),
        99 => Box::new(VsSystem::new(cartridge)),
        224 => Box::new(MMC3::new(cartridge)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cartridge(prg_banks: usize, chr_banks: usize) -> Cartridge {
        let mut prg = Vec::with_capacity(prg_banks);
        for bank in 0..prg_banks {
            prg.push([bank as u8; 0x4000]);
        }

        let mut chr = Vec::with_capacity(chr_banks);
        for bank in 0..chr_banks {
            chr.push([bank as u8; 0x2000]);
        }

        Cartridge {
            submapper: None,
            prg: Rc::new(PRG { banks: prg }),
            chr: CHR::ROM(Rc::new(chr)),
            sram: vec![[0; 0x2000]],
            mirror: MirroringMode::Horizontal,
        }
    }

    // Tag every 1 KiB physical page, rather than filling whole 8 KiB banks
    // with one value: otherwise lost PPU address bits are invisible.
    fn chr_page_cartridge() -> Cartridge {
        let mut cart = cartridge(8, 4);
        let mut banks = vec![[0u8; 0x2000]; 4];
        for (bank, bytes) in banks.iter_mut().enumerate() {
            for (page, bytes) in bytes.chunks_mut(0x400).enumerate() {
                bytes.fill((bank * 8 + page) as u8);
            }
        }
        cart.chr = CHR::ROM(Rc::new(banks));
        cart
    }

    fn assert_chr_window(mapper: &dyn Mapper, start: u16, size: u16, first_page: u8) {
        for offset in 0..size {
            assert_eq!(
                mapper.read(start + offset),
                first_page + (offset / 0x400) as u8,
                "CHR address {:04x}",
                start + offset
            );
        }
    }

    #[test]
    fn fixed_chr_windows_preserve_all_ppu_address_bits() {
        for id in [0, 2, 7, 15, 20, 50] {
            let mapper = new(chr_page_cartridge(), id).unwrap();
            assert_chr_window(mapper.as_ref(), 0, 0x2000, 0);
        }
    }

    #[test]
    fn switched_8k_chr_windows_preserve_all_ppu_address_bits() {
        for (id, register, value) in [
            (3, 0x8000, 2),
            (11, 0x8000, 0x20),
            (66, 0x8000, 0x02),
            (99, 0x4016, 4),
        ] {
            let mut mapper = new(chr_page_cartridge(), id).unwrap();
            assert_chr_window(mapper.as_ref(), 0, 0x2000, 0);
            mapper.write(register, value);
            assert_chr_window(mapper.as_ref(), 0, 0x2000, if id == 99 { 8 } else { 16 });
        }
    }

    #[test]
    fn mmc1_chr_windows_preserve_offsets_in_both_bank_modes() {
        let mut mapper = new(chr_page_cartridge(), 1).unwrap();
        assert_chr_window(mapper.as_ref(), 0, 0x2000, 0);
        // In 8 KiB mode the low bank bit is ignored.
        write_mmc1(mapper.as_mut(), 0xa000, 3);
        assert_chr_window(mapper.as_ref(), 0, 0x2000, 8);
        write_mmc1(mapper.as_mut(), 0x8000, 0x1c);
        write_mmc1(mapper.as_mut(), 0xa000, 5);
        write_mmc1(mapper.as_mut(), 0xc000, 2);
        assert_chr_window(mapper.as_ref(), 0, 0x1000, 20);
        assert_chr_window(mapper.as_ref(), 0x1000, 0x1000, 8);
    }

    #[test]
    fn chr_ram_uploads_do_not_alias_between_1k_pages() {
        for id in [0, 1, 2, 7, 9, 10, 15, 34, 50, 99] {
            let mut cart = cartridge(8, 1);
            cart.chr = CHR::RAM(vec![[0; 0x2000]]);
            let mut mapper = new(cart, id).unwrap();
            if id == 9 || id == 10 {
                mapper.write(0xb000, 0);
                mapper.write(0xc000, 0);
                mapper.write(0xd000, 1);
                mapper.write(0xe000, 1);
            }
            for address in 0..0x2000u16 {
                mapper.write(address, ((address / 0x400) as u8) ^ (address as u8));
            }
            for address in 0..0x2000u16 {
                assert_eq!(
                    mapper.read(address),
                    ((address / 0x400) as u8) ^ (address as u8),
                    "mapper {id}, CHR address {address:04x}"
                );
            }
        }
    }

    #[test]
    fn mmc2_and_mmc4_chr_windows_preserve_offsets_across_latches() {
        for id in [9, 10] {
            let mut mapper = new(chr_page_cartridge(), id).unwrap();
            mapper.write(0xb000, 1);
            mapper.write(0xc000, 3);
            mapper.write(0xd000, 4);
            mapper.write(0xe000, 6);
            assert_chr_window(mapper.as_ref(), 0, 0x0fd8, 4);
            mapper.read(0x0fe8);
            assert_chr_window(mapper.as_ref(), 0, 0x0fd8, 12);
            assert_chr_window(mapper.as_ref(), 0x1000, 0x0fd8, 16);
            mapper.read(0x1fe8);
            assert_chr_window(mapper.as_ref(), 0x1000, 0x0fd8, 24);
        }
    }

    #[test]
    fn collection_mapper_ids_have_factories() {
        for mapper in [15, 20, 50, 64, 96, 99, 224] {
            assert!(
                new(cartridge(8, 4), mapper).is_some(),
                "mapper {mapper} has no cartridge factory"
            );
        }
    }

    #[test]
    fn mapper15_modes_select_complete_windows_and_ignore_bit7_except_in_8k_mode() {
        let mut mapper = new(cartridge(8, 1), 15).unwrap();
        mapper.write(0x8000, 0x81);
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xc000), 1);
        mapper.write(0x8001, 0x82);
        assert_eq!(mapper.read(0x8000), 2);
        assert_eq!(mapper.read(0xc000), 7);
        mapper.write(0x8003, 0x83);
        assert_eq!(mapper.read(0x8000), 3);
        assert_eq!(mapper.read(0xc000), 3);
    }

    #[test]
    fn vs_system_switches_chr_at_4016_and_uses_four_screen_mirroring() {
        let mut mapper = new(cartridge(2, 2), 99).unwrap();
        assert_eq!(mapper.mirror(), MirroringMode::FourScreen);
        assert_eq!(mapper.read(0x0000), 0);
        mapper.write(0x4016, 4);
        assert_eq!(mapper.read(0x0000), 1);
    }

    #[test]
    fn vs_chr_switch_does_not_replace_the_first_prg_page_in_32k_games() {
        let mut mapper = new(cartridge(2, 2), 99).unwrap();
        for bit in [0, 4, 0, 4] {
            mapper.write(0x4016, bit);
            assert_eq!(mapper.read(0x8000), 0);
            assert_eq!(mapper.read_page(0x80).unwrap()[0], 0);
        }
        let mut gumshoe = new(cartridge(4, 2), 99).unwrap();
        gumshoe.write(0x4016, 4);
        assert_eq!(gumshoe.read(0x8000), 2);
        assert_eq!(gumshoe.read_page(0x80).unwrap()[0], 2);
    }

    #[test]
    fn vs_palette_detection_is_checksum_scoped_and_preserves_unknown_boards() {
        assert_eq!(crc32(b"123456789".iter().copied()), 0xcbf4_3926);
        let palette = vs_palette_for_prg_crc(0x8192_c804).unwrap();
        assert_eq!(palette[0x1a], 0x22); // VS SMB's sky, not ordinary NES green.
        assert!(palette.iter().all(|&color| color < 64));
        assert!(vs_palette_for_prg_crc(0).is_none());
        assert!(new(cartridge(2, 2), 99).unwrap().palette_map().is_none());
    }

    fn write_mmc1(mapper: &mut dyn Mapper, address: u16, value: u8) {
        for bit in 0..5 {
            mapper.write(address, (value >> bit) & 1);
        }
    }

    #[test]
    fn nrom_mirrors_a_16k_prg_rom() {
        let mapper = new(cartridge(1, 1), 0).unwrap();
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xc000), 0);
    }

    #[test]
    fn uxrom_switches_the_lower_prg_window() {
        let mut mapper = new(cartridge(3, 1), 2).unwrap();
        assert_eq!(mapper.read(0x8000), 0);
        mapper.write(0x8000, 1);
        assert_eq!(mapper.read(0x8000), 1);
        assert_eq!(mapper.read(0xc000), 2);
    }

    #[test]
    fn cnrom_switches_chr_banks() {
        let mut mapper = new(cartridge(2, 4), 3).unwrap();
        assert_eq!(mapper.read(0x0000), 0);
        mapper.write(0x8000, 2);
        assert_eq!(mapper.read(0x0000), 2);
    }

    #[test]
    fn mmc1_accepts_serial_register_writes() {
        let mut mapper = new(cartridge(8, 2), 1).unwrap();
        write_mmc1(mapper.as_mut(), 0xe000, 3);
        assert_eq!(mapper.read(0x8000), 3);
        assert_eq!(mapper.read(0xc000), 7);
    }

    #[test]
    fn axrom_switches_prg_and_single_screen_mirroring() {
        let mut mapper = new(cartridge(8, 1), 7).unwrap();
        mapper.write(0x8000, 2);
        assert_eq!(mapper.read(0x8000), 4);
        assert_eq!(mapper.mirror(), MirroringMode::SingleScreenLowerBank);
        mapper.write(0x8000, 0x12);
        assert_eq!(mapper.mirror(), MirroringMode::SingleScreenUpperBank);
    }

    #[test]
    fn mmc3_switches_prg_and_raises_irq() {
        let mut mapper = new(cartridge(8, 2), 4).unwrap();
        mapper.write(0x8000, 6);
        mapper.write(0x8001, 3);
        assert_eq!(mapper.read(0x8000), 1);

        mapper.write(0xc000, 1);
        mapper.write(0xc001, 0);
        mapper.write(0xe001, 0);
        mapper.clock_scanline();
        assert!(!mapper.irq_pending());
        mapper.clock_scanline();
        assert!(mapper.irq_pending());
        mapper.write(0xe000, 0);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn mmc2_and_mmc4_switch_prg_windows() {
        let mut mmc2 = new(cartridge(8, 4), 9).unwrap();
        mmc2.write(0xa000, 3);
        assert_eq!(mmc2.read(0x8000), 1);

        let mut mmc4 = new(cartridge(8, 4), 10).unwrap();
        mmc4.write(0xa000, 2);
        assert_eq!(mmc4.read(0x8000), 2);
    }

    #[test]
    fn vrc_and_vrc6_switch_prg_windows() {
        let mut vrc = new(cartridge(8, 4), 23).unwrap();
        vrc.write(0x8000, 3);
        assert_eq!(vrc.read(0x8000), 1);

        let mut vrc6 = new(cartridge(8, 4), 24).unwrap();
        vrc6.write(0x8000, 2);
        vrc6.write(0xc000, 3);
        assert_eq!(vrc6.read(0x8000), 2);
        assert_eq!(vrc6.read(0xc000), 1);
    }

    #[test]
    fn gxrom_uses_high_bits_for_prg_and_low_bits_for_chr() {
        let mut mapper = new(chr_page_cartridge(), 66).unwrap();
        mapper.write(0x8000, 0x12);
        assert_eq!(mapper.read(0x8000), 2);
        assert_eq!(mapper.read(0xc000), 3);
        assert_eq!(mapper.read_page(0x80).unwrap()[0], 2);
        assert_eq!(mapper.read_page(0xc0).unwrap()[0], 3);
        assert_chr_window(mapper.as_ref(), 0, 0x2000, 16);
    }

    #[test]
    fn fme7_switches_6000_window_between_rom_enabled_ram_and_disabled_ram() {
        let mut mapper = new(cartridge(8, 4), 69).unwrap();
        mapper.write(0x8000, 8);
        mapper.write(0xa000, 0xc0);
        mapper.write(0x6000, 0x5a);
        mapper.write(0x7fff, 0xa5);
        assert_eq!(mapper.read(0x6000), 0x5a);
        assert_eq!(mapper.read(0x7fff), 0xa5);
        assert!(mapper.read_page(0x60).is_none());
        mapper.write(0xa000, 0x40);
        mapper.write(0x6000, 0xff);
        mapper.write(0xa000, 0xc0);
        assert_eq!(mapper.read(0x6000), 0x5a);
        mapper.write(0xa000, 3);
        assert_eq!(mapper.read(0x6000), 1);
        assert_eq!(mapper.read_page(0x60).unwrap()[0], 1);
    }

    #[test]
    fn rambo1_has_three_prg_registers_and_a_four_bit_selector() {
        let mut mapper = new(cartridge(8, 4), 64).unwrap();
        for (register, bank) in [(6, 2), (7, 4), (15, 6)] {
            mapper.write(0x8000, register);
            mapper.write(0x8001, bank);
        }
        for (page, value) in [(0x80, 1), (0xa0, 2), (0xc0, 3), (0xe0, 7)] {
            assert_eq!(mapper.read((page as u16) << 8), value);
            assert_eq!(mapper.read_page(page).unwrap()[0], value);
        }
        mapper.write(0x8000, 0x40);
        for (page, value) in [(0x80, 3), (0xa0, 1), (0xc0, 2), (0xe0, 7)] {
            assert_eq!(mapper.read((page as u16) << 8), value);
            assert_eq!(mapper.read_page(page).unwrap()[0], value);
        }
    }

    #[test]
    fn rambo1_supports_independent_chr_subpages_and_inversion() {
        let mut mapper = new(chr_page_cartridge(), 64).unwrap();
        for (register, bank) in [
            (0, 2),
            (1, 6),
            (2, 12),
            (3, 13),
            (4, 14),
            (5, 15),
            (8, 20),
            (9, 24),
        ] {
            mapper.write(0x8000, register | 0x20);
            mapper.write(0x8001, bank);
        }
        for (slot, bank) in [2, 20, 6, 24, 12, 13, 14, 15].into_iter().enumerate() {
            assert_chr_window(mapper.as_ref(), slot as u16 * 0x400, 0x400, bank);
        }
        mapper.write(0x8000, 0xa0);
        for (slot, bank) in [12, 13, 14, 15, 2, 20, 6, 24].into_iter().enumerate() {
            assert_chr_window(mapper.as_ref(), slot as u16 * 0x400, 0x400, bank);
        }
    }

    #[test]
    fn rambo1_cpu_irq_clocks_every_four_cycles_and_acknowledges() {
        let mut mapper = new(cartridge(8, 4), 64).unwrap();
        mapper.write(0xc000, 1);
        mapper.write(0xc001, 1);
        mapper.write(0xe001, 0);
        for _ in 0..8 {
            mapper.clock_scanline();
            mapper.clock_cpu();
            assert!(!mapper.irq_pending());
        }
        mapper.clock_cpu();
        assert!(mapper.irq_pending());
        mapper.write(0xe000, 0);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn mmc5_separates_background_and_sprite_chr_with_8x16_sprites() {
        let mut mapper = new(chr_page_cartridge(), 5).unwrap();
        mapper.notify_ppu_control(0x20);
        mapper.write(0x5101, 1);
        mapper.write(0x5123, 1);
        mapper.write(0x5127, 2);
        mapper.write(0x512b, 3);
        assert_eq!(mapper.read_background_chr(0x0400), 13);
        assert_eq!(mapper.read_background_chr(0x1400), 13);
        assert_eq!(mapper.read_sprite_chr(0x0400), 5);
        assert_eq!(mapper.read_sprite_chr(0x1400), 9);
        // PPUDATA uses the last set in 8x16 mode; 8x8 fetches always use A.
        mapper.notify_ppu_control(0x20);
        assert_eq!(mapper.read(0x0400), 13);
        mapper.notify_ppu_control(0);
        assert_eq!(mapper.read_sprite_chr(0x0400), 5);
        assert_eq!(mapper.read_background_chr(0x0400), 5);
        assert_eq!(mapper.read(0x0400), 5);
        mapper.write(0x5123, 1);
        assert_eq!(mapper.read_background_chr(0x0400), 5);
        assert_eq!(mapper.read(0x0400), 5);
        mapper.notify_ppu_control(0x20);
        let wrapper = MapperInstance::Dynamic(mapper);
        assert_eq!(wrapper.read_background_chr(0x0400), 13);
        assert_eq!(wrapper.read_sprite_chr(0x0400), 5);
    }

    #[test]
    fn mmc5_irq_status_read_acknowledges_and_each_frame_restarts_the_counter() {
        let mut mapper = new(cartridge(8, 4), 5).unwrap();
        mapper.write(0x5203, 1);
        mapper.write(0x5204, 0x80);
        mapper.clock_scanline();
        assert_eq!(mapper.read(0x5204), 0x40);
        mapper.clock_scanline();
        assert!(mapper.irq_pending());
        assert_eq!(mapper.read(0x5204), 0xc0);
        assert!(!mapper.irq_pending());
        assert_eq!(mapper.read(0x5204), 0x40);
        mapper.end_frame();
        assert_eq!(mapper.read(0x5204), 0);
        mapper.clock_scanline();
        assert!(!mapper.irq_pending());
        mapper.clock_scanline();
        assert!(mapper.irq_pending());
    }

    #[test]
    fn mmc5_2k_chr_mode_uses_the_odd_numbered_registers() {
        let mut mapper = new(chr_page_cartridge(), 5).unwrap();
        mapper.write(0x5101, 2);
        for (register, bank) in [(0x5121, 2), (0x5123, 4), (0x5125, 6), (0x5127, 8)] {
            mapper.write(register, bank);
        }
        for (slot, first) in [4, 8, 12, 16].into_iter().enumerate() {
            assert_eq!(mapper.read_sprite_chr(slot as u16 * 0x800), first);
            assert_eq!(
                mapper.read_sprite_chr(slot as u16 * 0x800 + 0x400),
                first + 1
            );
        }
    }

    #[test]
    fn fme7_mmc5_and_namco163_map_registers() {
        let mut fme7 = new(cartridge(8, 4), 69).unwrap();
        fme7.write(0x8000, 9);
        fme7.write(0xa000, 3);
        assert_eq!(fme7.read(0x8000), 1);

        let mut mmc5 = new(cartridge(8, 4), 5).unwrap();
        mmc5.write(0x5100, 3);
        mmc5.write(0x5114, 0x84); // Bit 7 selects ROM; clear selects RAM.
        mmc5.write(0x5205, 13);
        mmc5.write(0x5206, 17);
        assert_eq!(mmc5.read(0x8000), 2);
        assert_eq!(mmc5.read(0x5205), 13 * 17);

        let mut namco = new(cartridge(8, 16), 19).unwrap();
        namco.write(0xe000, 4);
        assert_eq!(namco.read(0x8000), 2);
    }
}

#[cfg(test)]
#[path = "cartridge_audit_tests.rs"]
mod audit;
