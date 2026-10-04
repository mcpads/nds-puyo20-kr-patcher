//! Convert frozen generated images and font-authored slots to existing NDS members.
pub(crate) mod game_over;
use crate::{
    art_pixels, assets::json_file, battle_ui, format::*, graphics::write_png, screens, titles,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    state: String,
    entries: Vec<Entry>,
    #[serde(default)]
    game_over_layout: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    archive: String,
    archive_sha256: String,
    member: usize,
    source_sha256: String,
    palette: usize,
    palette_sha256: String,
    width: usize,
    height: usize,
    format: String,
    #[serde(default)]
    alpha_values: Vec<usize>,
    #[serde(default)]
    palette_indices: Vec<usize>,
    #[serde(default)]
    bank_change_cost: i64,
    #[serde(default)]
    map: Option<usize>,
    #[serde(default)]
    map_sha256: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    image_sha256: Option<String>,
    #[serde(default)]
    image_sampling: Option<String>,
    /// Translate reduced alpha lettering in native pixels without resampling.
    #[serde(default)]
    image_offset: Option<[i32; 2]>,
    /// Add one native-pixel ring to transparent I4 lettering, keeping existing ink.
    #[serde(default)]
    outer_outline_index: Option<usize>,
    #[serde(default)]
    image_matte: Option<art_pixels::ProductionMatte>,
    #[serde(default)]
    image_background: Option<SolidBackground>,
    /// Explicit glyph cell from a generated sheet, never an inferred source background cut.
    #[serde(default)]
    image_region: Option<[usize; 4]>,
    /// Reduce one complete title once, then select this sprite's native rectangle.
    #[serde(default)]
    image_canvas_crop: Option<CanvasCrop>,
    #[serde(default)]
    image_cells: Vec<Option<[usize; 4]>>,
    #[serde(default)]
    image_pieces: Vec<ImagePiece>,
    #[serde(default)]
    text_overlays: Vec<TextOverlay>,
    /// Font-drawn display lettering used in place of a generated image.
    #[serde(default)]
    lettering: Option<Lettering>,
    /// Recolour reduced opaque pixels near `from` before palette conversion.
    #[serde(default)]
    rgb_remap: Vec<RgbRemap>,
    /// Rings added around the reduced lettering silhouette, innermost first.
    #[serde(default)]
    outer_rim: Vec<RimRing>,
    #[serde(default)]
    source_restores: Vec<SourceRestore>,
    #[serde(default)]
    font: Option<String>,
    #[serde(default)]
    font_sha256: Option<String>,
    #[serde(default)]
    slots: Vec<String>,
    #[serde(default)]
    size: f32,
    #[serde(default)]
    colors: [[u8; 3]; 3],
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SolidBackground {
    region: [usize; 4],
    rgb555: u16,
    /// Exact source corner colors when lettering touches a rectangle corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_corner_rgb555: Option<[u16; 4]>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TextOverlay {
    region: [usize; 4],
    text: String,
    font: String,
    font_sha256: String,
    size: f32,
    colors: [[u8; 3]; 3],
    /// `cw` lays the text out horizontally and turns it 90° clockwise into the
    /// region, like rotated `image_pieces`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotate: Option<String>,
}
/// Display lettering drawn at native size: each line's face runs through the
/// `fill` colour stops from its top ink row to its bottom one, inside a round
/// `outline`, over a lower-right `depth` extrusion and an optional translucent
/// `halo`. Glyphs use grid-fitted area rasterisation so stems keep one width.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Lettering {
    font: String,
    font_sha256: String,
    lines: Vec<LetteringLine>,
    fill: Vec<[u8; 3]>,
    outline: [u8; 3],
    outline_width: usize,
    depth: [u8; 3],
    depth_steps: usize,
    /// One-pixel lower-right drop of the face inside the outline.
    #[serde(default)]
    shadow: Option<[u8; 3]>,
    #[serde(default)]
    halo: Option<RimRing>,
    /// Canvas `[width, height]` when it differs from the texture, e.g. a
    /// horizontal word that `image_pieces` turn into a vertical texture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    canvas: Option<[usize; 2]>,
    /// Opaque colour under the whole canvas, so pieces replace a plate
    /// interior instead of leaving transparent texels around the letters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<[u8; 3]>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LetteringLine {
    text: String,
    size: usize,
    /// Left and top native pixel of the line's face ink, or
    #[serde(default)]
    left: Option<usize>,
    #[serde(default)]
    top: Option<usize>,
    /// a cell `[x0, y0, x1, y1)` in which the face ink is centred.
    #[serde(default)]
    cell: Option<[usize; 4]>,
}

fn draw_lettering(l: &Lettering, width: usize, height: usize) -> Result<art_pixels::Image> {
    ensure!(
        !l.lines.is_empty() && l.fill.len() >= 2 && (1..=3).contains(&l.outline_width),
        "lettering style"
    );
    ensure!(l.depth_steps <= 4, "lettering depth");
    let bytes = fs::read(&l.font)?;
    ensure!(sha(&bytes) == l.font_sha256, "lettering font identity");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|v| anyhow::anyhow!(v))?;
    let mut face: BTreeMap<(i32, i32), [u8; 3]> = BTreeMap::new();
    for line in &l.lines {
        let (rect, left, top) = match (line.cell, line.left, line.top) {
            (Some(cell), None, None) => (cell, None, None),
            (None, Some(left), Some(top)) => ([0, 0, width, height], Some(left as i32), Some(top)),
            _ => anyhow::bail!("lettering line needs a cell or a left/top origin"),
        };
        ensure!(
            rect[0] < rect[2] && rect[1] < rect[3] && rect[2] <= width && rect[3] <= height,
            "lettering cell outside canvas"
        );
        let glyphs = crate::labels::ink(
            &font,
            &line.text,
            line.size,
            rect,
            left,
            crate::labels::Spacing {
                scale_x: Some(1.0),
                grid_fit: true,
                top,
                ..Default::default()
            },
            None,
        )?;
        let top = glyphs.iter().map(|p| p.1).min().unwrap_or(0);
        let bottom = glyphs.iter().map(|p| p.1).max().unwrap_or(0);
        let span = (bottom - top).max(1) as f64;
        let stops = (l.fill.len() - 1) as f64;
        for &(x, y) in &glyphs {
            let t = f64::from(y - top) / span * stops;
            face.insert((x, y), l.fill[(t.round() as usize).min(l.fill.len() - 1)]);
        }
    }
    let shadow: BTreeSet<(i32, i32)> = if l.shadow.is_some() {
        face.keys()
            .map(|&(x, y)| (x + 1, y + 1))
            .filter(|p| !face.contains_key(p))
            .collect()
    } else {
        BTreeSet::new()
    };
    let inner: BTreeSet<(i32, i32)> = face.keys().copied().chain(shadow.iter().copied()).collect();
    let r = l.outline_width as i32;
    let mut stroke = BTreeSet::new();
    for &(x, y) in &inner {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r + 1 && !inner.contains(&(x + dx, y + dy)) {
                    stroke.insert((x + dx, y + dy));
                }
            }
        }
    }
    let body: BTreeSet<(i32, i32)> = inner.iter().chain(&stroke).copied().collect();
    let mut depth = BTreeSet::new();
    for step in 1..=l.depth_steps as i32 {
        for &(x, y) in &body {
            let p = (x + step, y + step);
            if !body.contains(&p) {
                depth.insert(p);
            }
        }
    }
    let solid: BTreeSet<(i32, i32)> = body.iter().chain(&depth).copied().collect();
    let mut halo = BTreeSet::new();
    if l.halo.is_some() {
        for &(x, y) in &solid {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                if !solid.contains(&(x + dx, y + dy)) {
                    halo.insert((x + dx, y + dy));
                }
            }
        }
    }
    let mut rgba = match l.background {
        Some(c) => [c[0], c[1], c[2], 255].repeat(width * height),
        None => vec![0u8; width * height * 4],
    };
    let mut put = |(x, y): (i32, i32), c: [u8; 3], a: u8| -> Result<()> {
        ensure!(
            x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height,
            "lettering leaves the canvas at ({x},{y})"
        );
        let p = (y as usize * width + x as usize) * 4;
        rgba[p..p + 4].copy_from_slice(&[c[0], c[1], c[2], a]);
        Ok(())
    };
    if let Some(h) = &l.halo {
        for &p in &halo {
            put(p, h.rgb, h.alpha)?;
        }
    }
    for &p in &depth {
        put(p, l.depth, 255)?;
    }
    for &p in &stroke {
        put(p, l.outline, 255)?;
    }
    if let Some(c) = l.shadow {
        for &p in &shadow {
            put(p, c, 255)?;
        }
    }
    for (&p, &c) in &face {
        put(p, c, 255)?;
    }
    Ok(art_pixels::Image {
        width,
        height,
        rgba,
    })
}

/// Pixels whose RGB lies within `tolerance` of `from` on every channel become `to`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RgbRemap {
    from: [u8; 3],
    to: [u8; 3],
    tolerance: u8,
}
/// One pixel of colour and alpha around every pixel of the silhouette so far.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RimRing {
    rgb: [u8; 3],
    alpha: u8,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceRestore {
    region: [usize; 4],
    /// Destination origin for reconstructing an occluded glyph from intact source pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<[usize; 2]>,
    #[serde(default)]
    exclude_indices: Vec<u8>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CanvasCrop {
    width: usize,
    height: usize,
    #[serde(default)]
    content_region: Option<[usize; 4]>,
    #[serde(default)]
    shared_overlap_colors: bool,
    region: [usize; 4],
}
struct CanvasPalette {
    region: [usize; 4],
    colors: Vec<usize>,
}
fn canvas_identity(e: &Entry, crop: &CanvasCrop) -> Result<String> {
    Ok(sha(&serde_json::to_vec(&json!({
        "archive":e.archive,"image":e.image_sha256,"source_region":e.image_region,
        "width":crop.width,"height":crop.height,"content_region":crop.content_region,
        "matte":e.image_matte,"background":e.image_background
    }))?))
}
struct CanvasPreview {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    conflicting_overlaps: usize,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ImagePiece {
    /// Generated sheet rectangle; null deliberately clears the target fragment.
    source: Option<[usize; 4]>,
    target: [usize; 4],
    /// False fills the target with an explicitly selected complete generated panel.
    #[serde(default)]
    image_fit: Option<bool>,
    /// Consumer storage orientation, applied after native-size reduction.
    #[serde(default)]
    rotate: Option<String>,
}
fn piece_mask(pieces: &[ImagePiece], width: usize, height: usize) -> Result<Vec<bool>> {
    let mut mask = vec![false; width * height];
    for piece in pieces {
        ensure!(
            piece.source.is_some() || (piece.image_fit.is_none() && piece.rotate.is_none()),
            "panel fit requires a source region"
        );
        let [x0, y0, x1, y1] = piece.target;
        ensure!(
            x0 < x1 && y0 < y1 && x1 <= width && y1 <= height,
            "piece target outside atlas"
        );
        for y in y0..y1 {
            for x in x0..x1 {
                ensure!(!mask[y * width + x], "overlapping artwork pieces");
                mask[y * width + x] = true;
            }
        }
    }
    Ok(mask)
}
fn payload(n: &Narc, id: usize) -> Result<Vec<u8>> {
    unpack(
        n.members
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("member missing"))?,
    )
}

/// NDS map flip bits let mirrored tiles share storage without changing a pixel.
pub(crate) fn share_flipped_tiles(
    enc: &mut screens::EncodedScreen,
    capacity: usize,
    palette: &[u8],
) -> Result<usize> {
    let mut dictionary: BTreeMap<Vec<u8>, (usize, usize)> = BTreeMap::new();
    let mut tiles = Vec::new();
    for cell in 0..768 {
        let attr = u16le(&enc.map, cell * 2)?;
        ensure!(attr & 0xc00 == 0, "expected unflipped freshly encoded BG");
        let raw = &enc.tiles[(attr & 1023) * 32..][..32];
        let px = crate::battle_ui::indices(raw);
        let mut variants = Vec::new();
        for flags in [0, 0x400, 0x800, 0xc00] {
            let mut p = vec![0; 64];
            for y in 0..8 {
                for x in 0..8 {
                    let sx = if flags & 0x400 != 0 { 7 - x } else { x };
                    let sy = if flags & 0x800 != 0 { 7 - y } else { y };
                    p[y * 8 + x] = px[sy * 8 + sx];
                }
            }
            variants.push((crate::battle_ui::pack(&p)?, flags));
        }
        let (id, flags) = if let Some(&binding) = dictionary.get(raw) {
            binding
        } else {
            let id = tiles.len() / 32;
            tiles.extend_from_slice(raw);
            for (bytes, flags) in variants {
                dictionary.entry(bytes).or_insert((id, flags));
            }
            (id, 0)
        };
        let word = (attr & 0xf000) | flags | id;
        enc.map[cell * 2..cell * 2 + 2].copy_from_slice(&(word as u16).to_le_bytes());
    }
    ensure!(
        tiles.len() <= capacity,
        "lossless BG tile dictionary exceeds capacity: {} > {} tiles",
        tiles.len() / 32,
        capacity / 32
    );
    let tile_count = tiles.len() / 32;
    tiles.resize(capacity, 0);
    let next = screens::render(&enc.map, &tiles, palette.len() / 32)?;
    ensure!(next == enc.pixels, "flipped tile roundtrip differs");
    enc.pixels = next;
    enc.tiles = tiles;
    enc.tile_count = tile_count;
    Ok(tile_count)
}

fn pack(n: &Narc, id: usize, data: &[u8]) -> Result<Vec<u8>> {
    let old = n.members[id];
    let mut b = if old.starts_with(b"COMP") {
        crate::compress::pack(data)?
    } else {
        data.to_vec()
    };
    if b.len() > old.len() && data.len() <= 4096 && old.starts_with(b"COMP") {
        b = crate::compress::pack_compact(data)?;
    }
    if b.len() > old.len() && (4097..=32768).contains(&data.len()) && old.starts_with(b"COMP") {
        let compact = crate::compress::pack_layout(data)?;
        if compact.len() < b.len() {
            b = compact;
        }
    }
    ensure!(
        b.len() <= old.len(),
        "member {id} capacity: {} > {}",
        b.len(),
        old.len()
    );
    ensure!(
        unpack_halfword(&b)? == data,
        "member {id} halfword roundtrip"
    );
    Ok(b)
}
pub fn prepare(rom: &Rom, spec: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let specbytes = fs::read(spec)?;
    let tr: Input = serde_json::from_slice(&specbytes)?;
    ensure!(
        tr.state == "development_art_draft" && !tr.entries.is_empty(),
        "artwork draft required"
    );
    fs::create_dir_all(out)?;
    let mut archives: BTreeMap<String, BTreeMap<usize, Vec<u8>>> = BTreeMap::new();
    let mut records = Vec::new();
    let mut canvas_previews: BTreeMap<String, CanvasPreview> = BTreeMap::new();
    let mut canvas_palettes: BTreeMap<String, Vec<CanvasPalette>> = BTreeMap::new();
    for e in &tr.entries {
        if let Some(crop) = &e.image_canvas_crop {
            let n = Narc::parse(rom.data(rom.file(&e.archive)?))?;
            let pal = payload(&n, e.palette)?;
            ensure!(sha(&pal) == e.palette_sha256, "canvas palette identity");
            let indices: Vec<_> = if e.palette_indices.is_empty() {
                (1..pal.len() / 2).collect()
            } else {
                e.palette_indices.clone()
            };
            let colors = indices
                .into_iter()
                .map(|i| {
                    ensure!(i > 0 && i < pal.len() / 2, "canvas palette index");
                    u16le(&pal, i * 2)
                })
                .collect::<Result<Vec<_>>>()?;
            canvas_palettes
                .entry(canvas_identity(e, crop)?)
                .or_default()
                .push(CanvasPalette {
                    region: crop.region,
                    colors,
                });
        }
    }
    let mut bg_records = Vec::new();
    let preview_columns = tr.entries.len().min(4);
    let preview_cell_width = tr.entries.iter().map(|e| e.width).max().unwrap() + 4;
    let preview_cell_height = tr.entries.iter().map(|e| e.height).max().unwrap() + 4;
    let preview_width = preview_columns * preview_cell_width;
    let preview_height = tr.entries.len().div_ceil(preview_columns) * preview_cell_height;
    let mut preview_sheet = [20, 20, 20, 255].repeat(preview_width * preview_height);
    let mut preview_regions = Vec::new();
    for e in tr.entries {
        ensure!(
            (e.text_overlays.is_empty() && e.source_restores.is_empty())
                || (e.format == "i4" && e.image.is_some())
                || (e.format == "i4" && e.lettering.is_some() && e.text_overlays.is_empty()),
            "text overlays and source restores require generated I4 artwork"
        );
        ensure!(
            e.image_canvas_crop.is_none()
                || (e.image.is_some()
                    && e.format == "obj4"
                    && e.image_cells.is_empty()
                    && e.image_pieces.is_empty()
                    && e.image_sampling.is_none()),
            "canvas crop requires a whole OBJ4 image"
        );
        ensure!(
            (0..=512).contains(&e.bank_change_cost),
            "bank cost outside adopted range"
        );
        ensure!(
            e.format != "bg4" || e.palette_indices.is_empty(),
            "palette subset is only supported for standalone artwork"
        );
        ensure!(
            e.image_sampling.is_none()
                || (e.format == "bg4"
                    && e.image.is_some()
                    && e.image_region.is_none()
                    && e.image_cells.is_empty()
                    && e.image_pieces.is_empty()
                    && e.image_sampling.as_deref() == Some("nearest")),
            "explicit sampling requires nearest full BG artwork"
        );
        ensure!(
            e.id.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'-'),
            "unsafe id"
        );
        let source = rom.data(rom.file(&e.archive)?);
        ensure!(sha(source) == e.archive_sha256, "archive identity");
        let n = Narc::parse(source)?;
        let old = payload(&n, e.member)?;
        let pal = payload(&n, e.palette)?;
        ensure!(
            sha(&old) == e.source_sha256 && sha(&pal) == e.palette_sha256,
            "source/palette identity"
        );
        let mask = piece_mask(&e.image_pieces, e.width, e.height)?;
        ensure!(
            e.image_pieces.is_empty()
                || (matches!(
                    e.format.as_str(),
                    "a3i5" | "a5i3" | "i4" | "obj4" | "obj4-pair"
                ) && (e.image.is_some() || e.lettering.is_some())
                    && e.image_region.is_none()
                    && e.image_cells.is_empty()),
            "pieces require an alpha texture or supported I4 image"
        );
        let mut matte_report = None;
        let loaded = match (&e.image, &e.lettering) {
            (Some(path), None) => {
                let bytes = fs::read(path)?;
                ensure!(
                    Some(sha(&bytes)) == e.image_sha256,
                    "generated image identity"
                );
                Some(art_pixels::read(&bytes)?)
            }
            (None, Some(lettering)) => {
                ensure!(
                    e.image_sha256.is_none() && e.image_matte.is_none(),
                    "lettering has no image identity or matte"
                );
                ensure!(
                    lettering.canvas.is_none() || !e.image_pieces.is_empty(),
                    "a separate lettering canvas reaches the texture through pieces"
                );
                let [width, height] = lettering.canvas.unwrap_or([e.width, e.height]);
                ensure!(
                    (1..=256).contains(&width) && (1..=256).contains(&height),
                    "lettering canvas geometry"
                );
                Some(draw_lettering(lettering, width, height)?)
            }
            (None, None) => None,
            _ => anyhow::bail!("two pixel producers"),
        };
        let mut rgba = if let Some(mut image) = loaded {
            ensure!(e.font.is_none(), "two pixel producers");
            if let Some(matte) = &e.image_matte {
                matte_report = Some(art_pixels::remove_matte(&mut image, matte)?);
            }
            ensure!(
                e.image_cells.is_empty() || e.image_region.is_none(),
                "ambiguous sheet selection"
            );
            if let Some(region) = e.image_region {
                image = art_pixels::region(&image, region)?;
            }
            if let Some(crop) = &e.image_canvas_crop {
                let [x0, y0, x1, y1] =
                    crop.content_region
                        .unwrap_or([0, 0, crop.width, crop.height]);
                ensure!(
                    crop.width <= 1024
                        && crop.height <= 1024
                        && x0 < x1
                        && y0 < y1
                        && x1 <= crop.width
                        && y1 <= crop.height,
                    "title content outside canvas"
                );
                let reduced = art_pixels::reduce(&image, x1 - x0, y1 - y0, true)?;
                let mut canvas = art_pixels::Image {
                    width: crop.width,
                    height: crop.height,
                    rgba: vec![0; crop.width * crop.height * 4],
                };
                for y in y0..y1 {
                    canvas.rgba[(y * crop.width + x0) * 4..(y * crop.width + x1) * 4]
                        .copy_from_slice(
                            &reduced[(y - y0) * (x1 - x0) * 4..(y - y0 + 1) * (x1 - x0) * 4],
                        );
                }
                let selected = art_pixels::region(&canvas, crop.region)?;
                ensure!(
                    selected.width == e.width && selected.height == e.height,
                    "canvas crop must match native sprite dimensions"
                );
                selected.rgba
            } else if !e.image_pieces.is_empty() {
                let mut rgba = vec![0; e.width * e.height * 4];
                for piece in &e.image_pieces {
                    let [x0, y0, x1, y1] = piece.target;
                    if let Some(region) = piece.source {
                        ensure!(
                            piece.rotate.is_none() || piece.rotate.as_deref() == Some("cw"),
                            "unsupported piece rotation"
                        );
                        let rotated = piece.rotate.is_some();
                        let (rw, rh) = if rotated {
                            (y1 - y0, x1 - x0)
                        } else {
                            (x1 - x0, y1 - y0)
                        };
                        let reduced = art_pixels::reduce(
                            &art_pixels::region(&image, region)?,
                            rw,
                            rh,
                            piece.image_fit.unwrap_or(true),
                        )?;
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let (sx, sy) = if rotated {
                                    (y - y0, rh - 1 - (x - x0))
                                } else {
                                    (x - x0, y - y0)
                                };
                                let p = (sy * rw + sx) * 4;
                                let q = (y * e.width + x) * 4;
                                rgba[q..q + 4].copy_from_slice(&reduced[p..p + 4]);
                            }
                        }
                    }
                }
                rgba
            } else if e.image_cells.is_empty() {
                if e.image_sampling.as_deref() == Some("nearest") {
                    art_pixels::reduce_nearest(&image, e.width, e.height)?
                } else {
                    art_pixels::reduce(&image, e.width, e.height, e.format != "bg4")?
                }
            } else {
                ensure!(
                    e.width == 32 && e.height == e.image_cells.len() * 32 && e.format == "a3i5",
                    "unsupported sheet atlas geometry"
                );
                let mut rgba = Vec::new();
                for cell in &e.image_cells {
                    match cell {
                        Some(region) => rgba.extend(art_pixels::reduce(
                            &art_pixels::region(&image, *region)?,
                            32,
                            32,
                            true,
                        )?),
                        None => rgba.extend(vec![0; 32 * 32 * 4]),
                    }
                }
                rgba
            }
        } else {
            ensure!(
                e.image_region.is_none() && e.image_cells.is_empty() && e.image_matte.is_none(),
                "image region requires image input"
            );
            let bytes = fs::read(
                e.font
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("no pixel producer"))?,
            )?;
            ensure!(Some(sha(&bytes)) == e.font_sha256, "font identity");
            let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|v| anyhow::anyhow!(v))?;
            ensure!(
                e.width == 32 && e.height == 32 * e.slots.len(),
                "slot geometry"
            );
            let mut rgba = Vec::new();
            for s in &e.slots {
                rgba.extend(art_pixels::lettering(&font, s, e.size, 32, 32, e.colors)?);
            }
            rgba
        };
        if let Some([dx, dy]) = e.image_offset {
            ensure!(
                e.image.is_some() && matches!(e.format.as_str(), "a3i5" | "a5i3"),
                "image offset requires alpha lettering"
            );
            ensure!(
                e.image_background.is_none()
                    && e.text_overlays.is_empty()
                    && e.source_restores.is_empty(),
                "image offset cannot combine with overlays or restores"
            );
            let mut shifted = vec![0; rgba.len()];
            for y in 0..e.height {
                for x in 0..e.width {
                    let pixel = &rgba[(y * e.width + x) * 4..][..4];
                    if pixel[3] == 0 {
                        continue;
                    }
                    let (xx, yy) = (x as i64 + i64::from(dx), y as i64 + i64::from(dy));
                    ensure!(
                        xx >= 0 && yy >= 0 && xx < e.width as i64 && yy < e.height as i64,
                        "image offset clips visible lettering"
                    );
                    let p = yy as usize * e.width + xx as usize;
                    ensure!(
                        e.image_pieces.is_empty() || mask[p],
                        "image offset leaves declared pieces"
                    );
                    shifted[p * 4..p * 4 + 4].copy_from_slice(pixel);
                }
            }
            rgba = shifted;
        }
        if let Some(bg) = &e.image_background {
            ensure!(
                e.format == "i4" && e.image.is_some(),
                "solid background requires generated linear I4 artwork"
            );
            let [x0, y0, x1, y1] = bg.region;
            ensure!(
                x0 < x1 && y0 < y1 && x1 <= e.width && y1 <= e.height,
                "solid background outside canvas"
            );
            let colors: Vec<u16> = pal
                .chunks_exact(2)
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect();
            let index = colors
                .iter()
                .enumerate()
                .find(|&(i, c)| i > 0 && *c == bg.rgb555)
                .map(|(i, _)| i)
                .ok_or_else(|| anyhow::anyhow!("background color absent from source palette"))?;
            ensure!(
                e.palette_indices.is_empty() || e.palette_indices.contains(&index),
                "palette subset omits background"
            );
            let original = battle_ui::indices(&old);
            ensure!(
                original.len() >= e.width * e.height,
                "background source extent"
            );
            for (i, &v) in original.iter().enumerate() {
                let (x, y) = (i % e.width, i / e.width);
                let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
                ensure!(
                    (v != 0) == inside,
                    "source background alpha rectangle differs"
                );
            }
            let expected_corners = bg.source_corner_rgb555.unwrap_or([bg.rgb555; 4]);
            for ((x, y), expected) in [(x0, y0), (x1 - 1, y0), (x0, y1 - 1), (x1 - 1, y1 - 1)]
                .into_iter()
                .zip(expected_corners)
            {
                ensure!(
                    colors[usize::from(original[y * e.width + x])] == expected,
                    "source background corner differs"
                );
            }
            for (i, pixel) in rgba.chunks_exact_mut(4).enumerate() {
                let (x, y) = (i % e.width, i / e.width);
                if x >= x0 && x < x1 && y >= y0 && y < y1 {
                    ensure!(
                        e.image_pieces.is_empty() || mask[i],
                        "background outside editable pieces"
                    );
                    let alpha = u32::from(pixel[3]);
                    for (k, channel) in pixel[..3].iter_mut().enumerate() {
                        let base = u32::from((bg.rgb555 >> (k * 5)) & 31) * 255 / 31;
                        *channel = ((u32::from(*channel) * alpha + base * (255 - alpha) + 127)
                            / 255) as u8;
                    }
                    pixel[3] = 255;
                } else {
                    ensure!(
                        pixel[3] == 0,
                        "generated lettering outside solid background"
                    );
                }
            }
        }
        for overlay in &e.text_overlays {
            let [x0, y0, x1, y1] = overlay.region;
            ensure!(
                x0 < x1 && y0 < y1 && x1 <= e.width && y1 <= e.height,
                "text overlay outside image"
            );
            for restore in &e.source_restores {
                let [a, b, c, d] = restore.region;
                let [tx, ty] = restore.target.unwrap_or([a, b]);
                ensure!(a < c && b < d, "empty source restore");
                let right = tx
                    .checked_add(c - a)
                    .ok_or_else(|| anyhow::anyhow!("restore width overflow"))?;
                let bottom = ty
                    .checked_add(d - b)
                    .ok_or_else(|| anyhow::anyhow!("restore height overflow"))?;
                ensure!(
                    x1 <= tx || x0 >= right || y1 <= ty || y0 >= bottom,
                    "text overlaps protected source glyph"
                );
            }
            let bytes = fs::read(&overlay.font)?;
            ensure!(sha(&bytes) == overlay.font_sha256, "overlay font identity");
            let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|v| anyhow::anyhow!(v))?;
            let rotated = match overlay.rotate.as_deref() {
                None => false,
                Some("cw") => true,
                Some(other) => anyhow::bail!("unsupported overlay rotation {other}"),
            };
            let (rw, rh) = if rotated {
                (y1 - y0, x1 - x0)
            } else {
                (x1 - x0, y1 - y0)
            };
            let text =
                art_pixels::lettering(&font, &overlay.text, overlay.size, rw, rh, overlay.colors)?;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (sx, sy) = if rotated {
                        (y - y0, rh - 1 - (x - x0))
                    } else {
                        (x - x0, y - y0)
                    };
                    let pixel = &text[(sy * rw + sx) * 4..][..4];
                    if pixel[3] != 0 {
                        rgba[(y * e.width + x) * 4..][..4].copy_from_slice(pixel);
                    }
                }
            }
        }
        ensure!(
            (e.rgb_remap.is_empty() && e.outer_rim.is_empty())
                || e.image.is_some()
                || e.lettering.is_some(),
            "recolouring and rims apply to generated artwork"
        );
        for pixel in rgba.chunks_exact_mut(4) {
            if pixel[3] == 0 {
                continue;
            }
            if let Some(m) = e
                .rgb_remap
                .iter()
                .find(|m| (0..3).all(|k| pixel[k].abs_diff(m.from[k]) <= m.tolerance))
            {
                pixel[..3].copy_from_slice(&m.to);
            }
        }
        // The silhouette is every pixel at least half opaque; each ring claims the
        // less opaque 8-neighbours of the silhouette grown so far.
        let mut solid: Vec<bool> = rgba.chunks_exact(4).map(|p| p[3] >= 128).collect();
        for ring in &e.outer_rim {
            ensure!(ring.alpha > 0, "rim alpha");
            let before = solid.clone();
            for y in 0..e.height {
                for x in 0..e.width {
                    let p = y * e.width + x;
                    if before[p] {
                        continue;
                    }
                    let touches = (-1i64..=1).any(|dy| {
                        (-1i64..=1).any(|dx| {
                            let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                            xx >= 0
                                && yy >= 0
                                && (xx as usize) < e.width
                                && (yy as usize) < e.height
                                && before[yy as usize * e.width + xx as usize]
                        })
                    });
                    if touches {
                        rgba[p * 4..p * 4 + 4].copy_from_slice(&[
                            ring.rgb[0],
                            ring.rgb[1],
                            ring.rgb[2],
                            ring.alpha,
                        ]);
                        solid[p] = true;
                    }
                }
            }
            let edge = (0..e.width).any(|x| solid[x] || solid[(e.height - 1) * e.width + x])
                || (0..e.height).any(|y| solid[y * e.width] || solid[y * e.width + e.width - 1]);
            ensure!(!edge, "outer rim reaches the canvas edge");
        }
        write_png(
            &out.join(format!("{}-reduced.png", e.id)),
            e.width,
            e.height,
            &rgba,
        )?;
        let changes = archives.entry(e.archive.clone()).or_default();
        let (bytes, preview) = if e.format == "bg4" {
            ensure!(e.width == 256 && e.height == 192, "BG geometry");
            let id = e.map.ok_or_else(|| anyhow::anyhow!("no map"))?;
            let map = payload(&n, id)?;
            ensure!(Some(sha(&map)) == e.map_sha256, "map identity");
            let pixels = screens::render(&map, &old, pal.len() / 32)?;
            let s = screens::Screen {
                map,
                // Temporary capacity for one tile per cell; compact losslessly below.
                tiles: vec![0; 768 * 32],
                palette: pal.clone(),
                pixels,
            };
            let desired: Vec<[i32; 3]> = rgba
                .chunks_exact(4)
                .map(|c| {
                    [
                        i32::from(c[0]) * 31 / 255,
                        i32::from(c[1]) * 31 / 255,
                        i32::from(c[2]) * 31 / 255,
                    ]
                })
                .collect();
            let mut enc = screens::encode_screen_with_bank_cost(
                &s,
                |_, _| true,
                &desired,
                |cell, bank, left| {
                    if cell % 32 < 18 && cell / 32 < 12 {
                        0
                    } else if left.is_some_and(|b| b != bank) {
                        e.bank_change_cost
                    } else {
                        0
                    }
                },
            )?;
            let tile_count = share_flipped_tiles(&mut enc, old.len(), &pal)?;
            bg_records.push(json!({"tile_count":tile_count,"tile_capacity":old.len()/32,"bank_change_cost":e.bank_change_cost,"unchanged_quantization_region":[0,0,144,96]}));
            ensure!(
                screens::render(&enc.map, &enc.tiles, pal.len() / 32)? == enc.pixels,
                "BG pixel roundtrip"
            );
            ensure!(
                changes.insert(id, pack(&n, id, &enc.map)?).is_none(),
                "duplicate map writer"
            );
            (enc.tiles, screens::rgba(&enc.pixels, &pal)?)
        } else {
            let (bits, levels) = match e.format.as_str() {
                "a3i5" => (5, 7),
                "a5i3" => (3, 31),
                "obj4" | "i4" | "obj4-pair" => (4, 1),
                _ => anyhow::bail!("unsupported art format"),
            };
            let colors: Vec<_> = (0..pal.len() / 2)
                .map(|i| u16le(&pal, i * 2))
                .collect::<Result<_>>()?;
            ensure!(colors.len() <= 1 << bits, "palette exceeds format");
            ensure!(
                e.palette_indices
                    .iter()
                    .all(|&i| i < colors.len() && (bits != 4 || i != 0)),
                "invalid artwork palette subset"
            );
            let candidates: Vec<usize> = if e.palette_indices.is_empty() {
                ((if bits == 4 { 1 } else { 0 })..colors.len()).collect()
            } else {
                e.palette_indices.clone()
            };
            let mut px = Vec::new();
            let mut preview = Vec::new();
            let overlap_palettes = if let Some(crop) = &e.image_canvas_crop {
                if crop.shared_overlap_colors {
                    Some((&canvas_palettes[&canvas_identity(&e, crop)?], crop))
                } else {
                    None
                }
            } else {
                None
            };
            for (pixel_index, c) in rgba.chunks_exact(4).enumerate() {
                let mut alpha = (usize::from(c[3]) * levels + 127) / 255;
                if !e.alpha_values.is_empty() {
                    ensure!(
                        e.alpha_values.contains(&0)
                            && e.alpha_values.contains(&levels)
                            && e.alpha_values.iter().all(|v| *v <= levels),
                        "invalid adopted alpha levels"
                    );
                    alpha = *e
                        .alpha_values
                        .iter()
                        .min_by_key(|v| v.abs_diff(alpha))
                        .unwrap();
                }
                let shared_here = overlap_palettes.is_some_and(|(palettes, crop)| {
                    let x = crop.region[0] + pixel_index % e.width;
                    let y = crop.region[1] + pixel_index / e.width;
                    palettes
                        .iter()
                        .filter(|p| {
                            let [x0, y0, x1, y1] = p.region;
                            x >= x0 && x < x1 && y >= y0 && y < y1
                        })
                        .count()
                        > 1
                });
                let idx = if alpha == 0 {
                    0
                } else {
                    candidates
                        .iter()
                        .copied()
                        .filter(|&i| {
                            overlap_palettes.is_none_or(|(palettes, crop)| {
                                let x = crop.region[0] + pixel_index % e.width;
                                let y = crop.region[1] + pixel_index / e.width;
                                palettes.iter().all(|p| {
                                    let [x0, y0, x1, y1] = p.region;
                                    x < x0
                                        || x >= x1
                                        || y < y0
                                        || y >= y1
                                        || p.colors.contains(&colors[i])
                                })
                            })
                        })
                        .min_by_key(|&i| {
                            let v = colors[i];
                            let distance = (0..3)
                                .map(|k| {
                                    let d =
                                        ((v >> (k * 5)) & 31) as i32 * 255 / 31 - i32::from(c[k]);
                                    d * d
                                })
                                .sum::<i32>();
                            (distance, if shared_here { v } else { 0 })
                        })
                        .ok_or_else(|| anyhow::anyhow!("no shared color at canvas overlap"))?
                };
                px.push(if bits == 4 {
                    idx as u8
                } else {
                    (alpha << bits | idx) as u8
                });
                let v = colors[idx];
                preview.extend([
                    ((v & 31) * 255 / 31) as u8,
                    (((v >> 5) & 31) * 255 / 31) as u8,
                    (((v >> 10) & 31) * 255 / 31) as u8,
                    (alpha * 255 / levels) as u8,
                ]);
            }
            let mut restored = vec![false; px.len()];
            for restore in &e.source_restores {
                let [x0, y0, x1, y1] = restore.region;
                ensure!(
                    x0 < x1 && y0 < y1 && x1 <= e.width && y1 <= e.height,
                    "source restore outside image"
                );
                ensure!(old.len() * 2 >= px.len(), "source I4 size");
                let [tx, ty] = restore.target.unwrap_or([x0, y0]);
                ensure!(
                    tx <= e.width
                        && ty <= e.height
                        && x1 - x0 <= e.width - tx
                        && y1 - y0 <= e.height - ty,
                    "source restore target outside image"
                );
                for y in y0..y1 {
                    for x in x0..x1 {
                        let source_i = y * e.width + x;
                        let i = (ty + y - y0) * e.width + tx + x - x0;
                        let index = (old[source_i / 2] >> ((source_i % 2) * 4)) & 15;
                        if restore.exclude_indices.contains(&index) {
                            continue;
                        }
                        ensure!(!restored[i], "overlapping source restores");
                        restored[i] = true;
                        px[i] = index;
                        let v = *colors
                            .get(usize::from(index))
                            .ok_or_else(|| anyhow::anyhow!("source restore palette index"))?;
                        preview[i * 4..i * 4 + 4].copy_from_slice(&[
                            ((v & 31) * 255 / 31) as u8,
                            (((v >> 5) & 31) * 255 / 31) as u8,
                            (((v >> 10) & 31) * 255 / 31) as u8,
                            if index == 0 { 0 } else { 255 },
                        ]);
                    }
                }
            }
            if let Some(index) = e.outer_outline_index {
                ensure!(
                    e.format == "i4" && e.image.is_some(),
                    "outer outline requires generated I4 lettering"
                );
                ensure!(
                    index > 0 && index < colors.len(),
                    "outer outline palette index"
                );
                ensure!(
                    e.image_pieces.is_empty() && e.source_restores.is_empty(),
                    "outer outline cannot combine with protected pieces or restores"
                );
                let original = px.clone();
                let color = colors[index];
                for y in 0..e.height {
                    for x in 0..e.width {
                        if original[y * e.width + x] == 0 {
                            continue;
                        }
                        ensure!(
                            x > 0 && y > 0 && x + 1 < e.width && y + 1 < e.height,
                            "outer outline clips lettering"
                        );
                        for yy in y - 1..=y + 1 {
                            for xx in x - 1..=x + 1 {
                                let p = yy * e.width + xx;
                                if original[p] != 0 {
                                    continue;
                                }
                                px[p] = index as u8;
                                preview[p * 4..p * 4 + 4].copy_from_slice(&[
                                    ((color & 31) * 255 / 31) as u8,
                                    (((color >> 5) & 31) * 255 / 31) as u8,
                                    (((color >> 10) & 31) * 255 / 31) as u8,
                                    255,
                                ]);
                            }
                        }
                    }
                }
            }
            let mut bytes =
                if e.format == "obj4-pair" || (e.format == "obj4" && !e.image_pieces.is_empty()) {
                    let original = if e.format == "obj4-pair" {
                        ensure!(e.width == 128 && e.height == 32, "sprite pair geometry");
                        let pixels = battle_ui::sprite_pair_pixels(&old)?;
                        ensure!(
                            battle_ui::sprite_pair_bytes(&pixels)? == old,
                            "source sprite pair roundtrip"
                        );
                        pixels
                    } else {
                        let pixels = titles::untile(&old, e.width, e.height, 4)?;
                        ensure!(
                            titles::tile(&pixels, e.width, e.height, 4)? == old,
                            "source OBJ roundtrip"
                        );
                        pixels
                    };
                    if !e.image_pieces.is_empty() {
                        for (i, selected) in mask.iter().enumerate() {
                            if !selected {
                                px[i] = original[i];
                                let v = colors[usize::from(px[i])];
                                preview[i * 4..i * 4 + 4].copy_from_slice(&[
                                    ((v & 31) * 255 / 31) as u8,
                                    (((v >> 5) & 31) * 255 / 31) as u8,
                                    (((v >> 10) & 31) * 255 / 31) as u8,
                                    if px[i] == 0 { 0 } else { 255 },
                                ]);
                            }
                        }
                    }
                    if e.format == "obj4-pair" {
                        let paired = battle_ui::sprite_pair_bytes(&px)?;
                        ensure!(
                            battle_ui::sprite_pair_pixels(&paired)? == px,
                            "generated sprite pair roundtrip"
                        );
                        paired
                    } else {
                        let tiled = titles::tile(&px, e.width, e.height, 4)?;
                        ensure!(
                            titles::untile(&tiled, e.width, e.height, 4)? == px,
                            "generated OBJ roundtrip"
                        );
                        tiled
                    }
                } else if e.format == "i4" {
                    ensure!(px.len() % 2 == 0, "linear I4 requires pixel pairs");
                    let packed: Vec<u8> = px.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect();
                    ensure!(
                        packed
                            .iter()
                            .flat_map(|v| [v & 15, v >> 4])
                            .eq(px.iter().copied()),
                        "linear I4 pixel roundtrip"
                    );
                    packed
                } else if bits == 4 {
                    let tiled = titles::tile(&px, e.width, e.height, 4)?;
                    ensure!(
                        titles::untile(&tiled, e.width, e.height, 4)? == px,
                        "OBJ pixel roundtrip"
                    );
                    tiled
                } else {
                    px
                };
            ensure!(bytes.len() <= old.len(), "decoded art exceeds source");
            if !e.image_pieces.is_empty() && !matches!(e.format.as_str(), "obj4" | "obj4-pair") {
                for (i, selected) in mask.iter().enumerate() {
                    if !selected {
                        let (index, alpha) = if e.format == "i4" {
                            let shift = (i % 2) * 4;
                            let index = (old[i / 2] >> shift) & 15;
                            bytes[i / 2] = (bytes[i / 2] & !(15 << shift)) | (index << shift);
                            (index, if index == 0 { 0 } else { 255 })
                        } else {
                            bytes[i] = old[i];
                            (
                                old[i] & ((1 << bits) - 1),
                                ((usize::from(old[i] >> bits) * 255) / levels) as u8,
                            )
                        };
                        let v = colors[usize::from(index)];
                        preview[i * 4..i * 4 + 4].copy_from_slice(&[
                            ((v & 31) * 255 / 31) as u8,
                            (((v >> 5) & 31) * 255 / 31) as u8,
                            (((v >> 10) & 31) * 255 / 31) as u8,
                            alpha,
                        ]);
                    }
                }
            }
            bytes.extend_from_slice(&old[bytes.len()..]);
            (bytes, preview)
        };
        let protected_piece_sha256 = if e.image_pieces.is_empty() {
            None
        } else {
            let protected = |data: &[u8]| -> Result<Vec<u8>> {
                let pixels: Vec<u8> = if e.format == "i4" {
                    data.iter().flat_map(|v| [v & 15, v >> 4]).collect()
                } else if e.format == "obj4-pair" {
                    battle_ui::sprite_pair_pixels(data)?
                } else if e.format == "obj4" {
                    titles::untile(data, e.width, e.height, 4)?
                } else {
                    data.to_vec()
                };
                Ok(pixels
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !mask.get(*i).copied().unwrap_or(false))
                    .map(|(_, b)| *b)
                    .collect())
            };
            let before = protected(&old)?;
            let after = protected(&bytes)?;
            ensure!(
                before == after,
                "changed protected atlas punctuation or padding"
            );
            Some(sha(&after))
        };
        let packed = pack(&n, e.member, &bytes)?;
        ensure!(
            changes.insert(e.member, packed.clone()).is_none(),
            "duplicate art writer"
        );
        write_png(
            &out.join(format!("{}-encoded.png", e.id)),
            e.width,
            e.height,
            &preview,
        )?;
        let preview_x = records.len() % preview_columns * preview_cell_width + 2;
        if let Some(crop) = &e.image_canvas_crop {
            let identity = canvas_identity(&e, crop)?;
            let canvas = canvas_previews
                .entry(identity)
                .or_insert_with(|| CanvasPreview {
                    width: crop.width,
                    height: crop.height,
                    rgba: vec![0; crop.width * crop.height * 4],
                    conflicting_overlaps: 0,
                });
            for y in 0..e.height {
                for x in 0..e.width {
                    let pixel = &preview[(y * e.width + x) * 4..][..4];
                    if pixel[3] == 0 {
                        continue;
                    }
                    let target = &mut canvas.rgba
                        [((y + crop.region[1]) * crop.width + x + crop.region[0]) * 4..][..4];
                    if target[3] != 0 && target != pixel {
                        canvas.conflicting_overlaps += 1;
                    }
                    target.copy_from_slice(pixel);
                }
            }
        }
        let preview_y = records.len() / preview_columns * preview_cell_height + 2;
        for y in 0..e.height {
            for x in 0..e.width {
                let source = (y * e.width + x) * 4;
                let target = ((preview_y + y) * preview_width + preview_x + x) * 4;
                let alpha = u32::from(preview[source + 3]);
                for channel in 0..3 {
                    preview_sheet[target + channel] =
                        ((u32::from(preview[source + channel]) * alpha + 20 * (255 - alpha) + 127)
                            / 255) as u8;
                }
            }
        }
        preview_regions.push(
            json!({"id":e.id,"region":[preview_x,preview_y,preview_x+e.width,preview_y+e.height]}),
        );
        let mut record = json!({"id":e.id,"archive":e.archive,"member":e.member,"source_sha256":e.source_sha256,"decoded_sha256":sha(&bytes),"stored_size":packed.len(),"capacity":n.members[e.member].len(),"palette_sha256":e.palette_sha256,"image_sha256":e.image_sha256,"image_matte":e.image_matte,"matte_conversion":matte_report,"font_sha256":e.font_sha256,"slots":e.slots,"image_region":e.image_region,"image_canvas_crop":e.image_canvas_crop,"image_cells":e.image_cells,"image_pieces":e.image_pieces,"text_overlays":e.text_overlays,"lettering":e.lettering,"rgb_remap":e.rgb_remap,"outer_rim":e.outer_rim,"source_restores":e.source_restores,"protected_piece_sha256":protected_piece_sha256,"palette_indices":e.palette_indices,"alpha_values":e.alpha_values,"encoded_preview_sha256":sha(&preview)});
        if let Some(bg) = &e.image_background {
            record["image_background"] = serde_json::to_value(bg)?;
        }
        if let Some(sampling) = &e.image_sampling {
            record["image_sampling"] = json!(sampling);
        }
        records.push(record);
    }
    let mut canvas_reports = Vec::new();
    for (identity, canvas) in canvas_previews {
        let file = format!("canvas-{identity}.png");
        write_png(&out.join(&file), canvas.width, canvas.height, &canvas.rgba)?;
        canvas_reports.push(json!({"identity":identity,"image":file,"width":canvas.width,
            "height":canvas.height,"rgba_sha256":sha(&canvas.rgba),
            "conflicting_overlaps":canvas.conflicting_overlaps,
            "claim":"Static recomposition in entry order; runtime consumer binding remains unproven."}));
    }
    json_file(&out.join("canvas-previews.json"), &json!(canvas_reports))?;
    write_png(
        &out.join("encoded-on-dark.png"),
        preview_width,
        preview_height,
        &preview_sheet,
    )?;
    json_file(
        &out.join("preview-regions.json"),
        &json!({"scale":1,"background_rgb":[20,20,20],"regions":preview_regions}),
    )?;
    let mut layout_report = Value::Null;
    if tr.game_over_layout {
        let path = "puyo/result/result.narc";
        let n = Narc::parse(rom.data(rom.file(path)?))?;
        let changes = archives.entry(path.to_owned()).or_default();
        ensure!(
            [55, 57, 58, 59, 60, 62]
                .iter()
                .all(|id| changes.contains_key(id)),
            "game-over layout requires all six graphics writers"
        );
        let mut reports = Vec::new();
        for id in [51, 54] {
            let old = payload(&n, id)?;
            let (bytes, report) = game_over::reposition(id, &old)?;
            let mut packed = crate::compress::pack(&bytes)?;
            if packed.len() > n.members[id].len() {
                packed = crate::compress::pack_layout(&bytes)?;
            }
            ensure!(
                packed.len() <= n.members[id].len(),
                "layout {id} capacity {} > {}",
                packed.len(),
                n.members[id].len()
            );
            ensure!(unpack_halfword(&packed)? == bytes, "layout roundtrip");
            ensure!(
                changes.insert(id, packed).is_none(),
                "duplicate layout writer"
            );
            reports.push(report);
        }
        layout_report = json!(reports);
        game_over::preview(&n, changes, out)?;
    }
    let mut replacements = Vec::new();
    for (i, (archive, changes)) in archives.iter().enumerate() {
        let source = rom.data(rom.file(archive)?);
        let b = battle_ui::rebuilt(source, changes)?;
        let name = format!("{i}.narc");
        fs::write(out.join(&name), &b)?;
        replacements.push(json!({"file":archive,"expected_sha256":sha(source),"input":name,"input_sha256":sha(&b)}));
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":replacements}),
    )?;
    let r = json!({"source_sha256":sha(rom.bytes),"spec_sha256":sha(&specbytes),"records":records,"bg_records":bg_records,"layout_edits":layout_report,"protected":"all palettes, other NARC members, texture tail rows; optional game-over layout changes only reported x coordinates; notebook entire BG image is the adopted generated replacement","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("report.json"), &r)?;
    Ok(r)
}

#[cfg(test)]
mod tests;
