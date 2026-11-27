//! Ordered screen-space drawing for game HUDs, UI and debug overlays.

use std::ops::Range;

pub use engine_core::glam::{Affine2, Vec2};

use crate::resources::SampledGroupHandle;

mod gpu;
pub(crate) use gpu::CanvasRenderer;

/// The physical drawable and its display scale, with a top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    size: [u32; 2],
    scale: f32,
}

impl Viewport {
    /// Returns a viewport when the extent and pixels per logical unit are positive and finite.
    pub fn new(size: [u32; 2], scale: f32) -> Option<Self> {
        let viewport = Self { size, scale };
        (!size.contains(&0)
            && scale.is_finite()
            && scale > 0.0
            && viewport.logical_size().is_finite())
        .then_some(viewport)
    }

    /// Width and height of the drawable in physical pixels.
    pub fn physical_size(self) -> [u32; 2] {
        self.size
    }

    /// Physical pixels per logical unit.
    pub fn display_scale(self) -> f32 {
        self.scale
    }

    /// Width and height available to screen-space content in logical units.
    pub fn logical_size(self) -> Vec2 {
        Vec2::from_array(self.size.map(|value| value as f32)) / self.scale
    }

    fn scissor(self, clip: Option<Rect>) -> Option<[u32; 4]> {
        match clip {
            Some(clip) => self.pixel_rect(clip),
            None => Some([0, 0, self.size[0], self.size[1]]),
        }
    }

    /// Converts a logical rectangle to clipped physical `[x, y, width, height]`.
    pub fn pixel_rect(self, clip: Rect) -> Option<[u32; 4]> {
        if clip.is_empty() {
            return None;
        }
        let [width, height] = self.size;
        // Keep a pixel when its centre is inside the half-open clip rectangle.
        let edge =
            |value: f32, limit: u32| (value.mul_add(self.scale, -0.5).ceil() as u32).min(limit);
        let x = edge(clip.min.x, width);
        let y = edge(clip.min.y, height);
        let right = edge(clip.max.x, width);
        let bottom = edge(clip.max.y, height);
        (right > x && bottom > y).then(|| [x, y, right - x, bottom - y])
    }
}

/// A half-open rectangle from its top-left corner to its bottom-right corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Included edges of the rectangle.
    pub min: Vec2,
    /// Excluded edges of the rectangle.
    pub max: Vec2,
}

impl Rect {
    /// Creates a rectangle given a top-left corner and a size.
    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }

    fn is_empty(self) -> bool {
        !(self.min.is_finite() && self.max.is_finite() && self.min.cmplt(self.max).all())
    }

    fn corners(self) -> [Vec2; 4] {
        [
            self.min,
            Vec2::new(self.max.x, self.min.y),
            self.max,
            Vec2::new(self.min.x, self.max.y),
        ]
    }
}

struct Vertex {
    position: Vec2,
    uv: Vec2,
    color: [f32; 4],
}

struct Batch {
    vertices: Range<usize>,
    clip: Option<Rect>,
    texture: Option<SampledGroupHandle>,
}

/// An ordered list of logical screen-space primitives with straight-alpha blending.
///
/// Coordinates increase to the right and down. Transforms apply to individual
/// primitives. Clips use viewport coordinates and intersect their ancestors.
/// Later primitives appear over earlier ones. Clear and reuse the list each frame.
#[derive(Default)]
pub struct Canvas {
    vertices: Vec<Vertex>,
    batches: Vec<Batch>,
    clips: Vec<Rect>,
}

impl Canvas {
    /// Clears all drawing and clips, retaining the buffers for reuse.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.batches.clear();
        self.clips.clear();
    }

    /// Intersects a logical viewport-space clip with the current clip.
    pub fn push_clip(&mut self, rect: Rect) {
        let rect = if rect.is_empty() {
            Rect::from_min_size(Vec2::ZERO, Vec2::ZERO)
        } else {
            self.clips
                .last()
                .map_or(rect, |parent| parent.intersect(rect))
        };
        self.clips.push(rect);
    }

    /// Restores the prior clip. Returns false if the stack was empty.
    pub fn pop_clip(&mut self) -> bool {
        self.clips.pop().is_some()
    }

    /// Adds a solid quad with a linear RGBA color.
    pub fn solid_quad(&mut self, rect: Rect, transform: Affine2, color: [f32; 4]) {
        if !rect.is_empty() {
            self.quad(rect.corners(), [Vec2::ZERO; 4], transform, color, None);
        }
    }

    /// Adds a quad with normalized top-left texture coordinates and a linear RGBA tint.
    /// The group must bind a filterable 2D color view and a filtering sampler
    /// from the device that draws the list.
    pub fn textured_quad(
        &mut self,
        rect: Rect,
        uv: Rect,
        transform: Affine2,
        tint: [f32; 4],
        texture: SampledGroupHandle,
    ) {
        if !rect.is_empty() {
            self.quad(rect.corners(), uv.corners(), transform, tint, Some(texture));
        }
    }

    /// Adds a flat-ended solid line. The transform applies to its width and endpoints.
    pub fn line(&mut self, from: Vec2, to: Vec2, width: f32, transform: Affine2, color: [f32; 4]) {
        if width.is_finite()
            && width > 0.0
            && let Some(direction) = (to - from).try_normalize()
        {
            let normal = direction.perp() * (width * 0.5);
            self.quad(
                [from - normal, to - normal, to + normal, from + normal],
                [Vec2::ZERO; 4],
                transform,
                color,
                None,
            );
        }
    }

    fn quad(
        &mut self,
        points: [Vec2; 4],
        uv: [Vec2; 4],
        transform: Affine2,
        color: [f32; 4],
        texture: Option<SampledGroupHandle>,
    ) {
        let clip = self.clips.last().copied();
        if clip.is_some_and(Rect::is_empty) {
            return;
        }
        let points = points.map(|point| transform.transform_point2(point));
        if points.iter().chain(&uv).any(|point| !point.is_finite())
            || color.iter().any(|channel| !channel.is_finite())
        {
            return;
        }
        let start = self.vertices.len();
        self.vertices.extend([0, 1, 2, 0, 2, 3].map(|index| Vertex {
            position: points[index],
            uv: uv[index],
            color,
        }));
        let end = self.vertices.len();
        if let Some(batch) = self.batches.last_mut()
            && batch.clip == clip
            && batch.texture == texture
        {
            batch.vertices.end = end;
        } else {
            self.batches.push(Batch {
                vertices: start..end,
                clip,
                texture,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scissors_use_half_open_logical_edges_at_fractional_scale() {
        let viewport = Viewport::new([96, 48], 1.5).unwrap();
        assert_eq!(viewport.logical_size(), Vec2::new(64.0, 32.0));
        let clip = Rect::from_min_size(Vec2::new(1.0, 2.0), Vec2::new(4.0, 3.0));
        assert_eq!(viewport.scissor(Some(clip)), Some([1, 3, 6, 4]));
        let outside = Rect::from_min_size(Vec2::splat(100.0), Vec2::ONE);
        assert_eq!(viewport.scissor(Some(outside)), None);
        assert!(Viewport::new([0, 48], 1.5).is_none());
        assert!(Viewport::new([96, 48], f32::NAN).is_none());
    }
}
