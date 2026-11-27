//! A turn cutoff hides an ending the as-of projection has not reached.
use super::*;
use crate::knowledge::world_episodes::{
    EndBasis, EpisodeChanges, EpisodeConflict, EpisodeEnd, EpisodeModel, ReferenceBasis,
    WorldEpisode,
};
use crate::knowledge::{
    Authorship, BindingRole, ClaimEndpoint, ClaimStatus, Coverage, CreateRecord, DependencyRole,
    EvidenceBinding, EvidenceRevision, GraphClaim, GraphNode, KnowledgeStore, RecordKind,
    RepresentationRole, RetainMaterial, RetainNewSource, SealWorld, Selector, SourceClass,
    TimePoint, TimePrecision,
};

fn knowledge(state: &AppState) -> KnowledgeStore {
    KnowledgeStore::attach(
        state.store.clone(),
        crate::commands::knowledge::knowledge_blobs(state).unwrap(),
    )
}

fn day(value: &str) -> TimePoint {
    TimePoint {
        precision: TimePrecision::Date,
        value: Some(value.into()),
        approximate: false,
    }
}

fn at(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .timestamp_millis()
}

fn retain(state: &AppState, title: &str, published: &str) -> (String, String, String, String) {
    let retained = knowledge(state)
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: "loupist-a".into(),
            at_ms: at("2026-01-01T00:00:00Z"),
            title: title.into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: format!("fixture:{title}"),
            provider_identity: "fixture".into(),
            authorship: Authorship::Publisher,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: title.into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                selector: Selector::ByteRange { start: 0, end: 4 },
                context_note: title.into(),
                bytes: b"text".to_vec(),
            }),
            publication_time: Some(day(published)),
        })
        .unwrap();
    (
        retained.record_id,
        retained.revision_id,
        retained.revision_digest,
        retained.span_id.unwrap(),
    )
}

fn claim(id: &str, span: &str, end: Option<&str>) -> GraphClaim {
    GraphClaim {
        id: id.into(),
        proposition: format!("proposition {id}"),
        attribution: "fixture".into(),
        relation: "reports".into(),
        endpoints: vec![ClaimEndpoint {
            node_id: "firm".into(),
            role: "subject".into(),
        }],
        qualifiers: Vec::new(),
        evidence: vec![EvidenceBinding {
            span_id: span.into(),
            role: BindingRole::ExtractedFrom,
            attribution: "fixture".into(),
            figure_check: None,
            passage: None,
        }],
        status: ClaimStatus::Active,
        recorded_at_ms: Some(at("2026-01-20T12:00:00Z")),
        publication_time: Some(day("2026-01-15")),
        effective_time: None,
        valid_at: None,
        invalid_at: end.map(day),
        expired_at: None,
    }
}

#[tokio::test]
async fn a_cutoff_hides_a_later_ending_and_an_unknown_witness() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let early = retain(&state, "january-filing", "2026-01-15");
    let later = retain(&state, "june-filing", "2026-06-01");
    knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-02-01T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![
                    claim("later-end", &early.3, Some("2026-06-01")),
                    claim("known-end", &early.3, Some("2026-01-10")),
                    claim("witness", &later.3, None),
                ],
                annotations: Vec::new(),
                dependencies: vec![
                    EvidenceRevision {
                        record_id: early.0.clone(),
                        revision_digest: early.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                    EvidenceRevision {
                        record_id: later.0.clone(),
                        revision_digest: later.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                ],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_cutoff".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id,
                job_id: "job-cutoff".into(),
                sealed_revision_id: "rev-cutoff".into(),
                previous_revision_id: None,
                source_record_id: early.0.clone(),
                source_revision_id: early.1.clone(),
                span_id: early.3.clone(),
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-01-15".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-01-15")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    created: vec!["later-end".into(), "known-end".into()],
                    ended: vec![
                        EpisodeEnd {
                            claim: "later-end".into(),
                            by: Some("witness".into()),
                            end: day("2026-06-01"),
                            basis: EndBasis::Stated,
                        },
                        EpisodeEnd {
                            claim: "known-end".into(),
                            by: Some("witness".into()),
                            end: day("2026-01-10"),
                            basis: EndBasis::Reference,
                        },
                    ],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-01-20T12:00:00Z"),
            })?;
            Ok(())
        })
        .unwrap();
    let ended = |body: &serde_json::Value| {
        body["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "ended")
            .map(|row| row["claimId"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let full = open
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01"}),
        )
        .await
        .unwrap();
    assert_eq!(full["first_run"], false);
    let mut shown = ended(&full);
    shown.sort();
    assert_eq!(shown, ["known-end", "later-end"]);
    assert!(full.to_string().contains("<<untrusted id="), "{full}");
    let compared = open
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"between": {"to": later.0.clone(), "from": early.0.clone()}}),
        )
        .await
        .unwrap();
    assert!(
        compared.to_string().contains("<<untrusted id="),
        "{compared}"
    );
    let embedded = open
        .returning_untrusted(
            ToolMethod::AssetPageRead,
            &json!({"sinceRead": {"changes": [{
                "proposition": "proposition known-end",
                "source": {"title": "january-filing", "readArgs": {
                    "recordId": early.0, "revisionId": early.1, "spanId": early.3
                }}
            }]}}),
        )
        .unwrap();
    assert!(embedded.is_some(), "an embedded feed leaves the turn clean");
    let cutoff = ToolPolicy {
        as_of: Some("2026-03-01".into()),
        ..policy(true, false)
    };
    let hidden = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01"}),
        )
        .await
        .unwrap();
    assert!(ended(&hidden).is_empty(), "{hidden}");
    let refused = open
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"since": "previous_run"}),
        )
        .await
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("since_previous_run_unavailable"),
        "{refused}"
    );
    let capture = Arc::new(ToolCapture {
        previous_run_started_at_ms: Some(at("2026-01-01T00:00:00Z")),
        ..ToolCapture::default()
    });
    let resumed = executor_for(&state, capture);
    let previous = resumed
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"since": "previous_run"}),
        )
        .await
        .unwrap();
    assert_eq!(previous["window"]["since"], at("2026-01-01T00:00:00Z"));
    assert_eq!(previous["first_run"], false);
    let capture = Arc::new(ToolCapture {
        previous_run_started_at_ms: Some(at("2026-01-01T00:00:00Z")),
        first_run: true,
        ..ToolCapture::default()
    });
    let first = executor_for(&state, capture)
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"since": "previous_run"}),
        )
        .await
        .unwrap();
    assert_eq!(first["first_run"], true);
    assert_eq!(first["window"]["since"], at("2026-01-01T00:00:00Z"));
}

#[tokio::test]
async fn a_backfilled_conflict_hides_a_claim_published_after_the_replay() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let early = retain(&state, "january-pair", "2026-01-15");
    let later = retain(&state, "june-pair", "2026-06-01");
    knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-02-01T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![
                    claim("kept", &early.3, None),
                    claim("future", &later.3, None),
                ],
                annotations: Vec::new(),
                dependencies: vec![
                    EvidenceRevision {
                        record_id: early.0.clone(),
                        revision_digest: early.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                    EvidenceRevision {
                        record_id: later.0.clone(),
                        revision_digest: later.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                ],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_backfill_conflict".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id,
                job_id: "job-backfill-conflict".into(),
                sealed_revision_id: "rev-backfill-conflict".into(),
                previous_revision_id: None,
                source_record_id: early.0,
                source_revision_id: early.1,
                span_id: early.3,
                chunk_index: None,
                chunk_count: None,
                char_start: None,
                char_end: None,
                reference_time: None,
                reference_basis: None,
                publication_time: Some(day("2026-01-15")),
                model: None,
                changes: EpisodeChanges {
                    conflicts: vec![EpisodeConflict {
                        claim: "kept".into(),
                        with: "future".into(),
                    }],
                    ..EpisodeChanges::default()
                },
                backfilled: true,
                created_at_ms: at("2026-07-01T12:00:00Z"),
            })?;
            Ok(())
        })
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let cutoff = ToolPolicy {
        as_of: Some("2026-03-01".into()),
        ..policy(true, false)
    };
    let body = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01", "axis": "published", "kinds": ["conflict"]}),
        )
        .await
        .unwrap();
    let partners: Vec<_> = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["withClaimId"].as_str())
        .collect();
    assert!(partners.is_empty(), "{body}");
}

#[tokio::test]
async fn two_visible_claims_do_not_pass_a_conflict_the_projection_dropped() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let early = retain(&state, "january-conflict", "2026-01-15");
    let other = retain(&state, "january-other", "2026-01-20");
    knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-02-01T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![
                    claim("kept", &early.3, None),
                    claim("partner", &early.3, None),
                ],
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: early.0.clone(),
                    revision_digest: early.2.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_conflict".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id,
                job_id: "job-conflict".into(),
                sealed_revision_id: "rev-conflict".into(),
                previous_revision_id: None,
                source_record_id: other.0.clone(),
                source_revision_id: other.1,
                span_id: other.3,
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-01-20".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-01-20")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    conflicts: vec![EpisodeConflict {
                        claim: "kept".into(),
                        with: "partner".into(),
                    }],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-01-21T12:00:00Z"),
            })?;
            Ok(())
        })
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let cutoff = ToolPolicy {
        as_of: Some("2026-03-01".into()),
        ..policy(true, false)
    };
    let body = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01", "axis": "published", "kinds": ["conflict"]}),
        )
        .await
        .unwrap();
    assert!(body["changes"].as_array().unwrap().is_empty(), "{body}");
    let compared = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"between": {"to": other.0, "from": early.0}}),
        )
        .await
        .unwrap();
    assert!(
        compared["conflicts"].as_array().unwrap().is_empty(),
        "{compared}"
    );
}

#[tokio::test]
async fn a_filing_comparison_stages_the_claims_it_returns() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let older = retain(&state, "q2-stage", "2026-06-01");
    let newer = retain(&state, "q3-stage", "2026-09-01");
    knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-09-02T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![claim("new-claim", &newer.3, None)],
                annotations: Vec::new(),
                dependencies: vec![
                    EvidenceRevision {
                        record_id: older.0.clone(),
                        revision_digest: older.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                    EvidenceRevision {
                        record_id: newer.0.clone(),
                        revision_digest: newer.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                ],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_stage".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id,
                job_id: "job-stage".into(),
                sealed_revision_id: "rev-stage".into(),
                previous_revision_id: None,
                source_record_id: newer.0.clone(),
                source_revision_id: newer.1.clone(),
                span_id: newer.3.clone(),
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-09-01".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-09-01")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    created: vec!["new-claim".into()],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-09-02T12:00:00Z"),
            })?;
            Ok(())
        })
        .unwrap();
    let capture = Arc::new(ToolCapture::default());
    let open = executor_for(&state, capture.clone());
    let body = open
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"between": {"to": newer.0, "from": older.0}}),
        )
        .await
        .unwrap();
    assert!(body["readRef"].as_str().unwrap().starts_with("rdg_"));
    let reads = capture.staged_reads.lock().await;
    let view = reads.iter().find_map(|read| match &read.payload {
        super::AckedPayload::World { view_json, .. } => Some(view_json.clone()),
        _ => None,
    });
    assert!(
        view.is_some_and(|view| view.contains("new-claim")),
        "{body}"
    );
}

#[tokio::test]
async fn a_withdrawn_row_without_a_source_id_uses_the_claim_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let filing = retain(&state, "withdrawn-filing", "2026-01-15");
    let world = knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            let mut withdrawn = claim("dropped", &filing.3, None);
            withdrawn.status = ClaimStatus::Withdrawn;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-05-20T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![withdrawn],
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: filing.0.clone(),
                    revision_digest: filing.2.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            Ok(record.record_id)
        })
        .unwrap();
    state
        .store
        .lock_loupist("loupist-a")
        .unwrap()
        .execute(
            "INSERT INTO knowledge_growth_events (
                event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary
             ) VALUES ('gev_withdrawn_row', 'loupist-a', ?1, 'claim_withdrawn', ?2, 'dropped', 'processing', 'withdrew')",
            rusqlite::params![at("2026-05-15T12:00:00Z"), world],
        )
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let body = open
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01", "kinds": ["withdrawn"]}),
        )
        .await
        .unwrap();
    let row = &body["changes"][0];
    assert_eq!(row["kind"], "withdrawn");
    assert_eq!(row["source"]["readArgs"]["recordId"], "");
    let delivered = body.to_string();
    assert!(
        delivered.contains("proposition dropped")
            && delivered.contains("<<untrusted id=")
            && delivered.contains("saved Source: withdrawn-filing"),
        "{body}"
    );
}

#[tokio::test]
async fn a_user_supporting_episode_still_marks_a_publisher_proposition() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let filing = retain(&state, "original-filing", "2026-01-15");
    let note = knowledge(&state)
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: "loupist-a".into(),
            at_ms: at("2026-02-01T00:00:00Z"),
            title: "my-note".into(),
            source_class: SourceClass::UserStatement,
            origin_locator: "note:my-note".into(),
            provider_identity: "user".into(),
            authorship: Authorship::User,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: "my-note".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                selector: Selector::ByteRange { start: 0, end: 4 },
                context_note: "my-note".into(),
                bytes: b"note".to_vec(),
            }),
            publication_time: Some(day("2026-02-02")),
        })
        .unwrap();
    let note_span = note.span_id.clone().unwrap();
    knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-03-01T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![claim("quoted", &filing.3, None)],
                annotations: Vec::new(),
                dependencies: vec![
                    EvidenceRevision {
                        record_id: filing.0.clone(),
                        revision_digest: filing.2.clone(),
                        role: DependencyRole::WorldSource,
                    },
                    EvidenceRevision {
                        record_id: note.record_id.clone(),
                        revision_digest: note.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    },
                ],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_support".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id,
                job_id: "job-support".into(),
                sealed_revision_id: "rev-support".into(),
                previous_revision_id: None,
                source_record_id: note.record_id.clone(),
                source_revision_id: note.revision_id.clone(),
                span_id: note_span,
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-02-02".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-02-02")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    supported: vec!["quoted".into()],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-02-03T12:00:00Z"),
            })?;
            Ok(())
        })
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let body = open
        .execute(
            &policy(true, false),
            ToolMethod::WorldChanges,
            json!({"since": "2026-02-01", "kinds": ["supported"]}),
        )
        .await
        .unwrap();
    let row = &body["changes"][0];
    assert_eq!(row["source"]["readArgs"]["recordId"], note.record_id);
    assert_eq!(row["source"]["title"], "my-note", "{body}");
    let proposition = row["proposition"].as_str().unwrap();
    assert!(
        proposition.contains("saved Source: original-filing"),
        "{body}"
    );
    assert!(!proposition.contains("my-note"), "{body}");
}

#[tokio::test]
async fn a_hidden_filing_does_not_label_the_visible_comparison() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let visible = retain(&state, "q2-visible", "2026-06-01");
    let hidden_bytes = br#"{"company":{"symbol":"LMP"},"form":"10-Q"}"#.to_vec();
    let hidden_end = hidden_bytes.len() as u64;
    let hidden = knowledge(&state)
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: "loupist-a".into(),
            at_ms: at("2026-01-01T00:00:00Z"),
            title: "q3-hidden".into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: "https://hidden.example/q3".into(),
            provider_identity: "fixture".into(),
            authorship: Authorship::Publisher,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: "q3-hidden".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                selector: Selector::ByteRange {
                    start: 0,
                    end: hidden_end,
                },
                context_note: "q3-hidden".into(),
                bytes: hidden_bytes,
            }),
            publication_time: Some(day("2026-09-01")),
        })
        .unwrap();
    knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-06-15T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![claim("kept", &visible.3, None)],
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: visible.0.clone(),
                    revision_digest: visible.2.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_visible".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id,
                job_id: "job-visible".into(),
                sealed_revision_id: "rev-visible".into(),
                previous_revision_id: None,
                source_record_id: visible.0.clone(),
                source_revision_id: visible.1.clone(),
                span_id: visible.3.clone(),
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-06-01".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-06-01")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    created: vec!["kept".into()],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-06-02T12:00:00Z"),
            })?;
            Ok(())
        })
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let cutoff = ToolPolicy {
        as_of: Some("2026-07-01".into()),
        ..policy(true, false)
    };
    let body = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"between": {"to": hidden.record_id, "from": visible.0}}),
        )
        .await
        .unwrap();
    assert!(body["to"]["title"].is_null(), "{body}");
    let delivered = body.to_string();
    assert!(!delivered.contains("hidden.example"), "{body}");
    assert!(!delivered.contains("q3-hidden"), "{body}");
    assert!(delivered.contains("saved Source: q2-visible"), "{body}");
}

#[tokio::test]
async fn a_february_replay_does_not_show_a_may_withdrawal_as_ended() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let filing = retain(&state, "january-status", "2026-01-15");
    let world = knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            let mut withdrawn = claim("kept", &filing.3, None);
            withdrawn.status = ClaimStatus::Withdrawn;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-05-20T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![withdrawn],
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: filing.0.clone(),
                    revision_digest: filing.2.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_added".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id.clone(),
                job_id: "job-added".into(),
                sealed_revision_id: "rev-added".into(),
                previous_revision_id: None,
                source_record_id: filing.0,
                source_revision_id: filing.1,
                span_id: filing.3,
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-01-15".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-01-15")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    created: vec!["kept".into()],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-01-20T12:00:00Z"),
            })?;
            Ok(record.record_id)
        })
        .unwrap();
    state
        .store
        .lock_loupist("loupist-a")
        .unwrap()
        .execute(
            "INSERT INTO knowledge_growth_events (
                event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary
             ) VALUES ('gev_may_withdrawal', 'loupist-a', ?1, 'claim_withdrawn', ?2, 'kept', 'processing', 'withdrew')",
            rusqlite::params![at("2026-05-15T12:00:00Z"), world],
        )
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let cutoff = ToolPolicy {
        as_of: Some("2026-02-28".into()),
        ..policy(true, false)
    };
    let body = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01", "axis": "published", "kinds": ["added"]}),
        )
        .await
        .unwrap();
    let row = &body["changes"][0];
    assert_eq!(row["claimId"], "kept");
    assert_eq!(row["status"], "active", "{body}");
    assert!(row["invalidAt"].is_null(), "{body}");
}

#[tokio::test]
async fn an_april_replay_keeps_a_march_dispute_under_a_may_withdrawal() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let filing = retain(&state, "status-filing", "2026-01-15");
    let world = knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            let mut withdrawn = claim("kept", &filing.3, None);
            withdrawn.status = ClaimStatus::Withdrawn;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-05-20T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![withdrawn],
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: filing.0.clone(),
                    revision_digest: filing.2.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_status".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id.clone(),
                job_id: "job-status".into(),
                sealed_revision_id: "rev-status".into(),
                previous_revision_id: None,
                source_record_id: filing.0,
                source_revision_id: filing.1,
                span_id: filing.3,
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-01-15".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-01-15")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    created: vec!["kept".into()],
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-01-20T12:00:00Z"),
            })?;
            Ok(record.record_id)
        })
        .unwrap();
    state
        .store
        .lock_loupist("loupist-a")
        .unwrap()
        .execute(
            "INSERT INTO knowledge_growth_events (
                event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary
             ) VALUES
             ('gev_march_dispute', 'loupist-a', ?1, 'claim_disputed', ?2, 'kept', 'processing', 'disputed'),
             ('gev_may_withdrawal', 'loupist-a', ?3, 'claim_withdrawn', ?2, 'kept', 'processing', 'withdrew')",
            rusqlite::params![
                at("2026-03-15T12:00:00Z"),
                world,
                at("2026-05-15T12:00:00Z")
            ],
        )
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let cutoff = ToolPolicy {
        as_of: Some("2026-04-30".into()),
        ..policy(true, false)
    };
    let body = open
        .execute(
            &cutoff,
            ToolMethod::WorldChanges,
            json!({"since": "2026-01-01", "axis": "published", "kinds": ["added"]}),
        )
        .await
        .unwrap();
    assert_eq!(body["changes"][0]["status"], "disputed", "{body}");
}

#[tokio::test]
async fn an_as_of_edge_status_matches_the_projected_claim() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let filing = retain(&state, "edge-filing", "2026-01-15");
    let world = knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            let mut withdrawn = claim("kept", &filing.3, None);
            withdrawn.status = ClaimStatus::Withdrawn;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-05-20T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: vec![withdrawn],
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: filing.0.clone(),
                    revision_digest: filing.2,
                    role: DependencyRole::WorldSource,
                }],
            })?;
            Ok(record.record_id)
        })
        .unwrap();
    state
        .store
        .lock_loupist("loupist-a")
        .unwrap()
        .execute(
            "INSERT INTO knowledge_growth_events (
                event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary
             ) VALUES ('gev_edge_withdrawal', 'loupist-a', ?1, 'claim_withdrawn', ?2, 'kept', 'processing', 'withdrew')",
            rusqlite::params![at("2026-05-15T12:00:00Z"), world],
        )
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let read = open
        .execute(
            &ToolPolicy {
                as_of: Some("2026-02-28".into()),
                ..policy(true, false)
            },
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": world, "centerClaimId": "kept"}),
        )
        .await
        .unwrap();
    let text = read["text"].as_str().unwrap();
    let view = serde_json::from_str::<serde_json::Value>(
        crate::custom_connections::taint::delimited_body(text).unwrap_or(text),
    )
    .unwrap();
    let edge = view["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["id"] == "kept")
        .unwrap();
    assert_eq!(edge["claim"]["status"], "active", "{edge}");
    assert_eq!(edge["status"], edge["claim"]["status"], "{edge}");
}

#[tokio::test]
async fn unresolved_comparison_evidence_is_still_labeled() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let marked = open
        .returning_untrusted(
            ToolMethod::WorldChanges,
            &json!({
                "from": {"title": "q2-visible"},
                "to": {"title": null},
                "not_restated": [{
                    "proposition": "visible claim",
                    "evidence": [],
                    "evidenceUnresolved": true
                }]
            }),
        )
        .unwrap()
        .expect("unresolved evidence stays third-party");
    let crate::custom_connections::taint::Returning::Fields(fields) = marked else {
        panic!("expected field labels");
    };
    assert_eq!(
        fields
            .get("/not_restated/0/proposition")
            .map(String::as_str),
        Some("saved Source")
    );
}

#[tokio::test]
async fn an_agent_asset_page_uses_the_full_feed_block() {
    use crate::commands::assets::wire::AssetReferenceOrigin;

    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let filing = retain(&state, "asset-filing", "2026-01-15");
    let asset = state
        .store
        .asset_follow_name(
            "loupist-a",
            "Firm",
            None,
            AssetReferenceOrigin::User,
            at("2026-01-01T00:00:00Z"),
        )
        .unwrap();
    crate::commands::assets::with_assets(&state, "loupist-a", true, |tx| {
        crate::knowledge::assets::ensure_asset_page(
            tx,
            "loupist-a",
            &asset,
            at("2026-01-01T00:00:00Z"),
        )
    })
    .unwrap();
    state
        .store
        .lock_loupist("loupist-a")
        .unwrap()
        .execute(
            "UPDATE knowledge_asset_pages SET entity_id=?3 WHERE loupist_id=?1 AND asset_id=?2",
            rusqlite::params!["loupist-a", asset.asset_id, "firm"],
        )
        .unwrap();
    let world = knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: at("2026-01-01T00:00:00Z"),
            })?;
            let claims = (0..9)
                .map(|index| claim(&format!("c{index}"), &filing.3, None))
                .collect();
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: at("2026-03-01T12:00:00Z"),
                nodes: vec![GraphNode {
                    id: "firm".into(),
                    name: "Firm".into(),
                    aliases: Vec::new(),
                    description: String::new(),
                    kind: "company".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims,
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: filing.0.clone(),
                    revision_digest: filing.2.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            write.insert_world_episode(&WorldEpisode {
                episode_id: "eps_asset".into(),
                loupist_id: "loupist-a".into(),
                world_record_id: record.record_id.clone(),
                job_id: "job-asset".into(),
                sealed_revision_id: "rev-asset".into(),
                previous_revision_id: None,
                source_record_id: filing.0.clone(),
                source_revision_id: filing.1,
                span_id: filing.3,
                chunk_index: Some(0),
                chunk_count: Some(1),
                char_start: Some(0),
                char_end: Some(4),
                reference_time: Some("2026-01-15".into()),
                reference_basis: Some(ReferenceBasis::Published),
                publication_time: Some(day("2026-01-15")),
                model: Some(EpisodeModel {
                    model_profile: "fixture".into(),
                    thinking_level: "low".into(),
                }),
                changes: EpisodeChanges {
                    created: (0..9).map(|index| format!("c{index}")).collect(),
                    ..EpisodeChanges::default()
                },
                backfilled: false,
                created_at_ms: at("2026-01-20T12:00:00Z"),
            })?;
            Ok(record.record_id)
        })
        .unwrap();
    state
        .store
        .lock_loupist("loupist-a")
        .unwrap()
        .execute(
            "INSERT INTO knowledge_graph_outbox (
                job_id, loupist_id, world_record_id, world_revision_id, status,
                payload_json, payload_digest, created_at_ms
             ) VALUES ('job-asset-wait', 'loupist-a', ?1, 'rev', 'queued', ?2, 'd', 1)",
            rusqlite::params![
                world,
                json!({"sourceBinding": {"sourceRecordId": filing.0}}).to_string()
            ],
        )
        .unwrap();
    let lens = knowledge(&state)
        .file_method(
            "loupist-a",
            "Firm notes",
            "The supply ended.",
            &crate::knowledge::test_support::turn_origin("chat", "msg"),
            at("2026-01-10T12:00:00Z"),
        )
        .unwrap();
    knowledge(&state)
        .transact(|write| {
            write.supersede_section(
                "loupist-a",
                &lens.record_id,
                "steps",
                vec!["c0".into()],
                at("2026-01-10T12:00:00Z"),
            )
        })
        .unwrap();
    let open = executor_for(&state, Arc::new(ToolCapture::default()));
    let page = open
        .execute(
            &policy(true, false),
            ToolMethod::AssetPageRead,
            json!({"assetId": asset.asset_id}),
        )
        .await
        .unwrap();
    let since = &page["sinceRead"];
    assert_eq!(since["counts"]["added"], 9, "{page}");
    assert_eq!(since["counts"]["lens"], 1, "{page}");
    // Lens rows come beside the eight changes, not inside them.
    assert_eq!(since["changes"].as_array().unwrap().len(), 8, "{page}");
    assert_eq!(since["pending"], 1, "{page}");
    assert!(
        since["lens"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["sectionKey"] == "steps")),
        "{page}"
    );
}
