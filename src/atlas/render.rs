use anyhow::{Result, ensure};

pub(super) fn paint_cell(
    font: &fontdue::Font,
    text: &str,
    center: usize,
    row: usize,
    ink: u8,
    outline: u8,
    pixels: &mut [u8],
) -> Result<usize> {
    paint_text_cell(
        font,
        text,
        CellLayout {
            center,
            row,
            size: 12.0,
            max_width: 94,
        },
        ink,
        outline,
        pixels,
    )
}

pub(super) struct CellLayout {
    pub(super) center: usize,
    pub(super) row: usize,
    pub(super) size: f32,
    pub(super) max_width: usize,
}

pub(super) fn paint_text_cell(
    font: &fontdue::Font,
    text: &str,
    cell: CellLayout,
    ink: u8,
    outline: u8,
    pixels: &mut [u8],
) -> Result<usize> {
    let CellLayout {
        center,
        row,
        size,
        max_width,
    } = cell;
    ensure!(!text.trim().is_empty(), "empty label");
    let width: usize = text
        .chars()
        .map(|c| {
            if c == ' ' {
                5
            } else {
                font.metrics(c, size).advance_width.round() as usize
            }
        })
        .sum();
    ensure!(width <= max_width, "label too wide: {text} ({width}px)");
    let mut cursor = center
        .checked_sub(width / 2)
        .ok_or_else(|| anyhow::anyhow!("label exceeds left boundary: {text}"))?;
    let mut points = Vec::new();
    for c in text.chars() {
        ensure!(font.lookup_glyph_index(c) != 0, "missing glyph: {c}");
        let (m, b) = font.rasterize(c, size);
        for y in 0..m.height {
            for x in 0..m.width {
                if b[y * m.width + x] < 128 {
                    continue;
                }
                let px = cursor as i32 + m.xmin + x as i32;
                let py = 13 - m.ymin - m.height as i32 + y as i32;
                ensure!(
                    (center as i32 - 47..center as i32 + 47).contains(&px)
                        && (1..127).contains(&px)
                        && (1..14).contains(&py),
                    "glyph exceeds cell: {text}"
                );
                points.push((px as usize, row * 16 + py as usize));
            }
        }
        cursor += if c == ' ' {
            5
        } else {
            m.advance_width.round() as usize
        };
    }
    for &(x, y) in &points {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            pixels[(y as isize + dy) as usize * 128 + (x as isize + dx) as usize] = outline;
        }
    }
    for (x, y) in points {
        pixels[y * 128 + x] = ink;
    }
    Ok(width)
}
