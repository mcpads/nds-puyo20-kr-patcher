use crate::{
    archive, assets::json_file, buttons::rgb, compress, format::*, graphics::write_png,
    titles::rgba,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    #[serde(default)]
    surface: Option<String>,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    id: usize,
    source_pixels_sha256: String,
    source: String,
    korean: String,
    #[serde(default = "default_scale")]
    scale: usize,
}

fn default_scale() -> usize {
    1
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    let surface = tr.surface.as_deref().unwrap_or("story");
    let small = surface == "character_select_small";
    let selection = match surface {
        "story" => false,
        "character_select" | "character_select_small" => true,
        _ => anyhow::bail!("unknown name surface"),
    };
    let (path, table_path, population, first, count, height) = if selection {
        (
            "menu/select_character.narc",
            if small {
                "menu/characterselect_2p_t_texlist.bin"
            } else {
                "menu/characterselect_1p_t_texlist.bin"
            },
            627,
            if small { 34 } else { 30 },
            41,
            if small { 32 } else { 48 },
        )
    } else {
        (
            "story_demo/mz_name_plate.narc",
            "story_demo/mz_name_texlist.bin",
            66,
            0,
            29,
            32,
        )
    };
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == count,
        "unexpected name draft population"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let source = rom.data(rom.file(path)?);
    let narc = Narc::parse(source)?;
    let table = rom.data(rom.file(table_path)?);
    ensure!(
        narc.members.len() == population
            && table.len()
                == if small {
                    134 * 12
                } else if selection {
                    110 * 12
                } else {
                    33 * 12
                },
        "name population changed"
    );
    if !selection {
        ensure!(
            hex::encode(&table[32 * 12..]) == "000000000200000000000000",
            "zero-dimension name table record changed"
        );
    }
    if small {
        let shared = rom.data(rom.file("menu/characterselect_4p_t_texlist.bin")?);
        ensure!(
            shared.len() == 78 * 12 && shared[30 * 12..71 * 12] == table[34 * 12..75 * 12],
            "shared 4p name records changed"
        );
    }
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    for (index, label) in tr.entries.iter().enumerate() {
        let id = first + index;
        let scale = label.scale;
        ensure!(
            (1..=2).contains(&scale) && ((selection && !small) || scale == 1),
            "invalid name scale"
        );
        ensure!(
            label.id == id && !label.source.is_empty() && !label.korean.trim().is_empty(),
            "invalid name draft identity"
        );
        let row = slice(table, id * 12, 12)?;
        let member = u16le(row, 0)?;
        let palette_member = u16le(row, 2)?;
        ensure!(
            u16le(row, 4)? == 3
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == 128
                && u16le(row, 10)? == height,
            "name geometry changed"
        );
        let old = unpack_halfword(narc.members[member])?;
        let palette = unpack(narc.members[palette_member])?;
        ensure!(
            old.len() == 128 * (height + usize::from(selection && !small)) / 2
                && palette.len() == 32
                && sha(&old) == label.source_pixels_sha256,
            "name source identity changed"
        );
        let source_pixels = old
            .iter()
            .flat_map(|v| [v & 15, v >> 4])
            .collect::<Vec<_>>();
        let pack = |pixels: &[u8]| {
            pixels
                .chunks_exact(2)
                .map(|p| p[0] | (p[1] << 4))
                .collect::<Vec<_>>()
        };
        ensure!(
            pack(&source_pixels) == old,
            "I4 unchanged round trip failed"
        );
        // The names are isolated word-art. Preserve question marks, decorative plates,
        // every palette, resource table and all other NARC members.
        let width = label
            .korean
            .chars()
            .map(|c| font.metrics(c, 12.0).advance_width.round() as usize)
            .sum::<usize>()
            * scale;
        ensure!(width > 0 && width <= 116, "name exceeds canvas");
        let mut ink = vec![false; 128 * height];
        let mut cursor = (128 - width) / 2;
        for c in label.korean.chars() {
            ensure!(
                !c.is_control() && font.lookup_glyph_index(c) != 0,
                "missing name glyph: {c}"
            );
            let (m, bitmap) = font.rasterize(c, 12.0);
            for y in 0..m.height {
                for x in 0..m.width {
                    if bitmap[y * m.width + x] < 128 {
                        continue;
                    }
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = cursor as i32 + (m.xmin + x as i32) * scale as i32 + sx as i32;
                            let baseline = if selection && !small {
                                if scale == 2 { 35 } else { 29 }
                            } else {
                                23
                            };
                            let py = baseline
                                + (-m.ymin - m.height as i32 + y as i32) * scale as i32
                                + sy as i32;
                            ensure!(
                                (3..125).contains(&px) && (3..height as i32 - 3).contains(&py),
                                "name ink exceeds canvas"
                            );
                            ink[py as usize * 128 + px as usize] = true;
                        }
                    }
                }
            }
            cursor += m.advance_width.round() as usize * scale;
        }
        ensure!(ink.iter().any(|v| *v), "empty name ink");
        let mut colors = (1..16)
            .map(|i| Ok((i as u8, rgb(&palette, i)?)))
            .collect::<Result<Vec<_>>>()?;
        colors.sort_by_key(|(_, c)| c.iter().map(|v| v * v).sum::<i32>());
        let mut outline = colors[0].0;
        colors.sort_by_key(|(_, c)| c.iter().map(|v| (31 - v).pow(2)).sum::<i32>());
        let mut white = colors[0].0;
        let nearest = |target: [i32; 3]| -> u8 {
            colors
                .iter()
                .min_by_key(|(_, c)| {
                    c.iter()
                        .zip(target)
                        .map(|(a, b)| (a - b).pow(2))
                        .sum::<i32>()
                })
                .unwrap()
                .0
        };
        let outer = if selection {
            if index < 30 {
                outline = nearest([0, 20, 9]);
                Some(nearest([31, 30, 0]))
            } else {
                outline = white;
                white = nearest([18, 13, 29]);
                Some(nearest([8, 20, 31]))
            }
        } else {
            None
        };
        ensure!(outline != white, "name palette lacks contrast");
        let mut pixels = vec![0; ink.len()];
        if let Some(color) = outer {
            for (p, &on) in ink.iter().enumerate() {
                if on {
                    for dy in -3_i32..=3 {
                        for dx in -3_i32..=3 {
                            if dx * dx + dy * dy <= 10 {
                                pixels[((p / 128) as i32 + dy) as usize * 128
                                    + ((p % 128) as i32 + dx) as usize] = color;
                            }
                        }
                    }
                }
            }
        }
        for (p, &on) in ink.iter().enumerate() {
            if !on {
                continue;
            }
            for dy in -2_i32..=2 {
                for dx in -2_i32..=2 {
                    if dx * dx + dy * dy <= 5 {
                        pixels[((p / 128) as i32 + dy) as usize * 128
                            + ((p % 128) as i32 + dx) as usize] = outline;
                    }
                }
            }
        }
        for (p, &on) in ink.iter().enumerate() {
            if on {
                pixels[p] = white;
            }
        }
        let tail = &source_pixels[128 * height..];
        ensure!(tail.iter().all(|v| *v == 0), "unexpected name tail pixels");
        pixels.extend_from_slice(tail);
        let packed = compress::pack(&pack(&pixels))?;
        ensure!(
            unpack_halfword(&packed)? == pack(&pixels),
            "name compression round trip failed"
        );
        ensure!(
            replacements.insert(member, packed.clone()).is_none(),
            "shared name writer"
        );
        records.push(json!({"id":id,"source":label.source,"korean":label.korean,"member":member,"palette_member":palette_member,"source_pixels_sha256":sha(&old),"pixels_sha256":sha(&pack(&pixels)),"palette_sha256":sha(&palette),"line_width":width,"scale":scale,"stored_bytes":packed.len(),"capacity":narc.members[member].len(),"editable_rectangle":[0,0,128,height],"preserved_tail_bytes":tail.len()/2}));
        previews.push((id, pixels, palette));
    }
    let rebuilt = archive::replace(source, &replacements)?;
    fs::create_dir_all(out)?;
    for (id, pixels, palette) in previews {
        write_png(
            &out.join(format!("{id:02}-korean.png")),
            128,
            pixels.len() / 128,
            &rgba(&pixels, &palette)?,
        )?;
    }
    fs::write(out.join("names.narc"), &rebuilt)?;
    let plan = json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"names.narc","input_sha256":sha(&rebuilt)}]});
    json_file(&out.join("plan.json"), &plan)?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive":path,"surface":surface,"table":table_path,"table_sha256":sha(table),"shared_4p_records_verified":small,"entries":records,"preservation":"all unlisted members, palettes and texture tables unchanged; stored tail pixels preserved","rasterizer":if small {"Galmuri11 12px baseline 23 threshold 128; original green/purple palettes"} else if selection {"Galmuri11 12px with explicit integer scale; baseline 29/35; threshold 128; original green/purple palettes"} else {"Galmuri11 12px baseline 23 threshold 128; 2px palette outline; I4 index 0 transparent"},"state":"development_art_draft","human_reviewed":false,"runtime_verified":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
