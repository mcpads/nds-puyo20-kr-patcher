use crate::{assets::json_file, format::*, graphics::write_png, screens};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

const ARCHIVE: &str = "menu/select_character.narc";

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 627, "selection population changed");
    fs::create_dir_all(out)?;
    let mut members = Vec::new();
    for id in 0..151 {
        let data = unpack(narc.members[id])?;
        fs::write(out.join(format!("member-{id}.bin")), &data)?;
        members.push(json!({"id":id,"decoded_size":data.len(),"decoded_sha256":sha(&data),"prefix":hex::encode(&data[..data.len().min(16)])}));
    }
    let mut backgrounds = Vec::new();
    for first in (0..15).step_by(3) {
        let map = unpack(narc.members[first])?;
        let tiles = unpack(narc.members[first + 1])?;
        let palette = unpack(narc.members[first + 2])?;
        ensure!(palette.len() % 32 == 0, "invalid palette");
        let pixels = screens::render(&map, &tiles, palette.len() / 32)?;
        write_png(
            &out.join(format!("bg-{first}.png")),
            256,
            192,
            &screens::rgba(&pixels, &palette)?,
        )?;
        fs::write(out.join(format!("bg-{first}-pixels.bin")), &pixels)?;
        backgrounds.push(json!({"members":[first,first+1,first+2],"pixels_sha256":sha(&pixels)}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"archive":ARCHIVE,"archive_sha256":sha(source),"members":members,"backgrounds":backgrounds,"claim":"Five static 4bpp backgrounds and decoded UI member candidates; sprite dimensions and consumers not inferred"});
    json_file(&out.join("selection.json"), &report)?;
    Ok(report)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    entries: Vec<Label>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    member: usize,
    source_pixels_sha256: String,
    japanese: String,
    korean: String,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    use std::collections::BTreeMap;
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && matches!(
                tr.entries
                    .iter()
                    .map(|e| e.member)
                    .collect::<Vec<_>>()
                    .as_slice(),
                [40, 42, 44, 46, 124, 146, 148] | [344, 346, 348, 350, 352, 354, 356, 358]
            ),
        "selection draft population changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let source = rom.data(rom.file(ARCHIVE)?);
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 627, "selection population changed");
    let vs = rom.data(rom.file("menu/charselect_vs8p_texlist.bin")?);
    let one = rom.data(rom.file("menu/characterselect_1p_t_texlist.bin")?);
    let two = rom.data(rom.file("menu/characterselect_2p_t_texlist.bin")?);
    ensure!(
        vs.len() == 87 * 12 && two.len() == 134 * 12,
        "selection tables changed"
    );
    let mut changes = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    for label in tr.entries {
        let member = label.member;
        let (table, id, width, height, format) = match member {
            40 => (vs, 39, 256, 18, 3),
            42 => (vs, 40, 256, 18, 3),
            44 => (vs, 41, 256, 18, 3),
            46 => (vs, 42, 256, 18, 3),
            124 => (vs, 35, 128, 35, 3),
            146 => (two, 132, 64, 64, 1),
            148 => (vs, 83, 32, 32, 1),
            344..=358 => (one, 71 + (member - 344) / 2, 128, 14, 3),
            _ => unreachable!(),
        };
        let row = slice(table, id * 12, 12)?;
        ensure!(
            [
                u16le(row, 0)?,
                u16le(row, 2)?,
                u16le(row, 4)?,
                u16le(row, 6)?,
                u16le(row, 8)?,
                u16le(row, 10)?
            ] == [member, member - 1, format, 0, width, height],
            "selection texture mapping changed"
        );
        let raw = unpack_halfword(narc.members[member])?;
        let palette = unpack(narc.members[member - 1])?;
        ensure!(
            sha(&raw) == label.source_pixels_sha256
                && !label.japanese.is_empty()
                && !label.korean.trim().is_empty(),
            "selection source/content mismatch"
        );
        let original: Vec<u8> = if format == 3 {
            raw.iter().flat_map(|v| [v & 15, v >> 4]).collect()
        } else {
            raw.clone()
        };
        ensure!(
            original.len() == width * (height + usize::from(format == 3)),
            "selection stored rows changed"
        );
        ensure!(
            palette.len()
                == match member {
                    146 => 50,
                    148 => 58,
                    _ => 32,
                },
            "selection palette size changed"
        );
        let (x0, y0, x1, y1) = if member == 124 {
            (62, 3, 120, 18)
        } else {
            (0, 0, width, height)
        };
        let contains = |x, y| (x0..x1).contains(&x) && (y0..y1).contains(&y);
        let nearest = |target: [i32; 3]| -> Result<u8> {
            (1..palette.len() / 2)
                .map(|i| {
                    Ok((
                        i as u8,
                        crate::buttons::rgb(&palette, i)?
                            .iter()
                            .zip(target)
                            .map(|(a, b)| (a - b).pow(2))
                            .sum::<i32>(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .min_by_key(|(_, d)| *d)
                .map(|(i, _)| i)
                .ok_or_else(|| anyhow::anyhow!("empty palette"))
        };
        let white = nearest([31, 31, 31])?;
        let outline = nearest(match member {
            40 => [0, 16, 31],
            42 => [31, 0, 17],
            44 => [0, 23, 0],
            46 => [31, 10, 0],
            124 => [0, 10, 0],
            344..=358 => [0, 0, 0],
            _ => [31, 2, 0],
        })?;
        ensure!(white != outline, "selection colors lack contrast");
        let (size, scale, baseline) = match member {
            146 => (11.0, 2, 42),
            148 => (11.0, 1, 21),
            124 => (11.0, 1, 15),
            344..=358 => (12.0, 1, 12),
            _ => (12.0, 1, 14),
        };
        let advance = |ch| {
            if ch == ' ' {
                5
            } else {
                font.metrics(ch, size).advance_width.round() as usize
            }
        };
        let text_width = label.korean.chars().map(advance).sum::<usize>() * scale;
        ensure!(
            text_width > 0 && text_width + 4 <= x1 - x0,
            "selection caption too wide"
        );
        let mut cursor = x0 + (x1 - x0 - text_width) / 2;
        let mut ink = Vec::new();
        for ch in label.korean.chars() {
            ensure!(
                !ch.is_control() && font.lookup_glyph_index(ch) != 0,
                "missing selection glyph {ch}"
            );
            let (m, bitmap) = font.rasterize(ch, size);
            for y in 0..m.height {
                for x in 0..m.width {
                    if bitmap[y * m.width + x] < 128 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = cursor as i32 + (m.xmin + x as i32) * scale as i32 + dx as i32;
                            let py = baseline
                                + (-m.ymin - m.height as i32 + y as i32) * scale as i32
                                + dy as i32;
                            ensure!(
                                px > x0 as i32
                                    && px < (x1 - 1) as i32
                                    && py > y0 as i32
                                    && py < (y1 - 1) as i32,
                                "selection ink outside region"
                            );
                            ink.push((px as usize, py as usize));
                        }
                    }
                }
            }
            cursor += advance(ch) * scale;
        }
        ensure!(!ink.is_empty(), "empty selection ink");
        let mut pixels = original.clone();
        for y in y0..y1 {
            for x in x0..x1 {
                // START's original clear column supplies the same row's green backdrop.
                pixels[y * width + x] = if member == 124 {
                    original[y * width + 60]
                } else {
                    0
                };
            }
        }
        let opaque = |v: u8| if format == 1 { v | 0xe0 } else { v };
        if format == 1 {
            // The two complete stamp artworks are editable. Keep the original palette;
            // draw a round seal at the unchanged texture size, with explicit alpha.
            let gold = nearest([31, 25, 0])?;
            let radius = width as i32 / 2 - 1;
            for y in 0..height {
                for x in 0..width {
                    let dx = x as i32 - width as i32 / 2;
                    let dy = y as i32 - height as i32 / 2;
                    let d = dx * dx + dy * dy;
                    if d <= radius * radius {
                        pixels[y * width + x] = opaque(if d >= (radius - 2) * (radius - 2) {
                            gold
                        } else {
                            outline
                        });
                    }
                }
            }
        }
        for &(x, y) in &ink {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                pixels[(y as i32 + dy) as usize * width + (x as i32 + dx) as usize] =
                    opaque(outline);
            }
        }
        for &(x, y) in &ink {
            pixels[y * width + x] = opaque(white);
        }
        for (p, (&old, &new)) in original.iter().zip(&pixels).enumerate() {
            if !contains(p % width, p / width) {
                ensure!(old == new, "protected selection texel changed");
            }
        }
        let encoded: Vec<u8> = if format == 3 {
            pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect()
        } else {
            pixels.clone()
        };
        let packed = crate::compress::pack(&encoded)?;
        ensure!(
            unpack_halfword(&packed)? == encoded,
            "selection halfword round trip failed"
        );
        let mut rgba = Vec::new();
        for &v in &pixels {
            let c = crate::buttons::rgb(
                &palette,
                if format == 1 {
                    v as usize & 31
                } else {
                    v as usize
                },
            )?;
            rgba.extend([
                (c[0] * 255 / 31) as u8,
                (c[1] * 255 / 31) as u8,
                (c[2] * 255 / 31) as u8,
                if format == 1 {
                    ((v as u16 >> 5) * 255 / 7) as u8
                } else if v == 0 {
                    0
                } else {
                    255
                },
            ]);
        }
        previews.push((member, width, pixels.len() / width, rgba));
        records.push(json!({"member":member,"table_id":id,"format":format,"width":width,"height":height,"editable":[x0,y0,x1,y1],"japanese":label.japanese,"korean":label.korean,"source_pixels_sha256":sha(&raw),"decoded_sha256":sha(&encoded),"stored_size":packed.len(),"capacity":narc.members[member].len(),"palette_sha256":sha(&palette)}));
        ensure!(
            changes.insert(member, packed).is_none(),
            "duplicate selection member"
        );
    }
    let rebuilt = crate::archive::replace(source, &changes)?;
    let parsed = Narc::parse(&rebuilt)?;
    for (&id, expected) in &changes {
        ensure!(
            parsed.members[id] == expected,
            "rebuilt selection member differs"
        );
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("selection.narc"), &rebuilt)?;
    for (id, w, h, rgba) in previews {
        write_png(&out.join(format!("{id}-korean.png")), w, h, &rgba)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"selection.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"labels":records,"protected":"all palettes, table/DSIF/layout metadata, other members, texels outside explicit regions including stored trailing rows","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
