use super::*;
fn fixture() -> Result<Vec<u8>> {
    let mut bytes = vec![0; 0x5000];
    // A single named file and an independent FAT, with no executable images.
    let names = [8, 0, 0, 0, 0, 0, 1, 0, 5, b'a', b'.', b'b', b'i', b'n', 0];
    put32(&mut bytes, 0x40, 0x4000)?;
    put32(&mut bytes, 0x44, names.len())?;
    put32(&mut bytes, 0x48, 0x4020)?;
    put32(&mut bytes, 0x4c, 8)?;
    bytes[0x4000..0x4000 + names.len()].copy_from_slice(&names);
    put32(&mut bytes, 0x4020, 0x4100)?;
    put32(&mut bytes, 0x4024, 0x4104)?;
    bytes[0x4100..0x4104].copy_from_slice(b"abcd");
    Ok(bytes)
}
#[test]
fn rebuild_shrinks_file_updates_fat_and_preserves_every_other_byte() -> Result<()> {
    let source = fixture()?;
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("replacement"), b"xy")?;
    let plan = Plan {
        source_sha256: sha(&source),
        arm9: None,
        banner: None,
        anti_piracy_bypass: false,
        replacements: vec![Replacement {
            file: "a.bin".into(),
            expected_sha256: sha(b"abcd"),
            input: "replacement".into(),
            input_sha256: sha(b"xy"),
            placement: Placement::Original,
        }],
    };
    let (output, report) = build(&source, &plan, dir.path())?;
    let mut expected = source.clone();
    expected[0x4100..0x4102].copy_from_slice(b"xy");
    put32(&mut expected, 0x4024, 0x4102)?;
    assert_eq!(output, expected);
    assert_eq!(report["changed_bytes"], 3);
    assert_eq!(report["runtime_verified"], false);
    assert_eq!(report["profile"], "development");
    Ok(())
}
#[test]
fn rebuild_rejects_growth_hash_mismatch_duplicate_and_protected_extent() -> Result<()> {
    let mut source = fixture()?;
    let dir = tempfile::tempdir()?;
    let replacement = |data: &[u8]| Replacement {
        file: "a.bin".into(),
        expected_sha256: sha(b"abcd"),
        input: "replacement".into(),
        input_sha256: sha(data),
        placement: Placement::Original,
    };
    fs::write(dir.path().join("replacement"), b"abcde")?;
    let mut plan = Plan {
        source_sha256: sha(&source),
        arm9: None,
        banner: None,
        anti_piracy_bypass: false,
        replacements: vec![replacement(b"abcde")],
    };
    assert!(build(&source, &plan, dir.path()).is_err());
    fs::write(dir.path().join("replacement"), b"xy")?;
    assert!(build(&source, &plan, dir.path()).is_err());
    plan.replacements = vec![replacement(b"xy"), replacement(b"xy")];
    assert!(build(&source, &plan, dir.path()).is_err());
    plan.replacements.pop();
    put32(&mut source, 0x20, 0x4100)?;
    put32(&mut source, 0x2c, 4)?;
    plan.source_sha256 = sha(&source);
    assert!(build(&source, &plan, dir.path()).is_err());
    Ok(())
}
#[test]
fn tail_allocation_preserves_original_and_rejects_unknown_or_exhausted_tail() -> Result<()> {
    let mut source = fixture()?;
    source.resize(0x20000, 255);
    source[0x4104..].fill(255);
    put32(&mut source, 0x80, 0x4104)?;
    let crc = crc16(&source[..0x15e]);
    source[0x15e..0x160].copy_from_slice(&crc.to_le_bytes());
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("replacement"), b"longer")?;
    let mut plan = Plan {
        source_sha256: sha(&source),
        arm9: None,
        banner: None,
        anti_piracy_bypass: false,
        replacements: vec![Replacement {
            file: "a.bin".into(),
            expected_sha256: sha(b"abcd"),
            input: "replacement".into(),
            input_sha256: sha(b"longer"),
            placement: Placement::FfTail,
        }],
    };
    let (output, _) = build(&source, &plan, dir.path())?;
    let rom = Rom::parse(&output)?;
    let e = rom.file("a.bin")?;
    assert_eq!((e.id, e.start, e.end), (0, 0x4200, 0x4206));
    assert_eq!(rom.data(e), b"longer");
    let mut expected = source.clone();
    expected[0x4200..0x4206].copy_from_slice(b"longer");
    put32(&mut expected, 0x4020, 0x4200)?;
    put32(&mut expected, 0x4024, 0x4206)?;
    put32(&mut expected, 0x80, 0x4206)?;
    let crc = crc16(&expected[..0x15e]);
    expected[0x15e..0x160].copy_from_slice(&crc.to_le_bytes());
    assert_eq!(output, expected);
    source[0x5000] = 0;
    plan.source_sha256 = sha(&source);
    assert!(
        build(&source, &plan, dir.path())
            .unwrap_err()
            .to_string()
            .contains("non-FF")
    );
    source[0x5000] = 255;
    source[0x15e] ^= 1;
    plan.source_sha256 = sha(&source);
    assert!(
        build(&source, &plan, dir.path())
            .unwrap_err()
            .to_string()
            .contains("CRC")
    );
    source[0x15e] ^= 1;
    plan.source_sha256 = sha(&source);
    let large = vec![42; source.len()];
    fs::write(dir.path().join("replacement"), &large)?;
    plan.replacements[0].input_sha256 = sha(&large);
    assert!(
        build(&source, &plan, dir.path())
            .unwrap_err()
            .to_string()
            .contains("physical ROM tail")
    );
    Ok(())
}
#[test]
fn applies_disjoint_writes_and_preserves_rest() -> Result<()> {
    let mut w = vec![Write {
        offset: 2,
        before: vec![2, 3],
        after: vec![9, 8],
        label: "fixture".into(),
    }];
    assert_eq!(apply(&[0, 1, 2, 3, 4], &mut w)?, [0, 1, 9, 8, 4]);
    Ok(())
}
#[test]
fn rejects_overlap_and_wrong_source() {
    let mk = |offset| Write {
        offset,
        before: vec![0, 0],
        after: vec![1, 1],
        label: "fixture".into(),
    };
    assert!(apply(&[0; 4], &mut [mk(0), mk(1)]).is_err());
    assert!(apply(&[2; 4], &mut [mk(0)]).is_err());
}

#[test]
fn banner_titles_replace_every_slot_and_keep_the_icon() -> Result<()> {
    let mut source = vec![0u8; 0x1000];
    put32(&mut source, 0x68, 0x200)?;
    let banner = 0x200;
    source[banner] = 1;
    source[banner + 0x20..banner + 0x240].fill(0x5a);
    let title: Vec<u8> = "ぷよ\nSEGA"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    for slot in 0..6 {
        let at = banner + 0x240 + slot * 0x100;
        source[at..at + title.len()].copy_from_slice(&title);
    }
    let crc = crc16(&source[banner + 0x20..banner + 0x840]);
    source[banner + 2..banner + 4].copy_from_slice(&crc.to_le_bytes());
    let titles = BannerTitles {
        scope: "test".into(),
        version: 1,
        source_title: "ぷよ\nSEGA".into(),
        title: "뿌요\nSEGA".into(),
    };
    let mut writes = vec![banner_write(&source, &titles)?];
    let out = apply(&source, &mut writes)?;
    let korean: Vec<u8> = "뿌요\nSEGA"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    for slot in 0..6 {
        let at = banner + 0x240 + slot * 0x100;
        assert_eq!(&out[at..at + korean.len()], &korean[..]);
    }
    assert_eq!(
        out[banner + 0x20..banner + 0x240],
        source[banner + 0x20..banner + 0x240]
    );
    assert_eq!(
        u16le(&out, banner + 2)?,
        usize::from(crc16(&out[banner + 0x20..banner + 0x840]))
    );
    let wrong = BannerTitles {
        source_title: "ぷよぷよ".into(),
        ..titles
    };
    assert!(banner_write(&source, &wrong).is_err());
    Ok(())
}
