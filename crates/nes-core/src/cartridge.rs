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

    /// Return a directly addressable CHR page when the mapper can expose one.
    /// The generic PPU path falls back to `read` for mappers with banked or
    /// otherwise dynamic CHR mappings.
    fn read_chr_page(&self, _page: u8) -> Option<&[u8; 256]> {
        None
    }

    /// Clock mapper hardware at the end of a rendered scanline.
    /// Most mappers do nothing here; MMC3 uses it for its scanline IRQ counter.
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

    #[inline]
    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match self {
            Self::Nrom(mapper) => mapper.read_page(page),
            Self::Dynamic(mapper) => mapper.read_page(page),
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
    fn clock_scanline(&mut self) {
        match self {
            Self::Nrom(mapper) => mapper.clock_scanline(),
            Self::Dynamic(mapper) => mapper.clock_scanline(),
        }
    }

    #[inline]
    fn clock_cpu(&mut self) {
        match self {
            Self::Nrom(mapper) => mapper.clock_cpu(),
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
            0x0000..=0x1fff => read_chr_1k(&self.cartridge, 0, address as usize),
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
            0x0000..=0x1fff => write_chr_1k(&mut self.cartridge, 0, address as usize, data),
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
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, self.selected_bank * 8, address as usize)
            }
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
        let last = self.cartridge.prg.banks.len() - 1;
        match (self.control >> 2) & 3 {
            0 | 1 => ((self.prg_bank as usize & !1) + (address >= 0xc000) as usize) * 1,
            2 => {
                if address < 0xc000 {
                    0
                } else {
                    self.prg_bank as usize
                }
            }
            _ => {
                if address < 0xc000 {
                    self.prg_bank as usize
                } else {
                    last
                }
            }
        }
    }

    fn chr_bank_for(&self, address: u16) -> usize {
        if self.control & 0x10 == 0 {
            ((self.chr_bank0 as usize & !1) * 4) + (address as usize / 0x1000) * 4
        } else if address < 0x1000 {
            self.chr_bank0 as usize * 4
        } else {
            self.chr_bank1 as usize * 4
        }
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
            0x0000..=0x1fff => read_chr_1k(&self.cartridge, 0, address as usize),
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
            0x0000..=0x1fff => write_chr_1k(&mut self.cartridge, 0, address as usize, data),
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
            0x0000..=0x1fff => read_chr_1k(&self.cartridge, 0, address as usize),
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
            0x0000..=0x1fff => write_chr_1k(&mut self.cartridge, 0, address as usize, data),
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
            0x0000..=0x1fff => read_chr_1k(&self.cartridge, self.chr_bank * 8, address as usize),
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
            0x0000..=0x1fff => read_chr_1k(&self.cartridge, self.chr_bank * 8, address as usize),
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
                self.chr_bank = ((data >> 4) & 3) as usize;
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
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
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
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
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
            0x0fd8..=0x0fdf => self.left_latch.set(false),
            0x0fe8..=0x0fef => self.left_latch.set(true),
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
                self.latch_chr(address);
                read_chr_1k(
                    &self.cartridge,
                    self.chr_bank(address) * 4,
                    address as usize,
                )
            }
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0x9fff if self.mmc4 => {
                prg_16k(&self.cartridge, self.prg_bank, address as usize - 0x8000)
            }
            0x8000..=0x9fff => prg_8k(&self.cartridge, self.prg_bank, address as usize - 0x8000),
            0xa000..=0xbfff if self.mmc4 => prg_16k(
                &self.cartridge,
                self.cartridge.prg.banks.len() - 1,
                address as usize - 0x8000,
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
                write_chr_1k(&mut self.cartridge, bank * 4, address as usize, data)
            }
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0xa000..=0xafff => self.prg_bank = data as usize,
            0xb000..=0xbfff => self.left_fd = data as usize,
            0xc000..=0xcfff => self.left_fe = data as usize,
            0xd000..=0xdfff => self.right_fd = data as usize,
            0xe000..=0xefff => self.right_fe = data as usize,
            0xf000..=0xffff => {
                self.mirror = match data & 3 {
                    0 => MirroringMode::Vertical,
                    _ => MirroringMode::Horizontal,
                }
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0x9f if self.mmc4 => prg_page(&self.cartridge, self.prg_bank, page),
            0x80..=0x9f => prg_page_8k(&self.cartridge, self.prg_bank, page),
            0xa0..=0xff if self.mmc4 => {
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
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_mode_cpu: bool,
    irq_pending: bool,
}

impl VRC {
    fn new(cartridge: Cartridge, mapper: u8) -> Self {
        Self {
            mirror: cartridge.mirror,
            cartridge,
            prg: [0, 0],
            chr: [0; 8],
            swap: false,
            vrc2: mapper == 22,
            chr_shift: mapper == 22,
            vrc6: mapper == 24 || mapper == 26,
            address_mode: match mapper {
                21 => 0,
                22 => 1,
                23 => 2,
                24 => 3,
                25 => 0,
                26 => 1,
                _ => 3,
            },
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_mode_cpu: false,
            irq_pending: false,
        }
    }

    fn port(&self, address: u16) -> u8 {
        let low = (address & 0x0f) as u8;
        match self.address_mode {
            0 => (low >> 1) & 3,
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
                self.chr[address as usize / 0x400],
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
                let bank = self.chr[address as usize / 0x400];
                write_chr_1k(&mut self.cartridge, bank, address as usize, data)
            }
            0x6000..=0x7fff => write_sram(&mut self.cartridge, address, data),
            0x8000..=0x8fff => self.prg[0] = data as usize,
            0x9000..=0x9fff if self.vrc6 => {}
            0x9000..=0x9fff => match self.port(address) {
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
            0xb000..=0xefff => {
                let block = ((address as usize - 0xb000) / 0x1000) * 2;
                let port = self.port(address) as usize & 1;
                let index = block + port;
                if index < 8 {
                    if port == 0 {
                        self.chr[index] = (self.chr[index] & 0xf0) | (data as usize & 0x0f);
                    } else {
                        self.chr[index] = (self.chr[index] & 0x0f) | ((data as usize & 0x1f) << 4);
                    }
                    if self.chr_shift {
                        self.chr[index] >>= 1;
                    }
                }
            }
            0xf000..=0xffff => match self.port(address) {
                0 => self.irq_latch = (self.irq_latch & 0xf0) | (data & 0x0f),
                1 => self.irq_latch = (self.irq_latch & 0x0f) | ((data & 0x0f) << 4),
                2 => {
                    self.irq_mode_cpu = data & 4 != 0;
                    self.irq_enabled = data & 2 != 0;
                    self.irq_counter = self.irq_latch;
                    self.irq_pending = false;
                }
                _ => {
                    self.irq_enabled = data & 1 != 0;
                    self.irq_pending = false;
                }
            },
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
        if self.irq_enabled && self.irq_mode_cpu {
            if self.irq_counter == 0 {
                self.irq_counter = self.irq_latch;
                self.irq_pending = true;
            } else {
                self.irq_counter -= 1;
            }
        }
    }

    fn clock_scanline(&mut self) {
        if self.irq_enabled && !self.irq_mode_cpu {
            if self.irq_counter == 0 {
                self.irq_counter = self.irq_latch;
                self.irq_pending = true;
            } else {
                self.irq_counter -= 1;
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
            0x6000..=0x7fff => prg_8k(&self.cartridge, self.prg[0], address as usize - 0x6000),
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
            0x60..=0x7f => prg_page_8k(&self.cartridge, self.prg[0], page - 0x40),
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
    chr_mode: u8,
    prg: [u8; 5],
    chr: [u16; 12],
    mirror: MirroringMode,
    ram_protect1: u8,
    ram_protect2: u8,
    irq_line: u8,
    irq_enabled: bool,
    irq_pending: bool,
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
            chr_mode: 0,
            prg: [0, 0, 0, 0, 0xff],
            chr: [0; 12],
            mirror,
            ram_protect1: 0,
            ram_protect2: 0,
            irq_line: 0,
            irq_enabled: false,
            irq_pending: false,
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

    fn chr_bank(&self, address: u16) -> usize {
        let slot = address as usize / 0x400;
        match self.chr_mode & 3 {
            0 => self.chr[7] as usize * 8 + slot,
            1 => {
                if slot < 4 {
                    self.chr[3] as usize * 4 + slot
                } else {
                    self.chr[7] as usize * 4 + slot - 4
                }
            }
            2 => self.chr[slot / 2] as usize * 2 + slot % 2,
            _ => self.chr[slot] as usize,
        }
    }
}

impl Mapper for MMC5 {
    fn mirror(&self) -> MirroringMode {
        self.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => {
                read_chr_1k(&self.cartridge, self.chr_bank(address), address as usize)
            }
            0x5205 => (self.multiplier_a as u16 * self.multiplier_b as u16) as u8,
            0x5206 => ((self.multiplier_a as u16 * self.multiplier_b as u16) >> 8) as u8,
            0x5204 => (self.irq_pending as u8) << 7,
            0x6000..=0x7fff => read_sram(&self.cartridge, address),
            0x8000..=0xffff => prg_8k(
                &self.cartridge,
                self.prg_bank(address),
                address as usize - 0x8000,
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
            0x5100 => self.prg_mode = data & 3,
            0x5101 => self.chr_mode = data & 3,
            0x5102 => self.ram_protect1 = data & 3,
            0x5103 => self.ram_protect2 = data & 3,
            0x5105 => {
                self.mirror = match data & 3 {
                    0 => MirroringMode::Vertical,
                    1 => MirroringMode::Horizontal,
                    2 => MirroringMode::SingleScreenLowerBank,
                    _ => MirroringMode::SingleScreenUpperBank,
                }
            }
            0x5113..=0x5117 => self.prg[(address - 0x5113) as usize] = data,
            0x5120..=0x512b => self.chr[(address - 0x5120) as usize] = data as u16,
            0x5203 => self.irq_line = data,
            0x5204 => {
                self.irq_enabled = data & 0x80 != 0;
                if !self.irq_enabled {
                    self.irq_pending = false;
                }
            }
            0x5205 => self.multiplier_a = data,
            0x5206 => self.multiplier_b = data,
            0x6000..=0x7fff if self.ram_protect1 == 2 && self.ram_protect2 == 1 => {
                write_sram(&mut self.cartridge, address, data)
            }
            _ => {}
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        match page {
            0x80..=0xff => prg_page_8k(&self.cartridge, self.prg_bank((page as u16) << 8), page),
            _ => None,
        }
    }

    fn clock_scanline(&mut self) {
        self.scanline = self.scanline.wrapping_add(1);
        if self.irq_enabled && self.scanline == self.irq_line {
            self.irq_pending = true;
        }
        if self.scanline >= 241 {
            self.scanline = 0;
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}

#[derive(Clone)]
struct Namco163 {
    cartridge: Cartridge,
    chr: [usize; 12],
    prg: [usize; 3],
    internal_ram: [u8; 128],
    ram_address: u8,
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
            ram_address: 0,
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

    fn ram_window_writable(&self, address: u16) -> bool {
        let bit = ((address as usize - 0x6000) / 0x800) as u8;
        (self.ram_protect & (1 << bit)) == 0 && (self.ram_protect & 0xf0) == 0x40
    }

    fn ram_data_read(&self) -> u8 {
        self.internal_ram[(self.ram_address & 0x7f) as usize]
    }

    fn ram_data_write(&mut self, data: u8) {
        self.internal_ram[(self.ram_address & 0x7f) as usize] = data;
        if self.ram_auto_increment {
            self.ram_address = self.ram_address.wrapping_add(1);
        }
    }
}

impl Mapper for Namco163 {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
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
                self.ram_address = data & 0x7f;
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
        21 | 22 | 23 | 25 => Box::new(VRC::new(cartridge, mapper)),
        24 | 26 => Box::new(VRC::new(cartridge, mapper)),
        34 => Box::new(BNROM::new(cartridge)),
        66 => Box::new(GxROM::new(cartridge)),
        69 => Box::new(FME7::new(cartridge)),
        19 => Box::new(Namco163::new(cartridge)),
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
            prg: Rc::new(PRG { banks: prg }),
            chr: CHR::ROM(Rc::new(chr)),
            sram: vec![[0; 0x2000]],
            mirror: MirroringMode::Horizontal,
        }
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
    fn fme7_mmc5_and_namco163_map_registers() {
        let mut fme7 = new(cartridge(8, 4), 69).unwrap();
        fme7.write(0x8000, 9);
        fme7.write(0xa000, 3);
        assert_eq!(fme7.read(0x8000), 1);

        let mut mmc5 = new(cartridge(8, 4), 5).unwrap();
        mmc5.write(0x5100, 3);
        mmc5.write(0x5114, 4);
        mmc5.write(0x5205, 13);
        mmc5.write(0x5206, 17);
        assert_eq!(mmc5.read(0x8000), 2);
        assert_eq!(mmc5.read(0x5205), 13 * 17);

        let mut namco = new(cartridge(8, 16), 19).unwrap();
        namco.write(0xe000, 4);
        assert_eq!(namco.read(0x8000), 2);
    }
}
