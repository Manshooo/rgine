use std::{error::Error, fs, io::Write, path::Path};

pub(super) fn run(base: &Path, current: &Path, other: &Path) -> Result<(), Box<dyn Error>> {
    let read = |path: &Path| {
        fs::read_to_string(path).map_err(|error| format!("reading {}: {error}", path.display()))
    };
    let current_text = read(current)?;
    let merged = engine_scene::merge_scene_ron(&read(base)?, &current_text, &read(other)?)?;
    if merged == current_text {
        return Ok(());
    }
    let directory = current
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    output.write_all(merged.as_bytes())?;
    output.persist(current)?;
    Ok(())
}
