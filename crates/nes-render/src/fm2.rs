//! Parsing for the text form of FCEUX version-3 `.fm2` movie files.
//!
//! The exporter intentionally supports the useful common subset for NES TAS
//! movies: power-on movies with a single standard gamepad on port 0.

use nes_core::controller::{Button, ButtonState};
use std::{error::Error, fmt, fs, path::Path};

const NTSC_NUMERATOR: u64 = 1_008_307_711;
const PAL_NUMERATOR: u64 = 838_977_920;
const FRAME_RATE_DENOMINATOR: u64 = 256 * 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRate {
    pub numerator: u64,
    pub denominator: u64,
}

impl FrameRate {
    pub const NTSC: Self = Self {
        numerator: NTSC_NUMERATOR,
        denominator: FRAME_RATE_DENOMINATOR,
    };

    pub const PAL: Self = Self {
        numerator: PAL_NUMERATOR,
        denominator: FRAME_RATE_DENOMINATOR,
    };

    pub const FPS_60: Self = Self {
        numerator: 60,
        denominator: 1,
    };

    pub const FPS_50: Self = Self {
        numerator: 50,
        denominator: 1,
    };

    pub fn as_ffmpeg_value(self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fm2Frame {
    pub commands: u32,
    pub buttons: ButtonState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fm2Movie {
    pub version: i32,
    pub emulator_version: String,
    pub rom_filename: String,
    pub rom_checksum: Option<String>,
    pub frame_rate: FrameRate,
    pub frames: Vec<Fm2Frame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fm2Error {
    message: String,
}

impl Fm2Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Fm2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for Fm2Error {}

impl Fm2Movie {
    pub fn read(path: &Path) -> Result<Self, Fm2Error> {
        let bytes = fs::read(path).map_err(|error| {
            Fm2Error::new(format!("failed to read {}: {error}", path.display()))
        })?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Fm2Error::new("FM2 must be UTF-8/ASCII text"))?;
        Self::parse(text)
    }

    pub fn parse(text: &str) -> Result<Self, Fm2Error> {
        let mut headers = std::collections::BTreeMap::<&str, &str>::new();
        let mut lines = text.lines();
        let first = lines
            .next()
            .ok_or_else(|| Fm2Error::new("FM2 file is empty"))?
            .trim_end_matches('\r');
        let (first_key, first_value) = parse_header_line(first)?;
        if first_key != "version" {
            return Err(Fm2Error::new("the first FM2 header must be version"));
        }
        headers.insert(first_key, first_value);

        let mut input_lines = Vec::new();
        for line in lines {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            if line.starts_with('|') {
                input_lines.push(line);
            } else {
                let (key, value) = parse_header_line(line)?;
                headers.insert(key, value);
            }
        }

        let version = parse_i32(required_header(&headers, "version")?, "version")?;
        if version != 3 {
            return Err(Fm2Error::new(format!(
                "unsupported FM2 version {version}; expected version 3"
            )));
        }

        let emulator_version = required_header(&headers, "emuVersion")?.to_owned();
        let rom_filename = required_header(&headers, "romFilename")?.to_owned();
        if rom_filename.is_empty() {
            return Err(Fm2Error::new("FM2 romFilename cannot be empty"));
        }

        let port0 = parse_i32(required_header(&headers, "port0")?, "port0")?;
        if port0 != 1 {
            return Err(Fm2Error::new(
                "FM2 port0 must be a standard gamepad (port0 1)",
            ));
        }
        let port1 = parse_i32(required_header(&headers, "port1")?, "port1")?;
        if port1 != 0 {
            return Err(Fm2Error::new(
                "only one gamepad is supported; FM2 port1 must be none (port1 0)",
            ));
        }
        let port2 = parse_i32(required_header(&headers, "port2")?, "port2")?;
        if port2 != 0 {
            return Err(Fm2Error::new("FM2 port2 must be none (port2 0)"));
        }
        if parse_bool(headers.get("fourscore").copied(), "fourscore")? {
            return Err(Fm2Error::new("FourScore FM2 movies are not supported"));
        }
        if parse_bool(headers.get("binary").copied(), "binary")? {
            return Err(Fm2Error::new("binary FM2 input logs are not supported"));
        }
        if headers.contains_key("savestate") {
            return Err(Fm2Error::new(
                "FM2 movies that start from a savestate are not supported",
            ));
        }
        if parse_bool(headers.get("FDS").copied(), "FDS")? {
            return Err(Fm2Error::new("FDS FM2 movies are not supported"));
        }
        if parse_bool(headers.get("NewPPU").copied(), "NewPPU")? {
            return Err(Fm2Error::new(
                "FM2 movies requiring FCEUX NewPPU behavior are not supported",
            ));
        }
        if headers.get("port0").is_none() || headers.get("port1").is_none() {
            return Err(Fm2Error::new("FM2 must declare port0 and port1"));
        }

        let pal = parse_bool(headers.get("palFlag").copied(), "palFlag")?;
        if pal {
            return Err(Fm2Error::new(
                "PAL FM2 movies are not supported by the current NES timing core",
            ));
        }

        let length = headers
            .get("length")
            .map(|value| {
                let value = parse_i32(value, "length")?;
                if value < 0 {
                    return Err(Fm2Error::new("FM2 length cannot be negative"));
                }
                Ok(value as usize)
            })
            .transpose()?;

        let mut frames = Vec::with_capacity(length.unwrap_or(input_lines.len()));
        for (index, line) in input_lines.iter().enumerate() {
            if length.is_some_and(|limit| index >= limit) {
                break;
            }
            frames.push(parse_frame(line, index)?);
        }
        if let Some(length) = length {
            if frames.len() != length {
                return Err(Fm2Error::new(format!(
                    "FM2 length declares {length} frames, but only {} were found",
                    frames.len()
                )));
            }
        }

        Ok(Self {
            version,
            emulator_version,
            rom_filename,
            rom_checksum: headers.get("romChecksum").map(|value| (*value).to_owned()),
            frame_rate: if pal { FrameRate::PAL } else { FrameRate::NTSC },
            frames,
        })
    }

    /// Verify the checksum recorded by FCEUX against the exact ROM bytes
    /// supplied to the exporter. Movies without a checksum are accepted.
    pub fn verify_rom_checksum(&self, rom: &[u8]) -> Result<(), Fm2Error> {
        let Some(expected) = self.rom_checksum.as_deref() else {
            return Ok(());
        };
        let digest = md5_digest(rom);
        let decoded = decode_base64(expected)
            .ok_or_else(|| Fm2Error::new("FM2 romChecksum is not valid base64"))?;
        let hex = digest
            .iter()
            .flat_map(|byte| format!("{byte:02x}").into_bytes())
            .collect::<Vec<_>>();
        let matches = decoded.len() == hex.len()
            && decoded
                .iter()
                .zip(&hex)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected));
        if matches {
            Ok(())
        } else {
            Err(Fm2Error::new(
                "ROM checksum does not match the FM2 romChecksum",
            ))
        }
    }
}

fn md5_digest(input: &[u8]) -> [u8; 16] {
    const SHIFT: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76a_a478,
        0xe8c7_b756,
        0x2420_70db,
        0xc1bd_ceee,
        0xf57c_0faf,
        0x4787_c62a,
        0xa830_4613,
        0xfd46_9501,
        0x6980_98d8,
        0x8b44_f7af,
        0xffff_5bb1,
        0x895c_d7be,
        0x6b90_1122,
        0xfd98_7193,
        0xa679_438e,
        0x49b4_0821,
        0xf61e_2562,
        0xc040_b340,
        0x265e_5a51,
        0xe9b6_c7aa,
        0xd62f_105d,
        0x0244_1453,
        0xd8a1_e681,
        0xe7d3_fbc8,
        0x21e1_cde6,
        0xc337_07d6,
        0xf4d5_0d87,
        0x455a_14ed,
        0xa9e3_e905,
        0xfcef_a3f8,
        0x676f_02d9,
        0x8d2a_4c8a,
        0xfffa_3942,
        0x8771_f681,
        0x6d9d_6122,
        0xfde5_380c,
        0xa4be_ea44,
        0x4bde_cfa9,
        0xf6bb_4b60,
        0xbebf_bc70,
        0x289b_7ec6,
        0xeaa1_27fa,
        0xd4ef_3085,
        0x0488_1d05,
        0xd9d4_d039,
        0xe6db_99e5,
        0x1fa2_7cf8,
        0xc4ac_5665,
        0xf429_2244,
        0x432a_ff97,
        0xab94_23a7,
        0xfc93_a039,
        0x655b_59c3,
        0x8f0c_cc92,
        0xffef_f47d,
        0x8584_5dd1,
        0x6fa8_7e4f,
        0xfe2c_e6e0,
        0xa301_4314,
        0x4e08_11a1,
        0xf753_7e82,
        0xbd3a_f235,
        0x2ad7_d2bb,
        0xeb86_d391,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let padded_len = (input.len() + 9).next_multiple_of(64);
    let mut padded = vec![0u8; padded_len];
    padded[..input.len()].copy_from_slice(input);
    padded[input.len()] = 0x80;
    padded[padded_len - 8..].copy_from_slice(&bit_len.to_le_bytes());

    let mut a0 = 0x6745_2301u32;
    let mut b0 = 0xefcd_ab89u32;
    let mut c0 = 0x98ba_dcfeu32;
    let mut d0 = 0x1032_5476u32;

    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 16];
        for (word, bytes) in words.iter_mut().zip(chunk.chunks_exact(4)) {
            *word = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (mut f, g) = if i < 16 {
                ((b & c) | ((!b) & d), i)
            } else if i < 32 {
                ((d & b) | ((!d) & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(words[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(SHIFT[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..].copy_from_slice(&d0.to_le_bytes());
    digest
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = value
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'\r' | b'\n' | b'\t'))
        .collect();
    if clean.is_empty() || clean.len() % 4 != 0 {
        return None;
    }
    let value_of = |byte: u8| -> Option<u8> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut output = Vec::with_capacity(clean.len() / 4 * 3);
    for (chunk_index, chunk) in clean.chunks_exact(4).enumerate() {
        let last = chunk_index + 1 == clean.len() / 4;
        let first = value_of(chunk[0])?;
        let second = value_of(chunk[1])?;
        let third = if chunk[2] == b'=' {
            None
        } else {
            Some(value_of(chunk[2])?)
        };
        let fourth = if chunk[3] == b'=' {
            None
        } else {
            Some(value_of(chunk[3])?)
        };
        if (!last && (third.is_none() || fourth.is_none())) || (third.is_none() && fourth.is_some())
        {
            return None;
        }
        if third.is_none() && second & 0x0f != 0 {
            return None;
        }
        if let Some(third) = third {
            if fourth.is_none() && third & 0x03 != 0 {
                return None;
            }
            output.push((first << 2) | (second >> 4));
            output.push((second << 4) | (third >> 2));
            if let Some(fourth) = fourth {
                output.push((third << 6) | fourth);
            }
        } else {
            output.push(first << 2 | second >> 4);
        }
    }
    Some(output)
}

fn parse_header_line(line: &str) -> Result<(&str, &str), Fm2Error> {
    let Some((key, value)) = line.split_once(' ') else {
        return Err(Fm2Error::new(format!("invalid FM2 header line: {line}")));
    };
    if key.is_empty() {
        return Err(Fm2Error::new(format!("invalid FM2 header line: {line}")));
    }
    Ok((key, value))
}

fn required_header<'a>(
    headers: &'a std::collections::BTreeMap<&str, &str>,
    key: &str,
) -> Result<&'a str, Fm2Error> {
    headers
        .get(key)
        .copied()
        .ok_or_else(|| Fm2Error::new(format!("FM2 is missing required header {key}")))
}

fn parse_i32(value: &str, key: &str) -> Result<i32, Fm2Error> {
    value
        .parse()
        .map_err(|_| Fm2Error::new(format!("FM2 header {key} must be an integer")))
}

fn parse_bool(value: Option<&str>, key: &str) -> Result<bool, Fm2Error> {
    let Some(value) = value else { return Ok(false) };
    match parse_i32(value, key)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Fm2Error::new(format!("FM2 header {key} must be 0 or 1"))),
    }
}

fn parse_frame(line: &str, index: usize) -> Result<Fm2Frame, Fm2Error> {
    let fields: Vec<&str> = line.split('|').collect();
    if fields.first() != Some(&"") || fields.last() != Some(&"") {
        return Err(Fm2Error::new(format!(
            "FM2 frame {} must begin and end with |",
            index
        )));
    }
    let fields = &fields[1..fields.len() - 1];
    if fields.len() != 4 {
        return Err(Fm2Error::new(format!(
            "FM2 frame {} has {} fields; expected 4",
            index,
            fields.len()
        )));
    }

    let commands = fields[0]
        .parse::<u32>()
        .map_err(|_| Fm2Error::new(format!("FM2 frame {} has invalid command bits", index)))?;
    if commands != 0 {
        return Err(Fm2Error::new(format!(
            "FM2 frame {} contains unsupported reset/peripheral commands 0x{commands:x}",
            index
        )));
    }
    let buttons = parse_gamepad(fields[1], index)?;
    if !fields[2].is_empty() {
        return Err(Fm2Error::new(format!(
            "FM2 frame {} contains unsupported port 1 input",
            index
        )));
    }
    if !fields[3].is_empty() {
        return Err(Fm2Error::new(format!(
            "FM2 frame {} contains unsupported port 2 input",
            index
        )));
    }

    Ok(Fm2Frame { commands, buttons })
}

fn parse_gamepad(value: &str, index: usize) -> Result<ButtonState, Fm2Error> {
    let characters: Vec<char> = value.chars().collect();
    if characters.len() != 8 {
        return Err(Fm2Error::new(format!(
            "FM2 frame {} gamepad field must contain 8 characters",
            index
        )));
    }
    let mut buttons = ButtonState::default();
    let mapping = [
        Button::Right,
        Button::Left,
        Button::Down,
        Button::Up,
        Button::Start,
        Button::Select,
        Button::B,
        Button::A,
    ];
    for (character, button) in characters.into_iter().zip(mapping) {
        if character != '.' && character != ' ' {
            buttons.set(button);
        }
    }
    Ok(buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(input: &str) -> String {
        format!(
            "version 3\nemuVersion 2.6.0\nromFilename smb.nes\nromChecksum ignored\nport0 1\nport1 0\nport2 0\nlength 2\n{input}"
        )
    }

    #[test]
    fn parses_controller_columns_and_moonwalk_input() {
        let parsed = Fm2Movie::parse(&movie("|0|RL......|||\n|0|........|||\n")).unwrap();
        assert_eq!(parsed.frames.len(), 2);
        assert_eq!(parsed.frames[0].buttons.bits(), 0b1100_0000);
        assert_eq!(parsed.frames[1].buttons.bits(), 0);
    }

    #[test]
    fn honors_declared_length() {
        let parsed = Fm2Movie::parse(&movie("|0|........|||\n|0|........|||\n")).unwrap();
        assert_eq!(parsed.frames.len(), 2);
    }

    #[test]
    fn accepts_crlf_text_and_rejects_missing_port_fields() {
        let parsed =
            Fm2Movie::parse(&movie("|0|........|||\n|0|........|||\n").replace('\n', "\r\n"))
                .unwrap();
        assert_eq!(parsed.frames.len(), 2);
        assert!(Fm2Movie::parse(&movie("|0|........||\n|0|........|||\n")).is_err());
    }

    #[test]
    fn rejects_nonzero_commands() {
        let text = movie("|2|........|||\n|0|........|||\n");
        let error = Fm2Movie::parse(&text).unwrap_err();
        assert!(error.to_string().contains("commands"));
    }

    #[test]
    fn rejects_binary_and_pal_movies() {
        let binary = movie("|0|........|||\n").replace("romChecksum ignored", "binary 1");
        assert!(Fm2Movie::parse(&binary).is_err());

        let pal = movie("|0|........|||\n").replace("romChecksum ignored", "palFlag 1");
        assert!(Fm2Movie::parse(&pal).is_err());
    }

    #[test]
    fn verifies_fceux_style_md5_checksum() {
        let text = movie("|0|........|||\n|0|........|||\n");
        let mut parsed = Fm2Movie::parse(&text).unwrap();
        parsed.rom_checksum = Some("OTAwMTUwOTgzY2QyNGZiMGQ2OTYzZjdkMjhlMTdmNzI=".to_owned());
        assert!(parsed.verify_rom_checksum(b"abc").is_ok());
        assert!(parsed.verify_rom_checksum(b"abd").is_err());
    }
}
