#[derive(Clone, Copy)]
pub enum MirroringMode {
    Horizontal,
    Vertical,
    SingleScreenLowerBank,
    FourScreen,
    SingleScreenUpperBank,
}

pub type ProgBank = [u8; 0x4000];
pub type ChrBank = [u8; 0x2000];
pub type SaveRamBank = [u8; 0x2000];

pub struct Cartridge {
    pub prg: Vec<ProgBank>,     // 0x4000 aligned
    pub chr: Vec<ChrBank>,      // 0x2000 aligned
    pub sram: Vec<SaveRamBank>, // 0x2000 aligned
    pub mirror: MirroringMode,
}

pub(crate) trait Mapper {
    // fn new(cartridge: Cartridge) -> Self;
    fn mirror(&self) -> MirroringMode;
    fn read(&self, address: u16) -> u8;
    fn write(&mut self, address: u16, data: u8);
    fn read_page(&self, page: u8) -> Option<&[u8; 256]>;
}

struct UxROM {
    cartridge: Cartridge,
    first_bank: usize,
    last_bank: usize,
}

impl UxROM {
    fn new(cartridge: Cartridge) -> Self {
        UxROM {
            last_bank: cartridge.prg.len() - 1,
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
        if address < 0x2000 {
            self.cartridge.chr[0][address as usize]
        } else if address < 0x8000 {
            0
        } else if address < 0xC000 {
            // CPU $8000-$BFFF: 16 KB switchable PRG ROM bank
            self.cartridge.prg[self.first_bank][address as usize % 0x4000]
        } else {
            // CPU $C000-$FFFF: 16 KB PRG ROM bank, fixed to the last bank
            self.cartridge.prg[self.last_bank][address as usize % 0x4000]
        }
    }

    fn write(&mut self, address: u16, data: u8) {
        if address < 0x2000 {
            self.cartridge.chr[0][address as usize] = data
        } else if address < 0x8000 {
        } else {
            self.first_bank = data as usize & 0x0f;
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        let bank_start = ((page as usize) << 8) % 0x4000;
        let bank_stop = (bank_start + 256) % 0x4000;

        if page < 0x80 {
            // internal CPU read
            None
        } else if page < 0xC0 {
            // CPU $8000-$BFFF: 16 KB switchable PRG ROM bank
            self.cartridge.prg[self.first_bank][bank_start..bank_stop]
                .try_into()
                .ok()
        } else {
            // CPU $C000-$FFFF: 16 KB PRG ROM bank, fixed to the last bank
            self.cartridge.prg[self.last_bank][bank_start..bank_stop]
                .try_into()
                .ok()
        }
    }
}

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
        if address < 0x2000 {
            self.uxrom.write(address, data);
        }
    }

    fn read_page(&self, page: u8) -> Option<&[u8; 256]> {
        self.uxrom.read_page(page)
    }
}

pub(crate) fn new(cartridge: Cartridge, mapper: u8) -> Option<Box<dyn Mapper>> {
    match mapper {
        0 => Some(Box::new(NROM::new(cartridge))),
        2 => Some(Box::new(UxROM::new(cartridge))),
        _ => None,
    }
}
