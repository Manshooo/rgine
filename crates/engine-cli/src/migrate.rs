use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Deserialize)]
struct ManifestVersion {
    schema_version: u32,
}

#[derive(Deserialize)]
struct ManifestV1 {
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "engine_version")]
    _engine_version: String,
}

struct ProjectCopy {
    directories: Vec<PathBuf>,
    files: Vec<ProjectFile>,
    migrated: usize,
}

struct ProjectFile {
    path: PathBuf,
    contents: Option<String>,
}

pub(super) fn run(project: &Path, output: &Path) -> Result<usize, String> {
    let root = fs::canonicalize(project).map_err(|error| at(project, error))?;
    let copy = ProjectCopy::read(&root)?;
    if copy.migrated == 0 {
        return Ok(0);
    }

    let name = output
        .file_name()
        .ok_or_else(|| at(output, "expected an output directory name"))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| at(parent, error))?;
    let destination = parent.join(name);
    if destination.starts_with(&root) {
        return Err(at(output, "output must be outside the source project"));
    }
    check_new_destination(&destination)?;

    let staged = tempfile::Builder::new()
        .prefix(".rgine-migrate-")
        .tempdir_in(&parent)
        .map_err(|error| at(&parent, error))?;
    copy.write(&root, staged.path())?;
    check_new_destination(&destination)?;
    fs::rename(staged.path(), &destination).map_err(|error| at(&destination, error))?;
    let _ = staged.keep();
    Ok(copy.migrated)
}

impl ProjectCopy {
    fn read(root: &Path) -> Result<Self, String> {
        let manifest = root.join("project.ron");
        let metadata = fs::symlink_metadata(&manifest).map_err(|error| at(&manifest, error))?;
        if !metadata.is_file() {
            return Err(at(&manifest, "expected a regular project manifest"));
        }
        let contents = fs::read_to_string(&manifest).map_err(|error| at(&manifest, error))?;
        let version: ManifestVersion =
            ron::from_str(&contents).map_err(|error| at(&manifest, error))?;
        if version.schema_version != 1 {
            return Err(at(
                &manifest,
                format!(
                    "unsupported project schema version {}",
                    version.schema_version
                ),
            ));
        }
        ron::from_str::<ManifestV1>(&contents).map_err(|error| at(&manifest, error))?;

        let mut copy = Self {
            directories: Vec::new(),
            files: vec![ProjectFile {
                path: PathBuf::from("project.ron"),
                contents: Some(contents),
            }],
            migrated: 0,
        };
        let mut pending = vec![PathBuf::new()];
        while let Some(directory) = pending.pop() {
            let absolute = root.join(&directory);
            for entry in fs::read_dir(&absolute).map_err(|error| at(&absolute, error))? {
                let entry = entry.map_err(|error| at(&absolute, error))?;
                let path = directory.join(entry.file_name());
                if path == Path::new("project.ron") || path == Path::new(".git") {
                    continue;
                }
                let kind = entry
                    .file_type()
                    .map_err(|error| at(&entry.path(), error))?;
                if kind.is_dir() {
                    pending.push(path.clone());
                    copy.directories.push(path);
                } else if kind.is_file() {
                    copy.read_file(root, path)?;
                } else {
                    return Err(at(&entry.path(), "expected a regular file or directory"));
                }
            }
        }
        Ok(copy)
    }

    fn read_file(&mut self, root: &Path, path: PathBuf) -> Result<(), String> {
        let contents = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) if name.ends_with(".scene.ron") || name.ends_with(".prefab.ron") => {
                let absolute = root.join(&path);
                let text = fs::read_to_string(&absolute).map_err(|error| at(&absolute, error))?;
                let migrated =
                    engine_scene::migrate_scene_ron(&text).map_err(|error| at(&absolute, error))?;
                self.migrated += usize::from(migrated.is_some());
                Some(migrated.unwrap_or(text))
            }
            _ => None,
        };
        self.files.push(ProjectFile { path, contents });
        Ok(())
    }

    fn write(&self, root: &Path, output: &Path) -> Result<(), String> {
        for directory in &self.directories {
            let destination = output.join(directory);
            fs::create_dir(&destination).map_err(|error| at(&destination, error))?;
        }
        for file in &self.files {
            let destination = output.join(&file.path);
            if let Some(contents) = &file.contents {
                fs::write(&destination, contents).map_err(|error| at(&destination, error))?;
            } else {
                let source = root.join(&file.path);
                fs::copy(&source, &destination).map_err(|error| {
                    at(
                        &source,
                        format!("copying to {}: {error}", destination.display()),
                    )
                })?;
            }
        }
        Ok(())
    }
}

fn check_new_destination(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(at(path, error)),
        Ok(_) => Err(at(path, "output already exists")),
    }
}

fn at(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}
