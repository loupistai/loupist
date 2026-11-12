use super::*;

fn directory() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
fn write(root: &Path, source: &str) {
    std::fs::write(root.join(FILE_NAME), source).unwrap();
}
fn scope() -> ConfigScope {
    ConfigScope::Project {
        loupist_id: "research".into(),
    }
}
fn update(
    global: &Path,
    project: Option<&Path>,
    scope: ConfigScope,
    set: ConfigDocument,
    unset: Vec<Vec<String>>,
) -> ConfigSnapshot {
    let prior = read_scope(global, project, scope.clone()).unwrap();
    update_scope(
        global,
        project,
        &ConfigUpdateRequest {
            scope,
            expected_revision: prior.revision,
            set,
            unset,
        },
    )
    .unwrap()
}
fn patch(source: &str) -> ConfigDocument {
    toml_edit::de::from_str(source).unwrap()
}

#[test]
fn merges_each_field_and_records_its_source() {
    let global = directory();
    let project = directory();
    write(global.path(), "[appearance]\ntheme = 'dark'\n[chat]\nmodel_profile_id = 'provider/model'\nthinking_level = 'high'\n[chat.run_options]\na = 'global'\nb = 'global'\n[pi.retry]\nmax_retries = 5\nbase_delay_ms = 100\n");
    write(project.path(), "[chat]\nthinking_level = 'low'\n[chat.run_options]\nb = 'project'\n[pi.retry]\nenabled = false\n");
    let result = read_scope(global.path(), Some(project.path()), scope()).unwrap();
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        result.effective.chat.model_profile_id.as_deref(),
        Some("provider/model")
    );
    assert_eq!(result.effective.chat.thinking_level.as_deref(), Some("low"));
    assert_eq!(result.effective.chat.run_options["a"], "global");
    assert_eq!(result.effective.chat.run_options["b"], "project");
    assert!(!result.effective.pi.retry.enabled);
    assert_eq!(result.effective.pi.retry.max_retries, 5);
    assert_eq!(result.sources["chat.model_profile_id"], "global");
    assert_eq!(result.sources["chat.thinking_level"], "project");
    assert_eq!(
        result.sources["pi.compaction.keep_recent_tokens"],
        "default"
    );
    let second = directory();
    assert!(
        resolve(global.path(), Some(second.path()))
            .unwrap()
            .pi
            .retry
            .enabled
    );
}

#[test]
fn saves_toml_preserving_comments_and_unsets_to_inherit() {
    let global = directory();
    let project = directory();
    write(global.path(), "[pi.retry]\nmax_retries = 4\n");
    write(project.path(), "# Research configuration\n[pi.retry] # Project policy\nmax_retries = 1 # Cost limit\nbase_delay_ms = 20 # Backoff\n");
    let result = update(
        global.path(),
        Some(project.path()),
        scope(),
        patch("[pi.retry]\nbase_delay_ms = 55"),
        vec![vec!["pi".into(), "retry".into(), "max_retries".into()]],
    );
    assert_eq!(result.effective.pi.retry.max_retries, 4);
    assert_eq!(result.effective.pi.retry.base_delay_ms, 55);
    assert_eq!(result.sources["pi.retry.max_retries"], "global");
    let text = std::fs::read_to_string(project.path().join(FILE_NAME)).unwrap();
    assert!(text.contains("# Research configuration"));
    assert!(text.contains("# Project policy"));
    assert!(text.contains("base_delay_ms = 55 # Backoff"));
    assert!(!text.contains("max_retries"));
}

#[test]
fn array_setting_is_replaced_and_ui_model_updates_share_the_file() {
    let global = directory();
    write(
        global.path(),
        "[models]\nhidden_model_profile_ids = ['first', 'second'] # hidden\n",
    );
    update(
        global.path(),
        None,
        ConfigScope::Global,
        patch("[models]\nhidden_model_profile_ids = ['third']"),
        vec![],
    );
    assert_eq!(
        read_hidden_model_profile_ids(global.path()).unwrap(),
        ["third"]
    );
    set_model_visible(global.path(), "fourth", false).unwrap();
    set_model_visible(global.path(), "third", true).unwrap();
    assert_eq!(
        read_hidden_model_profile_ids(global.path()).unwrap(),
        ["fourth"]
    );
    assert!(std::fs::read_to_string(global.path().join(FILE_NAME))
        .unwrap()
        .contains("# hidden"));
}

#[test]
fn stale_revision_does_not_overwrite_external_edits() {
    let global = directory();
    let snapshot = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    write(
        global.path(),
        "# Outside change\n[appearance]\ntheme = 'dark'\n",
    );
    let error = update_scope(
        global.path(),
        None,
        &ConfigUpdateRequest {
            scope: ConfigScope::Global,
            expected_revision: snapshot.revision,
            set: patch("[appearance]\ntheme = 'light'"),
            unset: vec![],
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "conflict");
    assert_eq!(
        resolve(global.path(), None).unwrap().appearance.theme,
        "dark"
    );
    assert!(std::fs::read_to_string(global.path().join(FILE_NAME))
        .unwrap()
        .contains("# Outside change"));
}

#[test]
fn invalid_file_reports_location_and_blocks_execution_without_clobbering() {
    let global = directory();
    let project = directory();
    write(global.path(), "[appearance]\ntheme = 'dark'\n");
    for invalid in [
        "[pi.retry]\nmax_retries = 11\n",
        "[pi.retry]\nenabled = \n",
        "[pi]\ntransport = 'http'\n",
        "[appearance]\ntheme = 'dark'\n",
        "[pi.compaction]\nreserve_tokens = 9007199254740992\n",
    ] {
        write(project.path(), invalid);
        let snapshot = read_scope(global.path(), Some(project.path()), scope()).unwrap();
        assert!(snapshot.explicit.is_none(), "{invalid}");
        assert_eq!(snapshot.diagnostics.len(), 1, "{invalid}");
        assert!(snapshot.diagnostics[0].line.is_some(), "{invalid}");
        assert_eq!(snapshot.effective.appearance.theme, "dark");
        assert!(resolve(global.path(), Some(project.path())).is_err());
        assert!(update_scope(
            global.path(),
            Some(project.path()),
            &ConfigUpdateRequest {
                scope: scope(),
                expected_revision: snapshot.revision,
                set: patch("[pi.retry]\nmax_retries=2"),
                unset: vec![]
            }
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(project.path().join(FILE_NAME)).unwrap(),
            invalid
        );
    }
}

#[test]
fn accepts_supported_numeric_boundaries_and_rejects_project_global_fields() {
    let global = directory();
    update(global.path(), None, ConfigScope::Global, patch("[pi.compaction]\nreserve_tokens = 9007199254740991\nkeep_recent_tokens = 0\n[pi.retry]\nmax_retries = 10\nbase_delay_ms = 300000\nmax_agent_delay_ms = 0\n"), vec![]);
    let config = resolve(global.path(), None).unwrap();
    assert_eq!(config.pi.compaction.reserve_tokens, MAX_SAFE_INTEGER);
    for source in [
        "[appearance]\ntheme='dark'",
        "[models]\nhidden_model_profile_ids=[]",
        "[connectors]",
    ] {
        assert!(validate(&patch(source), &scope()).is_err());
    }
}

#[test]
fn execution_digest_tracks_settings_without_presentation_changes() {
    let global = directory();
    let first = resolve(global.path(), None).unwrap().execution_digest();
    update(
        global.path(),
        None,
        ConfigScope::Global,
        patch("[appearance]\ntheme='dark'"),
        vec![],
    );
    assert_eq!(
        first,
        resolve(global.path(), None).unwrap().execution_digest()
    );
    update(
        global.path(),
        None,
        ConfigScope::Global,
        patch("[pi.retry]\nenabled=false"),
        vec![],
    );
    assert_ne!(
        first,
        resolve(global.path(), None).unwrap().execution_digest()
    );
}

#[test]
fn bounded_reads_reject_special_or_large_files_and_writes_leave_no_temporary_files() {
    let global = directory();
    write(global.path(), &" ".repeat(MAX_CONFIG_BYTES + 1));
    assert!(resolve(global.path(), None).is_err());
    std::fs::remove_file(global.path().join(FILE_NAME)).unwrap();
    update(
        global.path(),
        None,
        ConfigScope::Global,
        patch("[pi.retry]\nenabled=false"),
        vec![],
    );
    assert_eq!(std::fs::read_dir(global.path()).unwrap().count(), 1);
    let config: ConfigDocument =
        toml_edit::de::from_str(&std::fs::read_to_string(global.path().join(FILE_NAME)).unwrap())
            .unwrap();
    assert_eq!(config.pi.unwrap().retry.unwrap().enabled, Some(false));
}

#[cfg(unix)]
#[test]
fn refuses_symbolic_link_files_and_directories_without_touching_their_targets() {
    use std::os::unix::fs::symlink;
    let global = directory();
    let outside = directory();
    write(outside.path(), "[appearance]\ntheme='dark'\n");
    symlink(
        outside.path().join(FILE_NAME),
        global.path().join(FILE_NAME),
    )
    .unwrap();
    let snapshot = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    assert!(!snapshot.diagnostics.is_empty());
    assert!(update_scope(
        global.path(),
        None,
        &ConfigUpdateRequest {
            scope: ConfigScope::Global,
            expected_revision: snapshot.revision,
            set: patch("[appearance]\ntheme='light'"),
            unset: vec![]
        }
    )
    .is_err());
    assert_eq!(
        resolve(outside.path(), None).unwrap().appearance.theme,
        "dark"
    );
    let linked = global.path().join("linked");
    symlink(outside.path(), &linked).unwrap();
    assert!(resolve(&linked, None).is_err());
}

#[test]
fn scope_only_revision_does_not_conflate_global_and_project_edits() {
    let global = directory();
    let project = directory();
    let before = read_scope(global.path(), Some(project.path()), scope()).unwrap();
    write(global.path(), "[pi.retry]\nmax_retries=8\n");
    let after = update_scope(
        global.path(),
        Some(project.path()),
        &ConfigUpdateRequest {
            scope: scope(),
            expected_revision: before.revision,
            set: patch("[pi.retry]\nenabled=false"),
            unset: vec![],
        },
    )
    .unwrap();
    assert_eq!(after.effective.pi.retry.max_retries, 8);
    assert!(!after.effective.pi.retry.enabled);
}

#[test]
fn connector_helper_preserves_table_and_retained_field_comments() {
    let global = directory();
    write(global.path(), "# Global setup\n[connectors.\"sec.edgar\"] # SEC identity\ncontact_email = 'old@example.invalid' # Required by SEC\n");
    write_global_connector_settings(
        global.path(),
        "sec.edgar",
        &BTreeMap::from([("contact_email".into(), "new@example.invalid".into())]),
    )
    .unwrap();
    let source = std::fs::read_to_string(global.path().join(FILE_NAME)).unwrap();
    assert!(source.contains("# Global setup"));
    assert!(source.contains("# SEC identity"));
    assert!(source.contains("# Required by SEC"));
    assert_eq!(
        read_global_connector_settings(global.path(), "sec.edgar").unwrap()["contact_email"],
        "new@example.invalid"
    );
    write_global_connector_settings(global.path(), "sec.edgar", &BTreeMap::new()).unwrap();
    let source = std::fs::read_to_string(global.path().join(FILE_NAME)).unwrap();
    assert!(source.contains("# SEC identity"));
    assert!(!source.contains("contact_email"));
    assert!(read_global_connector_settings(global.path(), "sec.edgar")
        .unwrap()
        .is_empty());
}

#[test]
fn semantic_connector_error_reports_the_field_location_without_its_value() {
    let global = directory();
    write(
        global.path(),
        "# Contact identity\n[connectors.\"sec.edgar\"]\ncontact_email = 'invalid-contact-value'\n",
    );
    let snapshot = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].line, Some(3));
    assert_eq!(snapshot.diagnostics[0].column, Some(1));
    assert!(!snapshot.diagnostics[0]
        .message
        .contains("invalid-contact-value"));
}

#[test]
fn skill_preferences_inherit_individually_and_reset_without_pinning_other_skills() {
    let global = directory();
    let project = directory();
    let other = directory();
    write(
        global.path(),
        "[skills.research]\nenabled=false\n[skills.pdf]\nenabled=false\n",
    );
    write(project.path(), "# Local workflows\n[pi.retry]\nmax_retries=1\n[skills.research]\nenabled=true # Override\n");
    let result = resolve(global.path(), Some(project.path())).unwrap();
    assert!(result.enabled_skill_ids().contains(&"research".into()));
    assert!(!result.enabled_skill_ids().contains(&"pdf".into()));
    assert!(result.enabled_skill_ids().contains(&"valuation".into()));
    assert!(!resolve(global.path(), Some(other.path())).unwrap().skills["research"].enabled);
    let before = result.execution_digest();
    let result = update(
        global.path(),
        Some(project.path()),
        scope(),
        ConfigDocument::default(),
        vec![vec!["skills".into(), "research".into(), "enabled".into()]],
    );
    assert!(!result.effective.skills["research"].enabled);
    assert_eq!(result.sources["skills.research.enabled"], "global");
    assert_ne!(before, result.effective.execution_digest());
    assert_eq!(result.effective.pi.retry.max_retries, 1);
    assert!(std::fs::read_to_string(project.path().join(FILE_NAME))
        .unwrap()
        .contains("# Local workflows"));
    // A later global change reaches both Loupists after the local reset.
    write(
        global.path(),
        "[skills.research]\nenabled=true\n[skills.pdf]\nenabled=true\n",
    );
    assert!(resolve(global.path(), Some(project.path())).unwrap().skills["pdf"].enabled);
    assert!(resolve(global.path(), Some(other.path())).unwrap().skills["pdf"].enabled);
}

#[test]
fn skill_authoring_is_off_by_default_and_an_explicit_enable_wins_until_reset() {
    let global = directory();
    let project = directory();
    let enabled =
        |config: &ResolvedConfig| config.enabled_skill_ids().contains(&"skill-creator".into());
    assert!(!enabled(
        &resolve(global.path(), Some(project.path())).unwrap()
    ));
    write(global.path(), "[skills.skill-creator]\nenabled=false\n");
    write(project.path(), "[skills.skill-creator]\nenabled=true\n");
    assert!(enabled(
        &resolve(global.path(), Some(project.path())).unwrap()
    ));
    let reset = update(
        global.path(),
        Some(project.path()),
        scope(),
        ConfigDocument::default(),
        vec![vec![
            "skills".into(),
            "skill-creator".into(),
            "enabled".into(),
        ]],
    );
    assert_eq!(reset.sources["skills.skill-creator.enabled"], "global");
    assert!(!enabled(&reset.effective));
    assert!(!reset
        .effective
        .enabled_skill_ids()
        .contains(&"skill-installer".into()));
}

#[test]
fn fixed_or_invalid_skill_preferences_cannot_change_the_runtime_surface() {
    let global = directory();
    let surface = resolve(global.path(), None).unwrap().enabled_skill_ids();
    for id in ["world", "lens", "journal", "visualize"] {
        // A Skill that is always on in this version is ignored with a warning.
        let source = format!("# Older choice\n[skills.{id}]\nenabled=false\n");
        write(global.path(), &source);
        let snapshot = read_scope(global.path(), None, ConfigScope::Global).unwrap();
        assert!(snapshot.diagnostics.is_empty(), "{id}");
        assert_eq!(snapshot.warnings.len(), 1, "{id}");
        assert_eq!(snapshot.warnings[0].line, Some(2), "{id}");
        assert!(!snapshot.explicit.unwrap().skills.unwrap().contains_key(id));
        assert_eq!(
            resolve(global.path(), None).unwrap().enabled_skill_ids(),
            surface
        );
        // Saves keep working, keep the ignored text, and can reset it.
        let saved = update(
            global.path(),
            None,
            ConfigScope::Global,
            patch("[appearance]\ntheme = 'dark'"),
            vec![],
        );
        assert_eq!(saved.effective.appearance.theme, "dark");
        let text = std::fs::read_to_string(global.path().join(FILE_NAME)).unwrap();
        assert!(
            text.contains(&format!("[skills.{id}]\nenabled=false")),
            "{text}"
        );
        let reset = update(
            global.path(),
            None,
            ConfigScope::Global,
            ConfigDocument::default(),
            vec![vec!["skills".into(), id.into()]],
        );
        assert!(reset.warnings.is_empty(), "{id}");
    }
    for invalid in [
        "[skills.invalid_name]\nenabled=false\n",
        "[skills.pdf]\nenabled='false'\n",
    ] {
        write(global.path(), invalid);
        assert!(resolve(global.path(), None).is_err(), "{invalid}");
    }
}

#[test]
fn connectors_the_catalog_no_longer_has_are_ignored_and_never_block_saves() {
    let global = directory();
    let source = "[connectors.\"retired.connector\"]\nregion = 'eu'\n\n[connectors.\"sec.edgar\"]\ncontact_email = 'research@example.invalid'\nretired_field = 'kept'\n";
    write(global.path(), source);
    let snapshot = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    assert!(snapshot.diagnostics.is_empty());
    assert_eq!(
        snapshot
            .warnings
            .iter()
            .map(|warning| warning.line)
            .collect::<Vec<_>>(),
        [Some(1), Some(6)]
    );
    assert!(!snapshot.warnings[1].message.contains("kept"));
    let effective = resolve(global.path(), None).unwrap();
    assert_eq!(
        effective.connectors,
        BTreeMap::from([(
            "sec.edgar".to_owned(),
            BTreeMap::from([(
                "contact_email".to_owned(),
                "research@example.invalid".to_owned()
            )])
        )])
    );

    // Settings and connector saves keep working and keep the ignored text.
    update(
        global.path(),
        None,
        ConfigScope::Global,
        patch("[appearance]\ntheme = 'dark'"),
        vec![],
    );
    write_global_connector_settings(
        global.path(),
        "sec.edgar",
        &BTreeMap::from([("contact_email".into(), "new@example.invalid".into())]),
    )
    .unwrap();
    let text = std::fs::read_to_string(global.path().join(FILE_NAME)).unwrap();
    assert!(text.contains("[connectors.\"retired.connector\"]\nregion = 'eu'"));
    assert!(text.contains("retired_field = 'kept'"));
    assert_eq!(
        read_global_connector_settings(global.path(), "sec.edgar").unwrap(),
        BTreeMap::from([("contact_email".into(), "new@example.invalid".into())])
    );

    // A request names only settings this version has.
    let prior = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    assert!(update_scope(
        global.path(),
        None,
        &ConfigUpdateRequest {
            scope: ConfigScope::Global,
            expected_revision: prior.revision,
            set: patch("[connectors.\"retired.connector\"]\nregion = 'us'"),
            unset: vec![],
        },
    )
    .is_err());

    // Credentials never belong in the file, whether the connector is known or not.
    for credential in [
        "[connectors.\"retired.connector\"]\napi_key = 'fixture'\n",
        "[connectors.\"koreainvestment.market-data\"]\napp_key = 'fixture'\n",
    ] {
        write(global.path(), credential);
        assert!(resolve(global.path(), None).is_err(), "{credential}");
    }
}

#[test]
fn chat_memory_defaults_inherit_and_do_not_require_read_permission_to_save() {
    let global = directory();
    let project = directory();
    write(
        global.path(),
        "[chat]\nuse_memory=false\nupdate_memory=false\n",
    );
    write(project.path(), "[chat]\nupdate_memory=true\n");
    let result = resolve(global.path(), Some(project.path())).unwrap();
    assert!(!result.chat.use_memory);
    assert!(result.chat.update_memory);
    let result = update(
        global.path(),
        Some(project.path()),
        scope(),
        ConfigDocument::default(),
        vec![vec!["chat".into(), "update_memory".into()]],
    );
    assert!(!result.effective.chat.update_memory);
    assert_eq!(result.sources["chat.update_memory"], "global");
}

#[test]
fn appearance_language_is_global_only_validated_and_unsets_to_system() {
    let global = directory();
    let initial = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    assert_eq!(initial.effective.appearance.language, "system");
    let result = update(
        global.path(),
        None,
        ConfigScope::Global,
        patch("[appearance]\nlanguage = 'ko'"),
        vec![],
    );
    assert_eq!(result.effective.appearance.language, "ko");
    assert_eq!(result.sources["appearance.language"], "global");
    let result = update(
        global.path(),
        None,
        ConfigScope::Global,
        ConfigDocument::default(),
        vec![vec!["appearance".into(), "language".into()]],
    );
    assert_eq!(result.effective.appearance.language, "system");
    assert_eq!(result.sources["appearance.language"], "default");

    for language in ["en-XA", "fr", ""] {
        let config = patch(&format!("[appearance]\nlanguage = '{language}'"));
        assert!(validate(&config, &ConfigScope::Global).is_err());
    }
    assert!(validate(&patch("[appearance]\nlanguage = 'ko'"), &scope()).is_err());
    write(global.path(), "[appearance]\nlanguage = 'en-XA'");
    let result = read_scope(global.path(), None, ConfigScope::Global).unwrap();
    assert_eq!(result.effective.appearance.language, "system");
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn cached_configuration_refreshes_on_external_write_replacement_and_removal() {
    let root = directory();
    let path = root.path().join(FILE_NAME);
    write(root.path(), "[chat]\nuse_memory = false\n");
    let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(at))
        .unwrap();
    assert!(!resolve(root.path(), None).unwrap().chat.use_memory);
    assert!(!resolve(root.path(), None).unwrap().chat.use_memory);
    write(root.path(), "[chat]\nuse_memory = true\n");
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(at + std::time::Duration::from_secs(1)))
        .unwrap();
    assert!(resolve(root.path(), None).unwrap().chat.use_memory);
    // Same mtime and byte count, different inode: do not reuse the old parse.
    let replacement = root.path().join("replacement");
    std::fs::write(&replacement, "[chat]\nuse_memory = false\n").unwrap();
    File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(at))
        .unwrap();
    std::fs::rename(replacement, &path).unwrap();
    assert!(!resolve(root.path(), None).unwrap().chat.use_memory);
    std::fs::remove_file(&path).unwrap();
    assert!(resolve(root.path(), None).unwrap().chat.use_memory);
}

#[test]
fn cached_configuration_keeps_global_and_project_validation_separate() {
    let root = directory();
    write(root.path(), "[appearance]\ntheme = 'dark'\n");
    assert!(load(root.path(), &ConfigScope::Global)
        .diagnostics
        .is_empty());
    assert!(!load(root.path(), &scope()).diagnostics.is_empty());
    #[cfg(unix)]
    {
        let path = root.path().join(FILE_NAME);
        std::fs::remove_file(&path).unwrap();
        let other = directory();
        write(other.path(), "[appearance]\ntheme = 'light'\n");
        std::os::unix::fs::symlink(other.path().join(FILE_NAME), &path).unwrap();
        assert!(!load(root.path(), &ConfigScope::Global)
            .diagnostics
            .is_empty());
    }
}

#[test]
fn cache_rejects_same_size_same_mtime_replacement() {
    let root = directory();
    let path = root.path().join(FILE_NAME);
    let timestamp = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
    write(root.path(), "[chat]\nthinking_level = 'low'\n");
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(timestamp))
        .unwrap();
    assert_eq!(
        resolve(root.path(), None)
            .unwrap()
            .chat
            .thinking_level
            .as_deref(),
        Some("low")
    );
    let replacement = root.path().join("replacement");
    std::fs::write(&replacement, "[chat]\nthinking_level = 'off'\n").unwrap();
    File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(timestamp))
        .unwrap();
    std::fs::rename(replacement, path).unwrap();
    assert_eq!(
        resolve(root.path(), None)
            .unwrap()
            .chat
            .thinking_level
            .as_deref(),
        Some("off")
    );
}
