use crate::{
    archive,
    assets::json_file,
    buttons::{nearest, rgb},
    compress,
    format::*,
    graphics::write_png,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

use super::render::{CellLayout, paint_cell, paint_text_cell};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleLabels {
    state: String,
    labels: Vec<String>,
    values: Vec<String>,
    choices: Vec<String>,
}

pub fn prepare_rules(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: RuleLabels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.labels.len() == 16
            && tr.values.len() == 8
            && tr.choices.len() == 2,
        "unexpected rule labels"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let path = "rule_edit/rule_edit.narc";
    let source = rom.data(rom.file(path)?);
    let narc = Narc::parse(source)?;
    let table = rom.data(rom.file("rule_edit/edit_rule_b_texlist.bin")?);
    ensure!(
        narc.members.len() == 40 && table.len() == 16 * 12,
        "rule texture population changed"
    );
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut outputs = Vec::new();
    for (id, expected_member, width, strings) in [
        (0, 26, 128, &tr.labels[..8]),
        (1, 30, 128, &tr.labels[8..]),
        (2, 24, 128, &tr.labels[..8]),
        (3, 28, 128, &tr.labels[8..]),
        (9, 34, 64, &tr.values[..]),
        (10, 32, 64, &tr.values[..]),
        (13, 38, 64, &tr.choices[..]),
        (14, 36, 64, &tr.choices[..]),
    ] {
        let row = slice(table, id * 12, 12)?;
        let member = u16le(row, 0)?;
        let pal_id = u16le(row, 2)?;
        let height = strings.len() * 16;
        ensure!(
            member == expected_member
                && pal_id == member - 1
                && u16le(row, 4)? == 6
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == width
                && u16le(row, 10)? == height,
            "rule texture mapping changed"
        );
        let old = unpack_halfword(narc.members[member])?;
        let palette = unpack(narc.members[pal_id])?;
        ensure!(
            old.len() == width * height && palette.len() == 16,
            "rule texture extent changed"
        );
        let white = nearest(&palette, [31, 31, 31])? as u8;
        let dark = nearest(&palette, [0, 0, 0])? as u8;
        ensure!(white != dark, "rule palette lacks contrast");
        let (left, right) = if width == 128 { (14, 114) } else { (0, 48) };
        let mut pixels = old.clone();
        for (p, v) in pixels.iter_mut().enumerate() {
            if (left..right).contains(&(p % width)) {
                *v = 0;
            } else {
                ensure!(*v >> 3 == 0, "rule source ink outside cell");
            }
        }
        let mut widths = Vec::new();
        for (line, text) in strings.iter().enumerate() {
            if width == 128 {
                widths.push(paint_cell(
                    &font,
                    text,
                    64,
                    line,
                    248 | white,
                    248 | dark,
                    &mut pixels,
                )?);
            } else {
                // Paint a 128px scratch row with the shared rasterizer. The resource
                // displays x=0..48 centered at 24; copy only that adopted cell.
                let mut cell = vec![0; 128 * 16];
                widths.push(paint_text_cell(
                    &font,
                    text,
                    CellLayout {
                        center: 64,
                        row: 0,
                        size: 11.0,
                        max_width: 44,
                    },
                    248 | white,
                    248 | dark,
                    &mut cell,
                )?);
                for y in 0..16 {
                    ensure!(
                        cell[y * 128..y * 128 + 40]
                            .iter()
                            .chain(&cell[y * 128 + 88..(y + 1) * 128])
                            .all(|v| *v == 0),
                        "rule value exceeds 48px cell"
                    );
                    pixels[(line * 16 + y) * width..(line * 16 + y) * width + 48]
                        .copy_from_slice(&cell[y * 128 + 40..y * 128 + 88]);
                }
            }
        }
        for (p, (&a, &b)) in old.iter().zip(&pixels).enumerate() {
            if !(left..right).contains(&(p % width)) {
                ensure!(a == b, "protected rule pixel changed");
            }
        }
        let packed = compress::pack(&pixels)?;
        ensure!(
            unpack_halfword(&packed)? == pixels,
            "rule atlas round trip failed"
        );
        ensure!(
            replacements.insert(member, packed.clone()).is_none(),
            "duplicate rule writer"
        );
        let mut rgba = Vec::new();
        for &p in &pixels {
            let c = rgb(&palette, (p & 7) as usize)?;
            rgba.extend([
                (c[0] * 255 / 31) as u8,
                (c[1] * 255 / 31) as u8,
                (c[2] * 255 / 31) as u8,
                ((p >> 3) as u16 * 255 / 31) as u8,
            ]);
        }
        records.push(json!({"id":id,"member":member,"palette_member":pal_id,"source_sha256":sha(&old),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&palette),"texts":strings,"line_widths":widths,"editable_rectangle":[left,0,right,height],"stored_size":packed.len(),"capacity":narc.members[member].len()}));
        outputs.push((id, width, height, pixels, rgba));
    }
    let rebuilt = archive::replace(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("rules.narc"), &rebuilt)?;
    for (id, width, height, pixels, rgba) in outputs {
        fs::write(out.join(format!("{id:02}-pixels.bin")), pixels)?;
        write_png(
            &out.join(format!("{id:02}-korean.png")),
            width,
            height,
            &rgba,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"rules.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"table_sha256":sha(table),"records":records,"preserved":"palettes, texture list, numbers, button backgrounds and pixels outside declared masks","rasterizer":"Galmuri11 labels 12px; values 11px; baseline 13; threshold 128; 1px outline","state":"development_art_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
