//! Target ARM9 backward compression, autoload analysis and unchanged-code control.
use crate::{assets::json_file, format::*};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

const BASE: usize = 0x02000000;
const PARAMS: usize = 0xb88;
const MAX_IMAGE: usize = 4 * 1024 * 1024;

/// Byte validation of recorded boot stops. Live launch binding is separate evidence.
pub fn check_boot(rom: &[u8], ram: &[u8], observation: &[u8]) -> Result<Value> {
    Rom::parse(rom)?;
    ensure!(
        ram.len() == MAX_IMAGE
            && u32le(rom, 0x20)? == 0x4000
            && u32le(rom, 0x28)? == BASE
            && u32le(rom, 0x2c)? == 0xa5888,
        "ARM9 boot input layout mismatch"
    );
    let (decoded, _) = decode(slice(rom, 0x4000, 0xa5888)?)?;
    ensure!(
        slice(ram, 0, decoded.len())? == decoded,
        "ARM9 live decoded image differs from artifact"
    );
    let o: Value = serde_json::from_slice(observation)?;
    for (stage, pc) in [("decoder", BASE + 0x9fc), ("autoload", BASE + 0x8a8)] {
        ensure!(
            o[stage]["state"]["cpu"] == "arm9" && o[stage]["state"]["state"]["cpu.pc"] == pc,
            "ARM9 boot stop PC mismatch"
        );
        let events = o[stage]["events"]["events"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing boot stop events"))?;
        ensure!(
            events.iter().any(|e| e["cpu"] == "arm9"
                && e["type"] == "breakpoint_hit"
                && e["pc"] == pc
                && e["address"] == pc),
            "ARM9 boot stop event missing"
        );
    }
    let (regions, _) = sections(&decoded)?;
    let reads = o["autoload"]["reads"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing autoload reads"))?;
    ensure!(
        reads.len() == regions.len() - 1,
        "autoload read population mismatch"
    );
    let mut results = Vec::new();
    for (section, read) in regions[1..].iter().zip(reads) {
        ensure!(
            read["memory_type"] == "arm9"
                && read["cpu"] == "arm9"
                && read["address"] == section.address
                && read["length"] == section.bytes.len() + section.bss,
            "autoload read address/length mismatch"
        );
        let bytes = hex::decode(
            read["hex"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("autoload read lacks hex"))?,
        )?;
        ensure!(
            bytes.len() == section.bytes.len() + section.bss
                && bytes[..section.bytes.len()] == section.bytes
                && bytes[section.bytes.len()..].iter().all(|&b| b == 0),
            "autoload data or zero BSS differs"
        );
        results.push(json!({"address":section.address,"data_size":section.bytes.len(),"bss_size":section.bss,"read_sha256":sha(&bytes),"data_exact":true,"bss_zero":true}));
    }
    Ok(
        json!({"rom_sha256":sha(rom),"ram_sha256":sha(ram),"observation_sha256":sha(observation),"decoded_size":decoded.len(),"decoded_sha256":sha(&decoded),"decoded_bytes_exact":true,"autoload":results,"claim":"artifact byte comparison at recorded decoder/autoload stops; live launch binding and interaction are separate evidence"}),
    )
}

pub fn validate_replacement(
    rom: &[u8],
    replacement: &[u8],
    mode: crate::build::Arm9Mode,
) -> Result<Value> {
    ensure!(
        rom.len() == 67108864
            && sha(rom) == "6b8227780eea751aa22b4201aea63c36d8844a994791109708d9fe2df399944d",
        "ARM9 replacement requires the registered JP source"
    );
    let original = slice(rom, 0x4000, 0xa5888)?;
    ensure!(
        replacement.len() == original.len(),
        "ARM9 fixed storage size changed"
    );
    ensure!(
        replacement[..0x4000] == original[..0x4000],
        "ARM9 secure area changed"
    );
    let (source_decoded, _) = decode(original)?;
    let (decoded, compression) = decode(replacement)?;
    let (expected, edits) = match mode {
        crate::build::Arm9Mode::RecompressedBaseline => (source_decoded.clone(), json!([])),
        crate::build::Arm9Mode::PauseTitle => crate::pause_title::rewrite_code(&source_decoded)?,
    };
    ensure!(
        decoded == expected,
        "ARM9 contains undeclared decoded code/data changes"
    );
    let (_, layout) = sections(&decoded)?;
    Ok(
        json!({"mode":mode,"source_stored_sha256":sha(original),"stored_sha256":sha(replacement),"decoded_sha256":sha(&decoded),"decoded_changes":decoded.iter().zip(&source_decoded).filter(|(a,b)|a!=b).count(),"decoded_writes":edits,"compression":compression,"layout":layout,"secure_area_preserved":true,"storage_extent_preserved":true,"module_params_preserved":true,"runtime_verified":false}),
    )
}

pub(crate) fn prepare_image(rom: &[u8], mode: crate::build::Arm9Mode) -> Result<(Vec<u8>, Value)> {
    let original = slice(rom, 0x4000, 0xa5888)?;
    let (mut decoded, _) = decode(original)?;
    if mode == crate::build::Arm9Mode::PauseTitle {
        decoded = crate::pause_title::rewrite_code(&decoded)?.0;
    }
    let replacement = encode(&decoded, 0x4004, Some(original.len()))?;
    let report = validate_replacement(rom, &replacement, mode)?;
    Ok((replacement, report))
}
pub fn prepare_control(rom: &[u8], out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output already exists");
    let original = slice(rom, 0x4000, 0xa5888)?;
    let (replacement, report) = prepare_image(rom, crate::build::Arm9Mode::RecompressedBaseline)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("arm9.bin"), &replacement)?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom),"replacements":[],"arm9":{"mode":"recompressed_baseline","expected_sha256":sha(original),"input":"arm9.bin","input_sha256":sha(&replacement)}}),
    )?;
    json_file(&out.join("arm9.json"), &report)?;
    Ok(report)
}

/// Simulate the target's in-place backwards reader, including unread input.
pub(crate) fn decode(stored: &[u8]) -> Result<(Vec<u8>, Value)> {
    ensure!(stored.len() >= 8, "truncated ARM9 footer");
    let footer = u32le(stored, stored.len() - 8)?;
    let header = footer >> 24;
    let compressed = footer & 0xffffff;
    let extra = u32le(stored, stored.len() - 4)?;
    ensure!(
        (8..=255).contains(&header)
            && compressed > header
            && compressed <= stored.len()
            && extra > 0
            && stored
                .len()
                .checked_add(extra)
                .is_some_and(|n| n <= MAX_IMAGE),
        "invalid or unsupported ARM9 compression footer"
    );
    ensure!(
        stored[stored.len() - header..stored.len() - 8]
            .iter()
            .all(|&b| b == 255),
        "ARM9 footer padding changed"
    );
    let prefix = stored.len() - compressed;
    let mut read = stored.len() - header;
    let mut write = stored.len() + extra;
    let mut memory = stored.to_vec();
    memory.resize(write, 0);
    let mut literals = 0;
    let mut copies = 0;
    let mut min_gap = write - read;
    let mut lengths = [0usize; 19];
    let mut min_distance = usize::MAX;
    let mut max_distance = 0;
    while read > prefix {
        read -= 1;
        let flags = memory[read];
        for bit in (0..8).rev() {
            if read == prefix {
                break;
            }
            if flags & (1 << bit) == 0 {
                read -= 1;
                let byte = memory[read];
                ensure!(write > prefix, "ARM9 literal output overrun");
                write -= 1;
                ensure!(write >= read, "ARM9 output overwrites unread input");
                memory[write] = byte;
                literals += 1;
            } else {
                ensure!(read - prefix >= 2, "truncated ARM9 match");
                let hi = usize::from(memory[read - 1]);
                let lo = usize::from(memory[read - 2]);
                read -= 2;
                let len = (hi >> 4) + 3;
                let distance = ((hi & 15) << 8 | lo) + 3;
                ensure!(
                    len <= write - prefix && distance <= memory.len() - write,
                    "invalid ARM9 match reference/output extent"
                );
                for _ in 0..len {
                    write -= 1;
                    ensure!(write >= read, "ARM9 match overwrites unread input");
                    memory[write] = memory[write + distance];
                }
                copies += 1;
                lengths[len] += 1;
                min_distance = min_distance.min(distance);
                max_distance = max_distance.max(distance);
            }
            min_gap = min_gap.min(write - read);
        }
    }
    ensure!(write == prefix, "ARM9 output does not end at raw prefix");
    let report = json!({"stored_size":stored.len(),"decoded_size":memory.len(),"prefix_size":prefix,"compressed_size":compressed,"footer_size":header,"extra_size":extra,"literals":literals,"matches":copies,"match_length_counts":lengths,"minimum_distance":(copies>0).then_some(min_distance),"maximum_distance":max_distance,"minimum_unread_gap":min_gap,"terminal_read":read,"terminal_write":write});
    Ok((memory, report))
}

/// Greedy backward matches, retaining the safest/smallest complete flag group.
/// A prefix remains raw; no fallback to a different compression representation.
pub(crate) fn encode(
    data: &[u8],
    minimum_prefix: usize,
    stored_size: Option<usize>,
) -> Result<Vec<u8>> {
    ensure!(
        data.len() <= MAX_IMAGE && minimum_prefix < data.len(),
        "ARM9 encoder input extent"
    );
    let reversed: Vec<u8> = data[minimum_prefix..].iter().rev().copied().collect();
    let n = reversed.len();
    let mut previous = vec![None; n];
    let mut heads: HashMap<[u8; 3], usize> = HashMap::new();
    let mut stream = Vec::new();
    let mut p = 0;
    let mut maximum_gap: isize = 0;
    let mut best: Option<(usize, usize, usize)> = None;
    while p < n {
        let flag = stream.len();
        stream.push(0);
        for bit in (0..8).rev() {
            if p == n {
                break;
            }
            let mut length = 1;
            let mut distance = 0;
            if p + 3 <= n {
                let key: [u8; 3] = reversed[p..p + 3].try_into()?;
                let mut at = heads.get(&key).copied();
                while let Some(q) = at {
                    let d = p - q;
                    if d > 4098 {
                        break;
                    }
                    if d >= 3 {
                        let mut len = 3;
                        while len < 18 && p + len < n && reversed[p + len] == reversed[q + len] {
                            len += 1;
                        }
                        if len > length {
                            length = len;
                            distance = d;
                        }
                        if length == 18 {
                            break;
                        }
                    }
                    at = previous[q];
                }
            }
            if length >= 3 {
                stream[flag] |= 1 << bit;
                let d = distance - 3;
                stream.extend([(((length - 3) << 4) | (d >> 8)) as u8, d as u8]);
            } else {
                stream.push(reversed[p]);
            }
            for q in p..p + length {
                if q + 3 <= n {
                    let key: [u8; 3] = reversed[q..q + 3].try_into()?;
                    previous[q] = heads.insert(key, q);
                }
            }
            p += length;
            maximum_gap = maximum_gap.max(p as isize - stream.len() as isize);
        }
        // In-place output must never cross the remaining compressed bytes.
        // Footer alignment is outside the stream and already consumed at entry.
        if p as isize - stream.len() as isize >= maximum_gap {
            let prefix = data.len() - p;
            let body = prefix + stream.len();
            let size = stored_size.unwrap_or_else(|| body.div_ceil(4) * 4 + 8);
            let footer_fits = size
                .checked_sub(body)
                .is_some_and(|h| (8..=255).contains(&h));
            if footer_fits
                && size < data.len()
                && (stored_size.is_some() || best.is_none_or(|(_, _, old)| size < old))
            {
                best = Some((p, stream.len(), size));
            }
        }
    }
    let (produced, used, total) =
        best.ok_or_else(|| anyhow::anyhow!("ARM9 has no safe compressed representation"))?;
    let prefix = data.len() - produced;
    let mut out = data[..prefix].to_vec();
    out.extend(stream[..used].iter().rev());
    let header = total - out.len();
    out.resize(total - 8, 255);
    out.extend(((header << 24 | (total - prefix)) as u32).to_le_bytes());
    out.extend(((data.len() - total) as u32).to_le_bytes());
    ensure!(
        decode(&out)?.0 == data,
        "ARM9 compression round trip failed"
    );
    Ok(out)
}

pub(crate) struct Section {
    pub(crate) address: usize,
    bss: usize,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn sections(data: &[u8]) -> Result<(Vec<Section>, Value)> {
    ensure!(
        slice(data, PARAMS + 28, 8)? == hex::decode("2106c0dedec00621")?,
        "ARM9 module magic changed"
    );
    let offset = |p| -> Result<usize> {
        u32le(data, p)?
            .checked_sub(BASE)
            .ok_or_else(|| anyhow::anyhow!("ARM9 pointer below load base"))
    };
    let table = offset(PARAMS)?;
    let end = offset(PARAMS + 4)?;
    let start = offset(PARAMS + 8)?;
    ensure!(
        start <= table && table <= end && end == data.len() && (end - table) % 12 == 0,
        "ARM9 autoload table extent changed"
    );
    let mut regions = vec![Section {
        address: BASE,
        bss: 0,
        bytes: slice(data, 0, start)?.to_vec(),
    }];
    let mut rows = vec![
        json!({"address":BASE,"offset":0,"size":start,"bss_size":0,"implicit":true,"sha256":sha(&data[..start])}),
    ];
    let mut cursor = start;
    for row in (table..end).step_by(12) {
        let address = u32le(data, row)?;
        let len = u32le(data, row + 4)?;
        let bss = u32le(data, row + 8)?;
        ensure!(
            address % 4 == 0 && len % 4 == 0 && bss % 4 == 0,
            "unaligned ARM9 autoload region"
        );
        let bytes = slice(data, cursor, len)?.to_vec();
        rows.push(json!({"address":address,"offset":cursor,"size":len,"bss_size":bss,"implicit":false,"sha256":sha(&bytes)}));
        regions.push(Section {
            address,
            bss,
            bytes,
        });
        cursor += len;
    }
    ensure!(cursor == table, "ARM9 autoload payload gap or overlap");
    let mut reconstructed = Vec::new();
    for section in &regions {
        reconstructed.extend(&section.bytes);
    }
    for section in &regions[1..] {
        for value in [section.address, section.bytes.len(), section.bss] {
            reconstructed.extend(u32::try_from(value)?.to_le_bytes());
        }
    }
    ensure!(reconstructed == data, "ARM9 section reassembly differs");
    Ok((
        regions,
        json!({"module_params_offset":PARAMS,"autoload_table_offset":table,"autoload_table_end":end,"static_bss_start":u32le(data,PARAMS+12)?,"static_bss_end":u32le(data,PARAMS+16)?,"compressed_end":u32le(data,PARAMS+20)?,"sdk_version":u32le(data,PARAMS+24)?,"regions":rows,"section_reassembly_exact":true}),
    ))
}

pub fn inspect(rom: &[u8], ram: Option<&[u8]>, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output already exists");
    ensure!(
        u32le(rom, 0x20)? == 0x4000
            && u32le(rom, 0x24)? == BASE + 0x800
            && u32le(rom, 0x28)? == BASE
            && u32le(rom, 0x2c)? == 0xa5888,
        "target ARM9 header mismatch"
    );
    let stored = slice(rom, 0x4000, 0xa5888)?;
    ensure!(
        sha(stored) == "b57e4fb11dcd472abc427e8d4cf551bfa94087bb7ea14d27acaa433dae0e0065",
        "target ARM9 stored identity mismatch"
    );
    let (decoded, compression) = decode(stored)?;
    ensure!(
        sha(&decoded) == "a0ccae8270e9dd0a11c64670a263d4367df35457a2603ed0893e03732004df85",
        "ARM9 independent decoded identity mismatch"
    );
    let (regions, layout) = sections(&decoded)?;
    ensure!(
        u32le(&decoded, PARAMS + 20)? == BASE + stored.len()
            && u32le(rom, 0x70)? == BASE + 0xa74
            && u32le(&decoded, 0xa70)? == BASE + PARAMS,
        "ARM9 module pointer/end mismatch"
    );
    let expected = [
        (
            0x02000000,
            1098560,
            0,
            "2c5899af92a7cc4fe9ae7aacd34e299053e67f3f68da271f2f84008cf202ca41",
        ),
        (
            0x01ff8000,
            30336,
            0,
            "178a4abbc6beb9a3b209b150b3119e2fa7e62c9d13f43b48e764bb46c4cf3d9b",
        ),
        (
            0x027e0000,
            96,
            32,
            "3b47de0e50c257c5cc49626b9c0faf9227f404d79d13e068e5c37adc53159fde",
        ),
    ];
    ensure!(
        regions.len() == expected.len(),
        "ARM9 section population changed"
    );
    for (section, (ea, elen, ebss, hash)) in regions.iter().zip(expected) {
        ensure!(
            section.address == ea
                && section.bss == ebss
                && section.bytes.len() == elen
                && sha(&section.bytes) == hash,
            "ARM9 independent section mismatch"
        );
    }
    let mut consumers = Vec::new();
    if let Some(ram) = ram {
        ensure!(
            ram.len() == MAX_IMAGE,
            "ARM9 RAM must be a 4 MiB main-memory dump"
        );
        ensure!(
            slice(ram, PARAMS, 36)? == slice(&decoded, PARAMS, 36)?,
            "ARM9 RAM module parameters differ"
        );
    }
    for (start, len, label) in [
        (0x950, 0xac, "backward decompressor"),
        (0x9fc, 0x74, "autoload loop"),
    ] {
        if let Some(ram) = ram {
            ensure!(
                slice(ram, start, len)? == slice(&decoded, start, len)?,
                "ARM9 RAM consumer differs from source"
            );
        }
        let mut instructions = Vec::new();
        for (i, word) in slice(&decoded, start, len)?.chunks_exact(4).enumerate() {
            let typed = arm946e_s::decode_arm_bytes(word)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&typed)? == word,
                "ARM9 consumer typed round trip failed"
            );
            instructions.push(json!({"address":BASE+start+i*4,"bytes":hex::encode(word),"instruction":format!("{typed:?}")}));
        }
        consumers.push(json!({"label":label,"sha256":sha(slice(&decoded,start,len)?),"instructions":instructions}));
    }
    let repacked = encode(&decoded, 0x4004, None)?;
    let (_, repacked_compression) = decode(&repacked)?;
    // Preserve the target's load length and module compressed-end pointer.
    // The actual reader uses an 8-bit footer length, so keep a larger raw prefix
    // when necessary and use only the verified FF padding representation.
    let fixed = encode(&decoded, 0x4004, Some(stored.len()))?;
    let (_, fixed_compression) = decode(&fixed)?;
    ensure!(
        fixed.len() == stored.len() && fixed[..0x4000] == stored[..0x4000],
        "fixed ARM9 changed secure-area bytes or load length"
    );
    fs::create_dir_all(out)?;
    fs::write(out.join("decoded.bin"), &decoded)?;
    fs::write(out.join("repacked-analysis.bin"), &repacked)?;
    fs::write(out.join("repacked-fixed-size.bin"), &fixed)?;
    for (i, section) in regions.iter().enumerate() {
        fs::write(out.join(format!("section-{i}.bin")), &section.bytes)?;
    }
    let report = json!({"source_sha256":sha(rom),"rom_offset":0x4000,"load_address":BASE,"entry_address":BASE+0x800,"stored_sha256":sha(stored),"decoded_sha256":sha(&decoded),"compression":compression,"layout":layout,"consumers":consumers,"ram_check":ram.map(|r|json!({"sha256":sha(r),"consumer_regions_exact":true,"module_params_exact":true,"claim":"residency identity, not execution tracing or new-stream consumption"})),"repacked":{"sha256":sha(&repacked),"compression":repacked_compression,"round_trip_exact":true,"source_stored_exact":repacked==stored,"module_compressed_end_updated":false,"rom_emitted":false},"fixed_size_repacked":{"sha256":sha(&fixed),"compression":fixed_compression,"round_trip_exact":true,"load_length_preserved":true,"secure_area_bytes_preserved":true,"module_params_preserved":true,"rom_emitted":false},"claim":"source decode and section reconstruction match independent baseline; in-place checked analysis recompression only, no product adoption or runtime proof"});
    json_file(&out.join("arm9.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
