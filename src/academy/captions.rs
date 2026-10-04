//! Small academy title plates drawn like the originals: each phrase sits on its
//! own cream plate with rounded corners, a white halo around black lettering and
//! a black shadow two pixels right and down. The two embedded source symbols
//! (IDs 36/41) and the stored tail row are kept byte for byte.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Captions {
    state: String,
    entries: Vec<Caption>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Caption {
    id: usize,
    korean: String,
    /// Phrases drawn on separate plates. Omitted means one plate.
    #[serde(default)]
    plates: Vec<String>,
    /// Word space in pixels; only a caption that cannot fit otherwise narrows it.
    #[serde(default)]
    space: Option<usize>,
}

const WIDTH: usize = 128;
const HEIGHT: usize = 22;
const SIZE: usize = 12;
const SPACE: usize = 4;
/// Blank columns between a digit run and the hangul it touches (`3연쇄`).
const DIGIT_GAP: usize = 2;
/// Width of a plate that holds only a number; the source number plates are 17px.
const NUMBER_PLATE: usize = 14;
/// First hangul ink row; the original lettering spans rows 4..=16 of an 18-row plate.
const INK_TOP: usize = 5;
const PAD_LEFT: usize = 2;
const PAD_RIGHT: usize = 3;
const GAP: usize = 3;
const SHADOW: usize = 2;
const PLATE_TOP: usize = 1;
const PLATE_BOTTOM: usize = 18;

/// Left columns that keep the source symbol, and whether the new plate continues
/// the source plate (ID 41 draws the symbol and phrase on one plate).
fn protected(id: usize) -> (usize, bool) {
    match id {
        36 => (44, false),
        41 => (30, true),
        _ => (0, false),
    }
}

/// Ink of one phrase normalised to x=0 and shifted by `dy` rows. `bold` widens
/// each stroke one pixel to the right unless that would close a one-pixel gap
/// (ㅌ bars, ㅃ halves, ㄹ turns and ㅐ pairs keep their counters). `tall` also
/// thickens strokes one pixel downward under the same rule, as the source draws
/// its numbers with two-pixel bars as well as two-pixel stems.
fn phrase_ink(
    font: &fontdue::Font,
    size: usize,
    text: &str,
    dy: isize,
    bold: bool,
    tall: bool,
    space: usize,
) -> Result<(Vec<(usize, usize)>, usize)> {
    let ink = battle_ui::text_ink_with_space(font, text, size, [0, 0, 512, HEIGHT], 17, space)?;
    let left = ink.iter().map(|p| p.0).min().unwrap();
    let mut set = ink
        .iter()
        .map(|&(x, y)| (x - left, y))
        .collect::<std::collections::BTreeSet<_>>();
    let widen = |set: &std::collections::BTreeSet<(usize, usize)>, dx: usize, dy: usize| {
        let mut out = set.clone();
        for &(x, y) in set {
            if !set.contains(&(x + dx, y + dy)) && !set.contains(&(x + 2 * dx, y + 2 * dy)) {
                out.insert((x + dx, y + dy));
            }
        }
        out
    };
    if bold {
        set = widen(&set, 1, 0);
    }
    if tall {
        set = widen(&set, 0, 1);
    }
    let width = set.iter().map(|p| p.0).max().unwrap() + 1;
    let mut out = Vec::with_capacity(set.len());
    for (x, y) in set {
        let row = y as isize + dy;
        ensure!(
            row > PLATE_TOP as isize && row < PLATE_BOTTOM as isize,
            "caption ink outside plate rows: {text}"
        );
        out.push((x, row as usize));
    }
    Ok((out, width))
}

/// A phrase whose digit runs are thickened in both directions like the source's
/// bold numbers (two-pixel stems and bars, about the hangul height), while the
/// hangul is widened to the right only. A space at a run boundary keeps the word
/// space; otherwise digits sit `DIGIT_GAP` pixels from the hangul.
fn phrase_with_digits(
    font: &fontdue::Font,
    dy: isize,
    text: &str,
    space: usize,
) -> Result<(Vec<(usize, usize)>, usize)> {
    let mut runs: Vec<(bool, String)> = Vec::new();
    for ch in text.chars() {
        let digit = ch.is_ascii_digit();
        match runs.last_mut() {
            Some((d, run)) if *d == digit || ch == ' ' => run.push(ch),
            _ => runs.push((digit, ch.to_string())),
        }
    }
    let mut ink = Vec::new();
    let mut pen = 0;
    let mut spaced = false;
    for (k, (digit, run)) in runs.iter().enumerate() {
        let body = run.trim_end();
        if k > 0 {
            pen += if spaced { space } else { DIGIT_GAP };
        }
        spaced = body.len() != run.len();
        let (part, w) = phrase_ink(font, SIZE, body, dy, true, *digit, space)?;
        ink.extend(part.into_iter().map(|(x, y)| (x + pen, y)));
        pen += w;
    }
    Ok((ink, pen))
}

/// Rows to move a glyph so its ink starts at `top`.
fn row_shift(font: &fontdue::Font, size: usize, sample: &str, top: usize) -> Result<isize> {
    let ink = battle_ui::text_ink_with_space(font, sample, size, [0, 0, 64, HEIGHT], 17, 4)?;
    Ok(top as isize - ink.iter().map(|p| p.1).min().unwrap() as isize)
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Captions = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == 112,
        "expected all 112 title plates"
    );
    let path = "academy/academy.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b0c7244f4d3de4d9836a1e6607097bffdbfb63644f8e01e28f191f0ddd0a7ed3",
        "academy archive changed"
    );
    let table = rom.data(rom.file("academy/lesson_item_s_texlist.bin")?);
    ensure!(
        sha(table) == "babcc3815cae7c516a521c99c7be3b1cf3487493493ced23dc556e70338be3d4"
            && table.len() == 113 * 12,
        "caption table changed"
    );
    let n = Narc::parse(source)?;
    let bytes = fs::read(font_path)?;
    let font_sha256 = sha(&bytes);
    ensure!(crate::fonts::is_galmuri11(&bytes), "font identity mismatch");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let dy = row_shift(&font, SIZE, "가", INK_TOP)?;
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut preview = vec![0u8; WIDTH * 28 * 112 * 4];
    for p in preview.chunks_exact_mut(4) {
        p.copy_from_slice(&[65, 75, 80, 255]);
    }
    for (id, entry) in tr.entries.iter().enumerate() {
        ensure!(entry.id == id, "caption order changed");
        let member = match id {
            0..=42 => 332 + id * 2,
            43..=93 => 230 + (id - 43) * 2,
            _ => 194 + (id - 94) * 2,
        };
        let row = slice(table, id * 12, 12)?;
        ensure!(
            u16le(row, 0)? == member
                && u16le(row, 2)? == member - 1
                && u16le(row, 4)? == 3
                && u16le(row, 8)? == 128
                && u16le(row, 10)? == 22,
            "caption mapping changed"
        );
        let old = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[member - 1])?;
        ensure!(
            old.len() == 1472 && palette.len() == 32,
            "caption storage changed"
        );
        let color = |i: usize| u16le(&palette, i * 2);
        let black = (1..16)
            .find(|&i| color(i).ok() == Some(0))
            .ok_or_else(|| anyhow::anyhow!("missing source black"))? as u8;
        let white = (1..16)
            .find(|&i| color(i).ok() == Some(0x7fff))
            .ok_or_else(|| anyhow::anyhow!("missing source white"))? as u8;
        let source_pixels = old
            .iter()
            .flat_map(|b| [b & 15, b >> 4])
            .collect::<Vec<_>>();
        // The source plate color is the most used opaque index of the original.
        let mut counts = [0usize; 16];
        for &p in &source_pixels[..WIDTH * HEIGHT] {
            counts[p as usize] += 1;
        }
        let cream = (1..16)
            .filter(|&i| i != black as usize && i != white as usize)
            .max_by_key(|&i| counts[i])
            .unwrap() as u8;
        let c = color(cream as usize)?;
        ensure!(
            (0..3).all(|k| (c >> (k * 5)) & 31 >= 26),
            "caption plate color is not the light source plate"
        );
        let texts = if entry.plates.is_empty() {
            vec![entry.korean.clone()]
        } else {
            entry.plates.clone()
        };
        ensure!(
            texts.join(" ") == entry.korean,
            "caption plates disagree with text: {}",
            entry.korean
        );
        let (protected_end, joined) = protected(id);
        let mut pixels = source_pixels.clone();
        for y in 0..HEIGHT {
            for x in protected_end..WIDTH {
                pixels[y * WIDTH + x] = 0;
            }
        }
        // A joined plate continues the source plate rows at the protected edge.
        let (top, bottom) = if joined {
            let rows = (0..HEIGHT)
                .filter(|&y| {
                    let p = source_pixels[y * WIDTH + protected_end - 1];
                    p != 0 && p != black
                })
                .collect::<Vec<_>>();
            ensure!(!rows.is_empty(), "joined caption plate missing");
            (rows[0], *rows.last().unwrap())
        } else {
            (PLATE_TOP, PLATE_BOTTOM)
        };
        let space = entry.space.unwrap_or(SPACE);
        ensure!((2..=SPACE).contains(&space), "caption space outside 2..=4");
        let phrases = texts
            .iter()
            .map(|t| phrase_with_digits(&font, dy, t, space))
            .collect::<Result<Vec<_>>>()?;
        // A narrowed caption also narrows the right plate margin to the left one.
        let pad_right = if entry.space.is_some() {
            PAD_LEFT
        } else {
            PAD_RIGHT
        };
        // (plate width, left padding): a number-only plate is widened to
        // NUMBER_PLATE with the number centered, like the source number plates.
        let mut layout = phrases
            .iter()
            .zip(&texts)
            .map(|((_, w), t)| {
                let tight = w + PAD_LEFT + pad_right;
                if t.bytes().all(|b| b.is_ascii_digit()) && tight < NUMBER_PLATE {
                    (NUMBER_PLATE, (NUMBER_PLATE - w).div_ceil(2))
                } else {
                    (tight, PAD_LEFT)
                }
            })
            .collect::<Vec<_>>();
        if joined {
            layout[0].0 += 2;
        }
        let widths = layout.iter().map(|l| l.0).collect::<Vec<_>>();
        let total = widths.iter().sum::<usize>() + GAP * (widths.len() - 1) + SHADOW;
        let region = WIDTH - protected_end - if protected_end > 0 && !joined { 2 } else { 0 };
        ensure!(
            total <= region,
            "caption exceeds plate width ({total} > {region}): {}",
            entry.korean
        );
        let mut x = if joined {
            protected_end
        } else if protected_end > 0 {
            protected_end + 2 + (region - total) / 2
        } else {
            (WIDTH - total) / 2
        };
        let mut plate_boxes = Vec::new();
        for (k, ((ink, _), &(width, pad))) in phrases.iter().zip(&layout).enumerate() {
            let (x0, x1) = (x, x + width - 1);
            let continues = joined && k == 0;
            let corner = |xx: usize, yy: usize| {
                (yy == top || yy == bottom) && (xx == x1 || (xx == x0 && !continues))
            };
            // Shadow: the plate shape moved two pixels right and down, drawn only
            // on transparent pixels. A joined plate keeps the source shadow line.
            for yy in top..=bottom {
                for xx in x0..=x1 {
                    if corner(xx, yy) {
                        continue;
                    }
                    let (sx, sy) = (xx + SHADOW, yy + SHADOW);
                    let shift_left = if continues { SHADOW.min(xx - x0) } else { 0 };
                    let sx = sx - shift_left;
                    ensure!(sx < WIDTH && sy < HEIGHT, "caption shadow outside canvas");
                    if pixels[sy * WIDTH + sx] == 0 {
                        pixels[sy * WIDTH + sx] = black;
                    }
                }
            }
            for yy in top..=bottom {
                for xx in x0..=x1 {
                    if !corner(xx, yy) {
                        pixels[yy * WIDTH + xx] = cream;
                    }
                }
            }
            let ox = x0 + pad + if continues { 2 } else { 0 };
            for &(gx, gy) in ink {
                for hy in gy - 1..=gy + 1 {
                    for hx in ox + gx - 1..=ox + gx + 1 {
                        if hx >= x0 && hx <= x1 && pixels[hy * WIDTH + hx] == cream {
                            pixels[hy * WIDTH + hx] = white;
                        }
                    }
                }
            }
            for &(gx, gy) in ink {
                ensure!(ox + gx < x1, "caption ink outside plate");
                pixels[gy * WIDTH + ox + gx] = black;
            }
            plate_boxes.push([x0, top, x1 + 1, bottom + 1]);
            x = x1 + 1 + GAP;
        }
        let raw = pixels
            .chunks_exact(2)
            .map(|p| p[0] | p[1] << 4)
            .collect::<Vec<_>>();
        ensure!(raw[1408..] == old[1408..], "caption tail row changed");
        for y in 0..HEIGHT {
            ensure!(
                raw[y * 64..y * 64 + protected_end / 2] == old[y * 64..y * 64 + protected_end / 2],
                "source icon changed"
            );
        }
        battle_ui::compress_member(&mut replacements, member, &raw)?;
        ensure!(
            replacements[&member].len() <= n.members[member].len(),
            "caption {id} exceeds stored capacity"
        );
        for (i, &index) in pixels.iter().take(WIDTH * HEIGHT).enumerate() {
            if index == 0 {
                continue;
            }
            let c = color(index as usize)?;
            let at = (id * 28 * WIDTH + i) * 4;
            preview[at..at + 4].copy_from_slice(&[
                ((c & 31) * 255 / 31) as u8,
                (((c >> 5) & 31) * 255 / 31) as u8,
                (((c >> 10) & 31) * 255 / 31) as u8,
                255,
            ]);
        }
        records.push(json!({"id":id,"member":member,"palette_member":member-1,"source_pixels_sha256":sha(&old),"pixels_sha256":sha(&raw),"protected_left_columns":protected_end,"joined_source_plate":joined,"preserved_tail_bytes":64,"korean":entry.korean,"plates":plate_boxes,"palette_roles":{"plate":cream,"halo":white,"ink_and_shadow":black},"stored_bytes":replacements[&member].len(),"capacity":n.members[member].len()}));
    }
    let rebuilt = battle_ui::rebuilt(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("captions.narc"), &rebuilt)?;
    write_png(&out.join("captions.png"), WIDTH, 112 * 28, &preview)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"captions.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"translation_sha256":sha(&input),"font_sha256":font_sha256,"source_sha256":sha(rom.bytes),"archive_sha256":sha(&rebuilt),"records":records,"runtime_verified":false,"human_reviewed":false,"profile":"Galmuri11 Regular 12px widened 1px right where no 1px gap closes; digits also thickened 1px down under the same rule, and number-only plates at least 15px wide with the number centered; source-colored cream plates: 2/3px padding, 3px plate gap, white 1px halo, black ink and black plate shadow offset (2,2); source icons and tail row fixed"});
    json_file(&out.join("captions.json"), &report)?;
    Ok(report)
}
