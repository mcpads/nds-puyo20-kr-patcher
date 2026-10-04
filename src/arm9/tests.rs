use super::*;
#[test]
fn backward_matches_overlap_and_preserve_prefix() -> Result<()> {
    let mut data = (0..64).collect::<Vec<u8>>();
    data.extend(b"abcabcabcabcabcabc".repeat(100));
    let encoded = encode(&data, 64, None)?;
    assert_eq!(&encoded[..64], &data[..64]);
    assert_eq!(decode(&encoded)?.0, data);
    let fixed = encode(&data, 64, Some(encoded.len() + 284))?;
    assert_eq!(fixed.len(), encoded.len() + 284);
    assert_eq!(decode(&fixed)?.0, data);
    assert_eq!(&fixed[..64], &data[..64]);
    let mut broken = encoded.clone();
    let n = broken.len();
    put32(&mut broken, n - 4, 1)?;
    assert!(decode(&broken).is_err());
    assert!(decode(&encoded[..n - 1]).is_err());
    assert!(encode(b"uncompressible", 0, None).is_err());
    assert!(encode(&data, 64, Some(16)).is_err());
    Ok(())
}
#[test]
fn rejects_unwritten_reference_and_unread_input_overwrite() {
    // First token is a match, with no output to reference.
    let mut no_output = vec![0, 0, 128];
    no_output.extend([11, 0, 0, 8, 20, 0, 0, 0]);
    assert!(decode(&no_output).is_err());
    // Three literals establish a distance-3 source, then an 18-byte match
    // overtakes unread input when the declared extra space is too small.
    let mut stream = vec![0x10, b'a', b'b', b'c', 0xf0, 0, 1, 2, 3, 4];
    stream.extend([0; 18]);
    stream.reverse();
    stream.extend([36, 0, 0, 8, 1, 0, 0, 0]);
    assert!(
        decode(&stream)
            .unwrap_err()
            .to_string()
            .contains("overwrites unread input")
    );
}
