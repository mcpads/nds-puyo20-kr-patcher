use super::*;
#[test]
fn supersession_requires_exact_old_and_new_bytes_and_is_single_use() -> Result<()> {
    let rule = Supersession {
        file: "a".into(),
        member: 1,
        previous_sha256: sha(b"old"),
        replacement_sha256: sha(b"new"),
    };
    let mut rules = BTreeMap::from([(("a".into(), 1), rule)]);
    assert!(replace_writer("a", 1, b"wrong", b"new", &mut rules).is_err());
    assert!(replace_writer("a", 1, b"old", b"wrong", &mut rules).is_err());
    assert!(replace_writer("b", 1, b"old", b"new", &mut rules).is_err());
    replace_writer("a", 1, b"old", b"new", &mut rules)?;
    assert!(replace_writer("a", 1, b"old", b"new", &mut rules).is_err());
    Ok(())
}
fn fixture() -> Result<Vec<u8>> {
    let mut b = hex::decode(
        "4e415243feff00010000000010000300425441461c000000020000000000000004000000040000000800000042544e46100000000400000000000100474d4946100000006162636465666768",
    )?;
    let len = b.len();
    put32(&mut b, 8, len)?;
    Ok(b)
}
#[test]
fn no_change_and_shrink_preserve_untouched_bytes() -> Result<()> {
    let b = fixture()?;
    assert_eq!(replace(&b, &BTreeMap::new())?, b);
    let mut r = BTreeMap::new();
    r.insert(0, b"xy".to_vec());
    let out = replace(&b, &r)?;
    let n = Narc::parse(&out)?;
    assert_eq!(n.members[0], b"xy");
    assert_eq!(n.members[1], b"efgh");
    let mut expected = b.clone();
    let len = expected.len();
    expected[len - 8..len - 6].copy_from_slice(b"xy");
    put32(&mut expected, 32, 2)?;
    assert_eq!(out, expected);
    r.insert(0, b"exceeds".to_vec());
    assert!(replace(&b, &r).is_err());
    Ok(())
}
