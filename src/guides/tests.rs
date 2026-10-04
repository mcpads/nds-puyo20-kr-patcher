use super::*;

/// Pixel fonts drop strokes when rasterised off their native grid (e.g. the
/// ㅅ of 소 in Galmuri11 at 11px), so every coverage value must be 0 or 255.
fn assert_native_grid(path: &str, expected_sha256: &str, size: f32) {
    let bytes = crate::test_input::read(path);
    assert_eq!(sha(&bytes), expected_sha256);
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).unwrap();
    for ch in "결정다음취소뒤로삭제힌트규칙아주순함보통매움".chars() {
        let (_, bitmap) = font.rasterize(ch, size);
        assert!(
            bitmap.iter().all(|&v| v == 0 || v == 255),
            "{path} at {size}px has partial coverage for {ch}"
        );
    }
}

#[test]
#[ignore = "requires assets/fonts/galmuri9/Galmuri9.ttf and assets/fonts/galmuri7/Galmuri7.ttf"]
fn caption_fonts_rasterise_on_their_native_grid() {
    assert_native_grid("assets/fonts/galmuri9/Galmuri9.ttf", FONT_SHA256, FONT_SIZE);
    assert_native_grid(
        "assets/fonts/galmuri7/Galmuri7.ttf",
        "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        8.0,
    );
}
