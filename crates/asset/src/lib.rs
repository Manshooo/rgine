//! Stable source asset identities and versioned RON sidecars.

mod cache;
pub mod gltf_prop;
pub mod ktx2;
mod sources;
pub use cache::{ImportCache, ImportSettings, Importer};
pub use sources::SourceFiles;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use uuid::Uuid as AssetGuid;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sidecar {
    schema_version: u32,
    guid: AssetGuid,
    importer_settings: BTreeMap<String, ron::Value>,
}

impl Sidecar {
    fn read(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let meta: Self = ron::de::from_bytes(&bytes)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
        if meta.schema_version != 1 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("unsupported asset schema version {}", meta.schema_version),
            ));
        }
        Ok(meta)
    }

    fn load_or_create(path: &Path) -> io::Result<Self> {
        match Self::read(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            result => return result,
        }
        let meta = Self {
            schema_version: 1,
            guid: AssetGuid::new_v4(),
            importer_settings: BTreeMap::new(),
        };
        let ron =
            ron::ser::to_string_pretty(&meta, Default::default()).map_err(io::Error::other)?;
        // Scans skip the temporary .meta file. Publish only complete sidecars
        // and keep the winning identity if another scanner created one first.
        let mut temp = tempfile::Builder::new()
            .suffix(".meta")
            .tempfile_in(path.parent().expect("sidecar has a source directory"))?;
        writeln!(temp, "{ron}")?;
        match temp.persist_noclobber(path) {
            Ok(_) => Ok(meta),
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Self::read(path),
            Err(error) => Err(error.error),
        }
    }
}

#[derive(Debug)]
pub struct SourceAsset {
    /// Source path relative to the scanned root.
    pub path: PathBuf,
    pub importer_settings: ImportSettings,
}

#[derive(Debug)]
pub struct AssetIndex {
    assets: BTreeMap<AssetGuid, SourceAsset>,
}

impl AssetIndex {
    /// Scan a dedicated source asset directory, including its subdirectories.
    /// Symlinks and files with the reserved .meta suffix are skipped.
    /// A duplicate GUID or unreadable sidecar fails the entire index build.
    pub fn scan(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref();
        let mut dirs = vec![PathBuf::new()];
        let mut assets = BTreeMap::<AssetGuid, SourceAsset>::new();
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(root.join(&dir))? {
                let entry = entry?;
                let name = entry.file_name();
                let path = dir.join(&name);
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if !kind.is_file()
                    || name
                        .as_encoded_bytes()
                        .to_ascii_lowercase()
                        .ends_with(b".meta")
                {
                    continue;
                }
                let mut sidecar = entry.path().into_os_string();
                sidecar.push(".meta");
                let sidecar = Path::new(&sidecar);
                let meta = Sidecar::load_or_create(sidecar).map_err(|error| {
                    io::Error::new(error.kind(), format!("{}: {error}", sidecar.display()))
                })?;
                if let Some(other) = assets.get(&meta.guid) {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "duplicate asset GUID {}: {} and {}",
                            meta.guid,
                            other.path.display(),
                            path.display()
                        ),
                    ));
                }
                assets.insert(
                    meta.guid,
                    SourceAsset {
                        path,
                        importer_settings: meta.importer_settings,
                    },
                );
            }
        }
        Ok(Self { assets })
    }

    pub fn get(&self, guid: AssetGuid) -> Option<&SourceAsset> {
        self.assets.get(&guid)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (AssetGuid, &SourceAsset)> {
        self.assets.iter().map(|(&guid, asset)| (guid, asset))
    }
}
