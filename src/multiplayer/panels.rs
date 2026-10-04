use super::*;
use crate::graphics::write_png;

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    prepare_for(rom, translation, font_path, out, false)
}

pub fn prepare_participation(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    prepare_for(rom, translation, font_path, out, true)
}

fn prepare_for(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    out: &Path,
    participation: bool,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = if participation {
        "menu/multi_participate.narc"
    } else {
        "menu/multi_raise.narc"
    };
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source)
            == if participation {
                "6967e601f9f06152060e69a263f5df6403ae901777ffd1012748f7ff364e8011"
            } else {
                "777b0fd31aed8a3b7ffbbbc4dd84cc7ca1fe4d293345662fc598c39d0daae3c4"
            },
        "recruitment archive changed"
    );
    let n = Narc::parse(source)?;
    let table = rom.data(rom.file(if participation {
        "menu/multi_join_b_texlist.bin"
    } else {
        "menu/multi_invite01_b_texlist.bin"
    })?);
    ensure!(
        sha(table)
            == if participation {
                "cfc40dcd99dde8596facb75b6f45e8ffe93789e6daffb06677073898b5ce482e"
            } else {
                "a39cda57667fb442758f26860b0d5347dde9574b2c90143b9587fc68c2d686d8"
            },
        "recruitment table changed"
    );
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.entries.len() == if participation { 4 } else { 2 },
        "panel population changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "panel font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    let specs: Vec<(&str, usize, usize, usize, [usize; 2])> = if participation {
        vec![
            (
                "へやを探しています\nしばらくおまちください",
                8,
                11,
                2,
                [22, 24],
            ),
            ("データ転送中…", 5, 13, 4, [10, 12]),
            ("通信に失敗しました", 6, 17, 8, [18, 20]),
            ("参加できませんでした", 7, 15, 6, [14, 16]),
        ]
    } else {
        vec![
            ("データ転送中…", 25, 12, 12, [28, 30]),
            ("通信に失敗しました", 26, 14, 14, [32, 34]),
        ]
    };
    for (entry, (japanese, layout_member, first_crop, first_texture, members)) in
        tr.entries.iter().zip(specs)
    {
        ensure!(entry.japanese == japanese, "panel source order changed");
        let layout = unpack(n.members[layout_member])?;
        ensure!(
            slice(&layout, 0, 4)? == b"DSIF"
                && u32le(&layout, 12)? == 32
                && slice(&layout, 32, 4)? == b"nCSC",
            "panel layout header changed"
        );
        ensure!(
            u32le(&layout, 76)? == if participation { 19 } else { 16 } && u32le(&layout, 92)? == 3,
            "panel layout population changed"
        );
        let bank = 32 + u32le(&layout, 88)?;
        ensure!(u32le(&layout, bank)? == 3, "panel node count changed");
        let nodes = 32 + u32le(&layout, bank + 4)?;
        let crops = 32 + u32le(&layout, 80)?;
        let lines: Vec<_> = entry.korean.split('\n').collect();
        let two_lines = japanese.contains('\n');
        ensure!(
            lines.len() == if two_lines { 2 } else { 1 },
            "panel line count changed"
        );
        let rect = if two_lines {
            [24, 12, 172, 50]
        } else if japanese == "参加できませんでした" {
            [24, 20, 172, 44]
        } else {
            [32, 20, 164, 44]
        };
        let mut mask = vec![0; 192 * 64];
        for (i, line) in lines.iter().enumerate() {
            let line_rect = if two_lines {
                [22, 12 + i * 19, 170, 32 + i * 19]
            } else {
                [rect[0] - 2, rect[1], rect[2] - 2, rect[3]]
            };
            let ink = battle_ui::text_ink(
                &font,
                line,
                12,
                line_rect,
                if two_lines { 27 + i * 19 } else { 36 },
            )?;
            // Original lettering has a thick dark rim and a lower-right shadow.
            // Keep the unchanged editable rectangle as the hard write boundary.
            for &(x, y) in &ink {
                for (sx, sy) in [(0, 0), (1, 1)] {
                    for dy in -2..=2 {
                        for dx in -2..=2 {
                            let px = x as i32 + dx + sx;
                            let py = y as i32 + dy + sy;
                            ensure!(
                                px >= rect[0] as i32
                                    && px < rect[2] as i32
                                    && py >= rect[1] as i32
                                    && py < rect[3] as i32,
                                "panel outline leaves editable rectangle"
                            );
                            mask[py as usize * 192 + px as usize] = 1;
                        }
                    }
                }
            }
            for &(x, y) in &ink {
                mask[y * 192 + x] = 2;
            }
        }
        let mut rgba = vec![0; 192 * 64 * 4];
        let mut parts = Vec::new();
        for part in 0..2 {
            let texture = first_texture + part;
            let crop_id = first_crop + part;
            let width = [128, 64][part];
            let x_offset = [0, 128][part];
            let crop = crops + crop_id * 20;
            ensure!(
                u32le(&layout, crop)? == texture,
                "panel crop texture changed"
            );
            for (axis, value) in [0, 0, 4096, 4096].iter().enumerate() {
                ensure!(
                    u32le(&layout, crop + 4 + axis * 4)? == *value,
                    "panel crop changed"
                );
            }
            let node = 32 + u32le(&layout, nodes + (part + 1) * 4)?;
            let params = 32 + u32le(&layout, node + 64)?;
            ensure!(
                u32le(&layout, params)? == crop_id,
                "panel node texture changed"
            );
            let left = u32le(&layout, node + 12)? as i32;
            let right = u32le(&layout, node + 28)? as i32;
            let x = u32le(&layout, params + 132)? as i32;
            ensure!(
                left == [-1024, -512][part]
                    && right == [1024, 512][part]
                    && x == [-512, 1024][part],
                "panel horizontal geometry changed"
            );
            ensure!(
                (x + left) / 16 + 96 == x_offset as i32 && right - left == width as i32 * 16,
                "panel join changed"
            );
            ensure!(
                u32le(&layout, node + 16)? as i32 == -683 && u32le(&layout, node + 24)? == 683,
                "panel vertical geometry changed"
            );
            ensure!(
                u32le(&layout, params + 136)? == 0
                    && u32le(&layout, params + 140)? == 0
                    && u32le(&layout, params + 144)? == 4096
                    && u32le(&layout, params + 148)? == 4096
                    && u32le(&layout, params + 152)? == 0,
                "panel transform changed"
            );
            let row = slice(table, texture * 12, 12)?;
            let member = u16le(row, 0)?;
            let palette_id = u16le(row, 2)?;
            ensure!(
                member == members[part]
                    && u16le(row, 4)? == 1
                    && u16le(row, 6)? == 0
                    && u16le(row, 8)? == width
                    && u16le(row, 10)? == 64,
                "panel texture changed"
            );
            let old = unpack_halfword(n.members[member])?;
            ensure!(old.len() == width * 64, "panel extent changed");
            let palette = n.members[palette_id];
            let bg = [228, 225][part];
            ensure!(
                (4..width - 4).all(|x| old[(if two_lines { 8 } else { 16 }) * width + x] == bg),
                "panel background reference changed"
            );
            let white = 224 | buttons::nearest(palette, [31, 31, 31])? as u8;
            let dark = 224 | buttons::nearest(palette, [0, 0, 0])? as u8;
            let mut pixels = old.clone();
            for y in 0..64 {
                for x in 0..width {
                    let p = y * width + x;
                    let global = y * 192 + x + x_offset;
                    if (rect[0]..rect[2]).contains(&(x + x_offset))
                        && (rect[1]..rect[3]).contains(&y)
                    {
                        ensure!(old[p] >> 5 == 7, "panel edit outside opaque interior");
                        pixels[p] = match mask[global] {
                            0 => bg,
                            1 => dark,
                            2 => white,
                            _ => unreachable!(),
                        };
                    } else {
                        ensure!(
                            mask[global] == 0 && pixels[p] == old[p],
                            "protected panel changed"
                        );
                    }
                    ensure!(pixels[p] >> 5 == old[p] >> 5, "panel alpha changed");
                    let c = buttons::rgb(palette, (pixels[p] & 31) as usize)?;
                    rgba[global * 4..global * 4 + 4].copy_from_slice(&[
                        (c[0] * 255 / 31) as u8,
                        (c[1] * 255 / 31) as u8,
                        (c[2] * 255 / 31) as u8,
                        ((pixels[p] >> 5) as u16 * 255 / 7) as u8,
                    ]);
                }
            }
            battle_ui::compress_member(&mut replacements, member, &pixels)?;
            parts.push(json!({"member":member,"palette_member":palette_id,"node":node,"params":params,"x":x_offset,"width":width,"source_sha256":sha(&old),"pixels_sha256":sha(&pixels),"stored_size":replacements[&member].len(),"capacity":n.members[member].len()}));
        }
        records.push(json!({"japanese":japanese,"korean":entry.korean,"layout_member":layout_member,"layout_sha256":sha(&layout),"rect":rect,"parts":parts,"font_size":12,"outline_pixels":2,"shadow_offset":[1,1],"line_pitch":19,"text_center_x":96}));
        previews.push(rgba);
    }
    let result = battle_ui::rebuilt(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("panels.narc"), &result)?;
    for (i, rgba) in previews.iter().enumerate() {
        write_png(&out.join(format!("{i}-korean.png")), 192, 64, rgba)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"panels.narc","input_sha256":sha(&result)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&result),"records":records,"runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("panels.json"), &report)?;
    Ok(report)
}
