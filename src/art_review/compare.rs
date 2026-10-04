//! Artifact-bound, standalone JP/KR inspection gallery. Flags are review hints.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
fn base64(b: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        for shift in [18, 12, 6, 0] {
            s.push(T[((n >> shift) & 63) as usize] as char);
        }
        if c.len() < 3 {
            s.pop();
            s.push('=');
        }
        if c.len() < 2 {
            let p = s.len() - 2;
            s.replace_range(p..p + 1, "=");
        }
    }
    s
}
fn metrics(img: &crate::art_pixels::Image) -> Value {
    let w = img.width;
    let h = img.height;
    let visible: Vec<_> = img.rgba.chunks_exact(4).map(|p| p[3] > 0).collect();
    let mut seen = vec![false; visible.len()];
    let mut tiny = Vec::new();
    let mut bounds = [w, h, 0, 0];
    let mut main_bounds = [w, h, 0, 0];
    let mut count = 0;
    let mut colors = BTreeMap::<String, usize>::new();
    for pixel in img.rgba.chunks_exact(4).filter(|p| p[3] > 0) {
        *colors.entry(hex::encode(&pixel[..3])).or_default() += 1;
    }
    for i in 0..visible.len() {
        if visible[i] {
            count += 1;
            bounds[0] = bounds[0].min(i % w);
            bounds[1] = bounds[1].min(i / w);
            bounds[2] = bounds[2].max(i % w + 1);
            bounds[3] = bounds[3].max(i / w + 1);
        }
    }
    for i in 0..visible.len() {
        if !visible[i] || seen[i] {
            continue;
        }
        let mut q = VecDeque::from([i]);
        seen[i] = true;
        let mut size = 0;
        let mut box_ = [w, h, 0, 0];
        while let Some(p) = q.pop_front() {
            size += 1;
            let x = p % w;
            let y = p / w;
            box_[0] = box_[0].min(x);
            box_[1] = box_[1].min(y);
            box_[2] = box_[2].max(x + 1);
            box_[3] = box_[3].max(y + 1);
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let n = ny as usize * w + nx as usize;
                        if visible[n] && !seen[n] {
                            seen[n] = true;
                            q.push_back(n);
                        }
                    }
                }
            }
        }
        if size <= 3 {
            tiny.push(json!({"pixels":size,"bbox":box_}));
        } else {
            main_bounds[0] = main_bounds[0].min(box_[0]);
            main_bounds[1] = main_bounds[1].min(box_[1]);
            main_bounds[2] = main_bounds[2].max(box_[2]);
            main_bounds[3] = main_bounds[3].max(box_[3]);
        }
    }
    json!({"visible_pixels":count,"rgb_usage":colors,"bbox":if count==0 {None}else{Some(bounds)},"main_bbox":if main_bounds[2]==0 {None}else{Some(main_bounds)},"small_components":tiny,"meaning":"alpha-connected components, not a text mask; punctuation and decoration can be intentional; RGB counts do not identify fill/outline roles"})
}
pub fn run(jp: &Rom, kr: &Rom, catalog: &Path, build: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let build_bytes = fs::read(build)?;
    let b: Value = serde_json::from_slice(&build_bytes)?;
    ensure!(
        b["source_sha256"] == sha(jp.bytes) && b["output_sha256"] == sha(kr.bytes),
        "build identity differs"
    );
    let bytes = fs::read(catalog)?;
    let mut c: Catalog = serde_json::from_slice(&bytes)?;
    for (i, spec) in c.sprite_specs.iter().enumerate() {
        for g in crate::sprites::review_groups(jp, spec, &format!("sprite-{i}"))? {
            c.groups.push(serde_json::from_value(g)?);
        }
    }
    for (i, spec) in c.texture_specs.iter().enumerate() {
        for g in crate::labels::review_groups(jp, spec, &format!("label-{i}"))? {
            c.groups.push(serde_json::from_value(g)?);
        }
    }
    fs::create_dir_all(out.join("images"))?;
    let layout_checks = if c.game_over_layout {
        crate::art_import::game_over::review_final(jp, kr, out)?
    } else {
        Value::Null
    };
    let mut rows = Vec::new();
    let mut embedded = Vec::new();
    let mut failures = Vec::new();
    let mut covered = BTreeSet::new();
    let mut owners = BTreeMap::new();
    for r in b["member_writers"].as_array().unwrap() {
        owners.insert(
            (
                r["file"].as_str().unwrap().to_owned(),
                r["member"].as_u64().unwrap() as usize,
            ),
            r["writers"].clone(),
        );
    }
    for g in c.groups {
        for (n, s) in g.surfaces.iter().enumerate() {
            ensure!(
                g.id.bytes().all(|x| x.is_ascii_alphanumeric() || x == b'-'),
                "unsafe id"
            );
            let id = format!("{}-{n}", g.id);
            let mut versions = Vec::new();
            let mut images = Vec::new();
            let mut urls = Vec::new();
            let mut failed = false;
            for (tag, rom) in [("jp", jp), ("kr", kr)] {
                let file = format!("images/{id}-{tag}.png");
                match preview(rom, &g.archive, s, &out.join(&file)) {
                    Ok(mut v) => {
                        v["image"] = json!(file);
                        if g.white_background_preview || g.preview_background.is_some() {
                            ensure!(
                                !g.white_background_preview || g.preview_background.is_none(),
                                "conflicting diagnostic backgrounds"
                            );
                            let background = g.preview_background.unwrap_or([255; 3]);
                            let mut display = crate::art_pixels::read(&fs::read(out.join(&file))?)?;
                            for pixel in display.rgba.chunks_exact_mut(4) {
                                let alpha = u32::from(pixel[3]);
                                for (channel, backdrop) in pixel[..3].iter_mut().zip(background) {
                                    *channel = ((u32::from(*channel) * alpha
                                        + u32::from(backdrop) * (255 - alpha)
                                        + 127)
                                        / 255) as u8;
                                }
                                pixel[3] = 255;
                            }
                            let suffix = if g.white_background_preview {
                                "white"
                            } else {
                                "background"
                            };
                            let white_file = format!("images/{id}-{tag}.{suffix}.png");
                            write_png(&out.join(&white_file), s.width, s.height, &display.rgba)?;
                            let key = if g.white_background_preview {
                                "white_background_preview"
                            } else {
                                "background_preview"
                            };
                            v[key] = json!({
                                "image": white_file,
                                "sha256": sha(&fs::read(out.join(&white_file))?),
                                "rgb": background,
                                "claim": "Diagnostic alpha composite; native RGBA and metrics unchanged; not a product input."
                            });
                        }
                        versions.push(v);
                        let raw = fs::read(out.join(&file))?;
                        images.push(crate::art_pixels::read(&raw)?);
                        urls.push(format!("data:image/png;base64,{}", base64(&raw)));
                    }
                    Err(e) => {
                        failures.push(json!({"id":id,"archive":g.archive,"member":s.member,"version":tag,"error":e.to_string()}));
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            let a = &images[0];
            let k = &images[1];
            let changed = a
                .rgba
                .chunks_exact(4)
                .zip(k.rgba.chunks_exact(4))
                .filter(|(x, y)| (x[3] != 0 || y[3] != 0) && x != y)
                .count();
            let newly_visible = a
                .rgba
                .chunks_exact(4)
                .zip(k.rgba.chunks_exact(4))
                .filter(|(x, y)| x[3] == 0 && y[3] > 0)
                .count();
            let removed = a
                .rgba
                .chunks_exact(4)
                .zip(k.rgba.chunks_exact(4))
                .filter(|(x, y)| x[3] > 0 && y[3] == 0)
                .count();
            let am = metrics(a);
            let km = metrics(k);
            let mut flags = Vec::new();
            let name = owners
                .get(&(g.archive.clone(), s.member))
                .is_some_and(|o| o.to_string().contains("name"));
            if name && s.height <= 64 && g.surfaces.len() == 1 {
                if let (Some(j), Some(k)) = (am["main_bbox"].as_array(), km["main_bbox"].as_array())
                {
                    let center =
                        |b: &[Value]| (b[0].as_f64().unwrap() + b[2].as_f64().unwrap()) / 2.0;
                    if (center(j) - s.width as f64 / 2.0).abs() <= 1.5
                        && (center(k) - s.width as f64 / 2.0).abs() > 3.0
                    {
                        flags.push("이름 셀 중심 이탈 후보");
                    }
                }
            }
            let new_color_pixels: u64 = km["rgb_usage"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(rgb, _)| am["rgb_usage"].get(*rgb).is_none())
                .map(|(_, count)| count.as_u64().unwrap())
                .sum();
            if new_color_pixels >= 16 {
                flags.push("원본 그림에 없던 색 사용");
            }
            let area = (s.width * s.height) as u64;
            if am["visible_pixels"].as_u64().unwrap() * 100 >= area * 95
                && km["visible_pixels"].as_u64().unwrap() * 100 < area * 85
            {
                flags.push("불투명 배경 범위 감소");
            }
            if g.surfaces.len() == 1
                && km["small_components"].as_array().unwrap().len()
                    > am["small_components"].as_array().unwrap().len()
            {
                flags.push("작은 고립 조각 증가");
            }
            if g.archive == "puyo/menu/puyo_menu.narc" && [29, 31, 34, 37, 41].contains(&s.member) {
                flags.push("버튼 색상 보고");
            }
            let row = json!({"id":id,"archive":g.archive,"member":s.member,"palette":s.palette,"palette_offset":s.palette_offset,"width":s.width,"height":s.height,"format":s.format,"japanese":g.japanese,"korean":g.korean_draft,"brief":g.brief,"note":s.note,"owners":owners.get(&(g.archive.clone(),s.member)),"changed_pixels":changed,"newly_visible":newly_visible,"removed":removed,"flags":flags,"jp_metrics":am,"kr_metrics":km,"versions":versions});
            let mut e = row.clone();
            e["images"] = json!(urls);
            embedded.push(e);
            rows.push(row);
            for member in [Some(s.member), Some(s.palette), s.map]
                .into_iter()
                .flatten()
            {
                covered.insert((g.archive.clone(), member));
            }
        }
        if let Some(layout) = &g.composition {
            if g.surfaces.len() > 1 {
                ensure!(
                    layout.positions.len() == g.surfaces.len(),
                    "composition mismatch"
                );
                let id = format!("{}-composed", g.id);
                let mut imgs = Vec::new();
                let mut urls = Vec::new();
                let mut versions = Vec::new();
                for (tag, rom) in [("jp", jp), ("kr", kr)] {
                    let positions = match &layout.gem_scene {
                        Some(scene) => {
                            let parts = crate::academy::logos::scene_positions(
                                rom,
                                &g.archive,
                                scene.member,
                                &scene.ilf,
                                &scene.scene,
                            )?;
                            ensure!(parts.len() == g.surfaces.len(), "Gem scene part count");
                            g.surfaces
                                .iter()
                                .zip(&parts)
                                .map(|(s, &(member, x, y))| {
                                    ensure!(member == s.member, "Gem scene part order");
                                    Ok([x, y])
                                })
                                .collect::<Result<Vec<_>>>()?
                        }
                        None => layout.positions.clone(),
                    };
                    let mut rgba = vec![0u8; layout.width * layout.height * 4];
                    for (n, s) in g.surfaces.iter().enumerate() {
                        let image = crate::art_pixels::read(&fs::read(
                            out.join(format!("images/{}-{n}-{tag}.png", g.id)),
                        )?)?;
                        let [x0, y0] = positions[n];
                        ensure!(
                            x0 + s.width <= layout.width && y0 + s.height <= layout.height,
                            "composition extent"
                        );
                        for y in 0..s.height {
                            for x in 0..s.width {
                                let p = (y * s.width + x) * 4;
                                let q = ((y + y0) * layout.width + x + x0) * 4;
                                if image.rgba[p + 3] > 0 {
                                    rgba[q..q + 4].copy_from_slice(&image.rgba[p..p + 4]);
                                }
                            }
                        }
                    }
                    let file = format!("images/{id}-{tag}.png");
                    write_png(&out.join(&file), layout.width, layout.height, &rgba)?;
                    urls.push(format!(
                        "data:image/png;base64,{}",
                        base64(&fs::read(out.join(&file))?)
                    ));
                    versions.push(json!({"image":file}));
                    imgs.push(crate::art_pixels::Image {
                        width: layout.width,
                        height: layout.height,
                        rgba,
                    });
                }
                let changed = imgs[0]
                    .rgba
                    .chunks_exact(4)
                    .zip(imgs[1].rgba.chunks_exact(4))
                    .filter(|(a, b)| (a[3] > 0 || b[3] > 0) && a != b)
                    .count();
                let row = json!({"id":id,"archive":g.archive,"member":g.surfaces.iter().map(|s|s.member.to_string()).collect::<Vec<_>>().join("+"),"palette":g.surfaces[0].palette,"palette_offset":0,"width":layout.width,"height":layout.height,"format":"composed","japanese":g.japanese,"korean":g.korean_draft,"brief":g.brief,"note":"제품 입력의 연결 순서를 재사용한 조합. 분할 조각을 따로 가운데 정렬하지 않는다.","owners":[],"changed_pixels":changed,"newly_visible":0,"removed":0,"flags":[],"jp_metrics":metrics(&imgs[0]),"kr_metrics":metrics(&imgs[1]),"versions":versions});
                let mut e = row.clone();
                e["images"] = json!(urls);
                embedded.push(e);
                rows.push(row);
            }
        }
    }
    for (row, embedded_row) in rows.iter_mut().zip(embedded.iter_mut()) {
        if let Some(notes) = c.review_notes.get(row["id"].as_str().unwrap()) {
            row["review_notes"] = json!(notes);
            row["flags"]
                .as_array_mut()
                .unwrap()
                .push(json!("지적된 문제"));
            embedded_row["review_notes"] = json!(notes);
            embedded_row["flags"] = row["flags"].clone();
        }
    }
    let mut uncovered = Vec::new();
    for ((archive, id), writers) in &owners {
        if covered.contains(&(archive.clone(), *id)) {
            continue;
        }
        let j = Narc::parse(jp.data(jp.file(archive)?))?;
        let k = Narc::parse(kr.data(kr.file(archive)?))?;
        let a = unpack(j.members[*id])?;
        let b = unpack(k.members[*id])?;
        if a != b {
            uncovered.push(json!({"archive":archive,"member":id,"writers":writers,"decoded_bytes":b.len(),"prefix_hex":hex::encode(&b[..b.len().min(16)]),"jp_sha256":sha(&a),"kr_sha256":sha(&b)}));
        }
    }
    let report = json!({"jp_sha256":sha(jp.bytes),"kr_sha256":sha(kr.bytes),"build_sha256":sha(&build_bytes),"catalog_sha256":sha(&bytes),"pairs":rows.len(),"changed_pairs":rows.iter().filter(|r|r["changed_pixels"].as_u64().unwrap()>0).count(),"rows":rows,"failures":failures,"writer_members_without_catalog_view":uncovered,"claim":"Exact ROM pixels with catalog geometry; alpha and metadata interpretation follow each source catalog. Flags are hints, not confirmed residue. No runtime or human approval claim."});
    json_file(&out.join("report.json"), &report)?;
    json_file(&out.join("layout-checks.json"), &layout_checks)?;
    let data = serde_json::to_string(&embedded)?.replace('<', "\\u003c");
    let html = include_str!("compare.html")
        .replace("__DATA__", &data)
        .replace("__JP__", &sha(jp.bytes))
        .replace("__KR__", &sha(kr.bytes))
        .replace("__ERRORS__", &failures.len().to_string())
        .replace("__UNMAPPED__", &uncovered.len().to_string());
    fs::write(out.join("index.html"), html)?;
    Ok(
        json!({"pairs":report["pairs"],"changed_pairs":report["changed_pairs"],"failures":failures.len(),"unmapped_writer_members":uncovered.len(),"out":out}),
    )
}
