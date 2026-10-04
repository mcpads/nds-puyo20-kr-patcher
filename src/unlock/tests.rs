use super::*;

#[test]
fn encode_deduplicates_identical_tiles_in_first_use_order() {
    let mut pixels = vec![0u8; 256 * 192];
    // Cell 1 differs from all other (blank) cells.
    pixels[9] = 5;
    let (map, tiles) = encode(&pixels);
    assert_eq!(tiles.len(), 2 * 64);
    assert_eq!(u16::from_le_bytes([map[0], map[1]]), 0);
    assert_eq!(u16::from_le_bytes([map[2], map[3]]), 1);
    assert_eq!(u16::from_le_bytes([map[4], map[5]]), 0);
}
