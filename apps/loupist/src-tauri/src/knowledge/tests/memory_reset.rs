use super::*;
use crate::knowledge::{memory_controls, memory_reset};

fn node(id: &str, name: &str, kind: &str, properties: BTreeMap<String, Value>) -> GraphNode {
    GraphNode {
        source_role: "source".into(),
        ..super::node(id, name, kind, properties)
    }
}

fn settings_table(write: &KnowledgeWrite<'_>) {
    write
        .tx
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS app_settings(id TEXT PRIMARY KEY, data_json TEXT NOT NULL)",
        )
        .unwrap();
}

fn method(store: &KnowledgeStore, title: &str, actor: &str) -> LensView {
    let record = create(store, "loupist-a", RecordKind::Lens, title);
    store
        .transact(|write| {
            write.revise_lens_with_origin(
                lens_request("loupist-a", &record.record_id, None, title),
                actor,
                None,
                None,
            )
        })
        .unwrap()
}

#[test]
fn reset_removes_only_loupist_authored_memory_and_queues_world_reload() {
    let (_temp, store) = open_store();
    store
        .transact(|write| {
            settings_table(write);
            Ok(())
        })
        .unwrap();
    let learned = method(&store, "Learned research method", "loupist");
    let learned_principle = method(&store, "Learned principle", "loupist");
    store
        .transact(|write| {
            write.tx.execute(
                "UPDATE knowledge_lens_metadata SET lens_kind='principle' WHERE record_id=?1",
                [&learned_principle.record_id],
            )?;
            Ok(())
        })
        .unwrap();
    let user_method = method(&store, "My own method", "user");
    let shared_method = method(&store, "Shared method", "import");
    let principle = store
        .transact(|write| {
            principles::principle_save(
                write.tx,
                "loupist-a",
                None,
                None,
                PrincipleInput {
                    statement: "Check cash conversion".into(),
                    why: None,
                    example: None,
                    origin: "settings".into(),
                    origin_chat_id: None,
                    origin_message_id: None,
                },
                AT,
            )
        })
        .unwrap();
    let retained = |text: &[u8], actor: &str, authorship: Authorship| {
        let mut request = source_request("loupist-a", text);
        request.authorship = authorship;
        request.origin_chat_id = (actor == "loupist").then(|| "chat-research".into());
        store.retain_new_source(request).unwrap()
    };
    let research = retained(
        b"A public source saved during research",
        "loupist",
        Authorship::Publisher,
    );
    let manual = retained(
        b"A public source saved by the user",
        "user",
        Authorship::Publisher,
    );
    let user_source = retained(
        b"A private user file saved from Chat",
        "loupist",
        Authorship::User,
    );
    let shared_source = retained(
        b"An imported source",
        "import",
        Authorship::ImportedContributor,
    );
    let world = create(&store, "loupist-a", RecordKind::World, "World");
    let mut nodes = Vec::new();
    let mut claims = Vec::new();
    let mut dependencies = Vec::new();
    let mut head = None;
    for (actor, prefix, source) in [
        ("user", "user", &manual),
        ("import", "shared", &shared_source),
        ("loupist", "learned", &research),
    ] {
        let (left, right) = (format!("{prefix}-a"), format!("{prefix}-b"));
        nodes.extend([
            node(&left, &left, "Organization", BTreeMap::new()),
            node(&right, &right, "Organization", BTreeMap::new()),
        ]);
        let mut link = claim(
            &format!("{prefix}-claim"),
            &format!("{prefix} relationship"),
            &left,
            &right,
        );
        link.evidence.push(EvidenceBinding {
            span_id: source.span_id.clone().unwrap(),
            role: BindingRole::Supports,
            attribution: "Exact source".into(),
        });
        claims.push(link);
        dependencies.push(EvidenceRevision {
            record_id: source.record_id.clone(),
            revision_digest: source.revision_digest.clone(),
            role: DependencyRole::WorldSource,
        });
        let sealed = store
            .transact(|write| {
                super::super::world::seal_world_with_actor(
                    write.tx,
                    SealWorld {
                        loupist_id: "loupist-a".into(),
                        record_id: world.record_id.clone(),
                        expected_revision_id: head.clone(),
                        at_ms: AT,
                        nodes: nodes.clone(),
                        claims: claims.clone(),
                        annotations: Vec::new(),
                        dependencies: dependencies.clone(),
                    },
                    actor,
                    None,
                    None,
                )
            })
            .unwrap();
        head = Some(sealed.revision_id);
    }
    let counts = store
        .read(|write| memory_reset::preview(write.tx, "loupist-a"))
        .unwrap();
    assert_eq!(
        counts,
        memory_reset::ResetCounts {
            principles: 1,
            methods: 1,
            names: 2,
            relationships: 1,
            sources: 1
        }
    );
    store
        .transact(|write| memory_reset::reset(write.tx, write.blobs, "loupist-a", &counts, AT + 1))
        .unwrap();
    store.read(|write| {
        assert!(write.owned_record("loupist-a", &learned.record_id).is_err());
        assert!(write.owned_record("loupist-a", &learned_principle.record_id).is_err());
        assert!(write.get_revision("loupist-a", &research.revision_id).is_err());
        for id in [&user_method.record_id, &shared_method.record_id, &principle.record_id, &manual.record_id, &user_source.record_id, &shared_source.record_id] { write.owned_record("loupist-a", id)?; }
        let head = write.owned_record("loupist-a", &world.record_id)?.revision_id.unwrap();
        let body: WorldSnapshotBody = serde_json::from_value(write.get_revision("loupist-a", &head)?.payload)?;
        assert_eq!(body.nodes.len(), 4);
        assert_eq!(body.claims.len(), 2);
        assert!(body.claims.iter().all(|c| !c.id.starts_with("learned")));
        // The retained immutable source still exists, but every public read is denied.
        let retained: bool = write.tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_source_captures WHERE record_id=?1)", [&research.record_id], |r| r.get(0))?;
        assert!(retained);
        let reload: bool = write.tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox WHERE loupist_id='loupist-a' AND status='queued')", [], |r| r.get(0))?;
        assert!(reload);
        let events: i64 = write.tx.query_row("SELECT count(*) FROM knowledge_growth_events WHERE kind='lens_archived' AND json_extract(origin,'$.kind')='memory_reset'", [], |r| r.get(0))?;
        assert_eq!(events, 1);
        assert_eq!(memory_reset::preview(write.tx, "loupist-a")?, memory_reset::ResetCounts::default());
        Ok(())
    }).unwrap();
}

#[test]
fn reset_refuses_a_running_world_job_without_changing_lens_or_source() {
    let (_temp, store) = open_store();
    let learned = method(&store, "A method", "loupist");
    let source = retain_text(&store, "loupist-a", b"Research material");
    store
        .transact(|write| {
            settings_table(write);
            let settings = GraphProcessingSettings {
                enrichment_enabled: true,
                helper_provider: Some("fixture".into()),
                helper_model: Some("fixture-local".into()),
                ..Default::default()
            };
            assert!(write
                .enqueue_source_world("loupist-a", &source, &settings, AT)?
                .is_some());
            write.tx.execute(
                "UPDATE knowledge_graph_outbox SET status='running' WHERE loupist_id='loupist-a'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let counts = store
        .read(|write| memory_reset::preview(write.tx, "loupist-a"))
        .unwrap();
    let error = store
        .transact(|write| memory_reset::reset(write.tx, write.blobs, "loupist-a", &counts, AT + 1))
        .unwrap_err();
    assert!(matches!(
        error,
        KnowledgeError::Invalid("world processing is busy")
    ));
    store
        .read(|write| {
            write.owned_record("loupist-a", &learned.record_id)?;
            assert!(memory_controls::load(write.tx, "loupist-a")?
                .removed
                .is_empty());
            Ok(())
        })
        .unwrap();
}

#[test]
fn reset_keeps_user_claims_and_independent_support_and_removes_owned_unbacked_claims() {
    let (_temp, store) = open_store();
    store
        .transact(|write| {
            settings_table(write);
            Ok(())
        })
        .unwrap();
    let mut request = source_request("loupist-a", b"Research retained by the Loupist");
    request.origin_chat_id = Some("chat-research".into());
    let own = store.retain_new_source(request).unwrap();
    let other = retain_text(&store, "loupist-a", b"Evidence the user kept");
    let world = create(&store, "loupist-a", RecordKind::World, "World");
    let mut nodes = vec![
        node("user-a", "User A", "Organization", BTreeMap::new()),
        node("user-b", "User B", "Organization", BTreeMap::new()),
    ];
    let binding = |span: &Option<String>| EvidenceBinding {
        span_id: span.clone().unwrap(),
        role: BindingRole::Supports,
        attribution: "Source".into(),
    };
    let mut user_claim = claim(
        "user-claim",
        "A relationship the user wrote",
        "user-a",
        "user-b",
    );
    user_claim.evidence.push(binding(&own.span_id));
    let dependencies = vec![
        EvidenceRevision {
            record_id: own.record_id.clone(),
            revision_digest: own.revision_digest.clone(),
            role: DependencyRole::WorldSource,
        },
        EvidenceRevision {
            record_id: other.record_id.clone(),
            revision_digest: other.revision_digest.clone(),
            role: DependencyRole::WorldSource,
        },
    ];
    let mut claims = vec![user_claim];
    let first = store
        .transact(|write| {
            super::super::world::seal_world_with_actor(
                write.tx,
                SealWorld {
                    loupist_id: "loupist-a".into(),
                    record_id: world.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: AT,
                    nodes: nodes.clone(),
                    claims: claims.clone(),
                    annotations: vec![],
                    dependencies: dependencies.clone(),
                },
                "user",
                None,
                None,
            )
        })
        .unwrap();
    nodes.extend([
        node("own-a", "Own A", "Organization", BTreeMap::new()),
        node("own-b", "Own B", "Organization", BTreeMap::new()),
    ]);
    claims.push(claim(
        "owned-no-support",
        "Learned without saved support",
        "own-a",
        "own-b",
    ));
    let mut mixed = claim(
        "mixed",
        "A relationship with independent support",
        "user-a",
        "user-b",
    );
    mixed.evidence = vec![binding(&own.span_id), binding(&other.span_id)];
    claims.push(mixed);
    store
        .transact(|write| {
            super::super::world::seal_world_with_actor(
                write.tx,
                SealWorld {
                    loupist_id: "loupist-a".into(),
                    record_id: world.record_id.clone(),
                    expected_revision_id: Some(first.revision_id.clone()),
                    at_ms: AT + 1,
                    nodes: nodes.clone(),
                    claims: claims.clone(),
                    annotations: vec![],
                    dependencies: dependencies.clone(),
                },
                "loupist",
                None,
                None,
            )
        })
        .unwrap();
    let counts = store
        .read(|write| memory_reset::preview(write.tx, "loupist-a"))
        .unwrap();
    assert_eq!(
        (counts.names, counts.relationships, counts.sources),
        (2, 1, 0)
    );
    store
        .transact(|write| memory_reset::reset(write.tx, write.blobs, "loupist-a", &counts, AT + 2))
        .unwrap();
    store
        .read(|write| {
            let head = write
                .owned_record("loupist-a", &world.record_id)?
                .revision_id
                .unwrap();
            let body: WorldSnapshotBody =
                serde_json::from_value(write.get_revision("loupist-a", &head)?.payload)?;
            assert_eq!(body.claims.len(), 2);
            assert_eq!(
                body.claims
                    .iter()
                    .find(|c| c.id == "user-claim")
                    .unwrap()
                    .evidence
                    .len(),
                1
            );
            assert_eq!(
                body.claims
                    .iter()
                    .find(|c| c.id == "mixed")
                    .unwrap()
                    .evidence
                    .len(),
                2
            );
            write.owned_record("loupist-a", &own.record_id)?;
            Ok(())
        })
        .unwrap();
}
