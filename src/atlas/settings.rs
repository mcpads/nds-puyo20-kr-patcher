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

use super::render::paint_cell;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    labels: Vec<String>,
    values: Vec<String>,
    screens: Vec<String>,
    styles: Vec<Style>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Style {
    source: String,
    korean: String,
}

pub fn prepare_settings(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let translation_bytes = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&translation_bytes)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.labels.len() == 8
            && tr.values.len() == 8
            && tr.screens.len() == 2,
        "unexpected settings label structure"
    );
    ensure!(tr.styles.len() == 19, "expected 19 Puyo styles");
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let path = "option/setting.narc";
    let source = rom.data(rom.file(path)?);
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 114, "unexpected settings archive");
    let table = rom.data(rom.file("option/setting_b_texlist.bin")?);
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut outputs = Vec::new();
    for (id, expected_member, format, center, strings) in [
        (11, 28, 6, 64, &tr.labels),
        (14, 26, 6, 64, &tr.labels),
        (12, 32, 6, 56, &tr.values),
        (15, 30, 6, 56, &tr.values),
        (13, 37, 1, 56, &tr.screens),
        (16, 35, 1, 56, &tr.screens),
    ] {
        let row = slice(table, id * 12, 12)?;
        let member = u16le(row, 0)?;
        let palette_member = u16le(row, 2)?;
        ensure!(
            member == expected_member
                && palette_member == member - 1
                && u16le(row, 4)? == format
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == 128
                && u16le(row, 10)? == strings.len() * 16,
            "settings texture mapping mismatch"
        );
        let old = unpack_halfword(narc.members[member])?;
        let palette = unpack(narc.members[palette_member])?;
        ensure!(
            old.len() == 128 * strings.len() * 16 && palette.len() == 16,
            "unexpected settings extent"
        );
        let alpha_shift = if format == 6 { 3 } else { 5 };
        let opaque = if format == 6 { 248 } else { 224 };
        let index_mask = if format == 6 { 7 } else { 31 };
        let white = nearest(&palette, [31, 31, 31])? as u8;
        let dark = nearest(&palette, [0, 0, 0])? as u8;
        let mut pixels = old.clone();
        // Whole text cells are adopted editable masks. Alpha must change with the
        // new glyph shapes; preserve palette and all pixels outside these masks.
        for (p, v) in pixels.iter_mut().enumerate() {
            if (center - 48..center + 48).contains(&(p % 128)) {
                *v = 0;
            } else {
                ensure!(
                    *v >> alpha_shift == 0,
                    "source ink outside adopted cell mask"
                );
            }
        }
        let mut widths = Vec::new();
        for (line, text) in strings.iter().enumerate() {
            widths.push(paint_cell(
                &font,
                text,
                center,
                line,
                opaque | white,
                opaque | dark,
                &mut pixels,
            )?);
        }
        let mut rgba = Vec::new();
        for (p, (&before, &after)) in old.iter().zip(&pixels).enumerate() {
            if !(center - 48..center + 48).contains(&(p % 128)) {
                ensure!(before == after, "protected pixel changed");
            }
            let c = rgb(&palette, (after & index_mask) as usize)?;
            let a = (after >> alpha_shift) as u16 * 255 / (255 >> alpha_shift);
            rgba.extend([
                (c[0] * 255 / 31) as u8,
                (c[1] * 255 / 31) as u8,
                (c[2] * 255 / 31) as u8,
                a as u8,
            ]);
        }
        let packed = compress::pack(&pixels)?;
        ensure!(
            unpack_halfword(&packed)? == pixels,
            "atlas round trip mismatch"
        );
        records.push(json!({"id":id,"member":member,"palette_member":palette_member,"format":format,"center":center,"cell_height":16,"texts":strings,"widths":widths,"source_sha256":sha(&old),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&palette),"stored_size":packed.len(),"capacity":narc.members[member].len()}));
        replacements.insert(member, packed);
        outputs.push((id, pixels, rgba));
    }
    for (style_id, style) in tr.styles.iter().enumerate() {
        ensure!(!style.source.is_empty(), "missing source label");
        if style_id == 15 {
            ensure!(
                style.source == "MSX" && style.korean == "MSX",
                "MSX label must remain unchanged"
            );
            continue;
        }
        for id in [17 + style_id, 36 + style_id] {
            let row = slice(table, id * 12, 12)?;
            let member = u16le(row, 0)?;
            let palette_member = u16le(row, 2)?;
            ensure!(
                u16le(row, 4)? == 1
                    && u16le(row, 6)? == 0
                    && u16le(row, 8)? == 128
                    && u16le(row, 10)? == 16,
                "unexpected Puyo texture geometry"
            );
            let old = unpack_halfword(narc.members[member])?;
            let palette = unpack(narc.members[palette_member])?;
            ensure!(
                old.len() == 2048 && palette.len() <= 64 && palette.len() % 2 == 0,
                "unexpected Puyo texture extent"
            );
            let active: Vec<_> = (0..128)
                .map(|x| (0..16).any(|y| old[y * 128 + x] >> 5 != 0))
                .collect();
            let icon_end = active
                .iter()
                .rposition(|v| *v)
                .ok_or_else(|| anyhow::anyhow!("empty Puyo image"))?
                + 1;
            let mut icon_start = icon_end - 1;
            while icon_start > 0 && active[icon_start - 1] {
                icon_start -= 1;
            }
            ensure!(
                (11..=14).contains(&(icon_end - icon_start)) && icon_start > 1,
                "unresolved Puyo icon boundary"
            );
            let text_end = active[..icon_start]
                .iter()
                .rposition(|v| *v)
                .ok_or_else(|| anyhow::anyhow!("missing Puyo text"))?
                + 1;
            let text_start = active.iter().position(|v| *v).unwrap();
            ensure!(text_end + 1 < icon_start, "missing text/icon gap");
            // The original draws the icon right after the name, keeping the
            // name/icon group centred. Keep the source gap and group centre.
            let gap = icon_start - text_end;
            let group_center2 = text_start + icon_end;
            let icon_width = icon_end - icon_start;
            // Icons share this palette. Select ink only from colors actually
            // used by the source text, so icon reds never become text outlines.
            let (mut lightest, mut darkest) = ((i32::MAX, 0), (i32::MAX, 0));
            for (p, &v) in old.iter().enumerate() {
                if p % 128 >= icon_start - 1 || v >> 5 == 0 {
                    continue;
                }
                let index = v & 31;
                let c = rgb(&palette, index as usize)?;
                let light = c.iter().map(|v| (31 - v).pow(2)).sum();
                let dark = c.iter().map(|v| v.pow(2)).sum();
                if light < lightest.0 {
                    lightest = (light, index);
                }
                if dark < darkest.0 {
                    darkest = (dark, index);
                }
            }
            let white = lightest.1;
            let dark = darkest.1;
            // Measure the rendered ink (outline included) at a trial centre.
            let mut trial = vec![0u8; 2048];
            paint_cell(
                &font,
                &style.korean,
                64,
                0,
                224 | white,
                224 | dark,
                &mut trial,
            )?;
            let ink: Vec<_> = (0..128)
                .filter(|&x| (0..16).any(|y| trial[y * 128 + x] >> 5 != 0))
                .collect();
            let (ink_start, ink_end) = (ink[0], ink[ink.len() - 1] + 1);
            let group = ink_end - ink_start + gap + icon_width;
            ensure!(group_center2 >= group, "Puyo label group leaves texture");
            let group_start = (group_center2 - group) / 2;
            let new_icon_start = group_start + ink_end - ink_start + gap;
            ensure!(
                group_start >= 1 && new_icon_start + icon_width <= 128,
                "Puyo label group leaves texture"
            );
            let center = 64 + group_start - ink_start;
            let mut pixels = vec![0u8; 2048];
            let width = paint_cell(
                &font,
                &style.korean,
                center,
                0,
                224 | white,
                224 | dark,
                &mut pixels,
            )?;
            for y in 0..16 {
                for x in 0..icon_width {
                    ensure!(
                        pixels[y * 128 + new_icon_start + x] == 0,
                        "Puyo label overlaps moved icon"
                    );
                    pixels[y * 128 + new_icon_start + x] = old[y * 128 + icon_start + x];
                }
            }
            let mask_end = new_icon_start;
            let mut rgba = Vec::new();
            for (p, (&before, &after)) in old.iter().zip(&pixels).enumerate() {
                let x = p % 128;
                if x >= new_icon_start + icon_width {
                    ensure!(after == 0, "ink right of the moved icon");
                }
                if x >= icon_end {
                    ensure!(before >> 5 == 0, "source ink right of the icon");
                }
                let c = rgb(&palette, (after & 31) as usize)?;
                rgba.extend([
                    (c[0] * 255 / 31) as u8,
                    (c[1] * 255 / 31) as u8,
                    (c[2] * 255 / 31) as u8,
                    ((after >> 5) as u16 * 255 / 7) as u8,
                ]);
            }
            let packed = compress::pack(&pixels)?;
            ensure!(
                unpack_halfword(&packed)? == pixels,
                "Puyo round trip mismatch"
            );
            records.push(json!({"id":id,"member":member,"palette_member":palette_member,"format":1,"style_id":style_id,"source_text":style.source,"text":style.korean,"center":center,"width":width,"editable_x":[0,mask_end],"source_icon_x":[icon_start,icon_end],"icon_x":[new_icon_start,new_icon_start+icon_width],"gap":gap,"source_sha256":sha(&old),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&palette),"stored_size":packed.len(),"capacity":narc.members[member].len()}));
            ensure!(
                replacements.insert(member, packed).is_none(),
                "duplicate texture writer"
            );
            outputs.push((id, pixels, rgba));
        }
    }
    let rebuilt = archive::replace(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("setting.narc"), &rebuilt)?;
    for (id, pixels, rgba) in outputs {
        write_png(
            &out.join(format!("{id:02}-korean.png")),
            128,
            pixels.len() / 128,
            &rgba,
        )?;
        fs::write(out.join(format!("{id:02}-pixels.bin")), pixels)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"setting.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&translation_bytes),"archive_sha256":sha(&rebuilt),"table_sha256":sha(table),"records":records,"state":"development_art_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
