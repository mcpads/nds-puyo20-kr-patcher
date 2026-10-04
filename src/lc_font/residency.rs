//! Pointer-bound comparison at the separately recorded loader breakpoint.
use crate::format::*;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub fn check_bitmaps(rom: &Rom, ram: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected main RAM");
    let (font, table) = expanded_files(rom)?;
    let mut glyphs = Vec::with_capacity(table.len() / 2 * 64);
    for i in 0..table.len() / 2 {
        for p in 0..64 {
            glyphs.push((font[i / 8 * 64 + p] >> (i % 8)) & 1);
        }
    }
    let mut report = super::check_names(ram, table, &glyphs)?;
    report["rom_sha256"] = json!(sha(rom.bytes));
    report["font_sha256"] = json!(sha(font));
    report["table_sha256"] = json!(sha(table));
    Ok(report)
}

fn expanded_files<'a>(rom: &'a Rom<'_>) -> Result<(&'a [u8], &'a [u8])> {
    let font = rom.data(rom.file("lc_font/lc_font_8.bin")?);
    let table = rom.data(rom.file("lc_font/unicode_tbl.bin")?);
    ensure!(
        sha(font) == "cd738b49db2e42e92db8ebddf2381c2a8bce2d4d94d3ad1f8188595a246b326b"
            && sha(table) == "1970a33060ea53234db57de0aecb0a66ed21b5adcc5c2e20d67b886d775c2f4e",
        "expected verified expanded font candidate"
    );
    Ok((font, table))
}

pub fn check_loaded(rom: &Rom, ram: &[u8], object: usize) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected main RAM");
    let offset = object
        .checked_sub(0x02000000)
        .ok_or_else(|| anyhow::anyhow!("object outside main RAM"))?;
    let header = slice(ram, offset, 28)?;
    let (font, table) = expanded_files(rom)?;
    ensure!(
        u32le(header, 24)? == table.len() / 2,
        "loader item count differs"
    );
    let mut payloads = Vec::new();
    for (field, bytes, role) in [(16, font, "font"), (20, table, "unicode_table")] {
        let address = u32le(header, field)?;
        let start = address
            .checked_sub(0x02000000)
            .ok_or_else(|| anyhow::anyhow!("payload outside main RAM"))?;
        ensure!(
            slice(ram, start, bytes.len())? == bytes,
            "loaded {role} differs from ROM"
        );
        payloads.push(json!({"role":role,"address":address,"size":bytes.len(),"sha256":sha(bytes),"exact":true}));
    }
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"ram_sha256":sha(ram),"object_address":object,"object_hex":hex::encode(header),"count":table.len()/2,"payloads":payloads,"claim":"Object pointers and full font payloads match artifact; breakpoint identity, heap headroom, timing and visible consumption are separate"}),
    )
}
