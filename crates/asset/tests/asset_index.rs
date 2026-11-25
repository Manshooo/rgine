use std::{fs, path::Path};

use engine_asset::{AssetGuid, AssetIndex};

#[test]
fn new_identity_survives_rescans_and_moves_with_sidecar() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    fs::write(dir.join("mesh.gltf"), b"mesh").unwrap();
    let scan = || AssetIndex::scan(dir).unwrap();
    let first = scan();
    assert_eq!(first.iter().len(), 1);
    let (guid, _) = first.iter().next().unwrap();
    assert_eq!(scan().get(guid).unwrap().path, Path::new("mesh.gltf"));
    fs::create_dir(dir.join("props")).unwrap();
    for suffix in ["", ".meta"] {
        fs::rename(
            dir.join(format!("mesh.gltf{suffix}")),
            dir.join(format!("props/tree.gltf{suffix}")),
        )
        .unwrap();
    }
    assert_eq!(scan().get(guid).unwrap().path, Path::new("props/tree.gltf"));
}

#[test]
fn duplicate_identity_reports_both_assets() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    fs::write(dir.join("original"), []).unwrap();
    AssetIndex::scan(dir).unwrap();
    fs::write(dir.join("copy"), []).unwrap();
    fs::copy(dir.join("original.meta"), dir.join("copy.meta")).unwrap();
    let error = AssetIndex::scan(dir).unwrap_err().to_string();
    assert!(
        error.contains("duplicate asset GUID")
            && error.contains("original")
            && error.contains("copy"),
        "{error}"
    );
}

#[test]
fn reads_version_one_settings_and_preserves_invalid_metadata() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    fs::write(dir.join("texture"), []).unwrap();
    let path = dir.join("texture.meta");
    let guid = "6635179a-c14c-4fb8-a069-17820a5ef9e7";
    let fixture =
        format!(r#"(schema_version: 1, guid: "{guid}", importer_settings: {{"srgb": true}})"#);
    fs::write(&path, &fixture).unwrap();
    let index = AssetIndex::scan(dir).unwrap();
    let asset = index.get(AssetGuid::parse_str(guid).unwrap()).unwrap();
    assert_eq!(asset.importer_settings["srgb"], ron::Value::Bool(true));
    for invalid in [
        fixture.replace("version: 1", "version: 2"),
        String::from("("),
    ] {
        fs::write(&path, &invalid).unwrap();
        assert!(
            AssetIndex::scan(dir)
                .unwrap_err()
                .to_string()
                .contains("texture.meta")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
}
