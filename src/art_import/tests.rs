use super::*;

#[test]
fn split_artwork_rejects_overlaps_and_preserves_gaps() {
    let mut pieces = vec![
        ImagePiece {
            source: None,
            target: [0, 0, 20, 32],
            image_fit: None,
            rotate: None,
        },
        ImagePiece {
            source: None,
            target: [20, 0, 42, 32],
            image_fit: None,
            rotate: None,
        },
        ImagePiece {
            source: None,
            target: [48, 0, 60, 10],
            image_fit: None,
            rotate: None,
        },
    ];
    let mask = piece_mask(&pieces, 64, 32).unwrap();
    assert!(mask[19]);
    assert!(!mask[42]);
    assert!(!mask[10 * 64 + 48]); // Original exclamation mark must remain unselected.
    pieces[2].target = [19, 0, 30, 10];
    assert!(piece_mask(&pieces, 64, 32).is_err());
    pieces[2].target = [48, 0, 65, 10];
    assert!(piece_mask(&pieces, 64, 32).is_err());
    pieces[2].target = [48, 0, 48, 10];
    assert!(piece_mask(&pieces, 64, 32).is_err());
}

fn palette() -> Vec<u8> {
    (0..16u16).flat_map(|c| c.to_le_bytes()).collect()
}
#[test]
fn mirrored_bg_tiles_share_storage_without_pixel_changes() {
    let pixels: Vec<u8> = (0..64).map(|i| if i % 8 == 0 { 1 } else { 2 }).collect();
    let mirror: Vec<u8> = (0..64).map(|i| pixels[i / 8 * 8 + 7 - i % 8]).collect();
    let mut tiles = crate::battle_ui::pack(&pixels).unwrap();
    tiles.extend(crate::battle_ui::pack(&mirror).unwrap());
    let mut map = vec![0; 1536];
    map[62] = 1;
    let original = screens::render(&map, &tiles, 1).unwrap();
    let mut enc = screens::EncodedScreen {
        map,
        tiles,
        pixels: original.clone(),
        tile_count: 2,
    };
    let count = share_flipped_tiles(&mut enc, 32, &palette()).unwrap();
    assert_eq!(count, 1);
    assert_eq!(enc.tiles.len(), 32);
    assert_eq!(enc.pixels, original);
    assert_eq!(u16le(&enc.map, 62).unwrap(), 0x400);
}
