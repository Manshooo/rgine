use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use harfrust::{Font, font::Blob};

static NEXT_FACE: AtomicU64 = AtomicU64::new(0);

/// A unique font face revision, suitable for glyph cache keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFaceId(u64);

/// A retained reference to the exact face used by a shaped run.
#[derive(Clone)]
pub struct FontFace(Arc<Face>);

struct Face {
    id: FontFaceId,
    family: String,
    weight: u16,
    source: String,
    index: u32,
    data: Blob,
    font: Font,
}

impl fmt::Debug for FontFace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FontFace")
            .field("id", &self.id())
            .field("family", &self.family())
            .field("weight", &self.weight())
            .field("source", &self.source())
            .field("index", &self.index())
            .finish()
    }
}

impl PartialEq for FontFace {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for FontFace {}

impl FontFace {
    /// Unique identity of this loaded revision.
    pub fn id(&self) -> FontFaceId {
        self.0.id
    }

    /// Project family name used to register this face.
    pub fn family(&self) -> &str {
        &self.0.family
    }

    /// Registered font weight.
    pub fn weight(&self) -> u16 {
        self.0.weight
    }

    /// Source asset name or path used in diagnostics.
    pub fn source(&self) -> &str {
        &self.0.source
    }

    /// Face index within a collection, or zero for a standalone face.
    pub fn index(&self) -> u32 {
        self.0.index
    }

    /// Source bytes for rasterizing the glyph identifiers of this face.
    pub fn bytes(&self) -> &[u8] {
        self.0.data.as_ref()
    }

    pub(super) fn font(&self) -> &Font {
        &self.0.font
    }
}

/// A font asset read or validation error with its source name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontLoadError {
    /// Asset name or filesystem path.
    pub source: String,
    /// Description of the failed load.
    pub message: String,
}

impl fmt::Display for FontLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.source, self.message)
    }
}

impl std::error::Error for FontLoadError {}

/// Project-owned font faces and the fallback list used by default typography.
#[derive(Debug, Default)]
pub struct FontBook {
    families: HashMap<String, BTreeMap<u16, FontFace>>,
    defaults: Vec<String>,
}

impl FontBook {
    /// Sets the family order for typography with an empty `font-family` list.
    pub fn set_default_families(&mut self, families: impl IntoIterator<Item = impl Into<String>>) {
        self.defaults = families.into_iter().map(Into::into).collect();
    }

    /// Reads and adds a face from a project file. Use index zero for a single face.
    pub fn load_face(
        &mut self,
        path: impl AsRef<Path>,
        index: u32,
        family: &str,
        weight: u16,
    ) -> Result<FontFace, FontLoadError> {
        let path = path.as_ref();
        let source = path.display().to_string();
        let bytes = fs::read(path).map_err(|error| FontLoadError {
            source: source.clone(),
            message: error.to_string(),
        })?;
        self.add_face(&source, bytes, index, family, weight)
    }

    /// Adds a TrueType or OpenType face from project asset bytes.
    /// A successful replacement of a family and weight produces a new identity.
    /// Existing shaped runs retain their original face and bytes.
    pub fn add_face(
        &mut self,
        source: &str,
        bytes: Vec<u8>,
        index: u32,
        family: &str,
        weight: u16,
    ) -> Result<FontFace, FontLoadError> {
        let error = |message: String| FontLoadError {
            source: source.into(),
            message,
        };
        let family = family.trim();
        if family.is_empty() || !(1..=1000).contains(&weight) {
            return Err(error(
                "expected a project family name and a weight from 1 to 1000".into(),
            ));
        }
        let data = Blob::from(bytes);
        let font = Font::new(data.clone(), index)
            .filter(|font| {
                font.num_glyphs() > 0 && (16..=16384).contains(&font.metrics().raw_units_per_em)
            })
            .ok_or_else(|| {
                error(format!(
                    "invalid TrueType or OpenType face at index {index}"
                ))
            })?;
        let mut id = NEXT_FACE.load(Ordering::Relaxed);
        loop {
            let next = id.checked_add(1).expect("font face identities exhausted");
            match NEXT_FACE.compare_exchange_weak(id, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(current) => id = current,
            }
        }
        let face = FontFace(Arc::new(Face {
            id: FontFaceId(id),
            family: family.into(),
            weight,
            source: source.into(),
            index,
            data,
            font,
        }));
        self.families
            .entry(family.to_lowercase())
            .or_default()
            .insert(weight, face.clone());
        Ok(face)
    }

    pub(super) fn resolve(&self, names: &[String], weight: u16) -> Vec<&FontFace> {
        let names = if names.is_empty() {
            &self.defaults
        } else {
            names
        };
        let mut faces = Vec::new();
        for name in names {
            if let Some(choices) = self.families.get(&name.to_lowercase())
                && let Some((_, face)) = choices
                    .iter()
                    .min_by_key(|(candidate, _)| weight.abs_diff(**candidate))
                && !faces.contains(&face)
            {
                faces.push(face);
            }
        }
        faces
    }
}
