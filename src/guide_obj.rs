use crate::{
    assets::json_file,
    battle_ui::{member_residency, paint, rebuilt, text_ink},
    format::*,
    graphics::write_png,
    titles,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
const ARCHIVE: &str = "menu/guide_icon_obj.narc";
const IMAGES: [(usize, usize, usize, usize); 5] = [
    (0, 1, 32, 32),
    (4, 5, 32, 16),
    (6, 5, 32, 16),
    (2, 3, 32, 32),
    (7, 8, 32, 16),
];
struct Image {
    member: usize,
    palette: Vec<u8>,
    pixels: Vec<u8>,
    raw: Vec<u8>,
    width: usize,
    height: usize,
}
fn read(rom: &Rom) -> Result<(Vec<Image>, Value)> {
    let n = Narc::parse(rom.data(rom.file(ARCHIVE)?))?;
    ensure!(n.members.len() == 11, "guide OBJ population changed");
    let mut mappings = Vec::new();
    let mut images = Vec::new();
    for (gem_id, ilf_path, range, names, expected) in [
        (
            9,
            "menu/guide_icon_obj_ilf.bin",
            0..3,
            vec!["btn_a", "btn_a2"],
            "b5dc4afcddf7a231b7dc561230c3ad4f55e265b1663614ca0f48cac4fc3bef64",
        ),
        (
            10,
            "menu/guide_icon_score_ilf.bin",
            3..5,
            vec!["btn_b"],
            "c5921d85eae3e5aac54b357c55beb4f6787375aa881188e6f3b75de783bcc94c",
        ),
    ] {
        let gem = unpack(n.members[gem_id])?;
        ensure!(sha(&gem) == expected, "guide OBJ layout changed");
        let ilf = rom.data(rom.file(ilf_path)?);
        let base = u32le(&gem, 20)?;
        let gem2 = u32le(&gem, 88)?;
        ensure!(
            slice(&gem, 0, 4)? == b"Gem1"
                && u32le(&gem, 8)? == gem.len()
                && slice(&gem, gem2, 4)? == b"Gem2"
                && gem2 + u32le(&gem, gem2 + 20)? == base,
            "guide Gem container mismatch"
        );
        ensure!(
            ilf.len() == range.len() * 4
                && u32le(&gem, 40)? == range.len()
                && u32le(&gem, 80)? == range.len(),
            "guide Gem/ILF count mismatch"
        );
        let mut scene_names = Vec::new();
        let mut p = u32le(&gem, 24)?;
        for _ in 0..u32le(&gem, 32)? {
            let scene = base + u32le(&gem, p)?;
            ensure!(slice(&gem, scene, 4)? == b"Scen", "guide scene mismatch");
            let len = slice(&gem, p + 4, 1)?[0] as usize;
            scene_names.push(std::str::from_utf8(slice(&gem, p + 5, len)?)?.to_owned());
            p = (p + 5 + len + 3) & !3;
        }
        ensure!(scene_names == names, "guide scene names changed");
        let mut rows = Vec::new();
        for (row, i) in range.enumerate() {
            let (member, pal, w, h) = IMAGES[i];
            let word = u32le(ilf, row * 4)?;
            let descriptor = base + u32le(&gem, 44)? + row * 32;
            let size = base + u32le(&gem, 84)? + row * 16;
            ensure!(
                (word >> 8) & 4095 == member
                    && word >> 20 == pal
                    && u32le(&gem, descriptor)? == word & 255
                    && u32le(&gem, descriptor + 4)? == 3
                    && u16le(&gem, descriptor + 8)? == w
                    && u16le(&gem, descriptor + 10)? == h,
                "guide image mapping changed"
            );
            ensure!(
                base + u32le(&gem, size + 4)? == descriptor
                    && u32le(&gem, size + 8)? == w << 16
                    && u32le(&gem, size + 12)? == h << 16,
                "guide size backlink mismatch"
            );
            let raw = unpack_halfword(n.members[member])?;
            let palette = unpack(n.members[pal])?;
            ensure!(
                raw.len() == w * h / 2 && [8, 12, 32].contains(&palette.len()),
                "guide image extent changed"
            );
            let pixels = titles::untile(&raw, w, h, 4)?;
            ensure!(
                titles::tile(&pixels, w, h, 4)? == raw
                    && pixels.iter().all(|&v| (v as usize) < palette.len() / 2),
                "guide pixel round trip or palette index mismatch"
            );
            rows.push(json!({"logical_id":word&255,"member":member,"palette_member":pal,"width":w,"height":h,"raw_sha256":sha(&raw),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&palette)}));
            images.push(Image {
                member,
                palette,
                pixels,
                raw,
                width: w,
                height: h,
            });
        }
        mappings.push(json!({"gem_member":gem_id,"gem_sha256":sha(&gem),"ilf":ilf_path,"ilf_sha256":sha(ilf),"scenes":scene_names,"images":rows}));
    }
    Ok((
        images,
        json!({"source_sha256":sha(rom.bytes),"archive_sha256":sha(rom.data(rom.file(ARCHIVE)?)),"mappings":mappings,"claim":"target Gem/ILF geometry and tile round trips; animation and runtime consumption separate"}),
    ))
}
pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (images, report) = read(rom)?;
    fs::create_dir_all(out)?;
    for im in images {
        write_png(
            &out.join(format!("member-{}.png", im.member)),
            im.width,
            im.height,
            &titles::rgba(&im.pixels, &im.palette)?,
        )?;
    }
    json_file(&out.join("guide-obj.json"), &report)?;
    Ok(report)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    member: usize,
    source_sha256: String,
    japanese: String,
    korean: String,
}
pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.entries.iter().map(|x| x.member).collect::<Vec<_>>() == [4, 6, 7],
        "guide draft population changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        // Galmuri9 at its native 10px grid, matching the guide legend captions.
        sha(&font_bytes) == "48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee",
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let (mut images, mapping) = read(rom)?;
    let source = rom.data(rom.file(ARCHIVE)?);
    let n = Narc::parse(source)?;
    let mut changes = BTreeMap::new();
    let mut records = Vec::new();
    for label in tr.entries {
        let im = images
            .iter_mut()
            .find(|im| im.member == label.member)
            .unwrap();
        ensure!(
            sha(&im.raw) == label.source_sha256 && !label.japanese.is_empty(),
            "guide source caption mismatch"
        );
        let mut pixels = vec![0; 32 * 16];
        let ink = text_ink(&font, &label.korean, 10, [0, 0, 32, 16], 12)?;
        let (white, dark) = if label.member == 7 { (2, 1) } else { (3, 2) };
        paint(&mut pixels, 32, &ink, white, dark);
        let raw = titles::tile(&pixels, 32, 16, 4)?;
        ensure!(
            titles::untile(&raw, 32, 16, 4)? == pixels,
            "edited guide tile round trip failed"
        );
        let packed = crate::compress::pack_compact(&raw)?;
        ensure!(
            changes.insert(label.member, packed).is_none(),
            "duplicate guide writer"
        );
        records.push(json!({"member":label.member,"japanese":label.japanese,"korean":label.korean,"source_sha256":label.source_sha256,"decoded_sha256":sha(&raw),"stored_size":changes[&label.member].len(),"capacity":n.members[label.member].len()}));
        im.pixels = pixels;
    }
    let data = rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("guide.narc"), &data)?;
    for im in images {
        write_png(
            &out.join(format!("member-{}.png", im.member)),
            im.width,
            im.height,
            &titles::rgba(&im.pixels, &im.palette)?,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"guide.narc","input_sha256":sha(&data)}]}),
    )?;
    let report = json!({"mapping":mapping,"translation_sha256":sha(&input),"captions":records,"editable":"complete 32x16 text-only sprite payloads","protected":"A/B icons, all palettes, Gem/ILF layout and every other member/padding","font_size":10,"baseline":12,"horizontal_center":16,"runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}
pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    member_residency(rom, ram, &[(ARCHIVE, vec![0, 2, 4, 6, 7])])
}
