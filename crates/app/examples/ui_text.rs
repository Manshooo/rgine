//! Exercise cached HUD text on the normal window and Canvas APIs.

use engine_app::App;
use engine_app::canvas::{Affine2, Rect, Vec2};
use engine_app::platform::WindowDesc;
use engine_app::render_resources::ResourceError;
use engine_app::scene::MeshRegistry;
use engine_ui::Typography;
use engine_ui::text::canvas::GlyphAtlas;
use engine_ui::text::{FontBook, TextLayout};

const MIXED: &str = "Score / Счёт";
const WRAPPED: &str = "Keep moving across the platforms. Собирайте монеты и продолжайте путь.";

struct Labels {
    factor: f32,
    score_style: Typography,
    mixed: TextLayout,
    wrapped: TextLayout,
}

impl Labels {
    fn prepare(fonts: &FontBook, factor: f32) -> Result<Self, ResourceError> {
        let style = |size| Typography {
            font_size: size * factor,
            ..Default::default()
        };
        Ok(Self {
            factor,
            score_style: style(30.0),
            mixed: fonts
                .prepare(MIXED, &style(18.0), None)
                .map_err(drawing_error)?,
            wrapped: fonts
                .prepare(WRAPPED, &style(16.0), Some(222.0 * factor))
                .map_err(drawing_error)?,
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine_app::core::init_logging();
    let scale = std::env::args()
        .nth(1)
        .map(|scale| scale.parse::<f32>())
        .transpose()?;
    if scale.is_some_and(|scale| !scale.is_finite() || scale <= 0.0) {
        return Err("the optional display scale must be finite and positive".into());
    }
    let mut fonts = FontBook::default();
    fonts.add_face(
        "ABeeZee",
        include_bytes!("../../ui/tests/fixtures/fonts/ABeeZee-Regular.ttf").to_vec(),
        0,
        "Primary",
        400,
    )?;
    fonts.add_face(
        "PT Serif",
        include_bytes!("../../ui/tests/fixtures/fonts/PT_Serif-Web-Regular.ttf").to_vec(),
        0,
        "Fallback",
        400,
    )?;
    fonts.set_default_families(["Primary", "Fallback"]);
    let mut atlas = GlyphAtlas::default();
    let mut labels = None;
    let mut counter = 0_u64;
    let app = App::new()
        .with_window(WindowDesc::new("rgine - glyph atlas").with_size(720, 480))
        .with_canvas(move |_, viewport, resources, canvas| {
            let display_scale = scale.unwrap_or(viewport.display_scale());
            let factor = display_scale / viewport.display_scale();
            if labels
                .as_ref()
                .is_none_or(|labels: &Labels| labels.factor != factor)
            {
                labels = Some(Labels::prepare(&fonts, factor)?);
            }
            let labels = labels.as_ref().expect("prepared fixture labels");
            let place = |x, y| Vec2::new(x, y) * factor;
            let outer = Rect::from_min_size(place(16.0, 16.0), place(256.0, 152.0));
            canvas.solid_quad(outer, Affine2::IDENTITY, [0.03, 0.08, 0.13, 0.96]);
            let score = match counter {
                0 => "0123456789".to_owned(),
                12 => "-0012".to_owned(),
                _ => format!("{:04}", counter % 10_000),
            };
            let score = fonts
                .prepare(&score, &labels.score_style, None)
                .map_err(drawing_error)?;
            let mut frame = atlas.begin_frame(resources).map_err(drawing_error)?;
            frame
                .draw_text(
                    canvas,
                    viewport,
                    &labels.mixed,
                    place(30.0, 27.0),
                    [0.68, 0.84, 0.94, 1.0],
                )
                .map_err(drawing_error)?;
            frame
                .draw_text(
                    canvas,
                    viewport,
                    &score,
                    place(30.0, 52.0),
                    [0.22, 0.92, 0.71, 1.0],
                )
                .map_err(drawing_error)?;
            let clipped = Rect::from_min_size(place(30.0, 100.0), place(222.0, 50.0));
            canvas.push_clip(clipped);
            frame
                .draw_text(
                    canvas,
                    viewport,
                    &labels.wrapped,
                    clipped.min,
                    [0.9, 0.91, 0.95, 1.0],
                )
                .map_err(drawing_error)?;
            canvas.pop_clip();

            let stats = frame.stats();
            if counter < 15 || stats.uploads > 0 {
                eprintln!(
                    "glyph frame {counter} scale={display_scale} rasterized={} uploads={} \
                     bytes={} pages={} clipped_lines={}",
                    stats.rasterized,
                    stats.uploads,
                    stats.uploaded_bytes,
                    stats.resident_pages,
                    labels.wrapped.lines.len(),
                );
            }
            counter += 1;
            Ok(())
        });
    app.run_scene(MeshRegistry::default(), [0.012, 0.022, 0.04, 1.0])?;
    Ok(())
}

fn drawing_error(error: impl std::fmt::Display) -> ResourceError {
    ResourceError::InvalidDescriptor(error.to_string())
}
