use crate::{assets::json_file, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    #[serde(default)]
    source_sha256: Option<String>,
    entries: Vec<Label>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    surface: String,
    scene: String,
    korean: String,
    #[serde(default)]
    artwork: Option<Artwork>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artwork {
    image: String,
    image_sha256: String,
    region: [usize; 4],
    target: [usize; 4],
    #[serde(default)]
    palette_indices: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_matte: Option<crate::art_pixels::ProductionMatte>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fit: Option<bool>,
}

pub(crate) fn generated_pixels(
    width: usize,
    height: usize,
    palette: &[u8],
    art: &Artwork,
) -> Result<Vec<u8>> {
    let bytes = fs::read(&art.image)?;
    ensure!(sha(&bytes) == art.image_sha256, "title artwork identity");
    let mut image = crate::art_pixels::read(&bytes)?;
    if let Some(matte) = &art.image_matte {
        crate::art_pixels::remove_matte(&mut image, matte)?;
    }
    let image = crate::art_pixels::region(&image, art.region)?;
    let [x0, y0, x1, y1] = art.target;
    ensure!(
        x0 < x1 && y0 < y1 && x1 <= width && y1 <= height,
        "generated title target outside canvas"
    );
    let reduced = crate::art_pixels::reduce(&image, x1 - x0, y1 - y0, art.fit.unwrap_or(true))?;
    let colors: Vec<_> = (0..palette.len() / 2)
        .map(|i| u16le(palette, i * 2))
        .collect::<Result<_>>()?;
    ensure!(colors.len() <= 16, "generated title requires 4bpp");
    let candidates = if art.palette_indices.is_empty() {
        (1..colors.len()).collect::<Vec<_>>()
    } else {
        art.palette_indices.clone()
    };
    ensure!(
        !candidates.is_empty() && candidates.iter().all(|&i| i > 0 && i < colors.len()),
        "invalid title palette subset"
    );
    let quantized: Vec<u8> = reduced
        .chunks_exact(4)
        .map(|c| {
            if c[3] < 128 {
                return 0;
            }
            *candidates
                .iter()
                .min_by_key(|&&i| {
                    (0..3)
                        .map(|k| {
                            let d =
                                ((colors[i] >> (k * 5)) & 31) as i32 * 255 / 31 - i32::from(c[k]);
                            d * d
                        })
                        .sum::<i32>()
                })
                .unwrap() as u8
        })
        .collect();
    let mut pixels = vec![0; width * height];
    for y in y0..y1 {
        pixels[y * width + x0..y * width + x1]
            .copy_from_slice(&quantized[(y - y0) * (x1 - x0)..(y - y0 + 1) * (x1 - x0)]);
    }
    Ok(pixels)
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && !tr.entries.is_empty(),
        "expected title drafts"
    );
    ensure!(
        tr.source_sha256
            .as_ref()
            .is_none_or(|v| *v == sha(rom.bytes))
            && (!tr.entries.iter().any(|e| e.artwork.is_some()) || tr.source_sha256.is_some()),
        "generated titles require exact source identity"
    );
    let bytes = fs::read(font_path)?;
    ensure!(crate::fonts::is_galmuri11(&bytes), "font identity mismatch");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut labels = BTreeMap::new();
    for label in tr.entries {
        ensure!(
            labels
                .insert((label.surface.clone(), label.scene.clone()), label)
                .is_none(),
            "duplicate title draft"
        );
    }
    let mut plans = Vec::new();
    let mut records = Vec::new();
    let mut files = Vec::new();
    let mut previews = Vec::new();
    for (surface, &(name, path, _, _, _)) in SURFACES.iter().enumerate() {
        if !labels.keys().any(|(surface, _)| surface == name) {
            continue;
        }
        let (titles, mapping) = read(rom, surface)?;
        let source = rom.data(rom.file(path)?);
        let narc = Narc::parse(source)?;
        let mut replacements = BTreeMap::new();
        for title in titles {
            if title.name == "title_wifi" {
                continue;
            }
            let label = labels
                .remove(&(name.to_string(), title.name.clone()))
                .ok_or_else(|| anyhow::anyhow!("missing title draft"))?;
            let text = &label.korean;
            ensure!(!text.trim().is_empty(), "empty title");
            let (pixels, rendering) = if let Some(art) = &label.artwork {
                (
                    generated_pixels(title.width, title.height, &title.palette, art)?,
                    json!({"image":art.image,"image_sha256":art.image_sha256,"image_region":art.region,"image_target":art.target,"palette_indices":art.palette_indices}),
                )
            } else {
                let lines: Vec<_> = text.split('\n').collect();
                ensure!(
                    (1..=2).contains(&lines.len())
                        && lines.iter().all(|line| !line.trim().is_empty()),
                    "expected one or two title lines"
                );
                let mut width = 0;
                let mut ink = vec![false; title.width * title.height];
                // Keep native pixel shapes and the original one-line placement.
                for (row, line) in lines.iter().enumerate() {
                    let line_width = line
                        .chars()
                        .map(|c| {
                            if c == ' ' {
                                5
                            } else {
                                font.metrics(c, 12.0).advance_width.round() as usize
                            }
                        })
                        .sum::<usize>()
                        * 2;
                    ensure!(line_width + 12 <= title.width, "title text exceeds canvas");
                    width = width.max(line_width);
                    let mut cursor = (title.width - line_width) / 2;
                    let baseline = if lines.len() == 1 {
                        38
                    } else {
                        30 + row as i32 * 24
                    };
                    for c in line.chars() {
                        ensure!(font.lookup_glyph_index(c) != 0, "missing title glyph: {c}");
                        let (m, bitmap) = font.rasterize(c, 12.0);
                        for y in 0..m.height {
                            for x in 0..m.width {
                                if bitmap[y * m.width + x] < 128 {
                                    continue;
                                }
                                let px = cursor as i32 + 2 * (m.xmin + x as i32);
                                let py = baseline - 2 * (m.ymin + m.height as i32 - y as i32);
                                ensure!(
                                    px >= 6
                                        && px + 1 < title.width as i32 - 6
                                        && py >= 6
                                        && py + 1 < title.height as i32 - 8,
                                    "title ink outside canvas"
                                );
                                for dy in 0..2 {
                                    for dx in 0..2 {
                                        ink[(py as usize + dy) * title.width + px as usize + dx] =
                                            true;
                                    }
                                }
                            }
                        }
                        cursor += 2 * if c == ' ' {
                            5
                        } else {
                            m.advance_width.round() as usize
                        };
                    }
                }
                let mut used = std::collections::BTreeSet::new();
                used.extend(title.pixels.iter().copied().filter(|v| *v != 0));
                let mut white = (i32::MAX, 0);
                let mut dark = (i32::MAX, 0);
                for i in used {
                    let c = crate::buttons::rgb(&title.palette, i as usize)?;
                    let d = c.iter().map(|v| v * v).sum();
                    let w = c.iter().map(|v| (31 - v).pow(2)).sum();
                    if d < dark.0 {
                        dark = (d, i);
                    }
                    if w < white.0 {
                        white = (w, i);
                    }
                }
                ensure!(
                    white.1 != 0 && dark.1 != 0 && white.1 != dark.1,
                    "title palette lacks contrast"
                );
                let mut pixels = vec![0; ink.len()];
                // Dark outline follows the new Hangul silhouette; no original Japanese remains.
                for (p, &on) in ink.iter().enumerate() {
                    if !on {
                        continue;
                    }
                    let x = p % title.width;
                    let y = p / title.width;
                    for dy in -3_i32..=4 {
                        for dx in -3_i32..=3 {
                            if dx * dx + (dy - 1) * (dy - 1) <= 10 {
                                pixels[(y as i32 + dy) as usize * title.width
                                    + (x as i32 + dx) as usize] = dark.1;
                            }
                        }
                    }
                }
                for (p, &on) in ink.iter().enumerate() {
                    if on {
                        pixels[p] = white.1;
                    }
                }
                (
                    pixels,
                    json!({"ink_width":width,"ink_index":white.1,"outline_index":dark.1}),
                )
            };
            let mut members = Vec::new();
            for part in &title.parts {
                let mut cropped = Vec::new();
                for y in 0..part.height {
                    cropped.extend_from_slice(
                        &pixels[(part.y + y) * title.width + part.x
                            ..(part.y + y) * title.width + part.x + part.width],
                    );
                }
                let tiled = tile(&cropped, part.width, part.height, 4)?;
                ensure!(
                    untile(&tiled, part.width, part.height, 4)? == cropped,
                    "title tile round trip failed"
                );
                let packed = crate::compress::pack(&tiled)?;
                ensure!(
                    unpack_halfword(&packed)? == tiled,
                    "title halfword round trip failed"
                );
                ensure!(
                    packed.len() <= narc.members[part.member].len(),
                    "{} {} member {} capacity: {} > {}",
                    name,
                    title.name,
                    part.member,
                    packed.len(),
                    narc.members[part.member].len()
                );
                members.push(json!({"member":part.member,"stored_size":packed.len(),"capacity":narc.members[part.member].len(),"pixels_sha256":sha(&tiled)}));
                ensure!(
                    replacements.insert(part.member, packed).is_none(),
                    "shared title member"
                );
            }
            records.push(json!({"surface":name,"scene":title.name,"text":text,"width":title.width,"height":title.height,"ink_width":rendering.get("ink_width"),"ink_index":rendering.get("ink_index"),"outline_index":rendering.get("outline_index"),"rendering":rendering,"editable":"complete title pixel payloads; palette and Gem metadata unchanged","members":members}));
            previews.push((
                format!("{name}-{}.png", title.name),
                title.width,
                title.height,
                rgba(&pixels, &title.palette)?,
            ));
        }
        let rebuilt = crate::archive::replace(source, &replacements)?;
        let file = format!("{name}.narc");
        plans.push(json!({"file":path,"expected_sha256":sha(source),"input":file,"input_sha256":sha(&rebuilt)}));
        files.push((file, rebuilt));
        records.push(json!({"surface_mapping":mapping}));
    }
    ensure!(labels.is_empty(), "unused title drafts");
    fs::create_dir_all(out)?;
    for (name, bytes) in files {
        fs::write(out.join(name), bytes)?;
    }
    for (name, w, h, pixels) in previews {
        write_png(&out.join(name), w, h, &pixels)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":plans}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"records":records,"state":"development_art_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}

pub struct Title {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub palette: Vec<u8>,
    pub parts: Vec<Part>,
}
pub struct Part {
    pub member: usize,
    pub width: usize,
    pub height: usize,
    pub x: usize,
    pub y: usize,
}

pub const SURFACES: [(&str, &str, &str, usize, usize); 4] = [
    (
        "main",
        "menu/main_menu.narc",
        "menu/mainmenu_t_ilf.bin",
        41,
        5,
    ),
    (
        "option",
        "option/option_menu.narc",
        "option/option_menu_t_ilf.bin",
        14,
        3,
    ),
    (
        "single",
        "menu/single_common.narc",
        "menu/single_menu_t_ilf.bin",
        11,
        3,
    ),
    (
        "academy",
        "menu/academy_common.narc",
        "menu/lesson_menu_t_ilf.bin",
        19,
        6,
    ),
];

pub fn untile(bytes: &[u8], width: usize, height: usize, bpp: usize) -> Result<Vec<u8>> {
    ensure!(
        matches!(bpp, 4 | 8) && width % 8 == 0 && height % 8 == 0,
        "invalid tile geometry"
    );
    ensure!(
        bytes.len() == width * height * bpp / 8,
        "tile payload extent mismatch"
    );
    let mut pixels = vec![0; width * height];
    for y in 0..height {
        for x in 0..width {
            let p = ((y / 8) * (width / 8) + x / 8) * 64 + (y % 8) * 8 + x % 8;
            pixels[y * width + x] = if bpp == 8 {
                bytes[p]
            } else {
                (bytes[p / 2] >> ((p % 2) * 4)) & 15
            };
        }
    }
    Ok(pixels)
}

pub fn tile(pixels: &[u8], width: usize, height: usize, bpp: usize) -> Result<Vec<u8>> {
    ensure!(
        matches!(bpp, 4 | 8) && width % 8 == 0 && height % 8 == 0,
        "invalid tile geometry"
    );
    ensure!(pixels.len() == width * height, "pixel extent mismatch");
    let mut bytes = vec![0; width * height * bpp / 8];
    for y in 0..height {
        for x in 0..width {
            let p = ((y / 8) * (width / 8) + x / 8) * 64 + (y % 8) * 8 + x % 8;
            let v = pixels[y * width + x];
            if bpp == 8 {
                bytes[p] = v;
            } else {
                ensure!(v < 16, "4bpp palette overflow");
                bytes[p / 2] |= v << ((p % 2) * 4);
            }
        }
    }
    Ok(bytes)
}

pub fn rgba(pixels: &[u8], palette: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for &v in pixels {
        let c = u16le(palette, v as usize * 2)?;
        out.extend([
            ((c & 31) * 255 / 31) as u8,
            (((c >> 5) & 31) * 255 / 31) as u8,
            (((c >> 10) & 31) * 255 / 31) as u8,
            if v == 0 { 0 } else { 255 },
        ]);
    }
    Ok(out)
}

fn integer_fixed(bytes: &[u8], offset: usize) -> Result<i32> {
    let v = u32le(bytes, offset)? as i32;
    ensure!(v % 4096 == 0, "fractional title geometry");
    Ok(v / 4096)
}

/// Target-local Gem1/Gem2 subset. Animation tracks and non-title scenes remain opaque.
pub fn read(rom: &Rom, surface: usize) -> Result<(Vec<Title>, Value)> {
    read_layout(rom, SURFACES[surface])
}

pub(crate) fn read_layout(
    rom: &Rom,
    (label, archive, ilf_path, gem_member, title_count): (&str, &str, &str, usize, usize),
) -> Result<(Vec<Title>, Value)> {
    let narc = Narc::parse(rom.data(rom.file(archive)?))?;
    let gem = unpack(narc.members[gem_member])?;
    let ilf = rom.data(rom.file(ilf_path)?);
    ensure!(
        slice(&gem, 0, 4)? == b"Gem1" && u32le(&gem, 8)? == gem.len(),
        "unexpected Gem1 container"
    );
    let base = u32le(&gem, 0x14)?;
    let gem2 = u32le(&gem, 0x58)?;
    ensure!(
        slice(&gem, gem2, 4)? == b"Gem2" && gem2 + u32le(&gem, gem2 + 0x14)? == base,
        "Gem2 base mismatch"
    );
    let count = u32le(&gem, 0x28)?;
    ensure!(
        count * 4 == ilf.len() && count == u32le(&gem, 0x50)?,
        "Gem/ILF counts differ"
    );
    let images = base + u32le(&gem, 0x2c)?;
    let sizes = base + u32le(&gem, 0x54)?;
    let mut image_records = Vec::new();
    let mut decoded = Vec::new();
    for i in 0..count {
        let descriptor = images + i * 32;
        let size = sizes + i * 16;
        let packed = u32le(ilf, i * 4)?;
        let logical = packed & 255;
        let member = (packed >> 8) & 4095;
        let palette_member = packed >> 20;
        let width = u16le(&gem, descriptor + 8)?;
        let height = u16le(&gem, descriptor + 10)?;
        ensure!(
            u32le(&gem, descriptor)? == logical && u32le(&gem, descriptor + 4)? == 3,
            "Gem/ILF identity mismatch"
        );
        ensure!(
            base + u32le(&gem, size + 4)? == descriptor
                && u32le(&gem, size + 8)? == width << 16
                && u32le(&gem, size + 12)? == height << 16,
            "Gem size reference mismatch"
        );
        let old = unpack(
            narc.members
                .get(member)
                .ok_or_else(|| anyhow::anyhow!("image outside NARC"))?,
        )?;
        let palette = unpack(
            narc.members
                .get(palette_member)
                .ok_or_else(|| anyhow::anyhow!("palette outside NARC"))?,
        )?;
        let bpp = match palette.len() {
            32 => 4,
            512 => 8,
            _ => anyhow::bail!("unexpected sprite palette"),
        };
        let pixels = untile(&old, width, height, bpp)?;
        ensure!(
            tile(&pixels, width, height, bpp)? == old,
            "sprite unchanged round trip failed"
        );
        image_records.push(json!({"logical_id":logical,"member":member,"palette_member":palette_member,"width":width,"height":height,"bpp":bpp,"descriptor_offset":descriptor,"pixels_sha256":sha(&old),"palette_sha256":sha(&palette)}));
        decoded.push((member, width, height, pixels, palette));
    }
    let mut names = BTreeMap::new();
    let mut cursor = u32le(&gem, 0x18)?;
    for _ in 0..u32le(&gem, 0x20)? {
        let scene = base + u32le(&gem, cursor)?;
        let len = slice(&gem, cursor + 4, 1)?[0] as usize;
        let name = std::str::from_utf8(slice(&gem, cursor + 5, len)?)?.to_string();
        ensure!(names.insert(scene, name).is_none(), "duplicate named scene");
        cursor = (cursor + 5 + len + 3) & !3;
    }
    let mut titles = Vec::new();
    let mut scenes = Vec::new();
    let mut image_index = 0;
    let mut first_attribute = None;
    for i in 0..title_count {
        let scene = base + i * 64;
        ensure!(
            slice(&gem, scene, 4)? == b"Scen" && u32le(&gem, scene + 0x10)? == 1,
            "unexpected title scene banks"
        );
        let bank = base + u32le(&gem, scene + 0x14)?;
        let nodes = base + u32le(&gem, bank + 4)?;
        let node_count = u32le(&gem, bank)?;
        ensure!((3..=4).contains(&node_count), "unexpected title part count");
        let name = names
            .get(&scene)
            .ok_or_else(|| anyhow::anyhow!("unnamed title"))?
            .clone();
        ensure!(name.starts_with("title_"), "not a title scene");
        let mut parts = Vec::new();
        let mut positions = Vec::new();
        let first = image_index;
        for n in 1..node_count {
            let node = nodes + n * 80;
            let aux = base + u32le(&gem, node + 16)?;
            let (member, width, height, _, _) = &decoded[image_index];
            ensure!(
                u32le(&gem, aux)? == u32le(&gem, node)? && u32le(&gem, aux + 8)? + base == node,
                "title node backlink mismatch"
            );
            ensure!(
                u16le(&gem, aux + 24)? == *width && u16le(&gem, aux + 26)? == *height,
                "title node dimensions mismatch"
            );
            ensure!(
                u32le(&gem, node + 24)? + base == nodes
                    && u32le(&gem, node + 28)? == u32::MAX as usize,
                "unexpected title hierarchy"
            );
            ensure!(
                u32le(&gem, node + 40)? == 4096
                    && u32le(&gem, node + 44)? == 4096
                    && u32le(&gem, node + 48)? == 0,
                "nonidentity title transform"
            );
            let attr = u32le(&gem, aux + 20)?;
            let origin = *first_attribute.get_or_insert(attr);
            ensure!(
                attr == origin + image_index * 4,
                "unresolved title resource order"
            );
            let x = integer_fixed(&gem, node + 32)? - integer_fixed(&gem, node + 8)?;
            let y = integer_fixed(&gem, node + 36)? - integer_fixed(&gem, node + 12)?;
            positions.push((x, y));
            parts.push(Part {
                member: *member,
                width: *width,
                height: *height,
                x: 0,
                y: 0,
            });
            image_index += 1;
        }
        let min_x = positions.iter().map(|p| p.0).min().unwrap();
        let min_y = positions.iter().map(|p| p.1).min().unwrap();
        for (part, (x, y)) in parts.iter_mut().zip(&positions) {
            part.x = (x - min_x) as usize;
            part.y = (y - min_y) as usize;
        }
        let width = parts.iter().map(|p| p.x + p.width).max().unwrap();
        let height = parts.iter().map(|p| p.y + p.height).max().unwrap();
        let palette = decoded[first].4.clone();
        ensure!(palette.len() == 32, "title is not 4bpp");
        let mut pixels = vec![0; width * height];
        let mut overlap_conflicts = 0;
        for (n, part) in parts.iter().enumerate() {
            let (_, _, _, src, pal) = &decoded[first + n];
            ensure!(*pal == palette, "title palettes differ");
            for y in 0..part.height {
                for x in 0..part.width {
                    let v = src[y * part.width + x];
                    let p = (part.y + y) * width + part.x + x;
                    if v != 0 {
                        if pixels[p] != 0 && pixels[p] != v {
                            overlap_conflicts += 1;
                        }
                        pixels[p] = v;
                    }
                }
            }
        }
        scenes.push(json!({"name":name,"scene_offset":scene,"bank_offset":bank,"nodes_offset":nodes,"node_count":node_count,"width":width,"height":height,"local_origin":[min_x,min_y],"nontransparent_overlap_conflicts":overlap_conflicts,"parts":parts.iter().map(|p|json!({"member":p.member,"x":p.x,"y":p.y,"width":p.width,"height":p.height})).collect::<Vec<_>>()}));
        titles.push(Title {
            name,
            width,
            height,
            pixels,
            palette,
            parts,
        });
    }
    Ok((
        titles,
        json!({"surface":label,"archive":archive,"gem_member":gem_member,"gem_sha256":sha(&gem),"base":base,"ilf_sha256":sha(ilf),"images":image_records,"titles":scenes,"claim":"target-local static Gem/ILF mapping and tile round trips; animations and runtime resource selection require separate proof"}),
    ))
}

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let mut reports = Vec::new();
    for (i, surface) in SURFACES.iter().enumerate() {
        let (titles, report) = read(rom, i)?;
        let dir = out.join(surface.0);
        fs::create_dir_all(&dir)?;
        for title in titles {
            write_png(
                &dir.join(format!("{}.png", title.name)),
                title.width,
                title.height,
                &rgba(&title.pixels, &title.palette)?,
            )?;
        }
        reports.push(report);
    }
    let report = json!({"source_sha256":sha(rom.bytes),"surfaces":reports});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
