use std::ops::Range;

use harfrust::{Buffer, Direction, Script, ShaperFont, unicode};
use unicode_bidi::{BidiClass, Level, ParagraphBidiInfo, bidi_class};
use unicode_segmentation::UnicodeSegmentation;

use super::{FontFace, Glyph, GlyphRun, TextError, TextLine};

const REPLACEMENT: char = '\u{fffd}';

#[derive(Clone, Copy, Default)]
struct Metrics {
    ascent: f32,
    descent: f32,
    gap: f32,
}

impl Metrics {
    fn include(&mut self, other: Self) {
        self.ascent = self.ascent.max(other.ascent);
        self.descent = self.descent.max(other.descent);
        self.gap = self.gap.max(other.gap);
    }
}

struct ShapingFace<'a> {
    face: &'a FontFace,
    font: ShaperFont<'a, 'static>,
    scale: f32,
    metrics: Metrics,
}

pub(super) struct Shaping<'a> {
    faces: Vec<ShapingFace<'a>>,
    replacement: usize,
    line_height: Option<f32>,
    buffer: Buffer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Properties {
    face: usize,
    script: Script,
    level: Level,
}

pub(super) struct Cluster {
    pub start: usize,
    pub wrap_space: bool,
    properties: Properties,
    missing: bool,
}

pub(super) struct Paragraph<'a> {
    pub text: &'a str,
    pub offset: usize,
    pub clusters: Vec<Cluster>,
}

impl Paragraph<'_> {
    pub(super) fn byte(&self, cluster: usize) -> usize {
        self.clusters
            .get(cluster)
            .map_or(self.text.len(), |cluster| cluster.start)
    }
}

impl<'a> Shaping<'a> {
    pub(super) fn new(faces: Vec<&'a FontFace>, size: f32, line_height: Option<f32>) -> Self {
        let faces: Vec<_> = faces
            .into_iter()
            .map(|face| {
                let font = ShaperFont::new(face.font());
                let scale = size / f32::from(font.units_per_em());
                let metrics = font.metrics().h_line().map_or(
                    Metrics {
                        ascent: size * 0.8,
                        descent: size * 0.2,
                        gap: 0.0,
                    },
                    |line| Metrics {
                        ascent: line.ascender.to_f32() * scale,
                        descent: -line.descender.to_f32() * scale,
                        gap: line.line_gap.to_f32().max(0.0) * scale,
                    },
                );
                ShapingFace {
                    face,
                    font,
                    scale,
                    metrics,
                }
            })
            .collect();
        let replacement = faces
            .iter()
            .position(|face| has_glyph(&face.font, REPLACEMENT))
            .unwrap_or(0);
        Self {
            faces,
            replacement,
            line_height,
            buffer: Buffer::new(),
        }
    }

    pub(super) fn paragraph<'t>(&self, text: &'t str, offset: usize) -> Paragraph<'t> {
        let bidi = ParagraphBidiInfo::new(text, None);
        let mut script = strong_script(text).unwrap_or(Script::COMMON);
        let clusters = text
            .grapheme_indices(true)
            .map(|(start, text)| {
                if let Some(next) = strong_script(text) {
                    script = next;
                }
                let face = self.faces.iter().position(|face| covers(&face.font, text));
                Cluster {
                    start,
                    wrap_space: text.bytes().all(|c| matches!(c, b' ' | b'\t')),
                    properties: Properties {
                        face: face.unwrap_or(self.replacement),
                        script,
                        level: bidi.levels[start],
                    },
                    missing: face.is_none(),
                }
            })
            .collect();
        Paragraph {
            text,
            offset,
            clusters,
        }
    }

    pub(super) fn line(
        &mut self,
        paragraph: &Paragraph<'_>,
        range: Range<usize>,
    ) -> Result<TextLine, TextError> {
        let mut groups: Vec<(Range<usize>, Properties)> = Vec::new();
        for index in range.clone() {
            let properties = paragraph.clusters[index].properties;
            if let Some((range, previous)) = groups.last_mut()
                && *previous == properties
            {
                range.end = index + 1;
            } else {
                groups.push((index..index + 1, properties));
            }
        }
        let levels: Vec<_> = groups
            .iter()
            .map(|(_, properties)| properties.level)
            .collect();
        let order = ParagraphBidiInfo::reorder_visual(&levels);
        let mut runs = Vec::with_capacity(groups.len());
        let mut metrics = if groups.is_empty() {
            self.faces[0].metrics
        } else {
            Metrics::default()
        };
        let mut advance = 0.0;
        for index in order {
            let (range, properties) = &groups[index];
            let run = self.run(paragraph, range.clone(), *properties, advance)?;
            advance += run.advance;
            metrics.include(self.faces[properties.face].metrics);
            runs.push(run);
        }
        let em = metrics.ascent + metrics.descent;
        let height = self.line_height.unwrap_or(em + metrics.gap);
        Ok(TextLine {
            source: paragraph.offset + paragraph.byte(range.start)
                ..paragraph.offset + paragraph.byte(range.end),
            advance,
            height,
            baseline: metrics.ascent + (height - em) * 0.5,
            runs,
        })
    }

    fn run(
        &mut self,
        paragraph: &Paragraph<'_>,
        range: Range<usize>,
        properties: Properties,
        origin: f32,
    ) -> Result<GlyphRun, TextError> {
        self.buffer.clear();
        self.buffer.set_direction(if properties.level.is_rtl() {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        self.buffer.set_script(Some(properties.script));
        let start = paragraph.byte(range.start);
        let end = paragraph.byte(range.end);
        for index in range {
            let cluster = &paragraph.clusters[index];
            let source = (paragraph.offset + cluster.start) as u32;
            if cluster.missing {
                self.buffer.push(REPLACEMENT as u32, source);
            } else {
                let text = &paragraph.text[cluster.start..paragraph.byte(index + 1)];
                for character in text.chars() {
                    self.buffer.push(normalize_space(character) as u32, source);
                }
            }
        }
        let face = &self.faces[properties.face];
        if harfrust::shape(&face.font, &mut self.buffer, Default::default()).is_err()
            || !self.buffer.allocation_successful()
        {
            return Err(TextError::Shaping {
                source: face.face.source().into(),
            });
        }
        let mut pen = [0.0, 0.0];
        let mut glyphs = Vec::with_capacity(self.buffer.glyph_infos().len());
        for (glyph, position) in self
            .buffer
            .glyph_infos()
            .iter()
            .zip(self.buffer.glyph_positions())
        {
            let advance = [
                position.x_advance as f32 * face.scale,
                -(position.y_advance as f32) * face.scale,
            ];
            glyphs.push(Glyph {
                id: glyph.glyph_id,
                cluster: glyph.cluster as usize,
                position: [
                    origin + pen[0] + position.x_offset as f32 * face.scale,
                    pen[1] - position.y_offset as f32 * face.scale,
                ],
                advance,
            });
            pen[0] += advance[0];
            pen[1] += advance[1];
        }
        Ok(GlyphRun {
            face: face.face.clone(),
            source: paragraph.offset + start..paragraph.offset + end,
            glyphs,
            advance: pen[0],
        })
    }
}

fn strong_script(text: &str) -> Option<Script> {
    text.chars()
        .map(|c| unicode::script(c as u32))
        .find(|&script| !matches!(script, Script::COMMON | Script::INHERITED | Script::UNKNOWN))
}

fn has_glyph(font: &ShaperFont<'_, '_>, character: char) -> bool {
    font.nominal_glyph(character as u32)
        .is_some_and(|glyph| glyph.to_u32() != 0)
}

fn covers(font: &ShaperFont<'_, '_>, text: &str) -> bool {
    if text
        .chars()
        .map(normalize_space)
        .all(|c| ignorable_for_coverage(c) || has_glyph(font, c))
    {
        return true;
    }
    // A face can support a decomposed cluster through its composed glyph.
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    chars
        .try_fold(first as u32, |previous, next| {
            unicode::compose(previous, next as u32)
        })
        .and_then(char::from_u32)
        .is_some_and(|character| has_glyph(font, character))
}

fn normalize_space(character: char) -> char {
    if character == '\t' { ' ' } else { character }
}

fn ignorable_for_coverage(character: char) -> bool {
    use BidiClass::*;
    matches!(
        bidi_class(character),
        BN | LRE | RLE | LRO | RLO | PDF | LRI | RLI | FSI | PDI
    ) || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}
