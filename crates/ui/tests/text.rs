use std::path::Path;

use engine_ui::text::{FontBook, FontFace, TextError, TextLayout};
use engine_ui::{
    LineHeight, StyleSheet, TextAlign, Typography, WidgetKind, WidgetState, WidgetTree,
};

const LATIN: &[u8] = include_bytes!("fixtures/fonts/ABeeZee-Regular.ttf");
const SERIF: &[u8] = include_bytes!("fixtures/fonts/PT_Serif-Web-Regular.ttf");
const SERIF_BOLD: &[u8] = include_bytes!("fixtures/fonts/PT_Serif-Web-Bold.ttf");

fn faces() -> (FontBook, [FontFace; 3]) {
    let mut book = FontBook::default();
    book.set_default_families(["Primary", "Fallback"]);
    let primary = book
        .add_face("ABeeZee", LATIN.to_vec(), 0, "Primary", 400)
        .unwrap();
    let regular = book
        .add_face("PT Serif", SERIF.to_vec(), 0, "Fallback", 400)
        .unwrap();
    let bold = book
        .add_face("PT Serif Bold", SERIF_BOLD.to_vec(), 0, "Fallback", 700)
        .unwrap();
    (book, [primary, regular, bold])
}

fn approx(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn shapes_kerning_combining_marks_and_cyrillic_fallback() {
    let (book, [primary, regular, bold]) = faces();
    let mut style = Typography {
        font_size: 32.0,
        ..Typography::default()
    };
    let width = |text| book.prepare(text, &style, None).unwrap().size[0];
    assert!(width("AV") < width("A") + width("V"));

    let text = "AV МИР";
    let layout = book.prepare(text, &style, None).unwrap();
    let line = &layout.lines[0];
    let [latin, cyrillic] = line.runs.as_slice() else {
        panic!("unexpected runs {line:?}")
    };
    assert_eq!(latin.face, primary);
    assert_eq!(cyrillic.face, regular);
    assert_eq!(&text[cyrillic.source.clone()], "МИР");
    assert!(
        line.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .all(|glyph| glyph.id != 0)
    );
    assert!(line.baseline > 0.0 && line.baseline < line.height);
    approx(line.advance, latin.advance + cyrillic.advance);
    approx(cyrillic.glyphs[0].position[0], latin.advance);
    approx(layout.size[0], line.advance);
    approx(layout.size[1], line.height);

    let combined = book.prepare("é", &style, None).unwrap();
    let decomposed = book.prepare("e\u{301}", &style, None).unwrap();
    assert_eq!(combined.size, decomposed.size);
    assert_eq!(
        combined.lines[0].runs[0].glyphs,
        decomposed.lines[0].runs[0].glyphs
    );
    let mark = book.prepare("x\u{301}", &style, None).unwrap();
    let [base, accent] = mark.lines[0].runs[0].glyphs.as_slice() else {
        panic!("missing positioned mark {mark:?}")
    };
    assert_ne!(accent.id, 0);
    assert_eq!(accent.cluster, base.cluster);
    assert_eq!(accent.advance, [0.0; 2]);
    assert_ne!(accent.position, base.position);

    style.font_weight = 700;
    style.font_size = 24.0;
    let selected = book.prepare("М", &style, None).unwrap();
    assert_eq!(selected.font_size, 24.0);
    assert_eq!(selected.lines[0].runs[0].face, bold);
}

#[test]
fn prepares_styled_label_bounds_and_placements_at_two_widths() {
    let (book, _) = faces();
    let tree = WidgetTree::parse(
        "wrap.xml",
        r#"<ui><panel><label text="AV AV Я" /></panel></ui>"#,
    )
    .unwrap();
    let css = StyleSheet::parse("wrap.css", r#"panel { font-family: "Missing", primary, fallback; font-size: 20px; line-height: 1.5; text-align: right; }"#).unwrap();
    let styled = css.resolve(&tree, |_, _| WidgetState::default());
    let label = &styled.children[0];
    let WidgetKind::Label { text } = label.widget.kind() else {
        panic!("expected a label")
    };
    let style = &label.style.typography;
    let measure = |text: &str| book.prepare(text, style, None).unwrap().size[0];
    let wide = measure("AV AV") + 0.1;
    let narrow = measure("AV").max(measure("Я")) + 0.1;
    let natural = book.prepare(text, style, None).unwrap();
    assert_eq!(natural.lines.len(), 1);
    approx(natural.size[1], 30.0);
    for (width, expected) in [
        (wide, &(["AV AV", "Я"])[..]),
        (narrow, &(["AV", "AV", "Я"])[..]),
    ] {
        let shaped = book.prepare(text, style, Some(width)).unwrap();
        assert_eq!(
            shaped
                .lines
                .iter()
                .map(|line| &text[line.source.clone()])
                .collect::<Vec<_>>(),
            expected
        );
        let max_advance = expected
            .iter()
            .map(|text| measure(text))
            .fold(0.0_f32, f32::max);
        approx(shaped.size[0], max_advance);
        approx(shaped.size[1], 30.0 * expected.len() as f32);
        assert!(shaped.size[0] <= width);
        for line in &shaped.lines {
            approx(line.height, 30.0);
            approx(line.advance, line.runs.iter().map(|run| run.advance).sum());
            for glyph in line.runs.iter().flat_map(|run| &run.glyphs) {
                assert_eq!(glyph.physical_position(1.0), glyph.position);
                assert_eq!(
                    glyph.physical_position(2.0),
                    glyph.position.map(|value| value * 2.0)
                );
            }
        }
        let short = shaped.lines.last().unwrap();
        let standalone = book.prepare("Я", style, None).unwrap();
        approx(
            short.runs[0].glyphs[0].position[0] - standalone.lines[0].runs[0].glyphs[0].position[0],
            shaped.size[0] - short.advance,
        );
        assert_eq!(shaped, book.prepare(text, style, Some(width)).unwrap());
    }
}

#[test]
fn loads_opentype_rejects_invalid_data_and_retains_prior_revisions() {
    let (mut book, [primary, _, _]) = faces();
    let style = Typography::default();
    let prior = book.prepare("A", &style, None).unwrap();
    let invalid = book
        .add_face("broken.ttf", vec![1, 2, 3], 0, "Primary", 400)
        .unwrap_err();
    assert!(invalid.to_string().starts_with("broken.ttf: invalid"));
    let index = book
        .add_face("single.ttf", LATIN.to_vec(), 1, "Primary", 400)
        .unwrap_err();
    assert!(index.to_string().contains("index 1"));
    assert_eq!(prior, book.prepare("A", &style, None).unwrap());

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fonts/SourceCodePro-Regular.otf");
    let replacement = book.load_face(&path, 0, "Primary", 400).unwrap();
    assert_eq!(&replacement.bytes()[..4], b"OTTO");
    assert_ne!(replacement.id(), primary.id());
    assert_eq!(prior.lines[0].runs[0].face.bytes(), LATIN);
    assert_eq!(
        book.prepare("A", &style, None).unwrap().lines[0].runs[0].face,
        replacement
    );
}

#[test]
fn missing_glyphs_and_forced_breaks_have_deterministic_source_offsets() {
    let (book, _) = faces();
    let style = Typography {
        line_height: LineHeight::Px(20.0),
        align: TextAlign::Left,
        ..Typography::default()
    };
    let glyphs = |layout: TextLayout| {
        layout
            .lines
            .into_iter()
            .flat_map(|line| line.runs)
            .flat_map(|run| run.glyphs)
            .collect::<Vec<_>>()
    };
    let replacement = glyphs(book.prepare("\u{fffd}", &style, None).unwrap());
    let missing = glyphs(book.prepare("🧐", &style, None).unwrap());
    assert_eq!(missing, replacement);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].cluster, 0);
    let missing_families = Typography {
        font_families: vec!["Unknown".into()],
        ..style.clone()
    };
    assert_eq!(
        book.prepare("a", &missing_families, None),
        Err(TextError::NoFace)
    );

    let text = "x\u{301}y\r\na\n";
    let layout = book.prepare(text, &style, Some(0.0)).unwrap();
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| &text[line.source.clone()])
            .collect::<Vec<_>>(),
        ["x\u{301}", "y", "a", ""]
    );
    assert_eq!(layout.size[1], 80.0);
    assert_eq!(layout.lines[2].runs[0].glyphs[0].cluster, 6);
}

#[test]
fn glyphs_follow_visual_order_without_changing_source_offsets() {
    let (book, _) = faces();
    let styled = book
        .prepare("A\u{202e}BC\u{202c}D", &Typography::default(), None)
        .unwrap();
    let visible: Vec<_> = styled.lines[0]
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .filter(|glyph| glyph.advance[0] > 0.0)
        .map(|glyph| glyph.cluster)
        .collect();
    assert_eq!(visible, [0, 5, 4, 9]);
}
