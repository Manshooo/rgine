//! The crate boundaries recorded under "Enforced boundaries" in
//! `docs/ARCHITECTURE.md`.
//!
//! Each rule is stated here once, in data, so the document and the check cannot
//! drift apart silently.

use crate::graph::Graph;

/// Crates that exist to serve authoring. Nothing a shipped game links may reach
/// them, or editor-only functionality has become a runtime dependency.
const EDITOR_SIDE: &[&str] = &["engine-editor", "engine-devtools"];

/// Backend crates, and the single workspace crate allowed to name each.
///
/// ADR 0008: a caller that names a backend type has already lost the ability to
/// replace that backend. Extend this as backends land - `wgpu` under `render`,
/// a physics backend under `physics`, an audio one under `audio`.
const BACKEND_OWNERS: &[(&str, &str)] = &[("winit", "engine-platform")];

/// One broken boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Which rule was broken.
    pub rule: &'static str,
    /// What is wrong, and where.
    pub detail: String,
    /// What to do about it.
    pub remedy: String,
}

/// Runs every rule and returns everything that is broken, rather than stopping
/// at the first failure.
pub fn check(graph: &Graph) -> Vec<Violation> {
    let mut violations = Vec::new();
    editor_side_is_unreachable(graph, &mut violations);
    domain_crates_do_not_depend_on_apps(graph, &mut violations);
    backends_are_named_only_by_their_owner(graph, &mut violations);
    violations
}

/// `editor` and `devtools` are not reachable from the dependency graph of any
/// runtime crate.
///
/// Only runtime edges count: a test that pulls in `devtools` does not put it in
/// a shipped binary. One path is reported per offending crate - the shortest -
/// rather than every path, because they are usually the same edge seen twice.
fn editor_side_is_unreachable(graph: &Graph, violations: &mut Vec<Violation>) {
    for member in graph.members() {
        if EDITOR_SIDE.contains(&member.name.as_str()) {
            continue;
        }

        let Some(path) = graph.shortest_runtime_path(&member.name, EDITOR_SIDE) else {
            continue;
        };

        let reached = path.last().cloned().unwrap_or_default();
        violations.push(Violation {
            rule: "editor-side crates are unreachable from runtime crates",
            detail: format!(
                "`{}` reaches `{reached}` at runtime: {}",
                member.name,
                path.join(" -> ")
            ),
            remedy: format!("cut the edge into `{reached}`, or move the shared code below it"),
        });
    }
}

/// Domain crates do not depend on `apps`.
///
/// Any dependency table counts: there is no reason for a crate under `crates/`
/// to name an application at all.
fn domain_crates_do_not_depend_on_apps(graph: &Graph, violations: &mut Vec<Violation>) {
    for member in graph.members() {
        if member.is_app() {
            continue;
        }

        for dependency in &member.dependencies {
            let Some(depended_on) = graph.get(&dependency.name) else {
                continue;
            };
            if !depended_on.is_app() {
                continue;
            }

            violations.push(Violation {
                rule: "domain crates do not depend on apps",
                detail: format!(
                    "`{}` names the application `{}` in {} ({})",
                    member.name,
                    depended_on.name,
                    dependency.table(),
                    member.manifest_path
                ),
                remedy: format!(
                    "dependencies run `apps -> app -> domain crates`; move what `{}` needs out of `{}`",
                    member.name, depended_on.name
                ),
            });
        }
    }
}

/// A backend crate is named only by the crate that owns it (ADR 0008).
///
/// Every dependency table counts, including target-specific ones: a backend
/// named only for Android is still named.
fn backends_are_named_only_by_their_owner(graph: &Graph, violations: &mut Vec<Violation>) {
    for &(backend, owner) in BACKEND_OWNERS {
        // A rule that silently stops applying is worse than no rule, so the
        // table is checked against the workspace before it is enforced.
        match graph.get(owner) {
            None => violations.push(Violation {
                rule: "the backend ownership table is current",
                detail: format!(
                    "`{backend}` is recorded as owned by `{owner}`, which is not a workspace crate"
                ),
                remedy: format!("update the owner of `{backend}` in xtask, or drop the entry"),
            }),
            Some(owner_crate) if owner_crate.names(backend).is_none() => {
                violations.push(Violation {
                    rule: "the backend ownership table is current",
                    detail: format!(
                        "`{owner}` is recorded as the owner of `{backend}` but no longer depends on it"
                    ),
                    remedy: format!(
                        "drop the entry for `{backend}` in xtask, or point it at the crate that owns it now"
                    ),
                })
            }
            Some(_) => {}
        }

        for member in graph.members() {
            if member.name == owner {
                continue;
            }
            let Some(dependency) = member.names(backend) else {
                continue;
            };

            violations.push(Violation {
                rule: "backends are named only by the crate that owns them",
                detail: format!(
                    "`{}` names the backend `{backend}` in {} ({})",
                    member.name,
                    dependency.table(),
                    member.manifest_path
                ),
                remedy: format!(
                    "`{owner}` owns `{backend}`; expose what `{}` needs from `{owner}` instead",
                    member.name
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Crate, DependencyKind, NamedDependency};

    fn dependency(name: &str, kind: DependencyKind) -> NamedDependency {
        NamedDependency {
            name: name.to_string(),
            kind,
            target: None,
        }
    }

    fn krate(name: &str, path: &str, dependencies: &[NamedDependency]) -> Crate {
        Crate {
            name: name.to_string(),
            manifest_path: path.to_string(),
            dependencies: dependencies.to_vec(),
        }
    }

    /// The shape the workspace is meant to have: apps on top, `platform` alone
    /// naming the windowing backend.
    fn clean_workspace() -> Graph {
        Graph::new(vec![
            krate("engine-core", "crates/core/Cargo.toml", &[]),
            krate(
                "engine-platform",
                "crates/platform/Cargo.toml",
                &[dependency("winit", DependencyKind::Normal)],
            ),
            krate(
                "engine-app",
                "crates/app/Cargo.toml",
                &[
                    dependency("engine-core", DependencyKind::Normal),
                    dependency("engine-platform", DependencyKind::Normal),
                ],
            ),
            krate("engine-devtools", "crates/devtools/Cargo.toml", &[]),
            krate(
                "engine-editor",
                "apps/editor/Cargo.toml",
                &[
                    dependency("engine-app", DependencyKind::Normal),
                    dependency("engine-devtools", DependencyKind::Normal),
                ],
            ),
        ])
    }

    fn with_extra(name: &str, extra: NamedDependency) -> Graph {
        let mut members = clean_workspace().members().to_vec();
        let member = members
            .iter_mut()
            .find(|member| member.name == name)
            .expect("crate under test is in the fixture");
        member.dependencies.push(extra);
        Graph::new(members)
    }

    #[test]
    fn the_intended_shape_passes() {
        assert_eq!(check(&clean_workspace()), Vec::new());
    }

    #[test]
    fn a_runtime_crate_reaching_devtools_is_reported_with_the_path() {
        let graph = with_extra(
            "engine-platform",
            dependency("engine-devtools", DependencyKind::Normal),
        );

        let violations = check(&graph);
        let reported: Vec<&str> = violations
            .iter()
            .map(|violation| violation.detail.as_str())
            .collect();

        assert!(
            reported
                .iter()
                .any(|detail| detail.contains("engine-app -> engine-platform -> engine-devtools")),
            "expected the full path, got {reported:?}"
        );
        assert!(
            reported
                .iter()
                .any(|detail| detail.starts_with("`engine-platform` reaches"))
        );
    }

    #[test]
    fn a_dev_dependency_on_devtools_is_allowed() {
        let graph = with_extra(
            "engine-platform",
            dependency("engine-devtools", DependencyKind::Development),
        );

        assert_eq!(check(&graph), Vec::new());
    }

    #[test]
    fn a_domain_crate_depending_on_an_app_is_reported() {
        let graph = with_extra(
            "engine-app",
            dependency("engine-editor", DependencyKind::Normal),
        );

        let violations = check(&graph);

        assert!(violations.iter().any(|violation| {
            violation.rule == "domain crates do not depend on apps"
                && violation
                    .detail
                    .contains("`engine-app` names the application `engine-editor`")
        }));
    }

    #[test]
    fn a_second_crate_naming_the_backend_is_reported() {
        let graph = with_extra("engine-app", dependency("winit", DependencyKind::Normal));

        let violations = check(&graph);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].rule,
            "backends are named only by the crate that owns them"
        );
        assert!(
            violations[0]
                .detail
                .contains("`engine-app` names the backend `winit` in [dependencies]")
        );
    }

    #[test]
    fn a_backend_named_only_for_one_target_is_still_reported() {
        let graph = with_extra(
            "engine-app",
            NamedDependency {
                name: "winit".to_string(),
                kind: DependencyKind::Normal,
                target: Some("cfg(target_os = \"android\")".to_string()),
            },
        );

        let violations = check(&graph);

        assert_eq!(violations.len(), 1);
        assert!(
            violations[0]
                .detail
                .contains("[target.'cfg(target_os = \"android\")'.dependencies]")
        );
    }

    #[test]
    fn a_backend_named_in_dev_dependencies_is_still_reported() {
        let graph = with_extra(
            "engine-app",
            dependency("winit", DependencyKind::Development),
        );

        let violations = check(&graph);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].detail.contains("[dev-dependencies]"));
    }

    #[test]
    fn an_owner_that_no_longer_uses_its_backend_is_reported_as_a_stale_table() {
        let members: Vec<Crate> = clean_workspace()
            .members()
            .iter()
            .map(|member| {
                let mut member = member.clone();
                member
                    .dependencies
                    .retain(|dependency| dependency.name != "winit");
                member
            })
            .collect();

        let violations = check(&Graph::new(members));

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "the backend ownership table is current");
    }

    #[test]
    fn an_owner_that_is_not_a_workspace_crate_is_reported_as_a_stale_table() {
        let graph = Graph::new(vec![krate("engine-core", "crates/core/Cargo.toml", &[])]);

        let violations = check(&graph);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "the backend ownership table is current");
        assert!(violations[0].detail.contains("not a workspace crate"));
    }
}
