//! Replace the caption inside each panel while protecting its contour and body.
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
        "HUD archive changed"
    );
    let n = Narc::parse(source)?;
    let input = fs::read(translation)?;
    let tr: Label = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.japanese == "もんだい",
        "panel caption identity changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        // Galmuri9 at its native 10px grid for the 12px caption band.
        sha(&font_bytes) == "48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee",
        "font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut changes = BTreeMap::new();
    let mut previews = Vec::new();
    let mut entries = Vec::new();
    for (member, height, y0) in [(738, 58, 4), (804, 56, 5)] {
        let original = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[member + 1])?;
        ensure!(
            original.len() == 128 * height / 2 && palette.len() == 32,
            "panel extent changed"
        );
        let old = battle_ui::indices(&original);
        let mut pixels = old.clone();
        let mut allowed = vec![false; pixels.len()];
        // Reconstruct only the lettering band. Its two background colors are
        // sampled from the unobscured right side; contour colors stay immutable.
        for y in y0..y0 + 12 {
            for x in 5..48 {
                let i = y * 128 + x;
                if [1, 9, 10, 12, 13, 15].contains(&old[i]) {
                    allowed[i] = true;
                    pixels[i] = if y < y0 + 8 { 9 } else { 13 };
                }
            }
        }
        let ink = battle_ui::text_ink(&font, &tr.korean, 10, [5, y0, 48, y0 + 12], y0 + 10)?;
        battle_ui::paint(&mut pixels, 128, &ink, 15, 1);
        ensure!(
            old.iter()
                .zip(&pixels)
                .zip(&allowed)
                .all(|((a, b), ok)| a == b || *ok),
            "changed protected panel pixels"
        );
        let raw = battle_ui::pack(&pixels)?;
        ensure!(
            battle_ui::indices(&raw) == pixels,
            "panel I4 round trip failed"
        );
        battle_ui::compress_member(&mut changes, member, &raw)?;
        previews.push((
            member,
            height,
            titles::rgba(&old, &palette)?,
            titles::rgba(&pixels, &palette)?,
        ));
        entries.push(json!({"member":member,"palette_member":member+1,"width":128,"height":height,"caption_bounds":[5,y0,43,12],"editable_source_indices":[1,9,10,12,13,15],"background_indices":[9,13],"ink_index":15,"outline_index":1,"decoded_sha256":sha(&raw),"stored_size":changes[&member].len(),"capacity":n.members[member].len()}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("panels.narc"), &rebuilt)?;
    for (member, height, before, after) in previews {
        write_png(
            &out.join(format!("{member}-before.png")),
            128,
            height,
            &before,
        )?;
        write_png(
            &out.join(format!("{member}-after.png")),
            128,
            height,
            &after,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"panels.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"japanese":tr.japanese,"korean":tr.korean,"entries":entries,"renderer":renderer,"background_policy":"two-color lettering-band reconstruction inferred from unobscured background; not a recovered untitled source","protected":"caption exterior, contour colors, body, palettes, every other member and code","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("panels.json"), &report)?;
    Ok(report)
}
