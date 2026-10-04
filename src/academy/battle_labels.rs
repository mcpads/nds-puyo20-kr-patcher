//! Lettering only: preserve the battle portraits and overlapping sprite placement.
use super::*;
use crate::titles;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    state: String,
    entries: Vec<Label>,
    #[serde(default)]
    source_sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    id: String,
    japanese: String,
    korean: Vec<String>,
    #[serde(default)]
    artwork: Option<titles::Artwork>,
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let path = "academy/academy.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b0c7244f4d3de4d9836a1e6607097bffdbfb63644f8e01e28f191f0ddd0a7ed3",
        "academy archive changed"
    );
    let n = Narc::parse(source)?;
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    let generated = tr.entries.iter().any(|label| label.artwork.is_some());
    ensure!(
        tr.state == "development_art_draft",
        "unexpected battle label state"
    );
    ensure!(
        if generated {
            matches!(tr.entries.len(), 5 | 6)
                && tr.entries.iter().all(|label| label.artwork.is_some())
                && tr.source_sha256.as_deref() == Some(sha(rom.bytes).as_str())
        } else {
            tr.entries.len() == 6 && tr.source_sha256.is_none()
        },
        "expected six font labels or five/six source-bound generated labels"
    );
    let bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&bytes), "font changed");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut changes = BTreeMap::new();
    let mut previews = Vec::new();
    let mut records = Vec::new();
    for (label, (id, japanese, left, right, pal, offset)) in tr.entries.iter().zip([
        ("fev", "あかいアミティ", 143, 145, 144, 63),
        ("nazo", "あやしいクルーク", 146, 148, 147, 63),
        ("puyo2", "くろいシグ", 137, 139, 138, 63),
        ("puyo3", "きいろいサタン", 140, 142, 141, 63),
        ("puyo", "しろいフェーリ", 134, 136, 135, 63),
        ("common", "チャレンジたいせん", 190, 192, 191, 64),
    ]) {
        ensure!(
            label.id == id && label.japanese == japanese && label.korean.len() == 2,
            "battle label identity changed"
        );
        let palette = unpack(n.members[pal])?;
        ensure!(palette.len() == 32, "battle label palette changed");
        let mut used = Vec::new();
        for member in [left, right] {
            let raw = unpack_halfword(n.members[member])?;
            let pixels = titles::untile(&raw, 64, 64, 4)?;
            ensure!(
                titles::tile(&pixels, 64, 64, 4)? == raw,
                "source tile round trip"
            );
            used.extend(pixels);
        }
        let mut colors = Vec::new();
        for i in 1..16 {
            if used.contains(&(i as u8)) {
                let c = crate::buttons::rgb(&palette, i)?;
                colors.push((
                    i as u8,
                    c.iter().map(|v| v * v).sum::<i32>(),
                    c.iter().map(|v| (31 - v).pow(2)).sum::<i32>(),
                ));
            }
        }
        let dark = colors
            .iter()
            .min_by_key(|c| c.1)
            .ok_or_else(|| anyhow::anyhow!("empty source lettering"))?
            .0;
        let white = colors.iter().min_by_key(|c| c.2).unwrap().0;
        ensure!(dark != white, "missing contrast");
        let width = offset + 64;
        let pixels = if let Some(art) = &label.artwork {
            titles::generated_pixels(width, 64, &palette, art)?
        } else {
            let mut ink = Vec::new();
            for (line, text) in label.korean.iter().enumerate() {
                for (x, y) in
                    battle_ui::text_ink(&font, text, 12, [0, 0, width / 2, 32], 14 + line * 12)?
                {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            ink.push((x * 2 + dx, y * 2 + dy));
                        }
                    }
                }
            }
            let mut pixels = vec![0u8; width * 64];
            for &(x, y) in &ink {
                ensure!(
                    x >= 2 && x + 2 < width && y >= 2 && y + 3 < 64,
                    "lettering margin exceeded"
                );
                for dy in -1i32..=2 {
                    for dx in -1i32..=1 {
                        pixels[(y as i32 + dy) as usize * width + (x as i32 + dx) as usize] = dark;
                    }
                }
            }
            for (x, y) in ink {
                pixels[y * width + x] = white;
            }
            pixels
        };
        let a = battle_ui::crop(&pixels, width, 0, 0, 64, 64);
        let b = battle_ui::crop(&pixels, width, offset, 0, 64, 64);
        if offset == 63 {
            for y in 0..64 {
                ensure!(a[y * 64 + 63] == b[y * 64], "overlapping columns disagree");
            }
        }
        for (member, p) in [(left, a), (right, b)] {
            let raw = titles::tile(&p, 64, 64, 4)?;
            ensure!(
                titles::untile(&raw, 64, 64, 4)? == p,
                "prepared tile round trip"
            );
            battle_ui::compress_member(&mut changes, member, &raw)?;
        }
        previews.push((id, width, titles::rgba(&pixels, &palette)?));
        let mut record = json!({"id":id,"japanese":japanese,"korean":label.korean,"members":[left,right],"palette_member":pal,"width":width,"height":64,"overlap_columns":64-offset,"ink_index":white,"outline_index":dark,"stored_sizes":[changes[&left].len(),changes[&right].len()],"capacities":[n.members[left].len(),n.members[right].len()]});
        if let Some(art) = &label.artwork {
            record.as_object_mut().unwrap().remove("ink_index");
            record.as_object_mut().unwrap().remove("outline_index");
            record["artwork"] = serde_json::to_value(art)?;
        }
        records.push(record);
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("labels.narc"), &rebuilt)?;
    for (id, w, pixels) in previews {
        write_png(&out.join(format!("{id}.png")), w, 64, &pixels)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"labels.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&rebuilt),"labels":records,"protected":"all portraits, Gem/ILF, palettes, symbol assets and all non-label members","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("labels.json"), &report)?;
    Ok(report)
}

/// Diagnose label residency; missing labels in another selected mode are reported.
pub fn check_ram(rom: &Rom, ram: &[u8]) -> Result<Value> {
    battle_ui::member_residency(
        rom,
        ram,
        &[(
            "academy/academy.narc",
            vec![134, 136, 137, 139, 140, 142, 143, 145, 146, 148, 190, 192],
        )],
    )
}
