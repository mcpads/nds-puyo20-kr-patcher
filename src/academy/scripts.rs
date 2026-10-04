//! Preserve script text-call sites without pretending to interpret PSS execution.
use super::*;

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let mut scripts = Vec::new();
    for file in &rom.files {
        let Some(path) = file.path.as_deref() else {
            continue;
        };
        if !path.starts_with("script/academy/") || !path.ends_with(".pss") {
            continue;
        }
        let raw = unpack(rom.data(file))?;
        ensure!(
            raw.last() == Some(&0) && !raw[..raw.len() - 1].contains(&0),
            "PSS terminator changed: {path}"
        );

        let candidate = if path.starts_with("script/academy/tutorial/tuto0") {
            Some("text/academy/tuto00")
        } else if path.starts_with("script/academy/tutorial/tuto1") {
            Some("text/academy/tuto01")
        } else {
            None
        };
        let mut references = BTreeMap::new();
        if let Some(stem) = candidate {
            let f = unpack(rom.data(rom.file(&format!("{stem}.fnt"))?))?;
            let t = unpack(rom.data(rom.file(&format!("{stem}.mtx"))?))?;
            let info = text_pair(&f, &t)?;
            for (id, start) in info.references.iter().enumerate() {
                let table = info.groups[0] + id * 4;
                let group = info
                    .groups
                    .iter()
                    .rposition(|g| *g <= table)
                    .ok_or_else(|| anyhow::anyhow!("missing group"))?;
                let index = (table - info.groups[group]) / 4;
                let end = info.references.get(id + 1).copied().unwrap_or(t.len());
                references.insert((group,index), json!({"id":id,"offset":start,"source":crate::story::decode(&f,slice(&t,*start,end-*start)?,info.stride,info.glyph_count)?}));
            }
        }
        let mut section = "";
        let mut calls = Vec::new();
        let mut waits = 0;
        let mut clears = 0;
        let mut opaque_lines = Vec::new();
        for (line, bytes) in raw[..raw.len() - 1].split(|v| *v == b'\n').enumerate() {
            if !bytes.is_ascii() {
                opaque_lines.push(json!({"line":line+1,"hex":hex::encode(bytes)}));
                continue;
            }
            let value = std::str::from_utf8(bytes)?;
            let words: Vec<_> = value.split_whitespace().collect();
            let Some(command) = words.first().copied() else {
                continue;
            };
            if command == ":" {
                section = value.trim();
            }
            match command {
                "SetText" => {
                    ensure!(
                        words.len() == 6 || (words.len() == 7 && words[6] == "WaitText"),
                        "SetText arity changed: {path}:{}",
                        line + 1
                    );
                    if words.len() == 7 {
                        waits += 1;
                    }
                    let pair = words[1]
                        .parse::<usize>()
                        .ok()
                        .zip(words[2].parse::<usize>().ok());
                    let matched = pair.and_then(|key| references.get(&key));
                    calls.push(json!({"line":line+1,"section":section,"arguments":&words[1..6],"trailing_tokens":&words[6..],"literal_pair":pair,"candidate_reference":matched}));
                }
                "WaitText" => waits += 1,
                "ClearText" => clears += 1,
                _ => {}
            }
        }
        scripts.push(json!({"path":path,"file_id":file.id,"sha256":sha(&raw),"bytes":raw.len(),"candidate_text_pair":candidate,"set_text":calls,"opaque_non_ascii_lines":opaque_lines,"wait_text_count":waits,"clear_text_count":clears}));
    }
    let report = json!({"source_sha256":sha(rom.bytes),"scripts":scripts,"claim":"literal PSS SetText arguments and candidate MTX group/index lookup; filename family association is a hypothesis, not execution or variable resolution"});
    json_file(out, &report)?;
    Ok(report)
}
