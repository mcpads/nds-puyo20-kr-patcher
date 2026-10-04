//! Ready/Go use the common cutin call table, not the per-mode battle archive.
use super::*;
use crate::battle_ui;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    state: String,
    ready: String,
    go: String,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let renderer = super::renderer::verify(rom, None)?;
    let path = "puyo/cutin/cutin.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "95e29026ba7479465b61ad32fda24ff019358ebfd87f441ca0c625c370f4eba0",
        "call archive changed"
    );
    let table = rom.data(rom.file("puyo/cutin/call_texlist.bin")?);
    ensure!(
        sha(table) == "87eb53bf7da3214c3b2552ac617c8aaff93582b34b79890ec186c811d74331aa",
        "call texture mapping changed"
    );
    let n = Narc::parse(source)?;
    let layout = unpack(n.members[1116])?;
    ensure!(
        sha(&layout) == "83d2b59fdc7646ae7e673e14c4f024bf14aa132fa444f81221d41ab054c63df3",
        "call layout changed"
    );
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.ready == "준비…" && tr.go == "시작!",
        "call fragment mapping changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&font_bytes), "font changed");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut changes = BTreeMap::new();
    let mut previews = Vec::new();
    let mut entries = Vec::new();
    for (member, palette_member, width, height, edit_rows, size) in
        [(125, 124, 32, 97, 82, 22), (123, 122, 128, 64, 64, 36)]
    {
        let original = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[palette_member])?;
        ensure!(
            original.len() == width * height
                && palette.len() == if member == 125 { 36 } else { 50 },
            "call storage changed"
        );
        let mut colors = Vec::new();
        for i in 0..palette.len() / 2 {
            let rgb = crate::buttons::rgb(&palette, i)?;
            colors.push((
                i as u8,
                rgb.iter().sum::<i32>(),
                rgb.iter().map(|v| (31 - v).pow(2)).sum::<i32>(),
            ));
        }
        let dark = colors.iter().min_by_key(|c| c.1).unwrap().0;
        let white = colors.iter().min_by_key(|c| c.2).unwrap().0;
        ensure!(dark != white, "call contrast missing");
        let mut pixels = original.clone();
        pixels[..width * edit_rows].fill(0);
        if member == 125 {
            for (label, rect, baseline) in [("준", [0, 0, 32, 28], 24), ("비", [0, 28, 32, 56], 52)]
            {
                let ink = battle_ui::text_ink(&font, label, size, rect, baseline)?;
                battle_ui::paint(&mut pixels, width, &ink, 0xe0 | white, 0xe0 | dark);
            }
        } else {
            let ink = battle_ui::text_ink(&font, &tr.go, size, [4, 4, 124, 60], 49)?;
            for &(x, y) in &ink {
                ensure!(
                    x >= 7 && x + 7 < width && y >= 7 && y + 7 < height,
                    "Go outline exceeds crop"
                );
                for dy in -5i32..=5 {
                    for dx in -5i32..=5 {
                        pixels[(y as i32 + dy) as usize * width + (x as i32 + dx) as usize] =
                            0xe0 | white;
                    }
                }
            }
            for &(x, y) in &ink {
                for dy in -3i32..=3 {
                    for dx in -3i32..=3 {
                        pixels[(y as i32 + dy) as usize * width + (x as i32 + dx) as usize] =
                            0xe0 | dark;
                    }
                }
            }
            for (x, y) in ink {
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let row = (y as i32 + dy) as usize;
                        let gradient = [14, 16, 17, 18, 19, 21, 22, 23, 24];
                        let color = gradient[(row.saturating_sub(14) * 8 / 35).min(8)];
                        pixels[row * width + (x as i32 + dx) as usize] = 0xe0 | color;
                    }
                }
            }
        }
        ensure!(
            pixels[width * edit_rows..] == original[width * edit_rows..],
            "changed protected call punctuation or tail"
        );
        battle_ui::compress_member(&mut changes, member, &pixels)?;
        previews.push((
            member,
            width,
            height,
            super::results::rgba(&original, &palette)?,
            super::results::rgba(&pixels, &palette)?,
        ));
        entries.push(json!({"member":member,"palette_member":palette_member,"format":"A3I5","width":width,"stored_rows":height,"edited_rows":edit_rows,"font_size":size,"light_index":white,"outline_index":dark,"fill_indices":if member==125 {vec![white]} else {vec![14,16,17,18,19,21,22,23,24]},"stored_size":changes[&member].len(),"capacity":n.members[member].len(),"decoded_sha256":sha(&pixels)}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("start.narc"), &rebuilt)?;
    for (member, w, h, before, after) in previews {
        write_png(&out.join(format!("{member}-before.png")), w, h, &before)?;
        write_png(&out.join(format!("{member}-after.png")), w, h, &after)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"start.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"entries":entries,"renderer":renderer,"layout_member":1116,"layout_sha256":sha(&layout),"protected":"Ready dot crop and tail row, palettes, other members, texture table and animation layout/code","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("start.json"), &report)?;
    Ok(report)
}
