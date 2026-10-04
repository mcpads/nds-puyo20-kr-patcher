//! Wi-Fi connect dialog button artwork; preserves every layout and palette.
use crate::{assets::json_file, battle_ui, buttons, format::*, graphics::write_png};
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
    id: usize,
    japanese: String,
    korean: String,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = "menu/wifi_connect.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "26b8229da69069ed736be86aee3e74b3f0e77bb8f845bd923ddcdc17a82971df",
        "connect archive changed"
    );
    let table = rom.data(rom.file("menu/connect_b_texlist.bin")?);
    ensure!(
        sha(table) == "883e95cf33940e52426a93188b1067d679d3ded0e8430da4f256402bccf0b696",
        "connect texture table changed"
    );
    let n = Narc::parse(source)?;
    ensure!(n.members.len() == 17, "connect members changed");
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 5,
        "expected five button textures"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&font_bytes), "font changed");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut changes = BTreeMap::new();
    let mut reports = Vec::new();
    let mut previews = Vec::new();
    for (id, (member, japanese, width, height, rect, size, baseline)) in [
        (1, "つぎへ", 128, 35, [34, 8, 96, 26], 12, 23),
        (11, "はい", 64, 64, [12, 22, 52, 41], 12, 37),
        (7, "いいえ", 64, 64, [12, 22, 52, 41], 12, 37),
        (9, "はい", 64, 64, [6, 15, 43, 32], 10, 28),
        (5, "いいえ", 64, 64, [6, 15, 43, 32], 10, 28),
    ]
    .into_iter()
    .enumerate()
    {
        let label = &tr.entries[id];
        ensure!(
            label.id == id && label.japanese == japanese,
            "button source mapping changed"
        );
        let old = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[member - 1])?;
        ensure!(old.len() == width * height, "button geometry changed");
        let colors = (0..palette.len() / 2)
            .map(|i| buttons::rgb(&palette, i))
            .collect::<Result<Vec<_>>>()?;
        let [x0, y0, x1, y1] = rect;
        let mut pixels = old.clone();
        for y in y0..y1 {
            for x in x0..x1 {
                let pos = y * width + x;
                ensure!(old[pos] >> 5 == 7, "label region touches translucent edge");
                if colors[usize::from(old[pos] & 31)] == [0, 0, 0] {
                    continue;
                }
                let color = |x: usize, y: usize| colors[usize::from(old[y * width + x] & 31)];
                let target = if id == 0 {
                    // Unlettered horizontal interior at this row supplies the bar background.
                    color(x0 - 1, y)
                } else {
                    let top = color(x, y0 - 1);
                    let bottom = color(x, y1);
                    let left = color(x0 - 1, y);
                    let right = color(x1, y);
                    let corners = [
                        color(x0 - 1, y0 - 1),
                        color(x1, y0 - 1),
                        color(x0 - 1, y1),
                        color(x1, y1),
                    ];
                    let dx = (x1 - x0 + 1) as i32;
                    let dy = (y1 - y0 + 1) as i32;
                    let u = (x - x0 + 1) as i32;
                    let v = (y - y0 + 1) as i32;
                    // Join all four unchanged edges, subtracting the double-counted corners.
                    std::array::from_fn(|c| {
                        let vertical = (top[c] * (dy - v) + bottom[c] * v) * dx;
                        let horizontal = (left[c] * (dx - u) + right[c] * u) * dy;
                        let corner = corners[0][c] * (dx - u) * (dy - v)
                            + corners[1][c] * u * (dy - v)
                            + corners[2][c] * (dx - u) * v
                            + corners[3][c] * u * v;
                        ((vertical + horizontal - corner + dx * dy / 2) / (dx * dy)).clamp(0, 31)
                    })
                };
                pixels[pos] = 224 | buttons::nearest(&palette, target)? as u8;
            }
        }
        let ink = battle_ui::text_ink(&font, &label.korean, size, rect, baseline)?;
        battle_ui::paint(
            &mut pixels,
            width,
            &ink,
            224 | buttons::nearest(&palette, [31, 31, 31])? as u8,
            224 | buttons::nearest(&palette, [0, 0, 0])? as u8,
        );
        for (p, (&a, &b)) in old.iter().zip(&pixels).enumerate() {
            ensure!(a >> 5 == b >> 5, "button alpha changed");
            ensure!(
                a == b || ((x0..x1).contains(&(p % width)) && (y0..y1).contains(&(p / width))),
                "protected button pixel changed"
            );
            ensure!(
                colors[usize::from(a & 31)] != [0, 0, 0] || a == b,
                "black boundary changed"
            );
        }
        battle_ui::compress_member(&mut changes, member, &pixels)?;
        reports.push(json!({"id":id,"member":member,"japanese":japanese,"korean":label.korean,"region":rect,"font_size":size,"baseline":baseline,"stored_size":changes[&member].len(),"capacity":n.members[member].len(),"decoded_sha256":sha(&pixels)}));
        let rgba = |bytes: &[u8]| -> Vec<u8> {
            bytes
                .iter()
                .flat_map(|v| {
                    let c = colors[usize::from(v & 31)];
                    [
                        (c[0] * 255 / 31) as u8,
                        (c[1] * 255 / 31) as u8,
                        (c[2] * 255 / 31) as u8,
                        ((v >> 5) as u16 * 255 / 7) as u8,
                    ]
                })
                .collect()
        };
        previews.push((id, width, height, rgba(&old), rgba(&pixels)));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("wifi_connect.narc"), &rebuilt)?;
    for (id, w, h, before, after) in previews {
        write_png(&out.join(format!("{id}-before.png")), w, h, &before)?;
        write_png(&out.join(format!("{id}-after.png")), w, h, &after)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"wifi_connect.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"entries":reports,"protected":"alpha, pixels outside label rectangles, black boundary pixels, all palettes/layouts and other members","background":"unchanged row sample for next bar; four-edge palette interpolation for circles; not original hidden-pixel recovery","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

/// Other archives that store byte-identical copies of the connect dialog circles
/// (connect member -> target member). Verified against the source before copying.
const COPIES: [(&str, [(usize, usize); 4]); 2] = [
    (
        "menu/wifi_continue.narc",
        [(11, 10), (7, 6), (9, 8), (5, 4)],
    ),
    (
        "menu/select_rule.narc",
        [(11, 58), (7, 53), (9, 56), (5, 51)],
    ),
];

/// Copy the prepared Korean circles into archives that reuse the same artwork.
pub fn copy(rom: &Rom, prepared: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = "menu/wifi_connect.narc";
    let original = Narc::parse(rom.data(rom.file(path)?))?;
    let edited_bytes = fs::read(prepared.join("wifi_connect.narc"))?;
    let edited = Narc::parse(&edited_bytes)?;
    fs::create_dir_all(out)?;
    let mut plans = Vec::new();
    for (k, (archive, pairs)) in COPIES.iter().enumerate() {
        let source = rom.data(rom.file(archive)?);
        let narc = Narc::parse(source)?;
        let mut changes = BTreeMap::new();
        for &(from, to) in pairs {
            ensure!(
                unpack(original.members[from])? == unpack(narc.members[to])?
                    && unpack(original.members[from - 1])? == unpack(narc.members[to - 1])?,
                "{archive} member {to} is not a copy of connect member {from}"
            );
            let data = edited.members[from].to_vec();
            ensure!(
                data.len() <= narc.members[to].len(),
                "{archive} member {to} capacity"
            );
            changes.insert(to, data);
        }
        let rebuilt = battle_ui::rebuilt(source, &changes)?;
        let dir = out.join(k.to_string());
        fs::create_dir_all(&dir)?;
        let name = archive.rsplit('/').next().unwrap();
        fs::write(dir.join(name), &rebuilt)?;
        let plan = json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":archive,"expected_sha256":sha(source),"input":name,"input_sha256":sha(&rebuilt)}]});
        json_file(&dir.join("plan.json"), &plan)?;
        plans.push(json!({"archive":archive,"members":pairs,"plan":dir.join("plan.json")}));
    }
    let report = json!({"source":sha(&edited_bytes),"copies":plans,"claim":"target circles are byte-identical to the connect originals (pixels and palettes); prepared Korean members are copied unchanged"});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
