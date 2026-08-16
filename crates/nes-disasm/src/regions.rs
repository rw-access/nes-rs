//! Input regions, provenance, and lossless iNES/NROM mapping.

use crate::diagnostics::DisasmError;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    Code,
    Bytes,
    Words,
    Strings,
    Palette,
    Chr,
    Padding,
    Trainer,
    PlayChoice,
    Header,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classification {
    Code,
    Data,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Explicit,
    EntryPoint,
    RecursiveTraversal,
    LinearSweep,
    Vector,
    UserForced,
    Inferred,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub name: Option<String>,
    pub file_offset: usize,
    pub cpu_address: Option<u16>,
    pub bytes: Vec<u8>,
    pub kind: RegionKind,
    pub classification: Classification,
    pub provenance: Provenance,
}

impl Region {
    pub fn new(
        file_offset: usize,
        cpu_address: Option<u16>,
        bytes: Vec<u8>,
        kind: RegionKind,
        classification: Classification,
        provenance: Provenance,
    ) -> Self {
        Self {
            name: None,
            file_offset,
            cpu_address,
            bytes,
            kind,
            classification,
            provenance,
        }
    }
    pub fn end_file_offset(&self) -> usize {
        self.file_offset + self.bytes.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InesFormat {
    Ines,
    Nes2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InesHeader {
    pub raw: [u8; 16],
    pub format: InesFormat,
    pub prg_rom_size: usize,
    pub chr_rom_size: usize,
    pub mapper: u16,
    pub submapper: u8,
    pub has_trainer: bool,
    pub has_battery: bool,
    pub vertical_mirroring: bool,
    pub four_screen_mirroring: bool,
    pub playchoice: bool,
    pub misc_roms: u8,
    pub prg_ram_size: usize,
    pub chr_ram_size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InesImage {
    pub bytes: Vec<u8>,
    pub header: InesHeader,
    pub trainer: Option<Range<usize>>,
    pub prg: Range<usize>,
    pub chr: Option<Range<usize>>,
    pub playchoice_inst: Option<Range<usize>>,
    pub playchoice_prom: Option<Range<usize>>,
    pub misc: Vec<Range<usize>>,
    /// Bytes after all declared regions, retained for exact reconstruction.
    pub trailing: Option<Range<usize>>,
}

impl InesImage {
    pub fn parse(bytes: &[u8]) -> Result<Self, DisasmError> {
        if bytes.len() < 16 {
            return Err(DisasmError::TruncatedRegion {
                region: "iNES header",
                needed: 16,
                available: bytes.len(),
            });
        }
        if &bytes[..4] != b"NES\x1a" {
            return Err(DisasmError::InvalidInesHeader {
                reason: "missing NES\\x1A magic".into(),
            });
        }
        let raw: [u8; 16] = bytes[..16].try_into().unwrap();
        let format = if raw[7] & 0x0c == 0x08 {
            InesFormat::Nes2
        } else {
            InesFormat::Ines
        };
        let mapper = u16::from(raw[6] >> 4)
            | u16::from(raw[7] & 0xf0)
            | if matches!(format, InesFormat::Nes2) {
                u16::from(raw[8] & 0x0f) << 8
            } else {
                0
            };
        let submapper = if matches!(format, InesFormat::Nes2) {
            raw[8] >> 4
        } else {
            0
        };
        let prg_rom_size = rom_size(raw[4], raw[9] & 0x0f, 0x4000, format)?;
        let chr_rom_size = rom_size(raw[5], raw[9] >> 4, 0x2000, format)?;
        let prg_ram_size = if matches!(format, InesFormat::Nes2) {
            nes2_ram_size(raw[10] & 0x0f)
        } else {
            usize::from(raw[8].max(1)) * 0x2000
        };
        let chr_ram_size = if matches!(format, InesFormat::Nes2) {
            nes2_ram_size(raw[11] & 0x0f)
        } else if chr_rom_size == 0 {
            0x2000
        } else {
            0
        };
        if prg_rom_size == 0 {
            return Err(DisasmError::InvalidInesHeader {
                reason: "PRG ROM size is zero".into(),
            });
        }
        let trainer = if raw[6] & 0x04 != 0 {
            Some(16..528)
        } else {
            None
        };
        let prg_start = trainer.as_ref().map_or(16, |r| r.end);
        let prg = checked_range("PRG ROM", prg_start, prg_rom_size, bytes.len())?;
        let chr = if chr_rom_size == 0 {
            None
        } else {
            Some(checked_range(
                "CHR ROM",
                prg.end,
                chr_rom_size,
                bytes.len(),
            )?)
        };
        let mut cursor = chr.as_ref().map_or(prg.end, |r| r.end);
        let playchoice_inst = if raw[7] & 0x02 != 0 {
            let r = checked_range("PlayChoice INST-ROM", cursor, 0x2000, bytes.len())?;
            cursor = r.end;
            Some(r)
        } else {
            None
        };
        let playchoice_prom = if raw[7] & 0x02 != 0 {
            let r = checked_range("PlayChoice PROM", cursor, 32, bytes.len())?;
            cursor = r.end;
            Some(r)
        } else {
            None
        };
        let misc_count = if matches!(format, InesFormat::Nes2) {
            raw[14] & 0x03
        } else {
            0
        };
        let mut misc = Vec::new();
        if misc_count != 0 && cursor < bytes.len() {
            // NES 2.0 records the count but not individual miscellaneous ROM
            // lengths in the header. Keep the complete tail opaque.
            misc.push(cursor..bytes.len());
            cursor = bytes.len();
        }
        let trailing = (cursor < bytes.len()).then_some(cursor..bytes.len());
        Ok(Self {
            bytes: bytes.to_vec(),
            header: InesHeader {
                raw,
                format,
                prg_rom_size,
                chr_rom_size,
                mapper,
                submapper,
                has_trainer: trainer.is_some(),
                has_battery: raw[6] & 0x02 != 0,
                vertical_mirroring: raw[6] & 1 != 0,
                four_screen_mirroring: raw[6] & 8 != 0,
                playchoice: raw[7] & 2 != 0,
                misc_roms: misc_count,
                prg_ram_size,
                chr_ram_size,
            },
            trainer,
            prg,
            chr,
            playchoice_inst,
            playchoice_prom,
            misc,
            trailing,
        })
    }

    pub fn is_nrom(&self) -> bool {
        self.header.mapper == 0 && self.header.prg_rom_size == 0x4000
            || self.header.mapper == 0 && self.header.prg_rom_size == 0x8000
    }
    pub fn nrom(&self) -> Result<NromMapping<'_>, DisasmError> {
        NromMapping::new(self)
    }
    pub fn regions(&self) -> Vec<Region> {
        let mut out = vec![Region::new(
            0,
            None,
            self.bytes[0..16].to_vec(),
            RegionKind::Header,
            Classification::Data,
            Provenance::Explicit,
        )];
        if let Some(r) = &self.trainer {
            out.push(Region::new(
                r.start,
                None,
                self.bytes[r.clone()].to_vec(),
                RegionKind::Trainer,
                Classification::Data,
                Provenance::Explicit,
            ));
        }
        out.push(Region::new(
            self.prg.start,
            Some(0x8000),
            self.bytes[self.prg.clone()].to_vec(),
            RegionKind::Code,
            Classification::Unknown,
            Provenance::Unknown,
        ));
        if let Some(r) = &self.chr {
            out.push(Region::new(
                r.start,
                None,
                self.bytes[r.clone()].to_vec(),
                RegionKind::Chr,
                Classification::Data,
                Provenance::Explicit,
            ));
        }
        for r in self
            .playchoice_inst
            .iter()
            .chain(self.playchoice_prom.iter())
            .chain(self.misc.iter())
        {
            out.push(Region::new(
                r.start,
                None,
                self.bytes[r.clone()].to_vec(),
                RegionKind::PlayChoice,
                Classification::Data,
                Provenance::Explicit,
            ));
        }
        if let Some(r) = &self.trailing {
            out.push(Region::new(
                r.start,
                None,
                self.bytes[r.clone()].to_vec(),
                RegionKind::Unknown,
                Classification::Unknown,
                Provenance::Explicit,
            ));
        }
        out
    }

    /// Return the exact original file bytes, including headers and opaque data.
    pub fn reassemble(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

fn checked_range(
    region: &'static str,
    start: usize,
    len: usize,
    total: usize,
) -> Result<Range<usize>, DisasmError> {
    let end = start
        .checked_add(len)
        .ok_or_else(|| DisasmError::InvalidInesHeader {
            reason: format!("{region} size overflows"),
        })?;
    if end > total {
        return Err(DisasmError::TruncatedRegion {
            region,
            needed: end,
            available: total,
        });
    }
    Ok(start..end)
}

fn rom_size(
    low: u8,
    high_nibble: u8,
    unit: usize,
    format: InesFormat,
) -> Result<usize, DisasmError> {
    if matches!(format, InesFormat::Nes2) && high_nibble == 0x0f {
        let exponent = u32::from(low >> 2);
        let multiplier = usize::from((low & 0x03) * 2 + 1);
        return 1usize
            .checked_shl(exponent)
            .and_then(|size| size.checked_mul(multiplier))
            .ok_or_else(|| DisasmError::InvalidInesHeader {
                reason: "NES 2.0 exponent/multiplier ROM size overflows".into(),
            });
    }
    usize::from(low)
        .checked_add(if matches!(format, InesFormat::Nes2) {
            usize::from(high_nibble) << 8
        } else {
            0
        })
        .and_then(|banks| banks.checked_mul(unit))
        .ok_or_else(|| DisasmError::InvalidInesHeader {
            reason: "ROM size overflows".into(),
        })
}

fn nes2_ram_size(nibble: u8) -> usize {
    if nibble == 0 {
        0
    } else {
        64usize << nibble
    }
}

pub struct NromMapping<'a> {
    image: &'a InesImage,
}

impl<'a> NromMapping<'a> {
    pub fn new(image: &'a InesImage) -> Result<Self, DisasmError> {
        if image.header.mapper != 0 {
            return Err(DisasmError::UnsupportedMapper {
                mapper: image.header.mapper,
            });
        }
        if !matches!(image.header.prg_rom_size, 0x4000 | 0x8000) {
            return Err(DisasmError::InvalidInesHeader {
                reason: "NROM PRG must be 16 KiB or 32 KiB".into(),
            });
        }
        Ok(Self { image })
    }
    pub fn image(&self) -> &'a InesImage {
        self.image
    }
    pub fn cpu_to_prg_offset(&self, address: u16) -> Result<usize, DisasmError> {
        if address < 0x8000 {
            return Err(DisasmError::MappingOutOfRange { address });
        }
        let offset = if self.image.header.prg_rom_size == 0x4000 {
            usize::from(address.wrapping_sub(0x8000)) & 0x3fff
        } else {
            usize::from(address - 0x8000)
        };
        Ok(offset)
    }
    pub fn cpu_to_file_offset(&self, address: u16) -> Result<usize, DisasmError> {
        Ok(self.image.prg.start + self.cpu_to_prg_offset(address)?)
    }
    pub fn byte(&self, address: u16) -> Result<u8, DisasmError> {
        let offset = self.cpu_to_file_offset(address)?;
        self.image
            .bytes
            .get(offset)
            .copied()
            .ok_or(DisasmError::MappingOutOfRange { address })
    }
    pub fn read(&self, start: u16, len: usize) -> Result<Vec<u8>, DisasmError> {
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let address = usize::from(start)
                .checked_add(i)
                .ok_or(DisasmError::MappingOutOfRange { address: start })?;
            if address > 0xffff {
                return Err(DisasmError::MappingOutOfRange { address: start });
            }
            out.push(self.byte(address as u16)?);
        }
        Ok(out)
    }
    pub fn vectors(&self) -> Result<[u16; 3], DisasmError> {
        Ok([
            u16::from_le_bytes([self.byte(0xfffa)?, self.byte(0xfffb)?]),
            u16::from_le_bytes([self.byte(0xfffc)?, self.byte(0xfffd)?]),
            u16::from_le_bytes([self.byte(0xfffe)?, self.byte(0xffff)?]),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_16k_prg_and_preserves_file() {
        let mut rom = vec![0; 16 + 0x4000];
        rom[..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1;
        rom[16] = 0xa9;
        let image = InesImage::parse(&rom).unwrap();
        let map = image.nrom().unwrap();
        assert_eq!(map.byte(0x8000).unwrap(), 0xa9);
        assert_eq!(map.cpu_to_file_offset(0xc000).unwrap(), 16);
        assert_eq!(image.bytes, rom);
    }
}
