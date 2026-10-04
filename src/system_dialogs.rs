//! Delete-confirmation and card-error art; dialog decisions and code are preserved.
use crate::{assets::json_file, battle_ui, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    member: usize,
    japanese: String,
    korean: String,
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    small_font_path: &Path,
    body_font_path: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = "system/system.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "68a0fdbbf402272326158b826e166a713cd2fef2e5e19b26f0ac50bda4f8ee44",
        "system archive changed"
    );
    let table = rom.data(rom.file("system/error_b_texlist.bin")?);
    ensure!(
        sha(table) == "486499e9a5d376fe6d2464293bdeec3b4f9ff2f139b0818eb8b15ec912468272",
        "system texture table changed"
    );
    let n = Narc::parse(source)?;
    for (id, hash) in [
        (
            0,
            "2848e02547d856928cc977c85e57b80a524c13b7d139522bcf851ff08b12c756",
        ),
        (
            1,
            "5fe330953b4f3f1a68b59ab72161229b8a85ca32b3a149f0f80a5908bbfed8dd",
        ),
        (
            32,
            "43b2f8820bc4f3f78df9ecddba825ef39b4323db5e3219313b0bd268b6f3c951",
        ),
    ] {
        ensure!(
            sha(&unpack(n.members[id])?) == hash,
            "system layout changed"
        );
    }
    let overlay = rom
        .overlays
        .iter()
        .find(|o| o.cpu == "arm9" && o.id == 6)
        .ok_or_else(|| anyhow::anyhow!("missing system overlay"))?;
    let (code, _) = crate::arm9::decode(rom.data(&rom.files[overlay.file_id]))?;
    ensure!(
        sha(&code) == "615c612d9b03ad1b658e31868ca415fb39dd893b4778fe6bfaeda1e705010a8a",
        "system overlay changed"
    );
    ensure!(
        slice(&code, 0x02131524 - overlay.ram, 27)? == b"system/error_b_texlist.bin\0",
        "system table reference changed"
    );
    let input = fs::read(translation)?;
    let labels: Labels = serde_json::from_slice(&input)?;
    ensure!(
        labels.state == "development_art_draft" && labels.entries.len() == 9,
        "expected nine system labels"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&font_bytes), "font changed");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let body_font = body_font_path
        .map(|path| -> Result<_> {
            let bytes = fs::read(path)?;
            ensure!(
                sha(&bytes) == crate::fonts::GALMURI14_SHA256,
                "body font changed"
            );
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;
    let small_bytes = fs::read(small_font_path)?;
    ensure!(
        sha(&small_bytes) == "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        "small font changed"
    );
    let small_font = fontdue::Font::from_bytes(small_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let expected = [
        "いいえ",
        "いいえ",
        "はい",
        "はい",
        "セーブデータを\nすべてけします\nよろしいですか?",
        "セーブデータが\nこわれています。\nけしても よろしいですか?",
        "ほんとうに\nセーブデータをけして\nもよろしいですか?",
        "セーブデータを\nけしました。",
        "でんげんをきり\nDSカードを\nさしこみなおしてください。",
    ];
    let mut changes = BTreeMap::new();
    let mut entries = Vec::new();
    let mut previews = Vec::new();
    for (i, label) in labels.entries.iter().enumerate() {
        let member = 3 + i * 2;
        ensure!(
            label.member == member && label.japanese == expected[i],
            "system source identity changed"
        );
        let original = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[member - 1])?;
        let colors = (0..palette.len() / 2)
            .map(|c| crate::buttons::rgb(&palette, c))
            .collect::<Result<Vec<_>>>()?;
        let white = (0..colors.len())
            .min_by_key(|i| colors[*i].iter().map(|v| (31 - v).pow(2)).sum::<i32>())
            .unwrap() as u8;
        let dark = (0..colors.len())
            .min_by_key(|i| colors[*i].iter().sum::<i32>())
            .unwrap() as u8;
        let (pixels, width, height, region) = if member < 11 {
            ensure!(original.len() == 4096, "button extent changed");
            let large = member == 5 || member == 9;
            let rect = if large {
                [12, 22, 52, 41]
            } else {
                [4, 12, 34, 27]
            };
            let [x0, y0, x1, y1] = rect;
            let mut pixels = original.clone();
            for y in y0..y1 {
                for x in x0..x1 {
                    let original_color = colors[usize::from(original[y * 64 + x] & 31)];
                    ensure!(
                        original[y * 64 + x] >> 5 == 7,
                        "button label region touches translucent boundary"
                    );
                    if original_color == [0, 0, 0] {
                        continue;
                    }
                    let above = colors[usize::from(original[(y0 - 1) * 64 + x] & 31)];
                    let below = colors[usize::from(original[y1 * 64 + x] & 31)];
                    let distance = (y1 - y0 + 1) as i32;
                    let t = (y - y0 + 1) as i32;
                    let target = std::array::from_fn::<_, 3, _>(|c| {
                        (above[c] * (distance - t) + below[c] * t + distance / 2) / distance
                    });
                    let index = (0..colors.len())
                        .min_by_key(|i| {
                            (0..3)
                                .map(|c| (colors[*i][c] - target[c]).pow(2))
                                .sum::<i32>()
                        })
                        .unwrap();
                    pixels[y * 64 + x] = 0xe0 | index as u8;
                }
            }
            let ink = battle_ui::text_ink(
                if large { &font } else { &small_font },
                &label.korean,
                if large { 11 } else { 7 },
                rect,
                if large { 37 } else { 23 },
            )?;
            battle_ui::paint(&mut pixels, 64, &ink, 0xe0 | white, 0xe0 | dark);
            ensure!(
                pixels
                    .iter()
                    .zip(&original)
                    .all(|(a, b)| colors[usize::from(b & 31)] != [0, 0, 0] || a == b),
                "changed black button boundary"
            );
            ensure!(
                pixels
                    .iter()
                    .zip(&original)
                    .enumerate()
                    .all(|(i, (a, b))| ((x0..x1).contains(&(i % 64))
                        && (y0..y1).contains(&(i / 64)))
                        || a == b),
                "changed protected button pixels"
            );
            ensure!(
                pixels.iter().zip(&original).all(|(a, b)| a >> 5 == b >> 5),
                "changed button alpha"
            );
            (pixels, 64, 64, rect)
        } else {
            ensure!(
                original.len() == 16384 && palette.len() == 32,
                "system body extent changed"
            );
            let mut pixels = vec![0u8; 256 * 128];
            let lines = label.korean.lines().collect::<Vec<_>>();
            ensure!(
                (2..=3).contains(&lines.len()),
                "system body line count changed"
            );
            let top = (128 - lines.len() * 18) / 2;
            for (row, line) in lines.iter().enumerate() {
                let y = top + row * 18;
                let (body, size, baseline) = if let Some(body) = &body_font {
                    (body, 15, y + 15)
                } else {
                    (&font, 12, y + 14)
                };
                let ink = battle_ui::text_ink(body, line, size, [16, y, 240, y + 18], baseline)?;
                battle_ui::paint(&mut pixels, 256, &ink, white, dark);
            }
            ensure!(
                white != 0 && dark != 0,
                "system body ink uses transparent index"
            );
            let bytes = battle_ui::pack(&pixels)?;
            (bytes, 256, 128, [0, 0, 256, 128])
        };
        battle_ui::compress_member(&mut changes, member, &pixels)?;
        let rgba = |bytes: &[u8]| -> Result<Vec<u8>> {
            let values = if member < 11 {
                bytes.to_vec()
            } else {
                battle_ui::indices(bytes)
            };
            let mut result = Vec::with_capacity(values.len() * 4);
            for v in values {
                let index = if member < 11 { v & 31 } else { v };
                let rgb = colors[index as usize];
                let alpha = if member < 11 {
                    u16::from(v >> 5) * 255 / 7
                } else if v == 0 {
                    0
                } else {
                    255
                };
                result.extend([
                    rgb[0] as u8 * 8,
                    rgb[1] as u8 * 8,
                    rgb[2] as u8 * 8,
                    alpha as u8,
                ]);
            }
            Ok(result)
        };
        previews.push((member, width, height, rgba(&original)?, rgba(&pixels)?));
        entries.push(json!({"member":member,"japanese":label.japanese,"korean":label.korean,"region":region,"stored_size":changes[&member].len(),"capacity":n.members[member].len(),"decoded_sha256":sha(&pixels)}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("system.narc"), &rebuilt)?;
    for (member, w, h, before, after) in previews {
        write_png(&out.join(format!("{member}-before.png")), w, h, &before)?;
        write_png(&out.join(format!("{member}-after.png")), w, h, &after)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"system.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let mut report = json!({"translation_sha256":sha(&input),"entries":entries,"overlay_id":6,"overlay_sha256":sha(&code),"protected":"button alpha and pixels outside label rectangles; palettes, layouts, other members and dialog decisions/code","background":"button interior vertical color interpolation, not recovery of hidden original pixels","runtime_verified":false,"human_reviewed":false});
    if body_font_path.is_some() {
        report["body_font"] = json!({"sha256":crate::fonts::GALMURI14_SHA256,"size":15,"line_height":18,"baseline":15});
    }
    json_file(&out.join("system.json"), &report)?;
    Ok(report)
}
