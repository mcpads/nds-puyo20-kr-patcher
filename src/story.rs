use crate::{assets::json_file, format::*};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn inspect_controls(ram: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB main RAM");
    let table = slice(ram, 0x1018c0, 132)?;
    ensure!(
        sha(table) == "5b79bd7f4f8c33556a2aebeeaacfa15b41694de2e98d21c3e0b5cda884705253",
        "control dispatch table changed"
    );
    let mut records = Vec::new();
    for row in table.chunks_exact(12) {
        records.push(json!({"opcode":format!("{:04X}",u16le(row,0)?),"argument_halfwords":u16le(row,2)?,"handler_address":u32le(row,4)?,"reserved":u32le(row,8)?}));
    }
    let mut handlers = Vec::new();
    for (offset, expected) in [
        (0x757f4, "0000a0e31eff2fe1"),
        (0x757fc, "0100a0e11eff2fe1"),
        (
            0x75804,
            "403090e5602090e5483080e5082092e5fa35d0e1102092e54cc090e5042092e5022083e002268ce04c2080e50100a0e11eff2fe1",
        ),
        (
            0x7535c,
            "64009ae500008de548009ae504008de54c009ae508008de5ba12dae12830dae560009ae55efeffeb",
        ),
        (
            0x753b8,
            "5c109ae560009ae5b010d1e144feffeb5c109ae548209ae5010a80e2003082e0022081e234109ae538009ae548308ae5000081e05c208ae534008ae5",
        ),
        (
            0x75480,
            "08402de90020a0e1600092e50010a0e35c1082e5000050e30880bd08ba12d2e170feffeb0880bde8",
        ),
        (
            0x75838,
            "38402de90050a0e1403095e5442095e50140a0e1483085e54c2085e509ffffeb3c1095e50400a0e1381085e53880bde8",
        ),
        (
            0x75868,
            "b020d1e1543090e5000053e38220a011b2209311bc22c011020081e21eff2fe1",
        ),
        (0x75888, "be22d0e1bc22c0e10100a0e11eff2fe1"),
        (0x75898, "0020e0e3342080e50100a0e11eff2fe1"),
        (0x758a8, "b020d1e10226a0e1342080e5020081e21eff2fe1"),
    ] {
        let expected = hex::decode(expected)?;
        let bytes = slice(ram, offset, expected.len())?;
        ensure!(bytes == expected, "control handler code changed");
        let mut instructions = Vec::new();
        for (i, word) in bytes.chunks_exact(4).enumerate() {
            let typed = arm946e_s::decode_arm_bytes(word)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&typed)? == word,
                "ARM946E-S instruction round trip mismatch"
            );
            instructions.push(json!({"address":0x02000000+offset+i*4,"bytes":hex::encode(word),"typed_instruction":format!("{typed:?}")}));
        }
        handlers.push(
            json!({"address":0x02000000+offset,"sha256":sha(bytes),"instructions":instructions}),
        );
    }
    Ok(
        json!({"ram_sha256":sha(ram),"region_base":0x02000000,"table_address":0x021018c0,"table_sha256":sha(table),"records":records,"handlers":handlers,"isa":"ARM946E-S ARM state; retro-typed-isa 399e64f86a06d10b6358f26a00569c58b92e7da2","claim":"exact table and handler bytes; typed decode/encode verification; live execution identity is recorded separately"}),
    )
}

/// Preserve argument words as controls, never reinterpret them as glyph slots.
pub fn decode(f: &[u8], bytes: &[u8], stride: usize, count: usize) -> Result<String> {
    ensure!(bytes.len() % 2 == 0, "odd story payload length");
    let mut text = String::new();
    let mut p = 0;
    while p < bytes.len() {
        let v = u16le(bytes, p)?;
        p += 2;
        match v {
            0xffff => {
                ensure!(
                    bytes[p..].chunks_exact(2).all(|b| b == [255, 255]),
                    "non-padding after terminator"
                );
                return Ok(text);
            }
            0xfffd => text.push('\n'),
            0xfffe => text.push_str("{FFFE}"),
            0xf801 => text.push_str("{F801}"),
            0xf812 => text.push_str("{F812}"),
            0xf813 => text.push_str("{F813}"),
            0xf800 | 0xf881 => {
                let arg = u16le(bytes, p)?;
                p += 2;
                text.push_str(&format!("{{{v:04X}:{arg:04X}}}"));
            }
            _ => {
                ensure!(v < count, "unsupported story control {v:04X}");
                let c = char::from_u32(u16le(f, 48 + v * stride)? as u32)
                    .ok_or_else(|| anyhow::anyhow!("invalid source character"))?;
                ensure!(
                    !matches!(c, '{' | '}' | '\n'),
                    "source character conflicts with markup"
                );
                text.push(c);
            }
        }
    }
    anyhow::bail!("missing story terminator")
}

/// Line widths of a decoded source reference with the same advance rule as `prepare`.
fn source_line_widths(f: &[u8], bytes: &[u8], stride: usize, academy: bool) -> Result<Vec<usize>> {
    let mut widths = vec![0usize];
    let mut p = 0;
    while p < bytes.len() {
        let v = u16le(bytes, p)?;
        p += 2;
        match v {
            0xffff => break,
            0xf800 | 0xf881 => p += 2,
            v if v >= 0xf800 => {
                if resets_line_width(&[u16::try_from(v)?], academy) {
                    widths.push(0);
                }
            }
            _ => *widths.last_mut().unwrap() += u16le(f, 48 + v * stride + 2)? + 1,
        }
    }
    Ok(widths)
}

pub fn export(rom: &Rom, path: &str, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    ensure!(
        [
            "text/story_demo/",
            "text/win_dialogue/",
            "text/shop/",
            "text/simulator/"
        ]
        .iter()
        .any(|p| path.starts_with(p))
            || academy_scope(path).is_some(),
        "not a story text path"
    );
    let f = unpack(rom.data(rom.file(&format!("{path}.fnt"))?))?;
    let t = unpack(rom.data(rom.file(&format!("{path}.mtx"))?))?;
    let info = text_pair(&f, &t)?;
    ensure!(
        info.height == 11 && info.width == 16 && info.groups.windows(2).all(|g| g[0] < g[1]),
        "unexpected story topology: {}x{} cells, groups {:?}",
        info.width,
        info.height,
        info.groups
    );
    let mut entries = Vec::new();
    for (i, &start) in info.references.iter().enumerate() {
        let end = info.references.get(i + 1).copied().unwrap_or(t.len());
        ensure!(end > start, "non-increasing story spans");
        let table = info.groups[0] + i * 4;
        let group = info.groups.iter().rposition(|g| *g <= table).unwrap();
        let bytes = slice(&t, start, end - start)?;
        let source = decode(&f, bytes, info.stride, info.glyph_count)?;
        entries.push(json!({"id":i,"group":group,"index":(table-info.groups[group])/4,"source":source,"source_line_widths":source_line_widths(&f,bytes,info.stride,academy_scope(path).is_some())?,"korean":null}));
    }
    let result = json!({"path":path,"source_font_sha256":sha(&f),"source_text_sha256":sha(&t),"font_sha256":"2c709890595668f7bdb6df408420fda957dde0288e95b31a1cc17a2ab98b4b4f","state":"untranslated","entries":entries});
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    json_file(out, &result)?;
    Ok(result)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    path: String,
    source_font_sha256: String,
    source_text_sha256: String,
    font_sha256: String,
    #[serde(default)]
    cell_height: Option<usize>,
    state: String,
    entries: Vec<Entry>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: usize,
    group: usize,
    index: usize,
    source: String,
    korean: String,
}
#[derive(Debug, PartialEq, Eq)]
enum Token {
    Glyph(char),
    Control(Vec<u16>),
}
fn tokens(text: &str) -> Result<Vec<Token>> {
    parse_tokens(text, false)
}
fn parse_tokens(text: &str, academy: bool) -> Result<Vec<Token>> {
    let mut result = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        result.push(match c {
            '\n' => Token::Control(vec![0xfffd]),
            '{' => {
                let mut tag = String::new();
                loop {
                    let next = chars
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("unterminated control"))?;
                    if next == '}' {
                        break;
                    }
                    ensure!(tag.len() < 9 && next.is_ascii(), "invalid control tag");
                    tag.push(next);
                }
                if tag == "F813" || (academy && matches!(tag.as_str(), "F801" | "F812")) {
                    Token::Control(vec![u16::from_str_radix(&tag, 16)?])
                } else {
                    ensure!(
                        (tag.starts_with("F881:") || (academy && tag.starts_with("F800:")))
                            && tag.len() == 9,
                        "unsupported control tag"
                    );
                    let arg = u16::from_str_radix(&tag[5..], 16)?;
                    let opcode = u16::from_str_radix(&tag[..4], 16)?;
                    ensure!(
                        tag == format!("{opcode:04X}:{arg:04X}"),
                        "noncanonical control"
                    );
                    Token::Control(vec![opcode, arg])
                }
            }
            '}' => anyhow::bail!("unmatched closing brace"),
            _ => {
                ensure!(
                    !c.is_control() && (c as u32) < 0xfffd,
                    "unsupported character"
                );
                Token::Glyph(c)
            }
        });
    }
    Ok(result)
}
fn controls(tokens: &[Token]) -> Vec<&[u16]> {
    tokens
        .iter()
        .filter_map(|t| match t {
            Token::Control(v) => Some(v.as_slice()),
            _ => None,
        })
        .collect()
}

fn academy_scope(path: &str) -> Option<(usize, usize)> {
    match path {
        "text/academy/challenge" => Some((1, 18)),
        "text/academy/tuto00" => Some((7, 175)),
        "text/academy/tuto01" => Some((12, 309)),
        _ => None,
    }
}

// Preserve occupied prose segments and symbol order at every control boundary.
fn academy_segments(tokens: &[Token]) -> Vec<(bool, Vec<char>)> {
    let mut segments = vec![(false, Vec::new())];
    for token in tokens {
        match token {
            Token::Glyph(c) => {
                let segment = segments.last_mut().unwrap();
                segment.0 = true;
                if matches!(c, 'Θ' | 'Ω') {
                    segment.1.push(*c);
                }
            }
            Token::Control(v) if v != &[0xfffd] => segments.push((false, Vec::new())),
            _ => {}
        }
    }
    segments
}

fn resets_line_width(units: &[u16], academy: bool) -> bool {
    units == [0xfffd] || (academy && units == [0xf812])
}

fn page_line_breaks(tokens: &[Token]) -> Vec<usize> {
    let mut counts = vec![0];
    for token in tokens {
        if let Token::Control(units) = token {
            match units[0] {
                0xfffd => *counts.last_mut().unwrap() += 1,
                0xf812 | 0xf813 | 0xf881 => counts.push(0),
                _ => {}
            }
        }
    }
    counts
}

/// Story MTX topology (groups, references) verified from the registered source.
fn story_scope(path: &str) -> Option<(usize, usize)> {
    Some(match path.strip_prefix("text/story_demo/")? {
        "general" => (2, 52),
        "00_arl" => (16, 238),
        "01_ami" => (16, 248),
        "02_rng" => (16, 214),
        "03_sig" => (16, 265),
        "04_raf" => (16, 202),
        "05_shz" => (16, 235),
        "06_rul" => (16, 228),
        "07_sat" => (16, 208),
        "08_car" => (16, 193),
        "09_suk" => (16, 199),
        "10_wch" => (16, 251),
        "11_dra" => (16, 193),
        "12_rid" => (16, 244),
        "13_klu" => (16, 182),
        "14_fel" => (16, 211),
        "15_lem" => (16, 188),
        "16_aco" => (16, 254),
        "17_yur" => (16, 189),
        "18_oni" => (16, 219),
        "19_dng" => (16, 180),
        "20_pri" => (16, 260),
        "21_mag" => (16, 219),
        "22_ris" => (16, 214),
        "23_eco" => (16, 225),
        "extra" => (16, 356),
        _ => return None,
    })
}

/// Win dialogue MTX topology; originals reach 209px, which is the adopted line limit.
fn win_scope(path: &str) -> Option<(usize, usize)> {
    let name = path.strip_prefix("text/win_dialogue/")?;
    Some(match name {
        "academy_boss_win" => (2, 46),
        "vs_win" => (1, 123),
        "story_win_extra" => (1, 14),
        // One file per character story, e.g. story_win_00_arl.
        _ => {
            story_scope(&format!(
                "text/story_demo/{}",
                name.strip_prefix("story_win_")?
            ))?;
            (1, 16)
        }
    })
}

pub fn prepare(rom: &Rom, translation: &Path, font_path: &Path, out: &Path) -> Result<Value> {
    use std::collections::{BTreeMap, BTreeSet};
    ensure!(!out.exists(), "output exists");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    let academy = academy_scope(&tr.path);
    let win = win_scope(&tr.path);
    let story = story_scope(&tr.path).or(win);
    ensure!(
        (academy.is_some() || story.is_some()) && tr.state == "development_draft",
        "unverified story scope/state"
    );
    let fe = rom.file(&format!("{}.fnt", tr.path))?;
    let te = rom.file(&format!("{}.mtx", tr.path))?;
    let f = unpack_halfword(rom.data(fe))?;
    let t = unpack(rom.data(te))?;
    ensure!(
        sha(&f) == tr.source_font_sha256 && sha(&t) == tr.source_text_sha256,
        "translation source identity mismatch"
    );
    let info = text_pair(&f, &t)?;
    let expected = story.or(academy).unwrap();
    let limit = if academy.is_some() {
        120
    } else if win.is_some() {
        209
    } else {
        220
    };
    ensure!(
        info.height == 11
            && info.width == 16
            && info.groups.len() == expected.0
            && info.references.len() == expected.1
            && info.groups.windows(2).all(|g| g[0] < g[1])
            && info.references.first() == Some(&info.payload)
            && tr.entries.len() == expected.1,
        "unsupported story topology"
    );
    let parsed = tr
        .entries
        .iter()
        .map(|e| {
            if academy.is_some() {
                parse_tokens(&e.korean, true)
            } else {
                tokens(&e.korean)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let mut chars = parsed
        .iter()
        .flatten()
        .filter_map(|t| match t {
            Token::Glyph(c) => Some(*c),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut preserved = BTreeMap::new();
    if academy.is_some() {
        ensure!(info.stride == 92, "academy glyph stride changed");
        for slot in 0..info.glyph_count {
            let record = slice(&f, 48 + slot * info.stride, info.stride)?;
            let c = char::from_u32(u16le(record, 0)? as u32)
                .ok_or_else(|| anyhow::anyhow!("invalid source codepoint"))?;
            if matches!(c, 'Θ' | 'Ω') {
                ensure!(preserved.insert(c, record).is_none(), "duplicate symbol");
                chars.insert(c);
            }
        }
    }
    ensure!(
        !chars.is_empty() && chars.len() < 0xf800,
        "invalid glyph population"
    );
    let font_bytes = fs::read(font_path)?;
    ensure!(sha(&font_bytes) == tr.font_sha256, "font identity mismatch");
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let height = tr.cell_height.unwrap_or(11);
    ensure!(matches!(height, 11 | 12), "unsupported story cell height");
    if height == 12 {
        let (arm9, _) = crate::arm9::decode(slice(
            rom.bytes,
            u32le(rom.bytes, 0x20)?,
            u32le(rom.bytes, 0x2c)?,
        )?)?;
        // The setter derives record stride from FNT height, and the glyph
        // upload reads FNT width/height rather than a fixed eleven-row extent.
        for (start, size, expected) in [
            (
                0xda104,
                0x48,
                "35b653a09f3d9187018a0425cc334efdbd51eae170fad7b4b3c21c11f4205967",
            ),
            (
                0x74d00,
                0x74,
                "6eaac6a3c383fc9fbfc258dcbf490de8a087fa1fcbe3b64f9111a1658ffc0320",
            ),
        ] {
            ensure!(
                sha(slice(&arm9, start, size)?) == expected,
                "story FNT dimension consumer changed"
            );
        }
    }
    let output_stride = 4 + height * 8;
    let mut changed_font = f[..48].to_vec();
    put32(&mut changed_font, 4, height)?;
    put32(&mut changed_font, 12, chars.len())?;
    let mut slots = BTreeMap::new();
    for (i, c) in chars.iter().enumerate() {
        if let Some(record) = preserved.get(c) {
            changed_font.extend_from_slice(record);
            // Original icon pixels stay at their original coordinates.
            changed_font.resize(changed_font.len() + (height - info.height) * 8, 0);
            slots.insert(*c, (i as u16, 13));
            continue;
        }
        ensure!(
            !matches!(c, 'Θ' | 'Ω') || academy.is_none(),
            "missing original symbol"
        );
        let (pixels, advance) = crate::localize::glyph(&font, *c, height)?;
        if height == 12 {
            ensure!(
                pixels[(height - 1) * 8..]
                    .iter()
                    .all(|v| v & 15 != 1 && v >> 4 != 1),
                "story glyph leaves no complete shadow row: {c}"
            );
        }
        let advance = if *c == ' '
            && tr.font_sha256 == "2380e9cc83e03a71f6abecbdbcad226061e3483ae19d4265f28797d4be73c2e5"
        {
            font.metrics(' ', 12.0).advance_width.round() as usize
        } else {
            advance
        };
        changed_font.extend_from_slice(&(*c as u16).to_le_bytes());
        changed_font.extend_from_slice(&(advance as u16).to_le_bytes());
        changed_font.extend(pixels);
        slots.insert(*c, (i as u16, advance));
    }
    let mut changed_text = t[..info.payload].to_vec();
    let mut entries = Vec::new();
    for (i, e) in tr.entries.iter().enumerate() {
        let table = info.groups[0] + i * 4;
        let group = info.groups.iter().rposition(|g| *g <= table).unwrap();
        ensure!(
            e.id == i && e.group == group && e.index == (table - info.groups[group]) / 4,
            "entry topology/order mismatch"
        );
        let start = info.references[i];
        let end = info.references.get(i + 1).copied().unwrap_or(t.len());
        ensure!(end > start, "non-increasing source spans");
        ensure!(
            decode(
                &f,
                slice(&t, start, end - start)?,
                info.stride,
                info.glyph_count
            )? == e.source,
            "source prose mismatch: {i}"
        );
        let original = if academy.is_some() {
            parse_tokens(&e.source, true)?
        } else {
            tokens(&e.source)?
        };
        let relevant_controls = |ts: &[Token]| -> Vec<Vec<u16>> {
            controls(ts)
                .into_iter()
                .filter(|v| academy.is_none() || *v != [0xfffd])
                .map(|v| v.to_vec())
                .collect()
        };
        ensure!(
            relevant_controls(&original) == relevant_controls(&parsed[i]),
            "control sequence/argument changed: {i}"
        );
        ensure!(
            original.iter().any(|t| matches!(t, Token::Glyph(_)))
                == parsed[i].iter().any(|t| matches!(t, Token::Glyph(_))),
            "empty/nonempty translation mismatch: {i}"
        );
        if academy.is_some() {
            ensure!(
                academy_segments(&original) == academy_segments(&parsed[i]),
                "prose segment or protected symbol changed: {i}"
            );
            ensure!(
                page_line_breaks(&original) == page_line_breaks(&parsed[i]),
                "academy line count across waits/pages changed: {i}"
            );
        }
        let target = changed_text.len();
        put32(&mut changed_text, info.groups[0] + i * 4, target)?;
        let mut widths = vec![0usize];
        for token in &parsed[i] {
            match token {
                Token::Glyph(c) => {
                    let (slot, advance) = slots[c];
                    changed_text.extend_from_slice(&slot.to_le_bytes());
                    // ARM9 0x020753d0 adds 0x1000 to glyph advance (12-bit fixed point).
                    *widths.last_mut().unwrap() += advance + 1;
                }
                Token::Control(units) => {
                    for unit in units {
                        changed_text.extend_from_slice(&unit.to_le_bytes());
                    }
                    // F812 restores the initial cursor; F813 only waits.
                    if resets_line_width(units, academy.is_some()) {
                        widths.push(0);
                    }
                }
            }
        }
        ensure!(
            widths.iter().all(|w| *w <= limit),
            "text line exceeds adopted width: {i}: {widths:?}"
        );
        changed_text.extend_from_slice(&0xffffu16.to_le_bytes());
        ensure!(
            decode(
                &changed_font,
                &changed_text[target..],
                output_stride,
                chars.len()
            )? == e.korean,
            "translation round trip failed: {i}"
        );
        entries.push(json!({"id":i,"offset":target,"used_bytes":changed_text.len()-target,"line_widths":widths,"korean":e.korean}));
    }
    let used = changed_text.len();
    changed_text.resize(t.len().max(used).next_multiple_of(2), 0xff);
    let size = changed_text.len();
    put32(&mut changed_text, 0, size)?;
    text_pair(&changed_font, &changed_text)?;
    ensure!(
        changed_font[..4] == f[..4]
            && u32le(&changed_font, 4)? == height
            && changed_font[8..12] == f[8..12]
            && changed_font[16..48] == f[16..48]
            && changed_text[4..info.groups[0]] == t[4..info.groups[0]],
        "protected header/group pointers changed"
    );
    ensure!(
        rom.data(fe).starts_with(b"COMP\x11") && rom.data(te) == t,
        "unverified storage variant"
    );
    let packed = crate::compress::pack(&changed_font)?;
    ensure!(
        unpack_halfword(&packed)? == changed_font,
        "font halfword round trip failed"
    );
    let replacement = |entry: &crate::format::Entry, name: &str, data: &[u8]| json!({"file":entry.path,"expected_sha256":sha(rom.data(entry)),"input":name,"input_sha256":sha(data),"placement":if data.len()>rom.data(entry).len(){"ff_tail"}else{"original"}});
    let plan = json!({"source_sha256":sha(rom.bytes),"replacements":[replacement(fe,"font.fnt",&packed),replacement(te,"text.mtx",&changed_text)]});
    fs::create_dir_all(out)?;
    fs::write(out.join("font.fnt"), &packed)?;
    fs::write(out.join("font.decoded.bin"), &changed_font)?;
    fs::write(out.join("text.mtx"), &changed_text)?;
    json_file(&out.join("plan.json"), &plan)?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"font_sha256":tr.font_sha256,"line_width_limit":limit,"preserved_symbols":preserved.keys().collect::<Vec<_>>(),"symbol_runtime_advance":if academy.is_some() {Some(13)} else {None},"cell_height":height,"source_cell_height":info.height,"glyph_stride":output_stride,"shadow_clipped":height == 11,"rasterizer":"fontdue 0.9.4, native 12px, baseline 11, threshold 128, shadow +1,+1; 12-row output requires a complete lower shadow row","active_glyphs":chars.len(),"source_glyph_count":info.glyph_count,"font_stored_bytes":packed.len(),"original_font_stored_bytes":rom.data(fe).len(),"mtx_bytes":changed_text.len(),"mtx_used_bytes":used,"entries":entries,"state":"development_draft","runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("translation.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
