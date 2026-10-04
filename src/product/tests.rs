use super::*;

fn spec() -> (PathBuf, Spec) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/build.json");
    let spec: Spec = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    (path.parent().unwrap().to_path_buf(), spec)
}

#[test]
#[ignore = "requires the translation and art sources under assets/translations and assets/art"]
fn build_spec_names_only_tracked_preparation_inputs() {
    let (base, spec) = spec();
    let mut ids = BTreeSet::new();
    for c in &spec.components {
        assert!(ids.insert(c.id.clone()), "duplicate component {}", c.id);
        assert!(is_preparation(&c.command), "{} runs {}", c.id, c.command);
        for p in [&c.translation, &c.spec].into_iter().flatten() {
            let path = base.join(p);
            assert!(path.exists(), "{}: missing {}", c.id, path.display());
            assert!(
                path.canonicalize().unwrap().starts_with(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .canonicalize()
                        .unwrap()
                ),
                "{}: source outside the repository",
                c.id
            );
        }
        for f in [
            &c.font,
            &c.small_font,
            &c.medium_font,
            &c.narrow_font,
            &c.button_font,
        ]
        .into_iter()
        .flatten()
        {
            assert!(spec.fonts.contains_key(f), "{}: unknown font {f}", c.id);
        }
        if let Some(p) = &c.prepared {
            assert!(
                ids.contains(p) && p != &c.id,
                "{}: prepared must name an earlier component",
                c.id
            );
        }
    }
    for members in spec.owners.values() {
        for owner in members.values() {
            assert!(ids.contains(owner), "unknown owner {owner}");
        }
    }
}

#[test]
fn investigation_commands_are_not_preparations() {
    assert!(is_preparation("prepare-labels"));
    assert!(is_preparation("copy-dialog-buttons"));
    assert!(!is_preparation("prepare-menu-poc"));
    assert!(!is_preparation("inspect-textures"));
    assert!(!is_preparation("build"));
}
