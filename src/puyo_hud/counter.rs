use super::*;
use crate::battle_ui;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    state: String,
    japanese: String,
    korean: String,
    /// The win-count caption at `WIN_RECT`, drawn beside `/` and the digit
    /// brackets in the same player-coloured atlas.
    #[serde(default)]
    win: Option<Piece>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Piece {
    japanese: String,
    korean: String,
}

/// Bounds of `かち`; the `/` ends at x=6 and the `[` bracket starts at x=25.
const WIN_RECT: [usize; 4] = [8, 32, 25, 45];

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let renderer = super::renderer::verify(rom, None)?;
    let path = "puyo/puyo2P/puyo2P.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b338518e1710dee5f6546deabeb3aa367bdaff2935146c4807e8b9d57c54c510",
        "HUD archive changed"
    );
    let n = Narc::parse(source)?;
    let original = unpack_halfword(n.members[646])?;
    let palette = unpack(n.members[647])?;
    ensure!(
        original.len() == 768 && palette.len() == 352,
        "counter geometry changed"
    );
    let input = fs::read(translation)?;
    let tr: Label = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.japanese == "あと",
        "counter identity changed"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        // Galmuri9 at its native 10px grid; Galmuri11 cannot fit 남음 in 22x12.
        sha(&font_bytes) == "48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee",
        "font changed"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let old = original
        .iter()
        .flat_map(|b| [b & 3, (b >> 2) & 3, (b >> 4) & 3, b >> 6])
        .collect::<Vec<_>>();
    let mut pixels = old.clone();
    for y in 32..44 {
        pixels[y * 64 + 42..y * 64 + 64].fill(0);
    }
    let ink = battle_ui::text_ink(&font, &tr.korean, 10, [42, 32, 64, 44], 42)?;
    battle_ui::paint(&mut pixels, 64, &ink, 2, 1);
    let [wx0, wy0, wx1, wy1] = WIN_RECT;
    if let Some(win) = &tr.win {
        ensure!(win.japanese == "かち", "win caption identity changed");
        for y in wy0..wy1 {
            pixels[y * 64 + wx0..y * 64 + wx1].fill(0);
        }
        let ink = battle_ui::text_ink(&font, &win.korean, 10, WIN_RECT, 43)?;
        battle_ui::paint(&mut pixels, 64, &ink, 2, 1);
    }
    for (i, (&a, &b)) in old.iter().zip(&pixels).enumerate() {
        let (x, y) = (i % 64, i / 64);
        let in_remaining = (32..44).contains(&y) && x >= 42;
        let in_win = tr.win.is_some() && (wy0..wy1).contains(&y) && (wx0..wx1).contains(&x);
        ensure!(
            a == b || in_remaining || in_win,
            "protected counter pixel changed"
        );
    }
    let raw = pixels
        .chunks_exact(4)
        .map(|p| p[0] | p[1] << 2 | p[2] << 4 | p[3] << 6)
        .collect::<Vec<_>>();
    let round = raw
        .iter()
        .flat_map(|b| [b & 3, (b >> 2) & 3, (b >> 4) & 3, b >> 6])
        .collect::<Vec<_>>();
    ensure!(round == pixels, "counter I2 round trip failed");
    let mut changes = BTreeMap::new();
    battle_ui::compress_member(&mut changes, 646, &raw)?;
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("counter.narc"), &rebuilt)?;
    for (name, p) in [("before", &old), ("after", &pixels)] {
        write_png(
            &out.join(format!("{name}.png")),
            64,
            48,
            &titles::rgba(p, &palette[128..136])?,
        )?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"counter.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"member":646,"palette_member":647,"crop":[42,32,22,12],"japanese":tr.japanese,"korean":tr.korean,"win":tr.win.as_ref().map(|w| json!({"japanese":w.japanese,"korean":w.korean,"crop":WIN_RECT})),"decoded_sha256":sha(&raw),"stored_size":changes[&646].len(),"capacity":n.members[646].len(),"renderer":renderer,"protected":"all pixels outside counter label, all palette banks, all other members including numbers and position icons","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("counter.json"), &report)?;
    Ok(report)
}
