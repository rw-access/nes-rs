use std::rc::Rc;

use dyn_clone::DynClone;

#[derive(Clone, Copy)]
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
    // fn new(cartridge: Cartridge) -> Self;
    fn mirror(&self) -> MirroringMode;
    fn read(&self, address: u16) -> u8;
    fn write(&mut self, address: u16, data: u8);
    fn read_page(&self, page: u8) -> Option<&[u8; 256]>;
}

dyn_clone::clone_trait_object!(Mapper);

#[derive(Clone)]
struct UxROM {
    cartridge: Cartridge,
    first_bank: usize,
    last_bank: usize,
}

impl UxROM {
    fn new(cartridge: Cartridge) -> Self {
        UxROM {
            last_bank: cartridge.prg.banks.len() - 1,
            cartridge,
            first_bank: 0,
        }
    }
}

impl Mapper for UxROM {
    fn mirror(&self) -> MirroringMode {
        self.cartridge.mirror
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => self.cartridge.chr.get_banks()[0][address as usize],
            0x2000..=0x7fff => 0,
            0x8000..=0xbfff => {
                // CPU $8000-$BFFF: 16 KB switchable PRG ROM bank
                self.cartridge.prg.banks[self.first_bank][address as usize % 0x4000]
            }
            0xc000.. => {
                // CPU $C000-$FFFF: 16 KB PRG ROM bank, fixed to the last bank
                self.cartridge.prg.banks[self.last_bank][address as usize % 0x4000]
            }
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => {
                if let Some(banks) = self.cartridge.chr.get_banks_mut() {
                    banks[0][address as usize] = data;
                }
            }
            0x2000..=0x7fff => {}
            0x8000.. => self.first_bank = data as usize & 0x0f,
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        let bank_start = ((page as usize) << 8) % 0x4000;
        let bank_stop = (bank_start + 256) % 0x4000;

        match page {
            // internal CPU read
            0x00..=0x7f => None,
            0x80..=0xBF => {
                // CPU $8000-$BFFF: 16 KB switchable PRG ROM bank
                self.cartridge.prg.banks[self.first_bank][bank_start..bank_stop]
                    .try_into()
                    .ok()
            }
            0xC0.. => {
                // CPU $C000-$FFFF: 16 KB PRG ROM bank, fixed to the last bank
                self.cartridge.prg.banks[self.last_bank][bank_start..bank_stop]
                    .try_into()
                    .ok()
            }
        }
    }
}

#[derive(Clone)]
struct NROM {
    uxrom: UxROM,
}

impl NROM {
    fn new(cartridge: Cartridge) -> Self {
        NROM {
            uxrom: UxROM::new(cartridge),
        }
    }
}

impl Mapper for NROM {
    fn mirror(&self) -> MirroringMode {
        self.uxrom.mirror()
    }

    fn read(&self, address: u16) -> u8 {
        self.uxrom.read(address)
    }

    fn write(&mut self, address: u16, data: u8) {
        match address {
            0x0000..=0x1fff => self.uxrom.write(address, data),
            0x2000.. => {}
        };
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        self.uxrom.read_page(page)
    }
}

pub fn new(cartridge: Cartridge, mapper: u8) -> Option<Box<dyn Mapper>> {
    match mapper {
        0 => Some(Box::new(NROM::new(cartridge))),
        2 => Some(Box::new(UxROM::new(cartridge))),
        _ => None,
    }
}
