use crate::{archive, assets::json_file, buttons::rgb, compress, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

use super::render::paint_cell;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Appreciation {
    state: String,
    labels: Vec<String>,
    chapters: Vec<String>,
    names: Vec<Vec<Name>>,
    #[serde(default)]
    name_font: Option<NameFont>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameFont {
    path: String,
    sha256: String,
    #[serde(default)]
    overflow_font: Option<FontFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontFile {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    source: String,
    korean: String,
    font_size: u8,
    #[serde(default)]
    artwork: Option<NameArtwork>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NameArtwork {
    image: String,
    image_sha256: String,
    image_region: [usize; 4],
}

fn paint_generated_name(
    art: &NameArtwork,
    row: usize,
    left: usize,
    preferred_center: usize,
    colors: (u8, u8),
    pixels: &mut [u8],
) -> Result<usize> {
    let bytes = fs::read(&art.image)?;
    ensure!(sha(&bytes) == art.image_sha256, "name artwork identity");
    let image = crate::art_pixels::region(&crate::art_pixels::read(&bytes)?, art.image_region)?;
    // Fit complete generated lettering to 12 visible rows. The reducer's two
    // padding rows are discarded, then the original atlas guard is restored.
    let w = 88 - left;
    let rgba = crate::art_pixels::reduce(&image, w, 16, true)?;
    let points: Vec<_> = rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, c)| c[3] >= 128)
        .map(|(i, c)| (i % w, i / w, c))
        .collect();
    ensure!(!points.is_empty(), "empty generated name");
    let x0 = points.iter().map(|p| p.0).min().unwrap();
    let x1 = points.iter().map(|p| p.0).max().unwrap() + 1;
    let width = x1 - x0;
    let center = preferred_center.clamp(left + width / 2, 88 - (width - width / 2));
    let target_x = center - width / 2;
    for (x, y, c) in points {
        ensure!((2..14).contains(&y), "generated name leaves guarded rows");
        let px = target_x + x - x0;
        ensure!((left..88).contains(&px), "generated name leaves cell");
        // Authored white face / navy outline become the existing state palette.
        let face = u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]) >= 500;
        pixels[(row * 14 + y) * 128 + px] = if face { colors.0 } else { colors.1 };
    }
    Ok(width)
}

fn paint_name(
    font: &fontdue::Font,
    use_generated_artwork: bool,
    name: &Name,
    row: usize,
    left: usize,
    preferred_center: usize,
    colors: (u8, u8),
    pixels: &mut [u8],
) -> Result<usize> {
    if use_generated_artwork {
        if let Some(art) = &name.artwork {
            return paint_generated_name(art, row, left, preferred_center, colors, pixels);
        }
        ensure!(
            matches!(name.korean.as_str(), "프롤로그" | "엑스트라" | "에필로그"),
            "character names require generated artwork or an explicit name-font profile"
        );
    }
    ensure!(
        !name.source.is_empty()
            && !name.korean.trim().is_empty()
            && matches!(name.font_size, 10 | 12),
        "invalid name cell"
    );
    // Use native pixel sizes; adjust placement, never rescale individual glyphs.
    let size = name.font_size as f32;
    // Original atlas rows are 14px apart; their two blank top rows are observed
    // padding, not a proven hardware sampling requirement.
    let baseline = 13;
    let width: usize = name
        .korean
        .chars()
        .map(|c| {
            if c == ' ' {
                5
            } else {
                font.metrics(c, size).advance_width.round() as usize
            }
        })
        .sum();
    ensure!(
        width + 2 <= 88 - left,
        "name too wide: {} ({width}px)",
        name.korean
    );
    let center = preferred_center.clamp(left + 1 + width / 2, 87 - (width - width / 2));
    let mut cursor = center - width / 2;
    let mut points = Vec::new();
    for c in name.korean.chars() {
        ensure!(font.lookup_glyph_index(c) != 0, "missing name glyph: {c}");
        let (m, b) = font.rasterize(c, size);
        for y in 0..m.height {
            for x in 0..m.width {
                if b[y * m.width + x] < 128 {
                    continue;
                }
                let px = cursor as i32 + m.xmin + x as i32;
                let py = baseline - m.ymin - m.height as i32 + y as i32;
                ensure!(
                    (left as i32 + 1..87).contains(&px),
                    "name ink outside width"
                );
                points.push((px as usize, py));
            }
        }
        cursor += if c == ' ' {
            5
        } else {
            m.advance_width.round() as usize
        };
    }
    let min_y = points
        .iter()
        .map(|p| p.1)
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty name"))?;
    let max_y = points.iter().map(|p| p.1).max().unwrap();
    ensure!(max_y - min_y < 11, "name ink exceeds 11 visible rows");
    // Preserve all ink at y=2..12, leaving y=13 for the lower contour.
    // Use y=1 for the upper contour and leave y=0 blank between atlas rows.
    let points: Vec<_> = points
        .into_iter()
        .map(|(x, y)| (x, row * 14 + (y - min_y + 2) as usize))
        .collect();
    for &(x, y) in &points {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let py = (y as isize + dy) as usize;
            if (row * 14 + 1..row * 14 + 14).contains(&py) {
                pixels[py * 128 + (x as isize + dx) as usize] = colors.1;
            }
        }
    }
    for (x, y) in points {
        pixels[y * 128 + x] = colors.0;
    }
    Ok(width)
}

pub fn prepare_appreciation(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    small_font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let translation_bytes = fs::read(translation)?;
    let tr: Appreciation = serde_json::from_slice(&translation_bytes)?;
    ensure!(
        tr.state == "development_art_draft" && tr.labels.len() == 5 && tr.chapters.len() == 8,
        "unexpected appreciation labels"
    );
    ensure!(
        tr.names.iter().map(Vec::len).collect::<Vec<_>>() == [9, 9, 9, 9, 8],
        "expected five name sheets with 44 cells"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let small_bytes = fs::read(small_font_path)?;
    ensure!(
        sha(&small_bytes) == "48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee",
        "small font identity mismatch"
    );
    let small_font = fontdue::Font::from_bytes(small_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let dedicated_font = tr
        .name_font
        .as_ref()
        .map(|spec| -> Result<fontdue::Font> {
            let bytes = fs::read(&spec.path)?;
            ensure!(sha(&bytes) == spec.sha256, "dedicated name font identity");
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;
    let overflow_font = tr
        .name_font
        .as_ref()
        .and_then(|spec| spec.overflow_font.as_ref())
        .map(|spec| -> Result<fontdue::Font> {
            let bytes = fs::read(&spec.path)?;
            ensure!(sha(&bytes) == spec.sha256, "overflow name font identity");
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;
    let path = "option/appreciate.narc";
    let source = rom.data(rom.file(path)?);
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 42, "unexpected appreciation archive");
    let table = rom.data(rom.file("option/appreciate_b_texlist.bin")?);
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut outputs = Vec::new();
    for (id, member) in [(7, 20), (13, 18)] {
        let row = slice(table, id * 12, 12)?;
        ensure!(
            u16le(row, 0)? == member
                && u16le(row, 2)? == member - 1
                && u16le(row, 4)? == 3
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == 128
                && u16le(row, 10)? == 128,
            "appreciation atlas mapping mismatch"
        );
        let old = unpack_halfword(narc.members[member])?;
        let palette = unpack(narc.members[member - 1])?;
        ensure!(
            old.len() == 8192 && palette.len() == 32,
            "unexpected I4 atlas extent"
        );
        let indices: Vec<_> = old.iter().flat_map(|v| [v & 15, v >> 4]).collect();
        let mut pixels = indices.clone();
        let mut widths = Vec::new();
        // Original ink centres: labels x=41.5 (not the cell centre 40), chapters x=103.5.
        for (left, right, center, strings) in
            [(0, 80, 42, &tr.labels), (80, 128, 104, &tr.chapters)]
        {
            for (line, text) in strings.iter().enumerate() {
                let (mut lightest, mut darkest) = ((i32::MAX, 0), (i32::MAX, 0));
                for y in line * 16..line * 16 + 16 {
                    for x in left..right {
                        let v = indices[y * 128 + x];
                        if v == 1 {
                            continue;
                        }
                        let c = rgb(&palette, v as usize)?;
                        let light = c.iter().map(|v| (31 - v).pow(2)).sum();
                        let dark = c.iter().map(|v| v.pow(2)).sum();
                        if light < lightest.0 {
                            lightest = (light, v);
                        }
                        if dark < darkest.0 {
                            darkest = (dark, v);
                        }
                    }
                    pixels[y * 128 + left..y * 128 + right].fill(1);
                }
                ensure!(
                    lightest.0 < i32::MAX && darkest.0 < i32::MAX,
                    "empty source label cell"
                );
                let before = pixels.clone();
                let width = paint_cell(
                    &font,
                    text,
                    center,
                    line,
                    lightest.1,
                    darkest.1,
                    &mut pixels,
                )?;
                for p in 0..pixels.len() {
                    if !(left..right).contains(&(p % 128))
                        || !(line * 16..line * 16 + 16).contains(&(p / 128))
                    {
                        ensure!(before[p] == pixels[p], "appreciation label exceeds cell");
                    }
                }
                widths.push(width);
            }
        }
        let mut rgba = Vec::new();
        for (p, (&before, &after)) in indices.iter().zip(&pixels).enumerate() {
            if p % 128 < 80 && p / 128 >= 80 {
                ensure!(before == after, "protected digits changed");
            }
            ensure!(after < 16, "I4 palette overflow");
            let c = rgb(&palette, after as usize)?;
            rgba.extend([
                (c[0] * 255 / 31) as u8,
                (c[1] * 255 / 31) as u8,
                (c[2] * 255 / 31) as u8,
                255,
            ]);
        }
        let packed_pixels: Vec<u8> = pixels.chunks_exact(2).map(|p| p[0] | (p[1] << 4)).collect();
        let packed = compress::pack(&packed_pixels)?;
        ensure!(
            unpack_halfword(&packed)? == packed_pixels,
            "I4 round trip mismatch"
        );
        records.push(json!({"id":id,"member":member,"palette_member":member-1,"widths":widths,"source_sha256":sha(&old),"pixels_sha256":sha(&packed_pixels),"palette_sha256":sha(&palette),"stored_size":packed.len(),"capacity":narc.members[member].len(),"editable_regions":[[0,0,80,80],[80,0,128,128]],"background_index":1}));
        replacements.insert(member, packed);
        outputs.push((id, packed_pixels, rgba));
    }
    for (sheet, names) in tr.names.iter().enumerate() {
        for (id, member) in [(8 + sheet, 24 + sheet * 4), (14 + sheet, 22 + sheet * 4)] {
            let row = slice(table, id * 12, 12)?;
            ensure!(
                u16le(row, 0)? == member
                    && u16le(row, 2)? == member - 1
                    && u16le(row, 4)? == 3
                    && u16le(row, 6)? == 0
                    && u16le(row, 8)? == 128
                    && u16le(row, 10)? == 128,
                "name atlas mapping mismatch"
            );
            let old = unpack_halfword(narc.members[member])?;
            let palette = unpack(narc.members[member - 1])?;
            ensure!(
                old.len() == 8192 && palette.len() == 32,
                "unexpected name atlas extent"
            );
            let indices: Vec<_> = old.iter().flat_map(|v| [v & 15, v >> 4]).collect();
            let mut pixels = indices.clone();
            let mut cells = Vec::new();
            for (line, name) in names.iter().enumerate() {
                let has_symbol = (sheet == 3 && line >= 3) || (sheet == 4 && line < 5);
                let active: Vec<_> = (0..88)
                    .map(|x| (line * 14..line * 14 + 14).any(|y| indices[y * 128 + x] != 1))
                    .collect();
                let start = active
                    .iter()
                    .position(|v| *v)
                    .ok_or_else(|| anyhow::anyhow!("empty source name"))?;
                let end = active.iter().rposition(|v| *v).unwrap() + 1;
                let mut left = 0;
                let mut preferred_center = 44;
                // (symbol start, JP gap after it, doubled JP group centre)
                let mut symbol = None;
                if has_symbol {
                    let mut symbol_end = start;
                    while symbol_end < 88 && active[symbol_end] {
                        symbol_end += 1;
                    }
                    ensure!(
                        symbol_end - start == 9 && symbol_end < end,
                        "unresolved name prefix symbol"
                    );
                    left = symbol_end + 1;
                    let text_start = active[left..]
                        .iter()
                        .position(|v| *v)
                        .ok_or_else(|| anyhow::anyhow!("missing name after symbol"))?
                        + left;
                    preferred_center = (text_start + end) / 2;
                    symbol = Some((start, text_start - symbol_end, start + end));
                }
                let clear_from = symbol.map_or(left, |(start, _, _)| start);
                let (mut lightest, mut darkest) = ((i32::MAX, 0), (i32::MAX, 0));
                for y in line * 14..line * 14 + 14 {
                    for x in left..88 {
                        let v = indices[y * 128 + x];
                        if v == 1 {
                            continue;
                        }
                        let c = rgb(&palette, v as usize)?;
                        let light = c.iter().map(|v| (31 - v).pow(2)).sum();
                        let dark = c.iter().map(|v| v.pow(2)).sum();
                        if light < lightest.0 {
                            lightest = (light, v);
                        }
                        if dark < darkest.0 {
                            darkest = (dark, v);
                        }
                    }
                    pixels[y * 128 + clear_from..y * 128 + 88].fill(1);
                }
                ensure!(
                    lightest.0 < i32::MAX && darkest.0 < i32::MAX,
                    "empty name ink"
                );
                let before = pixels.clone();
                let mut chosen_font = dedicated_font.as_ref().unwrap_or(&small_font);
                let measured_width: usize = name
                    .korean
                    .chars()
                    .map(|c| {
                        if c == ' ' {
                            5
                        } else {
                            chosen_font
                                .metrics(c, name.font_size as f32)
                                .advance_width
                                .round() as usize
                        }
                    })
                    .sum();
                let overflow_used = dedicated_font.is_some() && measured_width + 2 > 88 - left;
                if overflow_used {
                    chosen_font = overflow_font.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("dedicated name font exceeds width: {}", name.korean)
                    })?;
                }
                let ink_columns = |px: &[u8]| -> Option<(usize, usize)> {
                    let cols: Vec<_> = (0..88)
                        .filter(|&x| {
                            (line * 14..line * 14 + 14)
                                .any(|y| px[y * 128 + x] != before[y * 128 + x])
                        })
                        .collect();
                    Some((*cols.first()?, *cols.last()? + 1))
                };
                // The original draws the symbol just before the name and centres
                // the pair; move the symbol with the name instead of fixing it.
                let mut symbol_x = None;
                let mut edit_from = left;
                if let Some((start, gap, center2)) = symbol {
                    let mut trial = before.clone();
                    paint_name(
                        chosen_font,
                        dedicated_font.is_none(),
                        name,
                        line,
                        0,
                        44,
                        (lightest.1, darkest.1),
                        &mut trial,
                    )?;
                    let (a, b) =
                        ink_columns(&trial).ok_or_else(|| anyhow::anyhow!("empty name ink"))?;
                    let group = 9 + gap + b - a;
                    ensure!(center2 >= group + 2, "symbol and name exceed row");
                    let group_start = (center2 - group) / 2;
                    let text_left = group_start + 9 + gap;
                    preferred_center = 44 + text_left - a;
                    left = text_left;
                    edit_from = group_start.min(start);
                    symbol_x = Some((start, group_start));
                }
                let width = paint_name(
                    chosen_font,
                    dedicated_font.is_none(),
                    name,
                    line,
                    left,
                    preferred_center,
                    (lightest.1, darkest.1),
                    &mut pixels,
                )?;
                if let Some((start, group_start)) = symbol_x {
                    ensure!(
                        ink_columns(&pixels).map(|c| c.0) == Some(left),
                        "name did not follow its symbol"
                    );
                    for y in line * 14..line * 14 + 14 {
                        for dx in 0..9 {
                            let target = y * 128 + group_start + dx;
                            ensure!(pixels[target] == 1, "moved symbol overlaps name");
                            pixels[target] = indices[y * 128 + start + dx];
                        }
                    }
                }
                for p in 0..pixels.len() {
                    if !(edit_from..88).contains(&(p % 128))
                        || !(line * 14..line * 14 + 14).contains(&(p / 128))
                    {
                        ensure!(
                            before[p] == pixels[p],
                            "name overlaps symbol or adjacent cell"
                        );
                    }
                }
                cells.push(json!({"row":line,"source":name.source,"text":name.korean,"font_size":if dedicated_font.is_none() && name.artwork.is_some() { None } else { Some(name.font_size) },"overflow_font_used":overflow_used,"renderer":if dedicated_font.is_some() { "dedicated_font" } else if name.artwork.is_some() { "generated" } else { "section_font" },"artwork":if dedicated_font.is_none() { name.artwork.as_ref() } else { None },"width":width,"editable_region":[edit_from,line*14,88,line*14+14],"symbol_preserved":has_symbol,"symbol_x":symbol_x.map(|(from, to)| [from, to])}));
            }
            let mut rgba = Vec::new();
            for (p, (&before, &after)) in indices.iter().zip(&pixels).enumerate() {
                if p % 128 >= 88 || p / 128 >= names.len() * 14 {
                    ensure!(before == after, "protected name atlas area changed");
                }
                ensure!(after < 16, "name palette overflow");
                let c = rgb(&palette, after as usize)?;
                rgba.extend([
                    (c[0] * 255 / 31) as u8,
                    (c[1] * 255 / 31) as u8,
                    (c[2] * 255 / 31) as u8,
                    255,
                ]);
            }
            let packed_pixels: Vec<_> = pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect();
            let packed = compress::pack(&packed_pixels)?;
            ensure!(
                unpack_halfword(&packed)? == packed_pixels,
                "name round trip mismatch"
            );
            records.push(json!({"id":id,"member":member,"palette_member":member-1,"sheet":sheet,"cells":cells,"source_sha256":sha(&old),"pixels_sha256":sha(&packed_pixels),"palette_sha256":sha(&palette),"stored_size":packed.len(),"capacity":narc.members[member].len()}));
            ensure!(
                replacements.insert(member, packed).is_none(),
                "duplicate name member"
            );
            outputs.push((id, packed_pixels, rgba));
        }
    }
    let rebuilt = archive::replace(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("appreciate.narc"), &rebuilt)?;
    for (id, pixels, rgba) in outputs {
        fs::write(out.join(format!("{id:02}-pixels.bin")), pixels)?;
        write_png(&out.join(format!("{id:02}-korean.png")), 128, 128, &rgba)?;
        let mut enlarged = Vec::with_capacity(512 * 512 * 4);
        for y in 0..512 {
            for x in 0..512 {
                let i = ((y / 4) * 128 + x / 4) * 4;
                enlarged.extend_from_slice(&rgba[i..i + 4]);
            }
        }
        write_png(
            &out.join(format!("{id:02}-korean-4x.png")),
            512,
            512,
            &enlarged,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"appreciate.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&translation_bytes),"archive_sha256":sha(&rebuilt),"table_sha256":sha(table),"records":records,"state":"development_art_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
