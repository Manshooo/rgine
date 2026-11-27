use std::path::{Path, PathBuf};

use engine_platform::{Input, KeyCode, MouseButton, PhysicalKey, ScrollDelta};
use engine_render::canvas::{Rect, Vec2, Viewport};
use engine_scene::ObjectId;

use crate::{
    document::Document,
    preview::Preview,
    view::{Actions, Layout},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    New,
    Open,
    Save,
    Add,
    Rename,
    Move,
    Root,
    Delete,
    Select(Option<ObjectId>),
    Cancel,
    Confirm,
}

#[derive(Clone, Copy)]
enum Prompt {
    New,
    Open,
    Save,
    Rename(ObjectId),
    Delete(ObjectId),
    Quit,
}

pub(crate) struct Dialog {
    prompt: Prompt,
    pub entry: Option<String>,
    pub replace_on_input: bool,
}

impl Dialog {
    pub fn heading(&self, dirty: bool) -> &'static str {
        match self.prompt {
            Prompt::New => "Discard changes and create a new level?",
            Prompt::Open if dirty => "Open a level and discard unsaved changes?",
            Prompt::Open => "Open level",
            Prompt::Save => "Save level",
            Prompt::Rename(_) => "Rename node",
            Prompt::Delete(_) => "Delete this node and its descendants?",
            Prompt::Quit => "Discard unsaved changes and close?",
        }
    }

    pub fn confirm_label(&self) -> &'static str {
        match self.prompt {
            Prompt::New => "New level",
            Prompt::Open => "Open",
            Prompt::Save => "Save",
            Prompt::Rename(_) => "Rename",
            Prompt::Delete(_) => "Delete",
            Prompt::Quit => "Close",
        }
    }
}

pub(crate) struct Hierarchy<'registry> {
    pub document: Document<'registry>,
    pub notice: String,
    pub moving: Option<ObjectId>,
    pub dialog: Option<Dialog>,
    pub offset: usize,
    pub exit: bool,
    pointer: Option<Vec2>,
}

impl<'registry> Hierarchy<'registry> {
    pub fn new(document: Document<'registry>) -> Self {
        Self {
            document,
            notice: String::new(),
            moving: None,
            dialog: None,
            offset: 0,
            exit: false,
            pointer: None,
        }
    }

    pub fn accepts_text(&self) -> bool {
        self.dialog
            .as_ref()
            .is_some_and(|dialog| dialog.entry.is_some())
    }

    fn prompt(&mut self, prompt: Prompt, entry: Option<String>) {
        self.dialog = Some(Dialog {
            prompt,
            entry,
            replace_on_input: true,
        });
        self.notice.clear();
    }

    pub fn close(&mut self) {
        if self.document.is_dirty() {
            self.prompt(Prompt::Quit, None);
        } else {
            self.exit = true;
        }
    }

    fn confirm(&mut self) -> Result<(), String> {
        let Some(dialog) = &self.dialog else {
            return Ok(());
        };
        let entry = dialog.entry.as_deref().unwrap_or_default();
        match dialog.prompt {
            Prompt::New => self.reset(),
            Prompt::Open => {
                self.document.open(Path::new(entry))?;
                self.moving = None;
                self.offset = 0;
            }
            Prompt::Save => self.document.save(Path::new(entry))?,
            Prompt::Rename(id) => self.document.rename(id, entry)?,
            Prompt::Delete(id) => self
                .document
                .delete(id)
                .map_err(|error| error.to_string())?,
            Prompt::Quit => self.exit = true,
        }
        self.dialog = None;
        Ok(())
    }

    fn reset(&mut self) {
        self.document.reset();
        self.moving = None;
        self.offset = 0;
    }

    fn path_text(&self) -> String {
        self.document
            .path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn act(&mut self, action: Action) -> Result<(), String> {
        self.notice.clear();
        let selected = self.document.selected;
        match action {
            Action::New if self.document.is_dirty() => self.prompt(Prompt::New, None),
            Action::New => self.reset(),
            Action::Open => self.prompt(Prompt::Open, Some(self.path_text())),
            Action::Save => match self.document.path().map(PathBuf::from) {
                Some(path) => self.document.save(&path)?,
                None => self.prompt(Prompt::Save, Some("level.scene.ron".into())),
            },
            Action::Add => self.document.add().map_err(|error| error.to_string())?,
            Action::Rename => {
                if let Some(id) = selected {
                    self.prompt(Prompt::Rename(id), Some(self.document.name(id)));
                }
            }
            Action::Delete => {
                if let Some(id) = selected {
                    self.prompt(Prompt::Delete(id), None);
                }
            }
            Action::Move => self.moving = selected,
            Action::Root => return self.move_to(None),
            Action::Select(parent) if self.moving.is_some() => return self.move_to(parent),
            Action::Select(id) => self.document.selected = id,
            Action::Cancel => {
                if self.dialog.take().is_none() && self.moving.take().is_none() {
                    self.document.selected = None;
                }
            }
            Action::Confirm => return self.confirm(),
        }
        Ok(())
    }

    fn move_to(&mut self, parent: Option<ObjectId>) -> Result<(), String> {
        if let Some(id) = self.moving {
            self.document
                .reparent(id, parent)
                .map_err(|error| error.to_string())?;
            self.moving = None;
        }
        Ok(())
    }

    fn apply(&mut self, action: Action) -> bool {
        if let Err(error) = self.act(action) {
            self.notice = error;
        }
        true
    }

    pub fn text(&mut self, text: &str) -> bool {
        let Some(dialog) = &mut self.dialog else {
            return false;
        };
        let Some(entry) = &mut dialog.entry else {
            return false;
        };
        if dialog.replace_on_input {
            entry.clear();
            dialog.replace_on_input = false;
        }
        entry.push_str(text);
        true
    }

    fn key(&mut self, key: KeyCode) -> bool {
        if let Some(dialog) = &mut self.dialog {
            return match key {
                KeyCode::Escape => self.apply(Action::Cancel),
                KeyCode::Enter | KeyCode::NumpadEnter => self.apply(Action::Confirm),
                KeyCode::Backspace => {
                    let Some(entry) = dialog.entry.as_mut() else {
                        return false;
                    };
                    if dialog.replace_on_input {
                        entry.clear();
                        dialog.replace_on_input = false;
                    } else {
                        entry.pop();
                    }
                    true
                }
                KeyCode::ArrowRight | KeyCode::End => {
                    dialog.replace_on_input = false;
                    true
                }
                _ => false,
            };
        }
        match key {
            KeyCode::Escape => self.apply(Action::Cancel),
            KeyCode::F2 if self.moving.is_none() => self.apply(Action::Rename),
            KeyCode::Delete if self.moving.is_none() => self.apply(Action::Delete),
            _ => false,
        }
    }

    pub fn input(&mut self, input: Input, viewport: Viewport, preview: &mut Preview) -> bool {
        let scale = viewport.display_scale();
        match input {
            Input::PointerMoved { x, y } => {
                self.pointer = Some(Vec2::new(x as f32, y as f32));
                false
            }
            Input::Cancelled => {
                self.pointer = None;
                false
            }
            Input::MouseButton {
                button: MouseButton::Left,
                pressed: true,
            } => {
                let Some(pixel) = self.pointer else {
                    return false;
                };
                let pointer = (pixel.floor() + Vec2::splat(0.5)) / scale;
                let layout = Layout::new(viewport.logical_size());
                let actions = Actions::new(self, &layout);
                if let Some(action) = actions.hit(pointer) {
                    return self.apply(action);
                }
                if self.dialog.is_some() {
                    return false;
                }
                if contains(layout.scene, pointer)
                    && let Some(region) = viewport.pixel_rect(layout.scene)
                {
                    return self.pick(preview, region, pixel, &layout);
                }
                self.moving.is_none()
                    && contains(layout.rows, pointer)
                    && self.apply(Action::Select(None))
            }
            Input::Scroll { delta, .. } if self.dialog.is_none() => {
                let layout = Layout::new(viewport.logical_size());
                if !self
                    .pointer
                    .is_some_and(|point| contains(layout.rows, point / scale))
                {
                    return false;
                }
                let rows = match delta {
                    ScrollDelta::Lines(_, y) => (y * 3.0).round() as isize,
                    ScrollDelta::Pixels(_, y) => (y / (26.0 * f64::from(scale))).round() as isize,
                };
                self.offset = self.offset.saturating_add_signed(rows.saturating_neg());
                self.offset = self
                    .offset
                    .min(self.document.len().saturating_sub(layout.visible_rows()));
                rows != 0
            }
            Input::Key {
                key: PhysicalKey::Code(key),
                pressed: true,
                repeat,
            } if !repeat || key == KeyCode::Backspace => self.key(key),
            _ => false,
        }
    }

    fn pick(
        &mut self,
        preview: &mut Preview,
        region: [u32; 4],
        pixel: Vec2,
        layout: &Layout,
    ) -> bool {
        let id = preview.pick(&self.document, region, pixel);
        if self.moving.is_some() {
            return id.is_some() && self.apply(Action::Select(id));
        }
        self.apply(Action::Select(id));
        if let Some(id) = id {
            self.reveal_row(id, layout.visible_rows());
        }
        true
    }

    fn reveal_row(&mut self, id: ObjectId, visible: usize) {
        if visible == 0 {
            return;
        }
        let rows = self.document.hierarchy();
        if let Some(index) = rows.iter().position(|row| row.id == id) {
            let offset = self.offset.min(rows.len().saturating_sub(visible));
            self.offset = offset.clamp((index + 1).saturating_sub(visible), index);
        }
    }
}

pub(crate) fn contains(rect: Rect, point: Vec2) -> bool {
    rect.min.cmple(point).all() && point.cmplt(rect.max).all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(state: &mut Hierarchy<'_>, preview: &mut Preview, x: f64, y: f64) -> bool {
        click_pixel(
            state,
            preview,
            Viewport::new([1920, 1260], 2.0).unwrap(),
            x * 2.0,
            y * 2.0,
        )
    }

    fn click_pixel(
        state: &mut Hierarchy<'_>,
        preview: &mut Preview,
        viewport: Viewport,
        x: f64,
        y: f64,
    ) -> bool {
        state.input(Input::PointerMoved { x, y }, viewport, preview);
        state.input(
            Input::MouseButton {
                button: MouseButton::Left,
                pressed: true,
            },
            viewport,
            preview,
        )
    }

    #[test]
    fn hierarchy_controls_surface_cycle_errors_and_reopen_the_saved_level() {
        let registry = crate::document::registry();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.scene.ron");
        let mut state = Hierarchy::new(Document::new(&registry));
        let mut preview = Preview::new();

        assert!(click(&mut state, &mut preview, 35.0, 115.0)); // Add root
        let root = state.document.selected.unwrap();
        assert!(state.key(KeyCode::F2));
        assert!(state.text("Старт"));
        assert!(state.key(KeyCode::Enter));
        assert!(click(&mut state, &mut preview, 35.0, 115.0)); // Add child
        let child = state.document.selected.unwrap();

        assert!(click(&mut state, &mut preview, 38.0, 155.0)); // Select root
        assert!(click(&mut state, &mut preview, 200.0, 115.0)); // Move
        assert!(click(&mut state, &mut preview, 38.0, 181.0)); // Attempt to parent root under its child
        assert!(state.notice.contains("would create a cycle"));
        assert_eq!(state.moving, Some(root));
        assert_eq!(state.document.hierarchy()[1].depth, 1);
        assert!(state.key(KeyCode::Escape));
        assert_eq!(state.document.selected, Some(root));

        assert!(click(&mut state, &mut preview, 385.0, 52.0)); // Save
        assert!(state.accepts_text());
        assert!(state.text(&path.to_string_lossy()));
        assert!(state.key(KeyCode::Enter));
        assert!(state.notice.is_empty(), "{}", state.notice);
        assert!(!state.document.is_dirty());
        assert!(click(&mut state, &mut preview, 244.0, 52.0)); // Open
        assert!(state.key(KeyCode::Enter)); // Reopen the current path
        assert!(state.notice.is_empty(), "{}", state.notice);
        assert_eq!(state.document.selected, Some(root));
        assert_eq!(state.document.name(root), "Старт");
        assert_eq!(state.document.hierarchy()[1].id, child);
    }

    #[test]
    fn viewport_pick_reveals_the_row_and_hierarchy_selection_updates_the_cube() {
        let registry = crate::document::registry();
        let mut state = Hierarchy::new(Document::new(&registry));
        state.document.add().unwrap();
        let root = state.document.selected.unwrap();
        state.document.add().unwrap();
        let child = state.document.selected.unwrap();
        state.document.selected = None;

        let viewport = Viewport::new([1920, 500], 2.0).unwrap();
        let layout = Layout::new(viewport.logical_size());
        assert_eq!(layout.visible_rows(), 1);
        let region @ [x, y, width, height] = viewport.pixel_rect(layout.scene).unwrap();
        let mut preview = Preview::new();
        assert_eq!(preview.update(&state.document, Some(region)), None);
        let child_index = usize::from(child > root);
        let child_object = preview.scene.objects[child_index];
        let clip = preview
            .scene
            .camera
            .unwrap()
            .clip_from_world([width, height])
            .unwrap();
        let ndc = clip.project_point3(child_object.transform.w_axis.truncate());
        let pixel = [
            f64::from(x) + f64::from(ndc.x + 1.0) * 0.5 * f64::from(width),
            f64::from(y) + f64::from(1.0 - ndc.y) * 0.5 * f64::from(height),
        ];

        assert!(click_pixel(
            &mut state,
            &mut preview,
            viewport,
            pixel[0],
            pixel[1]
        ));
        assert_eq!(state.document.selected, Some(child));
        assert_eq!(state.offset, 1);
        assert!(
            Actions::new(&state, &layout).hit(layout.rows.min + Vec2::ONE)
                == Some(Action::Select(Some(child)))
        );
        assert_eq!(preview.update(&state.document, Some(region)), None);
        let selected_color = preview.scene.objects[child_index].color;
        assert_ne!(selected_color, child_object.color);

        assert!(click_pixel(
            &mut state,
            &mut preview,
            viewport,
            f64::from(x),
            f64::from(y)
        ));
        assert_eq!(state.document.selected, None);
        assert_eq!(preview.update(&state.document, Some(region)), None);
        assert_eq!(preview.scene.objects[child_index].color, child_object.color);

        state.offset = 0;
        assert!(click_pixel(&mut state, &mut preview, viewport, 76.0, 310.0));
        assert_eq!(state.document.selected, Some(root));
        assert_eq!(preview.update(&state.document, Some(region)), None);
        assert_eq!(preview.scene.objects[1 - child_index].color, selected_color);

        assert!(state.key(KeyCode::Delete));
        assert!(!click_pixel(
            &mut state,
            &mut preview,
            viewport,
            pixel[0],
            pixel[1]
        ));
        assert_eq!(state.document.selected, Some(root));
        assert!(state.key(KeyCode::Enter));
        assert_eq!(state.document.selected, None);
        assert_eq!(preview.update(&state.document, Some(region)), None);
        assert!(preview.scene.objects.is_empty());
    }
}
