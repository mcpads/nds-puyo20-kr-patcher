//! One white A5I3 mask per academy lesson, cropped to 146×16 by DSIF.
use super::*;

pub fn prepare(
    rom: &Rom,
    surface: &str,
    translation: &Path,
    font_path: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let (archive, archive_sha, table_name, table_sha, first, originals): (_, _, _, _, _, &[&str]) =
        match surface {
            "guide" => (
                "academy_guide",
                "770815f8894178c01579db3a03af16947d7299606fca02773d94a8b4e6d8caee",
                "nyumon_b",
                "dc9501fac16e97e190e145ff54176e3f607c7b424d418d06a798cb8150e9c325",
                32,
                &[
                    "ぷよぷよのきほん",
                    "ぷよぷよのテクニック",
                    "ぷよぷよSUNルール",
                    "フィーバールール",
                    "ぷよぷよルール",
                    "ぷよぷよ通ルール",
                    "なぞぷよルール",
                ],
            ),
            "practice" => (
                "academy_practice",
                "3eb756dc9b543d40e32e55fa1716b5505e6544e23a7ff3bc645346798b61ce29",
                "jissen_b",
                "854cf3116854350082e1ea9ce8f79af10a5bf740682c5823e8a6fd9e6346733e",
                35,
                &[
                    "２れんさをつくろう",
                    "２れんさから３れんさ",
                    "かいだんづみ　その１",
                    "かいだんづみ　その２",
                    "はさみこみ　その１",
                    "はさみこみ　その２",
                    "かいだんづみとはさみこみ",
                    "おりかえし　その１",
                    "おりかえし　その２",
                    "そうごうふくしゅう",
                ],
            ),
            "challenge-menu" => (
                "challenge_menu",
                "e1402989d66a1927538ac0363f8f05594992111647e3b772c0f8973901126426",
                "challenge_menu_b",
                "d4e4c34ce60cadb0cd44aa2d427e98fa56d570ce6ae0da343f3804e3afca870d",
                19,
                &["チャレンジテスト", "チャレンジたいせん"],
            ),
            "challenge-play" => (
                "challenge_play",
                "008b2fb07158b4902390dd141090f22e002d66dc3b4c7cad960147b245599b89",
                "challenge_taisen_b",
                "3f98da1278dcdcee9905c0803155809b0475ab3e0bd41d4511b68fdcb794c3d5",
                32,
                &[
                    "チャレンジぷよぷよ",
                    "チャレンジぷよ通",
                    "チャレンジぷよSUN",
                    "チャレンジフィーバー",
                    "チャレンジなぞぷよ",
                ],
            ),
            "challenge-problem" => (
                "challenge_problem",
                "d8bcae711519e706ec45cbd88082ffe958c7afcc5f4bb9966883e93fe0ff8c9d",
                "challenge_mondai_b",
                "0811c3788c8ec8552b8bf808a65560e05e899dabdcbc47365610da64835ff881",
                49,
                &[
                    "だいれんさ その１",
                    "だいれんさ その２",
                    "だいれんさ その３",
                    "だいれんさでぜんけし １",
                    "だいれんさでぜんけし ２",
                    "どうじけし その１",
                    "どうじけし その２",
                    "どうじけし その３",
                    "れんさのタネ その１",
                    "れんさのタネ その２",
                    "れんさのタネ その３",
                    "ちびぷよだいれんさ １",
                    "ちびぷよだいれんさ ２",
                    "ちびぷよれんさタネ １",
                    "ちびぷよれんさタネ２",
                    "むかいてんだいれんさ",
                    "めかくしだいれんさ １",
                    "めかくしだいれんさ ２",
                ],
            ),
            _ => anyhow::bail!("unknown academy list: {surface}"),
        };
    let (first_texture, crop, crop_width, layout_members): (usize, [usize; 4], usize, Vec<usize>) =
        match surface {
            "challenge-menu" => (8, [0xfffffff0, 0, 2288, 4096], 143, vec![0, 1]),
            "challenge-play" => (6, [0, 0, 2336, 4096], 146, vec![0, 5, 6, 7, 8, 9, 28]),
            "challenge-problem" => (
                6,
                [0, 0, 2336, 4096],
                146,
                std::iter::once(0).chain(5..24).chain([42, 43]).collect(),
            ),
            _ => (
                6,
                [0, 0, 2336, 4096],
                146,
                (5..5 + originals.len()).collect(),
            ),
        };
    let path = format!("menu/{archive}.narc");
    let source = rom.data(rom.file(&path)?);
    ensure!(sha(source) == archive_sha, "academy archive changed");
    let n = Narc::parse(source)?;
    let table = rom.data(rom.file(&format!("menu/{table_name}_texlist.bin"))?);
    ensure!(sha(table) == table_sha, "academy table changed");
    let input = fs::read(translation)?;
    let tr: Translation = serde_json::from_slice(&input)?;
    ensure!(
        tr.state == "development_art_draft" && tr.entries.len() == originals.len(),
        "academy population changed"
    );
    let font_bytes = fs::read(font_path)?;
    let font_sha256 = sha(&font_bytes);
    let rounded = font_sha256 == crate::fonts::BMJUA_SHA256;
    let (font_size, baseline) = if crate::fonts::is_galmuri11(&font_bytes) {
        (12, 14)
    } else if font_sha256 == crate::fonts::GALMURI14_SHA256 {
        (15, 15)
    } else if rounded {
        // Rounded bold outline font with antialiased alpha, like the source
        // lettering; the baseline is derived below from the hangul top row.
        (16, 0)
    } else {
        anyhow::bail!("academy font changed");
    };
    let space_width = if font_size == 15 { 4 } else { 5 };
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut layouts = Vec::new();
    for member in layout_members {
        let b = unpack(n.members[member])?;
        ensure!(
            slice(&b, 0, 4)? == b"DSIF" && u32le(&b, 12)? == 32 && slice(&b, 32, 4)? == b"nCSC",
            "academy layout changed"
        );
        // This original problem layout alone exposes the complete texture width.
        // Render within the shared 146px region so both consumers retain the label.
        let layout_crop = if surface == "challenge-problem" && member == 15 {
            [0, 0, 4096, 4096]
        } else {
            crop
        };
        let offset = 32 + u32le(&b, 80)?;
        let mut seen = vec![0; originals.len()];
        for i in 0..u32le(&b, 76)? {
            let p = offset + i * 20;
            let tex = u32le(&b, p)?;
            if (first_texture..first_texture + originals.len()).contains(&tex) {
                for (j, v) in layout_crop.iter().enumerate() {
                    ensure!(u32le(&b, p + 4 + j * 4)? == *v, "academy list crop changed");
                }
                seen[tex - first_texture] += 1;
            }
        }
        ensure!(
            seen.iter().all(|v| *v == 1),
            "academy list crop population changed"
        );
        layouts.push(json!({"member":member,"sha256":sha(&b)}));
    }
    let mut replacements = BTreeMap::new();
    let mut records = Vec::new();
    let mut preview = vec![20u8; 256 * originals.len() * 20 * 4];
    for pixel in preview.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    for (row, (entry, original)) in tr.entries.iter().zip(originals).enumerate() {
        ensure!(entry.japanese == *original, "academy source order changed");
        let member = first + row * 2;
        let p = (first_texture + row) * 12;
        ensure!(
            u16le(table, p)? == member
                && u16le(table, p + 2)? == member - 1
                && u16le(table, p + 4)? == 6
                && u16le(table, p + 8)? == 256
                && u16le(table, p + 10)? == 16,
            "academy mask geometry changed"
        );
        let old = unpack_halfword(n.members[member])?;
        ensure!(
            old.len() == 4096
                && old.iter().all(|v| v & 7 == 0)
                && n.members[member - 1] == [255, 127],
            "academy mask/palette changed"
        );
        let mut pixels = old.clone();
        for y in 0..16 {
            for x in 0..256 {
                if x < crop_width {
                    pixels[y * 256 + x] = 0;
                } else {
                    ensure!(old[y * 256 + x] == 0, "academy non-text margin changed");
                }
            }
        }
        if rounded {
            let coverage = rounded_coverage(&font, &entry.korean, font_size, crop_width)?;
            for (i, &a) in coverage.iter().enumerate() {
                pixels[(i / crop_width) * 256 + i % crop_width] = a << 3;
            }
            battle_ui::compress_member(&mut replacements, member, &pixels)?;
            ensure!(
                replacements[&member].len() <= n.members[member].len(),
                "academy label exceeds stored capacity: {} > {}",
                replacements[&member].len(),
                n.members[member].len()
            );
            for (i, v) in pixels.iter().enumerate() {
                let c = 20 + ((*v >> 3) as u16 * 235 / 31) as u8;
                let at = (row * 20 * 256 + i) * 4;
                preview[at..at + 4].copy_from_slice(&[c, c, c, 255]);
            }
            records.push(json!({"member":member,"texture":first_texture+row,"rect":[0,0,crop_width,16],"japanese":original,"korean":entry.korean,"stored_size":replacements[&member].len(),"capacity":n.members[member].len(),"pixels_sha256":sha(&pixels)}));
            continue;
        }
        let ink = battle_ui::text_ink_with_space(
            &font,
            &entry.korean,
            font_size,
            [0, 0, crop_width, 16],
            baseline,
            space_width,
        )?;
        let left = ink
            .iter()
            .map(|(x, _)| *x)
            .min()
            .ok_or_else(|| anyhow::anyhow!("empty academy label"))?;
        for (x, y) in ink {
            let x = x - left + 2;
            let bold = usize::from(font_size == 15);
            ensure!(x + bold < crop_width, "academy label exceeds crop");
            for dx in 0..=bold {
                pixels[y * 256 + x + dx] = 248;
            }
        }
        battle_ui::compress_member(&mut replacements, member, &pixels)?;
        for (i, v) in pixels.iter().enumerate() {
            let c = 20 + ((*v >> 3) as u16 * 235 / 31) as u8;
            let at = (row * 20 * 256 + i) * 4;
            preview[at..at + 4].copy_from_slice(&[c, c, c, 255]);
        }
        records.push(json!({"member":member,"texture":first_texture+row,"rect":[0,0,crop_width,16],"japanese":original,"korean":entry.korean,"stored_size":replacements[&member].len(),"capacity":n.members[member].len(),"pixels_sha256":sha(&pixels)}));
    }
    let rebuilt = battle_ui::rebuilt(source, &replacements)?;
    fs::create_dir_all(out)?;
    fs::write(out.join("labels.narc"), &rebuilt)?;
    write_png(
        &out.join("labels-on-dark.png"),
        256,
        originals.len() * 20,
        &preview,
    )?;
    json_file(
        &out.join("plan.json"),
        &json!({"source_sha256":sha(rom.bytes),"replacements":[{"file":path,"expected_sha256":sha(source),"input":"labels.narc","input_sha256":sha(&rebuilt)}]}),
    )?;
    let report = json!({"source_sha256":sha(rom.bytes),"translation_sha256":sha(&input),"font_sha256":font_sha256,"font_size":font_size,"baseline":baseline,"space_width":space_width,"horizontal_bold_pixels":usize::from(font_size == 15),"antialiased_alpha":rounded,"surface":surface,"layouts":layouts,"records":records,"archive_sha256":sha(&rebuilt),"runtime_verified":false,"human_reviewed":false});
    json_file(&out.join("labels.json"), &report)?;
    Ok(report)
}

/// Antialiased 5-bit alpha of `text` drawn from x=2 with the hangul top on row 1
/// of a 16-row mask. Coverage below 12/255 stays transparent.
fn rounded_coverage(
    font: &fontdue::Font,
    text: &str,
    size: usize,
    width: usize,
) -> Result<Vec<u8>> {
    let size = size as f32;
    let (m, _) = font.rasterize('가', size);
    let baseline = 1 + m.ymin + m.height as i32;
    let mut alpha = vec![0u8; width * 16];
    let mut cursor = 2.0f32;
    for ch in text.chars() {
        ensure!(
            !ch.is_control() && font.lookup_glyph_index(ch) != 0,
            "missing academy glyph {ch}"
        );
        let (m, bitmap) = font.rasterize(ch, size);
        let left = cursor.round() as i32 + m.xmin;
        for y in 0..m.height {
            for x in 0..m.width {
                let v = bitmap[y * m.width + x];
                if v < 12 {
                    continue;
                }
                let px = left + x as i32;
                let py = baseline - m.ymin - m.height as i32 + y as i32;
                ensure!(
                    px >= 0 && (px as usize) + 1 < width && (0..16).contains(&py),
                    "academy label exceeds crop: {text}"
                );
                // Five alpha levels, as the generated academy lists, keep the
                // compressed mask inside the original stored size.
                let a = [0u8, 8, 16, 24, 31][((u32::from(v) * 4 + 127) / 255) as usize];
                let at = py as usize * width + px as usize;
                alpha[at] = alpha[at].max(a);
            }
        }
        cursor += m.advance_width;
    }
    ensure!(alpha.iter().any(|&a| a > 0), "empty academy label");
    Ok(alpha)
}
