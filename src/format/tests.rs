use super::nds::names;
use super::*;
#[test]
fn comp_literals_and_overlap_all_lengths() -> Result<()> {
    for (hex, n) in [
        ("1003000000616263", 3),
        ("10060000106162630002", 6),
        ("1103000000616263", 3),
        ("11060000106162632002", 6),
        ("111200004061000000", 18),
        ("11120100406110000000", 274),
        ("110000000300000000616263", 3),
    ] {
        let mut b = b"COMP".to_vec();
        b.extend(hex::decode(hex)?);
        let o = unpack(&b)?;
        assert_eq!(
            o,
            if n == 3 {
                b"abc".to_vec()
            } else if n == 6 {
                b"abcabc".to_vec()
            } else {
                vec![b'a'; n]
            }
        );
    }
    Ok(())
}
#[test]
fn comp_rejects_bad_reference_truncation_and_overrun() {
    for h in ["110300000061", "11030000802000", "1102000040612000"] {
        let mut b = b"COMP".to_vec();
        b.extend(hex::decode(h).unwrap());
        assert!(unpack(&b).is_err());
    }
}
#[test]
fn extents_reject_overflow() {
    assert!(slice(&[0; 8], usize::MAX, 2).is_err());
    assert!(slice(&[0; 8], 7, 2).is_err());
}
#[test]
fn rejects_cyclic_fnt() {
    let b = [8, 0, 0, 0, 0, 0, 1, 0, 0x81, b'x', 0, 0xf0, 0];
    assert!(names(&b).is_err());
}
