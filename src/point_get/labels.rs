use super::{ARCHIVE, SOURCE};
use crate::{assets::json_file, battle_ui, format::*, graphics::write_png, titles};
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

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(sha(source) == SOURCE, "point label source changed");
    let n = Narc::parse(source)?;
    let ilf = rom.data(rom.file("puyo/result/point_get_top_s_ilf.bin")?);
    ensure!(
        sha(ilf) == "f7212a85b1e8b83f049757f320f15f11c7388d71b9b2e5aa335c77df46e4479f",
        "point label ILF changed"
    );
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 2,
        "point label population changed"
    );
    ensure!(
        tr.entries[0].japanese == "合計ポイント" && tr.entries[1].japanese == "現在",
        "point label source text changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "point label font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let rects = [[25, 4, 107, 19], [26, 30, 106, 47]];
    let inks = tr
        .entries
        .iter()
        .zip(rects)
        .zip([16, 44])
        .map(|((e, r), b)| battle_ui::text_ink(&font, &e.korean, 12, r, b))
        .collect::<Result<Vec<_>>>()?;
    let mut changes = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    for (name, members, palette_id, tiled, width, height) in [
        ("linear", vec![73], 72, false, 128, 55),
        ("obj", vec![71, 74], 131, true, 127, 64),
    ] {
        let palette = unpack(n.members[palette_id])?;
        let mut original = vec![0u8; width * height];
        let mut parts = Vec::new();
        for (part, &member) in members.iter().enumerate() {
            let w = if tiled { 64 } else { 128 };
            let x0 = part * 63;
            let raw = unpack_halfword(n.members[member])?;
            ensure!(raw.len() == w * height / 2, "point label geometry changed");
            let p = if tiled {
                titles::untile(&raw, w, height, 4)?
            } else {
                battle_ui::indices(&raw)
            };
            for y in 0..height {
                original[y * width + x0..y * width + x0 + w].copy_from_slice(&p[y * w..y * w + w]);
            }
            parts.push((member, w, x0, raw, p));
        }
        let colors = if tiled {
            [[3, 14, 8], [14, 6, 13]]
        } else {
            // The current-value caption rims its brown strokes (2) with the
            // light olive edge colour (8) of the original 現在; the darker
            // brown 7 merged with the strokes into one blob.
            [[10, 5, 12], [5, 2, 8]]
        };
        let mut pixels = original.clone();
        for (i, [x0, y0, x1, y1]) in rects.into_iter().enumerate() {
            // Source column 25/30 lies outside the original glyphs and demonstrates
            // a uniform green/yellow interior beneath both caption rectangles.
            let sample = if i == 0 { 25 } else { 30 };
            ensure!(
                (y0..y1).all(|y| original[y * width + sample] == colors[i][0]),
                "point label background changed"
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    pixels[y * width + x] = colors[i][0];
                }
            }
            battle_ui::paint(&mut pixels, width, &inks[i], colors[i][1], colors[i][2]);
        }
        let editable = |x: usize, y: usize| {
            rects
                .iter()
                .any(|r| (r[0]..r[2]).contains(&x) && (r[1]..r[3]).contains(&y))
        };
        ensure!(
            original
                .iter()
                .zip(&pixels)
                .enumerate()
                .all(|(p, (a, b))| editable(p % width, p / width) || a == b),
            "protected point label pixel changed"
        );
        for (member, w, x0, raw, before) in parts {
            let mut after = before.clone();
            for y in 0..height {
                for x in 0..w {
                    if editable(x0 + x, y) {
                        after[y * w + x] = pixels[y * width + x0 + x];
                    } else {
                        ensure!(
                            after[y * w + x] == before[y * w + x],
                            "protected OBJ copy changed"
                        );
                    }
                }
            }
            let bytes = if tiled {
                titles::tile(&after, w, height, 4)?
            } else {
                battle_ui::pack(&after)?
            };
            battle_ui::compress_member(&mut changes, member, &bytes)?;
            records.push(json!({"member":member,"palette_member":palette_id,"source_sha256":sha(&raw),"decoded_sha256":sha(&bytes),"stored_size":changes[&member].len(),"capacity":n.members[member].len(),"protected_pixels":(0..w*height).filter(|p|!editable(x0+p%w,p/w)).count()}));
        }
        previews.push((name, width, height, titles::rgba(&pixels, &palette)?));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("result.narc"), &rebuilt)?;
    for (name, w, h, rgba) in previews {
        write_png(&out.join(format!("{name}.png")), w, h, &rgba)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":SOURCE,"input":"result.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&rebuilt),"records":records,"rects":rects,"font_size":12,"protected":"caption exterior, frame, palettes, extra rows, title and numeric/Pts members","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("labels.json"), &report)?;
    Ok(report)
}
