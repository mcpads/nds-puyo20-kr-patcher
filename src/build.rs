use crate::{assets::delta_ranges, format::*};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub source_sha256: String,
    pub replacements: Vec<Replacement>,
    #[serde(default)]
    pub arm9: Option<Arm9Replacement>,
    #[serde(default)]
    pub banner: Option<BannerTitles>,
    /// Install the TP4J anti-piracy bypass in the raw ARM9 prefix.
    #[serde(default)]
    pub anti_piracy_bypass: bool,
}
/// Replace every language title of a version 1 banner; the icon is preserved.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerTitles {
    pub scope: String,
    pub version: u16,
    pub source_title: String,
    pub title: String,
}

const BANNER_V1_SIZE: usize = 0x840;
const BANNER_TITLES: usize = 6;

fn banner_write(source: &[u8], titles: &BannerTitles) -> Result<Write> {
    let offset = u32le(source, 0x68)?;
    let banner = slice(source, offset, BANNER_V1_SIZE)?;
    ensure!(
        titles.version == 1 && u16le(banner, 0)? == 1,
        "only version 1 banners are supported"
    );
    ensure!(
        u16le(banner, 2)? == usize::from(crc16(&banner[0x20..])),
        "source banner CRC mismatch"
    );
    let encode = |text: &str| -> Result<Vec<u8>> {
        let mut units: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        ensure!(units.len() < 0x100, "banner title too long: {text}");
        units.resize(0x100, 0);
        Ok(units)
    };
    let expected = encode(&titles.source_title)?;
    let replacement = encode(&titles.title)?;
    let mut after = banner.to_vec();
    for slot in 0..BANNER_TITLES {
        let at = 0x240 + slot * 0x100;
        ensure!(
            banner[at..at + 0x100] == expected[..],
            "banner title {slot} differs from the declared source"
        );
        after[at..at + 0x100].copy_from_slice(&replacement);
    }
    let crc = crc16(&after[0x20..]);
    after[2..4].copy_from_slice(&crc.to_le_bytes());
    ensure!(
        after[0x20..0x240] == banner[0x20..0x240],
        "banner icon changed"
    );
    Ok(Write {
        offset: offset + 2,
        before: banner[2..].to_vec(),
        after: after[2..].to_vec(),
        label: "banner titles and CRC16".into(),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arm9Replacement {
    pub mode: Arm9Mode,
    pub expected_sha256: String,
    pub input: String,
    pub input_sha256: String,
}
#[derive(Deserialize, serde::Serialize, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Arm9Mode {
    RecompressedBaseline,
    PauseTitle,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub file: String,
    pub expected_sha256: String,
    pub input: String,
    pub input_sha256: String,
    #[serde(default)]
    pub placement: Placement,
}
#[derive(Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Original,
    FfTail,
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for &b in bytes {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xa001 } else { 0 };
        }
    }
    crc
}
pub struct Write {
    pub offset: usize,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub label: String,
}

pub fn apply(source: &[u8], writes: &mut [Write]) -> Result<Vec<u8>> {
    writes.sort_by_key(|w| w.offset);
    let mut end = 0;
    for w in writes.iter() {
        ensure!(
            w.before.len() == w.after.len() && !w.after.is_empty(),
            "write length mismatch/empty write"
        );
        ensure!(w.offset >= end, "overlapping writers");
        ensure!(
            slice(source, w.offset, w.before.len())? == w.before,
            "expected bytes mismatch: {}",
            w.label
        );
        end = w
            .offset
            .checked_add(w.after.len())
            .ok_or_else(|| anyhow::anyhow!("write overflow"))?;
    }
    let mut output = source.to_vec();
    for w in writes.iter() {
        output[w.offset..w.offset + w.after.len()].copy_from_slice(&w.after);
    }
    for (s, e) in delta_ranges(source, &output) {
        ensure!(
            writes
                .iter()
                .any(|w| s >= w.offset && e <= w.offset + w.after.len())
                || (s..e).all(|p| writes
                    .iter()
                    .any(|w| p >= w.offset && p < w.offset + w.after.len())),
            "unexplained final difference"
        );
    }
    Ok(output)
}

pub fn build(source: &[u8], plan: &Plan, root: &Path) -> Result<(Vec<u8>, Value)> {
    ensure!(
        sha(source) == plan.source_sha256,
        "plan/source identity mismatch"
    );
    let rom = Rom::parse(source)?;
    let mut writes = Vec::new();
    let mut arm9_report = None;
    let mut arm9_input = None;
    if let Some(item) = &plan.arm9 {
        let stored = slice(source, 0x4000, 0xa5888)?;
        ensure!(
            sha(stored) == item.expected_sha256,
            "ARM9 source hash mismatch"
        );
        let data = fs::read(root.join(&item.input))?;
        ensure!(
            sha(&data) == item.input_sha256,
            "ARM9 replacement hash mismatch"
        );
        arm9_report = Some(crate::arm9::validate_replacement(source, &data, item.mode)?);
        // The complete secure area is checked unchanged, and excluded from the writer.
        writes.push(Write {
            offset: 0x8000,
            before: stored[0x4000..].to_vec(),
            after: data[0x4000..].to_vec(),
            label: "ARM9 fixed-size image".into(),
        });
        arm9_input = Some(data);
    }
    let mut selected = BTreeSet::new();
    let mut modified = BTreeSet::new();
    let source_used = u32le(source, 0x80)?;
    let mut tail = source_used;
    if plan
        .replacements
        .iter()
        .any(|r| r.placement == Placement::FfTail)
    {
        ensure!(
            source[0x12] == 0
                && u32le(source, 0x1000)? == 0
                && u32le(source, 0x160)? == 0
                && u32le(source, 0x164)? == 0,
            "unverified signed, debug or enhanced ROM tail"
        );
        let capacity = 0x20000usize
            .checked_shl(u32::from(source[0x14]))
            .ok_or_else(|| anyhow::anyhow!("invalid device capacity"))?;
        ensure!(
            capacity == source.len() && source_used >= 0x4000 && source_used < source.len(),
            "unverified physical/used ROM extent"
        );
        ensure!(
            rom.files.iter().map(|f| f.end).max() == Some(source_used),
            "used-ROM end differs from FAT population"
        );
        for (off, lenoff) in [
            (0x20, 0x2c),
            (0x30, 0x3c),
            (0x40, 0x44),
            (0x48, 0x4c),
            (0x50, 0x54),
            (0x58, 0x5c),
        ] {
            ensure!(
                u32le(source, off)?
                    .checked_add(u32le(source, lenoff)?)
                    .is_some_and(|e| e <= source_used),
                "protected region beyond used ROM"
            );
        }
        ensure!(
            u32le(source, 0x68)? + banner(source)?.len() <= source_used,
            "banner beyond used ROM"
        );
        ensure!(
            source[source_used..].iter().all(|v| *v == 255),
            "ROM tail contains non-FF data"
        );
        ensure!(
            u16le(source, 0x15e)? == usize::from(crc16(&source[..0x15e])),
            "source header CRC mismatch"
        );
    }
    for item in &plan.replacements {
        let entry = rom.file(&item.file)?;
        ensure!(selected.insert(entry.id), "duplicate replacement");
        ensure!(
            sha(rom.data(entry)) == item.expected_sha256,
            "file source hash mismatch: {}",
            item.file
        );
        let data = fs::read(root.join(&item.input))?;
        ensure!(sha(&data) == item.input_sha256, "replacement hash mismatch");
        ensure!(
            !data.is_empty()
                && (item.placement == Placement::FfTail || data.len() <= entry.end - entry.start),
            "replacement exceeds original file capacity: {}",
            item.file
        );
        // File IDs and protected structures remain fixed, including explicit tail allocation.
        for other in &rom.files {
            if other.id != entry.id {
                ensure!(
                    entry.end <= other.start || entry.start >= other.end,
                    "replacement aliases another file extent"
                );
            }
        }
        for (off, lenoff) in [
            (0x20, 0x2c),
            (0x30, 0x3c),
            (0x40, 0x44),
            (0x48, 0x4c),
            (0x50, 0x54),
            (0x58, 0x5c),
        ] {
            let s = u32le(source, off)?;
            let n = u32le(source, lenoff)?;
            ensure!(
                entry.end <= s || entry.start >= s + n,
                "replacement overlaps protected ROM structure"
            );
        }
        let bs = u32le(source, 0x68)?;
        let bn = banner(source)?.len();
        ensure!(
            entry.end <= bs || entry.start >= bs + bn,
            "replacement overlaps banner"
        );
        ensure!(
            entry.start >= 0x4000 && !rom.overlays.iter().any(|o| o.file_id == entry.id),
            "code/secure-area replacement requires a separate validated builder"
        );
        let decoded = unpack(&data)?;
        if item.file.ends_with(".fnt") {
            unpack_halfword(&data)?;
        }
        if item.file.ends_with(".narc") {
            Narc::parse(&decoded)?;
        }
        let target = if item.placement == Placement::FfTail {
            tail = tail
                .checked_add(511)
                .ok_or_else(|| anyhow::anyhow!("tail alignment overflow"))?
                & !511;
            let start = tail;
            tail = tail
                .checked_add(data.len())
                .ok_or_else(|| anyhow::anyhow!("tail allocation overflow"))?;
            ensure!(
                tail <= source.len(),
                "replacement exceeds physical ROM tail"
            );
            start
        } else {
            entry.start
        };
        writes.push(Write {
            offset: target,
            before: slice(source, target, data.len())?.to_vec(),
            after: data.clone(),
            label: item.file.clone(),
        });
        if target != entry.start || data.len() != entry.end - entry.start {
            let offset = rom.fat + entry.id * 8;
            let mut after = vec![0; 8];
            put32(&mut after, 0, target)?;
            put32(&mut after, 4, target + data.len())?;
            writes.push(Write {
                offset,
                before: source[offset..offset + 8].to_vec(),
                after,
                label: format!("{} FAT extent", item.file),
            });
        }
        if item.file.ends_with(".fnt") || item.file.ends_with(".mtx") {
            modified.insert(item.file[..item.file.len() - 4].to_string());
        }
    }
    if tail != source_used {
        let mut header = source[..0x160].to_vec();
        put32(&mut header, 0x80, tail)?;
        let crc = crc16(&header[..0x15e]);
        header[0x15e..0x160].copy_from_slice(&crc.to_le_bytes());
        for (offset, size, label) in [(0x80, 4, "used ROM size"), (0x15e, 2, "header CRC16")] {
            writes.push(Write {
                offset,
                before: source[offset..offset + size].to_vec(),
                after: header[offset..offset + size].to_vec(),
                label: label.into(),
            });
        }
    }
    if let Some(titles) = &plan.banner {
        writes.push(banner_write(source, titles)?);
    }
    // Secure-area spans: disjoint from the ARM9 replacement writer at 0x8000.
    if plan.anti_piracy_bypass {
        writes.extend(crate::anti_piracy::writes(source)?);
    }
    let result = apply(source, &mut writes)?;
    let rebuilt = Rom::parse(&result)?;
    if plan.banner.is_some() {
        let offset = u32le(&result, 0x68)?;
        let banner = slice(&result, offset, BANNER_V1_SIZE)?;
        ensure!(
            u16le(banner, 2)? == usize::from(crc16(&banner[0x20..])),
            "final banner CRC mismatch"
        );
    }
    if let Some(item) = &plan.arm9 {
        let final_arm9 = if plan.anti_piracy_bypass {
            crate::anti_piracy::without_bypass(source, &result)?
        } else {
            slice(&result, 0x4000, 0xa5888)?.to_vec()
        };
        ensure!(
            sha(&final_arm9) == item.input_sha256,
            "final ARM9 stored hash mismatch"
        );
        let final_report = crate::arm9::validate_replacement(source, &final_arm9, item.mode)?;
        ensure!(
            Some(&final_report) == arm9_report.as_ref(),
            "final ARM9 validation changed"
        );
    }
    let anti_piracy_report = plan
        .anti_piracy_bypass
        .then(|| {
            let expected = match &arm9_input {
                Some(data) => data.as_slice(),
                None => slice(source, 0x4000, 0xa5888)?,
            };
            crate::anti_piracy::verify(source, &result, expected)
        })
        .transpose()?;
    crate::pause_title::verify_pair(
        &rom,
        &rebuilt,
        plan.arm9
            .as_ref()
            .is_some_and(|a| a.mode == Arm9Mode::PauseTitle),
    )?;
    if tail != source_used {
        ensure!(
            u32le(&result, 0x80)? == tail
                && u16le(&result, 0x15e)? == usize::from(crc16(&result[..0x15e])),
            "final header verification failed"
        );
    }
    for stem in modified {
        let f = unpack(rebuilt.data(rebuilt.file(&format!("{stem}.fnt"))?))?;
        let t = unpack(rebuilt.data(rebuilt.file(&format!("{stem}.mtx"))?))?;
        text_pair(&f, &t)?;
    }
    for item in &plan.replacements {
        let e = rebuilt.file(&item.file)?;
        ensure!(
            sha(rebuilt.data(e)) == item.input_sha256,
            "final file verification failed"
        );
    }
    let changes = delta_ranges(source, &result);
    let write_records=writes.iter().map(|w|json!({"label":w.label,"offset":w.offset,"size":w.after.len(),"before_sha256":sha(&w.before),"after_sha256":sha(&w.after)})).collect::<Vec<_>>();
    let report = json!({"source_sha256":sha(source),"output_sha256":sha(&result),"size_bytes":result.len(),"profile":if changes.is_empty(){"unchanged_baseline"}else{"development"},"writes":write_records,"changed_ranges":changes,"changed_bytes":changes.iter().map(|(s,e)|e-s).sum::<usize>(),"arm9":arm9_report,"anti_piracy_bypass":anti_piracy_report,"banner":plan.banner.as_ref().map(|b| json!({"title":b.title,"scope":b.scope})),"verification":"source identity, expected bytes, disjoint writers, capacity, final file hashes, complete byte diff, optional ARM9 fixed-size, expected decoded writes, anti-piracy bypass spans and paired title graphics","runtime_verified":false,"release_eligible":false});
    Ok((result, report))
}

#[cfg(test)]
mod tests;
