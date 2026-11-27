//! Draw authored UI over a scene and reload the XML and CSS while it runs.

mod controls;

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use engine_app::canvas::{Canvas, Viewport};
use engine_app::core::glam::{Quat, Vec3};
use engine_app::hot_reload::{UiDocument, UiReload};
use engine_app::platform::WindowDesc;
use engine_app::render_resources::{
    ResourceError, ResourceTable, SampledBinding, SampledGroupHandle, SamplerDescriptor,
    TextureDescriptor,
};
use engine_app::scene::{Camera, Mesh, MeshRegistry, Renderable, Transform};
use engine_app::{App, RenderError};
use engine_ui::canvas::UiDrawList;
use engine_ui::geometry;
use engine_ui::text::{FontBook, FontLoadError, canvas::GlyphAtlas};

const MARKER: &str = "marker";
const MARKER_SIZE: [u32; 2] = [2, 2];
const MARKER_RGBA: &[u8] = &[
    255, 215, 71, 255, 31, 132, 253, 255, 31, 132, 253, 255, 255, 215, 71, 255,
];

use controls::Controls;

struct Overlay {
    reload: UiReload,
    document: UiDocument,
    fonts: FontBook,
    atlas: GlyphAtlas,
    image: Option<SampledGroupHandle>,
    controls: Rc<RefCell<Controls>>,
    prepared: Option<(Viewport, UiDrawList)>,
}

impl Overlay {
    fn draw(
        &mut self,
        viewport: Viewport,
        resources: &mut ResourceTable,
        canvas: &mut Canvas,
    ) -> Result<(), ResourceError> {
        match self.reload.poll(&mut self.document) {
            Ok(true) => {
                self.prepared = None;
                self.controls.borrow_mut().input.reset();
                eprintln!("ui overlay: applied authored edit");
            }
            Ok(false) => {}
            Err(error) => eprintln!("ui overlay: {error}"),
        }
        let size = viewport.logical_size().to_array();
        let viewport_changed = self
            .prepared
            .as_ref()
            .is_none_or(|(previous, _)| *previous != viewport);
        let mut controls = self.controls.borrow_mut();
        if controls.dirty || viewport_changed {
            let root = self
                .document
                .styles
                .resolve(&self.document.tree, |path, widget| {
                    controls.state(path, widget)
                });
            let coins = controls.coins.to_string();
            let geometry = geometry::layout_with_text(
                &root,
                size,
                &self.fonts,
                |src| (src == MARKER).then_some(MARKER_SIZE.map(|extent| extent as f32)),
                |binding| controls.text(binding, &coins),
            )
            .map_err(drawing_error)?;
            controls.set_layout(&geometry, viewport.display_scale());
            controls.dirty = false;
            self.prepared = Some((viewport, UiDrawList::prepare(geometry)));
            if viewport_changed {
                eprintln!(
                    "ui overlay: prepared {}x{} logical pixels",
                    size[0], size[1]
                );
            }
        }
        drop(controls);
        let marker = self.marker(resources)?;
        let prepared = &self.prepared.as_ref().expect("prepared current viewport").1;
        let mut glyphs = self.atlas.begin_frame(resources).map_err(drawing_error)?;
        prepared
            .draw(canvas, viewport, &mut glyphs, |src| {
                (src == MARKER).then_some(marker)
            })
            .map_err(drawing_error)
    }

    fn marker(
        &mut self,
        resources: &mut ResourceTable,
    ) -> Result<SampledGroupHandle, ResourceError> {
        if let Some(marker) = self
            .image
            .filter(|&marker| resources.sampled_binding(marker).is_ok())
        {
            return Ok(marker);
        }
        let texture = resources.create_texture(TextureDescriptor::sampled_rgba8(MARKER_SIZE))?;
        resources.write_texture(texture, 0, 0, MARKER_RGBA)?;
        let sampler = resources.create_sampler(SamplerDescriptor::NEAREST_CLAMP)?;
        let marker = resources.create_sampled_group(SampledBinding { texture, sampler })?;
        self.image = Some(marker);
        Ok(marker)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine_app::core::init_logging();
    let mut args = std::env::args_os().skip(1);
    let directory = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fixtures/ui_overlay")
    });
    let width = args
        .next()
        .map(|arg| arg.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(720);
    let (reload, document) = UiReload::load(directory.join("hud.xml"), directory.join("hud.css"))?;
    let controls = Rc::new(RefCell::new(Controls::default()));
    let mut overlay = Overlay {
        reload,
        document,
        fonts: fonts()?,
        atlas: GlyphAtlas::default(),
        image: None,
        controls: Rc::clone(&controls),
        prepared: None,
    };
    let app = App::new()
        .with_window(WindowDesc::new("rgine - authored UI").with_size(width, 480))
        .with_canvas(move |_, viewport, resources, canvas| {
            overlay.draw(viewport, resources, canvas)
        });
    run_scene(app, controls)?;
    Ok(())
}

fn fonts() -> Result<FontBook, FontLoadError> {
    let mut fonts = FontBook::default();
    fonts.add_face(
        "PT Serif",
        include_bytes!("../../../ui/tests/fixtures/fonts/PT_Serif-Web-Regular.ttf").to_vec(),
        0,
        "HUD",
        400,
    )?;
    fonts.set_default_families(["HUD"]);
    Ok(fonts)
}

fn run_scene(mut app: App, controls: Rc<RefCell<Controls>>) -> Result<(), RenderError> {
    let mut meshes = MeshRegistry::default();
    let mesh = meshes.insert(Mesh::cube().with_checkerboard());
    let world = app.world_mut();
    let camera = world.spawn_empty();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::default()).unwrap();
    let cube = world.spawn_empty();
    world
        .insert(
            cube,
            Renderable {
                mesh,
                color: [0.17, 0.81, 0.66, 1.0],
            },
        )
        .unwrap();
    world
        .insert(
            cube,
            Transform {
                translation: Vec3::new(0.0, 0.5, -3.4),
                rotation: Quat::from_rotation_y(0.65) * Quat::from_rotation_x(0.3),
                scale: Vec3::splat(2.0),
            },
        )
        .unwrap();
    controls::install(&mut app, cube, controls);
    app.run_scene(meshes, [0.016, 0.023, 0.035, 1.0])
}

fn drawing_error(error: impl std::fmt::Display) -> ResourceError {
    ResourceError::InvalidDescriptor(error.to_string())
}
