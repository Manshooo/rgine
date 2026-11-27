//! Project font faces and reusable, renderer-independent text layout.

use std::{fmt, ops::Range};

use crate::{LineHeight, TextAlign, Typography};

mod font;
mod shape;
mod wrap;

pub use font::{FontBook, FontFace, FontFaceId, FontLoadError};
use shape::Shaping;

/// One glyph positioned in logical pixels from the top-left of the text block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Glyph index in the run's face.
    pub id: u32,
    /// Byte offset of the glyph's cluster in the original UTF-8 string.
    pub cluster: usize,
    /// Baseline origin for rasterization, including the shaping offset.
    pub position: [f32; 2],
    /// Shaped horizontal and vertical pen advances in the same units, with Y down.
    pub advance: [f32; 2],
}

impl Glyph {
    /// Converts the logical rasterization origin to physical pixels without reflow.
    pub fn physical_position(self, display_scale: f32) -> [f32; 2] {
        self.position.map(|coordinate| coordinate * display_scale)
    }
}

/// Consecutive glyphs shaped with the same face, script and direction.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    /// The font revision and source bytes needed to draw these glyphs.
    pub face: FontFace,
    /// Input byte range covered by this run.
    pub source: Range<usize>,
    /// Positioned glyphs in visual order.
    pub glyphs: Vec<Glyph>,
    /// Total horizontal advance in logical pixels.
    pub advance: f32,
}

/// A line of measured text and the source runs used to draw it.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// Displayed input byte range, excluding hard breaks and trimmed wrap spaces.
    pub source: Range<usize>,
    /// Horizontal advance measured from the runs.
    pub advance: f32,
    /// Line box height in logical pixels.
    pub height: f32,
    /// Baseline Y in logical pixels from the top of the text block.
    pub baseline: f32,
    /// Runs in visual order, with positions relative to the text block.
    pub runs: Vec<GlyphRun>,
}

/// A prepared text block that supplies both layout bounds and drawing positions.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    /// Width of the widest line and sum of line heights, in logical pixels.
    pub size: [f32; 2],
    /// Logical font size to use for rasterization.
    pub font_size: f32,
    /// Prepared lines in top-to-bottom order.
    pub lines: Vec<TextLine>,
}

/// A failure to prepare text from the available faces or layout constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextError {
    /// None of the requested or default families is loaded.
    NoFace,
    /// A size, weight, line height or wrap width is outside its valid range.
    InvalidTypography,
    /// Input is too large for the shaper's byte offsets.
    TextTooLong,
    /// The shaper rejected the run or exceeded its work limit.
    Shaping { source: String },
}

impl fmt::Display for TextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFace => {
                formatter.write_str("none of the requested project font families is loaded")
            }
            Self::InvalidTypography => {
                formatter.write_str("invalid text size, weight, line height or wrap width")
            }
            Self::TextTooLong => formatter.write_str("text exceeds the shaper's byte range"),
            Self::Shaping { source } => write!(formatter, "{source}: could not shape the text run"),
        }
    }
}

impl std::error::Error for TextError {}

impl FontBook {
    /// Shapes and measures UTF-8 text with the computed widget typography.
    /// Pass a finite nonnegative width to wrap the text in logical pixels.
    pub fn prepare(
        &self,
        text: &str,
        typography: &Typography,
        max_width: Option<f32>,
    ) -> Result<TextLayout, TextError> {
        let size = typography.font_size;
        let line_height = match typography.line_height {
            LineHeight::Normal => None,
            LineHeight::Factor(factor) => Some(size * factor),
            LineHeight::Px(pixels) => Some(pixels),
        };
        let valid_extent = |value: f32| value.is_finite() && value >= 0.0;
        if !(valid_extent(size) && size > 0.0 && (1..=1000).contains(&typography.font_weight))
            || !max_width.is_none_or(valid_extent)
            || !line_height.is_none_or(valid_extent)
        {
            return Err(TextError::InvalidTypography);
        }
        if u32::try_from(text.len()).is_err() {
            return Err(TextError::TextTooLong);
        }
        let faces = self.resolve(&typography.font_families, typography.font_weight);
        if faces.is_empty() {
            return Err(TextError::NoFace);
        }
        let mut shaping = Shaping::new(faces, size, line_height);
        let mut lines = wrap::lines(text, max_width, &mut shaping)?;
        let width = lines
            .iter()
            .map(|line| line.advance)
            .fold(0.0_f32, f32::max);
        let mut height = 0.0;
        for line in &mut lines {
            line.baseline += height;
            height += line.height;
            if !line.baseline.is_finite() {
                return Err(TextError::InvalidTypography);
            }
            let align = match typography.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (width - line.advance) * 0.5,
                TextAlign::Right => width - line.advance,
            };
            for glyph in line.runs.iter_mut().flat_map(|run| &mut run.glyphs) {
                glyph.position[0] += align;
                glyph.position[1] += line.baseline;
                if !glyph
                    .position
                    .into_iter()
                    .chain(glyph.advance)
                    .all(f32::is_finite)
                {
                    return Err(TextError::InvalidTypography);
                }
            }
        }
        if !width.is_finite() || !height.is_finite() {
            return Err(TextError::InvalidTypography);
        }
        Ok(TextLayout {
            size: [width, height],
            font_size: size,
            lines,
        })
    }
}
