//! Backend-neutral video output types and the standard NES palette.

/// Width of a rendered NES frame, in pixels.
pub const FRAME_WIDTH: usize = 256;
/// Height of a rendered NES frame, in pixels.
pub const FRAME_HEIGHT: usize = 240;
/// Number of palette-indexed pixels in one frame.
pub const FRAME_PIXELS: usize = FRAME_WIDTH * FRAME_HEIGHT;
/// Number of bytes in one RGBA frame.
pub const FRAME_RGBA_BYTES: usize = FRAME_PIXELS * 4;
/// Number of entries in the NES master palette.
pub const PALETTE_ENTRIES: usize = 64;

/// The commonly used NES master palette, packed as `0x00RRGGBB` values.
///
/// Palette indices emitted by the PPU address this table directly. The
/// leading zero makes the values convenient for native pixel surfaces while
/// [`VideoBuffer::update_rgba`] expands them to browser-friendly RGBA bytes.
pub const NES_PALETTE_RGB: [u32; PALETTE_ENTRIES] = [
    0x666666, 0x002A88, 0x1412A7, 0x3B00A4, 0x5C007E, 0x6E0040, 0x6C0600, 0x561D00, 0x333500,
    0x0B4800, 0x005200, 0x004F08, 0x00404D, 0x000000, 0x000000, 0x000000, 0xADADAD, 0x155FD9,
    0x4240FF, 0x7527FE, 0xA01ACC, 0xB71E7B, 0xB53120, 0x994E00, 0x6B6D00, 0x388700, 0x0C9300,
    0x008F32, 0x007C8D, 0x000000, 0x000000, 0x000000, 0xFFFEFF, 0x64B0FF, 0x9290FF, 0xC676FF,
    0xF36AFF, 0xFE6ECC, 0xFE8170, 0xEA9E22, 0xBCBE00, 0x88D800, 0x5CE430, 0x45E082, 0x48CDDE,
    0x4F4F4F, 0x000000, 0x000000, 0xFFFEFF, 0xC0DFFF, 0xD3D2FF, 0xE8C8FF, 0xFBC2FF, 0xFEC4EA,
    0xFECCC5, 0xF7D8A5, 0xE4E594, 0xCFEF96, 0xBDF4AB, 0xB3F3CC, 0xB5EBF2, 0xB8B8B8, 0x000000,
    0x000000,
];

const fn rgba_palette() -> [[u8; 4]; PALETTE_ENTRIES] {
    let mut palette = [[0; 4]; PALETTE_ENTRIES];
    let mut index = 0;
    while index < PALETTE_ENTRIES {
        let [_, red, green, blue] = NES_PALETTE_RGB[index].to_be_bytes();
        palette[index] = [red, green, blue, 0xff];
        index += 1;
    }
    palette
}

/// The shared NES palette in browser/native RGBA byte order.
pub const NES_PALETTE_RGBA: [[u8; 4]; PALETTE_ENTRIES] = rgba_palette();

/// An owned, reusable, flat frame buffer for palette-indexed and RGBA output.
///
/// The palette-indexed buffer is the emulator's native video representation;
/// the RGBA buffer is expanded only when [`update_rgba`](Self::update_rgba)
/// is called. This lets a frontend allocate once and reuse the same buffers
/// for every frame. The accessors borrow those owned allocations; the slices
/// remain valid until the next mutable operation on this buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoBuffer {
    palette_indices: Vec<u8>,
    rgba: Vec<u8>,
}

/// Compatibility-friendly name for an owned video frame buffer.
pub type FrameBuffer = VideoBuffer;

impl Default for VideoBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoBuffer {
    /// Allocate zeroed palette-indexed and RGBA frame buffers.
    pub fn new() -> Self {
        let mut buffer = Self {
            palette_indices: vec![0; FRAME_PIXELS],
            rgba: vec![0; FRAME_RGBA_BYTES],
        };
        buffer.update_rgba();
        buffer
    }

    /// Return the flat palette-indexed frame.
    pub fn palette_indices(&self) -> &[u8] {
        &self.palette_indices
    }

    /// Return the mutable flat palette-indexed frame.
    ///
    /// Call [`update_rgba`](Self::update_rgba) after modifying this slice if
    /// the RGBA view is also needed.
    pub fn palette_indices_mut(&mut self) -> &mut [u8] {
        &mut self.palette_indices
    }

    /// Return the flat RGBA frame in row-major order.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Replace the indexed frame and refresh the RGBA view.
    ///
    /// The input must contain exactly [`FRAME_PIXELS`] bytes.
    pub fn set_palette_indices(&mut self, pixels: &[u8]) {
        assert_eq!(pixels.len(), FRAME_PIXELS, "invalid NES frame size");
        self.palette_indices.copy_from_slice(pixels);
        self.update_rgba();
    }

    /// Construct a buffer from one flat palette-indexed frame.
    pub fn from_palette_indices(pixels: &[u8]) -> Self {
        let mut buffer = Self::new();
        buffer.set_palette_indices(pixels);
        buffer
    }

    /// Copy a nested PPU-style frame into the reusable flat buffers.
    pub fn set_screen(&mut self, pixels: &[[u8; FRAME_WIDTH]; FRAME_HEIGHT]) {
        for (destination, row) in self
            .palette_indices
            .chunks_exact_mut(FRAME_WIDTH)
            .zip(pixels)
        {
            destination.copy_from_slice(row);
        }
        self.update_rgba();
    }

    /// Expand the current palette-indexed frame into RGBA bytes.
    pub fn update_rgba(&mut self) {
        expand_rgba(&self.palette_indices, &mut self.rgba);
    }

    /// Clear both views to palette index zero / opaque black.
    pub fn clear(&mut self) {
        self.palette_indices.fill(0);
        self.update_rgba();
    }
}

/// Expand a flat palette-indexed frame into a flat RGBA frame.
///
/// Both slices must describe exactly one NES frame. Palette indices are
/// masked to six bits, matching the PPU's 64-entry palette address space.
pub fn expand_rgba(palette_indices: &[u8], rgba: &mut [u8]) {
    assert_eq!(
        palette_indices.len(),
        FRAME_PIXELS,
        "invalid NES frame size"
    );
    assert_eq!(rgba.len(), FRAME_RGBA_BYTES, "invalid RGBA frame size");

    for (palette_index, pixel) in palette_indices.iter().zip(rgba.chunks_exact_mut(4)) {
        pixel.copy_from_slice(&NES_PALETTE_RGBA[*palette_index as usize & 0x3f]);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        expand_rgba, VideoBuffer, FRAME_HEIGHT, FRAME_PIXELS, FRAME_RGBA_BYTES, FRAME_WIDTH,
    };

    #[test]
    fn dimensions_and_palette_have_expected_sizes() {
        assert_eq!(FRAME_PIXELS, FRAME_WIDTH * FRAME_HEIGHT);
        assert_eq!(FRAME_RGBA_BYTES, FRAME_PIXELS * 4);
        assert_eq!(super::NES_PALETTE_RGB.len(), 64);
        assert_eq!(super::NES_PALETTE_RGBA.len(), 64);
    }

    #[test]
    fn reusable_buffer_copies_and_expands_indexed_pixels() {
        let mut buffer = VideoBuffer::new();
        let mut pixels = vec![0; FRAME_PIXELS];
        pixels[0] = 0x3f;
        pixels[1] = 0x40; // The PPU address space wraps to palette entry zero.

        buffer.set_palette_indices(&pixels);

        assert_eq!(&buffer.palette_indices()[..2], &[0x3f, 0x40]);
        assert_eq!(&buffer.rgba()[..4], &super::NES_PALETTE_RGBA[0x3f]);
        assert_eq!(&buffer.rgba()[4..8], &super::NES_PALETTE_RGBA[0]);
    }

    #[test]
    fn expand_rgba_reuses_existing_destination() {
        let indexed = vec![0x1d; FRAME_PIXELS];
        let mut rgba = vec![0; FRAME_RGBA_BYTES];
        expand_rgba(&indexed, &mut rgba);

        assert!(rgba
            .chunks_exact(4)
            .all(|pixel| pixel == super::NES_PALETTE_RGBA[0x1d]));
    }
}
