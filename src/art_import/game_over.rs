//! Target-local horizontal repositioning of the four Korean game-over glyphs.
use crate::format::*;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Verify adopted coordinate writes against the final ROM, then render its end keys.
pub fn review_final(jp: &Rom, kr: &Rom, out: &std::path::Path) -> Result<Value> {
    let path = "puyo/result/result.narc";
    let source = Narc::parse(jp.data(jp.file(path)?))?;
    let product = Narc::parse(kr.data(kr.file(path)?))?;
    let mut records = Vec::new();
    for id in [51, 54] {
        let original = unpack(source.members[id])?;
        let actual = unpack(product.members[id])?;
        let (expected, report) = reposition(id, &original)?;
        ensure!(
            actual == expected,
            "final game-over layout {id} differs from adopted writes"
        );
        records.push(report);
    }
    let members = [51, 54, 55, 57, 58, 59, 60, 62];
    let changes = members
        .into_iter()
        .map(|id| (id, product.members[id].to_vec()))
        .collect();
    preview(&product, &changes, out)?;
    Ok(
        json!({"archive":path,"source_sha256":sha(jp.bytes),"product_sha256":sha(kr.bytes),
        "members":records,"final_rom_equal":true,
        "previews":["game-over-obj-end-key.png","game-over-atlas-end-key.png"],
        "claim":"Exact adopted horizontal coordinate writes and final-key static composition; animation and runtime are separate."}),
    )
}

/// Static end-key composition, not an emulator frame or interpolation proof.
pub fn preview(n: &Narc, changes: &BTreeMap<usize, Vec<u8>>, out: &std::path::Path) -> Result<()> {
    for atlas in [false, true] {
        let palette = unpack(n.members[if atlas { 61 } else { 56 }])?;
        let mut rgba = vec![0u8; 160 * 64 * 4];
        let sheet = if atlas {
            unpack(&changes[&62])?
        } else {
            Vec::new()
        };
        let layout = unpack(&changes[&if atlas { 54 } else { 51 }])?;
        for (member, slot) in [(55, 0), (57, 2), (58, 3), (59, 5)] {
            let mut xy = [0i32; 2];
            for (axis, value) in xy.iter_mut().enumerate() {
                let (base, channel, stride, unit) = if atlas {
                    let record = 0x970 + (slot * 2 + 1) * 8;
                    (
                        32,
                        32 + u32le(&layout, record + 4)? + (axis + 1) * 12,
                        24,
                        16,
                    )
                } else {
                    let record = 0x1fb0 + (slot + 1) * 32;
                    (224, 224 + u32le(&layout, record + 8)? + axis * 16, 16, 4096)
                };
                let count = u32le(&layout, channel + 4)?;
                let keys = base + u32le(&layout, channel + 8)?;
                *value = u32le(&layout, keys + (count - 1) * stride + 4)? as i32 / unit;
            }
            let [x, y] = xy;
            let px = if atlas {
                sheet[slot * 1024..(slot + 1) * 1024].to_vec()
            } else {
                crate::titles::untile(&unpack(&changes[&member])?, 32, 32, 4)?
            };
            for py in 0..32 {
                for px_x in 0..32 {
                    let v = px[py * 32 + px_x];
                    let index = if atlas { v & 31 } else { v };
                    let alpha = if atlas {
                        (u16::from(v >> 5) * 255 / 7) as u8
                    } else if index == 0 {
                        0
                    } else {
                        255
                    };
                    if alpha == 0 {
                        continue;
                    }
                    let color = u16le(&palette, usize::from(index) * 2)?;
                    let dx = (80 + x - 16 + px_x as i32) as usize;
                    let dy = (32 + y - 16 + py as i32) as usize;
                    ensure!(dx < 160 && dy < 64, "static game-over composition clipped");
                    rgba[(dy * 160 + dx) * 4..(dy * 160 + dx + 1) * 4].copy_from_slice(&[
                        ((color & 31) * 255 / 31) as u8,
                        (((color >> 5) & 31) * 255 / 31) as u8,
                        (((color >> 10) & 31) * 255 / 31) as u8,
                        alpha,
                    ]);
                }
            }
        }
        crate::graphics::write_png(
            &out.join(if atlas {
                "game-over-atlas-end-key.png"
            } else {
                "game-over-obj-end-key.png"
            }),
            160,
            64,
            &rgba,
        )?;
    }
    Ok(())
}

fn translated(old: &[u8], writes: &mut BTreeMap<usize, i32>, p: usize, delta: i32) -> Result<()> {
    let value = u32le(old, p)? as i32;
    let new = value
        .checked_add(delta)
        .ok_or_else(|| anyhow::anyhow!("coordinate overflow"))?;
    ensure!(writes.insert(p, new).is_none(), "shared coordinate write");
    Ok(())
}

pub fn reposition(member: usize, old: &[u8]) -> Result<(Vec<u8>, Value)> {
    let expected = match member {
        51 => "56af771b90b3bd46ff254fd3b0a6847d5d15b85811408c6739386946e287b6e7",
        54 => "4a2747d33f5d5457a01be3c4faf1bdc88f05afaae10599622a9e2619b779757f",
        _ => anyhow::bail!("not a game-over layout"),
    };
    ensure!(sha(old) == expected, "game-over layout source differs");
    let mut writes = BTreeMap::new();
    // Keep complete 32px glyphs separate: 32px within words, 44px between words.
    for (slot, target) in [(0usize, -54i32), (2, -22), (3, 22), (5, 54)] {
        if member == 51 {
            let base = u32le(old, 20)?;
            ensure!(base == 224, "Gem base");
            let node = 0xa90 + slot * 80;
            let original = u32le(old, node + 32)? as i32;
            let delta = target * 4096 - original;
            translated(old, &mut writes, node + 32, delta)?;
            for array in [0x1fb0, 0x20b0] {
                let record = array + (slot + 1) * 32;
                ensure!(
                    u32le(old, record + 4)? == 3 && u32le(old, record + 16)? == 0x23,
                    "Gem track flags"
                );
                let channel = base + u32le(old, record + 8)?;
                ensure!(u32le(old, channel)? == 0, "Gem horizontal channel");
                let count = u32le(old, channel + 4)?;
                ensure!(
                    count == if array == 0x1fb0 { 3 } else { 2 },
                    "Gem key count"
                );
                let keys = base + u32le(old, channel + 8)?;
                ensure!(
                    u32le(old, keys + (count - 1) * 16 + 4)? as i32 == original,
                    "Gem final x differs from node"
                );
                for key in 0..count {
                    translated(old, &mut writes, keys + key * 16 + 4, delta)?;
                }
            }
        } else {
            let index = slot * 2 + 1;
            let node = 32 + u32le(old, 0x300 + index * 4)?;
            let transform = 32 + u32le(old, node + 48)?;
            let original = u32le(old, transform + 4)? as i32;
            ensure!(original == (slot as i32 * 24 - 72) * 16, "DSIF original x");
            let delta = target * 16 - original;
            translated(old, &mut writes, transform + 4, delta)?;
            for array in [0x970, 0x9e8] {
                let record = array + index * 8;
                ensure!(u32le(old, record)? == 7, "DSIF track flags");
                let channel = 32 + u32le(old, record + 4)? + 12;
                let count = u32le(old, channel + 4)?;
                ensure!(
                    count == if array == 0x970 { 3 } else { 2 },
                    "DSIF x key count"
                );
                let keys = 32 + u32le(old, channel + 8)?;
                ensure!(
                    u32le(old, keys + (count - 1) * 24 + 4)? as i32 == original,
                    "DSIF final x differs from node"
                );
                for key in 0..count {
                    translated(old, &mut writes, keys + key * 24 + 4, delta)?;
                }
            }
        }
    }
    ensure!(writes.len() == 24, "unexpected coordinate population");
    let mut bytes = old.to_vec();
    let mut records = Vec::new();
    for (&p, &value) in &writes {
        bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
        records.push(json!({"offset":p,"expected":u32le(old,p)? as i32,"value":value}));
    }
    for i in 0..bytes.len() {
        if !writes.contains_key(&(i & !3)) {
            ensure!(bytes[i] == old[i], "unplanned layout difference");
        }
    }
    let report = json!({"member":member,"source_sha256":expected,"output_sha256":sha(&bytes),"writes":records,"protected":"all other bytes, key times, interpolation modes and tangents, vertical motion, visibility, hierarchy and resource links","runtime_verified":false});
    Ok((bytes, report))
}
