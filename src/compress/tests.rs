use super::*;
use crate::format::unpack;
#[test]
fn bounded_layout_parse_roundtrips_across_match_and_sprite_limits() -> Result<()> {
    let data: Vec<u8> = (0..5000).map(|i| ((i % 1024) * 17 % 251) as u8).collect();
    let packed = pack_layout(&data)?;
    assert_eq!(unpack_halfword(&packed)?, data);
    assert!(pack_layout(&vec![0; 65538]).is_err());
    Ok(())
}
#[test]
fn compact_parse_saves_capacity_and_preserves_halfword_output() -> Result<()> {
    // A longest match is not always the smallest complete stream.
    let bytes = hex::decode(
        "010001030301030100030203000201010002000002020103020100000103010003010200010301030003020300020302",
    )?;
    assert_eq!(pack(&bytes)?.len(), 53);
    let compact = pack_compact(&bytes)?;
    assert_eq!(compact.len(), 52);
    assert_eq!(unpack_halfword(&compact)?, bytes);
    for len in [6, 16, 18, 272, 274, 512] {
        let data = vec![42; len];
        let packed = pack_compact(&data)?;
        assert!(packed.len() <= pack(&data)?.len());
        assert_eq!(unpack_halfword(&packed)?, data);
    }
    assert!(pack_compact(b"odd").is_err());
    assert!(pack_compact(&vec![0; 4098]).is_err());
    Ok(())
}
#[test]
fn rejects_byte_valid_stream_that_reads_uncommitted_halfword() -> Result<()> {
    let mut stream = b"COMP".to_vec();
    stream.extend(hex::decode("1106000040614000")?);
    assert_eq!(unpack(&stream)?, b"aaaaaa");
    assert!(unpack_halfword(&stream).is_err());
    assert_eq!(unpack_halfword(&pack(b"aaaaaa")?)?, b"aaaaaa");
    Ok(())
}
#[test]
fn encodes_known_literals_and_overlapping_matches() -> Result<()> {
    assert_eq!(
        hex::encode(pack(b"abcabc")?),
        "434f4d5011060000106162632002"
    );
    for n in [4, 16, 18, 272, 274, 4096, 65810, 70000] {
        assert_eq!(unpack(&pack(&vec![b'a'; n])?)?, vec![b'a'; n]);
    }
    let bytes = (0..12000)
        .map(|i| ((i * 37 + i / 255) % 256) as u8)
        .collect::<Vec<_>>();
    assert_eq!(unpack(&pack(&bytes)?)?, bytes);
    assert!(pack(&[]).is_err());
    assert!(pack(b"abc").is_err());
    Ok(())
}
