//! Read-only JP/EN/KR previews for a reviewed catalog of generated-art candidates.
use crate::{assets::json_file, format::*, graphics::write_png, screens, titles};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::Path};
pub mod compare;
pub mod product_diff;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    groups: Vec<Group>,
    #[serde(default)]
    game_over_layout: bool,
    #[serde(default)]
    sprite_specs: Vec<std::path::PathBuf>,
    #[serde(default)]
    texture_specs: Vec<std::path::PathBuf>,
    #[serde(default)]
    review_notes: std::collections::BTreeMap<String, Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    id: String,
    japanese: String,
    korean_draft: String,
    brief: String,
    archive: String,
    /// Additional diagnostic contact sheet for dark alpha-only lettering.
    #[serde(default)]
    white_background_preview: bool,
    /// Optional diagnostic alpha composite color for compare-art only.
    #[serde(default)]
    preview_background: Option<[u8; 3]>,
    #[serde(default)]
    composition: Option<Composition>,
    surfaces: Vec<Surface>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Composition {
    width: usize,
    height: usize,
    positions: Vec<[usize; 2]>,
    /// Read each ROM's own part positions from this Gem scene instead of
    /// `positions` (a product may move a part of the scene).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gem_scene: Option<GemScene>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct GemScene {
    member: usize,
    ilf: String,
    scene: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Surface {
    member: usize,
    palette: usize,
    #[serde(default)]
    palette_offset: usize,
    width: usize,
    height: usize,
    format: String,
    #[serde(default)]
    map: Option<usize>,
    /// First hardware BG palette bank represented by the stored palette member.
    #[serde(default)]
    palette_bank_start: usize,
    /// First hardware tile index represented by the stored BG tile member.
    #[serde(default)]
    tile_index_start: usize,
    /// Select one complete 32x24 map from a concatenated BG map member.
    #[serde(default)]
    map_frame: Option<usize>,
    #[serde(default)]
    table: Option<String>,
    #[serde(default)]
    table_entry: Option<usize>,
    #[serde(default)]
    ilf: Option<String>,
    #[serde(default)]
    gem: Option<usize>,
    /// Optional integer nearest-neighbor enlargement for inspecting tiny glyphs.
    #[serde(default)]
    zoom: Option<usize>,
    /// First stored row of an I8 diagnostic slice; full payload hash is retained.
    #[serde(default)]
    row_start: usize,
    note: String,
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn member(n: &Narc, id: usize) -> Result<Vec<u8>> {
    unpack(
        n.members
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("member {id} missing"))?,
    )
}
fn preview(rom: &Rom, archive: &str, s: &Surface, path: &Path) -> Result<Value> {
    ensure!(
        s.width > 0 && s.height > 0 && s.width <= 1024 && s.height <= 1024,
        "invalid geometry"
    );
    let n = Narc::parse(rom.data(rom.file(archive)?))?;
    let raw = member(&n, s.member)?;
    let pal = member(&n, s.palette)?;
    ensure!(
        s.row_start == 0 || s.format == "i8",
        "row selection is only supported for linear I8"
    );
    ensure!(
        (s.palette_bank_start == 0 && s.tile_index_start == 0 && s.map_frame.is_none())
            || s.format == "bg4",
        "map frame and hardware origins are only supported for BG4"
    );
    if let Some(table) = &s.table {
        let b = rom.data(rom.file(table)?);
        let row = slice(
            b,
            s.table_entry
                .ok_or_else(|| anyhow::anyhow!("missing table entry"))?
                * 12,
            12,
        )?;
        let fmt = match s.format.as_str() {
            "a3i5" => 1,
            "i2" => 2,
            "a5i3" => 6,
            "i8" => 4,
            _ => 3,
        };
        ensure!(
            u16le(row, 0)? == s.member
                && u16le(row, 2)? == s.palette
                && u16le(row, 4)? == fmt
                && u16le(row, 8)? == s.width
                && u16le(row, 10)? == s.height,
            "texture descriptor differs"
        );
    }
    if let Some(ilf) = &s.ilf {
        let list = rom.data(rom.file(ilf)?);
        let gem = member(&n, s.gem.ok_or_else(|| anyhow::anyhow!("missing Gem"))?)?;
        ensure!(
            slice(&gem, 0, 4)? == b"Gem1" && u32le(&gem, 8)? == gem.len(),
            "invalid Gem"
        );
        ensure!(
            u32le(&gem, 0x28)? * 4 == list.len(),
            "Gem/ILF count differs"
        );
        let base = u32le(&gem, 0x14)? + u32le(&gem, 0x2c)?;
        let mut found = false;
        for (i, w) in list.chunks_exact(4).enumerate() {
            let word = u32le(w, 0)?;
            if (word >> 8) & 4095 == s.member {
                let d = base + i * 32;
                ensure!(
                    word >> 20 == s.palette
                        && u32le(&gem, d)? == word & 255
                        && u16le(&gem, d + 8)? == s.width
                        && u16le(&gem, d + 10)? == s.height,
                    "sprite descriptor differs"
                );
                found = true;
            }
        }
        ensure!(found, "sprite missing from ILF");
    }
    let count = s.width * s.height;
    let mut map_hash = None;
    let texels: Vec<(usize, u8)> = match s.format.as_str() {
        "i2" => raw
            .iter()
            .flat_map(|v| [v & 3, (v >> 2) & 3, (v >> 4) & 3, v >> 6])
            .map(|v| (usize::from(v), if v == 0 { 0 } else { 255 }))
            .collect(),
        "i8" => raw
            .iter()
            .map(|&v| (usize::from(v), if v == 0 { 0 } else { 255 }))
            .collect(),
        "i4" => raw
            .iter()
            .flat_map(|v| [v & 15, v >> 4])
            .map(|v| (usize::from(v), if v == 0 { 0 } else { 255 }))
            .collect(),
        "obj4-pair" => {
            ensure!(
                s.width == 128 && s.height > 0 && s.height % 32 == 0 && raw.len() == count / 2,
                "paired OBJ geometry"
            );
            raw.chunks_exact(2048)
                .map(crate::battle_ui::sprite_pair_pixels)
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .map(|v| (usize::from(v), if v == 0 { 0 } else { 255 }))
                .collect()
        }
        "a3i5" => raw
            .iter()
            .map(|v| (usize::from(v & 31), ((u16::from(v >> 5) * 255) / 7) as u8))
            .collect(),
        "a5i3" => raw
            .iter()
            .map(|v| (usize::from(v & 7), ((u16::from(v >> 3) * 255) / 31) as u8))
            .collect(),
        "obj4" | "obj8" => titles::untile(
            &raw,
            s.width,
            s.height,
            if s.format == "obj8" { 8 } else { 4 },
        )?
        .into_iter()
        .map(|v| (usize::from(v), if v == 0 { 0 } else { 255 }))
        .collect(),
        "bg8" => {
            ensure!(s.width == 256 && s.height == 192, "BG canvas differs");
            let map = member(&n, s.map.ok_or_else(|| anyhow::anyhow!("missing BG map"))?)?;
            map_hash = Some(sha(&map));
            ensure!(
                map.len() == 1536 && raw.len() % 64 == 0 && pal.len() == 512,
                "invalid BG8 map, tile or palette extent"
            );
            let mut pixels = vec![(0, 255); count];
            for cell in 0..32 * 24 {
                let attr = u16le(&map, cell * 2)?;
                let tile = attr & 1023;
                ensure!(
                    attr >> 12 == 0 && tile < raw.len() / 64,
                    "BG8 bank attributes or tile outside supported extent"
                );
                for y in 0..8 {
                    for x in 0..8 {
                        let sx = if attr & 0x400 != 0 { 7 - x } else { x };
                        let sy = if attr & 0x800 != 0 { 7 - y } else { y };
                        let index = raw[tile * 64 + sy * 8 + sx];
                        pixels[((cell / 32) * 8 + y) * 256 + (cell % 32) * 8 + x] =
                            (usize::from(index), 255);
                    }
                }
            }
            pixels
        }
        "bg4" => {
            ensure!(s.width == 256 && s.height == 192, "BG canvas differs");
            let mut map = member(&n, s.map.ok_or_else(|| anyhow::anyhow!("missing BG map"))?)?;
            map_hash = Some(sha(&map));
            if let Some(frame) = s.map_frame {
                ensure!(
                    map.len() % 1536 == 0 && frame < map.len() / 1536,
                    "BG map frame outside member"
                );
                map = map[frame * 1536..(frame + 1) * 1536].to_vec();
            }
            ensure!(
                s.palette_bank_start < 16
                    && !pal.is_empty()
                    && pal.len() % 2 == 0
                    && s.tile_index_start < 1024,
                "invalid BG palette extent"
            );
            let banks = pal.len().div_ceil(32);
            for row in map.chunks_exact_mut(2) {
                let word = u16::from_le_bytes([row[0], row[1]]);
                let bank = usize::from(word >> 12);
                let tile = usize::from(word & 1023);
                ensure!(
                    bank >= s.palette_bank_start && bank - s.palette_bank_start < banks,
                    "BG map bank outside stored palette"
                );
                ensure!(tile >= s.tile_index_start, "BG tile precedes stored tiles");
                let normalized = (word & 0x0c00)
                    | ((tile - s.tile_index_start) as u16)
                    | (((bank - s.palette_bank_start) as u16) << 12);
                row.copy_from_slice(&normalized.to_le_bytes());
            }
            screens::render(&map, &raw, banks)?
                .into_iter()
                .map(|v| (usize::from(v), 255))
                .collect()
        }
        _ => anyhow::bail!("unsupported format"),
    };
    let first = s
        .row_start
        .checked_mul(s.width)
        .ok_or_else(|| anyhow::anyhow!("row offset overflow"))?;
    let end = first
        .checked_add(count)
        .ok_or_else(|| anyhow::anyhow!("row extent overflow"))?;
    ensure!(texels.len() >= end, "image too small");
    let mut rgba = Vec::with_capacity(count * 4);
    for (i, a) in &texels[first..end] {
        let c = u16le(&pal, (i + s.palette_offset) * 2)?;
        rgba.extend([
            ((c & 31) * 255 / 31) as u8,
            (((c >> 5) & 31) * 255 / 31) as u8,
            (((c >> 10) & 31) * 255 / 31) as u8,
            *a,
        ]);
    }
    write_png(path, s.width, s.height, &rgba)?;
    let mut zoom_preview = None;
    if let Some(scale) = s.zoom {
        ensure!((2..=8).contains(&scale), "inspection zoom outside 2..8");
        let width = s.width * scale;
        let height = s.height * scale;
        let mut enlarged = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let p = ((y / scale) * s.width + x / scale) * 4;
                enlarged.extend_from_slice(&rgba[p..p + 4]);
            }
        }
        let zoom_path = path.with_extension("zoom.png");
        write_png(&zoom_path, width, height, &enlarged)?;
        zoom_preview = Some(
            json!({"file":zoom_path.file_name().unwrap().to_string_lossy(),"scale":scale,"sha256":sha(&fs::read(&zoom_path)?)}),
        );
    }
    let mut record = json!({"member":s.member,"palette":s.palette,"width":s.width,"height":s.height,"format":s.format,"stored_bytes":n.members[s.member].len(),"decoded_bytes":raw.len(),"decoded_sha256":sha(&raw),"palette_sha256":sha(&pal),"palette_colors":pal.len()/2,"palette_bank_start":s.palette_bank_start,"tile_index_start":s.tile_index_start,"map_frame":s.map_frame,"map_sha256":map_hash,"preview_sha256":sha(&fs::read(path)?),"trailing_texels":texels.len()-count,"zoom_preview":zoom_preview,"note":s.note});
    if s.row_start != 0 {
        record["row_start"] = json!(s.row_start);
        record["preceding_texels"] = json!(first);
        record["trailing_texels"] = json!(texels.len() - end);
    }
    Ok(record)
}

/// Source-only static previews, without editing or recompressing archive members.
pub fn inspect_source(rom: &Rom, catalog: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let bytes = fs::read(catalog)?;
    let c: Catalog = serde_json::from_slice(&bytes)?;
    fs::create_dir_all(out.join("images"))?;
    let mut groups = Vec::new();
    let mut paths = std::collections::BTreeSet::new();
    for g in c.groups {
        ensure!(
            !g.id.is_empty() && g.id.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'-'),
            "unsafe group id"
        );
        let mut surfaces = Vec::new();
        for s in &g.surfaces {
            let file = format!("images/{}-{}.png", g.id, s.member);
            ensure!(paths.insert(file.clone()), "duplicate preview output");
            let mut record = preview(rom, &g.archive, s, &out.join(&file))?;
            record["image"] = json!(file);
            surfaces.push(record);
        }
        let mut composed = None;
        if let Some(layout) = &g.composition {
            ensure!(
                layout.width > 0
                    && layout.height > 0
                    && layout.width <= 1024
                    && layout.height <= 1024
                    && layout.positions.len() == g.surfaces.len(),
                "invalid source composition"
            );
            let mut rgba = vec![0; layout.width * layout.height * 4];
            let mut conflicting_overlaps = 0;
            let mut identical_overlaps = 0;
            for (i, s) in g.surfaces.iter().enumerate() {
                let [x0, y0] = layout.positions[i];
                ensure!(
                    s.width <= layout.width
                        && s.height <= layout.height
                        && x0 <= layout.width - s.width
                        && y0 <= layout.height - s.height,
                    "surface outside composition"
                );
                let image = crate::art_pixels::read(&fs::read(
                    out.join(surfaces[i]["image"].as_str().unwrap()),
                )?)?;
                for y in 0..s.height {
                    for x in 0..s.width {
                        let source = &image.rgba[(y * s.width + x) * 4..][..4];
                        ensure!(
                            source[3] == 0 || source[3] == 255,
                            "composition requires binary alpha"
                        );
                        if source[3] == 0 {
                            continue;
                        }
                        let target = &mut rgba[((y0 + y) * layout.width + x0 + x) * 4..][..4];
                        if target[3] != 0 {
                            if target == source {
                                identical_overlaps += 1;
                            } else {
                                conflicting_overlaps += 1;
                            }
                        }
                        target.copy_from_slice(source);
                    }
                }
            }
            let file = format!("images/{}-composed.png", g.id);
            write_png(&out.join(&file), layout.width, layout.height, &rgba)?;
            composed = Some(
                json!({"image":file,"sha256":sha(&fs::read(out.join(&file))?),
                "layout":layout,"identical_overlaps":identical_overlaps,"conflicting_overlaps":conflicting_overlaps,
                "claim":"Catalog-defined static source composition; overlap counts do not prove consumer order or member bindings."}),
            );
        }
        let mut contact = None;
        if g.surfaces.len() > 3 && g.surfaces.iter().all(|s| s.width <= 256 && s.height <= 256) {
            let cell_width = g.surfaces.iter().map(|s| s.width).max().unwrap();
            let cell_height = g.surfaces.iter().map(|s| s.height).max().unwrap();
            let columns = if cell_width > 128 || cell_height > 128 {
                3
            } else {
                6
            };
            let width = columns * cell_width;
            let height = g.surfaces.len().div_ceil(columns) * cell_height;
            let mut rgba = vec![0; width * height * 4];
            for (i, s) in g.surfaces.iter().enumerate() {
                let image = crate::art_pixels::read(&fs::read(
                    out.join(surfaces[i]["image"].as_str().unwrap()),
                )?)?;
                for y in 0..s.height {
                    let dst =
                        ((i / columns * cell_height + y) * width + i % columns * cell_width) * 4;
                    rgba[dst..dst + s.width * 4]
                        .copy_from_slice(&image.rgba[y * s.width * 4..(y + 1) * s.width * 4]);
                }
            }
            let file = format!("images/{}-contact.png", g.id);
            write_png(&out.join(&file), width, height, &rgba)?;
            contact = Some(
                json!({"image":file,"sha256":sha(&fs::read(out.join(&file))?),"columns":columns,"cell_width":cell_width,"cell_height":cell_height,"order":"surface order, left to right then top to bottom; no resampling"}),
            );
            if g.white_background_preview {
                for pixel in rgba.chunks_exact_mut(4) {
                    let alpha = u32::from(pixel[3]);
                    for channel in &mut pixel[..3] {
                        *channel =
                            ((u32::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
                    }
                    pixel[3] = 255;
                }
                let white_file = format!("images/{}-contact.white.png", g.id);
                write_png(&out.join(&white_file), width, height, &rgba)?;
                contact.as_mut().unwrap()["white_background_preview"] = json!({
                    "image": white_file,
                    "sha256": sha(&fs::read(out.join(&white_file))?),
                    "claim": "Diagnostic alpha composite over white only; native RGBA retained separately; not a product input."
                });
            }
        }
        groups.push(json!({"id":g.id,"archive":g.archive,"archive_sha256":sha(rom.data(rom.file(&g.archive)?)),"brief":g.brief,"surfaces":surfaces,"contact_sheet":contact,"composition":composed}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"catalog_sha256":sha(&bytes),"groups":groups,"claim":"Catalog-selected source previews only; geometry without table/ILF remains a hypothesis, index-zero OBJ transparency is assumed, runtime consumption and full archive coverage are not proven."});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

pub fn inspect(roms: [&Rom; 3], catalog: &Path, build: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let bytes = fs::read(catalog)?;
    let c: Catalog = serde_json::from_slice(&bytes)?;
    let b: Value = serde_json::from_slice(&fs::read(build)?)?;
    ensure!(
        b["source_sha256"] == sha(roms[0].bytes) && b["output_sha256"] == sha(roms[2].bytes),
        "build identity differs"
    );
    fs::create_dir_all(out.join("images"))?;
    let mut html = String::from(
        "<!doctype html><html lang=ko><meta charset=utf-8><title>생성 미술 후보 JP · EN · KR</title><style>body{font:16px system-ui;margin:32px;background:#eee;color:#222}section{background:white;padding:20px;margin:24px 0}table{border-collapse:collapse;width:100%}td,th{border:1px solid #bbb;padding:12px;vertical-align:top}img{image-rendering:pixelated;max-width:100%;background:repeating-conic-gradient(#ddd 0% 25%,#aaa 0% 50%) 0/16px 16px;width:auto;min-width:64px}small{display:block}p{max-width:1000px}</style><h1>생성 미술 후보</h1><p>일본판을 시각 원본으로, 영어판은 의미·배치 비교 자료로 사용. 한국어 문구는 초안. 이미지 추출은 삽입·실행·사람 검수 완료를 뜻하지 않음. 세로 atlas와 개별 글자 조각은 실제 화면 배치가 아님.</p>",
    );
    let mut groups = Vec::new();
    for g in c.groups {
        ensure!(
            !g.id.is_empty() && g.id.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'-'),
            "unsafe group id"
        );
        html.push_str(&format!("<section id=\"{}\"><h2>{} → {}</h2><p>{}</p><p><code>{}</code></p><table><tr><th>멤버 / 제약</th><th>JP</th><th>EN</th><th>KR</th></tr>",g.id,escape(&g.japanese),escape(&g.korean_draft),escape(&g.brief),escape(&g.archive)));
        let mut surfaces = Vec::new();
        for s in &g.surfaces {
            html.push_str(&format!(
                "<tr><td>#{} / {}×{} / {}<small>{}</small></td>",
                s.member,
                s.width,
                s.height,
                escape(&s.format),
                escape(&s.note)
            ));
            let mut versions = Vec::new();
            for (tag, rom) in ["jp", "en", "kr"].iter().zip(roms) {
                let file = format!("images/{}-{}-{tag}.png", g.id, s.member);
                versions.push(preview(rom, &g.archive, s, &out.join(&file))?);
                html.push_str(&format!(
                    "<td><a href=\"{file}\"><img src=\"{file}\" alt=\"{tag} {} #{}\"></a></td>",
                    escape(&g.japanese),
                    s.member
                ));
            }
            html.push_str("</tr>");
            surfaces.push(json!({"member":s.member,"versions":versions}));
        }
        html.push_str("</table></section>");
        groups.push(json!({"id":g.id,"archive":g.archive,"japanese":g.japanese,"korean_draft":g.korean_draft,"brief":g.brief,"surfaces":surfaces}));
    }
    html.push_str("</html>");
    fs::write(out.join("index.html"), html)?;
    let report = json!({"source_sha256":sha(roms[0].bytes),"reference_sha256":sha(roms[1].bytes),"product_sha256":sha(roms[2].bytes),"catalog_sha256":sha(&bytes),"groups":groups,"runtime_verified":false,"human_reviewed":false,"claim":"catalog-selected static previews; exhaustive delta coverage belongs to compare-scope"});
    json_file(&out.join("report.json"), &report)?;
    Ok(
        json!({"groups":report["groups"].as_array().unwrap().len(),"out":out,"report":out.join("report.json")}),
    )
}
