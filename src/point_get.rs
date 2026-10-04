//! Result title: shared linear artwork and two palette-specific OBJ sets.
mod labels;
use crate::{assets::json_file, battle_ui, format::*, graphics::write_png, titles};
use anyhow::{Result, ensure};
pub use labels::prepare as prepare_labels;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const ARCHIVE: &str = "puyo/result/result.narc";
/// Light band inside the dotted frame, in 159-pixel panel coordinates.
const BAND: [usize; 4] = [11, 11, 148, 30];
const SOURCE: &str = "9d0a2e5bb8a61b38e95fdc8663eec10d4a8c0097842c65048a1e5833f9cddad1";

pub fn panel_residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    battle_ui::member_residency(
        rom,
        ram,
        &[(
            ARCHIVE,
            vec![71, 73, 74, 125, 127, 128, 130, 132, 134, 135, 136, 138],
        )],
    )
}

pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    battle_ui::member_residency(
        rom,
        ram,
        &[(ARCHIVE, vec![125, 127, 128, 130, 132, 134, 135, 136, 138])],
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    japanese: String,
    korean: String,
}

fn nearest(palette: &[u8], target: [i32; 3]) -> Result<u8> {
    let mut colors = Vec::new();
    for i in 1..16 {
        let c = crate::buttons::rgb(palette, i)?;
        colors.push((
            c.iter()
                .zip(target)
                .map(|(a, b)| (a - b).pow(2))
                .sum::<i32>(),
            i as u8,
        ));
    }
    colors.sort_unstable();
    Ok(colors[0].1)
}

fn check_layout(
    rom: &Rom,
    n: &Narc,
    gem_id: usize,
    ilf_name: &str,
    expected: [usize; 3],
) -> Result<()> {
    let b = unpack(n.members[gem_id])?;
    let ilf = rom.data(rom.file(ilf_name)?);
    ensure!(&b[..4] == b"Gem1", "point title Gem signature");
    let base = u32le(&b, 20)?;
    let scene = base + u32le(&b, u32le(&b, 24)?)?;
    let bank = base + u32le(&b, scene + 20)?;
    let nodes = base + u32le(&b, bank + 4)?;
    let mut positions = Vec::new();
    for (i, expected_member) in expected.into_iter().enumerate() {
        ensure!(
            (u32le(ilf, i * 4)? >> 8) & 4095 == expected_member,
            "point title ILF order"
        );
        let node = nodes + (i + 1) * 80;
        let x = u32le(&b, node + 32)? as i32 - u32le(&b, node + 8)? as i32;
        let y = u32le(&b, node + 36)? as i32 - u32le(&b, node + 12)? as i32;
        ensure!(
            x % 4096 == 0 && y % 4096 == 0,
            "fractional point title layout"
        );
        positions.push((x / 4096, y / 4096));
    }
    ensure!(
        positions[0].0 - positions[1].0 == 63
            && positions[2].0 - positions[1].0 == 127
            && positions.iter().all(|p| p.1 == positions[0].1),
        "point title placement changed"
    );
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artwork {
    translation_sha256: String,
    image: String,
    image_sha256: String,
    region: [usize; 4],
    #[serde(default)]
    palette_indices: Vec<usize>,
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    artwork: Option<&Path>,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(sha(source) == SOURCE, "point title source changed");
    let n = Narc::parse(source)?;
    ensure!(n.members.len() == 578, "result archive population changed");
    check_layout(
        rom,
        &n,
        69,
        "puyo/result/point_get_ilf.bin",
        [136, 135, 138],
    )?;
    check_layout(
        rom,
        &n,
        169,
        "puyo/result/point_get_top_s_ilf.bin",
        [128, 125, 132],
    )?;
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    let generated = artwork.map(|path| -> Result<_> {
        let spec = fs::read(path)?;
        let art: Artwork = serde_json::from_slice(&spec)?;
        ensure!(art.translation_sha256 == sha(&input), "point artwork translation identity");
        ensure!(art.palette_indices.iter().all(|&i| i > 0 && i < 16), "point artwork palette subset");
        let bytes = fs::read(&art.image)?;
        ensure!(sha(&bytes) == art.image_sha256, "point artwork image identity");
        let image = crate::art_pixels::region(&crate::art_pixels::read(&bytes)?, art.region)?;
        let rgba = crate::art_pixels::reduce(&image, 159, 41, false)?;
        let report = json!({"spec_sha256":sha(&spec),"image":art.image,"image_sha256":art.image_sha256,"region":art.region,"palette_indices":art.palette_indices,"whole_panel_size":[159,41],"reduced_rgba_sha256":sha(&rgba)});
        Ok((rgba, art.palette_indices, report))
    }).transpose()?;
    ensure!(
        tr.state == "development_art_draft" && tr.japanese == "かくとくポイント",
        "point title draft identity"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        sha(&font_bytes) == crate::fonts::GALMURI14_SHA256,
        "point title font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    // Galmuri14 on its 15px grid, one extra column per stroke for the source's
    // two-pixel strokes, centred in the light band between the dotted rows.
    let ink = battle_ui::text_ink(&font, &tr.korean, 15, [0, 0, 159, 41], 30)?;
    let ink: Vec<_> = ink
        .into_iter()
        .flat_map(|(x, y)| [(x, y), (x + 1, y)])
        .collect();
    let (x_min, x_max) = (
        ink.iter().map(|p| p.0).min().unwrap(),
        ink.iter().map(|p| p.0).max().unwrap(),
    );
    let (y_min, y_max) = (
        ink.iter().map(|p| p.1).min().unwrap(),
        ink.iter().map(|p| p.1).max().unwrap(),
    );
    let [bx0, by0, bx1, by1] = BAND;
    let dx = (bx0 + bx1 - 1) as i32 / 2 - (x_min + x_max) as i32 / 2;
    let dy = (by0 + by1 - 1) as i32 / 2 - (y_min + y_max) as i32 / 2;
    let ink: Vec<_> = ink
        .into_iter()
        .map(|(x, y)| ((x as i32 + dx) as usize, (y as i32 + dy) as usize))
        .collect();
    ensure!(
        ink.iter()
            .all(|&(x, y)| x > bx0 && x + 1 < bx1 && y > by0 && y + 1 < by1),
        "point title ink outside band"
    );
    let mut mask = vec![0u8; 159 * 41];
    for &(x, y) in &ink {
        for oy in y - 1..=y + 1 {
            for ox in x - 1..=x + 1 {
                mask[oy * 159 + ox] = 2;
            }
        }
    }
    for &(x, y) in &ink {
        mask[y * 159 + x] = 1;
    }
    let mut changes = BTreeMap::new();
    let mut records = Vec::new();
    let mut previews = Vec::new();
    for (set, members, palettes, tiled) in [
        ("linear", [127, 130, 134], [126, 129, 133], false),
        ("top_obj", [125, 128, 132], [131, 131, 131], true),
        ("full_obj", [135, 136, 138], [137, 137, 137], true),
    ] {
        let mut rgba = vec![0; 159 * 64 * 4];
        let overlap_colors = if generated.is_some() {
            let first = unpack(n.members[palettes[0]])?;
            let second = unpack(n.members[palettes[1]])?;
            let color_set = |bytes: &[u8]| -> Vec<u16> {
                (1..16)
                    .filter(|i| {
                        generated
                            .as_ref()
                            .is_none_or(|(_, subset, _)| subset.is_empty() || subset.contains(i))
                    })
                    .map(|i| u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]))
                    .collect()
            };
            let second = color_set(&second);
            let shared: Vec<_> = color_set(&first)
                .into_iter()
                .filter(|c| second.contains(c))
                .collect();
            ensure!(
                !shared.is_empty(),
                "point artwork overlap has no shared colour"
            );
            shared
        } else {
            Vec::new()
        };
        for (part, (&member, &pal)) in members.iter().zip(&palettes).enumerate() {
            let width = [64, 64, 32][part];
            let origin = [0, 63, 127][part];
            let raw = unpack_halfword(n.members[member])?;
            let height = if tiled { 64 } else { 41 };
            ensure!(
                raw.len() == width * height / 2,
                "point title geometry changed"
            );
            let original = if tiled {
                titles::untile(&raw, width, height, 4)?
            } else {
                battle_ui::indices(&raw)
            };
            let mut pixels = original.clone();
            let palette = unpack(n.members[pal])?;
            ensure!(palette.len() == 32, "point title palette extent");
            let colors = [
                nearest(&palette, [10, 26, 31])?,
                nearest(&palette, [31, 31, 14])?,
                nearest(&palette, [4, 17, 25])?,
            ];
            ensure!(
                colors[0] != colors[1] && colors[1] != colors[2] && colors[0] != colors[2],
                "point title colors collide"
            );
            let mut edited = 0;
            for y in 0..height {
                for x in 0..width {
                    let gx = origin + x;
                    let editable = if generated.is_some() {
                        y < 41
                    } else {
                        (BAND[0]..BAND[2]).contains(&gx) && (BAND[1]..BAND[3]).contains(&y)
                    };
                    let p = y * width + x;
                    if editable {
                        pixels[p] = if let Some((rgba, subset, _)) = &generated {
                            let c = &rgba[(y * 159 + gx) * 4..][..4];
                            if c[3] < 128 {
                                0
                            } else {
                                (1..16)
                                    .filter(|i| subset.is_empty() || subset.contains(i))
                                    .filter(|&i| {
                                        gx != 63
                                            || overlap_colors.contains(&u16::from_le_bytes([
                                                palette[i * 2],
                                                palette[i * 2 + 1],
                                            ]))
                                    })
                                    .min_by_key(|&i| {
                                        let v = u16::from_le_bytes([
                                            palette[i * 2],
                                            palette[i * 2 + 1],
                                        ]);
                                        (0..3)
                                            .map(|k| {
                                                let delta = ((v >> (5 * k)) & 31) as i32 * 255 / 31
                                                    - i32::from(c[k]);
                                                delta * delta
                                            })
                                            .sum::<i32>()
                                    })
                                    .unwrap() as u8
                            }
                        } else {
                            colors[mask[y * 159 + gx] as usize]
                        };
                        edited += 1;
                    } else {
                        ensure!(
                            pixels[p] == original[p],
                            "protected point title pixel changed"
                        );
                    }
                }
            }
            let bytes = if tiled {
                titles::tile(&pixels, width, height, 4)?
            } else {
                battle_ui::pack(&pixels)?
            };
            battle_ui::compress_member(&mut changes, member, &bytes)?;
            let image = titles::rgba(&pixels, &palette)?;
            for y in 0..height {
                for x in 0..width {
                    let p = (y * 159 + origin + x) * 4;
                    rgba[p..p + 4]
                        .copy_from_slice(&image[(y * width + x) * 4..(y * width + x + 1) * 4]);
                }
            }
            records.push(json!({"set":set,"member":member,"palette_member":pal,"origin_x":origin,"width":width,"height":height,"colors":colors,"edited_pixels":edited,"protected_pixels":pixels.len()-edited,"source_sha256":sha(&raw),"decoded_sha256":sha(&bytes),"stored_size":changes[&member].len(),"capacity":n.members[member].len()}));
        }
        previews.push((set, rgba));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("result.narc"), &rebuilt)?;
    for (name, rgba) in previews {
        write_png(&out.join(format!("{name}.png")), 159, 64, &rgba)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":SOURCE,"input":"result.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let mut report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"title":tr.korean,"archive_sha256":sha(&rebuilt),"records":records,"protected":"frame, dotted rows, band edges, palette, source pixels outside the band interior, padding rows, Gem/ILF/DSIF, numbers and all other archive members","runtime_verified":false,"human_reviewed":false});
    if let Some((_, _, artwork_report)) = generated {
        report["artwork"] = artwork_report;
        report["protected"] = json!(
            "palettes, OBJ rows 41..64, original overlapping Gem/ILF placement, DSIF, numbers and all other archive members; complete 159x41 panel regenerated"
        );
    }
    json_file(&out.join("title.json"), &report)?;
    Ok(report)
}
