use engine_ui::{WidgetKind, WidgetTree};

#[test]
fn authored_panels_preserve_hierarchy_order_content_and_local_ids() {
    let layout = WidgetTree::parse("hud.xml", include_str!("fixtures/hud.xml")).unwrap();
    let root = layout.root();
    assert_eq!(root.id(), Some("hud"));
    assert_eq!(root.classes(), ["hud"]);
    assert_eq!(root.kind(), &WidgetKind::Panel);

    let [scoreboard, pause] = root.children() else {
        panic!("expected a scoreboard followed by a pause button");
    };
    assert_eq!(scoreboard.id(), Some("scoreboard"));
    assert_eq!(pause.id(), Some("pause"));
    assert_eq!(
        pause.kind(),
        &WidgetKind::Button {
            text: "Pause".into()
        }
    );

    let [one, two] = scoreboard.children() else {
        panic!("expected two instantiated counters");
    };
    assert_eq!(one.id(), Some("player-one"));
    assert_eq!(two.id(), Some("player-two"));
    assert_eq!(one.classes(), ["counter", "compact", "left"]);
    assert_eq!(two.classes(), ["counter", "compact", "right"]);
    assert_eq!(one.children(), two.children());

    let [icon, amount, details] = one.children() else {
        panic!("expected the template's three children");
    };
    assert_eq!(icon.id(), Some("icon"));
    assert_eq!(
        icon.kind(),
        &WidgetKind::Image {
            src: "ui/coin.png".into()
        }
    );
    assert_eq!(amount.id(), Some("amount"));
    assert_eq!(
        amount.kind(),
        &WidgetKind::Label {
            text: "Монеты & очки".into()
        }
    );
    assert_eq!(details.id(), Some("details"));
    assert_eq!(
        details.kind(),
        &WidgetKind::Button {
            text: "Details".into()
        }
    );
}

#[test]
fn a_template_can_use_an_earlier_template_with_its_own_id_scope() {
    let xml = r#"<ui>
      <template name="inner"><label id="title" text="Inner" /></template>
      <template name="outer">
        <label id="title" text="Outer" />
        <use template="inner" id="inside" />
      </template>
      <panel id="title"><use template="outer" id="outside" /></panel>
    </ui>"#;
    let layout = WidgetTree::parse("nested.xml", xml).unwrap();
    let outer = &layout.root().children()[0];
    let inner = &outer.children()[1];
    assert_eq!(outer.id(), Some("outside"));
    assert_eq!(outer.children()[0].id(), Some("title"));
    assert_eq!(inner.id(), Some("inside"));
    assert_eq!(inner.children()[0].id(), Some("title"));
}

#[test]
fn reports_xml_syntax_and_schema_errors_with_source_positions() {
    let syntax =
        WidgetTree::parse("menus/hud.xml", "<ui>\n  <panel>\n  </button>\n</ui>").unwrap_err();
    assert_eq!(syntax.source, "menus/hud.xml");
    assert_eq!(syntax.line, 3);
    assert!(syntax.column > 0);
    assert!(syntax.to_string().starts_with("menus/hud.xml:3:"));

    let schema = WidgetTree::parse(
        "hud.xml",
        "<ui>\n  <panel>\n    <label />\n  </panel>\n</ui>",
    )
    .unwrap_err();
    assert_eq!((schema.line, schema.column), (3, 5));
    assert_eq!(schema.message, "<label> requires text");
}

#[test]
fn rejects_invalid_widget_structure_and_ambiguous_identifiers() {
    for (xml, message) in [
        (r#"<ui><panel><box /></panel></ui>"#, "unknown widget <box>"),
        (
            r#"<ui><panel typo="5" /></ui>"#,
            "unexpected attribute typo",
        ),
        (
            r#"<ui><panel><label text=""><panel /></label></panel></ui>"#,
            "cannot contain widgets",
        ),
        (
            r#"<ui><panel><label text="0" bind="" /></panel></ui>"#,
            "bind must be a nonempty token",
        ),
        (
            r#"<ui><panel><button text="Resume" action="pause menu" /></panel></ui>"#,
            "action must be a nonempty token",
        ),
        (
            r#"<ui><panel><label text="Resume" action="resume" /></panel></ui>"#,
            "unexpected attribute action",
        ),
        (
            r#"<ui><panel><use id="x" template="missing" /></panel></ui>"#,
            "has not been declared",
        ),
        (
            r#"<ui><template name="x"/><panel><use template="x" /></panel></ui>"#,
            "requires an id",
        ),
        (
            r#"<ui><template name="x"/><template name="x"/><panel /></ui>"#,
            "duplicate template",
        ),
        (
            r#"<ui><panel id="x"><panel><button id="x" text="x" /></panel></panel></ui>"#,
            "duplicate widget id",
        ),
        (r#"<ui><panel /><panel /></ui>"#, "expected </ui>"),
        (r#"<ui><panel /></ui><ui><panel /></ui>"#, "Unexpected"),
        (r#"<!DOCTYPE ui><ui><panel /></ui>"#, "DOCTYPE"),
    ] {
        let error = WidgetTree::parse("invalid.xml", xml).unwrap_err();
        assert!(error.message.contains(message), "{xml}: {error}");
    }
}

#[test]
fn rejects_excessive_depth_and_template_expansion() {
    let deep = format!("<ui>{}{}</ui>", "<panel>".repeat(65), "</panel>".repeat(65));
    assert!(
        WidgetTree::parse("deep.xml", &deep)
            .unwrap_err()
            .message
            .contains("depth")
    );

    let mut expanded = String::from("<ui><template name=\"t0\" />");
    for next in 1..16 {
        use std::fmt::Write;
        let previous = next - 1;
        write!(
            expanded,
            r#"<template name="t{next}"><use id="a" template="t{previous}"/><use id="b" template="t{previous}"/></template>"#
        ).unwrap();
    }
    expanded.push_str("<panel /></ui>");
    assert!(
        WidgetTree::parse("large.xml", &expanded)
            .unwrap_err()
            .message
            .contains("widget budget")
    );
}
