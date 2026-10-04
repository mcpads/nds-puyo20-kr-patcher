use super::*;
use crate::battle_ui;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
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
    let renderer = super::renderer::verify(rom, None)?;
    let path = "puyo/puyo2P/puyo2P.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b338518e1710dee5f6546deabeb3aa367bdaff2935146c4807e8b9d57c54c510",
        "HUD archive changed"
    );
    let n = Narc::parse(source)?;
    let original = unpack_halfword(n.members[736])?;
    let palette = unpack(n.members[737])?;
    ensure!(
        original.len() == 2560 && palette.len() == 32,
        "HUD geometry changed"
    );
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 4,
        "expected four HUD labels"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        // DenkiChip at its native 12px grid: Galmuri11's height in a 10px advance, so 연쇄! fits 26px.
        sha(&font_bytes) == "4589cb1a59bcbd669ad7ac0669827e5a4d411832048e9bdd9618c907c1a8d272",
        "font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let old = battle_ui::indices(&original);
    let mut pixels = old.clone();
    let mut allowed = vec![false; pixels.len()];
    let mut entries = Vec::new();
    for (label, (japanese, x)) in
        tr.entries
            .iter()
            .zip([("れんさ", 0), ("ぷよ", 28), ("いろ", 55), ("あと", 81)])
    {
        ensure!(label.japanese == japanese, "HUD label identity changed");
        for y in 22..40 {
            for col in x..x + 26 {
                pixels[y * 128 + col] = 0;
                allowed[y * 128 + col] = true;
            }
        }
        let ink = battle_ui::text_ink(&font, &label.korean, 12, [x, 22, x + 26, 40], 36)?;
        battle_ui::paint(&mut pixels, 128, &ink, 11, 2);
        entries.push(json!({"japanese":japanese,"korean":label.korean,"crop":[x,22,26,18]}));
    }
    ensure!(
        old.iter()
            .zip(&pixels)
            .zip(&allowed)
            .all(|((a, b), ok)| a == b || *ok),
        "changed protected HUD pixels"
    );
    let raw = battle_ui::pack(&pixels)?;
    ensure!(
        battle_ui::indices(&raw) == pixels,
        "HUD I4 round trip failed"
    );
    let mut changes = BTreeMap::new();
    battle_ui::compress_member(&mut changes, 736, &raw)?;
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("labels.narc"), &rebuilt)?;
    write_png(
        &out.join("before.png"),
        128,
        40,
        &titles::rgba(&old, &palette)?,
    )?;
    write_png(
        &out.join("after.png"),
        128,
        40,
        &titles::rgba(&pixels, &palette)?,
    )?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"labels.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"member":736,"palette_member":737,"decoded_sha256":sha(&raw),"stored_size":changes[&736].len(),"capacity":n.members[736].len(),"entries":entries,"renderer":renderer,"protected":"all pixels outside four label crops, palette and other archive members","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("labels.json"), &report)?;
    Ok(report)
}
