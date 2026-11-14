use std::{
    collections::BTreeSet,
    sync::{Arc, Barrier},
};

use super::{
    connector_config::{configure_connector, valid_setup_value},
    credentials::{
        credential_entry, credential_verification_outcome, delete_credential_for_state,
        validate_credential_secrets, validate_required_credential_fields,
    },
    *,
};

fn seed_loupist(state: &AppState, id: &str) {
    state
        .store
        .create_loupist(&crate::test_fixtures::loupist_profile(id))
        .unwrap();
}

fn has_active_credential(state: &AppState, entry_id: &str) -> bool {
    crate::finance_credentials::status(&state.credential_environment, entry_id)
        .unwrap()
        .active
}

fn binding(
    state: &AppState,
    loupist_id: &str,
    entry_id: &str,
) -> crate::domain::LoupistCapabilityBinding {
    state
        .store
        .get_loupist_capability(loupist_id, entry_id)
        .unwrap()
        .unwrap()
}

fn set_binding(
    state: &AppState,
    loupist_id: &str,
    entry_id: &str,
    enabled: bool,
) -> Result<(), CommandError> {
    save_binding(
        LoupistCapabilityRequest {
            loupist_id: loupist_id.to_owned(),
            entry_id: entry_id.to_owned(),
        },
        state,
        enabled,
    )
    .map(|_| ())
}

fn configure(
    state: &AppState,
    entry_id: &str,
    field: &str,
    value: &str,
) -> Result<MarketplaceConnectorStatusDto, CommandError> {
    configure_connector(
        MarketplaceConnectorConfigureRequest {
            entry_id: entry_id.to_owned(),
            config: BTreeMap::from([(field.to_owned(), value.to_owned())]),
        },
        state,
    )
}

fn execute_ids(state: &AppState, loupist_id: &str) -> Vec<String> {
    crate::commands::enabled_execute_capability_ids(state, loupist_id).unwrap()
}

fn promote_test_credential(state: &AppState, entry_id: &str, secret: &str, timestamp: i64) {
    crate::finance_credentials::stage(
        &state.credential_environment,
        entry_id,
        &BTreeMap::from([("api_key".to_owned(), secret.to_owned())]),
    )
    .unwrap();
    let candidate = crate::finance_credentials::candidate(&state.credential_environment, entry_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        crate::finance_credentials::finish_verification(
            &state.credential_environment,
            entry_id,
            candidate.revision(),
            crate::finance_credentials::VerificationOutcome::Verified,
            timestamp,
            None,
        )
        .unwrap(),
        crate::finance_credentials::FinishVerification::Applied
    );
}

fn add_test_openbb_runtime(state: &mut AppState, root: &std::path::Path) {
    let catalog = bundled_catalog().unwrap();
    let provider_ids = catalog
        .entries
        .iter()
        .filter(|entry| entry.runtime.server_id.as_deref() == Some("openbb"))
        .flat_map(|entry| entry.runtime.provider_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let provider_network_hosts = provider_ids
        .iter()
        .map(|provider| {
            (
                provider.clone(),
                BTreeSet::from([format!("{provider}.example.test")]),
            )
        })
        .collect();
    let provider_config_fields = catalog
        .entries
        .iter()
        .filter(|entry| entry.runtime.server_id.as_deref() == Some("openbb"))
        .filter_map(|entry| {
            let [provider_id] = entry.runtime.provider_ids.as_slice() else {
                return None;
            };
            (!entry.runtime.config_mapping.is_empty()).then(|| {
                (
                    provider_id.clone(),
                    entry.runtime.config_mapping.keys().cloned().collect(),
                )
            })
        })
        .collect();
    Arc::make_mut(&mut state.artifacts).mcp_runtimes.insert(
        "openbb".into(),
        crate::mcp::BundledMcpRuntime {
            executable: root.join("loupist-openbb"),
            lease_dir: root.join("leases"),
            allowed_categories: vec!["equity".into()],
            provider_ids,
            provider_network_hosts,
            provider_config_fields,
            providerless_tool_policy: Default::default(),
            provider_receipt_pointer: "/structuredContent/provider".into(),
            tool_activation: Some(crate::mcp::McpToolActivation {
                tool_name: "activate_tools".into(),
                argument_name: "tool_names".into(),
            }),
            control_tool_names: BTreeSet::from(["activate_tools".into()]),
        },
    );
}

/// Credential-free entries a new Loupist starts with; host-only wallet reads
/// turn on with the first wallet instead.
fn zero_setup_catalog_ids() -> BTreeSet<String> {
    bundled_catalog()
        .unwrap()
        .entries
        .into_iter()
        .filter(|entry| entry.id != crate::finance_data::onchain::ONCHAIN_SOURCE_ID)
        .filter(|entry| {
            entry.setup.as_ref().is_none_or(|setup| {
                setup.config_fields.iter().all(|field| !field.required)
                    && setup.credential_fields.iter().all(|field| !field.required)
            })
        })
        .map(|entry| entry.id)
        .collect()
}

#[test]
fn bundled_catalog_snapshot_reports_readiness_from_the_staged_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let mut state = AppState::for_test(temporary.path().join("app"));
    let readiness = |state: &AppState| {
        let snapshot = marketplace_snapshot_for_state(state).unwrap();
        validate_snapshot(&snapshot).expect("bundled Marketplace snapshot");
        assert_eq!(
            snapshot
                .installed
                .iter()
                .map(|entry| entry.entry_id.as_str())
                .collect::<BTreeSet<_>>(),
            snapshot
                .catalog
                .entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<BTreeSet<_>>()
        );
        snapshot
            .connectors
            .iter()
            .map(|connector| (connector.entry_id.clone(), connector.readiness))
            .collect::<BTreeMap<_, _>>()
    };
    let before = readiness(&state);
    assert_eq!(
        before["openbb.platform"],
        MarketplaceConnectorReadiness::RuntimeUnavailable
    );
    assert_eq!(
        before["world-bank.indicators"],
        MarketplaceConnectorReadiness::Ready
    );

    add_test_openbb_runtime(&mut state, temporary.path());
    let after = readiness(&state);
    assert_eq!(
        after["openbb.platform"],
        MarketplaceConnectorReadiness::Ready
    );
    assert_eq!(
        after["sec.edgar"],
        MarketplaceConnectorReadiness::NeedsConfiguration
    );
}

#[test]
fn openbb_catalog_authority_matches_the_bundled_runtime_manifest() {
    let catalog = bundled_catalog().unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../openbb/runtime-manifest.json")).unwrap();
    let providers = manifest["providers"].as_array().unwrap();
    let provider = |id: &str| {
        providers
            .iter()
            .find(|provider| provider["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("{id} missing from the OpenBB manifest"))
    };
    let openbb_entries = catalog
        .entries
        .iter()
        .filter(|entry| entry.runtime.server_id.as_deref() == Some("openbb"))
        .collect::<Vec<_>>();
    assert_eq!(
        openbb_entries
            .iter()
            .flat_map(|entry| entry.runtime.provider_ids.iter().cloned())
            .collect::<BTreeSet<_>>(),
        providers
            .iter()
            .map(|provider| provider["id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>()
    );
    for entry in openbb_entries {
        let manifest_hosts = entry
            .runtime
            .provider_ids
            .iter()
            .flat_map(|id| provider(id)["network_hosts"].as_array().unwrap().clone())
            .map(|host| host.as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            entry
                .permissions
                .network_hosts
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            manifest_hosts,
            "network authority for {}",
            entry.id
        );
        if let [provider_id] = entry.runtime.provider_ids.as_slice() {
            assert_eq!(
                entry.runtime.credential_mapping,
                serde_json::from_value(
                    provider(provider_id)
                        .get("credential_mapping")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                )
                .unwrap(),
                "credential mapping for {}",
                entry.id
            );
        }
    }
}

#[test]
fn every_marketplace_entry_reaches_the_agent_runtime_profile() {
    let catalog = bundled_catalog().unwrap();
    let capability_ids = catalog
        .entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let profile =
        crate::agent_harness::AgentRuntimeProfile::new("chat", false, false, &capability_ids)
            .unwrap();
    for entry in catalog.entries {
        let represented = match entry.runtime.kind {
            MarketplaceRuntimeKind::BundledMcp => profile.components.iter().any(|component| {
                component.kind == "mcp"
                    && component.bundled
                    && component.server_id == entry.runtime.server_id
                    && component.connection.as_deref() == Some(entry.plugin.as_str())
            }),
            MarketplaceRuntimeKind::Native => profile
                .components
                .iter()
                .any(|component| component.connection.as_deref() == Some(entry.plugin.as_str())),
        };
        assert!(
            represented,
            "Marketplace entry {} has no runtime component",
            entry.id
        );
    }
}

#[test]
fn fresh_loupist_enables_only_credential_free_capabilities() {
    let temporary = tempfile::tempdir().unwrap();
    let mut state = AppState::for_test(temporary.path().join("app"));
    add_test_openbb_runtime(&mut state, temporary.path());
    seed_loupist(&state, "loupist-fresh");

    let bindings = loupist_capability_list_for_state("loupist-fresh", &state).unwrap();
    let connectors = bindings
        .iter()
        .filter(|binding| binding.enabled && !binding.entry_id.starts_with("skill."))
        .collect::<Vec<_>>();
    let zero_setup = zero_setup_catalog_ids();
    assert_eq!(
        connectors
            .iter()
            .map(|binding| binding.entry_id.clone())
            .collect::<BTreeSet<_>>(),
        zero_setup
    );
    for binding in connectors {
        assert!(binding.available, "{}", binding.entry_id);
        assert_eq!(binding.granted_permissions, ["execute"]);
    }

    let ready = execute_ids(&state, "loupist-fresh");
    assert!(zero_setup.iter().all(|id| ready.contains(id)), "{ready:?}");
    let profile =
        crate::agent_harness::AgentRuntimeProfile::new("chat", false, false, &ready).unwrap();
    assert!(profile.components.iter().any(|component| component
        .tool_names
        .iter()
        .any(|name| name == "dataset_import")));
}

#[test]
fn each_component_names_the_enabled_entries_it_serves() {
    let enabled = [
        "fred.macro".to_string(),
        "world-bank.indicators".to_string(),
    ];
    let profile =
        crate::agent_harness::AgentRuntimeProfile::new("chat", false, false, &enabled).unwrap();
    let served = |id: &str| {
        profile
            .components
            .iter()
            .find(|component| component.id == id)
            .map(|component| component.entry_ids.clone())
            .unwrap_or_else(|| panic!("{id} missing"))
    };
    // FRED is a provider inside market data, so loading `fred.macro` loads it.
    assert_eq!(served("openbb"), ["fred.macro"]);
    assert_eq!(served("world-bank"), ["world-bank.indicators"]);

    // DefiLlama's plugin also holds wallets, which its tools cannot read.
    let onchain = ["onchain.wallets".to_string(), "defillama.tvl".to_string()];
    let profile =
        crate::agent_harness::AgentRuntimeProfile::new("chat", false, false, &onchain).unwrap();
    let defillama = profile
        .components
        .iter()
        .find(|component| component.id == "defillama")
        .unwrap();
    assert!(defillama.entry_ids.is_empty(), "{:?}", defillama.entry_ids);
}

#[test]
fn wallet_reads_turn_on_for_every_loupist_with_the_first_wallet() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_loupist(&state, "loupist-wallets");
    let id = crate::finance_data::onchain::ONCHAIN_SOURCE_ID;
    assert!(!binding(&state, "loupist-wallets", id).enabled);
    enable_on_first_use(&state, id).unwrap();
    assert!(binding(&state, "loupist-wallets", id).enabled);
}

#[test]
fn user_disabled_defaults_stay_disabled_after_listing_or_reconfiguring() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_loupist(&state, "g-disabled");
    set_binding(&state, "g-disabled", "world-bank.indicators", false).unwrap();
    set_binding(&state, "g-disabled", "community.web-research", false).unwrap();

    let bindings = loupist_capability_list_for_state("g-disabled", &state).unwrap();
    assert!(
        !bindings
            .iter()
            .find(|binding| binding.entry_id == "world-bank.indicators")
            .unwrap()
            .enabled
    );
    configure(
        &state,
        "community.web-research",
        "search_policy",
        "exa_only",
    )
    .unwrap();
    assert!(!binding(&state, "g-disabled", "community.web-research").enabled);
    let ready = execute_ids(&state, "g-disabled");
    assert!(!ready.iter().any(|id| id == "world-bank.indicators"));
    assert!(!ready.iter().any(|id| id == "community.web-research"));
}

#[test]
fn kis_credential_patch_accepts_optional_profile_fields_but_requires_app_keys_overall() {
    let catalog = bundled_catalog().unwrap();
    let entry = credential_entry(&catalog, "koreainvestment.market-data").unwrap();
    let fields = |values: &[(&str, &str)]| {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    let status = |active: &[&str]| crate::finance_credentials::CredentialStatus {
        stored: !active.is_empty(),
        active: !active.is_empty(),
        pending: false,
        active_fields: active.iter().map(|field| (*field).to_owned()).collect(),
        candidate_fields: BTreeSet::new(),
        verification: crate::finance_credentials::CredentialVerification::Never,
        verified_at: None,
        krx_coverage: None,
    };
    let keys = fields(&[
        ("app_key", "test-kis-app-key"),
        ("app_secret", "test-kis-app-secret"),
    ]);
    let profile_only = fields(&[("account_number", "12345678")]);
    assert!(validate_credential_secrets(entry, &keys).is_ok());
    assert!(validate_credential_secrets(entry, &profile_only).is_ok());
    assert!(validate_required_credential_fields(entry, &profile_only, &status(&[])).is_err());
    assert!(validate_required_credential_fields(
        entry,
        &profile_only,
        &status(&["app_key", "app_secret"])
    )
    .is_ok());
    for invalid in [
        fields(&[
            ("app_key", "test-kis-app-key"),
            ("app_secret", "test-kis-app-secret"),
            ("unexpected", "test-extra-secret"),
        ]),
        fields(&[
            ("app_key", "test kis app key"),
            ("app_secret", "test-kis-app-secret"),
        ]),
        fields(&[("account_number", "1234ABCD")]),
    ] {
        assert!(validate_credential_secrets(entry, &invalid).is_err());
    }
}

#[test]
fn kis_environment_is_an_exact_select_that_scopes_credentials() {
    let mut catalog = bundled_catalog().unwrap();
    let entry = catalog
        .entries
        .iter_mut()
        .find(|entry| entry.id == crate::finance_data::KIS_SOURCE_ID)
        .unwrap();
    let setup = entry.setup.as_mut().unwrap();
    let environment = &setup.config_fields[0];
    assert!(valid_setup_value(environment, "real"));
    assert!(valid_setup_value(environment, "demo"));
    assert!(!valid_setup_value(environment, "prod"));
    assert_eq!(setup.credential_scope_fields, ["environment"]);

    setup.config_fields[0].options[1] = "paper".to_owned();
    assert!(validate_catalog(&catalog).is_err());
}

#[test]
fn a_native_vendor_entry_must_declare_the_hosts_its_reads_reach() {
    let mut catalog = bundled_catalog().unwrap();
    let entry = catalog
        .entries
        .iter_mut()
        .find(|entry| entry.id == crate::finance_data::DEFILLAMA_SOURCE_ID)
        .unwrap();
    entry.permissions.network_hosts.pop();
    assert!(validate_catalog(&catalog).is_err());
    let mut catalog = bundled_catalog().unwrap();
    let entry = catalog
        .entries
        .iter_mut()
        .find(|entry| entry.id == crate::finance_data::WORLD_BANK_SOURCE_ID)
        .unwrap();
    entry.permissions.credential_kinds.push("api_key".into());
    assert!(validate_catalog(&catalog).is_err());
}

#[test]
fn changing_kis_environment_disables_bindings_and_deletes_credentials() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let entry_id = crate::finance_data::KIS_SOURCE_ID;
    let loupist_id = "g-kis-env";
    crate::finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
    seed_loupist(&state, loupist_id);

    configure(&state, entry_id, "environment", "real").unwrap();
    crate::finance_credentials::stage(
        &state.credential_environment,
        entry_id,
        &BTreeMap::from([
            ("app_key".to_owned(), "test-kis-app-key".to_owned()),
            ("app_secret".to_owned(), "test-kis-app-secret".to_owned()),
        ]),
    )
    .unwrap();
    let candidate = crate::finance_credentials::candidate(&state.credential_environment, entry_id)
        .unwrap()
        .unwrap();
    crate::finance_credentials::finish_verification(
        &state.credential_environment,
        entry_id,
        candidate.revision(),
        crate::finance_credentials::VerificationOutcome::Verified,
        1,
        None,
    )
    .unwrap();
    set_binding(&state, loupist_id, entry_id, true).unwrap();

    let status = configure(&state, entry_id, "environment", "demo").unwrap();
    assert_eq!(
        status.readiness,
        MarketplaceConnectorReadiness::NeedsConfiguration
    );
    assert!(!has_active_credential(&state, entry_id));
    let binding = binding(&state, loupist_id, entry_id);
    assert!(!binding.enabled);
    assert!(binding.granted_permissions.is_empty());

    assert_eq!(
        configure(&state, entry_id, "environment", "prod")
            .unwrap_err()
            .code,
        "invalid_request"
    );
    crate::finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
}

#[test]
fn credential_verification_separates_rejection_from_outage() {
    assert_eq!(
        credential_verification_outcome::<()>(&Err(
            crate::finance_data::FinanceDataError::CredentialRejected("provider"),
        )),
        crate::finance_credentials::VerificationOutcome::Rejected
    );
    assert_eq!(
        credential_verification_outcome::<()>(&Err(
            crate::finance_data::FinanceDataError::NetworkSafe("provider"),
        )),
        crate::finance_credentials::VerificationOutcome::TemporarilyUnavailable
    );
}

#[test]
fn first_successful_setup_enables_connector_for_every_loupist_except_one_being_deleted() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let entry_id = "krx.market-data";
    let loupists = ["g-auto-a", "g-auto-b", "g-deleting"];
    crate::finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
    for loupist_id in loupists {
        seed_loupist(&state, loupist_id);
        assert!(!binding(&state, loupist_id, entry_id).enabled);
    }
    let deleting_before = binding(&state, loupists[2], entry_id);
    state.begin_loupist_deletion(loupists[2]).unwrap();

    promote_test_credential(&state, entry_id, "test-krx-auto-enable-secret", 20);
    let lifecycle_guard = connector_lifecycle_lock().unwrap();
    enable_ready_bindings_locked(&state, entry_id, &lifecycle_guard).unwrap();
    drop(lifecycle_guard);

    for loupist_id in &loupists[..2] {
        let binding = binding(&state, loupist_id, entry_id);
        assert!(
            binding.enabled,
            "{loupist_id} should be enabled after setup"
        );
        assert_eq!(binding.granted_permissions, vec!["execute".to_owned()]);
    }
    let deleting_after = binding(&state, loupists[2], entry_id);
    assert!(!deleting_after.enabled);
    assert_eq!(deleting_after.updated_at_ms, deleting_before.updated_at_ms);
    crate::finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
}

#[test]
fn update_maintenance_rejects_connector_and_credential_mutations() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let _update = state.maintenance.begin_update().unwrap();

    let config_error =
        configure(&state, "sec.edgar", "contact_email", "research@example.com").unwrap_err();
    let credential_error = delete_credential_for_state(
        MarketplaceCredentialRequest {
            entry_id: "krx.market-data".to_owned(),
        },
        &state,
    )
    .unwrap_err();

    assert_eq!(config_error.code, "maintenance_active");
    assert_eq!(credential_error.code, "maintenance_active");
}

#[test]
fn web_research_policy_defaults_persists_and_does_not_change_loupist_binding() {
    let temporary = tempfile::tempdir().unwrap();
    let app_data = temporary.path().join("app");
    let state = AppState::for_test(app_data.clone());
    let loupist_id = "g-web-policy";
    seed_loupist(&state, loupist_id);
    assert_eq!(
        web_research_policy(&state).unwrap(),
        crate::web::WebSearchPolicy::Automatic
    );
    set_binding(&state, loupist_id, "community.web-research", true).unwrap();

    let status = configure(
        &state,
        "community.web-research",
        "search_policy",
        "model_only",
    )
    .unwrap();
    assert_eq!(status.readiness, MarketplaceConnectorReadiness::Ready);
    assert!(binding(&state, loupist_id, "community.web-research").enabled);
    drop(state);

    let restarted = AppState::for_test(app_data);
    assert_eq!(
        web_research_policy(&restarted).unwrap(),
        crate::web::WebSearchPolicy::ModelOnly
    );
}

#[test]
fn pending_secret_is_not_ready_and_delete_disables_every_loupist() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let loupists = ["g-capture-a", "g-capture-b"];
    let entry_id = "krx.market-data";
    crate::finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
    for loupist_id in loupists {
        seed_loupist(&state, loupist_id);
    }

    crate::finance_credentials::stage(
        &state.credential_environment,
        entry_id,
        &BTreeMap::from([("api_key".to_owned(), "pending-krx-secret".to_owned())]),
    )
    .unwrap();
    assert!(set_binding(&state, loupists[0], entry_id, true).is_err());
    assert!(!execute_ids(&state, loupists[0]).contains(&entry_id.to_owned()));

    let pending = crate::finance_credentials::candidate(&state.credential_environment, entry_id)
        .unwrap()
        .unwrap();
    crate::finance_credentials::finish_verification(
        &state.credential_environment,
        entry_id,
        pending.revision(),
        crate::finance_credentials::VerificationOutcome::Verified,
        10,
        None,
    )
    .unwrap();
    for loupist_id in loupists {
        set_binding(&state, loupist_id, entry_id, true).unwrap();
    }
    assert!(execute_ids(&state, loupists[0]).contains(&entry_id.to_owned()));

    delete_credential_for_state(
        MarketplaceCredentialRequest {
            entry_id: entry_id.to_owned(),
        },
        &state,
    )
    .unwrap();
    for loupist_id in loupists {
        let binding = binding(&state, loupist_id, entry_id);
        assert!(!binding.enabled);
        assert!(binding.granted_permissions.is_empty());
        assert!(!execute_ids(&state, loupist_id).contains(&entry_id.to_owned()));
    }
    assert!(
        !crate::finance_credentials::status(&state.credential_environment, entry_id)
            .unwrap()
            .stored
    );
}

#[test]
fn concurrent_enable_and_delete_never_leave_enabled_without_an_active_key() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let loupist_id = "g-cred-cycle";
    let entry_id = "alpha-vantage.market-data";
    crate::finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
    seed_loupist(&state, loupist_id);

    for iteration in 0..12 {
        promote_test_credential(
            &state,
            entry_id,
            &format!("active-secret-{iteration:02}"),
            100 + iteration,
        );
        let barrier = Arc::new(Barrier::new(3));
        let enable_state = state.clone();
        let enable_barrier = barrier.clone();
        let enable = std::thread::spawn(move || {
            enable_barrier.wait();
            save_binding(
                LoupistCapabilityRequest {
                    loupist_id: loupist_id.to_owned(),
                    entry_id: entry_id.to_owned(),
                },
                &enable_state,
                true,
            )
        });
        let delete_state = state.clone();
        let delete_barrier = barrier.clone();
        let delete = std::thread::spawn(move || {
            delete_barrier.wait();
            delete_credential_for_state(
                MarketplaceCredentialRequest {
                    entry_id: entry_id.to_owned(),
                },
                &delete_state,
            )
        });
        barrier.wait();
        let _ = enable.join().unwrap();
        delete.join().unwrap().unwrap();

        let binding = binding(&state, loupist_id, entry_id);
        assert!(!binding.enabled);
        assert!(binding.granted_permissions.is_empty());
        assert!(!has_active_credential(&state, entry_id));
        assert!(!execute_ids(&state, loupist_id).contains(&entry_id.to_owned()));
    }
}

#[tokio::test]
async fn offline_folder_does_not_prevent_revoking_reachable_bindings() {
    let temporary = tempfile::tempdir().unwrap();
    let app = temporary.path().join("app");
    let state = AppState::for_folder_test(app.clone());
    let entry_id = "krx.market-data";
    let mut folders = Vec::new();
    for name in ["offline", "reachable"] {
        let path = temporary.path().join(name);
        std::fs::create_dir(&path).unwrap();
        let profile = crate::commands::loupist::create_folder_loupist(&state, &path)
            .await
            .unwrap();
        folders.push((path, profile));
    }
    promote_test_credential(&state, entry_id, "test-only-secret", 10);
    for (_, profile) in &folders {
        set_binding(&state, &profile.id, entry_id, true).unwrap();
    }
    // Put the unavailable folder first without relying on creation clock order.
    let mut offline = state.store.get_loupist(&folders[0].1.id).unwrap().unwrap();
    offline.updated_at_ms = state
        .store
        .list_loupists()
        .unwrap()
        .iter()
        .map(|profile| profile.updated_at_ms)
        .max()
        .unwrap()
        + 1;
    state.store.save_loupist(&offline).unwrap();
    assert_eq!(state.store.list_loupists().unwrap()[0].id, folders[0].1.id);
    drop(state);
    let removed = temporary.path().join("unmounted");
    std::fs::rename(&folders[0].0, &removed).unwrap();
    let state = AppState::for_folder_test(app);
    let request = || MarketplaceCredentialRequest {
        entry_id: entry_id.into(),
    };
    let error = delete_credential_for_state(request(), &state).unwrap_err();
    assert!(error.message.contains(&folders[0].1.name), "{error:?}");
    assert!(
        error.message.contains(folders[0].0.to_str().unwrap()),
        "{error:?}"
    );
    let reachable = binding(&state, &folders[1].1.id, entry_id);
    assert!(!reachable.enabled);
    assert!(reachable.granted_permissions.is_empty());
    assert!(has_active_credential(&state, entry_id));
    std::fs::rename(removed, &folders[0].0).unwrap();
    delete_credential_for_state(request(), &state).unwrap();
    assert!(!binding(&state, &folders[0].1.id, entry_id).enabled);
    assert!(!has_active_credential(&state, entry_id));
}

#[test]
fn concurrent_configure_and_disable_never_reenable_a_stale_binding() {
    let temporary = tempfile::tempdir().unwrap();
    let mut state = AppState::for_test(temporary.path().join("app"));
    add_test_openbb_runtime(&mut state, temporary.path());
    let loupist_id = "g-conf-cycle";
    let entry_id = "sec.edgar";
    seed_loupist(&state, loupist_id);

    configure(&state, entry_id, "contact_email", "research@example.com").unwrap();

    for iteration in 0..12 {
        set_binding(&state, loupist_id, entry_id, true).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let configure_state = state.clone();
        let configure_barrier = barrier.clone();
        let configure = std::thread::spawn(move || {
            configure_barrier.wait();
            configure_connector(
                MarketplaceConnectorConfigureRequest {
                    entry_id: entry_id.to_owned(),
                    config: BTreeMap::from([(
                        "contact_email".to_owned(),
                        format!("research-{iteration:02}@example.com"),
                    )]),
                },
                &configure_state,
            )
        });
        let disable_state = state.clone();
        let disable_barrier = barrier.clone();
        let disable = std::thread::spawn(move || {
            disable_barrier.wait();
            save_binding(
                LoupistCapabilityRequest {
                    loupist_id: loupist_id.to_owned(),
                    entry_id: entry_id.to_owned(),
                },
                &disable_state,
                false,
            )
        });
        barrier.wait();
        configure.join().unwrap().unwrap();
        disable.join().unwrap().unwrap();

        let binding = binding(&state, loupist_id, entry_id);
        assert!(!binding.enabled);
        assert!(binding.granted_permissions.is_empty());
    }
}

#[test]
fn unfinished_or_failed_setup_never_configures_or_grants_a_connector() {
    use crate::finance_credentials::{self, VerificationOutcome};
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_loupist(&state, "setup-owner");
    let entry_id = "krx.market-data";
    let installed = marketplace_snapshot_for_state(&state).unwrap().installed;
    let ids = installed
        .iter()
        .map(|item| item.entry_id.clone())
        .collect::<Vec<_>>();
    for outcome in [
        None,
        Some(VerificationOutcome::Rejected),
        Some(VerificationOutcome::TemporarilyUnavailable),
    ] {
        finance_credentials::stage(
            &state.credential_environment,
            entry_id,
            &BTreeMap::from([("api_key".into(), "unverified-test-secret".into())]),
        )
        .unwrap();
        // No finish models cancellation after staging, before verification commits.
        if let Some(outcome) = outcome {
            let candidate = finance_credentials::candidate(&state.credential_environment, entry_id)
                .unwrap()
                .unwrap();
            finance_credentials::finish_verification(
                &state.credential_environment,
                entry_id,
                candidate.revision(),
                outcome,
                10,
                None,
            )
            .unwrap();
        }
        let snapshot = marketplace_snapshot_for_state(&state).unwrap();
        assert_eq!(
            snapshot
                .installed
                .iter()
                .map(|item| item.entry_id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(
            !snapshot
                .installed
                .iter()
                .find(|item| item.entry_id == entry_id)
                .unwrap()
                .configured
        );
        assert!(!has_active_credential(&state, entry_id));
        assert!(set_binding(&state, "setup-owner", entry_id, true).is_err());
        assert!(!execute_ids(&state, "setup-owner").contains(&entry_id.into()));
        finance_credentials::delete(&state.credential_environment, entry_id).unwrap();
    }
    assert!(configure(&state, "sec.edgar", "contact_email", "not-an-email").is_err());
    assert!(connector_config::read_connector_config(&state, "sec.edgar")
        .unwrap()
        .is_empty());
}

#[test]
fn a_locked_key_shows_on_its_own_connection_and_the_rest_keep_loading() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let environment = state.credential_environment.to_string();
    seed_loupist(&state, "g-locked");
    let (locked, other) = ("fred.macro", "krx.market-data");
    let catalog = bundled_catalog().unwrap();
    let field = |entry_id: &str| {
        credential_entry(&catalog, entry_id)
            .unwrap()
            .setup
            .as_ref()
            .unwrap()
            .credential_fields[0]
            .id
            .clone()
    };
    for entry_id in [locked, other] {
        crate::finance_credentials::stage(
            &environment,
            entry_id,
            &BTreeMap::from([(field(entry_id), "test-locked-fixture-key".to_owned())]),
        )
        .unwrap();
        let candidate = crate::finance_credentials::candidate(&environment, entry_id)
            .unwrap()
            .unwrap();
        crate::finance_credentials::finish_verification(
            &environment,
            entry_id,
            candidate.revision(),
            crate::finance_credentials::VerificationOutcome::Verified,
            10,
            None,
        )
        .unwrap();
        // The update: a new build, and this session has read nothing yet.
        crate::finance_credentials::test_store::restart(&environment, entry_id);
    }
    crate::finance_credentials::test_store::deny(&environment, locked, true);
    let reads_before = crate::finance_credentials::test_store::secret_reads(&environment, locked);

    let connector = |snapshot: &MarketplaceSnapshotDto, id: &str| {
        snapshot
            .connectors
            .iter()
            .find(|connector| connector.entry_id == id)
            .unwrap()
            .clone()
    };
    for _ in 0..3 {
        let snapshot =
            with_connector_lifecycle(&state, || marketplace_snapshot_for_state(&state)).unwrap();
        let fred = connector(&snapshot, locked);
        assert!(fred.credentials.iter().all(|credential| credential.locked));
        assert_ne!(fred.readiness, MarketplaceConnectorReadiness::Ready);
        let krx = connector(&snapshot, other);
        assert!(krx
            .credentials
            .iter()
            .all(|credential| !credential.locked && credential.active));
        // Bot's connection list and every run's authority (Following,
        // Watching, asset checks) still load.
        with_connector_lifecycle(&state, || {
            loupist_capability_list_for_state("g-locked", &state)
        })
        .unwrap();
        crate::commands::capture_chat_connector_authority(&state, "g-locked").unwrap();
    }
    // The refused prompt was asked once, not on every screen.
    assert_eq!(
        crate::finance_credentials::test_store::secret_reads(&environment, locked),
        reads_before + 1
    );

    let request = || MarketplaceCredentialRequest {
        entry_id: locked.to_owned(),
    };
    let still_locked = super::credentials::unlock_credential_for_state(request(), &state).unwrap();
    assert!(still_locked.iter().all(|credential| credential.locked));
    crate::finance_credentials::test_store::deny(&environment, locked, false);
    let unlocked = super::credentials::unlock_credential_for_state(request(), &state).unwrap();
    assert!(unlocked
        .iter()
        .all(|credential| !credential.locked && credential.active));
    for entry_id in [locked, other] {
        crate::finance_credentials::delete(&environment, entry_id).unwrap();
    }
}
