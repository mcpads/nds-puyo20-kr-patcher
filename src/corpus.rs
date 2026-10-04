use crate::{assets::json_file, format::*, story};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

/// Enumerate named FNT/MTX references. Unsupported token streams remain explicit,
/// and raw source units and headers preserve every byte independently of prose decoding.
pub fn census(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    fs::create_dir_all(out)?;
    let source_sha256 = sha(rom.bytes);
    let mut pairs = Vec::new();
    let mut references = 0;
    let mut decoded = 0;
    let mut characters = 0;
    for fe in rom.files.iter().filter(|f| {
        f.path
            .as_deref()
            .is_some_and(|p| p.starts_with("text/") && p.ends_with(".fnt"))
    }) {
        let stem = fe.path.as_deref().unwrap().trim_end_matches(".fnt");
        let te = rom.file(&format!("{stem}.mtx"))?;
        let f = unpack(rom.data(fe))?;
        let t = unpack(rom.data(te))?;
        let info = text_pair(&f, &t)?;
        ensure!(
            info.groups.windows(2).all(|g| g[0] < g[1]),
            "non-increasing groups: {stem}"
        );
        let starts = info.references.iter().copied().collect::<BTreeSet<_>>();
        ensure!(
            starts.first() == Some(&info.payload),
            "unreferenced prefix: {stem}"
        );
        let ends = starts
            .iter()
            .copied()
            .skip(1)
            .chain([t.len()])
            .collect::<Vec<_>>();
        let mut spans = Vec::new();
        let mut restored = t[..info.payload].to_vec();
        for (&start, &end) in starts.iter().zip(&ends) {
            let bytes = slice(&t, start, end - start)?;
            let units = bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>();
            restored.extend(units.iter().flat_map(|v| v.to_le_bytes()));
            let prose = story::decode(&f, bytes, info.stride, info.glyph_count);
            let (text, error) = match prose {
                Ok(text) => (Some(text), None),
                Err(e) => (None, Some(e.to_string())),
            };
            spans.push(json!({"offset":start,"end":end,"raw_units":units,"source":text,"unresolved":error}));
        }
        ensure!(restored == t, "source unit round trip failed: {stem}");
        let entries = info
            .references
            .iter()
            .enumerate()
            .map(|(i, start)| {
                let span = spans.iter().position(|s| s["offset"] == *start).unwrap();
                let table = info.groups[0] + i * 4;
                let group = info.groups.iter().rposition(|g| *g <= table).unwrap();
                json!({"id":i,"group":group,"index":(table-info.groups[group])/4,"span":span})
            })
            .collect::<Vec<_>>();
        let resolved = entries
            .iter()
            .filter(|e| spans[e["span"].as_u64().unwrap() as usize]["source"].is_string())
            .count();
        let char_count = spans
            .iter()
            .filter_map(|s| s["source"].as_str())
            .map(|s| {
                let mut in_tag = false;
                s.chars()
                    .filter(|c| match c {
                        '{' => {
                            in_tag = true;
                            false
                        }
                        '}' => {
                            in_tag = false;
                            false
                        }
                        '\n' => false,
                        _ => !in_tag,
                    })
                    .count()
            })
            .sum::<usize>();
        let name = format!("{}.json", fe.id);
        json_file(
            &out.join(&name),
            &json!({"path":stem,"source_sha256":source_sha256,"font_sha256":sha(&f),"text_sha256":sha(&t),"header_hex":hex::encode(&t[..info.payload]),"groups":info.groups,"entries":entries,"spans":spans,"round_trip":"MTX header plus raw unit spans equals source bytes","consumer_status":"runtime scope must be established separately"}),
        )?;
        pairs.push(json!({"path":stem,"font_file_id":fe.id,"text_file_id":te.id,"height":info.height,"reference_count":info.references.len(),"unique_spans":starts.len(),"decoded_references":resolved,"unresolved_references":info.references.len()-resolved,"decoded_characters_in_unique_spans":char_count,"artifact":name}));
        references += info.references.len();
        decoded += resolved;
        characters += char_count;
    }
    let result = json!({"source_sha256":source_sha256,"pairs":pairs,"reference_count":references,"decoded_references":decoded,"unresolved_references":references-decoded,"decoded_characters_in_unique_spans":characters,"scope":"all named text/*.fnt with matching MTX; graphics, executable strings and unproven runtime paths are additional unresolved populations","decoding":"observed FFFD/FFFE/FFFF/F800/F801/F812/F813/F881; unknown controls are retained as raw units, not excluded"});
    json_file(&out.join("census.json"), &result)?;
    Ok(result)
}
