use super::*;

fn field_error(error: BrokerError, expected_field: &str, repair: &str) {
    let BrokerError::InvalidArguments { field, expected } = error else {
        panic!("{error:?}")
    };
    assert_eq!(field, expected_field);
    assert!(expected.contains(repair), "{expected}");
    assert!(!expected.contains("Load the capability"));
}

#[tokio::test]
async fn active_tools_name_stale_sources_wrong_artifact_kinds_and_missing_schedule_context() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = ToolCapture {
        bound_chat_id: Some("chat-a".into()),
        surface: Some(crate::chat::surface::ChatSurface::Session),
        ..ToolCapture::for_chat("artifact-reply".into())
    };
    let executor = AppToolExecutor {
        capability_ids: BTreeSet::from(["community.web-research".into()]),
        ..executor_for(&state, Arc::new(capture))
    };
    let policy = policy(false, false);
    field_error(
        executor
            .execute(
                &policy,
                ToolMethod::WebFetch,
                json!({"source_id":"web:earlier-reply"}),
            )
            .await
            .unwrap_err(),
        "source_id",
        "search again or pass the URL",
    );
    field_error(
        executor
            .execute(
                &policy,
                ToolMethod::ScheduleHandoff,
                json!({"request_key":"missing-context","title":"Review","prompt":"Read research"}),
            )
            .await
            .unwrap_err(),
        "schedule",
        "no scheduling context",
    );
    executor.execute(&policy, ToolMethod::ArtifactPublish,
        json!({"mode":"create","title":"Report","payload":{"kind":"markdown","schema":"loupist-markdown/1","markdown":"Report body"}})).await.unwrap();
    let commit = executor.capture.artifacts.lock().await[0].clone();
    save_artifact_turn(&state, &commit);
    for (method, params, repair) in [
        (
            ToolMethod::ChartQuery,
            json!({"artifact_id":commit.artifact.id,"revision":1}),
            "Not a chart in this Chat",
        ),
        (
            ToolMethod::ChartPublish,
            json!({"mode":"revise","title":"Chart","artifact_id":commit.artifact.id,"edit_token":"unused"}),
            "Not a chart in this Chat",
        ),
        (
            ToolMethod::GridPublish,
            json!({"mode":"revise","layout":"assets","title":"Table","artifact_id":commit.artifact.id,"edit_token":"unused","rows":[],"columns":[],"cells":[]}),
            "Not a table in this Chat",
        ),
    ] {
        field_error(
            executor.execute(&policy, method, params).await.unwrap_err(),
            "artifact_id",
            repair,
        );
    }
}

#[tokio::test]
async fn missing_finance_setup_names_only_the_refused_provider() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let id = crate::finance_data::OPENDART_SOURCE_ID;
    let executor = AppToolExecutor {
        capability_ids: BTreeSet::from([id.into()]),
        ..executor_for(&state, Arc::new(ToolCapture::default()))
    };
    let error = match executor.provider_credential_bundle(id).await {
        Err(error) => error,
        Ok(_) => panic!("fixture has no key"),
    };
    assert!(error.to_string().contains(id));
    let BrokerError::WithNext { next, .. } = error else {
        panic!("setup needs a repair")
    };
    assert!(next.contains(id));
    assert!(!next.contains("SEC EDGAR"));
}

#[tokio::test]
async fn saving_off_is_not_reported_as_memory_reads_off() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(
        &state,
        Arc::new(ToolCapture {
            surface: Some(crate::chat::surface::ChatSurface::Bot),
            ..ToolCapture::default()
        }),
    );
    for method in [
        ToolMethod::BotMemory,
        ToolMethod::LensFile,
        ToolMethod::PrincipleSave,
        ToolMethod::WorldSchemaUpdate,
        ToolMethod::LoupistSetup,
        ToolMethod::JournalSave,
        ToolMethod::JournalReview,
    ] {
        for (read, save, saving_off) in [
            (false, false, false),
            (false, true, false),
            (true, false, true),
        ] {
            let error = executor
                .execute(&policy(read, save), method, json!({}))
                .await
                .unwrap_err();
            assert!(
                if saving_off {
                    matches!(error, BrokerError::ProposalDisabled)
                } else {
                    matches!(error, BrokerError::MemoryDisabled)
                },
                "{method:?}: {error:?}"
            );
        }
    }
}

#[tokio::test]
async fn asset_saves_name_the_disabled_grant_and_preserve_their_validation_codes() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    for (method, field, code) in [
        (ToolMethod::ThesisUpdate, "thesis_update", "thesis_private"),
        (
            ToolMethod::EarningsSnapshotSave,
            "earnings_snapshot_save",
            "earnings_private",
        ),
    ] {
        for (read, save) in [(false, false), (false, true), (true, false)] {
            let error = executor
                .execute(&policy(read, save), method, json!({}))
                .await
                .unwrap_err();
            let BrokerError::InvalidArguments {
                field: actual,
                expected,
            } = error
            else {
                panic!("{error:?}")
            };
            assert_eq!(actual, field);
            assert!(expected.starts_with(code));
            assert!(
                expected.contains(if read {
                    "Saving is off"
                } else {
                    "Memory reads are off"
                }),
                "{expected}"
            );
        }
    }
}
