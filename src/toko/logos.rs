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
    mode: String,
    japanese: String,
    korean: Vec<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    image_sha256: Option<String>,
    #[serde(default)]
    palette_indices: Vec<usize>,
    #[serde(default)]
    image_pieces: Vec<crate::art_pixels::SheetPiece>,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let archive = "toko/toko.narc";
    let source = rom.data(rom.file(archive)?);
    ensure!(
        sha(source) == "562d9eda2d0eb07b1279a598513935982f6ffbe1c94bc7f3c9df4edb0637c68b",
        "toko logo source changed"
    );
    let n = Narc::parse(source)?;
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 6,
        "toko logo population changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "toko logo font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let modes = [
        ("chibi", 32, "とことんちびぷよ"),
        ("deka", 34, "とことんでかぷよラッシュ"),
        ("fev", 36, "とことんフィーバー"),
        ("nazo", 38, "とことんなぞぷよ"),
        ("prac", 39, "とことんれんしゅう"),
        ("puyo", 41, "とことんぷよぷよ"),
    ];
    let mut changes = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    for (label, (mode, gem, japanese)) in tr.entries.iter().zip(modes) {
        ensure!(
            label.mode == mode && label.japanese == japanese && label.korean.len() == 2,
            "toko logo identity/line count changed"
        );
        let ilf = format!("toko/tokoton_{mode}_top_ilf.bin");
        let (mut layouts, mapping) = titles::read_layout(rom, (mode, archive, &ilf, gem, 1))?;
        ensure!(layouts.len() == 1, "toko logo scene count changed");
        let title = layouts.remove(0);
        ensure!(
            title.name == format!("title_toko_{mode}"),
            "toko logo scene identity changed"
        );
        ensure!(
            label.image.is_some() == label.image_sha256.is_some(),
            "toko image/hash must be paired"
        );
        ensure!(
            label.image.is_some()
                || (label.palette_indices.is_empty() && label.image_pieces.is_empty()),
            "toko palette subset and pieces require an image"
        );
        previews.push((
            format!("{mode}-source"),
            title.width,
            title.height,
            titles::rgba(&title.pixels, &title.palette)?,
        ));
        let mut ink = Vec::new();
        if label.image.is_none() {
            for (line, text) in label.korean.iter().enumerate() {
                let glyphs = battle_ui::text_ink(
                    &font,
                    text,
                    12,
                    [0, 0, title.width / 2, 32],
                    14 + line * 12,
                )?;
                for (x, y) in glyphs {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let (px, py) = (x * 2 + dx, y * 2 + dy);
                            ensure!(
                                px >= 4 && px + 4 < title.width && py >= 3 && py + 4 < title.height,
                                "toko logo ink outside canvas"
                            );
                            ink.push((px, py));
                        }
                    }
                }
            }
        }
        let mut colors = Vec::new();
        for i in 1..16 {
            if !title.pixels.contains(&(i as u8)) {
                continue;
            }
            let c = crate::buttons::rgb(&title.palette, i)?;
            colors.push((
                i as u8,
                c.iter().map(|v| v * v).sum::<i32>(),
                c.iter().map(|v| (31 - v).pow(2)).sum::<i32>(),
            ));
        }
        let dark = colors
            .iter()
            .min_by_key(|c| c.1)
            .ok_or_else(|| anyhow::anyhow!("empty logo palette"))?
            .0;
        let white = colors.iter().min_by_key(|c| c.2).unwrap().0;
        ensure!(dark != white, "toko logo contrast missing");
        let mut pixels = vec![0u8; title.width * title.height];
        for &(x, y) in &ink {
            for dy in -2i32..=3 {
                for dx in -2i32..=2 {
                    if dx * dx + (dy - 1) * (dy - 1) <= 5 {
                        pixels[(y as i32 + dy) as usize * title.width + (x as i32 + dx) as usize] =
                            dark;
                    }
                }
            }
        }
        for (x, y) in ink {
            pixels[y * title.width + x] = white;
        }
        if let Some(path) = &label.image {
            let bytes = fs::read(path)?;
            ensure!(
                Some(sha(&bytes)) == label.image_sha256,
                "generated toko logo image changed"
            );
            let image = crate::art_pixels::read(&bytes)?;
            let rgba = if label.image_pieces.is_empty() {
                crate::art_pixels::reduce(&image, title.width, title.height, true)?
            } else {
                crate::art_pixels::assemble(&image, title.width, title.height, &label.image_pieces)?
            };
            ensure!(
                label.palette_indices.iter().all(|&i| i > 0 && i < 16),
                "toko logo palette subset outside palette"
            );
            let candidates = if label.palette_indices.is_empty() {
                (1..16).collect::<Vec<_>>()
            } else {
                label.palette_indices.clone()
            };
            for (pixel, color) in pixels.iter_mut().zip(rgba.chunks_exact(4)) {
                *pixel = if color[3] < 128 {
                    0
                } else {
                    candidates
                        .iter()
                        .copied()
                        .min_by_key(|&i| {
                            let c = u16le(&title.palette, i * 2).unwrap();
                            (0..3)
                                .map(|k| {
                                    let d = ((c >> (k * 5)) & 31) as i32 * 255 / 31
                                        - i32::from(color[k]);
                                    d * d
                                })
                                .sum::<i32>()
                        })
                        .unwrap() as u8
                };
            }
        }
        let mut covered = vec![false; pixels.len()];
        let mut members = Vec::new();
        for part in &title.parts {
            for y in part.y..part.y + part.height {
                for x in part.x..part.x + part.width {
                    covered[y * title.width + x] = true;
                }
            }
            let p = battle_ui::crop(
                &pixels,
                title.width,
                part.x,
                part.y,
                part.width,
                part.height,
            );
            let raw = titles::tile(&p, part.width, part.height, 4)?;
            ensure!(
                titles::untile(&raw, part.width, part.height, 4)? == p,
                "toko logo tile round trip"
            );
            battle_ui::compress_member(&mut changes, part.member, &raw)?;
            if changes[&part.member].len() > n.members[part.member].len() && raw.len() <= 4096 {
                let packed = crate::compress::pack_compact(&raw)?;
                ensure!(
                    unpack_halfword(&packed)? == raw,
                    "compact toko logo roundtrip"
                );
                changes.insert(part.member, packed);
            }
            members.push(json!({"member":part.member,"x":part.x,"y":part.y,"width":part.width,"height":part.height,"decoded_sha256":sha(&raw),"stored_size":changes[&part.member].len(),"capacity":n.members[part.member].len()}));
        }
        ensure!(
            pixels.iter().zip(&covered).all(|(&p, &c)| p == 0 || c),
            "toko logo ink outside resource coverage"
        );
        previews.push((
            mode.to_string(),
            title.width,
            title.height,
            titles::rgba(&pixels, &title.palette)?,
        ));
        records.push(json!({"mode":mode,"japanese":japanese,"korean":label.korean,"mapping":mapping,"members":members,"ink_index":white,"outline_index":dark,"image":label.image,"image_sha256":label.image_sha256,"palette_indices":label.palette_indices,"image_pieces":label.image_pieces}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("toko.narc"), &rebuilt)?;
    for (mode, w, h, p) in previews {
        write_png(&out.join(format!("{mode}.png")), w, h, &p)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":archive,"expected_sha256":sha(source),"input":"toko.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&rebuilt),"logos":records,"protected":"Gem/ILF, palette, statistics, other members; entire logo glyph artwork editable","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("logos.json"), &report)?;
    Ok(report)
}
