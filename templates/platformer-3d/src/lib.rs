#![forbid(unsafe_code)]

mod game;
mod level;
mod level_data;
mod ui;

#[cfg(test)]
mod tests;

use game::{Game, Run};
use ui::Overlay;

#[cfg(target_os = "android")]
engine_app::platform::android_entrypoint!(run_android);

#[cfg(target_os = "android")]
fn run_android() {
    if let Err(error) = run() {
        eprintln!("{error}");
    }
}

/// Launches the platformer on the active platform event loop.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    engine_app::core::init_logging();
    let Game {
        app,
        meshes,
        player,
        interaction,
    } = Game::build()?;
    let mut overlay = Overlay::new(interaction, cfg!(target_os = "android"))?;
    app.with_canvas(move |world, viewport, resources, canvas| {
        let run = world.get::<Run>(player).expect("platformer run");
        overlay.draw(&run, viewport, resources, canvas)
    })
    .run_scene(meshes, [0.023, 0.043, 0.080, 1.0])?;
    Ok(())
}
