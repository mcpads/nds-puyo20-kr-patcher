//! Deterministic reduction and font rasterization for already-authored artwork.
use crate::{format::*, graphics::write_png};
use anyhow::{Result, ensure};
use std::{fs, io::Cursor, path::Path};

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SheetPiece {
    pub source: [usize; 4],
    pub target: [usize; 4],
    #[serde(default = "default_piece_fit")]
    pub fit: bool,
}
fn default_piece_fit() -> bool {
    true
}
/// Assemble explicitly authored, independent sheet elements into a title canvas.
pub fn assemble(im: &Image, w: usize, h: usize, pieces: &[SheetPiece]) -> Result<Vec<u8>> {
    ensure!(
        w > 0 && h > 0 && w <= 1024 && h <= 1024,
        "title canvas geometry"
    );
    let mut out = vec![0; w * h * 4];
    let mut occupied = vec![false; w * h];
    for piece in pieces {
        let [x0, y0, x1, y1] = piece.target;
        ensure!(
            x0 < x1 && y0 < y1 && x1 <= w && y1 <= h,
            "title piece outside canvas"
        );
        let rgba = reduce(&region(im, piece.source)?, x1 - x0, y1 - y0, piece.fit)?;
        for y in y0..y1 {
            for x in x0..x1 {
                let dst = y * w + x;
                ensure!(!occupied[dst], "overlapping title pieces");
                occupied[dst] = true;
                let src = (y - y0) * (x1 - x0) + x - x0;
                out[dst * 4..dst * 4 + 4].copy_from_slice(&rgba[src * 4..src * 4 + 4]);
            }
        }
    }
    Ok(out)
}
/// Select an explicitly authored cell from a generated sprite sheet.
pub fn region(im: &Image, [x0, y0, x1, y1]: [usize; 4]) -> Result<Image> {
    ensure!(
        x0 < x1 && y0 < y1 && x1 <= im.width && y1 <= im.height,
        "generated sheet region outside image"
    );
    let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        rgba.extend_from_slice(&im.rgba[(y * im.width + x0) * 4..(y * im.width + x1) * 4]);
    }
    Ok(Image {
        width: x1 - x0,
        height: y1 - y0,
        rgba,
    })
}
pub fn read(bytes: &[u8]) -> Result<Image> {
    let mut d = png::Decoder::new(Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info()?;
    let mut b = vec![
        0;
        r.output_buffer_size()
            .ok_or_else(|| anyhow::anyhow!("PNG buffer overflow"))?
    ];
    let info = r.next_frame(&mut b)?;
    ensure!(info.bit_depth == png::BitDepth::Eight, "PNG depth");
    let n = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => anyhow::bail!("PNG color"),
    };
    let rgba = b[..info.buffer_size()]
        .chunks_exact(n)
        .flat_map(|c| match n {
            4 => [c[0], c[1], c[2], c[3]],
            3 => [c[0], c[1], c[2], 255],
            2 => [c[0], c[0], c[0], c[1]],
            _ => [c[0], c[0], c[0], 255],
        })
        .collect();
    Ok(Image {
        width: info.width as usize,
        height: info.height as usize,
        rgba,
    })
}
/// Sample full authored artwork at pixel centers without creating blended edge colors.
pub fn reduce_nearest(im: &Image, w: usize, h: usize) -> Result<Vec<u8>> {
    ensure!(w > 0 && h > 0 && w <= 1024 && h <= 1024, "target geometry");
    ensure!(im.width > 0 && im.height > 0, "empty source image");
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let sx = ((2 * x + 1) * im.width / (2 * w)).min(im.width - 1);
            let sy = ((2 * y + 1) * im.height / (2 * h)).min(im.height - 1);
            out.extend_from_slice(&im.rgba[(sy * im.width + sx) * 4..][..4]);
        }
    }
    Ok(out)
}

/// Area reduction with premultiplied alpha; no color from invisible pixels leaks in.
pub fn reduce(im: &Image, w: usize, h: usize, fit: bool) -> Result<Vec<u8>> {
    ensure!(w > 0 && h > 0 && w <= 1024 && h <= 1024, "target geometry");
    ensure!(!fit || (w > 4 && h > 4), "insufficient lettering padding");
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, im.width, im.height);
    if fit {
        let positions: Vec<_> = im
            .rgba
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, c)| c[3] > 16)
            .map(|(i, _)| (i % im.width, i / im.width))
            .collect();
        ensure!(!positions.is_empty(), "blank generated image");
        x0 = positions.iter().map(|p| p.0).min().unwrap();
        y0 = positions.iter().map(|p| p.1).min().unwrap();
        x1 = positions.iter().map(|p| p.0).max().unwrap() + 1;
        y1 = positions.iter().map(|p| p.1).max().unwrap() + 1;
    }
    let (sw, sh) = ((x1 - x0) as f64, (y1 - y0) as f64);
    let (dw, dh) = if fit {
        let scale = ((w - 4) as f64 / sw).min((h - 4) as f64 / sh);
        ((sw * scale).round() as usize, (sh * scale).round() as usize)
    } else {
        (w, h)
    };
    let (ox, oy) = ((w - dw) / 2, (h - dh) / 2);
    let mut out = vec![0; w * h * 4];
    for y in 0..dh {
        for x in 0..dw {
            let (l, t, r, b) = (
                x0 as f64 + x as f64 * sw / dw as f64,
                y0 as f64 + y as f64 * sh / dh as f64,
                x0 as f64 + (x + 1) as f64 * sw / dw as f64,
                y0 as f64 + (y + 1) as f64 * sh / dh as f64,
            );
            let mut sum = [0.0; 4];
            let mut weight = 0.0;
            for sy in t.floor() as usize..(b.ceil() as usize).min(im.height) {
                for sx in l.floor() as usize..(r.ceil() as usize).min(im.width) {
                    let v = (r.min((sx + 1) as f64) - l.max(sx as f64)).max(0.0)
                        * (b.min((sy + 1) as f64) - t.max(sy as f64)).max(0.0);
                    let c = &im.rgba[(sy * im.width + sx) * 4..][..4];
                    for i in 0..3 {
                        sum[i] += f64::from(c[i]) * f64::from(c[3]) * v;
                    }
                    sum[3] += f64::from(c[3]) * v;
                    weight += v;
                }
            }
            let o = ((y + oy) * w + x + ox) * 4;
            if sum[3] > 0.0 {
                for i in 0..3 {
                    out[o + i] = (sum[i] / sum[3]).round() as u8;
                }
            }
            out[o + 3] = (sum[3] / weight).round() as u8;
        }
    }
    Ok(out)
}

pub fn lettering(
    font: &fontdue::Font,
    text: &str,
    size: f32,
    w: usize,
    h: usize,
    colors: [[u8; 3]; 3],
) -> Result<Vec<u8>> {
    let mut positions = Vec::new();
    let mut cursor = 0.0f32;
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for c in text.chars() {
        ensure!(
            font.lookup_glyph_index(c) != 0,
            "missing artwork glyph: {c}"
        );
        let (m, b) = font.rasterize(c, size);
        let x = cursor.round() as i32 + m.xmin;
        let y = -m.ymin - m.height as i32;
        if m.width > 0 && m.height > 0 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + m.width as i32);
            bottom = bottom.max(y + m.height as i32);
        }
        cursor += m.advance_width;
        positions.push((x, y, m, b));
    }
    let mut out = vec![0; w * h * 4];
    if text.is_empty() {
        return Ok(out);
    }
    ensure!(
        right - left + 4 <= w as i32 && bottom - top + 4 <= h as i32,
        "font text {text} does not fit {w}x{h}"
    );
    let dx = (w as i32 - (right - left)) / 2 - left;
    let dy = (h as i32 - (bottom - top)) / 2 - top;
    let mut ink = vec![false; w * h];
    for (x, y, m, b) in positions {
        for yy in 0..m.height {
            for xx in 0..m.width {
                if b[yy * m.width + xx] >= 112 {
                    ink[((y + dy + yy as i32) as usize) * w + (x + dx + xx as i32) as usize] = true;
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let near = |radius: i32| {
                (-radius..=radius).any(|dy| {
                    (-radius..=radius).any(|dx| {
                        let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                        xx >= 0
                            && yy >= 0
                            && xx < w as i32
                            && yy < h as i32
                            && ink[yy as usize * w + xx as usize]
                    })
                })
            };
            let c = if ink[y * w + x] {
                Some(colors[0])
            } else if near(1) {
                Some(colors[1])
            } else if near(2) {
                Some(colors[2])
            } else {
                None
            };
            if let Some(c) = c {
                out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
    }
    Ok(out)
}

pub fn font_gallery(paths: &[std::path::PathBuf], out: &Path) -> Result<serde_json::Value> {
    ensure!(!out.exists(), "output exists");
    fs::create_dir_all(out)?;
    let mut rows = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let bytes = fs::read(p)?;
        let font = fontdue::Font::from_bytes(bytes.clone(), fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        let mut strip = vec![0; 32 * 10 * 32 * 4];
        for (j, (text, size)) in [
            ("계", 20.0),
            ("속", 20.0),
            ("할", 20.0),
            ("까", 20.0),
            ("요", 20.0),
            ("?", 20.0),
            ("게임", 13.0),
            ("오버", 13.0),
            ("게", 20.0),
            ("임", 20.0),
        ]
        .iter()
        .enumerate()
        {
            let px = lettering(
                &font,
                text,
                *size,
                32,
                32,
                [[20, 190, 240], [255, 230, 90], [50, 70, 110]],
            )?;
            for y in 0..32 {
                strip[(y * 320 + j * 32) * 4..(y * 320 + j * 32 + 32) * 4]
                    .copy_from_slice(&px[y * 128..(y + 1) * 128]);
            }
        }
        write_png(&out.join(format!("{i}.png")), 320, 32, &strip)?;
        rows.push(serde_json::json!({"id":i,"path":p,"sha256":sha(&bytes)}));
    }
    let v = serde_json::json!({"fonts":rows,"tokens":["계","속","할","까","요","?","게임","오버","게","임"]});
    crate::assets::json_file(&out.join("report.json"), &v)?;
    Ok(v)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MatteSheet {
    image: std::path::PathBuf,
    image_sha256: String,
    output: String,
    matte_rgb: [u8; 3],
    tolerance: u8,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductionMatte {
    pub rgb: [u8; 3],
    pub tolerance: u8,
}

/// Remove only the declared authored matte, preserving every other RGBA pixel.
pub(crate) fn remove_matte(
    image: &mut Image,
    matte: &ProductionMatte,
) -> Result<serde_json::Value> {
    ensure!(
        matte.tolerance <= 64,
        "matte tolerance exceeds supported bound"
    );
    ensure!(
        image.rgba.chunks_exact(4).all(|c| c[3] == 255),
        "matte source must be fully opaque"
    );
    let original = image.rgba.clone();
    let is_matte = |c: &[u8]| (0..3).all(|k| c[k].abs_diff(matte.rgb[k]) <= matte.tolerance);
    for y in 0..image.height {
        for x in 0..image.width {
            if x == 0 || y == 0 || x + 1 == image.width || y + 1 == image.height {
                let offset = (y * image.width + x) * 4;
                ensure!(
                    is_matte(&original[offset..offset + 4]),
                    "non-matte pixel at sheet edge"
                );
            }
        }
    }
    let mut removed = 0usize;
    let mut foreground = Vec::new();
    for c in image.rgba.chunks_exact_mut(4) {
        if is_matte(c) {
            c.fill(0);
            removed += 1;
        } else {
            foreground.extend_from_slice(c);
        }
    }
    let pixels = image.width * image.height;
    ensure!(
        removed >= pixels / 20 && removed < pixels * 99 / 100,
        "matte/foreground coverage outside expected sheet bounds"
    );
    for (before, after) in original.chunks_exact(4).zip(image.rgba.chunks_exact(4)) {
        ensure!(
            if is_matte(before) {
                after == [0, 0, 0, 0]
            } else {
                before == after
            },
            "unexpected matte conversion difference"
        );
    }
    Ok(serde_json::json!({
        "width":image.width,"height":image.height,"matte_rgb":matte.rgb,
        "tolerance":matte.tolerance,"removed_pixels":removed,
        "preserved_pixels":pixels-removed,"preserved_rgba_sha256":sha(&foreground),
        "only_declared_matte_changed":true
    }))
}

/// Convert a declared uniform production matte. Never infer a game's background.
pub fn prepare_sheets(spec: &Path, out: &Path) -> Result<serde_json::Value> {
    ensure!(!out.exists(), "sheet output already exists");
    let spec_bytes = fs::read(spec)?;
    let sheets: Vec<MatteSheet> = serde_json::from_slice(&spec_bytes)?;
    ensure!(!sheets.is_empty(), "empty matte sheet list");
    let mut names = std::collections::BTreeSet::new();
    let mut prepared = Vec::new();
    for sheet in sheets {
        ensure!(
            sheet.output.ends_with(".png")
                && sheet
                    .output
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                && !sheet.output.starts_with('.')
                && names.insert(sheet.output.clone()),
            "invalid or duplicate sheet output name"
        );
        ensure!(
            sheet.tolerance <= 64,
            "matte tolerance exceeds supported bound"
        );
        let bytes = fs::read(&sheet.image)?;
        ensure!(
            sha(&bytes) == sheet.image_sha256,
            "matte source identity differs"
        );
        let mut image = read(&bytes)?;
        let mut row = remove_matte(
            &mut image,
            &ProductionMatte {
                rgb: sheet.matte_rgb,
                tolerance: sheet.tolerance,
            },
        )?;
        row["image"] = serde_json::to_value(&sheet.image)?;
        row["image_sha256"] = sheet.image_sha256.into();
        row["output"] = sheet.output.clone().into();
        row["native_size_reviewed"] = false.into();
        prepared.push((sheet.output, image, row));
    }
    fs::create_dir_all(out)?;
    let mut rows = Vec::new();
    for (name, image, mut row) in prepared {
        let path = out.join(name);
        write_png(&path, image.width, image.height, &image.rgba)?;
        let bytes = fs::read(&path)?;
        let decoded = read(&bytes)?;
        ensure!(
            decoded.width == image.width
                && decoded.height == image.height
                && decoded.rgba == image.rgba,
            "matte PNG round trip differs"
        );
        row["output_sha256"] = sha(&bytes).into();
        rows.push(row);
    }
    let report = serde_json::json!({"spec_sha256":sha(&spec_bytes),"sheets":rows,
        "scope":"static authored PNG conversion only","runtime_verified":false});
    crate::assets::json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;

/// Preview candidates through the same packed glyph renderer used by story/menu builds.
pub fn text_font_gallery(paths: &[std::path::PathBuf], out: &Path) -> Result<serde_json::Value> {
    ensure!(!out.exists(), "output exists");
    fs::create_dir_all(out)?;
    let samples = [
        "비과학적인 건",
        "무, 무, 무서워요!",
        "과학으론 안 풀리니",
        "무릎도 풀리네~!",
        "리스쿠마 선배",
        "드라코켄타우로스",
        "뿌요를 움직여 주세요",
        "값 읽고 앉아 빛 흙 꽃",
    ];
    let mut records = Vec::new();
    for (id, path) in paths.iter().enumerate() {
        let bytes = fs::read(path)?;
        let font = fontdue::Font::from_bytes(bytes.clone(), fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        for (height, shadow) in [(11, true), (12, true), (11, false)] {
            let mut rgba = vec![0u8; 256 * 128 * 4];
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[60, 38, 78, 255]);
            }
            let mut widths = Vec::new();
            let mut errors = Vec::new();
            for (row, text) in samples.iter().enumerate() {
                let mut cursor = 8;
                for c in text.chars() {
                    match crate::localize::glyph_with_shadow(&font, c, height, shadow) {
                        Ok((packed, advance)) => {
                            for (p, v) in packed.iter().flat_map(|v| [v & 15, v >> 4]).enumerate() {
                                let x = cursor + p % 16;
                                let y = row * 16 + p / 16 + 2;
                                if v != 0 && x < 256 && y < 128 {
                                    let color = if v == 1 {
                                        [255, 255, 255, 255]
                                    } else {
                                        [15, 8, 22, 255]
                                    };
                                    rgba[(y * 256 + x) * 4..(y * 256 + x + 1) * 4]
                                        .copy_from_slice(&color);
                                }
                            }
                            cursor += advance;
                        }
                        Err(error) => {
                            errors.push(format!("{c}: {error}"));
                            cursor += 12;
                        }
                    }
                }
                widths.push(cursor - 8);
            }
            let suffix = if shadow { "" } else { "-no-shadow" };
            let file = format!("font-{id}-height-{height}{suffix}.png");
            write_png(&out.join(&file), 256, 128, &rgba)?;
            let mut enlarged = Vec::with_capacity(1024 * 512 * 4);
            for y in 0..512 {
                for x in 0..1024 {
                    let p = ((y / 4) * 256 + x / 4) * 4;
                    enlarged.extend_from_slice(&rgba[p..p + 4]);
                }
            }
            write_png(
                &out.join(format!("font-{id}-height-{height}{suffix}-4x.png")),
                1024,
                512,
                &enlarged,
            )?;
            records.push(serde_json::json!({"font":path,"sha256":sha(&bytes),"cell_height":height,"shadow":shadow,"size":12,"file":file,"widths":widths,"errors":errors,"renderer":"current fontdue story/menu glyph renderer, not FreeType auto-hinting"}));
        }
    }
    let report = serde_json::json!({"samples":samples,"records":records,"scope":"Static candidate comparison only; no font adoption"});
    crate::assets::json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
