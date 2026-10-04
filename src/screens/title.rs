//! Rule screen titles drawn like the source capsule lettering: bold Galmuri14
//! on its 15px grid, slanted one pixel per four rows, with a navy outline and
//! a two-pixel drop. The leading accent run (mode name) uses the source pink,
//! the rest the source pale fill. Colours come from the source title pixels.

use super::restore::{TITLE_ZONE, WIDTH, in_title, warm};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const SIZE: f32 = 15.0;

pub(super) struct TitleColors {
    pub accent: [i32; 3],
    pub fill: Option<[i32; 3]>,
    pub outline: [i32; 3],
    pub background: [i32; 3],
}

/// Most common pink and pale letter colours inside the source title.
pub(super) fn source_colors(original: &[[i32; 3]], background: [i32; 3]) -> Result<TitleColors> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<[i32; 3], usize> = BTreeMap::new();
    for (p, &color) in original.iter().enumerate() {
        if in_title(p % WIDTH, p / WIDTH) {
            *counts.entry(color).or_default() += 1;
        }
    }
    let pick = |f: &dyn Fn([i32; 3]) -> bool| {
        counts
            .iter()
            .filter(|(c, _)| **c != background && f(**c))
            .max_by_key(|(_, n)| **n)
            .map(|(c, _)| *c)
    };
    let accent = pick(&|c| c[0] > c[1] + 4 && c[0] > 20)
        .ok_or_else(|| anyhow::anyhow!("source title has no accent colour"))?;
    let fill = pick(&|c| c.iter().sum::<i32>() >= 66 && !warm(c) && c[0] <= c[1] + 4);
    let outline = pick(&|c| c.iter().sum::<i32>() < 45)
        .ok_or_else(|| anyhow::anyhow!("source title has no outline colour"))?;
    Ok(TitleColors {
        accent,
        fill,
        outline,
        background,
    })
}

/// True when one palette bank holds every colour a mixed tile needs.
pub(super) fn single_bank(palette: &[[i32; 3]], colors: &TitleColors) -> bool {
    palette.chunks(16).any(|bank| {
        let bank = &bank[1..];
        [
            Some(colors.accent),
            colors.fill,
            Some(colors.outline),
            Some(colors.background),
        ]
        .into_iter()
        .flatten()
        .all(|c| bank.contains(&c))
    })
}

struct Glyphs {
    /// (x, y relative to baseline, accent)
    fill: Vec<(i32, i32, bool)>,
    /// Fill pixels of the widened 통 (same coordinates as `fill`).
    tong: std::collections::BTreeSet<(i32, i32)>,
    split: i32,
}

fn rasterize(font: &fontdue::Font, text: &str, accent: usize, gap: i32) -> Result<Glyphs> {
    let mut fill = Vec::new();
    let mut tong = std::collections::BTreeSet::new();
    let mut cursor = 0i32;
    let mut split = 0;
    for (i, c) in text.chars().enumerate() {
        if i == accent && i > 0 {
            cursor += gap;
            split = cursor;
        }
        if c != ' ' {
            ensure!(font.lookup_glyph_index(c) != 0, "missing title glyph: {c}");
        }
        let (m, mut bitmap) = font.rasterize(c, SIZE);
        let mut height = m.height;
        if c == '통' {
            // Galmuri14's ㅌ puts its middle bar one pixel under the top bar;
            // outlined and slanted, 통 then reads as 동. Repeat the row under
            // the top bar (the glyph grows one pixel upwards) so both bars keep
            // a two-pixel gap, and shorten the middle bar by two pixels.
            let full: Vec<usize> = (0..m.height)
                .filter(|&y| {
                    (0..m.width)
                        .filter(|&x| bitmap[y * m.width + x] >= 128)
                        .count()
                        >= 9
                })
                .collect();
            ensure!(
                full.len() >= 3 && full[1] == full[0] + 2,
                "unexpected 통 glyph"
            );
            let middle = full[1];
            let mut right: Vec<usize> = (0..m.width)
                .filter(|&x| bitmap[middle * m.width + x] >= 128)
                .collect();
            right.reverse();
            for &x in right.iter().take(2) {
                bitmap[middle * m.width + x] = 0;
            }
            let gap = full[0] + 1;
            let row: Vec<u8> = bitmap[gap * m.width..(gap + 1) * m.width].to_vec();
            bitmap.splice(gap * m.width..gap * m.width, row);
            height += 1;
        }
        for y in 0..height {
            for x in 0..m.width {
                if bitmap[y * m.width + x] >= 128 {
                    let dx = cursor + m.xmin + x as i32;
                    let dy = -m.ymin - height as i32 + y as i32;
                    // Bold by one pixel to the right, like the source weight.
                    fill.push((dx, dy, i < accent));
                    fill.push((dx + 1, dy, i < accent));
                    if c == '통' {
                        tong.insert((dx, dy));
                        tong.insert((dx + 1, dy));
                    }
                }
            }
        }
        cursor += m.advance_width.round() as i32 + i32::from(c != ' ');
    }
    Ok(Glyphs { fill, tong, split })
}

pub(super) struct TitleLayer {
    pub pixels: Vec<(usize, [i32; 3])>,
    pub record: Value,
}

/// Lay out a centred slanted title. When no bank holds both fills, the pale
/// run moves right until no 8x8 tile mixes accent and pale fill.
pub(super) fn draw(
    font: &fontdue::Font,
    text: &str,
    accent: usize,
    colors: &TitleColors,
    single_bank: bool,
) -> Result<TitleLayer> {
    let count = text.chars().count();
    ensure!(
        accent <= count && (accent == count || colors.fill.is_some()),
        "title accent range"
    );
    let [x0, y0, x1, y1] = TITLE_ZONE;
    for gap in 0..16 {
        let glyphs = rasterize(font, text, accent, gap)?;
        // Slant: one pixel right per four rows above the baseline.
        let tong: std::collections::BTreeSet<(i32, i32)> = glyphs
            .tong
            .iter()
            .map(|&(x, y)| (x + (-y).div_euclid(4), y))
            .collect();
        let slanted: Vec<_> = glyphs
            .fill
            .iter()
            .map(|&(x, y, a)| (x + (-y).div_euclid(4), y, a))
            .collect();
        let mut fill = std::collections::BTreeMap::new();
        for &(x, y, a) in &slanted {
            fill.insert((x, y), a);
        }
        let mut outline = std::collections::BTreeSet::new();
        for &(x, y) in fill.keys() {
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
                (0, 2),
                (1, 2),
            ] {
                if !fill.contains_key(&(x + dx, y + dy)) {
                    outline.insert((x + dx, y + dy));
                }
            }
        }
        // A one-pixel gap between two stacked bars (a run of at least three
        // pixels with fill above and below, as in ㅌ) stays capsule-coloured so
        // the bars remain separate instead of merging through the outline.
        // In the widened 통 also the lower row of a two-pixel gap, so its ㅌ
        // bars show one outline row and one capsule row between them.
        let sandwiched = |x: i32, y: i32| {
            fill.contains_key(&(x, y + 1))
                && (fill.contains_key(&(x, y - 1))
                    || (tong.contains(&(x, y - 2))
                        && tong.contains(&(x, y + 1))
                        && !fill.contains_key(&(x, y - 1))))
        };
        let gap: std::collections::BTreeSet<(i32, i32)> = outline
            .iter()
            .copied()
            .filter(|&(x, y)| {
                sandwiched(x, y)
                    && ((sandwiched(x - 1, y) && sandwiched(x + 1, y))
                        || (sandwiched(x - 2, y) && sandwiched(x - 1, y))
                        || (sandwiched(x + 1, y) && sandwiched(x + 2, y)))
            })
            .collect();
        outline.retain(|p| !gap.contains(p));
        let all = || fill.keys().chain(outline.iter());
        let (min_x, max_x) = (
            all().map(|p| p.0).min().unwrap(),
            all().map(|p| p.0).max().unwrap(),
        );
        let (min_y, max_y) = (
            all().map(|p| p.1).min().unwrap(),
            all().map(|p| p.1).max().unwrap(),
        );
        let (w, h) = (max_x - min_x + 1, max_y - min_y + 1);
        ensure!(
            w <= (x1 - x0) as i32 && h <= (y1 - y0) as i32,
            "title too large: {text} {w}x{h}"
        );
        let ox = (x0 + x1) as i32 / 2 - w / 2 - min_x;
        let oy = y0 as i32 + (y1 - y0) as i32 / 2 - h / 2 - min_y;
        let tile = |x: i32, y: i32| ((x + ox) / 8, (y + oy) / 8);
        let accent_tiles: std::collections::BTreeSet<_> = fill
            .iter()
            .filter(|(_, a)| **a)
            .map(|(p, _)| tile(p.0, p.1))
            .collect();
        let mixed = fill
            .iter()
            .any(|(p, a)| !*a && accent_tiles.contains(&tile(p.0, p.1)));
        if mixed && !single_bank {
            continue;
        }
        let mut pixels = Vec::new();
        for (&(x, y), &a) in &fill {
            let color = if a {
                colors.accent
            } else {
                colors.fill.unwrap()
            };
            pixels.push((((y + oy) as usize) * WIDTH + (x + ox) as usize, color));
        }
        for &(x, y) in &outline {
            pixels.push((
                ((y + oy) as usize) * WIDTH + (x + ox) as usize,
                colors.outline,
            ));
        }
        for &(p, _) in &pixels {
            ensure!(
                in_title(p % WIDTH, p / WIDTH),
                "title outside capsule: {text}"
            );
        }
        return Ok(TitleLayer {
            pixels,
            record: json!({"font_px":SIZE,"accent_chars":accent,"accent_rgb5":colors.accent,
                "fill_rgb5":colors.fill,"outline_rgb5":colors.outline,"background_rgb5":colors.background,
                "single_bank":single_bank,"run_gap":gap,"split_x":glyphs.split + ox,
                "bbox":[min_x + ox, min_y + oy, max_x + ox + 1, max_y + oy + 1]}),
        });
    }
    anyhow::bail!("title runs cannot be separated by tile: {text}")
}
