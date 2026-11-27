#![forbid(unsafe_code)]

mod game;
mod level;
mod level_data;
mod ui;

#[cfg(test)]
mod tests;

use game::{Game, Run};
use ui::Overlay;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine_app::core::init_logging();
    let Game {
        app,
        meshes,
        player,
        interaction,
    } = Game::build()?;
    let mut overlay = Overlay::new(interaction)?;
    app.with_canvas(move |world, viewport, resources, canvas| {
        let run = world.get::<Run>(player).expect("platformer run");
        overlay.draw(&run, viewport, resources, canvas)
    })
    .run_scene(meshes, [0.023, 0.043, 0.080, 1.0])?;
    Ok(())
}
