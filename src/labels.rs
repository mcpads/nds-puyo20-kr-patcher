//! Text labels stored as texture-list textures (A3I5, A5I3, 4bpp).
//! Redraws only a declared rectangle; palette, other rows and members stay intact.
use crate::{assets::json_file, battle_ui, buttons, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    archive: String,
    archive_sha256: String,
    table: String,
    table_sha256: String,
    state: String,
    entries: Vec<Label>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Label {
    pub(crate) texture: usize,
    pub(crate) japanese: String,
    pub(crate) korean: String,
    /// Original decoded texel hash; the Japanese reading was checked against this image.
    pub(crate) source_sha256: String,
    /// Editable rectangle `[x0, y0, x1, y1)`; default is the opaque bounding box.
    #[serde(default)]
    region: Option<[usize; 4]>,
    /// Convex native-pixel boundary protecting slanted frames inside the rectangle.
    #[serde(default)]
    region_polygon: Vec<[i32; 2]>,
    /// Vertical shear numerator/denominator about the rectangle's horizontal centre.
    #[serde(default)]
    shear_y: Option<[i32; 2]>,
    /// Switch glyph fill to this palette index at the given absolute row.
    #[serde(default)]
    lower_fill: Option<[usize; 2]>,
    /// Row gradient of the glyph fill: `[row, index]` pairs in ascending absolute
    /// rows; each index applies from its row until the next pair.
    #[serde(default)]
    fill_rows: Vec<[usize; 2]>,
    /// `[index, step]`: sparse highlight dots on the upper edge of strokes
    /// (glyph pixels with no glyph pixel above) where `(x + y) % step == 0`,
    /// like the original's speckled lettering.
    #[serde(default)]
    highlight: Option<[usize; 2]>,
    /// `all` spreads the highlight dots over every stroke edge (a glyph pixel
    /// with any of its four neighbours outside the glyph) where
    /// `(x + 2y) % step == 0`, a scattered lattice that forms no diagonal
    /// streaks, like the original's speckles on all sides of a stroke.
    #[serde(default)]
    highlight_all_edges: bool,
    /// `clear` makes the rectangle transparent; `row` interpolates each row
    /// between the unchanged pixels just outside the rectangle.
    #[serde(default = "clear")]
    background: String,
    #[serde(default = "regular")]
    font: String,
    #[serde(default)]
    size: Option<usize>,
    /// Outline thickness in pixels, drawn with 8-neighbour dilation.
    #[serde(default = "one")]
    outline: usize,
    #[serde(default)]
    shadow: Option<[i32; 2]>,
    #[serde(default)]
    fill: Option<[i32; 3]>,
    #[serde(default)]
    outline_color: Option<[i32; 3]>,
    /// Explicit palette indices, used where detection cannot separate text from art.
    #[serde(default)]
    fill_index: Option<usize>,
    #[serde(default)]
    outline_index: Option<usize>,
    /// `left` starts lines at the region's left edge plus the outline; default centres.
    #[serde(default)]
    align: Option<String>,
    /// Widen every stroke one pixel to the right, matching bold source lettering.
    #[serde(default)]
    bold: bool,
    /// With `bold`: skip widening that would close a one-pixel gap, so dense
    /// syllables such as 배 keep their counters.
    #[serde(default)]
    bold_keep_gaps: bool,
    /// Thicken vertical strokes by one pixel where a one-pixel gap to other
    /// ink remains (`bold_vertical_runs`), keeping narrow Hangul counters and
    /// vowel ticks open.
    #[serde(default)]
    bold_keep_gap: bool,
    /// Optional one-pixel ring outside the outline, for three-layer lettering.
    #[serde(default)]
    rim_color: Option<[i32; 3]>,
    /// Rim thickness in pixels (default 1), grown by 4-neighbour steps.
    #[serde(default)]
    rim_width: Option<usize>,
    /// Alpha for outline/shadow texels (alpha formats only); default fully opaque.
    #[serde(default)]
    stroke_alpha: Option<usize>,
    /// Extra pixels between lines of a multi-line label.
    #[serde(default)]
    line_gap: usize,
    /// Pixels added after each glyph advance; -1 lets outlined text fill a tight crop.
    #[serde(default)]
    letter_spacing: i32,
    /// Horizontal glyph scale in 0.5..1 for names wider than their slot;
    /// rasterised by area coverage instead of squeezing native pixels.
    #[serde(default)]
    scale_x: Option<f64>,
    /// With `scale_x`: pick quarter-pixel glyph phases that keep stems even.
    #[serde(default)]
    grid_fit: bool,
    /// Absolute top row of the ink instead of vertical centring in the region.
    #[serde(default)]
    top: Option<usize>,
    /// Leave 1px gaps between strokes unoutlined (a stroke on both opposite
    /// sides), so dense small lettering keeps the artwork showing through.
    #[serde(default)]
    open_counters: bool,
    /// Drop outline pixels outside the region instead of failing, for glyphs
    /// as tall as a cell whose original outline also meets the cell edge.
    #[serde(default)]
    clip_stroke: bool,
    /// `cw` lays the text out horizontally and turns it 90° clockwise into the
    /// region, for headers the game draws rotated.
    #[serde(default)]
    rotate: Option<String>,
    /// Palette index painted over the region by the `flat` background.
    #[serde(default)]
    background_index: Option<usize>,
    /// Limit harmonic reconstruction to source lettering and its declared rim.
    #[serde(default)]
    background_mask: Option<BackgroundMask>,
    /// Restrict reconstructed background colours; keep frame/ink colours out.
    #[serde(default)]
    background_indices: Vec<usize>,
    /// Rectangles copied within the texture before any label draws, for
    /// lettering that reaches artwork another cell of the atlas shows clean.
    #[serde(default)]
    pub(crate) copies: Vec<Copy>,
    /// Source icon inside the region that leads the name, as in the original
    /// rows: it is moved to sit `gap` pixels before the text ink and the pair
    /// is centred in the region together.
    #[serde(default)]
    lead_icon: Option<LeadIcon>,
    /// Draw ASCII letters with another role's face at the label size.
    #[serde(default)]
    latin_font: Option<String>,
    /// Space advance in pixels; default a third of the font size.
    #[serde(default)]
    space_width: Option<i32>,
    /// `masked_rows` only: a texture of the same pill artwork whose clean
    /// texels stand in for texels this label's lettering hides.
    #[serde(default)]
    row_source: Option<RowSource>,
    /// Colours resolved from `row_source` before drawing.
    #[serde(skip)]
    reference: Reference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LeadIcon {
    /// Source rectangle `[x0, y0, x1, y1)` of the icon texels.
    rect: [usize; 4],
    /// Empty columns between the icon and the text ink (stroke included).
    gap: usize,
}

/// Source colours per texel (`clean` columns only) and per interior row.
#[derive(Default)]
struct Reference {
    width: usize,
    texels: Vec<Option<[i32; 3]>>,
    rows: Vec<Option<[i32; 3]>>,
    /// Every opaque source texel, for `calibrate`.
    opaque: Vec<Option<[i32; 3]>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowSource {
    archive: String,
    archive_sha256: String,
    table: String,
    table_sha256: String,
    texture: usize,
    /// Source column ranges `[x0, x1)` without lettering; masked texels there
    /// take the source texel at the same position.
    clean: Vec<[usize; 2]>,
    /// Clean interior columns `[x0, x1)` of the source sampled for a row colour.
    columns: [usize; 2],
    /// Columns `[x0, x1)` of this label whose other masked texels take the row colour.
    span: [usize; 2],
    /// Label row minus source row, for artwork stacked lower in an atlas.
    #[serde(default)]
    row_offset: usize,
    /// Map each source colour to the palette index this label keeps at the
    /// texels both leave unchanged (majority), instead of the nearest palette
    /// colour. For the same artwork quantised to a slightly different palette;
    /// colours seen nowhere unchanged still take the nearest colour.
    #[serde(default)]
    calibrate: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackgroundMask {
    indices: Vec<usize>,
    radius: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Copy {
    /// Destination rectangle `[x0, y0, x1, y1)`.
    rect: [usize; 4],
    /// Top-left corner of the source rectangle.
    from: [usize; 2],
}
fn clear() -> String {
    "clear".into()
}
fn regular() -> String {
    "regular".into()
}
fn one() -> usize {
    1
}

pub(crate) struct Texture {
    pub(crate) member: usize,
    pub(crate) palette: Vec<u8>,
    pub(crate) format: usize,
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// (colour index, alpha) for every stored texel, including rows below `height`.
    pub(crate) texels: Vec<(usize, usize)>,
}

impl Texture {
    pub(crate) fn max_alpha(&self) -> usize {
        match self.format {
            1 => 7,
            6 => 31,
            _ => 1,
        }
    }
    fn encode(&self, texels: &[(usize, usize)]) -> Result<Vec<u8>> {
        Ok(match self.format {
            1 => texels.iter().map(|&(i, a)| (a << 5 | i) as u8).collect(),
            6 => texels.iter().map(|&(i, a)| (a << 3 | i) as u8).collect(),
            3 => {
                ensure!(
                    texels.iter().all(|&(i, a)| (i == 0) == (a == 0)),
                    "4bpp transparency is index zero"
                );
                texels
                    .chunks_exact(2)
                    .map(|p| (p[0].0 | p[1].0 << 4) as u8)
                    .collect()
            }
            _ => anyhow::bail!("unsupported texture format"),
        })
    }
    fn rgb(&self, i: usize) -> Result<[i32; 3]> {
        buttons::rgb(&self.palette, i)
    }
    /// Nearest palette colour; 4bpp index zero is transparency and never chosen.
    fn nearest(&self, target: [i32; 3]) -> Result<usize> {
        let first = usize::from(self.format == 3);
        let mut best = (i32::MAX, first);
        for i in first..self.palette.len() / 2 {
            let c = self.rgb(i)?;
            let d = (0..3).map(|k| (c[k] - target[k]).pow(2)).sum();
            if d < best.0 {
                best = (d, i);
            }
        }
        Ok(best.1)
    }
}

fn texture(narc: &Narc, table: &[u8], id: usize) -> Result<Texture> {
    let row = slice(table, id * 12, 12)?;
    let member = u16le(row, 0)?;
    let format = u16le(row, 4)?;
    let (width, height) = (u16le(row, 8)?, u16le(row, 10)?);
    ensure!(
        matches!(format, 1 | 3 | 6) && width > 0 && height > 0,
        "unsupported texture {id}"
    );
    let raw = unpack_halfword(
        narc.members
            .get(member)
            .ok_or_else(|| anyhow::anyhow!("member outside archive"))?,
    )?;
    let palette = unpack(
        narc.members
            .get(u16le(row, 2)?)
            .ok_or_else(|| anyhow::anyhow!("palette outside archive"))?,
    )?;
    let texels: Vec<(usize, usize)> = match format {
        1 => raw
            .iter()
            .map(|v| ((v & 31) as usize, (v >> 5) as usize))
            .collect(),
        6 => raw
            .iter()
            .map(|v| ((v & 7) as usize, (v >> 3) as usize))
            .collect(),
        _ => raw
            .iter()
            .flat_map(|v| [v & 15, v >> 4])
            .map(|i| (i as usize, usize::from(i != 0)))
            .collect(),
    };
    ensure!(
        texels.len() % width == 0 && texels.len() >= width * height,
        "texture {id} extent"
    );
    Ok(Texture {
        member,
        palette,
        format,
        width,
        height,
        texels,
    })
}

/// Most common opaque colour of each row across the source columns; `None`
/// where fewer than three quarters of the columns share it (not a flat row).
fn reference(rom: &Rom, r: &RowSource) -> Result<Reference> {
    let source = rom.data(rom.file(&r.archive)?);
    let table = rom.data(rom.file(&r.table)?);
    ensure!(
        sha(source) == r.archive_sha256 && sha(table) == r.table_sha256,
        "row source identity changed"
    );
    let t = texture(&Narc::parse(source)?, table, r.texture)?;
    let [c0, c1] = r.columns;
    ensure!(
        c0 < c1 && c1 <= t.width && r.span[0] < r.span[1],
        "row source columns"
    );
    ensure!(
        r.clean.iter().all(|&[a, b]| a < b && b <= t.width),
        "row source clean columns"
    );
    let full = t.max_alpha();
    let opaque = (0..t.width * t.height)
        .map(|p| {
            let (i, a) = t.texels[p];
            (a == full).then(|| t.rgb(i)).transpose()
        })
        .collect::<Result<_>>()?;
    let texels = (0..t.width * t.height)
        .map(|p| {
            let (i, a) = t.texels[p];
            let clean = r
                .clean
                .iter()
                .any(|&[a0, b0]| (a0..b0).contains(&(p % t.width)));
            (clean && a == full).then(|| t.rgb(i)).transpose()
        })
        .collect::<Result<_>>()?;
    let rows = (0..t.height)
        .map(|y| {
            let mut counts = BTreeMap::new();
            for x in c0..c1 {
                let (i, a) = t.texels[y * t.width + x];
                if a == t.max_alpha() {
                    *counts.entry(t.rgb(i)?).or_insert(0usize) += 1;
                }
            }
            Ok(counts
                .into_iter()
                .max_by_key(|&(c, n)| (n, std::cmp::Reverse(c)))
                .filter(|&(_, n)| n * 4 >= (c1 - c0) * 3)
                .map(|(c, _)| c))
        })
        .collect::<Result<_>>()?;
    Ok(Reference {
        width: t.width,
        texels,
        rows,
        opaque,
    })
}

/// Area coverage by native pixel.
type Coverage = BTreeMap<(i32, i32), f64>;

/// Coverage of one supersampled glyph bitmap placed at supersample origin
/// `(ox, oy)`, area-averaged into target pixels (x scaled, y sheared).
fn splat(
    m: &fontdue::Metrics,
    bitmap: &[u8],
    (ox, oy): (i32, i32),
    scale: f64,
    shear: f64,
    k: f64,
) -> BTreeMap<(i32, i32), f64> {
    let mut coverage = BTreeMap::new();
    for y in 0..m.height {
        for x in 0..m.width {
            let value = f64::from(bitmap[y * m.width + x]) / 255.0;
            if value == 0.0 {
                continue;
            }
            let hx = ox + x as i32;
            let hy = oy + y as i32;
            // The supersample spans [hx, hx+1) x [hy, hy+1) in supersample
            // units; in target units x is scaled and y shifted by the shear.
            let (x0, x1) = (f64::from(hx) * scale / k, f64::from(hx + 1) * scale / k);
            let y0 = f64::from(hy) / k + shear * (x0 + x1) / 2.0;
            let y1 = y0 + 1.0 / k;
            let mut ty = y0.floor() as i32;
            while f64::from(ty) < y1 {
                let dy = y1.min(f64::from(ty + 1)) - y0.max(f64::from(ty));
                let mut tx = x0.floor() as i32;
                while f64::from(tx) < x1 {
                    let dx = x1.min(f64::from(tx + 1)) - x0.max(f64::from(tx));
                    if dx > 0.0 && dy > 0.0 {
                        *coverage.entry((tx, ty)).or_insert(0.0) += value * dx * dy;
                    }
                    tx += 1;
                }
                ty += 1;
            }
        }
    }
    coverage
}

/// Pixels whose coverage lies near the ink threshold, where a stroke edge can
/// flip between rows and grow a stray nub.
fn ambiguity(coverage: &BTreeMap<(i32, i32), f64>) -> usize {
    coverage
        .values()
        .filter(|&&v| (0.3..0.7).contains(&v))
        .count()
}

/// One line narrowed horizontally by `scale` (0.5..1) and optionally sheared
/// vertically by `shear` rows per column: glyphs are rasterised at four times the
/// size, each supersample is moved as a small box (x scaled, y shifted by the
/// shear at its centre) and its coverage is area-averaged into target pixels;
/// pixels at least half covered become ink. Shearing before thresholding keeps
/// slanted strokes continuous instead of stepping whole native pixels. With
/// `grid_fit`, each glyph takes the horizontal quarter-pixel phase and the line
/// the vertical phase that leave the fewest pixels near the threshold, so
/// straight stems keep one width. Points use the unscaled ink coordinates of
/// `ink`: x from the line start, y from the line top with the font ascent as
/// baseline.
fn condensed_line(
    font: &fontdue::Font,
    line: &str,
    size: usize,
    letter_spacing: i32,
    scale: f64,
    shear: f64,
    grid_fit: bool,
) -> Result<Vec<(i32, i32)>> {
    const K: i32 = 4;
    let k = f64::from(K);
    ensure!((0.5..=1.0).contains(&scale), "scale_x outside 0.5..1");
    ensure!(shear.abs() <= 1.0, "shear outside -1..1");
    let big = (size as i32 * K) as f32;
    let ascent = font
        .horizontal_line_metrics(big)
        .map(|m| m.ascent.round() as i32)
        .unwrap_or(size as i32 * K);
    // Target-pixel steps in supersamples.
    let to_super = |px: i32| (f64::from(px) * k / scale).round() as i32;
    let mut glyphs = Vec::new();
    let mut cursor = 0i32;
    for ch in line.chars() {
        ensure!(
            ch == ' ' || font.lookup_glyph_index(ch) != 0,
            "font has no glyph for {ch}"
        );
        if ch == ' ' {
            cursor += to_super((size as i32 + 1) / 3);
            continue;
        }
        let (m, bitmap) = font.rasterize(ch, big);
        let origin = (cursor + m.xmin, ascent - m.ymin - m.height as i32);
        cursor += m.advance_width.round() as i32 + to_super(letter_spacing);
        glyphs.push((m, bitmap, origin));
    }
    let phases = if grid_fit { K } else { 1 };
    let mut best: Option<(usize, Coverage)> = None;
    for oy in 0..phases {
        let mut total = 0;
        let mut coverage: BTreeMap<(i32, i32), f64> = BTreeMap::new();
        for (m, bitmap, (x, y)) in &glyphs {
            let (score, glyph) = (0..phases)
                .map(|ox| {
                    let c = splat(m, bitmap, (x + ox, y + oy), scale, shear, k);
                    (ambiguity(&c), c)
                })
                .min_by_key(|(score, _)| *score)
                .expect("at least one phase");
            total += score;
            for (p, v) in glyph {
                *coverage.entry(p).or_insert(0.0) += v;
            }
        }
        if best.as_ref().is_none_or(|(score, _)| total < *score) {
            best = Some((total, coverage));
        }
    }
    Ok(best
        .map(|(_, c)| c)
        .unwrap_or_default()
        .into_iter()
        .filter(|&(_, v)| v >= 0.5)
        .map(|(p, _)| p)
        .collect())
}

/// Native-pixel ink positions.
pub(crate) type Ink = BTreeSet<(i32, i32)>;

/// How glyphs of a label are spaced and rasterised.
#[derive(Clone, Copy, Default)]
pub(crate) struct Spacing {
    /// Extra pixels between lines.
    pub(crate) line_gap: usize,
    /// Pixels added after each glyph advance.
    pub(crate) letter_spacing: i32,
    /// Horizontal scale; `Some` selects area rasterisation.
    pub(crate) scale_x: Option<f64>,
    /// Vertical shear applied during area rasterisation.
    pub(crate) shear: f64,
    /// Space advance on the native glyph path; default a third of the size.
    pub(crate) space: Option<i32>,
    /// Choose quarter-pixel glyph phases that keep stems even (area path only).
    pub(crate) grid_fit: bool,
    /// Absolute top row of the ink; `None` centres it vertically.
    pub(crate) top: Option<usize>,
}

/// Glyph ink centred in `rect` plus its rounded outline (radius `outline`) and an
/// optional drop shadow, exactly as `draw` builds them; for callers that paint
/// their own canvases. Stroke excludes the glyph pixels.
pub(crate) fn outlined(
    font: &fontdue::Font,
    text: &str,
    size: usize,
    rect: [usize; 4],
    spacing: Spacing,
    outline: usize,
    shadow: Option<[i32; 2]>,
) -> Result<(Ink, Ink)> {
    let glyphs = ink(font, text, size, rect, None, spacing, None)?;
    let mut stroke = BTreeSet::new();
    let r = outline as i32;
    for &(x, y) in &glyphs {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r + 1 {
                    stroke.insert((x + dx, y + dy));
                }
            }
        }
        if let Some([sx, sy]) = shadow {
            for d in 0..=r {
                stroke.insert((x + sx + d, y + sy + d));
            }
        }
    }
    let stroke = stroke.difference(&glyphs).copied().collect();
    Ok((glyphs, stroke))
}

/// Rasterise lines centred in the rectangle; returns glyph ink in texture coordinates.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ink(
    font: &fontdue::Font,
    text: &str,
    size: usize,
    rect: [usize; 4],
    left: Option<i32>,
    spacing: Spacing,
    latin: Option<&fontdue::Font>,
) -> Result<Ink> {
    let Spacing {
        line_gap,
        letter_spacing,
        scale_x,
        shear,
        space,
        grid_fit,
        top,
    } = spacing;
    ensure!(
        scale_x.is_none() || (latin.is_none() && space.is_none()),
        "latin font and space width need the native glyph path"
    );
    let [x0, y0, x1, y1] = rect;
    let lines: Vec<&str> = text.split('\n').collect();
    let line_height = (size + 1 + line_gap) as i32;
    let ascent = font
        .horizontal_line_metrics(size as f32)
        .map(|m| m.ascent.round() as i32)
        .unwrap_or(size as i32);
    let mut raw = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        if let Some(scale) = scale_x {
            let points = condensed_line(font, line, size, letter_spacing, scale, shear, grid_fit)?;
            raw.extend(
                points
                    .into_iter()
                    .map(|(x, y)| (row, x, row as i32 * line_height + y)),
            );
            continue;
        }
        let mut cursor = 0i32;
        for ch in line.chars() {
            // Latin letters may come from the regular face when the label font's
            // own letters misread (DenkiChip's rounded capitals).
            let font = match latin {
                Some(latin) if ch.is_ascii_alphabetic() => latin,
                _ => font,
            };
            ensure!(
                ch == ' ' || font.lookup_glyph_index(ch) != 0,
                "font has no glyph for {ch}"
            );
            let (m, bitmap) = font.rasterize(ch, size as f32);
            for y in 0..m.height {
                for x in 0..m.width {
                    if bitmap[y * m.width + x] >= 128 {
                        let py =
                            row as i32 * line_height + ascent - m.ymin - m.height as i32 + y as i32;
                        raw.push((row, cursor + m.xmin + x as i32, py));
                    }
                }
            }
            cursor += if ch == ' ' {
                space.unwrap_or((size as i32 + 1) / 3)
            } else {
                m.advance_width.round() as i32 + letter_spacing
            };
        }
    }
    ensure!(!raw.is_empty(), "empty label {text}");
    let (ymin, ymax) = (
        raw.iter().map(|p| p.2).min().unwrap(),
        raw.iter().map(|p| p.2).max().unwrap(),
    );
    let dy = match top {
        Some(row) => {
            ensure!((y0..y1).contains(&row), "label top outside region");
            row as i32 - ymin
        }
        None => y0 as i32 + ((y1 - y0) as i32 - (ymax - ymin + 1)) / 2 - ymin,
    };
    let mut out = BTreeSet::new();
    for row in 0..lines.len() {
        let pts: Vec<_> = raw.iter().filter(|p| p.0 == row).collect();
        if pts.is_empty() {
            continue;
        }
        let (xmin, xmax) = (
            pts.iter().map(|p| p.1).min().unwrap(),
            pts.iter().map(|p| p.1).max().unwrap(),
        );
        let dx = match left {
            Some(pad) => x0 as i32 + pad - xmin,
            None => x0 as i32 + ((x1 - x0) as i32 - (xmax - xmin + 1)) / 2 - xmin,
        };
        out.extend(pts.iter().map(|p| (p.1 + dx, p.2 + dy)));
    }
    Ok(out)
}

fn most_common(v: impl Iterator<Item = usize>) -> Option<usize> {
    let mut c = BTreeMap::new();
    for i in v {
        *c.entry(i).or_insert(0) += 1;
    }
    c.into_iter()
        .max_by_key(|&(i, n)| (n, std::cmp::Reverse(i)))
        .map(|(i, _)| i)
}

/// Widen each vertical stroke (a column run of two or more ink pixels) by one
/// pixel, to the right when that keeps a one-pixel gap to other ink, else to
/// the left under the same condition, else not at all. Vowel ticks and narrow
/// counters therefore stay open.
pub(crate) fn bold_vertical_runs(ink: &BTreeSet<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut wide = Vec::new();
    for &(x, y) in ink {
        if ink.contains(&(x, y - 1)) || !ink.contains(&(x, y + 1)) {
            continue;
        }
        let mut end = y;
        while ink.contains(&(x, end + 1)) {
            end += 1;
        }
        let free = |side: i32| {
            (y - 1..=end + 1).all(|r| !ink.contains(&(x + side, r)))
                && (y..=end).all(|r| !ink.contains(&(x + 2 * side, r)))
        };
        let side = if free(1) {
            1
        } else if free(-1) {
            -1
        } else {
            continue;
        };
        wide.extend((y..=end).map(|r| (x + side, r)));
    }
    wide
}

/// Paint one label into `next`, reading colours from the untouched `stored` texels.
pub(crate) fn draw(
    t: &Texture,
    next: &mut [(usize, usize)],
    e: &Label,
    regular: &fontdue::Font,
    small: &fontdue::Font,
    medium: Option<&fontdue::Font>,
    narrow: Option<&fontdue::Font>,
) -> Result<([usize; 4], Value)> {
    let stored = &t.texels;
    // A texture may contain several disjoint labels. Check this draw against
    // its entry state so earlier labels remain protected without being mistaken
    // for writes made by the current label.
    let before_draw = e.background_mask.as_ref().map(|_| next.to_vec());
    let w = t.width;
    let at = |x: usize, y: usize| stored[y * w + x];
    let mode = e.background.as_str();
    ensure!(
        matches!(
            mode,
            "clear"
                | "flat"
                | "row"
                | "column"
                | "box"
                | "modal"
                | "bilinear"
                | "harmonic"
                | "masked_rows"
                | "edges"
                | "keep"
        ),
        "unknown background mode"
    );
    let rect = match e.region {
        Some(r) => r,
        None if mode != "row" => [0, 0, w, t.height],
        None => {
            let opaque: Vec<_> = (0..t.height)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|&(x, y)| at(x, y).1 > 0)
                .collect();
            ensure!(!opaque.is_empty(), "texture {} is empty", e.texture);
            // Interior of the opaque box: keep its one-pixel rim for interpolation.
            [
                opaque.iter().map(|p| p.0).min().unwrap() + 1,
                opaque.iter().map(|p| p.1).min().unwrap() + 1,
                opaque.iter().map(|p| p.0).max().unwrap(),
                opaque.iter().map(|p| p.1).max().unwrap(),
            ]
        }
    };
    let [x0, y0, x1, y1] = rect;
    ensure!(
        x0 < x1 && y0 < y1 && x1 <= w && y1 <= t.height,
        "texture {} region",
        e.texture
    );
    let polygon = &e.region_polygon;
    if !polygon.is_empty() {
        ensure!((3..=8).contains(&polygon.len()), "polygon vertex count");
        ensure!(
            polygon
                .iter()
                .all(|&[x, y]| x >= x0 as i32 && y >= y0 as i32 && x < x1 as i32 && y < y1 as i32),
            "polygon outside region"
        );
        let mut sign = 0;
        for i in 0..polygon.len() {
            let [ax, ay] = polygon[i];
            let [bx, by] = polygon[(i + 1) % polygon.len()];
            for (j, &[x, y]) in polygon.iter().enumerate() {
                if j == i || j == (i + 1) % polygon.len() {
                    continue;
                }
                let cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax);
                ensure!(cross != 0, "degenerate polygon");
                if sign == 0 {
                    sign = cross.signum();
                }
                ensure!(cross.signum() == sign, "polygon must be convex and ordered");
            }
        }
    }
    let inside = |x: i32, y: i32| {
        if x < x0 as i32 || y < y0 as i32 || x >= x1 as i32 || y >= y1 as i32 {
            return false;
        }
        if polygon.is_empty() {
            return true;
        }
        let crosses: Vec<_> = (0..polygon.len())
            .map(|i| {
                let [ax, ay] = polygon[i];
                let [bx, by] = polygon[(i + 1) % polygon.len()];
                (bx - ax) * (y - ay) - (by - ay) * (x - ax)
            })
            .collect();
        crosses.iter().all(|&v| v >= 0) || crosses.iter().all(|&v| v <= 0)
    };
    if matches!(mode, "row" | "bilinear" | "harmonic" | "masked_rows") {
        ensure!(
            x0 > 0 && x1 < w,
            "interpolated background needs pixels on both sides"
        );
    }
    if matches!(mode, "bilinear" | "column" | "harmonic") {
        ensure!(
            y0 > 0 && y1 < t.height,
            "bilinear background needs rows above and below"
        );
    }
    // Background is transparency, or the most common opaque region colour for a
    // painted box. Text pixels within the outline thickness of it are outline;
    // deeper pixels are the fill.
    // The box colour is read from its rim: opaque pixels beside transparency or the
    // texture edge, so dense lettering cannot outvote it.
    let box_colour = most_common(
        (0..t.height)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                at(x, y).1 > 0
                    && (x == 0
                        || y == 0
                        || x + 1 == w
                        || y + 1 == t.height
                        || [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                            .iter()
                            .any(|&(nx, ny)| at(nx, ny).1 == 0))
            })
            .map(|(x, y)| at(x, y).0),
    );
    let is_bg = |x: usize, y: usize| {
        let (i, a) = at(x, y);
        a == 0 || (matches!(mode, "box" | "row") && Some(i) == box_colour)
    };
    let reach = e.outline.max(1) as i32;
    let mut edge = Vec::new();
    let mut body = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            if is_bg(x, y) {
                continue;
            }
            let near = (-reach..=reach).any(|dy| {
                (-reach..=reach).any(|dx| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    nx < 0
                        || ny < 0
                        || nx >= w as i32
                        || ny >= t.height as i32
                        || is_bg(nx as usize, ny as usize)
                })
            });
            if near {
                edge.push(at(x, y).0)
            } else {
                body.push(at(x, y).0)
            }
        }
    }
    let pick = |c: Option<[i32; 3]>, fallback: Option<usize>| -> Result<usize> {
        match c {
            Some(rgb) => Ok(t.nearest(rgb)?),
            None => fallback
                .ok_or_else(|| anyhow::anyhow!("texture {}: colour not detectable", e.texture)),
        }
    };
    let outline = match e.outline_index {
        Some(i) => i,
        None if t.palette.len() / 2 == 1 => 0,
        None => pick(e.outline_color, most_common(edge.iter().copied()))?,
    };
    let fill = match e.fill_index {
        Some(i) => i,
        None if t.palette.len() / 2 == 1 => 0,
        None => pick(
            e.fill,
            most_common(body.iter().copied().filter(|&i| i != outline))
                .or(most_common(edge.iter().copied().filter(|&i| i != outline))),
        )?,
    };
    ensure!(
        fill < t.palette.len() / 2 && outline < t.palette.len() / 2,
        "palette index outside palette"
    );
    // Single-colour palettes shade with alpha only; there fill and outline share index 0.
    let single = t.palette.len() / 2 == 1;
    ensure!(
        single || fill != outline || (e.outline == 0 && e.shadow.is_none()),
        "texture {}: fill and outline colours coincide",
        e.texture
    );
    let (font, size) = match e.font.as_str() {
        "regular" => (regular, e.size.unwrap_or(12)),
        "small" => (small, e.size.unwrap_or(8)),
        // Third face of the component (`--medium-font`): usually Galmuri9 at its
        // native 10px grid; some inputs name another face and size explicitly.
        "medium" => (
            medium.ok_or_else(|| anyhow::anyhow!("medium font label needs --medium-font"))?,
            e.size.unwrap_or(10),
        ),
        // DenkiChip: Galmuri11's 12px height in Galmuri9's 10px advance.
        "narrow" => (
            narrow.ok_or_else(|| anyhow::anyhow!("narrow font label needs --narrow-font"))?,
            e.size.unwrap_or(12),
        ),
        _ => anyhow::bail!("unknown font"),
    };
    let latin = match e.latin_font.as_deref() {
        None => None,
        Some("regular") => Some(regular),
        Some(other) => anyhow::bail!("unknown latin font {other}"),
    };
    let left = match e.align.as_deref() {
        None | Some("center") => None,
        Some("left") => Some(
            e.outline as i32
                + if e.rim_color.is_some() {
                    e.rim_width.unwrap_or(1) as i32
                } else {
                    0
                },
        ),
        Some(other) => anyhow::bail!("unknown alignment {other}"),
    };
    // With area rasterisation the shear is applied before thresholding.
    let smooth_shear = match (e.scale_x, e.shear_y) {
        (Some(_), Some([numerator, denominator])) => {
            ensure!(
                denominator > 0 && numerator.unsigned_abs() <= denominator as u32,
                "invalid vertical shear"
            );
            ensure!(
                e.rotate.is_none() && !e.bold,
                "smooth shear is horizontal and regular"
            );
            Some(f64::from(numerator) / f64::from(denominator))
        }
        _ => None,
    };
    let mut glyphs = match e.rotate.as_deref() {
        None => ink(
            font,
            &e.korean,
            size,
            rect,
            left,
            Spacing {
                line_gap: e.line_gap,
                letter_spacing: e.letter_spacing,
                scale_x: e.scale_x,
                shear: smooth_shear.unwrap_or(0.0),
                space: e.space_width,
                grid_fit: e.grid_fit,
                top: e.top,
            },
            latin,
        )?,
        Some("cw") => {
            ensure!(!e.bold, "bold widens across rotated strokes");
            let (w0, h0) = (x1 - x0, y1 - y0);
            ink(
                font,
                &e.korean,
                size,
                [0, 0, h0, w0],
                left,
                Spacing {
                    line_gap: e.line_gap,
                    letter_spacing: e.letter_spacing,
                    scale_x: e.scale_x,
                    shear: 0.0,
                    space: e.space_width,
                    grid_fit: e.grid_fit,
                    top: None,
                },
                latin,
            )?
            .into_iter()
            .map(|(vx, vy)| (x0 as i32 + w0 as i32 - 1 - vy, y0 as i32 + vx))
            .collect()
        }
        Some(other) => anyhow::bail!("unknown rotation {other}"),
    };
    ensure!(!e.bold_keep_gaps || e.bold, "bold_keep_gaps needs bold");
    if e.bold && e.bold_keep_gaps {
        let source = glyphs.clone();
        glyphs.extend(
            source
                .iter()
                .filter(|&&(x, y)| !source.contains(&(x + 1, y)) && !source.contains(&(x + 2, y)))
                .map(|&(x, y)| (x + 1, y))
                .collect::<Vec<_>>(),
        );
    } else if e.bold {
        glyphs = glyphs
            .iter()
            .flat_map(|&(x, y)| [(x, y), (x + 1, y)])
            .collect();
    }
    if e.bold_keep_gap {
        ensure!(!e.bold, "choose one bold mode");
        let wide = bold_vertical_runs(&glyphs);
        glyphs.extend(wide);
    }
    if let (Some([numerator, denominator]), None) = (e.shear_y, smooth_shear) {
        ensure!(
            denominator > 0 && numerator.unsigned_abs() <= denominator as u32,
            "invalid vertical shear"
        );
        glyphs = glyphs
            .into_iter()
            .map(|(x, y)| {
                let delta = f64::from(2 * x - (x0 + x1 - 1) as i32) * f64::from(numerator)
                    / (2.0 * f64::from(denominator));
                (x, y + delta.round() as i32)
            })
            .collect();
    }
    let mut stroke = BTreeSet::new();
    for &(x, y) in &glyphs {
        let r = e.outline as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                // Rounded for thick outlines so small Hangul counters stay open.
                if dx * dx + dy * dy <= r * r + 1 {
                    stroke.insert((x + dx, y + dy));
                }
            }
        }
        if let Some([sx, sy]) = e.shadow {
            for d in 0..=r {
                stroke.insert((x + sx + d, y + sy + d));
            }
        }
    }
    // Icon placement: (source rect, destination x) once the text is shifted.
    let lead_icon = match &e.lead_icon {
        None => None,
        Some(icon) => {
            let [ix0, iy0, ix1, iy1] = icon.rect;
            ensure!(
                ix0 < ix1 && iy0 < iy1 && ix0 >= x0 && ix1 <= x1 && iy0 >= y0 && iy1 <= y1,
                "texture {}: lead icon outside region",
                e.texture
            );
            ensure!(e.rotate.is_none(), "lead icon needs horizontal text");
            let xmin = stroke.iter().chain(&glyphs).map(|p| p.0).min().unwrap();
            let xmax = stroke.iter().chain(&glyphs).map(|p| p.0).max().unwrap();
            let icon_width = (ix1 - ix0) as i32;
            let group = icon_width + icon.gap as i32 + (xmax - xmin + 1);
            let start = x0 as i32 + ((x1 - x0) as i32 - group) / 2;
            ensure!(
                start >= x0 as i32,
                "texture {}: icon and label exceed region",
                e.texture
            );
            let dx = start + icon_width + icon.gap as i32 - xmin;
            glyphs = glyphs.iter().map(|&(x, y)| (x + dx, y)).collect();
            stroke = stroke.iter().map(|&(x, y)| (x + dx, y)).collect();
            Some((icon.rect, start as usize))
        }
    };
    if e.open_counters {
        stroke.retain(|&(x, y)| {
            let g = |dx: i32, dy: i32| glyphs.contains(&(x + dx, y + dy));
            !((g(-1, 0) && g(1, 0)) || (g(0, -1) && g(0, 1)))
        });
    }
    if e.clip_stroke {
        stroke.retain(|&(x, y)| inside(x, y));
    }
    for &(x, y) in stroke.iter().chain(&glyphs) {
        ensure!(
            inside(x, y),
            "texture {}: label {:?} leaves region at ({x},{y})",
            e.texture,
            e.korean
        );
    }
    let full = t.max_alpha();
    ensure!(next.len() == stored.len(), "label texel population differs");
    let mut reconstruct = vec![true; stored.len()];
    let masked_mode = matches!(mode, "harmonic" | "masked_rows");
    ensure!(
        mode != "masked_rows" || e.background_mask.is_some(),
        "masked_rows background needs background_mask"
    );
    ensure!(
        e.row_source.as_ref().is_none_or(|r| {
            mode == "masked_rows"
                && y0 >= r.row_offset
                && e.reference.rows.len() + r.row_offset >= y1
                && e.reference.width >= x1
        }),
        "row_source needs masked_rows and a resolved source of the label height"
    );
    if let Some(mask) = &e.background_mask {
        ensure!(
            masked_mode,
            "background mask requires harmonic or masked_rows mode"
        );
        ensure!(
            !mask.indices.is_empty() && mask.radius <= 4,
            "background mask geometry"
        );
        for &index in &mask.indices {
            t.rgb(index)?;
        }
        reconstruct.fill(false);
        let mut seeds = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                if inside(x as i32, y as i32)
                    && at(x, y).1 == full
                    && mask.indices.contains(&at(x, y).0)
                {
                    seeds += 1;
                    for yy in y.saturating_sub(mask.radius).max(y0)..=(y + mask.radius).min(y1 - 1)
                    {
                        for xx in
                            x.saturating_sub(mask.radius).max(x0)..=(x + mask.radius).min(x1 - 1)
                        {
                            reconstruct[yy * w + xx] =
                                inside(xx as i32, yy as i32) && at(xx, yy).1 == full;
                        }
                    }
                }
            }
        }
        ensure!(seeds > 0, "background mask found no source lettering");
    }
    // `masked_rows` also marks source lettering just outside the region, so
    // row anchors never land on a clipped stroke or its rim.
    let mut lettering = vec![false; stored.len()];
    if let (Some(mask), "masked_rows") = (&e.background_mask, mode) {
        let r = mask.radius;
        let h = t.height;
        for y in y0.saturating_sub(r)..(y1 + r).min(h) {
            for x in x0.saturating_sub(r)..(x1 + r).min(w) {
                if at(x, y).1 != full || !mask.indices.contains(&at(x, y).0) {
                    continue;
                }
                for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                    for xx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                        if at(xx, yy).1 == full {
                            lettering[yy * w + xx] = true;
                        }
                    }
                }
            }
        }
        for y in y0..y1 {
            for x in x0..x1 {
                reconstruct[y * w + x] = lettering[y * w + x] && inside(x as i32, y as i32);
            }
        }
    }
    // Smoothly join the unchanged boundary of an opaque curved background.
    // Work in RGB before palette quantization so iteration cannot lock to bands.
    let background_colors: Vec<_> = e
        .background_indices
        .iter()
        .map(|&index| {
            ensure!(
                masked_mode,
                "background indices require harmonic or masked_rows mode"
            );
            ensure!(
                t.format != 3 || index != 0,
                "background index is transparent"
            );
            Ok((index, t.rgb(index)?))
        })
        .collect::<Result<_>>()?;
    let background_index = |rgb: [i32; 3]| -> Result<usize> {
        if background_colors.is_empty() {
            return t.nearest(rgb);
        }
        Ok(background_colors
            .iter()
            .min_by_key(|(_, color)| {
                color
                    .iter()
                    .zip(rgb)
                    .map(|(a, b)| (a - b).pow(2))
                    .sum::<i32>()
            })
            .unwrap()
            .0)
    };
    let harmonic = if mode == "harmonic" {
        let stride = x1 - x0 + 2;
        let rows = y1 - y0 + 2;
        let mut colors = vec![[0.0f64; 3]; stride * rows];
        let mut opaque = vec![false; stride * rows];
        for y in 0..rows {
            for x in 0..stride {
                let p = at(x0 - 1 + x, y0 - 1 + y);
                ensure!(
                    e.background_mask.is_some() || p.1 == full,
                    "harmonic background must be opaque"
                );
                opaque[y * stride + x] = p.1 == full;
                let rgb = t.rgb(p.0)?;
                colors[y * stride + x] = if background_colors.is_empty() {
                    rgb
                } else {
                    t.rgb(background_index(rgb)?)?
                }
                .map(f64::from);
            }
        }
        let mut converged = false;
        for _ in 0..4096 {
            let mut largest = 0.0f64;
            for y in 1..rows - 1 {
                for x in 1..stride - 1 {
                    if !reconstruct[(y0 + y - 1) * w + x0 + x - 1] {
                        continue;
                    }
                    let p = y * stride + x;
                    let neighbors = [p - 1, p + 1, p - stride, p + stride];
                    let count = neighbors.iter().filter(|&&n| opaque[n]).count();
                    ensure!(count > 0, "isolated reconstruction pixel");
                    for k in 0..3 {
                        let value = neighbors
                            .iter()
                            .filter(|&&n| opaque[n])
                            .map(|&n| colors[n][k])
                            .sum::<f64>()
                            / count as f64;
                        largest = largest.max((value - colors[p][k]).abs());
                        colors[p][k] = value;
                    }
                }
            }
            if largest < 0.00001 {
                converged = true;
                break;
            }
        }
        ensure!(converged, "harmonic background did not converge");
        Some((stride, colors))
    } else {
        None
    };
    // Glossy pills keep one colour per interior row, while their highlight and
    // shade bands change from row to row. Rebuild each masked run from the
    // nearest pixels beside it in the same row that are not source lettering,
    // so no band leaks vertically. When only one side had to step past
    // lettering outside the region, that side reaches the pill's end shading
    // and the other side alone colours the run.
    let mut row_runs = BTreeMap::new();
    // Calibration votes per (row, source colour), then per source colour:
    // the same source colour can stand for different indices on other rows.
    let mut calibration: BTreeMap<(Option<usize>, [i32; 3]), usize> = BTreeMap::new();
    if let Some(r) = e.row_source.as_ref().filter(|r| r.calibrate) {
        let mut votes: BTreeMap<_, BTreeMap<usize, usize>> = BTreeMap::new();
        for y in r.row_offset..t.height {
            let sy = y - r.row_offset;
            for x in 0..w.min(e.reference.width) {
                let p = y * w + x;
                let source = e
                    .reference
                    .opaque
                    .get(sy * e.reference.width + x)
                    .copied()
                    .flatten();
                if let (Some(rgb), (index, alpha)) = (source, at(x, y)) {
                    let allowed = background_colors.is_empty()
                        || background_colors.iter().any(|&(i, _)| i == index);
                    if alpha == full && allowed && !lettering[p] && !reconstruct[p] {
                        for key in [(Some(y), rgb), (None, rgb)] {
                            *votes.entry(key).or_default().entry(index).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        for (key, counts) in votes {
            let (index, _) = counts
                .into_iter()
                .max_by_key(|&(index, n)| (n, std::cmp::Reverse(index)))
                .expect("vote");
            calibration.insert(key, index);
        }
    }
    let source_index = |y: usize, rgb: [i32; 3]| -> Result<usize> {
        match calibration
            .get(&(Some(y), rgb))
            .or_else(|| calibration.get(&(None, rgb)))
        {
            Some(&index) => Ok(index),
            None => background_index(rgb),
        }
    };
    if mode == "masked_rows" {
        // Nearest non-lettering texel from `x` in `step` direction:
        // (position, palette index) and whether any lettering was skipped.
        let walk = |mut x: i32, y: usize, step: i32| -> (Option<(i32, usize)>, bool) {
            let first = x;
            let opaque = |x: i32| x >= 0 && (x as usize) < w && at(x as usize, y).1 == full;
            while opaque(x) && lettering[y * w + x as usize] {
                x += step;
            }
            (opaque(x).then(|| (x, at(x as usize, y).0)), x != first)
        };
        for y in y0..y1 {
            let mut x = x0;
            while x < x1 {
                if !reconstruct[y * w + x] {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < x1 && reconstruct[y * w + x] {
                    x += 1;
                }
                let (mut left, left_walked) = walk(start as i32 - 1, y, -1);
                let (mut right, right_walked) = walk(x as i32, y, 1);
                if left_walked && !right_walked && right.is_some() {
                    left = None;
                }
                if right_walked && !left_walked && left.is_some() {
                    right = None;
                }
                for xx in start..x {
                    let reference = e.row_source.as_ref().and_then(|r| {
                        let sy = y - r.row_offset;
                        let own = e.reference.texels[sy * e.reference.width + xx];
                        let row = (r.span[0]..r.span[1])
                            .contains(&xx)
                            .then(|| e.reference.rows[sy])
                            .flatten();
                        own.or(row)
                    });
                    if let Some(rgb) = reference {
                        row_runs.insert(y * w + xx, source_index(y, rgb)?);
                        continue;
                    }
                    let index = match (left, right) {
                        (Some((lx, l)), Some((rx, r))) if l != r => {
                            let (a, b) = (t.rgb(l)?, t.rgb(r)?);
                            let (u, n) = (xx as i32 - lx, rx - lx);
                            background_index(std::array::from_fn(|k| {
                                (a[k] * (n - u) + b[k] * u + n / 2) / n
                            }))?
                        }
                        (Some((_, i)), _) | (_, Some((_, i))) => i,
                        (None, None) => anyhow::bail!(
                            "texture {}: masked row {y} run {start}..{x} has no unchanged neighbour",
                            e.texture
                        ),
                    };
                    row_runs.insert(y * w + xx, index);
                }
            }
        }
    }
    for y in y0..y1 {
        for x in x0..x1 {
            if !inside(x as i32, y as i32) {
                continue;
            }
            let p = y * w + x;
            if mode == "keep" {
                // The caller already prepared this background in `next`.
                continue;
            }
            if e.background_mask.is_some() && !reconstruct[p] {
                continue;
            }
            next[p] = if mode == "masked_rows" {
                (row_runs[&p], at(x, y).1)
            } else if let Some((stride, colors)) = &harmonic {
                let rgb = colors[(y - y0 + 1) * stride + x - x0 + 1].map(|v| v.round() as i32);
                (background_index(rgb)?, at(x, y).1)
            } else if mode == "flat" {
                let i = e
                    .background_index
                    .ok_or_else(|| anyhow::anyhow!("flat background needs background_index"))?;
                // Index-zero formats: a painted colour is opaque wherever it lands.
                if t.format == 3 {
                    (i, usize::from(i != 0))
                } else {
                    (i, at(x, y).1)
                }
            } else if mode == "edges" {
                // Row colour from the four columns at each end of the region,
                // which lie outside the lettering of wide pill buttons.
                let sides = (x0..(x0 + 4).min(x1)).chain(x1.saturating_sub(4).max(x0)..x1);
                match most_common(sides.filter(|&x| at(x, y).1 > 0).map(|x| at(x, y).0)) {
                    Some(i) if at(x, y).1 > 0 => (i, at(x, y).1),
                    _ => (0, 0),
                }
            } else if mode == "modal" {
                // Each row keeps its dominant non-lettering colour.
                match most_common(
                    (x0..x1)
                        .filter(|&x| at(x, y).1 > 0 && at(x, y).0 != fill && at(x, y).0 != outline)
                        .map(|x| at(x, y).0),
                ) {
                    Some(i) if at(x, y).1 > 0 => (i, at(x, y).1),
                    _ => (0, 0),
                }
            } else if mode == "bilinear" {
                // Join all four unchanged edges, subtracting the double-counted corners.
                let c = |x: usize, y: usize| t.rgb(at(x, y).0);
                let (top, bottom, left, right) =
                    (c(x, y0 - 1)?, c(x, y1)?, c(x0 - 1, y)?, c(x1, y)?);
                let corners = [
                    c(x0 - 1, y0 - 1)?,
                    c(x1, y0 - 1)?,
                    c(x0 - 1, y1)?,
                    c(x1, y1)?,
                ];
                let (dx, dy) = ((x1 - x0 + 1) as i32, (y1 - y0 + 1) as i32);
                let (u, v) = ((x - x0 + 1) as i32, (y - y0 + 1) as i32);
                let rgb = std::array::from_fn(|k| {
                    let vertical = (top[k] * (dy - v) + bottom[k] * v) * dx;
                    let horizontal = (left[k] * (dx - u) + right[k] * u) * dy;
                    let corner = corners[0][k] * (dx - u) * (dy - v)
                        + corners[1][k] * u * (dy - v)
                        + corners[2][k] * (dx - u) * v
                        + corners[3][k] * u * v;
                    ((vertical + horizontal - corner + dx * dy / 2) / (dx * dy)).clamp(0, 31)
                });
                (t.nearest(rgb)?, at(x, y).1)
            } else if mode == "column" {
                // Vertical gradients (glossy pills): interpolate each column.
                let (a, b) = (t.rgb(at(x, y0 - 1).0)?, t.rgb(at(x, y1).0)?);
                let (u, n) = ((y - y0 + 1) as i32, (y1 - y0 + 1) as i32);
                let c = std::array::from_fn(|k| (a[k] * (n - u) + b[k] * u + n / 2) / n);
                (t.nearest(c)?, at(x, y).1)
            } else if mode == "row" {
                let (l, r) = (t.rgb(at(x0 - 1, y).0)?, t.rgb(at(x1, y).0)?);
                let (u, n) = ((x - x0 + 1) as i32, (x1 - x0 + 1) as i32);
                let c = std::array::from_fn(|k| (l[k] * (n - u) + r[k] * u + n / 2) / n);
                (t.nearest(c)?, at(x, y).1)
            } else if mode == "box" && at(x, y).1 > 0 {
                (box_colour.unwrap(), at(x, y).1)
            } else {
                (0, 0)
            };
        }
    }
    if let Some(([ix0, iy0, ix1, iy1], start)) = lead_icon {
        for y in iy0..iy1 {
            for x in ix0..ix1 {
                let (dx, dy) = ((start + x - ix0) as i32, y as i32);
                ensure!(
                    inside(dx, dy) && !stroke.contains(&(dx, dy)) && !glyphs.contains(&(dx, dy)),
                    "texture {}: moved icon overlaps label",
                    e.texture
                );
                next[y * w + start + x - ix0] = at(x, y);
            }
        }
    }
    let mut rim = BTreeSet::new();
    if let Some(c) = e.rim_color {
        let width = e.rim_width.unwrap_or(1);
        ensure!((1..=3).contains(&width), "rim width outside 1..3");
        let mut edge: BTreeSet<(i32, i32)> = stroke.clone();
        for _ in 0..width {
            let mut grown = BTreeSet::new();
            for &(x, y) in &edge {
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let p = (x + dx, y + dy);
                    if !stroke.contains(&p) && !glyphs.contains(&p) && !rim.contains(&p) {
                        grown.insert(p);
                    }
                }
            }
            rim.extend(grown.iter().copied());
            edge = grown;
        }
        let index = t.nearest(c)?;
        for &(x, y) in &rim {
            ensure!(
                inside(x, y),
                "texture {}: rim leaves region at ({x},{y})",
                e.texture
            );
            next[y as usize * w + x as usize] = (index, full);
        }
    }
    let stroke_alpha = e.stroke_alpha.unwrap_or(full);
    ensure!(
        stroke_alpha > 0 && stroke_alpha <= full,
        "stroke alpha outside format"
    );
    for &(x, y) in &stroke {
        next[y as usize * w + x as usize] = (outline, stroke_alpha);
    }
    if let Some([row, index]) = e.lower_fill {
        ensure!(
            row >= y0 && row < y1 && index < t.palette.len() / 2 && (t.format != 3 || index != 0),
            "lower fill outside region or palette"
        );
    }
    ensure!(
        e.fill_rows.windows(2).all(|w| w[0][0] < w[1][0])
            && e.fill_rows.iter().all(|&[row, index]| {
                row >= y0
                    && row < y1
                    && index < t.palette.len() / 2
                    && (t.format != 3 || index != 0)
            }),
        "fill rows must ascend inside the region and palette"
    );
    if let Some([dot, step]) = e.highlight {
        ensure!(
            step > 0 && dot < t.palette.len() / 2 && (t.format != 3 || dot != 0),
            "highlight index or step"
        );
    }
    ensure!(
        !e.highlight_all_edges || e.highlight.is_some(),
        "highlight_all_edges needs highlight"
    );
    let row_fill = |y: i32| {
        e.fill_rows
            .iter()
            .rev()
            .find(|&&[row, _]| y as usize >= row)
            .map_or(fill, |&[_, index]| index)
    };
    for &(x, y) in &glyphs {
        let index = e
            .lower_fill
            .filter(|&[row, _]| y as usize >= row)
            .map_or(row_fill(y), |[_, index]| index);
        let index = match e.highlight {
            Some([dot, step]) if e.highlight_all_edges => {
                let edge = [(0, -1), (0, 1), (-1, 0), (1, 0)]
                    .iter()
                    .any(|&(dx, dy)| !glyphs.contains(&(x + dx, y + dy)));
                if edge && (x + 2 * y).rem_euclid(step as i32) == 0 {
                    dot
                } else {
                    index
                }
            }
            Some([dot, step])
                if !glyphs.contains(&(x, y - 1)) && (x + y).rem_euclid(step as i32) == 0 =>
            {
                dot
            }
            _ => index,
        };
        next[y as usize * w + x as usize] = (index, full);
    }
    if mode == "box" {
        // Text stays on the painted box; transparent corners remain transparent.
        for &(x, y) in stroke.iter().chain(&glyphs) {
            ensure!(
                at(x as usize, y as usize).1 > 0,
                "texture {}: label leaves the box",
                e.texture
            );
        }
    }
    if t.format == 3 {
        ensure!(
            outline != 0 && fill != 0,
            "texture {}: 4bpp ink uses transparent index",
            e.texture
        );
    }
    if !polygon.is_empty() {
        for (p, (before, after)) in stored.iter().zip(next.iter()).enumerate() {
            ensure!(
                inside((p % w) as i32, (p / w) as i32) || before == after,
                "protected polygon pixel changed"
            );
        }
    }
    let background_mask_report = if let Some(mask) = &e.background_mask {
        let mut allowed = reconstruct.clone();
        for &(x, y) in stroke.iter().chain(&glyphs).chain(&rim) {
            allowed[y as usize * w + x as usize] = true;
        }
        let mut protected = 0;
        for (p, (&before, &after)) in stored.iter().zip(next.iter()).enumerate() {
            ensure!(
                before.1 == after.1,
                "texture {} {:?}: masked reconstruction changed silhouette at ({},{})",
                e.texture,
                e.korean,
                p % w,
                p / w
            );
            if !allowed[p] {
                ensure!(
                    before_draw.as_ref().unwrap()[p] == after,
                    "masked reconstruction changed protected texel"
                );
                protected += 1;
            }
        }
        Some(
            json!({"indices":mask.indices,"radius":mask.radius,"reconstructed_pixels":reconstruct.iter().filter(|&&v|v).count(),"protected_pixels":protected,"protected_changes":0,"silhouette_changes":0}),
        )
    } else {
        None
    };
    Ok((
        rect,
        json!({"texture":e.texture,"member":t.member,"format":t.format,"japanese":e.japanese,"korean":e.korean,"region":rect,"region_polygon":e.region_polygon,"shear_y":e.shear_y,"lower_fill":e.lower_fill,"fill_rows":e.fill_rows,"background":e.background,"background_mask":background_mask_report,"background_indices":e.background_indices,"font":e.font,"size":size,"outline_index":outline,"fill_index":fill,"lead_icon":lead_icon.map(|(r, start)| json!({"source":r,"x":start}))}),
    ))
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    small_font: &Path,
    medium_font: Option<&Path>,
    narrow_font: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let mut tr: Input = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft",
        "unexpected input state"
    );
    let source = rom.data(rom.file(&tr.archive)?);
    let table = rom.data(rom.file(&tr.table)?);
    ensure!(
        sha(source) == tr.archive_sha256 && sha(table) == tr.table_sha256,
        "source identity changed"
    );
    ensure!(table.len() % 12 == 0, "texture list size");
    let narc = Narc::parse(source)?;
    for e in &mut tr.entries {
        if let Some(r) = &e.row_source {
            e.reference = reference(rom, r)?;
        }
    }
    let load = |p: &Path| -> Result<(fontdue::Font, String)> {
        let b = fs::read(p)?;
        let h = sha(&b);
        Ok((
            fontdue::Font::from_bytes(b, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!(e))?,
            h,
        ))
    };
    let (regular, regular_sha) = load(font_path)?;
    let (small, small_sha) = load(small_font)?;
    let medium = medium_font.map(load).transpose()?;
    let narrow = narrow_font.map(load).transpose()?;
    let mut changes = BTreeMap::new();
    let mut reports = Vec::new();
    let mut members = Vec::new();
    let mut groups: BTreeMap<usize, Vec<&Label>> = BTreeMap::new();
    for e in &tr.entries {
        groups.entry(e.texture).or_default().push(e);
    }
    for (id, labels) in groups {
        let mut t = texture(&narc, table, id)?;
        let stored = t.texels.clone();
        let stored = &stored;
        let enc = t.encode(stored)?;
        ensure!(
            labels.iter().all(|e| sha(&enc) == e.source_sha256),
            "texture {id} source pixels changed"
        );
        let w = t.width;
        let full = t.max_alpha();
        let mut copied: Vec<[usize; 4]> = Vec::new();
        for c in labels.iter().flat_map(|e| &e.copies) {
            let [x0, y0, x1, y1] = c.rect;
            let [fx, fy] = c.from;
            ensure!(
                x0 < x1
                    && y0 < y1
                    && x1 <= w
                    && y1 <= t.height
                    && fx + x1 - x0 <= w
                    && fy + y1 - y0 <= t.height,
                "texture {id}: copy outside texture"
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    t.texels[y * w + x] = stored[(fy + y - y0) * w + fx + x - x0];
                }
            }
            copied.push(c.rect);
        }
        let mut next = t.texels.clone();
        let mut rects: Vec<[usize; 4]> = Vec::new();
        for e in labels {
            let (rect, report) = draw(
                &t,
                &mut next,
                e,
                &regular,
                &small,
                medium.as_ref().map(|m| &m.0),
                narrow.as_ref().map(|m| &m.0),
            )?;
            ensure!(
                rects.iter().all(|r| rect[2] <= r[0]
                    || r[2] <= rect[0]
                    || rect[3] <= r[1]
                    || r[3] <= rect[1]),
                "texture {id}: overlapping label regions"
            );
            rects.push(rect);
            reports.push(report);
        }
        let inside = |x: usize, y: usize| {
            rects
                .iter()
                .chain(&copied)
                .any(|r| x >= r[0] && y >= r[1] && x < r[2] && y < r[3])
        };
        for (p, (a, b)) in stored.iter().zip(&next).enumerate() {
            ensure!(a == b || inside(p % w, p / w), "protected texel changed");
        }
        let bytes = t.encode(&next)?;
        let capacity = narc.members[t.member].len();
        let mut packed = crate::compress::pack(&bytes)?;
        if packed.len() > capacity && bytes.len() <= 4096 {
            packed = crate::compress::pack_compact(&bytes)?;
        }
        ensure!(unpack_halfword(&packed)? == bytes, "halfword round trip");
        ensure!(
            packed.len() <= capacity,
            "texture {} member {} capacity {} > {}",
            id,
            t.member,
            packed.len(),
            capacity
        );
        ensure!(
            changes.insert(t.member, packed.clone()).is_none(),
            "member {} edited twice",
            t.member
        );
        let preview = |texels: &[(usize, usize)]| -> Result<Vec<u8>> {
            let mut rgba = Vec::new();
            for &(i, a) in &texels[..w * t.height] {
                let c = t.rgb(i)?;
                rgba.extend(c.map(|v| (v * 255 / 31) as u8));
                rgba.push((a * 255 / full) as u8);
            }
            Ok(rgba)
        };
        fs::create_dir_all(out)?;
        write_png(
            &out.join(format!("{id:03}-before.png")),
            w,
            t.height,
            &preview(stored)?,
        )?;
        write_png(
            &out.join(format!("{id:03}-after.png")),
            w,
            t.height,
            &preview(&next)?,
        )?;
        members.push(json!({"texture":id,"member":t.member,"stored_size":packed.len(),"capacity":capacity,"decoded_sha256":sha(&bytes)}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::write(out.join("archive.narc"), &rebuilt)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":tr.archive,"expected_sha256":sha(source),"input":"archive.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"font_sha256":regular_sha,"small_font_sha256":small_sha,"medium_font_sha256":medium.as_ref().map(|m| m.1.clone()),"narrow_font_sha256":narrow.as_ref().map(|m| m.1.clone()),"entries":reports,"members":members,"protected":"palette, texels outside each region, stored tail rows and every other member","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinearInput {
    archive: String,
    archive_sha256: String,
    state: String,
    entries: Vec<LinearEntry>,
}

/// A row-major (untiled) 4bpp image with its own palette member.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinearEntry {
    palette: usize,
    width: usize,
    label: Label,
    /// Further labels on the same image, in non-overlapping regions.
    #[serde(default)]
    more: Vec<Label>,
    /// 8x8 tiles laid out row-major across `width` (OBJ sheets drawn by code
    /// with 1D mapping) instead of linear rows.
    #[serde(default)]
    tiled: bool,
    /// Bits per texel: 4 (default) or 2 (four-colour texture format 2).
    #[serde(default = "four")]
    bits: usize,
}

fn four() -> usize {
    4
}

/// Redraw labels stored as linear 4bpp images (e.g. staff-roll lyric subtitles).
pub fn prepare_linear(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    small_font: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: LinearInput = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft",
        "unexpected input state"
    );
    let source = rom.data(rom.file(&tr.archive)?);
    ensure!(sha(source) == tr.archive_sha256, "linear archive changed");
    let narc = Narc::parse(source)?;
    let load = |p: &Path| -> Result<fontdue::Font> {
        fontdue::Font::from_bytes(fs::read(p)?, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))
    };
    let (regular, small) = (load(font_path)?, load(small_font)?);
    let mut changes = BTreeMap::new();
    let mut reports = Vec::new();
    fs::create_dir_all(out)?;
    for e in &tr.entries {
        let member = e.label.texture;
        let raw = unpack_halfword(narc.members[member])?;
        ensure!(
            sha(&raw) == e.label.source_sha256,
            "linear member {member} changed"
        );
        ensure!(
            e.width > 0 && (raw.len() * 2) % e.width == 0,
            "linear geometry"
        );
        ensure!(
            e.bits == 4 || (e.bits == 2 && !e.tiled),
            "linear depth must be 4, or 2 for untiled textures"
        );
        let per_byte = 8 / e.bits;
        ensure!(
            e.width > 0 && (raw.len() * per_byte) % e.width == 0,
            "linear geometry"
        );
        let indices: Vec<u8> = if e.bits == 2 {
            raw.iter()
                .flat_map(|v| [v & 3, v >> 2 & 3, v >> 4 & 3, v >> 6])
                .collect()
        } else if e.tiled {
            ensure!(
                e.width % 8 == 0 && raw.len() % (e.width * 4) == 0,
                "tiled geometry"
            );
            crate::titles::untile(&raw, e.width, raw.len() * 2 / e.width, 4)?
        } else {
            raw.iter().flat_map(|v| [v & 15, v >> 4]).collect()
        };
        let texels: Vec<(usize, usize)> = indices
            .into_iter()
            .map(|i| (usize::from(i), usize::from(i != 0)))
            .collect();
        let mut palette = unpack(narc.members[e.palette])?;
        // A shared palette may be longer than the texture's colour count.
        palette.truncate(2 << e.bits);
        let t = Texture {
            member,
            palette,
            format: 3,
            width: e.width,
            height: texels.len() / e.width,
            texels,
        };
        let mut next = t.texels.clone();
        let mut rects: Vec<[usize; 4]> = Vec::new();
        let mut report = Vec::new();
        for label in std::iter::once(&e.label).chain(&e.more) {
            ensure!(label.texture == member, "linear label identity");
            let (rect, r) = draw(&t, &mut next, label, &regular, &small, None, None)?;
            // A `keep` label paints over earlier ones without clearing, so it
            // may share their rows (stacked lettering such as シンクロ/れんさ).
            ensure!(
                label.background == "keep"
                    || rects.iter().all(|q| rect[2] <= q[0]
                        || q[2] <= rect[0]
                        || rect[3] <= q[1]
                        || q[3] <= rect[1]),
                "linear member {member}: overlapping label regions"
            );
            rects.push(rect);
            report.push(r);
        }
        for (p, (a, b)) in t.texels.iter().zip(&next).enumerate() {
            let (x, y) = (p % t.width, p / t.width);
            ensure!(
                a == b
                    || rects
                        .iter()
                        .any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]),
                "protected linear texel changed"
            );
        }
        let bytes = if e.bits == 2 {
            ensure!(
                next.iter().all(|&(i, a)| (i == 0) == (a == 0) && i < 4),
                "2bpp texels are 0..4 with index-zero transparency"
            );
            next.chunks_exact(4)
                .map(|q| (q[0].0 | q[1].0 << 2 | q[2].0 << 4 | q[3].0 << 6) as u8)
                .collect()
        } else if e.tiled {
            let px: Vec<u8> = next.iter().map(|&(i, _)| i as u8).collect();
            ensure!(
                next.iter().all(|&(i, a)| (i == 0) == (a == 0)),
                "4bpp transparency is index zero"
            );
            crate::titles::tile(&px, t.width, t.height, 4)?
        } else {
            t.encode(&next)?
        };
        let capacity = narc.members[member].len();
        // Members stored raw stay raw.
        let mut packed = if unpack(narc.members[member])? == narc.members[member] {
            ensure!(
                bytes.len() == capacity,
                "raw linear member {member} size changed"
            );
            bytes.clone()
        } else {
            crate::compress::pack(&bytes)?
        };
        if packed.len() > capacity && bytes.len() <= 4096 {
            packed = crate::compress::pack_compact(&bytes)?;
        }
        ensure!(
            unpack_halfword(&packed)? == bytes,
            "linear halfword round trip"
        );
        ensure!(
            packed.len() <= capacity,
            "linear member {member} capacity {} > {capacity}",
            packed.len()
        );
        ensure!(
            changes.insert(member, packed).is_none(),
            "member {member} written twice"
        );
        let preview = |tx: &[(usize, usize)]| -> Result<Vec<u8>> {
            let mut rgba = Vec::new();
            for &(i, a) in tx {
                rgba.extend(t.rgb(i)?.map(|c| (c * 255 / 31) as u8));
                rgba.push(if a > 0 { 255 } else { 0 });
            }
            Ok(rgba)
        };
        write_png(
            &out.join(format!("{member:03}-before.png")),
            t.width,
            t.height,
            &preview(&t.texels)?,
        )?;
        write_png(
            &out.join(format!("{member:03}-after.png")),
            t.width,
            t.height,
            &preview(&next)?,
        )?;
        reports.push(report);
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::write(out.join("archive.narc"), &rebuilt)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":tr.archive,"expected_sha256":sha(source),"input":"archive.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"entries":reports,"layout":"linear 4bpp rows","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;

/// Review the same texture descriptors consumed by prepare-labels.
pub(crate) fn review_groups(rom: &Rom, path: &Path, prefix: &str) -> Result<Vec<Value>> {
    let input: Input = serde_json::from_slice(&fs::read(path)?)?;
    let bytes = rom.data(rom.file(&input.archive)?);
    ensure!(
        sha(bytes) == input.archive_sha256,
        "review label archive identity"
    );
    let n = Narc::parse(bytes)?;
    let table = rom.data(rom.file(&input.table)?);
    ensure!(
        sha(table) == input.table_sha256,
        "review label table identity"
    );
    let mut grouped = BTreeMap::<usize, Vec<&Label>>::new();
    for label in &input.entries {
        grouped.entry(label.texture).or_default().push(label);
    }
    grouped.into_iter().map(|(id, labels)| {
        let t = texture(&n, table, id)?;
        let palette = u16le(table, id*12+2)?;
        let format = match t.format { 1 => "a3i5", 3 => "i4", 6 => "a5i3", _ => unreachable!() };
        Ok(json!({"id":format!("{prefix}-{id}"),"archive":input.archive,
            "japanese":labels.iter().map(|l| l.japanese.as_str()).collect::<Vec<_>>().join(" / "),
            "korean_draft":labels.iter().map(|l| l.korean.as_str()).collect::<Vec<_>>().join(" / "),
            "brief":format!("prepare-labels source descriptors: {}",path.display()),
            "surfaces":[{"member":t.member,"palette":palette,"width":t.width,"height":t.height,"format":format,
                "table":input.table,"table_entry":id,"note":"Exact texture-list dimensions used by product label preparation."}]}))
    }).collect()
}
