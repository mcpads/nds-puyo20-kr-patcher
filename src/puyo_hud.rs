//! Fixed-source HUD candidates. Preview layout is a hypothesis, not an edit contract.
use crate::{assets::json_file, format::*, graphics::write_png, titles};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub mod conditions;
pub mod counter;
pub mod labels;
pub mod panels;
pub mod reach;
mod renderer;
pub mod results;
pub mod start;

pub fn inspect(rom: &Rom, ram: &[u8], out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    ensure!(ram.len() == 0x400000, "expected 4 MiB frozen RAM");
    let path = "puyo/puyo2P/puyo2P.narc";
    let source = rom.data(rom.file(path)?);
    ensure!(
        sha(source) == "b338518e1710dee5f6546deabeb3aa367bdaff2935146c4807e8b9d57c54c510",
        "puyo HUD source changed"
    );
    let n = Narc::parse(source)?;
    ensure!(n.members.len() == 832, "HUD member population changed");
    let references = inspect_references(rom, ram, &n)?;
    let renderer = renderer::verify(rom, Some(ram))?;
    let mut entries = Vec::new();
    let mut previews = Vec::new();
    for member in [732, 736, 738, 804] {
        let bytes = unpack_halfword(n.members[member])?;
        let palette = unpack(n.members[member + 1])?;
        ensure!(
            palette.len() == 32 && bytes.len() % 64 == 0,
            "candidate extent changed"
        );
        let pixels = bytes
            .iter()
            .flat_map(|b| [b & 15, b >> 4])
            .collect::<Vec<_>>();
        ensure!(
            pixels
                .chunks_exact(2)
                .map(|p| p[0] | p[1] << 4)
                .collect::<Vec<_>>()
                == bytes,
            "I4 round trip failed"
        );
        let addresses = ram
            .windows(bytes.len())
            .enumerate()
            .filter_map(|(p, b)| (b == bytes).then_some(0x02000000 + p))
            .collect::<Vec<_>>();
        let mut views = Vec::new();
        for width in [64, 128, 256] {
            ensure!(pixels.len() % width == 0, "candidate raster remainder");
            let height = pixels.len() / width;
            previews.push((
                format!("{member}-linear-{width}"),
                width,
                height,
                titles::rgba(&pixels, &palette)?,
            ));
            views.push(json!({"width":width,"height":height,"storage":"linear I4 hypothesis"}));
            if height % 8 == 0 {
                let tiled = titles::untile(&bytes, width, height, 4)?;
                ensure!(
                    titles::tile(&tiled, width, height, 4)? == bytes,
                    "tile round trip failed"
                );
                previews.push((
                    format!("{member}-tiled-{width}"),
                    width,
                    height,
                    titles::rgba(&tiled, &palette)?,
                ));
                views.push(
                    json!({"width":width,"height":height,"storage":"8x8 tiled I4 hypothesis"}),
                );
            }
        }
        entries.push(json!({"member":member,"palette_member":member+1,"decoded_size":bytes.len(),"decoded_sha256":sha(&bytes),"palette_sha256":sha(&palette),"whole_payload_ram_addresses":addresses,"views":views}));
    }
    fs::create_dir_all(out)?;
    for (name, w, h, pixels) in previews {
        write_png(&out.join(format!("{name}.png")), w, h, &pixels)?;
    }
    let report = json!({"source_sha256":sha(rom.bytes),"archive":path,"archive_sha256":sha(source),"ram_sha256":sha(ram),"references":references,"renderer":renderer,"entries":entries,"claim":"original code and descriptors equal frozen RAM; descriptor format reaches texture register; four 736 label crops statically resolved; other previews remain hypotheses; active consumption and VRAM bytes require separate evidence; no product changes"});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

fn inspect_references(rom: &Rom, ram: &[u8], archive: &Narc<'_>) -> Result<Value> {
    let overlay = rom
        .overlays
        .iter()
        .find(|o| o.cpu == "arm9" && o.id == 0)
        .ok_or_else(|| anyhow::anyhow!("missing HUD overlay"))?;
    let (bytes, _) = crate::arm9::decode(rom.data(&rom.files[overlay.file_id]))?;
    ensure!(
        overlay.ram == 0x02124e60 && bytes.len() == overlay.size,
        "HUD overlay layout changed"
    );
    let original = |address, len| slice(&bytes, address - overlay.ram, len);
    let live = |address, len| slice(ram, address - 0x02000000, len);
    let mut code = Vec::new();
    // End before each literal pool: data must not be decoded as ARM instructions.
    for (start, end, expected) in [
        (
            0x02125780,
            0x02125868,
            "4ef993c9ebdf00802625fb3c5572db16a86486dcbcf8aee3365bdd8000ace236",
        ),
        (
            0x0212dad8,
            0x0212db80,
            "b8231ab72856696b96cfb8f46947e718d4d1ff1627ed96e498fff35318abe7e9",
        ),
        (
            0x021469ac,
            0x021469f8,
            "35eb24acc20f5bc788b6369312001d60104a2c13da9001a3c432dd5bddfaf8bc",
        ),
    ] {
        let window = original(start, end - start)?;
        ensure!(
            sha(window) == expected && window == live(start, end - start)?,
            "HUD source/runtime code differs at {start:#x}"
        );
        let mut instructions = Vec::new();
        for (i, raw) in window.chunks_exact(4).enumerate() {
            let instruction = arm946e_s::decode_arm_bytes(raw)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&instruction)? == raw,
                "ARM round trip failed"
            );
            instructions.push(json!({"address":start+i*4,"bytes":hex::encode(raw),"instruction":format!("{instruction:?}")}));
        }
        code.push(json!({"address":start,"sha256":expected,"instructions":instructions}));
    }
    let mut tables = Vec::new();
    for (pointer, address, count, expected) in [
        (
            0x0212db80,
            0x0214bdc8,
            20,
            "3b25d839e183b5403de085363f438563c80e20e57b2015bf8ee0614cf1858676",
        ),
        (
            0x021469f8,
            0x0214c944,
            2,
            "492b728cc0e7cca0b2ed223213f1d911d0a3b0963ec1b53a9891d6d45a8b9cc1",
        ),
        (
            0x021469fc,
            0x0214caa4,
            4,
            "0ea1a996e9acf84da0e745d448d2a82d4ed099b225a9c60020bd96a58272019b",
        ),
    ] {
        ensure!(
            u32le(original(pointer, 4)?, 0)? == address
                && original(pointer, 4)? == live(pointer, 4)?,
            "HUD descriptor pointer changed"
        );
        let table = original(address, count * 16)?;
        ensure!(
            sha(table) == expected && table == live(address, count * 16)?,
            "HUD descriptor table changed"
        );
        let mut entries = Vec::new();
        for (i, row) in table.chunks_exact(16).enumerate() {
            let member = u16le(row, 8)?;
            let palette = u16le(row, 10)?;
            let width = u16le(row, 4)?;
            let height = u16le(row, 6)?;
            ensure!(
                member < archive.members.len() && palette < archive.members.len(),
                "HUD descriptor member outside archive"
            );
            let candidate = [736, 738, 804].contains(&member);
            if candidate {
                ensure!(
                    u32le(row, 0)? == 3
                        && width == 128
                        && palette == member + 1
                        && unpack_halfword(archive.members[member])?.len() * 2 == width * height,
                    "HUD candidate dimensions changed"
                );
            }
            entries.push(json!({"address":address+i*16,"format_value":u32le(row,0)?,"width":width,"height":height,"member":member,"palette_member":palette,"flags_bytes":hex::encode(&row[12..16]),"candidate_i4_extent_matches":candidate}));
        }
        tables.push(json!({"pointer_address":pointer,"address":address,"sha256":expected,"entries":entries}));
    }
    Ok(
        json!({"overlay_id":0,"overlay_file_id":overlay.file_id,"overlay_ram_address":overlay.ram,"decoded_overlay_sha256":sha(&bytes),"source_runtime_windows_equal":true,"tables":tables,"code":code}),
    )
}
