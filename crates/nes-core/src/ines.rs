use std::io::Cursor;
use std::rc::Rc;

use crate::cartridge::{ChrBank, ProgBank, CHR, PRG};

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

        // The iNES header is exactly 16 bytes.
        reader.read_exact(buffer.as_mut_slice()).ok()?;

        if &buffer[..4] != &MAGIC[..] {
            return None;
        }

        ines_header.magic[..4].copy_from_slice(&buffer[..4]);
        ines_header.prg_banks = buffer[4];
        ines_header.chr_banks = buffer[5];
        ines_header.mirror = (buffer[6] & 0b0001) != 0;
        ines_header.has_battery = (buffer[6] & 0b0010) != 0;
        ines_header.has_trainer = (buffer[6] & 0b0100) != 0;
        ines_header.four_screen_mirror = (buffer[6] & 0b1000) != 0;
        ines_header.vs_unisystem = buffer[7] & 0b0001 != 0;
        ines_header.playchoice10 = buffer[7] & 0b0010 != 0;
        ines_header.nes2 = buffer[7] & 0b1100 == 0b1000;
        ines_header.mapper = (buffer[6] >> 4) | (buffer[7] & 0xf0);
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
        if self.has_trainer {
            let mut trainer = [0u8; 512];
            reader.read_exact(&mut trainer).ok()?;
        }

        // load PRG ROM
        if self.prg_banks == 0 {
            return None;
        }
        let mut prg_banks: Vec<ProgBank> = vec![[0u8; 0x4000]; self.prg_banks as usize];

        for bank in &mut prg_banks {
            reader.read_exact(bank.as_mut_slice()).ok()?;
        }

        // load CHR ROM / CHR RAM
        let chr = if self.chr_banks == 0 {
            CHR::RAM(vec![[0u8; 8192]])
        } else {
            let mut chr_banks: Vec<ChrBank> = vec![[0u8; 0x2000]; self.chr_banks as usize];

            for bank in &mut chr_banks {
                reader.read_exact(bank.as_mut_slice()).ok()?;
            }

            CHR::ROM(Rc::new(chr_banks))
        };

        // iNES byte 8 is the number of 8 KiB PRG-RAM banks. A zero value
        // historically means one bank for boards that expose PRG-RAM.
        let sram_banks = usize::from(self.ram_size.max(1));

        Some(Cartridge {
            prg: Rc::new(PRG { banks: prg_banks }),
            chr,
            sram: vec![[0u8; 0x2000]; sram_banks],
            mirror: match (self.four_screen_mirror, self.mirror) {
                (true, _) => cartridge::MirroringMode::FourScreen,
                (false, false) => cartridge::MirroringMode::Horizontal,
                (false, true) => cartridge::MirroringMode::Vertical,
            },
        })
    }
}

pub fn load<R: std::io::Read>(reader: &mut R) -> Option<(cartridge::Cartridge, u8)> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).ok()?;

    if bytes.starts_with(&MAGIC) {
        return load_ines(&bytes);
    }
    if bytes.starts_with(b"UNIF") {
        return load_unif(&bytes);
    }
    None
}

fn load_ines(bytes: &[u8]) -> Option<(cartridge::Cartridge, u8)> {
    let mut header = INESHeader::parse(&mut Cursor::new(bytes))?;
    let payload_len = bytes.len().checked_sub(16)?;
    let declared_payload_len = (header.prg_banks as usize)
        .checked_mul(0x4000)?
        .checked_add((header.chr_banks as usize).checked_mul(0x2000)?)
        .and_then(|size| size.checked_add(if header.has_trainer { 512 } else { 0 }))?;

    // A few ROMs in the collection set the trainer bit but omit the trainer
    // bytes. Accept that common bad-dump variant when the untrained payload
    // exactly matches the file size.
    if header.has_trainer
        && declared_payload_len >= 512
        && payload_len + 512 == declared_payload_len
    {
        header.has_trainer = false;
    }

    // Some old dumps have a zeroed size field, or use mapper 20 as an iNES
    // wrapper around a 64 KiB FDS side. Infer the available 16 KiB PRG size
    // when there is no ambiguity in the payload.
    let payload_without_trainer = payload_len - usize::from(header.has_trainer) * 512;
    if (header.prg_banks == 0 && header.chr_banks == 0)
        || (header.mapper == 20 && declared_payload_len > payload_len)
    {
        if payload_without_trainer % 0x4000 == 0 {
            header.prg_banks = (payload_without_trainer / 0x4000) as u8;
        }
    }

    // The header parser consumed the first 16 bytes conceptually, but the
    // cartridge reader must start at the payload immediately after it.
    let cartridge = header.read(&mut Cursor::new(&bytes[16..]))?;
    Some((cartridge, header.mapper))
}

fn load_unif(bytes: &[u8]) -> Option<(cartridge::Cartridge, u8)> {
    // UNIF has a fixed 32-byte header followed by id/length/data chunks.
    if bytes.len() < 32 {
        return None;
    }

    let mut prg = Vec::new();
    let mut chr = Vec::new();
    let mut mirror = cartridge::MirroringMode::Horizontal;
    let mut offset = 32;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        offset += 8;

        // Early UNIF writers emitted NAME# with a zero length and stored the
        // text inline anyway. Skip that legacy text until the next chunk.
        if id == b"NAME#" && length == 0 {
            let next = bytes[offset..]
                .windows(4)
                .position(|candidate| candidate == b"CTRL")?;
            offset += next;
            continue;
        }

        let end = offset.checked_add(length)?;
        let data = bytes.get(offset..end)?;
        match id {
            b"PRG0" | b"PRG1" | b"PRG2" | b"PRG3" => prg.extend_from_slice(data),
            b"CHR0" | b"CHR1" | b"CHR2" | b"CHR3" => chr.extend_from_slice(data),
            b"MIRR" if data.first().copied().unwrap_or(0) != 0 => {
                mirror = cartridge::MirroringMode::Vertical
            }
            _ => {}
        }
        offset = end;
    }

    // A legacy dump may include a small auxiliary PRG chunk after the real
    // 16 KiB-aligned program data. Keep the complete aligned portion.
    prg.truncate(prg.len() / 0x4000 * 0x4000);
    if prg.is_empty() || (!chr.is_empty() && chr.len() % 0x2000 != 0) {
        return None;
    }

    let prg_banks = prg
        .chunks_exact(0x4000)
        .map(|bank| bank.try_into().ok())
        .collect::<Option<Vec<ProgBank>>>()?;
    let chr = if chr.is_empty() {
        CHR::RAM(vec![[0; 0x2000]])
    } else {
        CHR::ROM(Rc::new(
            chr.chunks_exact(0x2000)
                .map(|bank| bank.try_into().ok())
                .collect::<Option<Vec<ChrBank>>>()?,
        ))
    };

    Some((
        Cartridge {
            prg: Rc::new(crate::cartridge::PRG { banks: prg_banks }),
            chr,
            sram: vec![[0; 0x2000]],
            mirror,
        },
        0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unif_chunk(bytes: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
    }

    #[test]
    fn loads_minimal_unif_program_and_chr() {
        let mut bytes = vec![0; 32];
        bytes[..4].copy_from_slice(b"UNIF");
        unif_chunk(&mut bytes, b"PRG0", &[0; 0x4000]);
        unif_chunk(&mut bytes, b"CHR0", &[0; 0x2000]);

        let (cartridge, mapper) = load(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(mapper, 0);
        assert_eq!(cartridge.prg.banks.len(), 1);
        assert_eq!(cartridge.chr.get_banks().len(), 1);
    }

    #[test]
    fn accepts_a_trainer_flag_when_the_trainer_is_missing() {
        let mut bytes = vec![0; 16 + 0x4000 + 0x2000];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4] = 1;
        bytes[5] = 1;
        bytes[6] = 0x04;

        let (cartridge, mapper) = load(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(mapper, 0);
        assert_eq!(cartridge.prg.banks.len(), 1);
    }

    #[test]
    fn starts_prg_reading_after_the_ines_header() {
        let mut bytes = vec![0; 16 + 0x4000];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4] = 1;
        bytes[16] = 0xa9;
        bytes[16 + 0x3ffc] = 0x00;
        bytes[16 + 0x3ffd] = 0x80;

        let (cartridge, mapper) = load(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(mapper, 0);
        assert_eq!(cartridge.prg.banks[0][0], 0xa9);
        assert_eq!(&cartridge.prg.banks[0][0x3ffc..0x3ffe], &[0x00, 0x80]);
    }
}
