use super::*;

#[test]
fn most_common_prefers_lower_index_on_ties() {
    assert_eq!(most_common([3, 1, 3, 1].into_iter()), Some(1));
    assert_eq!(most_common(std::iter::empty()), None);
}

#[test]
fn four_bit_encoding_rejects_opaque_index_zero() {
    let t = Texture {
        member: 0,
        palette: vec![0; 4],
        format: 3,
        width: 2,
        height: 1,
        texels: vec![(0, 0), (1, 1)],
    };
    assert_eq!(t.encode(&t.texels).unwrap(), vec![0x10]);
    assert!(t.encode(&[(0, 1), (1, 1)]).is_err());
}

fn small_font() -> fontdue::Font {
    fontdue::Font::from_bytes(
        crate::test_input::read("assets/fonts/galmuri7/Galmuri7.ttf"),
        fontdue::FontSettings::default(),
    )
    .unwrap()
}

fn plain(width: usize, height: usize) -> Texture {
    // Palette: 0 transparent, 1 background, 2 fill, 3 outline.
    let mut palette = Vec::new();
    for c in [0u16, 0x7c00, 0x7fff, 0x001f] {
        palette.extend(c.to_le_bytes());
    }
    Texture {
        member: 0,
        palette,
        format: 3,
        width,
        height,
        texels: vec![(1, 1); width * height],
    }
}

fn ink_box(t: &Texture, next: &[(usize, usize)]) -> [usize; 4] {
    let changed: Vec<usize> = (0..next.len())
        .filter(|&p| next[p] != t.texels[p])
        .collect();
    let xs = changed.iter().map(|p| p % t.width);
    let ys = changed.iter().map(|p| p / t.width);
    [
        xs.clone().min().unwrap(),
        ys.clone().min().unwrap(),
        xs.max().unwrap() + 1,
        ys.max().unwrap() + 1,
    ]
}

#[test]
#[ignore = "requires assets/fonts/galmuri7/Galmuri7.ttf"]
fn clockwise_rotation_lays_text_along_the_region_height() {
    let font = small_font();
    let t = plain(16, 48);
    let label: Label = serde_json::from_value(serde_json::json!({
        "texture": 0, "japanese": "", "korean": "이름", "source_sha256": "",
        "region": [0, 0, 16, 48], "background": "keep", "font": "small",
        "outline": 0, "fill_index": 2, "outline_index": 3, "rotate": "cw"
    }))
    .unwrap();
    let mut next = t.texels.clone();
    draw(&t, &mut next, &label, &font, &font, None, None).unwrap();
    let [x0, y0, x1, y1] = ink_box(&t, &next);
    assert!(y1 - y0 > x1 - x0, "rotated text runs vertically");
}

#[test]
#[ignore = "requires assets/fonts/galmuri7/Galmuri7.ttf"]
fn flat_and_column_backgrounds_fill_the_region() {
    let font = small_font();
    let mut t = plain(8, 12);
    // Column gradient source: row above is index 2, row below is index 3.
    for x in 0..8 {
        t.texels[x] = (2, 1);
        t.texels[11 * 8 + x] = (3, 1);
    }
    for (mode, extra) in [
        ("flat", serde_json::json!({"background_index": 2})),
        ("column", serde_json::json!({})),
    ] {
        let mut v = serde_json::json!({
            "texture": 0, "japanese": "", "korean": "-", "source_sha256": "",
            "region": [0, 1, 8, 11], "background": mode, "font": "small",
            "outline": 0, "fill_index": 1, "outline_index": 3
        });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let label: Label = serde_json::from_value(v).unwrap();
        let mut next = t.texels.clone();
        draw(&t, &mut next, &label, &font, &font, None, None).unwrap();
        if mode == "flat" {
            assert_eq!(next[8], (2, 1));
        } else {
            // Top of the region leans to the colour above, bottom to the colour below.
            assert_eq!(next[8], (2, 1));
            assert_eq!(next[10 * 8], (3, 1));
        }
    }
}

#[test]
#[ignore = "requires assets/fonts/galmuri7/Galmuri7.ttf"]
fn masked_rows_rebuild_lettering_from_the_same_row() {
    let font = small_font();
    // Palette: 0 transparent, 1 outer rows, 2 fill, 3 outline, 4 band, 5 source lettering.
    let mut palette = Vec::new();
    for c in [0u16, 0x7c00, 0x7fff, 0x001f, 0x03e0, 0x3def] {
        palette.extend(c.to_le_bytes());
    }
    let (w, h) = (16, 5);
    let mut texels = vec![(1, 1); w * h];
    for y in 1..4 {
        for x in 0..w {
            texels[y * w + x] = (4, 1);
        }
    }
    // Lettering inside the region and one stroke just left of it.
    for x in [2, 3, 4] {
        texels[2 * w + x] = (5, 1);
    }
    let t = Texture {
        member: 0,
        palette,
        format: 3,
        width: w,
        height: h,
        texels,
    };
    let label: Label = serde_json::from_value(serde_json::json!({
        "texture": 0, "japanese": "", "korean": "-", "source_sha256": "",
        "region": [3, 1, 13, 4], "background": "masked_rows",
        "background_mask": {"indices": [5], "radius": 1}, "font": "small",
        "outline": 0, "fill_index": 2, "outline_index": 3
    }))
    .unwrap();
    let mut next = t.texels.clone();
    draw(&t, &mut next, &label, &font, &font, None, None).unwrap();
    // The band colour comes from the clean right side, never from the stroke
    // outside the region or from the rows above and below.
    assert_eq!(next[2 * w + 3], (4, 1));
    assert_eq!(next[2 * w + 4], (4, 1));
    assert_eq!(next[w + 3], (4, 1));
    // Texels outside the region stay untouched.
    assert_eq!(next[2 * w + 2], (5, 1));
}
