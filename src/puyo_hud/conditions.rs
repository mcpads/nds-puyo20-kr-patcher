//! Condition fragments share a texture with the color-puyo backing circle.
use super::*;
use crate::battle_ui;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    state: String,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    japanese: String,
    korean: String,
    /// Word-space width in pixels (default 5) for phrases that fill their slot.
    #[serde(default)]
    space_width: Option<usize>,
    /// Draw with the narrow font (Galmuri11 Condensed 12px, 8px advance) plus
    /// 1px tracking and gap-keeping bold, for phrases too long for DenkiChip's
    /// 10px cells: the bold matches the other lines' two-pixel stems.
    #[serde(default)]
    narrow: bool,
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    narrow_font_path: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let renderer = super::renderer::verify(rom, None)?;
    let path = "puyo/puyo2P/puyo2P.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b338518e1710dee5f6546deabeb3aa367bdaff2935146c4807e8b9d57c54c510",
        "HUD archive changed"
    );
    let n = Narc::parse(source)?;
    let original = unpack_halfword(n.members[800])?;
    let palette = unpack(n.members[801])?;
    ensure!(
        original.len() == 2560 && palette.len() == 6,
        "condition geometry changed"
    );
    let input = fs::read(translation)?;
    let tr: Labels = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 10,
        "expected ten condition fragments"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        // DenkiChip uses a native 12px grid with 10px advance, fitting the source 16px rows.
        sha(&font_bytes) == "4589cb1a59bcbd669ad7ac0669827e5a4d411832048e9bdd9618c907c1a8d272",
        "font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let narrow_font = narrow_font_path
        .map(|path| -> Result<fontdue::Font> {
            let bytes = fs::read(path)?;
            ensure!(
                // Galmuri11 Condensed at its native 12px grid.
                sha(&bytes) == "7b433b4a007c36dfb535fdea11de3e4f4c8b641ab591ed05d3bc0a4bbd75eb5f",
                "narrow font changed"
            );
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;
    let old = original
        .iter()
        .flat_map(|b| [b & 3, (b >> 2) & 3, (b >> 4) & 3, b >> 6])
        .collect::<Vec<_>>();
    ensure!(
        old.iter().all(|p| *p < 3),
        "undefined condition palette index"
    );
    let mut pixels = old.clone();
    let mut allowed = vec![false; pixels.len()];
    let mut entries = Vec::new();
    for (row, (label, (japanese, width, size))) in tr
        .entries
        .iter()
        .zip([
            ("ちょうど", 48, 12),
            ("連鎖", 32, 12),
            ("連鎖以上", 48, 12),
            ("色以上", 48, 12),
            ("個以上", 48, 12),
            ("同時消し", 48, 12),
            ("ぷよを", 48, 12),
            ("個消す", 48, 12),
            ("の", 16, 12),
            ("位置で消す", 64, 12),
        ])
        .enumerate()
    {
        ensure!(
            label.japanese == japanese,
            "condition fragment identity changed"
        );
        let y = row * 16;
        for line in y..y + 16 {
            for x in 0..width {
                pixels[line * 64 + x] = 0;
                allowed[line * 64 + x] = true;
            }
        }
        let space = label.space_width.unwrap_or(5);
        ensure!((2..=5).contains(&space), "condition space width");
        let ink = if label.narrow {
            let narrow = narrow_font
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("narrow condition needs --narrow-font"))?;
            battle_ui::text_ink_tracked(
                narrow,
                &label.korean,
                size,
                [0, y, width, y + 16],
                y + 12,
                space,
                1,
                true,
            )?
        } else {
            battle_ui::text_ink_with_space(
                &font,
                &label.korean,
                size,
                [0, y, width, y + 16],
                y + 12,
                space,
            )?
        };
        battle_ui::paint(&mut pixels, 64, &ink, 2, 1);
        entries.push(json!({"japanese":japanese,"korean":label.korean,"crop":[0,y,width,16],"font_pixels":size,"narrow_font":label.narrow,"space_width":space}));
    }
    ensure!(
        old.iter()
            .zip(&pixels)
            .zip(&allowed)
            .all(|((a, b), ok)| a == b || *ok),
        "protected condition pixels changed"
    );
    let raw = pixels
        .chunks_exact(4)
        .map(|p| p[0] | p[1] << 2 | p[2] << 4 | p[3] << 6)
        .collect::<Vec<_>>();
    ensure!(
        raw.iter()
            .flat_map(|b| [b & 3, (b >> 2) & 3, (b >> 4) & 3, b >> 6])
            .eq(pixels.iter().copied()),
        "condition I2 round trip failed"
    );
    // The minimal-parse compressor keeps the fuller Korean crops in the
    // original stored size when the default parse does not.
    let mut packed = crate::compress::pack(&raw)?;
    if packed.len() > n.members[800].len() {
        packed = crate::compress::pack_compact(&raw)?;
    }
    ensure!(
        unpack_halfword(&packed)? == raw,
        "condition halfword round trip failed"
    );
    let changes = BTreeMap::from([(800, packed)]);
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("conditions.narc"), &rebuilt)?;
    for (name, p) in [("before", &old), ("after", &pixels)] {
        write_png(
            &out.join(format!("{name}.png")),
            64,
            160,
            &titles::rgba(p, &palette)?,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"conditions.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"member":800,"palette_member":801,"entries":entries,"decoded_sha256":sha(&raw),"stored_size":changes[&800].len(),"capacity":n.members[800].len(),"renderer":renderer,"protected":"all pixels outside ten crops including backing circle, palette, all other members and code","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("conditions.json"), &report)?;
    Ok(report)
}
