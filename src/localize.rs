mod dialog;
use crate::{assets::json_file, compress, format::*};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    path: String,
    source_font_sha256: String,
    source_text_sha256: String,
    font_sha256: String,
    state: String,
    entries: Vec<EntryText>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryText {
    id: usize,
    source: Vec<String>,
    korean: Vec<String>,
}

fn source_lines(
    f: &[u8],
    t: &[u8],
    start: usize,
    end: usize,
    stride: usize,
    count: usize,
) -> Result<Vec<String>> {
    let mut lines = vec![String::new()];
    let mut row = 0;
    let mut p = start;
    while p < end {
        let v = u16le(t, p)?;
        p += 2;
        if v == 0xffff {
            ensure!(p == end, "unexpected source terminator");
            return Ok(lines);
        }
        if v == 0xfffe {
            ensure!(u16le(t, p)? == 0xfffd, "unexpected source line control");
            p += 2;
            row += 1;
            lines.push(String::new());
            continue;
        }
        if v == 0xfffd {
            row += 1;
            lines.push(String::new());
            continue;
        }
        ensure!(v < count, "unsupported source control");
        let c = char::from_u32(u16le(f, 48 + v * stride)? as u32)
            .ok_or_else(|| anyhow::anyhow!("invalid source codepoint"))?;
        lines[row].push(c);
    }
    anyhow::bail!("missing source terminator")
}

fn source_breaks(bytes: &[u8], glyph_count: usize) -> Result<Vec<Vec<u16>>> {
    ensure!(bytes.len() % 2 == 0, "odd text span");
    let mut breaks = Vec::new();
    let mut p = 0;
    while p < bytes.len() {
        let value = u16le(bytes, p)?;
        p += 2;
        match value {
            0xffff => {
                ensure!(p == bytes.len(), "unexpected source terminator");
                return Ok(breaks);
            }
            0xfffe => {
                ensure!(u16le(bytes, p)? == 0xfffd, "unexpected source line control");
                p += 2;
                breaks.push(vec![0xfffe, 0xfffd]);
            }
            0xfffd => breaks.push(vec![0xfffd]),
            _ => ensure!(value < glyph_count, "unsupported source control"),
        }
    }
    anyhow::bail!("missing source terminator")
}

pub(crate) fn glyph(font: &fontdue::Font, c: char, height: usize) -> Result<(Vec<u8>, usize)> {
    glyph_with_shadow(font, c, height, true)
}

pub(crate) fn glyph_with_shadow(
    font: &fontdue::Font,
    c: char,
    height: usize,
    shadow: bool,
) -> Result<(Vec<u8>, usize)> {
    ensure!(matches!(height, 11 | 12), "unverified glyph height");
    ensure!(font.lookup_glyph_index(c) != 0, "font has no glyph for {c}");
    // Galmuri11 is only stroke-complete on its native 12px grid; smaller sizes drop strokes.
    let (m, bitmap) = font.rasterize(c, 12.0);
    let advance = if c == ' ' {
        5
    } else {
        m.advance_width.round() as usize
    };
    ensure!(advance > 0 && advance <= 16, "invalid advance for {c}");
    let mut pixels = vec![0u8; 16 * height];
    // Baseline row 11 puts Hangul (11 rows of ink) at rows 0..=10. Glyphs that
    // would reach below the cell are raised by the overflow; only shadow pixels
    // outside the cell are omitted.
    let bottom = 11 - m.ymin;
    let raise = (bottom - height as i32).max(0);
    let top = bottom - m.height as i32 - raise;
    let mut ink = Vec::new();
    for y in 0..m.height {
        for x in 0..m.width {
            if bitmap[y * m.width + x] < 128 {
                continue;
            }
            let px = m.xmin + x as i32;
            let py = top + y as i32;
            ensure!(
                (0..15).contains(&px) && (0..height as i32).contains(&py),
                "glyph/shadow outside 16x{height} cell: {c} ({px},{py})"
            );
            ink.push((px as usize, py as usize));
        }
    }
    ensure!(c == ' ' || !ink.is_empty(), "empty glyph {c}");
    for (x, y) in &ink {
        if shadow && y + 1 < height {
            pixels[(y + 1) * 16 + x + 1] = 2;
        }
    }
    for (x, y) in ink {
        pixels[y * 16 + x] = 1;
    }
    Ok((
        pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect(),
        advance,
    ))
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let translation_bytes = fs::read(translation)?;
    let mut tr: Translation = serde_json::from_slice(&translation_bytes)?;
    ensure!(
        matches!(
            tr.path.as_str(),
            "text/menu/main_menu"
                | "text/option/option_text"
                | "text/menu/single_menu"
                | "text/menu/free_menu"
                | "text/menu/thoroughly_menu"
                | "text/rule_edit/rule_edit_text"
                | "text/menu/minnade_bosyu"
                | "text/menu/minnade_bosyu_kettei"
                | "text/menu/minnade_error"
                | "text/menu/minnade_menu"
                | "text/menu/minnade_sanka"
                | "text/menu/wifi_continue"
                | "text/menu/wifi_friend_check"
                | "text/menu/wifi_friend_input"
                | "text/menu/wifi_friend_list"
                | "text/menu/wifi_friend_menu"
                | "text/menu/wifi_menu"
                | "text/menu/wifi_ranking"
                | "text/menu/wifi_indefinite"
                | "text/menu/wifi_matching"
                | "text/menu/wifi_play_menu"
                | "text/menu/wifi_dialog"
                | "text/menu/academy_menu"
                | "text/menu/academy_guide"
                | "text/menu/academy_practice"
                | "text/menu/challenge_menu"
                | "text/menu/challenge_play"
                | "text/menu/challenge_problem"
                | "text/menu/wifi_designate_c"
                | "text/menu/wifi_designate_p"
                | "text/simulator/simulator"
                | "text/shop/shop_text"
                | "text/shop/shop_trivia_text"
        ) && tr.state == "development_draft",
        "unsupported translation scope/state"
    );
    let fe = rom.file(&format!("{}.fnt", tr.path))?;
    let te = rom.file(&format!("{}.mtx", tr.path))?;
    let f = unpack(rom.data(fe))?;
    let t = unpack(rom.data(te))?;
    ensure!(
        sha(&f) == tr.source_font_sha256 && sha(&t) == tr.source_text_sha256,
        "translation source hash mismatch"
    );
    let info = text_pair(&f, &t)?;
    let shop = tr.path.starts_with("text/shop/");
    let trivia = tr.path == "text/shop/shop_trivia_text";
    // Measured source maxima without letter spacing: shop 207px, trivia 180px.
    let line_limit = if trivia { 180 } else { 208 };
    ensure!(
        info.width == 16
            && (info.height == 12 || (trivia && info.height == 16))
            && info.references.len() == tr.entries.len(),
        "unsupported source geometry/coverage"
    );
    let is_dialog = tr.path == "text/menu/wifi_dialog";
    let mut dialog_anchors = vec![Vec::new(); tr.entries.len()];
    if is_dialog {
        for (i, entry) in tr.entries.iter_mut().enumerate() {
            let start = info.references[i];
            let end = info.references.get(i + 1).copied().unwrap_or(t.len());
            let (_, anchors) = dialog::span(&t[start..end])?;
            entry.source = dialog::lines(&entry.source, &anchors)?;
            entry.korean = dialog::lines(&entry.korean, &anchors)?;
            if (9..=22).contains(&i) {
                ensure!(
                    entry
                        .korean
                        .iter()
                        .map(|s| s.matches("*****").count())
                        .sum::<usize>()
                        == 1
                        && entry
                            .korean
                            .iter()
                            .map(|s| s.matches('*').count())
                            .sum::<usize>()
                            == 5,
                    "error number placeholder changed"
                );
            }
            if i == 32 {
                ensure!(entry.korean == ["1234567890"], "numeric supply changed");
            }
            dialog_anchors[i] = anchors;
        }
    }
    let font_bytes = fs::read(font_path)?;
    ensure!(sha(&font_bytes) == tr.font_sha256, "font identity mismatch");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let chars = tr
        .entries
        .iter()
        .flat_map(|e| e.korean.iter().flat_map(|s| s.chars()))
        .collect::<BTreeSet<_>>();
    let expanded = tr.path != "text/menu/main_menu";
    let spaced = matches!(
        tr.path.as_str(),
        "text/menu/single_menu"
            | "text/menu/free_menu"
            | "text/menu/thoroughly_menu"
            | "text/rule_edit/rule_edit_text"
            | "text/menu/minnade_bosyu"
            | "text/menu/minnade_bosyu_kettei"
            | "text/menu/minnade_error"
            | "text/menu/minnade_menu"
            | "text/menu/minnade_sanka"
            | "text/menu/wifi_continue"
            | "text/menu/wifi_friend_check"
            | "text/menu/wifi_friend_input"
            | "text/menu/wifi_friend_list"
            | "text/menu/wifi_friend_menu"
            | "text/menu/wifi_menu"
            | "text/menu/wifi_ranking"
            | "text/menu/wifi_indefinite"
            | "text/menu/wifi_matching"
            | "text/menu/wifi_play_menu"
            | "text/menu/wifi_dialog"
            | "text/menu/academy_menu"
            | "text/menu/academy_guide"
            | "text/menu/academy_practice"
            | "text/menu/challenge_menu"
            | "text/menu/challenge_play"
            | "text/menu/challenge_problem"
            | "text/menu/wifi_designate_c"
            | "text/menu/wifi_designate_p"
            | "text/simulator/simulator"
    );
    let glyph_count = if expanded {
        info.glyph_count.max(chars.len())
    } else {
        info.glyph_count
    };
    ensure!(
        chars.len() <= glyph_count && glyph_count < 0xfffd,
        "glyph capacity exceeded"
    );
    let mut slots = BTreeMap::new();
    let mut changed_font = f.clone();
    changed_font.resize(48 + glyph_count * info.stride, 0);
    put32(&mut changed_font, 12, glyph_count)?;
    changed_font[48..].fill(0);
    for i in 0..glyph_count {
        changed_font[48 + i * info.stride..50 + i * info.stride]
            .copy_from_slice(&0xffffu16.to_le_bytes());
    }
    for (i, c) in chars.iter().enumerate() {
        ensure!((*c as u32) < 0xfffd, "unrepresentable character");
        let (pixels, width) = if trivia {
            trivia_glyph(&font, *c)?
        } else {
            glyph(&font, *c, info.height)?
        };
        let p = 48 + i * info.stride;
        changed_font[p..p + 2].copy_from_slice(&(*c as u16).to_le_bytes());
        changed_font[p + 2..p + 4].copy_from_slice(&(width as u16).to_le_bytes());
        changed_font[p + 4..p + info.stride].copy_from_slice(&pixels);
        slots.insert(*c, (i, width));
    }
    let mut changed_text = t.clone();
    let mut entries = Vec::new();
    let mut cursor = info.payload;
    ensure!(
        info.references.first() == Some(&info.payload)
            && info.groups[0] + 4 * info.references.len() == info.payload
            && (0..info.references.len())
                .all(|i| u32le(&t, info.groups[0] + 4 * i).ok() == Some(info.references[i])),
        "unsupported reference topology"
    );
    if expanded {
        changed_text[info.payload..].fill(0xff);
    }
    for (i, e) in tr.entries.iter().enumerate() {
        ensure!(e.id == i, "entry order/coverage mismatch");
        let start = info.references[i];
        let end = info.references.get(i + 1).copied().unwrap_or(t.len());
        ensure!(start < end, "non-increasing text spans");
        let prose = if is_dialog {
            dialog::span(&t[start..end])?.0
        } else {
            t[start..end].to_vec()
        };
        ensure!(
            source_lines(&f, &prose, 0, prose.len(), info.stride, info.glyph_count)? == e.source,
            "source prose mismatch: {i}"
        );
        ensure!(
            e.source.len() == e.korean.len() && !e.source.is_empty(),
            "line structure changed"
        );
        let breaks = source_breaks(&prose, info.glyph_count)?;
        ensure!(
            breaks.len() + 1 == e.korean.len(),
            "line break coverage changed"
        );
        let mut units = Vec::new();
        let mut widths = Vec::new();
        for (row, line) in e.korean.iter().enumerate() {
            ensure!(
                line.is_empty() == e.source[row].is_empty(),
                "empty line coverage changed"
            );
            if row > 0 {
                units.extend_from_slice(&breaks[row - 1]);
            }
            if dialog_anchors[i].contains(&(row, true)) {
                units.push(0xfffe);
            }
            let mut width = 0;
            for c in line.chars() {
                let (slot, advance) = slots[&c];
                units.push(slot as u16);
                width += advance + usize::from(spaced);
            }
            ensure!(
                width <= line_limit,
                "line outside adopted menu display range: {i}/{row}: {width}"
            );
            if dialog_anchors[i].contains(&(row, false)) {
                units.push(0xfffe);
            }
            widths.push(width);
        }
        units.push(0xffff);
        let source_controls = (start..end)
            .step_by(2)
            .map(|p| u16le(&t, p))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|v| *v >= info.glyph_count)
            .collect::<Vec<_>>();
        ensure!(
            source_controls
                == units
                    .iter()
                    .copied()
                    .filter(|v| *v as usize >= glyph_count)
                    .map(usize::from)
                    .collect::<Vec<_>>(),
            "control sequence changed"
        );
        let used = units.len() * 2;
        let target = if expanded { cursor } else { start };
        if expanded {
            ensure!(target + used <= t.len(), "MTX total capacity exceeded");
            put32(&mut changed_text, info.groups[0] + i * 4, target)?;
            cursor += used;
        } else {
            ensure!(used <= end - start, "text span overflow: {i}");
            units.resize((end - start) / 2, 0xffff);
        }
        for (j, v) in units.iter().enumerate() {
            changed_text[target + j * 2..target + j * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        // Re-extract stored bytes, checking dialog anchors independently of prose.
        let stored = &changed_text[target..target + used];
        let output_prose = if is_dialog {
            let (prose, anchors) = dialog::span(stored)?;
            ensure!(
                anchors == dialog_anchors[i],
                "dialog control anchors changed"
            );
            prose
        } else {
            stored.to_vec()
        };
        ensure!(
            source_lines(
                &changed_font,
                &output_prose,
                0,
                output_prose.len(),
                info.stride,
                glyph_count
            )? == e.korean,
            "translation round trip failed"
        );
        entries.push(json!({"id":i,"source":e.source,"korean":e.korean,"source_offset":start,"offset":target,"capacity_bytes":end-start,"used_bytes":used,"line_widths":widths,"independent_fffe_anchors":dialog_anchors[i]}));
    }
    text_pair(&changed_font, &changed_text)?;
    ensure!(
        changed_font[..12] == f[..12]
            && changed_font[16..48] == f[16..48]
            && changed_text[..info.groups[0]] == t[..info.groups[0]],
        "protected header or pointer changed"
    );
    ensure!(
        rom.data(fe).starts_with(b"COMP\x11"),
        "unverified original font compression variant"
    );
    let packed = compress::pack(&changed_font)?;
    // Shop fonts may move to the verified ROM tail; other menus keep their extent.
    ensure!(
        shop || packed.len() <= rom.data(fe).len(),
        "font compressed capacity exceeded: {} > {}",
        packed.len(),
        rom.data(fe).len()
    );
    fs::create_dir_all(out)?;
    fs::write(out.join("font.fnt"), &packed)?;
    fs::write(out.join("text.mtx"), &changed_text)?;
    fs::write(out.join("font.decoded.bin"), &changed_font)?;
    let replacement = |entry: &crate::format::Entry, name: &str, data: &[u8]| {
        let mut r = json!({"file":entry.path,"expected_sha256":sha(rom.data(entry)),"input":name,"input_sha256":sha(data)});
        if data.len() > rom.data(entry).len() {
            r["placement"] = json!("ff_tail");
        }
        r
    };
    let plan = json!({"source_sha256":sha(rom.bytes),"replacements":[replacement(fe,"font.fnt",&packed),replacement(te,"text.mtx",&changed_text)]});
    json_file(&out.join("plan.json"), &plan)?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&translation_bytes),"font_sha256":tr.font_sha256,"rasterizer":"fontdue 0.9.4, 12px, baseline 11, threshold 128, shadow +1,+1","active_glyphs":chars.len(),"source_glyph_count":info.glyph_count,"glyph_capacity":glyph_count,"font_decoded_sha256":sha(&changed_font),"font_stored_bytes":packed.len(),"original_font_stored_bytes":rom.data(fe).len(),"mtx_sha256":sha(&changed_text),"entries":entries,"state":"development_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("translation.json"), &report)?;
    Ok(report)
}

/// Shop trivia cells are 16x16 with 12px ink on rows 3..=14 at full level 3.
fn trivia_glyph(font: &fontdue::Font, c: char) -> Result<(Vec<u8>, usize)> {
    let (packed, advance) = glyph(font, c, 12)?;
    let mut pixels = vec![0u8; 16 * 16];
    for (i, byte) in packed.iter().enumerate() {
        for (half, v) in [byte & 15, byte >> 4].into_iter().enumerate() {
            // Level 1 is ink; the menu shadow (2) has no counterpart here.
            if v == 1 {
                pixels[(i * 2 + half) + 3 * 16] = 3;
            }
        }
    }
    Ok((
        pixels.chunks_exact(2).map(|p| p[0] | p[1] << 4).collect(),
        advance,
    ))
}

/// Find complete prepared assets in a frozen main-RAM dump and verify relocation.
pub fn check_ram(rom: &Rom, prepared: &Path, ram: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB main RAM");
    let plan: crate::build::Plan = serde_json::from_slice(&fs::read(prepared.join("plan.json"))?)?;
    ensure!(
        plan.replacements.len() == 2,
        "expected one prepared FNT/MTX pair"
    );
    for item in &plan.replacements {
        let input = fs::read(prepared.join(&item.input))?;
        ensure!(
            sha(&input) == item.input_sha256 && rom.data(rom.file(&item.file)?) == input,
            "prepared asset differs from runtime ROM"
        );
    }
    let fe = plan
        .replacements
        .iter()
        .find(|r| r.file.ends_with(".fnt"))
        .ok_or_else(|| anyhow::anyhow!("missing FNT"))?;
    let te = plan
        .replacements
        .iter()
        .find(|r| r.file.ends_with(".mtx"))
        .ok_or_else(|| anyhow::anyhow!("missing MTX"))?;
    ensure!(
        fe.file.trim_end_matches(".fnt") == te.file.trim_end_matches(".mtx"),
        "mismatched asset pair"
    );
    check_source_ram(rom, fe.file.trim_end_matches(".fnt"), ram)
}

/// Verify an unchanged or prepared pair directly from the exact executing ROM.
pub fn check_source_ram(rom: &Rom, path: &str, ram: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB main RAM");
    let font_path = format!("{path}.fnt");
    let f = unpack_halfword(rom.data(rom.file(&font_path)?))?;
    check_decoded_ram(rom, path, ram, &f)
}

/// Match a caller-verified runtime font transformation and the relocated MTX.
pub(crate) fn check_decoded_ram(rom: &Rom, path: &str, ram: &[u8], f: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB main RAM");
    let font_path = format!("{path}.fnt");
    let text_path = format!("{path}.mtx");
    let t = unpack(rom.data(rom.file(&text_path)?))?;
    let info = text_pair(f, &t)?;
    let fonts = ram
        .windows(f.len())
        .enumerate()
        .filter(|(_, b)| *b == f)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    ensure!(
        fonts.len() == 1,
        "expected one complete resident font, found {}",
        fonts.len()
    );
    let payload = &t[info.payload..];
    let mut matches = Vec::new();
    for (p, b) in ram.windows(payload.len()).enumerate() {
        if b != payload || p < info.payload {
            continue;
        }
        let start = p - info.payload;
        let address = 0x02000000 + start;
        let mut relocated = t.clone();
        for offset in (4..info.payload).step_by(4) {
            put32(&mut relocated, offset, address + u32le(&t, offset)?)?;
        }
        if ram.get(start..start + t.len()) == Some(relocated.as_slice()) {
            matches.push((address, sha(&relocated)));
        }
    }
    ensure!(
        matches.len() == 1,
        "expected one complete relocated MTX, found {}",
        matches.len()
    );
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"ram_sha256":sha(ram),"font_path":font_path,"font_address":0x02000000+fonts[0],"font_size":f.len(),"font_sha256":sha(f),"text_path":text_path,"text_address":matches[0].0,"text_size":t.len(),"text_relocated_sha256":matches[0].1,"claim":"complete stored assets to frozen RAM; screen and live binding require separate evidence"}),
    )
}

#[cfg(test)]
mod tests;
