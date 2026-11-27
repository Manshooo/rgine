use unicode_linebreak::linebreaks;

use super::shape::{Paragraph, Shaping};
use super::{TextError, TextLine};

pub(super) fn lines(
    text: &str,
    width: Option<f32>,
    shaping: &mut Shaping<'_>,
) -> Result<Vec<TextLine>, TextError> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if index < start
            || !matches!(
                character,
                '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
        {
            continue;
        }
        let paragraph = shaping.paragraph(&text[start..index], start);
        append_paragraph(&mut lines, &paragraph, width, shaping)?;
        start = index + character.len_utf8();
        if character == '\r' && text[start..].starts_with('\n') {
            start += 1;
        }
    }
    let paragraph = shaping.paragraph(&text[start..], start);
    append_paragraph(&mut lines, &paragraph, width, shaping)?;
    Ok(lines)
}

fn append_paragraph(
    output: &mut Vec<TextLine>,
    paragraph: &Paragraph<'_>,
    width: Option<f32>,
    shaping: &mut Shaping<'_>,
) -> Result<(), TextError> {
    let clusters = &paragraph.clusters;
    let natural = shaping.line(paragraph, 0..clusters.len())?;
    if !natural.advance.is_finite() {
        return Err(TextError::InvalidTypography);
    }
    let Some(width) = width.filter(|&width| natural.advance > width) else {
        output.push(natural);
        return Ok(());
    };

    // The paragraph advances let us find a likely break without shaping every
    // prefix. Shape each final line afresh so kerning, ligatures and joining
    // reflect its actual boundaries.
    let mut advances = vec![0.0_f64; clusters.len() + 1];
    for glyph in natural.runs.iter().flat_map(|run| &run.glyphs) {
        let byte = glyph.cluster - paragraph.offset;
        let end = clusters.partition_point(|cluster| cluster.start <= byte);
        advances[end] += f64::from(glyph.advance[0].max(0.0));
    }
    for index in 1..advances.len() {
        advances[index] += advances[index - 1];
    }

    let mut visible = vec![0; clusters.len() + 1];
    for (index, cluster) in clusters.iter().enumerate() {
        visible[index + 1] = if cluster.wrap_space {
            visible[index]
        } else {
            index + 1
        };
    }
    let mut preferred: Vec<_> = linebreaks(paragraph.text)
        .filter_map(|(byte, _)| {
            clusters
                .binary_search_by_key(&byte, |cluster| cluster.start)
                .ok()
        })
        .collect();
    preferred.push(clusters.len());

    let mut start = 0;
    while start < clusters.len() {
        let remaining = &preferred[preferred.partition_point(|&end| end <= start)..];
        let limit = advances[start] + f64::from(width);
        let fit = remaining.partition_point(|&end| advances[visible[end]] <= limit);
        let mut end = if fit > 0 {
            remaining[fit - 1]
        } else {
            advances
                .partition_point(|&advance| advance <= limit)
                .saturating_sub(1)
                .max(start + 1)
        };
        loop {
            let line = shaping.line(paragraph, start..visible[end].max(start))?;
            if line.advance <= width || end == start + 1 {
                output.push(line);
                break;
            }
            let prior = remaining.partition_point(|&candidate| candidate < end);
            end = if prior > 0 {
                remaining[prior - 1]
            } else {
                end - 1
            };
        }
        start = end;
        while start < clusters.len() && clusters[start].wrap_space {
            start += 1;
        }
    }
    Ok(())
}
