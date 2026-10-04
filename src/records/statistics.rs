use super::ARCHIVE;
use crate::{archive, assets::json_file, compress, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    source_pixels_sha256: String,
    font_sha256: String,
    entries: Vec<Caption>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Caption {
    crop_id: usize,
    source: String,
    source_crop_sha256: String,
    korean: String,
}
const SOURCES: [&str; 8] = [
    "スコア",
    "プレイ時間",
    "最大連鎖数",
    "かちぬき回数",
    "クリア問題数",
    "ぷよ消した数",
    "タネ消化数",
    "レベル",
];
const WIDTHS: [usize; 8] = [39, 40, 46, 50, 48, 49, 41, 24];

fn caption_pixels(font: &fontdue::Font, text: &str, width: usize) -> Result<Vec<u8>> {
    ensure!(!text.trim().is_empty(), "empty record caption");
    let mut points = Vec::new();
    let mut cursor = 0;
    for c in text.chars() {
        ensure!(font.lookup_glyph_index(c) != 0, "missing record glyph: {c}");
        let (m, bitmap) = font.rasterize(c, 8.0);
        let ink_start = points.len();
        for y in 0..m.height {
            for x in 0..m.width {
                if bitmap[y * m.width + x] < 128 {
                    continue;
                }
                let px = cursor + m.xmin + x as i32;
                let py = 7 - m.ymin - m.height as i32 + y as i32;
                ensure!(
                    px >= 0 && px < width as i32 && (0..9).contains(&py),
                    "record caption ink exceeds crop: {text} at {px},{py}"
                );
                points.push((px as usize, py as usize));
            }
        }
        ensure!(
            c == ' ' || points.len() > ink_start,
            "empty record glyph: {c}"
        );
        cursor += m.advance_width.round() as i32;
    }
    ensure!(
        cursor <= width as i32 && !points.is_empty(),
        "record caption advance exceeds crop: {text}"
    );
    let mut pixels = vec![0; width * 9];
    for &(x, y) in &points {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (px, py) = (x as i32 + dx, y as i32 + dy);
            if px >= 0 && px < width as i32 && (0..9).contains(&py) {
                pixels[py as usize * width + px as usize] = 248;
            }
        }
    }
    for (x, y) in points {
        pixels[y * width + x] = 249;
    }
    Ok(pixels)
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(
        sha(source) == "27417db6a9a533ad988e9a37c6a0f87149339510da64a034a18e34535ecaa776",
        "record source changed"
    );
    let n = Narc::parse(source)?;
    let original = unpack_halfword(n.members[196])?;
    let palette = unpack(n.members[195])?;
    ensure!(
        original.len() == 64 * 128
            && sha(&palette) == "8febadbce87cbeb6f85056a925c6a24a9afc4b49de30b8b2f2f076ecaee773f9",
        "record statistics format changed"
    );
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.entries.len() == 8
            && sha(&original) == tr.source_pixels_sha256,
        "record statistics input mismatch"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        sha(&font_bytes) == tr.font_sha256
            && tr.font_sha256 == "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut changed = original.clone();
    let mut editable = vec![false; original.len()];
    let mut records = Vec::new();
    for (i, e) in tr.entries.iter().enumerate() {
        ensure!(
            e.crop_id == i + 2 && e.source == SOURCES[i],
            "record source caption/order changed"
        );
        let width = WIDTHS[i];
        let y0 = 1 + i * 10;
        let crop: Vec<u8> = (y0..y0 + 9)
            .flat_map(|y| original[y * 64 + 1..y * 64 + 1 + width].iter().copied())
            .collect();
        ensure!(
            sha(&crop) == e.source_crop_sha256,
            "record source crop changed"
        );
        for member in [12, 17, 19] {
            let dsif = unpack(n.members[member])?;
            let offset = 32 + u32le(&dsif, 80)? + (i + 2) * 20;
            ensure!(
                u32le(&dsif, offset)? == 1,
                "record statistics texture changed"
            );
            for (axis, expected) in [1, y0, 1 + width, y0 + 9].into_iter().enumerate() {
                let scale = if axis % 2 == 0 { 64 } else { 128 };
                ensure!(
                    u32le(&dsif, offset + 4 + axis * 4)? * scale == expected * 4096,
                    "record statistics shared crop changed"
                );
            }
        }
        let pixels = caption_pixels(&font, &e.korean, width)?;
        for y in 0..9 {
            let start = (y0 + y) * 64 + 1;
            ensure!(
                editable[start..start + width].iter().all(|v| !*v),
                "overlapping caption writes"
            );
            editable[start..start + width].fill(true);
            changed[start..start + width].copy_from_slice(&pixels[y * width..(y + 1) * width]);
        }
        records.push(json!({"crop_id":e.crop_id,"source":e.source,"korean":e.korean,"rect":[1,y0,1+width,y0+9],"source_crop_sha256":sha(&crop),"pixels_sha256":sha(&pixels)}));
    }
    ensure!(
        original
            .iter()
            .zip(&changed)
            .zip(&editable)
            .all(|((&a, &b), &edit)| edit || a == b),
        "protected statistics pixels changed"
    );
    let packed = compress::pack(&changed)?;
    ensure!(
        unpack_halfword(&packed)? == changed,
        "statistics compression round trip failed"
    );
    let rebuilt = archive::replace(source, &BTreeMap::from([(196, packed.clone())]))?;
    let mut rgba = Vec::new();
    for &pixel in &changed {
        let c = u16le(&palette, (pixel & 7) as usize * 2)?;
        rgba.extend([
            ((c & 31) * 255 / 31) as u8,
            (((c >> 5) & 31) * 255 / 31) as u8,
            (((c >> 10) & 31) * 255 / 31) as u8,
            ((pixel >> 3) as usize * 255 / 31) as u8,
        ]);
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("record.narc"), &rebuilt)?;
    fs::write(out.join("statistics.bin"), &changed)?;
    write_png(&out.join("statistics.png"), 64, 128, &rgba)?;
    let mut enlarged = Vec::new();
    for y in 0..512 {
        for x in 0..256 {
            let p = (y / 4 * 64 + x / 4) * 4;
            enlarged.extend_from_slice(&rgba[p..p + 4]);
        }
    }
    write_png(&out.join("statistics-4x.png"), 256, 512, &enlarged)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"record.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"state":tr.state,"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"font_sha256":tr.font_sha256,"rasterizer":"Galmuri7 native 8px, fontdue 0.9.4, baseline 7, threshold 128; matched upstream BDF bitmaps, four-neighbor outline clipped to crop; ink never clipped","member":196,"palette_member":195,"palette_sha256":sha(&palette),"pixels_sha256":sha(&changed),"stored_size":packed.len(),"capacity":n.members[196].len(),"records":records,"protected_pixels":editable.iter().filter(|v|!**v).count(),"archive_sha256":sha(&rebuilt),"runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("statistics.json"), &report)?;
    Ok(report)
}
