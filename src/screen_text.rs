//! Text drawn on 256x192 BG screens (4bpp banked or 8bpp), limited to declared parts.
use crate::{assets::json_file, battle_ui, buttons, format::*, graphics::write_png, screens};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    archive: String,
    archive_sha256: String,
    state: String,
    screens: Vec<ScreenInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenInput {
    map: usize,
    tiles: usize,
    palette: usize,
    bpp: usize,
    tiles_sha256: String,
    #[serde(default)]
    background_artwork: Option<BackgroundArtwork>,
    #[serde(default)]
    source_copies: Vec<SourceCopy>,
    /// Require the authored colors in text parts to exist in their tile banks.
    #[serde(default)]
    exact_part_colors: bool,
    /// Require exact glyph/outline colours while allowing background quantization.
    #[serde(default)]
    exact_text_colors: bool,
    parts: Vec<Part>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SourceCopy {
    source: [usize; 4],
    target: [usize; 2],
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct BackgroundArtwork {
    image: String,
    image_sha256: String,
    sampling: String,
    #[serde(default)]
    bank_change_cost: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    colors: Vec<[i32; 3]>,
    /// Adopt these areas from the complete generated background, protecting the rest.
    /// An empty list retains the existing whole-screen behavior.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    regions: Vec<[usize; 4]>,
    /// Copy generated background only over these original lettering colours.
    #[serde(default)]
    source_mask: Option<BackgroundMask>,
    /// Expand the source mask to palette-cell boundaries within the declared regions.
    #[serde(default)]
    mask_tile_alignment: bool,
    /// Adopt the artwork only where it differs from the source: the old and
    /// the new lettering of a whole-screen edit, not a rectangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    difference_mask: Option<DifferenceMask>,
    /// After colour snapping, replace each adopted pixel whose colour occurs
    /// only once in its 3x3 neighbourhood by a colour held by at least five of
    /// those nine pixels (generation noise; one-pixel lines keep three).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    despeckle: bool,
    /// Rounds of re-choosing each adopted pixel's colour within the bank its
    /// tile got, trading closeness to the artwork against agreement with the
    /// neighbours the artwork shows alike (a bank-poor tile then continues a
    /// stripe instead of showing an off-colour speck or a shade step).
    #[serde(default, skip_serializing_if = "is_zero")]
    smooth_quantization: usize,
}

fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// Squared RGB5 distance below which two artwork pixels count as one area.
const SMOOTH_ALIKE: i32 = 12;

/// One round of iterated conditional modes over `free` pixels: each takes the
/// colour of its tile's bank (`pixels`, bank = index / 16) minimising twice
/// the squared distance to its artwork colour plus the squared distances to
/// the current colours of those 8-neighbours whose artwork colour is alike.
fn smooth_quantized(
    artwork: &[[i32; 3]],
    pixels: &[u8],
    colors: &[[i32; 3]],
    free: impl Fn(usize) -> bool,
    rounds: usize,
) -> Vec<[i32; 3]> {
    let d2 = |a: [i32; 3], b: [i32; 3]| (0..3).map(|k| (a[k] - b[k]).pow(2)).sum::<i32>();
    let mut current: Vec<[i32; 3]> = pixels.iter().map(|&v| colors[usize::from(v)]).collect();
    for _ in 0..rounds {
        let before = current.clone();
        for p in 0..256 * 192 {
            if !free(p) {
                continue;
            }
            let (x, y) = (p % 256, p / 256);
            let alike: Vec<usize> = (y.saturating_sub(1)..=(y + 1).min(191))
                .flat_map(|ny| {
                    (x.saturating_sub(1)..=(x + 1).min(255)).map(move |nx| ny * 256 + nx)
                })
                .filter(|&q| q != p && d2(artwork[q], artwork[p]) <= SMOOTH_ALIKE)
                .collect();
            let bank = usize::from(pixels[p]) / 16;
            current[p] = (1..16)
                .map(|i| colors[bank * 16 + i])
                .min_by_key(|&c| {
                    2 * d2(c, artwork[p]) + alike.iter().map(|&q| d2(c, before[q])).sum::<i32>()
                })
                .unwrap();
        }
    }
    current
}

/// Pixels inside the regions whose reduced artwork colour differs from the
/// source by at least `threshold` (sum of RGB5 channel differences), kept in
/// 8-connected components of at least `min_component` pixels, with enclosed
/// pixels filled and the result grown by a round `radius`.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct DifferenceMask {
    threshold: i32,
    min_component: usize,
    radius: usize,
}

fn difference_mask(
    source: &[[i32; 3]],
    artwork: &[[i32; 3]],
    regions: &[[usize; 4]],
    mask: &DifferenceMask,
) -> Result<Vec<bool>> {
    const W: usize = 256;
    const H: usize = 192;
    ensure!(
        !regions.is_empty() && mask.threshold > 0 && mask.radius <= 8,
        "difference mask needs regions and valid geometry"
    );
    let mut region = vec![false; W * H];
    for &[x0, y0, x1, y1] in regions {
        for y in y0..y1 {
            for x in x0..x1 {
                region[y * W + x] = true;
            }
        }
    }
    let differs: Vec<bool> = (0..W * H)
        .map(|p| {
            region[p]
                && (0..3)
                    .map(|k| (source[p][k] - artwork[p][k]).abs())
                    .sum::<i32>()
                    >= mask.threshold
        })
        .collect();
    let neighbours = |p: usize| {
        let (x, y) = ((p % W) as i64, (p / W) as i64);
        (-1..=1i64)
            .flat_map(move |dy| (-1..=1i64).map(move |dx| (x + dx, y + dy)))
            .filter(|&(nx, ny)| (0..W as i64).contains(&nx) && (0..H as i64).contains(&ny))
            .map(|(nx, ny)| ny as usize * W + nx as usize)
    };
    let mut keep = vec![false; W * H];
    let mut seen = vec![false; W * H];
    for start in 0..W * H {
        if !differs[start] || seen[start] {
            continue;
        }
        let mut component = vec![start];
        seen[start] = true;
        let mut next = 0;
        while next < component.len() {
            for q in neighbours(component[next]) {
                if differs[q] && !seen[q] {
                    seen[q] = true;
                    component.push(q);
                }
            }
            next += 1;
        }
        if component.len() >= mask.min_component {
            for p in component {
                keep[p] = true;
            }
        }
    }
    ensure!(
        keep.iter().any(|&v| v),
        "difference mask found no changed lettering"
    );
    // Enclosed pixels: region pixels not 4-connected to the region border
    // without crossing kept pixels.
    let mut outside = vec![false; W * H];
    let mut stack: Vec<usize> = (0..W * H)
        .filter(|&p| {
            region[p] && {
                let (x, y) = (p % W, p / W);
                x == 0
                    || y == 0
                    || x == W - 1
                    || y == H - 1
                    || !region[p - 1]
                    || !region[p + 1]
                    || !region[p - W]
                    || !region[p + W]
            }
        })
        .collect();
    while let Some(p) = stack.pop() {
        if outside[p] || keep[p] || !region[p] {
            continue;
        }
        outside[p] = true;
        let (x, y) = (p % W, p / W);
        if x > 0 {
            stack.push(p - 1);
        }
        if x + 1 < W {
            stack.push(p + 1);
        }
        if y > 0 {
            stack.push(p - W);
        }
        if y + 1 < H {
            stack.push(p + W);
        }
    }
    let filled: Vec<bool> = (0..W * H).map(|p| region[p] && !outside[p]).collect();
    let r = mask.radius as i64;
    let mut out = vec![false; W * H];
    for p in (0..W * H).filter(|&p| filled[p]) {
        let (x, y) = ((p % W) as i64, (p / W) as i64);
        for dy in -r..=r {
            for dx in -r..=r {
                let (nx, ny) = (x + dx, y + dy);
                if dx * dx + dy * dy <= r * r + r
                    && (0..W as i64).contains(&nx)
                    && (0..H as i64).contains(&ny)
                    && region[ny as usize * W + nx as usize]
                {
                    out[ny as usize * W + nx as usize] = true;
                }
            }
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    region: [usize; 4],
    japanese: String,
    /// Lines of `(text, [r, g, b])` segments, colours on the 0..31 scale.
    korean: Vec<Vec<(String, [i32; 3])>>,
    /// `flat`, `row`, `bilinear`, masked `harmonic`, or `keep`.
    background: String,
    /// Reconstruct only these source colors and their surrounding rim.
    #[serde(default)]
    background_mask: Option<BackgroundMask>,
    #[serde(default)]
    flat_color: Option<[i32; 3]>,
    #[serde(default = "regular")]
    font: String,
    #[serde(default)]
    size: Option<usize>,
    #[serde(default)]
    line_height: Option<usize>,
    /// Optional absolute font block top, within the editable region.
    #[serde(default)]
    text_top: Option<usize>,
    #[serde(default)]
    left: bool,
    /// End each line one pixel inside the region's right edge.
    #[serde(default)]
    right: bool,
    #[serde(default)]
    outline_color: Option<[i32; 3]>,
    /// Drop shadow drawn under the glyphs at `offset` in `color`.
    #[serde(default)]
    shadow: Option<Shadow>,
    /// Plate layers under the whole part's glyphs, bottom first: each is the
    /// glyph ink dilated by a round `radius` and moved by `offset`. Glyphs are
    /// drawn after every layer so neighbouring letters never cover each other.
    #[serde(default)]
    layers: Vec<Layer>,
    /// Filled rectangles `[x0, y0, x1, y1)` added to the glyph ink for layers
    /// with a positive radius, giving a rounded plate behind short text.
    #[serde(default)]
    plate_rects: Vec<[usize; 4]>,
    /// Plate footprint image (non-black = plate, full 256x192) merged into
    /// every layer with a positive radius, grown by `radius - inset` (shrunk
    /// when negative), so an existing plate outline is kept and covered.
    #[serde(default)]
    plate_mask: Option<PlateMask>,
    /// Absolute left x of each line; default centres every line.
    #[serde(default)]
    line_x: Vec<usize>,
    /// Square outline radius; omitted retains the original one-pixel cross.
    #[serde(default)]
    outline_radius: Option<usize>,
    /// Widen strokes one pixel to the right for large or bold source lettering.
    #[serde(default)]
    bold: bool,
    /// Thicken vertical strokes where a one-pixel gap remains
    /// (`labels::bold_vertical_runs`), keeping narrow Hangul counters open.
    #[serde(default)]
    bold_keep_gap: bool,
    /// Line indices drawn with the small font (8px), centred in the line slot,
    /// for lines too wide for the part font.
    #[serde(default)]
    small_lines: Vec<usize>,
    /// Line indices drawn with `--medium-font` at 12px, vertically centred on
    /// the line slot, for lines too wide for the part font.
    #[serde(default)]
    medium_lines: Vec<usize>,
    /// Line indices drawn with `--narrow-font` at 12px, placed like
    /// `medium_lines`, for short lines whose small Hangul parts blur at the
    /// part font (e.g. the ㅄ of 없 on a 33px orb).
    #[serde(default)]
    narrow_lines: Vec<usize>,
    /// Independent generated lettering composited over the reconstructed part
    /// background, e.g. a decorative title; text lines may then be empty.
    #[serde(default)]
    overlay: Option<Overlay>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Overlay {
    image: String,
    image_sha256: String,
    /// Declared uniform production matte, removed before reduction.
    #[serde(default)]
    matte: Option<crate::art_pixels::ProductionMatte>,
    /// Source rectangle `[x0, y0, x1, y1)` in the generated image.
    source: [usize; 4],
    /// Screen rectangle inside the part region; the source is scaled to fill it.
    target: [usize; 4],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlateMask {
    image: String,
    image_sha256: String,
    inset: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    radius: usize,
    offset: [i32; 2],
    color: [i32; 3],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Shadow {
    offset: [i32; 2],
    color: [i32; 3],
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct BackgroundMask {
    colors: Vec<[i32; 3]>,
    radius: usize,
    /// Also reconstruct pixels enclosed by the mask (the interior of an outlined
    /// source title), found as the part pixels not connected to the part border.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    fill_holes: bool,
    /// Explicit source-lettering areas to reconstruct completely, for
    /// anti-aliased lettering whose colours are shared with the artwork.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    areas: Vec<MaskArea>,
    /// Continue striped artwork through the hole along its local orientation
    /// instead of the isotropic harmonic fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flow: Option<FlowFill>,
}

/// Streamline fill: the stripe orientation is the Gaussian-smoothed structure
/// tensor of the listed artwork colours; each hole pixel interpolates the first
/// artwork pixels reached along it on both sides.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct FlowFill {
    sigma: f64,
    colors: Vec<[i32; 3]>,
}

fn flow_fill(
    original: &[[i32; 3]],
    selected: &[bool],
    [x0, y0, x1, y1]: [usize; 4],
    flow: &FlowFill,
) -> Result<Vec<[i32; 3]>> {
    const W: usize = 256;
    const H: usize = 192;
    ensure!(
        (1.0..=16.0).contains(&flow.sigma) && !flow.colors.is_empty(),
        "flow fill geometry"
    );
    let art: Vec<bool> = (0..W * H)
        .map(|p| !selected[p] && flow.colors.contains(&original[p]))
        .collect();
    let lum: Vec<f64> = original
        .iter()
        .map(|c| f64::from(c[0] + c[1] + c[2]))
        .collect();
    let mut tensor = vec![[0.0f64; 3]; W * H];
    for y in 1..H - 1 {
        for x in 1..W - 1 {
            let p = y * W + x;
            if [p, p - 1, p + 1, p - W, p + W].iter().all(|&q| art[q]) {
                let gx = (lum[p + 1] - lum[p - 1]) / 2.0;
                let gy = (lum[p + W] - lum[p - W]) / 2.0;
                tensor[p] = [gx * gx, gy * gy, gx * gy];
            }
        }
    }
    let reach = (flow.sigma * 3.0).ceil() as i64;
    let kernel: Vec<f64> = (-reach..=reach)
        .map(|d| (-(d * d) as f64 / (2.0 * flow.sigma * flow.sigma)).exp())
        .collect();
    let blur = |input: &[[f64; 3]], horizontal: bool| -> Vec<[f64; 3]> {
        let mut out = vec![[0.0; 3]; W * H];
        for y in 0..H as i64 {
            for x in 0..W as i64 {
                let mut acc = [0.0; 3];
                for (k, w) in kernel.iter().enumerate() {
                    let d = k as i64 - reach;
                    let (sx, sy) = if horizontal { (x + d, y) } else { (x, y + d) };
                    if sx < 0 || sy < 0 || sx >= W as i64 || sy >= H as i64 {
                        continue;
                    }
                    let v = input[sy as usize * W + sx as usize];
                    for c in 0..3 {
                        acc[c] += v[c] * w;
                    }
                }
                out[y as usize * W + x as usize] = acc;
            }
        }
        out
    };
    let smooth = blur(&blur(&tensor, true), false);
    // Stripe direction is perpendicular to the dominant gradient.
    let angle: Vec<f64> = smooth
        .iter()
        .map(|[xx, yy, xy]| 0.5 * (2.0 * xy).atan2(xx - yy) + std::f64::consts::FRAC_PI_2)
        .collect();
    let march = |x: usize, y: usize, sign: f64| -> Option<(f64, [i32; 3])> {
        let a = angle[y * W + x];
        let (mut dx, mut dy) = (sign * a.cos(), sign * a.sin());
        let (mut px, mut py, mut t) = (x as f64, y as f64, 0.0);
        for _ in 0..1200 {
            let (xi, yi) = (px.round(), py.round());
            if xi < 0.0 || yi < 0.0 || xi >= W as f64 || yi >= H as f64 {
                return None;
            }
            let a = angle[yi as usize * W + xi as usize];
            let (mut nx, mut ny) = (a.cos(), a.sin());
            if nx * dx + ny * dy < 0.0 {
                (nx, ny) = (-nx, -ny);
            }
            (dx, dy) = (nx, ny);
            px += dx * 0.5;
            py += dy * 0.5;
            t += 0.5;
            let (xi, yi) = (px.round(), py.round());
            if xi < 0.0 || yi < 0.0 || xi >= W as f64 || yi >= H as f64 {
                return None;
            }
            let q = yi as usize * W + xi as usize;
            if !selected[q] {
                return art[q].then(|| (t, original[q]));
            }
        }
        None
    };
    let mut out = original.to_vec();
    let mut pending = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let p = y * W + x;
            if !selected[p] {
                continue;
            }
            match (march(x, y, 1.0), march(x, y, -1.0)) {
                (Some((t1, c1)), Some((t2, c2))) => {
                    out[p] = std::array::from_fn(|k| {
                        ((f64::from(c1[k]) * t2 + f64::from(c2[k]) * t1) / (t1 + t2)).round() as i32
                    })
                }
                (Some((_, c)), None) | (None, Some((_, c))) => out[p] = c,
                (None, None) => pending.push(p),
            }
        }
    }
    // Pixels whose streamlines meet no artwork take their resolved neighbours.
    let mut resolved: Vec<bool> = (0..W * H)
        .map(|p| !selected[p] || !pending.contains(&p))
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = Vec::new();
        let mut updates = Vec::new();
        for &p in &pending {
            let near: Vec<usize> = [p - 1, p + 1, p - W, p + W]
                .into_iter()
                .filter(|&q| resolved[q] && (selected[q] || art[q]))
                .collect();
            if near.is_empty() {
                next.push(p);
            } else {
                let colour: [i32; 3] = std::array::from_fn(|k| {
                    near.iter().map(|&q| out[q][k]).sum::<i32>() / near.len() as i32
                });
                updates.push((p, colour));
            }
        }
        for (p, colour) in updates {
            out[p] = colour;
            resolved[p] = true;
        }
        ensure!(next.len() < before, "flow fill left isolated pixels");
        pending = next;
    }
    Ok(out)
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct MaskArea {
    rect: [usize; 4],
    /// Optional `[cx, cy, r]` disc clipping the rectangle, e.g. a round button face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    circle: Option<[f64; 3]>,
}

fn masked_background(original: &[[i32; 3]], part: &Part) -> Result<Vec<[i32; 3]>> {
    let mask = part.background_mask.as_ref().unwrap();
    let [x0, y0, x1, y1] = part.region;
    ensure!(
        part.background == "harmonic",
        "background mask requires harmonic mode"
    );
    ensure!(
        x0 > 0 && y0 > 0 && x1 < 256 && y1 < 192,
        "harmonic needs border"
    );
    ensure!(
        (!mask.colors.is_empty() || !mask.areas.is_empty()) && mask.radius <= 4,
        "background mask geometry"
    );
    ensure!(
        mask.colors.iter().flatten().all(|v| (0..=31).contains(v)),
        "mask color outside RGB555"
    );
    let mut selected = vec![false; original.len()];
    let mut seeds = 0;
    for area in &mask.areas {
        let [ax0, ay0, ax1, ay1] = area.rect;
        ensure!(
            x0 <= ax0 && ax0 < ax1 && ax1 <= x1 && y0 <= ay0 && ay0 < ay1 && ay1 <= y1,
            "mask area outside part"
        );
        for y in ay0..ay1 {
            for x in ax0..ax1 {
                let inside = area.circle.is_none_or(|[cx, cy, r]| {
                    (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2) <= r * r
                });
                if inside {
                    selected[y * 256 + x] = true;
                    seeds += 1;
                }
            }
        }
    }
    for y in y0..y1 {
        for x in x0..x1 {
            if mask.colors.contains(&original[y * 256 + x]) {
                seeds += 1;
                for yy in y.saturating_sub(mask.radius).max(y0)..=(y + mask.radius).min(y1 - 1) {
                    for xx in x.saturating_sub(mask.radius).max(x0)..=(x + mask.radius).min(x1 - 1)
                    {
                        selected[yy * 256 + xx] = true;
                    }
                }
            }
        }
    }
    ensure!(seeds > 0, "background mask found no source lettering");
    if mask.fill_holes {
        let (w, h) = (x1 - x0, y1 - y0);
        let mut outside = vec![false; w * h];
        let mut stack: Vec<(usize, usize)> = (0..w)
            .flat_map(|x| [(x, 0), (x, h - 1)])
            .chain((0..h).flat_map(|y| [(0, y), (w - 1, y)]))
            .collect();
        while let Some((x, y)) = stack.pop() {
            let i = y * w + x;
            if outside[i] || selected[(y0 + y) * 256 + x0 + x] {
                continue;
            }
            outside[i] = true;
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < w {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < h {
                stack.push((x, y + 1));
            }
        }
        for y in 0..h {
            for x in 0..w {
                if !outside[y * w + x] {
                    selected[(y0 + y) * 256 + x0 + x] = true;
                }
            }
        }
    }
    if let Some(flow) = &mask.flow {
        return flow_fill(original, &selected, part.region, flow);
    }
    let mut colors: Vec<[f64; 3]> = original.iter().map(|c| c.map(f64::from)).collect();
    let mut converged = false;
    for _ in 0..4096 {
        let mut largest = 0.0f64;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = y * 256 + x;
                if !selected[p] {
                    continue;
                }
                for k in 0..3 {
                    let v = (colors[p - 1][k]
                        + colors[p + 1][k]
                        + colors[p - 256][k]
                        + colors[p + 256][k])
                        / 4.0;
                    largest = largest.max((v - colors[p][k]).abs());
                    colors[p][k] = v;
                }
            }
        }
        if largest < 0.00001 {
            converged = true;
            break;
        }
    }
    ensure!(converged, "harmonic background did not converge");
    Ok(colors
        .iter()
        .enumerate()
        .map(|(p, c)| {
            if selected[p] {
                c.map(|v| v.round() as i32)
            } else {
                original[p]
            }
        })
        .collect())
}
fn regular() -> String {
    "regular".into()
}

struct Decoded {
    palette: Vec<u8>,
    colors: Vec<[i32; 3]>,
    /// Palette index per pixel (bank * 16 + value for 4bpp).
    pixels: Vec<u8>,
    map: Vec<u8>,
    tiles: Vec<u8>,
}

fn decode(narc: &Narc, s: &ScreenInput) -> Result<Decoded> {
    let map = unpack(narc.members[s.map])?;
    let tiles = unpack(narc.members[s.tiles])?;
    let palette = unpack(narc.members[s.palette])?;
    ensure!(
        sha(&tiles) == s.tiles_sha256,
        "screen tiles {} changed",
        s.tiles
    );
    ensure!(map.len() == 1536, "screen map size");
    let pixels = match s.bpp {
        4 => screens::render(&map, &tiles, palette.len() / 32)?,
        8 => {
            ensure!(tiles.len() % 64 == 0, "8bpp tile size");
            let mut px = vec![0; 256 * 192];
            for cell in 0..768 {
                let attr = u16le(&map, cell * 2)?;
                let id = attr & 1023;
                ensure!(id < tiles.len() / 64, "screen tile outside set");
                for p in 0..64 {
                    let (x, y) = (p % 8, p / 8);
                    let sx = if attr & 0x400 != 0 { 7 - x } else { x };
                    let sy = if attr & 0x800 != 0 { 7 - y } else { y };
                    px[(cell / 32 * 8 + y) * 256 + cell % 32 * 8 + x] =
                        tiles[id * 64 + sy * 8 + sx];
                }
            }
            px
        }
        _ => anyhow::bail!("unsupported bpp"),
    };
    let colors = (0..palette.len() / 2)
        .map(|i| buttons::rgb(&palette, i))
        .collect::<Result<Vec<_>>>()?;
    Ok(Decoded {
        palette,
        colors,
        pixels,
        map,
        tiles,
    })
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
    let tr: Input = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft",
        "unexpected input state"
    );
    let source = rom.data(rom.file(&tr.archive)?);
    ensure!(sha(source) == tr.archive_sha256, "screen archive changed");
    let narc = Narc::parse(source)?;
    let load = |p: &Path| -> Result<fontdue::Font> {
        fontdue::Font::from_bytes(fs::read(p)?, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))
    };
    let (regular, small) = (load(font_path)?, load(small_font)?);
    let medium = medium_font.map(load).transpose()?;
    let narrow = narrow_font.map(load).transpose()?;
    let mut changes = BTreeMap::new();
    let mut reports = Vec::new();
    fs::create_dir_all(out)?;
    for s in &tr.screens {
        let d = decode(&narc, s)?;
        let mut desired: Vec<[i32; 3]> =
            d.pixels.iter().map(|&i| d.colors[usize::from(i)]).collect();
        let mut artwork_pixels = vec![false; 256 * 192];
        let mut artwork_regions = artwork_pixels.clone();
        if let Some(art) = &s.background_artwork {
            ensure!(
                art.bank_change_cost >= 0,
                "negative background bank change cost"
            );
            if art.regions.is_empty() {
                artwork_pixels.fill(true);
            } else {
                for &[x0, y0, x1, y1] in &art.regions {
                    ensure!(
                        x0 < x1 && y0 < y1 && x1 <= 256 && y1 <= 192,
                        "background artwork region outside screen"
                    );
                    for y in y0..y1 {
                        for x in x0..x1 {
                            ensure!(!artwork_pixels[y * 256 + x], "overlapping artwork regions");
                            artwork_pixels[y * 256 + x] = true;
                        }
                    }
                }
            }
            artwork_regions.clone_from(&artwork_pixels);
            ensure!(
                !art.mask_tile_alignment || art.source_mask.is_some(),
                "tile alignment requires source mask"
            );
            if let Some(mask) = &art.source_mask {
                ensure!(
                    !art.regions.is_empty()
                        && !mask.colors.is_empty()
                        && mask.radius <= 4
                        && !mask.fill_holes,
                    "background source mask needs regions and valid geometry"
                );
                ensure!(
                    mask.colors.iter().flatten().all(|v| (0..=31).contains(v)),
                    "background source mask color outside RGB555"
                );
                artwork_pixels.fill(false);
                let mut seeds = 0;
                for &[x0, y0, x1, y1] in &art.regions {
                    for y in y0..y1 {
                        for x in x0..x1 {
                            if !mask.colors.contains(&desired[y * 256 + x]) {
                                continue;
                            }
                            seeds += 1;
                            for yy in y.saturating_sub(mask.radius).max(y0)
                                ..=(y + mask.radius).min(y1 - 1)
                            {
                                for xx in x.saturating_sub(mask.radius).max(x0)
                                    ..=(x + mask.radius).min(x1 - 1)
                                {
                                    artwork_pixels[yy * 256 + xx] = true;
                                }
                            }
                        }
                    }
                }
                ensure!(seeds > 0, "background source mask found no lettering");
                if art.mask_tile_alignment {
                    let selected = artwork_pixels.clone();
                    for (p, &on) in selected.iter().enumerate() {
                        if !on {
                            continue;
                        }
                        let tx = (p % 256) / 8 * 8;
                        let ty = (p / 256) / 8 * 8;
                        for y in ty..ty + 8 {
                            for x in tx..tx + 8 {
                                if artwork_regions[y * 256 + x] {
                                    artwork_pixels[y * 256 + x] = true;
                                }
                            }
                        }
                    }
                }
            }
            ensure!(s.bpp == 4, "whole background requires 4bpp screen");
            ensure!(
                s.parts
                    .iter()
                    .all(|p| p.background == "keep" || p.background == "harmonic"),
                "generated background text requires keep or disjoint harmonic mode"
            );
            let bytes = fs::read(&art.image)?;
            ensure!(
                sha(&bytes) == art.image_sha256,
                "background artwork identity"
            );
            let image = crate::art_pixels::read(&bytes)?;
            let rgba = match art.sampling.as_str() {
                "nearest" => crate::art_pixels::reduce_nearest(&image, 256, 192)?,
                "area" => crate::art_pixels::reduce(&image, 256, 192, false)?,
                _ => anyhow::bail!("unknown background sampling"),
            };
            ensure!(
                rgba.chunks_exact(4).all(|p| p[3] == 255),
                "background must be opaque"
            );
            desired = rgba
                .chunks_exact(4)
                .map(|p| std::array::from_fn(|k| i32::from(p[k]) * 31 / 255))
                .collect();
            if let Some(mask) = &art.difference_mask {
                ensure!(
                    art.source_mask.is_none() && !art.mask_tile_alignment,
                    "difference mask replaces the source mask"
                );
                let source: Vec<[i32; 3]> =
                    d.pixels.iter().map(|&i| d.colors[usize::from(i)]).collect();
                artwork_pixels = difference_mask(&source, &desired, &art.regions, mask)?;
            }
            if !art.colors.is_empty() {
                ensure!(
                    art.colors.iter().flatten().all(|v| (0..=31).contains(v)),
                    "background color outside RGB555"
                );
                for color in &mut desired {
                    *color = *art
                        .colors
                        .iter()
                        .min_by_key(|candidate| {
                            (0..3)
                                .map(|k| (candidate[k] - color[k]).pow(2))
                                .sum::<i32>()
                        })
                        .unwrap();
                }
            }
        }
        for (p, color) in desired.iter_mut().enumerate() {
            if !artwork_pixels[p] {
                *color = d.colors[usize::from(d.pixels[p])];
            }
        }
        if s.background_artwork.as_ref().is_some_and(|a| a.despeckle) {
            let before = desired.clone();
            for y in 1..191 {
                for x in 1..255 {
                    let p = y * 256 + x;
                    if !artwork_pixels[p] {
                        continue;
                    }
                    let around: Vec<[i32; 3]> = (y - 1..=y + 1)
                        .flat_map(|yy| (x - 1..=x + 1).map(move |xx| yy * 256 + xx))
                        .map(|q| before[q])
                        .collect();
                    if around.iter().filter(|&&c| c == before[p]).count() > 1 {
                        continue;
                    }
                    if let Some(&major) = around
                        .iter()
                        .find(|&&c| around.iter().filter(|&&o| o == c).count() >= 5)
                    {
                        desired[p] = major;
                    }
                }
            }
        }
        let mut editable = vec![false; 256 * 192];
        let mut text_pixels = vec![false; 256 * 192];
        let mut ink_pixels = vec![false; 256 * 192];
        for part in &s.parts {
            let [x0, y0, x1, y1] = part.region;
            ensure!(x0 < x1 && y0 < y1 && x1 <= 256 && y1 <= 192, "part region");
            if s.background_artwork.is_some() && part.background == "keep" {
                ensure!(
                    (y0..y1).all(|y| (x0..x1).all(|x| artwork_regions[y * 256 + x])),
                    "text part outside generated artwork regions"
                );
            }
            if s.background_artwork.is_some() && part.background == "harmonic" {
                ensure!(
                    (y0..y1).all(|y| (x0..x1).all(|x| !artwork_regions[y * 256 + x])),
                    "harmonic text part overlaps generated artwork regions"
                );
            }
            let original = desired.clone();
            let reconstructed = part
                .background_mask
                .as_ref()
                .map(|_| masked_background(&original, part))
                .transpose()?;
            let at = |x: usize, y: usize| original[y * 256 + x];
            for y in y0..y1 {
                for x in x0..x1 {
                    ensure!(!editable[y * 256 + x], "overlapping screen parts");
                    editable[y * 256 + x] = true;
                    desired[y * 256 + x] = match part.background.as_str() {
                        "keep" => at(x, y),
                        "harmonic" => reconstructed
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("harmonic requires background mask"))?
                            [y * 256 + x],
                        "flat" => part
                            .flat_color
                            .ok_or_else(|| anyhow::anyhow!("flat needs flat_color"))?,
                        "row" => {
                            ensure!(x0 > 0 && x1 < 256, "row needs side pixels");
                            let (l, r) = (at(x0 - 1, y), at(x1, y));
                            let (u, n) = ((x - x0 + 1) as i32, (x1 - x0 + 1) as i32);
                            std::array::from_fn(|k| (l[k] * (n - u) + r[k] * u + n / 2) / n)
                        }
                        "bilinear" => {
                            ensure!(
                                x0 > 0 && x1 < 256 && y0 > 0 && y1 < 192,
                                "bilinear needs a border"
                            );
                            let (dx, dy) = ((x1 - x0 + 1) as i32, (y1 - y0 + 1) as i32);
                            let (u, v) = ((x - x0 + 1) as i32, (y - y0 + 1) as i32);
                            let (t, b, l, r) = (at(x, y0 - 1), at(x, y1), at(x0 - 1, y), at(x1, y));
                            let c = [
                                at(x0 - 1, y0 - 1),
                                at(x1, y0 - 1),
                                at(x0 - 1, y1),
                                at(x1, y1),
                            ];
                            std::array::from_fn(|k| {
                                let vert = (t[k] * (dy - v) + b[k] * v) * dx;
                                let hori = (l[k] * (dx - u) + r[k] * u) * dy;
                                let corner = c[0][k] * (dx - u) * (dy - v)
                                    + c[1][k] * u * (dy - v)
                                    + c[2][k] * (dx - u) * v
                                    + c[3][k] * u * v;
                                ((vert + hori - corner + dx * dy / 2) / (dx * dy)).clamp(0, 31)
                            })
                        }
                        other => anyhow::bail!("unknown background {other}"),
                    };
                }
            }
            if let Some(ov) = &part.overlay {
                let [tx0, ty0, tx1, ty1] = ov.target;
                ensure!(
                    x0 <= tx0 && tx0 < tx1 && tx1 <= x1 && y0 <= ty0 && ty0 < ty1 && ty1 <= y1,
                    "overlay target outside part"
                );
                let bytes = fs::read(&ov.image)?;
                ensure!(sha(&bytes) == ov.image_sha256, "overlay artwork identity");
                let mut image = crate::art_pixels::read(&bytes)?;
                if let Some(matte) = &ov.matte {
                    crate::art_pixels::remove_matte(&mut image, matte)?;
                }
                let rgba = crate::art_pixels::reduce(
                    &crate::art_pixels::region(&image, ov.source)?,
                    tx1 - tx0,
                    ty1 - ty0,
                    false,
                )?;
                let mut covered = 0;
                for y in ty0..ty1 {
                    for x in tx0..tx1 {
                        let c = &rgba[((y - ty0) * (tx1 - tx0) + x - tx0) * 4..][..4];
                        // Same alpha-128 rule as other generated lettering.
                        if c[3] >= 128 {
                            desired[y * 256 + x] =
                                std::array::from_fn(|k| (i32::from(c[k]) * 31 + 127) / 255);
                            covered += 1;
                        }
                    }
                }
                ensure!(covered > 0, "overlay artwork is empty");
            }
            ensure!(
                !part.korean.is_empty() || part.overlay.is_some(),
                "part without text or overlay"
            );
            if part.korean.is_empty() {
                continue;
            }
            let (font, size) = match part.font.as_str() {
                "regular" => (&regular, part.size.unwrap_or(12)),
                "small" => (&small, part.size.unwrap_or(8)),
                _ => anyhow::bail!("unknown font"),
            };
            let line_height = part.line_height.unwrap_or(size + 4);
            ensure!(
                part.small_lines.iter().all(|&i| i < part.korean.len())
                    && (part.small_lines.is_empty() || part.font == "regular"),
                "small lines need a regular part and existing lines"
            );
            ensure!(
                part.narrow_lines.iter().all(|&i| i < part.korean.len()),
                "narrow lines must name existing lines"
            );
            let block = part.korean.len() * line_height - (line_height - size);
            ensure!(block <= y1 - y0, "part text block too tall");
            let mut top = part.text_top.unwrap_or(y0 + (y1 - y0 - block) / 2);
            ensure!(top >= y0 && top + block <= y1, "text top leaves part");
            ensure!(
                part.line_x.is_empty() || part.line_x.len() == part.korean.len(),
                "line_x needs one entry per line"
            );
            ensure!(
                part.layers.is_empty() || (part.outline_color.is_none() && part.shadow.is_none()),
                "layers replace outline and shadow"
            );
            let mut part_ink: Vec<(usize, usize, [i32; 3])> = Vec::new();
            let (part_font, part_size) = (font, size);
            for (index, line) in part.korean.iter().enumerate() {
                let (font, size, line_top) = if part.small_lines.contains(&index) {
                    ensure!(part_size >= 8, "small line larger than its slot");
                    (&small, 8, top + (part_size - 8).div_ceil(2))
                } else if part.medium_lines.contains(&index) || part.narrow_lines.contains(&index) {
                    ensure!(
                        !(part.medium_lines.contains(&index) && part.narrow_lines.contains(&index)),
                        "line {index} is both medium and narrow"
                    );
                    let face = if part.medium_lines.contains(&index) {
                        medium
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("medium lines need --medium-font"))?
                    } else {
                        narrow
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("narrow lines need --narrow-font"))?
                    };
                    // Centre the 12px line on the part-font slot.
                    let centred = (top + part_size / 2).checked_sub(6);
                    (
                        face,
                        12,
                        centred.ok_or_else(|| anyhow::anyhow!("12px line leaves the screen"))?,
                    )
                } else {
                    (part_font, part_size, top)
                };
                let advance = |c: char| {
                    if c == ' ' {
                        (size + 1) / 3
                    } else {
                        font.metrics(c, size as f32).advance_width.round() as usize
                    }
                };
                let width: usize = line.iter().flat_map(|(t, _)| t.chars()).map(advance).sum();
                ensure!(
                    width + 2 <= x1 - x0,
                    "screen line too wide: {width} in {}",
                    x1 - x0
                );
                ensure!(!(part.left && part.right), "part cannot align both sides");
                let mut cursor = if let Some(&x) = part.line_x.get(index) {
                    ensure!(x >= x0 && x + width <= x1, "line_x leaves part");
                    x
                } else if part.left {
                    x0 + 1
                } else if part.right {
                    x1 - 1 - width
                } else {
                    x0 + (x1 - x0 - width) / 2
                };
                for (text, colour) in line {
                    for c in text.chars() {
                        ensure!(
                            c == ' ' || font.lookup_glyph_index(c) != 0,
                            "font has no glyph for {c}"
                        );
                        let (m, bitmap) = font.rasterize(c, size as f32);
                        let mut ink = Vec::new();
                        for gy in 0..m.height {
                            for gx in 0..m.width {
                                if bitmap[gy * m.width + gx] >= 128 {
                                    let px = cursor as i32 + m.xmin + gx as i32;
                                    let py =
                                        (line_top + size - 1) as i32 - m.ymin - m.height as i32
                                            + gy as i32;
                                    ensure!(
                                        px > x0 as i32 - 1
                                            && px < x1 as i32
                                            && py >= y0 as i32
                                            && py < y1 as i32,
                                        "glyph leaves part"
                                    );
                                    ink.push((px as usize, py as usize));
                                }
                            }
                        }
                        if part.bold_keep_gap {
                            ensure!(!part.bold, "choose one bold mode");
                            let set: std::collections::BTreeSet<(i32, i32)> =
                                ink.iter().map(|&(x, y)| (x as i32, y as i32)).collect();
                            for (x, y) in crate::labels::bold_vertical_runs(&set) {
                                ensure!(x >= x0 as i32 && x < x1 as i32, "bold glyph leaves part");
                                ink.push((x as usize, y as usize));
                            }
                        }
                        if part.bold {
                            let wide: Vec<_> = ink.iter().map(|&(x, y)| (x + 1, y)).collect();
                            for &(x, _) in &wide {
                                ensure!(x < x1, "bold glyph leaves part");
                            }
                            ink.extend(wide);
                        }
                        if let Some(o) = part.outline_color {
                            let offsets: Vec<(i32, i32)> = if let Some(radius) = part.outline_radius
                            {
                                ensure!((1..=4).contains(&radius), "outline radius outside 1..4");
                                let radius = radius as i32;
                                (-radius..=radius)
                                    .flat_map(|dy| (-radius..=radius).map(move |dx| (dx, dy)))
                                    .filter(|&(dx, dy)| dx != 0 || dy != 0)
                                    .collect()
                            } else {
                                vec![(-1, 0), (1, 0), (0, -1), (0, 1)]
                            };
                            for &(x, y) in &ink {
                                for &(dx, dy) in &offsets {
                                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                                    ensure!(
                                        nx >= x0 as i32
                                            && nx < x1 as i32
                                            && ny >= y0 as i32
                                            && ny < y1 as i32,
                                        "outline leaves part"
                                    );
                                    desired[ny as usize * 256 + nx as usize] = o;
                                    text_pixels[ny as usize * 256 + nx as usize] = true;
                                }
                            }
                        }
                        if let Some(shadow) = &part.shadow {
                            ensure!(
                                shadow.color.iter().all(|v| (0..=31).contains(v)),
                                "shadow colour outside RGB555"
                            );
                            let [dx, dy] = shadow.offset;
                            for &(x, y) in &ink {
                                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                                ensure!(
                                    nx >= x0 as i32
                                        && nx < x1 as i32
                                        && ny >= y0 as i32
                                        && ny < y1 as i32,
                                    "shadow leaves part"
                                );
                                // Shadows never cover glyph ink already drawn on this line.
                                let p = ny as usize * 256 + nx as usize;
                                if !ink_pixels[p] {
                                    desired[p] = shadow.color;
                                    text_pixels[p] = true;
                                }
                            }
                        }
                        if !part.layers.is_empty() {
                            part_ink.extend(ink.iter().map(|&(x, y)| (x, y, *colour)));
                            cursor += advance(c);
                            continue;
                        }
                        for &(x, y) in &ink {
                            desired[y * 256 + x] = *colour;
                            text_pixels[y * 256 + x] = true;
                            ink_pixels[y * 256 + x] = true;
                        }
                        cursor += advance(c);
                    }
                }
                top += line_height;
            }
            ensure!(
                part.plate_rects.is_empty() || !part.layers.is_empty(),
                "plate rectangles need layers"
            );
            let mut plate_base: Vec<(usize, usize)> =
                part_ink.iter().map(|&(x, y, _)| (x, y)).collect();
            for &[rx0, ry0, rx1, ry1] in &part.plate_rects {
                ensure!(
                    rx0 < rx1 && ry0 < ry1 && rx0 >= x0 && ry0 >= y0 && rx1 <= x1 && ry1 <= y1,
                    "plate rectangle outside part"
                );
                plate_base.extend((ry0..ry1).flat_map(|y| (rx0..rx1).map(move |x| (x, y))));
            }
            let mask: Vec<bool> = match &part.plate_mask {
                None => Vec::new(),
                Some(m) => {
                    ensure!(!part.layers.is_empty(), "plate mask needs layers");
                    let bytes = fs::read(&m.image)?;
                    ensure!(sha(&bytes) == m.image_sha256, "plate mask identity");
                    let image = crate::art_pixels::read(&bytes)?;
                    ensure!(
                        image.width == 256 && image.height == 192,
                        "plate mask must be a full screen"
                    );
                    let mask: Vec<bool> = image
                        .rgba
                        .chunks_exact(4)
                        .map(|c| c[3] > 0 && (c[0] | c[1] | c[2]) != 0)
                        .collect();
                    for (p, &on) in mask.iter().enumerate() {
                        let (x, y) = (p % 256, p / 256);
                        ensure!(
                            !on || (x0..x1).contains(&x) && (y0..y1).contains(&y),
                            "plate mask outside part"
                        );
                    }
                    mask
                }
            };
            let disc = |r: i32| {
                (-r..=r).flat_map(move |dy| {
                    (-r..=r)
                        .filter(move |dx| dx * dx + dy * dy <= r * r + r)
                        .map(move |dx| (dx, dy))
                })
            };
            for layer in &part.layers {
                ensure!(
                    layer.radius <= 8 && layer.color.iter().all(|v| (0..=31).contains(v)),
                    "invalid plate layer"
                );
                let r = layer.radius as i32;
                let mut painted = vec![false; 256 * 192];
                if r > 0 && !mask.is_empty() {
                    let inset = part.plate_mask.as_ref().unwrap().inset;
                    let k = r - inset;
                    let on = |x: i32, y: i32| {
                        (0..256).contains(&x)
                            && (0..192).contains(&y)
                            && mask[(y * 256 + x) as usize]
                    };
                    for y in 0..192i32 {
                        for x in 0..256i32 {
                            let keep = if k >= 0 {
                                disc(k).any(|(dx, dy)| on(x - dx, y - dy))
                            } else {
                                on(x, y) && disc(-k).all(|(dx, dy)| on(x + dx, y + dy))
                            };
                            if !keep {
                                continue;
                            }
                            let (nx, ny) = (x + layer.offset[0], y + layer.offset[1]);
                            ensure!(
                                nx >= x0 as i32
                                    && nx < x1 as i32
                                    && ny >= y0 as i32
                                    && ny < y1 as i32,
                                "plate mask layer leaves part at ({nx},{ny})"
                            );
                            painted[ny as usize * 256 + nx as usize] = true;
                        }
                    }
                }
                let base: Vec<(usize, usize)> = if r > 0 {
                    plate_base.clone()
                } else {
                    part_ink.iter().map(|&(x, y, _)| (x, y)).collect()
                };
                for &(x, y) in &base {
                    for dy in -r..=r {
                        for dx in -r..=r {
                            if dx * dx + dy * dy > r * r + r {
                                continue;
                            }
                            let (nx, ny) = (
                                x as i32 + dx + layer.offset[0],
                                y as i32 + dy + layer.offset[1],
                            );
                            ensure!(
                                nx >= x0 as i32
                                    && nx < x1 as i32
                                    && ny >= y0 as i32
                                    && ny < y1 as i32,
                                "plate layer leaves part at ({nx},{ny})"
                            );
                            painted[ny as usize * 256 + nx as usize] = true;
                        }
                    }
                }
                for (p, &on) in painted.iter().enumerate() {
                    if on {
                        desired[p] = layer.color;
                        text_pixels[p] = true;
                    }
                }
            }
            for &(x, y, colour) in &part_ink {
                desired[y * 256 + x] = colour;
                text_pixels[y * 256 + x] = true;
                ink_pixels[y * 256 + x] = true;
            }
        }
        if (s.background_artwork.is_none() && s.parts.iter().any(|p| p.background_mask.is_some()))
            || s.background_artwork
                .as_ref()
                .is_some_and(|a| a.source_mask.is_some())
        {
            // Preserve unchanged source colors even inside the declared text rectangle.
            for (p, writable) in editable.iter_mut().enumerate() {
                *writable &= desired[p] != d.colors[usize::from(d.pixels[p])];
            }
        }
        let mut copied_pixels = vec![false; 256 * 192];
        let mut encoding_pixels = d.pixels.clone();
        ensure!(
            s.source_copies.is_empty() || (s.bpp == 4 && s.background_artwork.is_some()),
            "source copies require a generated 4bpp background"
        );
        for copy in &s.source_copies {
            let [x0, y0, x1, y1] = copy.source;
            let [tx, ty] = copy.target;
            ensure!(
                x0 < x1 && y0 < y1 && x1 <= 256 && y1 <= 192,
                "source copy outside screen"
            );
            let (width, height) = (x1 - x0, y1 - y0);
            ensure!(
                tx <= 256 - width && ty <= 192 - height,
                "source copy target outside screen"
            );
            for y in 0..height {
                for x in 0..width {
                    let p = (ty + y) * 256 + tx + x;
                    ensure!(
                        !editable[p] && !copied_pixels[p],
                        "source copy overlaps text or another copy"
                    );
                    desired[p] = d.colors[usize::from(d.pixels[(y0 + y) * 256 + x0 + x])];
                    encoding_pixels[p] = d.pixels[(y0 + y) * 256 + x0 + x];
                    copied_pixels[p] = true;
                    artwork_pixels[p] = true;
                }
            }
        }
        if s.background_artwork.is_some() || s.parts.iter().any(|p| p.overlay.is_some()) {
            let rgba: Vec<u8> = desired
                .iter()
                .flat_map(|c| {
                    [
                        (c[0] * 255 / 31) as u8,
                        (c[1] * 255 / 31) as u8,
                        (c[2] * 255 / 31) as u8,
                        255,
                    ]
                })
                .collect();
            write_png(
                &out.join(format!("{:03}-authored.png", s.tiles)),
                256,
                192,
                &rgba,
            )?;
        }
        let mut generated_tile_count = None;
        let (map, tiles, pixels) = match s.bpp {
            4 => {
                let screen = screens::Screen {
                    map: d.map.clone(),
                    tiles: if s.background_artwork.is_some() {
                        vec![0; 768 * 32]
                    } else {
                        d.tiles.clone()
                    },
                    palette: d.palette.clone(),
                    pixels: encoding_pixels,
                };
                let exact = |x: usize, y: usize| {
                    (s.exact_part_colors && editable[y * 256 + x])
                        || (s.exact_text_colors && text_pixels[y * 256 + x])
                };
                let encode = |desired: &[[i32; 3]]| {
                    screens::encode_screen_with_exact(
                        &screen,
                        |x, y| {
                            !copied_pixels[y * 256 + x]
                                && (artwork_pixels[y * 256 + x] || editable[y * 256 + x])
                        },
                        desired,
                        |_, bank, left| {
                            if left.is_some_and(|previous| previous != bank) {
                                s.background_artwork
                                    .as_ref()
                                    .map_or(0, |a| a.bank_change_cost)
                            } else {
                                0
                            }
                        },
                        exact,
                    )
                };
                let mut e = encode(&desired)?;
                let rounds = s
                    .background_artwork
                    .as_ref()
                    .map_or(0, |a| a.smooth_quantization);
                if rounds > 0 {
                    // Smoothed colours lie in the banks the tiles chose, so
                    // encoding them again keeps those banks.
                    let smoothed = smooth_quantized(
                        &desired,
                        &e.pixels,
                        &d.colors,
                        |p| artwork_pixels[p] && !copied_pixels[p] && !exact(p % 256, p / 256),
                        rounds,
                    );
                    let banks: Vec<u8> = e.pixels.iter().map(|v| v / 16).collect();
                    for (p, color) in smoothed.into_iter().enumerate() {
                        if artwork_pixels[p] && !copied_pixels[p] && !exact(p % 256, p / 256) {
                            desired[p] = color;
                        }
                    }
                    e = encode(&desired)?;
                    ensure!(
                        e.pixels.iter().map(|v| v / 16).eq(banks),
                        "smoothed artwork changed tile banks"
                    );
                }
                if s.background_artwork.is_some()
                    || s.parts.iter().any(|part| part.background_mask.is_some())
                {
                    generated_tile_count = Some(crate::art_import::share_flipped_tiles(
                        &mut e,
                        d.tiles.len(),
                        &d.palette,
                    )?);
                    ensure!(
                        screens::render(&e.map, &e.tiles, d.palette.len() / 32)? == e.pixels,
                        "generated screen round trip"
                    );
                }
                (e.map, e.tiles, e.pixels)
            }
            _ => {
                let mut px = d.pixels.clone();
                for p in 0..px.len() {
                    if editable[p] {
                        px[p] = buttons::nearest(&d.palette, desired[p])? as u8;
                    }
                }
                let (map, tiles) = crate::unlock::encode(&px);
                ensure!(tiles.len() / 64 <= 1024, "screen tile overflow");
                (map, tiles, px)
            }
        };
        for (p, (a, b)) in d.pixels.iter().zip(&pixels).enumerate() {
            ensure!(
                !s.exact_text_colors || !text_pixels[p] || desired[p] == d.colors[usize::from(*b)],
                "text color changed during encoding"
            );
            ensure!(
                !s.exact_part_colors || !editable[p] || desired[p] == d.colors[usize::from(*b)],
                "text part color changed during encoding"
            );
            ensure!(
                !copied_pixels[p] || desired[p] == d.colors[usize::from(*b)],
                "source copy color changed during encoding"
            );
            ensure!(
                artwork_pixels[p]
                    || editable[p]
                    || d.colors[usize::from(*a)] == d.colors[usize::from(*b)],
                "protected screen pixel changed"
            );
        }
        for (member, data) in [(s.map, &map), (s.tiles, &tiles)] {
            let stored = narc.members[member];
            let packed = if unpack(stored)? == stored {
                // Raw members keep their stored length; unused tail tiles stay zero.
                ensure!(
                    data.len() <= stored.len(),
                    "raw screen member {member} grew"
                );
                let mut raw = data.clone();
                raw.resize(stored.len(), 0);
                raw
            } else {
                let mut packed = crate::compress::pack(data)?;
                if packed.len() > stored.len() && data.len() <= 4096 {
                    packed = crate::compress::pack_compact(data)?;
                }
                if packed.len() > stored.len()
                    && data.len() <= 32768
                    && s.parts.iter().any(|part| part.background_mask.is_some())
                {
                    // The bounded COMP parser is also byte-exact for masked BG tiles.
                    packed = crate::compress::pack_layout(data)?;
                }
                ensure!(unpack(&packed)? == *data, "screen round trip");
                packed
            };
            ensure!(
                packed.len() <= stored.len(),
                "screen member {member} capacity {} > {}",
                packed.len(),
                stored.len()
            );
            ensure!(
                changes.insert(member, packed).is_none(),
                "member {member} written twice"
            );
        }
        let preview = |px: &[u8]| -> Vec<u8> {
            px.iter()
                .flat_map(|&v| {
                    let c = d.colors[usize::from(v)];
                    [
                        (c[0] * 255 / 31) as u8,
                        (c[1] * 255 / 31) as u8,
                        (c[2] * 255 / 31) as u8,
                        255,
                    ]
                })
                .collect()
        };
        write_png(
            &out.join(format!("{:03}-before.png", s.tiles)),
            256,
            192,
            &preview(&d.pixels),
        )?;
        write_png(
            &out.join(format!("{:03}-after.png", s.tiles)),
            256,
            192,
            &preview(&pixels),
        )?;
        reports.push(json!({"map":s.map,"tiles":s.tiles,"palette":s.palette,"bpp":s.bpp,"source_copies":s.source_copies,"copied_pixel_count":copied_pixels.iter().filter(|&&v| v).count(),"copied_pixel_colors_equal":true,"parts":s.parts.iter().map(|p| {let mut v = json!({"region":p.region,"japanese":p.japanese,"korean":p.korean}); if let Some(o) = &p.overlay { v["overlay"] = serde_json::to_value(o).unwrap_or_default(); } v}).collect::<Vec<_>>()}));
        if s.exact_part_colors {
            let report = reports.last_mut().unwrap();
            report["exact_part_colors"] = json!(true);
            report["exact_part_pixel_count"] = json!(editable.iter().filter(|&&v| v).count());
        }
        if s.exact_text_colors {
            let report = reports.last_mut().unwrap();
            report["exact_text_colors"] = json!(true);
            report["text_pixel_count"] = json!(text_pixels.iter().filter(|&&v| v).count());
        }
        if let Some(art) = &s.background_artwork {
            let report = reports.last_mut().unwrap();
            report["background_artwork"] = serde_json::to_value(art)?;
            report["whole_screen_replaced"] = json!(art.regions.is_empty());
            report["protected_pixel_count"] = json!(
                artwork_pixels
                    .iter()
                    .enumerate()
                    .filter(|&(p, v)| !v && !editable[p])
                    .count()
            );
            report["protected_pixel_colors_equal"] = json!(true);
            report["font_sha256"] = json!(sha(&fs::read(font_path)?));
            report["small_font_sha256"] = json!(sha(&fs::read(small_font)?));
            report["tile_count"] = json!(generated_tile_count);
            report["tile_capacity"] = json!(d.tiles.len() / 32);
        }
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::write(out.join("archive.narc"), &rebuilt)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":tr.archive,"expected_sha256":sha(source),"input":"archive.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let mut report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"screens":reports,"protected":"pixels outside parts keep their colour; palettes and other members unchanged; maps and tiles re-encoded","runtime_verified":false,"human_reviewed":false});
    if tr.screens.iter().any(|s| s.background_artwork.is_some()) {
        report["protected"] = json!(
            "generated backgrounds replace declared regions or the whole screen when regions are omitted; all other pixel colors, palettes and other members are preserved"
        );
    }
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
