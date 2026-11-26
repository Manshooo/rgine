use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const ATTRIBUTES: &str = include_str!("../../../.gitattributes");
const SCENE: &str = include_str!("../../scene/tests/fixtures/authored.scene.ron");
const OBJECT: &str = "00000000-0000-0000-0000-000000000001";

fn run_git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("Git must be available to exercise the merge driver")
}

fn git(root: &Path, args: &[&str]) {
    let output = run_git(root, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn one_line_scene() -> String {
    SCENE.split_whitespace().collect::<Vec<_>>().join(" ") + "\n"
}

fn divergent_scene(root: &Path, scene_path: &str, current: &str, incoming: &str) {
    git(root, &["init", "--quiet", "--initial-branch=main"]);
    git(root, &["config", "--local", "user.name", "rgine tests"]);
    git(
        root,
        &[
            "config",
            "--local",
            "user.email",
            "rgine-test@example.invalid",
        ],
    );
    let binary = env!("CARGO_BIN_EXE_engine-cli").replace('\\', "/");
    let driver = format!("'{}' merge %O %A %B %P", binary.replace('\'', "'\\''"));
    git(root, &["config", "--local", "merge.rgine.driver", &driver]);
    fs::write(root.join(".gitattributes"), ATTRIBUTES).unwrap();
    let scene = root.join(scene_path);
    fs::write(&scene, one_line_scene()).unwrap();
    git(root, &["add", "--", ".gitattributes", scene_path]);
    git(root, &["commit", "--quiet", "--no-gpg-sign", "-m", "base"]);

    git(root, &["switch", "--quiet", "-c", "incoming"]);
    fs::write(&scene, incoming).unwrap();
    git(
        root,
        &["commit", "--quiet", "--no-gpg-sign", "-am", "incoming edit"],
    );
    git(root, &["switch", "--quiet", "main"]);
    fs::write(&scene, current).unwrap();
    git(
        root,
        &["commit", "--quiet", "--no-gpg-sign", "-am", "current edit"],
    );
}

#[test]
fn git_combines_different_fields_on_one_line() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    let filename = "authored scene.scene.ron";
    let input = one_line_scene();
    let current = input.replace("\"child\"", "\"current name\"");
    let incoming = input.replace("speed:2", "speed:4");
    divergent_scene(root, filename, &current, &incoming);
    git(root, &["merge", "--no-commit", "--no-ff", "incoming"]);
    let expected = SCENE
        .replace("\"child\"", "\"current name\"")
        .replace("speed:2", "speed:4");
    assert_eq!(fs::read_to_string(root.join(filename)).unwrap(), expected);
    assert!(run_git(root, &["ls-files", "--unmerged"]).stdout.is_empty());
}

#[test]
fn git_records_a_property_conflict_with_the_original_inputs() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    let filename = "source prefab's $.prefab.ron";
    let current = SCENE.replace("\"child\"", "\"current name\"");
    let incoming = SCENE.replace("\"child\"", "\"incoming name\"");
    divergent_scene(root, filename, &current, &incoming);
    let output = run_git(root, &["merge", "--no-commit", "--no-ff", "incoming"]);
    assert_eq!(output.status.code(), Some(1));
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains(filename), "{diagnostic}");
    assert!(diagnostic.contains(OBJECT), "{diagnostic}");
    assert!(diagnostic.contains("property \"name\""), "{diagnostic}");
    assert_eq!(fs::read_to_string(root.join(filename)).unwrap(), current);
    let stages = run_git(root, &["ls-files", "--unmerged"]);
    assert!(stages.status.success());
    assert_eq!(String::from_utf8_lossy(&stages.stdout).lines().count(), 3);
}
