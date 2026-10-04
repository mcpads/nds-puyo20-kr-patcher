//! Inventory complete FNT copies and setter-compatible object candidates.
//! Pointer values and object-shaped bytes do not establish allocation liveness.
use crate::format::*;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub fn inspect(rom: &Rom, path: &str, ram: &[u8]) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB main RAM");
    ensure!(
        u32le(rom.bytes, 0x28)? == 0x02000000,
        "unexpected ARM9 load address"
    );
    let font = unpack_halfword(rom.data(rom.file(&format!("{path}.fnt"))?))?;
    let text = unpack(rom.data(rom.file(&format!("{path}.mtx"))?))?;
    let info = text_pair(&font, &text)?;
    let (arm9, _) = crate::arm9::decode(slice(
        rom.bytes,
        u32le(rom.bytes, 0x20)?,
        u32le(rom.bytes, 0x2c)?,
    )?)?;
    let start = 0xda104;
    let code = slice(&arm9, start, 0x48)?;
    ensure!(
        sha(code) == "35b653a09f3d9187018a0425cc334efdbd51eae170fad7b4b3c21c11f4205967",
        "FNT object setter changed"
    );
    ensure!(
        slice(ram, start, code.len())? == code,
        "RAM FNT setter differs from ROM"
    );
    let mut instructions = Vec::new();
    for (i, word) in code.chunks_exact(4).enumerate() {
        let instruction = arm946e_s::decode_arm_bytes(word)?;
        ensure!(
            arm946e_s::encode_arm_bytes(&instruction)? == word,
            "FNT setter ARM round trip failed"
        );
        instructions.push(json!({"address":0x02000000+start+i*4,"bytes":hex::encode(word),"instruction":format!("{instruction:?}")}));
    }
    let references = |address: usize| -> Vec<usize> {
        ram.chunks_exact(4)
            .enumerate()
            .filter_map(|(i, b)| {
                (u32::from_le_bytes(b.try_into().unwrap()) as usize == address)
                    .then_some(0x02000000 + i * 4)
            })
            .collect()
    };
    let lookup_code = slice(&arm9, 0xda09c, 16)?;
    ensure!(
        sha(lookup_code) == "8ea5090a17689ea6f49766c61a5c31b3a773b9d7f07338d961618165a4977434"
            && slice(ram, 0xda09c, 16)? == lookup_code,
        "glyph record lookup changed"
    );
    let mut lookup_instructions = Vec::new();
    for (i, word) in lookup_code.chunks_exact(4).enumerate() {
        let instruction = arm946e_s::decode_arm_bytes(word)?;
        ensure!(
            arm946e_s::encode_arm_bytes(&instruction)? == word,
            "glyph lookup round trip failed"
        );
        lookup_instructions.push(json!({"address":0x020da09c+i*4,"bytes":hex::encode(word),"instruction":format!("{instruction:?}")}));
    }
    let mut copies = Vec::new();
    for (p, bytes) in ram.windows(font.len()).enumerate() {
        if bytes != font {
            continue;
        }
        let address = 0x02000000 + p;
        let direct = references(address);
        let mut objects = Vec::new();
        for &reference in &direct {
            let field = reference - 0x02000000;
            if field < 16 || field + 16 > ram.len() {
                continue;
            }
            let object = field - 16;
            if u32le(ram, object + 20)? != address + 16
                || u32le(ram, object + 24)? != address + 48
                || u32le(ram, object + 28)? != info.stride
            {
                continue;
            }
            objects.push(json!({"address":0x02000000+object,"header_word":u32le(ram,object)?,"raw_hex":hex::encode(&ram[object..object+32]),"font":address,"metrics":address+16,"glyphs":address+48,"stride":info.stride,"aligned_incoming_pointer_values":references(0x02000000+object)}));
        }
        copies.push(json!({"font_address":address,"aligned_incoming_pointer_values":direct,"setter_compatible_object_candidates":objects}));
    }
    ensure!(!copies.is_empty(), "no complete font copies in RAM");
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"ram_sha256":sha(ram),"path":path,"font_sha256":sha(&font),"font_size":font.len(),"stride":info.stride,"copies":copies,"setter":{"address":0x020da104,"sha256":sha(code),"instructions":instructions},"glyph_lookup":{"address":0x020da09c,"sha256":sha(lookup_code),"instructions":lookup_instructions},"claim":"all complete font copies and aligned pointer-value references; object fields agree with verified ARM setter; no heap liveness, render selection or active consumer claim"}),
    )
}
