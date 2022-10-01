use super::cartridge;
use super::cartridge::Cartridge;

const MAGIC: [u8; 4] = ['N' as u8, 'E' as u8, 'S' as u8, 0x1a];

// https://www.nesdev.org/wiki/INES
#[derive(Default)]
struct INESHeader {
    magic: [u8; 4],           // NES \x1a
    prg_banks: u8,            // 4: Size of PRG ROM in 16 KB units
    chr_banks: u8, // 5: Size of CHR ROM in 8 KB units (Value 0 means the board uses CHR RAM)
    mirror: bool,  // 6 0
    has_battery: bool, // 6 1
    has_trainer: bool, // 6 2
    four_screen_mirror: bool, // 6 3
    // mapper_lo: // 6 4..7
    vs_unisystem: bool, // 7 0
    playchoice10: bool, // 7 1
    nes2: bool,         // 7 2..3
    // mapper_hi // 7 4..6
    ram_size: u8, // 8
    pal: bool,    // 9 1
    // ignored   // 9 2.. 7
    tv_system_prg_ram_presence: u8, // 10
    // ignored // 11-15
    mapper: u8, // mapper_hi << 4 | mapper_lo
}

impl INESHeader {
    fn parse<R: std::io::Read>(reader: &mut R) -> Option<INESHeader> {
        let mut buffer: [u8; 16] = [0; 16];
        let mut ines_header = INESHeader::default();

        // read exactly 10 bytes
        reader.read_exact(buffer.as_mut_slice()).ok()?;

        if &buffer[..4] != &MAGIC[..] {
            return None;
        }

        ines_header.magic[..4].copy_from_slice(&buffer[..4]);
        ines_header.prg_banks = buffer[4];
        ines_header.chr_banks = buffer[5];
        ines_header.mirror = (buffer[6] & 0b0001) != 0;
        ines_header.has_battery = (buffer[6] & 0b0010) != 0;
        ines_header.has_battery = (buffer[6] & 0b0100) != 0;
        ines_header.has_trainer = (buffer[6] & 0b0100) != 0;
        ines_header.four_screen_mirror = (buffer[6] & 0b1000) != 0;
        ines_header.mapper = buffer[6] >> 4;
        ines_header.vs_unisystem = buffer[7] & 0b0001 != 0;
        ines_header.playchoice10 = buffer[7] & 0b0010 != 0;
        ines_header.nes2 = buffer[7] & 0b1100 == 0b1000;
        ines_header.mapper = buffer[7] >> 4;
        ines_header.ram_size = buffer[8];
        ines_header.pal = buffer[9] & 0b1 != 0;
        ines_header.tv_system_prg_ram_presence = buffer[10];

        Some(ines_header)
    }

    fn read<R: std::io::Read>(&self, reader: &mut R) -> Option<cartridge::Cartridge> {
        // https://www.nesdev.org/wiki/INES
        // 1. Header (16 bytes)
        // 2. Trainer, if present (0 or 512 bytes)
        // 3. PRG ROM data (16384 * x bytes)
        // 4. CHR ROM data, if present (8192 * y bytes)
        // 5. PlayChoice INST-ROM, if present (0 or 8192 bytes)
        // 6. PlayChoice PROM, if present (16 bytes Data, 16 bytes CounterOut) (this is often missing, see PC10 ROM-Images for details)

        let mut c = Cartridge {
            prg: Vec::with_capacity(self.prg_banks as usize),
            chr: Vec::with_capacity(self.chr_banks as usize),
            sram: Vec::with_capacity(self.ram_size as usize),
            mirror: match (self.four_screen_mirror, self.mirror) {
                (true, _) => cartridge::MirroringMode::FourScreen,
                (false, false) => cartridge::MirroringMode::Horizontal,
                (false, true) => cartridge::MirroringMode::Vertical,
            },
        };

        unsafe {
            // already preallocated with capacity, so this is perfectly safe
            // since unitialized u8 is perfectly acceptable and it will be scanned into
            c.prg.set_len(self.prg_banks as usize);
            c.chr.set_len(self.chr_banks as usize);
            c.sram.set_len(self.ram_size as usize);
        };

        if self.has_trainer {
            return None;
        }

        // load PRG ROM
        for prg in &mut c.prg {
            reader.read_exact(prg.as_mut_slice()).ok()?;
        }

        // load CHR ROM
        for chr in &mut c.chr {
            reader.read_exact(chr.as_mut_slice()).ok()?;
        }

        // CHR RAM
        if c.chr.len() == 0 {
            c.chr = vec![[0u8; 8192]];
        }

        // PRG RAM??
        ();

        Some(c)
    }
}

pub fn load<R: std::io::Read>(reader: &mut R) -> Option<(cartridge::Cartridge, u8)> {
    let header = INESHeader::parse(reader)?;
    let cartridge = header.read(reader)?;

    Some((cartridge, header.mapper))
}
