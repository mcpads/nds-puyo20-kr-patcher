//! Resolve academy title nodes through resource -> size -> image indirection.
use super::*;
use crate::titles;

pub fn inspect(rom: &Rom, challenge: bool, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (report, layouts, _) = read_layouts(rom, challenge)?;
    fs::create_dir_all(out)?;
    for title in layouts {
        write_png(
            &out.join(format!("{}.png", title.name)),
            title.width,
            title.height,
            &titles::rgba(&title.pixels, &title.palette)?,
        )?;
    }
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

/// Where each title part of a scene is stored in its Gem, for moving a part.
struct Placement {
    gem_member: usize,
    scene: usize,
    /// Node offset of each part, in `Title::parts` order.
    nodes: Vec<usize>,
    /// Gem x of canvas column 0.
    left: i64,
}

type Layouts = (Value, Vec<titles::Title>, Vec<Placement>);

fn read_layouts(rom: &Rom, challenge: bool) -> Result<Layouts> {
    let source = rom.data(rom.file("academy/academy.narc")?);
    ensure!(
        sha(source) == "b0c7244f4d3de4d9836a1e6607097bffdbfb63644f8e01e28f191f0ddd0a7ed3",
        "academy source changed"
    );
    let n = Narc::parse(source)?;
    let mut reports = Vec::new();
    let mut layouts = Vec::new();
    let mut placements = Vec::new();
    let surfaces = if challenge {
        vec![(
            "challenge_mondai",
            17,
            19,
            "9315e9ba9f19264f038e0d6122279b7afb33ef5885317998c0558a053cb92dd6",
            "e32bcda8424b0e5f28a8458f5d06fdc1c0647a5310a2e9671f2e04874f8d623e",
        )]
    } else {
        vec![
            (
                "nyumon",
                156,
                9,
                "c9ccea3af2fbef2891c2bad921b43a655b18fca03c760ede72523991d68b5586",
                "c3e18e10334499b81a2e3ffe7150eff100b4051169aaac2a34afa37ed606618e",
            ),
            (
                "jissen",
                74,
                12,
                "1e116f223f428dc3d699709dcc883e412bd0bac8c35a3fda5c9fc515a40ca288",
                "9360cde3653c215d132b13c2ad343ee3f675d998891848f2c9a94c6e7cd4746e",
            ),
        ]
    };
    for (surface, member, count, gem_hash, ilf_hash) in surfaces {
        let gem = unpack(n.members[member])?;
        let ilf = rom.data(rom.file(&format!("academy/{surface}_ingame_t_ilf.bin"))?);
        ensure!(
            sha(&gem) == gem_hash && sha(ilf) == ilf_hash,
            "academy layout identity changed"
        );
        ensure!(
            slice(&gem, 0, 4)? == b"Gem1" && u32le(&gem, 8)? == gem.len(),
            "invalid Gem1"
        );
        let base = u32le(&gem, 0x14)?;
        let gem2 = u32le(&gem, 0x58)?;
        ensure!(
            slice(&gem, gem2, 4)? == b"Gem2" && gem2 + u32le(&gem, gem2 + 0x14)? == base,
            "invalid Gem2 base"
        );
        let images = base + u32le(&gem, 0x2c)?;
        let sizes = base + u32le(&gem, 0x54)?;
        let image_count = u32le(&gem, 0x28)?;
        ensure!(
            image_count * 4 == ilf.len()
                && u32le(&gem, 0x50)? == image_count
                && u32le(&gem, 0x20)? == count,
            "academy population changed"
        );
        let mut image_records = Vec::new();
        let mut decoded = Vec::new();
        for id in 0..image_count {
            let packed = u32le(ilf, id * 4)?;
            let pixel_member = (packed >> 8) & 4095;
            let palette_member = packed >> 20;
            let descriptor = images + id * 32;
            let width = u16le(&gem, descriptor + 8)?;
            let height = u16le(&gem, descriptor + 10)?;
            ensure!(
                u32le(&gem, descriptor)? == packed & 255 && u32le(&gem, descriptor + 4)? == 3,
                "image identity changed"
            );
            let size = sizes + id * 16;
            ensure!(
                base + u32le(&gem, size + 4)? == descriptor
                    && u32le(&gem, size + 8)? == width << 16
                    && u32le(&gem, size + 12)? == height << 16,
                "size descriptor mismatch"
            );
            let bytes = unpack_halfword(n.members[pixel_member])?;
            let palette = unpack(n.members[palette_member])?;
            let bpp = match palette.len() {
                32 => 4,
                512 => 8,
                _ => anyhow::bail!("unsupported palette"),
            };
            let pixels = titles::untile(&bytes, width, height, bpp)?;
            ensure!(
                titles::tile(&pixels, width, height, bpp)? == bytes,
                "unchanged sprite round trip failed"
            );
            image_records.push(json!({"id":id,"member":pixel_member,"palette_member":palette_member,"width":width,"height":height,"bpp":bpp,"decoded_sha256":sha(&bytes)}));
            decoded.push((pixel_member, width, height, pixels, palette));
        }
        let mut cursor = u32le(&gem, 0x18)?;
        let mut scenes = Vec::new();
        let mut uses: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for index in 0..count {
            let scene = base + u32le(&gem, cursor)?;
            let len = slice(&gem, cursor + 4, 1)?[0] as usize;
            let name = std::str::from_utf8(slice(&gem, cursor + 5, len)?)?.to_owned();
            cursor = (cursor + 5 + len + 3) & !3;
            ensure!(
                scene == base + index * 64 && slice(&gem, scene, 4)? == b"Scen",
                "scene order changed"
            );
            if name == "navi_rng" {
                scenes.push(
                    json!({"name":name,"status":"protected character scene; not title artwork"}),
                );
                continue;
            }
            ensure!(
                name.starts_with("title_") && u32le(&gem, scene + 0x10)? == 1,
                "unsupported title banks"
            );
            let bank = base + u32le(&gem, scene + 0x14)?;
            let nodes = base + u32le(&gem, bank + 4)?;
            let node_count = u32le(&gem, bank)?;
            ensure!(
                (3..=7).contains(&node_count),
                "unsupported title part count"
            );
            let mut parts = Vec::new();
            for child in 1..node_count {
                let node = nodes + child * 80;
                let aux = base + u32le(&gem, node + 16)?;
                ensure!(
                    u32le(&gem, aux)? == u32le(&gem, node)?
                        && base + u32le(&gem, aux + 8)? == node
                        && base + u32le(&gem, node + 24)? == nodes
                        && u32le(&gem, node + 28)? == u32::MAX as usize,
                    "node backlink/hierarchy mismatch"
                );
                ensure!(
                    u32le(&gem, node + 40)? == 4096
                        && u32le(&gem, node + 44)? == 4096
                        && u32le(&gem, node + 48)? == 0,
                    "nonidentity title transform"
                );
                let resource = base + u32le(&gem, aux + 20)?;
                let size = base + u32le(&gem, resource)?;
                ensure!(
                    size >= sizes && (size - sizes) % 16 == 0 && (size - sizes) / 16 < image_count,
                    "resource size pointer outside table"
                );
                let id = (size - sizes) / 16;
                let (pixel_member, w, h, _, _) = &decoded[id];
                ensure!(
                    u16le(&gem, aux + 24)? == *w && u16le(&gem, aux + 26)? == *h,
                    "node/resolved image dimensions differ"
                );
                let fixed = |offset| -> Result<i64> {
                    let v = u32le(&gem, offset)? as u32 as i32;
                    ensure!(v % 4096 == 0, "fractional title position");
                    Ok(i64::from(v / 4096))
                };
                let x = fixed(node + 32)? - fixed(node + 8)?;
                let y = fixed(node + 36)? - fixed(node + 12)?;
                uses.entry(*pixel_member).or_default().push(name.clone());
                parts.push((id, x, y, node));
            }
            placements.push(Placement {
                gem_member: member,
                scene,
                nodes: parts.iter().map(|p| p.3).collect(),
                left: parts.iter().map(|p| p.1).min().unwrap(),
            });
            let parts = parts
                .into_iter()
                .map(|(id, x, y, _)| (id, x, y))
                .collect::<Vec<_>>();
            let left = parts.iter().map(|p| p.1).min().unwrap();
            let top = parts.iter().map(|p| p.2).min().unwrap();
            let width = parts
                .iter()
                .map(|p| (p.1 - left) as usize + decoded[p.0].1)
                .max()
                .unwrap();
            let height = parts
                .iter()
                .map(|p| (p.2 - top) as usize + decoded[p.0].2)
                .max()
                .unwrap();
            ensure!(width <= 256 && height <= 64, "title canvas exceeds bounds");
            let palette = &decoded[parts[0].0].4;
            ensure!(palette.len() == 32, "title is not I4");
            let mut canvas = vec![0u8; width * height];
            let mut mapping = Vec::new();
            let mut title_parts = Vec::new();
            for (id, x, y) in parts {
                let (pixel_member, w, h, pixels, pal) = &decoded[id];
                ensure!(pal == palette, "mixed title palettes");
                let x = (x - left) as usize;
                let y = (y - top) as usize;
                for py in 0..*h {
                    for px in 0..*w {
                        let v = pixels[py * w + px];
                        let at = (y + py) * width + x + px;
                        if v != 0 {
                            ensure!(
                                canvas[at] == 0 || canvas[at] == v,
                                "overlapping title pixels disagree"
                            );
                            canvas[at] = v;
                        }
                    }
                }
                title_parts.push(titles::Part {
                    member: *pixel_member,
                    width: *w,
                    height: *h,
                    x,
                    y,
                });
                mapping.push(
                    json!({"image":id,"member":pixel_member,"x":x,"y":y,"width":w,"height":h}),
                );
            }
            layouts.push(titles::Title {
                name: name.clone(),
                width,
                height,
                pixels: canvas,
                palette: palette.clone(),
                parts: title_parts,
            });
            scenes.push(json!({"name":name,"width":width,"height":height,"origin":[left,top],"parts":mapping}));
        }
        reports.push(json!({"surface":surface,"gem_member":member,"gem_sha256":gem_hash,"ilf_sha256":ilf_hash,"images":image_records,"scenes":scenes,"shared_title_members":uses.into_iter().filter(|(_,names)|names.len()>1).collect::<BTreeMap<_,_>>()}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"archive_sha256":sha(source),"surfaces":reports,"claim":"static indirect resource mapping, source tile round trips and title composition; no product changes or runtime rendering proof"});
    Ok((report, layouts, placements))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogoTranslations {
    state: String,
    entries: Vec<LogoLabel>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogoLabel {
    scene: String,
    korean: Vec<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    image_sha256: Option<String>,
    #[serde(default)]
    palette_indices: Vec<usize>,
    #[serde(default)]
    image_pieces: Vec<crate::art_pixels::SheetPiece>,
    #[serde(default)]
    image_shadow: Option<ImageShadow>,
    /// Font lettering with the source colour roles, instead of an image.
    #[serde(default)]
    lettering: Option<Lettering>,
    /// Move the scene's own last sprite left so it holds the trailing number
    /// drawn right after the text (the Gem node x of that sprite changes).
    #[serde(default)]
    move_number: bool,
}

/// Course-title lettering: coloured face, diamond outline, shadow (+1,+1)/(+1,+2).
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Lettering {
    size: usize,
    digit_size: usize,
    /// 0 is upright; otherwise each row above the baseline moves right by 1/slant px.
    slant: usize,
    /// `[x, top]` of each line: first ink column and first hangul row.
    lines: Vec<[usize; 2]>,
    /// Outline radius (diamond, |dx|+|dy| <= radius) around the face.
    outline: usize,
    /// Extra pixels after each glyph.
    tracking: i32,
    /// Thicken the face one pixel to the right (the challenge titles use a
    /// heavy face; the course titles a thin one).
    #[serde(default)]
    bold: bool,
    /// Horizontal condensing of the non-digit glyphs (0.8..=1.0) for long titles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale_x: Option<f32>,
    /// Word space in pixels instead of the face's own space advance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    space: Option<usize>,
    /// Draw the letters with the small face (`--small-font`, Galmuri11 12px)
    /// instead of `--font`, for a title too long for the large face uncondensed.
    #[serde(default)]
    small: bool,
    /// With `bold`, widen a column only where no one-pixel gap closes (the small
    /// face keeps ㅃ halves and glyph gaps open).
    #[serde(default)]
    keep_gaps: bool,
    face_rgb555: [u16; 3],
    outline_rgb555: [u16; 3],
    shadow_rgb555: [u16; 3],
}

fn palette_index(palette: &[u8], rgb: [u16; 3]) -> Result<u8> {
    let value = rgb[0] | rgb[1] << 5 | rgb[2] << 10;
    (1..16)
        .find(|&i| u16le(palette, i * 2).ok() == Some(value as usize))
        .map(|i| i as u8)
        .ok_or_else(|| anyhow::anyhow!("lettering colour {rgb:?} not in source palette"))
}

/// Pixels of the lettering and the first column of the trailing number, if any.
fn lettering_pixels(
    font: &fontdue::Font,
    digit_font: Option<&fontdue::Font>,
    l: &Lettering,
    lines: &[String],
    palette: &[u8],
    width: usize,
    height: usize,
) -> Result<(Vec<u8>, Option<usize>)> {
    ensure!(l.lines.len() == lines.len(), "lettering line count");
    let (face, outline, shadow) = (
        palette_index(palette, l.face_rgb555)?,
        palette_index(palette, l.outline_rgb555)?,
        palette_index(palette, l.shadow_rgb555)?,
    );
    let (m, _) = font.rasterize('가', l.size as f32);
    let mut body = std::collections::BTreeSet::new();
    let mut number_left = None;
    for (line, (text, &[x0, top])) in lines.iter().zip(&l.lines).enumerate() {
        let baseline = (top + m.height) as i32 + m.ymin;
        let trailing = text.len() - text.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let number_from = text.chars().count() - trailing;
        let mut cursor = x0 as f32;
        let mut ink = Vec::new();
        let mut number = None;
        for (i, ch) in text.chars().enumerate() {
            let size = if ch.is_ascii_digit() {
                l.digit_size
            } else {
                l.size
            } as f32;
            if ch == ' ' {
                cursor += match l.space {
                    Some(space) => space as f32,
                    None => font.metrics(ch, size).advance_width * l.scale_x.unwrap_or(1.0),
                };
                continue;
            }
            // Numbers use the bold digit face when one is given.
            let face = if ch.is_ascii_digit() {
                digit_font.unwrap_or(font)
            } else {
                font
            };
            ensure!(
                face.lookup_glyph_index(ch) != 0,
                "missing lettering glyph {ch}"
            );
            let (g, bitmap) = face.rasterize(ch, size);
            // Condense a pixel glyph by removing columns identical to their left
            // neighbour (inside bars and blank bearings), so strokes and counters
            // keep their shape; digits stay full width.
            let scale = if ch.is_ascii_digit() {
                1.0
            } else {
                l.scale_x.unwrap_or(1.0)
            };
            ensure!(
                (0.8..=1.0).contains(&scale),
                "lettering scale_x outside 0.8..=1.0"
            );
            let mut columns = (0..g.width)
                .map(|x| {
                    (0..g.height)
                        .map(|y| bitmap[y * g.width + x] >= 128)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let target = ((g.width as f32 * scale).round() as usize).max(1);
            while columns.len() > target {
                // Longest run of identical columns; ties go to the glyph centre.
                let centre = columns.len() as i32 / 2;
                let mut best: Option<(usize, usize)> = None;
                let mut run = 1;
                for c in 1..columns.len() {
                    // Only inked columns (bar interiors): removing a blank gap column would
                    // close the space between two strokes.
                    run = if columns[c] == columns[c - 1] && columns[c].iter().any(|&v| v) {
                        run + 1
                    } else {
                        1
                    };
                    // Runs of three or more identical columns are long bars; shorter runs
                    // are the space inside ㅐ-like pairs and must stay.
                    if run > 2 {
                        let better = match best {
                            None => true,
                            Some((r, b)) => {
                                run > r
                                    || (run == r
                                        && (c as i32 - centre).abs() < (b as i32 - centre).abs())
                            }
                        };
                        if better {
                            best = Some((run, c));
                        }
                    }
                }
                // Stop when only distinct columns remain; the canvas check
                // still rejects a title that does not fit.
                let Some((_, c)) = best else { break };
                columns.remove(c);
            }
            let width = columns.len();
            let mut bitmap_scaled = vec![0u8; width * g.height];
            for (x, column) in columns.iter().enumerate() {
                for (y, &on) in column.iter().enumerate() {
                    bitmap_scaled[y * width + x] = if on { 255 } else { 0 };
                }
            }
            let bitmap = bitmap_scaled;
            // The number starts at its pen position, the same for every digit, so
            // scenes sharing the text move their number sprites identically.
            if trailing > 0 && i == number_from {
                number = Some(cursor.round() as i32);
            }
            for y in 0..g.height {
                for x in 0..width {
                    if bitmap[y * width + x] < 128 {
                        continue;
                    }
                    // Digits from the digit face share the hangul bottom row instead of
                    // the baseline, so a number stands as tall as the letters.
                    let py = if ch.is_ascii_digit() && digit_font.is_some() {
                        (top + m.height) as i32 - g.height as i32 + y as i32
                    } else {
                        baseline - g.ymin - g.height as i32 + y as i32
                    };
                    let lean = if l.slant == 0 {
                        0
                    } else {
                        (baseline - py).div_euclid(l.slant as i32)
                    };
                    let px = cursor.round() as i32
                        + (g.xmin as f32 * scale).round() as i32
                        + x as i32
                        + lean;
                    ink.push((px, py, ch.is_ascii_digit()));
                }
            }
            cursor += g.advance_width * scale + l.tracking as f32;
        }
        // The first ink column sits exactly at x0 whatever the glyph bearing.
        let first = ink
            .iter()
            .map(|p| p.0)
            .min()
            .ok_or_else(|| anyhow::anyhow!("empty lettering line"))?;
        let shift = x0 as i32 - first;
        body.extend(ink.iter().map(|&(x, y, _)| (x + shift, y)));
        if l.bold {
            // Thicken letter strokes only; the digit face is already bold.
            let letters = ink
                .iter()
                .filter(|p| !p.2 || digit_font.is_none())
                .map(|&(x, y, _)| (x + shift, y))
                .collect::<std::collections::BTreeSet<_>>();
            body.extend(
                letters
                    .iter()
                    .filter(|&&(x, y)| {
                        !l.keep_gaps
                            || (!letters.contains(&(x + 1, y)) && !letters.contains(&(x + 2, y)))
                    })
                    .map(|&(x, y)| (x + 1, y)),
            );
        }
        if line + 1 == lines.len() {
            number_left = number.map(|n| n + shift);
        }
    }
    let mut pixels = vec![0u8; width * height];
    let mut put = |x: i32, y: i32, v: u8, only_empty: bool| -> Result<()> {
        ensure!(
            x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height,
            "lettering outside canvas at ({x},{y}) of {width}x{height}"
        );
        let at = y as usize * width + x as usize;
        if !only_empty || pixels[at] == 0 {
            pixels[at] = v;
        }
        Ok(())
    };
    ensure!((1..=2).contains(&l.outline), "lettering outline radius");
    let r = l.outline as i32;
    let mut outer = body.clone();
    for &(x, y) in &body {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() + dy.abs() <= r {
                    outer.insert((x + dx, y + dy));
                }
            }
        }
    }
    for &(x, y) in &outer {
        put(x, y, outline, false)?;
    }
    for &(x, y) in &body {
        put(x, y, face, false)?;
    }
    for &(x, y) in &outer {
        put(x + 1, y + 1, shadow, true)?;
        put(x + 1, y + 2, shadow, true)?;
    }
    Ok((pixels, number_left.map(|n| n.max(0) as usize)))
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ImageShadow {
    offset: [usize; 2],
    palette_index: usize,
    expected_rgb555: usize,
}

pub fn prepare(
    rom: &Rom,
    challenge: bool,
    translation: &Path,
    font_path: &Path,
    small_font_path: Option<&Path>,
    digit_font_path: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (mapping, layouts, placements) = read_layouts(rom, challenge)?;
    let input = fs::read(translation)?;
    let tr: LogoTranslations = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == layouts.len(),
        "expected all 19 title drafts"
    );
    let bytes = fs::read(font_path)?;
    let lettering_font =
        sha(&bytes) == crate::fonts::BMJUA_SHA256 || sha(&bytes) == crate::fonts::GALMURI14_SHA256;
    let digit_font = match digit_font_path {
        Some(path) => {
            let bytes = fs::read(path)?;
            ensure!(
                sha(&bytes) == crate::fonts::BMJUA_SHA256,
                "digit font identity changed"
            );
            Some(
                fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                    .map_err(|e| anyhow::anyhow!(e))?,
            )
        }
        None => None,
    };
    ensure!(
        crate::fonts::is_galmuri11(&bytes) || lettering_font,
        "font identity changed"
    );
    let small_font = match small_font_path {
        Some(path) => {
            let bytes = fs::read(path)?;
            ensure!(
                crate::fonts::is_galmuri11(&bytes),
                "small font identity changed"
            );
            Some(
                fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                    .map_err(|e| anyhow::anyhow!(e))?,
            )
        }
        None => None,
    };
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let archive = "academy/academy.narc";
    let source = rom.data(rom.file(archive)?);
    let n = Narc::parse(source)?;
    let mut raw_members = BTreeMap::new();
    let mut changes = BTreeMap::new();
    let mut previews = Vec::new();
    let mut records = Vec::new();
    let mut gem_moves: BTreeMap<usize, Vec<(usize, i32)>> = BTreeMap::new();
    let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
    for title in &layouts {
        for part in &title.parts {
            *uses.entry(part.member).or_default() += 1;
        }
    }
    for ((title, label), placement) in layouts.iter().zip(&tr.entries).zip(&placements) {
        ensure!(label.scene == title.name, "title order/identity changed");
        let double = title.height == 64;
        let scale = if double { 2 } else { 1 };
        let lines = if matches!(
            title.name.as_str(),
            "title_nyumon_B" | "title_jissen_G" | "title_challenge_test"
        ) {
            2
        } else {
            1
        };
        ensure!(label.korean.len() == lines, "title line count changed");
        let mut ink = Vec::new();
        ensure!(
            label.image.is_some() == label.image_sha256.is_some(),
            "generated logo identity required"
        );
        ensure!(
            label.image.is_some() || label.image_pieces.is_empty(),
            "title pieces require image"
        );
        ensure!(
            label.lettering.is_none() || label.image.is_none(),
            "lettering replaces the image"
        );
        ensure!(
            label.lettering.is_some() || label.image.is_some() || !lettering_font,
            "font drafts need Galmuri11"
        );
        if label.image.is_none() && label.lettering.is_none() {
            for (line, text) in label.korean.iter().enumerate() {
                let baseline = if double && lines == 2 {
                    14 + line * 12
                } else if double {
                    20
                } else if lines == 2 {
                    16 + line * 14
                } else {
                    21
                };
                // Paired courses share prefix sprites. Use the same centering width
                // even when the font gives 1 and 2 different advances.
                let paired = (title.name.starts_with("title_test_")
                    && title.name != "title_test_P")
                    || matches!(
                        title.name.as_str(),
                        "title_jissen_C"
                            | "title_jissen_D"
                            | "title_jissen_E"
                            | "title_jissen_F"
                            | "title_jissen_H"
                            | "title_jissen_I"
                    );
                let text_width = |t: &str| {
                    t.chars()
                        .map(|ch| {
                            if ch == ' ' {
                                5
                            } else {
                                font.metrics(ch, 12.0).advance_width.round() as usize
                            }
                        })
                        .sum::<usize>()
                };
                let shift = if paired {
                    let normalized = text.replace(['1', '3'], "2");
                    let normal_width = text_width(&normalized);
                    ensure!(
                        normal_width + 2 <= title.width,
                        "paired title exceeds width"
                    );
                    let left = if challenge {
                        // Several challenge variants reserve only the final 16px
                        // sprite for their number; keep the number in that sprite.
                        ensure!(normal_width + 4 <= title.width, "challenge title margin");
                        title.width - normal_width - 4
                    } else {
                        (title.width - normal_width) / 2
                    };
                    left as isize - ((title.width - text_width(text)) / 2) as isize
                } else {
                    0
                };
                for (x, y) in battle_ui::text_ink(
                    &font,
                    text,
                    12,
                    [0, 0, title.width / scale, title.height / scale],
                    baseline,
                )? {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let shifted_x = x as isize + shift;
                            ensure!(shifted_x >= 0, "paired title shift outside canvas");
                            ink.push((shifted_x as usize * scale + dx, y * scale + dy));
                        }
                    }
                }
            }
        }
        let mut colors = Vec::new();
        for i in 1..16 {
            if layouts
                .iter()
                .filter(|other| other.palette == title.palette)
                .any(|other| other.pixels.contains(&(i as u8)))
            {
                let c = crate::buttons::rgb(&title.palette, i)?;
                colors.push((
                    i as u8,
                    c.iter().map(|v| v * v).sum::<i32>(),
                    c.iter().map(|v| (31 - v).pow(2)).sum::<i32>(),
                ));
            }
        }
        let dark = colors
            .iter()
            .min_by_key(|c| c.1)
            .ok_or_else(|| anyhow::anyhow!("empty source palette"))?
            .0;
        let white = colors.iter().min_by_key(|c| c.2).unwrap().0;
        ensure!(dark != white, "missing title contrast");
        let mut pixels = vec![0u8; title.width * title.height];
        for &(x, y) in &ink {
            for dy in -1i32..=2 {
                for dx in -1i32..=1 {
                    let px = x as i32 + dx;
                    let py = y as i32 + dy;
                    ensure!(
                        px >= 0 && py >= 0 && px < title.width as i32 && py < title.height as i32,
                        "outline outside canvas"
                    );
                    pixels[py as usize * title.width + px as usize] = dark;
                }
            }
        }
        for (x, y) in ink {
            pixels[y * title.width + x] = white;
        }
        if let Some(path) = &label.image {
            let bytes = fs::read(path)?;
            ensure!(
                Some(sha(&bytes)) == label.image_sha256,
                "generated logo image changed"
            );
            let image = crate::art_pixels::read(&bytes)?;
            let rgba = if label.image_pieces.is_empty() {
                crate::art_pixels::reduce(&image, title.width, title.height, true)?
            } else {
                crate::art_pixels::assemble(&image, title.width, title.height, &label.image_pieces)?
            };
            ensure!(
                label.palette_indices.iter().all(|&i| i > 0 && i < 16),
                "logo palette subset outside palette"
            );
            let candidates = if label.palette_indices.is_empty() {
                (1..16).collect::<Vec<_>>()
            } else {
                label.palette_indices.clone()
            };
            for (pixel, color) in pixels.iter_mut().zip(rgba.chunks_exact(4)) {
                *pixel = if color[3] < 128 {
                    0
                } else {
                    candidates
                        .iter()
                        .copied()
                        .min_by_key(|&i| {
                            let c = u16le(&title.palette, i * 2).unwrap();
                            (0..3)
                                .map(|k| {
                                    let d = ((c >> (k * 5)) & 31) as i32 * 255 / 31
                                        - i32::from(color[k]);
                                    d * d
                                })
                                .sum::<i32>()
                        })
                        .unwrap() as u8
                };
            }
        }
        if let Some(shadow) = &label.image_shadow {
            let [dx, dy] = shadow.offset;
            ensure!(
                label.image.is_some() && dx <= 2 && dy <= 2 && dx + dy > 0,
                "invalid generated title shadow"
            );
            ensure!(
                shadow.palette_index > 0
                    && shadow.palette_index < 16
                    && u16le(&title.palette, shadow.palette_index * 2)? == shadow.expected_rgb555,
                "title shadow palette mismatch"
            );
            let foreground = pixels.clone();
            for (p, &color) in foreground.iter().enumerate() {
                if color == 0 {
                    continue;
                }
                let (x, y) = (p % title.width + dx, p / title.width + dy);
                ensure!(
                    x < title.width && y < title.height,
                    "title shadow outside canvas"
                );
                let q = y * title.width + x;
                if foreground[q] == 0 {
                    pixels[q] = shadow.palette_index as u8;
                }
            }
        }
        let mut number_left = None;
        if let Some(l) = &label.lettering {
            let letter_font = if l.small {
                small_font
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("small lettering needs --small-font"))?
            } else {
                &font
            };
            let (lettered, number) = lettering_pixels(
                letter_font,
                digit_font.as_ref(),
                l,
                &label.korean,
                &title.palette,
                title.width,
                title.height,
            )
            .map_err(|e| anyhow::anyhow!("{}: {e}", title.name))?;
            pixels = lettered;
            number_left = number;
        }
        // Part rectangles (x, y, width, height); the scene's own last sprite may
        // move left to carry the number that follows the text.
        let mut rects = title
            .parts
            .iter()
            .map(|p| (p.x, p.y, p.width, p.height))
            .collect::<Vec<_>>();
        let mut moved = None;
        if label.move_number {
            let last = rects.len() - 1;
            let part = &title.parts[last];
            ensure!(
                uses[&part.member] == 1,
                "{}: number sprite {} is shared",
                title.name,
                part.member
            );
            let digit =
                number_left.ok_or_else(|| anyhow::anyhow!("{}: no trailing number", title.name))?;
            // The sprite must also hold the number outline.
            let margin = label.lettering.as_ref().map_or(1, |l| l.outline + 1);
            let x = digit.saturating_sub(margin);
            if x < part.x {
                // Only root nodes carry animation channels in these Gems; a moved
                // child keeps its static offset at run time.
                let gem = unpack(n.members[placement.gem_member])?;
                let base = u32le(&gem, 0x14)?;
                let anims = base + u32le(&gem, placement.scene + 0x1c)?;
                for a in 0..u32le(&gem, placement.scene + 0x18)? {
                    let list = base + u32le(&gem, anims + a * 32 + 8)?;
                    ensure!(
                        u32le(&gem, list + 4)? == placement.nodes.len() + 1,
                        "animation node count"
                    );
                    let records = base + u32le(&gem, list + 8)?;
                    ensure!(
                        u32le(&gem, records + (last + 1) * 32 + 4)? == 0,
                        "{}: number sprite is animated",
                        title.name
                    );
                }
                let node = placement.nodes[last];
                let stored = u32le(&gem, node + 32)? as u32 as i32;
                let anchor = u32le(&gem, node + 8)? as u32 as i32;
                ensure!(
                    i64::from(stored / 4096 - anchor / 4096) - placement.left == part.x as i64,
                    "number sprite position disagrees with Gem"
                );
                let delta = (x as i32 - part.x as i32) * 4096;
                gem_moves
                    .entry(placement.gem_member)
                    .or_default()
                    .push((node + 32, delta));
                rects[last].0 = x;
                moved = Some(last);
            }
        }
        let mut covered = vec![false; pixels.len()];
        for (index, (part, &(px, py, pw, ph))) in title.parts.iter().zip(&rects).enumerate() {
            for y in py..py + ph {
                for x in px..px + pw {
                    covered[y * title.width + x] = true;
                }
            }
            let mut crop = battle_ui::crop(&pixels, title.width, px, py, pw, ph);
            // A moved number sprite owns the canvas under it; shared sprites stay
            // transparent there so every scene sharing them agrees.
            if let Some(m) = moved.filter(|&m| m != index) {
                let (mx, my, mw, mh) = rects[m];
                for y in 0..ph {
                    for x in 0..pw {
                        let (cx, cy) = (px + x, py + y);
                        if cx >= mx && cx < mx + mw && cy >= my && cy < my + mh {
                            crop[y * pw + x] = 0;
                        }
                    }
                }
            }
            let raw = titles::tile(&crop, pw, ph, 4)?;
            ensure!(
                titles::untile(&raw, pw, ph, 4)? == crop,
                "title tile round trip failed"
            );
            if let Some(previous) = raw_members.get(&part.member) {
                ensure!(
                    previous == &raw,
                    "shared member {} disagrees at {}",
                    part.member,
                    title.name
                );
            } else {
                battle_ui::compress_member(&mut changes, part.member, &raw)?;
                if changes[&part.member].len() > n.members[part.member].len() && raw.len() <= 4096 {
                    let packed = crate::compress::pack_compact(&raw)?;
                    ensure!(unpack_halfword(&packed)? == raw, "compact logo roundtrip");
                    changes.insert(part.member, packed);
                }
                raw_members.insert(part.member, raw);
            }
        }
        ensure!(
            pixels.iter().zip(covered).all(|(&p, c)| p == 0 || c),
            "{}: title ink outside sprite coverage",
            title.name
        );
        previews.push((
            title.name.clone(),
            title.width,
            title.height,
            titles::rgba(&pixels, &title.palette)?,
        ));
        records.push(json!({"scene":title.name,"korean":label.korean,"scale":scale,"ink_index":white,"outline_index":dark,"image":label.image,"image_sha256":label.image_sha256,"palette_indices":label.palette_indices,"image_pieces":label.image_pieces,"image_shadow":label.image_shadow,"lettering":label.lettering,"moved_number_sprite":moved.map(|m| json!({"member":title.parts[m].member,"x":rects[m].0,"source_x":title.parts[m].x}))}));
    }
    let mut gem_records = Vec::new();
    for (&member, moves) in &gem_moves {
        let mut gem = unpack(n.members[member])?;
        let before = sha(&gem);
        for &(offset, delta) in moves {
            let value = u32le(&gem, offset)? as u32 as i32 + delta;
            gem[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        // Keep the stored form: raw Gems stay raw, COMP Gems are recompressed.
        let packed = if n.members[member].starts_with(b"COMP") {
            let mut packed = crate::compress::pack(&gem)?;
            if packed.len() > n.members[member].len() && gem.len() <= 65536 {
                packed = crate::compress::pack_layout(&gem)?;
            }
            packed
        } else {
            gem.clone()
        };
        ensure!(unpack(&packed)? == gem, "Gem round trip");
        ensure!(
            packed.len() <= n.members[member].len(),
            "Gem {member} exceeds stored capacity"
        );
        let stored_compressed = packed.starts_with(b"COMP");
        ensure!(
            changes.insert(member, packed).is_none(),
            "Gem written twice"
        );
        gem_records.push(json!({"member":member,"stored_compressed":stored_compressed,"source_sha256":before,"decoded_sha256":sha(&gem),"node_x_writes":moves.iter().map(|(o,d)|json!({"offset":o,"delta_px":d/4096})).collect::<Vec<_>>()}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("logos.narc"), &rebuilt)?;
    for (name, w, h, pixels) in previews {
        write_png(&out.join(format!("{name}.png")), w, h, &pixels)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":archive,"expected_sha256":sha(source),"input":"logos.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let members = raw_members.iter().map(|(&member,raw)|json!({"member":member,"decoded_sha256":sha(raw),"stored_size":changes[&member].len(),"capacity":n.members[member].len()})).collect::<Vec<_>>();
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"archive_sha256":sha(&rebuilt),"mapping":mapping,"titles":records,"members":members,"gem_moves":gem_records,"protected":"Gem/ILF, palettes, character images, symbol assets and every non-title member","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("logos.json"), &report)?;
    Ok(report)
}

/// Report complete stored-image matches without inferring active screen consumption.
pub fn check_ram(rom: &Rom, surface: &str, ram: &[u8]) -> Result<Value> {
    if surface == "battle" {
        return super::battle_labels::check_ram(rom, ram);
    }
    let (hash, caption_range) = match surface {
        "nyumon" => (
            "c3e18e10334499b81a2e3ffe7150eff100b4051169aaac2a34afa37ed606618e",
            0..43,
        ),
        "jissen" => (
            "9360cde3653c215d132b13c2ad343ee3f675d998891848f2c9a94c6e7cd4746e",
            43..94,
        ),
        _ => anyhow::bail!("expected nyumon, jissen or battle"),
    };
    let ilf = rom.data(rom.file(&format!("academy/{surface}_ingame_t_ilf.bin"))?);
    ensure!(sha(ilf) == hash, "title ILF changed");
    let table = rom.data(rom.file("academy/lesson_item_s_texlist.bin")?);
    ensure!(
        sha(table) == "babcc3815cae7c516a521c99c7be3b1cf3487493493ced23dc556e70338be3d4",
        "caption table changed"
    );
    let archive = "academy/academy.narc";
    let n = Narc::parse(rom.data(rom.file(archive)?))?;
    let mut members = std::collections::BTreeSet::new();
    for id in (0..2).chain(8..ilf.len() / 4) {
        members.insert((u32le(ilf, id * 4)? >> 8) & 4095);
    }
    for id in caption_range {
        members.insert(u16le(table, id * 12)?);
    }
    let mut nonblank = Vec::new();
    let mut excluded = Vec::new();
    for member in members {
        let bytes = unpack_halfword(n.members[member])?;
        if bytes.iter().all(|&b| b == 0) {
            excluded.push(json!({"member":member,"reason":"blank payload cannot establish asset identity","decoded_size":bytes.len()}));
        } else {
            nonblank.push(member);
        }
    }
    let mut report = battle_ui::member_residency(rom, ram, &[(archive, nonblank)])?;
    report["surface"] = json!(surface);
    report["excluded"] = json!(excluded);
    report["ilf_sha256"] = json!(sha(ilf));
    report["caption_table_sha256"] = json!(sha(table));
    Ok(report)
}

/// Canvas positions `(pixel member, x, y)` of one Gem title scene as stored in
/// `rom`, without source identity checks, for comparing products whose layout
/// moved a part. Positions are relative to the scene's leftmost/topmost part.
pub fn scene_positions(
    rom: &Rom,
    archive: &str,
    gem_member: usize,
    ilf: &str,
    scene_name: &str,
) -> Result<Vec<(usize, usize, usize)>> {
    let n = Narc::parse(rom.data(rom.file(archive)?))?;
    let gem = unpack(n.members[gem_member])?;
    let list = rom.data(rom.file(ilf)?);
    ensure!(slice(&gem, 0, 4)? == b"Gem1", "invalid Gem1");
    let base = u32le(&gem, 0x14)?;
    let sizes = base + u32le(&gem, 0x54)?;
    let image_count = u32le(&gem, 0x28)?;
    ensure!(image_count * 4 == list.len(), "Gem/ILF count differs");
    let mut cursor = u32le(&gem, 0x18)?;
    for index in 0..u32le(&gem, 0x20)? {
        let scene = base + u32le(&gem, cursor)?;
        let len = slice(&gem, cursor + 4, 1)?[0] as usize;
        let name = std::str::from_utf8(slice(&gem, cursor + 5, len)?)?;
        cursor = (cursor + 5 + len + 3) & !3;
        ensure!(scene == base + index * 64, "scene order changed");
        if name != scene_name {
            continue;
        }
        let bank = base + u32le(&gem, scene + 0x14)?;
        let nodes = base + u32le(&gem, bank + 4)?;
        let fixed = |offset| -> Result<i64> {
            let v = u32le(&gem, offset)? as u32 as i32;
            ensure!(v % 4096 == 0, "fractional title position");
            Ok(i64::from(v / 4096))
        };
        let mut parts = Vec::new();
        for child in 1..u32le(&gem, bank)? {
            let node = nodes + child * 80;
            let aux = base + u32le(&gem, node + 16)?;
            let size = base + u32le(&gem, base + u32le(&gem, aux + 20)?)?;
            ensure!(
                size >= sizes && (size - sizes) % 16 == 0 && (size - sizes) / 16 < image_count,
                "resource size pointer outside table"
            );
            let member = (u32le(list, (size - sizes) / 16 * 4)? >> 8) & 4095;
            parts.push((
                member,
                fixed(node + 32)? - fixed(node + 8)?,
                fixed(node + 36)? - fixed(node + 12)?,
            ));
        }
        let left = parts.iter().map(|p| p.1).min().unwrap_or(0);
        let top = parts.iter().map(|p| p.2).min().unwrap_or(0);
        return Ok(parts
            .into_iter()
            .map(|(m, x, y)| (m, (x - left) as usize, (y - top) as usize))
            .collect());
    }
    anyhow::bail!("scene {scene_name} missing")
}
