use super::*;

#[test]
fn hierarchy_round_trips_with_stable_ids_and_selection() {
    let registry = registry();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.scene.ron");
    let mut doc = Document::new(&registry);
    doc.add().unwrap();
    let first_root = doc.selected.unwrap();
    doc.rename(first_root, "Start").unwrap();
    doc.add().unwrap();
    let child = doc.selected.unwrap();
    doc.rename(child, "Мост").unwrap();
    doc.selected = None;
    doc.add().unwrap();
    let second_root = doc.selected.unwrap();
    doc.rename(second_root, "Finish").unwrap();
    doc.reparent(child, Some(second_root)).unwrap();
    doc.delete(first_root).unwrap();
    doc.selected = Some(child);
    assert!(doc.is_dirty());

    doc.save(&path).unwrap();
    let saved = fs::read(&path).unwrap();
    assert!(!doc.is_dirty());
    doc.open(&path).unwrap();
    assert_eq!(doc.selected, Some(child));
    let rows: Vec<_> = doc
        .hierarchy()
        .into_iter()
        .map(|row| (row.id, row.depth, doc.name(row.id)))
        .collect();
    assert_eq!(
        rows,
        [(second_root, 0, "Finish".into()), (child, 1, "Мост".into())]
    );
    doc.save(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), saved);
}

#[test]
fn failed_edits_and_failed_open_preserve_the_document() {
    let registry = registry();
    let directory = tempfile::tempdir().unwrap();
    let damaged = directory.path().join("damaged.scene.ron");
    fs::write(&damaged, "(schema_version: -1)").unwrap();
    let mut doc = Document::new(&registry);
    doc.add().unwrap();
    let root = doc.selected.unwrap();
    doc.add().unwrap();
    let child = doc.selected.unwrap();
    let original = doc.truth.to_ron().unwrap();
    let journal_len = doc.truth.journal().len();

    assert!(
        doc.reparent(root, Some(child))
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    assert!(doc.reparent(child, Some(ObjectId::new_v4())).is_err());
    assert!(doc.rename(child, " \n ").is_err());
    assert!(
        doc.open(&damaged)
            .unwrap_err()
            .contains("damaged.scene.ron")
    );
    assert!(doc.save(&damaged).is_err());
    assert_eq!(doc.truth.to_ron().unwrap(), original);
    assert_eq!(doc.truth.journal().len(), journal_len);
    assert_eq!(doc.selected, Some(child));
    assert_eq!(
        fs::read_to_string(&damaged).unwrap(),
        "(schema_version: -1)"
    );
    assert!(doc.is_dirty());

    doc.delete(root).unwrap();
    assert!(doc.selected.is_none());
    assert!(doc.hierarchy().is_empty());
}
