//! Inspect the three independent banks of academy challenge battle scenes.
use super::*;
use crate::titles;

pub fn inspect(rom: &Rom, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let archive = rom.data(rom.file("academy/academy.narc")?);
    ensure!(
        sha(archive) == "b0c7244f4d3de4d9836a1e6607097bffdbfb63644f8e01e28f191f0ddd0a7ed3",
        "academy source changed"
    );
    let n = Narc::parse(archive)?;
    let mut reports = Vec::new();
    let mut previews = Vec::new();
    for (surface, member, hash) in [
        (
            "fev",
            18,
            "e97234edd46e52653bf3021fd99a5de14a44901d051340b916726c85fd3d21ef",
        ),
        (
            "nazo",
            19,
            "c1eb236cb600c386b833c0733771794b1aaf330e614ed1c617f12e4080bc2efe",
        ),
        (
            "puyo2",
            20,
            "93b916c93eac484d11c8215bdb50918c56bcdf09a0debaeca3fd6f0824d79172",
        ),
        (
            "puyo3",
            21,
            "50e1db67c85e7dec74b64e8e4bd16b30fbf312d368827a4e1c6249b3df8ac93d",
        ),
        (
            "puyo",
            22,
            "f5ecf9a43fe251f9236022619fb487525c1791b067df649c2312fefffec13a8c",
        ),
    ] {
        let gem = unpack(n.members[member])?;
        let ilf = rom.data(rom.file(&format!("academy/challenge_taisen_{surface}_t_ilf.bin"))?);
        ensure!(sha(ilf) == hash, "battle ILF changed");
        ensure!(
            slice(&gem, 0, 4)? == b"Gem1" && u32le(&gem, 8)? == gem.len(),
            "invalid Gem1"
        );
        let base = u32le(&gem, 20)?;
        let g2 = u32le(&gem, 0x58)?;
        ensure!(
            slice(&gem, g2, 4)? == b"Gem2" && g2 + u32le(&gem, g2 + 20)? == base,
            "Gem2 base mismatch"
        );
        ensure!(
            u32le(&gem, 0x20)? == 1 && u32le(&gem, base + 16)? == 3,
            "expected one three-bank scene"
        );
        let count = u32le(&gem, 0x28)?;
        ensure!(
            count * 4 == ilf.len() && u32le(&gem, 0x50)? == count,
            "image population mismatch"
        );
        let sizes = base + u32le(&gem, 0x54)?;
        let images = base + u32le(&gem, 0x2c)?;
        let banks = base + u32le(&gem, base + 20)?;
        let mut bank_reports = Vec::new();
        for bank_id in 0..3 {
            let bank = banks + bank_id * 16;
            let nodes = base + u32le(&gem, bank + 4)?;
            let mut parts = Vec::new();
            for child in 1..u32le(&gem, bank)? {
                let node = nodes + child * 80;
                let aux = base + u32le(&gem, node + 16)?;
                ensure!(
                    u32le(&gem, aux)? == u32le(&gem, node)?
                        && base + u32le(&gem, aux + 8)? == node
                        && base + u32le(&gem, node + 24)? == nodes,
                    "node hierarchy mismatch"
                );
                ensure!(
                    u32le(&gem, node + 40)? == 4096
                        && u32le(&gem, node + 44)? == 4096
                        && u32le(&gem, node + 48)? == 0,
                    "nonidentity child transform"
                );
                let resource = base + u32le(&gem, aux + 20)?;
                let size = base + u32le(&gem, resource)?;
                ensure!(
                    size >= sizes && (size - sizes) % 16 == 0 && (size - sizes) / 16 < count,
                    "size pointer outside table"
                );
                let id = (size - sizes) / 16;
                let descriptor = images + id * 32;
                let w = u16le(&gem, descriptor + 8)?;
                let h = u16le(&gem, descriptor + 10)?;
                let packed = u32le(ilf, id * 4)?;
                ensure!(
                    base + u32le(&gem, size + 4)? == descriptor
                        && u32le(&gem, descriptor)? == packed & 255
                        && u32le(&gem, descriptor + 4)? == 3
                        && u16le(&gem, aux + 24)? == w
                        && u16le(&gem, aux + 26)? == h
                        && u32le(&gem, size + 8)? == w << 16
                        && u32le(&gem, size + 12)? == h << 16,
                    "image/size mismatch"
                );
                let pixel_member = (packed >> 8) & 4095;
                let palette_member = packed >> 20;
                let bytes = unpack_halfword(n.members[pixel_member])?;
                let palette = unpack(n.members[palette_member])?;
                let bpp = match palette.len() {
                    32 => 4,
                    512 => 8,
                    _ => anyhow::bail!("unsupported palette"),
                };
                let pixels = titles::untile(&bytes, w, h, bpp)?;
                ensure!(
                    titles::tile(&pixels, w, h, bpp)? == bytes,
                    "tile round trip failed"
                );
                let fixed = |off| -> Result<i64> {
                    let v = u32le(&gem, off)? as u32 as i32;
                    ensure!(v % 4096 == 0, "fractional child position");
                    Ok(i64::from(v / 4096))
                };
                let x = fixed(node + 32)? - fixed(node + 8)?;
                let y = fixed(node + 36)? - fixed(node + 12)?;
                parts.push((x,y,w,h,titles::rgba(&pixels,&palette)?,json!({"image":id,"member":pixel_member,"palette_member":palette_member,"x":x,"y":y,"width":w,"height":h,"bpp":bpp,"decoded_sha256":sha(&bytes)})));
            }
            ensure!(!parts.is_empty(), "empty battle bank");
            let left = parts.iter().map(|p| p.0).min().unwrap();
            let top = parts.iter().map(|p| p.1).min().unwrap();
            let w = parts
                .iter()
                .map(|p| (p.0 - left) as usize + p.2)
                .max()
                .unwrap();
            let h = parts
                .iter()
                .map(|p| (p.1 - top) as usize + p.3)
                .max()
                .unwrap();
            ensure!(w <= 256 && h <= 256, "battle bank canvas exceeds bounds");
            let mut rgba = vec![0u8; w * h * 4];
            let mut overlap_pixels = 0;
            for (part_id, p) in parts.iter().enumerate() {
                previews.push((
                    format!("{surface}-bank-{bank_id}-part-{part_id}"),
                    p.2,
                    p.3,
                    p.4.clone(),
                ));
            }
            for (x, y, pw, ph, pixels, _) in &parts {
                for py in 0..*ph {
                    for px in 0..*pw {
                        let src = (py * pw + px) * 4;
                        let dst = (((y - top) as usize + py) * w + (x - left) as usize + px) * 4;
                        if pixels[src + 3] != 0 {
                            if rgba[dst + 3] != 0 && rgba[dst..dst + 4] != pixels[src..src + 4] {
                                overlap_pixels += 1;
                            }
                            rgba[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
                        }
                    }
                }
            }
            previews.push((format!("{surface}-bank-{bank_id}"), w, h, rgba));
            bank_reports.push(json!({"bank":bank_id,"overlap_pixels":overlap_pixels,"width":w,"height":h,"origin":[left,top],"root_x_fixed":u32le(&gem,nodes+32)? as u32 as i32,"root_y_fixed":u32le(&gem,nodes+36)? as u32 as i32,"parts":parts.into_iter().map(|p|p.5).collect::<Vec<_>>()}));
        }
        reports.push(json!({"surface":surface,"gem_member":member,"gem_sha256":sha(&gem),"ilf_sha256":hash,"banks":bank_reports}));
    }
    fs::create_dir_all(out)?;
    for (name, w, h, rgba) in previews {
        write_png(&out.join(format!("{name}.png")), w, h, &rgba)?;
    }
    let report = json!({"source_sha256":sha(rom.bytes),"archive_sha256":sha(archive),"surfaces":reports,"claim":"static bank-local composition in node order, unverified draw order; root transforms and runtime selection not applied; no product changes"});
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
