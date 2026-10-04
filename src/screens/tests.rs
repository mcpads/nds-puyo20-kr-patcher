use super::*;
#[test]
fn map_flips_and_shared_tile_edit_preserve_other_cells() -> Result<()> {
    let mut map = vec![0; 1536];
    map[2..4].copy_from_slice(&0x2c00_u16.to_le_bytes());
    let mut tiles = vec![0x11; 128];
    tiles[0] = 0x32;
    let pixels = render(&map, &tiles, 8)?;
    assert_eq!(&pixels[..2], &[2, 3]);
    assert_eq!(pixels[7 * 256 + 15], 34);
    assert_eq!(pixels[7 * 256 + 14], 35);
    let mut palette = Vec::new();
    for _ in 0..8 {
        for i in 0..16_u16 {
            palette.extend((i | (i << 5) | (i << 10)).to_le_bytes());
        }
    }
    let s = Screen {
        map,
        tiles,
        palette,
        pixels,
    };
    let c = colors(&s.palette)?;
    let mut desired: Vec<_> = s.pixels.iter().map(|&p| c[p as usize]).collect();
    desired[64 * 256 + 24] = [15, 15, 15];
    let EncodedScreen {
        map,
        tiles,
        pixels,
        tile_count: count,
    } = encode_screen(&s, |x, y| editable(0, x, y), &desired)?;
    assert_eq!(render(&map, &tiles, 8)?, pixels);
    assert!(count > 1);
    assert_eq!(c[pixels[64 * 256 + 24] as usize], [15, 15, 15]);
    assert_eq!(
        c[pixels[64 * 256 + 32] as usize],
        c[s.pixels[64 * 256 + 32] as usize]
    );
    Ok(())
}

// Two banks: bank 0 holds the decoration gold and cyan (2,22,28) but no
// white; bank 1 holds white, navy and cyan (2,23,29). A tile of cyan
// (2,22,28) background with one white letter pixel is the rule-screen palette
// leak: quantizing lets the white become gold.
fn leak_screen() -> (Screen, Vec<[i32; 3]>) {
    let mut palette = vec![0u8; 64];
    let mut set = |i: usize, c: [u16; 3]| {
        palette[i * 2..i * 2 + 2].copy_from_slice(&(c[0] | c[1] << 5 | c[2] << 10).to_le_bytes())
    };
    set(1, [31, 29, 4]);
    set(2, [2, 22, 28]);
    set(17, [31, 31, 31]);
    set(18, [4, 12, 21]);
    set(19, [2, 23, 29]);
    let map = vec![0; 1536];
    let mut tiles = vec![0x22; 64];
    tiles[32..].fill(0x11);
    let pixels = render(&map, &tiles, 2).unwrap();
    let c = colors(&palette).unwrap();
    let mut desired: Vec<_> = pixels.iter().map(|&p| c[p as usize]).collect();
    desired[3 * 256 + 3] = [31, 31, 31];
    (
        Screen {
            map,
            tiles,
            palette,
            pixels,
        },
        desired,
    )
}

#[test]
fn exact_lettering_is_never_quantized_into_a_decoration_bank() {
    let (s, desired) = leak_screen();
    let rule = |x: usize, y: usize| {
        if (x, y) == (3, 3) {
            PixelRule::Exact
        } else {
            PixelRule::Protected
        }
    };
    let Err(error) = encode_screen_with_rules(&s, rule, &desired, |_, _, _| 0) else {
        panic!("white letter pixel was quantized into the decoration bank");
    };
    assert!(error.to_string().contains("x 0, y 0"), "{error}");
}

#[test]
fn near_background_lets_the_tile_choose_a_bank_holding_white() -> Result<()> {
    let (s, desired) = leak_screen();
    let rule = |x: usize, y: usize| match (x, y) {
        (3, 3) => PixelRule::Exact,
        _ if x < 8 && y < 8 => PixelRule::Near(3),
        _ => PixelRule::Protected,
    };
    let encoded = encode_screen_with_rules(&s, rule, &desired, |_, _, _| 0)?;
    let c = colors(&s.palette)?;
    assert_eq!(c[encoded.pixels[3 * 256 + 3] as usize], [31, 31, 31]);
    assert_eq!(c[encoded.pixels[0] as usize], [2, 23, 29]);
    assert_eq!(render(&encoded.map, &encoded.tiles, 2)?, encoded.pixels);
    Ok(())
}
