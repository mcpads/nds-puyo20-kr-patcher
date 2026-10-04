//! Preserve the English halves, palettes and animation; replace Japanese crops.
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

pub(super) fn rgba(bytes: &[u8], palette: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len() * 4);
    for &b in bytes {
        let color = u16le(palette, (b & 31) as usize * 2)?;
        out.extend([
            (color & 31) as u8 * 8,
            ((color >> 5) & 31) as u8 * 8,
            ((color >> 10) & 31) as u8 * 8,
            (u16::from(b >> 5) * 255 / 7) as u8,
        ]);
    }
    Ok(out)
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
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 2,
        "expected two result labels"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&font_bytes), "font changed");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut changes = BTreeMap::new();
    let mut previews = Vec::new();
    let mut entries = Vec::new();
    for (label, (member, height, crop_height, japanese)) in tr
        .entries
        .iter()
        .zip([(640, 96, 54, "ばたんきゅ～"), (824, 88, 42, "やった!")])
    {
        ensure!(label.japanese == japanese, "result label identity changed");
        let original = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[member + 1])?;
        ensure!(
            original.len() == 64 * height && palette.len() == 64,
            "result A3I5 extent changed"
        );
        let mut colors = Vec::new();
        for i in 0..32 {
            if original[..64 * crop_height]
                .iter()
                .any(|b| b >> 5 > 0 && usize::from(b & 31) == i)
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
            .ok_or_else(|| anyhow::anyhow!("empty result colors"))?
            .0;
        let white = colors.iter().min_by_key(|c| c.2).unwrap().0;
        ensure!(dark != white, "result contrast missing");
        let ink = battle_ui::text_ink(
            &font,
            &label.korean,
            16,
            [0, 0, 64, crop_height],
            crop_height / 2 + 6,
        )?;
        let mut pixels = original.clone();
        pixels[..64 * crop_height].fill(0);
        for &(x, y) in &ink {
            ensure!(
                x >= 2 && x + 2 < 64 && y >= 2 && y + 3 < crop_height,
                "result lettering margin exceeded"
            );
            for dy in -1i32..=2 {
                for dx in -1i32..=1 {
                    pixels[(y as i32 + dy) as usize * 64 + (x as i32 + dx) as usize] = 0xe0 | dark;
                }
            }
        }
        for (x, y) in ink {
            pixels[y * 64 + x] = 0xe0 | white;
        }
        ensure!(
            pixels[64 * crop_height..] == original[64 * crop_height..],
            "changed protected English result artwork"
        );
        battle_ui::compress_member(&mut changes, member, &pixels)?;
        previews.push((
            member,
            height,
            rgba(&original, &palette)?,
            rgba(&pixels, &palette)?,
        ));
        entries.push(json!({"member":member,"palette_member":member+1,"format":"A3I5","width":64,"height":height,"crop":[0,0,64,crop_height],"japanese":japanese,"korean":label.korean,"ink_index":white,"outline_index":dark,"decoded_sha256":sha(&pixels),"protected_tail_sha256":sha(&original[64*crop_height..]),"stored_size":changes[&member].len(),"capacity":n.members[member].len()}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("results.narc"), &rebuilt)?;
    for (member, height, before, after) in previews {
        write_png(
            &out.join(format!("{member}-before.png")),
            64,
            height,
            &before,
        )?;
        write_png(&out.join(format!("{member}-after.png")), 64, height, &after)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"results.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"entries":entries,"renderer":renderer,"protected":"English lower halves, palettes, every other member, crop and animation code","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("results.json"), &report)?;
    Ok(report)
}
