//! Original Unicode-indexed font inspection; no input or persistence claim.
use crate::{assets::json_file, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

mod callers;
pub mod hangul;
mod residency;
pub use residency::{check_bitmaps, check_loaded};

pub fn inspect(rom: &Rom, ram: Option<&[u8]>, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    if let Some(ram) = ram {
        ensure!(ram.len() == 4 * 1024 * 1024, "expected main RAM dump");
    }
    let font = rom.data(rom.file("lc_font/lc_font_8.bin")?);
    let table = rom.data(rom.file("lc_font/unicode_tbl.bin")?);
    ensure!(
        font.len() == 3136
            && sha(font) == "4b5d5449ff6ab5ba24f0ba29f7a2e9800029a3bc4cb661a7162416a184642be6",
        "LC font changed"
    );
    ensure!(
        table.len() == 776
            && sha(table) == "c7b07819f26ab8a6a460e77b0df93f61823fbf9f8e748b0a23845aa40dbdbbec",
        "Unicode table changed"
    );
    let (arm9, _) = crate::arm9::decode(slice(
        rom.bytes,
        u32le(rom.bytes, 0x20)?,
        u32le(rom.bytes, 0x2c)?,
    )?)?;
    let mut code = Vec::new();
    for (role, start, end, hash) in [
        (
            "pack_name_bitmap",
            0xd8ccc,
            0xd8dec,
            "5f13ef029dcb9ecab822bce9c50e50ada3ffcf18c8f69122d7e0072ad0718b51",
        ),
        (
            "clear_name_bitmaps",
            0xd98a8,
            0xd98bc,
            "aace5a658fb47246125517d89b6232bc7456d178a0f3d617379ebebc47e2884c",
        ),
        (
            "name_bitmap_address",
            0xd98c4,
            0xd98d4,
            "ce8f594c43e6695d3956e3053ec774b8eecbc32c6f6402acd45c6b1cf35c7dea",
        ),
        (
            "unpack_name_bitmap",
            0xd98d8,
            0xd996c,
            "4ec6e7299a3faf2711ecab4b482737a95e318692ec767d7e25b92483cffd5a1f",
        ),
        (
            "draw_player_names",
            0xe0e5c,
            0xe0ecc,
            "f319903a23d2cf02fa167f3a018cc817f5476b4feca52272f0b62ae7bbf7e64c",
        ),
        (
            "import_player_names",
            0xe0ecc,
            0xe0f94,
            "d97e4e4c236a056b6578686b89119fd451e285392f40b244534a358bdfe15af2",
        ),
        (
            "read_personal_data",
            0x6cb0,
            0x6d2c,
            "a3f797ee054399429118c658a1a04072a2da5b8c133e22ecb28547ce6809aba8",
        ),
        (
            "load",
            0xd852c,
            0xd8570,
            "23bf2c44183f07d1a19aabadd18946f7e7a967b50665f62eb65e98dfc427aea6",
        ),
        (
            "lookup",
            0xd85b4,
            0xd8614,
            "6c452d0260841315ef937ec893d18656e810d53621b4d72754890a3c88d64c08",
        ),
        (
            "unpack_glyph",
            0xd8614,
            0xd86ec,
            "7934b0c99d1267cbc4b2f97433abb4965f549bd65c9a675580a75aefce2073c9",
        ),
        (
            "draw_prefix",
            0xd86ec,
            0xd8720,
            "e5017a343ea2bbc416a47b4ef9d8c7aff77d035d5e719c356b73da07ab4bbc34",
        ),
    ] {
        let bytes = slice(&arm9, start, end - start)?;
        ensure!(sha(bytes) == hash, "LC font {role} code changed");
        if let Some(ram) = ram {
            ensure!(
                slice(ram, start, bytes.len())? == bytes,
                "RAM {role} code differs"
            );
        }
        let mut instructions = Vec::new();
        for (i, word) in bytes.chunks_exact(4).enumerate() {
            let instruction = arm946e_s::decode_arm_bytes(word)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&instruction)? == word,
                "ARM round trip failed"
            );
            instructions.push(json!({"address":0x02000000+start+i*4,"bytes":hex::encode(word),"instruction":format!("{instruction:?}")}));
        }
        code.push(json!({"role":role,"address":0x02000000+start,"sha256":hash,"instructions":instructions}));
    }
    for (role, start, end, hash) in [
        (
            "player_name_address",
            0x55d68,
            0x55d72,
            "967a69dc0ffb5a89de8952ddd8799bec108e03309daa62083328785edacec20b",
        ),
        (
            "copy_player_name",
            0x55d78,
            0x55da2,
            "f140488a1fc98680402773f8468f6a16c03a640e95f79f888672d11d41bae6ef",
        ),
    ] {
        let bytes = slice(&arm9, start, end - start)?;
        ensure!(sha(bytes) == hash, "player name {role} code changed");
        if let Some(ram) = ram {
            ensure!(
                slice(ram, start, bytes.len())? == bytes,
                "RAM {role} code differs"
            );
        }
        let mut instructions = Vec::new();
        let mut pos = 0;
        while pos < bytes.len() {
            let size = if u16le(bytes, pos)? >> 11 == 0b11110 {
                4
            } else {
                2
            };
            let word = slice(bytes, pos, size)?;
            let instruction = arm946e_s::decode_thumb_bytes(word)?;
            ensure!(
                arm946e_s::encode_thumb_bytes(&instruction)? == word,
                "Thumb round trip failed"
            );
            instructions.push(json!({"address":0x02000000+start+pos,"bytes":hex::encode(word),"instruction":format!("{instruction:?}")}));
            pos += size;
        }
        code.push(json!({"role":role,"state":"thumb","address":0x02000000+start,"sha256":hash,"instructions":instructions}));
    }
    ensure!(
        u32le(&arm9, 0xd98bc)? == 0x021240bc
            && u32le(&arm9, 0xd98d4)? == 0x021240bc
            && u32le(&arm9, 0xd996c)? == 0x021240bc
            && u32le(&arm9, 0xd98c0)? == 0x020077b8
            && u32le(&arm9, 0x6d2c)? == 0x02fffc80
            && u32le(&arm9, 0x55d74)? == 0x0211d08c
            && u32le(&arm9, 0x55da4)? == 0x0211d08c
            && u32le(&arm9, 0xe0f94)? == 0x020f8eec
            && slice(&arm9, 0xf8eec, 10)? == [b'C', 0, b'O', 0, b'M', 0, b'1', 0, 0, 0],
        "player name source or destination changed"
    );
    for (literal, address, text) in [
        (0xd8570, 0x210a7bc, b"lc_font/lc_font_8.bin\0".as_slice()),
        (0xd8574, 0x210a7d4, b"lc_font/unicode_tbl.bin\0".as_slice()),
    ] {
        ensure!(
            u32le(&arm9, literal)? == address
                && slice(&arm9, address - 0x2000000, text.len())? == text,
            "LC font path reference changed"
        );
    }
    let count = table.len() / 2;
    let mut units = BTreeSet::new();
    let mut entries = Vec::new();
    let mut glyphs = Vec::new();
    let width = 32 * 10;
    let height = count.div_ceil(32) * 10;
    let mut rgba = vec![0u8; width * height * 4];
    for i in 0..count {
        let unit = u16le(table, i * 2)?;
        ensure!(units.insert(unit), "duplicate Unicode identity");
        let character = char::from_u32(unit as u32)
            .ok_or_else(|| anyhow::anyhow!("non-scalar Unicode identity"))?;
        let mut bitmap = Vec::with_capacity(64);
        for p in 0..64 {
            let bit = (font[(i / 8) * 64 + p] >> (i % 8)) & 1;
            bitmap.push(bit);
            let x = (i % 32) * 10 + p % 8;
            let y = (i / 32) * 10 + p / 8;
            rgba[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&[
                255 * bit,
                255 * bit,
                255 * bit,
                255,
            ]);
        }
        entries.push(json!({"index":i,"unicode":format!("U+{unit:04X}"),"character":character.to_string(),"bitmap_sha256":sha(&bitmap)}));
        glyphs.extend(bitmap);
    }
    ensure!(
        sha(&glyphs) == "d9084e5b2b6717c901f77435be5933d8d85c052dbfc32d2d62c320ef0b1a5539",
        "glyph reconstruction differs from initial inspection"
    );
    let residency = ram
        .map(|ram| check_names(ram, table, &glyphs))
        .transpose()?;
    let report = json!({
        "source_sha256":sha(rom.bytes), "font_sha256":sha(font), "table_sha256":sha(table),
        "count":count,"physical_slots":font.len()/8,"glyph_width":8,"glyph_height":8,
        "glyphs_sha256":sha(&glyphs),"hangul_syllables":units.range(0xac00..=0xd7a3).count(),
        "hangul_jamo":units.range(0x1100..=0x11ff).count()+units.range(0x3130..=0x318f).count(),
        "entries":entries,"code":code,
        "overlay_calls":callers::inspect(rom)?,
        "ram":residency,
        "player_names":{"personal_data_address":0x02fffc80usize,"records_address":0x0211d08cusize,"record_stride":36,"cleared_name_bytes":22,"draw_slots":8,"nickname_source_bytes":20},
        "claim":"Source font and personal-data-to-player-name draw path inspection; runtime reachability, other consumers and persistence remain unproven"
    });
    fs::create_dir_all(out)?;
    fs::write(out.join("glyph-bits.bin"), glyphs)?;
    write_png(&out.join("glyphs.png"), width, height, &rgba)?;
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

fn check_names(ram: &[u8], table: &[u8], glyphs: &[u8]) -> Result<Value> {
    let mut slots = Vec::new();
    for slot in 0..8 {
        let record = slice(ram, 0x11d08c + slot * 36, 22)?;
        let units = record
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect::<Vec<_>>();
        let end = units
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| anyhow::anyhow!("unterminated name slot {slot}"))?;
        ensure!(end <= 10, "name too long");
        let mut expected = vec![0u8; 80];
        for (position, &unit) in units[..end].iter().enumerate() {
            if unit == 0x20 || unit == 0x3000 {
                continue;
            }
            let index = table
                .chunks_exact(2)
                .position(|b| u16::from_le_bytes([b[0], b[1]]) == unit)
                .ok_or_else(|| anyhow::anyhow!("unmapped name unit U+{unit:04X}"))?;
            for y in 0..8 {
                for x in 0..8 {
                    expected[position * 8 + y] |= glyphs[index * 64 + y * 8 + x] << x;
                }
            }
        }
        let actual = slice(ram, 0x1240bc + slot * 80, 80)?;
        ensure!(actual == expected, "name bitmap differs in slot {slot}");
        slots.push(json!({"slot":slot,"record_address":0x0211d08c+slot*36,"text":String::from_utf16(&units[..end])?,"bitmap_address":0x021240bc+slot*80,"bitmap_sha256":sha(actual),"bitmap_exact":true}));
    }
    Ok(
        json!({"sha256":sha(ram),"slots":slots,"claim":"record-to-bitmap equality in supplied historical or live RAM; screenshot, reachability and persistence require separate evidence"}),
    )
}
