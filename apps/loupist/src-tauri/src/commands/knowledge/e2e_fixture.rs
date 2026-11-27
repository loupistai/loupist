use super::*;
use crate::commands::{map_internal, now_ms};
use crate::knowledge::CreateRecord;

pub(crate) fn seed_e2e_knowledge_fixture(state: &AppState) -> Result<(), CommandError> {
    let persistence = match std::env::var("LOUPIST_E2E_KNOWLEDGE_SEED").as_deref() {
        Ok("1") => false,
        Ok("persistence") => true,
        _ => return Ok(()),
    };
    let workspace = std::env::var_os("LOUPIST_E2E_WORKSPACE_DIR")
        .ok_or_else(|| CommandError::internal("the fixture workspace is missing"))?;
    let workspace = std::fs::canonicalize(workspace).map_err(map_internal)?;
    let loupist_id = state
        .store
        .list_loupists()
        .map_err(|error| CommandError::internal(error.to_string()))?
        .into_iter()
        .find(|loupist| std::path::Path::new(&loupist.workspace_root) == workspace)
        .map(|loupist| loupist.id)
        .ok_or_else(|| CommandError::internal("the default Loupist is missing"))?;
    let blobs = knowledge_blobs_for_loupist(state, &loupist_id)?;
    let text = format!("sealed-world-anchor sentence\nSynthetic technology and geopolitics scenario for UI verification.\n{}", fixture_claims("").iter().map(|claim| claim.proposition.as_str()).collect::<Vec<_>>().join("\n")).into_bytes();
    let handles = [
        ("LMPST", "Lamppost Test Works"),
        ("NWPST", "Newpost Test Works"),
    ]
    .into_iter()
    .map(|(symbol, name)| {
        state
            .store
            .seed_followed_listing(&loupist_id, symbol, "US", Some(name))
    })
    .collect::<crate::store::StoreResult<Vec<_>>>()
    .map_err(crate::commands::map_store)?;
    let (
        record_id,
        span_id,
        revision_id,
        revision_digest,
        capture_id,
        representation_id,
        selected,
        source_id,
    ) = with_loupist_knowledge(state, &loupist_id, |tx, blobs| {
        for handle in handles {
            crate::knowledge::assets::ensure_asset_page(
                tx,
                &loupist_id,
                &handle,
                1_700_000_000_000,
            )?;
        }
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let source = write.create_record(CreateRecord {
            loupist_id: loupist_id.clone(),
            kind: RecordKind::Source,
            title: "Sealed notes".into(),
            at_ms: 1_700_000_000_000,
        })?;
        let retained = write.retain_source(crate::knowledge::RetainSource {
            origin: None,
            loupist_id: loupist_id.clone(),
            record_id: source.record_id.clone(),
            expected_revision_id: None,
            at_ms: 1_700_000_000_010,
            source_id: None,
            title: "Sealed note".into(),
            source_class: crate::knowledge::SourceClass::UserStatement,
            origin_locator: "fixture:sealed".into(),
            provider_identity: "eval-fixture".into(),
            authorship: crate::knowledge::Authorship::User,
            coverage: Coverage::IncludedFull,
            origin_chat_id: Some("chat-authoring".into()),
            request_ref: None,
            declared_digest: None,
            publication_time: None,
            material: Some(crate::knowledge::RetainMaterial {
                media_type: "text/plain".into(),
                role: crate::knowledge::RepresentationRole::Original,
                document_id: "sealed-note".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                selector: crate::knowledge::Selector::ByteRange {
                    start: 0,
                    end: text.len() as u64,
                },
                context_note: "Exact sealed note".into(),
                bytes: text.to_vec(),
            }),
        })?;
        {
            use crate::knowledge::lens_page::{
                Basis, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
            };
            let turn = |message: &str| FilingOrigin::Turn {
                chat_id: "chat-authoring".into(),
                message_id: message.into(),
                user_message_id: None,
                unattended: false,
            };
            let steps = |markdown: &str, expected: Option<String>| LensOp::Put {
                key: SectionKey::new("steps").unwrap(),
                heading: "Steps".into(),
                markdown: markdown.into(),
                basis: Basis::default(),
                expected,
            };
            let filed = write.file(
                &loupist_id,
                &turn("assistant-authoring"),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Method,
                        title: "Desk method".into(),
                        anchors: Vec::new(),
                    },
                    ops: vec![
                        LensOp::Summary {
                            text: "Define a repeatable comparison method.".into(),
                        },
                        steps("Match scope and units before comparing.", None),
                    ],
                }],
                1_700_000_000_012,
            )?;
            let page = write.read_page(&loupist_id, &filed[0].record_id)?;
            write.file(
                &loupist_id,
                &turn("assistant-authoring-2"),
                vec![PageDelta {
                    target: PageTarget::Existing {
                        record_id: page.record_id.clone(),
                    },
                    ops: vec![steps(
                        "Match scope and units before comparing. Separate observations from assumptions.",
                        Some(page.body.sections()[0].digest.clone()),
                    )],
                }],
                1_700_000_000_013,
            )?;
        }
        let world = write.create_record(CreateRecord {
            loupist_id: loupist_id.clone(),
            kind: RecordKind::World,
            title: "Sealed world".into(),
            at_ms: 1_700_000_000_020,
        })?;
        let world_id = world.record_id.clone();
        let span_id = retained.span_id.clone().unwrap_or_default();
        write.seal_world(crate::knowledge::SealWorld {
            loupist_id: loupist_id.clone(),
            record_id: world.record_id,
            expected_revision_id: None,
            at_ms: 1_700_000_000_030,
            nodes: fixture_nodes(),
            claims: fixture_claims(&span_id),
            annotations: Vec::new(),
            dependencies: vec![crate::knowledge::EvidenceRevision {
                record_id: source.record_id.clone(),
                revision_digest: retained.revision_digest.clone(),
                role: crate::knowledge::DependencyRole::WorldSource,
            }],
        })?;
        seed_change_feed(
            &write,
            &loupist_id,
            &world_id,
            &source.record_id,
            &retained.revision_id,
            &span_id,
        )?;
        seed_table_source(&write, &loupist_id)?;
        Ok((
            source.record_id,
            retained.span_id.unwrap_or_default(),
            retained.revision_id,
            retained.revision_digest,
            retained.capture_id,
            retained.representation_id.unwrap_or_default(),
            retained.selected_digest.unwrap_or_default(),
            retained.source_id,
        ))
    })?;
    let message =
        |id: &str, role, created_at_ms| -> Result<crate::domain::ChatMessage, CommandError> {
            Ok(crate::domain::ChatMessage {
                tool_refusals: Vec::new(),
                performance: None,
                feedback: None,
                tool_trace: None,
                schedule_receipts: Vec::new(),
                delivery: None,
                replaced_model: None,
                as_of: None,
                number_checks: Vec::new(),
                as_of_report: None,
                alert: None,

                vocabulary: None,
                handoff: None,
                research_session_id: None,
                handoff_note: None,
                handoff_action: None,
                scheduled: None,
                link: None,
                offers: Vec::new(),
                ask: None,
                failure_kind: None,
                host_composed: false,
                steer: false,
                retry_after_seconds: None,
                id: id.into(),
                role,
                status: crate::domain::ChatMessageStatus::Complete,
                content: "The sealed note was read.".into(),
                created_at_ms,
                lens_receipts: Vec::new(),
                memory_refs: Vec::new(),
                observed_exact_count: 0,
                refs_truncated: false,
                refs_digest: crate::domain::memory_refs_digest(&[])
                    .map_err(|error| CommandError::internal(error.to_string()))?,
                memory_update: None,
                memory_revision: None,
                execution_model: None,
                agent_harness: None,
                attachments: Vec::new(),
                artifact_refs: Vec::new(),
                evidence_refs: Vec::new(),
                progress: None,
            })
        };
    let original = crate::domain::ChatSession {
        id: "chat-authoring".into(),
        loupist_id: loupist_id.clone(),
        pi_session_id: "123e4567-e89b-42d3-a456-426614174010".into(),
        pi_session_cache: None,
        title: "Authoring chat".into(),
        title_provisional: false,
        memory_policy: crate::domain::MemoryPolicy {
            use_memory: true,
            update_memory: true,
        },
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: vec![message(
            "user-authoring",
            crate::domain::ChatRole::User,
            10,
        )?],
        created_at_ms: 10,
        updated_at_ms: 10,
        bot_memory: Default::default(),
    };
    state
        .store
        .create_chat(&original)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let mut saved = original.clone();
    saved.messages.push(message(
        "assistant-authoring",
        crate::domain::ChatRole::Assistant,
        20,
    )?);
    saved.updated_at_ms = 20;
    let reads = vec![crate::commands::tool_executor::StagedKnowledgeRead {
        read_ref: span_id.clone(),
        scope: "source".into(),
        record_id: record_id.clone(),
        span_id: Some(span_id.clone()),
        revision_id: revision_id.clone(),
        object_id: source_id,
        title: "Sealed note".into(),
        evidence_origin: None,
        payload: crate::commands::tool_executor::AckedPayload::Source {
            revision_digest,
            capture_id,
            representation_id,
            bytes: text.to_vec(),
            media_type: "text/plain".into(),
            selected_digest: selected,
            coverage: "included_full".into(),
            attribution: "eval-fixture".into(),
            publication_time: None,
            provided: None,
        },
    }];
    let research = crate::commands::tool_executor::StagedResearch {
        thesis_pair: None,
        source_reads: vec![span_id.clone()],
        world_reads: Vec::new(),
        lens_reads: Vec::new(),
        previous_note_ids: Vec::new(),
        working_note: Some(crate::knowledge::WorkingNoteDraft { title: "Sealed research note".into(), markdown: "The sealed source was compared with its attributed World claims. This note is interpretation, not an original source.".into(), change_summary: "Saved the initial investigation.".into(), expected_research_id: None }),
        observations: "The sealed note was delivered.".into(),
        interpretation: "The bytes are the journal input.".into(),
        assumptions: "This turn acknowledged the span.".into(),
        evaluation: "The world claim cites that span.".into(),
        journal: Some(crate::commands::tool_executor::StagedResearchJournal {
            thesis: None,
            at_ms: 1,
            check: Some(crate::knowledge::journal::fixture_prediction()),
            outcome: None,
            judgment: "Keep the sealed note.".into(),
            reasoning: "The acknowledged bytes are the source.".into(),




            earlier: Vec::new(),
            source_reads: vec![span_id],
            world_reads: Vec::new(),
            lens_reads: Vec::new(),

 entities: Vec::new(),
}),
    };
    state
        .store
        .replace_chat_with(&original, &saved, |tx| {
            persist_saved_turn(
                tx,
                &blobs,
                &loupist_id,
                "chat-authoring",
                "assistant-authoring",
                20,
                &[],
                true,
                &[],
                &reads,
                Some(&research),
            )
            .map(|_| ())
            .map_err(|error| crate::store::StoreError::Invalid(error.to_string()))
        })
        .map_err(|error| CommandError::internal(error.to_string()))?;
    seed_handoff_fixture(state, &loupist_id)?;
    if persistence {
        seed_persistence_usage(&state.store, &loupist_id, now_ms())
            .map_err(crate::commands::map_store)?;
    }
    Ok(())
}

fn seed_persistence_usage(
    store: &crate::store::SqliteStore,
    loupist_id: &str,
    at_ms: i64,
) -> crate::store::StoreResult<()> {
    use crate::providers::status::CredentialKind;
    use crate::runs::log::UsageSurface;
    use crate::store::model_usage::{
        ModelUsageData, ModelUsageKind, ModelUsageMeasures, ModelUsageRow, ModelUsageStatus,
    };

    let run_id = "native-persistence-usage";
    store.put_model_usage(
        &format!("chat:{run_id}:{at_ms}"),
        &ModelUsageRow {
            finished_at_ms: at_ms,
            loupist_id: Some(loupist_id.into()),
            task_id: None,
            data: ModelUsageData {
                kind: ModelUsageKind::Chat,
                surface: UsageSurface::Research,
                world_origin: None,
                provider: "openai".into(),
                credential_kind: Some(CredentialKind::Plan),
                model: "fixture-model".into(),
                effort: None,
                chat_id: None,
                run_id: run_id.into(),
                started_at_ms: at_ms,
                duration_ms: 0,
                status: ModelUsageStatus::Ok,
                failure_kind: None,
                measures: ModelUsageMeasures {
                    requests: Some(37),
                    input: Some(200),
                    cached_input: Some(100),
                    output: Some(30),
                    ..Default::default()
                },
                accounts: Vec::new(),
            },
        },
    )
}

#[test]
fn persistence_usage_seed_counts_in_the_catalog_summary() {
    use crate::commands::usage::{summarize, UsageSummaryRequest};
    use crate::providers::status::CredentialKind;
    use crate::runs::log::UsageSurface;

    let root = tempfile::tempdir().unwrap();
    let store =
        crate::store::SqliteStore::open_catalog(root.path().join("catalog.sqlite3")).unwrap();
    let loupist_id = "persistence-usage";
    crate::test_fixtures::create_folder_loupist(&store, loupist_id, &root.path().join("workspace"));
    let at_ms = now_ms();
    seed_persistence_usage(&store, loupist_id, at_ms).unwrap();
    let read = |from_ms, to_ms| {
        summarize(
            UsageSummaryRequest {
                from_ms,
                to_ms,
                time_zone: "Asia/Seoul".into(),
            },
            &store,
            |_| CredentialKind::None,
        )
        .unwrap()
    };
    let summary = read(at_ms, at_ms + 1);
    assert_eq!(summary.totals.requests, 37);
    assert_eq!(summary.surfaces[0].surface, UsageSurface::Research);
    assert_eq!(summary.loupists[0].loupist_id.as_deref(), Some(loupist_id));
    assert_eq!(read(at_ms - 1, at_ms).totals.requests, 0);
}

fn seed_table_source(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
) -> crate::knowledge::KnowledgeResult<()> {
    use crate::knowledge::{Authorship, RepresentationRole, RetainMaterial, RetainNewSource};

    let bytes = serde_json::to_vec(&serde_json::json!({
        "dataset_ref": {},
        "data": {
            "columns": {"date": "Date", "units": "Units produced"},
            "rows": [
                {"date": "2026-03-30", "units": 80},
                {"date": "2026-03-31", "units": 90}
            ]
        }
    }))?;
    let end = bytes.len() as u64;
    write.retain_new_source(RetainNewSource {
        origin: None,
        loupist_id: loupist_id.into(),
        at_ms: 1_700_000_000_065,
        title: "Fixture production table".into(),
        source_class: crate::knowledge::SourceClass::ToolResult,
        origin_locator: "https://fixtures.invalid/production".into(),
        provider_identity: "eval-fixture".into(),
        authorship: Authorship::Tool,
        coverage: Coverage::IncludedFull,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time: None,
        material: Some(RetainMaterial {
            media_type: "application/json".into(),
            role: RepresentationRole::ProviderPayload,
            document_id: "fixture-production-table".into(),
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: Vec::new(),
            selector: crate::knowledge::Selector::ByteRange { start: 0, end },
            context_note: "Exact fixture table".into(),
            bytes,
        }),
    })?;
    Ok(())
}

fn seed_handoff_fixture(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    use crate::{
        chat::handoff::LinkPath,
        chat::home::{ensure_home_thread, host_message},
        commands::{map_store, now_ms},
        domain::{
            ChatRole, ChatSession, HandoffAction, HandoffNote, HomeLink, HomeLinkHop, MemoryPolicy,
        },
    };
    use serde_json::json;

    let now = now_ms();
    // The native persistence profile has no model catalog. Use fixture IDs
    // absent from Pi's registry so only the scripted cases can send.
    let mut models = vec![crate::providers::catalog::ConfiguredModel {
        id: crate::commands::handoff_fixture::MODEL_PROFILE.into(),
        name: "Fixture research model".into(),
        provider: "openai".into(),
        model: "loupist-e2e-link-research".into(),
        // Images too, so the native suite can send an attached image under the shipped policy.
        input: vec!["text".into(), "image".into()],
        reasoning: false,
        context_window: 32_000,
        max_tokens: 4_096,
        thinking_levels: vec!["off".into()],
        thinking_level_map: Default::default(),
        run_controls: Vec::new(),
    }];
    if crate::commands::handoff_fixture::release_learning() {
        let mut learning = models[0].clone();
        learning.id = crate::commands::handoff_fixture::LEARNING_MODEL_PROFILE.into();
        learning.name = "Fixture learning model".into();
        learning.model = "loupist-e2e-release-learning".into();
        models.push(learning);
    }
    state.replace_provider_models("openai", models)?;
    let home = ensure_home_thread(state, loupist_id)?;
    let mut updated = home.clone();
    let request_text =
        "Compare the sealed site's confirmed supply changes, delivery dates and sources. "
            .repeat(30)
            + "Answer in one table and skip any forecasts.";
    let mut user = host_message("fixture-user", request_text, now)?;
    user.role = ChatRole::User;
    user.host_composed = false;
    updated.messages.push(user.clone());
    let mut post = host_message("fixture-link", "A retained World link.".into(), now)?;
    let path = vec![
        HomeLinkHop {
            name: "Sealed Hub".into(),
            relation: "SUPPLIES".into(),
            reversed: false,
        },
        HomeLinkHop {
            name: "Newpost Test Works".into(),
            relation: "SUPPLIES".into(),
            reversed: false,
        },
    ];
    post.link = Some(HomeLink {
        connection_id: "con_fixture_supply".into(),
        seed_name: "Lamppost Test Works".into(),
        far_name: "Newpost Test Works".into(),
        node_ids: vec![
            "fixture-lamp".into(),
            "fixture-hub".into(),
            "fixture-newpost".into(),
        ],
        path,
    });
    updated.messages.push(post);
    updated.updated_at_ms = now.max(home.updated_at_ms);

    let mut typed = user.clone();
    typed.handoff_note = Some(HandoffNote {
        title: "Sealed supply check".into(),
        question: Some("Who else supplies the sealed part?".into()),
        brief: Some("Compare retained evidence and dated filings.".into()),
        untrusted_id: Some(crate::custom_connections::taint::fresh_id()),
    });
    let typed = vec![
        typed,
        host_message(
            "fixture-reply",
            "The saved sources were checked.".into(),
            now,
        )?,
    ];
    let mut action = user;
    action.id = super::super::new_id("fixture-action");
    action.content = crate::chat::handoff::LINK_CAPTION.into();
    let path = LinkPath::from(&HomeLink {
        connection_id: "con_fixture_hub".into(),
        seed_name: "Sealed Hub".into(),
        far_name: "Lamppost Test Works".into(),
        node_ids: vec!["fixture-hub".into(), "fixture-lamp".into()],
        path: vec![HomeLinkHop {
            name: "Lamppost Test Works".into(),
            relation: "SUPPLIES".into(),
            reversed: true,
        }],
    });
    let link_title = path.title("en");
    action.handoff_action = Some(HandoffAction::Link {
        connection_id: "con_fixture_hub".into(),
    });
    action.handoff_note = Some(HandoffNote {
        title: link_title.clone(),
        question: None,
        brief: Some(serde_json::to_string(&path).map_err(map_internal)?),
        untrusted_id: Some(crate::custom_connections::taint::fresh_id()),
    });
    let action = vec![
        action,
        host_message(
            "fixture-reply",
            "The stored relationship was checked.".into(),
            now,
        )?,
    ];
    let session = |id: &str, title: &str, mut messages: Vec<crate::domain::ChatMessage>| {
        for message in &mut messages {
            // These are already-settled fixture replies, not Bot announcements.
            message.host_composed = false;
        }
        ChatSession {
            id: id.into(),
            loupist_id: loupist_id.into(),
            pi_session_id: uuid::Uuid::new_v4().to_string(),
            pi_session_cache: None,
            title: title.into(),
            title_provisional: false,
            memory_policy: MemoryPolicy {
                use_memory: false,
                update_memory: false,
            },
            set_aside_memory: Vec::new(),
            versions: Vec::new(),
            messages,
            created_at_ms: now,
            updated_at_ms: now,
            bot_memory: Default::default(),
        }
    };
    let chats = [
        session("chat-fixture-bot-handoff", "Sealed supply check", typed),
        session("chat-fixture-link-handoff", &link_title, action),
        session(
            "chat-fixture-legacy-handoff",
            "Pre-update research",
            Vec::new(),
        ),
    ];
    for chat in &chats {
        chat.validate().map_err(map_internal)?;
    }
    let path = json!({"seedName":"Lamppost Test Works","hops":[
        {"claimId":"fixture-link-hub","relation":"SUPPLIES","fromRole":"source","toRole":"target",
            "reversed":false,"nodeId":"fixture-hub","name":"Sealed Hub"},
        {"claimId":"fixture-link-newpost","relation":"SUPPLIES","fromRole":"source","toRole":"target",
            "reversed":false,"nodeId":"fixture-newpost","name":"Newpost Test Works"},
    ]});
    let legacy = json!({
        "id":"handoff-fixture-legacy","loupist_id":loupist_id,"request_key":"fixture-legacy",
        "chat_id":chats[2].id,"title":chats[2].title,"prompt":"Pre-update synthetic request.",
        "run":{"model_profile_id":"fixture","thinking_level":"medium","run_options":{},
            "use_memory":false,"update_memory":false},
        "status":"created","run_id":null,"message_id":null,"error":null,
        "created_at_ms":now,"finished_at_ms":null,
    });
    state.store.replace_chat_with(&home, &updated, |tx| {
        for chat in &chats {
            tx.execute("INSERT INTO chat_sessions(id,loupist_id,updated_at_ms,data_json) VALUES(?1,?2,?3,?4)",
                rusqlite::params![chat.id, loupist_id, now, serde_json::to_string(chat)?])?;
        }
        tx.execute("INSERT INTO knowledge_connections(connection_id,loupist_id,seed_node_id,far_node_id,
            path_json,score,status,created_at_ms,status_at_ms,shown_at_ms)
            VALUES('con_fixture_supply',?1,'fixture-lamp','fixture-newpost',?2,1,'shown',?3,?3,?3)",
            rusqlite::params![loupist_id, path.to_string(), now])?;
        tx.execute("INSERT INTO chat_handoffs(id,loupist_id,request_key,chat_id,status,created_at_ms,data_json)
            VALUES('handoff-fixture-legacy',?1,'fixture-legacy',?2,'created',?3,?4)",
            rusqlite::params![loupist_id, chats[2].id, now, legacy.to_string()])?;
        Ok(())
    }).map_err(map_store)
}

fn seed_change_feed(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    world_id: &str,
    source_id: &str,
    source_revision: &str,
    span_id: &str,
) -> crate::knowledge::KnowledgeResult<()> {
    use crate::knowledge::lens_page::{
        Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
    };
    use crate::knowledge::world_episodes::{
        EndBasis, EpisodeChanges, EpisodeConflict, EpisodeEnd, EpisodeModel, ReferenceBasis,
        WorldEpisode,
    };
    let episode = |id: &str, at: i64, changes: EpisodeChanges| WorldEpisode {
        episode_id: id.into(),
        loupist_id: loupist_id.into(),
        world_record_id: world_id.into(),
        job_id: format!("job-{id}"),
        sealed_revision_id: format!("rev-{id}"),
        previous_revision_id: None,
        source_record_id: source_id.into(),
        source_revision_id: source_revision.into(),
        span_id: span_id.into(),
        chunk_index: Some(0),
        chunk_count: Some(1),
        char_start: Some(0),
        char_end: Some(8),
        reference_time: Some("2026-03-31".into()),
        reference_basis: Some(ReferenceBasis::Published),
        publication_time: Some(crate::knowledge::TimePoint {
            precision: crate::knowledge::TimePrecision::Date,
            value: Some("2026-03-31".into()),
            approximate: false,
        }),
        model: Some(EpisodeModel {
            model_profile: "fixture".into(),
            thinking_level: "low".into(),
        }),
        changes,
        backfilled: false,
        created_at_ms: at,
    };
    write.insert_world_episode(&episode(
        "fixture-created",
        1_700_000_000_040,
        EpisodeChanges {
            created: vec![
                "fixture-supply-ended".into(),
                "fixture-guidance-high".into(),
                "fixture-guidance-low".into(),
            ],
            ..EpisodeChanges::default()
        },
    ))?;
    write.insert_world_episode(&episode(
        "fixture-ended",
        1_700_000_000_050,
        EpisodeChanges {
            created: vec!["fixture-supply-ending".into()],
            ended: vec![EpisodeEnd {
                claim: "fixture-supply-ended".into(),
                by: Some("fixture-supply-ending".into()),
                end: crate::knowledge::TimePoint {
                    precision: crate::knowledge::TimePrecision::Date,
                    value: Some("2026-03-31".into()),
                    approximate: false,
                },
                basis: EndBasis::Stated,
            }],
            conflicts: vec![EpisodeConflict {
                claim: "fixture-guidance-high".into(),
                with: "fixture-guidance-low".into(),
            }],
            ..EpisodeChanges::default()
        },
    ))?;
    let filed = write.file(
        loupist_id,
        &FilingOrigin::Turn {
            chat_id: "chat-authoring".into(),
            message_id: "assistant-authoring".into(),
            user_message_id: None,
            unattended: false,
        },
        vec![PageDelta {
            target: PageTarget::Create {
                kind: PageKind::Subject,
                title: "Fixture supply".into(),
                anchors: Vec::new(),
            },
            ops: vec![LensOp::Put {
                key: SectionKey::new("supply").unwrap(),
                heading: "Supply".into(),
                markdown: "The supply agreement.".into(),
                basis: Basis {
                    refs: vec![BasisRef::Claim {
                        claim_id: "fixture-supply-ended".into(),
                    }],
                    uncertainty: None,
                },
                expected: None,
            }],
        }],
        1_700_000_000_055,
    )?;
    write.supersede_section(
        loupist_id,
        &filed[0].record_id,
        "supply",
        vec!["fixture-supply-ended".into()],
        1_700_000_000_060,
    )
}

fn fixture_nodes() -> Vec<crate::knowledge::GraphNode> {
    let mut nodes = Vec::new();
    let add =
        |nodes: &mut Vec<crate::knowledge::GraphNode>, id: &str, name: &str, description: &str| {
            nodes.push(crate::knowledge::GraphNode {
                id: id.into(),
                name: name.into(),
                aliases: Vec::new(),
                description: description.into(),
                kind: if ["ada"].contains(&id) {
                    "person"
                } else if ["harbor", "strait", "border"].contains(&id) {
                    "place"
                } else if ["ministry", "alliance", "fleet", "bloc", "embassy"].contains(&id) {
                    "organization"
                } else if [
                    "treaty", "sanction", "election", "tariff", "charter", "license", "standard",
                ]
                .contains(&id)
                {
                    "policy"
                } else {
                    "technology"
                }
                .into(),
                source_role: "source".into(),
                properties: std::collections::BTreeMap::new(),
            });
        };
    add(
        &mut nodes,
        "ada",
        "Ada",
        "Researcher describing the synthetic engineering scenario.",
    );
    add(
        &mut nodes,
        "engine",
        "Engine",
        "A programmable machine used in the synthetic scenario.",
    );
    for (id, name) in [
        ("fixture-lamp", "Lamppost Test Works"),
        ("fixture-hub", "Sealed Hub"),
        ("fixture-newpost", "Newpost Test Works"),
        ("compiler", "Compiler"),
        ("protocol", "Protocol"),
        ("chip", "Chip"),
        ("battery", "Battery"),
        ("satellite", "Satellite"),
        ("model", "Model"),
        ("dataset", "Dataset"),
        ("robot", "Robot"),
        ("sensor", "Sensor"),
        ("network", "Network"),
        ("firmware", "Firmware"),
        ("latency", "Latency"),
        ("standard", "Standard"),
        ("license", "License"),
        ("harbor", "Harbor"),
        ("strait", "Strait"),
        ("treaty", "Treaty"),
        ("sanction", "Sanction"),
        ("fleet", "Fleet"),
        ("ministry", "Ministry"),
        ("election", "Election"),
        ("border", "Border"),
        ("alliance", "Alliance"),
        ("currency", "Currency"),
        ("pipeline", "Pipeline"),
        ("embassy", "Embassy"),
        ("tariff", "Tariff"),
        ("bloc", "Bloc"),
        ("reserve", "Reserve"),
        ("charter", "Charter"),
    ] {
        add(
            &mut nodes,
            id,
            name,
            &format!("{name} in the synthetic technology and geopolitics scenario."),
        );
    }
    nodes
}

fn fixture_claims(span_id: &str) -> Vec<crate::knowledge::GraphClaim> {
    let claim = |id: &str, proposition: &str, relation: &str, source: &str, target: &str| {
        crate::knowledge::GraphClaim {
            id: id.into(),
            proposition: proposition.into(),
            attribution: "eval-fixture".into(),
            relation: relation.into(),
            endpoints: vec![
                crate::knowledge::ClaimEndpoint {
                    node_id: source.into(),
                    role: "source".into(),
                },
                crate::knowledge::ClaimEndpoint {
                    node_id: target.into(),
                    role: "target".into(),
                },
            ],
            qualifiers: Vec::new(),
            evidence: vec![crate::knowledge::EvidenceBinding {
                span_id: span_id.into(),
                role: crate::knowledge::BindingRole::ExtractedFrom,
                attribution: "eval-fixture".into(),
                figure_check: None,
                passage: None,
            }],
            status: crate::knowledge::ClaimStatus::Active,
            recorded_at_ms: Some(1_700_000_000_030),
            publication_time: None,
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        }
    };
    let mut claims = vec![
        claim(
            "fixture-link-hub",
            "Lamppost Test Works supplies Sealed Hub.",
            "SUPPLIES",
            "fixture-lamp",
            "fixture-hub",
        ),
        claim(
            "fixture-link-newpost",
            "Sealed Hub supplies Newpost Test Works.",
            "SUPPLIES",
            "fixture-hub",
            "fixture-newpost",
        ),
        claim(
            "described",
            "Ada described the engine.",
            "DESCRIBED",
            "ada",
            "engine",
        ),
        claim(
            "c-compiler",
            "The compiler builds the firmware.",
            "BUILDS",
            "compiler",
            "firmware",
        ),
        claim(
            "c-chip",
            "The chip supplies the robot.",
            "SUPPLIES",
            "chip",
            "robot",
        ),
        claim(
            "c-battery",
            "The battery supplies the sensor.",
            "SUPPLIES",
            "battery",
            "sensor",
        ),
        claim(
            "c-satellite",
            "The satellite relays the communication network.",
            "RELAYS",
            "satellite",
            "network",
        ),
        claim(
            "c-model",
            "The model depends on the dataset.",
            "DEPENDS_ON",
            "model",
            "dataset",
        ),
        claim(
            "c-firmware",
            "The firmware constrains the latency.",
            "CONSTRAINS",
            "firmware",
            "latency",
        ),
        claim(
            "c-standard",
            "The standard defines the communication protocol.",
            "DEFINES",
            "standard",
            "protocol",
        ),
        claim(
            "c-harbor",
            "The harbor is located in the strait.",
            "LOCATED_IN",
            "harbor",
            "strait",
        ),
        claim(
            "c-treaty",
            "The treaty constrains the sanction.",
            "CONSTRAINS",
            "treaty",
            "sanction",
        ),
        claim(
            "c-fleet",
            "The fleet is located in the harbor.",
            "LOCATED_IN",
            "fleet",
            "harbor",
        ),
        claim(
            "c-ministry",
            "The election changes the ministry leadership.",
            "CHANGES",
            "election",
            "ministry",
        ),
        claim(
            "c-border",
            "The alliance monitors the border.",
            "MONITORS",
            "alliance",
            "border",
        ),
        claim(
            "c-currency",
            "The currency depends on the reserve.",
            "DEPENDS_ON",
            "currency",
            "reserve",
        ),
        claim(
            "c-pipeline",
            "The pipeline supplies the bloc.",
            "SUPPLIES",
            "pipeline",
            "bloc",
        ),
        claim(
            "c-embassy",
            "The embassy reports to the ministry.",
            "REPORTS_TO",
            "embassy",
            "ministry",
        ),
        claim(
            "c-tariff",
            "The tariff constrains the harbor.",
            "CONSTRAINS",
            "tariff",
            "harbor",
        ),
        claim(
            "c-charter",
            "The charter constrains the treaty.",
            "CONSTRAINS",
            "charter",
            "treaty",
        ),
        claim(
            "c-ada-compiler",
            "Ada described the compiler.",
            "DESCRIBED",
            "ada",
            "compiler",
        ),
        claim(
            "c-engine-chip",
            "The engine depends on the chip.",
            "DEPENDS_ON",
            "engine",
            "chip",
        ),
        claim(
            "c-robot-sensor",
            "The robot reads the sensor.",
            "READS",
            "robot",
            "sensor",
        ),
        claim(
            "c-robot-model",
            "The robot runs the model.",
            "RUNS",
            "robot",
            "model",
        ),
        claim(
            "c-model-chip",
            "The model runs on the chip.",
            "RUNS_ON",
            "model",
            "chip",
        ),
        claim(
            "c-sensor-data",
            "The sensor contributes to the dataset.",
            "CONTRIBUTES_TO",
            "sensor",
            "dataset",
        ),
        claim(
            "c-network-protocol",
            "The network implements the protocol.",
            "IMPLEMENTS",
            "network",
            "protocol",
        ),
        claim(
            "c-network-latency",
            "The network is constrained by latency.",
            "CONSTRAINED_BY",
            "network",
            "latency",
        ),
        claim(
            "c-sanction-chip",
            "The sanction restricts semiconductor supply.",
            "RESTRICTS",
            "sanction",
            "chip",
        ),
        claim(
            "c-license-chip",
            "The export license permits semiconductor supply.",
            "PERMITS",
            "license",
            "chip",
        ),
        claim(
            "c-tariff-battery",
            "The tariff affects battery supply.",
            "AFFECTS",
            "tariff",
            "battery",
        ),
        claim(
            "c-pipeline-border",
            "The pipeline crosses the border.",
            "CROSSES",
            "pipeline",
            "border",
        ),
        claim(
            "c-alliance-treaty",
            "The alliance adopts the treaty.",
            "ADOPTS",
            "alliance",
            "treaty",
        ),
        claim(
            "c-bloc-currency",
            "The trade bloc settles using the currency.",
            "USES",
            "bloc",
            "currency",
        ),
        claim(
            "c-fleet-strait",
            "The fleet patrols the strait.",
            "PATROLS",
            "fleet",
            "strait",
        ),
        claim(
            "c-harbor-chip",
            "The harbor handles semiconductor shipments.",
            "HANDLES",
            "harbor",
            "chip",
        ),
    ];
    let mut ended = claim(
        "fixture-supply-ended",
        "Lamppost Test Works supplied the harbor.",
        "SUPPLIED",
        "fixture-lamp",
        "harbor",
    );
    ended.invalid_at = Some(crate::knowledge::TimePoint {
        precision: crate::knowledge::TimePrecision::Date,
        value: Some("2026-03-31".into()),
        approximate: false,
    });
    ended.qualifiers = vec![
        crate::knowledge::Qualifier {
            key: crate::knowledge::END_BASIS.into(),
            value: "stated".into(),
        },
        crate::knowledge::Qualifier {
            key: crate::knowledge::SUPERSEDED_BY_CLAIM.into(),
            value: "fixture-supply-ending".into(),
        },
    ];
    let ending = claim(
        "fixture-supply-ending",
        "The supply agreement ended.",
        "ENDED",
        "fixture-lamp",
        "harbor",
    );
    let mut high = claim(
        "fixture-guidance-high",
        "Guidance was raised.",
        "GUIDED",
        "fixture-lamp",
        "fixture-lamp",
    );
    high.qualifiers = vec![crate::knowledge::Qualifier {
        key: crate::knowledge::CONFLICTS_WITH_CLAIM.into(),
        value: "fixture-guidance-low".into(),
    }];
    let mut low = claim(
        "fixture-guidance-low",
        "Guidance was cut.",
        "GUIDED",
        "fixture-lamp",
        "fixture-lamp",
    );
    low.qualifiers = vec![crate::knowledge::Qualifier {
        key: crate::knowledge::CONFLICTS_WITH_CLAIM.into(),
        value: "fixture-guidance-high".into(),
    }];
    claims.extend([ended, ending, high, low]);
    claims
}
