use engine_ui::geometry::{GeometryError, Rect, layout};
use engine_ui::text::{FontBook, TextError};
use engine_ui::{StyleSheet, WidgetState, WidgetTree};

fn fonts() -> FontBook {
    let mut book = FontBook::default();
    book.add_face(
        "PT Serif",
        include_bytes!("fixtures/fonts/PT_Serif-Web-Regular.ttf").to_vec(),
        0,
        "Fixture",
        400,
    )
    .unwrap();
    book.set_default_families(["Fixture"]);
    book
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        origin: [x, y],
        size: [w, h],
    }
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn arranges_reusable_rows_and_columns_at_two_viewport_sizes_and_scales() {
    let tree = WidgetTree::parse("geometry.xml", include_str!("fixtures/geometry.xml")).unwrap();
    let sheet = StyleSheet::parse("geometry.css", include_str!("fixtures/geometry.css")).unwrap();
    let styled = sheet.resolve(&tree, |_, _| WidgetState::default());
    let fonts = fonts();
    for (physical, scale, expected_cards, expected_physical, copy_width) in [
        (
            [400.0, 250.0],
            1.25,
            [
                rect(20.0, 60.0, 120.0, 80.0),
                rect(180.0, 60.0, 120.0, 80.0),
            ],
            rect(25.0, 75.0, 150.0, 100.0),
            55.0,
        ),
        (
            [960.0, 540.0],
            2.0,
            [
                rect(20.0, 95.0, 160.0, 80.0),
                rect(300.0, 95.0, 160.0, 80.0),
            ],
            rect(40.0, 190.0, 320.0, 160.0),
            72.0,
        ),
    ] {
        let logical = physical.map(|extent| extent / scale);
        let mut images = 0;
        let hud = layout(&styled, logical, &fonts, |path| {
            assert_eq!(path, "ui/coin.png");
            images += 1;
            Some([18.0, 12.0])
        })
        .unwrap();
        assert_eq!(images, 2);
        assert_eq!(hud.bounds, rect(0.0, 0.0, logical[0], logical[1]));
        assert_eq!(
            hud.content,
            rect(16.0, 12.0, logical[0] - 32.0, logical[1] - 24.0)
        );
        assert_eq!(hud.children[0].bounds.physical(scale), expected_physical);
        assert_eq!(hud.children.len(), expected_cards.len());

        for (index, (card, expected)) in hud.children.iter().zip(expected_cards).enumerate() {
            assert_eq!(card.styled.widget.id(), Some(["left", "right"][index]));
            assert_eq!(card.bounds, expected);
            let [x, y] = expected.origin;
            assert_eq!(
                card.content,
                rect(x + 8.0, y + 8.0, expected.size[0] - 16.0, 64.0)
            );
            let [image, copy] = card.children.as_slice() else {
                panic!("expected image and copy panel")
            };
            assert_eq!(image.bounds, rect(x + 8.0, y + 34.0, 18.0, 12.0));
            assert_eq!(copy.bounds, rect(x + 32.0, y + 13.0, copy_width, 54.0));
            let [label, button] = copy.children.as_slice() else {
                panic!("expected label and button")
            };
            assert_eq!(
                label.bounds,
                rect(x + 35.0, y + 16.0, copy_width - 6.0, 18.0)
            );
            assert_eq!(
                button.bounds,
                rect(x + 35.0, y + 44.0, copy_width - 6.0, 20.0)
            );
            assert_eq!(
                button.bounds.physical(scale).origin,
                [(x + 35.0) * scale, (y + 44.0) * scale]
            );
            assert!(button.bounds.contains(button.bounds.origin));
            assert!(!button.bounds.contains([
                button.bounds.origin[0] + button.bounds.size[0],
                button.bounds.origin[1]
            ]));

            let text = label.text.as_ref().unwrap();
            assert_eq!(text.layout.lines.len(), 1);
            close(
                text.origin[0] + text.layout.size[0],
                label.content.origin[0] + label.content.size[0],
            );
            close(text.origin[1], label.content.origin[1]);
            let caption = button.text.as_ref().unwrap();
            close(
                caption.origin[0] + caption.layout.size[0] / 2.0,
                button.content.origin[0] + button.content.size[0] / 2.0,
            );
        }
    }
}

#[test]
fn measures_wrapped_text_in_the_padded_content_box() {
    let tree = WidgetTree::parse(
        "wrapped.xml",
        r#"<ui><panel id="root"><label id="caption" text="AV AV AV" /></panel></ui>"#,
    )
    .unwrap();
    let css = StyleSheet::parse("wrapped.css", r#"
        #root { width: 50%; margin: 4px; padding: 10px; flex-direction: column; align-items: flex-start; }
        #caption { width: 48px; padding: 4px; font-size: 20px; line-height: 16px; }
    "#).unwrap();
    let styled = css.resolve(&tree, |_, _| WidgetState::default());
    let result = layout(&styled, [200.0, 150.0], &fonts(), |_| None).unwrap();
    assert_eq!(result.bounds, rect(4.0, 4.0, 100.0, 142.0));
    let label = &result.children[0];
    assert_eq!(label.bounds, rect(14.0, 14.0, 48.0, 56.0));
    assert_eq!(label.content, rect(18.0, 18.0, 40.0, 48.0));
    let text = label.text.as_ref().unwrap();
    assert_eq!(text.origin, [18.0, 18.0]);
    assert_eq!(text.layout.lines.len(), 3);
    assert_eq!(text.layout.size[1], label.content.size[1]);
    assert!(
        text.layout
            .lines
            .iter()
            .all(|line| line.advance <= label.content.size[0])
    );
}

#[test]
fn reports_invalid_constraints_and_identifies_unmeasurable_content() {
    let tree = WidgetTree::parse("geometry.xml", include_str!("fixtures/geometry.xml")).unwrap();
    let styled = StyleSheet::default().resolve(&tree, |_, _| WidgetState::default());
    let fonts = FontBook::default();
    assert_eq!(
        layout(&styled, [f32::NAN, 150.0], &fonts, |_| None).unwrap_err(),
        GeometryError::InvalidViewport
    );
    assert_eq!(
        layout(&styled, [200.0, 150.0], &fonts, |_| Some([-1.0, 12.0])).unwrap_err(),
        GeometryError::InvalidImage("ui/coin.png".into())
    );
    assert_eq!(
        layout(&styled, [200.0, 150.0], &fonts, |_| None).unwrap_err(),
        GeometryError::Text {
            widget: "#caption".into(),
            source: TextError::NoFace
        }
    );
}
