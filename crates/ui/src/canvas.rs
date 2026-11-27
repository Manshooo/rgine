//! Canvas commands prepared from authored widget geometry.

use engine_render::canvas::{Affine2, Canvas, Rect, Vec2, Viewport};
use engine_render::resources::SampledGroupHandle;

use crate::WidgetKind;
use crate::geometry::{self, TextPlacement, WidgetBox};
use crate::text::canvas::{GlyphAtlasError, GlyphFrame};

enum Paint {
    Background {
        rect: Rect,
        color: [f32; 4],
    },
    Image {
        rect: Rect,
        src: String,
    },
    Text {
        clip: Rect,
        color: [f32; 4],
        text: TextPlacement,
    },
}

/// Ordered widget drawing commands with prepared text in logical pixels.
/// Rebuild after changes to styles, geometry, text or logical viewport size.
pub struct UiDrawList {
    paints: Vec<Paint>,
}

impl UiDrawList {
    /// Takes the geometry of a styled tree, keeping its shaped text and painter order.
    pub fn prepare(root: WidgetBox<'_, '_>) -> Self {
        let mut paints = Vec::new();
        collect(root, &mut paints);
        Self { paints }
    }

    /// Adds the widget paints over the scene. Images fill their content boxes.
    /// Missing images are skipped. The same image assets should supply the
    /// intrinsic sizes used to lay out the tree. Share the glyph frame with
    /// other text drawn into the same canvas submission.
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        viewport: Viewport,
        glyphs: &mut GlyphFrame<'_>,
        mut image: impl FnMut(&str) -> Option<SampledGroupHandle>,
    ) -> Result<(), GlyphAtlasError> {
        for paint in &self.paints {
            match paint {
                Paint::Background { rect, color } => {
                    canvas.solid_quad(*rect, Affine2::IDENTITY, *color);
                }
                Paint::Image { rect, src } => {
                    if let Some(texture) = image(src) {
                        canvas.textured_quad(
                            *rect,
                            Rect::from_min_size(Vec2::ZERO, Vec2::ONE),
                            Affine2::IDENTITY,
                            [1.0; 4],
                            texture,
                        );
                    }
                }
                Paint::Text { clip, color, text } => {
                    canvas.push_clip(*clip);
                    let draw = glyphs.draw_text(
                        canvas,
                        viewport,
                        &text.layout,
                        Vec2::from_array(text.origin),
                        *color,
                    );
                    canvas.pop_clip();
                    draw?;
                }
            }
        }
        Ok(())
    }
}

fn collect(node: WidgetBox<'_, '_>, paints: &mut Vec<Paint>) {
    let style = &node.styled.style;
    if style.background_color[3] > 0 && nonempty(node.bounds) {
        paints.push(Paint::Background {
            rect: rect(node.bounds),
            color: linear_rgba(style.background_color),
        });
    }
    if nonempty(node.content) {
        let content = rect(node.content);
        if let WidgetKind::Image { src } = node.styled.widget.kind() {
            paints.push(Paint::Image {
                rect: content,
                src: src.clone(),
            });
        }
        if let Some(text) = node.text.filter(|_| style.typography.color[3] > 0) {
            paints.push(Paint::Text {
                clip: content,
                color: linear_rgba(style.typography.color),
                text,
            });
        }
    }
    for child in node.children {
        collect(child, paints);
    }
}

fn nonempty(rect: geometry::Rect) -> bool {
    rect.size.into_iter().all(|extent| extent > 0.0)
}

fn rect(bounds: geometry::Rect) -> Rect {
    Rect::from_min_size(
        Vec2::from_array(bounds.origin),
        Vec2::from_array(bounds.size),
    )
}

fn linear_rgba(rgba: [u8; 4]) -> [f32; 4] {
    let [r, g, b, a] = rgba.map(|byte| f32::from(byte) / 255.0);
    let linear = |srgb: f32| {
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    [linear(r), linear(g), linear(b), a]
}

#[cfg(test)]
mod tests {
    use crate::{StyleSheet, WidgetTree, text::FontBook};

    use super::*;

    #[test]
    fn authored_paints_keep_tree_order_and_resize_in_logical_pixels() {
        let tree = WidgetTree::parse(
            "hud.xml",
            r#"<ui>
            <panel class="hud">
                <panel class="card"><image src="marker" /><label text="Очки" /></panel>
                <button text="Pause" />
            </panel>
        </ui>"#,
        )
        .unwrap();
        let style = StyleSheet::parse(
            "hud.css",
            "
            .hud { background-color: #80400080; justify-content: space-between; }
            .card { width: 50%; padding: 4px; background-color: #00808080; }
            button { width: 56px; background-color: black; }
        ",
        )
        .unwrap();
        let styled = style.resolve(&tree, |_, _| Default::default());
        let mut fonts = FontBook::default();
        fonts
            .add_face(
                "PT Serif",
                include_bytes!("../tests/fixtures/fonts/PT_Serif-Web-Regular.ttf").to_vec(),
                0,
                "Test",
                400,
            )
            .unwrap();
        fonts.set_default_families(["Test"]);
        for width in [240.0, 400.0] {
            let geometry = geometry::layout(&styled, [width, 100.0], &fonts, |src| {
                (src == "marker").then_some([18.0, 18.0])
            })
            .unwrap();
            let text = geometry.children[0].children[1].text.as_ref().unwrap();
            let text_origin = text.origin;
            let text_clip = rect(geometry.children[0].children[1].content);
            let list = UiDrawList::prepare(geometry);
            let [
                Paint::Background { rect: root, color },
                Paint::Background { rect: card, .. },
                Paint::Image { rect: image, src },
                Paint::Text { clip, text, .. },
                Paint::Background { rect: button, .. },
                Paint::Text { .. },
            ] = list.paints.as_slice()
            else {
                panic!("expected backgrounds, image and text in authored order");
            };
            assert_eq!(root.max, Vec2::new(width, 100.0));
            assert_eq!(card.max.x, width * 0.5);
            assert_eq!(button.min.x, width - 56.0);
            assert_eq!(image.min, Vec2::splat(4.0));
            assert_eq!(image.max.x - image.min.x, 18.0);
            assert_eq!(src, "marker");
            assert_eq!(*clip, text_clip);
            assert_eq!(text.origin, text_origin);
            assert!((color[0] - 0.21586).abs() < 0.00001);
            assert_eq!(color[3], 128.0 / 255.0);
        }
    }
}
