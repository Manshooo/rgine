use engine_ui::{
    AlignItems, Dimension, Edges, Flow, JustifyContent, LineHeight, StyleSheet, TextAlign,
    WidgetState, WidgetTree,
};

fn hud() -> WidgetTree {
    WidgetTree::parse("hud.xml", include_str!("fixtures/hud.xml")).unwrap()
}

#[test]
fn computes_layout_values_and_inherits_typography_into_panel_instances() {
    let tree = hud();
    let css = StyleSheet::parse("hud.css", include_str!("fixtures/hud.css")).unwrap();
    let styled = css.resolve(&tree, |_, _| WidgetState::default());
    let root = &styled.style;
    assert_eq!(root.width, Dimension::Percent(85.0));
    assert_eq!(root.min_width, Dimension::Px(320.0));
    assert_eq!(root.max_width, Dimension::Px(900.0));
    assert_eq!(root.flow, Flow::Column);
    assert_eq!(root.align_items, AlignItems::Center);
    assert_eq!(root.justify_content, JustifyContent::SpaceBetween);
    assert_eq!(
        root.padding,
        Edges {
            top: 8.0,
            right: 16.0,
            bottom: 10.0,
            left: 16.0
        }
    );
    let scoreboard = &styled.children[0];
    assert_eq!(scoreboard.style.gap, 12.0);
    assert_eq!(scoreboard.style.flow, Flow::Row);
    assert_eq!(scoreboard.style.width, Dimension::Auto);
    assert_eq!(scoreboard.style.padding, Edges::default());

    let [first, second] = scoreboard.children.as_slice() else {
        panic!("expected two separately styled counters");
    };
    for counter in [first, second] {
        assert_eq!(
            counter.style.margin,
            Edges {
                top: 0.0,
                right: 4.0,
                bottom: 2.0,
                left: 6.0
            }
        );
        assert_eq!(counter.style.min_height, Dimension::Px(24.0));
        assert_eq!(counter.style.max_height, Dimension::Percent(50.0));
        assert_eq!(counter.style.background_color, [17, 34, 51, 136]);
        let amount = &counter.children[1];
        assert_eq!(amount.widget.id(), Some("amount"));
        assert_eq!(
            amount.style.typography.font_families,
            ["Project UI", "Noto Sans", "sans-serif"]
        );
        assert_eq!(amount.style.typography.font_weight, 500);
        assert_eq!(
            amount.style.typography.line_height,
            LineHeight::Factor(1.25)
        );
        assert_eq!(amount.style.typography.align, TextAlign::Center);
        assert_eq!(amount.style.typography.color, [170, 187, 204, 255]);
    }
    assert_eq!(first.children[1].style.typography.font_size, 22.0);
    assert_eq!(second.children[1].style.typography.font_size, 18.0);
}

#[test]
fn recomputes_for_interaction_and_keeps_instance_state_separate() {
    let tree = hud();
    let css = StyleSheet::parse("hud.css", include_str!("fixtures/hud.css")).unwrap();
    for (state, expected) in [
        (WidgetState::default(), [51, 68, 85, 255]),
        (
            WidgetState {
                focused: true,
                ..Default::default()
            },
            [68, 119, 170, 255],
        ),
        (
            WidgetState {
                focused: true,
                active: true,
                ..Default::default()
            },
            [0, 0, 0, 255],
        ),
        (
            WidgetState {
                active: true,
                disabled: true,
                ..Default::default()
            },
            [0; 4],
        ),
    ] {
        let styled = css.resolve(&tree, |path, _| match path {
            [0, 0] => WidgetState {
                hovered: state.focused,
                ..Default::default()
            },
            [0, 0, 2] => state,
            _ => WidgetState::default(),
        });
        let scoreboard = &styled.children[0];
        let left = &scoreboard.children[0];
        let right = &scoreboard.children[1];
        assert_eq!(left.children[2].style.background_color, expected);
        assert_eq!(right.children[2].style.background_color, [51, 68, 85, 255]);
        let amount = if state.focused {
            [0, 255, 0, 255]
        } else {
            [170, 187, 204, 255]
        };
        assert_eq!(left.children[1].style.typography.color, amount);
        assert_eq!(
            right.children[1].style.typography.color,
            [170, 187, 204, 255]
        );
    }
}

#[test]
fn cascade_uses_only_matching_selectors_and_breaks_ties_by_source_order() {
    let tree = WidgetTree::parse(
        "test.xml",
        r#"<ui><panel class="a">
            <panel class="b"><panel><panel class="b">
                <button id="go" class="action" text="Go"/>
            </panel></panel></panel>
        </panel></ui>"#,
    )
    .unwrap();
    let css = StyleSheet::parse(
        "test.css",
        r#"
        #go { font-size: 30px; }
        button, #absent { color: #aaa; }
        .a > .b .action { height: 12px; }
        .action { color: #bbb; color: #ccc; font-size: 10px; }
        .action { color: #ddd; }
        .a > .action { height: 20px; }
        button { color: #eee; }
        */* comment */.action { gap: 7px; }
    "#,
    )
    .unwrap();
    let styled = css.resolve(&tree, |_, _| WidgetState::default());
    let button = &styled.children[0].children[0].children[0].children[0];
    assert_eq!(button.style.typography.color, [221, 221, 221, 255]);
    assert_eq!(button.style.typography.font_size, 30.0);
    assert_eq!(button.style.height, Dimension::Px(12.0));
    assert_eq!(button.style.gap, 7.0);
}

#[test]
fn rejects_invalid_rules_and_names_the_stylesheet_location() {
    let error = StyleSheet::parse(
        "ui/меню.css",
        "/* заголовок */\r\n.hud {\r\n  width: -1px;\r\n}",
    )
    .unwrap_err();
    assert_eq!((error.line, error.column), (3, 10));
    assert_eq!(error.source, "ui/меню.css");
    assert!(error.to_string().starts_with("ui/меню.css:3:10:"));

    for (css, expected) in [
        ("panel:unknown { color: white }", "unsupported state"),
        ("panel, { color: white }", "expected a widget selector"),
        ("panel { spacing: 4px }", "unsupported property"),
        ("panel { width: 12rem }", "unsupported length unit"),
        (
            "panel { color: #12345 }",
            "expected 3, 4, 6 or 8 hex digits",
        ),
        (
            "panel { padding: 1px 2px 3px 4px 5px }",
            "expected ';' or '}'",
        ),
        (
            "panel { font-family: 'unfinished",
            "unterminated quoted font family",
        ),
        ("panel { font-weight: 1001 }", "font weight must be"),
        ("panel { width: 3px", "expected ';' or '}'"),
        ("/* unfinished", "unterminated comment"),
    ] {
        let error = StyleSheet::parse("invalid.css", css).unwrap_err();
        assert!(error.message.contains(expected), "{css}: {error}");
    }
}
