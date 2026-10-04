use crate::{assets::json_file, battle_ui::*, format::*, graphics::write_png, titles};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
const ARCHIVE: &str = "puyo/menu/puyo_menu.narc";
const SHEETS: [(usize, usize, usize, usize); 5] = [
    (29, 40, 30, 5),
    (31, 33, 32, 5),
    (34, 36, 35, 6),
    (37, 39, 38, 5),
    (41, 43, 42, 5),
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    sheets: Vec<Sheet>,
    #[serde(default)]
    source_sha256: Option<String>,
    #[serde(default)]
    background_artwork: Option<[titles::Artwork; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sheet {
    linear_member: usize,
    tiled_member: usize,
    palette_member: usize,
    source_sha256: String,
    labels: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    japanese: String,
    korean: String,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.sheets.len() == SHEETS.len(),
        "pause draft population changed"
    );
    ensure!(
        tr.source_sha256
            .as_ref()
            .is_none_or(|v| *v == sha(rom.bytes))
            && (tr.background_artwork.is_none() || tr.source_sha256.is_some()),
        "generated pause backgrounds require exact source identity"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let source = rom.data(rom.file(ARCHIVE)?);
    let n = Narc::parse(source)?;
    ensure!(n.members.len() == 46, "pause archive population changed");
    let reference = unpack_halfword(n.members[41])?;
    ensure!(
        sha(&reference) == "d0bab23bc90434f8657b3c204b2f31995c522f7ecd4fc426294373849935bacb",
        "pause background reference changed"
    );
    let reference = indices(&reference);
    let mut backgrounds = Vec::new();
    // The short やめる captions leave columns 35 (large) and 36 (small)
    // clean. Reconstruct only their central glyph region from those rows;
    // all other gradient and border pixels come from the source sprite.
    for (slot, x0, x1, y0, y1, column) in [(3, 40, 85, 8, 24, 35), (8, 44, 84, 9, 23, 36)] {
        let mut pixels = reference[slot * 4096..(slot + 1) * 4096].to_vec();
        for y in y0..y1 {
            let color = pixels[y * 128 + column];
            for x in x0..x1 {
                pixels[y * 128 + x] = color;
            }
        }
        backgrounds.push(pixels);
    }
    if let Some(artwork) = &tr.background_artwork {
        let palette = unpack(n.members[42])?;
        backgrounds = artwork
            .iter()
            .map(|art| titles::generated_pixels(128, 32, &palette[..32], art))
            .collect::<Result<Vec<_>>>()?;
    }
    let mut changes = BTreeMap::new();
    let mut previews = Vec::new();
    let mut records = Vec::new();
    for (sheet, &(linear, tiled, pal, half)) in tr.sheets.iter().zip(&SHEETS) {
        ensure!(
            (
                sheet.linear_member,
                sheet.tiled_member,
                sheet.palette_member,
                sheet.labels.len()
            ) == (linear, tiled, pal, half),
            "pause sheet identity changed"
        );
        let raw = unpack_halfword(n.members[linear])?;
        ensure!(
            sha(&raw) == sheet.source_sha256 && raw.len() == half * 2 * 2048,
            "pause source mismatch"
        );
        let original = indices(&raw);
        let mut pixels = original.clone();
        let colors = unpack(n.members[pal])?;
        ensure!(
            sha(&colors) == "1ffac96c3f763ca49c4b2b250cfd34b38884ad97d0ced38884e84425c1d1025d",
            "pause palette changed"
        );
        let original_tiled = original
            .chunks_exact(4096)
            .map(sprite_pair_bytes)
            .collect::<Result<Vec<_>>>()?
            .concat();
        ensure!(
            original_tiled == unpack_halfword(n.members[tiled])?,
            "pause source pair mismatch"
        );
        for slot in 0..half * 2 {
            let small = usize::from(slot >= half);
            let label = &sheet.labels[slot % half];
            ensure!(!label.japanese.is_empty(), "empty source caption");
            let rect = if small == 0 {
                [9, 8, 120, 24]
            } else {
                [17, 9, 112, 23]
            };
            let (size, baseline) = if small == 0 { (12, 22) } else { (12, 21) };
            let cell = &mut pixels[slot * 4096..(slot + 1) * 4096];
            if tr.background_artwork.is_some() {
                cell.copy_from_slice(&backgrounds[small]);
            } else {
                for y in rect[1]..rect[3] {
                    for x in rect[0]..rect[2] {
                        cell[y * 128 + x] = backgrounds[small][y * 128 + x];
                    }
                }
            }
            let ink = text_ink(&font, &label.korean, size, rect, baseline)?;
            // Large and small source art use different palette indices for white.
            paint(cell, 128, &ink, if small == 0 { 15 } else { 14 }, 1);
            let old = &original[slot * 4096..(slot + 1) * 4096];
            for (q, (&a, &b)) in cell.iter().zip(old).enumerate() {
                if tr.background_artwork.is_some() {
                    ensure!(
                        (a == 0) == (backgrounds[small][q] == 0),
                        "pause lettering changed generated silhouette"
                    );
                    continue;
                }
                ensure!((a == 0) == (b == 0), "pause silhouette changed");
                if !(rect[0]..rect[2]).contains(&(q % 128))
                    || !(rect[1]..rect[3]).contains(&(q / 128))
                {
                    ensure!(a == b, "protected pause pixel changed");
                }
            }
            records.push(json!({"linear_member":linear,"tiled_member":tiled,"slot":slot,"japanese":label.japanese,"korean":label.korean,"editable":rect,"font_size":size,"baseline":baseline}));
        }
        let tiled_pixels = pixels
            .chunks_exact(4096)
            .map(sprite_pair_bytes)
            .collect::<Result<Vec<_>>>()?
            .concat();
        compress_member(&mut changes, linear, &pack(&pixels)?)?;
        compress_member(&mut changes, tiled, &tiled_pixels)?;
        previews.push((linear, pixels, colors));
    }
    let data = rebuilt(source, &changes)?;
    let checked = Narc::parse(&data)?;
    for &(l, t, _, _) in &SHEETS {
        let raw = unpack_halfword(checked.members[l])?;
        let converted = indices(&raw)
            .chunks_exact(4096)
            .map(sprite_pair_bytes)
            .collect::<Result<Vec<_>>>()?
            .concat();
        ensure!(
            converted == unpack_halfword(checked.members[t])?,
            "rebuilt pause pair mismatch"
        );
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("menu.narc"), &data)?;
    for (id, pixels, colors) in previews {
        for bank in 0..2 {
            write_png(
                &out.join(format!("pause-{id}-bank{bank}.png")),
                128,
                pixels.len() / 128,
                &titles::rgba(&pixels, &colors[bank * 32..bank * 32 + 32])?,
            )?;
        }
    }
    for (i, pixels) in backgrounds.iter().enumerate() {
        fs::write(out.join(format!("background-{i}.bin")), pixels)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"menu.narc","input_sha256":sha(&data)}]}),
    )?;
    let writes=changes.iter().map(|(&id,b)|json!({"member":id,"stored_size":b.len(),"capacity":n.members[id].len(),"decoded_sha256":sha(&unpack_halfword(b).unwrap())})).collect::<Vec<_>>();
    let mut report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"background_pixel_sha256":backgrounds.iter().map(|p|sha(p)).collect::<Vec<_>>(),"buttons":records,"writes":writes,"protected":"palette, silhouette, pixels outside caption rectangles, other archive members and padding; title unchanged","runtime_verified":false,"human_reviewed":false});
    if let Some(artwork) = &tr.background_artwork {
        report["background_artwork"] = json!(artwork);
        report["protected"] = json!(
            "original palettes, other archive members and padding; title unchanged; whole button backgrounds replaced, generated silhouette preserved while lettering"
        );
    }
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}

pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    member_residency(
        rom,
        ram,
        &[(
            ARCHIVE,
            SHEETS.iter().flat_map(|&(l, t, _, _)| [l, t]).collect(),
        )],
    )
}

/// Frozen main RAM plus the independently extracted immutable ARM9 section.
/// These offsets are execution-section offsets, never stored ROM offsets.
pub fn inspect_title(ram: &[u8], section: &[u8]) -> Result<Value> {
    ensure!(
        ram.len() == 0x400000
            && section.len() == 1098560
            && sha(section) == "2c5899af92a7cc4fe9ae7aacd34e299053e67f3f68da271f2f84008cf202ca41",
        "pause title input identity mismatch"
    );
    let mut regions = Vec::new();
    for (offset, len, expected) in [
        (
            0x673e8,
            92,
            "28a118a85898a48f7e0dd9e928fb9384c6fe142654085dd8bafb574c249b5007",
        ),
        (
            0x6794c,
            452,
            "21b0b68a0764243709d2ee840d3fe65c60467a433fd9203e5b2d90226756d0bd",
        ),
    ] {
        let bytes = slice(section, offset, len)?;
        ensure!(
            sha(bytes) == expected && slice(ram, offset, len)? == bytes,
            "pause title code differs from source section"
        );
        let mut instructions = Vec::new();
        for (i, word) in bytes.chunks_exact(4).enumerate() {
            let typed = arm946e_s::decode_arm_bytes(word)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&typed)? == word,
                "title ARM instruction round trip failed"
            );
            instructions.push(json!({"address":0x02000000+offset+i*4,"bytes":hex::encode(word),"typed_instruction":format!("{typed:?}")}));
        }
        regions.push(
            json!({"address":0x02000000+offset,"sha256":sha(bytes),"instructions":instructions}),
        );
    }
    let coords = slice(section, 0xf7048, 8)?;
    ensure!(
        coords == hex::decode("646682669e640000")? && slice(ram, 0xf7048, 8)? == coords,
        "title coordinate source mismatch"
    );
    Ok(
        json!({"ram_sha256":sha(ram),"source_section_sha256":sha(section),"isa":"ARM946E-S ARM state, retro-typed-isa pinned in Cargo.lock","regions":regions,"coordinates_address":0x020f7048,"coordinates":[[100,102],[130,102],[158,100]],"tail_bytes":hex::encode(&coords[6..]),"claim":"static exact-source code and coordinates; three-iteration loop r8=2..0, 32x96 linear / three 512-byte tiled cells; no code write or new layout adoption; compression and stored-image mapping unresolved"}),
    )
}
