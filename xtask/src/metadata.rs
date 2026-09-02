//! Reading the workspace graph out of `cargo metadata`.
//!
//! Only the handful of fields the checks need are modelled. `--no-deps` is
//! enough: every rule is about what a workspace manifest *names*, so the
//! resolved graph of external crates is not required.

use std::io;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::graph::{Crate, DependencyKind, Graph, NamedDependency};

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_root: String,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    manifest_path: String,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    /// `cfg(...)` or a target triple for a `[target.'...'.dependencies]` entry,
    /// `None` for an unconditional one. Present so target-specific tables are
    /// read like any other: a backend named only for Android is still named.
    target: Option<String>,
}

/// Runs `cargo metadata` for the workspace containing `manifest_dir`.
pub fn load(manifest_dir: &Path) -> Result<Graph, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .current_dir(manifest_dir)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|error: io::Error| format!("could not run cargo metadata: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed:\n{}", stderr.trim()));
    }

    let metadata: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("could not parse cargo metadata output: {error}"))?;

    Ok(into_graph(metadata))
}

fn into_graph(metadata: Metadata) -> Graph {
    let root = metadata.workspace_root.replace('\\', "/");
    let members = metadata
        .packages
        .into_iter()
        .map(|package| {
            let manifest_path = package.manifest_path.replace('\\', "/");
            let relative = manifest_path
                .strip_prefix(&root)
                .unwrap_or(&manifest_path)
                .trim_start_matches('/')
                .to_string();

            Crate {
                name: package.name,
                manifest_path: relative,
                dependencies: package
                    .dependencies
                    .into_iter()
                    .map(|dependency| NamedDependency {
                        name: dependency.name,
                        kind: DependencyKind::from_metadata(dependency.kind.as_deref()),
                        target: dependency.target,
                    })
                    .collect(),
            }
        })
        .collect();

    Graph::new(members)
}
