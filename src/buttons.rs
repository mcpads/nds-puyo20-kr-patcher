use crate::{archive, assets::json_file, compress, format::*};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn rgb(p: &[u8], i: usize) -> Result<[i32; 3]> {
    let c = u16le(p, i * 2)?;
    Ok([
        (c & 31) as i32,
        ((c >> 5) & 31) as i32,
        ((c >> 10) & 31) as i32,
    ])
}
pub(crate) fn nearest(p: &[u8], target: [i32; 3]) -> Result<usize> {
    let mut best = (i32::MAX, 0);
    for i in 0..p.len() / 2 {
        let c = rgb(p, i)?;
        let d = (0..3).map(|j| (c[j] - target[j]).pow(2)).sum();
        if d < best.0 {
            best = (d, i);
        }
    }
    Ok(best.1)
}
pub(crate) fn png(path: &Path, pixels: &[u8], palette: &[u8]) -> Result<()> {
    let mut rgba = Vec::new();
    for v in pixels {
        let c = rgb(palette, (v & 31) as usize)?;
        rgba.extend([
            ((c[0] * 255) / 31) as u8,
            ((c[1] * 255) / 31) as u8,
            ((c[2] * 255) / 31) as u8,
            ((v >> 5) as u16 * 255 / 7) as u8,
        ]);
    }
    let mut enc = png::Encoder::new(fs::File::create(path)?, 128, (pixels.len() / 128) as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    surface: String,
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    id: usize,
    korean: String,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let translation_bytes = fs::read(translation)?;
    let labels: Labels = serde_json::from_slice(&translation_bytes)?;
    ensure!(
        labels.state == "development_art_draft",
        "unsupported artwork state"
    );
    let (path, table_path, population, ids, stored_rows) = match labels.surface.as_str() {
        "main_menu" => (
            "menu/main_menu.narc",
            "menu/mainmenu_b_texlist.bin",
            128,
            vec![0, 1, 3, 4],
            35,
        ),
        "option_menu" => (
            "option/option_menu.narc",
            "option/option_menu_b_texlist.bin",
            34,
            vec![0, 1],
            34,
        ),
        "single_menu" => (
            "menu/single_menu.narc",
            "menu/single_menu_b_texlist.bin",
            12,
            vec![0, 1, 2, 3, 4],
            35,
        ),
        "free_menu" => (
            "menu/free_menu.narc",
            "menu/single_free_b_texlist.bin",
            10,
            (0..4).collect(),
            35,
        ),
        "endless_menu" => (
            "menu/thoroughly_menu.narc",
            "menu/single_tokoton_b_texlist.bin",
            19,
            (1..8).collect(),
            35,
        ),
        "rule_select" => (
            "menu/select_rule.narc",
            "menu/select_rule_b_texlist.bin",
            68,
            (2..24).collect(),
            35,
        ),
        "multi_menu" => (
            "menu/multi_menu.narc",
            "menu/multi_menu_b_texlist.bin",
            9,
            vec![0, 1],
            35,
        ),
        "multi_raise_scale" => (
            "menu/multi_raise_scale.narc",
            "menu/multi_invite00_b_texlist.bin",
            11,
            vec![0, 1, 2],
            35,
        ),
        "wifi_menu" => (
            "menu/wifi_menu.narc",
            "menu/wifi_menu_b_texlist.bin",
            11,
            vec![0, 1, 2],
            35,
        ),
        "wifi_friend_menu" => (
            "menu/wifi_friend_menu.narc",
            "menu/wifi_friend_b_texlist.bin",
            8,
            vec![0, 1, 2],
            35,
        ),
        _ => anyhow::bail!("unsupported button surface"),
    };
    ensure!(
        labels.entries.iter().map(|e| e.id).collect::<Vec<_>>() == ids,
        "unexpected button IDs"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let entry = rom.file(path)?;
    let source = rom.data(entry);
    let n = Narc::parse(source)?;
    ensure!(
        n.members.len() == population,
        "unexpected archive population"
    );
    let table = rom.data(rom.file(table_path)?);
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut outputs = Vec::new();
    for label in labels.entries {
        let id = label.id;
        let text = label.korean;
        // Wi-Fi lettering remains source artwork; only the Japanese suffix is edited.
        let (left, text_start) = match (labels.surface.as_str(), id) {
            ("wifi_menu", 0) => (56, Some(60)),
            ("wifi_menu", 2) => (69, Some(72)),
            _ => (10, None),
        };
        ensure!(!text.trim().is_empty(), "empty button text");
        let row = slice(table, id * 12, 12)?;
        let member = u16le(row, 0)?;
        let palette_member = u16le(row, 2)?;
        ensure!(
            u16le(row, 4)? == 1
                && u16le(row, 6)? == 0
                && u16le(row, 8)? == 128
                && u16le(row, 10)? == 34,
            "unsupported button geometry"
        );
        let palette = n.members[palette_member];
        let old = unpack_halfword(n.members[member])?;
        ensure!(
            old.len() == 128 * stored_rows && old[4352..].iter().all(|v| *v == 0),
            "unexpected texture extent"
        );
        let mut clean = old.clone();
        let high = rgb(palette, (old[7 * 128 + 64] & 31) as usize)?;
        let low = rgb(palette, (old[27 * 128 + 64] & 31) as usize)?;
        // Adopted development background: central gradient only. Keep the entire outer
        // border, original alpha, palette and extra transparent row byte-identical.
        for y in 8..26 {
            let target =
                std::array::from_fn(|c| (high[c] * (26 - y) as i32 + low[c] * (y - 7) as i32) / 19);
            let index = nearest(palette, target)? as u8;
            for x in left..118 {
                let p = y * 128 + x;
                clean[p] = (old[p] & 224) | index;
            }
        }
        let mut changed = clean.clone();
        let width = text
            .chars()
            .map(|c| {
                if c == ' ' {
                    5
                } else {
                    font.metrics(c, 12.0).advance_width.round() as usize
                }
            })
            .sum::<usize>();
        ensure!(width <= 104, "button text too wide");
        let mut cursor = text_start.unwrap_or((128 - width) / 2);
        let mut ink = Vec::new();
        for c in text.chars() {
            ensure!(font.lookup_glyph_index(c) != 0, "missing glyph {c}");
            let (m, b) = font.rasterize(c, 12.0);
            for y in 0..m.height {
                for x in 0..m.width {
                    if b[y * m.width + x] < 128 {
                        continue;
                    }
                    let px = cursor as i32 + m.xmin + x as i32;
                    let py = 23 - m.ymin - m.height as i32 + y as i32;
                    ensure!(
                        ((left + 1) as i32..116).contains(&px) && (9..25).contains(&py),
                        "button glyph outside editable region"
                    );
                    ink.push((px as usize, py as usize));
                }
            }
            cursor += if c == ' ' {
                5
            } else {
                m.advance_width.round() as usize
            };
        }
        let white = nearest(palette, [31, 31, 31])? as u8;
        let dark = nearest(palette, [0, 0, 0])? as u8;
        for &(x, y) in &ink {
            for (dy, dx) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                let p = (y as isize + dy) as usize * 128 + (x as isize + dx) as usize;
                changed[p] = (old[p] & 224) | dark;
            }
        }
        for &(x, y) in &ink {
            let p = y * 128 + x;
            changed[p] = (old[p] & 224) | white;
        }
        for (p, (&a, &b)) in old.iter().zip(&changed).enumerate() {
            ensure!(a & 224 == b & 224, "alpha changed");
            if !(left..118).contains(&(p % 128)) || !(8..26).contains(&(p / 128)) {
                ensure!(a == b, "protected pixel changed");
            }
        }
        let packed = compress::pack(&changed)?;
        ensure!(
            unpack_halfword(&packed)? == changed,
            "texture round trip mismatch"
        );
        records.push(json!({"id":id,"member":member,"palette_member":palette_member,"text":text,"editable_rectangle":[left,8,118,26],"source_sha256":sha(&old),"clean_sha256":sha(&clean),"pixels_sha256":sha(&changed),"stored_size":packed.len(),"capacity":n.members[member].len(),"palette_sha256":sha(palette),"line_width":width}));
        replacements.insert(member, packed);
        outputs.push((id, clean, changed, palette));
    }
    let rebuilt = archive::replace(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("buttons.narc"), &rebuilt)?;
    for (id, clean, changed, palette) in outputs {
        png(&out.join(format!("{id}-clean.png")), &clean, palette)?;
        png(&out.join(format!("{id}-korean.png")), &changed, palette)?;
        fs::write(out.join(format!("{id}-pixels.bin")), changed)?;
    }
    let plan = json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"buttons.narc","input_sha256":sha(&rebuilt)}]});
    json_file(&out.join("plan.json"), &plan)?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&translation_bytes),"archive":path,"table_sha256":sha(table),"archive_sha256":sha(&rebuilt),"buttons":records,"state":"development_art_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
