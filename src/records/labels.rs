use super::ARCHIVE;
use crate::{archive, assets::json_file, buttons::rgb, compress, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    table: usize,
    crop_id: usize,
    source: String,
    source_crop_sha256: String,
    korean: Vec<String>,
    font_size: usize,
    #[serde(default)]
    font: Option<String>,
    #[serde(default)]
    letter_spacing: i32,
    /// `source-left`/`source-right` keep the source ink's left or right edge;
    /// default centres the Korean ink in the crop.
    #[serde(default)]
    align: Option<String>,
    /// Source-style backing plate behind the fill instead of a thin outline.
    #[serde(default)]
    backing: Option<Backing>,
    /// Thicken vertical strokes where a one-pixel gap remains (`labels::bold_vertical_runs`).
    #[serde(default)]
    bold_keep_gap: bool,
    /// Source palette index for the glyph face instead of the crop's brightest colour.
    #[serde(default)]
    fill_index: Option<usize>,
}
/// Layered source lettering: fill on a dilated backing, an offset shadow and
/// a one-pixel rim, each with an explicit source palette index.
#[derive(Deserialize, serde::Serialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct Backing {
    backing_index: usize,
    shadow_index: usize,
    #[serde(default)]
    rim_index: Option<usize>,
    radius: i32,
    shadow: [i32; 2],
}
#[derive(Clone, Copy)]
enum Anchor {
    Center,
    Left(usize),
    Right(usize),
}
struct Texture {
    width: usize,
    height: usize,
    format: usize,
    palette: Vec<u8>,
    original: Vec<u8>,
    pixels: Vec<u8>,
    editable: Vec<bool>,
}

fn render(
    font: &fontdue::Font,
    lines: &[String],
    size: usize,
    width: usize,
    height: usize,
    colors: [u8; 3],
    letter_spacing: i32,
    latin_font: Option<&fontdue::Font>,
    anchor: Anchor,
    backing: Option<([u8; 3], &Backing)>,
    bold: bool,
) -> Result<Vec<u8>> {
    ensure!(
        !lines.is_empty() && lines.len() <= 2 && (lines.len() == 1 || size == 8),
        "unsupported label line count"
    );
    let block = lines.len() * size + lines.len() - 1;
    ensure!(block + 2 <= height, "record label block too tall");
    // Horizontal room taken by the layers around the ink on each side.
    let (pad_left, pad_right) = match backing {
        Some((_, b)) => (
            (b.radius + i32::from(b.rim_index.is_some()) - b.shadow[0].min(0)) as usize,
            (b.radius + i32::from(b.rim_index.is_some()) + b.shadow[0].max(0)) as usize,
        ),
        None => (1, 1),
    };
    let mut ink = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        ensure!(!text.trim().is_empty(), "empty record label");
        let mut cursor = 0;
        let start = ink.len();
        let baseline = (height - block) / 2 + line * (size + 1) + size - 1;
        for c in text.chars() {
            let glyph_font = if c.is_ascii_alphabetic() {
                latin_font.unwrap_or(font)
            } else {
                font
            };
            ensure!(
                glyph_font.lookup_glyph_index(c) != 0,
                "missing record label glyph: {c}"
            );
            let (m, b) = glyph_font.rasterize(c, size as f32);
            let before = ink.len();
            for y in 0..m.height {
                for x in 0..m.width {
                    if b[y * m.width + x] < 128 {
                        continue;
                    }
                    let px = cursor + m.xmin + x as i32;
                    let py = baseline as i32 - m.ymin - m.height as i32 + y as i32;
                    ensure!(
                        px >= 0 && py >= 1 && py < height as i32 - 1,
                        "record label ink outside crop: {text} ({px},{py})"
                    );
                    ink.push((px as usize, py as usize));
                }
            }
            ensure!(
                c == ' ' || ink.len() > before,
                "empty record label glyph: {c}"
            );
            cursor += m.advance_width.round() as i32 + letter_spacing
                - i32::from(latin_font.is_some() && c == ' ');
        }
        let left = ink[start..]
            .iter()
            .map(|p| p.0)
            .min()
            .ok_or_else(|| anyhow::anyhow!("empty record line"))?;
        let right = ink[start..].iter().map(|p| p.0).max().unwrap() + 1;
        let extent = right - left;
        ensure!(
            extent + pad_left + pad_right <= width,
            "record label ink too wide: {text}, {extent}/{width}"
        );
        let margin = match anchor {
            Anchor::Center => (width - extent) / 2,
            Anchor::Left(x) => x.max(pad_left),
            Anchor::Right(x) => x.min(width - pad_right).saturating_sub(extent),
        };
        ensure!(
            margin >= pad_left && margin + extent + pad_right <= width,
            "record label anchor leaves crop: {text}"
        );
        for point in &mut ink[start..] {
            point.0 = point.0 - left + margin;
        }
    }
    if bold {
        let set: BTreeSet<(i32, i32)> = ink.iter().map(|&(x, y)| (x as i32, y as i32)).collect();
        for (x, y) in crate::labels::bold_vertical_runs(&set) {
            ensure!(
                x >= 1 && (x as usize) + 1 < width,
                "record bold stroke leaves crop"
            );
            ink.push((x as usize, y as usize));
        }
    }
    let mut pixels = vec![colors[0]; width * height];
    if let Some((layer, b)) = backing {
        ensure!(
            (1..=3).contains(&b.radius) && b.shadow.iter().all(|v| (-2..=2).contains(v)),
            "record backing geometry"
        );
        let r = b.radius;
        let mut plate = BTreeSet::new();
        for &(x, y) in &ink {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r + 1 {
                        plate.insert((x as i32 + dx, y as i32 + dy));
                    }
                }
            }
        }
        let shadow: BTreeSet<_> = plate
            .iter()
            .map(|&(x, y)| (x + b.shadow[0], y + b.shadow[1]))
            .filter(|p| !plate.contains(p))
            .collect();
        let mut rim = BTreeSet::new();
        for &(x, y) in plate
            .iter()
            .chain(&shadow)
            .filter(|_| b.rim_index.is_some())
        {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let p = (x + dx, y + dy);
                if !plate.contains(&p) && !shadow.contains(&p) {
                    rim.insert(p);
                }
            }
        }
        for (set, colour) in [(&rim, layer[0]), (&shadow, layer[2]), (&plate, layer[1])] {
            for &(x, y) in set {
                ensure!(
                    x >= 0 && y >= 0 && x < width as i32 && y < height as i32,
                    "record backing outside crop ({x},{y})"
                );
                pixels[y as usize * width + x as usize] = colour;
            }
        }
    } else {
        for &(x, y) in &ink {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                pixels[(y as i32 + dy) as usize * width + (x as i32 + dx) as usize] = colors[1];
            }
        }
    }
    for (x, y) in ink {
        pixels[y * width + x] = colors[2];
    }
    Ok(pixels)
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    small_font_path: &Path,
    narrow_font_path: Option<&Path>,
    medium_font_path: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(
        sha(source) == "27417db6a9a533ad988e9a37c6a0f87149339510da64a034a18e34535ecaa776",
        "record source changed"
    );
    let n = Narc::parse(source)?;
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 71,
        "record label population changed"
    );
    let expected: Vec<_> = (13..=71)
        .map(|i| (2, i))
        .chain((32..=43).map(|i| (3, i)))
        .collect();
    ensure!(
        tr.entries
            .iter()
            .map(|e| (e.table, e.crop_id))
            .collect::<Vec<_>>()
            == expected,
        "record label order/coverage changed"
    );
    let regular_bytes = fs::read(font_path)?;
    let small_bytes = fs::read(small_font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&regular_bytes)
            && sha(&small_bytes)
                == "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        "record label font identity changed"
    );
    let regular_sha256 = sha(&regular_bytes);
    let regular = fontdue::Font::from_bytes(regular_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let small = fontdue::Font::from_bytes(small_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let narrow = narrow_font_path
        .map(|path| -> Result<_> {
            let bytes = fs::read(path)?;
            ensure!(
                sha(&bytes) == "4589cb1a59bcbd669ad7ac0669827e5a4d411832048e9bdd9618c907c1a8d272",
                "record narrow font identity changed"
            );
            let hash = sha(&bytes);
            let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok((font, hash))
        })
        .transpose()?;
    // Galmuri11 Condensed: Galmuri11's 12px height with 8px Hangul, for lines
    // whose vowel ticks the DenkiChip face merges.
    let medium = medium_font_path
        .map(|path| -> Result<_> {
            let bytes = fs::read(path)?;
            ensure!(
                sha(&bytes) == "7b433b4a007c36dfb535fdea11de3e4f4c8b641ab591ed05d3bc0a4bbd75eb5f",
                "record condensed font identity changed"
            );
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;
    let mut textures = BTreeMap::<usize, Texture>::new();
    let mut records = Vec::new();
    for e in tr.entries {
        ensure!(
            !e.source.is_empty() && matches!(e.font_size, 8 | 12),
            "invalid record label input"
        );
        ensure!(
            matches!(e.font.as_deref(), None | Some("narrow") | Some("condensed"))
                && (-1..=0).contains(&e.letter_spacing)
                && (e.font.is_some() || e.letter_spacing == 0)
                && (e.font.is_none() || (e.font_size == 12 && e.korean.len() == 1)),
            "unsupported record font/spacing selection"
        );
        let font = if e.font.as_deref() == Some("condensed") {
            medium
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("record label requires --medium-font"))?
        } else if e.font.as_deref() == Some("narrow") {
            &narrow
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("record label requires --narrow-font"))?
                .0
        } else if e.font_size == 8 {
            &small
        } else {
            &regular
        };
        let table = rom.data(rom.file(&format!("record/score_menu{:02}_t_texlist.bin", e.table))?);
        let layouts: &[usize] = if e.table == 2 {
            &[11, 13, 14, 18, 20]
        } else {
            &[12, 17, 19]
        };
        let mut mapping = None;
        for &layout in layouts {
            let b = unpack(n.members[layout])?;
            let pos = 32 + u32le(&b, 80)? + e.crop_id * 20;
            let id = u32le(&b, pos)?;
            let row = slice(table, id * 12, 12)?;
            let member = u16le(row, 0)?;
            let width = u16le(row, 8)?;
            let height = u16le(row, 10)?;
            let mut rect = [0; 4];
            for (axis, v) in rect.iter_mut().enumerate() {
                let scale = if axis % 2 == 0 { width } else { height };
                let uv = u32le(&b, pos + 4 + axis * 4)?;
                ensure!(
                    uv <= 4096 && uv * scale % 4096 == 0,
                    "invalid record crop UV"
                );
                *v = uv * scale / 4096;
            }
            ensure!(rect[0] < rect[2] && rect[1] < rect[3], "empty record crop");
            let current = (member, rect);
            if let Some(previous) = mapping {
                ensure!(previous == current, "record shared crop changed");
            } else {
                mapping = Some(current);
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = textures.entry(member) {
                let format = u16le(row, 4)?;
                ensure!(matches!(format, 1 | 3), "unsupported record label format");
                let raw = unpack_halfword(n.members[member])?;
                let pixels: Vec<_> = if format == 3 {
                    raw.iter().flat_map(|v| [v & 15, v >> 4]).collect()
                } else {
                    raw.clone()
                };
                ensure!(
                    pixels.len() == width * height,
                    "record label texture extent changed"
                );
                let repacked: Vec<_> = if format == 3 {
                    pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect()
                } else {
                    pixels.clone()
                };
                ensure!(raw == repacked, "record source round trip failed");
                entry.insert(Texture {
                    width,
                    height,
                    format,
                    palette: unpack(n.members[u16le(row, 2)?])?,
                    editable: vec![false; pixels.len()],
                    original: pixels.clone(),
                    pixels,
                });
            }
        }
        let (member, [x0, y0, x1, y1]) = mapping.unwrap();
        let t = textures.get_mut(&member).unwrap();
        let crop: Vec<u8> = (y0..y1)
            .flat_map(|y| {
                t.original[y * t.width + x0..y * t.width + x1]
                    .iter()
                    .copied()
            })
            .collect();
        ensure!(
            sha(&crop) == e.source_crop_sha256,
            "record label source crop hash changed"
        );
        let used: BTreeSet<usize> = crop
            .iter()
            .filter(|&&p| if t.format == 3 { p != 1 } else { p >> 5 != 0 })
            .map(|&p| {
                if t.format == 3 {
                    p as usize
                } else {
                    (p & 31) as usize
                }
            })
            .collect();
        ensure!(used.len() >= 2, "record label colors unresolved");
        let mut colors: Vec<_> = used
            .into_iter()
            .map(|i| Ok((rgb(&t.palette, i)?.iter().sum::<i32>(), i)))
            .collect::<Result<_>>()?;
        colors.sort_unstable();
        let alpha = if t.format == 3 { 0 } else { 224 };
        if let Some(i) = e.fill_index {
            ensure!(
                i < t.palette.len() / 2 && (t.format != 3 || i > 1),
                "record fill index outside palette"
            );
        }
        let colors = [
            if t.format == 3 { 1 } else { 0 },
            alpha | colors.first().unwrap().1 as u8,
            alpha | e.fill_index.unwrap_or(colors.last().unwrap().1) as u8,
        ];
        let source_ink: Vec<usize> = (0..y1 - y0)
            .flat_map(|y| (0..x1 - x0).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = crop[y * (x1 - x0) + x];
                if t.format == 3 {
                    p != 1 && p != 0
                } else {
                    p >> 5 != 0
                }
            })
            .map(|(x, _)| x)
            .collect();
        let anchor = match e.align.as_deref() {
            None => Anchor::Center,
            Some("source-left") => Anchor::Left(*source_ink.iter().min().unwrap()),
            Some("source-right") => Anchor::Right(*source_ink.iter().max().unwrap() + 1),
            Some(other) => anyhow::bail!("unknown record label alignment {other}"),
        };
        let layer = e
            .backing
            .map(|b| -> Result<[u8; 3]> {
                let n = t.palette.len() / 2;
                ensure!(
                    [
                        b.rim_index.unwrap_or(b.backing_index),
                        b.backing_index,
                        b.shadow_index
                    ]
                    .iter()
                    .all(|&i| i < n && (t.format != 3 || i > 1)),
                    "record backing index outside palette"
                );
                Ok([
                    alpha | b.rim_index.unwrap_or(b.backing_index) as u8,
                    alpha | b.backing_index as u8,
                    alpha | b.shadow_index as u8,
                ])
            })
            .transpose()?;
        let pixels = render(
            font,
            &e.korean,
            e.font_size,
            x1 - x0,
            y1 - y0,
            colors,
            e.letter_spacing,
            e.font.as_ref().map(|_| &regular),
            anchor,
            layer.zip(e.backing.as_ref()),
            e.bold_keep_gap,
        )?;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = y * t.width + x;
                ensure!(!t.editable[p], "overlapping record labels");
                t.editable[p] = true;
                t.pixels[p] = pixels[(y - y0) * (x1 - x0) + x - x0];
            }
        }
        records.push(json!({"table":e.table,"crop_id":e.crop_id,"member":member,"rect":[x0,y0,x1,y1],"source":e.source,"korean":e.korean,"font_size":e.font_size,"colors":colors,"pixels_sha256":sha(&pixels)}));
        if let Some(align) = &e.align {
            records.last_mut().unwrap()["align"] = json!(align);
        }
        if let Some(backing) = &e.backing {
            records.last_mut().unwrap()["backing"] = serde_json::to_value(backing)?;
        }
        if let Some(font) = e.font {
            let record = records.last_mut().unwrap();
            record["font"] = json!(font);
            record["letter_spacing"] = json!(e.letter_spacing);
            record["latin_font"] = json!("regular");
            record["space_adjustment"] = json!(-1);
        }
    }
    let mut replacements = BTreeMap::new();
    let mut members = Vec::new();
    let mut previews = Vec::new();
    for (&member, t) in &textures {
        ensure!(
            t.original
                .iter()
                .zip(&t.pixels)
                .zip(&t.editable)
                .all(|((&a, &b), &edit)| edit || a == b),
            "protected record pixels changed"
        );
        let raw: Vec<_> = if t.format == 3 {
            t.pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect()
        } else {
            t.pixels.clone()
        };
        let packed = compress::pack(&raw)?;
        ensure!(
            unpack_halfword(&packed)? == raw,
            "record label compression failed"
        );
        members.push(json!({"member":member,"raw_sha256":sha(&raw),"palette_sha256":sha(&t.palette),"protected_pixels":t.editable.iter().filter(|v|!**v).count(),"stored_size":packed.len(),"capacity":n.members[member].len()}));
        replacements.insert(member, packed);
        let mut rgba = Vec::new();
        for &p in &t.pixels {
            let c = rgb(
                &t.palette,
                if t.format == 3 {
                    p as usize
                } else {
                    (p & 31) as usize
                },
            )?;
            rgba.extend([
                (c[0] * 255 / 31) as u8,
                (c[1] * 255 / 31) as u8,
                (c[2] * 255 / 31) as u8,
                if t.format == 3 {
                    if p == 0 { 0 } else { 255 }
                } else {
                    ((p >> 5) as usize * 255 / 7) as u8
                },
            ]);
        }
        previews.push((member, t.width, t.height, rgba));
    }
    let rebuilt = archive::replace(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("record.narc"), &rebuilt)?;
    for (member, width, height, rgba) in previews {
        write_png(
            &out.join(format!("member-{member}.png")),
            width,
            height,
            &rgba,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"record.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let mut report = json!({"state":"development_art_draft","source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"regular_font_sha256":regular_sha256,"small_font_sha256":"1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396","rasterizer":"fontdue 0.9.4; native 12/8px; threshold 128; centered visible ink; complete four-neighbor outline","records":records,"members":members,"archive_sha256":sha(&rebuilt),"runtime_verified":false,"human_reviewed":false});
    if let Some((_, hash)) = narrow {
        report["narrow_font_sha256"] = json!(hash);
    }
    json_file(&out.join("labels.json"), &report)?;
    Ok(report)
}
