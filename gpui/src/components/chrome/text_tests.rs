use super::*;

#[test]
fn chrome_title_descenders_fit_clipped_typographic_box() {
    // Read the production font directly: constructing MacPlatform also acquires
    // pasteboards, which is deliberately outside this headless regression.
    let face = rustybuzz::Face::from_slice(
        include_bytes!("../../../assets/product-fonts/TauInter-400.ttf"),
        0,
    )
    .unwrap();
    for size in [12., 13.] {
        let scale = size / face.units_per_em() as f32;
        let ascent = px(face.ascender() as f32 * scale);
        let descent = px(-(face.descender() as f32) * scale);
        let height = title_leading(ascent, descent);
        let baseline = (height - ascent - descent) / 2. + ascent;
        let old_height = px(size); // Button's inherited relative(1.)
        let old_baseline = (old_height - ascent - descent) / 2. + ascent;
        let mut old_clips = false;
        for ch in "gypq".chars() {
            let ink = face
                .glyph_bounding_box(face.glyph_index(ch).unwrap())
                .unwrap();
            let top = baseline - px(ink.y_max as f32 * scale);
            let bottom = baseline - px(ink.y_min as f32 * scale);
            assert!(
                top >= px(0.) && bottom <= height,
                "{ch}: {top:?}..{bottom:?} in {height:?}"
            );
            old_clips |= old_baseline - px(ink.y_min as f32 * scale) > old_height;
        }
        assert!(
            old_clips,
            "negative control must reproduce bottom ink clipping"
        );
        for text in [
            "gypq debugging".to_owned(),
            "gypq debugging a very long session title ".repeat(12),
        ] {
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(&text);
            let shaped = rustybuzz::shape(&face, &[], buffer);
            assert!(!shaped.glyph_infos().is_empty());
        }
        eprintln!("size={size} ascent={ascent:?} descent={descent:?} label={height:?}");
    }
}
