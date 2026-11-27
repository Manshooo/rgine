use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use engine_core::{Reflect, TypeRegistry};
use engine_scene::{ObjectId, SceneTypeName, Truth, TruthError};

#[derive(Reflect)]
struct LevelNode {
    #[allow(dead_code)]
    name: String,
}

pub fn registry() -> TypeRegistry {
    let mut registry = TypeRegistry::default();
    registry
        .register_reflect::<LevelNode>()
        .insert_metadata(SceneTypeName("rgine.level_node"));
    registry
}

pub struct Row {
    pub id: ObjectId,
    pub depth: usize,
}

pub struct Document<'registry> {
    truth: Truth<'registry>,
    path: Option<PathBuf>,
    saved_at: usize,
    pub selected: Option<ObjectId>,
}

impl<'registry> Document<'registry> {
    pub fn new(registry: &'registry TypeRegistry) -> Self {
        Self {
            truth: Truth::new(registry),
            path: None,
            saved_at: 0,
            selected: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.truth.registry());
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_dirty(&self) -> bool {
        self.truth.journal().len() != self.saved_at
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let opened = (|| -> Result<_, Box<dyn std::error::Error>> {
            let path = fs::canonicalize(path)?;
            let scene = fs::read_to_string(&path)?;
            let truth = Truth::from_ron(self.truth.registry(), &scene)?;
            // Truth decodes fragments lazily. Check the editor's names before
            // replacing the current document so invalid files remain repairable.
            for (_, node) in truth.objects() {
                if let Some(name) = node.property("name") {
                    name.read::<String>()?;
                }
            }
            Ok((path, truth))
        })()
        .map_err(|error| format!("{}: {error}", path.display()))?;
        let (path, truth) = opened;
        self.selected = self
            .selected
            .filter(|&id| self.path.as_ref() == Some(&path) && truth.object(id).is_some());
        self.truth = truth;
        self.path = Some(path);
        self.saved_at = 0;
        Ok(())
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        let saved = (|| -> Result<_, Box<dyn std::error::Error>> {
            let directory = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let name = path.file_name().ok_or("expected a scene file name")?;
            let directory = fs::canonicalize(directory)?;
            let path = directory.join(name);
            let ron = self.truth.to_ron()?;
            let mut pending = tempfile::NamedTempFile::new_in(directory)?;
            pending.write_all(ron.as_bytes())?;
            pending.as_file().sync_all()?;
            if self.path.as_ref() == Some(&path) {
                pending.persist(&path)?;
            } else {
                pending.persist_noclobber(&path)?;
            }
            Ok(path)
        })()
        .map_err(|error| format!("{}: {error}", path.display()))?;
        self.path = Some(saved);
        self.saved_at = self.truth.journal().len();
        Ok(())
    }

    pub fn add(&mut self) -> Result<(), TruthError> {
        let id = ObjectId::new_v4();
        self.truth.create::<LevelNode>(id, self.selected)?;
        self.selected = Some(id);
        Ok(())
    }

    pub fn name(&self, id: ObjectId) -> String {
        self.truth
            .object(id)
            .and_then(|object| object.property("name"))
            .and_then(|value| value.read().ok())
            .unwrap_or_else(|| "Node".to_owned())
    }

    pub fn rename(&mut self, id: ObjectId, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("Enter a nonempty name without control characters.".into());
        }
        self.truth
            .set_property(id, "name", &name.to_owned())
            .map_err(|error| error.to_string())
    }

    pub fn reparent(&mut self, id: ObjectId, parent: Option<ObjectId>) -> Result<(), TruthError> {
        self.truth.reparent(id, parent)
    }

    pub fn delete(&mut self, id: ObjectId) -> Result<(), TruthError> {
        let parent = self.truth.object(id).and_then(|node| node.parent());
        self.truth.delete(id)?;
        if self
            .selected
            .is_some_and(|id| self.truth.object(id).is_none())
        {
            self.selected = parent;
        }
        Ok(())
    }

    pub fn hierarchy(&self) -> Vec<Row> {
        let mut pending: Vec<_> = self
            .truth
            .objects()
            .filter_map(|(id, object)| object.parent().is_none().then_some((id, 0)))
            .collect();
        pending.reverse();
        let mut rows = Vec::with_capacity(self.truth.objects().len());
        while let Some((id, depth)) = pending.pop() {
            let object = self
                .truth
                .object(id)
                .expect("hierarchy points to live nodes");
            let sibling_start = pending.len();
            pending.extend(object.children().map(|child| (child, depth + 1)));
            pending[sibling_start..].reverse();
            rows.push(Row { id, depth });
        }
        rows
    }

    pub fn len(&self) -> usize {
        self.truth.objects().len()
    }
}

#[cfg(test)]
mod tests;
