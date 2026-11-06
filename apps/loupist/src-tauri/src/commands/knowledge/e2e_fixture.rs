use super::*;
use crate::commands::map_internal;

pub(crate) fn seed_e2e_knowledge_fixture(state: &AppState) -> Result<(), CommandError> {
    if std::env::var_os("LOUPIST_E2E_KNOWLEDGE_SEED").as_deref() != Some(std::ffi::OsStr::new("1"))
    {
        return Ok(());
    }
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
        let method = write.create_record(CreateRecord {
            loupist_id: loupist_id.clone(),
            kind: RecordKind::Lens,
            title: "Desk method".into(),
            at_ms: 1_700_000_000_011,
        })?;
        let mut prior = None;
        for (index, (markdown, rationale)) in [
            (
                "Match scope and units before comparing.",
                "Define a repeatable comparison method.",
            ),
            (
                "Match scope and units before comparing. Separate observations from assumptions.",
                "Separate observed evidence from the analyst's assumptions.",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let view = write.revise_lens_with_origin(
                crate::knowledge::ReviseLens {
                    loupist_id: loupist_id.clone(),
                    record_id: method.record_id.clone(),
                    expected_revision_id: prior,
                    at_ms: 1_700_000_000_012 + index as i64,
                    document: crate::knowledge::LensDocument {
                        markdown: markdown.into(),
                        applicability: String::new(),
                        assumptions: String::new(),
                        procedure: String::new(),
                        principles: String::new(),
                        limitations: String::new(),
                    },
                    dependencies: Vec::new(),
                    change: crate::knowledge::LensChangeInput {
                        rationale: rationale.into(),
                        references: Vec::new(),
                        feedback: None,
                    },
                },
                "loupist",
                Some("chat-authoring"),
                Some("assistant-authoring"),
            )?;
            prior = Some(view.revision_id);
        }
        let world = write.create_record(CreateRecord {
            loupist_id: loupist_id.clone(),
            kind: RecordKind::World,
            title: "Sealed world".into(),
            at_ms: 1_700_000_000_020,
        })?;
        write.seal_world(crate::knowledge::SealWorld {
            loupist_id: loupist_id.clone(),
            record_id: world.record_id,
            expected_revision_id: None,
            at_ms: 1_700_000_000_030,
            nodes: fixture_nodes(),
            claims: fixture_claims(retained.span_id.as_deref().unwrap_or_default()),
            annotations: Vec::new(),
            dependencies: vec![crate::knowledge::EvidenceRevision {
                record_id: source.record_id.clone(),
                revision_digest: retained.revision_digest.clone(),
                role: crate::knowledge::DependencyRole::WorldSource,
            }],
        })?;
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
                sensitivity_cards: Vec::new(),
                as_of_report: None,
                alert: None,
                watch: None,
                earnings: None,
                vocabulary: None,
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
        },
    }];
    let research = crate::commands::tool_executor::StagedResearch {
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
                &[],
            )
            .map(|_| ())
            .map_err(|error| crate::store::StoreError::Invalid(error.to_string()))
        })
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let _ = record_id;
    Ok(())
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
    vec![
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
    ]
}
