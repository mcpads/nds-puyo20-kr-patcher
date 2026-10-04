use super::*;
#[test]
fn control_argument_is_not_a_glyph_and_changes_are_visible() -> Result<()> {
    let mut font = vec![0; 48 + 21 * 4];
    font[48 + 20 * 4..50 + 20 * 4].copy_from_slice(&('け' as u16).to_le_bytes());
    let text = [0x81, 0xf8, 0x14, 0, 0x13, 0xf8, 0xff, 0xff, 0xff, 0xff];
    assert_eq!(decode(&font, &text, 4, 21)?, "{F881:0014}{F813}");
    let a = tokens("나{F881:0014}\n야{F813}")?;
    let b = tokens("나{F881:0015}\n야{F813}")?;
    assert_ne!(controls(&a), controls(&b));
    assert!(tokens("나{F881:14}").is_err());
    assert!(tokens("나{F813").is_err());
    assert!(decode(&font, &[0x81, 0xf8], 4, 21).is_err());
    assert!(decode(&font, &[0xff, 0xff, 0, 0], 4, 21).is_err());
    assert!(decode(&font, &[0xff, 0xff, 0xff], 4, 21).is_err());
    Ok(())
}

#[test]
fn academy_controls_preserve_arguments_without_allowing_product_insertion() -> Result<()> {
    let mut font = vec![0; 56];
    font[52..54].copy_from_slice(&('あ' as u16).to_le_bytes());
    let words = [0xf800u16, 1, 1, 0xf801, 0xf812, 0xffff];
    let bytes: Vec<_> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(decode(&font, &bytes, 4, 2)?, "{F800:0001}あ{F801}{F812}");
    assert!(decode(&font, &[0, 0xf8], 4, 2).is_err());
    assert!(tokens("{F800:0001}가{F801}{F812}").is_err());
    Ok(())
}

#[test]
fn academy_segments_keep_waits_highlights_and_symbols() -> Result<()> {
    let source = parse_tokens("먼저\nΘ{F813}\n{F800:0001}Ω뿌요{F801}{F812}끝{F813}", true)?;
    let rewrapped = parse_tokens("먼저 Θ{F813}\n{F800:0001}Ω\n뿌요{F801}{F812}끝{F813}", true)?;
    assert_eq!(academy_segments(&source), academy_segments(&rewrapped));
    let moved = parse_tokens("먼저 Θ{F813}뿌요{F800:0001}Ω{F801}{F812}끝{F813}", true)?;
    assert_ne!(academy_segments(&source), academy_segments(&moved));
    let replaced = parse_tokens("먼저 Ω{F813}\n{F800:0001}Θ뿌요{F801}{F812}끝{F813}", true)?;
    assert_ne!(academy_segments(&source), academy_segments(&replaced));
    assert!(parse_tokens("{F800:01}", true).is_err());
    assert!(parse_tokens("{F800:000a}", true).is_err());
    assert!(parse_tokens("{F882:0001}", true).is_err());
    assert_eq!(
        controls(&parse_tokens("{F800:0001}{F801}{F812}", true)?),
        vec![&[0xf800, 1][..], &[0xf801], &[0xf812]]
    );
    Ok(())
}

#[test]
fn academy_cursor_reset_is_distinct_from_input_wait() -> Result<()> {
    assert!(resets_line_width(&[0xf812], true));
    assert!(!resets_line_width(&[0xf812], false));
    assert!(!resets_line_width(&[0xf813], true));
    assert!(!resets_line_width(&[0xf800, 2], true));
    assert!(resets_line_width(&[0xfffd], true));
    let source = parse_tokens("가\n나{F813}다{F812}라\n마{F813}", true)?;
    let moved = parse_tokens("가나{F813}\n다{F812}라\n마{F813}", true)?;
    assert_ne!(page_line_breaks(&source), page_line_breaks(&moved));
    Ok(())
}
