use super::*;
fn root() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    (t, p)
}
fn package(name: &str) -> Package {
    BTreeMap::from([(
        "SKILL.md".into(),
        format!("---\nname: {name}\ndescription: Analyze a fixture\n---\nUse fixture inputs.\n")
            .into_bytes(),
    )])
}
#[test]
fn create_discover_edit_conflict_delete_and_custom_enablement() {
    let (_g, global) = root();
    let (_w, workspace) = root();
    fs::create_dir(workspace.join(".loupist")).unwrap();
    create(
        &global,
        "Personal Method",
        "Analyze a fixture",
        "Use fixture inputs.",
    )
    .unwrap();
    let scope = crate::scoped_config::ConfigScope::Project {
        loupist_id: "test".into(),
    };
    let list = || {
        catalog(
            &global,
            Some(&workspace),
            crate::scoped_config::read_scope(
                &global,
                Some(&workspace.join(".loupist")),
                scope.clone(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let listed = list();
    let skill = listed
        .skills
        .into_iter()
        .find(|s| s.id == "personal-method")
        .unwrap();
    assert_eq!(skill.name, "Personal Method");
    assert!(skill.enabled);
    assert!(!skill.editable);
    fs::write(
        workspace.join(".loupist/config.toml"),
        "[skills.personal-method]\nenabled=false\n",
    )
    .unwrap();
    assert!(
        !list()
            .skills
            .into_iter()
            .find(|s| s.id == "personal-method")
            .unwrap()
            .enabled
    );
    let prefs = crate::scoped_config::resolve(&global, Some(&workspace.join(".loupist")))
        .unwrap()
        .skills;
    let snapshot =
        agent_harness::workspace::workspace_snapshot_with_preferences(&workspace, &global, &prefs)
            .unwrap();
    assert!(snapshot.skills.is_empty());
    let config =
        crate::scoped_config::read_scope(&global, None, crate::scoped_config::ConfigScope::Global)
            .unwrap();
    let skill = catalog(&global, None, config)
        .unwrap()
        .skills
        .into_iter()
        .find(|s| s.id == "personal-method")
        .unwrap();
    assert!(skill.editable);
    save(
        &skill,
        &skill.revision,
        "Revised description",
        "Use revised inputs.",
    )
    .unwrap();
    assert!(save(&skill, &skill.revision, "Stale description", "Stale inputs").is_err());
    let source = read(Path::new(skill.path.as_ref().unwrap())).unwrap();
    assert!(source.contains("Revised description"));
    let mut latest = skill.clone();
    latest.revision = crate::util::sha256(source.as_bytes());
    remove(&latest, &latest.revision).unwrap();
    assert!(!Path::new(latest.path.as_ref().unwrap()).exists());
}
#[test]
fn packages_preserve_references_and_never_overwrite_a_skill_or_bundled_name() {
    let (_g, global) = root();
    let mut data = package("custom-method");
    data.insert("references/method.md".into(), b"Method reference".to_vec());
    install(&global, data.clone()).unwrap();
    assert!(install(&global, data).is_err());
    assert_eq!(
        fs::read(global.join("skills/custom-method/references/method.md")).unwrap(),
        b"Method reference"
    );
    assert!(install(&global, package("visualize")).is_err());
    let mut escaped = package("outside");
    escaped.insert("../outside".into(), b"bad".to_vec());
    assert!(install(&global, escaped).is_err());
    assert!(!global.join("outside").exists());
}
#[test]
fn malformed_owned_skill_can_be_inspected_and_removed() {
    let (_g, global) = root();
    install(&global, package("broken-method")).unwrap();
    let path = global.join("skills/broken-method/SKILL.md");
    fs::write(&path, "Unfinished skill without frontmatter").unwrap();
    let config =
        crate::scoped_config::read_scope(&global, None, crate::scoped_config::ConfigScope::Global)
            .unwrap();
    let entry = catalog(&global, None, config)
        .unwrap()
        .skills
        .into_iter()
        .find(|skill| skill.id == "broken-method")
        .unwrap();
    assert!(entry.editable && !entry.enabled && entry.error.is_some());
    let doc = document(entry).unwrap();
    assert!(doc.instructions.contains("Unfinished skill"));
    remove(&doc.skill, &doc.skill.revision).unwrap();
    assert!(!path.exists());
}

#[test]
fn yaml_quoted_and_folded_metadata_is_supported_without_losing_policy() {
    let skill = parse("---\nname: quoted-skill\ndescription: >-\n  Read a filing\n  with evidence\ndisable-model-invocation: true\n---\nInstructions\n").unwrap();
    assert_eq!(skill.description, "Read a filing with evidence");
    assert_eq!(
        skill
            .yaml
            .get(serde_yaml_ng::Value::String(
                "disable-model-invocation".into()
            ))
            .unwrap()
            .as_bool(),
        Some(true)
    );
}
#[test]
fn zip_import_rejects_traversal_and_selects_one_github_subfolder() {
    use std::io::{Cursor, Write};
    fn zip(paths: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in paths {
            zip.start_file(*path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    let skill = package("zip-skill").remove("SKILL.md").unwrap();
    assert!(from_zip(zip(&[("../escaped", b"bad"), ("SKILL.md", &skill)]), None).is_err());
    let data = zip(&[
        ("repo-main/skills/one/SKILL.md", &skill),
        ("repo-main/skills/one/references/a.md", b"reference"),
        ("repo-main/skills/two/SKILL.md", &skill),
    ]);
    assert!(from_zip(data.clone(), None).is_err());
    assert_eq!(
        from_zip(data, Some("skills/one")).unwrap()["references/a.md"],
        b"reference"
    );
}
#[cfg(unix)]
#[test]
fn folder_import_refuses_symlinks_and_install_refuses_linked_destinations() {
    let (_g, global) = root();
    let (_s, source) = root();
    let (_o, outside) = root();
    fs::write(source.join("SKILL.md"), &package("linked")["SKILL.md"]).unwrap();
    std::os::unix::fs::symlink(&outside, source.join("references")).unwrap();
    assert!(from_folder(&source).is_err());
    std::os::unix::fs::symlink(&outside, global.join("skills")).unwrap();
    assert!(install(&global, package("linked")).is_err());
    assert!(!outside.join("linked").exists());
}
