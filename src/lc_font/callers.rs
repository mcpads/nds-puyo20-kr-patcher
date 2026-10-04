//! Direct ARM call candidates in original overlays, not a complete call graph.
use crate::format::*;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(super) fn inspect(rom: &Rom) -> Result<Value> {
    let mut calls = Vec::new();
    let mut found = Vec::new();
    for overlay in rom.overlays.iter().filter(|o| o.cpu == "arm9") {
        let stored = rom.data(&rom.files[overlay.file_id]);
        let bytes = if overlay.flags & 0x01000000 != 0 {
            crate::arm9::decode(stored)?.0
        } else {
            stored.to_vec()
        };
        ensure!(bytes.len() == overlay.size, "overlay decoded size differs");
        for offset in (0..bytes.len().saturating_sub(3)).step_by(4) {
            let word = u32le(&bytes, offset)? as u32;
            if word & 0x0f000000 != 0x0b000000 || word >> 28 == 15 {
                continue;
            }
            let displacement = ((word << 8) as i32 >> 8) as i64 * 4;
            let address = overlay.ram + offset;
            let target = address as i64 + 8 + displacement;
            if target != 0x020d99f4 && target != 0x020e0f98 {
                continue;
            }
            let instruction = arm946e_s::decode_arm_bytes(slice(&bytes, offset, 4)?)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&instruction)? == slice(&bytes, offset, 4)?,
                "call round trip failed"
            );
            let start = offset.saturating_sub(24);
            let end = (offset + 20).min(bytes.len());
            found.push((overlay.id, address, target));
            calls.push(json!({"overlay":overlay.id,"file_id":overlay.file_id,"decoded_sha256":sha(&bytes),"address":address,"target":target,"state":"arm","instruction":format!("{instruction:?}"),"context_address":overlay.ram+start,"context_hex":hex::encode(&bytes[start..end])}));
        }
    }
    ensure!(
        found
            == vec![
                (0, 0x021457c4, 0x020d99f4),
                (8, 0x021261ac, 0x020e0f98),
                (8, 0x021283f4, 0x020e0f98),
                (9, 0x02127c1c, 0x020e0f98),
                (9, 0x0212808c, 0x020e0f98),
                (9, 0x0212a314, 0x020e0f98),
                (12, 0x02155788, 0x020d99f4),
            ],
        "original overlay call candidates changed"
    );
    Ok(
        json!({"calls":calls,"claim":"aligned direct ARM BL candidates; indirect calls, Thumb and execution reachability are not covered"}),
    )
}
