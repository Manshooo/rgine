#![forbid(unsafe_code)]

mod game;
mod level;
mod level_data;
mod tuning;
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
    #[cfg(feature = "dev-reload")]
    let (app, development) = {
        use engine_app::hot_reload::RonReload;
        use std::{ffi::OsStr, path::PathBuf};

        let mut app = app;
        let args = std::env::args_os().skip(1).collect::<Vec<_>>();
        let development = match args.as_slice() {
            [] => None,
            [flag, dir] if flag == OsStr::new("--dev") => {
                let dir = PathBuf::from(dir);
                overlay.watch_hud(&dir)?;
                let path = dir.join("tuning.ron");
                let (watcher, tuning) = RonReload::load(path.clone(), tuning::Tuning::validate)?;
                app.world_mut().resources_mut().insert(tuning);
                Some((path, watcher))
            }
            _ => return Err("usage: platformer-3d [--dev asset-directory]".into()),
        };
        (app, development)
    };
    let app = app.with_canvas(move |world, viewport, resources, canvas| {
        let run = world.get::<Run>(player).expect("platformer run");
        overlay.draw(&run, viewport, resources, canvas)
    });
    const CLEAR: [f64; 4] = [0.023, 0.043, 0.080, 1.0];
    #[cfg(feature = "dev-reload")]
    if let Some((path, mut watcher)) = development {
        app.run_scene_with_refresh(meshes, CLEAR, move |world, _| {
            let tuning = world.resources_mut().get_mut::<tuning::Tuning>().unwrap();
            match watcher.poll(tuning) {
                Ok(true) => {
                    eprintln!("platformer: reloaded {}", path.display());
                    true
                }
                Ok(false) => false,
                Err(error) => {
                    eprintln!("platformer: {error}");
                    false
                }
            }
        })?;
        return Ok(());
    }
    app.run_scene(meshes, CLEAR)?;
    Ok(())
}
