//! Three-way worklist: byte changes are coverage candidates, never completion claims.
use super::*;

fn state(original: &[u8], stored: &[u8], decoded: &[u8], other_decoded: &[u8]) -> &'static str {
    if original == stored {
        "same"
    } else if decoded == other_decoded {
        "compression_only"
    } else {
        "content_changed"
    }
}

pub fn compare(
    j: &Rom,
    e: &Rom,
    k: &Rom,
    build_path: &Path,
    translations: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let build_bytes = fs::read(build_path)?;
    let build: Value = serde_json::from_slice(&build_bytes)?;
    let source_sha = sha(j.bytes);
    let product_sha = sha(k.bytes);
    ensure!(
        build["source_sha256"] == source_sha && build["output_sha256"] == product_sha,
        "build identity differs"
    );
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in build["plans"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing build plans"))?
    {
        let path = Path::new(
            entry["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing plan path"))?,
        );
        let bytes = fs::read(path)?;
        ensure!(
            entry["sha256"] == sha(&bytes),
            "plan changed: {}",
            path.display()
        );
        let plan: crate::build::Plan = serde_json::from_slice(&bytes)?;
        ensure!(plan.source_sha256 == source_sha, "plan source differs");
        for replacement in &plan.replacements {
            let input = fs::read(path.parent().unwrap().join(&replacement.input))?;
            ensure!(
                sha(&input) == replacement.input_sha256
                    && k.data(k.file(&replacement.file)?) == input,
                "planned product file differs: {}",
                replacement.file
            );
            owners
                .entry(replacement.file.clone())
                .or_default()
                .push(path.display().to_string());
        }
    }
    let mut inputs: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut paths = fs::read_dir(translations)?
        .map(|e| e.map(|v| v.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
    {
        let bytes = fs::read(&path)?;
        let tr: Value = serde_json::from_slice(&bytes)?;
        if let Some(target) = tr["path"].as_str().filter(|p| p.starts_with("text/")) {
            inputs.entry(target.to_string()).or_default().push(json!({"file":path,"sha256":sha(&bytes),"entries":tr["entries"].as_array().map(Vec::len),"state":tr["state"]}));
        }
    }
    let jk = j.keyed()?;
    let ek = e.keyed()?;
    let kk = k.keyed()?;
    ensure!(
        jk.keys().eq(ek.keys()) && jk.keys().eq(kk.keys()),
        "file identities differ"
    );
    let mut files = Vec::new();
    let mut members = Vec::new();
    let mut pairs = Vec::new();
    let mut archives = Vec::new();
    let mut scripts = Vec::new();
    let mut file_counts = BTreeMap::new();
    let mut member_counts = BTreeMap::new();
    let mut roles = BTreeMap::new();
    for (name, entry) in &jk {
        let a = j.data(entry);
        let b = e.data(ek[name]);
        let c = k.data(kk[name]);
        let da = unpack(a)?;
        let db = unpack(b)?;
        let dc = unpack(c)?;
        let en = state(a, b, &da, &db);
        let kr = state(a, c, &da, &dc);
        let key = format!("{en}/{kr}");
        inc(&mut file_counts, &key);
        files.push(json!({"path":name,"role":role(name,&da),"en_change":en,"kr_change":kr,"jp_sha256":sha(a),"en_sha256":sha(b),"kr_sha256":sha(c),"plans":owners.get(name).cloned().unwrap_or_default().join(";"),"disposition":if en!="same" && kr=="same" {"review_untouched_EN_delta"}else if kr!="same" {"modified_not_verified_complete"}else{"unchanged_not_excluded"}}));
        if let Some(stem) = name.strip_suffix(".fnt") {
            let text_name = format!("{stem}.mtx");
            let jt = unpack(j.data(j.file(&text_name)?))?;
            let et = unpack(e.data(e.file(&text_name)?))?;
            let kt = unpack(k.data(k.file(&text_name)?))?;
            let info = text_pair(&da, &jt)?;
            let translated = inputs.get(stem).cloned().unwrap_or_default();
            pairs.push(json!({"path":stem,"domain":stem.split('/').nth(1),"references":info.references.len(),"en_font_changed":da!=db,"en_text_changed":jt!=et,"kr_font_changed":da!=dc,"kr_text_changed":jt!=kt,"translation_inputs":translated,"plans":owners.get(&text_name).cloned().unwrap_or_default().join(";"),"disposition":if jt==kt && da==dc {"not_modified"}else{"modified_not_verified_complete"}}));
        }
        if name.ends_with(".pss") {
            scripts.push(json!({"path":name,"member_id":null,"en":script_delta(&da,&db),"kr":script_delta(&da,&dc)}));
        }
        if !name.ends_with(".narc") {
            continue;
        }
        let ja = Narc::parse(&da)?;
        let ea = Narc::parse(&db)?;
        let ka = Narc::parse(&dc)?;
        ensure!(
            ja.members.len() == ea.members.len() && ja.members.len() == ka.members.len(),
            "member count differs: {name}"
        );
        let eng = ea
            .members
            .iter()
            .map(|b| unpack(b))
            .collect::<Result<Vec<_>>>()?;
        let mut reverse: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, payload) in eng.iter().enumerate() {
            reverse.entry(sha(payload)).or_default().push(i);
        }
        let mut counts = BTreeMap::new();
        for (id, original) in ja.members.iter().enumerate() {
            if let Some(n) = ea.names.get(&id) {
                ensure!(
                    n.strip_suffix(".pss").unwrap_or(n).parse::<usize>()? == id,
                    "ENG name/index differs"
                );
            }
            let p = unpack(original)?;
            let q = &eng[id];
            let r = unpack(ka.members[id])?;
            let en = state(original, ea.members[id], &p, q);
            let kr = state(original, ka.members[id], &p, &r);
            let key = format!("{en}/{kr}");
            inc(&mut counts, &key);
            inc(&mut member_counts, &key);
            if name.starts_with("script/") {
                scripts.push(json!({"path":name,"member_id":id,"en":script_delta(&p,q),"kr":script_delta(&p,&r)}));
            }
            if en == "same" && kr == "same" {
                continue;
            }
            let kind = role(name, &p);
            inc(&mut roles, &format!("{kind}/{en}/{kr}"));
            let other = reverse
                .get(&sha(&p))
                .into_iter()
                .flatten()
                .filter(|&&n| n != id)
                .map(usize::to_string)
                .collect::<Vec<_>>();
            members.push(json!({"archive":name,"member_id":id,"role":kind,"en_change":en,"kr_change":kr,"jp_decoded_sha256":sha(&p),"en_decoded_sha256":sha(q),"kr_decoded_sha256":sha(&r),"jp_payload_other_en_ids":other.join(","),"alignment":"index_candidate_only","plans":owners.get(name).cloned().unwrap_or_default().join(";"),"disposition":if !other.is_empty(){"review_possible_member_reordering"}else if kind=="script"{"review_script_arguments_not_translation_count"}else if en=="compression_only"{"compression_only_not_translation"}else if kr=="same"{"review_untouched_EN_delta"}else{"modified_not_verified_complete"}}));
        }
        archives.push(json!({"path":name,"members":ja.members.len(),"counts":counts}));
    }
    let mut regions = Vec::new();
    for (name, off, len) in [
        ("arm9_stored", 0x20, 0x2c),
        ("arm7_stored", 0x30, 0x3c),
        ("arm9_overlay_table", 0x50, 0x54),
        ("arm7_overlay_table", 0x58, 0x5c),
    ] {
        let data = |r: &Rom| -> Result<Vec<u8>> {
            Ok(slice(r.bytes, u32le(r.bytes, off)?, u32le(r.bytes, len)?)?.to_vec())
        };
        let a = data(j)?;
        let b = data(e)?;
        let c = data(k)?;
        regions.push(json!({"region":name,"jp_sha256":sha(&a),"en_sha256":sha(&b),"kr_sha256":sha(&c),"en_changed":a!=b,"kr_changed":a!=c,"coordinate":"stored_image_not_execution_address","en_ranges":delta_ranges(&a,&b),"kr_changed_range_count":delta_ranges(&a,&c).len(),"kr_changed_bytes":delta_ranges(&a,&c).iter().map(|(start,end)|end-start).sum::<usize>()}));
    }
    let a = banner(j.bytes)?;
    let b = banner(e.bytes)?;
    let c = banner(k.bytes)?;
    regions.push(json!({"region":"banner","jp_sha256":sha(a),"en_sha256":sha(b),"kr_sha256":sha(c),"en_changed":a!=b,"kr_changed":a!=c}));
    let mut domains: BTreeMap<String, [u64; 6]> = BTreeMap::new();
    for pair in &pairs {
        let counts = domains
            .entry(pair["domain"].as_str().unwrap().to_string())
            .or_default();
        let refs = pair["references"].as_u64().unwrap();
        counts[0] += 1;
        counts[1] += refs;
        if pair["kr_text_changed"] == true {
            counts[2] += 1;
            counts[3] += refs;
        }
        if pair["en_text_changed"] == true && pair["kr_text_changed"] == false {
            counts[4] += 1;
            counts[5] += refs;
        }
    }
    let domain_rows=domains.into_iter().map(|(domain,c)|json!({"domain":domain,"pairs":c[0],"references":c[1],"modified_pairs":c[2],"references_in_modified_pairs":c[3],"en_changed_kr_untouched_pairs":c[4],"en_changed_kr_untouched_references":c[5]})).collect::<Vec<_>>();
    let archive_rows=archives.iter().map(|a| {
        let count=|key:&str|a["counts"][key].as_u64().unwrap_or(0);
        json!({"path":a["path"],"members":a["members"],"en_content_changed":count("content_changed/same")+count("content_changed/content_changed"),"kr_content_changed":count("content_changed/content_changed")+count("same/content_changed")+count("compression_only/content_changed"),"en_changed_kr_untouched":count("content_changed/same"),"compression_only":count("compression_only/same"),"unchanged_both":count("same/same")})
    }).collect::<Vec<_>>();
    let mut script_counts = BTreeMap::new();
    for row in &scripts {
        inc(
            &mut script_counts,
            &format!(
                "{}/{}",
                row["en"]["comparison"].as_str().unwrap(),
                row["kr"]["comparison"].as_str().unwrap()
            ),
        );
    }
    let summary = json!({"source_sha256":source_sha,"reference_sha256":sha(e.bytes),"product_sha256":product_sha,"build_manifest":build_path,"build_manifest_sha256":sha(&build_bytes),"plans":build["plans"].as_array().unwrap().len(),"file_count":files.len(),"archive_count":archives.len(),"text_pair_count":pairs.len(),"reference_count":pairs.iter().map(|p|p["references"].as_u64().unwrap()).sum::<u64>(),"script_counts":script_counts,"file_counts":file_counts,"member_counts":member_counts,"member_role_counts":roles,"limits":"All FAT files and same-index NARC members compared; row counts are not translation completion. Unchanged content is not excluded. Graphics/layout semantics, script consumers, embedded ROM contents and runtime/human verification remain separate. Plans identify file writers, not member-level completion."});
    fs::create_dir_all(out)?;
    json_file(&out.join("summary.json"), &summary)?;
    tsv(
        &out.join("domains.tsv"),
        &domain_rows,
        &[
            "domain",
            "pairs",
            "references",
            "modified_pairs",
            "references_in_modified_pairs",
            "en_changed_kr_untouched_pairs",
            "en_changed_kr_untouched_references",
        ],
    )?;
    tsv(
        &out.join("archive-worklist.tsv"),
        &archive_rows,
        &[
            "path",
            "members",
            "en_content_changed",
            "kr_content_changed",
            "en_changed_kr_untouched",
            "compression_only",
            "unchanged_both",
        ],
    )?;
    tsv(
        &out.join("files.tsv"),
        &files,
        &[
            "path",
            "role",
            "en_change",
            "kr_change",
            "disposition",
            "plans",
            "jp_sha256",
            "en_sha256",
            "kr_sha256",
        ],
    )?;
    tsv(
        &out.join("members.tsv"),
        &members,
        &[
            "archive",
            "member_id",
            "role",
            "en_change",
            "kr_change",
            "disposition",
            "jp_payload_other_en_ids",
            "plans",
            "jp_decoded_sha256",
            "en_decoded_sha256",
            "kr_decoded_sha256",
        ],
    )?;
    tsv(
        &out.join("text-pairs.tsv"),
        &pairs,
        &[
            "path",
            "domain",
            "references",
            "en_font_changed",
            "en_text_changed",
            "kr_font_changed",
            "kr_text_changed",
            "translation_inputs",
            "plans",
            "disposition",
        ],
    )?;
    json_file(&out.join("archives.json"), &json!(archives))?;
    json_file(&out.join("scripts.json"), &json!(scripts))?;
    json_file(&out.join("regions.json"), &json!(regions))?;
    Ok(summary)
}

#[cfg(test)]
mod tests;
