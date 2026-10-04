//! Academy menu lettering is an A5I3 mask; colored frames are separate assets.
pub mod battle;
pub mod battle_labels;
pub mod captions;
pub mod lists;
pub mod logos;
pub mod scripts;
pub mod symbols;

use crate::{assets::json_file, battle_ui, format::*, graphics::write_png};
use anyhow::{Result, ensure};
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

pub fn prepare_buttons(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = "menu/academy_menu.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "5b1654e879326ddf36db3be38b7ff909c2192d4cb2b15e228b5bdef9365885b6",
        "academy archive changed"
    );
    let n = Narc::parse(source)?;
    let table = rom.data(rom.file("menu/lesson_menu_b_texlist.bin")?);
    ensure!(
        sha(table) == "82dff02de55e83cde15586be62d9128b6b2467818c588c130ea3a043423c9fae",
        "academy texture table changed"
    );
    ensure!(
        u16le(table, 0)? == 5
            && u16le(table, 2)? == 4
            && u16le(table, 4)? == 6
            && u16le(table, 8)? == 128
            && u16le(table, 10)? == 64,
        "academy text geometry changed"
    );
    let mut layouts = Vec::new();
    for member in 0..4 {
        let b = unpack(n.members[member])?;
        ensure!(
            slice(&b, 0, 4)? == b"DSIF"
                && u32le(&b, 12)? == 32
                && slice(&b, 32, 4)? == b"nCSC"
                && u32le(&b, 76)? == 16,
            "academy layout changed"
        );
        let offset = 32 + u32le(&b, 80)?;
        for row in 0..4 {
            let p = offset + row * 20;
            ensure!(u32le(&b, p)? == 0, "academy caption texture changed");
            for (i, v) in [352, row * 1024, 3744, (row + 1) * 1024].iter().enumerate() {
                ensure!(
                    u32le(&b, p + 4 + i * 4)? == *v,
                    "academy caption crop changed"
                );
            }
        }
        layouts.push(json!({"member":member,"sha256":sha(&b)}));
    }
    let old = unpack_halfword(n.members[5])?;
    ensure!(
        old.len() == 8192 && old.iter().all(|v| v & 7 == 0) && n.members[4] == [255, 127],
        "academy mask/palette changed"
    );
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 4,
        "academy label population changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "academy font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut pixels = old.clone();
    let mut records = Vec::new();
    for (row, (entry, japanese)) in tr
        .entries
        .iter()
        .zip(["にゅうもん", "じっせん", "チャレンジ", "れんしゅうノート"])
        .enumerate()
    {
        ensure!(entry.japanese == japanese, "academy source order changed");
        let rect = [11, row * 16, 117, (row + 1) * 16];
        for y in rect[1]..rect[3] {
            for x in 0..128 {
                if (11..117).contains(&x) {
                    pixels[y * 128 + x] = 0;
                } else {
                    ensure!(old[y * 128 + x] == 0, "academy non-text margin changed");
                }
            }
        }
        let ink = battle_ui::text_ink(&font, &entry.korean, 12, rect, row * 16 + 14)?;
        for (x, y) in ink {
            pixels[y * 128 + x] = 248;
        }
        records.push(json!({"row":row,"rect":rect,"japanese":japanese,"korean":entry.korean}));
    }
    let mut replacements = BTreeMap::new();
    battle_ui::compress_member(&mut replacements, 5, &pixels)?;
    let rebuilt = battle_ui::rebuilt(source, &replacements)?;
    let rgba: Vec<_> = pixels
        .iter()
        .flat_map(|v| [255, 255, 255, ((*v >> 3) as u16 * 255 / 31) as u8])
        .collect();
    let preview: Vec<_> = pixels
        .iter()
        .flat_map(|v| {
            let c = 20 + ((*v >> 3) as u16 * 235 / 31) as u8;
            [c, c, c, 255]
        })
        .collect();
    fs::create_dir_all(out)?;
    fs::write(out.join("buttons.narc"), &rebuilt)?;
    write_png(&out.join("text-alpha.png"), 128, 64, &rgba)?;
    write_png(&out.join("text-on-dark.png"), 128, 64, &preview)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"buttons.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"layouts":layouts,"records":records,"stored_size":replacements[&5].len(),"capacity":n.members[5].len(),"pixels_sha256":sha(&pixels),"archive_sha256":sha(&rebuilt),"runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("buttons.json"), &report)?;
    Ok(report)
}
