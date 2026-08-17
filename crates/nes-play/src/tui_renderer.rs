//! Perceptual monochrome rendering for the terminal frontend.
//!
//! The renderer deliberately does not map one NES pixel to one character. It
//! first reduces the indexed frame to a small, inspectable visual field and
//! only then chooses a glyph from a restricted fill/edge grammar. This keeps
//! flat regions quiet, gives sprites a little extra visual weight, and makes
//! the Unicode output considerably less noisy than a literal pixel dump.

use nes_core::{FRAME_HEIGHT, FRAME_WIDTH, NES_PALETTE_RGB};

/// The nested frame shape used by `nes-core` and by the low-level API here.
pub type Pixels = [[u8; FRAME_WIDTH]; FRAME_HEIGHT];

/// A coverage mask has the same shape as a frame. Non-zero means covered.
pub type Coverage = [[u8; FRAME_WIDTH]; FRAME_HEIGHT];

const DEFAULT_COLUMNS: usize = 80;
const DEFAULT_ROWS: usize = 38;
const DEFAULT_HYSTERESIS: f32 = 0.11;
const EDGE_THRESHOLD: f32 = 0.18;
const QUIET_BACKGROUND_SPRITE: f32 = 0.42;
const TEXT_CONTRAST_THRESHOLD: f32 = 0.08;
const TEXT_MAX_EDGE_STRENGTH: f32 = 0.48;

/// The coarse orientation of a cell's strongest contour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeOrientation {
    #[default]
    None,
    Horizontal,
    Vertical,
    DiagonalDown,
    DiagonalUp,
}

/// A borrowed layer view accepted by [`Renderer::render_pixels`].
#[derive(Clone, Copy, Debug)]
pub struct LayerView<'a> {
    pub pixels: &'a Pixels,
    pub coverage: &'a Coverage,
}

/// Optional layer information used to preserve sprite silhouettes and
/// foreground/background boundaries.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayerData<'a> {
    pub background: Option<LayerView<'a>>,
    pub sprites: Option<LayerView<'a>>,
    pub visible_sprites: Option<&'a Coverage>,
}

impl<'a> LayerData<'a> {
    pub const fn none() -> Self {
        Self {
            background: None,
            sprites: None,
            visible_sprites: None,
        }
    }
}

/// An aspect-ratio-aware terminal image rectangle.
///
/// `columns` and `rows` are the complete terminal grid. `content_*` describe
/// the centered rectangle that receives the NES frame; the rest is padding.
/// A terminal cell is assumed to be approximately twice as tall as it is
/// wide, which is the useful approximation for Unicode shape rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalGrid {
    pub columns: usize,
    pub rows: usize,
    pub content_x: usize,
    pub content_y: usize,
    pub content_columns: usize,
    pub content_rows: usize,
}

impl TerminalGrid {
    /// Fit the NES frame in a terminal, preferring tile-aligned sampling.
    ///
    /// A terminal cell is approximately 1:2 wide-to-tall. The 64x30 profile
    /// therefore samples exactly 4x8 NES pixels per cell: two characters per
    /// 8x8 tile horizontally and one tile row vertically. The 128x60 profile
    /// doubles that resolution while preserving the same alignment. Short
    /// terminals fall back to ordinary aspect fitting rather than wasting
    /// most of the available screen on padding.
    pub fn fit(columns: usize, rows: usize) -> Self {
        let columns = columns.max(1);
        let rows = rows.max(1);

        let scale = (columns / 64).min(rows / 30);
        if scale > 0 {
            let content_columns = 64 * scale;
            let content_rows = 30 * scale;
            return Self {
                columns,
                rows,
                content_x: (columns - content_columns) / 2,
                content_y: (rows - content_rows) / 2,
                content_columns,
                content_rows,
            };
        }

        // Physical image aspect is columns / (2 * rows). The NES frame is
        // 256 / 240, so its cell-space aspect is 256 / 120.
        let target = FRAME_WIDTH as f32 / (FRAME_HEIGHT as f32 * 0.5);
        let width_from_height = ((rows as f32) * target).round() as usize;
        let height_from_width = ((columns as f32) / target).round() as usize;

        let (content_columns, content_rows) = if width_from_height <= columns {
            (width_from_height.max(1), rows)
        } else {
            (columns, height_from_width.clamp(1, rows))
        };

        Self {
            columns,
            rows,
            content_x: (columns - content_columns) / 2,
            content_y: (rows - content_rows) / 2,
            content_columns,
            content_rows,
        }
    }

    pub const fn cell_count(self) -> usize {
        self.columns * self.rows
    }

    pub const fn content_cell_count(self) -> usize {
        self.content_columns * self.content_rows
    }
}

impl Default for TerminalGrid {
    fn default() -> Self {
        Self::fit(DEFAULT_COLUMNS, DEFAULT_ROWS)
    }
}

/// Tunable parameters for the shape renderer.
#[derive(Clone, Copy, Debug)]
pub struct RendererConfig {
    pub grid: TerminalGrid,
    /// How much a small visual change must exceed before a glyph changes.
    /// Set to zero to disable temporal hysteresis.
    pub hysteresis: f32,
    /// Extra tone/contrast given to covered sprites.
    pub sprite_boost: f32,
    /// Strength of contour glyphs relative to fill glyphs.
    pub edge_boost: f32,
}

impl RendererConfig {
    pub fn for_terminal(columns: usize, rows: usize) -> Self {
        Self {
            grid: TerminalGrid::fit(columns, rows),
            hysteresis: DEFAULT_HYSTERESIS,
            sprite_boost: 0.18,
            edge_boost: 1.0,
        }
    }
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self::for_terminal(DEFAULT_COLUMNS, DEFAULT_ROWS)
    }
}

/// The perceptual information calculated for one terminal cell.
#[derive(Clone, Copy, Debug, Default)]
pub struct PerceptualCell {
    /// Tone after global frame mapping, in the range 0..=1.
    pub tone: f32,
    /// Local luminance variation, in the range 0..=1.
    pub contrast: f32,
    /// Boundary strength, including layer boundaries, in the range 0..=1.
    pub edge_strength: f32,
    pub edge_orientation: EdgeOrientation,
    /// Signed average luminance gradient, useful for corner diagnostics.
    pub gradient_x: f32,
    pub gradient_y: f32,
    /// Sprite coverage weighted toward pixels the NES compositor made visible.
    pub sprite_weight: f32,
    /// A stable coarse region key useful to diagnostic callers.
    pub region_id: u16,
}

/// The semantic family selected for a rendered cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphRole {
    #[default]
    Fill,
    Edge,
    Corner,
    Sprite,
}

/// One output terminal cell, including its perceptual data for diagnostics.
#[derive(Clone, Copy, Debug)]
pub struct RenderedCell {
    pub glyph: char,
    pub role: GlyphRole,
    pub perceptual: PerceptualCell,
}

impl RenderedCell {
    const fn blank() -> Self {
        Self {
            glyph: ' ',
            role: GlyphRole::Fill,
            perceptual: PerceptualCell {
                tone: 0.0,
                contrast: 0.0,
                edge_strength: 0.0,
                edge_orientation: EdgeOrientation::None,
                gradient_x: 0.0,
                gradient_y: 0.0,
                sprite_weight: 0.0,
                region_id: 0,
            },
        }
    }
}

/// A complete terminal-sized monochrome frame.
#[derive(Clone, Debug)]
pub struct RenderedFrame {
    pub grid: TerminalGrid,
    pub cells: Vec<RenderedCell>,
}

impl RenderedFrame {
    pub fn cell(&self, x: usize, y: usize) -> Option<&RenderedCell> {
        (x < self.grid.columns && y < self.grid.rows)
            .then(|| &self.cells[y * self.grid.columns + x])
    }

    pub fn rows(&self) -> impl Iterator<Item = &[RenderedCell]> {
        self.cells.chunks(self.grid.columns)
    }

    /// Return the glyph image without a trailing newline.
    pub fn text(&self) -> String {
        let mut output = String::with_capacity(self.grid.cell_count() + self.grid.rows);
        for (y, row) in self.rows().enumerate() {
            if y != 0 {
                output.push('\n');
            }
            for cell in row {
                output.push(cell.glyph);
            }
        }
        output
    }
}

/// Stateful perceptual renderer. Reuse it for consecutive frames to get
/// temporal hysteresis; create a new one when starting a new scene.
#[derive(Clone, Debug)]
pub struct Renderer {
    config: RendererConfig,
    previous: Vec<RenderedCell>,
}

impl Renderer {
    pub fn new(config: RendererConfig) -> Self {
        Self {
            previous: vec![RenderedCell::blank(); config.grid.cell_count()],
            config,
        }
    }

    pub fn for_terminal(columns: usize, rows: usize) -> Self {
        Self::new(RendererConfig::for_terminal(columns, rows))
    }

    pub fn config(&self) -> RendererConfig {
        self.config
    }

    /// Change the terminal grid and clear temporal history.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        self.config.grid = TerminalGrid::fit(columns, rows);
        self.previous = vec![RenderedCell::blank(); self.config.grid.cell_count()];
    }

    pub fn reset_history(&mut self) {
        self.previous.fill(RenderedCell::blank());
    }

    /// Render a palette-indexed NES frame with optional PPU layer data.
    pub fn render_pixels(&mut self, pixels: &Pixels, layers: LayerData<'_>) -> RenderedFrame {
        let grid = self.config.grid;
        let mut cells = vec![RenderedCell::blank(); grid.cell_count()];
        let mut perceptual = vec![PerceptualCell::default(); grid.cell_count()];

        for y in 0..grid.content_rows {
            for x in 0..grid.content_columns {
                let index = (grid.content_y + y) * grid.columns + grid.content_x + x;
                perceptual[index] = analyze_cell(pixels, layers, grid, x, y);
            }
        }

        tone_map(&mut perceptual, grid);
        for y in 0..grid.content_rows {
            for x in 0..grid.content_columns {
                let index = (grid.content_y + y) * grid.columns + grid.content_x + x;
                let candidate = select_cell(perceptual[index], self.config);
                cells[index] = stabilize(candidate, self.previous[index], self.config.hysteresis);
            }
        }

        self.previous = cells.clone();
        RenderedFrame { grid, cells }
    }

    /// Render the borrowed `FrameOutput` exposed by `nes-core` when layered
    /// rendering is enabled. This is the adapter a TUI binary normally uses.
    #[cfg(feature = "layered-render")]
    pub fn render_frame(&mut self, frame: &nes_core::FrameOutput<'_>) -> RenderedFrame {
        let layers = LayerData {
            background: Some(LayerView {
                pixels: &frame.layers.background.pixels,
                coverage: &frame.layers.background.coverage,
            }),
            sprites: Some(LayerView {
                pixels: &frame.layers.sprites.pixels,
                coverage: &frame.layers.sprites.coverage,
            }),
            visible_sprites: Some(&frame.layers.visible_sprites),
        };
        self.render_pixels(frame.pixels, layers)
    }
}

/// Convert an NES palette entry into perceptual luma in the range 0..=1.
pub fn palette_luminance(index: u8) -> f32 {
    let [_, r, g, b] = NES_PALETTE_RGB[(index as usize) & 0x3f].to_be_bytes();
    // Decode to linear light before applying the Rec. 709 luminance weights.
    fn linear(channel: u8) -> f32 {
        let value = channel as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    (0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)).sqrt()
}

fn source_luma(pixels: &Pixels, x: usize, y: usize) -> f32 {
    palette_luminance(pixels[y.min(FRAME_HEIGHT - 1)][x.min(FRAME_WIDTH - 1)])
}

fn analyze_cell(
    pixels: &Pixels,
    layers: LayerData<'_>,
    grid: TerminalGrid,
    cell_x: usize,
    cell_y: usize,
) -> PerceptualCell {
    let x0 = cell_x * FRAME_WIDTH / grid.content_columns;
    let x1 = ((cell_x + 1) * FRAME_WIDTH / grid.content_columns).max(x0 + 1);
    let y0 = cell_y * FRAME_HEIGHT / grid.content_rows;
    let y1 = ((cell_y + 1) * FRAME_HEIGHT / grid.content_rows).max(y0 + 1);
    let mut tone = 0.0;
    let mut square = 0.0;
    let mut gradient_x = 0.0;
    let mut gradient_y = 0.0;
    let mut edge = 0.0;
    let mut sprite = 0.0;
    let mut layer_edge = 0.0;
    let mut count: f32 = 0.0;

    for y in y0..y1.min(FRAME_HEIGHT) {
        for x in x0..x1.min(FRAME_WIDTH) {
            let value = source_luma(pixels, x, y);
            tone += value;
            square += value * value;
            let left = source_luma(pixels, x.saturating_sub(1), y);
            let right = source_luma(pixels, (x + 1).min(FRAME_WIDTH - 1), y);
            let top = source_luma(pixels, x, y.saturating_sub(1));
            let bottom = source_luma(pixels, x, (y + 1).min(FRAME_HEIGHT - 1));
            let dx = (right - left) * 0.5;
            let dy = (bottom - top) * 0.5;
            gradient_x += dx;
            gradient_y += dy;
            edge += (dx * dx + dy * dy).sqrt();

            if let Some(sprite_layer) = layers.sprites {
                if sprite_layer.coverage[y][x] != 0 {
                    let visible = layers
                        .visible_sprites
                        .map(|mask| mask[y][x] != 0)
                        .unwrap_or(true);
                    sprite += if visible { 1.0 } else { 0.55 };
                    if sprite_boundary(sprite_layer.coverage, x, y) {
                        if let Some(background) = layers.background {
                            layer_edge += (palette_luminance(sprite_layer.pixels[y][x])
                                - palette_luminance(background.pixels[y][x]))
                            .abs();
                        } else {
                            layer_edge += 0.35;
                        }
                    }
                }
            }
            count += 1.0;
        }
    }

    let tone = tone / count.max(1.0);
    let variance = (square / count.max(1.0) - tone * tone).max(0.0);
    let sprite_weight = (sprite / count.max(1.0)).clamp(0.0, 1.0);
    let edge_strength =
        (edge / count.max(1.0) * 3.2 + layer_edge / count.max(1.0) * 1.6).clamp(0.0, 1.0);
    let average_gradient_x = gradient_x / count.max(1.0);
    let average_gradient_y = gradient_y / count.max(1.0);
    let orientation = orientation(gradient_x, gradient_y, edge_strength);

    PerceptualCell {
        tone: tone.clamp(0.0, 1.0),
        contrast: variance.sqrt().clamp(0.0, 1.0),
        edge_strength,
        edge_orientation: orientation,
        gradient_x: average_gradient_x,
        gradient_y: average_gradient_y,
        sprite_weight,
        region_id: ((tone * 31.0).round() as u16) | ((sprite_weight > 0.2) as u16 * 0x100),
    }
}

fn sprite_boundary(coverage: &Coverage, x: usize, y: usize) -> bool {
    let here = coverage[y][x] != 0;
    if !here {
        return false;
    }
    let neighbors = [
        (x.saturating_sub(1), y),
        ((x + 1).min(FRAME_WIDTH - 1), y),
        (x, y.saturating_sub(1)),
        (x, (y + 1).min(FRAME_HEIGHT - 1)),
    ];
    neighbors.iter().any(|&(nx, ny)| coverage[ny][nx] == 0)
}

fn orientation(dx: f32, dy: f32, edge_strength: f32) -> EdgeOrientation {
    if edge_strength < EDGE_THRESHOLD {
        return EdgeOrientation::None;
    }
    let ax = dx.abs();
    let ay = dy.abs();
    if ax > ay * 1.35 {
        EdgeOrientation::Vertical
    } else if ay > ax * 1.35 {
        EdgeOrientation::Horizontal
    } else if dx * dy >= 0.0 {
        EdgeOrientation::DiagonalDown
    } else {
        EdgeOrientation::DiagonalUp
    }
}

fn tone_map(cells: &mut [PerceptualCell], grid: TerminalGrid) {
    let mut values = Vec::with_capacity(grid.content_cell_count());
    for y in grid.content_y..grid.content_y + grid.content_rows {
        for x in grid.content_x..grid.content_x + grid.content_columns {
            values.push(cells[y * grid.columns + x].tone);
        }
    }
    if values.is_empty() {
        return;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let low = values[values.len() / 20];
    let high = values[values.len() * 19 / 20];
    let span = (high - low).max(0.08);
    for y in grid.content_y..grid.content_y + grid.content_rows {
        for x in grid.content_x..grid.content_x + grid.content_columns {
            let cell = &mut cells[y * grid.columns + x];
            cell.tone = ((cell.tone - low) / span).clamp(0.0, 1.0).powf(0.9);
            cell.tone = (cell.tone + cell.sprite_weight * 0.10).clamp(0.0, 1.0);
        }
    }
}

fn select_cell(cell: PerceptualCell, config: RendererConfig) -> RenderedCell {
    let mut tone = (cell.tone + cell.sprite_weight * config.sprite_boost).clamp(0.0, 1.0);
    tone = (tone + cell.contrast * 0.04).clamp(0.0, 1.0);
    let edge = (cell.edge_strength * config.edge_boost).clamp(0.0, 1.0);
    // Text has strong local contrast, but its opposing strokes usually cancel
    // into a modest aggregate edge. Dense tile texture tends to have both
    // contrast and a much larger edge sum, so keep that out of the text path.
    let text_like =
        cell.contrast >= TEXT_CONTRAST_THRESHOLD && cell.edge_strength <= TEXT_MAX_EDGE_STRENGTH;
    let (glyph, role) =
        if edge < EDGE_THRESHOLD && cell.sprite_weight < QUIET_BACKGROUND_SPRITE && !text_like {
            // A terminal cell can always fall back to a blank. Preserve visual
            // hierarchy by reserving dense fill glyphs for texture, sprites, and
            // other regions that actually carry local information.
            (' ', GlyphRole::Fill)
        } else if edge >= EDGE_THRESHOLD {
            match cell.edge_orientation {
                EdgeOrientation::Horizontal => ('─', GlyphRole::Edge),
                EdgeOrientation::Vertical => ('│', GlyphRole::Edge),
                EdgeOrientation::DiagonalDown | EdgeOrientation::DiagonalUp
                    if edge > 0.40 && cell.contrast > 0.12 =>
                {
                    (
                        corner_glyph(cell.gradient_x, cell.gradient_y),
                        GlyphRole::Corner,
                    )
                }
                EdgeOrientation::DiagonalDown => ('╲', GlyphRole::Edge),
                EdgeOrientation::DiagonalUp => ('╱', GlyphRole::Edge),
                // High local texture without a coherent direction is noise, not
                // a contour. Preserve high-contrast text-like cells with a
                // density glyph; only low-contrast texture stays blank.
                EdgeOrientation::None if text_like => {
                    (fill_glyph((tone + 0.18).min(1.0)), GlyphRole::Fill)
                }
                EdgeOrientation::None => (' ', GlyphRole::Fill),
            }
        } else if text_like && cell.sprite_weight < QUIET_BACKGROUND_SPRITE {
            (fill_glyph((tone + 0.18).min(1.0)), GlyphRole::Fill)
        } else if cell.sprite_weight > 0.42 && tone > 0.18 {
            (fill_glyph((tone + 0.12).min(1.0)), GlyphRole::Sprite)
        } else {
            (fill_glyph(tone), GlyphRole::Fill)
        };
    RenderedCell {
        glyph,
        role,
        perceptual: PerceptualCell { tone, ..cell },
    }
}

fn corner_glyph(gradient_x: f32, gradient_y: f32) -> char {
    match (gradient_x >= 0.0, gradient_y >= 0.0) {
        (true, true) => '┌',
        (false, true) => '┐',
        (true, false) => '└',
        (false, false) => '┘',
    }
}

fn fill_glyph(tone: f32) -> char {
    // The set is intentionally small: it has a smooth density progression and
    // avoids glyphs whose widths or decorative serifs vary across terminals.
    const FILL: &[char] = &[' ', '.', ':', ';', '\'', '+', '*', '#', '%', '@'];
    let index = (tone.clamp(0.0, 1.0) * (FILL.len() - 1) as f32).round() as usize;
    FILL[index]
}

fn stabilize(candidate: RenderedCell, previous: RenderedCell, hysteresis: f32) -> RenderedCell {
    if hysteresis <= 0.0 || previous.glyph == ' ' && previous.perceptual.region_id == 0 {
        return candidate;
    }
    if candidate.glyph == previous.glyph {
        return candidate;
    }
    let tone_delta = (candidate.perceptual.tone - previous.perceptual.tone).abs();
    let edge_delta = (candidate.perceptual.edge_strength - previous.perceptual.edge_strength).abs();
    let region_penalty = (candidate.role != previous.role) as u8 as f32 * 0.08;
    if tone_delta * 0.7 + edge_delta * 0.3 + region_penalty < hysteresis {
        RenderedCell {
            glyph: previous.glyph,
            role: previous.role,
            perceptual: candidate.perceptual,
        }
    } else {
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(index: u8) -> Pixels {
        [[index; FRAME_WIDTH]; FRAME_HEIGHT]
    }

    #[test]
    fn palette_luminance_is_monotonic_for_black_gray_white() {
        assert!(palette_luminance(0x0d) < palette_luminance(0x00));
        assert!(palette_luminance(0x00) < palette_luminance(0x10));
        assert!(palette_luminance(0x10) < palette_luminance(0x20));
    }

    #[test]
    fn terminal_fit_preserves_physical_aspect() {
        let grid = TerminalGrid::fit(80, 38);
        let physical = grid.content_columns as f32 / (2.0 * grid.content_rows as f32);
        let source = FRAME_WIDTH as f32 / FRAME_HEIGHT as f32;
        assert!((physical - source).abs() < 0.04, "{physical} vs {source}");
        assert!(grid.content_columns <= grid.columns);
        assert!(grid.content_rows <= grid.rows);
    }

    #[test]
    fn terminal_fit_prefers_tile_aligned_profiles() {
        let standard = TerminalGrid::fit(80, 38);
        assert_eq!((standard.content_columns, standard.content_rows), (64, 30));
        assert_eq!(FRAME_WIDTH / standard.content_columns, 4);
        assert_eq!(FRAME_HEIGHT / standard.content_rows, 8);

        let large = TerminalGrid::fit(128, 62);
        assert_eq!((large.content_columns, large.content_rows), (128, 60));
        assert_eq!(FRAME_WIDTH / large.content_columns, 2);
        assert_eq!(FRAME_HEIGHT / large.content_rows, 4);
    }

    #[test]
    fn short_terminal_falls_back_to_maximum_aspect_fit() {
        let grid = TerminalGrid::fit(80, 24);
        assert_eq!(grid.content_rows, 24);
        assert!(grid.content_columns > 32);
    }

    #[test]
    fn vertical_boundary_gets_vertical_orientation() {
        let mut frame = flat(0x0d);
        for row in &mut frame {
            for pixel in &mut row[FRAME_WIDTH / 2..] {
                *pixel = 0x20;
            }
        }
        let grid = TerminalGrid::fit(80, 38);
        let cell = analyze_cell(
            &frame,
            LayerData::none(),
            grid,
            grid.content_columns / 2,
            10,
        );
        assert_eq!(cell.edge_orientation, EdgeOrientation::Vertical);
        assert!(cell.edge_strength > EDGE_THRESHOLD);
    }

    #[test]
    fn visible_sprite_has_more_weight_than_hidden_sprite() {
        let frame = flat(0x10);
        let sprite_pixels = flat(0x20);
        let sprite_coverage = flat(1);
        let background_pixels = flat(0x10);
        let background_coverage = flat(1);
        let sprites = LayerView {
            pixels: &sprite_pixels,
            coverage: &sprite_coverage,
        };
        let background = LayerView {
            pixels: &background_pixels,
            coverage: &background_coverage,
        };
        let visible = flat(1);
        let hidden = flat(0);
        let grid = TerminalGrid::fit(80, 38);
        let hidden_cell = analyze_cell(
            &frame,
            LayerData {
                background: Some(background),
                sprites: Some(sprites),
                visible_sprites: Some(&hidden),
            },
            grid,
            10,
            10,
        );
        let visible_cell = analyze_cell(
            &frame,
            LayerData {
                background: Some(background),
                sprites: Some(sprites),
                visible_sprites: Some(&visible),
            },
            grid,
            10,
            10,
        );
        assert!(visible_cell.sprite_weight > hidden_cell.sprite_weight);
        // Keep the bindings above intentionally explicit: these are borrowed
        // views and should not accidentally turn the API into owned layers.
        assert!(sprites.coverage[0][0] != 0);
    }

    #[test]
    fn flat_scene_uses_fill_grammar_and_hysteresis() {
        let mut renderer = Renderer::for_terminal(80, 38);
        let first = renderer.render_pixels(&flat(0x20), LayerData::none());
        let second = renderer.render_pixels(&flat(0x20), LayerData::none());
        assert!(first
            .cells
            .iter()
            .all(|cell| matches!(cell.role, GlyphRole::Fill)));
        assert_eq!(first.text(), second.text());
        assert!(first.text().lines().all(|line| line.len() == 80));
    }

    #[test]
    fn tiny_tone_change_does_not_flicker_cells() {
        let mut renderer = Renderer::new(RendererConfig::for_terminal(40, 20));
        let first = renderer.render_pixels(&flat(0x10), LayerData::none());
        let second = renderer.render_pixels(&flat(0x11), LayerData::none());
        assert_eq!(first.text(), second.text());
    }

    #[test]
    fn quiet_background_prefers_blank_over_dense_fill() {
        let cell = PerceptualCell {
            tone: 0.95,
            contrast: 0.0,
            edge_strength: 0.0,
            edge_orientation: EdgeOrientation::None,
            gradient_x: 0.0,
            gradient_y: 0.0,
            sprite_weight: 0.0,
            region_id: 1,
        };
        let rendered = select_cell(cell, RendererConfig::default());
        assert_eq!(rendered.glyph, ' ');
    }

    #[test]
    fn high_contrast_text_like_cell_survives_without_contour_direction() {
        let cell = PerceptualCell {
            tone: 0.72,
            contrast: 0.22,
            edge_strength: 0.21,
            edge_orientation: EdgeOrientation::None,
            gradient_x: 0.0,
            gradient_y: 0.0,
            sprite_weight: 0.0,
            region_id: 1,
        };
        let rendered = select_cell(cell, RendererConfig::default());
        assert_ne!(rendered.glyph, ' ');
        assert_eq!(rendered.role, GlyphRole::Fill);
    }
}
