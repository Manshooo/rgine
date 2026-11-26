use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use engine_asset::SourceFiles;
use engine_render::{Drawable, SceneShaderSources};

pub(super) struct ShaderReload {
    directory: PathBuf,
    inputs: Option<SourceFiles>,
    active: Option<SceneShaderSources>,
    last_poll: Option<Instant>,
}

impl ShaderReload {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            inputs: None,
            active: None,
            last_poll: None,
        }
    }

    pub(super) fn attach(&mut self, drawable: &mut Drawable) {
        if let Some(active) = &self.active {
            if let Err(error) = drawable.reload_scene_shaders(active) {
                eprintln!("shader reload {}: {error}", self.directory.display());
            }
        } else {
            self.poll(drawable);
        }
    }

    pub(super) fn poll(&mut self, drawable: &mut Drawable) -> bool {
        if self
            .last_poll
            .is_some_and(|last| last.elapsed() < Duration::from_millis(100))
        {
            return false;
        }
        self.last_poll = Some(Instant::now());
        if self.inputs.as_ref().is_some_and(|files| !files.changed()) {
            return false;
        }
        let mut tracked = SourceFiles::default();
        let sources = SceneShaderSources::load(&self.directory, &mut tracked);
        self.inputs = Some(tracked);
        let result = sources
            .map_err(|error| error.to_string())
            .and_then(|sources| {
                let changed = drawable
                    .reload_scene_shaders(&sources)
                    .map_err(|error| error.to_string())?;
                self.active = Some(sources);
                Ok(changed)
            });
        match result {
            Ok(changed) => changed,
            Err(error) => {
                eprintln!("shader reload {}: {error}", self.directory.display());
                false
            }
        }
    }
}
