use std::error::Error;

use engine_render::{
    canvas::{Affine2, Canvas, Rect, Vec2, Viewport},
    resources::ResourceTable,
};
use engine_ui::{
    Typography,
    text::{
        FontBook,
        canvas::{GlyphAtlas, GlyphFrame},
    },
};

use crate::hierarchy::{Action, Hierarchy, contains};

const TEXT: [f32; 4] = [0.82, 0.88, 0.94, 1.0];
const MUTED: [f32; 4] = [0.45, 0.54, 0.66, 1.0];
const PANEL: [f32; 4] = [0.034, 0.046, 0.066, 1.0];
const BUTTON: [f32; 4] = [0.067, 0.093, 0.13, 1.0];
const ACTIVE: [f32; 4] = [0.031, 0.20, 0.27, 1.0];
const ERROR: [f32; 4] = [1.0, 0.46, 0.22, 1.0];
const ROW_HEIGHT: f32 = 26.0;

pub(crate) struct Layout {
    size: Vec2,
    pub rows: Rect,
    dialog: Rect,
}

impl Layout {
    pub fn new(size: Vec2) -> Self {
        let width = (size.x - 32.0).max(0.0);
        let dialog_width = width.min(580.0);
        Self {
            size,
            rows: rect(16.0, 146.0, width, (size.y - 208.0).max(0.0)),
            dialog: rect(
                (size.x - dialog_width) * 0.5,
                (size.y * 0.24).min(150.0),
                dialog_width,
                192.0,
            ),
        }
    }

    pub fn visible_rows(&self) -> usize {
        ((self.rows.max.y - self.rows.min.y) / ROW_HEIGHT).floor() as usize
    }
}

struct Item {
    bounds: Rect,
    label: String,
    indent: f32,
    color: [f32; 4],
    action: Action,
}

#[derive(Default)]
pub(crate) struct Actions(Vec<Item>);

impl Actions {
    pub fn new(state: &Hierarchy<'_>, layout: &Layout) -> Self {
        let mut list = Self::default();
        if let Some(dialog) = &state.dialog {
            let start = layout.dialog.min;
            list.buttons(
                start + Vec2::new(16.0, 148.0),
                90.0,
                &[
                    (dialog.confirm_label(), Action::Confirm),
                    ("Cancel", Action::Cancel),
                ],
            );
            return list;
        }
        let width = (layout.rows.max.x - layout.rows.min.x).min(496.0);
        list.buttons(
            Vec2::new(16.0, 39.0),
            width / 3.0,
            &[
                ("New level", Action::New),
                ("Open", Action::Open),
                ("Save", Action::Save),
            ],
        );
        let selected = state.document.selected;
        let tools = if state.moving.is_some() {
            vec![("Move to root", Action::Root), ("Cancel", Action::Cancel)]
        } else if selected.is_some() {
            vec![
                ("Add child", Action::Add),
                ("Rename", Action::Rename),
                ("Move", Action::Move),
                ("Delete", Action::Delete),
            ]
        } else {
            vec![("Add root", Action::Add)]
        };
        list.buttons(Vec2::new(16.0, 104.0), width / 4.0, &tools);
        let rows = state.document.hierarchy();
        let first = state
            .offset
            .min(rows.len().saturating_sub(layout.visible_rows()));
        for (index, row) in rows
            .into_iter()
            .skip(first)
            .take(layout.visible_rows())
            .enumerate()
        {
            list.0.push(Item {
                bounds: rect(
                    layout.rows.min.x,
                    layout.rows.min.y + ROW_HEIGHT * index as f32,
                    layout.rows.max.x - layout.rows.min.x,
                    ROW_HEIGHT - 1.0,
                ),
                label: state.document.name(row.id),
                indent: (row.depth as f32 * 16.0)
                    .min((layout.rows.max.x - layout.rows.min.x) * 0.7)
                    + 14.0,
                color: if selected == Some(row.id) {
                    ACTIVE
                } else {
                    PANEL
                },
                action: Action::Select(Some(row.id)),
            });
        }
        list
    }

    fn buttons(&mut self, start: Vec2, width: f32, items: &[(&str, Action)]) {
        for (index, &(label, action)) in items.iter().enumerate() {
            self.0.push(Item {
                bounds: rect(
                    start.x + index as f32 * width,
                    start.y,
                    (width - 6.0).max(0.0),
                    28.0,
                ),
                label: label.into(),
                indent: 9.0,
                color: BUTTON,
                action,
            });
        }
    }

    pub fn hit(&self, point: Vec2) -> Option<Action> {
        self.0
            .iter()
            .find(|item| contains(item.bounds, point))
            .map(|item| item.action)
    }
}

pub(crate) struct View {
    fonts: FontBook,
    atlas: GlyphAtlas,
}

impl View {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let families = [
            (
                "Interface",
                include_bytes!("../../../crates/ui/tests/fixtures/fonts/ABeeZee-Regular.ttf")
                    .as_slice(),
            ),
            (
                "Fallback",
                include_bytes!("../../../crates/ui/tests/fixtures/fonts/PT_Serif-Web-Regular.ttf")
                    .as_slice(),
            ),
        ];
        let mut fonts = FontBook::default();
        for (family, bytes) in families {
            fonts.add_face(family, bytes.to_vec(), 0, family, 400)?;
        }
        fonts.set_default_families(families.map(|(family, _)| family));
        Ok(Self {
            fonts,
            atlas: GlyphAtlas::default(),
        })
    }

    pub fn draw(
        &mut self,
        state: &Hierarchy<'_>,
        viewport: Viewport,
        resources: &mut ResourceTable,
        canvas: &mut Canvas,
    ) -> Result<(), Box<dyn Error>> {
        let layout = Layout::new(viewport.logical_size());
        let mut text = Labels {
            fonts: &self.fonts,
            glyphs: self.atlas.begin_frame(resources)?,
            viewport,
            canvas,
        };
        let path = state
            .document
            .path()
            .map_or_else(|| "Untitled level".into(), |path| path.to_string_lossy());
        let unsaved = if state.document.is_dirty() {
            "  •  Unsaved"
        } else {
            ""
        };
        text.line(
            &format!("{path}{unsaved}"),
            rect(16.0, 12.0, layout.rows.max.x - 16.0, 23.0),
            TEXT,
        )?;
        text.line("Hierarchy", rect(16.0, 80.0, 150.0, 22.0), TEXT)?;
        if state.moving.is_some() {
            text.line(
                "Choose the new parent in the hierarchy",
                rect(270.0, 109.0, layout.size.x - 286.0, 20.0),
                MUTED,
            )?;
        }
        text.fill(layout.rows, PANEL);
        if layout.visible_rows() != 0 && state.document.len() == 0 {
            text.line(
                "Add a root node to start the level",
                rect(29.0, 157.0, layout.size.x - 59.0, 23.0),
                MUTED,
            )?;
        }
        if let Some(id) = state.document.selected {
            text.line(
                &id.to_string(),
                rect(16.0, layout.size.y - 54.0, layout.size.x - 32.0, 22.0),
                MUTED,
            )?;
        }
        if let Some(dialog) = &state.dialog {
            text.fill(
                Rect::from_min_size(Vec2::ZERO, layout.size),
                [0.01, 0.015, 0.025, 0.82],
            );
            text.fill(layout.dialog, PANEL);
            let origin = layout.dialog.min + Vec2::new(16.0, 16.0);
            let width = (layout.dialog.max.x - origin.x - 16.0).max(0.0);
            text.line(
                dialog.heading(state.document.is_dirty()),
                Rect::from_min_size(origin, Vec2::new(width, 22.0)),
                TEXT,
            )?;
            if let Some(entry) = &dialog.entry {
                let entry_box = rect(origin.x, origin.y + 38.0, width, 30.0);
                text.fill(
                    entry_box,
                    if dialog.replace_on_input {
                        ACTIVE
                    } else {
                        BUTTON
                    },
                );
                let label = if entry.is_empty() {
                    "Enter a value"
                } else {
                    entry
                };
                text.line(
                    label,
                    rect(origin.x + 8.0, origin.y + 44.0, width - 16.0, 20.0),
                    TEXT,
                )?;
            }
            text.line(
                &state.notice,
                rect(origin.x, origin.y + 81.0, width, 42.0),
                ERROR,
            )?;
        } else {
            text.line(
                &state.notice,
                rect(16.0, layout.size.y - 30.0, layout.size.x - 32.0, 24.0),
                ERROR,
            )?;
        }
        for item in Actions::new(state, &layout).0 {
            text.fill(item.bounds, item.color);
            text.line(
                &item.label,
                Rect {
                    min: item.bounds.min + Vec2::new(item.indent, 4.0),
                    max: item.bounds.max - Vec2::new(5.0, 0.0),
                },
                TEXT,
            )?;
        }
        Ok(())
    }
}

struct Labels<'a, 'gpu> {
    fonts: &'a FontBook,
    glyphs: GlyphFrame<'gpu>,
    viewport: Viewport,
    canvas: &'a mut Canvas,
}

impl Labels<'_, '_> {
    fn line(&mut self, value: &str, bounds: Rect, color: [f32; 4]) -> Result<(), Box<dyn Error>> {
        if value.is_empty() || !bounds.min.cmplt(bounds.max).all() {
            return Ok(());
        }
        let style = Typography {
            font_size: 14.0,
            ..Default::default()
        };
        let layout = self.fonts.prepare(value, &style, None)?;
        self.canvas.push_clip(bounds);
        self.glyphs
            .draw_text(self.canvas, self.viewport, &layout, bounds.min, color)?;
        self.canvas.pop_clip();
        Ok(())
    }

    fn fill(&mut self, bounds: Rect, color: [f32; 4]) {
        self.canvas.solid_quad(bounds, Affine2::IDENTITY, color);
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::from_min_size(Vec2::new(x, y), Vec2::new(width, height))
}
