use super::*;

#[test]
fn numbered_title_variants_share_prefix_and_reject_overlap() {
    let im = Image {
        width: 12,
        height: 8,
        rgba: (0..96)
            .flat_map(|i| match (i % 12) / 4 {
                0 => [255, 0, 0, 255],
                1 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            })
            .collect(),
    };
    let mut pieces = vec![
        SheetPiece {
            source: [0, 0, 4, 8],
            target: [0, 0, 12, 12],
            fit: true,
        },
        SheetPiece {
            source: [4, 0, 8, 8],
            target: [12, 0, 24, 12],
            fit: true,
        },
    ];
    let first = assemble(&im, 24, 12, &pieces).unwrap();
    pieces[1].source = [8, 0, 12, 8];
    let second = assemble(&im, 24, 12, &pieces).unwrap();
    for y in 0..12 {
        assert_eq!(&first[y * 96..y * 96 + 48], &second[y * 96..y * 96 + 48]);
    }
    assert_ne!(first, second);
    pieces[1].target = [11, 0, 23, 12];
    assert!(assemble(&im, 24, 12, &pieces).is_err());
    pieces[1].target = [12, 0, 25, 12];
    assert!(assemble(&im, 24, 12, &pieces).is_err());
}

#[test]
fn reduction_does_not_bleed_rgb_from_transparent_pixels() {
    let im = Image {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 0, 255, 0],
    };
    assert_eq!(reduce(&im, 1, 1, false).unwrap(), vec![255, 0, 0, 128]);
}
#[test]
fn sheet_cell_keeps_selected_rows_and_rejects_outside_bounds() {
    let im = super::Image {
        width: 3,
        height: 2,
        rgba: (0..24).collect(),
    };
    let cell = super::region(&im, [1, 0, 3, 2]).unwrap();
    assert_eq!((cell.width, cell.height), (2, 2));
    assert_eq!(cell.rgba, [&im.rgba[4..12], &im.rgba[16..24]].concat());
    assert!(super::region(&im, [0, 0, 4, 2]).is_err());
    assert!(super::region(&im, [1, 0, 1, 2]).is_err());
}
