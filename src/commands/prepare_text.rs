use super::profile;
use crate::{assets, format::*, localize};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

pub(super) fn run(
    source: PathBuf,
    translation: Vec<PathBuf>,
    font: PathBuf,
    out: PathBuf,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (b, _) = load(&source, &profile(None))?;
    let rom = Rom::parse(&b)?;
    let mut reports = Vec::new();
    let mut replacements = Vec::new();
    for (i, path) in translation.iter().enumerate() {
        let directory = out.join(i.to_string());
        reports.push(localize::prepare(&rom, path, &font, &directory)?);
        let plan: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("plan.json"))?)?;
        for item in plan["replacements"].as_array().unwrap() {
            let mut item = item.clone();
            item["input"] = json!(format!("{i}/{}", item["input"].as_str().unwrap()));
            replacements.push(item);
        }
    }
    assets::json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(&b),"replacements":replacements}),
    )?;
    Ok(json!({"reports":reports,"plan":out.join("plan.json")}))
}
