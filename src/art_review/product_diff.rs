//! Payload comparison of two identified development products, including unnamed FAT entries.
use super::*;

pub fn run(
    previous: &Path,
    previous_build: &Path,
    product: &Path,
    build: &Path,
    out: &Path,
) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let old = fs::read(previous)?;
    let new = fs::read(product)?;
    let old_record = fs::read(previous_build)?;
    let new_record = fs::read(build)?;
    let a: Value = serde_json::from_slice(&old_record)?;
    let b: Value = serde_json::from_slice(&new_record)?;
    ensure!(
        a["output_sha256"] == sha(&old) && b["output_sha256"] == sha(&new),
        "product build identity differs"
    );
    ensure!(
        a["source_sha256"].is_string() && a["source_sha256"] == b["source_sha256"],
        "product source identities differ"
    );
    let old_rom = Rom::parse(&old)?;
    let new_rom = Rom::parse(&new)?;
    ensure!(
        old_rom.files.len() == new_rom.files.len(),
        "FAT population differs"
    );
    let mut changed = Vec::new();
    let mut unchanged = 0;
    for (left, right) in old_rom.files.iter().zip(&new_rom.files) {
        ensure!(
            left.id == right.id && left.path == right.path,
            "file identity differs"
        );
        let x = old_rom.data(left);
        let y = new_rom.data(right);
        if x == y {
            unchanged += 1;
            continue;
        }
        let dx = unpack(x)?;
        let dy = unpack(y)?;
        let mut row = json!({"file_id":left.id,"archive":left.path,"previous_stored_sha256":sha(x),"stored_sha256":sha(y),"previous_decoded_sha256":sha(&dx),"decoded_sha256":sha(&dy)});
        if dx.starts_with(b"NARC") && dy.starts_with(b"NARC") {
            let nx = Narc::parse(&dx)?;
            let ny = Narc::parse(&dy)?;
            ensure!(
                nx.members.len() == ny.members.len(),
                "NARC population differs"
            );
            let mut members = Vec::new();
            let mut details = Vec::new();
            for (i, (&mx, &my)) in nx.members.iter().zip(&ny.members).enumerate() {
                if mx != my {
                    members.push(i);
                    details.push(json!({"member":i,"previous_stored_sha256":sha(mx),"stored_sha256":sha(my),"decoded_equal":unpack(mx)? == unpack(my)?}));
                }
            }
            row["members"] = json!(members);
            row["member_details"] = json!(details);
        }
        changed.push(row);
    }
    let report = json!({"previous_product_sha256":sha(&old),"product_sha256":sha(&new),"previous_build_sha256":sha(&old_record),"build_sha256":sha(&new_record),"source_sha256":a["source_sha256"],"changed_files":changed,"unchanged_fat_entries":unchanged,"fat_entries":old_rom.files.len(),"scope":"FAT payload and NARC member comparison, not non-file ROM ranges or visual acceptance","runtime_verified":false});
    fs::create_dir_all(out)?;
    json_file(&out.join("report.json"), &report)?;
    Ok(report)
}
