use crate::{assets::json_file, format::*, graphics::write_png, screens};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

const BACKGROUNDS: [(&str, &str, usize); 3] = [
    ("rule_select", "menu/select_rule.narc", 3),
    ("rule_edit_a", "rule_edit/rule_edit.narc", 3),
    ("rule_edit_b", "rule_edit/rule_edit.narc", 6),
];

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    fs::create_dir_all(out)?;
    let mut records = Vec::new();
    for (name, path, first) in BACKGROUNDS {
        let source = rom.data(rom.file(path)?);
        let narc = Narc::parse(source)?;
        let map = unpack(narc.members[first])?;
        let tiles = unpack(narc.members[first + 1])?;
        let palette = unpack(narc.members[first + 2])?;
        ensure!(
            palette.len() % 32 == 0 && palette.len() <= 512,
            "invalid palette banks"
        );
        let pixels = screens::render(&map, &tiles, palette.len() / 32)?;
        write_png(
            &out.join(format!("{name}.png")),
            256,
            192,
            &screens::rgba(&pixels, &palette)?,
        )?;
        for (suffix, data) in [
            ("map", &map),
            ("tiles", &tiles),
            ("palette", &palette),
            ("pixels", &pixels),
        ] {
            fs::write(out.join(format!("{name}-{suffix}.bin")), data)?;
        }
        records.push(json!({"name":name,"archive":path,"archive_sha256":sha(source),"members":[first,first+1,first+2],"map_sha256":sha(&map),"tiles_sha256":sha(&tiles),"palette_sha256":sha(&palette),"pixels_sha256":sha(&pixels),"banks":palette.len()/32,"tiles":tiles.len()/32}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"backgrounds":records,"claim":"4bpp BG map interpretation; index zero opaque in previews; selection/runtime unproven"});
    json_file(&out.join("panels.json"), &report)?;
    Ok(report)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    edit_title: Label,
    selection_notice: Label,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    source_pixels_sha256: String,
    japanese: String,
    korean: String,
}

fn title_region(x: usize, y: usize) -> bool {
    x < 144 && (16..80).contains(&y)
}
fn notice_region(x: usize, y: usize) -> bool {
    (12..119).contains(&x) && (3..21).contains(&y)
}

fn text_width(font: &fontdue::Font, text: &str) -> usize {
    text.chars()
        .map(|c| {
            if c == ' ' {
                5
            } else {
                font.metrics(c, 12.0).advance_width.round() as usize
            }
        })
        .sum()
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    use std::collections::BTreeMap;
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft",
        "unexpected panel draft state"
    );
    for label in [&tr.edit_title, &tr.selection_notice] {
        ensure!(
            !label.korean.trim().is_empty() && !label.japanese.is_empty(),
            "empty panel caption"
        );
    }
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut plans = Vec::new();
    let mut records = Vec::new();
    let mut outputs = Vec::new();

    let edit_path = "rule_edit/rule_edit.narc";
    let source = rom.data(rom.file(edit_path)?);
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 40, "rule edit population changed");
    let map = unpack(narc.members[6])?;
    let tiles = unpack(narc.members[7])?;
    let palette = unpack(narc.members[8])?;
    ensure!(
        palette.len() == 384 && tiles.len() == 16384,
        "rule title geometry changed"
    );
    let pixels = screens::render(&map, &tiles, 12)?;
    ensure!(
        sha(&pixels) == tr.edit_title.source_pixels_sha256,
        "rule title source mismatch"
    );
    let s = screens::Screen {
        map,
        tiles,
        palette,
        pixels,
    };
    let colors = screens::colors(&s.palette)?;
    let mut desired: Vec<_> = s.pixels.iter().map(|&v| colors[v as usize]).collect();
    // The old title's whole rectangle is artwork input, including its background.
    // A double-size pixel-font title replaces the Japanese word art.
    let mut small = vec![false; 256 * 192];
    screens::text_ink(&font, &tr.edit_title.korean, 0, 12, 64, &mut small)?;
    let title_x = (144 - text_width(&font, &tr.edit_title.korean) * 2) / 2;
    for y in 16..80 {
        for x in 0..144 {
            desired[y * 256 + x] = if !(2..142).contains(&x) || !(18..78).contains(&y) {
                [8, 27, 26]
            } else {
                [0, 12, 12]
            };
        }
    }
    for (p, &on) in small.iter().enumerate() {
        if !on {
            continue;
        }
        for dy in 0..2 {
            for dx in 0..2 {
                let x = title_x + p % 256 * 2 + dx;
                let y = 36 + p / 256 * 2 + dy;
                ensure!(title_region(x, y), "rule title ink outside region");
                desired[y * 256 + x] = [31, 31, 31];
            }
        }
    }
    let encoded = screens::encode_screen(&s, title_region, &desired)?;
    ensure!(
        screens::render(&encoded.map, &encoded.tiles, 12)? == encoded.pixels,
        "rule title map round trip failed"
    );
    let mut changes = BTreeMap::new();
    for (id, raw) in [(6, &encoded.map), (7, &encoded.tiles)] {
        let packed = crate::compress::pack(raw)?;
        ensure!(
            unpack_halfword(&packed)? == *raw,
            "rule title halfword round trip failed"
        );
        changes.insert(id, packed);
    }
    let rebuilt = crate::archive::replace(source, &changes)?;
    let check = Narc::parse(&rebuilt)?;
    ensure!(
        screens::render(&unpack(check.members[6])?, &unpack(check.members[7])?, 12)?
            == encoded.pixels
            && unpack(check.members[8])? == s.palette,
        "rebuilt title differs"
    );
    plans.push(json!({"file":edit_path,"expected_sha256":sha(source),"input":"rule-edit.narc","input_sha256":sha(&rebuilt)}));
    outputs.push(("rule-edit.narc", rebuilt));
    records.push(json!({"surface":"rule_edit_title","japanese":tr.edit_title.japanese,"korean":tr.edit_title.korean,"members":[6,7],"source_pixels_sha256":sha(&s.pixels),"pixels_sha256":sha(&encoded.pixels),"palette_sha256":sha(&s.palette),"tile_count":encoded.tile_count,"editable_rectangle":[0,16,144,80],"stored_sizes":[changes[&6].len(),changes[&7].len()],"capacities":[narc.members[6].len(),narc.members[7].len()]}));

    let select_path = "menu/select_rule.narc";
    let source = rom.data(rom.file(select_path)?);
    let narc = Narc::parse(source)?;
    let table = rom.data(rom.file("menu/select_rule_b_texlist.bin")?);
    ensure!(
        narc.members.len() == 68
            && table.len() == 300
            && u16le(table, 0)? == 67
            && u16le(table, 2)? == 66
            && u16le(table, 4)? == 1
            && u16le(table, 6)? == 0
            && u16le(table, 8)? == 128
            && u16le(table, 10)? == 32,
        "selection notice mapping changed"
    );
    let notice_palette = unpack(narc.members[66])?;
    let old = unpack_halfword(narc.members[67])?;
    ensure!(
        old.len() == 4096
            && notice_palette.len() == 32
            && sha(&old) == tr.selection_notice.source_pixels_sha256,
        "selection notice identity mismatch"
    );
    let mut changed = old.clone();
    let background = crate::buttons::nearest(&notice_palette, [0, 24, 30])? as u8;
    let white = crate::buttons::nearest(&notice_palette, [31, 31, 31])? as u8;
    let dark = crate::buttons::nearest(&notice_palette, [0, 5, 15])? as u8;
    let mut notice_ink = vec![false; 256 * 192];
    ensure!(
        text_width(&font, &tr.selection_notice.korean) <= 102,
        "notice line too wide"
    );
    screens::text_ink(
        &font,
        &tr.selection_notice.korean,
        (128 - text_width(&font, &tr.selection_notice.korean)) / 2,
        18,
        102,
        &mut notice_ink,
    )?;
    for y in 0..32 {
        for x in 0..128 {
            let p = y * 128 + x;
            if notice_region(x, y) {
                changed[p] = (old[p] & 224) | background;
            }
        }
    }
    for y in 0..32 {
        for x in 0..128 {
            if !notice_ink[y * 256 + x] {
                continue;
            }
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (0, 0)] {
                let px = (x as i32 + dx) as usize;
                let py = (y as i32 + dy) as usize;
                ensure!(notice_region(px, py), "notice ink outside region");
                let p = py * 128 + px;
                changed[p] = (old[p] & 224) | dark;
            }
        }
    }
    for y in 0..32 {
        for x in 0..128 {
            if notice_ink[y * 256 + x] {
                let p = y * 128 + x;
                changed[p] = (old[p] & 224) | white;
            }
        }
    }
    for (p, (&a, &b)) in old.iter().zip(&changed).enumerate() {
        ensure!(a & 224 == b & 224, "notice alpha changed");
        if !notice_region(p % 128, p / 128) {
            ensure!(a == b, "protected notice pixel changed");
        }
    }
    let packed = crate::compress::pack(&changed)?;
    ensure!(
        unpack_halfword(&packed)? == changed,
        "notice halfword round trip failed"
    );
    let stored_size = packed.len();
    let rebuilt = crate::archive::replace(source, &BTreeMap::from([(67, packed)]))?;
    let check = Narc::parse(&rebuilt)?;
    ensure!(
        unpack_halfword(check.members[67])? == changed
            && unpack(check.members[66])? == notice_palette,
        "rebuilt notice differs"
    );
    plans.push(json!({"file":select_path,"expected_sha256":sha(source),"input":"rule-select.narc","input_sha256":sha(&rebuilt)}));
    outputs.push(("rule-select.narc", rebuilt));
    records.push(json!({"surface":"selection_notice","japanese":tr.selection_notice.japanese,"korean":tr.selection_notice.korean,"table_sha256":sha(table),"member":67,"source_pixels_sha256":sha(&old),"pixels_sha256":sha(&changed),"palette_sha256":sha(&notice_palette),"editable_rectangle":[12,3,119,21],"alpha_preserved":true,"stored_size":stored_size,"capacity":narc.members[67].len()}));

    fs::create_dir_all(out)?;
    for (name, data) in outputs {
        fs::write(out.join(name), data)?;
    }
    write_png(
        &out.join("rule-edit-korean.png"),
        256,
        192,
        &screens::rgba(&encoded.pixels, &s.palette)?,
    )?;
    crate::buttons::png(
        &out.join("selection-notice-korean.png"),
        &changed,
        &notice_palette,
    )?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":plans}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"records":records,"state":"development_art_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
