use crate::{assets::json_file, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

const ARCHIVE: &str = "menu/select_rule_up.narc";

mod restore;
mod title;

pub(crate) struct Screen {
    pub map: Vec<u8>,
    pub tiles: Vec<u8>,
    pub palette: Vec<u8>,
    pub pixels: Vec<u8>,
}

fn read(narc: &Narc, id: usize) -> Result<Screen> {
    let map = unpack(narc.members[id * 3])?;
    let tiles = unpack(narc.members[id * 3 + 1])?;
    let palette = unpack(narc.members[id * 3 + 2])?;
    ensure!(
        map.len() == 32 * 24 * 2 && tiles.len() % 32 == 0 && palette.len() == 256,
        "unexpected rule screen geometry"
    );
    let pixels = render(&map, &tiles, 8)?;
    Ok(Screen {
        map,
        tiles,
        palette,
        pixels,
    })
}

pub(crate) fn render(map: &[u8], tiles: &[u8], banks: usize) -> Result<Vec<u8>> {
    ensure!(
        map.len() == 1536 && tiles.len() % 32 == 0,
        "invalid screen geometry"
    );
    let mut pixels = vec![0; 256 * 192];
    for cell in 0..32 * 24 {
        let attr = u16le(map, cell * 2)?;
        let tile_id = attr & 1023;
        let bank = attr >> 12;
        ensure!(
            tile_id < tiles.len() / 32 && bank < banks && banks <= 16,
            "rule screen tile/bank out of bounds"
        );
        for y in 0..8 {
            for x in 0..8 {
                let sx = if attr & 0x400 != 0 { 7 - x } else { x };
                let sy = if attr & 0x800 != 0 { 7 - y } else { y };
                let p = tile_id * 32 + (sy * 8 + sx) / 2;
                let shift = (sx % 2) * 4;
                let v = (tiles[p] >> shift) & 15;
                pixels[((cell / 32) * 8 + y) * 256 + (cell % 32) * 8 + x] = bank as u8 * 16 + v;
            }
        }
    }
    Ok(pixels)
}

pub(crate) fn rgba(pixels: &[u8], palette: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for &p in pixels {
        let c = u16le(palette, p as usize * 2)?;
        out.extend([
            ((c & 31) * 255 / 31) as u8,
            (((c >> 5) & 31) * 255 / 31) as u8,
            (((c >> 10) & 31) * 255 / 31) as u8,
            255,
        ]);
    }
    Ok(out)
}

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let source = rom.data(rom.file(ARCHIVE)?);
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 132, "rule screen population changed");
    fs::create_dir_all(out)?;
    let mut records = Vec::new();
    for id in 0..44 {
        let s = read(&narc, id)?;
        write_png(
            &out.join(format!("{id:02}-source.png")),
            256,
            192,
            &rgba(&s.pixels, &s.palette)?,
        )?;
        fs::write(out.join(format!("{id:02}-pixels.bin")), &s.pixels)?;
        fs::write(out.join(format!("{id:02}-palette.bin")), &s.palette)?;
        records.push(json!({"id":id,"members":[id*3,id*3+1,id*3+2],"map_sha256":sha(&s.map),"tiles_sha256":sha(&s.tiles),"palette_sha256":sha(&s.palette),"pixels_sha256":sha(&s.pixels),"tiles":s.tiles.len()/32,"map_stored_size":narc.members[id*3].len(),"tiles_stored_size":narc.members[id*3+1].len()}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"archive":ARCHIVE,"archive_sha256":sha(source),"screens":records,"claim":"static 32x24 4bpp text-BG map, tile flips and eight 16-color banks; palette index zero rendered opaque for inspection; screen selection and loader behavior need runtime proof"});
    json_file(&out.join("screens.json"), &report)?;
    Ok(report)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    archive_sha256: String,
    #[serde(default)]
    masked_body_background: bool,
    entries: Vec<Caption>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Caption {
    id: usize,
    source_pixels_sha256: String,
    japanese_title: String,
    title: String,
    lines: Vec<String>,
    #[serde(default)]
    source_prefixes: Vec<SourcePrefix>,
    #[serde(default)]
    line_baselines: Vec<usize>,
    /// Extra left indent per line, used where a corner flower tile cannot
    /// hold the body white.
    #[serde(default)]
    line_indents: Vec<usize>,
    #[serde(default)]
    emphasis: Vec<Emphasis>,
    /// Leading title characters drawn in the source accent (pink) colour.
    #[serde(default)]
    title_accent: Option<usize>,
    /// Keep the source title lettering (language-neutral titles such as ???).
    #[serde(default)]
    title_source: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Emphasis {
    line: usize,
    start: usize,
    text: String,
    source: [usize; 2],
    rgb5: [i32; 3],
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SourcePrefix {
    line: usize,
    replace: String,
    source: [usize; 4],
    target: [usize; 2],
    advance: usize,
    rgb5_sha256: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TitleArtworkInput {
    translation_sha256: String,
    entries: Vec<TitleArtwork>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TitleArtwork {
    id: usize,
    title: String,
    image: String,
    image_sha256: String,
    region: [usize; 4],
    target: [usize; 4],
    matte: crate::art_pixels::ProductionMatte,
    background_rgb5: [i32; 3],
}

fn paint_title_artwork(
    art: &TitleArtwork,
    background: [i32; 3],
    desired: &mut [[i32; 3]],
) -> Result<Value> {
    ensure!(
        background == art.background_rgb5,
        "title panel colour changed"
    );
    let bytes = fs::read(&art.image)?;
    ensure!(sha(&bytes) == art.image_sha256, "rule title image identity");
    let mut image = crate::art_pixels::read(&bytes)?;
    let matte = crate::art_pixels::remove_matte(&mut image, &art.matte)?;
    let cell = crate::art_pixels::region(&image, art.region)?;
    let [x0, y0, x1, y1] = art.target;
    ensure!(
        x0 >= 24 && x0 < x1 && x1 <= 232 && y0 >= 25 && y0 < y1 && y1 <= 46,
        "rule title artwork outside flat title panel"
    );
    let rgba = crate::art_pixels::reduce(&cell, x1 - x0, y1 - y0, true)?;
    for y in y0..y1 {
        for x in x0..x1 {
            let c = &rgba[((y - y0) * (x1 - x0) + x - x0) * 4..][..4];
            let alpha = i32::from(c[3]);
            for k in 0..3 {
                desired[y * 256 + x][k] =
                    (i32::from(c[k]) * 31 * alpha / 255 + background[k] * (255 - alpha) + 127)
                        / 255;
            }
        }
    }
    Ok(
        json!({"image":art.image,"image_sha256":art.image_sha256,"region":art.region,
        "target":art.target,"background_rgb5":background,"matte_conversion":matte,
        "reduced_rgba_sha256":sha(&rgba)}),
    )
}

fn editable(id: usize, x: usize, y: usize) -> bool {
    ((24..232).contains(&x) && (25..46).contains(&y))
        || ((if id >= 22 { 16 } else { 20 }..restore::body_right(id, y)).contains(&x)
            && (if id >= 22 { 56 } else { 60 }..if id == 35 { 184 } else { 180 }).contains(&y))
}

pub(crate) fn colors(palette: &[u8]) -> Result<Vec<[i32; 3]>> {
    ensure!(
        !palette.is_empty() && palette.len() % 32 == 0 && palette.len() <= 512,
        "invalid BG palette size"
    );
    (0..palette.len() / 2)
        .map(|i| crate::buttons::rgb(palette, i))
        .collect()
}

pub(crate) fn text_ink(
    font: &fontdue::Font,
    text: &str,
    x: usize,
    baseline: usize,
    max_width: usize,
    ink: &mut [bool],
) -> Result<()> {
    let mut cursor = x as i32;
    for c in text.chars() {
        ensure!(font.lookup_glyph_index(c) != 0, "missing screen glyph: {c}");
        let (m, bitmap) = font.rasterize(c, 12.0);
        for y in 0..m.height {
            for px in 0..m.width {
                if bitmap[y * m.width + px] < 128 {
                    continue;
                }
                let dx = cursor + m.xmin + px as i32;
                let dy = baseline as i32 - m.ymin - m.height as i32 + y as i32;
                ensure!(
                    dx >= x as i32 && dx < (x + max_width) as i32 && (0..192).contains(&dy),
                    "screen text exceeds region: {text}"
                );
                ink[dy as usize * 256 + dx as usize] = true;
            }
        }
        cursor += if c == ' ' {
            5
        } else {
            m.advance_width.round() as i32
        };
    }
    ensure!(
        cursor <= (x + max_width) as i32,
        "screen line too wide: {text}"
    );
    Ok(())
}

// Choose a palette bank for each tile. Protected pixels must match exactly;
// only the adopted caption panels may be quantized to an existing color.
pub(crate) struct EncodedScreen {
    pub map: Vec<u8>,
    pub tiles: Vec<u8>,
    pub pixels: Vec<u8>,
    pub tile_count: usize,
}

pub(crate) fn encode_screen(
    s: &Screen,
    editable: impl Fn(usize, usize) -> bool,
    desired: &[[i32; 3]],
) -> Result<EncodedScreen> {
    encode_screen_with_bank_cost(s, editable, desired, |_, _, _| 0)
}

/// Additional bank-selection cost for authored full-screen art. Ordinary label
/// edits use zero cost and retain their existing protected-pixel behavior.
pub(crate) fn encode_screen_with_bank_cost(
    s: &Screen,
    editable: impl Fn(usize, usize) -> bool,
    desired: &[[i32; 3]],
    bank_cost: impl Fn(usize, usize, Option<usize>) -> i64,
) -> Result<EncodedScreen> {
    encode_screen_with_exact(s, editable, desired, bank_cost, |_, _| false)
}

pub(crate) fn encode_screen_with_exact(
    s: &Screen,
    editable: impl Fn(usize, usize) -> bool,
    desired: &[[i32; 3]],
    bank_cost: impl Fn(usize, usize, Option<usize>) -> i64,
    exact: impl Fn(usize, usize) -> bool,
) -> Result<EncodedScreen> {
    encode_screen_with_rules(
        s,
        |x, y| {
            if !editable(x, y) {
                PixelRule::Protected
            } else if exact(x, y) {
                PixelRule::Exact
            } else {
                PixelRule::Free
            }
        },
        desired,
        bank_cost,
    )
}

/// How the encoder may represent one desired pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PixelRule {
    /// Source pixel: same colour and the same transparent-zero status.
    Protected,
    /// New pixel that must be drawn with exactly the desired colour.
    Exact,
    /// Unchanged background that may move to a palette colour within this
    /// squared RGB5 distance so the tile can use a bank holding exact text.
    Near(i32),
    /// Reconstructed pixel; nearest colour of the chosen bank.
    Free,
}

// Choose a palette bank for each tile; the cheapest bank satisfying every
// pixel rule wins. Errors name the cell and the colours no bank could hold.
pub(crate) fn encode_screen_with_rules(
    s: &Screen,
    rule: impl Fn(usize, usize) -> PixelRule,
    desired: &[[i32; 3]],
    bank_cost: impl Fn(usize, usize, Option<usize>) -> i64,
) -> Result<EncodedScreen> {
    use std::collections::BTreeMap;
    let palette = colors(&s.palette)?;
    let mut dictionary: BTreeMap<[u8; 32], usize> = BTreeMap::new();
    let mut tiles = Vec::new();
    let mut map = Vec::new();
    let mut pixels = vec![0; 256 * 192];
    for cell in 0..768 {
        let mut best: Option<(i64, usize, [u8; 64])> = None;
        for bank in 0..palette.len() / 16 {
            let mut cost = 0_i64;
            let mut local = [0; 64];
            let mut valid = true;
            for (p, v) in local.iter_mut().enumerate() {
                let x = cell % 32 * 8 + p % 8;
                let y = cell / 32 * 8 + p / 8;
                let target = desired[y * 256 + x];
                let r = rule(x, y);
                // Index zero is transparent on text BGs. Keep its original
                // local value outside the panels and never draw new ink with it.
                if r == PixelRule::Protected && s.pixels[y * 256 + x] % 16 == 0 {
                    if palette[bank * 16] != target {
                        valid = false;
                        break;
                    }
                    *v = 0;
                    continue;
                }
                let (distance, index) = (1..16)
                    .map(|i| {
                        let d: i32 = palette[bank * 16 + i]
                            .iter()
                            .zip(target)
                            .map(|(a, b)| (a - b).pow(2))
                            .sum();
                        (d, i)
                    })
                    .min()
                    .unwrap();
                let fits = match r {
                    PixelRule::Protected | PixelRule::Exact => distance == 0,
                    PixelRule::Near(limit) => distance <= limit,
                    PixelRule::Free => true,
                };
                if !fits {
                    valid = false;
                    break;
                }
                *v = index as u8;
                cost += distance as i64;
            }
            cost += bank_cost(
                cell,
                bank,
                if cell % 32 > 0 {
                    Some(u16le(&map, map.len() - 2)? >> 12)
                } else {
                    None
                },
            );
            if valid && best.as_ref().is_none_or(|b| cost < b.0) {
                best = Some((cost, bank, local));
            }
        }
        let (_, bank, local) = best.ok_or_else(|| {
            let (mut exact, mut kept) = (Vec::new(), Vec::new());
            for p in 0..64 {
                let (x, y) = (cell % 32 * 8 + p % 8, cell / 32 * 8 + p / 8);
                match rule(x, y) {
                    PixelRule::Exact => exact.push((x, y, desired[y * 256 + x])),
                    PixelRule::Protected => kept.push(desired[y * 256 + x]),
                    _ => {}
                }
            }
            kept.sort();
            kept.dedup();
            anyhow::anyhow!(
                "no lossless protected palette bank at cell {cell} (x {}, y {}); exact pixels {exact:?}; protected colours {kept:?}",
                cell % 32 * 8,
                cell / 32 * 8
            )
        })?;
        let mut tile = [0; 32];
        for p in 0..64 {
            tile[p / 2] |= local[p] << ((p % 2) * 4);
            pixels[(cell / 32 * 8 + p / 8) * 256 + cell % 32 * 8 + p % 8] =
                (bank * 16) as u8 + local[p];
        }
        let next = dictionary.len();
        let tile_id = *dictionary.entry(tile).or_insert_with(|| {
            tiles.extend(tile);
            next
        });
        ensure!(tile_id < 1024, "screen tile index overflow");
        map.extend(((bank << 12 | tile_id) as u16).to_le_bytes());
    }
    let count = dictionary.len();
    ensure!(
        tiles.len() <= s.tiles.len(),
        "screen tile dictionary overflow: {} > {}",
        tiles.len(),
        s.tiles.len()
    );
    tiles.resize(s.tiles.len(), 0);
    for (p, &v) in pixels.iter().enumerate() {
        match rule(p % 256, p / 256) {
            PixelRule::Exact => ensure!(
                palette[v as usize] == desired[p] && v % 16 != 0,
                "exact screen pixel changed"
            ),
            PixelRule::Protected => ensure!(
                palette[v as usize] == palette[s.pixels[p] as usize]
                    && (v % 16 == 0) == (s.pixels[p] % 16 == 0),
                "protected screen pixel changed"
            ),
            PixelRule::Near(_) | PixelRule::Free => {}
        }
    }
    Ok(EncodedScreen {
        map,
        tiles,
        pixels,
        tile_count: count,
    })
}

fn most_common(colors: &[[i32; 3]]) -> Option<[i32; 3]> {
    let mut counts = std::collections::BTreeMap::new();
    for &c in colors {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)
}

/// Pixels of a source symbol crop (red X or heart) connected to its red core.
/// Cyan background and neighbouring Japanese strokes in the crop are left out.
fn symbol_component(original: &[[i32; 3]], [x0, y0, x1, y1]: [usize; 4]) -> Vec<bool> {
    let (w, h) = (x1 - x0, y1 - y0);
    let color = |x: usize, y: usize| original[(y0 + y) * 256 + x0 + x];
    let significant = |c: [i32; 3]| !(c[0] <= 4 && c[1] >= 20 && c[2] >= 26);
    let mut keep = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = (0..w * h)
        .map(|p| (p % w, p / w))
        .filter(|&(x, y)| {
            let c = color(x, y);
            c[0] > c[1] + 4 && c[0] > c[2] + 4
        })
        .collect();
    while let Some((x, y)) = stack.pop() {
        if keep[y * w + x] || !significant(color(x, y)) {
            continue;
        }
        keep[y * w + x] = true;
        for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                stack.push((nx, ny));
            }
        }
    }
    keep
}

/// Pixel coordinates of each character of a body line, and the pen end.
type GlyphRun = (Vec<Vec<(i32, i32)>>, i32);

/// Extra advance before selected character indices of a body line.
type Gaps = std::collections::BTreeMap<usize, i32>;

/// Spacing that keeps each emphasised run (whose colour shares no bank with the
/// body white) in 8x8 tiles of its own, as the source spacing does. Word
/// spaces may shrink or widen by up to 2px (3..7px); a gap right after a run,
/// which would split it from its particle, is the last resort and costs 3 per
/// pixel. Returns the cheapest extra advance before each character.
fn separate_emphasis(
    font: &fontdue::Font,
    chars: &[char],
    x0: i32,
    baseline: i32,
    max_width: i32,
    runs: &[(usize, usize)],
    emphasised: &dyn Fn(usize) -> bool,
) -> Result<Option<Gaps>> {
    use std::collections::{BTreeMap, BTreeSet};
    let (glyphs, end) = glyph_run(font, chars, x0, baseline, &BTreeMap::new())?;
    // (character index receiving the extra advance, lowest, highest, cost per px)
    let mut vars: Vec<(usize, i32, i32, i32)> = Vec::new();
    for (k, &c) in chars.iter().enumerate() {
        if c == ' ' && k + 1 < chars.len() {
            vars.push((k + 1, -2, 2, 1));
        }
    }
    for &(_, b) in runs {
        if b < chars.len() {
            vars.push((b, 0, 4, 3));
        }
    }
    let fits = |values: &[i32]| {
        let mut gaps = BTreeMap::new();
        for (v, &(k, ..)) in values.iter().zip(&vars) {
            *gaps.entry(k).or_insert(0) += *v;
        }
        let shift = |k: usize| gaps.range(..=k).map(|(_, v)| *v).sum::<i32>();
        if end + shift(chars.len()) > x0 + max_width {
            return None;
        }
        let tiles = |k: usize| {
            let d = shift(k);
            glyphs[k]
                .iter()
                .map(move |&(x, y)| ((x + d).div_euclid(8), y.div_euclid(8)))
        };
        for &(a, b) in runs {
            let inside: BTreeSet<_> = (a..b).flat_map(tiles).collect();
            if (0..chars.len())
                .filter(|&k| !emphasised(k))
                .flat_map(tiles)
                .any(|t| inside.contains(&t))
            {
                return None;
            }
            if glyphs[a..b]
                .iter()
                .flatten()
                .any(|&(x, _)| x + shift(a) < x0)
            {
                return None;
            }
        }
        Some(gaps)
    };
    // Cheapest assignment first: depth-first enumeration under a growing budget.
    fn search(
        vars: &[(usize, i32, i32, i32)],
        values: &mut Vec<i32>,
        budget: i32,
        test: &dyn Fn(&[i32]) -> Option<Gaps>,
    ) -> Option<Gaps> {
        if values.len() == vars.len() {
            return test(values);
        }
        let (_, low, high, cost) = vars[values.len()];
        for v in low..=high {
            if v.abs() * cost > budget {
                continue;
            }
            values.push(v);
            let found = search(vars, values, budget - v.abs() * cost, test);
            values.pop();
            if found.is_some() {
                return found;
            }
        }
        None
    }
    for budget in 0..=24 {
        if let Some(gaps) = search(&vars, &mut Vec::new(), budget, &fits) {
            return Ok(Some(gaps));
        }
    }
    Ok(None)
}

/// Pixels of each character of one body line (Galmuri11 12px, threshold 128,
/// space 5px), with extra gaps before selected character indices.
fn glyph_run(
    font: &fontdue::Font,
    chars: &[char],
    x: i32,
    baseline: i32,
    gaps: &std::collections::BTreeMap<usize, i32>,
) -> Result<GlyphRun> {
    let mut cursor = x;
    let mut out = Vec::new();
    for (k, &c) in chars.iter().enumerate() {
        ensure!(font.lookup_glyph_index(c) != 0, "missing screen glyph: {c}");
        cursor += gaps.get(&k).copied().unwrap_or(0);
        let (m, bitmap) = font.rasterize(c, 12.0);
        let mut pixels = Vec::new();
        for y in 0..m.height {
            for px in 0..m.width {
                if bitmap[y * m.width + px] >= 128 {
                    pixels.push((
                        cursor + m.xmin + px as i32,
                        baseline - m.ymin - m.height as i32 + y as i32,
                    ));
                }
            }
        }
        out.push(pixels);
        cursor += if c == ' ' {
            5
        } else {
            m.advance_width.round() as i32
        };
    }
    Ok((out, cursor))
}

/// Largest squared error of the reconstructed (free) pixels of one 8x8 cell in
/// the bank the encoder would choose (lowest total error among the banks that
/// satisfy every rule) with that bank, or None when no bank satisfies the rules.
fn cell_free_error(
    s: &Screen,
    palette: &[[i32; 3]],
    rules: &[PixelRule],
    desired: &[[i32; 3]],
    cell: usize,
) -> Option<(i32, usize)> {
    let mut best: Option<(i64, i32, usize)> = None;
    'bank: for bank in 0..palette.len() / 16 {
        let (mut cost, mut worst) = (0i64, 0i32);
        for p in 0..64 {
            let q = (cell / 32 * 8 + p / 8) * 256 + cell % 32 * 8 + p % 8;
            let target = desired[q];
            if rules[q] == PixelRule::Protected && s.pixels[q] % 16 == 0 {
                if palette[bank * 16] != target {
                    continue 'bank;
                }
                continue;
            }
            let d = (1..16)
                .map(|i| {
                    (0..3)
                        .map(|k| (palette[bank * 16 + i][k] - target[k]).pow(2))
                        .sum::<i32>()
                })
                .min()
                .unwrap();
            let fits = match rules[q] {
                PixelRule::Protected | PixelRule::Exact => d == 0,
                PixelRule::Near(limit) => d <= limit,
                PixelRule::Free => {
                    worst = worst.max(d);
                    true
                }
            };
            if !fits {
                continue 'bank;
            }
            cost += d as i64;
        }
        if best.is_none_or(|b| cost < b.0) {
            best = Some((cost, worst, bank));
        }
    }
    best.map(|b| (b.1, b.2))
}

fn nearest_error(palette: &[[i32; 3]], bank: usize, target: [i32; 3]) -> i32 {
    (1..16)
        .map(|i| {
            (0..3)
                .map(|k| (palette[bank * 16 + i][k] - target[k]).pow(2))
                .sum::<i32>()
        })
        .min()
        .unwrap()
}

/// Reconstruction error above which a cell drops its optional outline pixels
/// when that lets it pick a bank closer to the rebuilt background.
const FREE_LIMIT: i32 = 27;

/// Largest squared RGB5 distance from the body white to a neutral near-white.
const WHITE_NEAR: i32 = 14;

/// Largest squared RGB5 move allowed for unchanged background pixels that
/// share a tile with new lettering (for example 2,23,28 -> 2,23,29).
const BACKGROUND_NEAR: i32 = 3;

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    title_artwork: Option<&Path>,
    title_font: Option<&Path>,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    use std::collections::BTreeMap;
    ensure!(!out.exists(), "output exists");
    ensure!(
        title_artwork.is_none() || title_font.is_none(),
        "choose title artwork or title font, not both"
    );
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    let artwork_bytes = title_artwork.map(fs::read).transpose()?;
    let artwork: Option<TitleArtworkInput> = artwork_bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()?;
    if let Some(art) = &artwork {
        ensure!(
            art.translation_sha256 == sha(&input) && art.entries.len() == 44,
            "rule title artwork translation identity/population mismatch"
        );
        for (id, entry) in art.entries.iter().enumerate() {
            ensure!(
                entry.id == id && tr.entries.get(id).is_some_and(|c| c.title == entry.title),
                "rule title artwork content/order mismatch"
            );
        }
    }
    let title_font_bytes = title_font.map(fs::read).transpose()?;
    let title_font = match &title_font_bytes {
        Some(bytes) => {
            ensure!(
                sha(bytes) == crate::fonts::GALMURI14_SHA256,
                "title font identity mismatch"
            );
            ensure!(
                tr.masked_body_background,
                "font titles need source lettering reconstruction"
            );
            Some(
                fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
                    .map_err(|e| anyhow::anyhow!(e))?,
            )
        }
        None => None,
    };
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(
        tr.state == "development_art_draft"
            && tr.archive_sha256 == sha(source)
            && tr.entries.len() == 44,
        "rule screen translation identity/population mismatch"
    );
    let bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&bytes), "font identity mismatch");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let narc = Narc::parse(source)?;
    ensure!(narc.members.len() == 132, "rule screen population changed");
    let screens: Vec<Screen> = (0..44).map(|id| read(&narc, id)).collect::<Result<_>>()?;
    let originals: Vec<Vec<[i32; 3]>> = screens
        .iter()
        .map(|s| -> Result<_> {
            let palette = colors(&s.palette)?;
            Ok(s.pixels.iter().map(|&p| palette[p as usize]).collect())
        })
        .collect::<Result<_>>()?;
    // Source lettering footprints, grouped as rule screens 0..21 and hints 22..43.
    let mut footprints = Vec::new();
    let mut restored = Vec::new();
    // Per group: its decoration as the screens clear of lettering show it.
    let mut group_decoration = Vec::new();
    if tr.masked_body_background {
        for group in [0..22, 22..44] {
            let members: Vec<&[[i32; 3]]> =
                group.clone().map(|id| originals[id].as_slice()).collect();
            let clear: Vec<Vec<bool>> = group
                .clone()
                .map(|id| restore::clear_of_lettering(id, &originals[id]))
                .collect();
            let clear: Vec<&[bool]> = clear.iter().map(|c| c.as_slice()).collect();
            let decoration = restore::decoration(&members, &clear);
            group_decoration.push(restore::shown_decoration(&members, &clear));
            let prints: Vec<_> = group
                .clone()
                .map(|id| {
                    restore::footprint(
                        id,
                        &originals[id],
                        &decoration,
                        tr.entries.get(id).is_some_and(|c| c.title_source),
                    )
                })
                .collect::<Result<_>>()?;
            let masks: Vec<&[bool]> = prints.iter().map(|f| f.mask.as_slice()).collect();
            // Petals hidden under letters: consensus of the screens that show
            // the pixel (outside their own lettering footprint).
            let shown: Vec<Vec<bool>> = masks
                .iter()
                .map(|m| m.iter().map(|&v| !v).collect())
                .collect();
            let shown: Vec<&[bool]> = shown.iter().map(|v| v.as_slice()).collect();
            let hidden_petals = restore::decoration(&members, &shown);
            for (own, id) in group.clone().enumerate() {
                restored.push(restore::restore(
                    own,
                    id,
                    &members,
                    &masks,
                    &clear,
                    &hidden_petals,
                    prints[own].title_background,
                )?);
            }
            let mut prints = prints;
            for (own, r) in restored[restored.len() - prints.len()..].iter().enumerate() {
                for &q in &r.cleared {
                    prints[own].mask[q] = true;
                }
            }
            footprints.extend(prints);
        }
    }
    let mut replacements = BTreeMap::new();
    let mut previews = Vec::new();
    let mut records = Vec::new();
    let mut failures = Vec::new();
    for (id, caption) in tr.entries.iter().enumerate() {
        let s = &screens[id];
        ensure!(
            caption.id == id
                && caption.source_pixels_sha256 == sha(&s.pixels)
                && !caption.japanese_title.is_empty()
                && !caption.title.trim().is_empty()
                && caption.lines.iter().any(|line| !line.trim().is_empty())
                && caption.lines.len() <= 8,
            "screen {id} identity/content mismatch"
        );
        let palette = colors(&s.palette)?;
        let original = &originals[id];
        let mut desired = original.clone();
        let mut ink = vec![false; 256 * 192];
        if artwork.is_none() && title_font.is_none() {
            text_ink(&font, &caption.title, 28, 40, 200, &mut ink)?;
        }
        ensure!(
            caption.line_baselines.is_empty()
                || caption.line_baselines.len() == caption.lines.len(),
            "line baseline population mismatch"
        );
        let baseline = |i: usize| {
            caption
                .line_baselines
                .get(i)
                .copied()
                .unwrap_or(73 + i * 15)
        };
        // Background: legacy inputs fill each panel with its most common source
        // colour; reconstruction rebuilds only the source lettering footprint.
        let region = |title: bool| -> Vec<[i32; 3]> {
            (0..256 * 192)
                .filter(|&p| editable(id, p % 256, p / 256) && ((p / 256) < 46) == title)
                .map(|p| original[p])
                .collect()
        };
        let title_base = most_common(&region(true)).unwrap_or([2, 10, 20]);
        let body_base = most_common(&region(false)).unwrap_or([0, 14, 23]);
        let mut outline_votes: BTreeMap<(bool, [i32; 3]), usize> = BTreeMap::new();
        for (p, &color) in original.iter().enumerate() {
            let (x, y) = (p % 256, p / 256);
            if editable(id, x, y) && color.iter().sum::<i32>() < 45 {
                *outline_votes.entry((y < 46, color)).or_default() += 1;
            }
        }
        if let Some(r) = restored.get(id) {
            desired.clone_from(&r.colors);
        } else {
            for (p, color) in desired.iter_mut().enumerate() {
                let (x, y) = (p % 256, p / 256);
                if editable(id, x, y) {
                    *color = if y < 46 { title_base } else { body_base };
                }
            }
        }
        let outline = |title: bool| {
            outline_votes
                .iter()
                .filter(|((t, _), _)| *t == title)
                .max_by_key(|(_, n)| **n)
                .map(|((_, c), _)| *c)
                .unwrap_or([2, 10, 20])
        };
        let (title_outline, body_outline) = (outline(true), outline(false));
        // Body fill: the source's own near-white letter colour when
        // reconstructing (not every screen has pure white), else pure white.
        let body_fill = if restored.is_empty() {
            [31, 31, 31]
        } else {
            most_common(
                &(0..256 * 192)
                    .filter(|&p| {
                        footprints[id].mask[p]
                            && p / 256 >= 46
                            && original[p].iter().all(|&v| v >= 27)
                    })
                    .map(|p| original[p])
                    .collect::<Vec<_>>(),
            )
            .ok_or_else(|| anyhow::anyhow!("screen {id} has no source letter white"))?
        };
        // Body lines. An emphasis colour that shares no bank with the body
        // white and outline must not share a tile with white letters, so the
        // emphasised run and the text after it move right by the smallest
        // gaps that separate them by tile, as the source layout does.
        let shared_bank = |color: [i32; 3]| {
            palette.chunks(16).any(|bank| {
                [color, body_fill, body_outline]
                    .iter()
                    .all(|c| bank[1..].contains(c))
            })
        };
        let mut emphasis_pixels = vec![false; original.len()];
        let mut emphasis_color = vec![None; original.len()];
        let mut line_gaps = Vec::new();
        for (i, line) in caption.lines.iter().enumerate() {
            let prefixes: Vec<_> = caption
                .source_prefixes
                .iter()
                .filter(|p| p.line == i)
                .collect();
            ensure!(prefixes.len() <= 1, "duplicate source prefix");
            let (text, advance, skip) = if let Some(prefix) = prefixes.first() {
                ensure!(
                    !prefix.replace.is_empty() && prefix.advance < 64,
                    "invalid source prefix"
                );
                (
                    line.strip_prefix(&prefix.replace)
                        .ok_or_else(|| anyhow::anyhow!("source prefix text mismatch"))?,
                    prefix.advance,
                    prefix.replace.chars().count(),
                )
            } else {
                (line.as_str(), 0, 0)
            };
            let chars: Vec<char> = text.chars().collect();
            let all: Vec<char> = line.chars().collect();
            let mut spans = Vec::new();
            for span in caption.emphasis.iter().filter(|e| e.line == i) {
                let end = span.start + span.text.chars().count();
                ensure!(
                    !span.text.is_empty()
                        && end <= all.len()
                        && all[span.start..end].iter().collect::<String>() == span.text,
                    "screen {id} emphasis text mismatch"
                );
                let [sx, sy] = span.source;
                ensure!(
                    sx < 256
                        && (56..180).contains(&sy)
                        && original[sy * 256 + sx] == span.rgb5
                        && span.rgb5.iter().all(|v| (0..32).contains(v)),
                    "screen {id} emphasis source color mismatch"
                );
                ensure!(span.start >= skip, "emphasis overlaps source prefix");
                spans.push((span.start - skip, end - skip, span.rgb5));
            }
            spans.sort();
            ensure!(
                caption
                    .emphasis
                    .iter()
                    .all(|e| e.line < caption.lines.len()),
                "emphasis line out of range"
            );
            let indent = caption.line_indents.get(i).copied().unwrap_or(0);
            ensure!(
                caption.line_indents.is_empty()
                    || caption.line_indents.len() == caption.lines.len()
                        && (indent == 0 || advance == 0),
                "line indent population"
            );
            let x0 = 24 + (advance + indent) as i32;
            let max_width = if id == 18 {
                132
            } else if id < 22 {
                148
            } else {
                208
            } - (advance + indent) as i32;
            let emphasised = |k: usize| spans.iter().any(|&(sa, sb, _)| (sa..sb).contains(&k));
            let separate: Vec<(usize, usize)> = spans
                .iter()
                .filter(|&&(_, _, c)| !restored.is_empty() && !shared_bank(c))
                .map(|&(a, b, _)| (a, b))
                .collect();
            let gaps = if separate.is_empty() {
                BTreeMap::new()
            } else {
                separate_emphasis(
                    &font,
                    &chars,
                    x0,
                    baseline(i) as i32,
                    max_width,
                    &separate,
                    &emphasised,
                )?
                .ok_or_else(|| {
                    anyhow::anyhow!("screen {id} line {i}: emphasis cannot be separated by tile")
                })?
            };
            let (glyphs, end) = glyph_run(&font, &chars, x0, baseline(i) as i32, &gaps)?;
            ensure!(end <= x0 + max_width, "screen line too wide: {text}");
            for (k, pixels) in glyphs.iter().enumerate() {
                let color = spans
                    .iter()
                    .find(|&&(sa, sb, _)| (sa..sb).contains(&k))
                    .map(|&(_, _, c)| c);
                for &(x, y) in pixels {
                    ensure!(
                        x >= x0 && x < x0 + max_width && (0..192).contains(&y),
                        "screen text exceeds region: {text}"
                    );
                    let p = y as usize * 256 + x as usize;
                    ink[p] = true;
                    if let Some(c) = color {
                        ensure!(!emphasis_pixels[p], "emphasis mask overlap");
                        emphasis_pixels[p] = true;
                        emphasis_color[p] = Some(c);
                    }
                }
            }
            let used: Vec<_> = gaps
                .iter()
                .filter(|(_, v)| **v != 0)
                .map(|(k, v)| json!([k + skip, v]))
                .collect();
            if !used.is_empty() {
                line_gaps.push(json!({"line":i,"gaps_before_char":used}));
            }
        }
        // Every new lettering pixel and its intended colour.
        let mut layer: Vec<Option<[i32; 3]>> = vec![None; 256 * 192];
        // Outline pixels a decoration tile cannot hold (or holds only with a
        // badly quantized rebuilt background) may be left out.
        let mut soft = vec![false; 256 * 192];
        // Body white may use a neutral near-white of a decoration tile's bank,
        // as the source letters do there (for example 27,28,29 for 30,30,30).
        let mut white = vec![false; 256 * 192];
        for y in 0..192 {
            for x in 0..256 {
                let p = y * 256 + x;
                ensure!(
                    !ink[p] || editable(id, x, y),
                    "screen ink outside adopted panel"
                );
                if ink[p] {
                    layer[p] = Some(emphasis_color[p].unwrap_or(body_fill));
                    white[p] = !restored.is_empty() && y >= 46 && !emphasis_pixels[p];
                    continue;
                }
                let near =
                    [(-1i32, 0i32), (1, 0), (0, -1), (0, 1), (1, 1)]
                        .iter()
                        .any(|&(dx, dy)| {
                            let (nx, ny) = (x as i32 - dx, y as i32 - dy);
                            (0..256).contains(&nx)
                                && (0..192).contains(&ny)
                                && ink[ny as usize * 256 + nx as usize]
                        });
                if near && editable(id, x, y) {
                    layer[p] = Some(if y < 46 { title_outline } else { body_outline });
                    soft[p] = true;
                }
            }
        }
        let mut title_record = None;
        if let Some(art) = &artwork {
            // Generated artwork keeps the legacy flat capsule and quantizes.
            for (p, color) in desired.iter_mut().enumerate() {
                if editable(id, p % 256, p / 256) && p / 256 < 46 {
                    *color = title_base;
                }
            }
            title_record = Some(paint_title_artwork(
                &art.entries[id],
                title_base,
                &mut desired,
            )?);
        }
        ensure!(
            !caption.title_source || title_font.is_some(),
            "a kept source title needs the font title path"
        );
        if caption.title_source {
            title_record = Some(json!({"source_title_kept":true}));
        } else if let Some(title_font) = &title_font {
            let background = footprints[id].title_background;
            let colors = title::source_colors(original, background)?;
            let single = title::single_bank(&palette, &colors);
            let accent = caption.title_accent.unwrap_or(0);
            let drawn = title::draw(title_font, &caption.title, accent, &colors, single)
                .map_err(|e| anyhow::anyhow!("screen {id}: {e}"))?;
            for &(p, color) in &drawn.pixels {
                ensure!(layer[p].is_none(), "title overlaps body lettering");
                layer[p] = Some(color);
                soft[p] = color == colors.outline;
            }
            title_record = Some(drawn.record);
        }
        let mut symbol_pixels = vec![false; original.len()];
        for prefix in &caption.source_prefixes {
            let [x0, y0, x1, y1] = prefix.source;
            let [tx, ty] = prefix.target;
            ensure!(
                prefix.line < caption.lines.len()
                    && x0 < x1
                    && y0 < y1
                    && x1 <= 256
                    && y1 <= 192
                    && tx == 24
                    && prefix.advance >= x1 - x0 + 2
                    && tx + x1 - x0 <= 256
                    && ty + y1 - y0 <= 192,
                "source prefix geometry"
            );
            let mut bytes = Vec::new();
            for y in y0..y1 {
                for x in x0..x1 {
                    bytes.extend(original[y * 256 + x].map(|v| v as u8));
                }
            }
            ensure!(
                sha(&bytes) == prefix.rgb5_sha256,
                "source prefix crop identity"
            );
            let symbol = symbol_component(original, prefix.source);
            for y in y0..y1 {
                for x in x0..x1 {
                    let color = original[y * 256 + x];
                    if !symbol[(y - y0) * (x1 - x0) + x - x0] {
                        continue;
                    }
                    let (dx, dy) = (tx + x - x0, ty + y - y0);
                    let p = dy * 256 + dx;
                    ensure!(
                        editable(id, dx, dy) && !ink[p] && !symbol_pixels[p],
                        "screen {id} source prefix overlaps lettering or protected area at {dx},{dy}"
                    );
                    layer[p] = Some(color);
                    symbol_pixels[p] = true;
                }
            }
        }
        let base = desired.clone();
        for p in 0..256 * 192 {
            if let Some(color) = layer[p] {
                desired[p] = color;
            }
        }
        // Pixel rules. Reconstruction: new lettering exact, source footprint
        // free, and unchanged opaque cool background sharing a tile with
        // lettering may move by BACKGROUND_NEAR so that the tile can select a
        // bank holding the lettering colours. Warm decoration (flowers, rim
        // dots) and everything else stay protected.
        let mut free = footprints
            .get(id)
            .map(|f| f.mask.clone())
            .unwrap_or_default();
        // Tiles whose decoration was completed from the group: like lettered
        // tiles, their unchanged cool background may move by BACKGROUND_NEAR
        // so that the tile can leave a source letter bank for the flower bank.
        let mut completed_tiles = [false; 768];
        let rules_for = |layer: &[Option<[i32; 3]>],
                         mask: &[bool],
                         completed_tiles: &[bool; 768]|
         -> Vec<PixelRule> {
            if restored.is_empty() {
                return (0..256 * 192)
                    .map(|p| {
                        if !editable(id, p % 256, p / 256) {
                            PixelRule::Protected
                        } else if symbol_pixels[p] || emphasis_pixels[p] {
                            PixelRule::Exact
                        } else {
                            PixelRule::Free
                        }
                    })
                    .collect();
            }
            let mut lettered = [false; 768];
            for p in 0..256 * 192 {
                if layer[p].is_some() {
                    lettered[p / 256 / 8 * 32 + p % 256 / 8] = true;
                }
            }
            (0..256 * 192)
                .map(|p| {
                    let (x, y) = (p % 256, p / 256);
                    if layer[p].is_some() && white[p] {
                        PixelRule::Near(WHITE_NEAR)
                    } else if layer[p].is_some() {
                        PixelRule::Exact
                    } else if mask[p] {
                        PixelRule::Free
                    } else if (lettered[y / 8 * 32 + x / 8] || completed_tiles[y / 8 * 32 + x / 8])
                        && s.pixels[p] % 16 != 0
                        && !restore::warm(original[p])
                    {
                        PixelRule::Near(BACKGROUND_NEAR)
                    } else {
                        PixelRule::Protected
                    }
                })
                .collect()
        };
        let mut rules = rules_for(&layer, &free, &completed_tiles);
        // Cells whose rebuilt background the chosen bank shows badly: first
        // leave out optional outline pixels, then replace the badly shown
        // rebuilt pixels by the nearest visible cyan.
        let mut dropped_outline = 0;
        let mut cleared_late = 0;
        let mut cleared_beside = 0;
        let mut restored_dots = 0;
        let mut completed_decoration = 0;
        let mut fallback_pixels = 0;
        let mut cut = vec![false; 256 * 192];
        if let Some(r) = restored.get(id) {
            for cell in 0..768 {
                let cell_pixels: Vec<usize> = (0..64)
                    .map(|p| (cell / 32 * 8 + p / 8) * 256 + cell % 32 * 8 + p % 8)
                    .collect();
                let before = cell_free_error(s, &palette, &rules, &desired, cell);
                if before.is_some_and(|e| e.0 <= FREE_LIMIT) {
                    continue;
                }
                let dropped: Vec<usize> = cell_pixels
                    .iter()
                    .copied()
                    .filter(|&q| soft[q] && layer[q].is_some())
                    .collect();
                if !dropped.is_empty() {
                    let mut trial_layer = layer.clone();
                    let mut trial_desired = desired.clone();
                    for &q in &dropped {
                        trial_layer[q] = None;
                        trial_desired[q] = base[q];
                    }
                    let trial_rules = rules_for(&trial_layer, &free, &completed_tiles);
                    let after = cell_free_error(s, &palette, &trial_rules, &trial_desired, cell);
                    let better = match (before, after) {
                        (None, Some(_)) => true,
                        (Some(b), Some(a)) => a.0 < b.0,
                        _ => false,
                    };
                    if better {
                        layer = trial_layer;
                        desired = trial_desired;
                        rules = trial_rules;
                        dropped_outline += dropped.len();
                    }
                }
                let Some((error, bank)) = cell_free_error(s, &palette, &rules, &desired, cell)
                else {
                    continue;
                };
                if error <= FREE_LIMIT {
                    continue;
                }
                let mut trial_desired = desired.clone();
                let mut changed = 0;
                for &q in &cell_pixels {
                    if rules[q] == PixelRule::Free
                        && nearest_error(&palette, bank, desired[q]) > FREE_LIMIT
                        && r.fallback[q] != desired[q]
                    {
                        trial_desired[q] = r.fallback[q];
                        changed += 1;
                    }
                }
                if changed > 0
                    && cell_free_error(s, &palette, &rules, &trial_desired, cell)
                        .is_some_and(|a| a.0 < error)
                {
                    for &q in &cell_pixels {
                        cut[q] |= trial_desired[q] != desired[q];
                    }
                    desired = trial_desired;
                    fallback_pixels += changed;
                }
            }
            // Decoration fragments left detached by these replacements.
            let locked: Vec<bool> = layer.iter().map(|c| c.is_some()).collect();
            let changed = restore::clear_fragments(
                id,
                &mut desired,
                original,
                &footprints[id].mask,
                &cut,
                restore::Keep::GroupDecoration(&vec![false; 256 * 192]),
                &locked,
            )?;
            for &q in &changed {
                free[q] = true;
            }
            // Any small petal or dot within 2px of a new body glyph (fill or
            // outline, symbols excluded) reads as a fragment stuck to the
            // letter: clear it too. The large corner flowers stay.
            let mut near_glyph = vec![false; 256 * 192];
            for (p, color) in layer.iter().enumerate() {
                let (x, y) = (p % 256, p / 256);
                if color.is_some() && y >= 46 && !symbol_pixels[p] {
                    for ny in y.saturating_sub(2)..=(y + 2).min(191) {
                        for nx in x.saturating_sub(2)..=(x + 2).min(255) {
                            near_glyph[ny * 256 + nx] = true;
                        }
                    }
                }
            }
            // Petal parts rebuilt under the source letters that reach within
            // 3px of a new glyph end abruptly at the glyph (the source always
            // hid them): return each such rebuilt petal piece to cyan.
            let mut near3 = vec![false; 256 * 192];
            for (p, color) in layer.iter().enumerate() {
                let (x, y) = (p % 256, p / 256);
                if color.is_some() && y >= 46 && !symbol_pixels[p] {
                    for ny in y.saturating_sub(3)..=(y + 3).min(191) {
                        for nx in x.saturating_sub(3)..=(x + 3).min(255) {
                            near3[ny * 256 + nx] = true;
                        }
                    }
                }
            }
            let rebuilt_petal: Vec<bool> = (0..256 * 192)
                .map(|q| footprints[id].mask[q] && layer[q].is_none() && restore::petal(desired[q]))
                .collect();
            let mut seen = vec![false; 256 * 192];
            let mut pruned = 0;
            for start in 0..256 * 192 {
                if seen[start] || !rebuilt_petal[start] {
                    continue;
                }
                let mut piece = vec![start];
                seen[start] = true;
                let mut k = 0;
                while k < piece.len() {
                    let (x, y) = (piece[k] % 256, piece[k] / 256);
                    k += 1;
                    for ny in y.saturating_sub(1)..=(y + 1).min(191) {
                        for nx in x.saturating_sub(1)..=(x + 1).min(255) {
                            let q = ny * 256 + nx;
                            if !seen[q] && rebuilt_petal[q] {
                                seen[q] = true;
                                piece.push(q);
                            }
                        }
                    }
                }
                if piece.iter().any(|&q| near3[q]) {
                    for &q in &piece {
                        desired[q] = r.fallback[q];
                        near_glyph[q] = true;
                    }
                    pruned += piece.len();
                }
            }
            fallback_pixels += pruned;
            let beside = restore::clear_fragments(
                id,
                &mut desired,
                original,
                &footprints[id].mask,
                &near_glyph,
                restore::Keep::Nothing,
                &locked,
            )?;
            for &q in &beside {
                free[q] = true;
            }
            cleared_beside = beside.len();
            // Last pass over the whole panel: a small petal piece next to or
            // inside the rebuilt footprint (a stem or tip left without its
            // flower body, a half dot, a dot the source never showed) goes;
            // whole visible dots and the flowers stay.
            let rebuilt = free.clone();
            let loose = restore::clear_fragments(
                id,
                &mut desired,
                original,
                &footprints[id].mask,
                &rebuilt,
                restore::Keep::VisibleDots,
                &locked,
            )?;
            for &q in &loose {
                free[q] = true;
            }
            cleared_beside += loose.len();
            // Stray specks (at most four petal pixels) anywhere in the body.
            let specks = restore::clear_fragments(
                id,
                &mut desired,
                original,
                &footprints[id].mask,
                &vec![true; 256 * 192],
                restore::Keep::Specks,
                &locked,
            )?;
            for &q in &specks {
                free[q] = true;
            }
            cleared_beside += specks.len();
            // One- to four-pixel leftovers of rebuilt colour (differing from
            // the source) in or next to the rebuilt area that are neither ray,
            // petal nor lettering colours (outline or halo pixels another
            // screen's petal or dot left) become the nearest visible cyan.
            let odd_at = |desired: &[[i32; 3]], q: usize| {
                let c = desired[q];
                c != original[q] && !restore::background_like(c) && !restore::petal(c)
            };
            let mut seen = vec![false; 256 * 192];
            for start in 0..256 * 192 {
                let (x, y) = (start % 256, start / 256);
                if seen[start]
                    || locked[start]
                    || !odd_at(&desired, start)
                    || !restore::in_body_source(id, x, y)
                {
                    continue;
                }
                let mut piece = vec![start];
                seen[start] = true;
                let mut k = 0;
                while k < piece.len() && piece.len() <= 4 {
                    let (px, py) = (piece[k] % 256, piece[k] / 256);
                    k += 1;
                    for ny in py.saturating_sub(1)..=(py + 1).min(191) {
                        for nx in px.saturating_sub(1)..=(px + 1).min(255) {
                            let q = ny * 256 + nx;
                            if !seen[q] && !locked[q] && odd_at(&desired, q) {
                                seen[q] = true;
                                piece.push(q);
                            }
                        }
                    }
                }
                let touches_rebuilt = piece.iter().any(|&q| {
                    let (px, py) = (q % 256, q / 256);
                    (py.saturating_sub(1)..=(py + 1).min(191)).any(|ny| {
                        (px.saturating_sub(1)..=(px + 1).min(255)).any(|nx| free[ny * 256 + nx])
                    })
                });
                // A rebuilt petal's own anti-aliased edge (next to its petal
                // pixels) stays.
                let edge = piece.iter().any(|&q| {
                    let (px, py) = (q % 256, q / 256);
                    [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)]
                        .iter()
                        .any(|&(dx, dy)| {
                            let (nx, ny) = (px as i32 + dx, py as i32 + dy);
                            (0..256).contains(&nx)
                                && (0..192).contains(&ny)
                                && restore::petal(desired[ny as usize * 256 + nx as usize])
                        })
                });
                if piece.len() > 4 || !touches_rebuilt || edge {
                    continue;
                }
                for &q in &piece {
                    desired[q] =
                        restore::nearest_cyan(original, &footprints[id].mask, q % 256, q / 256)
                            .ok_or_else(|| {
                                anyhow::anyhow!("screen {id} missing background donor")
                            })?;
                    free[q] = true;
                }
                cleared_beside += piece.len();
            }
            cleared_late = changed.len();
            // Flower dots the source shows whole (compact petal blobs of 9..30
            // pixels in a 4..6 square, two thirds filled), away from the new
            // glyphs, that the passes above emptied come back from the source
            // with their blended rim (lettering outline and white excluded).
            let mut seen = vec![false; 256 * 192];
            for start in 0..256 * 192 {
                if seen[start] || !restore::petal(original[start]) {
                    continue;
                }
                let mut blob = vec![start];
                seen[start] = true;
                let mut k = 0;
                while k < blob.len() && blob.len() <= 31 {
                    let (x, y) = (blob[k] % 256, blob[k] / 256);
                    k += 1;
                    for ny in y.saturating_sub(1)..=(y + 1).min(191) {
                        for nx in x.saturating_sub(1)..=(x + 1).min(255) {
                            let q = ny * 256 + nx;
                            if !seen[q] && restore::petal(original[q]) {
                                seen[q] = true;
                                blob.push(q);
                            }
                        }
                    }
                }
                let (xs, ys): (Vec<usize>, Vec<usize>) =
                    blob.iter().map(|&q| (q % 256, q / 256)).unzip();
                let w = xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1;
                let h = ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1;
                let dot = (9..=30).contains(&blob.len())
                    && (4..=6).contains(&w)
                    && (4..=6).contains(&h)
                    && 3 * blob.len() >= 2 * w * h
                    // not a dot the source letters clip (their dark outline
                    // along a side of it; a touched corner is fine)
                    && {
                        let mut outline = Vec::new();
                        for &q in &blob {
                            let (x, y) = (q % 256, q / 256);
                            for (dx, dy) in [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)] {
                                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                                if !(0..256).contains(&nx) || !(0..192).contains(&ny) {
                                    continue;
                                }
                                let r = ny as usize * 256 + nx as usize;
                                let c = original[r];
                                if c[1] <= 15 && !restore::background_like(c) && !outline.contains(&r)
                                {
                                    outline.push(r);
                                }
                            }
                        }
                        outline.len() < 3
                    };
                let emptied = 2 * blob
                    .iter()
                    .filter(|&&q| !restore::petal(desired[q]))
                    .count()
                    > blob.len();
                if dot
                    && emptied
                    && blob.iter().all(|&q| {
                        restore::in_body_source(id, q % 256, q / 256)
                            && !near_glyph[q]
                            && layer[q].is_none()
                    })
                {
                    let mut rim = Vec::new();
                    for &q in &blob {
                        let (x, y) = (q % 256, q / 256);
                        for ny in y.saturating_sub(1)..=(y + 1).min(191) {
                            for nx in x.saturating_sub(1)..=(x + 1).min(255) {
                                let r = ny * 256 + nx;
                                let c = original[r];
                                let blend = !restore::background_like(c)
                                    && c[1] >= 20
                                    && !(c[0] >= 26 && c[2] >= 26);
                                if blend
                                    && !restore::petal(c)
                                    && !near_glyph[r]
                                    && layer[r].is_none()
                                    && restore::in_body_source(id, nx, ny)
                                    && !rim.contains(&r)
                                {
                                    rim.push(r);
                                }
                            }
                        }
                    }
                    for &q in blob.iter().chain(&rim) {
                        desired[q] = original[q];
                        free[q] = true;
                    }
                    restored_dots += 1;
                }
            }
            // Petals and dots the passes above left cut, forked or clipped
            // come back whole from the screens that show them.
            let completed = restore::complete_decoration(
                id,
                &mut desired,
                &group_decoration[id / 22],
                &free,
                &near_glyph,
                &locked,
            );
            for &q in &completed {
                free[q] = true;
                completed_tiles[q / 256 / 8 * 32 + q % 256 / 8] = true;
            }
            rules = rules_for(&layer, &free, &completed_tiles);
            // In a tile holding new letters the letter bank shows a completed
            // petal's fill in its nearest warm colour (as the source does
            // under its letters) but would turn the petal's blended rim grey
            // or lilac: that rim goes back to the rays.
            let mut is_completed = vec![false; 256 * 192];
            for &q in &completed {
                is_completed[q] = true;
            }
            let mut rim_dropped = 0;
            for cell in 0..768 {
                let cell_pixels: Vec<usize> = (0..64)
                    .map(|p| (cell / 32 * 8 + p / 8) * 256 + cell % 32 * 8 + p % 8)
                    .collect();
                if !cell_pixels.iter().any(|&q| layer[q].is_some()) {
                    continue;
                }
                let Some((_, bank)) = cell_free_error(s, &palette, &rules, &desired, cell) else {
                    continue;
                };
                for &q in &cell_pixels {
                    let c = desired[q];
                    let fill = c[0] >= 26 && c[1] >= 20 && c[2] <= 10;
                    if is_completed[q] && !fill && nearest_error(&palette, bank, c) > FREE_LIMIT {
                        desired[q] =
                            restore::nearest_cyan(original, &footprints[id].mask, q % 256, q / 256)
                                .ok_or_else(|| {
                                    anyhow::anyhow!("screen {id} missing background donor")
                                })?;
                        rim_dropped += 1;
                    }
                }
            }
            completed_decoration = completed.len() - rim_dropped;
            rules = rules_for(&layer, &free, &completed_tiles);
        }
        let EncodedScreen {
            map,
            tiles,
            pixels,
            tile_count,
        } = match encode_screen_with_rules(s, |x, y| rules[y * 256 + x], &desired, |_, _, _| 0) {
            Ok(encoded) => encoded,
            Err(e) => {
                failures.push(format!("screen {id}: {e}"));
                continue;
            }
        };
        // Lettering colour check: every new lettering pixel keeps its intended
        // colour; approximated background stays within BACKGROUND_NEAR.
        let mut text_pixels = 0;
        let mut deviations = 0;
        let mut approximated = 0;
        let mut alternate_white = 0;
        for p in 0..256 * 192 {
            let got = palette[pixels[p] as usize];
            if let Some(color) = layer[p] {
                text_pixels += 1;
                let d: i32 = (0..3).map(|k| (got[k] - color[k]).pow(2)).sum();
                if pixels[p] % 16 == 0 || d > if white[p] { WHITE_NEAR } else { 0 } {
                    deviations += 1;
                }
                alternate_white += usize::from(white[p] && d > 0);
            }
            if let (PixelRule::Near(limit), None) = (rules[p], layer[p]) {
                let d: i32 = (0..3).map(|k| (got[k] - original[p][k]).pow(2)).sum();
                ensure!(d <= limit, "screen {id} background moved too far");
                approximated += usize::from(d > 0);
            }
        }
        ensure!(
            deviations == 0 || restored.is_empty(),
            "screen {id}: {deviations} lettering pixels changed colour"
        );
        let mut members = Vec::new();
        for (member, raw) in [(id * 3, map), (id * 3 + 1, tiles)] {
            let packed = crate::compress::pack(&raw)?;
            ensure!(
                unpack_halfword(&packed)? == raw,
                "screen halfword round trip failed"
            );
            if packed.len() > narc.members[member].len() {
                failures.push(format!(
                    "screen {id} member {member} capacity exceeded: {} > {}",
                    packed.len(),
                    narc.members[member].len()
                ));
            }
            members.push(json!({"member":member,"decoded_sha256":sha(&raw),"stored_size":packed.len(),"capacity":narc.members[member].len()}));
            replacements.insert(member, packed);
        }
        // Intended lettering colours (alpha 255 exact, 128 near-white body
        // fill) for checks of the final ROM against this preparation.
        let mut lettering_rgba = vec![0u8; 256 * 192 * 4];
        for p in 0..256 * 192 {
            if let Some(c) = layer[p] {
                for k in 0..3 {
                    lettering_rgba[p * 4 + k] = (c[k] * 255 / 31) as u8;
                }
                lettering_rgba[p * 4 + 3] = if white[p] { 128 } else { 255 };
            }
        }
        previews.push((id, pixels.clone(), s.palette.clone(), lettering_rgba));
        let mut record = json!({"id":id,"japanese_title":caption.japanese_title,"title":caption.title,"lines":caption.lines,"source_pixels_sha256":sha(&s.pixels),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&s.palette),"tile_count":tile_count,"members":members,
            "lettering_pixels":text_pixels,"lettering_color_deviations":deviations,"dropped_outline_pixels":dropped_outline,"fallback_cyan_pixels":fallback_pixels,"late_cleared_fragment_pixels":cleared_late,"glyph_side_fragment_pixels":cleared_beside,"restored_source_dots":restored_dots,"completed_decoration_pixels":completed_decoration,"alternate_white_pixels":alternate_white,"body_fill_rgb5":body_fill,"body_outline_rgb5":body_outline});
        if !caption.line_baselines.is_empty() {
            record["line_baselines"] = json!(caption.line_baselines);
        }
        if !caption.line_indents.is_empty() {
            record["line_indents"] = json!(caption.line_indents);
        }
        if !line_gaps.is_empty() {
            record["emphasis_tile_gaps"] = json!(line_gaps);
        }
        if !caption.emphasis.is_empty() {
            record["emphasis"] = json!(caption.emphasis);
            record["emphasis_pixels"] = json!(emphasis_pixels.iter().filter(|&&v| v).count());
        }
        if !caption.source_prefixes.is_empty() {
            record["source_prefixes"] = json!(caption.source_prefixes);
            record["line_baselines"] = json!(caption.line_baselines);
            record["source_symbol_pixels"] = json!(symbol_pixels.iter().filter(|&&v| v).count());
        }
        if let Some(r) = restored.get(id) {
            record["background"] = json!({"mode":"source_footprint_reconstruction",
                "footprint_pixels":footprints[id].mask.iter().filter(|&&v|v).count(),
                "cross_screen_pixels":r.cross_screen,"cleared_fragment_pixels":r.cleared.len(),"petal_consensus_pixels":r.consensus,"nearest_cyan_pixels":r.nearest,
                "approximated_background_pixels":approximated});
        }
        if let Some(rendering) = title_record {
            record["title_rendering"] = rendering;
        }
        records.push(record);
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    let rebuilt = crate::archive::replace(source, &replacements)?;
    let parsed = Narc::parse(&rebuilt)?;
    for (id, pixels, palette, _) in &previews {
        let check = read(&parsed, *id)?;
        ensure!(
            check.pixels == *pixels && check.palette == *palette,
            "rebuilt screen pixel/palette mismatch"
        );
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("screens.narc"), &rebuilt)?;
    for (id, pixels, palette, lettering) in previews {
        write_png(
            &out.join(format!("{id:02}-korean.png")),
            256,
            192,
            &rgba(&pixels, &palette)?,
        )?;
        if !restored.is_empty() {
            write_png(
                &out.join(format!("{id:02}-lettering.png")),
                256,
                192,
                &lettering,
            )?;
        }
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"screens.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let mut report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&rebuilt),"records":records,"background":"title and body regions each filled with their most common source colour, erasing source lettering; Korean white fill with the source lettering outline colour","editable":"title [24,232)x[25,46); body [20,176)x[60,180) for screens 0..21 except screen 18 right=min(176,160+floor(max(y-64,0)/5)); [16,244)x[56,180) for 22..43 except screen 35 bottom=184; panel artwork kept","protected":"all other rendered colors and transparent-zero status, all palette bytes, all other NARC members/metadata except selected ends","runtime_verified":false,"human_reviewed":false});
    if tr.masked_body_background {
        report["background"] = json!(
            "Source lettering footprint (title letters grown 1px; body outline/light letters grown 2px, scanned to y<186, excluding pixels warm in half the screen group) rebuilt: title with the capsule colour, body from another screen of the group whose visible 5x5 neighbourhood agrees within 2 per channel, else the nearest visible cyan pixel. Hidden pattern is estimated. New lettering is exact; unchanged cool background sharing its tile may move by squared RGB5 distance <= 3; everything else protected."
        );
    }
    if let Some(bytes) = artwork_bytes {
        report["title_artwork_sha256"] = json!(sha(&bytes));
        report["title_rendering"] = json!(
            "complete generated lettering composited over the verified flat title panel; body remains the existing font rendering"
        );
    }
    if let Some(bytes) = title_font_bytes {
        report["title_font_sha256"] = json!(sha(&bytes));
        report["title_rendering"] = json!(
            "Galmuri14 15px, bold by 1px, slanted 1px per 4 rows, source accent colour for the leading run and source pale fill for the rest, source outline with 2px drop; runs separated by tile when no bank holds both fills"
        );
    }
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
