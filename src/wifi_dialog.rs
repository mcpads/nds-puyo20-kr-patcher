//! Static Wi-Fi dialog path references; candidate windows are not execution proof.
use crate::{assets::json_file, format::*};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let mut regions = Vec::new();
    let (arm9, _) = crate::arm9::decode(slice(
        rom.bytes,
        u32le(rom.bytes, 0x20)?,
        u32le(rom.bytes, 0x2c)?,
    )?)?;
    for (i, section) in crate::arm9::sections(&arm9)?.0.into_iter().enumerate() {
        regions.push((format!("arm9-section-{i}"), section.address, section.bytes));
    }
    for o in rom.overlays.iter().filter(|o| o.cpu == "arm9") {
        let stored = rom.data(&rom.files[o.file_id]);
        let bytes = if o.flags & 0x01000000 != 0 {
            crate::arm9::decode(stored)?.0
        } else {
            stored.to_vec()
        };
        ensure!(bytes.len() == o.size, "overlay size differs");
        regions.push((format!("overlay-{}", o.id), o.ram, bytes));
    }
    ensure!(
        u32le(&arm9, 0x74cfc)? == 0x020da034,
        "glyph lookup target changed"
    );
    let mut verified_code = Vec::new();
    for (name, region, start, end, expected) in [
        (
            "error-number-call",
            "overlay-9",
            0x212cd78usize,
            0x212ce38usize,
            "115287f8e73bf88f6a829bdf5da38cd3beedc26420f0dc77278b5d1b2f6c7e58",
        ),
        (
            "text-substitution",
            "arm9-section-0",
            0x20754f8usize,
            0x20755ccusize,
            "ab6dd7966a6db4a98d019f9c93e0a52bdfd3a44c85e47a78dad7cf47d843acf3",
        ),
        (
            "glyph-lookup-wrapper",
            "arm9-section-0",
            0x2074cf0usize,
            0x2074cfcusize,
            "5e7d16f9ed9a20517e82a73b6b37ca0f00df60a84d688b974b035b8874fd9b38",
        ),
        (
            "glyph-lookup",
            "arm9-section-0",
            0x20da034usize,
            0x20da088usize,
            "d49d82dc158f833d1db519df5aba30702342f1b8ce51c9bab5acb4ed34afc164",
        ),
    ] {
        let (_, base, bytes) = regions
            .iter()
            .find(|(id, base, bytes)| id == region && start >= *base && end <= base + bytes.len())
            .ok_or_else(|| anyhow::anyhow!("missing code region: {name}"))?;
        let code = &bytes[start - base..end - base];
        ensure!(sha(code) == expected, "code changed: {name}");
        let mut instructions = Vec::new();
        for (i, word) in code.chunks_exact(4).enumerate() {
            let instruction = arm946e_s::decode_arm_bytes(word)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&instruction)? == word,
                "code round trip: {name}"
            );
            instructions.push(json!({"address":start+i*4,"bytes":hex::encode(word),"instruction":format!("{instruction:?}")}));
        }
        verified_code.push(json!({"name":name,"region":region,"start":start,"end_exclusive":end,"sha256":expected,"instructions":instructions}));
    }
    let mut entries = Vec::new();
    let mut outputs = Vec::new();
    for (name, base, bytes) in &regions {
        let mut paths = Vec::new();
        for suffix in ["fnt", "mtx"] {
            let path = format!("text/menu/wifi_dialog.{suffix}\0");
            for (offset, b) in bytes.windows(path.len()).enumerate() {
                if b != path.as_bytes() {
                    continue;
                }
                let address = base + offset;
                let mut references = Vec::new();
                for (p, word) in bytes.chunks_exact(4).enumerate() {
                    if u32::from_le_bytes(word.try_into().unwrap()) as usize != address {
                        continue;
                    }
                    let pos = p * 4;
                    let start = pos.saturating_sub(128);
                    let end = (pos + 4).min(bytes.len());
                    let mut window = Vec::new();
                    for (i, w) in bytes[start..end].chunks_exact(4).enumerate() {
                        let a = base + start + i * 4;
                        let decoded = match arm946e_s::decode_arm_bytes(w) {
                            Ok(instruction) => {
                                ensure!(
                                    arm946e_s::encode_arm_bytes(&instruction)? == w,
                                    "ARM round trip failed"
                                );
                                format!("{instruction:?}")
                            }
                            Err(e) => format!("undecoded: {e}"),
                        };
                        window.push(
                            json!({"address":a,"bytes":hex::encode(w),"arm_candidate":decoded}),
                        );
                    }
                    let mut loads = Vec::new();
                    for (i, raw) in bytes.chunks_exact(4).enumerate() {
                        let word = u32::from_le_bytes(raw.try_into().unwrap());
                        if word & 0x0f7f0000 != 0x051f0000 || word >> 28 == 15 {
                            continue;
                        }
                        let address = base + i * 4;
                        let displacement =
                            (word & 0xfff) as i64 * if word & 0x00800000 != 0 { 1 } else { -1 };
                        if address as i64 + 8 + displacement != (base + pos) as i64 {
                            continue;
                        }
                        let start = (i * 4).saturating_sub(16);
                        let end = (i * 4 + 36).min(bytes.len());
                        let mut context = Vec::new();
                        for (j, w) in bytes[start..end].chunks_exact(4).enumerate() {
                            let instruction = arm946e_s::decode_arm_bytes(w)?;
                            ensure!(
                                arm946e_s::encode_arm_bytes(&instruction)? == w,
                                "literal load context round trip failed"
                            );
                            let pc = base + start + j * 4;
                            let word = u32::from_le_bytes(w.try_into().unwrap());
                            let target = if word & 0x0f000000 == 0x0b000000 && word >> 28 != 15 {
                                Some(pc as i64 + 8 + ((word << 8) as i32 >> 8) as i64 * 4)
                            } else {
                                None
                            };
                            context.push(json!({"address":pc,"instruction":format!("{instruction:?}"),"direct_bl_target":target}));
                        }
                        loads.push(json!({"address":address,"destination_register":(word>>12)&15,"context":context}));
                    }
                    references.push(json!({"address":base+pos,"preceding_window":window,"literal_load_candidates":loads}));
                }
                paths.push(json!({"path":path.trim_end_matches('\0'),"address":address,"same_region_pointer_values":references}));
            }
        }
        if !paths.is_empty() {
            entries.push(json!({"region":name,"ram_base":base,"decoded_size":bytes.len(),"decoded_sha256":sha(bytes),"paths":paths}));
            outputs.push((format!("{name}.bin"), bytes));
        }
    }
    ensure!(!entries.is_empty(), "no dialog paths found");
    fs::create_dir_all(out)?;
    for (name, bytes) in outputs {
        fs::write(out.join(name), bytes)?;
    }
    let report = json!({"source_sha256":sha(rom.bytes),"searched_regions":regions.len(),"verified_code":verified_code,"entries":entries,"claim":"exact path bytes and aligned same-region pointer values; preceding ARM candidate windows include possible literal data; no complete cross-reference or runtime claim"});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

/// Compare the active message before and after the observed substitution call.
pub fn check_substitution(
    rom: &Rom,
    before: &[u8],
    after: &[u8],
    object: usize,
    reference: usize,
    number: &str,
) -> Result<Value> {
    ensure!(
        before.len() == 0x400000 && after.len() == before.len(),
        "expected 4 MiB RAM pair"
    );
    ensure!(
        number.len() == 5 && number.bytes().all(|c| c.is_ascii_digit()),
        "expected five ASCII digits"
    );
    let offset = |address: usize| {
        address
            .checked_sub(0x02000000)
            .ok_or_else(|| anyhow::anyhow!("pointer below RAM"))
    };
    let root = offset(object)?;
    let text_address = u32le(before, root + 0x5c)?;
    let wrapper = u32le(before, root + 0x60)?;
    let font_object = u32le(before, offset(wrapper)? + 8)?;
    let font_address = u32le(before, offset(font_object)? + 16)?;
    let f = unpack_halfword(rom.data(rom.file("text/menu/wifi_dialog.fnt")?))?;
    let t = unpack(rom.data(rom.file("text/menu/wifi_dialog.mtx")?))?;
    let info = text_pair(&f, &t)?;
    ensure!(
        (9..=22).contains(&reference),
        "not an error message reference"
    );
    let start = *info
        .references
        .get(reference)
        .ok_or_else(|| anyhow::anyhow!("missing reference"))?;
    let end = info
        .references
        .get(reference + 1)
        .copied()
        .unwrap_or(t.len());
    let original = &t[start..end];
    let lookup = |c: u16| -> Result<u16> {
        let matches = (0..info.glyph_count)
            .filter(|i| u16le(&f, 48 + i * info.stride).ok() == Some(c as usize))
            .collect::<Vec<_>>();
        ensure!(matches.len() == 1, "missing/duplicate substitution glyph");
        Ok(matches[0] as u16)
    };
    let star = lookup(b'*' as u16)?;
    let units = original
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect::<Vec<_>>();
    let positions = units
        .iter()
        .enumerate()
        .filter_map(|(i, v)| (*v == star).then_some(i))
        .collect::<Vec<_>>();
    ensure!(
        positions.len() == 5 && positions.windows(2).all(|p| p[1] == p[0] + 1),
        "placeholder shape changed"
    );
    let mut expected = original.to_vec();
    for (i, c) in positions.iter().zip(number.bytes()) {
        expected[i * 2..i * 2 + 2].copy_from_slice(&lookup(c as u16)?.to_le_bytes());
    }
    for ram in [before, after] {
        ensure!(
            slice(ram, offset(font_address)?, f.len())? == f,
            "runtime font differs"
        );
        ensure!(
            u32le(ram, root + 0x5c)? == text_address && u32le(ram, root + 0x60)? == wrapper,
            "text object pointers changed"
        );
    }
    ensure!(
        slice(before, offset(text_address)?, original.len())? == original,
        "active source message differs"
    );
    ensure!(
        slice(after, offset(text_address)?, expected.len())? == expected,
        "substitution differs beyond five digits"
    );
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"before_ram_sha256":sha(before),"after_ram_sha256":sha(after),"object":object,"text_address":text_address,"font_wrapper":wrapper,"font_object":font_object,"font_address":font_address,"font_sha256":sha(&f),"reference":reference,"number":number,"placeholder_slot":star,"changed_unit_positions":positions,"message_bytes":original.len(),"claim":"full runtime font and active message match this ROM; after message differs exactly by the five requested digit slots; launch and call-register binding recorded separately"}),
    )
}
