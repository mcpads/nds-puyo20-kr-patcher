use crate::format::*;
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

const FNT: usize = 0x02162c80;
const MTX: usize = 0x021643d8;
const BASE: usize = 0x02000000;

pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    ensure!(
        ram.len() == 0x400000,
        "expected 4 MiB main RAM at 0x02000000"
    );
    let f = unpack(rom.data(rom.file("text/menu/main_menu.fnt")?))?;
    let t = rom.data(rom.file("text/menu/main_menu.mtx")?);
    ensure!(
        slice(ram, FNT - BASE, f.len())? == f,
        "font not at observed checkpoint address"
    );
    let payload = u32le(t, 12)?;
    let mut relocated = t.to_vec();
    for p in (4..payload).step_by(4) {
        put32(&mut relocated, p, MTX + u32le(t, p)?)?;
    }
    ensure!(
        slice(ram, MTX - BASE, t.len())? == relocated,
        "MTX relocation mismatch"
    );
    Ok(
        json!({"ram_sha256":sha(ram),"font":{"address":FNT,"size":f.len(),"sha256":sha(&f)},"mtx":{"address":MTX,"size":t.len(),"source_sha256":sha(t),"relocated_sha256":sha(&relocated),"payload_offset":payload},"claim":"fixed original-menu checkpoint residency; not live binding verification"}),
    )
}
#[derive(Deserialize)]
struct Glyph {
    character: String,
    width: u16,
}
#[derive(Deserialize)]
struct Glyphs {
    pixels_sha256: String,
    width: usize,
    height: usize,
    glyphs: Vec<Glyph>,
    lines: Vec<String>,
}
pub fn prepare(rom: &Rom, ram: &[u8], assets: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    residency(rom, ram)?;
    let g: Glyphs = serde_json::from_slice(&fs::read(assets.join("glyphs.json"))?)?;
    let pixels = fs::read(assets.join("glyphs.bin"))?;
    ensure!(
        sha(&pixels) == g.pixels_sha256
            && g.width == 16
            && g.height == 12
            && g.glyphs.len() <= 58
            && pixels.len() == g.glyphs.len() * 96
            && g.lines.len() == 2,
        "invalid frozen glyph input"
    );
    let f = unpack(rom.data(rom.file("text/menu/main_menu.fnt")?))?;
    let t = rom.data(rom.file("text/menu/main_menu.mtx")?);
    let info = text_pair(&f, t)?;
    ensure!(
        info.stride == 100 && info.glyph_count == 58,
        "unexpected font geometry"
    );
    let mut changed = f.clone();
    let mut characters = BTreeSet::new();
    ensure!(
        pixels.iter().all(|v| v & 15 <= 2 && v >> 4 <= 2),
        "unknown frozen palette index"
    );
    for (i, glyph) in g.glyphs.iter().enumerate() {
        ensure!(
            glyph.character.chars().count() == 1
                && glyph.width <= 16
                && characters.insert(&glyph.character),
            "invalid glyph metadata"
        );
        changed[48 + i * 100 + 2..48 + i * 100 + 4].copy_from_slice(&glyph.width.to_le_bytes());
        changed[48 + i * 100 + 4..48 + (i + 1) * 100]
            .copy_from_slice(&pixels[i * 96..(i + 1) * 96]);
    }
    let mut units = Vec::<u16>::new();
    for (i, line) in g.lines.iter().enumerate() {
        if i == 1 {
            units.extend([0xfffe, 0xfffd]);
        }
        for c in line.chars() {
            let slot = g
                .glyphs
                .iter()
                .position(|x| x.character == c.to_string())
                .ok_or_else(|| anyhow::anyhow!("unmapped character {c}"))?;
            units.push(slot as u16);
        }
    }
    units.push(0xffff);
    let start = info.references[0];
    let end = info.references[1];
    ensure!(units.len() * 2 <= end - start, "text capacity exceeded");
    units.resize((end - start) / 2, 0xffff);
    let text = units
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    fs::create_dir_all(out)?;
    let mut writes = Vec::new();
    for (name, address, before, after) in [
        ("font", FNT, f.as_slice(), changed.as_slice()),
        ("text", MTX + start, &t[start..end], text.as_slice()),
    ] {
        fs::write(out.join(format!("{name}.before.bin")), before)?;
        fs::write(out.join(format!("{name}.after.bin")), after)?;
        writes.push(json!({"name":name,"address":address,"length":after.len(),"before_sha256":sha(before),"after_sha256":sha(after)}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"ram_sha256":sha(ram),"glyph_pixels_sha256":sha(&pixels),"lines":g.lines,"writes":writes,"scope":"Frozen first-menu RAM experiment, no ROM or font approval; caller must check live before bytes and restore"});
    crate::assets::json_file(&out.join("plan.json"), &report)?;
    Ok(report)
}
