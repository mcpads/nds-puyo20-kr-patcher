//! Labels split across adjacent Gem/ILF sprites or 4bpp texture-list textures.
//! Pieces of one label are joined left to right, redrawn with the texture-label
//! painter, then split and stored again (8x8 tiles for sprites, linear for textures).
use crate::{assets::json_file, battle_ui, format::*, graphics::write_png, labels, titles};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    archive: String,
    archive_sha256: String,
    state: String,
    sources: Vec<Source>,
}

/// One ILF sprite list and the Gem layout whose descriptors size its sprites,
/// or one texture list whose 4bpp rows give member, palette and size.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    #[serde(default)]
    ilf: Option<String>,
    #[serde(default)]
    ilf_sha256: Option<String>,
    #[serde(default)]
    gem_member: Option<usize>,
    #[serde(default)]
    texlist: Option<String>,
    #[serde(default)]
    texlist_sha256: Option<String>,
    groups: Vec<Group>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    /// Sprite members in left-to-right order.
    members: Vec<usize>,
    label: labels::Label,
    /// Further labels on the same composite, in non-overlapping regions.
    #[serde(default)]
    more: Vec<labels::Label>,
    /// Rectangles copied from another same-size single sprite before drawing,
    /// for lettering that reaches artwork the donor shows clean.
    #[serde(default)]
    patch: Option<Patch>,
    /// Complete generated artwork for this joined sprite group.
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    image_sha256: Option<String>,
    #[serde(default)]
    image_target: Option<[usize; 4]>,
    #[serde(default)]
    image_region: Option<[usize; 4]>,
    #[serde(default)]
    image_matte: Option<crate::art_pixels::ProductionMatte>,
    #[serde(default)]
    alpha_values: Vec<usize>,
    /// False scales an explicitly selected complete panel to its full target.
    #[serde(default)]
    image_fit: Option<bool>,
    /// Replace only image_target, retaining source texels everywhere else.
    #[serde(default)]
    image_patch: bool,
    /// A source-proven solid backing behind generated independent lettering.
    #[serde(default)]
    image_background: Option<ImageBackground>,
    #[serde(default)]
    palette_indices: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageBackground {
    region: [usize; 4],
    rgb555: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    donor: usize,
    rects: Vec<[usize; 4]>,
}

struct Sprite {
    width: usize,
    height: usize,
    palette_member: usize,
    bpp: usize,
    /// Texture-list pieces store rows linearly instead of in 8x8 tiles.
    linear: bool,
    /// Texture format: 3 means index zero is transparency (all OBJ sprites and
    /// 4bpp textures); 1 (A3I5) and 6 (A5I3) carry alpha beside the index.
    format: usize,
}

/// 4bpp, A3I5 and A5I3 texture geometry by member from a texture list.
fn textures(table: &[u8]) -> Result<BTreeMap<usize, Sprite>> {
    ensure!(table.len() % 12 == 0, "texture list size");
    let mut out = BTreeMap::new();
    for row in table.chunks_exact(12) {
        let format = u16le(row, 4)?;
        if !matches!(format, 1 | 3 | 6) {
            continue;
        }
        out.entry(u16le(row, 0)?).or_insert(Sprite {
            width: u16le(row, 8)?,
            height: u16le(row, 10)?,
            palette_member: u16le(row, 2)?,
            bpp: if format == 3 { 4 } else { 8 },
            linear: true,
            format,
        });
    }
    Ok(out)
}

/// (palette index, alpha) per texel; index-zero formats use alpha 0 or 1.
fn decode(bytes: &[u8], p: &Sprite) -> Result<Vec<(usize, usize)>> {
    let opaque = |v: u8| (usize::from(v), usize::from(v != 0));
    if !p.linear {
        return Ok(titles::untile(bytes, p.width, p.height, p.bpp)?
            .into_iter()
            .map(opaque)
            .collect());
    }
    let texels: Vec<(usize, usize)> = match p.format {
        1 => bytes
            .iter()
            .map(|v| (usize::from(v & 31), usize::from(v >> 5)))
            .collect(),
        6 => bytes
            .iter()
            .map(|v| (usize::from(v & 7), usize::from(v >> 3)))
            .collect(),
        _ => bytes
            .iter()
            .flat_map(|v| [v & 15, v >> 4])
            .map(opaque)
            .collect(),
    };
    ensure!(texels.len() >= p.width * p.height, "texture extent");
    Ok(texels)
}

fn encode(texels: &[(usize, usize)], p: &Sprite) -> Result<Vec<u8>> {
    ensure!(
        p.format != 3 || texels.iter().all(|&(i, a)| (i == 0) == (a == 0)),
        "transparency is index zero"
    );
    if !p.linear {
        let px: Vec<u8> = texels.iter().map(|&(i, _)| i as u8).collect();
        return titles::tile(&px, p.width, p.height, p.bpp);
    }
    Ok(match p.format {
        1 => texels.iter().map(|&(i, a)| (a << 5 | i) as u8).collect(),
        6 => texels.iter().map(|&(i, a)| (a << 3 | i) as u8).collect(),
        _ => texels
            .chunks_exact(2)
            .map(|q| (q[0].0 | q[1].0 << 4) as u8)
            .collect(),
    })
}

/// Nearest palette index for a colour; index-zero formats never choose index zero.
fn nearest_for(format: usize, palette: &[u8], target: [i32; 3]) -> Result<usize> {
    if format == 3 {
        return nearest_opaque(palette, target);
    }
    crate::buttons::nearest(palette, target)
}

/// Move a texel from one palette to another by colour, keeping its alpha.
fn remap(format: usize, (i, a): (usize, usize), from: &[u8], to: &[u8]) -> Result<(usize, usize)> {
    if from == to || (format == 3 && i == 0) {
        return Ok((i, a));
    }
    Ok((nearest_for(format, to, crate::buttons::rgb(from, i)?)?, a))
}

/// Sprite geometry by member from the Gem descriptor table aligned with the ILF words.
fn sprites(gem: &[u8], ilf: &[u8], narc: &Narc) -> Result<BTreeMap<usize, Sprite>> {
    ensure!(
        slice(gem, 0, 4)? == b"Gem1" && u32le(gem, 8)? == gem.len(),
        "unexpected Gem1"
    );
    let base = u32le(gem, 0x14)?;
    let count = u32le(gem, 0x28)?;
    ensure!(count * 4 == ilf.len(), "Gem/ILF counts differ");
    let table = base + u32le(gem, 0x2c)?;
    let mut out = BTreeMap::new();
    for i in 0..count {
        let word = u32le(ilf, i * 4)?;
        let descriptor = table + i * 32;
        ensure!(
            u32le(gem, descriptor)? == word & 255,
            "Gem/ILF identity mismatch"
        );
        let palette_member = word >> 20;
        let palette = unpack(narc.members[palette_member])?;
        let bpp = match palette.len() {
            32 => 4,
            512 => 8,
            _ => continue,
        };
        out.entry((word >> 8) & 4095).or_insert(Sprite {
            width: u16le(gem, descriptor + 8)?,
            height: u16le(gem, descriptor + 10)?,
            palette_member,
            bpp,
            linear: false,
            format: 3,
        });
    }
    Ok(out)
}

/// Nearest non-transparent palette index (index zero is transparency).
fn nearest_opaque(palette: &[u8], target: [i32; 3]) -> Result<usize> {
    let mut best = (i32::MAX, 1);
    for i in 1..palette.len() / 2 {
        let c = crate::buttons::rgb(palette, i)?;
        let d = (0..3).map(|k| (c[k] - target[k]).pow(2)).sum();
        if d < best.0 {
            best = (d, i);
        }
    }
    Ok(best.1)
}

pub fn prepare(
    rom: &Rom,
    translation: &Path,
    font_path: &Path,
    small_font: &Path,
    medium_font: Option<&Path>,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Input = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft",
        "unexpected input state"
    );
    let source = rom.data(rom.file(&tr.archive)?);
    ensure!(sha(source) == tr.archive_sha256, "sprite archive changed");
    let narc = Narc::parse(source)?;
    let load = |p: &Path| -> Result<fontdue::Font> {
        fontdue::Font::from_bytes(fs::read(p)?, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))
    };
    let (regular, small) = (load(font_path)?, load(small_font)?);
    let medium = medium_font.map(load).transpose()?;
    let mut changes = BTreeMap::new();
    let mut reports = Vec::new();
    fs::create_dir_all(out)?;
    for src in &tr.sources {
        let (list, list_sha) = match (&src.ilf, &src.texlist) {
            (Some(ilf), None) => (ilf, &src.ilf_sha256),
            (None, Some(texlist)) => (texlist, &src.texlist_sha256),
            _ => anyhow::bail!("source needs exactly one of ilf and texlist"),
        };
        let data = rom.data(rom.file(list)?);
        ensure!(Some(sha(data)) == *list_sha, "sprite list {list} changed");
        let geometry = match src.gem_member {
            Some(gem) if src.ilf.is_some() => sprites(&unpack(narc.members[gem])?, data, &narc)?,
            None if src.texlist.is_some() => textures(data)?,
            _ => anyhow::bail!("ILF sources need gem_member; texture lists take none"),
        };
        for g in &src.groups {
            ensure!(
                !g.members.is_empty() && g.label.texture == g.members[0],
                "group identity"
            );
            let pieces = g
                .members
                .iter()
                .map(|m| {
                    geometry
                        .get(m)
                        .ok_or_else(|| anyhow::anyhow!("member {m} is not a listed sprite"))
                })
                .collect::<Result<Vec<_>>>()?;
            let (height, bpp, format) = (pieces[0].height, pieces[0].bpp, pieces[0].format);
            ensure!(
                pieces
                    .iter()
                    .all(|p| p.height == height && p.bpp == bpp && p.format == format),
                "group pieces differ in height or format"
            );
            // Pieces may be quantised separately. Drawing uses the first piece's
            // palette; changed texels are then mapped to each piece's own palette.
            let palettes = pieces
                .iter()
                .map(|p| unpack(narc.members[p.palette_member]))
                .collect::<Result<Vec<_>>>()?;
            let reference = palettes[0].clone();
            let width: usize = pieces.iter().map(|p| p.width).sum();
            let mut raw = Vec::new();
            let mut decoded = Vec::new();
            for (m, p) in g.members.iter().zip(&pieces) {
                let bytes = unpack_halfword(narc.members[*m])?;
                raw.extend(&bytes);
                let px = decode(&bytes, p)?;
                ensure!(encode(&px, p)? == bytes, "sprite round trip");
                decoded.push(px);
            }
            ensure!(
                sha(&raw) == g.label.source_sha256,
                "group {:?} source changed",
                g.members
            );
            let mut texels = vec![(0, 0); width * height];
            let mut left = 0;
            for ((px, p), palette) in decoded.iter().zip(&pieces).zip(&palettes) {
                for y in 0..height {
                    for x in 0..p.width {
                        texels[y * width + left + x] =
                            remap(format, px[y * p.width + x], palette, &reference)?;
                    }
                }
                left += p.width;
            }
            let unpatched = texels.clone();
            let mut patched = Vec::new();
            if let Some(pa) = &g.patch {
                ensure!(g.members.len() == 1, "patching needs a single sprite");
                let donor = geometry
                    .get(&pa.donor)
                    .ok_or_else(|| anyhow::anyhow!("donor {} is not a listed sprite", pa.donor))?;
                ensure!(
                    donor.width == width
                        && donor.height == height
                        && donor.bpp == bpp
                        && donor.linear == pieces[0].linear,
                    "donor geometry differs"
                );
                let px = decode(&unpack_halfword(narc.members[pa.donor])?, donor)?;
                let dpal = unpack(narc.members[donor.palette_member])?;
                for &[x0, y0, x1, y1] in &pa.rects {
                    ensure!(
                        x0 < x1 && y0 < y1 && x1 <= width && y1 <= height,
                        "patch rect"
                    );
                    for y in y0..y1 {
                        for x in x0..x1 {
                            texels[y * width + x] =
                                remap(format, px[y * width + x], &dpal, &reference)?;
                        }
                    }
                    patched.push([x0, y0, x1, y1]);
                }
            }
            // Index-zero semantics (format 3) serve both OBJ depths here.
            let t = labels::Texture {
                member: g.members[0],
                palette: reference.clone(),
                format,
                width,
                height,
                texels,
            };
            let mut next = t.texels.clone();
            let mut rects: Vec<[usize; 4]> = Vec::new();
            let mut report = Vec::new();
            ensure!(
                g.image.is_some() == g.image_sha256.is_some(),
                "image/hash pair required"
            );
            ensure!(
                g.image.is_some()
                    || (g.image_target.is_none()
                        && g.image_region.is_none()
                        && g.image_matte.is_none()
                        && g.alpha_values.is_empty()
                        && g.image_fit.is_none()
                        && !g.image_patch
                        && g.image_background.is_none()
                        && g.palette_indices.is_empty()),
                "image options require image"
            );
            if let Some(path) = &g.image {
                ensure!(
                    g.patch.is_none() && g.more.is_empty() && matches!(format, 1 | 3 | 6),
                    "generated group requires whole supported texture/OBJ artwork without patches"
                );
                let input = fs::read(path)?;
                ensure!(
                    Some(sha(&input)) == g.image_sha256,
                    "generated sprite image identity"
                );
                let target = g.image_target.unwrap_or([0, 0, width, height]);
                let [x0, y0, x1, y1] = target;
                ensure!(
                    x0 < x1 && y0 < y1 && x1 <= width && y1 <= height,
                    "generated sprite target outside group"
                );
                let mut image = crate::art_pixels::read(&input)?;
                let matte_report = g
                    .image_matte
                    .as_ref()
                    .map(|matte| crate::art_pixels::remove_matte(&mut image, matte))
                    .transpose()?;
                if let Some(region) = g.image_region {
                    image = crate::art_pixels::region(&image, region)?;
                }
                let rgba = crate::art_pixels::reduce(
                    &image,
                    x1 - x0,
                    y1 - y0,
                    g.image_fit.unwrap_or(true),
                )?;
                let colors: Vec<_> = (0..reference.len() / 2)
                    .map(|i| u16le(&reference, i * 2))
                    .collect::<Result<_>>()?;
                ensure!(
                    g.palette_indices
                        .iter()
                        .all(|&i| (format != 3 || i > 0) && i < colors.len()),
                    "invalid generated palette subset"
                );
                let candidates: Vec<_> = if g.palette_indices.is_empty() {
                    ((if format == 3 { 1 } else { 0 })..colors.len()).collect()
                } else {
                    g.palette_indices.clone()
                };
                ensure!(!candidates.is_empty(), "empty opaque palette");
                if g.image_patch {
                    ensure!(
                        g.image_target.is_some() && g.image_background.is_none(),
                        "image patch needs an explicit target and source backing"
                    );
                    ensure!(
                        format == 3 && rgba.chunks_exact(4).all(|c| c[3] >= 128),
                        "image patch must provide an opaque replacement for the complete text region"
                    );
                } else {
                    next.fill((0, 0));
                }
                if let Some(bg) = &g.image_background {
                    ensure!(format == 3, "solid background requires I4 artwork");
                    let [bx0, by0, bx1, by1] = bg.region;
                    ensure!(
                        bx0 < bx1
                            && by0 < by1
                            && bx1 <= width
                            && by1 <= height
                            && x0 >= bx0
                            && y0 >= by0
                            && x1 <= bx1
                            && y1 <= by1,
                        "generated lettering outside solid background"
                    );
                    let index = colors
                        .iter()
                        .enumerate()
                        .find(|&(i, c)| i > 0 && *c == usize::from(bg.rgb555))
                        .map(|(i, _)| i)
                        .ok_or_else(|| {
                            anyhow::anyhow!("solid background color absent from source palette")
                        })?;
                    for (i, &(_, alpha)) in t.texels.iter().enumerate() {
                        let (x, y) = (i % width, i / width);
                        let inside = x >= bx0 && x < bx1 && y >= by0 && y < by1;
                        ensure!(
                            (alpha > 0) == inside,
                            "source solid-background alpha extent differs"
                        );
                        if inside {
                            next[i] = (index, 1);
                        }
                    }
                    for (x, y) in [
                        (bx0, by0),
                        (bx1 - 1, by0),
                        (bx0, by1 - 1),
                        (bx1 - 1, by1 - 1),
                    ] {
                        ensure!(
                            colors[t.texels[y * width + x].0] == usize::from(bg.rgb555),
                            "source background corner differs"
                        );
                    }
                }
                let levels = match format {
                    1 => 7,
                    6 => 31,
                    _ => 1,
                };
                ensure!(
                    g.alpha_values.is_empty()
                        || (g.alpha_values.contains(&0)
                            && g.alpha_values.contains(&levels)
                            && g.alpha_values.iter().all(|&a| a <= levels)),
                    "invalid generated alpha subset"
                );
                for (i, c) in rgba.chunks_exact(4).enumerate() {
                    let mut alpha = (usize::from(c[3]) * levels + 127) / 255;
                    if !g.alpha_values.is_empty() {
                        alpha = *g
                            .alpha_values
                            .iter()
                            .min_by_key(|&&a| a.abs_diff(alpha))
                            .unwrap();
                    }
                    if alpha == 0 {
                        continue;
                    }
                    let index = *candidates
                        .iter()
                        .min_by_key(|&&j| {
                            (0..3)
                                .map(|k| {
                                    let d = ((colors[j] >> (k * 5)) & 31) as i32 * 255 / 31
                                        - i32::from(c[k]);
                                    d * d
                                })
                                .sum::<i32>()
                        })
                        .unwrap();
                    next[(y0 + i / (x1 - x0)) * width + x0 + i % (x1 - x0)] = (index, alpha);
                }
                rects.push(if g.image_patch {
                    target
                } else {
                    [0, 0, width, height]
                });
                report.push(json!({"image":path,"image_sha256":g.image_sha256,"image_target":target,"image_region":g.image_region,"image_fit":g.image_fit.unwrap_or(true),"image_background":g.image_background.as_ref().map(|b| json!({"region":b.region,"rgb555":b.rgb555})),"palette_indices":g.palette_indices,"whole_group_replaced":true}));
                report.last_mut().unwrap()["whole_group_replaced"] = json!(!g.image_patch);
                report.last_mut().unwrap()["protected_outside_target"] = json!(g.image_patch);
                if let Some(matte) = matte_report {
                    report.last_mut().unwrap()["matte_conversion"] = matte;
                }
                if format != 3 {
                    report.last_mut().unwrap()["alpha_values"] = json!(g.alpha_values);
                }
            } else {
                for label in std::iter::once(&g.label).chain(&g.more) {
                    ensure!(label.texture == g.members[0], "group label identity");
                    ensure!(
                        label.copies.is_empty(),
                        "sprite labels patch through the group"
                    );
                    let (rect, r) = labels::draw(
                        &t,
                        &mut next,
                        label,
                        &regular,
                        &small,
                        medium.as_ref(),
                        None,
                    )?;
                    ensure!(
                        rects.iter().all(|q| rect[2] <= q[0]
                            || q[2] <= rect[0]
                            || rect[3] <= q[1]
                            || q[3] <= rect[1]),
                        "overlapping sprite label regions"
                    );
                    rects.push(rect);
                    report.push(r);
                }
            }
            for (p, (a, b)) in unpatched.iter().zip(&next).enumerate() {
                let (x, y) = (p % width, p / width);
                ensure!(
                    a == b
                        || rects
                            .iter()
                            .any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
                        || patched
                            .iter()
                            .any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]),
                    "protected sprite texel changed"
                );
            }
            let mut left = 0;
            for (((m, p), palette), original) in
                g.members.iter().zip(&pieces).zip(&palettes).zip(&decoded)
            {
                let mut px = original.clone();
                for y in 0..height {
                    for x in 0..p.width {
                        let c = y * width + left + x;
                        if next[c] != unpatched[c] {
                            px[y * p.width + x] = remap(format, next[c], &reference, palette)?;
                        }
                    }
                }
                let bytes = encode(&px, p)?;
                let capacity = narc.members[*m].len();
                let mut packed = crate::compress::pack(&bytes)?;
                if packed.len() > capacity && bytes.len() <= 4096 {
                    packed = crate::compress::pack_compact(&bytes)?;
                }
                ensure!(
                    unpack_halfword(&packed)? == bytes,
                    "sprite halfword round trip"
                );
                ensure!(
                    packed.len() <= capacity,
                    "sprite member {m} capacity {} > {capacity}",
                    packed.len()
                );
                ensure!(
                    changes.insert(*m, packed).is_none(),
                    "member {m} written twice"
                );
                left += p.width;
            }
            let preview = |tx: &[(usize, usize)]| -> Result<Vec<u8>> {
                let mut rgba = Vec::new();
                for &(i, a) in tx {
                    let c = crate::buttons::rgb(&t.palette, i)?;
                    rgba.extend(c.map(|v| (v * 255 / 31) as u8));
                    rgba.push((a * 255 / t.max_alpha()) as u8);
                }
                Ok(rgba)
            };
            write_png(
                &out.join(format!("{:03}-before.png", g.members[0])),
                width,
                height,
                &preview(&unpatched)?,
            )?;
            write_png(
                &out.join(format!("{:03}-after.png", g.members[0])),
                width,
                height,
                &preview(&next)?,
            )?;
            reports.push(json!({"list":list,"members":g.members,"width":width,"height":height,"bpp":bpp,"label":report}));
        }
    }
    let rebuilt = battle_ui::rebuilt(source, &changes)?;
    fs::write(out.join("archive.narc"), &rebuilt)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":tr.archive,"expected_sha256":sha(source),"input":"archive.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"groups":reports,"protected":"texels outside each label region, palettes, Gem/ILF layout and other members","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

/// Reuse the product's checked Gem/ILF mappings for read-only ROM comparisons.
pub(crate) fn review_groups(rom: &Rom, input: &Path, prefix: &str) -> Result<Vec<Value>> {
    let tr: Input = serde_json::from_slice(&fs::read(input)?)?;
    let raw = rom.data(rom.file(&tr.archive)?);
    ensure!(
        sha(raw) == tr.archive_sha256,
        "sprite review source identity"
    );
    let n = Narc::parse(raw)?;
    let mut groups = Vec::new();
    for source in tr.sources {
        let map = if let Some(path) = &source.ilf {
            let ilf = rom.data(rom.file(path)?);
            ensure!(
                Some(sha(ilf)) == source.ilf_sha256,
                "sprite review ILF identity"
            );
            sprites(
                &unpack(
                    n.members[source
                        .gem_member
                        .ok_or_else(|| anyhow::anyhow!("missing Gem"))?],
                )?,
                ilf,
                &n,
            )?
        } else {
            let table = rom.data(
                rom.file(
                    source
                        .texlist
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("missing table"))?,
                )?,
            );
            ensure!(
                Some(sha(table)) == source.texlist_sha256,
                "sprite review table identity"
            );
            textures(table)?
        };
        for g in source.groups {
            let mut surfaces = Vec::new();
            let mut positions = Vec::new();
            let mut width = 0;
            let mut height = 0;
            for member in g.members {
                let p = map
                    .get(&member)
                    .ok_or_else(|| anyhow::anyhow!("missing sprite geometry"))?;
                let format = if !p.linear {
                    if p.bpp == 8 { "obj8" } else { "obj4" }
                } else {
                    match p.format {
                        1 => "a3i5",
                        6 => "a5i3",
                        _ => "i4",
                    }
                };
                surfaces.push(json!({"member":member,"palette":p.palette_member,"width":p.width,"height":p.height,"format":format,"note":format!("Source mapping from {}",input.display())}));
                positions.push([width, 0]);
                width += p.width;
                height = height.max(p.height);
            }
            groups.push(json!({"id":format!("{prefix}-{}",groups.len()),"archive":tr.archive,"japanese":g.label.japanese,"korean_draft":g.label.korean,"brief":format!("제품 입력의 좌→우 조각 연결: {}",input.display()),"composition":{"width":width,"height":height,"positions":positions},"surfaces":surfaces}));
        }
    }
    Ok(groups)
}
