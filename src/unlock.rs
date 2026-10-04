//! Unlock notice screens: 256x192 8bpp BGs whose top three lines are text.
//! All notices share one background design, so the plate behind the text is
//! rebuilt from the per-pixel mode of every notice outside its text colours.
use crate::{assets::json_file, battle_ui, buttons, format::*, graphics::write_png};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const ARCHIVE: &str = "report_unlock/report_unlock.narc";
const WHITE: [i32; 3] = [31, 31, 31];
const RED: [i32; 3] = [31, 0, 0];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    archive_sha256: String,
    state: String,
    /// Text rectangle `[x0, y0, x1, y1)` rebuilt on every screen.
    region: [usize; 4],
    /// Top row of each of the three lines.
    line_tops: [usize; 3],
    entries: Vec<Notice>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Notice {
    map: usize,
    tiles: usize,
    palette: usize,
    tiles_sha256: String,
    japanese: [String; 3],
    /// Three lines of `(text, "white" | "red")` segments.
    korean: [Vec<(String, String)>; 3],
}

struct Screen {
    palette: Vec<u8>,
    pixels: Vec<u8>,
}

fn decode(narc: &Narc, n: &Notice) -> Result<Screen> {
    let map = unpack(narc.members[n.map])?;
    let tiles = unpack(narc.members[n.tiles])?;
    let palette = unpack(narc.members[n.palette])?;
    ensure!(
        sha(&tiles) == n.tiles_sha256,
        "notice tiles {} changed",
        n.tiles
    );
    ensure!(
        map.len() == 1536 && tiles.len() % 64 == 0 && palette.len() == 512,
        "notice geometry"
    );
    let mut pixels = vec![0; 256 * 192];
    for cell in 0..768 {
        let attr = u16le(&map, cell * 2)?;
        let id = attr & 1023;
        ensure!(id < tiles.len() / 64, "notice tile outside set");
        for p in 0..64 {
            let (x, y) = (p % 8, p / 8);
            let sx = if attr & 0x400 != 0 { 7 - x } else { x };
            let sy = if attr & 0x800 != 0 { 7 - y } else { y };
            pixels[(cell / 32 * 8 + y) * 256 + cell % 32 * 8 + x] = tiles[id * 64 + sy * 8 + sx];
        }
    }
    Ok(Screen { palette, pixels })
}

fn rgb(palette: &[u8], i: u8) -> Result<[i32; 3]> {
    buttons::rgb(palette, usize::from(i))
}

/// Deduplicate unflipped 8x8 tiles in first-use order.
pub(crate) fn encode(pixels: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut dictionary: BTreeMap<Vec<u8>, u16> = BTreeMap::new();
    let (mut tiles, mut map) = (Vec::new(), Vec::new());
    for cell in 0..768 {
        let tile: Vec<u8> = (0..64)
            .map(|p| pixels[(cell / 32 * 8 + p / 8) * 256 + cell % 32 * 8 + p % 8])
            .collect();
        let next = dictionary.len() as u16;
        let id = *dictionary.entry(tile.clone()).or_insert_with(|| {
            tiles.extend(&tile);
            next
        });
        map.extend(id.to_le_bytes());
    }
    (map, tiles)
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Input = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft",
        "unexpected input state"
    );
    let source = rom.data(rom.file(ARCHIVE)?);
    ensure!(sha(source) == tr.archive_sha256, "notice archive changed");
    let narc = Narc::parse(source)?;
    let font_bytes = fs::read(font_path)?;
    let font_sha = sha(&font_bytes);
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let screens = tr
        .entries
        .iter()
        .map(|n| decode(&narc, n))
        .collect::<Result<Vec<_>>>()?;
    let [x0, y0, x1, y1] = tr.region;
    ensure!(x0 < x1 && y0 < y1 && x1 <= 256 && y1 <= 192, "region");
    // Shared plate: per-pixel RGB mode over every notice, ignoring text colours.
    // Pixels covered by text on every notice take the nearest sampled pixel in the row.
    let mut plate: Vec<Option<[i32; 3]>> = vec![None; (x1 - x0) * (y1 - y0)];
    for y in y0..y1 {
        for x in x0..x1 {
            let mut votes: BTreeMap<[i32; 3], usize> = BTreeMap::new();
            for s in &screens {
                let c = rgb(&s.palette, s.pixels[y * 256 + x])?;
                if c != WHITE && c != RED {
                    *votes.entry(c).or_default() += 1;
                }
            }
            plate[(y - y0) * (x1 - x0) + x - x0] = votes
                .into_iter()
                .max_by_key(|&(c, n)| (n, std::cmp::Reverse(c)))
                .map(|(c, _)| c);
        }
    }
    let width = x1 - x0;
    let mut filled = 0;
    let plate: Vec<[i32; 3]> = (0..plate.len())
        .map(|p| {
            if let Some(c) = plate[p] {
                return Ok(c);
            }
            filled += 1;
            let (row, x) = (p / width, p % width);
            (1..width)
                .flat_map(|d| [x.checked_sub(d), Some(x + d).filter(|&v| v < width)])
                .flatten()
                .find_map(|v| plate[row * width + v])
                .ok_or_else(|| anyhow::anyhow!("no background sample in row {}", y0 + row))
        })
        .collect::<Result<_>>()?;
    let mut changes = BTreeMap::new();
    let mut reports = Vec::new();
    fs::create_dir_all(out)?;
    for (n, s) in tr.entries.iter().zip(&screens) {
        let exact = |c: [i32; 3]| -> Result<u8> {
            let i = buttons::nearest(&s.palette, c)?;
            ensure!(
                buttons::rgb(&s.palette, i)? == c,
                "notice {} lacks colour {c:?}",
                n.tiles
            );
            Ok(i as u8)
        };
        let mut pixels = s.pixels.clone();
        for y in y0..y1 {
            for x in x0..x1 {
                // Notices are quantised separately; the plate takes each palette's nearest colour.
                pixels[y * 256 + x] =
                    buttons::nearest(&s.palette, plate[(y - y0) * width + x - x0])? as u8;
            }
        }
        for (line, top) in n.korean.iter().zip(tr.line_tops) {
            // Measure the whole line, then centre it horizontally.
            let advance = |c: char| {
                if c == ' ' {
                    4
                } else {
                    font.metrics(c, 12.0).advance_width.round() as usize
                }
            };
            let width: usize = line.iter().flat_map(|(t, _)| t.chars()).map(advance).sum();
            ensure!(
                width <= x1 - x0,
                "notice {} line too wide: {width}",
                n.tiles
            );
            let mut cursor = x0 + (x1 - x0 - width) / 2;
            for (text, colour) in line {
                let index = exact(match colour.as_str() {
                    "white" => WHITE,
                    "red" => RED,
                    _ => anyhow::bail!("unknown colour {colour}"),
                })?;
                for c in text.chars() {
                    ensure!(
                        c == ' ' || font.lookup_glyph_index(c) != 0,
                        "font has no glyph for {c}"
                    );
                    let (m, bitmap) = font.rasterize(c, 12.0);
                    for gy in 0..m.height {
                        for gx in 0..m.width {
                            if bitmap[gy * m.width + gx] < 128 {
                                continue;
                            }
                            let px = cursor as i32 + m.xmin + gx as i32;
                            let py = top as i32 + 11 - m.ymin - m.height as i32 + gy as i32;
                            ensure!(
                                px >= x0 as i32
                                    && px < x1 as i32
                                    && py >= y0 as i32
                                    && py < y1 as i32,
                                "notice {} glyph leaves region",
                                n.tiles
                            );
                            pixels[py as usize * 256 + px as usize] = index;
                        }
                    }
                    cursor += advance(c);
                }
            }
        }
        for (p, (a, b)) in s.pixels.iter().zip(&pixels).enumerate() {
            let (x, y) = (p % 256, p / 256);
            ensure!(
                a == b || (x >= x0 && x < x1 && y >= y0 && y < y1),
                "protected notice pixel changed"
            );
        }
        let (map, tiles) = encode(&pixels);
        ensure!(tiles.len() / 64 <= 1024, "notice tile overflow");
        for (member, data) in [(n.map, &map), (n.tiles, &tiles)] {
            // Members stored raw stay raw; compressed ones are recompressed.
            let stored = narc.members[member];
            if unpack(stored)? == stored {
                ensure!(
                    data.len() == stored.len(),
                    "raw notice member {member} size changed"
                );
                ensure!(
                    changes.insert(member, data.clone()).is_none(),
                    "member {member} written twice"
                );
                continue;
            }
            let mut packed = crate::compress::pack(data)?;
            ensure!(unpack(&packed)? == *data, "notice round trip");
            if packed.len() > narc.members[member].len() && data.len() <= 4096 {
                packed = crate::compress::pack_compact(data)?;
            }
            ensure!(
                packed.len() <= narc.members[member].len(),
                "notice member {member} capacity {} > {}",
                packed.len(),
                narc.members[member].len()
            );
            ensure!(
                changes.insert(member, packed).is_none(),
                "member {member} written twice"
            );
        }
        let preview = |px: &[u8]| -> Result<Vec<u8>> {
            let mut rgba = Vec::new();
            for &v in px {
                rgba.extend(rgb(&s.palette, v)?.map(|c| (c * 255 / 31) as u8));
                rgba.push(255);
            }
            Ok(rgba)
        };
        write_png(
            &out.join(format!("{:02}-before.png", n.tiles)),
            256,
            192,
            &preview(&s.pixels)?,
        )?;
        write_png(
            &out.join(format!("{:02}-after.png", n.tiles)),
            256,
            192,
            &preview(&pixels)?,
        )?;
        reports.push(json!({"tiles":n.tiles,"map":n.map,"palette":n.palette,"japanese":n.japanese,"korean":n.korean,"tile_count":tiles.len()/64}));
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::write(out.join("report_unlock.narc"), &rebuilt)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":ARCHIVE,"expected_sha256":sha(source),"input":"report_unlock.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"font_sha256":font_sha,"region":tr.region,"entries":reports,"background":"per-pixel RGB mode over all prepared notices, excluding white and red text; always-covered pixels take the nearest sample in the row","plate_filled_pixels":filled,"protected":"pixels outside the text region, palettes and other members; maps and tiles re-encoded without flips","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
