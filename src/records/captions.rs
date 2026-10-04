//! Static DSIF crop census. Table compatibility is not runtime selection proof.
use super::ARCHIVE;
use crate::{assets::json_file, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

struct Texture {
    member: usize,
    palette_member: usize,
    width: usize,
    height: usize,
    format: usize,
    pixels: Vec<u8>,
    palette: Vec<u8>,
}

fn textures(n: &Narc, table: &[u8]) -> Result<Vec<Texture>> {
    ensure!(
        table.len() >= 12 && table.len() % 12 == 0,
        "unaligned record texture list"
    );
    let mut result = Vec::new();
    for row in table.chunks_exact(12).take(table.len() / 12 - 1) {
        let member = u16le(row, 0)?;
        let palette_member = u16le(row, 2)?;
        let format = u16le(row, 4)?;
        let width = u16le(row, 8)?;
        let height = u16le(row, 10)?;
        ensure!(
            member < n.members.len()
                && palette_member < n.members.len()
                && matches!(format, 1 | 3 | 6)
                && u16le(row, 6)? == 0
                && width > 0
                && height > 0,
            "invalid record texture row"
        );
        let raw = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[palette_member])?;
        let pixels: Vec<u8> = if format == 3 {
            raw.iter().flat_map(|v| [v & 15, v >> 4]).collect()
        } else {
            raw.clone()
        };
        ensure!(
            pixels.len() == width * height,
            "record texture geometry changed"
        );
        let repacked: Vec<u8> = if format == 3 {
            pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect()
        } else {
            pixels.clone()
        };
        ensure!(repacked == raw, "record texture round trip failed");
        result.push(Texture {
            member,
            palette_member,
            width,
            height,
            format,
            pixels,
            palette,
        });
    }
    ensure!(
        table[table.len() - 12..] == [0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        "record texture sentinel changed"
    );
    Ok(result)
}

fn preview(t: &Texture, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut rgba = Vec::new();
    for &p in pixels {
        let (index, alpha) = match t.format {
            1 => (p & 31, (p >> 5) as usize * 255 / 7),
            6 => (p & 7, (p >> 3) as usize * 255 / 31),
            3 => (p, if p == 0 { 0 } else { 255 }),
            _ => unreachable!(),
        };
        let c = u16le(&t.palette, index as usize * 2)?;
        rgba.extend([
            ((c & 31) * 255 / 31) as u8,
            (((c >> 5) & 31) * 255 / 31) as u8,
            (((c >> 10) & 31) * 255 / 31) as u8,
            alpha as u8,
        ]);
    }
    Ok(rgba)
}

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(
        sha(source) == "27417db6a9a533ad988e9a37c6a0f87149339510da64a034a18e34535ecaa776",
        "record source identity changed"
    );
    let n = Narc::parse(source)?;
    ensure!(n.members.len() == 246, "record archive population changed");
    let mut tables = Vec::new();
    for id in 1..=3 {
        let path = format!("record/score_menu{id:02}_t_texlist.bin");
        let bytes = rom.data(rom.file(&path)?);
        tables.push((path, bytes, textures(&n, bytes)?));
    }
    fs::create_dir_all(out)?;
    let mut layouts = Vec::new();
    let mut families: [Option<Vec<Value>>; 3] = [None, None, None];
    let mut all_crops = Vec::new();
    let mut dsif_members = Vec::new();
    for (member, stored) in n.members.iter().enumerate() {
        let b = unpack(stored)?;
        if !b.starts_with(b"DSIF") {
            continue;
        }
        dsif_members.push(member);
        ensure!(
            u32le(&b, 12)? == 32 && slice(&b, 32, 4)? == b"nCSC",
            "record DSIF header changed"
        );
        let count = u32le(&b, 68)?;
        let descriptors = 32 + u32le(&b, 72)?;
        let mut compatible = Vec::new();
        for (table_id, (_, _, images)) in tables.iter().enumerate() {
            if images.len() != count {
                continue;
            }
            let mut matches = true;
            for (i, t) in images.iter().enumerate() {
                // Measured normalized dimensions: width/256 and height/192 at 12-bit scale.
                matches &= u32le(&b, descriptors + i * 8)? == t.width * 16
                    && u32le(&b, descriptors + i * 8 + 4)? == (t.height * 4096 + 96) / 192;
            }
            if matches {
                compatible.push(table_id);
            }
        }
        ensure!(
            compatible.len() == 1,
            "record DSIF has ambiguous texture dimensions"
        );
        let table_id = compatible[0];
        let (table_path, _, images) = &tables[table_id];
        let crop_count = u32le(&b, 76)?;
        let offset = 32 + u32le(&b, 80)?;
        ensure!(
            crop_count == [50, 74, 85][table_id],
            "record crop population changed"
        );
        let mut crops = Vec::new();
        for id in 0..crop_count {
            let entry = offset + id * 20;
            let texture = u32le(&b, entry)?;
            let t = images
                .get(texture)
                .ok_or_else(|| anyhow::anyhow!("record crop texture outside table"))?;
            let mut rect = [0; 4];
            for (axis, value) in rect.iter_mut().enumerate() {
                let uv = u32le(&b, entry + 4 + axis * 4)?;
                let size = if axis % 2 == 0 { t.width } else { t.height };
                ensure!(uv <= 4096 && uv * size % 4096 == 0, "fractional record UV");
                *value = uv * size / 4096;
            }
            let [x0, y0, x1, y1] = rect;
            ensure!(x0 < x1 && y0 < y1, "empty record crop");
            crops.push(json!({"id":id,"texture":texture,"member":t.member,"rect":rect}));
            if families[table_id].is_none() {
                let pixels: Vec<u8> = (y0..y1)
                    .flat_map(|y| t.pixels[y * t.width + x0..y * t.width + x1].iter().copied())
                    .collect();
                let filename = format!("table-{:02}-crop-{id:02}", table_id + 1);
                let rgba = preview(t, &pixels)?;
                write_png(
                    &out.join(format!("{filename}.png")),
                    x1 - x0,
                    y1 - y0,
                    &rgba,
                )?;
                fs::write(out.join(format!("{filename}.bin")), &pixels)?;
                let caption = match table_id {
                    1 => (13..=71).contains(&id),
                    2 => (2..=9).contains(&id) || (32..=43).contains(&id),
                    _ => false,
                };
                all_crops.push(json!({"table":table_path,"id":id,"texture":texture,"member":t.member,"palette_member":t.palette_member,"format":t.format,"rect":rect,"pixels_sha256":sha(&pixels),"rgba_sha256":sha(&rgba),"palette_sha256":sha(&t.palette),"source_file":filename,"classification":if caption {"caption_candidate"} else {"non_caption_preserve"},"alpha":if t.format==3 {"index_zero_preview_assumption"}else{"explicit"},"runtime_consumer_verified":false}));
            }
        }
        if let Some(previous) = &families[table_id] {
            ensure!(*previous == crops, "shared record crop tables disagree");
        } else {
            families[table_id] = Some(crops);
        }
        layouts.push(json!({"member":member,"sha256":sha(&b),"table":table_path,"crop_count":crop_count,"crop_offset":offset,"crops":families[table_id]}));
    }
    ensure!(
        dsif_members == [11, 12, 13, 14, 17, 18, 19, 20, 230, 231],
        "record DSIF population changed"
    );
    ensure!(
        all_crops.len() == 209,
        "record unique crop population changed"
    );
    let captions = all_crops
        .iter()
        .filter(|c| c["classification"] == "caption_candidate")
        .count();
    ensure!(captions == 79, "record caption population changed");
    let report = json!({"source_sha256":sha(rom.bytes),"archive":ARCHIVE,"archive_sha256":sha(source),"tables":tables.iter().map(|(p,b,t)|json!({"path":p,"sha256":sha(b),"textures":t.len()})).collect::<Vec<_>>(),"layouts":layouts,"unique_crops":all_crops,"caption_candidates":captions,"claim":"Static dimension-compatible DSIF/texture mapping, exact integer UV crops and unchanged texel round trips. Runtime selection and final edit regions remain unverified."});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
