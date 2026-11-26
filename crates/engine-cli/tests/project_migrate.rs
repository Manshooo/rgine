use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const MANIFEST: &str = include_str!("../../../templates/empty/project.ron");
const SCENE_V1: &str = include_str!("../../scene/tests/fixtures/authored-v1.scene.ron");
const SCENE_V2: &str = include_str!("../../scene/tests/fixtures/authored.scene.ron");

fn migrate(source: &Path, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_engine-cli"))
        .arg("migrate")
        .arg(source)
        .arg(output)
        .output()
        .unwrap()
}

fn project(root: &Path) {
    fs::create_dir(root).unwrap();
    fs::create_dir(root.join("levels")).unwrap();
    fs::write(root.join("project.ron"), MANIFEST).unwrap();
    fs::write(root.join("levels/main.scene.ron"), SCENE_V1).unwrap();
}

#[test]
fn migrates_a_project_copy_and_can_rerun_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("original project");
    let target = temp.path().join("migrated project");
    project(&source);
    fs::write(source.join("prop.prefab.ron"), SCENE_V1).unwrap();
    let current = format!("// retained formatting\n{SCENE_V2}");
    fs::write(source.join("other.scene.ron"), &current).unwrap();
    fs::write(source.join("mesh.bin"), [0, 1, 2, 3]).unwrap();
    let metadata =
        "(schema_version:1,guid:\"00000000-0000-0000-0000-000000000064\",importer_settings:{})";
    fs::write(source.join("mesh.bin.meta"), metadata).unwrap();

    let result = migrate(&source, &target);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(source.join("levels/main.scene.ron")).unwrap(),
        SCENE_V1
    );
    assert_eq!(
        fs::read_to_string(source.join("prop.prefab.ron")).unwrap(),
        SCENE_V1
    );
    assert_eq!(
        fs::read_to_string(target.join("levels/main.scene.ron")).unwrap(),
        SCENE_V2
    );
    assert_eq!(
        fs::read_to_string(target.join("prop.prefab.ron")).unwrap(),
        SCENE_V2
    );
    assert_eq!(
        fs::read_to_string(target.join("other.scene.ron")).unwrap(),
        current
    );
    assert_eq!(
        fs::read_to_string(target.join("project.ron")).unwrap(),
        MANIFEST
    );
    assert_eq!(fs::read(target.join("mesh.bin")).unwrap(), [0, 1, 2, 3]);
    assert_eq!(
        fs::read_to_string(target.join("mesh.bin.meta")).unwrap(),
        metadata
    );

    let result = migrate(&target, &target);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("up to date"));
    assert_eq!(
        fs::read_to_string(target.join("levels/main.scene.ron")).unwrap(),
        SCENE_V2
    );
    assert_eq!(
        fs::read_to_string(target.join("other.scene.ron")).unwrap(),
        current
    );
}

#[test]
fn invalid_inputs_report_the_source_and_leave_the_project_intact() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("original");
    let target = temp.path().join("migrated");
    project(&source);
    let bad = source.join("levels/invalid.prefab.ron");
    let missing_parent = SCENE_V1.replace(
        "parent: Some(\"00000000-0000-0000-0000-000000000002\")",
        "parent: Some(\"00000000-0000-0000-0000-000000000003\")",
    );
    let future = "(schema_version:99,objects:{})";
    for (input, cause) in [
        (future, "unsupported scene schema version 99"),
        (&missing_parent, "missing parent"),
    ] {
        fs::write(&bad, input).unwrap();
        let result = migrate(&source, &target);
        assert_eq!(result.status.code(), Some(1));
        let message = String::from_utf8_lossy(&result.stderr);
        assert!(message.contains("invalid.prefab.ron"), "{message}");
        assert!(message.contains(cause), "{message}");
        assert!(!target.exists());
        assert_eq!(fs::read_to_string(&bad).unwrap(), input);
        assert_eq!(
            fs::read_to_string(source.join("levels/main.scene.ron")).unwrap(),
            SCENE_V1
        );
        assert_eq!(
            fs::read_to_string(source.join("project.ron")).unwrap(),
            MANIFEST
        );
    }
}

#[test]
fn output_cannot_replace_a_directory_or_be_created_inside_the_project() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("original");
    let target = temp.path().join("existing");
    project(&source);
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), "existing data").unwrap();
    let inside = source.join("migrated");

    for (destination, cause) in [
        (&target, "output already exists"),
        (&inside, "output must be outside the source project"),
    ] {
        let result = migrate(&source, destination);
        assert_eq!(result.status.code(), Some(1));
        let message = String::from_utf8_lossy(&result.stderr);
        assert!(message.contains(cause), "{message}");
    }
    assert!(!inside.exists());
    assert_eq!(
        fs::read_to_string(target.join("keep")).unwrap(),
        "existing data"
    );
    assert_eq!(
        fs::read_to_string(source.join("levels/main.scene.ron")).unwrap(),
        SCENE_V1
    );
}
