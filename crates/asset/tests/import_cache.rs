use std::fs;
use std::io::{self, Write};

use engine_asset::{ImportCache, ImportSettings, Importer};

struct CopyBytes;

impl Importer for CopyBytes {
    const VERSION: &'static str = "test.copy/1";

    fn import(source: &[u8], _: &ImportSettings, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(source)
    }
}

struct Interrupted;

impl Importer for Interrupted {
    const VERSION: &'static str = CopyBytes::VERSION;

    fn import(_: &[u8], _: &ImportSettings, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(b"partial")?;
        Err(io::Error::other("interrupted"))
    }
}

#[test]
fn independent_checkouts_produce_the_same_key_and_artifact() {
    let checkout_a = tempfile::tempdir().unwrap();
    let checkout_b = tempfile::tempdir().unwrap();
    let import = |root, text| {
        let settings = ron::from_str(text).unwrap();
        ImportCache::new(root)
            .get_or_import::<CopyBytes>(b"source", &settings)
            .unwrap()
    };
    let a = import(
        checkout_a.path(),
        r#"{"flags": {"a": true, "b": false}, "scale": 1}"#,
    );
    let b = import(
        checkout_b.path(),
        r#"{"scale":1,"flags":{"b":false,"a":true}}"#,
    );
    assert_eq!(a.file_name(), b.file_name());
    assert_eq!(fs::read(a).unwrap(), b"source");
    assert_eq!(fs::read(b).unwrap(), b"source");
}

#[test]
fn each_input_invalidates_the_key() {
    struct V2;
    impl Importer for V2 {
        const VERSION: &'static str = "test.copy/2";
        fn import(source: &[u8], _: &ImportSettings, out: &mut dyn Write) -> io::Result<()> {
            out.write_all(source)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let cache = ImportCache::new(dir.path());
    let settings = ImportSettings::new();
    let original = cache.get_or_import::<CopyBytes>(b"a", &settings).unwrap();
    assert_ne!(
        original,
        cache.get_or_import::<V2>(b"a", &settings).unwrap()
    );
    assert_ne!(
        original,
        cache.get_or_import::<CopyBytes>(b"b", &settings).unwrap()
    );
    let changed = ron::from_str(r#"{"srgb": true}"#).unwrap();
    assert_ne!(
        original,
        cache.get_or_import::<CopyBytes>(b"a", &changed).unwrap()
    );
}

#[test]
fn failed_write_can_be_retried_and_a_hit_skips_the_importer() {
    let dir = tempfile::tempdir().unwrap();
    let cache = ImportCache::new(dir.path());
    let settings = ImportSettings::new();
    assert!(
        cache
            .get_or_import::<Interrupted>(b"full", &settings)
            .is_err()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let path = cache
        .get_or_import::<CopyBytes>(b"full", &settings)
        .unwrap();
    assert_eq!(
        cache
            .get_or_import::<Interrupted>(b"full", &settings)
            .unwrap(),
        path
    );
    assert_eq!(fs::read(path).unwrap(), b"full");
}
