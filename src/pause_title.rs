use crate::{
    assets::json_file,
    battle_ui,
    build::{Arm9Mode, Write, apply},
    format::*,
    graphics::write_png,
    titles,
};
use anyhow::{Result, ensure};
use arm7tdmi::{
    AddressingMode3, ArmInstruction as BaseArm, Condition, DataOperation, HalfwordOffset,
    HalfwordTransferKind, ImmediateShift, IndexMode, Operand2, Register, RotatedImmediate,
};
use arm946e_s::{ArmInstruction, decode_arm_bytes, encode_arm_bytes};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const ARCHIVE: &str = "puyo/menu/puyo_menu.narc";
const X: [u8; 4] = [86, 114, 142, 170];

pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    battle_ui::member_residency(rom, ram, &[(ARCHIVE, vec![6, 8])])
}

fn data_op(
    operation: DataOperation,
    destination: u8,
    first: u8,
    second: Operand2,
) -> Result<ArmInstruction> {
    Ok(ArmInstruction::Armv4T(BaseArm::DataProcessing {
        condition: Condition::Always,
        operation,
        set_flags: false,
        destination: Register::new(destination)?,
        first: Register::new(first)?,
        second,
    }))
}
fn immediate(value: u8, rotation: u8) -> Result<Operand2> {
    Ok(Operand2::Immediate(RotatedImmediate::new(value, rotation)?))
}

/// Four exact ARM-state writes; no branch displacement or literal-pool changes.
pub(crate) fn rewrite_code(source: &[u8]) -> Result<(Vec<u8>, Value)> {
    let mut coordinates = Vec::new();
    for (i, x) in X.into_iter().enumerate() {
        coordinates.push(data_op(DataOperation::Move, 7, 0, immediate(102, 24)?)?);
        coordinates.push(data_op(DataOperation::Or, 7, 7, immediate(x, 0)?)?);
        coordinates.push(ArmInstruction::Armv4T(BaseArm::HalfwordTransfer {
            condition: Condition::Always,
            load: false,
            kind: HalfwordTransferKind::UnsignedHalfword,
            register: Register::new(7)?,
            address: AddressingMode3 {
                base: Register::SP,
                offset: HalfwordOffset::Immediate(0x2c + i as u8 * 2),
                add: true,
                index: IndexMode::Offset,
            },
        }));
    }
    coordinates.push(data_op(
        DataOperation::Move,
        6,
        0,
        Operand2::Register {
            register: Register::new(0)?,
            shift: ImmediateShift::LogicalLeft(0),
        },
    )?);
    let edits = [
        (
            0x673e8,
            "6010a0e3",
            vec![data_op(DataOperation::Move, 1, 0, immediate(128, 0)?)?],
            "linear title height 128",
        ),
        (
            0x67954,
            "70139fe5",
            vec![data_op(DataOperation::Move, 3, 0, immediate(102, 0)?)?],
            "preserve source r3 at first allocator call",
        ),
        (
            0x6795c,
            "0070d1e50150d1e50240d1e50330d1e50420d1e50510d1e52c70cde50060a0e12d50cde52e40cde52f30cde53020cde53110cde5",
            coordinates,
            "four coordinate halfwords and preserved r6=r0",
        ),
        (
            0x67994,
            "0280a0e3",
            vec![data_op(DataOperation::Move, 8, 0, immediate(3, 0)?)?],
            "four iterations r8=3..0",
        ),
    ];
    let mut writes = Vec::new();
    let mut records = Vec::new();
    for (offset, before_hex, instructions, label) in edits {
        let before = hex::decode(before_hex)?;
        let mut after = Vec::new();
        let mut typed = Vec::new();
        for (i, instruction) in instructions.iter().enumerate() {
            let encoded = encode_arm_bytes(instruction)?;
            ensure!(
                decode_arm_bytes(&encoded)? == *instruction,
                "title generated instruction identity mismatch"
            );
            after.extend(encoded);
            typed.push(json!({"address":0x02000000+offset+i*4,"instruction":format!("{instruction:?}"),"bytes":hex::encode(encoded)}));
        }
        for word in before.chunks_exact(4) {
            ensure!(
                encode_arm_bytes(&decode_arm_bytes(word)?)? == word,
                "title source instruction round trip failed"
            );
        }
        ensure!(
            before.len() == after.len(),
            "title instruction span changed"
        );
        records.push(json!({"address":0x02000000+offset,"decoded_offset":offset,"label":label,"before":before_hex,"after":hex::encode(&after),"instructions":typed}));
        writes.push(Write {
            offset,
            before,
            after,
            label: label.into(),
        });
    }
    Ok((apply(source, &mut writes)?, json!(records)))
}

pub(crate) fn verify_pair(source: &Rom, target: &Rom, enabled: bool) -> Result<()> {
    let Ok(entry) = source.file(ARCHIVE) else {
        ensure!(
            !enabled && target.file(ARCHIVE).is_err(),
            "missing pause title source archive"
        );
        return Ok(());
    };
    let before = Narc::parse(source.data(entry))?;
    let after = Narc::parse(target.data(target.file(ARCHIVE)?))?;
    ensure!(
        before.members.len() == 46 && after.members.len() == 46,
        "pause title member population mismatch"
    );
    let changed = before.members[6] != after.members[6] || before.members[8] != after.members[8];
    ensure!(
        changed || !enabled,
        "pause title code and graphics must be installed together"
    );
    if changed {
        let height = if enabled { 128 } else { 96 };
        ensure!(
            after.members[7] == before.members[7],
            "pause title palette changed"
        );
        let linear = unpack_halfword(after.members[6])?;
        let tiled = unpack_halfword(after.members[8])?;
        ensure!(
            linear.len() == 32 * height / 2 && tiled.len() == 32 * height / 2,
            "pause title cells must match the selected code layout"
        );
        ensure!(
            titles::tile(&battle_ui::indices(&linear), 32, height, 4)? == tiled,
            "pause title storage pair differs"
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    japanese: String,
    korean: String,
    linear_sha256: String,
    tiled_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artwork {
    translation_sha256: String,
    image: String,
    image_sha256: String,
    letters: Vec<String>,
    cells: Vec<[usize; 4]>,
    palette_indices: Vec<Vec<usize>>,
}

fn generated_pixels(
    path: &Path,
    translation: &[u8],
    korean: &str,
    palette: &[u8],
) -> Result<(Vec<u8>, Value)> {
    let input = fs::read(path)?;
    let art: Artwork = serde_json::from_slice(&input)?;
    ensure!(
        art.translation_sha256 == sha(translation)
            && art.cells.len() == korean.chars().count()
            && art.palette_indices.len() == korean.chars().count()
            && art
                .palette_indices
                .iter()
                .all(|indices| !indices.is_empty() && indices.iter().all(|&i| i > 0 && i < 16))
            && art.letters == korean.chars().map(|c| c.to_string()).collect::<Vec<_>>(),
        "pause artwork translation/cells mismatch"
    );
    let bytes = fs::read(&art.image)?;
    ensure!(sha(&bytes) == art.image_sha256, "pause artwork identity");
    let image = crate::art_pixels::read(&bytes)?;
    let mut pixels = Vec::new();
    for (&region, indices) in art.cells.iter().zip(&art.palette_indices) {
        let cell = crate::art_pixels::region(&image, region)?;
        let rgba = crate::art_pixels::reduce(&cell, 32, 32, true)?;
        for c in rgba.chunks_exact(4) {
            let index = if c[3] < 128 {
                0
            } else {
                indices
                    .iter()
                    .copied()
                    .min_by_key(|&i| {
                        let v = u16::from_le_bytes([palette[i * 2], palette[i * 2 + 1]]);
                        (0..3)
                            .map(|k| {
                                let delta =
                                    ((v >> (5 * k)) & 31) as i32 * 255 / 31 - i32::from(c[k]);
                                delta * delta
                            })
                            .sum::<i32>()
                    })
                    .unwrap()
            };
            pixels.push(index as u8);
        }
    }
    ensure!(
        pixels
            .iter()
            .enumerate()
            .all(|(p, &v)| v == 0
                || (p % 32 > 0 && p % 32 < 31 && p / 32 % 32 > 0 && p / 32 % 32 < 31)),
        "generated pause glyph reaches cell boundary"
    );
    Ok((
        pixels,
        json!({"spec_sha256":sha(&input),"image":art.image,"image_sha256":art.image_sha256,"letters":art.letters,"cells":art.cells,"palette_indices":art.palette_indices,"glyph_size":[32,32]}),
    ))
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    artwork: Option<&Path>,
    graphics_only: bool,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let tr_bytes = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&tr_bytes)?;
    ensure!(
        tr.state == "development_art_draft"
            && tr.japanese == "ポーズ"
            && matches!(tr.korean.chars().count(), 3 | 4),
        "pause title draft population mismatch"
    );
    let count = tr.korean.chars().count();
    let height = count * 32;
    let mode = if count == 3 {
        Arm9Mode::RecompressedBaseline
    } else {
        Arm9Mode::PauseTitle
    };
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
    let old_linear = unpack_halfword(n.members[6])?;
    let old_tiled = unpack_halfword(n.members[8])?;
    let palette = unpack(n.members[7])?;
    ensure!(
        sha(&old_linear) == tr.linear_sha256
            && sha(&old_tiled) == tr.tiled_sha256
            && old_linear.len() == 1536
            && old_tiled.len() == 1536
            && sha(&palette) == "67c573ebeacc8cdefca0dac137cef543a63b9eab1d3875e09b2c4e5fcc7b81a4",
        "title source identity mismatch"
    );
    ensure!(
        titles::tile(&battle_ui::indices(&old_linear), 32, 96, 4)? == old_tiled,
        "source title pair mismatch"
    );
    let mut pixels = Vec::new();
    let artwork_report = if let Some(path) = artwork {
        let (generated, report) = generated_pixels(path, &tr_bytes, &tr.korean, &palette)?;
        pixels = generated;
        Some(report)
    } else {
        None
    };
    for (letter, fill) in tr
        .korean
        .chars()
        .zip([9, 13, 15, 6])
        .filter(|_| artwork.is_none())
    {
        let mut cell = vec![0u8; 1024];
        let ink = battle_ui::text_ink(&font, &letter.to_string(), 22, [2, 2, 30, 30], 25)?;
        // Preserve the title's colored ink, dark inner edge, light outer edge.
        for (radius, color) in [(2, 7), (1, 1), (0, fill)] {
            for &(x, y) in &ink {
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx * dx + dy * dy <= radius * radius + 1 {
                            cell[(y as i32 + dy) as usize * 32 + (x as i32 + dx) as usize] = color;
                        }
                    }
                }
            }
        }
        ensure!(
            cell.iter()
                .enumerate()
                .all(|(p, &v)| v == 0 || (p % 32 > 0 && p % 32 < 31 && p / 32 > 0 && p / 32 < 31)),
            "title outline reaches cell boundary"
        );
        pixels.extend(cell);
    }
    let linear = battle_ui::pack(&pixels)?;
    let tiled = titles::tile(&pixels, 32, height, 4)?;
    ensure!(
        titles::untile(&tiled, 32, height, 4)? == pixels,
        "title tile round trip failed"
    );
    let mut changes = BTreeMap::new();
    for (id, raw) in [(6, &linear), (8, &tiled)] {
        changes.insert(id, crate::compress::pack_compact(raw)?);
    }
    let archive = battle_ui::rebuilt(source, &changes)?;
    // A three-glyph title keeps the original code, so the stored ARM9 stays as is.
    let code = if graphics_only || mode == Arm9Mode::RecompressedBaseline {
        None
    } else {
        Some(crate::arm9::prepare_image(rom.bytes, mode)?)
    };
    fs::create_dir_all(out)?;
    fs::write(out.join("title.narc"), &archive)?;
    write_png(
        &out.join("title.png"),
        32,
        height,
        &titles::rgba(&pixels, &palette)?,
    )?;
    json_file(
        &out.join("graphics-plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"title.narc","input_sha256":sha(&archive)}]}),
    )?;
    if let Some((code, _)) = &code {
        fs::write(out.join("arm9.bin"), code)?;
        json_file(
            &out.join("code-plan.json"),
            &json!({"source_sha256":sha(rom.bytes),"replacements":[],"arm9":{"mode":mode,"expected_sha256":sha(slice(rom.bytes,0x4000,0xa5888)?),"input":"arm9.bin","input_sha256":sha(code)}}),
        )?;
    }
    let code_report = code.map(|(_, report)| report).unwrap_or(Value::Null);
    let mut report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&tr_bytes),"korean":tr.korean,"font_size":22,"baseline":25,"coordinates":if count == 3 { vec![[100,102],[130,102],[158,100]] } else { X.map(|x|[x,102]).to_vec() },"phase_step_preserved":0x2aaa,"glyph_count":count,"writes":changes.iter().map(|(&id,b)|json!({"member":id,"stored_size":b.len(),"capacity":n.members[id].len(),"decoded_sha256":sha(&unpack_halfword(b).unwrap())})).collect::<Vec<_>>(),"code":code_report,"protected":"palette, all other members and padding, original coordinate table including trailing bytes, phase step, every code byte outside four declared spans","runtime_verified":false,"human_reviewed":false});
    if let Some(art) = artwork_report {
        report["artwork"] = art;
        report.as_object_mut().unwrap().remove("font_size");
        report.as_object_mut().unwrap().remove("baseline");
    }
    if graphics_only {
        report["graphics_only"] = json!(true);
        report["required_arm9_mode"] = json!(mode);
    }
    json_file(&out.join("title.json"), &report)?;
    Ok(report)
}
