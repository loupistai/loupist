use super::super::graph_inference::{InferenceDisposition, InferenceReply};
use super::super::lens_page::AnchorInput;
use super::super::test_support::turn_origin;
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
    let world = store
        .create_record(CreateRecord {
            loupist_id: LOUPIST.into(),
            kind: RecordKind::World,
            title: "Industrial research".into(),
            at_ms: AT,
        })
        .unwrap();
    store
        .seal_world(SealWorld {
            loupist_id: LOUPIST.into(),
            record_id: world.record_id,
            expected_revision_id: None,
            at_ms: AT + 1,
            nodes: (0..4)
                .map(|i| GraphNode {
                    id: format!("entity-{i}"),
                    name: format!("Manufacturer {i}"),
                    aliases: vec![],
                    description: "Fictional manufacturer".into(),
                    kind: "company".into(),
                    source_role: "organization".into(),
                    properties: BTreeMap::new(),
                })
                .collect(),
            claims: vec![],
            annotations: vec![],
            dependencies: vec![],
        })
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
fn retain(store: &KnowledgeStore, title: &str, authorship: Authorship, provider: &str) -> BasisRef {
    let bytes = format!("{title}: margins fell while utilization stayed near 60 percent.");
    let saved = store
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: LOUPIST.into(),
            at_ms: AT,
            title: title.into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: format!("https://fictional.test/{}", title.replace(' ', "-")),
            provider_identity: provider.into(),
            authorship,
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
                bytes: bytes.clone().into_bytes(),
                selector: Selector::ByteRange {
                    start: 0,
                    end: bytes.len() as u64,
                },
                context_note: "Test source".into(),
            }),
        })
        .unwrap();
    BasisRef::Span {
        record_id: saved.record_id,
        revision_digest: saved.revision_digest,
        span_id: saved.span_id.unwrap(),
    }
}
fn source(store: &KnowledgeStore, title: &str) -> BasisRef {
    retain(store, title, Authorship::Publisher, "fictional publisher")
}
fn record_of(span: &BasisRef) -> String {
    match span {
        BasisRef::Span { record_id, .. } => record_id.clone(),
        other => panic!("not a span: {other:?}"),
    }
}
/// What `commit_turn` does: file the turn's puts, then enqueue, in one transaction.
fn turn_with(
    store: &KnowledgeStore,
    chat: &str,
    message: &str,
    company: usize,
    markdown: &str,
    refs: &[BasisRef],
    now: i64,
) -> Option<String> {
    store
        .transact(|w| {
            w.file(
                LOUPIST,
                &turn_origin(chat, message),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Subject,
                        title: format!("Manufacturer {company}"),
                        anchors: vec![AnchorInput::Node {
                            node_id: format!("entity-{company}"),
                        }],
                    },
                    ops: vec![LensOp::Put {
                        key: SectionKey::new(&format!("ramp-{message}"))?,
                        heading: "Capacity ramp".into(),
                        markdown: markdown.into(),
                        basis: Basis {
                            refs: refs.to_vec(),
                            uncertainty: None,
                        },
                        expected: None,
                    }],
                }],
                now,
            )?;
            enqueue(w.tx, LOUPIST, now)
        })
        .unwrap()
}
fn turn(
    store: &KnowledgeStore,
    chat: &str,
    message: &str,
    company: usize,
    refs: &[BasisRef],
    now: i64,
) -> Option<String> {
    let text = format!(
        "Manufacturer {company} margins fell during a capacity ramp at 60 percent utilization."
    );
    turn_with(store, chat, message, company, &text, refs, now)
}
/// Two Chats, each citing its own Source: the smallest evidence that starts a review.
fn two_chats(store: &KnowledgeStore, now: i64) -> Option<String> {
    let (a, b) = (
        source(store, &format!("Filing A {now}")),
        source(store, &format!("Filing B {now}")),
    );
    assert!(turn(
        store,
        &format!("chat-a-{now}"),
        &format!("m-a-{now}"),
        0,
        &[a],
        now
    )
    .is_none());
    turn(
        store,
        &format!("chat-b-{now}"),
        &format!("m-b-{now}"),
        1,
        &[b],
        now + 1,
    )
}
fn begin(store: &KnowledgeStore, now: i64) -> Attempt {
    store
        .transact(|w| claim(w.tx, LOUPIST, "Industrial Loupist", "Capacity ramps", now))
        .unwrap()
        .expect("claim review")
}
fn try_claim(store: &KnowledgeStore, now: i64) -> Option<Attempt> {
    store
        .transact(|w| claim(w.tx, LOUPIST, "Name", "Focus", now))
        .unwrap()
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
fn methods(store: &KnowledgeStore) -> i64 {
    store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT COUNT(*) FROM knowledge_lens_metadata WHERE kind='method'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}
fn ids(a: &Attempt) -> Vec<String> {
    a.input.observations.iter().map(|o| o.id.clone()).collect()
}
fn output(a: &Attempt) -> Value {
    json!({"action":"create","title":"Check capacity ramp margin pressure","applicability":"Use when capacity ramps coincide with lower margins.","assumptions":"Reporting periods and units are comparable.","procedure":"Compare utilization, ramp milestones and inventory over successive quarters.","principles":"Check operating cash flow before trusting reported profit.","limitations":"Only two manufacturers in one sector; untested elsewhere.","markdown":"# Capacity ramp review\nCheck utilization, ramp costs and cash conversion over time.","evidenceIds":ids(a),"rationale":"Two Chats found margin pressure during capacity ramps."})
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
fn defer(store: &KnowledgeStore, a: &Attempt, now: i64) {
    complete(
        store,
        a,
        json!({"action":"defer","rationale":"No recurring pattern.","evidenceIds":[]}),
        now,
    );
}

#[test]
fn one_turn_never_queues_nor_do_two_turns_of_one_chat() {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    assert!(turn(&store, "chat-1", "m1", 0, &[a.clone(), b.clone()], AT + 2).is_none());
    assert!(turn(&store, "chat-1", "m2", 1, &[b], AT + 3).is_none());
    assert!(turn(&store, "chat-1", "m3", 2, &[a], AT + 4).is_none());
    assert!(review_states(&store).is_empty());
}

#[test]
fn chats_citing_one_source_never_queue() {
    let (_dir, store) = setup();
    let a = source(&store, "Filing A");
    assert!(turn(&store, "chat-1", "m1", 0, std::slice::from_ref(&a), AT + 2).is_none());
    assert!(turn(&store, "chat-2", "m2", 1, &[a], AT + 3).is_none());
    assert!(review_states(&store).is_empty());
}

#[test]
fn two_chats_and_two_sources_queue_exactly_one_review_and_admit_it() {
    let (_dir, store) = setup();
    let id = two_chats(&store, AT + 2).expect("queued");
    assert_eq!(review_states(&store), ["queued"]);
    // A later filing neither queues a second review nor joins the admitted one.
    let c = source(&store, "Filing C");
    assert!(turn(&store, "chat-c", "m-c", 2, &[c], AT + 4).is_none());
    assert_eq!(review_states(&store), ["queued"]);
    let a = begin(&store, AT + 5);
    assert_eq!(a.review_id, id);
    let seen: Vec<_> = a
        .input
        .observations
        .iter()
        .map(|o| (o.turn.as_str(), o.chat.as_str(), o.page.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("t1", "c1", "Manufacturer 1"),
            ("t2", "c2", "Manufacturer 0")
        ]
    );
    assert_eq!(
        a.input.observations[0].sources,
        [format!("Filing B {}", AT + 2)]
    );
    let sent = a.input.text().unwrap();
    assert!(!sent.contains("chat-a") && !sent.contains("m-a-"), "{sent}");
    assert!(try_claim(&store, AT + 6).is_none());
    assert_eq!(review_states(&store), ["running"]);
    // The single later turn left past the watermark is not enough on its own.
    defer(&store, &a, AT + 7);
    assert_eq!(review_states(&store), ["deferred"]);
}

#[test]
fn only_preserved_originals_count() {
    let (_dir, store) = setup();
    let a = source(&store, "Filing A");
    let analysis = retain(
        &store,
        "Analyst note",
        Authorship::Analysis,
        "fictional publisher",
    );
    let shared = retain(
        &store,
        "Shared filing",
        Authorship::Publisher,
        "shared_pack:peer",
    );
    assert!(turn(&store, "chat-1", "m1", 0, &[a], AT + 2).is_none());
    assert!(turn(&store, "chat-2", "m2", 1, &[analysis], AT + 3).is_none());
    assert!(turn(&store, "chat-3", "m3", 2, &[shared], AT + 4).is_none());
    assert!(review_states(&store).is_empty());
}

#[test]
fn a_removed_source_drops_its_evidence_before_inference() {
    let (_dir, store) = setup();
    two_chats(&store, AT + 2).expect("queued");
    let b = store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT record_id FROM knowledge_sources WHERE title=?1",
                [format!("Filing B {}", AT + 2)],
                |r| r.get::<_, String>(0),
            )?)
        })
        .unwrap();
    store
        .transact(|w| {
            let mut controls = super::super::memory_controls::load(w.tx, LOUPIST)?;
            controls.removed.insert(b);
            super::super::memory_controls::save(w.tx, LOUPIST, &controls)
        })
        .unwrap();
    assert!(try_claim(&store, AT + 4).is_none());
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("insufficient_turns")
    );
}

#[test]
fn a_rolled_back_turn_leaves_no_review() {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    turn(&store, "chat-1", "m1", 0, &[a], AT + 2);
    let rolled = store.transact(|w| -> KnowledgeResult<()> {
        w.file(
            LOUPIST,
            &turn_origin("chat-2", "m2"),
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Subject,
                    title: "Manufacturer 1".into(),
                    anchors: vec![AnchorInput::Node {
                        node_id: "entity-1".into(),
                    }],
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new("ramp")?,
                    heading: "Capacity ramp".into(),
                    markdown: "Margins fell during the ramp.".into(),
                    basis: Basis {
                        refs: vec![b.clone()],
                        uncertainty: None,
                    },
                    expected: None,
                }],
            }],
            AT + 3,
        )?;
        assert!(enqueue(w.tx, LOUPIST, AT + 3)?.is_some());
        Err(KnowledgeError::Invalid("rollback"))
    });
    assert!(rolled.is_err());
    assert!(review_states(&store).is_empty());
    assert!(turn(&store, "chat-2", "m2", 1, &[b], AT + 4).is_some());
}

#[test]
fn rate_limits_hold_a_pending_review_across_restart_and_settle_requeues_leftovers() {
    let (dir, store) = setup();
    let first = two_chats(&store, AT + 2).expect("queued");
    let a = begin(&store, AT + 4);
    assert_eq!(a.review_id, first);
    // Filed after admission: past this review's watermark.
    assert!(two_chats(&store, AT + 10).is_none());
    defer(&store, &a, AT + 20);
    assert_eq!(review_states(&store), ["deferred", "pending"]);
    let wake = store
        .read(|w| next_wake_at(w.tx, AT + 21))
        .unwrap()
        .expect("wake at the cooldown");
    assert_eq!(wake, AT + 4 + SIX_HOURS);
    drop(store);
    let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
    store.transact(|w| recover(w.tx)).unwrap();
    assert_eq!(review_states(&store), ["deferred", "pending"]);
    assert!(try_claim(&store, AT + 30).is_none());
    // Filed while pending: admission includes it.
    let late = source(&store, "Filing late");
    turn(&store, "chat-late", "m-late", 2, &[late], AT + 40);
    let b = begin(&store, wake);
    assert_eq!(b.input.observations.len(), 3);
    defer(&store, &b, wake + 1);
    // The daily cap holds even when the six-hour cooldown has passed.
    store.transact(|w|{for n in 0..2{w.tx.execute("INSERT INTO knowledge_learning_reviews(review_id,loupist_id,world_job_id,status,created_at_ms,started_at_ms,through_event,settings_json,call_budget) VALUES (?1,?2,'','deferred',?3,?3,0,'{}',1)",params![format!("lrn_history{n}"),LOUPIST,AT+5+n])?;}Ok(())}).unwrap();
    assert!(two_chats(&store, wake + SIX_HOURS + 10).is_none());
    assert!(store
        .transact(|w| enqueue(w.tx, LOUPIST, AT + DAY + 3))
        .unwrap()
        .is_none());
    assert!(store
        .transact(|w| enqueue(w.tx, LOUPIST, AT + DAY + 4))
        .unwrap()
        .is_some());
}

#[test]
fn frozen_budget_wait_and_startup_quarantine() {
    let (_dir, store) = setup();
    two_chats(&store, AT + 2).expect("queued");
    store
        .transact(|w| {
            w.tx.execute("UPDATE knowledge_learning_reviews SET call_budget=0", [])?;
            Ok(())
        })
        .unwrap();
    assert!(try_claim(&store, AT + 3).is_none());
    store.transact(|w| recover(w.tx)).unwrap();
    assert!(reviews(&store).is_empty());
    store
        .transact(|w| {
            w.tx.execute("UPDATE knowledge_learning_reviews SET call_budget=1", [])?;
            Ok(())
        })
        .unwrap();
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
    assert!(try_claim(&store, AT + DAY).is_none());
}

#[test]
fn input_is_bounded_newest_first_and_principles_are_complete() {
    let (_dir, store) = setup();
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
            // Hold admission until every turn is filed.
            save_settings(w.tx, LOUPIST, false)?;
            Ok(())
        })
        .unwrap();
    let long = "Utilization, ramp cost and margin moved together. ".repeat(20);
    let sources: Vec<_> = (0..4)
        .map(|i| source(&store, &format!("Filing {i}")))
        .collect();
    for i in 0..30 {
        turn_with(
            &store,
            &format!("chat-{}", i % 3),
            &format!("m{i:02}"),
            i % 4,
            &long,
            &[sources[i % 4].clone()],
            AT + 10 + i as i64,
        );
    }
    store
        .transact(|w| save_settings(w.tx, LOUPIST, true))
        .unwrap();
    store
        .transact(|w| enqueue(w.tx, LOUPIST, AT + 100))
        .unwrap()
        .expect("queued");
    let a = begin(&store, AT + 101);
    assert!(a.input.text().unwrap().chars().count() <= MAX_INPUT);
    assert!(a.input.observations.len() < 30);
    assert_eq!(a.input.principles.len(), 3);
    assert_eq!(a.input.observations[0].message_id, "m29");
    assert_eq!(a.input.observations[0].turn, "t1");
    assert!(a
        .input
        .observations
        .windows(2)
        .all(|w| w[0].message_id > w[1].message_id));
    assert!(a
        .input
        .observations
        .iter()
        .all(|o| o.text.chars().count() == 600 && o.kind == "subject"));
}

#[test]
fn create_refine_and_stale_proposal_keeps_the_user_edit() {
    let (_dir, store) = setup();
    // Ten spans across two turns: the learned basis keeps eight, every Source first.
    let first: Vec<_> = (0..6)
        .map(|i| source(&store, &format!("Ramp A{i}")))
        .collect();
    let second: Vec<_> = (0..4)
        .map(|i| source(&store, &format!("Ramp B{i}")))
        .collect();
    turn(&store, "chat-1", "m1", 0, &first, AT + 2);
    turn(&store, "chat-2", "m2", 1, &second, AT + 3).expect("queued");
    let a = begin(&store, AT + 4);
    complete(&store, &a, output(&a), AT + 5);
    let created = reviews(&store).remove(0);
    assert_eq!(created.status, "created");
    assert_eq!(created.evidence_count, 2);
    let id = created.lens_record_id.unwrap();
    let page = store.read_page(LOUPIST, &id).unwrap();
    assert_eq!(page.history[0].origin, "learning");
    assert!(page.learned);
    let cited: BTreeSet<_> = first.iter().chain(&second).map(record_of).collect();
    for section in page.body.sections() {
        let records: BTreeSet<_> = section.basis.refs.iter().map(record_of).collect();
        assert_eq!(section.basis.refs.len(), 8, "{}", section.key.as_str());
        assert_eq!(records.len(), 8);
        assert!(records.is_subset(&cited));
    }
    let origin: Value = store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT origin FROM knowledge_growth_events WHERE record_id = ?1 AND kind = 'lens_created'",
                [&id],
                |r| r.get::<_, String>(0),
            )?)
        })
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap();
    assert_eq!(
        origin,
        json!({"kind":"learning_review","reviewId":a.review_id})
    );
    let applied: Vec<String> = store
        .read(|w| {
            let mut q = w.tx.prepare("SELECT node_id FROM knowledge_lens_anchors WHERE record_id=?1 AND role='applied' ORDER BY node_id")?;
            let rows = q.query_map([&id], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(applied, ["entity-0", "entity-1"]);

    two_chats(&store, AT + SIX_HOURS + 5);
    let b = begin(&store, AT + SIX_HOURS + 6);
    let mut refined = output(&b);
    refined["action"] = json!("refine");
    refined["targetRecordId"] = json!(id);
    refined["procedure"] =
        json!("Also reconcile inventory aging and cash conversion against ramp milestones.");
    complete(&store, &b, refined.clone(), AT + SIX_HOURS + 7);
    assert_eq!(reviews(&store)[0].status, "refined");

    two_chats(&store, AT + 2 * SIX_HOURS + 8);
    let c = begin(&store, AT + 2 * SIX_HOURS + 9);
    let current = store.read_page(LOUPIST, &id).unwrap();
    let winner = store
        .transact(|w| {
            w.put_section(
                LOUPIST,
                &id,
                "limits",
                "User edited this while learning ran.",
                &super::super::test_support::user_origin(),
                AT + 2 * SIX_HOURS + 10,
            )
        })
        .unwrap();
    assert!(winner[0].changed);
    refined["evidenceIds"] = json!(ids(&c));
    refined["procedure"] = json!("Reconcile inventory aging, then compare cash conversion.");
    refined["limitations"] = json!("Learning tried to overwrite the user's limit.");
    complete(&store, &c, refined, AT + 2 * SIX_HOURS + 11);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("stale_head_folded")
    );
    let after = store.read_page(LOUPIST, &id).unwrap();
    assert_ne!(after.revision_id, current.revision_id);
    let section = |key: &str| {
        after
            .body
            .sections()
            .iter()
            .find(|s| s.key.as_str() == key)
            .unwrap()
            .markdown
            .clone()
    };
    assert_eq!(section("limits"), "User edited this while learning ran.");
    assert!(section("steps").starts_with("Reconcile inventory aging"));
}

#[test]
fn invalid_output_is_deferred_without_any_lens_write_or_retry() {
    for invalid in [
        "unknown_field",
        "foreign_evidence",
        "one_chat",
        "bad_action",
        "unresolved_id",
        "principle_target",
    ] {
        let (_dir, store) = setup();
        let (x, y) = (source(&store, "Filing X"), source(&store, "Filing Y"));
        turn(&store, "chat-1", "m1", 0, &[x], AT + 2);
        turn(&store, "chat-1", "m2", 1, std::slice::from_ref(&y), AT + 3);
        turn(&store, "chat-2", "m3", 2, &[y], AT + 4).expect("queued");
        let a = begin(&store, AT + 5);
        let mut body = output(&a);
        match invalid {
            "unknown_field" => body["surprise"] = json!(true),
            "foreign_evidence" => body["evidenceIds"] = json!(["foreign", "other"]),
            "one_chat" => {
                let one: Vec<_> = a
                    .input
                    .observations
                    .iter()
                    .filter(|o| o.chat_id == "chat-1")
                    .map(|o| &o.id)
                    .collect();
                assert_eq!(one.len(), 2);
                body["evidenceIds"] = json!(one);
            }
            "bad_action" => body["action"] = json!("buy"),
            "unresolved_id" => {
                body["markdown"] = json!("Use [missing](lens:rec_ffffffffffffffffffffffffffffffff)")
            }
            _ => {
                body["action"] = json!("refine");
                body["targetRecordId"] = json!("rec_principle");
            }
        }
        complete(&store, &a, body, AT + 6);
        assert_eq!(
            reviews(&store)[0].reason.as_deref(),
            Some("invalid_output"),
            "{invalid}"
        );
        assert_eq!(methods(&store), 0, "{invalid}");
        assert!(try_claim(&store, AT + DAY).is_none());
    }
}

#[test]
fn disabling_learning_during_inference_discards_the_method() {
    let (_dir, store) = setup();
    two_chats(&store, AT + 2);
    let a = begin(&store, AT + 3);
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    complete(&store, &a, output(&a), AT + 4);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("learning_disabled")
    );
    assert_eq!(methods(&store), 0);
}

#[test]
fn learning_settings_are_per_loupist_persistent_and_all_gates_apply() {
    let (directory, store) = setup();
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    assert!(two_chats(&store, AT + 2).is_none());
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
        assert!(
            store
                .transact(|w| enqueue(w.tx, LOUPIST, AT + 3))
                .unwrap()
                .is_none(),
            "{gate}"
        );
    }
    store
        .transact(|w| super::super::graph_jobs::store_settings(w.tx, LOUPIST, &config()))
        .unwrap();
    assert!(store
        .transact(|w| enqueue(w.tx, LOUPIST, AT + 4))
        .unwrap()
        .is_some());
}

#[test]
fn unchanged_method_defers_without_revision_or_growth_and_principle_races_defer() {
    let (_directory, store) = setup();
    let (x, y) = (source(&store, "Filing X"), source(&store, "Filing Y"));
    // Later Chats citing the same Sources give a refine the same basis.
    let pair = |round: i64, now: i64| {
        turn(
            &store,
            &format!("chat-{round}-a"),
            &format!("m{round}a"),
            0,
            std::slice::from_ref(&x),
            now,
        );
        turn(
            &store,
            &format!("chat-{round}-b"),
            &format!("m{round}b"),
            1,
            std::slice::from_ref(&y),
            now + 1,
        )
    };
    pair(1, AT + 2).expect("queued");
    let a = begin(&store, AT + 4);
    complete(&store, &a, output(&a), AT + 5);
    let id = reviews(&store)[0].lens_record_id.clone().unwrap();
    let original = store.read_page(LOUPIST, &id).unwrap();
    let growth = || {
        store
            .read(|w| {
                Ok(w.tx.query_row(
                    "SELECT COUNT(*) FROM knowledge_growth_events WHERE record_id=?1",
                    [&id],
                    |r| r.get::<_, i64>(0),
                )?)
            })
            .unwrap()
    };
    let before = growth();
    pair(2, AT + SIX_HOURS + 5);
    let b = begin(&store, AT + SIX_HOURS + 7);
    let mut out = output(&b);
    out["action"] = json!("refine");
    out["targetRecordId"] = json!(id);
    complete(&store, &b, out, AT + SIX_HOURS + 8);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("method_unchanged")
    );
    assert_eq!(
        store.read_page(LOUPIST, &id).unwrap().revision_id,
        original.revision_id
    );
    assert_eq!(growth(), before);
    pair(3, AT + 2 * SIX_HOURS + 9);
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
