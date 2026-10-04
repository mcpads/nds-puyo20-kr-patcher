//! Source-lettering removal for the rule description screens.
//!
//! Only the footprint of the Japanese lettering is rebuilt. Corner flowers and
//! other decoration shared by a screen group are identified by consensus; their
//! visible (warm) pixels never enter the footprint. A hidden petal pixel takes
//! the group's most common petal colour; another hidden pixel copies a screen
//! of the same group that shows it and whose surrounding visible pixels agree,
//! then the group's most common colour where a third of the screens show
//! decoration, else the nearest visible cyan pixel of its own screen. Small
//! decoration fragments left detached are cleared. All are estimates.

use anyhow::Result;

pub(super) const WIDTH: usize = 256;
pub(super) const HEIGHT: usize = 192;
/// Lowest row (exclusive) scanned for source lettering. Japanese descenders and
/// their outlines reach below the adopted text panel.
const SOURCE_BOTTOM: usize = 186;
/// Title capsule interior. The capsule rim and its anti-aliasing stay outside.
pub(super) const TITLE_ZONE: [usize; 4] = [27, 26, 229, 45];
const CYAN: [[i32; 3]; 2] = [[2, 22, 28], [2, 23, 28]];

pub(super) fn warm(c: [i32; 3]) -> bool {
    c[0] > c[2] + 4
}

pub(super) fn in_title(x: usize, y: usize) -> bool {
    let [x0, y0, x1, y1] = TITLE_ZONE;
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

pub(super) fn body_right(id: usize, y: usize) -> usize {
    if id >= 22 {
        244
    } else if id == 18 {
        // Conservative left edge of the tilted demonstration board, measured
        // in the registered screen 18. Its entire silhouette stays protected.
        176.min(160 + y.saturating_sub(64) / 5)
    } else {
        176
    }
}

/// Region scanned for source title lettering: the capsule interior from the
/// rim's inner edge, without the rounded corners.
pub(super) fn in_title_source(x: usize, y: usize) -> bool {
    let corner = (!(27..229).contains(&x)) && !(28..43).contains(&y);
    (25..231).contains(&x) && (26..45).contains(&y) && !corner
}

/// Region scanned for source body lettering.
pub(super) fn in_body_source(id: usize, x: usize, y: usize) -> bool {
    // Hint lines of the source reach x 245, just inside the dotted rim.
    let (left, top, right) = if id >= 22 {
        (16, 56, 246)
    } else {
        (20, 60, body_right(id, y))
    };
    (left..right).contains(&x) && (top..SOURCE_BOTTOM).contains(&y)
}

/// Light letter fills, and dark navy outlines (the body background rays and
/// the letters' mid-blue halo stay above green 15).
pub(super) fn lettering(c: [i32; 3]) -> bool {
    (c[0] >= 10 && c[2] >= c[0] && c[2] >= c[1]) || (c[1] <= 15 && c[2] >= c[0] + 8 && c[2] >= 18)
}

/// Pixels of a screen with no source lettering within 3px: safe to copy into
/// another screen of the group.
pub(super) fn clear_of_lettering(id: usize, s: &[[i32; 3]]) -> Vec<bool> {
    let mut clear = vec![true; WIDTH * HEIGHT];
    for (p, &c) in s.iter().enumerate() {
        let (x, y) = (p % WIDTH, p / WIDTH);
        if in_body_source(id, x, y) && lettering(c) {
            for ny in y.saturating_sub(3)..=(y + 3).min(HEIGHT - 1) {
                for nx in x.saturating_sub(3)..=(x + 3).min(WIDTH - 1) {
                    clear[ny * WIDTH + nx] = false;
                }
            }
        }
    }
    clear
}

/// Decoration (corner flowers, dotted rim): pixels warm in at least half of
/// the group screens clear of lettering there. Letters painted over a flower
/// in some screens do not hide it from the consensus.
pub(super) fn decoration(group: &[&[[i32; 3]]], clear: &[&[bool]]) -> Vec<bool> {
    (0..WIDTH * HEIGHT)
        .map(|p| {
            let seen: Vec<_> = (0..group.len()).filter(|&k| clear[k][p]).collect();
            seen.len() >= 3 && 2 * seen.iter().filter(|&&k| warm(group[k][p])).count() >= seen.len()
        })
        .collect()
}

pub(super) struct Footprint {
    pub mask: Vec<bool>,
    pub title_background: [i32; 3],
}

fn most_common(colors: impl Iterator<Item = [i32; 3]>) -> Option<[i32; 3]> {
    let mut counts = std::collections::BTreeMap::new();
    for c in colors {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)
}

/// Source lettering footprint: title letters (anything but the capsule colour,
/// grown 1px, unless the source title is kept) and body letters (dark navy or
/// light cool colours, grown 2px), excluding visible group decoration and the dark
/// demonstration board on the right (within 2px of a pixel darker than RGB5
/// sum 30 at x >= 150).
pub(super) fn footprint(
    id: usize,
    original: &[[i32; 3]],
    decoration: &[bool],
    keep_title: bool,
) -> Result<Footprint> {
    // Capsule colour: the most common colour on the rim of the title zone.
    let [x0, y0, x1, y1] = TITLE_ZONE;
    let title_background = most_common(
        (0..WIDTH * HEIGHT)
            .filter(|&p| {
                let (x, y) = (p % WIDTH, p / WIDTH);
                in_title(x, y) && (x == x0 || x == x1 - 1 || y == y0 || y == y1 - 1)
            })
            .map(|p| original[p]),
    )
    .ok_or_else(|| anyhow::anyhow!("empty title zone"))?;
    let mut board = vec![false; WIDTH * HEIGHT];
    for (p, color) in original.iter().enumerate() {
        let (x, y) = (p % WIDTH, p / WIDTH);
        if x >= 150 && color.iter().sum::<i32>() < 30 {
            for ny in y.saturating_sub(2)..=(y + 2).min(HEIGHT - 1) {
                for nx in x.saturating_sub(2)..=(x + 2).min(WIDTH - 1) {
                    board[ny * WIDTH + nx] = true;
                }
            }
        }
    }
    let body = |x: usize, y: usize| {
        let p = y * WIDTH + x;
        // A visible petal (warm here and in half the group) stays; letters
        // painted over a petal are cool here and are rebuilt.
        in_body_source(id, x, y) && !(decoration[p] && warm(original[p])) && !board[p]
    };
    let mut mask = vec![false; WIDTH * HEIGHT];
    let mut grow = |x: usize, y: usize, r: usize, inside: &dyn Fn(usize, usize) -> bool| {
        for ny in y.saturating_sub(r)..=(y + r).min(HEIGHT - 1) {
            for nx in x.saturating_sub(r)..=(x + r).min(WIDTH - 1) {
                if inside(nx, ny) {
                    mask[ny * WIDTH + nx] = true;
                }
            }
        }
    };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let p = y * WIDTH + x;
            let c = original[p];
            if !keep_title && in_title_source(x, y) && c != title_background {
                grow(x, y, 1, &in_title_source);
            }
            if body(x, y) && lettering(c) {
                grow(x, y, 2, &body);
            }
        }
    }
    Ok(Footprint {
        mask,
        title_background,
    })
}

pub(super) struct Restored {
    pub colors: Vec<[i32; 3]>,
    /// Nearest visible cyan pixel for every rebuilt body pixel; used where the
    /// tile's bank cannot show the first estimate.
    pub fallback: Vec<[i32; 3]>,
    /// Visible source pixels of decoration fragments cleared with the
    /// footprint (they must join the footprint for the encoder).
    pub cleared: Vec<usize>,
    pub cross_screen: usize,
    pub consensus: usize,
    pub nearest: usize,
}

/// Petal, dot or their blends with the cyan background (red above blue).
pub(super) fn petal(c: [i32; 3]) -> bool {
    c[0] > c[2]
}

/// The body's cyan ray colours and their close neighbours.
pub(super) fn background_like(c: [i32; 3]) -> bool {
    c[0] <= 4 && c[1] >= 20 && c[2] >= 26
}

/// Largest warm fragment (pixels) treated as a detached petal tip or dot.
const FRAGMENT: usize = 40;

pub(super) fn nearest_cyan(
    original: &[[i32; 3]],
    mask: &[bool],
    x: usize,
    y: usize,
) -> Option<[i32; 3]> {
    for radius in 1usize..=64 {
        let mut candidates = Vec::new();
        for ny in y.saturating_sub(radius)..=(y + radius).min(HEIGHT - 1) {
            for nx in x.saturating_sub(radius)..=(x + radius).min(WIDTH - 1) {
                if nx.abs_diff(x).max(ny.abs_diff(y)) != radius {
                    continue;
                }
                let q = ny * WIDTH + nx;
                if !mask[q] && CYAN.contains(&original[q]) {
                    candidates.push((nx.abs_diff(x).pow(2) + ny.abs_diff(y).pow(2), q));
                }
            }
        }
        if let Some((_, q)) = candidates.into_iter().min() {
            return Some(original[q]);
        }
    }
    None
}

/// Rebuild the footprint of screen `own` inside its group.
pub(super) fn restore(
    own: usize,
    id: usize,
    originals: &[&[[i32; 3]]],
    masks: &[&[bool]],
    clear: &[&[bool]],
    decoration: &[bool],
    title_background: [i32; 3],
) -> Result<Restored> {
    let original = originals[own];
    let mask = masks[own];
    let mut colors = original.to_vec();
    let mut fallback = original.to_vec();
    // Pixels rebuilt with the plain cyan fallback (decoration cut off there).
    let mut cut = vec![false; WIDTH * HEIGHT];
    let (mut cross_screen, mut consensus, mut nearest) = (0, 0, 0);
    for p in 0..WIDTH * HEIGHT {
        if !mask[p] {
            continue;
        }
        let (x, y) = (p % WIDTH, p / WIDTH);
        if in_title_source(x, y) {
            colors[p] = title_background;
            fallback[p] = title_background;
            continue;
        }
        fallback[p] = nearest_cyan(original, mask, x, y)
            .ok_or_else(|| anyhow::anyhow!("screen {id} missing background donor at {x},{y}"))?;
        // A petal hidden under the letters where most screens of the group
        // that show this pixel show petal: their most common colour, so the
        // petal continues whole.
        if decoration[p] {
            let petal = most_common(
                (0..originals.len())
                    .filter(|&t| t != own && !masks[t][p] && warm(originals[t][p]))
                    .map(|t| originals[t][p]),
            );
            if let Some(c) = petal {
                colors[p] = c;
                consensus += 1;
                continue;
            }
        }
        // Another screen of the group that shows this pixel and agrees with
        // this screen (within palette jitter, 2 per channel) on at least 90% of
        // the pixels both show in the surrounding 9x9 window, 12 or more. The
        // corner flowers, their dots and the rays repeat across the group, so
        // the donor continues them with their own anti-aliasing.
        let mut best: Option<(usize, usize, usize)> = None;
        for (t, other) in originals.iter().enumerate() {
            if t == own || !clear[t][p] {
                continue;
            }
            let (mut n, mut agree) = (0usize, 0usize);
            for ny in y.saturating_sub(4)..=(y + 4).min(HEIGHT - 1) {
                for nx in x.saturating_sub(4)..=(x + 4).min(WIDTH - 1) {
                    let q = ny * WIDTH + nx;
                    if mask[q] || !clear[t][q] {
                        continue;
                    }
                    n += 1;
                    agree += usize::from((0..3).all(|k| (original[q][k] - other[q][k]).abs() <= 2));
                }
            }
            if n >= 12
                && 10 * agree >= 9 * n
                && best.is_none_or(|(ba, bn, _)| {
                    agree * bn > ba * n || (agree * bn == ba * n && n > bn)
                })
            {
                best = Some((agree, n, t));
            }
        }
        if let Some((_, _, t)) = best {
            colors[p] = originals[t][p];
            cross_screen += 1;
            continue;
        }
        // Decoration without enough visible context (a flower dot or petal
        // wholly under the letters): when at least 6 other screens show this
        // pixel and a third of them show petal or dot there, take their most
        // common colour, anti-aliased edges included.
        let visible: Vec<[i32; 3]> = (0..originals.len())
            .filter(|&t| t != own && clear[t][p])
            .map(|t| originals[t][p])
            .collect();
        if visible.len() >= 6 && 3 * visible.iter().filter(|&&c| warm(c)).count() >= visible.len() {
            colors[p] = most_common(visible.into_iter()).unwrap();
            consensus += 1;
            continue;
        }
        colors[p] = fallback[p];
        cut[p] = true;
        nearest += 1;
    }
    let mut cleared = Vec::new();
    for q in clear_fragments(
        id,
        &mut colors,
        original,
        mask,
        &cut,
        Keep::GroupDecoration(decoration),
        &[],
    )? {
        fallback[q] = colors[q];
        if !mask[q] {
            cleared.push(q);
        }
    }
    Ok(Restored {
        colors,
        fallback,
        cleared,
        cross_screen,
        consensus,
        nearest,
    })
}

/// The group's decoration as the screens clear of lettering show it: per pixel
/// the most common colour, when two thirds of the screens that show the pixel
/// agree with it within palette jitter (2 per channel). Screens whose whole
/// 8x8 tile is clear come first (two needed, else three showing the pixel): a tile holding source letters
/// uses the letter bank, which shows a petal only in approximate colours. The
/// hint screens share their corner flowers and rim dots pixel for pixel, so
/// every petal and dot some screen shows whole is known with its own
/// anti-aliasing.
pub(super) fn shown_decoration(group: &[&[[i32; 3]]], clear: &[&[bool]]) -> Vec<Option<[i32; 3]>> {
    let tile_clear: Vec<Vec<bool>> = clear
        .iter()
        .map(|c| {
            (0..(WIDTH / 8) * (HEIGHT / 8))
                .map(|t| {
                    (0..64).all(|i| {
                        c[(t / (WIDTH / 8) * 8 + i / 8) * WIDTH + t % (WIDTH / 8) * 8 + i % 8]
                    })
                })
                .collect()
        })
        .collect();
    (0..WIDTH * HEIGHT)
        .map(|p| {
            let tile = p / WIDTH / 8 * (WIDTH / 8) + p % WIDTH / 8;
            let tiled: Vec<[i32; 3]> = (0..group.len())
                .filter(|&k| tile_clear[k][tile])
                .map(|k| group[k][p])
                .collect();
            // Two screens showing the tile in the flower bank suffice; pixel
            // views from letter-bank tiles need three.
            let (seen, least) = if tiled.len() >= 2 {
                (tiled, 2)
            } else {
                let pixel: Vec<[i32; 3]> = (0..group.len())
                    .filter(|&k| clear[k][p])
                    .map(|k| group[k][p])
                    .collect();
                (pixel, 3)
            };
            let c = most_common(seen.iter().copied())?;
            let agree = seen
                .iter()
                .filter(|s| (0..3).all(|k| (s[k] - c[k]).abs() <= 2))
                .count();
            (seen.len() >= least && 3 * agree >= 2 * seen.len()).then_some(c)
        })
        .collect()
}

/// Complete the flowers and dots of `shown` (the group's decoration) over the
/// rebuilt pixels of a prepared screen (`rebuilt`: the source footprint and
/// the pixels earlier passes changed), so a petal the source letters hid, or
/// earlier passes cut at a tile or a glyph, keeps its whole outline instead of
/// a forked or stepped end, and a dot the source letters clipped comes back
/// round. Each petal component of `shown` is copied with its anti-aliased rim
/// and the darker halo it casts on the rays where a rebuilt pixel differs from
/// it beyond palette jitter. A small component (a dot or a detached tip, at
/// most FRAGMENT pixels) within 2px of a new glyph (`near_glyph`), or with an
/// outline the group does not agree on, stays out: a dot peeking beside a
/// letter reads as a fragment. The corner flowers run on under the glyphs as
/// in the source. New lettering (`locked`) is never touched. Returns the
/// changed pixels.
pub(super) fn complete_decoration(
    id: usize,
    colors: &mut [[i32; 3]],
    shown: &[Option<[i32; 3]>],
    rebuilt: &[bool],
    near_glyph: &[bool],
    locked: &[bool],
) -> Vec<usize> {
    let is_petal = |q: usize| shown[q].is_some_and(petal);
    let mut seen = vec![false; WIDTH * HEIGHT];
    let mut completed = vec![false; WIDTH * HEIGHT];
    let mut changed = Vec::new();
    for start in 0..WIDTH * HEIGHT {
        if seen[start] || !is_petal(start) {
            continue;
        }
        let mut component = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < component.len() {
            let (x, y) = (component[k] % WIDTH, component[k] / WIDTH);
            k += 1;
            for ny in y.saturating_sub(1)..=(y + 1).min(HEIGHT - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(WIDTH - 1) {
                    let r = ny * WIDTH + nx;
                    if !seen[r] && is_petal(r) {
                        seen[r] = true;
                        component.push(r);
                    }
                }
            }
        }
        let mut member = vec![false; WIDTH * HEIGHT];
        for &q in &component {
            member[q] = true;
        }
        // Rim pixels next to another component's petal belong to that one.
        let foreign = |r: usize| {
            let (x, y) = (r % WIDTH, r / WIDTH);
            (y.saturating_sub(1)..=(y + 1).min(HEIGHT - 1)).any(|ny| {
                (x.saturating_sub(1)..=(x + 1).min(WIDTH - 1)).any(|nx| {
                    let t = ny * WIDTH + nx;
                    is_petal(t) && !member[t]
                })
            })
        };
        let mut region = component.clone();
        let mut in_region = member.clone();
        for &q in &component {
            let (x, y) = (q % WIDTH, q / WIDTH);
            for ny in y.saturating_sub(2)..=(y + 2).min(HEIGHT - 1) {
                for nx in x.saturating_sub(2)..=(x + 2).min(WIDTH - 1) {
                    let r = ny * WIDTH + nx;
                    let Some(c) = shown[r] else { continue };
                    let near = nx.abs_diff(x) <= 1 && ny.abs_diff(y) <= 1;
                    let take = if near {
                        !background_like(c)
                    } else {
                        !CYAN.contains(&c) && !petal(c) && !background_like(c)
                    };
                    if take && !in_region[r] && !foreign(r) {
                        in_region[r] = true;
                        region.push(r);
                    }
                }
            }
        }
        // A dot whose outline the group does not agree on would come back
        // with a flat side.
        let unknown_rim = component.iter().any(|&q| {
            let (x, y) = (q % WIDTH, q / WIDTH);
            (y.saturating_sub(1)..=(y + 1).min(HEIGHT - 1)).any(|ny| {
                (x.saturating_sub(1)..=(x + 1).min(WIDTH - 1))
                    .any(|nx| shown[ny * WIDTH + nx].is_none())
            })
        });
        if component.len() <= FRAGMENT && (unknown_rim || region.iter().any(|&q| near_glyph[q])) {
            continue;
        }
        for &q in &region {
            let (x, y) = (q % WIDTH, q / WIDTH);
            let c = shown[q].unwrap();
            completed[q] = true;
            // Within palette jitter the rebuilt pixel already shows the
            // group's colour.
            let differs = (0..3).any(|k| (colors[q][k] - c[k]).abs() > 2);
            if rebuilt[q] && differs && !locked[q] && in_body_source(id, x, y) {
                colors[q] = c;
                changed.push(q);
            }
        }
    }
    // The rest of the tiles changed above takes the group's colours: a
    // source pixel in a letter bank's colour (an approximate petal or ray,
    // even one within jitter) would hold the tile to that bank and show the
    // completed petal in its approximate colours. Outside the completed
    // components only pixels of the same kind (petal, or plain ray) change,
    // so the rim of a skipped dot stays out.
    let mut tiles = vec![false; (WIDTH / 8) * (HEIGHT / 8)];
    for &q in &changed {
        tiles[q / WIDTH / 8 * (WIDTH / 8) + q % WIDTH / 8] = true;
    }
    for q in 0..WIDTH * HEIGHT {
        let (x, y) = (q % WIDTH, q / WIDTH);
        let Some(c) = shown[q] else { continue };
        let kind = completed[q]
            || if petal(c) {
                petal(colors[q])
            } else {
                background_like(c) && !petal(colors[q])
            };
        if tiles[y / 8 * (WIDTH / 8) + x / 8]
            && colors[q] != c
            && kind
            && !locked[q]
            && in_body_source(id, x, y)
        {
            colors[q] = c;
            changed.push(q);
        }
    }
    changed
}

/// Which small petal components `clear_fragments` leaves in place.
#[derive(Clone, Copy)]
pub(super) enum Keep<'a> {
    /// Components matching the group's decoration (80% of their pixels) and
    /// compact whole dots.
    GroupDecoration(&'a [bool]),
    /// Compact whole dots made only of visible source pixels.
    VisibleDots,
    /// Nothing: every small component goes.
    Nothing,
    /// Everything but specks of at most four pixels.
    Specks,
}

/// Petal tips and flower dots that were partly hidden by the letters and could
/// not be completed would float detached. A small petal component (at most
/// FRAGMENT pixels, inside the body away from the dotted rim, excluding
/// `locked` pixels) that touches a `cut` pixel (rebuilt with plain cyan) and
/// becomes the nearest visible cyan together with its anti-aliased rim. What
/// stays depends on `keep`. Returns the changed pixels.
pub(super) fn clear_fragments(
    id: usize,
    colors: &mut [[i32; 3]],
    original: &[[i32; 3]],
    mask: &[bool],
    cut: &[bool],
    keep: Keep,
    locked: &[bool],
) -> Result<Vec<usize>> {
    let locked = |q: usize| locked.get(q).copied().unwrap_or(false);
    let rim = |x: usize, y: usize| !(14..242).contains(&x) || y >= 180;
    let mut seen = vec![false; WIDTH * HEIGHT];
    let mut changed = Vec::new();
    for start in 0..WIDTH * HEIGHT {
        if seen[start]
            || locked(start)
            || !petal(colors[start])
            || !in_body_source(id, start % WIDTH, start / WIDTH)
        {
            continue;
        }
        let mut component = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < component.len() {
            let q = component[k];
            k += 1;
            let (x, y) = (q % WIDTH, q / WIDTH);
            for ny in y.saturating_sub(1)..=(y + 1).min(HEIGHT - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(WIDTH - 1) {
                    let r = ny * WIDTH + nx;
                    if !seen[r] && !locked(r) && petal(colors[r]) {
                        seen[r] = true;
                        component.push(r);
                    }
                }
            }
        }
        let touches = component.iter().any(|&q| {
            let (x, y) = (q % WIDTH, q / WIDTH);
            [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)]
                .iter()
                .any(|&(dx, dy)| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    (0..WIDTH as i32).contains(&nx)
                        && (0..HEIGHT as i32).contains(&ny)
                        && cut[ny as usize * WIDTH + nx as usize]
                })
        });
        let matches_group = match keep {
            Keep::GroupDecoration(decoration) => {
                5 * component.iter().filter(|&&q| decoration[q]).count() >= 4 * component.len()
            }
            _ => false,
        };
        // A whole rim dot (compact, at least 3x3 and 9 pixels) stays even when
        // letters stood next to it.
        let (xs, ys): (Vec<usize>, Vec<usize>) =
            component.iter().map(|&q| (q % WIDTH, q / WIDTH)).unzip();
        let (w, h) = (
            xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1,
            ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1,
        );
        let whole_dot =
            w >= 3 && h >= 3 && component.len() >= 9 && 5 * component.len() >= 3 * w * h;
        if component.len() > FRAGMENT
            || (matches!(keep, Keep::Specks) && component.len() > 4)
            || !touches
            || matches_group
            || (whole_dot
                && match keep {
                    Keep::GroupDecoration(_) => true,
                    Keep::VisibleDots => component.iter().all(|&q| !mask[q]),
                    Keep::Nothing | Keep::Specks => false,
                })
            || component
                .iter()
                .any(|&q| rim(q % WIDTH, q / WIDTH) || !in_body_source(id, q % WIDTH, q / WIDTH))
        {
            continue;
        }
        // The fragment, its anti-aliased rim (non-background neighbours) and
        // the darker halo ring a dot casts on the rays (non-ray colours within
        // 2px), so no outline of the removed piece stays.
        let mut region = component.clone();
        for &q in &component {
            let (x, y) = (q % WIDTH, q / WIDTH);
            for ny in y.saturating_sub(2)..=(y + 2).min(HEIGHT - 1) {
                for nx in x.saturating_sub(2)..=(x + 2).min(WIDTH - 1) {
                    let r = ny * WIDTH + nx;
                    let near = nx.abs_diff(x) <= 1 && ny.abs_diff(y) <= 1;
                    let take = if near {
                        !background_like(colors[r])
                    } else {
                        !CYAN.contains(&colors[r]) && !petal(colors[r])
                    };
                    if take && !locked(r) && !region.contains(&r) && in_body_source(id, nx, ny) {
                        region.push(r);
                    }
                }
            }
        }
        for &q in &region {
            colors[q] = nearest_cyan(original, mask, q % WIDTH, q / WIDTH)
                .ok_or_else(|| anyhow::anyhow!("screen {id} missing background donor"))?;
            if !changed.contains(&q) {
                changed.push(q);
            }
        }
    }
    Ok(changed)
}
