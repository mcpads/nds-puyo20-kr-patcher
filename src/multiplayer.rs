//! DS recruitment status textures; runtime selection is verified separately.
mod panels;
use crate::{assets::json_file, battle_ui, buttons, format::*};
use anyhow::{Result, ensure};
pub use panels::prepare as prepare_panels;
pub use panels::prepare_participation;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    japanese: String,
    korean: String,
}

pub fn prepare_status(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = "menu/multi_raise.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "777b0fd31aed8a3b7ffbbbc4dd84cc7ca1fe4d293345662fc598c39d0daae3c4",
        "multiplayer archive changed"
    );
    let n = Narc::parse(source)?;
    let table = rom.data(rom.file("menu/multi_invite01_b_texlist.bin")?);
    ensure!(
        sha(table) == "a39cda57667fb442758f26860b0d5347dde9574b2c90143b9587fc68c2d686d8",
        "multiplayer table changed"
    );
    let input = fs::read(translation)?;
    let mut layouts = Vec::new();
    for member in [24, 25, 26, 35] {
        let bytes = unpack(n.members[member])?;
        ensure!(
            slice(&bytes, 0, 4)? == b"DSIF"
                && u32le(&bytes, 12)? == 32
                && slice(&bytes, 32, 4)? == b"nCSC",
            "recruitment layout changed"
        );
        ensure!(u32le(&bytes, 76)? == 16, "recruitment crop count changed");
        let offset = 32 + u32le(&bytes, 80)?;
        for texture in 9..12 {
            let crop = offset + texture * 20;
            ensure!(
                u32le(&bytes, crop)? == texture,
                "status crop texture changed"
            );
            for (axis, expected) in [0, 0, 2432, 4096].iter().enumerate() {
                ensure!(
                    u32le(&bytes, crop + 4 + axis * 4)? == *expected,
                    "status visible crop changed"
                );
            }
        }
        layouts.push(json!({"member":member,"sha256":sha(&bytes),"status_crop":[0,0,76,16]}));
    }
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 3,
        "unexpected status population"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        sha(&font_bytes) == "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        "status font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    for (i, (entry, japanese)) in tr
        .entries
        .iter()
        .zip(["ぼしゅう中…", "じゅんびOK!", "ダウンロード中…"])
        .enumerate()
    {
        ensure!(entry.japanese == japanese, "status source order changed");
        let row = slice(table, (9 + i) * 12, 12)?;
        let member = u16le(row, 0)?;
        let palette_id = u16le(row, 2)?;
        ensure!(
            member == [23, 21, 19][i]
                && u16le(row, 4)? == 1
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == 128
                && u16le(row, 10)? == 16,
            "status geometry changed"
        );
        let old = unpack_halfword(n.members[member])?;
        ensure!(old.len() == 2048, "status extent changed");
        // These payloads contain text only. Keep the transparent margin outside
        // the declared text rectangle, and regenerate alpha for the new glyphs.
        let rect = [2, 1, 74, 15];
        let inside = |p: usize| {
            (rect[0]..rect[2]).contains(&(p % 128)) && (rect[1]..rect[3]).contains(&(p / 128))
        };
        ensure!(
            old.iter().enumerate().all(|(p, v)| inside(p) || *v == 0),
            "status contains non-text outside rectangle"
        );
        let mut pixels = old.clone();
        for (p, v) in pixels.iter_mut().enumerate() {
            if inside(p) {
                *v = 0;
            }
        }
        let palette = n.members[palette_id];
        let ink = battle_ui::text_ink(&font, &entry.korean, 8, rect, 12)?;
        let white = 224 | buttons::nearest(palette, [31, 31, 31])? as u8;
        let dark = 224 | buttons::nearest(palette, [0, 0, 0])? as u8;
        battle_ui::paint(&mut pixels, 128, &ink, white, dark);
        ensure!(
            old.iter()
                .zip(&pixels)
                .enumerate()
                .all(|(p, (a, b))| inside(p) || a == b),
            "protected status pixel changed"
        );
        battle_ui::compress_member(&mut replacements, member, &pixels)?;
        records.push(json!({"texture":9+i,"member":member,"palette_member":palette_id,"japanese":japanese,"korean":entry.korean,"rect":rect,"source_sha256":sha(&old),"pixels_sha256":sha(&pixels),"stored_size":replacements[&member].len(),"capacity":n.members[member].len()}));
        previews.push((i, pixels, palette));
    }
    let result = battle_ui::rebuilt(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("status.narc"), &result)?;
    for (i, pixels, palette) in previews {
        buttons::png(&out.join(format!("{i}-korean.png")), &pixels, palette)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"status.narc","input_sha256":sha(&result)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&result),"layouts":layouts,"records":records,"runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("status.json"), &report)?;
    Ok(report)
}
