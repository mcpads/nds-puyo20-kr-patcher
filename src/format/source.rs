use super::sha;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub size_bytes: usize,
    pub sha256: String,
}
pub fn load(path: &Path, profile: &Path) -> Result<(Vec<u8>, Profile)> {
    let p: Profile = serde_json::from_slice(&fs::read(profile)?)?;
    let b = fs::read(path)?;
    ensure!(
        b.len() == p.size_bytes && sha(&b) == p.sha256,
        "source identity mismatch"
    );
    Ok((b, p))
}
