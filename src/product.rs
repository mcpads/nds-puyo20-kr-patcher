//! One cumulative product build from tracked sources.
//!
//! `config/build.json` names every adopted component: the preparation command
//! and its tracked inputs (translations, art specs, hash-pinned fonts). The build
//! runs each preparation into a temporary directory, composes archive members
//! under explicit ownership, and writes one ROM. It never runs investigation
//! commands, never reads earlier work outputs, and rejects undeclared overlaps.

use crate::{
    archive, assets,
    build::{self, Placement, Plan},
    cli::Cli,
    format::*,
};
use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// Candidate bytes per archive member, labelled with the writing component.
type MemberWriters = BTreeMap<usize, Vec<(String, Vec<u8>)>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    fonts: BTreeMap<String, FontRef>,
    components: Vec<Component>,
    /// Archive member owner where several components write the same member.
    #[serde(default)]
    owners: BTreeMap<String, BTreeMap<usize, String>>,
    /// Banner title specification, relative to this file.
    #[serde(default)]
    banner: Option<String>,
    /// Install the TP4J anti-piracy bypass in the raw ARM9 prefix.
    #[serde(default)]
    anti_piracy_bypass: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontRef {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    id: String,
    command: String,
    #[serde(default)]
    translation: Option<String>,
    #[serde(default)]
    spec: Option<String>,
    #[serde(default)]
    artwork: Option<String>,
    #[serde(default)]
    title_artwork: Option<String>,
    #[serde(default)]
    font: Option<String>,
    #[serde(default)]
    small_font: Option<String>,
    #[serde(default)]
    medium_font: Option<String>,
    #[serde(default)]
    narrow_font: Option<String>,
    #[serde(default)]
    button_font: Option<String>,
    #[serde(default)]
    name_font: Option<String>,
    #[serde(default)]
    body_font: Option<String>,
    #[serde(default)]
    title_font: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    challenge: bool,
    /// Id of an earlier component whose prepared output this one consumes.
    #[serde(default)]
    prepared: Option<String>,
}

/// Commands allowed in a product build. Investigation commands are excluded.
fn is_preparation(command: &str) -> bool {
    (command.starts_with("prepare-") && command != "prepare-menu-poc")
        || command == "copy-dialog-buttons"
}

fn source_path(base: &Path, relative: &str) -> Result<PathBuf> {
    let path = base.join(relative);
    ensure!(path.exists(), "missing build input {}", path.display());
    Ok(path)
}

fn argv(
    component: &Component,
    source: &Path,
    base: &Path,
    fonts: &BTreeMap<String, PathBuf>,
    parts: &Path,
) -> Result<Vec<String>> {
    let font = |id: &str| -> Result<String> {
        Ok(fonts
            .get(id)
            .with_context(|| format!("{}: unknown font {id}", component.id))?
            .to_string_lossy()
            .into_owned())
    };
    let mut args = vec![
        "nds-puyo20".to_owned(),
        component.command.clone(),
        source.to_string_lossy().into_owned(),
    ];
    if let Some(t) = &component.translation {
        args.push("--translation".into());
        args.push(source_path(base, t)?.to_string_lossy().into_owned());
    }
    if let Some(s) = &component.spec {
        args.push("--spec".into());
        args.push(source_path(base, s)?.to_string_lossy().into_owned());
    }
    for (flag, path) in [
        ("--artwork", &component.artwork),
        ("--title-artwork", &component.title_artwork),
    ] {
        if let Some(path) = path {
            args.push(flag.into());
            args.push(source_path(base, path)?.to_string_lossy().into_owned());
        }
    }
    for (flag, value) in [
        ("--font", &component.font),
        ("--small-font", &component.small_font),
        ("--medium-font", &component.medium_font),
        ("--narrow-font", &component.narrow_font),
        ("--button-font", &component.button_font),
        ("--name-font", &component.name_font),
        ("--body-font", &component.body_font),
        ("--title-font", &component.title_font),
    ] {
        if let Some(id) = value {
            args.push(flag.into());
            args.push(font(id)?);
        }
    }
    if let Some(surface) = &component.surface {
        args.push("--surface".into());
        args.push(surface.clone());
    }
    if component.challenge {
        args.push("--challenge".into());
    }
    if let Some(id) = &component.prepared {
        let dir = parts.join(id);
        ensure!(
            dir.exists(),
            "{}: prepared input {id} not built yet",
            component.id
        );
        args.push("--prepared".into());
        args.push(dir.to_string_lossy().into_owned());
    }
    args.push("--out".into());
    args.push(parts.join(&component.id).to_string_lossy().into_owned());
    Ok(args)
}

/// Plans a preparation wrote at its root. A command without root plans writes
/// one plan per numbered subdirectory (`0/plan.json`, `1/plan.json`, ...).
fn component_plans(dir: &Path) -> Result<Vec<(PathBuf, Plan)>> {
    let is_plan = |p: &Path| {
        p.is_file()
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("plan.json"))
    };
    let mut plans = Vec::new();
    let mut names: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_plan(p))
        .collect();
    if names.is_empty() {
        names = fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            })
            .map(|d| d.join("plan.json"))
            .filter(|p| is_plan(p))
            .collect();
    }
    names.sort();
    for path in names {
        let plan: Plan = serde_json::from_slice(&fs::read(&path)?)
            .with_context(|| format!("unreadable plan {}", path.display()))?;
        plans.push((path, plan));
    }
    ensure!(
        !plans.is_empty(),
        "component wrote no plan: {}",
        dir.display()
    );
    Ok(plans)
}

/// `rom_bytes` must already be verified against the registered source profile.
pub fn run(source: &Path, rom_bytes: &[u8], spec_path: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "output exists");
    let spec_bytes = fs::read(spec_path)?;
    let spec: Spec = serde_json::from_slice(&spec_bytes)?;
    let base = spec_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let rom = Rom::parse(rom_bytes)?;

    let mut fonts = BTreeMap::new();
    for (id, f) in &spec.fonts {
        let path = source_path(&base, &f.path)?;
        ensure!(
            sha(&fs::read(&path)?) == f.sha256,
            "font {id} identity mismatch: {}",
            path.display()
        );
        fonts.insert(id.clone(), path);
    }
    let mut ids = BTreeSet::new();
    for c in &spec.components {
        ensure!(ids.insert(c.id.clone()), "duplicate component {}", c.id);
        ensure!(
            is_preparation(&c.command),
            "{}: not a preparation command",
            c.id
        );
    }

    let parts = out.with_file_name(format!(
        ".{}.parts",
        out.file_name().and_then(|n| n.to_str()).unwrap_or("build")
    ));
    ensure!(!parts.exists(), "stale parts directory {}", parts.display());
    fs::create_dir_all(&parts)?;
    let result = compose(
        &rom,
        rom_bytes,
        &spec,
        &spec_bytes,
        &base,
        &fonts,
        source,
        &parts,
        out,
    );
    fs::remove_dir_all(&parts)?;
    result
}

#[allow(clippy::too_many_arguments)]
fn compose(
    rom: &Rom,
    rom_bytes: &[u8],
    spec: &Spec,
    spec_bytes: &[u8],
    base: &Path,
    fonts: &BTreeMap<String, PathBuf>,
    source: &Path,
    parts: &Path,
    out: &Path,
) -> Result<Value> {
    let mut components = Vec::new();
    for c in &spec.components {
        let args = argv(c, source, base, fonts, parts)?;
        let cli = Cli::try_parse_from(&args).with_context(|| format!("{}: arguments", c.id))?;
        crate::commands::run(cli.command).with_context(|| format!("component {}", c.id))?;
        let inputs: Vec<Value> = [&c.translation, &c.spec, &c.artwork, &c.title_artwork]
            .into_iter()
            .flatten()
            .map(|p| -> Result<Value> {
                Ok(json!({"path":p,"sha256":sha(&fs::read(base.join(p))?)}))
            })
            .collect::<Result<_>>()?;
        components.push(json!({"id":c.id,"command":c.command,"inputs":inputs}));
    }

    // Collect every write, then resolve archive members under explicit ownership.
    let mut members: BTreeMap<String, MemberWriters> = BTreeMap::new();
    let mut whole: BTreeMap<String, (String, build::Replacement)> = BTreeMap::new();
    let mut arm9: Option<(String, build::Arm9Replacement)> = None;
    for c in &spec.components {
        for (path, plan) in component_plans(&parts.join(&c.id))? {
            ensure!(
                plan.source_sha256 == sha(rom_bytes),
                "{}: plan source mismatch",
                c.id
            );
            let dir = path.parent().unwrap_or(Path::new("."));
            for mut r in plan.replacements {
                let data = fs::read(dir.join(&r.input))?;
                ensure!(sha(&data) == r.input_sha256, "{}: input identity", c.id);
                let original = rom.data(rom.file(&r.file)?);
                ensure!(
                    sha(original) == r.expected_sha256,
                    "{}: source identity",
                    c.id
                );
                if r.file.ends_with(".narc") && r.placement == Placement::Original {
                    let before = Narc::parse(original)?;
                    let after = Narc::parse(&data)?;
                    ensure!(
                        before.members.len() == after.members.len(),
                        "{}: archive population changed",
                        c.id
                    );
                    let mut changes = BTreeMap::new();
                    for (id, (&old, &new)) in before.members.iter().zip(&after.members).enumerate()
                    {
                        if old != new {
                            changes.insert(id, new.to_vec());
                        }
                    }
                    ensure!(
                        archive::replace(original, &changes)? == data,
                        "{}: protected archive bytes changed in {}",
                        c.id,
                        r.file
                    );
                    let slot = members.entry(r.file.clone()).or_default();
                    for (id, bytes) in changes {
                        slot.entry(id).or_default().push((c.id.clone(), bytes));
                    }
                } else {
                    r.input = dir.join(&r.input).to_string_lossy().into_owned();
                    if let Some((other, _)) = whole.get(&r.file) {
                        bail!("{} and {} both replace {}", other, c.id, r.file);
                    }
                    whole.insert(r.file.clone(), (c.id.clone(), r));
                }
            }
            if let Some(mut a) = plan.arm9 {
                a.input = dir.join(&a.input).to_string_lossy().into_owned();
                if let Some((other, _)) = &arm9 {
                    bail!("{} and {} both replace ARM9", other, c.id);
                }
                arm9 = Some((c.id.clone(), a));
            }
        }
    }

    let mut used_owners = BTreeSet::new();
    let mut resolutions = Vec::new();
    let merged_dir = parts.join(".merged");
    fs::create_dir_all(&merged_dir)?;
    let mut replacements = Vec::new();
    let mut ownership = Vec::new();
    for (i, (file, slots)) in members.iter().enumerate() {
        ensure!(
            !whole.contains_key(file),
            "{file} is both replaced and member-edited"
        );
        let mut chosen = BTreeMap::new();
        for (&id, writers) in slots {
            let bytes = if writers.len() == 1 {
                writers[0].1.clone()
            } else {
                let owner = spec
                    .owners
                    .get(file)
                    .and_then(|m| m.get(&id))
                    .with_context(|| {
                        format!(
                            "undeclared overlap: {file} member {id} written by {:?}",
                            writers.iter().map(|w| &w.0).collect::<Vec<_>>()
                        )
                    })?;
                used_owners.insert((file.clone(), id));
                let (_, bytes) = writers
                    .iter()
                    .find(|w| &w.0 == owner)
                    .with_context(|| format!("owner {owner} does not write {file} member {id}"))?;
                resolutions.push(json!({"file":file,"member":id,"owner":owner,"writers":writers.iter().map(|w| &w.0).collect::<Vec<_>>()}));
                bytes.clone()
            };
            ownership.push(json!({"file":file,"member":id,"writers":writers.iter().map(|w| &w.0).collect::<Vec<_>>()}));
            chosen.insert(id, bytes);
        }
        let original = rom.data(rom.file(file)?);
        let data = archive::replace(original, &chosen)?;
        let name = merged_dir.join(format!("{i}.narc"));
        fs::write(&name, &data)?;
        replacements.push(build::Replacement {
            file: file.clone(),
            expected_sha256: sha(original),
            input: name.to_string_lossy().into_owned(),
            input_sha256: sha(&data),
            placement: Placement::Original,
        });
    }
    for (file, members) in &spec.owners {
        for id in members.keys() {
            ensure!(
                used_owners.contains(&(file.clone(), *id)),
                "owner declared without overlap: {file} member {id}"
            );
        }
    }
    replacements.extend(whole.into_values().map(|(_, r)| r));
    let plan = Plan {
        source_sha256: sha(rom_bytes),
        replacements,
        arm9: arm9.map(|(_, a)| a),
        banner: spec
            .banner
            .as_ref()
            .map(|p| -> Result<build::BannerTitles> {
                Ok(serde_json::from_slice(&fs::read(source_path(base, p)?)?)?)
            })
            .transpose()?,
        anti_piracy_bypass: spec.anti_piracy_bypass,
    };
    let (output, mut report) = build::build(rom_bytes, &plan, Path::new("."))?;
    report["spec_sha256"] = json!(sha(spec_bytes));
    report["components"] = json!(components);
    report["member_writers"] = json!(ownership);
    report["ownership_resolutions"] = json!(resolutions);
    fs::create_dir_all(out)?;
    fs::write(out.join("development.nds"), &output)?;
    ensure!(
        sha(&fs::read(out.join("development.nds"))?) == sha(&output),
        "output readback mismatch"
    );
    assets::json_file(&out.join("build.json"), &report)?;
    Ok(
        json!({"output_sha256":sha(&output),"components":spec.components.len(),"ownership_resolutions":report["ownership_resolutions"].as_array().map(Vec::len)}),
    )
}

#[cfg(test)]
mod tests;
