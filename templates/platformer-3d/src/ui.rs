use std::{cell::RefCell, error::Error, rc::Rc};

use engine_app::canvas::{Canvas, Viewport};
use engine_app::platform::{Input, InputEvent, MouseButton, TouchPhase};
use engine_app::render_resources::{ResourceError, ResourceTable};
use engine_ui::canvas::UiDrawList;
use engine_ui::geometry;
use engine_ui::input::{InputResult, UiInput};
use engine_ui::text::{FontBook, canvas::GlyphAtlas};
use engine_ui::{StyleSheet, WidgetTree};

use crate::game::Run;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Resume,
    Restart,
}

#[derive(Default)]
pub struct Interaction {
    input: UiInput<MenuAction>,
    dirty: bool,
}

impl Interaction {
    pub fn route(&mut self, event: &InputEvent, paused: bool) -> InputResult<MenuAction> {
        let result = self.input.route(event);
        self.dirty |= paused
            && (result.consumed()
                || matches!(
                    event.input,
                    Input::PointerMoved { .. }
                        | Input::Cancelled
                        | Input::MouseButton {
                            button: MouseButton::Left,
                            pressed: true
                        }
                        | Input::Touch {
                            phase: TouchPhase::Started,
                            ..
                        }
                ));
        result
    }

    pub fn reset(&mut self) {
        self.input.reset();
        self.dirty = true;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Hud {
    coins: usize,
    total: usize,
    checkpoint: bool,
    paused: bool,
}

struct Prepared {
    viewport: Viewport,
    state: Hud,
    hud: UiDrawList,
    menu: Option<UiDrawList>,
}

pub struct Overlay {
    hud: WidgetTree,
    pause_menu: WidgetTree,
    styles: StyleSheet,
    fonts: FontBook,
    interaction: Rc<RefCell<Interaction>>,
    atlas: GlyphAtlas,
    prepared: Option<Prepared>,
}

impl Overlay {
    pub fn new(interaction: Rc<RefCell<Interaction>>) -> Result<Self, Box<dyn Error>> {
        let mut fonts = FontBook::default();
        fonts.add_face(
            "ABeeZee",
            include_bytes!("../assets/fonts/ABeeZee-Regular.ttf").to_vec(),
            0,
            "Platformer",
            400,
        )?;
        fonts.set_default_families(["Platformer"]);
        Ok(Self {
            hud: WidgetTree::parse("hud.xml", include_str!("../assets/hud.xml"))?,
            pause_menu: WidgetTree::parse("pause.xml", include_str!("../assets/pause.xml"))?,
            styles: StyleSheet::parse("ui.css", include_str!("../assets/ui.css"))?,
            fonts,
            interaction,
            atlas: GlyphAtlas::default(),
            prepared: None,
        })
    }

    pub fn draw(
        &mut self,
        run: &Run,
        viewport: Viewport,
        resources: &mut ResourceTable,
        canvas: &mut Canvas,
    ) -> Result<(), ResourceError> {
        self.prepare(run, viewport).map_err(resource_error)?;
        let prepared = self.prepared.as_ref().expect("prepared overlay");
        let mut frame = self.atlas.begin_frame(resources).map_err(resource_error)?;
        prepared
            .hud
            .draw(canvas, viewport, &mut frame, |_| None)
            .map_err(resource_error)?;
        if let Some(menu) = &prepared.menu {
            menu.draw(canvas, viewport, &mut frame, |_| None)
                .map_err(resource_error)?;
        }
        Ok(())
    }

    pub fn prepare(
        &mut self,
        run: &Run,
        viewport: Viewport,
    ) -> Result<(), geometry::GeometryError> {
        let mut interaction = self.interaction.borrow_mut();
        let state = Hud {
            coins: run.coins,
            total: run.total,
            checkpoint: run.checkpoint.is_some(),
            paused: run.paused,
        };
        if !interaction.dirty
            && self
                .prepared
                .as_ref()
                .is_some_and(|prepared| prepared.viewport == viewport && prepared.state == state)
        {
            return Ok(());
        }
        let size = viewport.logical_size().to_array();
        let hud = self.styles.resolve(&self.hud, |_, _| Default::default());
        let coins = format!("{} / {}", state.coins, state.total);
        let progress = if state.coins == state.total {
            "All coins collected"
        } else if state.checkpoint {
            "Checkpoint active"
        } else {
            "Find the violet checkpoint"
        };
        let hud = geometry::layout_with_text(
            &hud,
            size,
            &self.fonts,
            |_| None,
            |name| match name {
                "coins" => Some(&coins),
                "progress" => Some(progress),
                _ => None,
            },
        )?;
        let menu = if state.paused {
            let styled = self
                .styles
                .resolve(&self.pause_menu, |path, _| interaction.input.state(path));
            let bounds = geometry::layout(&styled, size, &self.fonts, |_| None)?;
            interaction
                .input
                .set_layout(&bounds, viewport.display_scale(), |_, name| match name {
                    "resume" => Some(MenuAction::Resume),
                    "restart" => Some(MenuAction::Restart),
                    _ => None,
                });
            Some(UiDrawList::prepare(bounds))
        } else {
            None
        };
        self.prepared = Some(Prepared {
            viewport,
            state,
            hud: UiDrawList::prepare(hud),
            menu,
        });
        interaction.dirty = false;
        Ok(())
    }
}

fn resource_error(error: impl std::fmt::Display) -> ResourceError {
    ResourceError::InvalidDescriptor(error.to_string())
}
