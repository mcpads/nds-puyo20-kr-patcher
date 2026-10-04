//! Full Hangul supply experiment and optional development plan; input method unchanged.
use crate::{assets::json_file, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

pub fn prepare(rom: &Rom, font_path: &Path, out: &Path) -> Result<Value> {
    let mut report = inspect(rom, font_path, out)?;
    let mut replacements = Vec::new();
    for name in ["lc_font_8.bin", "unicode_tbl.bin"] {
        let path = format!("lc_font/{name}");
        let bytes = fs::read(out.join(name))?;
        replacements.push(json!({"file":path,"expected_sha256":sha(rom.data(rom.file(&path)?)),"input":name,"input_sha256":sha(&bytes),"placement":"ff_tail"}));
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":replacements}),
    )?;
    report["claim"] = json!(
        "Experimental development plan; character supply verified statically; runtime heap, timing, persistence and human review pending"
    );
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

pub fn inspect(rom: &Rom, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let original = rom.data(rom.file("lc_font/lc_font_8.bin")?);
    let original_table = rom.data(rom.file("lc_font/unicode_tbl.bin")?);
    ensure!(
        sha(original) == "4b5d5449ff6ab5ba24f0ba29f7a2e9800029a3bc4cb661a7162416a184642be6"
            && sha(original_table)
                == "c7b07819f26ab8a6a460e77b0df93f61823fbf9f8e748b0a23845aa40dbdbbec",
        "source name font changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        sha(&font_bytes) == "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
        "expected registered Galmuri7"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(anyhow::Error::msg)?;
    let additions: Vec<u32> = (0xac00..=0xd7a3)
        .chain(0x3131..=0x3163)
        .chain([0x20])
        .collect();
    let count = 388 + additions.len();
    let mut packed = original.to_vec();
    packed.resize(count.div_ceil(8) * 64, 0);
    let mut table = original_table.to_vec();
    let mut all_bits = Vec::with_capacity(additions.len() * 64);
    let mut widest = 0;
    let mut tallest = 0;
    for (ordinal, &unit) in additions.iter().enumerate() {
        let ch = char::from_u32(unit).unwrap();
        ensure!(font.lookup_glyph_index(ch) != 0, "missing syllable {ch}");
        let (metrics, pixels) = font.rasterize(ch, 8.0);
        let top = 7 - metrics.ymin - metrics.height as i32;
        ensure!(
            metrics.xmin >= 0
                && top >= 0
                && metrics.xmin as usize + metrics.width <= 8
                && top as usize + metrics.height <= 8
                && metrics.advance_width <= 8.0,
            "syllable exceeds 8x8: {ch}"
        );
        ensure!(
            pixels.iter().all(|&p| p == 0 || p == 255),
            "non-binary native raster: {ch}"
        );
        if unit == 0x20 {
            ensure!(pixels.iter().all(|&p| p == 0), "space has ink");
        } else {
            ensure!(pixels.contains(&255), "empty Hangul character {ch}");
        }
        widest = widest.max(metrics.width);
        tallest = tallest.max(metrics.height);
        let mut bits = [0u8; 64];
        for y in 0..metrics.height {
            for x in 0..metrics.width {
                bits[(top as usize + y) * 8 + metrics.xmin as usize + x] =
                    pixels[y * metrics.width + x] / 255;
            }
        }
        let index = 388 + ordinal;
        for (p, &bit) in bits.iter().enumerate() {
            let byte = &mut packed[index / 8 * 64 + p];
            *byte = (*byte & !(1 << (index % 8))) | (bit << (index % 8));
        }
        all_bits.extend(bits);
        table.extend((unit as u16).to_le_bytes());
    }
    // Read serialized output again; original characters retain their exact planes.
    for i in 0..count {
        for p in 0..64 {
            let actual = (packed[i / 8 * 64 + p] >> (i % 8)) & 1;
            let expected = if i < 388 {
                (original[i / 8 * 64 + p] >> (i % 8)) & 1
            } else {
                all_bits[(i - 388) * 64 + p]
            };
            ensure!(actual == expected, "serialized glyph differs at {i}:{p}");
        }
    }
    ensure!(
        table[..original_table.len()] == *original_table
            && table.len() == count * 2
            && count % 2 == 0,
        "table identity or lookup parity changed"
    );
    let units: Vec<u16> = table
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    ensure!(
        units.iter().copied().collect::<BTreeSet<_>>().len() == count,
        "duplicate code point"
    );
    let mut maximum_comparisons = 0;
    let mut original_maximum_comparisons = 0;
    for (expected, &unit) in units.iter().enumerate() {
        let mut found = None;
        let mut comparisons = 0;
        for pair in 0..count / 2 {
            for index in [pair, count - pair - 1] {
                comparisons += 1;
                if units[index] == unit {
                    found = Some(index);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        ensure!(
            found == Some(expected),
            "lookup model cannot reach U+{unit:04X}"
        );
        maximum_comparisons = maximum_comparisons.max(comparisons);
        if expected < 388 {
            original_maximum_comparisons = original_maximum_comparisons.max(comparisons);
        }
    }
    let sample = "가나다라마바사아자차카타파하한글이름뿌요힣꿻뛟쀍ㄱㄲㄳㅏㅙㅢ";
    let width = sample.chars().count() * 10;
    let mut rgba = vec![0u8; width * 10 * 4];
    for (i, ch) in sample.chars().enumerate() {
        let index = additions.iter().position(|&u| u == ch as u32).unwrap();
        for p in 0..64 {
            let v = all_bits[index * 64 + p] * 255;
            let pos = ((p / 8 + 1) * width + i * 10 + p % 8 + 1) * 4;
            rgba[pos..pos + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    let report = json!({"source_sha256":sha(rom.bytes),"font_sha256":sha(&fs::read(font_path)?),"source_characters":388,"added_syllables":11172,"added_compatibility_jamo":51,"added_blank_space":"U+0020","total_characters":count,"font_bytes":packed.len(),"table_bytes":table.len(),"additional_payload_bytes":packed.len()+table.len()-original.len()-original_table.len(),"font_candidate_sha256":sha(&packed),"table_candidate_sha256":sha(&table),"native_size":8,"baseline":7,"maximum_ink_width":widest,"maximum_ink_height":tallest,"all_hangul_binary_and_nonempty":true,"source_glyph_planes_exact":true,"serialized_all_glyphs_exact":true,"lookup_model":{"all_indices_reached":true,"maximum_comparisons":maximum_comparisons,"original_maximum_comparisons":original_maximum_comparisons,"claim":"host model of original two-ended lookup; not ARM execution or timing evidence"},"sample":sample,"claim":"Analysis candidate only; no ROM insertion, runtime heap or timing proof, Hangul input, persistence or human approval"});
    fs::create_dir_all(out)?;
    fs::write(out.join("lc_font_8.bin"), packed)?;
    fs::write(out.join("unicode_tbl.bin"), table)?;
    write_png(&out.join("sample.png"), width, 10, &rgba)?;
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
