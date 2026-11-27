//! Rasterized text for the screen-space canvas.

use std::fmt;

use engine_render::canvas::{Affine2, Canvas, Rect, Vec2, Viewport};
use engine_render::resources::{
    ResourceError, ResourceTable, SampledBinding, SampledGroupHandle, SamplerDescriptor,
    SamplerFilter, SamplerHandle, TextureDescriptor, TextureHandle,
};

use super::{FontFace, FontFaceId, TextLayout};

mod bitmap;
mod cache;
#[cfg(test)]
mod tests;

use bitmap::Raster;
use cache::{Cache, Slot};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct GlyphKey {
    face: FontFaceId,
    glyph: u32,
    ppem: u32,
}

/// Counts work performed while filling one canvas frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlyphStats {
    /// Glyph coverage masks rasterized for this frame.
    pub rasterized: usize,
    /// Texture regions uploaded for new glyphs.
    pub uploads: usize,
    /// Bytes uploaded into those regions, including their transparent edges.
    pub uploaded_bytes: usize,
    /// Pages recycled after reaching the configured capacity.
    pub recycled_pages: usize,
    /// Pages currently allocated on the device.
    pub resident_pages: usize,
}

/// A glyph rasterization or atlas operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum GlyphAtlasError {
    /// The page size or page budget is outside the supported range.
    InvalidCapacity,
    /// A text draw supplied an invalid position, size or color.
    InvalidDraw,
    /// One glyph plus its transparent border cannot fit on an empty page.
    GlyphTooLarge { glyph: u32, page_side: u32 },
    /// All pages that could be recycled are already referenced by this frame.
    FrameFull,
    /// The font's outline could not be drawn.
    Outline {
        source: String,
        glyph: u32,
        message: String,
    },
    /// The graphics device rejected a sampled resource or upload.
    Resource(ResourceError),
}

impl fmt::Display for GlyphAtlasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCapacity => {
                formatter.write_str("invalid glyph atlas capacity for this device")
            }
            Self::InvalidDraw => formatter.write_str("invalid text draw position, size or color"),
            Self::GlyphTooLarge { glyph, page_side } => write!(
                formatter,
                "glyph {glyph} does not fit on a {page_side} × {page_side} atlas page"
            ),
            Self::FrameFull => {
                formatter.write_str("all glyph atlas pages are in use by the current frame")
            }
            Self::Outline {
                source,
                glyph,
                message,
            } => write!(
                formatter,
                "{source}: could not rasterize glyph {glyph}: {message}"
            ),
            Self::Resource(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for GlyphAtlasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ResourceError> for GlyphAtlasError {
    fn from(error: ResourceError) -> Self {
        Self::Resource(error)
    }
}

struct Page {
    texture: TextureHandle,
    group: SampledGroupHandle,
}

/// Reusable, bounded texture pages for glyphs from prepared text.
///
/// Create one per canvas renderer and start one [`GlyphFrame`] for each canvas
/// submission. Old pages can be reused after submission, and pages referenced
/// by the current frame stay intact. The atlas follows device replacement.
pub struct GlyphAtlas {
    cache: Cache,
    pages: Vec<Page>,
    sampler: Option<SamplerHandle>,
    raster: Raster,
    quads: Vec<(Rect, Slot)>,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new(512, 4).expect("default glyph atlas capacity is valid")
    }
}

impl GlyphAtlas {
    /// Creates a lazy atlas with square RGBA8 pages and a strict page limit.
    pub fn new(page_side: u32, max_pages: usize) -> Result<Self, GlyphAtlasError> {
        let side = page_side as usize;
        if page_side < 3
            || max_pages == 0
            || side
                .checked_mul(side)
                .and_then(|pixels| pixels.checked_mul(4))
                .and_then(|bytes| bytes.checked_mul(max_pages))
                .is_none()
        {
            return Err(GlyphAtlasError::InvalidCapacity);
        }
        Ok(Self {
            cache: Cache::new(page_side, max_pages),
            pages: Vec::new(),
            sampler: None,
            raster: Raster::default(),
            quads: Vec::new(),
        })
    }

    /// Starts drawing into a fresh canvas list, after submitting the previous one.
    pub fn begin_frame<'a>(
        &'a mut self,
        table: &'a mut ResourceTable,
    ) -> Result<GlyphFrame<'a>, GlyphAtlasError> {
        if self.cache.side > table.max_sampled_texture_side() {
            return Err(GlyphAtlasError::InvalidCapacity);
        }
        if let Some(sampler) = self.sampler {
            match table.sampler_descriptor(sampler) {
                Ok(_) => {}
                Err(ResourceError::ForeignHandle) => {
                    self.cache.clear();
                    self.pages.clear();
                    self.sampler = None;
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.cache.begin_frame();
        Ok(GlyphFrame {
            atlas: self,
            table,
            stats: GlyphStats::default(),
        })
    }

    /// Releases the atlas from a device that will outlive it. Call after the
    /// current canvas has been submitted, using the table that owns its pages.
    pub fn release(&mut self, table: &mut ResourceTable) -> Result<(), GlyphAtlasError> {
        if let Some(sampler) = self.sampler {
            table.sampler_descriptor(sampler)?;
            for page in &self.pages {
                table.texture_descriptor(page.texture)?;
            }
            for page in self.pages.drain(..) {
                table.release_texture(page.texture)?;
            }
            table.release_sampler(sampler)?;
            self.sampler = None;
        }
        self.cache.clear();
        Ok(())
    }
}

/// Drawing access for one canvas submission. Keep it across the text draws
/// of that submission so the atlas retains every page referenced by the list.
pub struct GlyphFrame<'a> {
    atlas: &'a mut GlyphAtlas,
    table: &'a mut ResourceTable,
    stats: GlyphStats,
}

impl GlyphFrame<'_> {
    /// Adds a prepared text block in painter order using the canvas's active clip.
    /// `origin` is the logical text origin and `color` is linear straight RGBA.
    /// A failed draw leaves the canvas unchanged.
    pub fn draw_text(
        &mut self,
        canvas: &mut Canvas,
        viewport: Viewport,
        layout: &TextLayout,
        origin: Vec2,
        color: [f32; 4],
    ) -> Result<(), GlyphAtlasError> {
        let scale = viewport.display_scale();
        let physical_size = layout.font_size * scale;
        if !physical_size.is_finite()
            || physical_size <= 0.0
            || !origin.is_finite()
            || !color.into_iter().all(f32::is_finite)
        {
            return Err(GlyphAtlasError::InvalidDraw);
        }
        self.atlas.quads.clear();
        for run in layout.lines.iter().flat_map(|line| &line.runs) {
            for glyph in &run.glyphs {
                let key = GlyphKey {
                    face: run.face.id(),
                    glyph: glyph.id,
                    ppem: physical_size.to_bits(),
                };
                let Some(slot) = self.slot(&run.face, key)? else {
                    continue;
                };
                let rect = quad_rect(origin + Vec2::from_array(glyph.position), scale, slot);
                if !rect.min.is_finite() || !rect.max.is_finite() {
                    return Err(GlyphAtlasError::InvalidDraw);
                }
                self.atlas.quads.push((rect, slot));
            }
        }
        let side = self.atlas.cache.side as f32;
        for &(rect, slot) in &self.atlas.quads {
            let uv =
                Rect::from_min_size(as_vec(slot.offset) / side, as_vec(slot.bitmap.size) / side);
            canvas.textured_quad(
                rect,
                uv,
                Affine2::IDENTITY,
                color,
                self.atlas.pages[slot.page].group,
            );
        }
        Ok(())
    }

    /// Returns the work and resident pages recorded for this frame.
    pub fn stats(&self) -> GlyphStats {
        GlyphStats {
            resident_pages: self.atlas.pages.len(),
            ..self.stats
        }
    }

    fn slot(&mut self, face: &FontFace, key: GlyphKey) -> Result<Option<Slot>, GlyphAtlasError> {
        if let Some(slot) = self.atlas.cache.get(key) {
            return Ok(Some(slot));
        }
        if self.atlas.cache.is_blank(key) {
            return Ok(None);
        }
        let side = self.atlas.cache.side;
        let Some(bitmap) = self.atlas.raster.glyph(face, key, side)? else {
            self.atlas.cache.mark_blank(key);
            return Ok(None);
        };
        self.stats.rasterized += 1;
        let reserved = self.atlas.cache.reserve(bitmap)?;
        let fresh = if reserved.slot.page == self.atlas.pages.len() {
            Some(self.new_page()?)
        } else {
            None
        };
        let page = fresh
            .as_ref()
            .unwrap_or_else(|| &self.atlas.pages[reserved.slot.page]);
        let rgba = &self.atlas.raster.rgba;
        let result = self.table.write_texture_region(
            page.texture,
            0,
            0,
            reserved.slot.offset,
            bitmap.size,
            rgba,
        );
        if let Err(error) = result {
            if let Some(fresh) = fresh {
                self.table.release_texture(fresh.texture)?;
            }
            return Err(error.into());
        }
        if let Some(fresh) = fresh {
            self.atlas.pages.push(fresh);
        }
        self.stats.uploads += 1;
        self.stats.uploaded_bytes += rgba.len();
        self.stats.recycled_pages += usize::from(reserved.recycle);
        Ok(Some(self.atlas.cache.commit(key, reserved)))
    }

    fn new_page(&mut self) -> Result<Page, ResourceError> {
        let sampler = if let Some(sampler) = self.atlas.sampler {
            sampler
        } else {
            let sampler = self.table.create_sampler(SamplerDescriptor {
                filter: SamplerFilter::Linear,
                ..SamplerDescriptor::NEAREST_CLAMP
            })?;
            self.atlas.sampler = Some(sampler);
            sampler
        };
        let side = self.atlas.cache.side;
        let texture = self
            .table
            .create_texture(TextureDescriptor::sampled_rgba8([side; 2]))?;
        match self
            .table
            .create_sampled_group(SampledBinding { texture, sampler })
        {
            Ok(group) => Ok(Page { texture, group }),
            Err(error) => {
                self.table.release_texture(texture)?;
                Err(error)
            }
        }
    }
}

fn quad_rect(baseline: Vec2, scale: f32, slot: Slot) -> Rect {
    Rect::from_min_size(
        baseline + slot.bitmap.origin / scale,
        as_vec(slot.bitmap.size) / scale,
    )
}

fn as_vec([x, y]: [u32; 2]) -> Vec2 {
    Vec2::new(x as f32, y as f32)
}
