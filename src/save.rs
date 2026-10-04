//! Game save banks inside a DeSmuME `.dsv`: two 0x200-byte banks at 0x000 and
//! 0x1000, each closed by a CRC16 of its first 0x1FE bytes.
use crate::format::sha;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

const BANKS: [usize; 2] = [0, 0x1000];
const CRC: usize = 0x1fe;

/// CRC16, reflected polynomial 0xA001, initial value zero.
fn crc16(data: &[u8]) -> u16 {
    let mut c = 0u16;
    for &b in data {
        c ^= u16::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { c >> 1 ^ 0xa001 } else { c >> 1 };
        }
    }
    c
}

fn stored(b: &[u8], bank: usize) -> u16 {
    u16::from_le_bytes([b[bank + CRC], b[bank + CRC + 1]])
}

/// Unlock every story character (bytes 0x08..0x10 of both banks) for runtime
/// observation; banks must be valid before and are resealed after.
pub fn unlock_story(input: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let mut b = fs::read(input)?;
    ensure!(b.len() >= 0x1200, "save too short");
    let source = sha(&b);
    for bank in BANKS {
        ensure!(
            crc16(&b[bank..bank + CRC]) == stored(&b, bank),
            "bank {bank:#x} checksum invalid"
        );
        b[bank + 8..bank + 0x10].fill(0xff);
        let c = crc16(&b[bank..bank + CRC]);
        b[bank + CRC..bank + CRC + 2].copy_from_slice(&c.to_le_bytes());
    }
    fs::write(out, &b)?;
    Ok(
        json!({"input_sha256":source,"output_sha256":sha(&b),"banks":BANKS,"unlocked":"0x08..0x10 = 0xFF"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_reference_check_value() {
        // CRC-16/ARC check value for "123456789".
        assert_eq!(crc16(b"123456789"), 0xbb3d);
    }
}
