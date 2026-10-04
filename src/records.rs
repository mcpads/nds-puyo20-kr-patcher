mod labels;
pub use labels::prepare as prepare_labels;

mod statistics;
pub use statistics::prepare as prepare_statistics;

mod captions;
pub use captions::inspect as inspect_captions;

use crate::{assets::json_file, battle_ui, format::*, graphics::write_png, screens, titles};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const ARCHIVE: &str = "record/record.narc";
const SCENES: [&str; 12] = [
    "btn_story",
    "btn_multi",
    "btn_wifi",
    "btn_clg_test",
    "btn_clg_compete",
    "btn_toko_compete",
    "btn_toko_puyo",
    "btn_toko_fever",
    "btn_toko_chibi",
    "btn_toko_deka",
    "btn_toko_nazo",
    "btn_toko_practice",
];
struct Button {
    member: usize,
    palette: Vec<u8>,
    raw: Vec<u8>,
    pixels: Vec<u8>,
}
fn read(rom: &Rom) -> Result<(Vec<Button>, Value)> {
    let n = Narc::parse(rom.data(rom.file(ARCHIVE)?))?;
    ensure!(n.members.len() == 246, "record archive population changed");
    let gem = unpack(n.members[229])?;
    let ilf = rom.data(rom.file("record/score_menu_b_ilf.bin")?);
    ensure!(
        sha(&gem) == "3f4d6742779912cd46b35c3fa4ac9eb12aff7fe3d5e83da31d89c0c7e776da91"
            && sha(ilf) == "14fce02c2ba8751d81c2ad36c0ddd9d19c4201f36baccf4fe1c889f9e868cec7",
        "record button layout changed"
    );
    let base = u32le(&gem, 20)?;
    let gem2 = u32le(&gem, 88)?;
    ensure!(
        slice(&gem, 0, 4)? == b"Gem1"
            && u32le(&gem, 8)? == gem.len()
            && slice(&gem, gem2, 4)? == b"Gem2"
            && gem2 + u32le(&gem, gem2 + 20)? == base
            && u32le(&gem, 40)? == 24
            && u32le(&gem, 80)? == 24,
        "record Gem container mismatch"
    );
    let mut p = u32le(&gem, 24)?;
    let mut names = Vec::new();
    for _ in 0..u32le(&gem, 32)? {
        ensure!(
            slice(&gem, base + u32le(&gem, p)?, 4)? == b"Scen",
            "record scene missing"
        );
        let len = slice(&gem, p + 4, 1)?[0] as usize;
        names.push(std::str::from_utf8(slice(&gem, p + 5, len)?)?.to_string());
        p = (p + 5 + len + 3) & !3;
    }
    ensure!(names == SCENES, "record scene names changed");
    let mut images = Vec::new();
    let mut rows = Vec::new();
    for i in 0..24 {
        let v = u32le(ilf, i * 4)?;
        let member = (v >> 8) & 4095;
        let pal = v >> 20;
        let expected = if i < 2 { 203 + i * 2 } else { 205 + i };
        let descriptor = base + u32le(&gem, 44)? + i * 32;
        let size = base + u32le(&gem, 84)? + i * 16;
        ensure!(
            member == expected
                && pal == if i % 2 == 0 { 204 } else { 206 }
                && u32le(&gem, descriptor)? == v & 255
                && u32le(&gem, descriptor + 4)? == 3
                && u16le(&gem, descriptor + 8)? == 64
                && u16le(&gem, descriptor + 10)? == 64
                && base + u32le(&gem, size + 4)? == descriptor
                && u32le(&gem, size + 8)? == 64 << 16
                && u32le(&gem, size + 12)? == 64 << 16,
            "record button mapping changed"
        );
        let raw = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[pal])?;
        ensure!(
            raw.len() == 2048 && palette.len() == 32,
            "record button extent changed"
        );
        let pixels = titles::untile(&raw, 64, 64, 4)?;
        ensure!(
            titles::tile(&pixels, 64, 64, 4)? == raw,
            "record button round trip failed"
        );
        rows.push(json!({"row":i,"logical_id":v&255,"scene":names[i/2],"state":if i%2==0 {"unselected"}else{"selected"},"member":member,"palette_member":pal,"raw_sha256":sha(&raw),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&palette)}));
        images.push(Button {
            member,
            palette,
            raw,
            pixels,
        });
    }
    Ok((
        images,
        json!({"gem_member":229,"gem_sha256":sha(&gem),"ilf_sha256":sha(ilf),"rows":rows}),
    ))
}
fn background(n: &Narc) -> Result<screens::Screen> {
    ensure!(n.members.len() == 246, "record archive population changed");
    let map = unpack_halfword(n.members[6])?;
    let tiles = unpack_halfword(n.members[7])?;
    let palette = unpack(n.members[8])?;
    ensure!(
        tiles.len() == 16384 && palette.len() == 384,
        "record title geometry changed"
    );
    let pixels = screens::render(&map, &tiles, 12)?;
    Ok(screens::Screen {
        map,
        tiles,
        palette,
        pixels,
    })
}
pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (buttons, mapping) = read(rom)?;
    let n = Narc::parse(rom.data(rom.file(ARCHIVE)?))?;
    let bg = background(&n)?;
    fs::create_dir_all(out)?;
    for (i, b) in buttons.iter().enumerate() {
        write_png(
            &out.join(format!("button-{i:02}.png")),
            64,
            64,
            &titles::rgba(&b.pixels, &b.palette)?,
        )?;
    }
    write_png(
        &out.join("title.png"),
        256,
        192,
        &screens::rgba(&bg.pixels, &bg.palette)?,
    )?;
    fs::write(out.join("title-pixels.bin"), &bg.pixels)?;
    let report = json!({"source_sha256":sha(rom.bytes),"mapping":mapping,"background":{"members":[6,7,8],"pixels_sha256":sha(&bg.pixels)},"claim":"target Gem/ILF mapping and source BG; consumer selection is separate"});
    json_file(&out.join("record.json"), &report)?;
    Ok(report)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    archive_sha256: String,
    title: Label,
    buttons: Vec<Caption>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    japanese: String,
    korean: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Caption {
    scene: String,
    japanese: String,
    korean_lines: Vec<String>,
    source_sha256: Vec<String>,
}
fn title_region(x: usize, y: usize) -> bool {
    x < 128 && y < 48
}
fn button_region(x: usize, y: usize) -> bool {
    (12..52).contains(&x) && (19..44).contains(&y)
}
pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    let source = rom.data(rom.file(ARCHIVE)?);
    let n = Narc::parse(source)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.archive_sha256 == sha(source)
            && tr.buttons.len() == 12
            && tr.title.japanese == "せいせき",
        "record draft population or source mismatch"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let (buttons, mapping) = read(rom)?;
    let mut changes = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    // Adopted central gradient: same index ramp for gold and green source palettes.
    // Border, shine above the rectangle, and transparent silhouette remain original.
    let gradient: [u8; 25] = [
        13, 13, 12, 11, 10, 7, 7, 4, 3, 3, 2, 2, 2, 2, 2, 3, 3, 5, 6, 8, 8, 9, 9, 9, 9,
    ];
    for (i, caption) in tr.buttons.iter().enumerate() {
        ensure!(
            caption.scene == SCENES[i]
                && !caption.japanese.is_empty()
                && caption.source_sha256.len() == 2,
            "record caption identity mismatch"
        );
        for state in 0..2 {
            let b = &buttons[i * 2 + state];
            // The single-line story image has no caption in the upper/lower
            // rows; its contour separates border ink from dark caption ink.
            let contour = &buttons[state].pixels;
            ensure!(
                sha(&b.raw) == caption.source_sha256[state],
                "record button source mismatch"
            );
            if i == 2 {
                ensure!(
                    caption.japanese == "Wi-Fi" && caption.korean_lines.is_empty(),
                    "Wi-Fi preservation changed"
                );
                continue;
            }
            ensure!(
                (1..=2).contains(&caption.korean_lines.len()),
                "record button line count"
            );
            let mut pixels = b.pixels.clone();
            for y in 19..44 {
                for x in 12..52 {
                    let p = y * 64 + x;
                    if contour[p] > 1 {
                        ensure!(b.pixels[p] != 0, "record contour crosses transparency");
                        pixels[p] = gradient[y - 19];
                    }
                }
            }
            let mut ink = Vec::new();
            for (line, text) in caption.korean_lines.iter().enumerate() {
                let size = if text.chars().count() > 3 {
                    9
                } else if caption.korean_lines.len() == 1 {
                    11
                } else {
                    10
                };
                let baseline = if caption.korean_lines.len() == 1 {
                    36
                } else {
                    30 + line * 10
                };
                ink.extend(battle_ui::text_ink(
                    &font,
                    text,
                    size,
                    [12, 18, 52, 44],
                    baseline,
                )?);
            }
            for &(x, y) in &ink {
                ensure!(
                    contour[y * 64 + x] > 1,
                    "record glyph reaches button border: {} state {} at {},{} index {}",
                    caption.scene,
                    state,
                    x,
                    y,
                    b.pixels[y * 64 + x]
                );
            }
            battle_ui::paint(&mut pixels, 64, &ink, 1, 15);
            ensure!(
                pixels.iter().enumerate().all(|(p, &v)| {
                    if button_region(p % 64, p / 64) && contour[p] > 1 {
                        v != 0
                    } else {
                        v == b.pixels[p]
                    }
                }),
                "protected record button pixel changed"
            );
            let raw = titles::tile(&pixels, 64, 64, 4)?;
            ensure!(
                titles::untile(&raw, 64, 64, 4)? == pixels,
                "record edited tile round trip failed"
            );
            changes.insert(b.member, crate::compress::pack_compact(&raw)?);
            records.push(json!({"scene":caption.scene,"state":state,"japanese":caption.japanese,"lines":caption.korean_lines,"member":b.member,"raw_sha256":sha(&raw),"background_sha256":sha(&gradient),"editable":[12,19,52,44]}));
            previews.push((
                format!("button-{:02}.png", i * 2 + state),
                64,
                64,
                titles::rgba(&pixels, &b.palette)?,
            ));
        }
    }
    let bg = background(&n)?;
    let colors = screens::colors(&bg.palette)?;
    let mut desired: Vec<_> = bg.pixels.iter().map(|&v| colors[v as usize]).collect();
    for y in 0..48 {
        for x in 0..128 {
            desired[y * 256 + x] = if (2..126).contains(&x) && (2..46).contains(&y) {
                [31, 25, 0]
            } else {
                [20, 14, 0]
            };
        }
    }
    let small = battle_ui::text_ink(&font, &tr.title.korean, 12, [0, 0, 64, 24], 17)?;
    let mut ink = Vec::new();
    for (x, y) in small {
        for dy in 0..2 {
            for dx in 0..2 {
                ink.push((x * 2 + dx, y * 2 + dy + 2));
            }
        }
    }
    for &(x, y) in &ink {
        for dy in -2_i32..=2 {
            for dx in -2_i32..=2 {
                desired[(y as i32 + dy) as usize * 256 + (x as i32 + dx) as usize] = [31, 31, 31];
            }
        }
    }
    for &(x, y) in &ink {
        desired[y * 256 + x] = [10, 7, 0];
    }
    let encoded = screens::encode_screen(&bg, title_region, &desired)?;
    for (id, raw) in [(6, &encoded.map), (7, &encoded.tiles)] {
        changes.insert(
            id,
            if raw.len() <= 4096 {
                crate::compress::pack_compact(raw)?
            } else {
                crate::compress::pack(raw)?
            },
        );
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    let check = Narc::parse(&rebuilt)?;
    ensure!(
        screens::render(
            &unpack_halfword(check.members[6])?,
            &unpack_halfword(check.members[7])?,
            12
        )? == encoded.pixels
            && check.members[8] == n.members[8],
        "rebuilt record title mismatch"
    );
    previews.push((
        "title.png".into(),
        256,
        192,
        screens::rgba(&encoded.pixels, &bg.palette)?,
    ));
    fs::create_dir_all(out)?;
    fs::write(out.join("record.narc"), &rebuilt)?;
    for (name, w, h, rgba) in previews {
        write_png(&out.join(name), w, h, &rgba)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"record.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"buttons":records,"mapping":mapping,"title":{"korean":tr.title.korean,"source_pixels_sha256":sha(&bg.pixels),"pixels_sha256":sha(&encoded.pixels),"editable":[0,0,128,48],"tile_count":encoded.tile_count},"writes":changes.iter().map(|(&id,b)|json!({"member":id,"stored_size":b.len(),"capacity":n.members[id].len()})).collect::<Vec<_>>(),"protected":"all palettes, Wi-Fi states, button border/alpha and outside caption rectangle, title outside rectangle colors/transparency, all other archive members/padding","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("record.json"), &report)?;
    Ok(report)
}
pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    let (buttons, _) = read(rom)?;
    let mut members = vec![6, 7, 8];
    members.extend(buttons.iter().map(|b| b.member));
    battle_ui::member_residency(rom, ram, &[(ARCHIVE, members)])
}

pub fn check_vram(rom: &Rom, observation: &[u8]) -> Result<Value> {
    let json: Value = serde_json::from_slice(observation)?;
    let reads = json["reads"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing VRAM reads"))?;
    let read = |address: u64, length: usize| -> Result<Vec<u8>> {
        let items: Vec<_> = reads
            .iter()
            .filter(|r| r["address"].as_u64() == Some(address))
            .collect();
        ensure!(items.len() == 1, "missing or duplicated graphics read");
        let r = items[0];
        ensure!(
            r["cpu"] == "arm9"
                && r["memory_type"] == "arm9"
                && r["length"].as_u64() == Some(length as u64),
            "graphics read identity mismatch"
        );
        let bytes = hex::decode(
            r["hex"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing graphics bytes"))?,
        )?;
        ensure!(bytes.len() == length, "graphics read length mismatch");
        Ok(bytes)
    };
    let regs = read(0x04001000, 32)?;
    let vram = read(0x06200000, 131072)?;
    let palette = read(0x05000400, 512)?;
    let n = Narc::parse(rom.data(rom.file(ARCHIVE)?))?;
    let bg = background(&n)?;
    let dispcnt = u32le(&regs, 0)?;
    let mut matches = Vec::new();
    for layer in 0..2 {
        let control = u16le(&regs, 8 + layer * 2)?;
        if dispcnt & (1 << (8 + layer)) == 0 || control & 0x80 != 0 || control >> 14 != 0 {
            continue;
        }
        let map_offset = ((control >> 8) & 31) * 2048;
        let tile_offset = ((control >> 2) & 15) * 16384;
        if vram.get(map_offset..map_offset + bg.map.len()) == Some(bg.map.as_slice())
            && vram.get(tile_offset..tile_offset + bg.tiles.len()) == Some(bg.tiles.as_slice())
        {
            ensure!(
                u16le(&regs, 16 + layer * 4)? == 0 && u16le(&regs, 18 + layer * 4)? == 0,
                "record BG scrolled"
            );
            ensure!(
                palette[..bg.palette.len()] == bg.palette,
                "record VRAM palette differs"
            );
            matches.push(json!({"layer":layer,"control":control,"map_address":0x06200000+map_offset,"tiles_address":0x06200000+tile_offset}));
        }
    }
    ensure!(
        dispcnt & 7 == 0 && matches.len() == 1,
        "record title requires one enabled matching text BG"
    );
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"observation_sha256":sha(observation),"map_bytes":bg.map.len(),"tile_bytes":bg.tiles.len(),"palette_bytes":bg.palette.len(),"pixels_sha256":sha(&bg.pixels),"matches":matches,"claim":"complete ROM BG map, tiles and palette match enabled unscrolled sub-engine text BG; launch identity and visible output recorded separately"}),
    )
}
