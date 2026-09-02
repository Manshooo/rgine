//! The workspace graph the boundary rules are asserted over.
//!
//! Kept free of `cargo metadata` types so the rules can be tested against
//! hand-built graphs.

use std::collections::HashMap;

/// Which dependency table an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// `[dependencies]` - links into the built artifact.
    Normal,
    /// `[dev-dependencies]` - tests, examples and benches only.
    Development,
    /// `[build-dependencies]` - build scripts only.
    Build,
}

impl DependencyKind {
    /// `cargo metadata` reports a normal dependency as `null`.
    pub fn from_metadata(kind: Option<&str>) -> Self {
        match kind {
            Some("dev") => Self::Development,
            Some("build") => Self::Build,
            _ => Self::Normal,
        }
    }

    /// Whether this kind reaches a shipped binary.
    ///
    /// Build dependencies do not: they run on the host at build time and are
    /// not linked into the artifact.
    pub fn is_runtime(self) -> bool {
        matches!(self, Self::Normal)
    }
}

/// One dependency as named by a manifest.
#[derive(Debug, Clone)]
pub struct NamedDependency {
    pub name: String,
    pub kind: DependencyKind,
    /// The `cfg(...)` or triple of a `[target.'...'.dependencies]` table, if the
    /// entry came from one.
    pub target: Option<String>,
}

impl NamedDependency {
    /// How to refer to this entry in a diagnostic.
    pub fn table(&self) -> String {
        let table = match self.kind {
            DependencyKind::Normal => "dependencies",
            DependencyKind::Development => "dev-dependencies",
            DependencyKind::Build => "build-dependencies",
        };
        match &self.target {
            Some(target) => format!("[target.'{target}'.{table}]"),
            None => format!("[{table}]"),
        }
    }
}

/// One workspace member.
#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    /// Manifest path relative to the workspace root, with forward slashes.
    pub manifest_path: String,
    pub dependencies: Vec<NamedDependency>,
}

impl Crate {
    /// Whether this crate lives under `apps/`.
    pub fn is_app(&self) -> bool {
        self.manifest_path.starts_with("apps/")
    }

    /// Whether this crate names `dependency` in any dependency table.
    pub fn names(&self, dependency: &str) -> Option<&NamedDependency> {
        self.dependencies
            .iter()
            .find(|candidate| candidate.name == dependency)
    }
}

/// The workspace members and the edges between them.
#[derive(Debug, Clone)]
pub struct Graph {
    members: Vec<Crate>,
    index: HashMap<String, usize>,
}

impl Graph {
    pub fn new(members: Vec<Crate>) -> Self {
        let index = members
            .iter()
            .enumerate()
            .map(|(position, member)| (member.name.clone(), position))
            .collect();
        Self { members, index }
    }

    pub fn members(&self) -> &[Crate] {
        &self.members
    }

    pub fn get(&self, name: &str) -> Option<&Crate> {
        self.index
            .get(name)
            .map(|&position| &self.members[position])
    }

    /// Workspace crates `from` depends on directly, restricted to kinds that
    /// reach a shipped artifact.
    pub fn runtime_edges<'a>(&'a self, from: &'a Crate) -> impl Iterator<Item = &'a Crate> {
        from.dependencies
            .iter()
            .filter(|dependency| dependency.kind.is_runtime())
            .filter_map(|dependency| self.get(&dependency.name))
    }

    /// The shortest runtime dependency path from `from` to any crate in
    /// `targets`, or `None` if none is reachable.
    ///
    /// The path is returned so a diagnostic can name the edge that has to be
    /// cut, rather than only the two endpoints.
    pub fn shortest_runtime_path(&self, from: &str, targets: &[&str]) -> Option<Vec<String>> {
        let start = self.get(from)?;
        let mut queue = std::collections::VecDeque::from([start]);
        let mut came_from: HashMap<&str, &str> = HashMap::new();
        let mut seen: Vec<&str> = vec![start.name.as_str()];

        while let Some(current) = queue.pop_front() {
            if current.name != from && targets.contains(&current.name.as_str()) {
                let mut path = vec![current.name.clone()];
                let mut cursor = current.name.as_str();
                while let Some(&previous) = came_from.get(cursor) {
                    path.push(previous.to_string());
                    cursor = previous;
                }
                path.reverse();
                return Some(path);
            }

            for next in self.runtime_edges(current) {
                if seen.contains(&next.name.as_str()) {
                    continue;
                }
                seen.push(next.name.as_str());
                came_from.insert(&next.name, &current.name);
                queue.push_back(next);
            }
        }

        None
    }
}
