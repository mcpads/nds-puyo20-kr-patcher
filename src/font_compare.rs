//! Read-only font-library comparisons against registered NDS source assets.
use crate::{assets::json_file, format::*, graphics::write_png, hinting::Hinted};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const SAMPLES: [&str; 8] = [
    "비과학적인 건",
    "무, 무, 무서워요!",
    "과학으론 안 풀리니",
    "뿌요를 움직여 주세요",
    "값 읽고 앉아 빛 흙 꽃",
    "아르르",
    "리스쿠마 선배",
    "드라코켄타우로스",
];
fn canvas(w: usize, h: usize) -> Vec<u8> {
    [48, 32, 64, 255].repeat(w * h)
}
fn pixel(image: &mut [u8], w: usize, x: i32, y: i32, c: [u8; 4]) {
    if x >= 0
        && y >= 0
        && (x as usize) < w
        && ((y as usize) * w + x as usize) * 4 + 4 <= image.len()
    {
        let p = (y as usize * w + x as usize) * 4;
        image[p..p + 4].copy_from_slice(&c);
    }
}
fn reference(rom: &Rom, out: &Path, name: &str, file: &str) -> Result<Value> {
    let packed = rom.data(rom.file(file)?);
    let raw = unpack(packed)?;
    let h = u32le(&raw, 4)?;
    let w = u32le(&raw, 8)?;
    let n = u32le(&raw, 12)?;
    let stride = 4 + w * h / 2;
    ensure!(48 + n * stride == raw.len(), "source font extent");
    let mut image = canvas(256, 128);
    let mut rows = Vec::new();
    for i in 0..n.min(128) {
        let p = 48 + i * stride;
        let mut ink = 0;
        for (j, v) in raw[p + 4..p + stride]
            .iter()
            .flat_map(|v| [v & 15, v >> 4])
            .enumerate()
        {
            if v != 0 {
                ink += 1;
                pixel(
                    &mut image,
                    256,
                    (i % 16 * 16 + j % w) as i32,
                    (i / 16 * 16 + j / w) as i32,
                    if v == 1 {
                        [255, 255, 255, 255]
                    } else {
                        [150, 120, 160, 255]
                    },
                );
            }
        }
        rows.push(json!({"code":u16le(&raw,p)?,"advance":u16le(&raw,p+2)?,"nonzero_pixels":ink}));
    }
    write_png(&out.join(format!("{name}.png")), 256, 128, &image)?;
    Ok(
        json!({"file":file,"stored_sha256":sha(packed),"decoded_sha256":sha(&raw),"cell":[w,h],"count":n,"preview_first_glyphs":rows}),
    )
}
pub fn run(rom: &Rom, inventory: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    fs::create_dir_all(out)?;
    let inv_raw = fs::read(inventory)?;
    let inv: Value = serde_json::from_slice(&inv_raw)?;
    let references = vec![
        reference(rom, out, "jp-story", "text/story_demo/02_rng.fnt")?,
        reference(rom, out, "jp-menu", "text/menu/main_menu.fnt")?,
    ];
    let n = Narc::parse(rom.data(rom.file("option/appreciate.narc")?))?;
    let raw = unpack(n.members[24])?;
    let pal = unpack(n.members[23])?;
    let mut rgba = Vec::new();
    for v in raw.iter().flat_map(|v| [v & 15, v >> 4]) {
        let c = u16le(&pal, v as usize * 2)?;
        rgba.extend([
            (c & 31) as u8 * 8,
            ((c >> 5) & 31) as u8 * 8,
            ((c >> 10) & 31) as u8 * 8,
            255,
        ]);
    }
    write_png(&out.join("jp-names.png"), 128, 128, &rgba)?;
    let mut results = Vec::new();
    for candidate in inv["candidates"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("candidates"))?
    {
        let id = candidate["id"].as_u64().unwrap();
        let path = candidate["normalized"].as_str().unwrap();
        let data = fs::read(path)?;
        ensure!(
            sha(&data) == candidate["normalized_sha256"].as_str().unwrap(),
            "font hash"
        );
        let font = fontdue::Font::from_bytes(data.clone(), fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!("{e}"));
        for size in [8, 10, 11, 12, 14, 16] {
            for method in ["fontdue", "autohint"] {
                let hinted = Hinted::new(&data, size as f32);
                let mut errors = Vec::new();
                let mut cache = BTreeMap::new();
                for c in SAMPLES.join("").chars() {
                    if cache.contains_key(&c) {
                        continue;
                    }
                    let result: Result<(Vec<(i32, i32)>, usize)> = (|| {
                        let f = font
                            .as_ref()
                            .map_err(|e| anyhow::anyhow!("fontdue metrics: {e}"))?;
                        ensure!(f.lookup_glyph_index(c) != 0, "missing glyph {c}");
                        let advance = if c == ' ' {
                            5
                        } else {
                            f.metrics(c, size as f32).advance_width.round().max(1.0) as usize
                        };
                        let mut points = Vec::new();
                        if method == "fontdue" {
                            let (m, b) = f.rasterize(c, size as f32);
                            for (y, row) in b.chunks(m.width.max(1)).enumerate() {
                                for (x, &v) in row.iter().enumerate() {
                                    if v >= 128 {
                                        points.push((
                                            m.xmin + x as i32,
                                            -m.ymin - m.height as i32 + y as i32,
                                        ));
                                    }
                                }
                            }
                        } else if let Some((x, y, w, _, b)) = hinted
                            .as_ref()
                            .map_err(|e| anyhow::anyhow!("{e}"))?
                            .placed(c, 16, 32, (64, 64))?
                        {
                            for (p, &v) in b.iter().enumerate() {
                                if v >= 128 {
                                    points.push((
                                        x + p as i32 % w as i32 - 16,
                                        y + p as i32 / w as i32 - 32,
                                    ));
                                }
                            }
                        }
                        ensure!(c == ' ' || !points.is_empty(), "empty glyph {c}");
                        Ok((points, advance))
                    })();
                    match result {
                        Ok(v) => {
                            cache.insert(c, v);
                        }
                        Err(e) => {
                            errors.push(e.to_string());
                        }
                    }
                }
                let mut image = canvas(256, 160);
                let mut measures = Vec::new();
                for (row, text) in SAMPLES.iter().enumerate() {
                    let names = row >= 5;
                    let height = if row < 3 {
                        11
                    } else if names {
                        14
                    } else {
                        12
                    };
                    let guard = if names { 2 } else { 0 };
                    let limit = if names { 88 } else { 208 };
                    let mut cursor = 0i32;
                    let mut ink = Vec::new();
                    let mut bad_glyphs = 0;
                    for c in text.chars() {
                        if let Some((points, advance)) = cache.get(&c) {
                            let min = points.iter().map(|p| p.1).min().unwrap_or(0);
                            let max = points.iter().map(|p| p.1).max().unwrap_or(-1);
                            let max_x = points.iter().map(|p| p.0).max().unwrap_or(0);
                            let baseline = if names { 13 } else { 11 };
                            let raise =
                                (max + baseline + if names { 1 } else { 0 } - (height - 1)).max(0);
                            if !points.is_empty()
                                && (min + baseline - raise - if names { 1 } else { 0 } < guard
                                    || max_x >= 15)
                            {
                                bad_glyphs += 1;
                            }
                            ink.extend(
                                points
                                    .iter()
                                    .map(|&(x, y)| (cursor + x, y + baseline - raise)),
                            );
                            cursor += *advance as i32;
                        } else {
                            cursor += size;
                        }
                    }
                    let mut painted = BTreeMap::new();
                    for &(x, y) in &ink {
                        if names {
                            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                                painted.insert((x + dx, y + dy), [80, 210, 140, 255]);
                            }
                        } else {
                            painted.insert((x + 1, y + 1), [15, 8, 22, 255]);
                        }
                    }
                    for p in &ink {
                        painted.insert(*p, [255, 255, 255, 255]);
                    }
                    let mut overflow = 0;
                    for ((x, y), color) in painted {
                        let bad = y < guard || y >= height || x < 0 || x >= limit;
                        if bad {
                            overflow += 1;
                        }
                        pixel(
                            &mut image,
                            256,
                            x + 8,
                            row as i32 * 20 + y + 2,
                            if bad { [255, 70, 70, 255] } else { color },
                        );
                    }
                    for x in 0..limit {
                        pixel(
                            &mut image,
                            256,
                            x + 8,
                            row as i32 * 20 + height + 2,
                            [80, 65, 95, 255],
                        );
                    }
                    measures.push(json!({"role":if names {"name"} else if row<3 {"story"} else {"menu"},"text":text,"width":cursor,"limit":limit,"height":height,"guard_rows":guard,"ink_bounds_failures":bad_glyphs,"out_of_region_pixels_including_shadow":overflow}));
                }
                let file = format!("font-{id:03}-{method}-{size}.png");
                write_png(&out.join(&file), 256, 160, &image)?;
                results.push(json!({"id":id,"family":candidate["family"],"style":candidate["style"],"size":size,"method":method,"image":file,"errors":errors,"measures":measures}));
            }
        }
        eprintln!("font {id} complete");
    }
    let report = json!({"source_sha256":sha(rom.bytes),"inventory_sha256":sha(&inv_raw),"references":references,"samples":SAMPLES,"results":results,"scope":"unadopted sample comparisons; full corpus and product capacity not verified; red pixels are out-of-region diagnostics"});
    json_file(&out.join("report.json"), &report)?;
    Ok(json!({"candidates":inv["candidates"].as_array().unwrap().len(),"previews":results.len()}))
}
