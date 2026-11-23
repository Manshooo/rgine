use std::any::TypeId;

use engine_core::reflection::TypeShape;
use engine_core::{Reflect, TypeRegistry};

#[derive(Reflect)]
#[reflect(version = 3, display_name = "Settings")]
#[allow(dead_code)]
struct Settings<T, const N: usize> {
    #[reflect(tooltip = "Gain applied to each sample", range = -1.0..=1)]
    r#type: T,
    values: [T; N],
}

#[derive(Reflect)]
#[allow(dead_code)]
enum Setting {
    Default,
    #[reflect(display_name = "Linear")]
    Linear(f32),
    Named {
        name: String,
    },
}

#[test]
fn registers_shapes_and_versions_in_a_headless_build() {
    let mut registry = TypeRegistry::default();
    registry.register::<f32>();
    registry.register_reflect::<Settings<f32, 3>>();
    let schema = registry
        .get::<Settings<f32, 3>>()
        .unwrap()
        .schema()
        .unwrap();
    assert_eq!(schema.version, 3);
    let TypeShape::Struct(fields) = &schema.shape else {
        panic!("expected a struct");
    };
    assert_eq!(fields[0].name, "type");
    assert_eq!(fields[0].type_id, TypeId::of::<f32>());
    assert_eq!(fields[1].name, "values");
    assert_eq!(fields[1].type_id, TypeId::of::<[f32; 3]>());
    assert_eq!(
        registry.get_by_type_id(fields[0].type_id).unwrap().name(),
        "f32"
    );

    let schema = registry.register_reflect::<Setting>().schema().unwrap();
    assert_eq!(schema.version, 1);
    let TypeShape::Enum(variants) = &schema.shape else {
        panic!("expected an enum");
    };
    assert_eq!(variants[0].name, "Default");
    assert!(variants[0].fields.is_empty());
    assert_eq!(variants[1].fields[0].name, "0");
    assert_eq!(variants[1].fields[0].type_id, TypeId::of::<f32>());
    assert_eq!(variants[2].fields[0].name, "name");
    assert_eq!(variants[2].fields[0].type_id, TypeId::of::<String>());
}

#[test]
fn repeat_registration_keeps_order_and_attached_metadata() {
    let mut first = TypeRegistry::default();
    let entry = first.register::<Setting>();
    let key = entry.key();
    entry.insert_metadata(42_u16);
    let later = first.register::<bool>().key();
    let entry = first.register_reflect::<Setting>();
    assert_eq!(entry.key(), key);
    assert_eq!(entry.metadata::<u16>(), Some(&42));
    assert_eq!(entry.schema().unwrap().version, 1);

    let mut second = TypeRegistry::default();
    assert_eq!(second.register_reflect::<Setting>().key(), key);
    assert_eq!(second.register::<bool>().key(), later);
    assert!(
        first
            .iter()
            .map(|entry| entry.type_id())
            .eq(second.iter().map(|entry| entry.type_id()))
    );
    assert_eq!(
        first.get_by_key(key).unwrap().type_id(),
        TypeId::of::<Setting>()
    );
}

#[cfg(feature = "editor-metadata")]
#[test]
fn reads_editor_labels_tooltips_and_bounds() {
    let schema = Settings::<f32, 1>::schema();
    assert_eq!(schema.editor.display_name, Some("Settings"));
    let TypeShape::Struct(fields) = &schema.shape else {
        panic!("expected a struct");
    };
    assert_eq!(
        fields[0].editor.tooltip,
        Some("Gain applied to each sample")
    );
    assert_eq!(fields[0].editor.range, Some((-1.0, 1.0)));

    let TypeShape::Enum(variants) = Setting::schema().shape else {
        panic!("expected an enum");
    };
    assert_eq!(variants[1].editor.display_name, Some("Linear"));
}
