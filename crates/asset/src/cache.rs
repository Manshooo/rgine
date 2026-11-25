use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Settings read from a source asset's sidecar.
pub type ImportSettings = BTreeMap<String, ron::Value>;

/// A deterministic conversion of source bytes and settings into runtime bytes.
pub trait Importer {
    /// Globally namespaced importer name and version, e.g. `"rgine.mesh/1"`.
    /// Change it whenever the conversion or artifact format changes.
    const VERSION: &'static str;

    /// Write the entire artifact using only these inputs.
    fn import(source: &[u8], settings: &ImportSettings, output: &mut dyn Write) -> io::Result<()>;
}

/// A directory of immutable, content-addressed import artifacts.
pub struct ImportCache {
    root: PathBuf,
}

impl ImportCache {
    /// Choose the cache directory. Keep it outside the scanned source tree.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the complete artifact's path, importing and publishing it on a miss.
    /// Its filename is the portable cache key, independent of the checkout.
    pub fn get_or_import<I: Importer>(
        &self,
        source: &[u8],
        settings: &ImportSettings,
    ) -> io::Result<PathBuf> {
        // Maps are ordered. Pin line endings and preserve numeric types across hosts.
        let config = ron::ser::PrettyConfig::new()
            .new_line("\n")
            .number_suffixes(true);
        let normalized = ron::ser::to_string_pretty(settings, config).map_err(io::Error::other)?;
        let mut hash = blake3::Hasher::new_derive_key("rgine import cache v1");
        for field in [I::VERSION.as_bytes(), normalized.as_bytes(), source] {
            hash.update(&(field.len() as u64).to_le_bytes());
            hash.update(field);
        }
        let path = self.root.join(hash.finalize().to_hex().as_str());
        if Path::try_exists(&path)? {
            return Ok(path);
        }
        fs::create_dir_all(&self.root)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        I::import(source, settings, temp.as_file_mut())?;
        temp.as_file().sync_all()?;
        match temp.persist_noclobber(&path) {
            Ok(_) => Ok(path),
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(path),
            Err(error) => Err(error.error),
        }
    }
}
