use super::bitmap::Bitmap;
use super::*;
use crate::Typography;
use crate::text::FontBook;

const FONT: &[u8] = include_bytes!("../../../tests/fixtures/fonts/PT_Serif-Web-Regular.ttf");

fn face(book: &mut FontBook) -> FontFace {
    book.add_face("PT Serif", FONT.to_vec(), 0, "Test", 400)
        .unwrap()
}

#[test]
fn a_small_cache_pins_live_pages_and_recycles_only_unreferenced_revisions() {
    let mut book = FontBook::default();
    let first = face(&mut book);
    let revised = face(&mut book);
    let key = |face: &FontFace, glyph| GlyphKey {
        face: face.id(),
        glyph,
        ppem: 16.0_f32.to_bits(),
    };
    let a = key(&first, 4);
    let b = key(&first, 7);
    let c = key(&revised, a.glyph);
    assert_ne!(a, c);
    let bitmap = Bitmap {
        origin: Vec2::ZERO,
        size: [8, 8],
    };
    let mut cache = Cache::new(12, 2);
    cache.begin_frame();
    let first = cache.reserve(bitmap).unwrap();
    let first = cache.commit(a, first);
    let second = cache.reserve(bitmap).unwrap();
    let second = cache.commit(b, second);
    assert_eq!((first.page, second.page), (0, 1));
    assert!(matches!(
        cache.reserve(bitmap),
        Err(GlyphAtlasError::FrameFull)
    ));
    assert_eq!(cache.get(a), Some(first));
    assert_eq!(cache.get(b), Some(second));

    cache.begin_frame();
    assert_eq!(cache.get(a), Some(first));
    let slot = cache.reserve(bitmap).unwrap();
    assert_eq!(slot.slot.page, second.page);
    assert!(slot.recycle);
    let revised = cache.commit(c, slot);
    assert_eq!(cache.get(a), Some(first));
    assert_eq!(cache.get(b), None);
    assert_eq!(cache.get(c), Some(revised));
}

#[test]
fn glyph_masks_have_transparent_borders_and_keep_shaped_baseline_at_both_scales() {
    let mut book = FontBook::default();
    let face = face(&mut book);
    book.set_default_families(["Test"]);
    let typography = Typography {
        font_size: 20.0,
        ..Typography::default()
    };
    let layout = book.prepare("AV\nМир", &typography, None).unwrap();
    let mut raster = Raster::default();
    for scale in [1.0, 2.0] {
        for line in &layout.lines {
            let glyph = line.runs[0].glyphs[0];
            let key = GlyphKey {
                face: face.id(),
                glyph: glyph.id,
                ppem: (layout.font_size * scale).to_bits(),
            };
            let bitmap = raster.glyph(&face, key, 64).unwrap().unwrap();
            let [w, h] = bitmap.size.map(|d| d as usize);
            let alpha = |x, y| raster.rgba[(y * w + x) * 4 + 3];
            assert!(
                raster
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|rgba| rgba[3] > 0)
            );
            assert!((0..w).all(|x| alpha(x, 0) == 0 && alpha(x, h - 1) == 0));
            assert!((0..h).all(|y| alpha(0, y) == 0 && alpha(w - 1, y) == 0));
            let origin = Vec2::new(11.0, 9.0);
            let rect = quad_rect(
                origin + Vec2::from_array(glyph.position),
                scale,
                Slot {
                    page: 0,
                    offset: [0, 0],
                    bitmap,
                },
            );
            let raster_origin = origin * scale + Vec2::from_array(glyph.physical_position(scale));
            assert!((rect.min * scale).abs_diff_eq(raster_origin + bitmap.origin, 0.001));
            assert_eq!(glyph.position[1], line.baseline);
        }
    }
}
