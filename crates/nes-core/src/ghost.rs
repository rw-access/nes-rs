//! Sprite-only rewind/replay ghosts for layered-render frontends.

use crate::video::RenderLayer;

/// Initial opacity for a replayed ghost layer.
pub const GHOST_BASE_OPACITY: f32 = 0.5;

/// One covered sprite pixel in a captured frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostSprite {
    pub x: u8,
    pub y: u8,
    pub palette_index: u8,
}

/// A sparse sprite frame. Pixels without sprite coverage are not allocated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostFrame {
    sprites: Box<[GhostSprite]>,
}

impl GhostFrame {
    pub fn sprites(&self) -> &[GhostSprite] {
        &self.sprites
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }
}

/// One independent replay layer and its current fading frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostLayer {
    frames: Box<[GhostFrame]>,
    frame_index: usize,
}

impl GhostLayer {
    /// The captured frames in replay order (oldest rewind frame first).
    pub fn frames(&self) -> &[GhostFrame] {
        &self.frames
    }

    /// The frame currently intended for presentation, if the layer is active.
    pub fn current_frame(&self) -> Option<&GhostFrame> {
        self.frames.get(self.frame_index)
    }

    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// Linear opacity: the first frame is `GHOST_BASE_OPACITY`, and opacity is
    /// exhausted when the captured span has been consumed.
    pub fn opacity(&self) -> f32 {
        let span = self.frames.len();
        if span == 0 || self.frame_index >= span {
            0.0
        } else {
            GHOST_BASE_OPACITY * (span - self.frame_index) as f32 / span as f32
        }
    }
}

/// Shared rewind/replay state for sprite ghosts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhostTimeline {
    capture: Vec<GhostFrame>,
    layers: Vec<GhostLayer>,
    new_layers: usize,
    /// A normal output frame has been exposed and must be advanced before the
    /// next normal output. Rewind frames deliberately do not advance ghosts.
    frame_open: bool,
}

impl GhostTimeline {
    /// Whether at least one replay layer still has a frame to present.
    pub fn is_active(&self) -> bool {
        !self.layers.is_empty()
    }

    /// Capture one rendered sprite layer as a sparse frame.
    pub fn capture_frame(&mut self, sprites: &RenderLayer) {
        let mut captured = Vec::new();
        for (y, (pixels, coverage)) in sprites.pixels.iter().zip(&sprites.coverage).enumerate() {
            for (x, (&palette_index, &covered)) in pixels.iter().zip(coverage).enumerate() {
                if covered != 0 {
                    captured.push(GhostSprite {
                        x: x as u8,
                        y: y as u8,
                        palette_index,
                    });
                }
            }
        }
        self.capture.push(GhostFrame {
            sprites: captured.into_boxed_slice(),
        });
    }

    /// Finish a rewind capture. Its frames are reversed into a new layer;
    /// existing layers remain independent and retain their own fade cursor.
    pub fn finish_capture(&mut self) -> bool {
        if self.capture.is_empty() {
            return false;
        }
        self.capture.reverse();
        self.layers.push(GhostLayer {
            frames: std::mem::take(&mut self.capture).into_boxed_slice(),
            frame_index: 0,
        });
        self.new_layers += 1;
        true
    }

    /// Begin a normal rendered frame and advance all layers after their prior
    /// presented frame. This is intentionally not called for rewind frames.
    pub fn begin_frame(&mut self) {
        if self.frame_open {
            let existing_layers = self.layers.len().saturating_sub(self.new_layers);
            for layer in self.layers.iter_mut().take(existing_layers) {
                layer.frame_index += 1;
            }
            self.layers
                .retain(|layer| layer.frame_index < layer.frames.len());
        }
        self.new_layers = 0;
        self.frame_open = true;
    }

    pub fn layers(&self) -> &[GhostLayer] {
        &self.layers
    }

    pub fn reset(&mut self) {
        self.capture.clear();
        self.layers.clear();
        self.new_layers = 0;
        self.frame_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{GhostTimeline, GHOST_BASE_OPACITY};
    use crate::video::{RenderLayer, FRAME_HEIGHT, FRAME_WIDTH};

    fn frame(x: usize, y: usize, color: u8) -> RenderLayer {
        let mut layer = RenderLayer {
            pixels: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
            coverage: [[0; FRAME_WIDTH]; FRAME_HEIGHT],
        };
        layer.pixels[y][x] = color;
        layer.coverage[y][x] = 1;
        layer
    }

    #[test]
    fn sparse_capture_reverses_frame_order() {
        let mut timeline = GhostTimeline::default();
        timeline.capture_frame(&frame(4, 5, 0x11));
        timeline.capture_frame(&RenderLayer::default());
        timeline.capture_frame(&frame(9, 10, 0x22));

        assert!(timeline.finish_capture());
        let layer = &timeline.layers()[0];
        assert_eq!(layer.frames().len(), 3);
        assert_eq!(
            layer.frames()[0].sprites(),
            &[super::GhostSprite {
                x: 9,
                y: 10,
                palette_index: 0x22
            }]
        );
        assert!(layer.frames()[1].is_empty());
        assert_eq!(layer.frames()[2].sprites()[0].x, 4);
    }

    #[test]
    fn layers_fade_and_expire_linearly() {
        let mut timeline = GhostTimeline::default();
        timeline.capture_frame(&frame(0, 0, 1));
        timeline.capture_frame(&frame(1, 0, 2));
        timeline.capture_frame(&frame(2, 0, 3));
        timeline.finish_capture();

        timeline.begin_frame();
        assert_eq!(timeline.layers()[0].opacity(), GHOST_BASE_OPACITY);
        timeline.begin_frame();
        assert_eq!(
            timeline.layers()[0].opacity(),
            GHOST_BASE_OPACITY * 2.0 / 3.0
        );
        timeline.begin_frame();
        assert_eq!(timeline.layers()[0].opacity(), GHOST_BASE_OPACITY / 3.0);
        timeline.begin_frame();
        assert!(timeline.layers().is_empty());
    }

    #[test]
    fn layers_have_independent_cursors() {
        let mut timeline = GhostTimeline::default();
        timeline.capture_frame(&frame(0, 0, 1));
        timeline.capture_frame(&frame(1, 0, 2));
        timeline.finish_capture();
        timeline.begin_frame();

        timeline.capture_frame(&frame(2, 0, 3));
        timeline.finish_capture();
        timeline.begin_frame();

        assert_eq!(timeline.layers().len(), 2);
        assert_eq!(timeline.layers()[0].frame_index(), 1);
        assert_eq!(timeline.layers()[1].frame_index(), 0);
    }
}
