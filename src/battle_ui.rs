use crate::{assets::json_file, format::*, graphics::write_png, titles};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const MENU: &str = "puyo/menu/puyo_menu.narc";
const CHARACTERS: &str = "puyo/2p_btl/2PBtl.narc";
const BUTTON_ORIGINS: [(usize, usize); 10] = [
    (0, 0),
    (64, 0),
    (128, 0),
    (0, 32),
    (64, 32),
    (128, 32),
    (0, 64),
    (64, 64),
    (128, 64),
    (192, 64),
];
fn name_members() -> Vec<usize> {
    [1048, 1049].into_iter().chain(1051..1090).collect()
}
pub(crate) fn indices(raw: &[u8]) -> Vec<u8> {
    raw.iter().flat_map(|v| [v & 15, v >> 4]).collect()
}
pub(crate) fn pack(pixels: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        pixels.len() % 2 == 0 && pixels.iter().all(|&v| v < 16),
        "invalid I4 pixels"
    );
    Ok(pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect())
}
pub(crate) fn crop(pixels: &[u8], width: usize, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    (y..y + h)
        .flat_map(|row| pixels[row * width + x..row * width + x + w].iter().copied())
        .collect()
}
pub(crate) fn sprite_pair_pixels(raw: &[u8]) -> Result<Vec<u8>> {
    ensure!(raw.len() == 2048, "sprite pair extent changed");
    let left = titles::untile(&raw[..1024], 64, 32, 4)?;
    let right = titles::untile(&raw[1024..], 64, 32, 4)?;
    Ok((0..32)
        .flat_map(|y| {
            left[y * 64..y * 64 + 64]
                .iter()
                .chain(&right[y * 64..y * 64 + 64])
                .copied()
        })
        .collect())
}
pub(crate) fn sprite_pair_bytes(pixels: &[u8]) -> Result<Vec<u8>> {
    ensure!(pixels.len() == 128 * 32, "sprite pair canvas changed");
    let mut raw = titles::tile(&crop(pixels, 128, 0, 0, 64, 32), 64, 32, 4)?;
    raw.extend(titles::tile(&crop(pixels, 128, 64, 0, 64, 32), 64, 32, 4)?);
    Ok(raw)
}
struct MenuImages {
    atlas: Vec<u8>,
    tiled: Vec<u8>,
    palette: Vec<u8>,
    start: Vec<u8>,
    start_palette: Vec<u8>,
}
fn menu_images(n: &Narc) -> Result<MenuImages> {
    ensure!(n.members.len() == 46, "battle menu population changed");
    let atlas = indices(&unpack_halfword(n.members[0])?);
    let tiled = unpack_halfword(n.members[2])?;
    let palette = unpack(n.members[1])?;
    ensure!(
        atlas.len() == 256 * 112 && tiled.len() == 15104 && palette.len() == 138,
        "handicap atlas geometry changed"
    );
    for (i, &(x, y)) in BUTTON_ORIGINS.iter().enumerate() {
        ensure!(
            titles::tile(&crop(&atlas, 256, x, y, 64, 32), 64, 32, 4)?
                == tiled[i * 1024..(i + 1) * 1024],
            "linear/tiled handicap button mismatch"
        );
    }
    let start = indices(&unpack_halfword(n.members[3])?);
    let start_palette = unpack(n.members[4])?;
    ensure!(
        start.len() == 64 * 64 && start_palette.len() == 32,
        "START geometry changed"
    );
    ensure!(
        titles::tile(&start, 64, 64, 4)? == unpack_halfword(n.members[5])?,
        "linear/tiled START mismatch"
    );
    Ok(MenuImages {
        atlas,
        tiled,
        palette,
        start,
        start_palette,
    })
}

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let menu = Narc::parse(rom.data(rom.file(MENU)?))?;
    let images = menu_images(&menu)?;
    let names = Narc::parse(rom.data(rom.file(CHARACTERS)?))?;
    ensure!(
        names.members.len() == 1102,
        "battle character population changed"
    );
    let palette = unpack(names.members[1050])?;
    ensure!(palette.len() == 32, "name palette changed");
    fs::create_dir_all(out)?;
    for bank in [2, 3] {
        write_png(
            &out.join(format!("handicap-bank{bank}.png")),
            256,
            112,
            &titles::rgba(&images.atlas, &images.palette[bank * 32..bank * 32 + 32])?,
        )?;
    }
    write_png(
        &out.join("start.png"),
        64,
        64,
        &titles::rgba(&images.start, &images.start_palette)?,
    )?;
    let mut records = Vec::new();
    for id in name_members() {
        let raw = unpack_halfword(names.members[id])?;
        let pixels = sprite_pair_pixels(&raw)?;
        ensure!(
            sprite_pair_bytes(&pixels)? == raw,
            "name pair unchanged round trip failed"
        );
        write_png(
            &out.join(format!("name-{id}.png")),
            128,
            32,
            &titles::rgba(&pixels, &palette)?,
        )?;
        fs::write(out.join(format!("name-{id}-pixels.bin")), &pixels)?;
        records.push(json!({"member":id,"raw_sha256":sha(&raw),"pixels_sha256":sha(&pixels)}));
    }
    fs::write(out.join("handicap-pixels.bin"), &images.atlas)?;
    fs::write(out.join("start-pixels.bin"), &images.start)?;
    let mut pause = Vec::new();
    for (linear, tiled, pal, slots) in [
        (29, 40, 30, 10),
        (31, 33, 32, 10),
        (34, 36, 35, 12),
        (37, 39, 38, 10),
        (41, 43, 42, 10),
    ] {
        let raw = unpack_halfword(menu.members[linear])?;
        let target = unpack_halfword(menu.members[tiled])?;
        let colors = unpack(menu.members[pal])?;
        ensure!(
            raw.len() == slots * 2048 && colors.len() == 64,
            "pause atlas geometry changed"
        );
        let pixels = indices(&raw);
        let mut converted = Vec::new();
        for cell in pixels.chunks_exact(128 * 32) {
            converted.extend(sprite_pair_bytes(cell)?);
        }
        ensure!(
            converted == target,
            "pause sprite-pair counterpart mismatch"
        );
        for bank in 0..2 {
            write_png(
                &out.join(format!("pause-{linear}-bank{bank}.png")),
                128,
                slots * 32,
                &titles::rgba(&pixels, &colors[bank * 32..bank * 32 + 32])?,
            )?;
        }
        pause.push(json!({"linear_member":linear,"tiled_member":tiled,"palette_member":pal,"slots":slots,"linear_sha256":sha(&raw),"tiled_sha256":sha(&target),"pixels_sha256":sha(&pixels)}));
    }
    let pause_title = indices(&unpack_halfword(menu.members[6])?);
    let pause_palette = unpack(menu.members[7])?;
    ensure!(
        pause_title.len() == 32 * 96 && pause_palette.len() == 32,
        "pause title geometry changed"
    );
    ensure!(
        titles::tile(&pause_title, 32, 96, 4)? == unpack_halfword(menu.members[8])?,
        "pause title tiled counterpart mismatch"
    );
    write_png(
        &out.join("pause-title.png"),
        32,
        96,
        &titles::rgba(&pause_title, &pause_palette)?,
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"menu_archive_sha256":sha(rom.data(rom.file(MENU)?)),"character_archive_sha256":sha(rom.data(rom.file(CHARACTERS)?)),"button_origins":BUTTON_ORIGINS,"buttons":"10 exact 64x32 linear atlas crops equal the first 10 tiled chunks; palette banks 2 and 3","name_layout":"two consecutive 64x32 I4 tiled sprites form a 128x32 name","name_palette_sha256":sha(&palette),"names":records,"pause_candidates":pause,"pause_title":"members 6/8: three 32x32 cells; placement and edit policy unadopted","claim":"target-local pixel reconstruction and unchanged round trips; runtime consumption separate"});
    json_file(&out.join("battle-ui.json"), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    state: String,
    #[serde(default)]
    source_sha256: Option<String>,
    #[serde(default)]
    background_artwork: Vec<titles::Artwork>,
    atlas_sha256: String,
    tiled_sha256: String,
    start_sha256: String,
    start_tiled_sha256: String,
    levels: Vec<Label>,
    start: Label,
    names: Vec<Name>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    japanese: String,
    korean: String,
    /// Caption style on the large button; absent keeps Galmuri9 10px.
    #[serde(default)]
    large: Option<ButtonText>,
    /// Caption style on the small button; absent keeps Galmuri7 8px.
    #[serde(default)]
    small: Option<ButtonText>,
}
/// A handicap caption centred in its editable rectangle with an 8-neighbour
/// one-pixel outline, optionally narrowed by area coverage.
#[derive(Deserialize, serde::Serialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct ButtonText {
    size: usize,
    #[serde(default)]
    scale_x: Option<f64>,
    #[serde(default)]
    letter_spacing: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    member: usize,
    source_pixels_sha256: String,
    japanese: String,
    korean: String,
    font_size: usize,
    /// Horizontal scale for outlined names wider than the sprite pair.
    #[serde(default)]
    scale_x: Option<f64>,
}

// Local canvas coordinates; all ink and its one-pixel outline must fit the region.
pub(crate) fn text_ink(
    font: &fontdue::Font,
    text: &str,
    size: usize,
    rect: [usize; 4],
    baseline: usize,
) -> Result<Vec<(usize, usize)>> {
    text_ink_with_space(font, text, size, rect, baseline, 5)
}

pub(crate) fn text_ink_with_space(
    font: &fontdue::Font,
    text: &str,
    size: usize,
    rect: [usize; 4],
    baseline: usize,
    space_width: usize,
) -> Result<Vec<(usize, usize)>> {
    text_ink_tracked(font, text, size, rect, baseline, space_width, 0, false)
}

/// Like `text_ink_with_space` with `tracking` extra pixels after each glyph
/// and optional per-glyph bolding: each ink pixel also inks its right
/// neighbour unless that would close a one-pixel gap inside the glyph.
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_ink_tracked(
    font: &fontdue::Font,
    text: &str,
    size: usize,
    rect: [usize; 4],
    baseline: usize,
    space_width: usize,
    tracking: usize,
    bold_keep_gaps: bool,
) -> Result<Vec<(usize, usize)>> {
    let [x0, y0, x1, y1] = rect;
    ensure!(!text.trim().is_empty(), "empty battle caption");
    let advance = |ch| {
        if ch == ' ' {
            space_width
        } else {
            font.metrics(ch, size as f32).advance_width.round() as usize + tracking
        }
    };
    // Trailing tracking after the last glyph is not ink.
    let width =
        text.chars().map(advance).sum::<usize>() - if text.ends_with(' ') { 0 } else { tracking };
    ensure!(width + 2 <= x1 - x0, "battle caption exceeds width: {text}");
    let mut cursor = x0 + (x1 - x0 - width) / 2;
    let mut ink = Vec::new();
    for ch in text.chars() {
        ensure!(
            !ch.is_control() && font.lookup_glyph_index(ch) != 0,
            "missing battle glyph {ch}"
        );
        let (m, bitmap) = font.rasterize(ch, size as f32);
        let mut glyph = std::collections::BTreeSet::new();
        for y in 0..m.height {
            for x in 0..m.width {
                if bitmap[y * m.width + x] >= 128 {
                    glyph.insert((
                        cursor as i32 + m.xmin + x as i32,
                        baseline as i32 - m.ymin - m.height as i32 + y as i32,
                    ));
                }
            }
        }
        if bold_keep_gaps {
            let right = glyph
                .iter()
                .map(|&(x, y)| (x + 1, y))
                .filter(|&(x, y)| !glyph.contains(&(x, y)) && !glyph.contains(&(x + 1, y)))
                .collect::<Vec<_>>();
            glyph.extend(right);
        }
        for (px, py) in glyph {
            ensure!(
                px > x0 as i32 && px < (x1 - 1) as i32 && py > y0 as i32 && py < (y1 - 1) as i32,
                "battle glyph outside region: {text} ({px},{py})"
            );
            ink.push((px as usize, py as usize));
        }
        cursor += advance(ch);
    }
    ensure!(!ink.is_empty(), "empty battle glyphs");
    Ok(ink)
}
pub(crate) fn paint(pixels: &mut [u8], width: usize, ink: &[(usize, usize)], white: u8, dark: u8) {
    for &(x, y) in ink {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            pixels[(y as i32 + dy) as usize * width + (x as i32 + dx) as usize] = dark;
        }
    }
    for &(x, y) in ink {
        pixels[y * width + x] = white;
    }
}
fn nearest(palette: &[u8], target: [i32; 3]) -> Result<u8> {
    (1..palette.len() / 2)
        .map(|i| {
            Ok((
                i as u8,
                crate::buttons::rgb(palette, i)?
                    .iter()
                    .zip(target)
                    .map(|(a, b)| (a - b).pow(2))
                    .sum::<i32>(),
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min_by_key(|(_, d)| *d)
        .map(|(i, _)| i)
        .ok_or_else(|| anyhow::anyhow!("empty palette"))
}
pub(crate) fn compress_member(
    changes: &mut BTreeMap<usize, Vec<u8>>,
    id: usize,
    raw: &[u8],
) -> Result<()> {
    let packed = crate::compress::pack(raw)?;
    ensure!(
        unpack_halfword(&packed)? == raw,
        "battle member halfword round trip failed"
    );
    ensure!(
        changes.insert(id, packed).is_none(),
        "duplicate battle member writer"
    );
    Ok(())
}
pub(crate) fn rebuilt(source: &[u8], changes: &BTreeMap<usize, Vec<u8>>) -> Result<Vec<u8>> {
    let data = crate::archive::replace(source, changes)?;
    let n = Narc::parse(&data)?;
    for (&id, bytes) in changes {
        ensure!(n.members[id] == bytes, "rebuilt battle member mismatch");
    }
    Ok(data)
}

/// Handicap captions use pixel fonts at their native grid. Captions without a
/// `large`/`small` style keep Galmuri9 10px and Galmuri7 8px; styled captions use
/// the bold narrow DenkiChip 12px (x10y12px).
const BUTTON_FONT_SHA256: [&str; 2] = [
    "48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee",
    "4589cb1a59bcbd669ad7ac0669827e5a4d411832048e9bdd9618c907c1a8d272",
];
const SMALL_BUTTON_FONT_SHA256: [&str; 2] = [
    "1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396",
    "4589cb1a59bcbd669ad7ac0669827e5a4d411832048e9bdd9618c907c1a8d272",
];
/// BM JUA for outlined battle names.
const NAME_FONT_SHA256: [&str; 1] =
    ["e8e6aa8b1b662c7bf0d7f136f29e822e0985176458a6e5d0ba08afc4a5c901a9"];

fn load_font(path: &Path, expected: &[&str]) -> Result<fontdue::Font> {
    let bytes = fs::read(path)?;
    ensure!(
        expected.contains(&sha(&bytes).as_str()),
        "font identity mismatch: {}",
        path.display()
    );
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    button_font_path: &Path,
    small_button_font_path: &Path,
    name_font_path: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.source_sha256
            .as_ref()
            .is_none_or(|v| *v == sha(rom.bytes))
            && (tr.background_artwork.is_empty()
                || (tr.background_artwork.len() == 10 && tr.source_sha256.is_some())),
        "generated handicap backgrounds require source identity and ten button entries"
    );
    ensure!(
        tr.state == "development_art_draft"
            && tr.levels.len() == 5
            && tr.names.iter().map(|n| n.member).collect::<Vec<_>>() == name_members(),
        "battle draft population changed"
    );
    ensure!(
        tr.levels.iter().all(|s| !s.japanese.is_empty()) && !tr.start.japanese.is_empty(),
        "empty source caption"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(
        crate::fonts::is_galmuri11(&font_bytes),
        "font identity mismatch"
    );
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let button_font = load_font(button_font_path, &BUTTON_FONT_SHA256)?;
    let small_button_font = load_font(small_button_font_path, &SMALL_BUTTON_FONT_SHA256)?;
    let name_font = name_font_path
        .map(|p| load_font(p, &NAME_FONT_SHA256))
        .transpose()?;
    let source = rom.data(rom.file(MENU)?);
    let menu = Narc::parse(source)?;
    let original = menu_images(&menu)?;
    for (id, expected) in [
        (0, &tr.atlas_sha256),
        (2, &tr.tiled_sha256),
        (3, &tr.start_sha256),
        (5, &tr.start_tiled_sha256),
    ] {
        ensure!(
            sha(&unpack_halfword(menu.members[id])?) == *expected,
            "battle UI source mismatch"
        );
    }
    let mut atlas = original.atlas.clone();
    let mut tiled = original.tiled.clone();
    let mut edited = vec![false; atlas.len()];
    let mut records = Vec::new();
    for (i, &(x, y)) in BUTTON_ORIGINS.iter().enumerate() {
        let label = &tr.levels[i % 5];
        let rect = if i < 5 {
            [10, 8, 54, 24]
        } else {
            [13, 8, 52, 24]
        };
        let (button_font, size) = if i < 5 {
            (&button_font, 10)
        } else {
            (&small_button_font, 8)
        };
        let baseline = if i < 5 { 20 } else { 19 };
        let style = if i < 5 { label.large } else { label.small };
        let (ink, ring) = match style {
            Some(t) => {
                let (glyphs, stroke) = crate::labels::outlined(
                    button_font,
                    &label.korean,
                    t.size,
                    rect,
                    crate::labels::Spacing {
                        letter_spacing: t.letter_spacing,
                        scale_x: t.scale_x,
                        ..Default::default()
                    },
                    1,
                    None,
                )?;
                let inside = |&(x, y): &(i32, i32)| {
                    x >= rect[0] as i32
                        && y >= rect[1] as i32
                        && x < rect[2] as i32
                        && y < rect[3] as i32
                };
                ensure!(
                    glyphs.iter().chain(&stroke).all(inside),
                    "handicap caption leaves its rectangle: {}",
                    label.korean
                );
                let to = |v: BTreeSet<(i32, i32)>| {
                    v.into_iter()
                        .map(|(x, y)| (x as usize, y as usize))
                        .collect::<Vec<_>>()
                };
                (to(glyphs), Some(to(stroke)))
            }
            None => (
                text_ink(button_font, &label.korean, size, rect, baseline)?,
                None,
            ),
        };
        let mut pixels = if let Some(art) = tr.background_artwork.get(i) {
            titles::generated_pixels(64, 32, &original.palette[64..96], art)?
        } else {
            crop(&original.atlas, 256, x, y, 64, 32)
        };
        if tr.background_artwork.is_empty() {
            for row in rect[1]..rect[3] {
                let row_pixels = &pixels[row * 64..(row + 1) * 64];
                let left = row_pixels.iter().position(|&v| v != 0);
                let right = row_pixels.iter().rposition(|&v| v != 0);
                for col in rect[0]..rect[2] {
                    // Small source captions bleed into the top/bottom border rows.
                    // Erase that fringe too, retaining transparent corners and the
                    // outermost contour pixels in those border rows. Index one
                    // also occurs inside Japanese strokes, so it is not a mask.
                    let old = pixels[row * 64 + col];
                    if old == 0
                        || (i >= 5
                            && !(10..22).contains(&row)
                            && (Some(col) == left || Some(col) == right))
                    {
                        continue;
                    }
                    pixels[row * 64 + col] =
                        original.atlas[(y + row) * 256 + x + if i < 5 { 12 } else { 17 }];
                }
            }
        } else {
            ensure!(
                ink.iter()
                    .chain(ring.iter().flatten())
                    .all(|&(px, py)| pixels[py * 64 + px] != 0),
                "handicap text leaves generated background"
            );
        }
        match &ring {
            Some(ring) => {
                for &(x, y) in ring {
                    pixels[y * 64 + x] = 1;
                }
                for &(x, y) in &ink {
                    pixels[y * 64 + x] = 15;
                }
            }
            None => paint(&mut pixels, 64, &ink, 15, 1),
        }
        for row in 0..32 {
            for col in 0..64 {
                let p = (y + row) * 256 + x + col;
                atlas[p] = pixels[row * 64 + col];
                if !tr.background_artwork.is_empty()
                    || ((rect[0]..rect[2]).contains(&col) && (rect[1]..rect[3]).contains(&row))
                {
                    edited[p] = true;
                }
            }
        }
        tiled[i * 1024..(i + 1) * 1024].copy_from_slice(&titles::tile(&pixels, 64, 32, 4)?);
        records.push(match style {
            Some(t) => json!({"button":i,"atlas_origin":[x,y],"tiled_byte_offset":i*1024,"editable":rect,"japanese":label.japanese,"korean":label.korean,"style":t,"outline":"8-neighbour 1px index 1","placement":"ink bbox centred in editable"}),
            None => json!({"button":i,"atlas_origin":[x,y],"tiled_byte_offset":i*1024,"editable":rect,"japanese":label.japanese,"korean":label.korean,"font_size":size,"baseline":baseline}),
        });
        if let Some(art) = tr.background_artwork.get(i) {
            records.last_mut().unwrap()["background_artwork"] = serde_json::to_value(art)?;
            records.last_mut().unwrap()["editable"] = json!([0, 0, 64, 32]);
        }
    }
    for (p, (&a, &b)) in atlas.iter().zip(&original.atlas).enumerate() {
        ensure!(
            (!tr.background_artwork.is_empty() && edited[p]) || (a == 0) == (b == 0),
            "handicap transparency silhouette changed"
        );
        if !edited[p] {
            ensure!(a == b, "protected handicap pixel changed");
        }
    }
    ensure!(
        tiled[10240..] == original.tiled[10240..],
        "protected menu sprites changed"
    );
    let mut start = original.start.clone();
    start[32 * 64..].fill(0);
    let ink = text_ink(&font, &tr.start.korean, 12, [0, 32, 64, 64], 48)?;
    paint(
        &mut start,
        64,
        &ink,
        nearest(&original.start_palette, [31, 31, 31])?,
        nearest(&original.start_palette, [0, 8, 0])?,
    );
    ensure!(
        start[..32 * 64] == original.start[..32 * 64],
        "START icon changed"
    );
    let mut changes = BTreeMap::new();
    for (id, raw) in [
        (0, pack(&atlas)?),
        (2, tiled),
        (3, pack(&start)?),
        (5, titles::tile(&start, 64, 64, 4)?),
    ] {
        compress_member(&mut changes, id, &raw)?;
    }
    let menu_data = rebuilt(source, &changes)?;
    // Recheck the adopted storage relationship after archive reconstruction.
    let checked = menu_images(&Narc::parse(&menu_data)?)?;
    ensure!(
        checked.atlas == atlas && checked.start == start,
        "rebuilt UI pixels differ"
    );
    let mut writes = Vec::new();
    for (&id, packed) in &changes {
        writes.push(json!({"archive":MENU,"member":id,"stored_size":packed.len(),"capacity":menu.members[id].len(),"decoded_sha256":sha(&unpack_halfword(packed)?)}));
    }
    let character_source = rom.data(rom.file(CHARACTERS)?);
    let names = Narc::parse(character_source)?;
    ensure!(
        names.members.len() == 1102,
        "battle character population changed"
    );
    let palette = unpack(names.members[1050])?;
    ensure!(palette.len() == 32, "battle name palette changed");
    let mut name_changes = BTreeMap::new();
    let mut name_records = Vec::new();
    let mut previews = Vec::new();
    for name in tr.names {
        ensure!(
            !name.japanese.is_empty()
                && if name_font.is_some() {
                    (12..=24).contains(&name.font_size)
                } else {
                    matches!(name.font_size, 12 | 16) && name.scale_x.is_none()
                },
            "invalid battle name input"
        );
        let raw = unpack_halfword(names.members[name.member])?;
        ensure!(
            sha(&raw) == name.source_pixels_sha256,
            "battle name source mismatch"
        );
        let pixels = sprite_pair_pixels(&raw)?;
        ensure!(
            sprite_pair_bytes(&pixels)? == raw,
            "battle name original pair round trip failed"
        );
        let mut pixels = vec![0; 128 * 32];
        match &name_font {
            // Outlined names: 2px rounded navy outline with a one-pixel lower-right
            // drop, the original's white-on-navy weight, centred in the pair.
            Some(name_font) => {
                let rect = [1, 2, 127, 32];
                let (glyphs, stroke) = crate::labels::outlined(
                    name_font,
                    &name.korean,
                    name.font_size,
                    rect,
                    crate::labels::Spacing {
                        scale_x: name.scale_x,
                        grid_fit: name.scale_x.is_some(),
                        ..Default::default()
                    },
                    2,
                    Some([1, 1]),
                )?;
                for &(x, y) in stroke.iter().chain(&glyphs) {
                    ensure!(
                        x >= 1 && y >= 1 && x < 127 && y < 31,
                        "battle name leaves its canvas: {} ({x},{y})",
                        name.korean
                    );
                }
                for &(x, y) in &stroke {
                    pixels[y as usize * 128 + x as usize] = 1;
                }
                for &(x, y) in &glyphs {
                    pixels[y as usize * 128 + x as usize] = 15;
                }
            }
            None => {
                let ink = text_ink(&font, &name.korean, name.font_size, [2, 2, 126, 30], 25)?;
                paint(&mut pixels, 128, &ink, 15, 1);
            }
        }
        let raw = sprite_pair_bytes(&pixels)?;
        ensure!(
            sprite_pair_pixels(&raw)? == pixels,
            "battle name edited pair round trip failed"
        );
        compress_member(&mut name_changes, name.member, &raw)?;
        name_records.push(json!({"member":name.member,"japanese":name.japanese,"korean":name.korean,"font_size":name.font_size,"scale_x":name.scale_x,"source_pixels_sha256":name.source_pixels_sha256,"decoded_sha256":sha(&raw)}));
        previews.push((name.member, pixels));
    }
    let character_data = rebuilt(character_source, &name_changes)?;
    for (&id, packed) in &name_changes {
        writes.push(json!({"archive":CHARACTERS,"member":id,"stored_size":packed.len(),"capacity":names.members[id].len(),"decoded_sha256":sha(&unpack_halfword(packed)?)}));
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("menu.narc"), &menu_data)?;
    fs::write(out.join("characters.narc"), &character_data)?;
    for bank in [2, 3] {
        write_png(
            &out.join(format!("handicap-bank{bank}.png")),
            256,
            112,
            &titles::rgba(&atlas, &original.palette[bank * 32..bank * 32 + 32])?,
        )?;
    }
    write_png(
        &out.join("start.png"),
        64,
        64,
        &titles::rgba(&start, &original.start_palette)?,
    )?;
    for (id, pixels) in previews {
        write_png(
            &out.join(format!("name-{id}.png")),
            128,
            32,
            &titles::rgba(&pixels, &palette)?,
        )?;
    }
    let plans=[(MENU,source,"menu.narc",&menu_data),(CHARACTERS,character_source,"characters.narc",&character_data)].into_iter().map(|(path,src,file,data)|json!({"file":path,"expected_sha256":sha(src),"input":file,"input_sha256":sha(data)})).collect::<Vec<_>>();
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":plans}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"buttons":records,"start":{"japanese":tr.start.japanese,"korean":tr.start.korean,"members":[3,5],"editable":[0,32,64,64]},"names":name_records,"writes":writes,"protected":"all palettes, START icon, other atlas texels and tiled sprites, other character sprites, archive metadata/padding","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("graphics.json"), &report)?;
    Ok(report)
}

pub fn residency(rom: &Rom, ram: &[u8]) -> Result<Value> {
    member_residency(
        rom,
        ram,
        &[(MENU, vec![0, 2, 3, 5]), (CHARACTERS, name_members())],
    )
}

pub(crate) fn member_residency(
    rom: &Rom,
    ram: &[u8],
    archives: &[(&str, Vec<usize>)],
) -> Result<Value> {
    ensure!(
        ram.len() == 4 * 1024 * 1024,
        "expected 4 MiB frozen NDS main RAM"
    );
    let mut records = Vec::new();
    let mut missing = Vec::new();
    for &(path, ref ids) in archives {
        let n = Narc::parse(rom.data(rom.file(path)?))?;
        for &id in ids {
            let raw = unpack_halfword(n.members[id])?;
            let addresses = ram
                .windows(raw.len())
                .enumerate()
                .filter(|(_, w)| *w == raw)
                .map(|(p, _)| 0x02000000 + p)
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                missing.push(json!({"archive":path,"member":id}));
            }
            records.push(json!({"archive":path,"member":id,"decoded_sha256":sha(&raw),"decoded_size":raw.len(),"addresses":addresses}));
        }
    }
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"ram_sha256":sha(ram),"region_base":0x02000000,"members":records,"missing":missing,"claim":"all complete byte matches, including duplicates and missing members; residency alone does not prove screen consumption or unique allocation"}),
    )
}
