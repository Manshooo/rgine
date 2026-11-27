use ab_glyph_rasterizer::{Point, Rasterizer, point};
use engine_render::canvas::Vec2;
use skrifa::{DrawMemory, GlyphId, PathSink, ScalerFont, font::glyph::outline::ControlBoundsSink};

use super::{FontFace, GlyphAtlasError, GlyphKey};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bitmap {
    pub origin: Vec2,
    pub size: [u32; 2],
}

pub(super) struct Raster {
    scaler: Option<(GlyphKey, ScalerFont)>,
    coverage: Rasterizer,
    pub rgba: Vec<u8>,
}

impl Default for Raster {
    fn default() -> Self {
        Self {
            scaler: None,
            coverage: Rasterizer::new(0, 0),
            rgba: Vec::new(),
        }
    }
}

impl Raster {
    pub fn glyph(
        &mut self,
        face: &FontFace,
        key: GlyphKey,
        side: u32,
    ) -> Result<Option<Bitmap>, GlyphAtlasError> {
        // Keep the parsed font instance and use its linear outlines so the
        // rasterizer does not change any shaped advances or glyph origins.
        if !self
            .scaler
            .as_ref()
            .is_some_and(|(current, _)| current.face == key.face && current.ppem == key.ppem)
        {
            self.scaler = Some((
                key,
                ScalerFont::force_unhinted(face.font().clone(), Some(f32::from_bits(key.ppem))),
            ));
        }
        let scaler = &self.scaler.as_ref().expect("configured scaler").1;
        let Some(outline) = scaler.outline(GlyphId::new(key.glyph)) else {
            return Ok(None);
        };
        let failure = |error: skrifa::ScaleError| GlyphAtlasError::Outline {
            source: face.source().into(),
            glyph: key.glyph,
            message: error.to_string(),
        };
        let mut bounds = ControlBoundsSink::new();
        outline
            .draw(DrawMemory::Scratch, &mut bounds)
            .map_err(failure)?;
        let Some(bounds) = bounds.bounding_box() else {
            return Ok(None);
        };
        if bounds.x_min == bounds.x_max || bounds.y_min == bounds.y_max {
            return Ok(None);
        }
        let ink = Vec2::new(bounds.x_min, -bounds.y_max).floor();
        let end = Vec2::new(bounds.x_max, -bounds.y_min).ceil();
        let origin = ink - Vec2::ONE;
        let extent = end - origin + Vec2::ONE;
        if !origin.is_finite()
            || !extent.is_finite()
            || extent.min_element() < 3.0
            || extent.max_element() > side as f32
        {
            return Err(GlyphAtlasError::GlyphTooLarge {
                glyph: key.glyph,
                page_side: side,
            });
        }
        let [width, height] = extent.to_array().map(|value| value as u32);
        self.coverage.reset(width as usize, height as usize);
        outline
            .draw(
                DrawMemory::Scratch,
                &mut Sink {
                    raster: &mut self.coverage,
                    origin,
                    start: point(0.0, 0.0),
                    current: point(0.0, 0.0),
                },
            )
            .map_err(failure)?;
        self.rgba.resize(width as usize * height as usize * 4, 255);
        self.coverage.for_each_pixel(|index, alpha| {
            self.rgba[index * 4 + 3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        });
        Ok(Some(Bitmap {
            origin,
            size: [width, height],
        }))
    }
}

struct Sink<'a> {
    raster: &'a mut Rasterizer,
    origin: Vec2,
    start: Point,
    current: Point,
}

impl Sink<'_> {
    fn map(&self, x: f32, y: f32) -> Point {
        point(x - self.origin.x, -y - self.origin.y)
    }
}

impl PathSink for Sink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.map(x, y);
        self.current = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let end = self.map(x, y);
        self.raster.draw_line(self.current, end);
        self.current = end;
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let control = self.map(cx, cy);
        let end = self.map(x, y);
        self.raster.draw_quad(self.current, control, end);
        self.current = end;
    }

    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        let a = self.map(ax, ay);
        let b = self.map(bx, by);
        let end = self.map(x, y);
        self.raster.draw_cubic(self.current, a, b, end);
        self.current = end;
    }

    fn close(&mut self) {
        self.raster.draw_line(self.current, self.start);
        self.current = self.start;
    }
}
