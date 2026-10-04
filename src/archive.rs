use crate::{
    build::{Write, apply},
    format::*,
};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Supersession {
    file: String,
    member: usize,
    previous_sha256: String,
    replacement_sha256: String,
}

fn replace_writer(
    file: &str,
    id: usize,
    previous: &[u8],
    next: &[u8],
    rules: &mut BTreeMap<(String, usize), Supersession>,
) -> Result<Supersession> {
    let key = (file.to_owned(), id);
    let rule = rules
        .get(&key)
        .ok_or_else(|| anyhow::anyhow!("duplicate archive member writer: {file} member {id}"))?;
    ensure!(
        sha(previous) == rule.previous_sha256 && sha(next) == rule.replacement_sha256,
        "superseded member identity differs: {file} member {id}"
    );
    Ok(rules.remove(&key).unwrap())
}

/// Combine independently prepared NARC plans; overlaps require exact supersession hashes.
/// Reconstruct each input from the immutable source to reject hidden metadata/padding edits.
pub fn merge_plans(
    rom: &Rom,
    plans: &[std::path::PathBuf],
    out: &std::path::Path,
    supersede: Option<&std::path::Path>,
) -> Result<serde_json::Value> {
    use crate::{assets::json_file, build::Plan};
    use serde_json::json;
    use std::fs;
    ensure!(!out.exists(), "output exists");
    let mut files: BTreeMap<String, BTreeMap<usize, Vec<u8>>> = BTreeMap::new();
    let mut inputs = Vec::new();
    let mut rules = BTreeMap::new();
    if let Some(path) = supersede {
        for rule in serde_json::from_slice::<Vec<Supersession>>(&fs::read(path)?)? {
            ensure!(
                rules
                    .insert((rule.file.clone(), rule.member), rule)
                    .is_none(),
                "duplicate supersession rule"
            );
        }
    }
    let mut superseded = Vec::new();
    for path in plans {
        let path = path.canonicalize()?;
        let bytes = fs::read(&path)?;
        let plan: Plan = serde_json::from_slice(&bytes)?;
        ensure!(
            plan.arm9.is_none(),
            "archive merge cannot consume an ARM9 writer"
        );
        ensure!(
            plan.source_sha256 == sha(rom.bytes),
            "archive plan/source mismatch"
        );
        for item in plan.replacements {
            ensure!(
                item.placement == crate::build::Placement::Original,
                "archive merge requires original placement"
            );
            let original = rom.data(rom.file(&item.file)?);
            ensure!(
                sha(original) == item.expected_sha256,
                "archive source identity mismatch"
            );
            let data = fs::read(path.parent().unwrap().join(&item.input))?;
            ensure!(
                sha(&data) == item.input_sha256,
                "archive input identity mismatch"
            );
            let before = Narc::parse(original)?;
            let after = Narc::parse(&data)?;
            ensure!(
                before.members.len() == after.members.len(),
                "archive member population changed"
            );
            let mut changes = BTreeMap::new();
            for (id, (&old, &new)) in before.members.iter().zip(&after.members).enumerate() {
                if old != new {
                    changes.insert(id, new.to_vec());
                }
            }
            ensure!(
                replace(original, &changes)? == data,
                "input changes protected archive bytes"
            );
            let merged = files.entry(item.file.clone()).or_default();
            for (&id, bytes) in &changes {
                if let Some(previous) = merged.get(&id) {
                    superseded.push(replace_writer(&item.file, id, previous, bytes, &mut rules)?);
                }
                merged.insert(id, bytes.clone());
            }
            inputs.push(json!({"plan":path,"plan_sha256":sha(&bytes),"file":item.file,"input_sha256":item.input_sha256,"members":changes.keys().collect::<Vec<_>>()}));
        }
    }
    ensure!(rules.is_empty(), "unused supersession rules");
    let mut outputs = Vec::new();
    let mut replacements = Vec::new();
    for (i, (name, members)) in files.iter().enumerate() {
        let source = rom.data(rom.file(name)?);
        let data = replace(source, members)?;
        let file = format!("{i}.narc");
        replacements.push(json!({"file":name,"expected_sha256":sha(source),"input":file,"input_sha256":sha(&data)}));
        outputs.push((file, data));
    }
    fs::create_dir_all(out)?;
    for (file, data) in outputs {
        fs::write(out.join(file), data)?;
    }
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":replacements}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"inputs":inputs,"files":replacements,"superseded":superseded,"verification":"each input rederived from original NARC; duplicate writers require exact consumed previous/replacement hash pairs; protected metadata and bytes unchanged"});
    json_file(&out.join("merge.json"), &report)?;
    Ok(report)
}

/// Replace members within their existing extents. Preserve headers, names, padding,
/// all member starts and all unselected bytes; only selected FAT ends may shrink.
pub fn replace(source: &[u8], replacements: &BTreeMap<usize, Vec<u8>>) -> Result<Vec<u8>> {
    let n = Narc::parse(source)?;
    let mut p = 16;
    let (mut fat, mut image) = (None, None);
    for _ in 0..3 {
        match slice(source, p, 4)? {
            b"BTAF" => fat = Some(p + 8),
            b"GMIF" => image = Some(p + 8),
            _ => {}
        };
        p += u32le(source, p + 4)?;
    }
    let fat = fat.ok_or_else(|| anyhow::anyhow!("missing BTAF"))?;
    let image = image.ok_or_else(|| anyhow::anyhow!("missing GMIF"))?;
    let mut writes = Vec::new();
    for (&id, data) in replacements {
        let old = *n
            .members
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("member outside table"))?;
        ensure!(
            !data.is_empty() && data.len() <= old.len(),
            "NARC member {id} exceeds capacity: {} > {}",
            data.len(),
            old.len()
        );
        let start = u32le(source, fat + 4 + id * 8)?;
        let end = u32le(source, fat + 8 + id * 8)?;
        for other in 0..n.members.len() {
            if other == id {
                continue;
            }
            let s = u32le(source, fat + 4 + other * 8)?;
            let e = u32le(source, fat + 8 + other * 8)?;
            ensure!(end <= s || start >= e, "aliased member extent");
        }
        writes.push(Write {
            offset: image + start,
            before: old[..data.len()].to_vec(),
            after: data.clone(),
            label: format!("NARC member {id}"),
        });
        if data.len() != old.len() {
            let offset = fat + 8 + id * 8;
            let mut after = vec![0; 4];
            put32(&mut after, 0, start + data.len())?;
            writes.push(Write {
                offset,
                before: slice(source, offset, 4)?.to_vec(),
                after,
                label: format!("NARC member {id} end"),
            });
        }
    }
    let result = apply(source, &mut writes)?;
    let rebuilt = Narc::parse(&result)?;
    ensure!(
        n.names == rebuilt.names && n.members.len() == rebuilt.members.len(),
        "NARC identities changed"
    );
    for (id, old) in n.members.iter().enumerate() {
        ensure!(
            rebuilt.members[id] == replacements.get(&id).map(Vec::as_slice).unwrap_or(old),
            "member changed unexpectedly"
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
