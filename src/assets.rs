pub mod scope;
use crate::format::*;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn inc(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.into()).or_default() += 1;
}
/// Investigation output: stored and decoded member identity, without format adoption.
pub fn extract_archive(rom: &Rom, file: &str, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let entry = rom.file(file)?;
    let archive = unpack(rom.data(entry))?;
    let narc = Narc::parse(&archive)?;
    fs::create_dir_all(out)?;
    let mut members = Vec::new();
    for (id, stored) in narc.members.iter().enumerate() {
        let decoded = unpack(stored)?;
        fs::write(out.join(format!("{id}.bin")), &decoded)?;
        members.push(json!({"member":id,"stored_size":stored.len(),"stored_sha256":sha(stored),"decoded_size":decoded.len(),"decoded_sha256":sha(&decoded),"prefix_hex":hex::encode(&decoded[..decoded.len().min(16)])}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"archive":file,"file_id":entry.id,"stored_archive_sha256":sha(rom.data(entry)),"decoded_archive_sha256":sha(&archive),"members":members,"claim":"decoded member extraction only; image/layout interpretation not adopted"});
    json_file(&out.join("archive.json"), &report)?;
    Ok(report)
}
pub fn json_file(p: &Path, value: &Value) -> Result<()> {
    fs::write(p, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}
fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        _ => v.to_string(),
    }
}
fn tsv(p: &Path, rows: &[Value], cols: &[&str]) -> Result<()> {
    fn field(s: String) -> String {
        if s.contains(['\t', '\n', '\r', '"']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s
        }
    }
    let mut out = cols.join("\t") + "\n";
    for r in rows {
        out += &(cols
            .iter()
            .map(|c| field(text(&r[*c])))
            .collect::<Vec<_>>()
            .join("\t")
            + "\n");
    }
    fs::write(p, out)?;
    Ok(())
}
fn role(path: &str, b: &[u8]) -> &'static str {
    if path.starts_with("script/") || path.ends_with(".pss") {
        "script"
    } else if path.ends_with(".fnt") {
        "font"
    } else if path.ends_with(".mtx") {
        "text"
    } else if path.starts_with("text/") {
        "text_candidate"
    } else if path.ends_with(".srl") {
        "embedded_rom"
    } else if b.starts_with(b"DSIF") {
        "DSIF_metadata"
    } else if path.ends_with(".narc") {
        "graphics_or_layout_candidate"
    } else {
        "other"
    }
}
pub fn delta_ranges(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for i in 0..a.len().max(b.len()) {
        let different = a.get(i) != b.get(i);
        if different && start.is_none() {
            start = Some(i);
        }
        if !different {
            if let Some(s) = start.take() {
                out.push((s, i));
            }
        }
    }
    if let Some(s) = start {
        out.push((s, a.len().max(b.len())));
    }
    out
}
fn script_delta(a: &[u8], b: &[u8]) -> Value {
    let tokenize = |v: &[u8]| -> Vec<Vec<Vec<u8>>> {
        v.split(|c| *c == b'\n' || *c == b'\r')
            .map(|line| {
                line.split(u8::is_ascii_whitespace)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_vec())
                    .collect::<Vec<_>>()
            })
            .filter(|l| !l.is_empty())
            .collect()
    };
    let x = tokenize(a);
    let y = tokenize(b);
    let mut commands = BTreeMap::new();
    let mut positions = BTreeMap::new();
    let comparison = if a == b {
        "same"
    } else if x == y {
        "whitespace_tokens_same"
    } else if x.iter().map(|r| &r[0]).ne(y.iter().map(|r| &r[0])) {
        "command_sequence_changed"
    } else {
        for (u, v) in x.iter().zip(&y) {
            if u == v {
                continue;
            }
            let op = String::from_utf8_lossy(&u[0]);
            inc(&mut commands, &op);
            if u.len() != v.len() {
                inc(&mut positions, &format!("{op}:argument_count"));
            }
            for i in 1..u.len().min(v.len()) {
                if u[i] != v[i] {
                    inc(&mut positions, &format!("{op}:argument_{i}"));
                }
            }
        }
        "arguments_changed"
    };
    json!({"comparison":comparison,"changed_commands":serde_json::to_string(&commands).unwrap(),"changed_positions":serde_json::to_string(&positions).unwrap()})
}

pub fn compare_assets(j: &Rom, e: &Rom, jp: &Profile, ep: &Profile, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output already exists");
    let jk = j.keyed()?;
    let ek = e.keyed()?;
    ensure!(
        jk.keys().eq(ek.keys()),
        "top-level identities differ; alignment review required"
    );
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    let mut members = Vec::new();
    let mut all_members = Vec::new();
    let mut archives = Vec::new();
    let mut scripts = Vec::new();
    let mut pairs = Vec::new();
    let mut counts = BTreeMap::new();
    let mut totals = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut suffixes = BTreeMap::new();
    for (name, x) in &jk {
        let y = ek[name];
        let a = j.data(x);
        let b = e.data(y);
        let da = unpack(a)?;
        let db = unpack(b)?;
        let state = if a == b { "same" } else { "changed" };
        inc(&mut counts, state);
        let rec = json!({"path":name,"jp_file_id":x.id,"eng_file_id":y.id,"status":state,"role":role(name,&[]),"jp_size":a.len(),"eng_size":b.len(),"jp_sha256":sha(a),"eng_sha256":sha(b),"decoded_same":da==db});
        if a != b {
            changed.push(rec);
            inc(
                &mut suffixes,
                &format!(
                    ".{}",
                    Path::new(name)
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            );
        } else if ["text/", "script/", "lc_font/", "debug_font/"]
            .iter()
            .any(|p| name.starts_with(p))
            || name.ends_with(".narc")
            || name.ends_with(".srl")
        {
            unchanged.push(rec);
        }
        if let Some(stem) = name.strip_suffix(".fnt") {
            let jt = unpack(j.data(j.file(&format!("{stem}.mtx"))?))?;
            let et = unpack(e.data(e.file(&format!("{stem}.mtx"))?))?;
            let ji = text_pair(&da, &jt)?;
            let ei = text_pair(&db, &et)?;
            pairs.push(json!({"path":stem,"font_changed":a!=b,"text_changed":jt!=et,"jp_glyphs":ji.glyph_count,"jp_references":ji.references.len(),"jp_height":ji.height,"jp_width":ji.width,"eng_glyphs":ei.glyph_count,"eng_references":ei.references.len(),"eng_height":ei.height,"eng_width":ei.width}));
        }
        if name.ends_with(".pss") {
            let mut r = script_delta(&da, &db);
            r["path"] = json!(name);
            r["member_id"] = json!("");
            scripts.push(r);
        }
        if !name.ends_with(".narc") {
            continue;
        }
        let ja = Narc::parse(&da)?;
        let ea = Narc::parse(&db)?;
        ensure!(
            ja.members.len() == ea.members.len(),
            "NARC member count changed: {name}"
        );
        let decoded_eng = ea
            .members
            .iter()
            .map(|b| unpack(b))
            .collect::<Result<Vec<_>>>()?;
        let mut reverse: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, b) in decoded_eng.iter().enumerate() {
            reverse.entry(sha(b)).or_default().push(i);
        }
        let mut c = BTreeMap::new();
        let mut renamed = 0;
        for (i, (a, b)) in ja.members.iter().zip(&ea.members).enumerate() {
            let ename = ea.names.get(&i);
            if let Some(n) = ename {
                let numeric = n.strip_suffix(".pss").unwrap_or(n);
                ensure!(
                    numeric.parse::<usize>()? == i,
                    "ENG archive name/index mismatch"
                );
            }
            renamed += usize::from(ja.names.get(&i) != ename);
            let pa = unpack(a)?;
            let pb = &decoded_eng[i];
            let sa = sha(&pa);
            let sb = sha(pb);
            let state = if a == b {
                "same"
            } else if pa == *pb {
                "compression_only"
            } else {
                "content_changed"
            };
            inc(&mut c, state);
            inc(&mut totals, state);
            if name.starts_with("script/") {
                let mut r = script_delta(&pa, pb);
                r["path"] = json!(name);
                r["member_id"] = json!(i);
                scripts.push(r);
            }
            let kind = role(name, &pa);
            if state == "content_changed" {
                inc(&mut roles, kind);
            }
            let others = reverse
                .get(&sa)
                .into_iter()
                .flatten()
                .filter(|id| **id != i)
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let record = json!({"archive":name,"member_id":i,"eng_name":ename.cloned().unwrap_or_default(),"alignment":if ename.is_some(){"same_count_index_and_eng_numeric_name"}else{"same_count_index"},"status":state,"role":kind,"jp_size":a.len(),"eng_size":b.len(),"jp_decoded_size":pa.len(),"eng_decoded_size":pb.len(),"jp_sha256":sha(a),"eng_sha256":sha(b),"jp_decoded_sha256":sa,"eng_decoded_sha256":sb,"jp_payload_other_eng_ids":others,"jp_prefix_hex":hex::encode(&pa[..pa.len().min(8)])});
            if state != "same" {
                members.push(record.clone());
            }
            all_members.push(record);
        }
        archives.push(json!({"path":name,"members":ja.members.len(),"outer_changed":a!=b,"same":c.get("same").unwrap_or(&0),"compression_only":c.get("compression_only").unwrap_or(&0),"content_changed":c.get("content_changed").unwrap_or(&0),"renamed_members":renamed,"role":role(name,&[])}));
    }
    let mut code = Vec::new();
    for (cpu, off, len_off) in [("arm9", 0x20, 0x2c), ("arm7", 0x30, 0x3c)] {
        let a = slice(j.bytes, u32le(j.bytes, off)?, u32le(j.bytes, len_off)?)?;
        let b = slice(e.bytes, u32le(e.bytes, off)?, u32le(e.bytes, len_off)?)?;
        for (start, end) in delta_ranges(a, b) {
            code.push(json!({"unit":format!("{cpu}_stored"),"coordinate":"stored_image_offset","start":start,"end":end,"jp_hex":hex::encode(a.get(start..end).unwrap_or_default()),"eng_hex":hex::encode(b.get(start..end).unwrap_or_default())}));
        }
    }
    // This comparison does not infer runtime coordinates from compressed offsets.
    let mut script_counts = BTreeMap::new();
    for s in &scripts {
        inc(&mut script_counts, s["comparison"].as_str().unwrap());
    }
    let equal_span = |off, lenoff| -> Result<bool> {
        Ok(
            slice(j.bytes, u32le(j.bytes, off)?, u32le(j.bytes, lenoff)?)?
                == slice(e.bytes, u32le(e.bytes, off)?, u32le(e.bytes, lenoff)?)?,
        )
    };
    let summary = json!({"source_sha256":jp.sha256,"reference_sha256":ep.sha256,"outer_counts":counts,"archive_count":archives.len(),"member_counts":totals,"changed_members_by_role":roles,"changed_files_by_suffix":suffixes,"script_comparisons":script_counts,"same_outer_nonmember_regions":{"arm7":equal_span(0x30,0x3c)?,"banner":banner(j.bytes)?==banner(e.bytes)?,"arm9_overlay_table":equal_span(0x50,0x54)?},"limits":"ENG edits are a reference worklist. Positional member alignment with numeric ENG names does not prove consumer equivalence. Graphics/layout candidates, script arguments, embedded SRL and unchanged Japanese text require review."});
    fs::create_dir_all(out)?;
    json_file(&out.join("summary.json"), &summary)?;
    let file_cols = [
        "path",
        "jp_file_id",
        "eng_file_id",
        "status",
        "role",
        "jp_size",
        "eng_size",
        "jp_sha256",
        "eng_sha256",
        "decoded_same",
    ];
    tsv(&out.join("changed-files.tsv"), &changed, &file_cols)?;
    tsv(
        &out.join("unchanged-candidates.tsv"),
        &unchanged,
        &file_cols,
    )?;
    let member_cols = [
        "archive",
        "member_id",
        "eng_name",
        "alignment",
        "status",
        "role",
        "jp_size",
        "eng_size",
        "jp_decoded_size",
        "eng_decoded_size",
        "jp_sha256",
        "eng_sha256",
        "jp_decoded_sha256",
        "eng_decoded_sha256",
        "jp_payload_other_eng_ids",
        "jp_prefix_hex",
    ];
    tsv(&out.join("changed-members.tsv"), &members, &member_cols)?;
    tsv(&out.join("all-members.tsv"), &all_members, &member_cols)?;
    tsv(
        &out.join("archives.tsv"),
        &archives,
        &[
            "path",
            "members",
            "outer_changed",
            "same",
            "compression_only",
            "content_changed",
            "renamed_members",
            "role",
        ],
    )?;
    tsv(
        &out.join("scripts.tsv"),
        &scripts,
        &[
            "path",
            "member_id",
            "comparison",
            "changed_commands",
            "changed_positions",
        ],
    )?;
    tsv(
        &out.join("text-pairs.tsv"),
        &pairs,
        &[
            "path",
            "font_changed",
            "text_changed",
            "jp_glyphs",
            "jp_references",
            "jp_height",
            "jp_width",
            "eng_glyphs",
            "eng_references",
            "eng_height",
            "eng_width",
        ],
    )?;
    tsv(
        &out.join("code-ranges.tsv"),
        &code,
        &["unit", "coordinate", "start", "end", "jp_hex", "eng_hex"],
    )?;
    Ok(summary)
}

pub fn inventory(rom: &Rom, profile: &Profile) -> Result<Value> {
    let mut files = Vec::new();
    let mut archives = Vec::new();
    let mut pairs = Vec::new();
    for (name, e) in rom.keyed()? {
        let raw = rom.data(e);
        let data = unpack(raw)?;
        files.push(json!({"path":name,"file_id":e.id,"rom_offset":e.start,"size_bytes":raw.len(),"sha256":sha(raw),"decoded_size":data.len(),"decoded_sha256":sha(&data)}));
        if name.ends_with(".narc") {
            let a = Narc::parse(&data)?;
            let mut members = Vec::new();
            for (i, b) in a.members.iter().enumerate() {
                let d = unpack(b)?;
                members.push(json!({"member_id":i,"size_bytes":b.len(),"sha256":sha(b),"decoded_size":d.len(),"decoded_sha256":sha(&d)}));
            }
            archives.push(json!({"path":name,"members":members}));
        }
        if let Some(stem) = name.strip_suffix(".fnt") {
            let t = unpack(rom.data(rom.file(&format!("{stem}.mtx"))?))?;
            pairs.push(json!({"path":stem,"font_sha256":sha(&data),"mtx_sha256":sha(&t),"structure":text_pair(&data,&t)?}));
        }
    }
    Ok(
        json!({"source":profile,"files":files,"archives":archives,"text_pairs":pairs,"overlays":rom.overlays,"claim":"static inventory and structure; not runtime verification"}),
    )
}

#[cfg(test)]
mod tests;
