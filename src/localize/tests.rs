use super::*;
#[test]
fn retains_mixed_line_breaks_and_empty_line() -> Result<()> {
    let units = [0u16, 0xfffe, 0xfffd, 1, 0xfffd, 0xfffd, 0, 0xffff];
    let bytes: Vec<_> = units.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(
        source_breaks(&bytes, 2)?,
        vec![vec![0xfffe, 0xfffd], vec![0xfffd], vec![0xfffd]]
    );
    assert!(source_breaks(&[0xfe, 0xff, 0, 0], 2).is_err());
    assert!(source_breaks(&[0, 0xf8, 0xff, 0xff], 2).is_err());
    Ok(())
}
#[test]
fn preserves_line_controls_and_rejects_unresolved_tokens() -> Result<()> {
    let mut f = vec![0; 56];
    f[48..50].copy_from_slice(&(b'A' as u16).to_le_bytes());
    f[52..54].copy_from_slice(&(b'B' as u16).to_le_bytes());
    let bytes = |u: &[u16]| u.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    for u in [
        vec![0, 0xfffd, 1, 0xffff],
        vec![0, 0xfffe, 0xfffd, 1, 0xffff],
    ] {
        let t = bytes(&u);
        assert_eq!(source_lines(&f, &t, 0, t.len(), 4, 2)?, ["A", "B"]);
    }
    for u in [
        vec![0, 0xfffe, 1, 0xffff],
        // Wi-Fi dialog 0 and 26 contain independent FFFE tokens. Until the
        // dialog consumer is supported, do not silently strip or merge them.
        vec![0, 0xfffe, 0xffff],
        vec![0, 0xfffd, 0xfffe, 1, 0xffff],
        vec![0, 0xf800, 1, 0xffff],
        vec![0, 1],
        vec![0, 0xffff, 1],
    ] {
        let t = bytes(&u);
        assert!(source_lines(&f, &t, 0, t.len(), 4, 2).is_err());
    }
    Ok(())
}

#[test]
fn dialog_retains_independent_controls_at_line_edges() -> Result<()> {
    let bytes = |units: &[u16]| {
        units
            .iter()
            .flat_map(|u| u.to_le_bytes())
            .collect::<Vec<_>>()
    };
    for (units, expected) in [
        (vec![0, 0xfffe, 0xffff], vec![(0, false)]),
        (vec![0, 0xfffd, 0xfffe, 1, 0xffff], vec![(1, true)]),
    ] {
        let (prose, anchors) = dialog::span(&bytes(&units))?;
        assert_eq!(anchors, expected);
        assert!(source_breaks(&prose, 2).is_ok());
    }
    assert_eq!(
        dialog::lines(&["질문{FFFE}".into()], &[(0, false)])?,
        ["질문"]
    );
    assert_eq!(
        dialog::lines(&["{FFFE}접속".into()], &[(0, true)])?,
        ["접속"]
    );
    assert!(dialog::lines(&["{FFFE}질문".into()], &[(0, false)]).is_err());
    assert!(dialog::lines(&["질문".into()], &[(0, false)]).is_err());
    assert!(dialog::lines(&["질{FFFE}문".into()], &[]).is_err());
    assert!(dialog::span(&bytes(&[0, 0xfffe, 1, 0xffff])).is_err());
    Ok(())
}
