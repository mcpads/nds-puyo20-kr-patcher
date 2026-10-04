use super::{slice, u16le, u32le};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize)]
pub struct Entry {
    pub id: usize,
    pub path: Option<String>,
    pub start: usize,
    pub end: usize,
}
pub struct Rom<'a> {
    pub bytes: &'a [u8],
    pub files: Vec<Entry>,
    pub fat: usize,
    pub overlays: Vec<Overlay>,
}
#[derive(Serialize)]
pub struct Overlay {
    pub cpu: String,
    pub id: usize,
    pub file_id: usize,
    pub ram: usize,
    pub size: usize,
    pub bss: usize,
    pub flags: usize,
}

pub(super) fn names(data: &[u8]) -> Result<BTreeMap<usize, String>> {
    if data.is_empty() {
        return Ok(BTreeMap::new());
    }
    let count = u16le(data, 6)?;
    ensure!(count > 0 && count <= 4096, "invalid directory count");
    slice(data, 0, count * 8)?;
    fn walk(
        data: &[u8],
        id: usize,
        prefix: &str,
        count: usize,
        visited: &mut BTreeSet<usize>,
        out: &mut BTreeMap<usize, String>,
    ) -> Result<()> {
        ensure!(
            id < count && visited.insert(id),
            "cyclic or duplicate FNT directory"
        );
        let mut pos = u32le(data, id * 8)?;
        ensure!(pos >= count * 8, "FNT names overlap directory table");
        let mut fid = u16le(data, id * 8 + 4)?;
        loop {
            let n = *slice(data, pos, 1)?.first().unwrap();
            pos += 1;
            if n == 0 {
                break;
            }
            let len = (n & 127) as usize;
            ensure!(len > 0, "empty FNT name");
            let name = std::str::from_utf8(slice(data, pos, len)?)?;
            pos += len;
            ensure!(
                !name.contains('/') && !name.contains('\\') && name != ".." && name != ".",
                "unsafe filename"
            );
            let full = format!("{prefix}{name}");
            if n & 128 != 0 {
                let sub = u16le(data, pos)?;
                pos += 2;
                ensure!(sub >= 0xf000, "invalid directory ID");
                walk(data, sub - 0xf000, &format!("{full}/"), count, visited, out)?;
            } else {
                ensure!(out.insert(fid, full).is_none(), "duplicate file ID");
                fid += 1;
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    walk(data, 0, "", count, &mut BTreeSet::new(), &mut result)?;
    Ok(result)
}

impl<'a> Rom<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        slice(bytes, 0, 0x200)?;
        for (offset, length) in [
            (0x20, 0x2c),
            (0x30, 0x3c),
            (0x40, 0x44),
            (0x48, 0x4c),
            (0x50, 0x54),
            (0x58, 0x5c),
        ] {
            slice(bytes, u32le(bytes, offset)?, u32le(bytes, length)?)?;
        }
        let n = names(slice(bytes, u32le(bytes, 0x40)?, u32le(bytes, 0x44)?)?)?;
        let fat = u32le(bytes, 0x48)?;
        let size = u32le(bytes, 0x4c)?;
        ensure!(size % 8 == 0, "invalid FAT size");
        let mut files = Vec::new();
        let mut paths = BTreeSet::new();
        for id in 0..size / 8 {
            let start = u32le(bytes, fat + id * 8)?;
            let end = u32le(bytes, fat + id * 8 + 4)?;
            ensure!(start <= end, "reversed FAT extent");
            slice(bytes, start, end - start)?;
            let path = n.get(&id).cloned();
            if let Some(p) = &path {
                ensure!(paths.insert(p.clone()), "duplicate file path");
            }
            files.push(Entry {
                id,
                path,
                start,
                end,
            });
        }
        ensure!(
            n.keys().all(|id| *id < files.len()),
            "FNT refers outside FAT"
        );
        let mut overlays = Vec::new();
        let mut ids = BTreeSet::new();
        for (cpu, off) in [("arm9", 0x50), ("arm7", 0x58)] {
            let table = slice(bytes, u32le(bytes, off)?, u32le(bytes, off + 4)?)?;
            ensure!(table.len() % 32 == 0, "invalid overlay table");
            for p in (0..table.len()).step_by(32) {
                let id = u32le(table, p)?;
                let fid = u32le(table, p + 24)?;
                ensure!(
                    fid < files.len() && ids.insert((cpu, id)),
                    "invalid overlay identity"
                );
                overlays.push(Overlay {
                    cpu: cpu.into(),
                    id,
                    file_id: fid,
                    ram: u32le(table, p + 4)?,
                    size: u32le(table, p + 8)?,
                    bss: u32le(table, p + 12)?,
                    flags: u32le(table, p + 28)?,
                });
            }
        }
        Ok(Self {
            bytes,
            files,
            fat,
            overlays,
        })
    }
    pub fn data(&self, e: &Entry) -> &'a [u8] {
        &self.bytes[e.start..e.end]
    }
    pub fn file(&self, path: &str) -> Result<&Entry> {
        self.files
            .iter()
            .find(|e| e.path.as_deref() == Some(path))
            .ok_or_else(|| anyhow::anyhow!("missing file {path}"))
    }
    pub fn keyed(&self) -> Result<BTreeMap<String, &Entry>> {
        let mut result = BTreeMap::new();
        let mut used = BTreeSet::new();
        for f in &self.files {
            if let Some(p) = &f.path {
                result.insert(p.clone(), f);
                used.insert(f.id);
            }
        }
        for o in &self.overlays {
            result.insert(
                format!("@{}/overlay/{}", o.cpu, o.id),
                &self.files[o.file_id],
            );
            used.insert(o.file_id);
        }
        ensure!(used.len() == self.files.len(), "unaccounted unnamed file");
        Ok(result)
    }
}

// Only the banner version measured in the two registered NDS inputs is supported.
pub fn banner(bytes: &[u8]) -> Result<&[u8]> {
    let offset = u32le(bytes, 0x68)?;
    if offset == 0 {
        return Ok(&[]);
    }
    ensure!(u16le(bytes, offset)? == 1, "unsupported banner version");
    slice(bytes, offset, 0x840)
}
