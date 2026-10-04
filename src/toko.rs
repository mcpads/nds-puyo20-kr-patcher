//! Shared endless-mode statistics sheet. Runtime selection is separate evidence.
mod logos;
use crate::{assets::json_file, battle_ui, format::*, graphics::write_png, titles};
use anyhow::{Result, ensure};
pub use logos::prepare as prepare_logos;
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

pub fn prepare_statistics(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let archive = "toko/toko.narc";
    let source = rom.data(rom.file(archive)?);
    ensure!(
        sha(source) == "562d9eda2d0eb07b1279a598513935982f6ffbe1c94bc7f3c9df4edb0637c68b",
        "toko source changed"
    );
    let n = Narc::parse(source)?;
    ensure!(n.members.len() == 43, "toko archive population changed");
    let mut tables = Vec::new();
    for name in ["deka", "fev", "nazo", "puyo", "taisen"] {
        let path = format!("toko/tokoton_{name}_b_texlist.bin");
        let b = rom.data(rom.file(&path)?);
        ensure!(
            sha(b) == "e9f60e0ce0ebd0b7152a99bf7e9ceb677ac513eb0aafa561893892578cbb99bb",
            "toko shared texture list changed"
        );
        tables.push(path);
    }
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    let japanese = [
        "レベル",
        "ぷよ消した数",
        "タネ消化数",
        "クリア問題数",
        "全消し回数",
        "最大連鎖数",
    ];
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 6,
        "toko statistics population changed"
    );
    ensure!(
        tr.entries
            .iter()
            .zip(japanese)
            .all(|(e, j)| e.japanese == j),
        "toko statistics source order changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        sha(&font_bytes) == "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        "toko statistics font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let raw = unpack_halfword(n.members[5])?;
    ensure!(
        raw.len() == 128 * 133 / 2
            && sha(&raw) == "8b26e72b7d1ad53912a51d46dd8a19e9bfbb8976e4a5448606f92d470f9d5c2f",
        "toko statistics sheet changed"
    );
    let original = battle_ui::indices(&raw);
    let mut pixels = original.clone();
    let mut editable = vec![false; pixels.len()];
    let background: Vec<_> = (6..15).map(|y| original[y * 128 + 32]).collect();
    ensure!(
        background == [13, 13, 13, 13, 13, 13, 13, 13, 11],
        "toko caption background changed"
    );
    let mut records = Vec::new();
    for (i, (e, x1)) in tr.entries.iter().zip([32, 59, 49, 57, 51, 53]).enumerate() {
        let rect = [5, i * 22 + 6, x1, i * 22 + 15];
        let centred = battle_ui::text_ink(&font, &e.korean, 8, rect, i * 22 + 14)?;
        // Left-align with the Japanese caption's first ink column, as in the source.
        let source_left = (rect[1]..rect[3])
            .flat_map(|y| (rect[0]..rect[2]).map(move |x| (x, y)))
            .filter(|&(x, y)| original[y * 128 + x] == 14)
            .map(|(x, _)| x)
            .min()
            .ok_or_else(|| anyhow::anyhow!("toko source caption has no ink"))?;
        let ink_left = centred.iter().map(|p| p.0).min().unwrap_or(source_left);
        ensure!(ink_left >= 1, "toko caption ink at crop edge");
        let ink: Vec<(usize, usize)> = centred
            .iter()
            .map(|&(x, y)| (x + source_left - ink_left, y))
            .collect();
        ensure!(
            ink.iter().all(|&(x, _)| x > rect[0] && x + 1 < rect[2]),
            "toko caption leaves its crop after alignment"
        );
        for y in rect[1]..rect[3] {
            for x in rect[0]..rect[2] {
                let p = y * 128 + x;
                ensure!(
                    !editable[p] && original[p] != 0,
                    "toko caption outside opaque panel or overlap"
                );
                pixels[p] = background[y - rect[1]];
                editable[p] = true;
            }
        }
        battle_ui::paint(&mut pixels, 128, &ink, 14, 1);
        records.push(json!({"row":i,"japanese":e.japanese,"korean":e.korean,"rect":rect}));
    }
    ensure!(
        original
            .iter()
            .zip(&pixels)
            .zip(&editable)
            .all(|((a, b), edit)| *edit || a == b),
        "protected toko pixel changed"
    );
    let changed = battle_ui::pack(&pixels)?;
    let mut replacements = BTreeMap::new();
    battle_ui::compress_member(&mut replacements, 5, &changed)?;
    let result = battle_ui::rebuilt(source, &replacements)?;
    let palette = unpack(n.members[4])?;
    fs::create_dir_all(out)?;
    fs::write(out.join("toko.narc"), &result)?;
    write_png(
        &out.join("statistics.png"),
        128,
        133,
        &titles::rgba(&pixels, &palette)?,
    )?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":archive,"expected_sha256":sha(source),"input":"toko.narc","input_sha256":sha(&result)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&result),"member":5,"decoded_sha256":sha(&changed),"stored_size":replacements[&5].len(),"capacity":n.members[5].len(),"tables":tables,"labels":records,"font_size":8,"protected_pixels":editable.iter().filter(|v|!**v).count(),"protected":"panel border, numeric area, palette, spare row, Total and all other members","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("statistics.json"), &report)?;
    Ok(report)
}
