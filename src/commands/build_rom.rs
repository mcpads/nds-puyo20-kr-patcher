use super::profile;
use crate::{assets, build, format::*};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn run(source: PathBuf, plan: Vec<PathBuf>, out: PathBuf) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (b, _) = load(&source, &profile(None))?;
    let mut combined = build::Plan {
        source_sha256: sha(&b),
        replacements: Vec::new(),
        arm9: None,
        banner: None,
        anti_piracy_bypass: false,
    };
    let mut identities = Vec::new();
    for path in plan {
        let bytes = fs::read(&path)?;
        let p: build::Plan = serde_json::from_slice(&bytes)?;
        ensure!(
            p.source_sha256 == combined.source_sha256,
            "plan source mismatch"
        );
        for mut replacement in p.replacements {
            replacement.input = fs::canonicalize(
                path.parent()
                    .unwrap_or(Path::new("."))
                    .join(&replacement.input),
            )?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non UTF-8 input path"))?
            .to_owned();
            combined.replacements.push(replacement);
        }
        if let Some(mut arm9) = p.arm9 {
            ensure!(combined.arm9.is_none(), "duplicate ARM9 writer");
            arm9.input =
                fs::canonicalize(path.parent().unwrap_or(Path::new(".")).join(&arm9.input))?
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("non UTF-8 ARM9 input path"))?
                    .to_owned();
            combined.arm9 = Some(arm9);
        }
        combined.anti_piracy_bypass |= p.anti_piracy_bypass;
        identities.push(json!({"path":path,"sha256":sha(&bytes)}));
    }
    let (output, mut report) = build::build(&b, &combined, Path::new("."))?;
    report["plans"] = json!(identities);
    fs::create_dir_all(&out)?;
    fs::write(out.join("development.nds"), &output)?;
    ensure!(
        sha(&fs::read(out.join("development.nds"))?) == sha(&output),
        "output readback mismatch"
    );
    assets::json_file(&out.join("build.json"), &report)?;
    Ok(report)
}
