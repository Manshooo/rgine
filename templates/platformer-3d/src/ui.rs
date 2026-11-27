use std::{cell::RefCell, error::Error, rc::Rc};

use engine_app::ActionBinding;
use engine_app::canvas::{Canvas, Viewport};
#[cfg(feature = "dev-reload")]
use engine_app::hot_reload::{UiDocument, UiReload};
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
    Pause,
    Resume,
    Restart,
}

#[derive(Clone, Copy)]
pub enum TouchButton {
    Left,
    Right,
    Jump,
}

impl TouchButton {
    pub fn binding(self) -> ActionBinding {
        ActionBinding::TouchButton(self as u16)
    }

    fn named(name: &str) -> Option<Self> {
        match name {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "jump" => Some(Self::Jump),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct Interaction {
    input: UiInput<MenuAction>,
    touch_targets: Vec<(TouchButton, geometry::Rect)>,
    dirty: bool,
}

impl Interaction {
    pub fn route(&mut self, event: &InputEvent, paused: bool) -> InputResult<MenuAction> {
        let result = self.input.route(event);
        self.dirty |= result.consumed()
            || (paused
                && matches!(
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
        self.touch_targets.clear();
        self.dirty = true;
    }

    pub fn touch_button(&self, event: &InputEvent) -> Option<TouchButton> {
        let Input::Touch {
            phase: TouchPhase::Started,
            x,
            y,
            ..
        } = event.input
        else {
            return None;
        };
        self.touch_targets
            .iter()
            .rev()
            .find_map(|&(button, bounds)| bounds.contains([x as f32, y as f32]).then_some(button))
    }
}

fn touch_targets(
    node: &geometry::WidgetBox<'_, '_>,
    scale: f32,
    targets: &mut Vec<(TouchButton, geometry::Rect)>,
) {
    if let Some(button) = node.styled.widget.action().and_then(TouchButton::named) {
        targets.push((button, node.bounds.physical(scale)));
    }
    for child in &node.children {
        touch_targets(child, scale, targets);
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
    foreground: Option<UiDrawList>,
}

pub struct Overlay {
    hud: WidgetTree,
    pause_menu: WidgetTree,
    touch_controls: Option<WidgetTree>,
    styles: StyleSheet,
    fonts: FontBook,
    interaction: Rc<RefCell<Interaction>>,
    atlas: GlyphAtlas,
    prepared: Option<Prepared>,
    #[cfg(feature = "dev-reload")]
    development: Option<(UiReload, UiDocument)>,
}

impl Overlay {
    pub fn new(interaction: Rc<RefCell<Interaction>>, touch: bool) -> Result<Self, Box<dyn Error>> {
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
            touch_controls: touch
                .then(|| WidgetTree::parse("touch.xml", include_str!("../assets/touch.xml")))
                .transpose()?,
            styles: StyleSheet::parse("ui.css", include_str!("../assets/ui.css"))?,
            fonts,
            interaction,
            atlas: GlyphAtlas::default(),
            prepared: None,
            #[cfg(feature = "dev-reload")]
            development: None,
        })
    }

    #[cfg(feature = "dev-reload")]
    pub fn watch_hud(&mut self, dir: &std::path::Path) -> std::io::Result<()> {
        self.development = Some(UiReload::load(dir.join("hud.xml"), dir.join("ui.css"))?);
        self.prepared = None;
        Ok(())
    }

    pub fn draw(
        &mut self,
        run: &Run,
        viewport: Viewport,
        resources: &mut ResourceTable,
        canvas: &mut Canvas,
    ) -> Result<(), ResourceError> {
        #[cfg(feature = "dev-reload")]
        if let Some((reload, document)) = &mut self.development {
            match reload.poll(document) {
                Ok(true) => {
                    self.prepared = None;
                    self.interaction.borrow_mut().reset();
                    eprintln!("platformer: reloaded HUD XML/CSS");
                }
                Ok(false) => {}
                Err(error) => eprintln!("platformer: {error}"),
            }
        }
        self.prepare(run, viewport).map_err(resource_error)?;
        let prepared = self.prepared.as_ref().expect("prepared overlay");
        let mut frame = self.atlas.begin_frame(resources).map_err(resource_error)?;
        prepared
            .hud
            .draw(canvas, viewport, &mut frame, |_| None)
            .map_err(resource_error)?;
        if let Some(foreground) = &prepared.foreground {
            foreground
                .draw(canvas, viewport, &mut frame, |_| None)
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
        let (hud_tree, styles) = self.hud_assets();
        let hud = styles.resolve(hud_tree, |_, _| Default::default());
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
                "controls" if self.touch_controls.is_some() => {
                    Some("Hold a direction and tap Jump")
                }
                _ => None,
            },
        )?;
        let foreground = if state.paused {
            let styled = styles.resolve(&self.pause_menu, |path, _| interaction.input.state(path));
            let bounds = geometry::layout(&styled, size, &self.fonts, |_| None)?;
            interaction
                .input
                .set_layout(&bounds, viewport.display_scale(), |_, name| match name {
                    "resume" => Some(MenuAction::Resume),
                    "restart" => Some(MenuAction::Restart),
                    _ => None,
                });
            Some(UiDrawList::prepare(bounds))
        } else if let Some(tree) = &self.touch_controls {
            let styled = styles.resolve(tree, |path, _| interaction.input.state(path));
            let bounds = geometry::layout(&styled, size, &self.fonts, |_| None)?;
            let scale = viewport.display_scale();
            interaction.input.set_layout(&bounds, scale, |_, name| {
                (name == "pause").then_some(MenuAction::Pause)
            });
            interaction.touch_targets.clear();
            touch_targets(&bounds, scale, &mut interaction.touch_targets);
            Some(UiDrawList::prepare(bounds))
        } else {
            None
        };
        self.prepared = Some(Prepared {
            viewport,
            state,
            hud: UiDrawList::prepare(hud),
            foreground,
        });
        interaction.dirty = false;
        Ok(())
    }

    fn hud_assets(&self) -> (&WidgetTree, &StyleSheet) {
        #[cfg(feature = "dev-reload")]
        if let Some((_, document)) = &self.development {
            return (&document.tree, &document.styles);
        }
        (&self.hud, &self.styles)
    }
}

fn resource_error(error: impl std::fmt::Display) -> ResourceError {
    ResourceError::InvalidDescriptor(error.to_string())
}
