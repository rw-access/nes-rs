//! Lossless inspection and rendering helpers for NES CHR pattern data.
//!
//! NES pattern data stores each 8x8 tile as two bit planes.  The first eight
//! bytes are plane zero and the next eight bytes are plane one.  Within a row,
//! bit seven is the leftmost pixel.  This module deliberately does not make
//! any assumptions about palettes, nametables, OAM, or whether a tile is used
//! as code: it only describes the bytes that are present and their PPU-side
//! interpretation.

use core::fmt;

/// Number of bytes in one NES 8x8, 2bpp tile.
pub const TILE_BYTES: usize = 16;
/// Width of an NES pattern tile in pixels.
pub const TILE_WIDTH: usize = 8;
/// Height of an NES pattern tile in pixels.
pub const TILE_HEIGHT: usize = 8;
/// Number of bytes in one 4 KiB pattern-table half.
pub const PATTERN_TABLE_BYTES: usize = 0x1000;
/// Number of complete 8x8 tiles in one pattern-table half.
pub const TILES_PER_PATTERN_TABLE: usize = PATTERN_TABLE_BYTES / TILE_BYTES;
/// Number of bytes occupied by one 8x16 sprite tile.
pub const TILE_8X16_BYTES: usize = TILE_BYTES * 2;

/// The two pattern tables selected by bit zero of an 8x16 sprite tile index.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PatternTable {
    /// PPU pattern-table range `$0000..$0FFF`.
    Lower,
    /// PPU pattern-table range `$1000..$1FFF`.
    Upper,
}

impl PatternTable {
    /// The byte offset of this table in a conventional 8 KiB CHR image.
    pub const fn offset(self) -> usize {
        match self {
            Self::Lower => 0,
            Self::Upper => PATTERN_TABLE_BYTES,
        }
    }

    /// The numeric table selected by the NES 8x16 tile-index bit.
    pub const fn index(self) -> u8 {
        match self {
            Self::Lower => 0,
            Self::Upper => 1,
        }
    }

    /// Convert the low bit used by the PPU into a pattern-table selection.
    pub const fn from_selector(selector: u8) -> Self {
        if selector & 1 == 0 {
            Self::Lower
        } else {
            Self::Upper
        }
    }
}

/// An error encountered while decoding or encoding CHR data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChrError {
    /// A fixed-size CHR object was shorter than required.
    Truncated {
        /// Human-readable name of the object being decoded.
        what: &'static str,
        /// Minimum number of bytes required.
        expected: usize,
        /// Number of bytes supplied.
        actual: usize,
    },
    /// A complete-tile operation received a non-multiple-of-16 byte slice.
    IncompleteTileData {
        /// Number of bytes supplied.
        actual: usize,
    },
    /// A pixel did not fit in the NES 2-bit indexed color range.
    PixelOutOfRange {
        /// Horizontal pixel coordinate.
        x: usize,
        /// Vertical pixel coordinate.
        y: usize,
        /// Invalid indexed color value.
        value: u8,
    },
    /// A renderer was given a zero or otherwise invalid dimension.
    InvalidRenderDimensions {
        /// Number of columns requested.
        columns: usize,
        /// Scale factor requested.
        scale: usize,
    },
    /// A renderer's calculated image dimensions overflowed `usize`.
    RenderDimensionsOverflow,
}

impl fmt::Display for ChrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                what,
                expected,
                actual,
            } => write!(
                f,
                "{what} is truncated: expected at least {expected} bytes, got {actual}"
            ),
            Self::IncompleteTileData { actual } => {
                write!(
                    f,
                    "CHR data has {actual} bytes, not a multiple of {TILE_BYTES}"
                )
            }
            Self::PixelOutOfRange { x, y, value } => {
                write!(f, "tile pixel ({x}, {y}) has value {value}, expected 0..=3")
            }
            Self::InvalidRenderDimensions { columns, scale } => {
                write!(
                    f,
                    "invalid render dimensions: columns={columns}, scale={scale}"
                )
            }
            Self::RenderDimensionsOverflow => write!(f, "render dimensions overflow usize"),
        }
    }
}

impl std::error::Error for ChrError {}

/// The decoded indexed pixels of one NES 8x8 tile.
///
/// Pixel values are palette indices in the range `0..=3`; they do not include
/// a universal background color or an RGB palette choice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tile {
    pixels: [[u8; TILE_WIDTH]; TILE_HEIGHT],
}

impl Tile {
    /// Construct a tile after validating all indexed pixel values.
    pub fn new(pixels: [[u8; TILE_WIDTH]; TILE_HEIGHT]) -> Result<Self, ChrError> {
        for (y, row) in pixels.iter().enumerate() {
            for (x, &value) in row.iter().enumerate() {
                if value > 3 {
                    return Err(ChrError::PixelOutOfRange { x, y, value });
                }
            }
        }
        Ok(Self { pixels })
    }

    /// Return the tile's indexed pixels in top-to-bottom, left-to-right order.
    pub const fn pixels(&self) -> &[[u8; TILE_WIDTH]; TILE_HEIGHT] {
        &self.pixels
    }

    /// Return one indexed pixel.
    pub const fn pixel(&self, x: usize, y: usize) -> u8 {
        self.pixels[y][x]
    }

    /// Decode the first 16 bytes of `bytes` as an NES 2bpp tile.
    pub fn decode(bytes: &[u8]) -> Result<Self, ChrError> {
        decode_tile(bytes)
    }

    /// Encode this tile into the NES plane-zero-then-plane-one byte layout.
    pub fn encode(&self) -> Result<[u8; TILE_BYTES], ChrError> {
        encode_tile(self)
    }
}

/// Decode one tile from the first 16 bytes of a byte slice.
pub fn decode_tile(bytes: &[u8]) -> Result<Tile, ChrError> {
    if bytes.len() < TILE_BYTES {
        return Err(ChrError::Truncated {
            what: "8x8 tile",
            expected: TILE_BYTES,
            actual: bytes.len(),
        });
    }

    let mut pixels = [[0; TILE_WIDTH]; TILE_HEIGHT];
    for y in 0..TILE_HEIGHT {
        let plane_zero = bytes[y];
        let plane_one = bytes[y + TILE_HEIGHT];
        for (x, pixel) in pixels[y].iter_mut().enumerate() {
            let bit = 7 - x;
            *pixel = ((plane_zero >> bit) & 1) | (((plane_one >> bit) & 1) << 1);
        }
    }
    // Decoding produces only values in 0..=3, so validation cannot fail here.
    Ok(Tile { pixels })
}

/// Encode one tile using NES plane-zero-then-plane-one ordering.
pub fn encode_tile(tile: &Tile) -> Result<[u8; TILE_BYTES], ChrError> {
    let mut bytes = [0; TILE_BYTES];
    for y in 0..TILE_HEIGHT {
        for x in 0..TILE_WIDTH {
            let value = tile.pixels[y][x];
            if value > 3 {
                return Err(ChrError::PixelOutOfRange { x, y, value });
            }
            let bit = 7 - x;
            bytes[y] |= (value & 1) << bit;
            bytes[y + TILE_HEIGHT] |= ((value >> 1) & 1) << bit;
        }
    }
    Ok(bytes)
}

/// Decode a byte slice containing only complete 8x8 tiles.
pub fn decode_tiles(bytes: &[u8]) -> Result<Vec<Tile>, ChrError> {
    if !bytes.len().is_multiple_of(TILE_BYTES) {
        return Err(ChrError::IncompleteTileData {
            actual: bytes.len(),
        });
    }
    bytes.chunks_exact(TILE_BYTES).map(decode_tile).collect()
}

/// Encode a sequence of complete tiles into contiguous CHR bytes.
pub fn encode_tiles(tiles: &[Tile]) -> Result<Vec<u8>, ChrError> {
    let mut bytes = Vec::with_capacity(tiles.len() * TILE_BYTES);
    for tile in tiles {
        bytes.extend_from_slice(&encode_tile(tile)?);
    }
    Ok(bytes)
}

/// A decoded tile with its stable index, byte offset, and original bytes.
///
/// Keeping `raw` beside the decoded pixels makes JSON-like consumers able to
/// show or re-emit the exact source representation without re-encoding it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileRecord {
    /// Zero-based tile index in the inspected CHR byte stream.
    pub index: usize,
    /// Byte offset from the start of the inspected CHR byte stream.
    pub offset: usize,
    /// Original 16-byte representation.
    pub raw: [u8; TILE_BYTES],
    /// Decoded 8x8 indexed pixels.
    pub tile: Tile,
}

/// One 4 KiB pattern-table half, including a partial final half when present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternTableHalf {
    /// Zero-based 4 KiB-half number in the CHR stream.
    pub index: usize,
    /// Byte offset from the start of the CHR stream.
    pub offset: usize,
    /// The pattern-table selection for the conventional first two halves.
    pub table: Option<PatternTable>,
    /// Original bytes in this half.  This can be shorter than 4 KiB at EOF.
    pub bytes: Vec<u8>,
    /// Complete decoded tiles in `bytes`.
    pub tiles: Vec<TileRecord>,
    /// Bytes after the last complete tile in this half.
    pub trailing: Vec<u8>,
}

impl PatternTableHalf {
    /// Whether this structure contains a complete 4 KiB pattern-table half.
    pub fn is_complete(&self) -> bool {
        self.bytes.len() == PATTERN_TABLE_BYTES
    }
}

/// A lossless, deterministic inspection of a CHR byte stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChrInspection {
    /// The exact source bytes, including any incomplete tile or extra data.
    bytes: Vec<u8>,
    /// Decoded complete tiles in source order.
    tiles: Vec<TileRecord>,
    /// Bytes after the last complete 8x8 tile.
    trailing: Vec<u8>,
    /// Four-KiB pattern-table halves in source order.
    pattern_tables: Vec<PatternTableHalf>,
}

impl ChrInspection {
    /// Inspect a CHR stream without discarding any bytes.
    pub fn new(bytes: &[u8]) -> Self {
        let bytes = bytes.to_vec();
        let tiles = make_tile_records(&bytes, 0);
        let trailing = bytes[tiles.len() * TILE_BYTES..].to_vec();
        let pattern_tables = bytes
            .chunks(PATTERN_TABLE_BYTES)
            .enumerate()
            .map(|(index, half)| make_pattern_table_half(index, half))
            .collect();
        Self {
            bytes,
            tiles,
            trailing,
            pattern_tables,
        }
    }

    /// Inspect a CHR stream, accepting either `&[u8]` or an owned byte vector.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::new(bytes)
    }

    /// Return the exact original bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the inspection and return the exact original bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Return all complete tiles in source order.
    pub fn tiles(&self) -> &[TileRecord] {
        &self.tiles
    }

    /// Return the trailing incomplete-tile bytes, if any.
    pub fn trailing(&self) -> &[u8] {
        &self.trailing
    }

    /// Return all 4 KiB halves, including a partial final half.
    pub fn pattern_tables(&self) -> &[PatternTableHalf] {
        &self.pattern_tables
    }

    /// Return a complete tile record by global tile index.
    pub fn tile(&self, index: usize) -> Option<&TileRecord> {
        self.tiles.get(index)
    }

    /// Re-emit the original data.  This is intentionally not an encode/decode
    /// reconstruction, so non-tile-aligned bytes remain byte-for-byte intact.
    pub fn reassemble(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

impl From<&[u8]> for ChrInspection {
    fn from(bytes: &[u8]) -> Self {
        Self::new(bytes)
    }
}

impl From<Vec<u8>> for ChrInspection {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(&bytes)
    }
}

/// Inspect a CHR byte stream without losing incomplete trailing data.
pub fn inspect_chr(bytes: &[u8]) -> ChrInspection {
    ChrInspection::new(bytes)
}

fn make_tile_records(bytes: &[u8], offset_base: usize) -> Vec<TileRecord> {
    bytes
        .chunks_exact(TILE_BYTES)
        .enumerate()
        .map(|(index, raw_slice)| {
            let mut raw = [0; TILE_BYTES];
            raw.copy_from_slice(raw_slice);
            // A complete 16-byte chunk always decodes successfully.
            let tile = decode_tile(raw_slice).expect("chunks_exact yielded a complete tile");
            TileRecord {
                index,
                offset: offset_base + index * TILE_BYTES,
                raw,
                tile,
            }
        })
        .collect()
}

fn make_pattern_table_half(index: usize, bytes: &[u8]) -> PatternTableHalf {
    let tile_bytes = bytes.len() - (bytes.len() % TILE_BYTES);
    let tiles = make_tile_records(&bytes[..tile_bytes], index * PATTERN_TABLE_BYTES);
    PatternTableHalf {
        index,
        offset: index * PATTERN_TABLE_BYTES,
        table: match index {
            0 => Some(PatternTable::Lower),
            1 => Some(PatternTable::Upper),
            _ => None,
        },
        bytes: bytes.to_vec(),
        tiles,
        trailing: bytes[tile_bytes..].to_vec(),
    }
}

/// The PPU pattern table selected by an 8x16 sprite tile index.
pub const fn pattern_table_for_8x16(tile_index: u8) -> PatternTable {
    PatternTable::from_selector(tile_index)
}

/// The first 8x8 tile index within an 8x16 sprite tile.
pub const fn top_tile_index_8x16(tile_index: u8) -> u8 {
    tile_index & 0xFE
}

/// The second 8x8 tile index within an 8x16 sprite tile.
pub const fn bottom_tile_index_8x16(tile_index: u8) -> u8 {
    top_tile_index_8x16(tile_index).wrapping_add(1)
}

/// Decoded interpretation of one NES 8x16 sprite tile index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tile8x16 {
    /// Original sprite tile index, including its pattern-table selector bit.
    pub sprite_tile_index: u8,
    /// Pattern table selected by bit zero of `sprite_tile_index`.
    pub pattern_table: PatternTable,
    /// Tile index of the upper 8x8 half within that table.
    pub top_tile_index: u8,
    /// Tile index of the lower 8x8 half within that table.
    pub bottom_tile_index: u8,
    /// Decoded upper half.
    pub top: Tile,
    /// Decoded lower half.
    pub bottom: Tile,
}

impl Tile8x16 {
    /// Return the two halves as a top-to-bottom 16-row indexed image.
    pub fn pixels(&self) -> [[u8; TILE_WIDTH]; TILE_HEIGHT * 2] {
        let mut pixels = [[0; TILE_WIDTH]; TILE_HEIGHT * 2];
        pixels[..TILE_HEIGHT].copy_from_slice(self.top.pixels());
        pixels[TILE_HEIGHT..].copy_from_slice(self.bottom.pixels());
        pixels
    }
}

/// Decode an 8x16 sprite tile from a conventional CHR address space.
///
/// In 8x16 mode, bit zero selects `$0000` or `$1000`, while bits 1..7 select
/// one of 128 pairs.  Therefore tile index `n` starts at
/// `table_offset + (n & 0xFE) * 16` and consumes two adjacent 8x8 tiles.
pub fn decode_8x16_tile(bytes: &[u8], sprite_tile_index: u8) -> Result<Tile8x16, ChrError> {
    let pattern_table = pattern_table_for_8x16(sprite_tile_index);
    let top_tile_index = top_tile_index_8x16(sprite_tile_index);
    let bottom_tile_index = bottom_tile_index_8x16(sprite_tile_index);
    let offset = pattern_table.offset() + usize::from(top_tile_index) * TILE_BYTES;
    let end = offset + TILE_8X16_BYTES;
    if bytes.len() < end {
        return Err(ChrError::Truncated {
            what: "8x16 tile",
            expected: end,
            actual: bytes.len(),
        });
    }
    Ok(Tile8x16 {
        sprite_tile_index,
        pattern_table,
        top_tile_index,
        bottom_tile_index,
        top: decode_tile(&bytes[offset..offset + TILE_BYTES])?,
        bottom: decode_tile(&bytes[offset + TILE_BYTES..end])?,
    })
}

/// Alias emphasizing that the 8x16 interpretation is for a sprite tile.
pub fn decode_8x16_sprite_tile(bytes: &[u8], sprite_tile_index: u8) -> Result<Tile8x16, ChrError> {
    decode_8x16_tile(bytes, sprite_tile_index)
}

/// Render one tile as a binary grayscale PGM (`P5`) image.
///
/// The four indexed values are emitted directly as grayscale values `0..=3`.
pub fn render_tile_pgm(tile: &Tile) -> Result<Vec<u8>, ChrError> {
    render_contact_sheet_pgm(core::slice::from_ref(tile), 1, 1)
}

/// Render one tile as a binary RGB PPM (`P6`) image using four RGB palette
/// entries indexed by the tile's 2-bit pixel values.
pub fn render_tile_ppm(tile: &Tile, palette: [[u8; 3]; 4]) -> Result<Vec<u8>, ChrError> {
    render_contact_sheet_ppm(core::slice::from_ref(tile), 1, 1, palette)
}

/// Render tiles in deterministic row-major order as a binary grayscale PGM
/// contact sheet.  Tiles are separated by one background-valued pixel.
pub fn render_contact_sheet_pgm(
    tiles: &[Tile],
    columns: usize,
    scale: usize,
) -> Result<Vec<u8>, ChrError> {
    let (width, height, rows) = sheet_dimensions(tiles.len(), columns, scale)?;
    let mut output = format!("P5\n{width} {height}\n3\n").into_bytes();
    let mut pixels = vec![
        0u8;
        width
            .checked_mul(height)
            .ok_or(ChrError::RenderDimensionsOverflow)?
    ];
    for (index, tile) in tiles.iter().enumerate() {
        let column = index % columns;
        let row = index / columns;
        blit_tile_gray(
            &mut pixels,
            width,
            tile,
            column * (TILE_WIDTH * scale + 1),
            row * (TILE_HEIGHT * scale + 1),
            scale,
        );
    }
    let _ = rows; // `rows` is part of checked dimension calculation for clarity.
    output.extend_from_slice(&pixels);
    Ok(output)
}

/// Render tiles in deterministic row-major order as a binary RGB PPM contact
/// sheet.  Tiles are separated by one background-valued pixel.
pub fn render_contact_sheet_ppm(
    tiles: &[Tile],
    columns: usize,
    scale: usize,
    palette: [[u8; 3]; 4],
) -> Result<Vec<u8>, ChrError> {
    let (width, height, rows) = sheet_dimensions(tiles.len(), columns, scale)?;
    let pixel_count = width
        .checked_mul(height)
        .ok_or(ChrError::RenderDimensionsOverflow)?;
    let byte_count = pixel_count
        .checked_mul(3)
        .ok_or(ChrError::RenderDimensionsOverflow)?;
    let mut output = format!("P6\n{width} {height}\n255\n").into_bytes();
    let mut pixels = vec![palette[0][0]; byte_count];
    for pixel in pixels.chunks_exact_mut(3) {
        pixel.copy_from_slice(&palette[0]);
    }
    for (index, tile) in tiles.iter().enumerate() {
        let column = index % columns;
        let row = index / columns;
        blit_tile_rgb(
            &mut pixels,
            width,
            tile,
            column * (TILE_WIDTH * scale + 1),
            row * (TILE_HEIGHT * scale + 1),
            scale,
            palette,
        );
    }
    let _ = rows;
    output.extend_from_slice(&pixels);
    Ok(output)
}

fn sheet_dimensions(
    tile_count: usize,
    columns: usize,
    scale: usize,
) -> Result<(usize, usize, usize), ChrError> {
    if columns == 0 || scale == 0 {
        return Err(ChrError::InvalidRenderDimensions { columns, scale });
    }
    let rows = tile_count.div_ceil(columns).max(1);
    let tile_width = TILE_WIDTH
        .checked_mul(scale)
        .ok_or(ChrError::RenderDimensionsOverflow)?;
    let tile_height = TILE_HEIGHT
        .checked_mul(scale)
        .ok_or(ChrError::RenderDimensionsOverflow)?;
    let width = columns
        .checked_mul(tile_width + 1)
        .and_then(|value| value.checked_sub(1))
        .ok_or(ChrError::RenderDimensionsOverflow)?;
    let height = rows
        .checked_mul(tile_height + 1)
        .and_then(|value| value.checked_sub(1))
        .ok_or(ChrError::RenderDimensionsOverflow)?;
    Ok((width, height, rows))
}

fn blit_tile_gray(
    output: &mut [u8],
    image_width: usize,
    tile: &Tile,
    x_origin: usize,
    y_origin: usize,
    scale: usize,
) {
    for y in 0..TILE_HEIGHT {
        for x in 0..TILE_WIDTH {
            for dy in 0..scale {
                let row = y_origin + y * scale + dy;
                let start = row * image_width + x_origin + x * scale;
                output[start..start + scale].fill(tile.pixels[y][x]);
            }
        }
    }
}

fn blit_tile_rgb(
    output: &mut [u8],
    image_width: usize,
    tile: &Tile,
    x_origin: usize,
    y_origin: usize,
    scale: usize,
    palette: [[u8; 3]; 4],
) {
    for y in 0..TILE_HEIGHT {
        for x in 0..TILE_WIDTH {
            for dy in 0..scale {
                let row = y_origin + y * scale + dy;
                let pixel_start = (row * image_width + x_origin + x * scale) * 3;
                for dx in 0..scale {
                    let start = pixel_start + dx * 3;
                    output[start..start + 3].copy_from_slice(&palette[tile.pixels[y][x] as usize]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plane_zero_then_plane_one_with_msb_on_the_left() {
        let mut raw = [0; TILE_BYTES];
        raw[0] = 0b1000_0001;
        raw[8] = 0b0100_0010;

        let tile = decode_tile(&raw).unwrap();
        assert_eq!(tile.pixels()[0], [1, 2, 0, 0, 0, 0, 2, 1]);
        assert_eq!(tile.encode().unwrap(), raw);
    }

    #[test]
    fn tile_and_stream_round_trips_are_lossless() {
        let mut pixels = [[0; TILE_WIDTH]; TILE_HEIGHT];
        for (y, row) in pixels.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = ((x + y * 3) % 4) as u8;
            }
        }
        let tile = Tile::new(pixels).unwrap();
        let encoded = encode_tile(&tile).unwrap();
        assert_eq!(decode_tile(&encoded).unwrap(), tile);

        let source = [encoded.as_slice(), &[0xAA; TILE_BYTES]].concat();
        let decoded = decode_tiles(&source).unwrap();
        assert_eq!(encode_tiles(&decoded).unwrap(), source);
    }

    #[test]
    fn inspection_preserves_trailing_bytes_and_records_pattern_halves() {
        let mut source = vec![0; PATTERN_TABLE_BYTES * 2];
        source[0] = 0x80;
        source[PATTERN_TABLE_BYTES] = 0x40;
        source.extend_from_slice(&[0xDE, 0xAD]);

        let inspection = inspect_chr(&source);
        assert_eq!(inspection.bytes(), source.as_slice());
        assert_eq!(inspection.reassemble(), source);
        assert_eq!(
            inspection.tiles().len(),
            (PATTERN_TABLE_BYTES * 2 + 3) / TILE_BYTES
        );
        assert_eq!(inspection.trailing(), &[0xDE, 0xAD]);
        assert_eq!(inspection.pattern_tables().len(), 3);
        assert_eq!(
            inspection.pattern_tables()[0].table,
            Some(PatternTable::Lower)
        );
        assert_eq!(
            inspection.pattern_tables()[1].table,
            Some(PatternTable::Upper)
        );
        assert_eq!(inspection.pattern_tables()[2].table, None);
        assert_eq!(inspection.pattern_tables()[2].trailing, vec![0xDE, 0xAD]);
    }

    #[test]
    fn decodes_8x16_tiles_using_selector_bit_and_adjacent_pair() {
        let mut source = vec![0; PATTERN_TABLE_BYTES * 2];
        let top = Tile::new([[1; TILE_WIDTH]; TILE_HEIGHT]).unwrap();
        let bottom = Tile::new([[2; TILE_WIDTH]; TILE_HEIGHT]).unwrap();
        source[0x1000 + 0x20..0x1000 + 0x30].copy_from_slice(&top.encode().unwrap());
        source[0x1000 + 0x30..0x1000 + 0x40].copy_from_slice(&bottom.encode().unwrap());

        let decoded = decode_8x16_tile(&source, 0b0000_0011).unwrap();
        assert_eq!(decoded.pattern_table, PatternTable::Upper);
        assert_eq!(decoded.top_tile_index, 2);
        assert_eq!(decoded.bottom_tile_index, 3);
        assert_eq!(decoded.top, top);
        assert_eq!(decoded.bottom, bottom);
        assert_eq!(decoded.pixels()[0], [1; TILE_WIDTH]);
        assert_eq!(decoded.pixels()[TILE_HEIGHT], [2; TILE_WIDTH]);
    }

    #[test]
    fn renderers_have_stable_headers_and_dimensions() {
        let tile = Tile::new([[3; TILE_WIDTH]; TILE_HEIGHT]).unwrap();
        let pgm = render_tile_pgm(&tile).unwrap();
        assert!(pgm.starts_with(b"P5\n8 8\n3\n"));
        assert_eq!(pgm.len(), b"P5\n8 8\n3\n".len() + 64);

        let ppm = render_contact_sheet_ppm(
            &[tile, tile],
            2,
            1,
            [[0, 1, 2], [3, 4, 5], [6, 7, 8], [9, 10, 11]],
        )
        .unwrap();
        assert!(ppm.starts_with(b"P6\n17 8\n255\n"));
    }
}
