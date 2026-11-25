use std::{fs, io::ErrorKind, path::Path};

use engine_asset::ktx2::{ColorSpace, Texture};

const FIXTURE: &[u8] = include_bytes!("fixtures/mip-colors.ktx2");

#[test]
fn loads_nonsquare_texture_with_authored_mips() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mip-colors.ktx2");
    let texture = Texture::load(&path).unwrap();
    assert_eq!(texture.size(), [16, 8]);
    assert_eq!(texture.color_space(), ColorSpace::Srgb);
    assert_eq!(
        texture.levels().map(<[u8]>::len).collect::<Vec<_>>(),
        [512, 128, 32, 8, 4],
    );
    let colors = texture
        .levels()
        .map(|level| &level[..4])
        .collect::<Vec<_>>();
    assert_eq!(colors[0], [128, 0, 0, 255]);
    assert!(colors[1..].iter().all(|&color| color == [0, 128, 0, 255]));
}

#[test]
fn invalid_and_unsupported_assets_name_the_file() {
    let directory = tempfile::tempdir().unwrap();
    for (name, offset, replacement, diagnostic) in [
        (
            "basis.ktx2",
            44,
            &1u32.to_le_bytes()[..],
            "supercompression scheme 1",
        ),
        ("format.ktx2", 12, &0u32.to_le_bytes()[..], "VkFormat 0"),
        ("orientation.ktx2", 311, b"ru", "orientation"),
        (
            "range.ktx2",
            80,
            &(u64::MAX - 3).to_le_bytes()[..],
            "mip level 0 byte range",
        ),
    ] {
        let mut bytes = FIXTURE.to_vec();
        bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
        let path = directory.path().join(name);
        fs::write(&path, bytes).unwrap();
        let error = Texture::load(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        let message = error.to_string();
        assert!(message.contains(name), "{message}");
        assert!(message.contains(diagnostic), "{message}");
    }
}
