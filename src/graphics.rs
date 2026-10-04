use crate::{assets::json_file, format::*};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub(crate) fn write_png(path: &Path, width: usize, height: usize, rgba: &[u8]) -> Result<()> {
    let mut encoder = png::Encoder::new(fs::File::create(path)?, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn color(colors: &[u8], index: usize) -> Result<[u8; 4]> {
    let c = u16le(colors, index * 2)?;
    Ok([
        ((c & 31) * 255 / 31) as u8,
        (((c >> 5) & 31) * 255 / 31) as u8,
        (((c >> 10) & 31) * 255 / 31) as u8,
        255,
    ])
}

/// Static candidate interpretation only; row layout and alpha require consumer proof.
pub fn inspect_menu(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let archive = rom.data(rom.file("menu/main_menu.narc")?);
    let narc = Narc::parse(archive)?;
    let table = rom.data(rom.file("menu/mainmenu_b_texlist.bin")?);
    let ilf = rom.data(rom.file("menu/mainmenu_t_ilf.bin")?);
    ensure!(ilf.len() % 4 == 0, "unaligned ILF list");
    let mut sprite_candidates = Vec::new();
    ensure!(table.len() % 12 == 0, "unexpected texture list size");
    fs::create_dir_all(out)?;
    for (i, row) in ilf.chunks_exact(4).enumerate() {
        let value = u32le(row, 0)?;
        let image_id = (value >> 8) & 0xfff;
        let palette_id = value >> 20;
        let data = unpack(
            narc.members
                .get(image_id)
                .ok_or_else(|| anyhow::anyhow!("ILF image candidate outside archive"))?,
        )?;
        let palette = unpack(
            narc.members
                .get(palette_id)
                .ok_or_else(|| anyhow::anyhow!("ILF palette candidate outside archive"))?,
        )?;
        ensure!(
            data.len() % 32 == 0 && matches!(palette.len(), 32 | 512),
            "ILF candidate geometry mismatch"
        );
        fs::write(out.join(format!("sprite-{i:02}-pixels.bin")), &data)?;
        fs::write(out.join(format!("sprite-{i:02}-palette.bin")), &palette)?;
        sprite_candidates.push(json!({"record":i,"logical_id_candidate":value&255,"image_member_candidate":image_id,"palette_member_candidate":palette_id,"pixels_size":data.len(),"palette_size":palette.len(),"pixels_sha256":sha(&data),"palette_sha256":sha(&palette),"status":"packed_8_12_12_identity_hypothesis_dimensions_unresolved"}));
    }
    let mut backgrounds = Vec::new();
    for base in (0..30).step_by(3) {
        let map = unpack(narc.members[base])?;
        let tiles = unpack(narc.members[base + 1])?;
        let palette = unpack(narc.members[base + 2])?;
        ensure!(
            map.len() == 32 * 24 * 2 && tiles.len() % 64 == 0 && palette.len() == 512,
            "unexpected background geometry"
        );
        let mut rgba = vec![0; 256 * 192 * 4];
        for cell in 0..32 * 24 {
            let attr = u16le(&map, cell * 2)?;
            ensure!(attr >> 12 == 0, "unresolved 8bpp palette attribute");
            let tile = slice(&tiles, (attr & 1023) * 64, 64)?;
            for y in 0..8 {
                for x in 0..8 {
                    let sx = if attr & 1024 != 0 { 7 - x } else { x };
                    let sy = if attr & 2048 != 0 { 7 - y } else { y };
                    let pixel = ((cell / 32 * 8 + y) * 256 + cell % 32 * 8 + x) * 4;
                    rgba[pixel..pixel + 4]
                        .copy_from_slice(&color(&palette, tile[sy * 8 + sx] as usize)?);
                }
            }
        }
        write_png(&out.join(format!("bg-{base:02}.png")), 256, 192, &rgba)?;
        backgrounds.push(json!({"map_member":base,"tiles_member":base+1,"palette_member":base+2,"tiles":tiles.len()/64,"map_sha256":sha(&map),"tiles_sha256":sha(&tiles),"palette_sha256":sha(&palette)}));
    }
    let entries = texture_entries(&narc, table, out)?;
    let report = json!({"source_sha256":sha(rom.bytes),"archive_sha256":sha(archive),"table_sha256":sha(table),"ilf_sha256":sha(ilf),"entries":entries,"backgrounds":backgrounds,"sprite_candidates":sprite_candidates,"claim":"static texture, 8bpp tilemap and packed ILF hypotheses; no edit or runtime format approval"});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

fn texture_entries(narc: &Narc, table: &[u8], out: &Path) -> Result<Vec<Value>> {
    ensure!(table.len() % 12 == 0, "unexpected texture list size");
    let mut entries = Vec::new();
    for (id, row) in table.chunks_exact(12).enumerate() {
        let member = u16le(row, 0)?;
        let palette = u16le(row, 2)?;
        let width = u16le(row, 8)?;
        let height = u16le(row, 10)?;
        if width == 0 || height == 0 {
            entries.push(json!({"id":id,"raw_hex":hex::encode(row),"status":"zero_dimension_record_unresolved"}));
            continue;
        }
        let pixels = unpack(
            narc.members
                .get(member)
                .ok_or_else(|| anyhow::anyhow!("texture member outside archive"))?,
        )?;
        let colors = unpack(
            narc.members
                .get(palette)
                .ok_or_else(|| anyhow::anyhow!("palette member outside archive"))?,
        )?;
        let format = u16le(row, 4)?;
        if !matches!(format, 1 | 3 | 4 | 6) {
            entries.push(json!({"id":id,"member":member,"palette_member":palette,"raw_hex":hex::encode(row),"width":width,"declared_height":height,"pixels_size":pixels.len(),"palette_size":colors.len(),"pixels_sha256":sha(&pixels),"palette_sha256":sha(&colors),"status":"unsupported_texture_format"}));
            fs::write(out.join(format!("{id:02}-pixels.bin")), &pixels)?;
            fs::write(out.join(format!("{id:02}-palette.bin")), &colors)?;
            continue;
        }
        ensure!(colors.len() % 2 == 0, "candidate geometry mismatch");
        let texels: Vec<u8> = if format == 3 {
            pixels.iter().flat_map(|v| [v & 15, v >> 4]).collect()
        } else {
            pixels.clone()
        };
        ensure!(texels.len() % width == 0, "unaligned texture rows");
        let stored_height = texels.len() / width;
        let mut rgba = Vec::new();
        let mut opaque = Vec::new();
        let mut invalid = 0;
        for v in &texels {
            let (i, a) = match format {
                1 => ((v & 31) as usize, (v >> 5) as usize * 255 / 7),
                6 => ((v & 7) as usize, (v >> 3) as usize * 255 / 31),
                3 | 4 => (*v as usize, if *v == 0 { 0 } else { 255 }),
                _ => unreachable!(),
            };
            let rgb = if i * 2 < colors.len() {
                let c = u16le(&colors, i * 2)?;
                [
                    (c & 31) * 255 / 31,
                    ((c >> 5) & 31) * 255 / 31,
                    ((c >> 10) & 31) * 255 / 31,
                ]
            } else {
                invalid += 1;
                [255, 0, 255]
            };
            rgba.extend([rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, a as u8]);
            opaque.extend([rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, 255]);
        }
        for (label, bytes) in [("alpha", &rgba), ("indices", &opaque)] {
            let file = fs::File::create(out.join(format!("{id:02}-{label}.png")))?;
            let mut encoder = png::Encoder::new(file, width as u32, stored_height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(bytes)?;
        }
        fs::write(out.join(format!("{id:02}-pixels.bin")), &pixels)?;
        fs::write(out.join(format!("{id:02}-palette.bin")), &colors)?;
        let tail = slice(
            &pixels,
            width * height / if format == 3 { 2 } else { 1 },
            pixels
                .len()
                .checked_sub(width * height / if format == 3 { 2 } else { 1 })
                .ok_or_else(|| anyhow::anyhow!("declared image exceeds payload"))?,
        )?;
        entries.push(json!({"id":id,"member":member,"palette_member":palette,"format":format,"unknown_field":u16le(row,6)?,"index_zero_transparency":if matches!(format, 3 | 4) {"preview_assumption"} else {"explicit_alpha"},"width":width,"declared_height":height,"stored_rows":stored_height,"trailing_bytes":tail.len(),"trailing_all_zero":tail.iter().all(|b|*b==0),"palette_colors":colors.len()/2,"out_of_palette_indices":invalid,"pixels_sha256":sha(&pixels),"palette_sha256":sha(&colors),"prefix_hex":hex::encode(&pixels[..pixels.len().min(32)])}));
    }
    Ok(entries)
}

pub fn inspect_textures(rom: &Rom, archive: &str, table: &str, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let archive_bytes = rom.data(rom.file(archive)?);
    let table_bytes = rom.data(rom.file(table)?);
    let narc = Narc::parse(archive_bytes)?;
    fs::create_dir_all(out)?;
    let entries = texture_entries(&narc, table_bytes, out)?;
    let report = json!({"source_sha256":sha(rom.bytes),"archive":archive,"archive_sha256":sha(archive_bytes),"table":table,"table_sha256":sha(table_bytes),"members":narc.members.len(),"entries":entries,"claim":"static A3I5/A5I3/I4/I8 interpretation; I4/I8 index-zero transparency is a preview assumption; inspect dimensions, palette, protected areas and runtime separately"});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}

/// Compare complete decoded texture payloads, including stored tail rows, against
/// a frozen ARM9 main-RAM dump. This does not prove which texture is on screen.
pub fn check_texture_ram(
    rom: &Rom,
    archive: &str,
    table: &str,
    ram: &[u8],
    allow_copies: bool,
) -> Result<Value> {
    ensure!(ram.len() == 0x400000, "expected 4 MiB main RAM");
    let source = rom.data(rom.file(archive)?);
    let narc = Narc::parse(source)?;
    let table_bytes = rom.data(rom.file(table)?);
    ensure!(table_bytes.len() % 12 == 0, "unexpected texture list size");
    let mut entries = Vec::new();
    let mut ignored = Vec::new();
    for (id, row) in table_bytes.chunks_exact(12).enumerate() {
        let width = u16le(row, 8)?;
        let height = u16le(row, 10)?;
        if width == 0 || height == 0 {
            ignored.push(json!({"id":id,"raw_hex":hex::encode(row),"reason":"zero-dimension record; not a texture residency claim"}));
            continue;
        }
        let member = u16le(row, 0)?;
        let format = u16le(row, 4)?;
        ensure!(matches!(format, 1 | 3 | 6), "unsupported texture format");
        let bytes = unpack_halfword(
            narc.members
                .get(member)
                .ok_or_else(|| anyhow::anyhow!("texture outside archive"))?,
        )?;
        ensure!(
            bytes.len() >= width * height / if format == 3 { 2 } else { 1 } && !bytes.is_empty(),
            "invalid texture extent"
        );
        let positions = ram
            .windows(bytes.len())
            .enumerate()
            .filter_map(|(p, b)| (b == bytes).then_some(p))
            .collect::<Vec<_>>();
        ensure!(
            !positions.is_empty() && (allow_copies || positions.len() == 1),
            "texture residency failed: ID {id}, found {} matches (allow_copies={allow_copies})",
            positions.len()
        );
        entries.push(json!({"id":id,"member":member,"format":format,"address":if positions.len() == 1 {Some(0x02000000+positions[0])} else {None},"addresses":positions.iter().map(|p|0x02000000+p).collect::<Vec<_>>(),"decoded_size":bytes.len(),"decoded_sha256":sha(&bytes)}));
    }
    ensure!(!entries.is_empty(), "no textures to compare");
    Ok(
        json!({"rom_sha256":sha(rom.bytes),"archive":archive,"archive_sha256":sha(source),"table":table,"table_sha256":sha(table_bytes),"ram_sha256":sha(ram),"region_base":0x02000000,"textures":entries,"excluded_records":ignored,"allow_copies":allow_copies,"claim":if allow_copies {"all nonzero-dimension decoded texture payloads present in frozen main RAM; all matching addresses recorded; no unique allocation or screen selection claim"} else {"all nonzero-dimension decoded texture payloads uniquely resident in frozen main RAM; capture and launch binding are separate evidence"}}),
    )
}
