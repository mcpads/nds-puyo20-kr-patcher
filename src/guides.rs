use crate::{assets::json_file, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const ARCHIVE: &str = "menu/guide_icon.narc";
const TABLE: &str = "menu/guide_icon_texlist.bin";
struct Texture {
    member: usize,
    palette: Vec<u8>,
    width: usize,
    height: usize,
    raw: Vec<u8>,
    pixels: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct Crop {
    texture: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}
impl Crop {
    fn contains(self, x: usize, y: usize) -> bool {
        (self.x0..self.x1).contains(&x) && (self.y0..self.y1).contains(&y)
    }
    fn pixels(self, t: &Texture) -> Vec<u8> {
        (self.y0..self.y1)
            .flat_map(|y| {
                t.pixels[y * t.width + self.x0..y * t.width + self.x1]
                    .iter()
                    .copied()
            })
            .collect()
    }
}

fn read(rom: &Rom) -> Result<(Vec<Texture>, Vec<Crop>, Value)> {
    let source = rom.data(rom.file(ARCHIVE)?);
    let narc = Narc::parse(source)?;
    let table = rom.data(rom.file(TABLE)?);
    ensure!(
        narc.members.len() == 14 && table.len() == 72,
        "guide population changed"
    );
    ensure!(
        slice(table, 60, 12)? == [0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        "guide sentinel changed"
    );
    let mut textures = Vec::new();
    let mut records = Vec::new();
    for (id, (member, pal, width, height)) in [
        (13, 12, 64, 64),
        (9, 8, 64, 64),
        (7, 6, 32, 32),
        (5, 4, 32, 32),
        (11, 10, 32, 32),
    ]
    .into_iter()
    .enumerate()
    {
        let row = slice(table, id * 12, 12)?;
        ensure!(
            u16le(row, 0)? == member
                && u16le(row, 2)? == pal
                && u16le(row, 4)? == 3
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == width
                && u16le(row, 10)? == height,
            "guide texture mapping changed"
        );
        let raw = unpack_halfword(narc.members[member])?;
        let palette = unpack(narc.members[pal])?;
        ensure!(
            raw.len() == width * height / 2 && palette.len() == 32,
            "guide I4 geometry changed"
        );
        let pixels = raw.iter().flat_map(|v| [v & 15, v >> 4]).collect();
        records.push(json!({"id":id,"member":member,"palette_member":pal,"width":width,"height":height,"pixels_sha256":sha(&raw),"palette_sha256":sha(&palette)}));
        textures.push(Texture {
            member,
            palette,
            width,
            height,
            raw,
            pixels,
        });
    }
    let mut shared = None;
    let mut layouts = Vec::new();
    for member in 0..4 {
        let bytes = unpack(narc.members[member])?;
        ensure!(
            slice(&bytes, 0, 4)? == b"DSIF"
                && u32le(&bytes, 12)? == 32
                && slice(&bytes, 32, 4)? == b"nCSC",
            "guide DSIF header changed"
        );
        let count = u32le(&bytes, 32 + 44)?;
        let offset = 32 + u32le(&bytes, 32 + 48)?;
        ensure!(count == 11 && offset == 160, "guide crop table changed");
        let mut crops = Vec::new();
        for n in 0..count {
            let entry = offset + n * 20;
            let texture = u32le(&bytes, entry)?;
            let t = textures
                .get(texture)
                .ok_or_else(|| anyhow::anyhow!("guide crop texture out of bounds"))?;
            let mut coords = [0; 4];
            for (axis, coord) in coords.iter_mut().enumerate() {
                let v = u32le(&bytes, entry + 4 + axis * 4)?;
                let size = if axis % 2 == 0 { t.width } else { t.height };
                ensure!(
                    v <= 4096 && v * size % 4096 == 0,
                    "fractional guide UV boundary"
                );
                *coord = v * size / 4096;
            }
            let [x0, y0, x1, y1] = coords;
            ensure!(x0 < x1 && y0 < y1, "empty guide crop");
            crops.push(Crop {
                texture,
                x0,
                y0,
                x1,
                y1,
            });
        }
        if let Some(expected) = &shared {
            ensure!(*expected == crops, "guide variants disagree on crops");
        } else {
            shared = Some(crops.clone());
        }
        layouts
            .push(json!({"member":member,"sha256":sha(&bytes),"crop_offset":offset,"crops":crops}));
    }
    let crops = shared.unwrap();
    let expected = [
        (0, [0, 1, 34, 41]),
        (0, [37, 2, 63, 14]),
        (0, [37, 18, 63, 30]),
        (1, [0, 0, 34, 12]),
        (1, [0, 12, 34, 24]),
        (1, [0, 24, 42, 36]),
        (1, [0, 36, 42, 48]),
        (1, [0, 48, 34, 60]),
        (2, [0, 0, 32, 32]),
        (3, [0, 0, 32, 32]),
        (4, [0, 0, 32, 32]),
    ];
    for (c, (t, [x0, y0, x1, y1])) in crops.iter().zip(expected) {
        ensure!(
            *c == Crop {
                texture: t,
                x0,
                y0,
                x1,
                y1
            },
            "guide crop interpretation changed"
        );
    }
    Ok((
        textures,
        crops,
        json!({"archive":ARCHIVE,"archive_sha256":sha(source),"table_sha256":sha(table),"textures":records,"layouts":layouts}),
    ))
}

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (textures, crops, mapping) = read(rom)?;
    fs::create_dir_all(out)?;
    let mut entries = Vec::new();
    for (id, c) in crops.iter().enumerate() {
        let t = &textures[c.texture];
        let pixels = c.pixels(t);
        write_png(
            &out.join(format!("crop-{id:02}.png")),
            c.x1 - c.x0,
            c.y1 - c.y0,
            &crate::titles::rgba(&pixels, &t.palette)?,
        )?;
        entries.push(json!({"id":id,"crop":c,"pixels_sha256":sha(&pixels)}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"mapping":mapping,"crops":entries,"claim":"target-local DSIF normalized UV rectangles matched to texture table; index-zero transparency preview; live selection is separate"});
    json_file(&out.join("guides.json"), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    crop: usize,
    source_pixels_sha256: String,
    japanese: String,
    korean: String,
}

/// Galmuri9 v2.40.3; 10px is its native pixel grid, so glyphs rasterise without partial coverage.
const FONT_SHA256: &str = "48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee";
const FONT_SIZE: f32 = 10.0;
/// Baseline row inside each 12px crop, leaving room for the 1px outline above and below.
const BASELINE: i32 = 10;

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.entries.iter().map(|e| e.crop).collect::<Vec<_>>() == (1..8).collect::<Vec<_>>(),
        "guide draft population changed"
    );
    let bytes = fs::read(font_path)?;
    ensure!(sha(&bytes) == FONT_SHA256, "font identity mismatch");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let (textures, crops, mapping) = read(rom)?;
    let mut changed: Vec<_> = textures.iter().map(|t| t.pixels.clone()).collect();
    let mut records = Vec::new();
    for label in tr.entries {
        let c = crops[label.crop];
        let t = &textures[c.texture];
        let original = c.pixels(t);
        ensure!(
            sha(&original) == label.source_pixels_sha256
                && !label.japanese.is_empty()
                && !label.korean.trim().is_empty(),
            "guide label identity/content mismatch"
        );
        let used: BTreeSet<_> = original.iter().copied().filter(|&v| v != 0).collect();
        let mut ranked = Vec::new();
        for &i in &used {
            let color = crate::buttons::rgb(&t.palette, i as usize)?;
            ranked.push((i, color));
        }
        let white = ranked
            .iter()
            .min_by_key(|(_, c)| c.iter().map(|v| (31 - v).pow(2)).sum::<i32>())
            .ok_or_else(|| anyhow::anyhow!("empty source label"))?
            .0;
        let dark = ranked
            .iter()
            .min_by_key(|(_, c)| c.iter().map(|v| v * v).sum::<i32>())
            .unwrap()
            .0;
        ensure!(white != dark, "guide palette lacks contrast");
        let width: usize = label
            .korean
            .chars()
            .map(|ch| font.metrics(ch, FONT_SIZE).advance_width.round() as usize)
            .sum();
        ensure!(width + 2 <= c.x1 - c.x0, "guide caption too wide");
        let mut cursor = c.x0 + (c.x1 - c.x0 - width) / 2;
        let mut ink = Vec::new();
        for ch in label.korean.chars() {
            ensure!(font.lookup_glyph_index(ch) != 0, "missing guide glyph {ch}");
            let (m, bitmap) = font.rasterize(ch, FONT_SIZE);
            for y in 0..m.height {
                for x in 0..m.width {
                    if bitmap[y * m.width + x] < 128 {
                        continue;
                    }
                    let px = cursor as i32 + m.xmin + x as i32;
                    let py = c.y0 as i32 + BASELINE - m.ymin - m.height as i32 + y as i32;
                    ensure!(
                        px > c.x0 as i32
                            && px < (c.x1 - 1) as i32
                            && py > c.y0 as i32
                            && py < (c.y1 - 1) as i32,
                        "guide ink exceeds crop {}: {} at ({px},{py})",
                        label.crop,
                        label.korean
                    );
                    ink.push((px as usize, py as usize));
                }
            }
            cursor += m.advance_width.round() as usize;
        }
        let pixels = &mut changed[c.texture];
        for y in c.y0..c.y1 {
            for x in c.x0..c.x1 {
                pixels[y * t.width + x] = 0;
            }
        }
        for &(x, y) in &ink {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                pixels[(y as i32 + dy) as usize * t.width + (x as i32 + dx) as usize] = dark;
            }
        }
        for &(x, y) in &ink {
            pixels[y * t.width + x] = white;
        }
        records.push(json!({"crop":label.crop,"rectangle":c,"japanese":label.japanese,"korean":label.korean,"source_pixels_sha256":sha(&original),"font_size":FONT_SIZE,"ink_index":white,"outline_index":dark,"advance_width":width}));
    }
    let source = rom.data(rom.file(ARCHIVE)?);
    let narc = Narc::parse(source)?;
    let mut replacements = BTreeMap::new();
    let mut members = Vec::new();
    for (id, t) in textures.iter().enumerate() {
        for (p, (&old, &new)) in t.pixels.iter().zip(&changed[id]).enumerate() {
            if !crops[1..8]
                .iter()
                .any(|c| c.texture == id && c.contains(p % t.width, p / t.width))
            {
                ensure!(old == new, "protected guide pixel changed");
            }
        }
        let raw: Vec<_> = changed[id]
            .chunks_exact(2)
            .map(|p| p[0] | (p[1] << 4))
            .collect();
        if raw == t.raw {
            continue;
        }
        let packed = crate::compress::pack(&raw)?;
        ensure!(
            unpack_halfword(&packed)? == raw,
            "guide halfword round trip failed"
        );
        members.push(json!({"texture":id,"member":t.member,"decoded_sha256":sha(&raw),"stored_size":packed.len(),"capacity":narc.members[t.member].len()}));
        replacements.insert(t.member, packed);
    }
    ensure!(
        replacements.keys().copied().collect::<Vec<_>>() == vec![9, 13],
        "unexpected guide member writers"
    );
    let rebuilt = crate::archive::replace(source, &replacements)?;
    let parsed = Narc::parse(&rebuilt)?;
    for t in &textures {
        let expected: Vec<_> = changed[textures.iter().position(|v| v.member == t.member).unwrap()]
            .chunks_exact(2)
            .map(|p| p[0] | p[1] << 4)
            .collect();
        ensure!(
            unpack_halfword(parsed.members[t.member])? == expected,
            "rebuilt guide mismatch"
        );
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("guide_icon.narc"), &rebuilt)?;
    for id in 0..2 {
        let t = &textures[id];
        write_png(
            &out.join(format!("{id:02}-korean.png")),
            t.width,
            t.height,
            &crate::titles::rgba(&changed[id], &t.palette)?,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"guide_icon.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"mapping":mapping,"labels":records,"members":members,"protected":"all other texels, all button icons, all palettes, all DSIF metadata, other NARC members and padding","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
