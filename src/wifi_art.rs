//! Local friend-code screen artwork, with DSIF joins and original palettes preserved.
use crate::{assets::json_file, battle_ui, buttons, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    state: String,
    input_japanese: String,
    input_korean: String,
    list_japanese: String,
    list_korean: String,
    clear_japanese: String,
    clear_korean: String,
}

/// Compare nonempty edited payloads; a blank suffix has no identifying RAM pattern.
pub fn check_ram(rom: &Rom, input_ram: &[u8], list_ram: &[u8]) -> Result<Value> {
    ensure!(
        input_ram.len() == 0x400000 && list_ram.len() == 0x400000,
        "expected 4 MiB main RAM dumps"
    );
    let mut entries = Vec::new();
    let mut excluded = Vec::new();
    for (path, ram, members) in [
        (
            "menu/wifi_friend_input.narc",
            input_ram,
            vec![22, 24, 26, 5],
        ),
        ("menu/wifi_friend_list.narc", list_ram, vec![13]),
    ] {
        let n = Narc::parse(rom.data(rom.file(path)?))?;
        for member in members {
            let bytes = unpack_halfword(n.members[member])?;
            ensure!(!bytes.is_empty(), "empty texture payload");
            if member == 26 && bytes.iter().all(|&b| b == 0) {
                ensure!(bytes.len() == 512, "blank title suffix extent changed");
                excluded.push(json!({"archive":path,"member":member,"size":bytes.len(),"sha256":sha(&bytes),"reason":"all-zero title suffix cannot identify its allocation; no residency or consumption claim"}));
                continue;
            }
            let positions = ram
                .windows(bytes.len())
                .enumerate()
                .filter_map(|(p, b)| (b == bytes).then_some(0x02000000 + p))
                .collect::<Vec<_>>();
            ensure!(
                positions.len() == 1,
                "edited Wi-Fi texture {path}/{member}: expected one complete copy, found {}",
                positions.len()
            );
            entries.push(json!({"archive":path,"member":member,"address":positions[0],"size":bytes.len(),"sha256":sha(&bytes),"ram_sha256":sha(ram)}));
        }
    }
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"entries":entries,"excluded":excluded,"claim":"complete nonblank edited texture bytes uniquely resident in each supplied RAM; blank suffix and active renderer require separate evidence"}),
    )
}

fn rgba(pixels: &[u8], palette: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for &p in pixels {
        let c = buttons::rgb(palette, (p & 31) as usize)?;
        out.extend(c.map(|v| (v * 255 / 31) as u8));
        out.push(((p >> 5) as u16 * 255 / 7) as u8);
    }
    Ok(out)
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.input_japanese == "ともだちコードの登録"
            && tr.list_japanese == "ともだちリスト"
            && tr.clear_japanese == "クリア",
        "Wi-Fi label identity changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&font_bytes), "font changed");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut plans = Vec::new();
    let mut reports = Vec::new();
    let mut outputs = Vec::new();
    let mut previews = Vec::new();
    for is_input in [true, false] {
        let (
            name,
            table_name,
            source_hash,
            table_hash,
            layout_id,
            layout_hash,
            title,
            canvas_width,
        ) = if is_input {
            (
                "wifi_friend_input",
                "code_register",
                "4732076c8ca9f3fbbf93e4bafec6f40d4e3784f5a4b6fcbf8c89cfea0248e161",
                "693f54d9432aabc366b8b4d00f7ef003edb95914adf402a1c9eb0422d6caf655",
                28,
                "aa08e596c099e5b1698f9de8f55bbf791a6e57d55c3c37da28c9cc8ec2ff3197",
                &tr.input_korean,
                144,
            )
        } else {
            (
                "wifi_friend_list",
                "friend_list",
                "c8e901595f0cd813a99d99e2e35e5a4189a3296ee8c658f1ffe0ebbe819790cd",
                "ddcb307d8eab9617d9660cdc374fac0ca3f30c58364245cddaed98b882f926bb",
                14,
                "41812f6078a52fda83eb853471e359a39751a2085e9060f566294780fc957a48",
                &tr.list_korean,
                102,
            )
        };
        let path = format!("menu/{name}.narc");
        let source = rom.data(rom.file(&path)?);
        let table = rom.data(rom.file(&format!("menu/{table_name}_b_texlist.bin"))?);
        ensure!(
            sha(source) == source_hash && sha(table) == table_hash,
            "Wi-Fi source changed"
        );
        let n = Narc::parse(source)?;
        let layout = unpack(n.members[layout_id])?;
        ensure!(sha(&layout) == layout_hash, "Wi-Fi layout changed");
        let crops = 32 + u32le(&layout, 80)?;
        let bank = 32 + u32le(&layout, 88)?;
        let nodes = 32 + u32le(&layout, bank + 4)?;
        let mut mask = vec![0; canvas_width * 20];
        let ink = battle_ui::text_ink(&font, title, 12, [2, 1, canvas_width - 2, 19], 15)?;
        battle_ui::paint(&mut mask, canvas_width, &ink, 2, 1);
        let mut joined = vec![0; canvas_width * 20 * 4];
        let mut changes = BTreeMap::new();
        let mut parts = Vec::new();
        let specs = if is_input {
            vec![
                (2, 22, 64, 64, 0, 13, 18, -512, 512, -640),
                (3, 24, 64, 64, 64, 14, 19, 0, 1024, -128),
                (4, 26, 16, 16, 128, 15, 20, 0, 256, 896),
            ]
        } else {
            vec![(2, 13, 128, 102, 0, 4, 0, -816, 816, 2048)]
        };
        for (texture, member, width, visible, offset, crop_id, node_id, left, right, center) in
            specs
        {
            let row = slice(table, texture * 12, 12)?;
            ensure!(
                u16le(row, 0)? == member
                    && u16le(row, 2)? == member - 1
                    && u16le(row, 4)? == 1
                    && u16le(row, 8)? == width
                    && u16le(row, 10)? == 32,
                "title texture geometry changed"
            );
            let crop = crops + crop_id * 20;
            let expected = [texture, 0, 0, visible * 4096 / width, 2560];
            for (i, value) in expected.iter().enumerate() {
                ensure!(
                    u32le(&layout, crop + i * 4)? == *value,
                    "title crop changed"
                );
            }
            let node = 32 + u32le(&layout, nodes + node_id * 4)?;
            let params = 32 + u32le(&layout, node + 64)?;
            ensure!(
                u32le(&layout, params)? == crop_id
                    && u32le(&layout, node + 12)? as i32 == left
                    && u32le(&layout, node + 28)? as i32 == right
                    && u32le(&layout, params + 132)? as i32 == center,
                "title part placement changed"
            );
            ensure!(
                (center + left - if is_input { 0 } else { 2048 }) / 16 + canvas_width as i32 / 2
                    == offset as i32
                    && right - left == visible as i32 * 16,
                "title parts do not join"
            );
            let old = unpack_halfword(n.members[member])?;
            ensure!(old.len() == width * 32, "title extent changed");
            let palette = n.members[member - 1];
            let light = 224 | buttons::nearest(palette, [31, 31, 31])? as u8;
            let dark = 224 | buttons::nearest(palette, [0, 0, 0])? as u8;
            let mut pixels = old.clone();
            for y in 0..20 {
                for x in 0..visible {
                    pixels[y * width + x] = match mask[y * canvas_width + offset + x] {
                        0 => 0,
                        1 => dark,
                        _ => light,
                    };
                }
            }
            ensure!(
                old.iter()
                    .zip(&pixels)
                    .enumerate()
                    .all(|(p, (a, b))| a == b || (p / width < 20 && p % width < visible)),
                "protected title pixel changed"
            );
            let colors = rgba(&pixels, palette)?;
            for y in 0..20 {
                joined[(y * canvas_width + offset) * 4..(y * canvas_width + offset + visible) * 4]
                    .copy_from_slice(&colors[y * width * 4..(y * width + visible) * 4]);
            }
            battle_ui::compress_member(&mut changes, member, &pixels)?;
            parts.push(json!({"texture":texture,"member":member,"offset":offset,"width":width,"visible":visible,"decoded_sha256":sha(&pixels),"stored_size":changes[&member].len(),"capacity":n.members[member].len()}));
        }
        if is_input {
            let old = unpack_halfword(n.members[5])?;
            ensure!(old.len() == 4480, "clear button extent changed");
            let palette = n.members[4];
            let mut pixels = old.clone();
            let high = buttons::rgb(palette, (old[7 * 128 + 32] & 31) as usize)?;
            let low = buttons::rgb(palette, (old[27 * 128 + 32] & 31) as usize)?;
            for y in 8..26 {
                let target = std::array::from_fn(|c| {
                    (high[c] * (26 - y) as i32 + low[c] * (y - 7) as i32) / 19
                });
                let index = buttons::nearest(palette, target)? as u8;
                for x in 8..58 {
                    pixels[y * 128 + x] = old[y * 128 + x] & 224 | index;
                }
            }
            let ink = battle_ui::text_ink(&font, &tr.clear_korean, 12, [9, 9, 57, 25], 23)?;
            battle_ui::paint(
                &mut pixels,
                128,
                &ink,
                224 | buttons::nearest(palette, [31, 31, 31])? as u8,
                224 | buttons::nearest(palette, [0, 0, 0])? as u8,
            );
            for (p, (&a, &b)) in old.iter().zip(&pixels).enumerate() {
                ensure!(a & 224 == b & 224, "clear button alpha changed");
                ensure!(
                    a == b || ((8..58).contains(&(p % 128)) && (8..26).contains(&(p / 128))),
                    "clear button protected pixel changed"
                );
            }
            battle_ui::compress_member(&mut changes, 5, &pixels)?;
            parts.push(json!({"texture":9,"member":5,"editable":[8,8,58,26],"decoded_sha256":sha(&pixels),"stored_size":changes[&5].len(),"capacity":n.members[5].len()}));
            previews.push(("clear".to_string(), 128, 35, rgba(&pixels, palette)?));
        }
        let rebuilt = battle_ui::rebuilt(source, &changes)?;
        let filename = format!("{name}.narc");
        plans.push(json!({"file":path,"expected_sha256":sha(source),"input":filename,"input_sha256":sha(&rebuilt)}));
        reports.push(json!({"archive":path,"layout_member":layout_id,"layout_sha256":layout_hash,"title":title,"parts":parts}));
        outputs.push((filename, rebuilt));
        previews.push((name.to_string(), canvas_width, 20, joined));
    }
    fs::create_dir_all(out)?;
    for (name, bytes) in outputs {
        fs::write(out.join(name), bytes)?;
    }
    for (name, w, h, colors) in previews {
        write_png(&out.join(format!("{name}.png")), w, h, &colors)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":plans}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"entries":reports,"protected":"palettes, DSIF, non-target members, all pixels outside crops; clear button alpha and frame","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("art.json"), &report)?;
    Ok(report)
}
