//! Four animated crops share one A3I5 texture; keep the punctuation crop intact.
use super::*;
use crate::battle_ui;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    state: String,
    japanese: String,
    korean: String,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let renderer = super::renderer::verify(rom, None)?;
    let path = "puyo/puyo2P/puyo2P.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b338518e1710dee5f6546deabeb3aa367bdaff2935146c4807e8b9d57c54c510",
        "battle archive changed"
    );
    let n = Narc::parse(source)?;
    let original = unpack_halfword(n.members[802])?;
    let palette = unpack(n.members[803])?;
    ensure!(
        original.len() == 64 * 32 && palette.len() == 64,
        "reach geometry changed"
    );
    let input = fs::read(translation)?;
    let tr: Label = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.japanese == "リーチ!" && tr.korean == "리치!",
        "reach fragment mapping changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&font_bytes), "font changed");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut pixels = original.clone();
    let mut allowed = vec![false; pixels.len()];
    for [x, y, w, h] in [[0, 0, 20, 32], [48, 0, 12, 10], [20, 0, 22, 32]] {
        for row in y..y + h {
            for col in x..x + w {
                allowed[row * 64 + col] = true;
                pixels[row * 64 + col] = 0;
            }
        }
    }
    let mut colors = Vec::new();
    for i in 0..32 {
        if original
            .iter()
            .zip(&allowed)
            .any(|(b, a)| *a && b >> 5 > 0 && usize::from(b & 31) == i)
        {
            let rgb = crate::buttons::rgb(&palette, i)?;
            colors.push((
                i as u8,
                rgb.iter().map(|v| v * v).sum::<i32>(),
                rgb.iter().map(|v| (31 - v).pow(2)).sum::<i32>(),
            ));
        }
    }
    let dark = colors
        .iter()
        .min_by_key(|c| c.1)
        .ok_or_else(|| anyhow::anyhow!("empty reach colors"))?
        .0;
    let white = colors.iter().min_by_key(|c| c.2).unwrap().0;
    ensure!(dark != white, "reach contrast missing");
    for (text, bounds) in [("리", [0, 0, 20, 32]), ("치", [20, 0, 42, 32])] {
        let ink = battle_ui::text_ink(&font, text, 18, bounds, 25)?;
        battle_ui::paint(&mut pixels, 64, &ink, 0xe0 | white, 0xe0 | dark);
    }
    ensure!(
        pixels
            .iter()
            .zip(&original)
            .zip(&allowed)
            .all(|((a, b), ok)| *ok || a == b),
        "changed protected reach punctuation or padding"
    );
    let mut changes = BTreeMap::new();
    battle_ui::compress_member(&mut changes, 802, &pixels)?;
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("reach.narc"), &rebuilt)?;
    for (name, bytes) in [("before", &original), ("after", &pixels)] {
        write_png(
            &out.join(format!("{name}.png")),
            64,
            32,
            &super::results::rgba(bytes, &palette)?,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"reach.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"member":802,"palette_member":803,"format":"A3I5","width":64,"height":32,"crops":[[0,0,20,32],[48,0,12,10],[20,0,22,32],[48,10,16,22]],"letters":["리","","치","!"],"ink_index":white,"outline_index":dark,"stored_size":changes[&802].len(),"capacity":n.members[802].len(),"decoded_sha256":sha(&pixels),"renderer":renderer,"protected":"exclamation, padding, palette, other members and animation code","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("reach.json"), &report)?;
    Ok(report)
}
