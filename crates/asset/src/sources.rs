use std::{
    collections::BTreeMap,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

/// Fingerprints of the files read during one development import attempt.
///
/// Unreadable files are recorded too, so creating a missing dependency can
/// trigger recovery. Polling reads only recorded paths and catches same-size edits.
#[derive(Default)]
pub struct SourceFiles {
    files: BTreeMap<PathBuf, Result<blake3::Hash, io::ErrorKind>>,
}

impl SourceFiles {
    /// Reads bytes and records the fingerprint of the exact input returned.
    pub fn read(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        let result = fs::read(path);
        self.files.insert(
            path.to_owned(),
            result.as_deref().map(blake3::hash).map_err(io::Error::kind),
        );
        result
    }

    /// Reports whether the contents or accessibility of a recorded file changed.
    pub fn changed(&self) -> bool {
        self.files.iter().any(|(path, previous)| {
            let current = File::open(path).and_then(|file| {
                let mut hasher = blake3::Hasher::new();
                hasher.update_reader(file)?;
                Ok(hasher.finalize())
            });
            current.map_err(|error| error.kind()) != *previous
        })
    }
}
