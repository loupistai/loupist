use super::super::graph_inference::{InferenceDisposition, InferenceReply};
use super::super::*;
use super::*;
const LOUPIST: &str = "loupist-learning";
const AT: i64 = 1_800_000_000_000;

fn setup() -> (tempfile::TempDir, KnowledgeStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
    store
        .transact(|w| super::super::graph_jobs::store_settings(w.tx, LOUPIST, &config()))
        .unwrap();
    (dir, store)
}
fn config() -> GraphProcessingSettings {
    GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("openai-codex".into()),
        helper_model: Some("gpt-6.1-sol".into()),
        thinking_level: "medium".into(),
        ..Default::default()
    }
}
fn fact(i: usize, text: &str) -> GraphClaim {
    GraphClaim {
        id: format!("claim-{i:03}"),
        proposition: text.into(),
        attribution: "Dated fictional company filing".into(),
        relation: "reports".into(),
        endpoints: vec![ClaimEndpoint {
            node_id: format!("entity-{}", i % 3),
            role: "subject".into(),
        }],
        qualifiers: vec![],
        evidence: vec![],
        status: ClaimStatus::Active,
        recorded_at_ms: Some(AT + i as i64),
        publication_time: Some(TimePoint {
            precision: TimePrecision::Date,
            value: Some(format!("2026-10-{:02}", i % 28 + 1)),
            approximate: false,
        }),
        effective_time: None,
        valid_at: None,
        invalid_at: None,
        expired_at: None,
    }
}
fn seed(store: &KnowledgeStore, count: usize) -> SealWorld {
    let world = store
        .create_record(CreateRecord {
            loupist_id: LOUPIST.into(),
            kind: RecordKind::World,
            title: "Industrial research".into(),
            at_ms: AT,
        })
        .unwrap();
    let mut request=SealWorld{loupist_id:LOUPIST.into(),record_id:world.record_id,expected_revision_id:None,at_ms:AT+1,nodes:(0..3).map(|i|GraphNode{id:format!("entity-{i}"),name:format!("Manufacturer {i}"),aliases:vec![],description:"Fictional manufacturer".into(),kind:"company".into(),source_role:"organization".into(),properties:BTreeMap::new()}).collect(),claims:(0..count).map(|i|fact(i,&format!("Company {} reported lower margins during a capacity ramp; utilization was 60 percent.",i%3))).collect(),annotations:vec![],dependencies:vec![]};
    let sealed = store.seal_world(request.clone()).unwrap();
    request.expected_revision_id = Some(sealed.revision_id);
    request
}
fn add(store: &KnowledgeStore, world: &mut SealWorld, count: usize, now: i64) {
    let start = world.claims.len();
    for i in start..start + count {
        world.claims.push(fact(i, "New unrelated observation"));
    }
    world.at_ms = now;
    let saved = store.seal_world(world.clone()).unwrap();
    world.expected_revision_id = Some(saved.revision_id);
}
fn queue(store: &KnowledgeStore, now: i64) -> Option<String> {
    store
        .transact(|w| enqueue(w.tx, LOUPIST, "job-world", now))
        .unwrap()
}
fn begin(store: &KnowledgeStore, now: i64) -> Attempt {
    store
        .transact(|w| claim(w.tx, LOUPIST, "Industrial Loupist", "Capacity ramps", now))
        .unwrap()
        .expect("claim review")
}
fn reviews(store: &KnowledgeStore) -> Vec<Review> {
    store
        .read(|w| {
            list(
                w.tx,
                &ReviewQuery {
                    loupist_id: LOUPIST.into(),
                    limit: Some(200),
                },
            )
        })
        .unwrap()
}
fn output(a: &Attempt) -> Value {
    json!({"action":"create","title":"Check capacity ramp margin pressure","applicability":"Use when capacity ramps coincide with lower margins.","assumptions":"Reporting periods and units are comparable.","procedure":"Compare utilization, ramp milestones and inventory over successive quarters.","principles":"Check operating cash flow before trusting reported profit.","limitations":"A single quarter cannot establish normalized margins.","markdown":"# Capacity ramp review\nCheck utilization, ramp costs and cash conversion over time.","evidenceClaimIds":a.input.claims.iter().take(2).map(|c|&c.claim_id).collect::<Vec<_>>(),"rationale":"Two new dated claims show recurring margin pressure during capacity ramps."})
}
/// Stands in for the isolated, tool-free inference call with one model reply.
fn complete(store: &KnowledgeStore, a: &Attempt, body: Value, now: i64) {
    let req = request(a).unwrap();
    assert!(req.get("tools").is_none());
    let reply = InferenceReply {
        disposition: InferenceDisposition::Completed,
        body: json!({"json_object":body,"usage":{"input_tokens":123,"output_tokens":45,"certainty":"accounted"}}),
    };
    store.transact(|w| finish(w, a, &reply, now)).unwrap();
}

fn waiting_world_job(store: &KnowledgeStore, world: &SealWorld) -> String {
    let id = super::super::model::mint_id("job");
    store.transact(|w| {
        w.tx.execute("INSERT INTO knowledge_graph_outbox(job_id,loupist_id,world_record_id,world_revision_id,status,payload_json,payload_digest,created_at_ms) VALUES (?1,?2,?3,?4,'queued','{}','unused',?5)", params![id,LOUPIST,world.record_id,world.expected_revision_id,world.at_ms])?;
        Ok(())
    }).unwrap();
    id
}

fn finish_world_job(store: &KnowledgeStore, job: &str, now: i64) {
    // Model the publication transaction; the separate publication/rollback test
    // below exercises the real graph_jobs hook with a source-grounded artifact.
    store
        .transact(|w| {
            w.tx.execute(
                "UPDATE knowledge_graph_outbox SET status='completed' WHERE job_id=?1",
                [job],
            )?;
            enqueue(w.tx, LOUPIST, job, now)?;
            Ok(())
        })
        .unwrap();
}

fn review_states(store: &KnowledgeStore) -> Vec<String> {
    store
        .read(|w| {
            let mut q = w
                .tx
                .prepare("SELECT status FROM knowledge_learning_reviews ORDER BY created_at_ms")?;
            let rows = q
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

#[test]
fn threshold_mid_batch_waits_for_last_world_commit_and_admits_once() {
    let (_dir, store) = setup();
    let mut world = seed(&store, 4);
    let first = waiting_world_job(&store, &world);
    add(&store, &mut world, 1, AT + 2);
    // The first chunk crosses the threshold while the next chunk is queued.
    let last = waiting_world_job(&store, &world);
    finish_world_job(&store, &first, AT + 3);
    assert_eq!(review_states(&store), ["pending"]);
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 5))
        .unwrap()
        .is_none());
    assert!(reviews(&store).is_empty());
    store
        .transact(|w| {
            w.tx.execute(
                "UPDATE knowledge_graph_outbox SET status='running' WHERE job_id=?1",
                [&last],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(queue(&store, AT + 7).is_none());
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 8))
        .unwrap()
        .is_none());
    finish_world_job(&store, &last, AT + 9);
    assert_eq!(review_states(&store), ["queued"]);
    assert!(queue(&store, AT + 10).is_none());
    let attempt = begin(&store, AT + 11);
    assert_eq!(attempt.input.claims.len(), 5);
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 12))
        .unwrap()
        .is_none());
    assert_eq!(review_states(&store), ["running"]);
}

#[test]
fn pending_review_survives_restart_and_includes_the_drained_batch() {
    let (dir, store) = setup();
    let mut world = seed(&store, 5);
    let job = waiting_world_job(&store, &world);
    assert!(queue(&store, AT + 2).is_none());
    assert_eq!(review_states(&store), ["pending"]);
    drop(store);
    let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
    store.transact(|w| recover(w.tx)).unwrap();
    assert_eq!(review_states(&store), ["pending"]);
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 3))
        .unwrap()
        .is_none());
    add(&store, &mut world, 3, AT + 4);
    let last = waiting_world_job(&store, &world);
    finish_world_job(&store, &job, AT + 5);
    assert_eq!(review_states(&store), ["pending"]);
    finish_world_job(&store, &last, AT + 7);
    assert_eq!(review_states(&store), ["queued"]);
    assert_eq!(begin(&store, AT + 9).input.claims.len(), 8);
}

#[test]
fn threshold_switch_and_rate_limits_reset_on_defer() {
    let (_dir, store) = setup();
    let mut world = seed(&store, 4);
    assert!(queue(&store, AT + 2).is_none());
    add(&store, &mut world, 1, AT + 3);
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    assert!(queue(&store, AT + 4).is_none());
    store
        .transact(|w| save_settings(w.tx, LOUPIST, true))
        .unwrap();
    assert_eq!(
        review_states(&store),
        ["queued"],
        "resuming admits retained evidence"
    );
    assert!(queue(&store, AT + 5).is_none());
    assert!(queue(&store, AT + 6).is_none());
    let a = begin(&store, AT + 7);
    complete(
        &store,
        &a,
        json!({"action":"defer","rationale":"No recurring pattern.","evidenceClaimIds":[]}),
        AT + 8,
    );
    assert_eq!(reviews(&store)[0].status, "deferred");
    add(&store, &mut world, 5, AT + 9);
    assert!(queue(&store, AT + 10).is_none());
    assert!(queue(&store, AT + SIX_HOURS + 7).is_some());
    let b = begin(&store, AT + SIX_HOURS + 8);
    assert_eq!(b.input.claims.len(), 5);
    complete(
        &store,
        &b,
        json!({"action":"defer","rationale":"One-off updates.","evidenceClaimIds":[]}),
        AT + SIX_HOURS + 9,
    );
    add(&store, &mut world, 2, AT + SIX_HOURS + 10);
    assert!(queue(&store, AT + 2 * SIX_HOURS + 8).is_none());
    // Independent daily cap remains effective even for historical rows with close timestamps.
    store.transact(|w|{for n in 0..2{w.tx.execute("INSERT INTO knowledge_learning_reviews(review_id,loupist_id,world_job_id,status,created_at_ms,started_at_ms,through_event,settings_json,call_budget) VALUES (?1,?2,'job','deferred',?3,?3,0,'{}',1)",params![format!("lrn_history{n}"),LOUPIST,AT+n])?;}Ok(())}).unwrap();
    add(&store, &mut world, 3, AT + 2 * SIX_HOURS + 9);
    assert!(queue(&store, AT + 2 * SIX_HOURS + 10).is_none());
    assert!(queue(&store, AT + DAY + 11).is_some());
}
#[test]
fn frozen_budget_wait_and_startup_quarantine() {
    let (_dir, store) = setup();
    seed(&store, 5);
    assert!(queue(&store, AT + 2).is_some());
    store
        .transact(|w| {
            w.tx.execute("UPDATE knowledge_learning_reviews SET call_budget=0", [])?;
            Ok(())
        })
        .unwrap();
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 3))
        .unwrap()
        .is_none());
    store.transact(|w| recover(w.tx)).unwrap();
    assert!(reviews(&store).is_empty());
    store
        .transact(|w| {
            w.tx.execute("DELETE FROM knowledge_learning_reviews", [])?;
            super::super::graph_jobs::store_settings(w.tx, LOUPIST, &config())
        })
        .unwrap();
    queue(&store, AT + 4);
    let a = begin(&store, AT + 5);
    store.transact(|w| recover(w.tx)).unwrap();
    let review = &reviews(&store)[0];
    assert_eq!(review.status, "failed");
    assert_eq!(review.reason.as_deref(), Some("uncertain_paid_attempt"));
    let reply = InferenceReply {
        disposition: InferenceDisposition::Completed,
        body: json!({"json_object":output(&a)}),
    };
    assert!(store.transact(|w| finish(w, &a, &reply, AT + 6)).is_err());
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + DAY))
        .unwrap()
        .is_none());
}
#[test]
fn input_is_bounded_newest_first_and_principles_are_complete() {
    let (_dir, store) = setup();
    seed(&store, 80);
    store
        .transact(|w| {
            for i in 0..3 {
                super::super::principles::principle_save(
                    w.tx,
                    LOUPIST,
                    None,
                    None,
                    PrincipleInput {
                        statement: format!("Check cash conversion {i}"),
                        why: None,
                        example: None,
                        origin: "settings".into(),
                        origin_chat_id: None,
                        origin_message_id: None,
                    },
                    AT + i,
                )?;
            }
            Ok(())
        })
        .unwrap();
    queue(&store, AT + 10);
    let a = begin(&store, AT + 11);
    assert!(a.input.text().unwrap().chars().count() <= MAX_INPUT);
    assert!(a.input.claims.len() <= 60);
    assert_eq!(a.input.principles.len(), 3);
    // Same-timestamp growth events use deterministic rowid ordering.
    assert_eq!(a.input.claims[0].claim_id, "claim-079");
    assert!(a
        .input
        .claims
        .windows(2)
        .all(|w| w[0].claim_id > w[1].claim_id));
    assert!(a
        .input
        .claims
        .iter()
        .all(|c| c.source_date.is_some() && !c.entity_names.is_empty()));
}
#[test]
fn create_refine_and_stale_proposal_keeps_the_user_edit() {
    let (_dir, store) = setup();
    let mut world = seed(&store, 8);
    queue(&store, AT + 2);
    let a = begin(&store, AT + 3);
    let body = output(&a);
    complete(&store, &a, body.clone(), AT + 4);
    let first = reviews(&store).remove(0);
    assert_eq!(first.status, "created");
    assert_eq!(first.evidence_count, 2);
    let id = first.lens_record_id.unwrap();
    let changes = store.read(|w| w.list_lens_changes(LOUPIST, &id)).unwrap();
    assert_eq!(
        changes[0].origin.as_ref().unwrap()["worldJobId"],
        "job-world"
    );
    add(&store, &mut world, 5, AT + SIX_HOURS + 5);
    queue(&store, AT + SIX_HOURS + 6);
    let b = begin(&store, AT + SIX_HOURS + 7);
    let mut refined = output(&b);
    refined["action"] = json!("refine");
    refined["targetRecordId"] = json!(id);
    refined["procedure"] =
        json!("Also reconcile inventory aging and cash conversion against ramp milestones.");
    complete(&store, &b, refined.clone(), AT + SIX_HOURS + 8);
    assert_eq!(reviews(&store)[0].status, "refined");
    add(&store, &mut world, 5, AT + 2 * SIX_HOURS + 9);
    queue(&store, AT + 2 * SIX_HOURS + 10);
    let c = begin(&store, AT + 2 * SIX_HOURS + 11);
    let current = store.read(|w| w.read_lens(LOUPIST, &id)).unwrap();
    let mut document = current.document;
    document.limitations = "User edited this while learning ran.".into();
    let winner = store
        .revise_lens(ReviseLens {
            loupist_id: LOUPIST.into(),
            record_id: id.clone(),
            expected_revision_id: Some(current.revision_id),
            at_ms: AT + 2 * SIX_HOURS + 12,
            document,
            dependencies: vec![],
            change: LensChangeInput {
                rationale: "User edit".into(),
                references: vec![],
                feedback: None,
            },
        })
        .unwrap();
    refined["evidenceClaimIds"] = json!(c
        .input
        .claims
        .iter()
        .take(2)
        .map(|c| &c.claim_id)
        .collect::<Vec<_>>());
    complete(&store, &c, refined, AT + 2 * SIX_HOURS + 13);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("stale_head_proposal")
    );
    assert_eq!(
        store
            .read(|w| w.read_lens(LOUPIST, &id))
            .unwrap()
            .revision_id,
        winner.revision_id
    );
    assert_eq!(
        store
            .read(|w| w.list_lens_drafts(LOUPIST, Some(&id), Some("loupist")))
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn invalid_output_is_deferred_without_any_lens_write_or_retry() {
    for invalid in [
        "unknown_field",
        "foreign_evidence",
        "bad_action",
        "unresolved_id",
        "principle_target",
    ] {
        let (_dir, store) = setup();
        seed(&store, 5);
        queue(&store, AT + 2);
        let a = begin(&store, AT + 3);
        let mut body = output(&a);
        match invalid {
            "unknown_field" => body["surprise"] = json!(true),
            "foreign_evidence" => body["evidenceClaimIds"] = json!(["foreign", "other"]),
            "bad_action" => body["action"] = json!("buy"),
            "unresolved_id" => {
                body["markdown"] = json!("Use [missing](lens:rec_ffffffffffffffffffffffffffffffff)")
            }
            _ => {
                body["action"] = json!("refine");
                body["targetRecordId"] = json!("rec_principle");
            }
        }
        complete(&store, &a, body, AT + 4);
        assert_eq!(
            reviews(&store)[0].reason.as_deref(),
            Some("invalid_output"),
            "{invalid}"
        );
        assert!(store
            .list_records(LOUPIST)
            .unwrap()
            .iter()
            .all(|r| r.kind != RecordKind::Lens));
        assert!(store
            .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + DAY))
            .unwrap()
            .is_none());
    }
}
#[test]
fn disabling_learning_during_inference_discards_the_method() {
    let (_dir, store) = setup();
    seed(&store, 5);
    queue(&store, AT + 2);
    let a = begin(&store, AT + 3);
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    complete(&store, &a, output(&a), AT + 4);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("learning_disabled")
    );
    assert!(store
        .list_records(LOUPIST)
        .unwrap()
        .iter()
        .all(|r| r.kind != RecordKind::Lens));
}

#[test]
fn world_commit_queues_review_atomically_and_rollbacks_do_not_leak_it() {
    let (_dir, store) = setup();
    let mut world = seed(&store, 6);
    let source = store
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: LOUPIST.into(),
            at_ms: AT + 1,
            title: "Capacity ramp filings".into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: "https://fictional.test/filings".into(),
            provider_identity: "fictional publisher".into(),
            authorship: Authorship::Publisher,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            publication_time: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: "filings".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: vec![],
                bytes: b"Capacity ramp filings".to_vec(),
                selector: Selector::ByteRange { start: 0, end: 21 },
                context_note: "Test source".into(),
            }),
        })
        .unwrap();
    for node in &mut world.nodes {
        node.source_role = "source".into();
    }
    for claim in &mut world.claims {
        claim.endpoints.push(ClaimEndpoint {
            node_id: "entity-2".into(),
            role: "object".into(),
        });
        claim.evidence.push(EvidenceBinding {
            span_id: source.span_id.clone().unwrap(),
            role: BindingRole::ExtractedFrom,
            attribution: "Fictional publisher".into(),
        });
    }
    world.dependencies.push(EvidenceRevision {
        record_id: source.record_id,
        revision_digest: source.revision_digest,
        role: DependencyRole::WorldSource,
    });
    world.at_ms = AT + 2;
    world.expected_revision_id = Some(store.seal_world(world.clone()).unwrap().revision_id);
    let mut settings = config();
    let job = store
        .graph_accept_sealed_world(
            &settings,
            super::super::graph_jobs::SealedWorldIndex {
                loupist_id: LOUPIST.into(),
                world_record_id: world.record_id,
                world_revision_id: world.expected_revision_id.unwrap(),
                at_ms: AT + 2,
            },
        )
        .unwrap();
    let super::super::graph_jobs::GraphJobClaim::Acquired(running) = store
        .graph_claim_running(&settings, &job.job_id, AT + 3)
        .unwrap()
    else {
        panic!("the queued World job was not claimed");
    };
    let attempt = running.document.attempt_id.as_deref().unwrap();
    let rolled = store.transact(|w| -> KnowledgeResult<()> {
        super::super::graph_jobs::complete_reindex(w, &mut settings, &job, attempt, AT + 4)?;
        assert_eq!(
            w.tx.query_row("SELECT COUNT(*) FROM knowledge_learning_reviews", [], |r| r
                .get::<_, i64>(0))?,
            1
        );
        Err(KnowledgeError::Invalid("rollback"))
    });
    assert!(rolled.is_err());
    assert_eq!(
        store
            .read(|w| Ok(w.tx.query_row(
                "SELECT COUNT(*) FROM knowledge_learning_reviews",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        0
    );
    store
        .transact(|w| {
            super::super::graph_jobs::complete_reindex(w, &mut settings, &job, attempt, AT + 5)
        })
        .unwrap();
    assert_eq!(begin(&store, AT + 6).world_job_id, Some(job.job_id));
}

#[test]
fn learning_settings_are_per_loupist_persistent_and_all_world_gates_apply() {
    let (directory, store) = setup();
    seed(&store, 5);
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    assert!(
        store
            .read(|w| settings(w.tx, "loupist-other"))
            .unwrap()
            .learn_methods
    );
    drop(store);
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
    assert!(
        !store
            .read(|w| settings(w.tx, LOUPIST))
            .unwrap()
            .learn_methods
    );
    store
        .transact(|w| save_settings(w.tx, LOUPIST, true))
        .unwrap();
    for gate in ["disabled", "paused", "memory_off", "inactive"] {
        let mut s = config();
        match gate {
            "disabled" => s.enrichment_enabled = false,
            "paused" => s.paused = true,
            "memory_off" => s.save_learning = false,
            _ => s.inactive_loupists.push(LOUPIST.into()),
        }
        store
            .transact(|w| super::super::graph_jobs::store_settings(w.tx, LOUPIST, &s))
            .unwrap();
        assert!(queue(&store, AT + 2).is_none(), "{gate}");
    }
}

#[test]
fn unchanged_method_defers_without_revision_or_growth_and_principle_races_defer() {
    let (_directory, store) = setup();
    let mut world = seed(&store, 6);
    queue(&store, AT + 2);
    let a = begin(&store, AT + 3);
    complete(&store, &a, output(&a), AT + 4);
    let id = reviews(&store)[0].lens_record_id.clone().unwrap();
    let original = store.read(|w| w.read_lens(LOUPIST, &id)).unwrap();
    add(&store, &mut world, 5, AT + SIX_HOURS + 5);
    queue(&store, AT + SIX_HOURS + 6);
    let b = begin(&store, AT + SIX_HOURS + 7);
    let mut out = output(&b);
    out["action"] = json!("refine");
    out["targetRecordId"] = json!(id);
    let count = store
        .read(|w| {
            Ok(w.tx
                .query_row("SELECT COUNT(*) FROM knowledge_growth_events", [], |r| {
                    r.get::<_, i64>(0)
                })?)
        })
        .unwrap();
    complete(&store, &b, out, AT + SIX_HOURS + 8);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("method_unchanged")
    );
    assert_eq!(
        store
            .read(|w| w.read_lens(LOUPIST, &id))
            .unwrap()
            .revision_id,
        original.revision_id
    );
    assert_eq!(
        store
            .read(|w| Ok(w.tx.query_row(
                "SELECT COUNT(*) FROM knowledge_growth_events",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        count
    );
    add(&store, &mut world, 5, AT + 2 * SIX_HOURS + 9);
    queue(&store, AT + 2 * SIX_HOURS + 10);
    let c = begin(&store, AT + 2 * SIX_HOURS + 11);
    store
        .transact(|w| {
            super::super::principles::principle_save(
                w.tx,
                LOUPIST,
                None,
                None,
                PrincipleInput {
                    statement: "Always identify the reporting currency.".into(),
                    why: None,
                    example: None,
                    origin: "settings".into(),
                    origin_chat_id: None,
                    origin_message_id: None,
                },
                AT + 2 * SIX_HOURS + 12,
            )
        })
        .unwrap();
    complete(&store, &c, output(&c), AT + 2 * SIX_HOURS + 13);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("principles_changed")
    );
}

fn research_request(store: &KnowledgeStore, turn: &str, as_of: Option<&str>) -> SaveResearch {
    let bytes = b"Quarterly filing: the capacity ramp raised costs before utilization recovered.";
    let source = store
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: LOUPIST.into(),
            at_ms: AT,
            title: "Quarterly filing".into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: "https://example.test/filing".into(),
            provider_identity: "example-publisher".into(),
            authorship: Authorship::Publisher,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            publication_time: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: "filing".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: vec![],
                bytes: bytes.to_vec(),
                selector: Selector::ByteRange {
                    start: 0,
                    end: bytes.len() as u64,
                },
                context_note: "Exact filing".into(),
            }),
        })
        .unwrap();
    store
        .transact(|w| {
            w.open_turn(OpenTurn {
                loupist_id: LOUPIST.into(),
                chat_session_id: "research-chat".into(),
                turn_id: turn.into(),
                at_ms: AT,
            })?;
            let receipt = w.record_acked_source(
                LOUPIST,
                "research-chat",
                turn,
                AT,
                &source.record_id,
                source.span_id.as_deref().unwrap(),
                &source.capture_id,
                source.representation_id.as_deref().unwrap(),
                source.selected_digest.as_deref().unwrap(),
                &source.revision_id,
                &source.revision_digest,
                "example-publisher",
                "included_full",
                None,
                "text/plain",
                bytes,
            )?;
            let expected = w
                .working_notes(LOUPIST, "research-chat", 1)?
                .first()
                .map(|note| note.research_id.clone());
            Ok(SaveResearch {
                research_review: Some(ResearchReviewDraft {
                    question: "Is ramp pressure temporary?".into(),
                    checks: vec![ResearchCheckDraft {
                        question: "Did utilization recover?".into(),
                        disposition: CheckDisposition::Mixed,
                        finding: "The new ramp raised costs but recovery is not established."
                            .into(),
                        evidence: vec![receipt.clone()],
                        method_read: None,
                        limitation: "The filing gives one period; compare successive periods."
                            .into(),
                    }],
                    next_check: Some("Compare inventory and utilization next quarter.".into()),
                }),
                as_of: as_of.map(str::to_string),
                loupist_id: LOUPIST.into(),
                turn_id: turn.into(),
                at_ms: AT,
                observations: String::new(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                journal: None,
                note_inputs: Some(NoteInputDraft {
                    source_receipt_ids: vec![receipt],
                    ..Default::default()
                }),
                working_note: Some(WorkingNoteDraft {
                    title: "Capacity research".into(),
                    markdown: format!("Finding for {turn}"),
                    change_summary: "Checked ramp recovery".into(),
                    expected_research_id: expected,
                }),
            })
        })
        .unwrap()
}

fn save_experience(store: &KnowledgeStore, turn: &str) -> SavedResearch {
    let request = research_request(store, turn, None);
    store.transact(|w| w.save_research(request)).unwrap()
}

fn observation_output(attempt: &Attempt) -> Value {
    let mut body = output(attempt);
    body["evidenceClaimIds"] = json!([]);
    body["evidenceObservationIds"] = json!(attempt
        .input
        .observations
        .iter()
        .map(|o| &o.id)
        .collect::<Vec<_>>());
    body
}

#[test]
fn two_completed_investigations_publish_a_candidate_without_world_claims() {
    let (_dir, store) = setup();
    save_experience(&store, "turn-one");
    assert!(review_states(&store).is_empty());
    save_experience(&store, "turn-two");
    assert_eq!(review_states(&store), ["queued"]);
    let attempt = begin(&store, AT + 1);
    assert!(attempt.world_job_id.is_none());
    assert!(attempt.input.claims.is_empty());
    assert_eq!(attempt.input.observations.len(), 2);
    assert!(attempt
        .input
        .observations
        .iter()
        .all(|o| o.eligible_original));
    complete(&store, &attempt, observation_output(&attempt), AT + 2);
    let review = reviews(&store).remove(0);
    assert_eq!(review.status, "created");
    assert_eq!(review.evidence_count, 2);
    let cited: BTreeSet<_> = attempt
        .input
        .observations
        .iter()
        .flat_map(|o| o.sources.iter())
        .map(|source| {
            (
                source.record_id.clone(),
                source.revision_id.clone(),
                source.span_id.clone(),
            )
        })
        .collect();
    store
        .read(|w| {
            let learned: bool = w.tx.query_row(
                "SELECT learned FROM knowledge_lens_metadata WHERE record_id=?1",
                [review.lens_record_id.unwrap()],
                |row| row.get(0),
            )?;
            assert!(learned);
            let inputs: String = w.tx.query_row(
                "SELECT inputs_json FROM knowledge_lens_changes WHERE actor='loupist'",
                [],
                |row| row.get(0),
            )?;
            let inputs: LensChangeInputs = serde_json::from_str(&inputs)?;
            assert_eq!(
                inputs
                    .references
                    .iter()
                    .map(|reference| (
                        reference.record_id.clone(),
                        reference.revision_id.clone(),
                        reference.span_id.clone().unwrap()
                    ))
                    .collect::<BTreeSet<_>>(),
                cited
            );
            for reference in inputs.references {
                assert!(reference.receipt_id.is_none());
                let object: String = w.tx.query_row(
                    "SELECT source_id FROM knowledge_sources WHERE record_id=?1",
                    [&reference.record_id],
                    |row| row.get(0),
                )?;
                assert_eq!(reference.object_id, object);
                assert_eq!(reference.kind, "source");
            }

            let journals: i64 =
                w.tx.query_row("SELECT COUNT(*) FROM knowledge_journals", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(
                journals, 0,
                "agent interpretations never become user judgments"
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn experience_snapshots_outlive_note_replacement_and_are_idempotent_and_erased() {
    let (_dir, store) = setup();
    let mut request = research_request(&store, "turn-one", None);
    let first = store
        .transact(|w| w.save_research(request.clone()))
        .unwrap();
    let frozen = store
        .read(|w| read_experiences(w.tx, LOUPIST, 0, i64::MAX))
        .unwrap();
    assert_eq!(frozen.len(), 1);
    let source = &frozen[0].checks[0].inputs.sources[0];
    let receipt = source.receipt_id.clone();
    assert_eq!(source.attribution, "example-publisher");
    let repeated_receipt = store
        .transact(|w| {
            let bytes: Vec<u8> = w.tx.query_row(
                "SELECT selected_bytes FROM knowledge_receipt_snapshots WHERE receipt_id=?1",
                [&receipt],
                |row| row.get(0),
            )?;
            w.record_acked_source(
                LOUPIST,
                "research-chat",
                "turn-one",
                AT,
                &source.record_id,
                &source.span_id,
                &source.capture_id,
                &source.representation_id,
                &source.selected_digest,
                &source.revision_id,
                &source.revision_digest,
                &source.attribution,
                &source.coverage,
                None,
                "text/plain",
                &bytes,
            )
        })
        .unwrap();
    request.note_inputs.as_mut().unwrap().source_receipt_ids = vec![repeated_receipt.clone()];
    request.research_review.as_mut().unwrap().checks[0].evidence = vec![repeated_receipt];
    request.working_note.as_mut().unwrap().expected_research_id = Some(first.research_id);
    let repeated = store
        .transact(|w| w.save_research(request.clone()))
        .unwrap();
    assert_eq!(
        store
            .read(|w| read_experiences(w.tx, LOUPIST, 0, i64::MAX))
            .unwrap()
            .len(),
        1
    );
    request.working_note.as_mut().unwrap().expected_research_id =
        Some(repeated.research_id.clone());
    request.research_review.as_mut().unwrap().checks[0].finding = "Conflicting replacement".into();
    assert!(store.transact(|w| w.save_research(request)).is_err());
    save_experience(&store, "turn-two");
    store
        .read(|w| {
            assert!(w.read_working_note(LOUPIST, &repeated.research_id).is_err());
            let snapshots = read_experiences(w.tx, LOUPIST, 0, i64::MAX)?;
            assert_eq!(snapshots.len(), 2);
            assert_eq!(snapshots[1].checks[0].inputs.sources[0], *source);
            let bytes: Vec<u8> = w.tx.query_row(
                "SELECT selected_bytes FROM knowledge_receipt_snapshots WHERE receipt_id=?1",
                [receipt],
                |row| row.get(0),
            )?;
            assert!(String::from_utf8(bytes).unwrap().contains("utilization"));
            assert!(read_experiences(w.tx, "loupist-other", 0, i64::MAX)?.is_empty());
            Ok(())
        })
        .unwrap();
    store
        .transact(|w| {
            erase_loupist_private(w.tx, LOUPIST)?;
            Ok(())
        })
        .unwrap();
    store
        .read(|w| {
            for table in [
                "knowledge_research_experiences",
                "knowledge_learning_reviews",
                "knowledge_receipt_snapshots",
            ] {
                let count: i64 =
                    w.tx.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })?;
                assert_eq!(count, 0, "{table}");
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn experiences_after_admission_survive_settlement_and_restart() {
    let (_dir, store) = setup();
    save_experience(&store, "turn-one");
    save_experience(&store, "turn-two");
    let first = begin(&store, AT + 1);
    save_experience(&store, "turn-three");
    save_experience(&store, "turn-four");
    assert_eq!(first.input.observations.len(), 2);
    complete(
        &store,
        &first,
        json!({"action":"defer", "rationale":"No reusable pattern", "evidenceClaimIds":[]}),
        AT + 2,
    );
    assert_eq!(review_states(&store), ["deferred", "pending"]);
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 3))
        .unwrap()
        .is_none());
    store.transact(|w| recover(w.tx)).unwrap();
    let second = begin(&store, AT + SIX_HOURS + 2);
    assert_eq!(
        second
            .input
            .observations
            .iter()
            .map(|o| o.turn_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["turn-three", "turn-four"])
    );
    assert!(second.through_experience > first.through_experience);
    store.transact(|w| recover(w.tx)).unwrap();
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("uncertain_paid_attempt")
    );
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + DAY))
        .unwrap()
        .is_none());
}

#[test]
fn experience_publication_refuses_unknown_ids_and_single_turn_evidence() {
    for mutation in ["unknown", "one-turn", "duplicate"] {
        let (_dir, store) = setup();
        save_experience(&store, "turn-one");
        save_experience(&store, "turn-two");
        let attempt = begin(&store, AT + 1);
        let mut body = observation_output(&attempt);
        body["evidenceObservationIds"] = match mutation {
            "unknown" => json!([attempt.input.observations[0].id, "obs-forged"]),
            "one-turn" => json!([attempt.input.observations[0].id]),
            _ => json!([
                attempt.input.observations[0].id,
                attempt.input.observations[0].id
            ]),
        };
        complete(&store, &attempt, body, AT + 2);
        assert_eq!(reviews(&store)[0].reason.as_deref(), Some("invalid_output"));
        assert!(reviews(&store)[0].lens_record_id.is_none());
    }
}

#[test]
fn as_of_investigations_and_rolled_back_completion_do_not_admit_learning() {
    let (_dir, store) = setup();
    for turn in ["historical-one", "historical-two"] {
        let request = research_request(&store, turn, Some("2026-01-01"));
        store.transact(|w| w.save_research(request)).unwrap();
    }
    assert!(review_states(&store).is_empty());
    assert!(store
        .read(|w| read_experiences(w.tx, LOUPIST, 0, i64::MAX))
        .unwrap()
        .is_empty());
    save_experience(&store, "current-one");
    let request = research_request(&store, "rollback", None);
    let result: KnowledgeResult<()> = store.transact(|w| {
        w.save_research(request)?;
        Err(KnowledgeError::Invalid("completion failed"))
    });
    assert!(result.is_err());
    assert!(review_states(&store).is_empty());
    assert_eq!(
        store
            .read(|w| read_experiences(w.tx, LOUPIST, 0, i64::MAX))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn experience_evidence_must_belong_to_the_note_turn_and_owner() {
    let (_dir, store) = setup();
    let first = research_request(&store, "turn-one", None);
    for mutation in ["unknown", "foreign-owner", "foreign-turn", "outside-note"] {
        let mut request = first.clone();
        match mutation {
            "unknown" => {
                request.research_review.as_mut().unwrap().checks[0].evidence =
                    vec!["rcp-forged".into()]
            }
            "foreign-owner" => request.loupist_id = "loupist-other".into(),
            "foreign-turn" => {
                let other = research_request(&store, "turn-two", None);
                request.turn_id = other.turn_id;
            }
            _ => request
                .note_inputs
                .as_mut()
                .unwrap()
                .source_receipt_ids
                .clear(),
        }
        assert!(
            store.transact(|w| w.save_research(request)).is_err(),
            "{mutation}"
        );
    }
    assert!(review_states(&store).is_empty());
    assert!(store
        .read(|w| read_experiences(w.tx, LOUPIST, 0, i64::MAX))
        .unwrap()
        .is_empty());
}

#[test]
fn experience_keeps_exact_world_and_lens_snapshots_after_their_heads_change() {
    let (_dir, store) = setup();
    let mut request = research_request(&store, "turn-one", None);
    let initial = LensDocument {
        markdown: "Compare dated capacity accounts.".into(),
        applicability: "Capacity expansions".into(),
        assumptions: "Comparable reporting periods".into(),
        procedure: "Check utilization".into(),
        principles: "Keep dated attribution".into(),
        limitations: "One period is insufficient".into(),
    };
    let lens = store
        .create_record(CreateRecord {
            loupist_id: LOUPIST.into(),
            kind: RecordKind::Lens,
            title: "Capacity method".into(),
            at_ms: AT,
        })
        .unwrap();
    let revision = store
        .revise_lens(ReviseLens {
            loupist_id: LOUPIST.into(),
            record_id: lens.record_id.clone(),
            expected_revision_id: None,
            at_ms: AT,
            document: initial.clone(),
            dependencies: vec![],
            change: LensChangeInput {
                rationale: "Initial check".into(),
                references: vec![],
                feedback: None,
            },
        })
        .unwrap();
    let delivery = store
        .transact(|w| {
            w.record_acked_lens(
                LOUPIST,
                "turn-one",
                AT,
                &lens.record_id,
                &revision.revision_id,
                &revision.revision_digest,
                &serde_json::to_string(&initial).unwrap(),
                &crate::util::sha256(initial.delivery_text().unwrap().as_bytes()),
            )
        })
        .unwrap();
    request
        .note_inputs
        .as_mut()
        .unwrap()
        .lenses
        .push(LensAcknowledgement {
            delivery_id: delivery.clone(),
            status: LensUseStatus::Applied,
        });
    request.research_review.as_mut().unwrap().checks[0].method_read = Some(delivery);
    let mut world = seed(&store, 1);
    let view = store.read(|w| w.query_world(serde_json::from_value(json!({"loupistId":LOUPIST,"recordId":world.record_id,"revisionId":world.expected_revision_id})).unwrap())).unwrap();
    let view_json = serde_json::to_string(&view).unwrap();
    let world_delivery = store
        .transact(|w| {
            w.record_acked_world(
                LOUPIST,
                "turn-one",
                AT,
                &view.record_id,
                &view.revision_id,
                &view.revision_digest,
                &view_json,
                &crate::util::sha256(view_json.as_bytes()),
            )
        })
        .unwrap();
    request
        .note_inputs
        .as_mut()
        .unwrap()
        .world_delivery_ids
        .push(world_delivery.clone());
    request.research_review.as_mut().unwrap().checks[0]
        .evidence
        .push(world_delivery);
    store.transact(|w| w.save_research(request)).unwrap();
    add(&store, &mut world, 1, AT + 1);
    let mut changed = initial.clone();
    changed.procedure = "Check utilization and inventory".into();
    store
        .revise_lens(ReviseLens {
            loupist_id: LOUPIST.into(),
            record_id: lens.record_id,
            expected_revision_id: Some(revision.revision_id.clone()),
            at_ms: AT + 1,
            document: changed,
            dependencies: vec![],
            change: LensChangeInput {
                rationale: "Add inventory".into(),
                references: vec![],
                feedback: None,
            },
        })
        .unwrap();
    save_experience(&store, "turn-two");
    let frozen = store
        .read(|w| read_experiences(w.tx, LOUPIST, 0, i64::MAX))
        .unwrap();
    let method = &frozen[1].checks[0].inputs.lenses[0];
    assert_eq!(method.revision_id, revision.revision_id);
    assert_eq!(method.document, initial);
    let frozen_world = &frozen[1].checks[0].inputs.worlds[0];
    assert_eq!(frozen_world.delivered_view_json, view_json);
    assert_eq!(frozen_world.revision_id, view.revision_id);
    assert_eq!(
        frozen_world.view_digest,
        crate::util::sha256(view_json.as_bytes())
    );
    let attempt = begin(&store, AT + 3);
    store
        .transact(|w| {
            super::super::memory_controls::pause(w.tx, LOUPIST, &method.record_id, true)?;
            let input = build_input(w.tx, LOUPIST, 0, i64::MAX, (0, i64::MAX), "Name", "Focus")?;
            assert!(input
                .observations
                .iter()
                .all(|observation| observation.turn_id != "turn-one"));
            assert!(input
                .methods
                .iter()
                .all(|candidate| candidate.record_id != method.record_id));
            Ok(())
        })
        .unwrap();
    complete(&store, &attempt, observation_output(&attempt), AT + 4);
    assert_eq!(reviews(&store)[0].reason.as_deref(), Some("invalid_output"));
    assert!(reviews(&store)[0].lens_record_id.is_none());
}

#[test]
fn experience_review_waits_for_world_and_rechecks_saving_before_publication() {
    let (_dir, store) = setup();
    let world = seed(&store, 1);
    let job = waiting_world_job(&store, &world);
    save_experience(&store, "turn-one");
    save_experience(&store, "turn-two");
    assert_eq!(review_states(&store), ["pending"]);
    assert!(store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", AT + 1))
        .unwrap()
        .is_none());
    finish_world_job(&store, &job, AT + 2);
    let attempt = begin(&store, AT + 3);
    let mut disabled = config();
    disabled.save_learning = false;
    store
        .transact(|w| super::super::graph_jobs::store_settings(w.tx, LOUPIST, &disabled))
        .unwrap();
    complete(&store, &attempt, observation_output(&attempt), AT + 4);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("learning_disabled")
    );
    let request = research_request(&store, "disabled-save", None);
    assert!(store.transact(|w| w.save_research(request)).is_err());
}

#[test]
fn memory_reset_erases_experiences_and_frozen_reviews_and_stops_publication() {
    let (_dir, store) = setup();
    save_experience(&store, "turn-one");
    save_experience(&store, "turn-two");
    let attempt = begin(&store, AT + 1);
    store
        .transact(|w| {
            let counts = super::super::memory_reset::preview(w.tx, LOUPIST)?;
            super::super::memory_reset::reset(w.tx, w.blobs, LOUPIST, &counts, AT + 2)?;
            assert!(read_experiences(w.tx, LOUPIST, 0, i64::MAX)?.is_empty());
            let count: i64 = w.tx.query_row(
                "SELECT COUNT(*) FROM knowledge_learning_reviews",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            assert_eq!(
                w.working_notes(LOUPIST, "research-chat", 1)?.len(),
                1,
                "reset preserves the documented working note"
            );
            Ok(())
        })
        .unwrap();
    let reply = InferenceReply {
        disposition: InferenceDisposition::Completed,
        body: json!({"json_object":observation_output(&attempt)}),
    };
    assert!(store
        .transact(|w| finish(w, &attempt, &reply, AT + 3))
        .is_err());
    assert!(reviews(&store).is_empty());
}

#[test]
fn experience_rows_are_immutable_and_corrupt_snapshots_refuse_learning() {
    let (_dir, store) = setup();
    save_experience(&store, "turn-one");
    store
        .transact(|w| {
            assert!(w
                .tx
                .execute(
                    "UPDATE knowledge_research_experiences SET body_json='{}'",
                    []
                )
                .is_err());
            assert!(w
                .tx
                .execute("DELETE FROM knowledge_research_experiences", [])
                .is_err());
            w.tx.execute_batch("DROP TRIGGER knowledge_research_experiences_no_update")?;
            w.tx.execute(
                "UPDATE knowledge_research_experiences SET body_json='{}'",
                [],
            )?;
            assert!(matches!(
                read_experiences(w.tx, LOUPIST, 0, i64::MAX),
                Err(KnowledgeError::DigestMismatch)
            ));
            Ok(())
        })
        .unwrap();
}

fn read_experiences(
    tx: &rusqlite::Transaction<'_>,
    owner: &str,
    start: i64,
    end: i64,
) -> KnowledgeResult<Vec<super::super::research::experience::Experience>> {
    let mut experiences = Vec::new();
    super::super::research::experience::visit_recent(tx, owner, start, end, |experience| {
        experiences.push(experience);
        Ok(())
    })?;
    Ok(experiences)
}

#[test]
fn removed_source_evidence_is_not_sent_to_the_learning_reviewer() {
    let (_dir, store) = setup();
    let request = research_request(&store, "turn-one", None);
    store.transact(|w| w.save_research(request)).unwrap();
    save_experience(&store, "turn-two");
    store
        .transact(|w| {
            let experiences = read_experiences(w.tx, LOUPIST, 0, i64::MAX)?;
            let source = &experiences[0].checks[0].inputs.sources[0];
            let mut controls = super::super::memory_controls::load(w.tx, LOUPIST)?;
            controls.removed.insert(source.record_id.clone());
            super::super::memory_controls::save(w.tx, LOUPIST, &controls)?;
            assert!(claim(w.tx, LOUPIST, "Name", "Focus", AT + 1)?.is_none());
            Ok(())
        })
        .unwrap();
    assert_eq!(reviews(&store)[0].status, "deferred");
    assert!(reviews(&store)[0].lens_record_id.is_none());
}

#[test]
fn a_refine_target_paused_during_inference_cannot_be_published_or_proposed() {
    let (_dir, store) = setup();
    let mut world = seed(&store, 5);
    queue(&store, AT + 1);
    let first = begin(&store, AT + 2);
    complete(&store, &first, output(&first), AT + 3);
    let target = reviews(&store)[0].lens_record_id.clone().unwrap();
    add(&store, &mut world, 5, AT + SIX_HOURS + 3);
    queue(&store, AT + SIX_HOURS + 4);
    let attempt = begin(&store, AT + SIX_HOURS + 5);
    let mut body = output(&attempt);
    body["action"] = json!("refine");
    body["targetRecordId"] = json!(target);
    body["procedure"] =
        json!("Compare utilization and inventory ageing over three reporting periods.");
    store
        .transact(|w| super::super::memory_controls::pause(w.tx, LOUPIST, &target, true))
        .unwrap();
    complete(&store, &attempt, body, AT + SIX_HOURS + 6);
    assert_eq!(reviews(&store)[0].reason.as_deref(), Some("invalid_output"));
    store
        .read(|w| {
            let count: i64 =
                w.tx.query_row("SELECT COUNT(*) FROM knowledge_lens_drafts", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(count, 0);
            assert_eq!(w.list_lens_changes(LOUPIST, &target)?.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn completed_research_rechecks_items_excluded_after_their_reads() {
    for change in ["removed-source", "paused-lens", "removed-lens"] {
        let (_dir, store) = setup();
        let mut request = research_request(&store, "turn-one", None);
        let record_id = if change == "removed-source" {
            store
                .list_records(LOUPIST)
                .unwrap()
                .into_iter()
                .find(|record| record.kind == RecordKind::Source)
                .unwrap()
                .record_id
        } else {
            let document = LensDocument {
                markdown: "Compare dated capacity accounts.".into(),
                applicability: "Capacity expansions".into(),
                assumptions: "Comparable reporting periods".into(),
                procedure: "Check utilization".into(),
                principles: "Keep dated attribution".into(),
                limitations: "One period is insufficient".into(),
            };
            let record = store
                .create_record(CreateRecord {
                    loupist_id: LOUPIST.into(),
                    kind: RecordKind::Lens,
                    title: "Capacity method".into(),
                    at_ms: AT,
                })
                .unwrap();
            let revision = store
                .revise_lens(ReviseLens {
                    loupist_id: LOUPIST.into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: AT,
                    document: document.clone(),
                    dependencies: vec![],
                    change: LensChangeInput {
                        rationale: "Initial check".into(),
                        references: vec![],
                        feedback: None,
                    },
                })
                .unwrap();
            let delivery = store
                .transact(|w| {
                    w.record_acked_lens(
                        LOUPIST,
                        "turn-one",
                        AT,
                        &record.record_id,
                        &revision.revision_id,
                        &revision.revision_digest,
                        &serde_json::to_string(&document).unwrap(),
                        &crate::util::sha256(document.delivery_text().unwrap().as_bytes()),
                    )
                })
                .unwrap();
            request
                .note_inputs
                .as_mut()
                .unwrap()
                .lenses
                .push(LensAcknowledgement {
                    delivery_id: delivery.clone(),
                    status: LensUseStatus::Applied,
                });
            request.research_review.as_mut().unwrap().checks[0].method_read = Some(delivery);
            record.record_id
        };
        store
            .transact(|w| {
                if change == "paused-lens" {
                    super::super::memory_controls::pause(w.tx, LOUPIST, &record_id, true)?;
                } else {
                    let mut controls = super::super::memory_controls::load(w.tx, LOUPIST)?;
                    controls.removed.insert(record_id.clone());
                    super::super::memory_controls::save(w.tx, LOUPIST, &controls)?;
                }
                Ok(())
            })
            .unwrap();
        assert!(
            store.transact(|w| w.save_research(request)).is_err(),
            "{change}"
        );
        store
            .read(|w| {
                assert!(read_experiences(w.tx, LOUPIST, 0, i64::MAX)?.is_empty());
                assert!(w.working_notes(LOUPIST, "research-chat", 10)?.is_empty());
                Ok(())
            })
            .unwrap();
        assert!(review_states(&store).is_empty());
    }
}
