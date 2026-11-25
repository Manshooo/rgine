use engine_asset::{
    ImportCache,
    gltf_prop::{self, Prop},
};
use std::{
    fs,
    time::{Duration, Instant},
};

#[test]
fn marker_hierarchy_material_and_warm_reimport() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("marker.gltf");
    let bin = root.path().join("marker.bin");
    fs::write(&path, include_bytes!("fixtures/marker.gltf")).unwrap();
    let mut bytes = *include_bytes!("fixtures/marker.bin");
    fs::write(&bin, bytes).unwrap();
    let cache = ImportCache::new(root.path().join("cache"));
    let import = || gltf_prop::import(&path, &cache, &Default::default()).unwrap();
    let cold = import();
    let warm = import();
    assert_eq!(cold, warm);
    let prop = Prop::load(&cold).unwrap();
    let instance = &prop.instances[0];
    assert_eq!(&instance.transform[12..15], &[0., 0., -3.]);
    let p = &prop.meshes[instance.mesh][0];
    assert_eq!((p.metallic, p.roughness), (0.25, 0.6));
    assert_eq!(p.base_color, [0.8, 0.08, 0.02, 1.]);
    assert_eq!(p.indices.len(), 18);
    bytes[..4].copy_from_slice(&(-0.5f32).to_le_bytes());
    fs::write(bin, bytes).unwrap();
    let changed = import();
    assert_ne!(changed, cold);
    assert_eq!(
        Prop::load(&changed).unwrap().meshes[0][0].positions[0][0],
        -0.5
    );
}

#[test]
fn measures_warm_reimport_of_ten_thousand_triangles() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("level.gltf");
    let vertex_count = 30_000;
    let length = vertex_count * 12;
    let mut model: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/marker.gltf")).unwrap();
    model["meshes"][0]["primitives"][0]
        .as_object_mut()
        .unwrap()
        .remove("indices");
    model["accessors"].as_array_mut().unwrap().truncate(1);
    model["accessors"][0]["count"] = vertex_count.into();
    model["bufferViews"].as_array_mut().unwrap().truncate(1);
    model["bufferViews"][0]["byteLength"] = length.into();
    model["buffers"][0]["byteLength"] = length.into();
    fs::write(&path, serde_json::to_vec(&model).unwrap()).unwrap();
    let triangle = &include_bytes!("fixtures/marker.bin")[..36];
    fs::write(root.path().join("marker.bin"), triangle.repeat(10_000)).unwrap();
    let cache = ImportCache::new(root.path().join("cache"));
    let import = || gltf_prop::import(&path, &cache, &Default::default()).unwrap();
    let artifact = import();
    let mut samples = [Duration::ZERO; 5];
    for sample in &mut samples {
        let start = Instant::now();
        assert_eq!(import(), artifact);
        *sample = start.elapsed();
    }
    samples.sort();
    eprintln!("Warm import median for 10,000 triangles: {:?}", samples[2]);
    assert!(samples[2] < Duration::from_millis(100), "{samples:?}");
}
