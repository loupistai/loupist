//! License and provider-policy behavior through the same selection/import/export paths.
use super::*;
use crate::sharing::pack::{ItemOrigin, License, LicensedItem, OriginEntry};
use crate::sharing::provider_policy::{self, DataPolicy};
use tauri::Manager;

#[tokio::test]
async fn share_summary_exposes_legal_code_urls_for_every_allowed_license() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let id = loupist(&state, "Research desk").await;
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    let request = serde_json::from_value(serde_json::json!({
        "loupistId": id, "scope": {"kind": "loupist"}
    }))
    .unwrap();
    let summary = commands::share::share_summary(request, app.state()).unwrap();
    let value = serde_json::to_value(summary).unwrap();
    let expected: Vec<_> = License::OPEN
        .into_iter()
        .map(|license| {
            serde_json::json!({
                "id": license.id(), "url": license.url(None)
            })
        })
        .collect();
    assert_eq!(value["allowedLicenses"], serde_json::json!(expected));
    assert_eq!(value["defaultLicense"], "CC-BY-NC-SA-4.0");
}

#[test]
fn pack_review_exposes_manifest_credits_with_optional_attribution() {
    let mut files = minimal_pack();
    let mut manifest = pack::validate(&files).unwrap().manifest;
    manifest.credits = vec![
        OriginEntry {
            id: "c1a148e4-bd49-46aa-a51b-373e2ee90621".into(),
            name: "Upstream desk".into(),
            author: Some("Researcher".into()),
            license: Some(License::CcBy),
            url: Some("https://github.com/researcher/pack".into()),
            version: 7,
            commit: None,
        },
        OriginEntry {
            id: "7a359642-6fc1-49ad-a0db-3c27969fdd34".into(),
            name: "Earlier desk".into(),
            author: None,
            license: None,
            url: None,
            version: 2,
            commit: None,
        },
    ];
    files.remove(pack::MANIFEST);
    let (_, files) = pack::seal(manifest, files);
    let OpenResult::Preview { preview } = import::stage(
        Fetched {
            files,
            ..Fetched::default()
        },
        None,
    ) else {
        panic!("pack with credits must import");
    };
    let value = serde_json::to_value(preview).unwrap();
    assert_eq!(
        value["manifest"]["credits"],
        serde_json::json!([
            { "name": "Upstream desk", "author": "Researcher", "license": "CC-BY-4.0", "url": "https://github.com/researcher/pack", "version": 7 },
            { "name": "Earlier desk", "author": null, "license": null, "url": null, "version": 2 }
        ])
    );
}

fn licensed(name: &str, license: License) -> LicensedItem {
    LicensedItem {
        key: name.into(),
        name: name.into(),
        license,
    }
}

#[test]
fn compatibility_intersects_every_license_and_names_conflicting_items() {
    use License::*;
    let cases = [
        (Cc0, License::OPEN.to_vec()),
        (CcBy, License::OPEN.to_vec()),
        (CcBySa, vec![CcBySa]),
        (CcByNc, vec![CcByNcSa, CcByNc]),
        (CcByNcSa, vec![CcByNcSa]),
        (Personal, License::OPEN.to_vec()),
    ];
    for (first, expected) in &cases {
        assert_eq!(
            pack::compatible_licenses(&[licensed("first", *first)]).unwrap(),
            *expected
        );
        for (second, allowed) in &cases {
            let intersection: Vec<_> = expected
                .iter()
                .filter(|license| allowed.contains(license))
                .copied()
                .collect();
            let result = pack::compatible_licenses(&[
                licensed("first", *first),
                licensed("second", *second),
            ]);
            if intersection.is_empty() {
                let conflict = result.unwrap_err();
                assert_eq!(conflict.code, "license_conflict");
                assert_eq!(
                    conflict
                        .items
                        .iter()
                        .map(|item| item.name.as_str())
                        .collect::<Vec<_>>(),
                    ["first", "second"]
                );
            } else {
                assert_eq!(result.unwrap(), intersection);
            }
        }
    }
    assert_eq!(License::default(), CcByNcSa);
    assert!(!Personal.open());
    assert!(!Personal.redistributable());
    for license in License::OPEN {
        assert!(license.open() && license.redistributable());
        assert!(license.url(None).ends_with("/legalcode"));
        assert_eq!(License::from_id(license.id()), Some(license));
    }
}

fn request(loupist_id: &str) -> commands::share::ShareRequest {
    serde_json::from_value(serde_json::json!({
        "loupistId": loupist_id, "scope": {"kind": "loupist"},
        "sections": {"profile": true, "perspective": true, "methods": true, "principles": true, "map": true},
        "excludedItemKeys": [], "acknowledgedFindingIds": []
    })).unwrap()
}

#[tokio::test]
async fn rust_defaults_a_new_share_to_nc_sa_and_changes_flat_versions_with_the_license() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let id = loupist(&state, "Research desk").await;
    let method_id = knowledge(&state, &id, |write| {
        Ok(method(
            write,
            &id,
            "Read filings",
            "Read the annual filing.",
        ))
    });
    let built = commands::share::build_pack(&state, &request(&id), None).unwrap();
    assert_eq!(
        pack::validate(&built.files).unwrap().manifest.license,
        License::CcByNcSa
    );
    assert!(String::from_utf8_lossy(&built.files["README.md"]).contains("Non-commercial use only"));
    let item_request = |license| {
        serde_json::from_value(serde_json::json!({
        "loupistId": id, "scope": {"kind": "item", "recordId": method_id},
        "sections": {"profile": false, "perspective": false, "methods": true, "principles": false, "map": false},
        "excludedItemKeys": [], "acknowledgedFindingIds": [], "license": license
    })).unwrap()
    };
    let by = commands::share::build_pack(&state, &item_request("CC-BY-4.0"), None).unwrap();
    let nc = commands::share::build_pack(&state, &item_request("CC-BY-NC-SA-4.0"), None).unwrap();
    assert_ne!(pack::content_hash(&by.files), pack::content_hash(&nc.files));
}

fn relabeled(mut files: Files, license: License, terms: Option<&str>) -> Files {
    let mut manifest: pack::Manifest =
        serde_json::from_slice(&files.remove(pack::MANIFEST).unwrap()).unwrap();
    manifest.license = license;
    manifest.terms_url = terms.map(str::to_owned);
    pack::seal(manifest, files).1
}

fn stage_files(files: Files) -> import::Staged {
    match import::stage(
        Fetched {
            files,
            ..Fetched::default()
        },
        None,
    ) {
        OpenResult::Preview { preview } => import::staged(&preview.staging_id).unwrap(),
        other => panic!("pack did not open: {other:?}"),
    }
}

#[test]
fn personal_requires_https_terms_and_review_exposes_the_license_rights() {
    let terms = "https://example.org/personal-terms";
    for invalid in [
        None,
        Some("http://example.org/terms"),
        Some("https://user:password@example.org/terms"),
        Some("https://"),
    ] {
        assert_eq!(
            pack::validate(&relabeled(minimal_pack(), License::Personal, invalid)).unwrap_err(),
            Rejection::Damaged
        );
    }
    for license in License::OPEN.into_iter().chain([License::Personal]) {
        let files = relabeled(
            minimal_pack(),
            license,
            (license == License::Personal).then_some(terms),
        );
        let result = import::stage(
            Fetched {
                files,
                ..Fetched::default()
            },
            None,
        );
        let OpenResult::Preview { preview } = result else {
            panic!("license must import");
        };
        let value = serde_json::to_value(preview).unwrap();
        assert_eq!(value["manifest"]["license"], license.id());
        assert_eq!(value["manifest"]["licenseUrl"], license.url(Some(terms)));
        assert_eq!(
            value["manifest"]["redistributable"],
            license.redistributable()
        );
    }
}

#[tokio::test]
async fn personal_packs_cannot_reach_github_repositories_or_gists() {
    use crate::sharing::github::{ApiError, Github};
    let api = Github::new(
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
    );
    let files = relabeled(
        minimal_pack(),
        License::Personal,
        Some("https://example.org/terms"),
    );
    assert_eq!(
        api.create_gist("unused", &files, "Pack", true)
            .await
            .unwrap_err(),
        ApiError::LicenseNotShareable
    );
    assert_eq!(
        api.commit_files("unused", "author/pack", "main", &files, "Pack")
            .await
            .unwrap_err(),
        ApiError::LicenseNotShareable
    );
    assert_eq!(
        commands::share::require_shareable(License::Personal)
            .unwrap_err()
            .code,
        "license_not_shareable"
    );
}

#[tokio::test]
async fn accepted_personal_items_and_sources_stay_excluded_through_updates_and_removal() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let author = loupist(&state, "Original desk").await;
    seed(&state, &author, false);
    let original = commands::share::build_pack(&state, &whole(&state, &author, &[]), None).unwrap();
    let staged = stage_files(relabeled(
        original.files,
        License::Personal,
        Some("https://example.org/terms"),
    ));
    let receiver = loupist(&state, "Reader desk").await;
    let row = commands::add::import_row(&staged, ImportMode::New, AT);
    commands::with_tx(&state, &receiver, |tx, blobs| {
        commands::add::record_import(
            tx,
            blobs,
            &receiver,
            &staged,
            &row,
            AddSections {
                perspective: false,
                methods: true,
                principles: true,
                map: true,
            },
            &|_| {},
        )?;
        let keys: Vec<_> = staged
            .items
            .iter()
            .filter(|(_, kind, _)| *kind != "perspective")
            .map(|(key, _, _)| key.clone())
            .collect();
        for key in keys {
            commands::review::decide(tx, &receiver, &row.import_id, &[key], Decision::Accept, AT)?;
        }
        Ok(())
    })
    .unwrap();
    let assert_excluded = || {
        let gathered = commands::share::gather(&state, &receiver).unwrap();
        assert!(!gathered.input.lens_origins.is_empty());
        assert!(gathered
            .input
            .lens_origins
            .values()
            .all(|origin| !origin.redistributable()));
        let prepared = export::prepare(&gathered.input, &gathered.identity.pack_id);
        let selected = export::select(&prepared, &loupist_choice(&[], &[]));
        assert!(
            selected.methods.is_empty()
                && selected.principles.is_empty()
                && selected.claims.is_empty()
                && selected.sources.is_empty()
        );
        assert!(export::counts(&selected).left_out_license >= 3);
        assert!(!selected.include_profile);
    };
    assert_excluded();
    // New public evidence must not erase the rights in the imported entity text
    // or relationship itself. Exercise each World kind independently.
    let gathered = commands::share::gather(&state, &receiver).unwrap();
    assert!(gathered
        .input
        .world_origins
        .keys()
        .any(|key| key.starts_with("node:")));
    assert!(gathered
        .input
        .world_origins
        .keys()
        .any(|key| key.starts_with("claim:")));
    for protected_kind in ["node:", "claim:"] {
        let mut input = gathered.input.clone();
        input
            .world_origins
            .retain(|key, _| key.starts_with(protected_kind));
        for source in input
            .snapshot
            .spans
            .values_mut()
            .filter(|source| source.authorship == "imported_contributor")
        {
            source.record_id = format!("independent-{}", source.record_id);
            source.provider_identity = "sec".into();
            source.authorship = "publisher".into();
            source.source_class = "tool_result".into();
        }
        let prepared = export::prepare(&input, &gathered.identity.pack_id);
        assert_eq!(
            prepared.claims.len(),
            2,
            "the independent public evidence is eligible"
        );
        let selected = export::select(&prepared, &loupist_choice(&[], &[]));
        assert!(
            selected.entities.is_empty()
                && selected.claims.is_empty()
                && selected.sources.is_empty(),
            "{protected_kind} rights survive new evidence"
        );
        assert!(selected.left_out_license >= 2);
        for origin in input.world_origins.values_mut() {
            origin.license = License::CcBySa;
        }
        let prepared = export::prepare(&input, &gathered.identity.pack_id);
        let selected = export::select(&prepared, &loupist_choice(&[], &[]));
        assert_eq!(
            export::license_options(&selected).unwrap(),
            [License::CcBySa],
            "{protected_kind} contributes to compatibility"
        );
    }
    let updated = stage_files(relabeled(staged.fetched.files.clone(), License::Cc0, None));
    let next = crate::sharing::update::advanced(&row, &updated);
    commands::with_tx(&state, &receiver, |tx, _| {
        store::advance_import(tx, &receiver, &next).unwrap();
        store::mark_removed(tx, &receiver, &row.import_id, AT + 1).unwrap();
        Ok(())
    })
    .unwrap();
    assert_excluded();
    knowledge(&state, &receiver, |write| {
        Ok(method(
            write,
            &receiver,
            "Own method",
            "Read the annual report.",
        ))
    });
    let built = commands::share::build_pack(&state, &request(&receiver), None).unwrap();
    assert!(
        !built.files.contains_key("LOUPIST.md"),
        "a removed Personal import does not free its profile content"
    );
    assert_eq!(
        pack::validate(&built.files).unwrap().manifest.name,
        "Shared Loupist"
    );
}

fn origin(name: &str, license: License) -> ItemOrigin {
    ItemOrigin {
        pack_id: "c1a148e4-bd49-46aa-a51b-373e2ee90621".into(),
        name: name.into(),
        author: Some("Researcher".into()),
        license,
        version: 7,
        url: Some("https://github.com/researcher/pack".into()),
        chain: Vec::new(),
    }
}

#[tokio::test]
async fn credits_follow_included_origin_chains_and_conflicts_can_be_excluded() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let id = loupist(&state, "My desk").await;
    let first = knowledge(&state, &id, |write| {
        Ok(method(write, &id, "Owner earnings", "Read cash flows."))
    });
    let second = knowledge(&state, &id, |write| {
        Ok(method(write, &id, "Debt limits", "Read debt notes."))
    });
    let mut gathered = commands::share::gather(&state, &id).unwrap();
    let mut by = origin("Upstream desk", License::CcBy);
    by.chain.push(OriginEntry {
        id: "7a359642-6fc1-49ad-a0db-3c27969fdd34".into(),
        name: "Earlier desk".into(),
        version: 2,
        url: Some("https://github.com/earlier/pack".into()),
        commit: None,
        author: Some("Earlier author".into()),
        license: Some(License::CcByNc),
    });
    gathered.input.lens_origins.insert(first.clone(), by);
    let prepared = export::prepare(&gathered.input, &gathered.identity.pack_id);
    let choice = loupist_choice(&[], &[]);
    let selected = export::select(&prepared, &choice);
    assert_eq!(
        export::license_options(&selected).unwrap(),
        [License::CcByNcSa, License::CcByNc]
    );
    let meta = export::PackMeta {
        pack_id: gathered.identity.pack_id.clone(),
        version: 1,
        created_at: "2026-11-01T00:00:00Z".into(),
        license: License::CcByNcSa,
        terms_url: None,
        byline: None,
        notice: String::new(),
        open_url: None,
        app: "1.0".into(),
        origin: Vec::new(),
    };
    let (manifest, files) = export::build(&selected, &choice, &meta);
    pack::validate(&files).unwrap();
    assert_eq!(manifest.credits.len(), 2);
    let readme = String::from_utf8_lossy(&files["README.md"]);
    for expected in [
        "## Credits",
        "Upstream desk",
        "Researcher",
        "CC-BY-4.0",
        "version 7",
        "https://github.com/researcher/pack",
        "Earlier author",
        "Non-commercial use only",
    ] {
        assert!(readme.contains(expected), "{expected}");
    }
    gathered
        .input
        .lens_origins
        .insert(second.clone(), origin("SA desk", License::CcBySa));
    let prepared = export::prepare(&gathered.input, &gathered.identity.pack_id);
    let selected = export::select(&prepared, &choice);
    let conflict = export::license_options(&selected).unwrap_err();
    assert_eq!(conflict.code, "license_conflict");
    assert!(conflict
        .items
        .iter()
        .any(|item| item.name == "Owner earnings"));
    assert!(conflict.items.iter().any(|item| item.name == "Debt limits"));
    gathered.input.lens_origins.get_mut(&first).unwrap().author =
        Some("private-origin-token".into());
    let private = export::prepare(&gathered.input, &gathered.identity.pack_id);
    let context = crate::sharing::scan::PrivateContext {
        secrets: vec!["private-origin-token".into()],
        ..Default::default()
    };
    assert!(export::findings(&private, &context)
        .iter()
        .any(|finding| finding.item_key
            == pack::item_key("m", &gathered.identity.pack_id, &first)
            && finding.class == PrivateClass::Credential));
    let key = pack::item_key("m", &gathered.identity.pack_id, &second);
    let selected = export::select(&prepared, &loupist_choice(&[key], &[]));
    assert_eq!(
        export::license_options(&selected).unwrap(),
        [License::CcByNcSa, License::CcByNc]
    );
}

#[tokio::test]
async fn restricted_sources_drop_relationships_with_counts_but_public_and_user_authored_survive() {
    use crate::knowledge::shared_pack::SpanSource;
    let source = |provider: &str, class: &str, authorship: &str| SpanSource {
        record_id: "s".into(),
        title: "Source".into(),
        source_class: class.into(),
        origin_locator: "https://example.org/data".into(),
        provider_identity: provider.into(),
        custom_origin: false,
        authorship: authorship.into(),
        publication_time: None,
    };
    for (provider, policy) in provider_policy::PROVIDERS {
        assert_eq!(
            export::source_is_private(&source(provider, "tool_result", "publisher")),
            *policy == DataPolicy::Restricted,
            "{provider}"
        );
    }
    for provider in [
        "unknown",
        "native-finance:krx_daily_quote",
        "native-finance:defillama_read",
        "openbb:polygon",
    ] {
        assert!(export::source_is_private(&source(
            provider,
            "tool_result",
            "publisher"
        )));
    }
    for provider in [
        "native-finance:dart_financials",
        "native-finance:worldbank_series",
        "openbb:sec",
    ] {
        assert!(!export::source_is_private(&source(
            provider,
            "tool_result",
            "publisher"
        )));
    }
    assert!(!export::source_is_private(&source(
        "sec",
        "web_page",
        "publisher"
    )));
    assert!(!export::source_is_private(&source(
        "user",
        "user_statement",
        "user"
    )));
    assert!(export::source_is_private(&source(
        "user",
        "user_file",
        "user"
    )));
    assert!(!export::source_is_private(&source(
        "native-web:web_fetch",
        "web_page",
        "publisher"
    )));
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let id = loupist(&state, "Provider desk").await;
    seed(&state, &id, false);
    let mut gathered = commands::share::gather(&state, &id).unwrap();
    for span in gathered
        .input
        .snapshot
        .spans
        .values_mut()
        .filter(|source| source.provider_identity == "web-research:web_fetch")
    {
        span.provider_identity = "fmp".into();
        span.source_class = "tool_result".into();
    }
    let restricted = export::prepare(&gathered.input, &gathered.identity.pack_id);
    assert!(restricted.claims.is_empty());
    assert!(restricted.left_out >= 2);
    for span in gathered
        .input
        .snapshot
        .spans
        .values_mut()
        .filter(|source| source.provider_identity == "fmp")
    {
        span.provider_identity = "sec".into();
    }
    let public = export::prepare(&gathered.input, &gathered.identity.pack_id);
    assert_eq!(public.claims.len(), 2);
    assert_eq!(restricted.left_out, public.left_out + 2);
    assert!(public.claims.iter().all(|claim| claim.quote.is_none()));
}

#[tokio::test]
async fn listing_requires_current_https_terms_and_records_the_acceptance_with_the_share_identity() {
    let terms = "https://example.org/gallery-terms";
    assert_eq!(
        commands::gallery::listing_terms(None, true, Some(terms))
            .unwrap_err()
            .code,
        "not_configured"
    );
    assert_eq!(
        commands::gallery::listing_terms(Some("http://example.org/terms"), true, Some(terms))
            .unwrap_err()
            .code,
        "not_configured"
    );
    for (accepted, supplied) in [
        (false, Some(terms)),
        (true, None),
        (true, Some("https://example.org/old")),
    ] {
        assert_eq!(
            commands::gallery::listing_terms(Some(terms), accepted, supplied)
                .unwrap_err()
                .code,
            "gallery_terms_required"
        );
    }
    let accepted = commands::gallery::listing_terms(Some(terms), true, Some(terms)).unwrap();
    let at = "2026-11-01T10:11:12Z";
    let body = commands::gallery::listing_body(accepted, at);
    assert!(body.contains(terms) && body.contains(at));
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let id = loupist(&state, "Gallery desk").await;
    let gathered = commands::share::gather(&state, &id).unwrap();
    commands::with_tx(&state, &id, |tx, _| {
        store::record_listing_acceptance(tx, &id, &gathered.identity.pack_id, "commit", accepted, at).unwrap();
        let recorded: (String, String, String, String) = tx.query_row("SELECT pack_id, commit_sha, terms_url, accepted_at FROM share_listing_acceptances WHERE loupist_id = ?1", [&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
        assert_eq!(recorded, (gathered.identity.pack_id.clone(), "commit".into(), terms.into(), at.into()));
        Ok(())
    }).unwrap();
}

#[test]
fn gallery_never_lists_personal_packs() {
    let entry = serde_json::json!({"id":"7f1d1c3e-0000-4000-8000-000000000001", "repo":"author/pack", "commit":"0123456789abcdef0123456789abcdef01234567", "name":"Private pack", "language":"en", "license":"LicenseRef-Loupist-Personal", "updatedAt":"2026-11-01T00:00:00Z"});
    let bytes =
        serde_json::to_vec(&serde_json::json!({"format":"loupist-index/1", "entries":[entry]}))
            .unwrap();
    assert!(crate::sharing::gallery::parse(&bytes)
        .unwrap()
        .entries
        .is_empty());
}

#[tokio::test]
async fn exporting_accepted_incompatible_packs_refuses_until_an_item_is_left_out() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let id = loupist(&state, "Combined desk").await;
    for (license, name) in [
        (License::CcBySa, "SA research"),
        (License::CcByNcSa, "NC-SA research"),
    ] {
        let mut files = relabeled(minimal_pack(), license, None);
        let mut manifest: pack::Manifest =
            serde_json::from_slice(&files.remove(pack::MANIFEST).unwrap()).unwrap();
        manifest.id = uuid::Uuid::new_v4().to_string();
        manifest.name = name.into();
        let staged = stage_files(pack::seal(manifest, files).1);
        let row = commands::add::import_row(&staged, ImportMode::Merge, AT);
        commands::with_tx(&state, &id, |tx, blobs| {
            commands::add::record_import(
                tx,
                blobs,
                &id,
                &staged,
                &row,
                AddSections {
                    perspective: false,
                    methods: true,
                    principles: true,
                    map: false,
                },
                &|_| {},
            )?;
            for (key, _, _) in &staged.items {
                commands::review::decide(
                    tx,
                    &id,
                    &row.import_id,
                    std::slice::from_ref(key),
                    Decision::Accept,
                    AT,
                )?;
            }
            Ok(())
        })
        .unwrap();
    }
    let error = commands::share::build_pack(&state, &request(&id), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "license_conflict");
    assert!(error.message.contains("Cash conversion"));
    let gathered = commands::share::gather(&state, &id).unwrap();
    let record = gathered
        .input
        .lens_origins
        .iter()
        .find(|(_, origin)| origin.license == License::CcBySa)
        .unwrap()
        .0;
    let excluded = pack::item_key("m", &gathered.identity.pack_id, record);
    let request: commands::share::ShareRequest = serde_json::from_value(serde_json::json!({
        "loupistId": id, "scope": {"kind":"loupist"},
        "sections":{"profile":true,"perspective":true,"methods":true,"principles":true,"map":true},
        "excludedItemKeys":[excluded], "acknowledgedFindingIds":[]
    }))
    .unwrap();
    let built = commands::share::build_pack(&state, &request, None).unwrap();
    assert_eq!(
        pack::validate(&built.files).unwrap().manifest.license,
        License::CcByNcSa
    );
}
