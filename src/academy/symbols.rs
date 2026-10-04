//! Inspect the observed tutorial glyph replacement, retaining strict byte evidence.
use super::*;
pub fn inspect(rom: &Rom, ram: &[u8], out: &Path) -> Result<Value> {
    ensure!(
        !out.exists() && ram.len() == 0x400000,
        "expected new output and 4 MiB RAM"
    );
    let (code, overlay_hash) = verify_code(rom, ram)?;
    let source = unpack(rom.data(rom.file("text/academy/tuto00.fnt")?))?;
    let base = 0x167b00;
    ensure!(
        source.len() == 17804 && slice(ram, base, 48)? == &source[..48],
        "font header changed"
    );
    let archive = Narc::parse(rom.data(rom.file("academy/academy.narc")?))?;
    let decoded = archive
        .members
        .iter()
        .map(|b| unpack(b))
        .collect::<Result<Vec<_>>>()?;
    let mut symbols = Vec::new();
    for slot in 0..193 {
        let offset = 48 + slot * 92;
        let old = slice(&source, offset, 92)?;
        let live = slice(ram, base + offset, 92)?;
        if ![4, 5].contains(&slot) {
            ensure!(old == live, "unexpected glyph change: {slot}");
            continue;
        }
        let cp = if slot == 4 { 0x398 } else { 0x3a9 };
        ensure!(
            u16le(old, 0)? == cp
                && u16le(live, 0)? == cp
                && u16le(old, 2)? == 9
                && u16le(live, 2)? == 13,
            "symbol identity/advance changed"
        );
        let pixels = &live[4..];
        let mut matches = Vec::new();
        for (member, b) in decoded.iter().enumerate() {
            for (offset, _) in b
                .windows(pixels.len())
                .enumerate()
                .filter(|(_, w)| *w == pixels)
            {
                matches.push(json!({"member":member,"offset":offset,"decoded_sha256":sha(b),"decoded_bytes":b.len(),"matched_bytes":pixels.len()}));
            }
        }
        ensure!(
            matches.len() == 1
                && matches[0]["member"] == if slot == 4 { 179 } else { 180 }
                && matches[0]["offset"] == 0,
            "symbol source asset match changed"
        );
        symbols.push(json!({"codepoint":cp,"slot":slot,"resource_id":if slot==4 {0xb3} else {0xb4},"runtime_address":0x02000000+base+offset,"runtime_pixels_sha256":sha(pixels),"archive_matches":matches}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"source_code_equal":true,"overlay_id":11,"overlay_sha256":overlay_hash,"ram_sha256":sha(ram),"code":code,"symbols":symbols,"unchanged_other_slots":191,"claim":"exact observed code, mapping, glyph changes and asset byte matches; resource ID routing and other lesson instances require separate evidence"});
    json_file(out, &report)?;
    Ok(report)
}

fn verify_code(rom: &Rom, ram: &[u8]) -> Result<(Vec<Value>, String)> {
    let overlay = rom
        .overlays
        .iter()
        .find(|o| o.cpu == "arm9" && o.id == 11)
        .ok_or_else(|| anyhow::anyhow!("missing academy overlay"))?;
    let (overlay_bytes, _) = crate::arm9::decode(rom.data(&rom.files[overlay.file_id]))?;
    ensure!(
        overlay.ram == 0x02153380 && overlay_bytes.len() == overlay.size,
        "academy overlay layout changed"
    );
    let (arm9, _) = crate::arm9::decode(slice(
        rom.bytes,
        u32le(rom.bytes, 0x20)?,
        u32le(rom.bytes, 0x2c)?,
    )?)?;
    let mut code = Vec::new();
    for (start, end, expected) in [
        (
            0x153eb4,
            0x153f24,
            "2cb54fb1b2d798b8c2fa86578ce118c9c2e1a08b420c5b396f12b02955574e01",
        ),
        (
            0xda034,
            0xda104,
            "c3a84292a4fcfbc3fb2856de3bd597269daed3a1d1d6e2efe51d961f6d0fe5c5",
        ),
    ] {
        let bytes = slice(ram, start, end - start)?;
        let original = if start >= 0x153380 {
            slice(
                &overlay_bytes,
                0x02000000 + start - overlay.ram,
                end - start,
            )?
        } else {
            slice(&arm9, start, end - start)?
        };
        ensure!(bytes == original, "source/runtime symbol code differs");
        ensure!(sha(bytes) == expected, "symbol code changed");
        let mut instructions = Vec::new();
        for (i, b) in bytes.chunks_exact(4).enumerate() {
            let instruction = arm946e_s::decode_arm_bytes(b)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&instruction)? == b,
                "instruction round trip failed"
            );
            instructions.push(json!({"address":0x02000000+start+i*4,"bytes":hex::encode(b),"instruction":format!("{instruction:?}")}));
        }
        code.push(
            json!({"address":0x02000000+start,"sha256":expected,"instructions":instructions}),
        );
    }
    ensure!(
        u32le(ram, 0x154268)? == 0x02158bbc
            && slice(ram, 0x158bbc, 8)? == hex::decode("a903b4009803b300")?,
        "symbol mapping changed"
    );
    ensure!(
        slice(&overlay_bytes, 0x2158bbc - overlay.ram, 8)? == slice(ram, 0x158bbc, 8)?,
        "source/runtime mapping differs"
    );
    Ok((code, sha(&overlay_bytes)))
}

/// Only the two proven codepoint substitutions are permitted; all other bytes match.
pub fn check_ram(rom: &Rom, ram: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB RAM");
    let (code, overlay_hash) = verify_code(rom, ram)?;
    let path = "text/academy/tuto00";
    let stored = unpack_halfword(rom.data(rom.file(&format!("{path}.fnt"))?))?;
    let text = unpack(rom.data(rom.file(&format!("{path}.mtx"))?))?;
    let info = text_pair(&stored, &text)?;
    ensure!(
        info.width == 16 && info.height == 11 && info.stride == 92,
        "unsupported academy font"
    );
    let archive = Narc::parse(rom.data(rom.file("academy/academy.narc")?))?;
    let mut expected = stored.clone();
    let mut symbols = Vec::new();
    for (cp, member, hash) in [
        (
            0x398,
            179,
            "1a05e6cc526a379c72ed2cfe6ed23cd8ac26763062325502ecf95d874675cd5f",
        ),
        (
            0x3a9,
            180,
            "d662f105fc33f515efa52db04ce5aa93135f2fa2ca90afae836694624557ea8f",
        ),
    ] {
        let slots = (0..info.glyph_count)
            .filter(|i| u16le(&stored, 48 + i * 92).ok() == Some(cp))
            .collect::<Vec<_>>();
        ensure!(slots.len() == 1, "expected one symbol slot");
        let pixels = unpack(
            archive
                .members
                .get(member)
                .ok_or_else(|| anyhow::anyhow!("missing symbol asset"))?,
        )?;
        ensure!(
            pixels.len() == 128 && sha(&pixels) == hash,
            "symbol asset changed"
        );
        let offset = 48 + slots[0] * 92;
        expected[offset + 2..offset + 4].copy_from_slice(&13u16.to_le_bytes());
        expected[offset + 4..offset + 92].copy_from_slice(&pixels[..88]);
        symbols.push(json!({"codepoint":cp,"slot":slots[0],"member":member,"asset_sha256":hash,"runtime_advance":13}));
    }
    let mut report = crate::localize::check_decoded_ram(rom, path, ram, &expected)?;
    report["stored_font_sha256"] = json!(sha(&stored));
    report["symbols"] = json!(symbols);
    report["code"] = json!(code);
    report["overlay_sha256"] = json!(overlay_hash);
    report["claim"] = json!(
        "complete ROM font with exactly two verified symbol substitutions and fully relocated MTX equal frozen RAM; display coverage recorded separately"
    );
    Ok(report)
}
