#![cfg(feature = "authoring")]

use engine_scene::{ObjectId, SceneError, SceneMergeError, merge_scene_ron};

const FIRST: ObjectId = ObjectId::from_u128(1);
const SECOND: ObjectId = ObjectId::from_u128(2);

fn node(id: ObjectId, parent: Option<ObjectId>) -> String {
    let parent = parent.map_or_else(|| "None".to_owned(), |parent| format!("Some(\"{parent}\")"));
    format!("\"{id}\":(type_name:\"test.node\",type_version:1,parent:{parent},properties:{{}})")
}

fn scene(nodes: &[String]) -> String {
    format!("(schema_version:1,objects:{{{}}})", nodes.join(","))
}

#[test]
fn independent_parent_edits_cannot_create_a_cycle() {
    let base = scene(&[node(FIRST, None), node(SECOND, None)]);
    let ours = scene(&[node(FIRST, Some(SECOND)), node(SECOND, None)]);
    let theirs = scene(&[node(FIRST, None), node(SECOND, Some(FIRST))]);
    let error = merge_scene_ron(&base, &ours, &theirs).unwrap_err();
    assert!(
        matches!(error, SceneMergeError::Hierarchy(source) if matches!(*source, SceneError::CyclicHierarchy(FIRST)))
    );
}

#[test]
fn add_add_uses_an_empty_base_and_delete_edit_conflicts() {
    let first = scene(&[node(FIRST, None)]);
    let second = scene(&[node(SECOND, None)]);
    let both = scene(&[node(FIRST, None), node(SECOND, None)]);
    assert_eq!(
        merge_scene_ron("", &first, &second).unwrap(),
        merge_scene_ron(&both, &both, &both).unwrap()
    );
    let theirs = both.replace("properties:{}", "properties:{\"label\":\"changed\"}");
    let error = merge_scene_ron(&both, &second, &theirs).unwrap_err();
    assert!(matches!(
        error,
        SceneMergeError::Conflict { object: FIRST, .. }
    ));
}
