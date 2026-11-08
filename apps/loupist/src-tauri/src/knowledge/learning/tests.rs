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
    span_of(&retain_saved(store, title, authorship, provider))
}
fn span_of(saved: &RetainedSource) -> BasisRef {
    BasisRef::Span {
        record_id: saved.record_id.clone(),
        revision_digest: saved.revision_digest.clone(),
        span_id: saved.span_id.clone().unwrap(),
    }
}
fn retain_saved(
    store: &KnowledgeStore,
    title: &str,
    authorship: Authorship,
    provider: &str,
) -> RetainedSource {
    let bytes = format!("{title}: margins fell while utilization stayed near 60 percent.");
    store
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
        .unwrap()
}
fn source(store: &KnowledgeStore, title: &str) -> BasisRef {
    retain(store, title, Authorship::Publisher, "fictional publisher")
}
/// One Source with two spans, as a sector report that discusses two companies
/// in two passages. The second span's ref cites the record's latest revision.
fn two_span_source(store: &KnowledgeStore, title: &str) -> (BasisRef, BasisRef) {
    let first = retain_saved(store, title, Authorship::Publisher, "fictional publisher");
    let bytes = format!("{title}, second passage: the other company kept its margins.");
    let second = store
        .retain_source(RetainSource {
            origin: None,
            loupist_id: LOUPIST.into(),
            record_id: first.record_id.clone(),
            expected_revision_id: Some(first.revision_id.clone()),
            at_ms: AT + 1,
            source_id: Some(first.source_id.clone()),
            title: title.into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: format!("https://fictional.test/{}", title.replace(' ', "-")),
            provider_identity: "fictional publisher".into(),
            authorship: Authorship::Publisher,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: "filing-2".into(),
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
            publication_time: None,
        })
        .unwrap();
    assert_eq!(first.record_id, second.record_id);
    assert_ne!(first.span_id, second.span_id);
    (span_of(&first), span_of(&second))
}
fn record_of(span: &BasisRef) -> String {
    match span {
        BasisRef::Span { record_id, .. } => record_id.clone(),
        other => panic!("not a span: {other:?}"),
    }
}
fn company(company: usize) -> PageTarget {
    PageTarget::Create {
        kind: PageKind::Subject,
        title: format!("Manufacturer {company}"),
        anchors: vec![AnchorInput::Node {
            node_id: format!("entity-{company}"),
        }],
    }
}
/// What `commit_turn` does: file the turn's put, then enqueue, in one transaction.
fn put_in(
    store: &KnowledgeStore,
    chat: &str,
    message: &str,
    target: PageTarget,
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
                    target,
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
fn turn_with(
    store: &KnowledgeStore,
    chat: &str,
    message: &str,
    n: usize,
    markdown: &str,
    refs: &[BasisRef],
    now: i64,
) -> Option<String> {
    put_in(store, chat, message, company(n), markdown, refs, now)
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
    store.transact(|w| recover(w.tx, AT + 21)).unwrap();
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
    store.transact(|w| recover(w.tx, AT + 3)).unwrap();
    assert!(reviews(&store).is_empty());
    store
        .transact(|w| {
            w.tx.execute("UPDATE knowledge_learning_reviews SET call_budget=1", [])?;
            Ok(())
        })
        .unwrap();
    let a = begin(&store, AT + 5);
    store.transact(|w| recover(w.tx, AT + 5)).unwrap();
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
            &[sources[i % 3].clone()],
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
        let (x, y, z) = (
            source(&store, "Filing X"),
            source(&store, "Filing Y"),
            source(&store, "Filing Z"),
        );
        turn(&store, "chat-1", "m1", 0, &[x], AT + 2);
        turn(&store, "chat-1", "m2", 1, &[y], AT + 3);
        turn(&store, "chat-2", "m3", 2, &[z], AT + 4).expect("queued");
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

fn remove_source(store: &KnowledgeStore, title: &str) {
    store
        .transact(|w| {
            let record: String = w.tx.query_row(
                "SELECT record_id FROM knowledge_sources WHERE title=?1",
                [title],
                |r| r.get(0),
            )?;
            let mut controls = super::super::memory_controls::load(w.tx, LOUPIST)?;
            controls.removed.insert(record);
            super::super::memory_controls::save(w.tx, LOUPIST, &controls)
        })
        .unwrap();
}

#[test]
fn a_forgotten_section_is_not_evidence() {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    turn(&store, "chat-1", "m1", 0, &[a], AT + 2);
    let page = store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT record_id FROM knowledge_lens_log WHERE message_id='m1'",
                [],
                |r| r.get::<_, String>(0),
            )?)
        })
        .unwrap();
    store
        .transact(|w| w.forget_section(LOUPIST, &page, "ramp-m1", AT + 3))
        .unwrap();
    assert!(turn(&store, "chat-2", "m2", 1, &[b], AT + 4).is_none());
    assert!(review_states(&store).is_empty());
}

#[test]
fn contested_research_is_not_evidence() {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    // This World holds no such claim, so the section is contested from its first write.
    let fallen = BasisRef::Claim {
        claim_id: "claim-fallen".into(),
    };
    turn(&store, "chat-1", "m1", 0, &[a.clone(), fallen], AT + 2);
    assert!(turn(&store, "chat-2", "m2", 1, &[b], AT + 3).is_none());
    assert!(review_states(&store).is_empty());
    let c = source(&store, "Filing C");
    turn(&store, "chat-3", "m3", 2, &[c], AT + 4).expect("an uncontested pair queues");
}

#[test]
fn evidence_withdrawn_after_claim_never_publishes() {
    for change in ["undo", "forget", "source_removed"] {
        let (_dir, store) = setup();
        two_chats(&store, AT + 2).expect("queued");
        let a = begin(&store, AT + 4);
        let withdrawn = a
            .input
            .observations
            .iter()
            .find(|o| o.chat_id.starts_with("chat-a"))
            .unwrap()
            .clone();
        match change {
            "undo" => {
                store
                    .transact(|w| w.undo_op(LOUPIST, &withdrawn.id, AT + 5))
                    .unwrap();
            }
            "forget" => {
                let key = format!("ramp-{}", withdrawn.message_id);
                store
                    .transact(|w| w.forget_section(LOUPIST, &withdrawn.page_id, &key, AT + 5))
                    .unwrap();
            }
            _ => remove_source(&store, &format!("Filing A {}", AT + 2)),
        }
        complete(&store, &a, output(&a), AT + 6);
        assert_eq!(
            reviews(&store)[0].reason.as_deref(),
            Some("invalid_output"),
            "{change}"
        );
        assert_eq!(methods(&store), 0, "{change}");
    }
}

#[test]
fn legacy_claim_count_reviews_neither_block_nor_set_the_watermark() {
    let (_dir, store) = setup();
    // Before lens-log watermarks, `through_event` held a growth-event rowid
    // far past this folder's lens-log head.
    store
        .transact(|w| {
            for (id, job, status, through) in [
                ("lrn_legacy_done", "job-1", "created", 10_000),
                ("lrn_legacy_wait", "job-2", "pending", 9_000),
            ] {
                w.tx.execute("INSERT INTO knowledge_learning_reviews(review_id,loupist_id,world_job_id,status,created_at_ms,through_event,settings_json,call_budget) VALUES (?1,?2,?3,?4,?5,?6,'{}',1)", params![id, LOUPIST, job, status, AT - 2 * DAY, through])?;
            }
            Ok(())
        })
        .unwrap();
    store.transact(|w| recover(w.tx, AT + 1)).unwrap();
    let legacy = reviews(&store);
    let waiting = legacy
        .iter()
        .find(|r| r.review_id == "lrn_legacy_wait")
        .unwrap();
    assert_eq!(waiting.status, "deferred");
    assert_eq!(waiting.reason.as_deref(), Some("trigger_changed"));
    let id = two_chats(&store, AT + 2).expect("queued past the legacy rows");
    assert!(!id.starts_with("lrn_legacy"));
    assert_eq!(begin(&store, AT + 3).input.observations.len(), 2);
}

#[test]
fn a_long_newer_chat_cannot_crowd_the_older_chat_out_of_the_input() {
    let (_dir, store) = setup();
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    let long = "Utilization, ramp cost and margin moved together. ".repeat(20);
    let older = source(&store, "Filing A");
    turn_with(&store, "chat-a", "m-a", 0, &long, &[older], AT + 2);
    let sources: Vec<_> = (0..3)
        .map(|i| source(&store, &format!("Filing B{i}")))
        .collect();
    for i in 0..25 {
        turn_with(
            &store,
            "chat-b",
            &format!("m-b{i:02}"),
            1 + i % 3,
            &long,
            &[sources[i % 3].clone()],
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
    assert!(a.input.observations.len() < 26);
    assert_eq!(a.input.observations[0].message_id, "m-b24");
    assert_eq!(a.input.observations.last().unwrap().message_id, "m-a");
}

#[test]
fn restart_requeues_evidence_filed_during_a_lost_attempt() {
    let (dir, store) = setup();
    two_chats(&store, AT + 2).expect("queued");
    let lost = begin(&store, AT + 4);
    assert!(two_chats(&store, AT + 10).is_none());
    drop(store);
    let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
    store.transact(|w| recover(w.tx, AT + 20)).unwrap();
    assert_eq!(review_states(&store), ["failed", "pending"]);
    // The uncertain attempt itself is not retried: the new review reads only
    // what was filed past its watermark.
    let next = begin(&store, AT + 4 + SIX_HOURS);
    assert_ne!(next.review_id, lost.review_id);
    let messages: Vec<_> = next
        .input
        .observations
        .iter()
        .map(|o| o.message_id.clone())
        .collect();
    assert_eq!(
        messages,
        [format!("m-b-{}", AT + 10), format!("m-a-{}", AT + 10)]
    );
}

#[test]
fn one_company_or_method_pages_are_not_independent_cases() {
    for second in ["same_company", "method_page"] {
        let (_dir, store) = setup();
        let (a, b, c) = (
            source(&store, "Filing A"),
            source(&store, "Filing B"),
            source(&store, "Filing C"),
        );
        assert!(turn(&store, "chat-1", "m1", 0, &[a], AT + 2).is_none());
        let target = match second {
            "same_company" => company(0),
            _ => PageTarget::Create {
                kind: PageKind::Method,
                title: "Ramp checks".into(),
                anchors: vec![],
            },
        };
        let queued = put_in(
            &store,
            "chat-2",
            "m2",
            target,
            "Compare utilization with margins.",
            &[b],
            AT + 3,
        );
        assert!(queued.is_none(), "{second}");
        assert!(review_states(&store).is_empty(), "{second}");
        // A second company's page is the independent case the gate wants.
        assert!(
            turn(&store, "chat-3", "m3", 1, &[c], AT + 4).is_some(),
            "{second}"
        );
    }
}

fn principle(statement: String) -> PrincipleInput {
    PrincipleInput {
        statement,
        why: None,
        example: None,
        origin: "settings".into(),
        origin_chat_id: None,
        origin_message_id: None,
    }
}

#[test]
fn a_review_deferred_before_inference_keeps_its_evidence_and_rate_slot() {
    let (_dir, store) = setup();
    let long = "Check cash conversion before trusting reported profit. ".repeat(6);
    let saved = store
        .transact(|w| {
            (0..45)
                .map(|i| {
                    let statement = format!("{i:02} {long}").chars().take(280).collect();
                    super::super::principles::principle_save(
                        w.tx,
                        LOUPIST,
                        None,
                        None,
                        principle(statement),
                        AT + i,
                    )
                })
                .collect::<KnowledgeResult<Vec<_>>>()
        })
        .unwrap();
    two_chats(&store, AT + 100).expect("queued");
    assert!(try_claim(&store, AT + 101).is_none());
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("principles_exceed_input_limit")
    );
    // Settling did not requeue the same evidence.
    assert_eq!(review_states(&store), ["deferred"]);
    store
        .transact(|w| {
            for (i, p) in saved.iter().enumerate() {
                super::super::principles::principle_save(
                    w.tx,
                    LOUPIST,
                    Some(&p.record_id),
                    Some(&p.revision_id),
                    principle(format!("Rule {i}")),
                    AT + 102,
                )?;
            }
            Ok(())
        })
        .unwrap();
    let c = source(&store, "Filing C");
    turn(&store, "chat-c", "m-c", 2, &[c], AT + 103).expect("queued without a cooldown");
    assert_eq!(begin(&store, AT + 104).input.observations.len(), 3);
}

#[test]
fn a_chat_that_repeats_another_chats_sources_is_not_a_second_case() {
    let (_dir, store) = setup();
    let (a, b, c) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
    );
    assert!(turn(&store, "chat-1", "m1", 0, &[a.clone(), b], AT + 2).is_none());
    assert!(turn(&store, "chat-2", "m2", 1, &[a], AT + 3).is_none());
    assert!(review_states(&store).is_empty());
    assert!(turn(&store, "chat-3", "m3", 2, &[c], AT + 4).is_some());
}

/// Files `text` on a method section as the user: by hand, or taught in a Chat,
/// where the turn cites the user's message.
fn user_writes(store: &KnowledgeStore, in_chat: bool, id: &str, key: &str, text: &str, now: i64) {
    let message = format!("m-said-{key}-{now}");
    let (origin, refs) = if in_chat {
        (
            turn_origin("chat-teach", &message),
            vec![BasisRef::UserSaid {
                message_id: message.clone(),
            }],
        )
    } else {
        (super::super::test_support::user_origin(), vec![])
    };
    let expected = store
        .read_page(LOUPIST, id)
        .unwrap()
        .body
        .sections()
        .iter()
        .find(|s| s.key.as_str() == key)
        .map(|s| s.digest.clone());
    store
        .transact(|w| {
            w.file(
                LOUPIST,
                &origin,
                vec![PageDelta {
                    target: PageTarget::Existing {
                        record_id: id.into(),
                    },
                    ops: vec![LensOp::Put {
                        key: SectionKey::new(key)?,
                        heading: key.into(),
                        markdown: text.into(),
                        basis: Basis {
                            refs: refs.clone(),
                            uncertainty: None,
                        },
                        expected: expected.clone(),
                    }],
                }],
                now,
            )
        })
        .unwrap();
}

#[test]
fn a_refine_keeps_the_sections_the_user_wrote_or_taught() {
    for in_chat in [false, true] {
        a_refine_keeps_user_sections(in_chat);
    }
}

fn a_refine_keeps_user_sections(in_chat: bool) {
    let (_dir, store) = setup();
    two_chats(&store, AT + 2).expect("queued");
    let a = begin(&store, AT + 4);
    complete(&store, &a, output(&a), AT + 5);
    let id = reviews(&store)[0].lens_record_id.clone().unwrap();
    let write = |key: &str, text: &str, now: i64| user_writes(&store, in_chat, &id, key, text, now);
    write("limits", "Only two Korean manufacturers so far.", AT + 6);
    let section = |key: &str| {
        store
            .read_page(LOUPIST, &id)
            .unwrap()
            .body
            .sections()
            .iter()
            .find(|s| s.key.as_str() == key)
            .unwrap()
            .markdown
            .clone()
    };
    two_chats(&store, AT + SIX_HOURS + 5);
    let b = begin(&store, AT + SIX_HOURS + 6);
    let mut refined = output(&b);
    refined["action"] = json!("refine");
    refined["targetRecordId"] = json!(id);
    refined["procedure"] = json!("Reconcile inventory aging against ramp milestones.");
    refined["limitations"] = json!("Learning's own limit.");
    complete(&store, &b, refined.clone(), AT + SIX_HOURS + 7);
    let review = reviews(&store).remove(0);
    assert_eq!(review.status, "refined", "in_chat={in_chat}");
    assert_eq!(review.reason, None, "in_chat={in_chat}");
    assert_eq!(
        section("limits"),
        "Only two Korean manufacturers so far.",
        "in_chat={in_chat}"
    );
    assert!(section("steps").starts_with("Reconcile inventory aging"));
    // Once the user wrote every section, a refine has nothing left to write.
    for key in ["use-when", "steps", "checks"] {
        write(key, "Written by the user.", AT + SIX_HOURS + 8);
    }
    let before = store.read_page(LOUPIST, &id).unwrap().revision_id;
    two_chats(&store, AT + 2 * SIX_HOURS + 9);
    let c = begin(&store, AT + 2 * SIX_HOURS + 10);
    refined["evidenceIds"] = json!(ids(&c));
    refined["markdown"] = json!("# Capacity ramp review\nA new summary learning would write.");
    complete(&store, &c, refined, AT + 2 * SIX_HOURS + 11);
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("method_unchanged"),
        "in_chat={in_chat}"
    );
    assert_eq!(store.read_page(LOUPIST, &id).unwrap().revision_id, before);
}

#[test]
fn two_pages_about_one_company_are_one_case() {
    let node = |n: usize| AnchorInput::Node {
        node_id: format!("entity-{n}"),
    };
    let page = |kind, title: &str, anchors| PageTarget::Create {
        kind,
        title: title.into(),
        anchors,
    };
    for (variant, first, second) in [
        (
            "idea_about_the_subject",
            company(0),
            page(PageKind::Idea, "Ramp thesis", vec![node(0)]),
        ),
        (
            "subject_sharing_an_anchor",
            company(0),
            page(PageKind::Subject, "Manufacturer 2", vec![node(2), node(0)]),
        ),
    ] {
        let (_dir, store) = setup();
        let (a, b, c, d) = (
            source(&store, "Filing A"),
            source(&store, "Filing B"),
            source(&store, "Filing C"),
            source(&store, "Filing D"),
        );
        let text = "Margins fell during a capacity ramp at 60 percent utilization.";
        assert!(put_in(&store, "chat-1", "m1", first, text, &[a], AT + 2).is_none());
        let queued = put_in(&store, "chat-2", "m2", second, text, &[b], AT + 3);
        assert!(queued.is_none(), "{variant}");
        assert!(review_states(&store).is_empty(), "{variant}");
        // A page with no subject anchor may be about the same company: no case.
        let other = page(PageKind::Idea, "Utilization thesis", vec![]);
        let queued = put_in(&store, "chat-3", "m3", other, text, &[c], AT + 4);
        assert!(queued.is_none(), "{variant}");
        assert!(
            turn(&store, "chat-4", "m4", 1, &[d], AT + 5).is_some(),
            "{variant}"
        );
    }
}

#[test]
fn a_page_about_an_unknown_company_is_never_one_of_the_two_cases() {
    let idea = |title: &str| PageTarget::Create {
        kind: PageKind::Idea,
        title: title.into(),
        anchors: vec![],
    };
    // One subject the World has not confirmed makes the whole page unknown.
    let partly_confirmed = PageTarget::Create {
        kind: PageKind::Subject,
        title: "Manufacturer 0 and Kestrel".into(),
        anchors: vec![
            AnchorInput::Node {
                node_id: "entity-0".into(),
            },
            AnchorInput::Name {
                name: "Kestrel Foundry".into(),
            },
        ],
    };
    for (variant, first) in [
        ("subject_and_unanchored_idea", company(0)),
        ("two_unanchored_ideas", idea("Ramp thesis")),
        ("unconfirmed_subject_and_unanchored_idea", partly_confirmed),
    ] {
        let (_dir, store) = setup();
        let (a, b, c) = (
            source(&store, "Filing A"),
            source(&store, "Filing B"),
            source(&store, "Filing C"),
        );
        let text = "Margins fell during a capacity ramp at 60 percent utilization.";
        assert!(put_in(&store, "chat-1", "m1", first, text, &[a], AT + 2).is_none());
        let unanchored = idea("Utilization recovery thesis");
        let queued = put_in(&store, "chat-2", "m2", unanchored, text, &[b], AT + 3);
        assert!(queued.is_none(), "{variant}");
        assert!(review_states(&store).is_empty(), "{variant}");
        // A second company is a case only beside a first one the gate knows.
        let queued = turn(&store, "chat-3", "m3", 1, &[c], AT + 4);
        assert_eq!(
            queued.is_some(),
            variant == "subject_and_unanchored_idea",
            "{variant}"
        );
        if queued.is_some() {
            let pages: Vec<_> = begin(&store, AT + 5)
                .input
                .observations
                .iter()
                .map(|o| o.page.clone())
                .collect();
            assert_eq!(pages, ["Manufacturer 1", "Manufacturer 0"]);
        }
    }
}

/// A claim between `from` and `to` whose evidence cites `span`, as extraction
/// records which Source confirmed a company.
fn claim_citing(id: &str, from: &str, to: &str, span: &BasisRef) -> GraphClaim {
    claim_with_status(id, from, to, span, ClaimStatus::Active)
}
fn claim_with_status(
    id: &str,
    from: &str,
    to: &str,
    span: &BasisRef,
    status: ClaimStatus,
) -> GraphClaim {
    let BasisRef::Span { span_id, .. } = span else {
        unreachable!()
    };
    GraphClaim {
        id: id.into(),
        proposition: format!("{from} supplies {to}"),
        attribution: "report".into(),
        relation: "supplies".into(),
        endpoints: vec![
            ClaimEndpoint {
                node_id: from.into(),
                role: "source".into(),
            },
            ClaimEndpoint {
                node_id: to.into(),
                role: "target".into(),
            },
        ],
        qualifiers: Vec::new(),
        evidence: vec![EvidenceBinding {
            span_id: span_id.clone(),
            role: BindingRole::ExtractedFrom,
            attribution: "report".into(),
        }],
        status,
        recorded_at_ms: Some(AT),
        publication_time: None,
        effective_time: None,
        valid_at: None,
        invalid_at: None,
        expired_at: None,
    }
}

/// Publishes the World again with `nodes`, as World processing does.
fn reseal(store: &KnowledgeStore, nodes: Vec<(&str, &str)>, at: i64) {
    reseal_claiming(store, nodes, Vec::new(), at);
}

/// The World record and its head revision.
fn world_head(store: &KnowledgeStore) -> (String, String) {
    store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT r.record_id,h.revision_id FROM knowledge_records r
                 JOIN knowledge_record_heads h ON h.record_id=r.record_id
                 WHERE r.owner_loupist_id=?1 AND r.kind='world'",
                [LOUPIST],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

/// The World job for `span`'s Source, queued as retaining the Source through a
/// Chat queues one, or the job it already has.
fn world_job(store: &KnowledgeStore, span: &BasisRef, at: i64) -> GraphJobView {
    let BasisRef::Span {
        record_id, span_id, ..
    } = span
    else {
        panic!("not a span: {span:?}")
    };
    let (world, revision) = world_head(store);
    let source_revision: String = store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT revision_id FROM knowledge_record_heads WHERE record_id=?1",
                [record_id],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    store
        .graph_accept_episode(
            &config(),
            super::super::graph_jobs::AddToWorld {
                loupist_id: LOUPIST.into(),
                world_record_id: world,
                world_revision_id: revision,
                source_record_id: record_id.clone(),
                source_revision_id: source_revision,
                span_id: span_id.clone(),
                at_ms: at,
                binding_role: "extracted_from".into(),
            },
        )
        .unwrap()
}

/// The World is through with each of `spans`' Sources: its job ran and ended
/// without publishing, as an extraction the model could not complete does.
/// The claims a test wants come from the seal that follows.
fn world_read(store: &KnowledgeStore, spans: &[&BasisRef], at: i64) {
    for span in spans {
        let job = world_job(store, span, at);
        if super::super::graph_jobs::final_status(&job.status) {
            continue;
        }
        let running = store
            .graph_mark_running(&config(), &job.job_id, at)
            .unwrap();
        assert_eq!(running.status, super::super::graph_jobs::JOB_RUNNING);
        let attempt = running.document.attempt_id.as_deref().unwrap();
        let finished = store
            .graph_finish(
                &mut config(),
                &job.job_id,
                attempt,
                super::super::graph_jobs::JOB_FAILED_BEFORE,
                false,
                false,
                at,
            )
            .unwrap();
        assert_eq!(finished.status, super::super::graph_jobs::JOB_FAILED_BEFORE);
    }
}

fn company_node(id: &str, name: &str, kind: &str) -> GraphNode {
    GraphNode {
        id: id.into(),
        name: name.into(),
        aliases: vec![],
        description: "Fictional manufacturer".into(),
        kind: kind.into(),
        source_role: "organization".into(),
        properties: BTreeMap::new(),
    }
}

/// The worker artifact for a World of `nodes` and `claims`, as extraction
/// returns one: the whole graph, not a delta.
fn artifact(nodes: &[GraphNode], claims: &[GraphClaim]) -> String {
    let nodes: Vec<Value> = nodes
        .iter()
        .map(|n| {
            json!({"adapter_id": n.id, "name": n.name, "aliases": n.aliases, "description": n.description,
                "kind": n.kind, "source_role": n.source_role, "properties": n.properties})
        })
        .collect();
    let claims: Vec<Value> = claims
        .iter()
        .map(|c| {
            json!({"adapter_id": c.id, "proposition": c.proposition, "attribution": c.attribution,
                "relation": c.relation,
                "endpoints": c.endpoints.iter().map(|e| json!({"node_id": e.node_id, "role": e.role})).collect::<Vec<_>>(),
                "qualifiers": [],
                "evidence": c.evidence.iter().map(|e| json!({"span_id": e.span_id, "role": "extracted_from", "attribution": e.attribution})).collect::<Vec<_>>(),
                "status": c.status.as_str(), "recorded_at_ms": c.recorded_at_ms, "publication_time": null,
                "effective_time": null, "valid_at": null, "invalid_at": null, "expired_at": null})
        })
        .collect();
    json!({"format": "loupist.world_graph_snapshot", "nodes": nodes, "claims": claims}).to_string()
}

/// `span`'s Source goes through World extraction, which publishes `nodes` and
/// `claims` as the whole World, as a completed job does. A claim may cite a
/// span of this Source or of one a publication before it cited.
fn world_publishes(
    store: &KnowledgeStore,
    span: &BasisRef,
    nodes: &[GraphNode],
    claims: &[GraphClaim],
    at: i64,
) {
    let job = world_job(store, span, at);
    let running = store
        .graph_mark_running(&config(), &job.job_id, at)
        .unwrap();
    assert_eq!(running.status, super::super::graph_jobs::JOB_RUNNING);
    let attempt = running.document.attempt_id.clone().unwrap();
    store.graph_note_inference(&job.job_id, &attempt).unwrap();
    let artifact = artifact(nodes, claims);
    let finished = store
        .graph_publish_and_complete(
            &mut config(),
            &job.job_id,
            &attempt,
            &artifact,
            &crate::util::sha256(artifact.as_bytes()),
            at,
            false,
        )
        .unwrap();
    assert_eq!(finished.status, super::super::graph_jobs::JOB_COMPLETED);
}

/// One paid extraction attempt on `span`'s Source ends without publishing.
/// The first such attempt runs the job again; the second quarantines it.
fn extraction_fails(store: &KnowledgeStore, span: &BasisRef, at: i64) -> String {
    let job = world_job(store, span, at);
    let running = store
        .graph_mark_running(&config(), &job.job_id, at)
        .unwrap();
    assert_eq!(running.status, super::super::graph_jobs::JOB_RUNNING);
    let attempt = running.document.attempt_id.clone().unwrap();
    store.graph_note_inference(&job.job_id, &attempt).unwrap();
    store
        .graph_finish(
            &mut config(),
            &job.job_id,
            &attempt,
            super::super::graph_jobs::JOB_FAILED_BEFORE,
            false,
            false,
            at,
        )
        .unwrap()
        .status
}

/// The Source records the current World revision depends on.
fn world_dependencies(store: &KnowledgeStore) -> BTreeSet<String> {
    let (_, head) = world_head(store);
    store
        .get_revision(LOUPIST, &head)
        .unwrap()
        .dependencies
        .into_iter()
        .map(|d| d.record_id)
        .collect()
}

/// `reseal_graph` with company nodes.
fn reseal_claiming(
    store: &KnowledgeStore,
    nodes: Vec<(&str, &str)>,
    claims: Vec<GraphClaim>,
    at: i64,
) {
    let nodes = nodes
        .into_iter()
        .map(|(id, name)| company_node(id, name, "company"))
        .collect();
    reseal_graph(store, nodes, claims, at);
}

/// Publishes the World again with `nodes` and `claims`.
fn reseal_graph(store: &KnowledgeStore, nodes: Vec<GraphNode>, claims: Vec<GraphClaim>, at: i64) {
    let (record_id, head) = world_head(store);
    store
        .seal_world(SealWorld {
            loupist_id: LOUPIST.into(),
            record_id,
            expected_revision_id: Some(head),
            at_ms: at,
            nodes,
            claims,
            annotations: vec![],
            dependencies: vec![],
        })
        .unwrap();
}

/// How many subject anchors the World has not confirmed in storage.
fn pending_subjects(store: &KnowledgeStore) -> i64 {
    store
        .read(|w| {
            Ok(w.tx.query_row(
                "SELECT COUNT(*) FROM knowledge_lens_anchors WHERE role='subject' AND node_id IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn companies_filed_before_the_world_knew_them_learn_once_their_sources_confirm_them() {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    // Each Chat names its company plainly, before any extraction.
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    assert!(put_in(
        &store,
        "chat-1",
        "m1",
        halvorsen,
        text,
        std::slice::from_ref(&a),
        AT + 2
    )
    .is_none());
    let ferroluz = named("Ferroluz ramp", "Ferroluz Componentes");
    assert!(put_in(
        &store,
        "chat-2",
        "m2",
        ferroluz,
        text,
        std::slice::from_ref(&b),
        AT + 3
    )
    .is_none());
    assert!(review_states(&store).is_empty());
    // Extraction canonicalizes both with legal forms, each from the report its
    // Chat cited. The publication confirms both pages and queues the review.
    world_read(&store, &[&a, &b], AT + 4);
    let mut nodes: Vec<(&str, &str)> = (0..4)
        .map(|n| {
            (
                ["entity-0", "entity-1", "entity-2", "entity-3"][n],
                [
                    "Manufacturer 0",
                    "Manufacturer 1",
                    "Manufacturer 2",
                    "Manufacturer 3",
                ][n],
            )
        })
        .collect();
    nodes.extend([
        ("entity-h", "Halvorsen Cellworks ASA"),
        ("entity-f", "Ferroluz Componentes S.A."),
    ]);
    let claims = vec![
        claim_citing("clm_a", "entity-h", "entity-0", &a),
        claim_citing("clm_b", "entity-f", "entity-1", &b),
    ];
    reseal_claiming(&store, nodes.clone(), claims.clone(), AT + 4);
    assert_eq!(review_states(&store), ["queued"]);
    // Confirmation is for learning only: storage still holds both anchors
    // pending, and the next publication does not queue a second review.
    assert_eq!(pending_subjects(&store), 2);
    reseal_claiming(&store, nodes, claims, AT + 5);
    assert_eq!(review_states(&store), ["queued"]);
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["Ferroluz ramp", "Halvorsen ramp"]
    );
}

/// A subject page about the World node `node_id`.
fn about_node(title: &str, node_id: &str) -> PageTarget {
    PageTarget::Create {
        kind: PageKind::Subject,
        title: title.into(),
        anchors: vec![AnchorInput::Node {
            node_id: node_id.into(),
        }],
    }
}

/// In a World of the manufacturers, `extra` and `claims`, files a page named
/// `name` in one Chat citing `cited` and `second` in another citing `other`,
/// then the World reads both Sources. Whether a review queues says whether
/// the gate sees two cases. Storage never binds the pending anchor.
fn two_cases_with(
    store: &KnowledgeStore,
    name: &str,
    cited: &BasisRef,
    second: PageTarget,
    other: &BasisRef,
    extra: Vec<GraphNode>,
    claims: Vec<GraphClaim>,
) -> bool {
    let mut nodes = manufacturers_and(&[]);
    nodes.extend(extra);
    reseal_graph(store, nodes, claims, AT + 2);
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let page = named(&format!("{name} ramp"), name);
    let cited_only = std::slice::from_ref(cited);
    assert!(put_in(store, "chat-1", "m1", page, text, cited_only, AT + 3).is_none());
    let other_only = std::slice::from_ref(other);
    assert!(put_in(store, "chat-2", "m2", second, text, other_only, AT + 4).is_none());
    assert_eq!(pending_subjects(store), 1);
    world_read(store, &[cited, other], AT + 5);
    assert_eq!(
        pending_subjects(store),
        1,
        "storage never binds a pending anchor"
    );
    !review_states(store).is_empty()
}

/// [`two_cases_with`] beside Manufacturer 0, a company unrelated to `name`:
/// whether the Sources let the page count at all.
fn sources_confirm(
    store: &KnowledgeStore,
    name: &str,
    cited: &BasisRef,
    other: &BasisRef,
    extra: Vec<GraphNode>,
    claims: Vec<GraphClaim>,
) -> bool {
    two_cases_with(store, name, cited, company(0), other, extra, claims)
}

#[test]
fn a_claim_on_the_cited_span_confirms_the_company_for_learning() {
    let (_dir, store) = setup();
    let (report, other) = (source(&store, "Report"), source(&store, "Filing C"));
    let nodes = vec![
        company_node("entity-h", "Halvorsen Cellworks ASA", "company"),
        company_node("entity-m", "Meridian Motors", "Organization"),
    ];
    let claims = vec![claim_citing("clm_1", "entity-h", "entity-m", &report)];
    assert!(sources_confirm(
        &store,
        "Halvorsen Cellworks",
        &report,
        &other,
        nodes,
        claims
    ));
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["Halvorsen Cellworks ramp", "Manufacturer 0"]
    );
}

#[test]
fn two_pages_the_world_never_extracts_learn_once_their_sources_are_read() {
    // The World read both reports and published nothing, as an extraction
    // the model never completes leaves it. Each page counts under the name
    // it was filed under, and the two names are two companies.
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    assert!(put_in(
        &store,
        "chat-1",
        "m1",
        halvorsen,
        text,
        std::slice::from_ref(&a),
        AT + 2
    )
    .is_none());
    let ferroluz = named("Ferroluz ramp", "Ferroluz Componentes");
    assert!(put_in(
        &store,
        "chat-2",
        "m2",
        ferroluz,
        text,
        std::slice::from_ref(&b),
        AT + 3
    )
    .is_none());
    world_read(&store, &[&a], AT + 4);
    assert!(review_states(&store).is_empty(), "Filing B is unread");
    world_read(&store, &[&b], AT + 5);
    assert_eq!(review_states(&store), ["queued"]);
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["Ferroluz ramp", "Halvorsen ramp"]
    );
    assert_eq!(pending_subjects(&store), 2, "nothing is persisted");
}

#[test]
fn two_pending_spellings_of_one_company_never_count_twice() {
    let (_dir, store) = setup();
    let (a, b, c) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
    );
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let plain = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(
        &store,
        "chat-1",
        "m1",
        plain,
        text,
        std::slice::from_ref(&a),
        AT + 2
    )
    .is_none());
    let suffixed = named("Kestrel utilization", "Kestrel Foundry Inc.");
    assert!(put_in(
        &store,
        "chat-2",
        "m2",
        suffixed,
        text,
        std::slice::from_ref(&b),
        AT + 3
    )
    .is_none());
    // Both reports read, neither name extracted: one company key, one case.
    world_read(&store, &[&a, &b], AT + 4);
    assert!(review_states(&store).is_empty());
    // Both pages are evidence about that company beside a second one.
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 5).is_some());
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["Kestrel ramp", "Kestrel utilization", "Manufacturer 0"]
    );
}

#[test]
fn a_short_name_on_a_span_naming_two_companies_is_a_case_with_both() {
    // A loan report's passage says the bank lends to the borrower. A "Hana"
    // page citing that passage could be about either, so it is one case with
    // a page about the bank and with a page about the borrower, and two
    // cases only beside a company the passage never names.
    let bank_world = || {
        let nodes = vec![
            company_node("entity-b", "Hana Financial Group", "company"),
            company_node("entity-c", "Hana Co.", "company"),
        ];
        (nodes, |loan: &BasisRef| {
            vec![claim_citing("clm_loan", "entity-b", "entity-c", loan)]
        })
    };
    for (second, expected) in [
        (about_node("Hana Financial Group", "entity-b"), false),
        (about_node("Hana Co.", "entity-c"), false),
        (company(0), true),
    ] {
        let (_dir, store) = setup();
        let (loan, other) = (source(&store, "Loan report"), source(&store, "Filing C"));
        let (nodes, claims) = bank_world();
        let title = match &second {
            PageTarget::Create { title, .. } => title.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            two_cases_with(&store, "Hana", &loan, second, &other, nodes, claims(&loan)),
            expected,
            "beside {title}"
        );
    }
}

#[test]
fn a_claim_on_another_passage_of_the_cited_report_names_a_pending_pages_company() {
    // The report names the bank in its first passage. A "Hana" page cites the
    // second passage, whose extraction published no claim. The page may still
    // be about the bank its report names, so it is one case with a page about
    // the bank's node whichever passage carries the claim, at admission and at
    // the next publication, and two cases only beside a company the report
    // never names.
    for claimed_on_cited in [false, true] {
        let nodes = || {
            vec![
                company_node("entity-b", "Hana Financial Group", "company"),
                company_node("entity-m", "Meridian Motors", "company"),
            ]
        };
        let claim = |first: &BasisRef, second: &BasisRef| {
            let span = if claimed_on_cited { second } else { first };
            vec![claim_citing("clm_1", "entity-b", "entity-m", span)]
        };
        let (_dir, store) = setup();
        let (first_passage, second_passage) = two_span_source(&store, "Sector report");
        let other = source(&store, "Filing C");
        let claims = claim(&first_passage, &second_passage);
        assert!(
            !two_cases_with(
                &store,
                "Hana",
                &second_passage,
                about_node("Hana Financial Group", "entity-b"),
                &other,
                nodes(),
                claims.clone()
            ),
            "claim on the cited passage: {claimed_on_cited}"
        );
        let mut all = manufacturers_and(&[]);
        all.extend(nodes());
        reseal_graph(&store, all, claims, AT + 6);
        assert!(
            review_states(&store).is_empty(),
            "a publication reads it the same"
        );
        assert_eq!(pending_subjects(&store), 1);
        let (_dir, store) = setup();
        let (first_passage, second_passage) = two_span_source(&store, "Sector report");
        let other = source(&store, "Filing C");
        let claims = claim(&first_passage, &second_passage);
        assert!(
            sources_confirm(&store, "Hana", &second_passage, &other, nodes(), claims),
            "beside Manufacturer 0, claim on the cited passage: {claimed_on_cited}"
        );
        assert_eq!(
            pages(&begin(&store, AT + 6)),
            ["Hana ramp", "Manufacturer 0"]
        );
    }
}

#[test]
fn a_pending_spelling_is_a_case_with_the_component_of_the_nodes_keyed_like_it() {
    // The World holds `삼성전자` and a node spelling it both ways. A page about
    // the Korean node and a pending "Samsung Electronics Inc." page whose
    // report published no claim are one company: the English keys meet the
    // bridge node, whose component holds the Korean one. Admission and the
    // next publication both read it so; the anchor stays pending; beside an
    // unrelated company the pending page still counts.
    let nodes = || {
        vec![
            company_node("entity-korean", "삼성전자", "company"),
            company_node("entity-bridge", "삼성전자(Samsung Electronics)", "company"),
        ]
    };
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Report A"), source(&store, "Report B"));
    assert!(!two_cases_with(
        &store,
        "Samsung Electronics Inc.",
        &a,
        about_node("삼성전자", "entity-korean"),
        &b,
        nodes(),
        vec![]
    ));
    let mut all = manufacturers_and(&[]);
    all.extend(nodes());
    reseal_graph(&store, all, vec![], AT + 6);
    assert!(
        review_states(&store).is_empty(),
        "a publication reads it the same"
    );
    assert_eq!(
        pending_subjects(&store),
        1,
        "storage binds exact spellings only"
    );
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Report A"), source(&store, "Report B"));
    assert!(sources_confirm(
        &store,
        "Samsung Electronics Inc.",
        &a,
        &b,
        nodes(),
        vec![]
    ));
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["Manufacturer 0", "Samsung Electronics Inc. ramp"]
    );
}

#[test]
fn a_withdrawn_claim_or_a_product_names_no_company_for_a_pending_page() {
    // Only an active claim binds a page to the counterparty on its span.
    for (status, expected_two_cases) in
        [(ClaimStatus::Withdrawn, true), (ClaimStatus::Active, false)]
    {
        let (_dir, store) = setup();
        let (report, other) = (source(&store, "Report"), source(&store, "Filing C"));
        let nodes = vec![
            company_node("entity-h", "Halvorsen Cellworks ASA", "company"),
            company_node("entity-m", "Meridian Motors", "company"),
        ];
        let claims = vec![claim_with_status(
            "clm_1", "entity-h", "entity-m", &report, status,
        )];
        assert_eq!(
            two_cases_with(
                &store,
                "Halvorsen Cellworks",
                &report,
                about_node("Meridian Motors", "entity-m"),
                &other,
                nodes,
                claims
            ),
            expected_two_cases,
            "{status:?}"
        );
    }
    // A Product on the span is no company the page could be about.
    for (kind, expected_two_cases) in [("Product", true), ("company", false)] {
        let (_dir, store) = setup();
        let (report, other) = (source(&store, "Report"), source(&store, "Filing C"));
        let nodes = vec![
            company_node("entity-h", "Halvorsen Cellworks ASA", kind),
            company_node("entity-m", "Meridian Motors", "company"),
        ];
        let claims = vec![claim_citing("clm_1", "entity-h", "entity-m", &report)];
        assert_eq!(
            two_cases_with(
                &store,
                "Zephyr Metals",
                &report,
                about_node("Halvorsen Cellworks ASA", "entity-h"),
                &other,
                nodes,
                claims
            ),
            expected_two_cases,
            "{kind}"
        );
    }
}

#[test]
fn a_pending_spelling_the_world_keys_differently_is_a_case_with_its_node() {
    // The page spells the company one way, the World another: a parenthetical
    // in another script, dropped initials, or a short name. Each counts beside
    // an unrelated company and is one case with a page about the node.
    for (node_name, page_name) in [
        ("삼성전자(Samsung Electronics)", "Samsung Electronics"),
        ("U.S. Bancorp", "US Bancorp"),
        ("Hana Financial Group", "Hana"),
    ] {
        let nodes = || {
            vec![
                company_node("entity-x", node_name, "company"),
                company_node("entity-m", "Meridian Motors", "company"),
            ]
        };
        let (_dir, store) = setup();
        let (report, other) = (source(&store, "Report"), source(&store, "Filing C"));
        let claims = vec![claim_citing("clm_1", "entity-x", "entity-m", &report)];
        assert!(
            sources_confirm(&store, page_name, &report, &other, nodes(), claims),
            "{page_name} beside Manufacturer 0"
        );
        let mut expected = [format!("{page_name} ramp"), "Manufacturer 0".to_owned()];
        expected.sort();
        assert_eq!(pages(&begin(&store, AT + 6)), expected);
        let (_dir, store) = setup();
        let (report, other) = (source(&store, "Report"), source(&store, "Filing C"));
        let claims = vec![claim_citing("clm_1", "entity-x", "entity-m", &report)];
        assert!(
            !two_cases_with(
                &store,
                page_name,
                &report,
                about_node(node_name, "entity-x"),
                &other,
                nodes(),
                claims
            ),
            "{page_name} beside {node_name}"
        );
    }
}

#[test]
fn a_page_citing_a_source_the_world_has_not_read_waits_for_it() {
    let (_dir, store) = setup();
    let (report, other) = (source(&store, "Report"), source(&store, "Filing C"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let page = named("Halvorsen ramp", "Halvorsen Cellworks");
    let report_only = std::slice::from_ref(&report);
    assert!(put_in(&store, "chat-1", "m1", page, text, report_only, AT + 2).is_none());
    assert!(turn(
        &store,
        "chat-2",
        "m2",
        0,
        std::slice::from_ref(&other),
        AT + 3
    )
    .is_none());
    let extra = [
        ("entity-h", "Halvorsen Cellworks ASA"),
        ("entity-m", "Meridian Motors"),
    ];
    let mut nodes: Vec<(&str, &str)> = vec![
        ("entity-0", "Manufacturer 0"),
        ("entity-1", "Manufacturer 1"),
        ("entity-2", "Manufacturer 2"),
        ("entity-3", "Manufacturer 3"),
    ];
    nodes.extend(extra);
    let claims = vec![claim_citing("clm_1", "entity-h", "entity-m", &report)];
    // A publication that claims from the report while its job is still queued
    // (another of its spans may still name a second spelling) holds the page.
    world_read(&store, &[&other], AT + 4);
    let queued = world_job(&store, &report, AT + 4);
    assert_eq!(queued.status, super::super::graph_jobs::JOB_QUEUED);
    reseal_claiming(&store, nodes.clone(), claims.clone(), AT + 4);
    assert!(review_states(&store).is_empty());
    // Once the job is through, the next publication confirms the page, with
    // no new filing.
    world_read(&store, &[&report], AT + 5);
    reseal_claiming(&store, nodes, claims, AT + 5);
    assert_eq!(review_states(&store), ["queued"]);
}

/// The setup's manufacturers beside `extra` company nodes.
fn manufacturers_and(extra: &[(&str, &str)]) -> Vec<GraphNode> {
    (0..4)
        .map(|n| {
            company_node(
                &format!("entity-{n}"),
                &format!("Manufacturer {n}"),
                "company",
            )
        })
        .chain(
            extra
                .iter()
                .map(|(id, name)| company_node(id, name, "company")),
        )
        .collect()
}

#[test]
fn a_source_the_world_read_without_a_claim_stays_processed_through_later_publications() {
    let (_dir, store) = setup();
    let (h_report, h_metrics) = (
        source(&store, "Halvorsen report"),
        source(&store, "Halvorsen metrics"),
    );
    let (f_report, f_metrics) = (
        source(&store, "Ferroluz report"),
        source(&store, "Ferroluz metrics"),
    );
    let unrelated = source(&store, "Unrelated report");
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    // Each Chat cites its company's report and a metrics table, before any
    // extraction; attachment intake queues a World job for every Source.
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    let h_cited = [h_report.clone(), h_metrics.clone()];
    assert!(put_in(&store, "chat-1", "m1", halvorsen, text, &h_cited, AT + 2).is_none());
    let ferroluz = named("Ferroluz ramp", "Ferroluz Componentes");
    let f_cited = [f_report.clone(), f_metrics.clone()];
    assert!(put_in(&store, "chat-2", "m2", ferroluz, text, &f_cited, AT + 3).is_none());
    for span in [&h_report, &h_metrics, &f_report, &f_metrics, &unrelated] {
        assert_eq!(
            world_job(&store, span, AT + 4).status,
            super::super::graph_jobs::JOB_QUEUED
        );
    }
    let nodes = manufacturers_and(&[
        ("entity-h", "Halvorsen Cellworks ASA"),
        ("entity-f", "Ferroluz Componentes S.A."),
        ("entity-u", "Unrelated Works"),
    ]);
    let mut claims = vec![claim_citing("clm_h", "entity-h", "entity-0", &h_report)];
    world_publishes(&store, &h_report, &nodes, &claims, AT + 5);
    claims.push(claim_citing("clm_f", "entity-f", "entity-1", &f_report));
    world_publishes(&store, &f_report, &nodes, &claims, AT + 6);
    // Both reports are extracted, but each page also cites a metrics Source
    // whose job is still queued: the World may yet name a second spelling.
    assert!(review_states(&store).is_empty());
    // The metrics tables yield no claim. That is a complete reading.
    world_publishes(&store, &h_metrics, &nodes, &claims, AT + 7);
    assert!(review_states(&store).is_empty(), "Ferroluz still waits");
    world_publishes(&store, &f_metrics, &nodes, &claims, AT + 8);
    assert_eq!(review_states(&store), ["queued"]);
    // An unrelated publication drops the metrics Sources from the revision's
    // dependency list. The Sources stay processed, and the review reads both.
    claims.push(claim_citing("clm_u", "entity-u", "entity-2", &unrelated));
    world_publishes(&store, &unrelated, &nodes, &claims, AT + 9);
    let dependencies = world_dependencies(&store);
    assert!(!dependencies.contains(&record_of(&h_metrics)));
    assert!(dependencies.contains(&record_of(&h_report)));
    assert_eq!(review_states(&store), ["queued"]);
    assert_eq!(
        pages(&begin(&store, AT + 10)),
        ["Ferroluz ramp", "Halvorsen ramp"]
    );
    assert_eq!(pending_subjects(&store), 2, "nothing is persisted");
}

#[test]
fn a_source_whose_extraction_was_quarantined_is_processed_and_rechecks_learning() {
    let (_dir, store) = setup();
    let (h_report, h_metrics) = (
        source(&store, "Halvorsen report"),
        source(&store, "Halvorsen metrics"),
    );
    let other = source(&store, "Filing C");
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    let h_cited = [h_report.clone(), h_metrics.clone()];
    assert!(put_in(&store, "chat-1", "m1", halvorsen, text, &h_cited, AT + 2).is_none());
    assert!(turn(&store, "chat-2", "m2", 0, &[other], AT + 3).is_none());
    let nodes = manufacturers_and(&[
        ("entity-h", "Halvorsen Cellworks ASA"),
        ("entity-m", "Meridian Motors"),
    ]);
    let claims = [claim_citing("clm_h", "entity-h", "entity-m", &h_report)];
    world_publishes(&store, &h_report, &nodes, &claims, AT + 4);
    assert!(
        review_states(&store).is_empty(),
        "the metrics job is queued"
    );
    // Two paid attempts on the metrics table publish nothing. The first runs
    // the job again; the second quarantines it. A quarantined job is through:
    // the World cannot reveal more from that Source, so the page counts, with
    // no publication and no new filing.
    assert_eq!(
        extraction_fails(&store, &h_metrics, AT + 5),
        super::super::graph_jobs::JOB_QUEUED
    );
    assert!(review_states(&store).is_empty());
    assert_eq!(
        extraction_fails(&store, &h_metrics, AT + 6),
        super::super::graph_jobs::JOB_RECONCILE
    );
    assert_eq!(review_states(&store), ["queued"]);
    assert_eq!(
        pages(&begin(&store, AT + 7)),
        ["Halvorsen ramp", "Manufacturer 0"]
    );
    assert_eq!(pending_subjects(&store), 1);
}

#[test]
fn a_resolved_anchor_is_its_node_not_the_name_it_was_filed_under() {
    let named = |title: &str, name: &str| PageTarget::Create {
        kind: PageKind::Idea,
        title: title.into(),
        anchors: vec![AnchorInput::Name { name: name.into() }],
    };
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    // Two companies that held one name at different times are two cases.
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    let first = named("Ramp thesis", "Manufacturer 1");
    assert!(put_in(&store, "chat-1", "m1", first, text, &[a], AT + 2).is_none());
    reseal(
        &store,
        vec![
            ("entity-0", "Manufacturer 0"),
            ("entity-1", "Manufacturer One"),
            ("entity-2", "Manufacturer 1"),
        ],
        AT + 3,
    );
    let second = named("Ramp thesis 2", "Manufacturer 1");
    assert!(put_in(&store, "chat-2", "m2", second, text, &[b], AT + 4).is_some());

    // A pending name and the node it later resolves to are one case.
    let (_dir, store) = setup();
    let (a, b, c) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
    );
    let pending = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-1", "m1", pending, text, &[a], AT + 2).is_none());
    reseal(
        &store,
        vec![
            ("entity-0", "Manufacturer 0"),
            ("entity-k", "Kestrel Foundry"),
        ],
        AT + 3,
    );
    let resolved = PageTarget::Create {
        kind: PageKind::Subject,
        title: "Kestrel Foundry".into(),
        anchors: vec![AnchorInput::Node {
            node_id: "entity-k".into(),
        }],
    };
    assert!(put_in(&store, "chat-2", "m2", resolved, text, &[b], AT + 4).is_none());
    assert!(review_states(&store).is_empty());
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 5).is_some());
}

fn named(title: &str, name: &str) -> PageTarget {
    PageTarget::Create {
        kind: PageKind::Idea,
        title: title.into(),
        anchors: vec![AnchorInput::Name { name: name.into() }],
    }
}

/// `reseal` with the setup's manufacturers kept beside `extra`.
fn reseal_with(store: &KnowledgeStore, extra: &[(&str, &str)], at: i64) {
    let kept: Vec<_> = (0..4)
        .map(|n| (format!("entity-{n}"), format!("Manufacturer {n}")))
        .collect();
    let mut nodes: Vec<(&str, &str)> = kept.iter().map(|(i, n)| (i.as_str(), n.as_str())).collect();
    nodes.extend_from_slice(extra);
    reseal(store, nodes, at);
}

#[test]
fn two_spellings_of_one_company_are_never_two_cases() {
    let (_dir, store) = setup();
    let (a, b, c) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
    );
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let plain = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-1", "m1", plain, text, &[a], AT + 2).is_none());
    let suffixed = named("Kestrel utilization", "Kestrel Foundry Inc.");
    assert!(put_in(&store, "chat-2", "m2", suffixed, text, &[b], AT + 3).is_none());
    assert!(review_states(&store).is_empty());
    // The World confirms each spelling as its own node. One company key, so
    // two independent Chats about them are still one case.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-k2", "Kestrel Foundry Inc."),
        ],
        AT + 4,
    );
    assert!(review_states(&store).is_empty());
    // Both pages are now evidence about that company, beside a second one.
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 5).is_some());
    let mut pages: Vec<_> = begin(&store, AT + 6)
        .input
        .observations
        .iter()
        .map(|o| o.page.clone())
        .collect();
    pages.sort();
    assert_eq!(
        pages,
        ["Kestrel ramp", "Kestrel utilization", "Manufacturer 0"]
    );
}

/// Two Chats file about two World nodes named `first` and `second`; whether
/// their confirmation starts a review says whether the gate sees two companies.
fn two_nodes_learn(first: &str, second: &str) -> bool {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let one = named("First ramp", first);
    assert!(put_in(&store, "chat-1", "m1", one, text, &[a], AT + 2).is_none());
    let other = named("Second ramp", second);
    assert!(put_in(&store, "chat-2", "m2", other, text, &[b], AT + 3).is_none());
    assert!(review_states(&store).is_empty());
    reseal_with(&store, &[("entity-x", first), ("entity-y", second)], AT + 4);
    let queued = !review_states(&store).is_empty();
    if !queued {
        // One company on two nodes is still evidence beside a third company.
        let c = source(&store, "Filing C");
        assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 5).is_some());
        let shown = pages(&begin(&store, AT + 6));
        assert_eq!(shown, ["First ramp", "Manufacturer 0", "Second ramp"]);
    }
    queued
}

#[test]
fn one_company_spelled_two_ways_on_two_nodes_is_one_case() {
    for (first, second) in [
        ("U.S. Bancorp", "US Bancorp"),
        ("J.P. Morgan", "JPMorgan"),
        ("A.O. Smith Corporation", "AO Smith"),
        ("Kestrel Foundry", "Kestrel Foundry Inc."),
        (
            "삼성전자(Samsung Electronics)",
            "Samsung Electronics Co., Ltd.",
        ),
        ("Apple", "Apple Inc. (NASDAQ:AAPL)"),
        ("삼성전자", "삼성전자 (KRX:005930)"),
        ("CEMEX", "CEMEX, S.A.B. de C.V."),
    ] {
        assert!(!two_nodes_learn(first, second), "{first} / {second}");
    }
}

#[test]
fn a_node_spelling_both_names_makes_one_case_of_two_nodes() {
    let (_dir, store) = setup();
    let (a, b, c) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
    );
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let korean = named("Korean ramp", "삼성전자");
    assert!(put_in(&store, "chat-1", "m1", korean, text, &[a], AT + 2).is_none());
    let english = named("English ramp", "Samsung Electronics");
    assert!(put_in(&store, "chat-2", "m2", english, text, &[b], AT + 3).is_none());
    // The two pages name nodes that share no key. A third node, with no page,
    // spells both, so the World itself says they are one company.
    reseal_with(
        &store,
        &[
            ("entity-a", "삼성전자"),
            ("entity-b", "삼성전자(Samsung Electronics)"),
            ("entity-c", "Samsung Electronics"),
        ],
        AT + 4,
    );
    assert!(review_states(&store).is_empty());
    // An unrelated company beside them still learns.
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 5).is_some());
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["English ramp", "Korean ramp", "Manufacturer 0"]
    );
}

#[test]
fn two_companies_with_one_qualifier_are_two_cases() {
    for (first, second) in [
        ("삼성전자(한국)", "LG전자(한국)"),
        ("Acme (Inc.)", "Beta (Inc.)"),
        ("Acme (Korea)", "Beta (Korea)"),
        ("Kestrel Foundry", "Halvorsen Cellworks ASA"),
        ("EXCH:AG", "EXCH:SE"),
    ] {
        assert!(two_nodes_learn(first, second), "{first} / {second}");
    }
}

#[test]
fn a_page_about_a_node_the_world_dropped_is_not_evidence() {
    let (_dir, store) = setup();
    let (a, b, c) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
    );
    assert!(turn(&store, "chat-1", "m1", 3, &[a], AT + 2).is_none());
    // A pack removal drops Manufacturer 3; extraction knows it again under a new id.
    reseal(
        &store,
        vec![
            ("entity-0", "Manufacturer 0"),
            ("entity-1", "Manufacturer 1"),
            ("entity-2", "Manufacturer 2"),
            ("entity-3b", "Manufacturer 3"),
        ],
        AT + 3,
    );
    let again = PageTarget::Create {
        kind: PageKind::Idea,
        title: "Manufacturer 3 ramp".into(),
        anchors: vec![AnchorInput::Node {
            node_id: "entity-3b".into(),
        }],
    };
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    assert!(put_in(&store, "chat-2", "m2", again, text, &[b], AT + 4).is_none());
    assert!(review_states(&store).is_empty());
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 5).is_some());
    assert_eq!(
        pages(&begin(&store, AT + 6)),
        ["Manufacturer 0", "Manufacturer 3 ramp"]
    );
}

#[test]
fn a_publication_that_renames_a_fused_node_rechecks_without_a_filing() {
    let (_dir, store) = setup();
    let (a, b) = (source(&store, "Filing A"), source(&store, "Filing B"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let plain = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-1", "m1", plain, text, &[a], AT + 2).is_none());
    let suffixed = named("Kestrel utilization", "Kestrel Foundry Inc.");
    assert!(put_in(&store, "chat-2", "m2", suffixed, text, &[b], AT + 3).is_none());
    let fused = [
        ("entity-k", "Kestrel Foundry"),
        ("entity-k2", "Kestrel Foundry Inc."),
    ];
    reseal_with(&store, &fused, AT + 4);
    assert!(review_states(&store).is_empty());
    // A publication that spells both nodes as before changes nothing.
    reseal_with(&store, &fused, AT + 5);
    assert!(review_states(&store).is_empty());
    // One that renames the second node makes two companies of the pair.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-k2", "Halvorsen Cellworks"),
        ],
        AT + 6,
    );
    assert_eq!(review_states(&store), ["queued"]);
    assert_eq!(
        pages(&begin(&store, AT + 7)),
        ["Kestrel ramp", "Kestrel utilization"]
    );
}

/// Enough principles that every claim defers before inference.
fn too_many_principles(store: &KnowledgeStore) {
    let long = "Check cash conversion before trusting reported profit. ".repeat(6);
    store
        .transact(|w| {
            for i in 0..45 {
                let statement = format!("{i:02} {long}").chars().take(280).collect();
                super::super::principles::principle_save(
                    w.tx,
                    LOUPIST,
                    None,
                    None,
                    principle(statement),
                    AT + i,
                )?;
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_publication_that_spells_every_subject_as_before_does_not_recheck() {
    let (_dir, store) = setup();
    too_many_principles(&store);
    two_chats(&store, AT + 100).expect("queued");
    assert!(try_claim(&store, AT + 101).is_none());
    assert_eq!(review_states(&store), ["deferred"]);
    // The head was read, so only a change in who the pages are about rechecks.
    reseal_with(&store, &[], AT + 102);
    assert_eq!(review_states(&store), ["deferred"]);
    reseal(
        &store,
        vec![
            ("entity-0", "Manufacturer 0"),
            ("entity-1", "Manufacturer One"),
            ("entity-2", "Manufacturer 2"),
            ("entity-3", "Manufacturer 3"),
        ],
        AT + 103,
    );
    assert_eq!(review_states(&store), ["deferred", "queued"]);
}

#[test]
fn companies_the_world_has_not_confirmed_count_once_it_does() {
    let (_dir, store) = setup();
    let (a, b, c, d) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
        source(&store, "Filing D"),
    );
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let kestrel = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-1", "m1", kestrel, text, &[a], AT + 2).is_none());
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks ASA");
    assert!(put_in(&store, "chat-2", "m2", halvorsen, text, &[b], AT + 3).is_none());
    assert!(review_states(&store).is_empty());
    // A review reads the whole log, then settles before inference.
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 4).is_none());
    assert!(turn(&store, "chat-4", "m4", 1, &[d], AT + 5).is_some());
    remove_source(&store, "Filing D");
    assert!(try_claim(&store, AT + 6).is_none());
    assert_eq!(review_states(&store), ["deferred"]);
    // A publication that knows neither name changes nothing.
    reseal_with(&store, &[], AT + 7);
    assert_eq!(review_states(&store), ["deferred"]);
    // The one that confirms both companies queues with no new filing.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks ASA"),
        ],
        AT + 8,
    );
    assert_eq!(review_states(&store), ["deferred", "queued"]);
}

/// Sorted page titles a claimed review shows.
fn pages(a: &Attempt) -> Vec<String> {
    let mut pages: Vec<_> = a
        .input
        .observations
        .iter()
        .map(|o| o.page.clone())
        .collect();
    pages.sort();
    pages
}

#[test]
fn unconfirmed_research_outlives_a_review_that_reached_inference_without_it() {
    let (_dir, store) = setup();
    let (a, b, c, d) = (
        source(&store, "Filing A"),
        source(&store, "Filing B"),
        source(&store, "Filing C"),
        source(&store, "Filing D"),
    );
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let kestrel = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-1", "m1", kestrel, text, &[a], AT + 2).is_none());
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    assert!(put_in(&store, "chat-2", "m2", halvorsen, text, &[b], AT + 3).is_none());
    // Two confirmed companies queue a review that reaches inference without
    // the pending pages and settles.
    assert!(turn(&store, "chat-3", "m3", 0, &[c], AT + 4).is_none());
    assert!(turn(&store, "chat-4", "m4", 1, &[d], AT + 5).is_some());
    let first = begin(&store, AT + 6);
    assert_eq!(pages(&first), ["Manufacturer 0", "Manufacturer 1"]);
    defer(&store, &first, AT + 7);
    assert_eq!(review_states(&store), ["deferred"]);
    // The World confirms the two pending companies: their research was not
    // consumed, so it is a review of its own under the normal rate limit.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks"),
        ],
        AT + 8,
    );
    assert_eq!(review_states(&store), ["deferred", "pending"]);
    assert!(try_claim(&store, AT + 9).is_none());
    let second = begin(&store, AT + 6 + SIX_HOURS);
    assert_eq!(pages(&second), ["Halvorsen ramp", "Kestrel ramp"]);
}

/// The watermark and the row sets the latest started review stored.
fn latest_consumed(store: &KnowledgeStore) -> (i64, BTreeSet<i64>, BTreeSet<i64>) {
    store
        .read(|w| {
            let (through, stored): (i64, String) = w.tx.query_row(
                "SELECT through_event,input_json FROM knowledge_learning_reviews
                 WHERE started_at_ms IS NOT NULL ORDER BY started_at_ms DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let stored: Value = serde_json::from_str(&stored)?;
            let rows = |key: &str| serde_json::from_value(stored[key].clone()).unwrap();
            Ok((through, rows("shown"), rows("held")))
        })
        .unwrap()
}

#[test]
fn a_subject_the_world_never_confirms_holds_its_rows_without_pinning_the_watermark() {
    let (_dir, store) = setup();
    let (k, h) = (source(&store, "Filing K"), source(&store, "Filing H"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let kestrel = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-k", "mk", kestrel, text, &[k], AT + 2).is_none());
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    assert!(put_in(&store, "chat-h", "mh", halvorsen, text, &[h], AT + 3).is_none());
    let pending: BTreeSet<i64> = (1..=2).collect();
    // Five reviews of other companies each reach inference. None re-reads a
    // shown row, and each stores only the two held rows above its watermark.
    let step = SIX_HOURS + 10;
    for i in 0..5 {
        let at = AT + 10 + i * step;
        two_chats(&store, at).expect("queued");
        let attempt = begin(&store, at + 2);
        assert_eq!(pages(&attempt), ["Manufacturer 0", "Manufacturer 1"], "{i}");
        defer(&store, &attempt, at + 3);
        let (through, shown, held) = latest_consumed(&store);
        assert_eq!(through, 2 + 2 * (i + 1), "{i}");
        assert!(shown.is_empty(), "{i}: {shown:?}");
        assert_eq!(held, pending, "{i}");
    }
    assert_eq!(review_states(&store), vec!["deferred"; 5]);
    // The publication that confirms both companies still finds their rows.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks"),
        ],
        AT + 10 + 5 * step,
    );
    assert_eq!(review_states(&store).len(), 6);
    assert_eq!(review_states(&store)[5], "queued");
    let next = begin(&store, AT + 10 + 6 * step);
    assert_eq!(pages(&next), ["Halvorsen ramp", "Kestrel ramp"]);
    let (_, shown, held) = latest_consumed(&store);
    assert!(shown.is_empty() && held.is_empty(), "{shown:?} {held:?}");
}

#[test]
fn a_held_backlog_larger_than_one_input_drains_once_and_only_once() {
    let (_dir, store) = setup();
    let long = "Utilization, ramp cost and margin moved together. ".repeat(12);
    // Forty rows of research about two companies the World has not confirmed,
    // each in its own Chat citing its own Source.
    let mut backlog = BTreeSet::new();
    for i in 0..20 {
        for (company, name) in [
            ("kestrel", "Kestrel Foundry"),
            ("halvorsen", "Halvorsen Cellworks"),
        ] {
            let filing = source(&store, &format!("Filing {company} {i}"));
            let page = named(&format!("{name} ramp {i}"), name);
            let chat = format!("chat-{company}-{i}");
            let message = format!("m-{company}-{i}");
            assert!(put_in(&store, &chat, &message, page, &long, &[filing], AT + 2 + i).is_none());
            backlog.insert(message);
        }
    }
    // An unrelated review reaches inference past them and holds all forty.
    let start = AT + 100;
    two_chats(&store, start).expect("queued");
    let first = begin(&store, start + 2);
    assert_eq!(pages(&first), ["Manufacturer 0", "Manufacturer 1"]);
    defer(&store, &first, start + 3);
    let (through, _, held) = latest_consumed(&store);
    assert_eq!(held.len(), 40);
    // The World confirms both companies: more evidence than one input holds.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks"),
        ],
        start + 4,
    );
    assert_eq!(
        review_states(&store).last().map(String::as_str),
        Some("pending")
    );
    let step = SIX_HOURS + 10;
    let mut shown: BTreeSet<String> = BTreeSet::new();
    let mut rounds = 0;
    loop {
        rounds += 1;
        let at = start + 2 + rounds * step;
        let Some(attempt) = try_claim(&store, at) else {
            break;
        };
        let ids: BTreeSet<String> = attempt
            .input
            .observations
            .iter()
            .map(|o| o.message_id.clone())
            .collect();
        assert!(ids.len() < 40, "one input holds the whole backlog");
        let again: Vec<_> = shown.intersection(&ids).collect();
        assert!(
            again.is_empty(),
            "round {rounds} showed rows again: {again:?}"
        );
        shown.extend(ids);
        let (now_through, _, _) = latest_consumed(&store);
        assert!(
            now_through >= through,
            "round {rounds}: the watermark moved back"
        );
        // The second attempt is lost to a restart; the rest defer after inference.
        if rounds == 2 {
            store.transact(|w| recover(w.tx, at + 1)).unwrap();
        } else {
            defer(&store, &attempt, at + 1);
        }
        assert!(rounds < 8, "the backlog never drains");
    }
    assert_eq!(shown, backlog, "every held row was shown exactly once");
    assert!(rounds > 3, "the backlog took several reviews");
    assert!(!review_states(&store)
        .iter()
        .any(|s| matches!(s.as_str(), "pending" | "queued" | "running")));
}

#[test]
fn a_company_confirmed_while_a_review_runs_is_rechecked_when_it_settles() {
    let (_dir, store) = setup();
    let sources: Vec<_> = ["A", "B", "C", "D", "E", "F"]
        .iter()
        .map(|n| source(&store, &format!("Filing {n}")))
        .collect();
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    for (i, (title, name)) in [
        ("Kestrel ramp", "Kestrel Foundry"),
        ("Halvorsen ramp", "Halvorsen Cellworks"),
        ("Ferroluz ramp", "Ferroluz"),
        ("Daeil ramp", "Daeil Steel"),
    ]
    .into_iter()
    .enumerate()
    {
        let page = named(title, name);
        let (chat, message) = (format!("chat-{i}"), format!("m{i}"));
        let at = AT + 2 + i as i64;
        assert!(put_in(
            &store,
            &chat,
            &message,
            page,
            text,
            &[sources[i].clone()],
            at
        )
        .is_none());
    }
    // A review admitted at the log head defers before inference, so the head
    // counts as read: only a publication can bring the pending pages back.
    assert!(turn(&store, "chat-4", "m4", 0, &[sources[4].clone()], AT + 6).is_none());
    assert!(turn(&store, "chat-5", "m5", 1, &[sources[5].clone()], AT + 7).is_some());
    remove_source(&store, "Filing F");
    assert!(try_claim(&store, AT + 8).is_none());
    assert_eq!(review_states(&store), ["deferred"]);
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks"),
        ],
        AT + 9,
    );
    let running = begin(&store, AT + 10);
    assert_eq!(
        pages(&running),
        ["Halvorsen ramp", "Kestrel ramp", "Manufacturer 0"]
    );
    // Two more companies are confirmed while that review runs.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks"),
            ("entity-f", "Ferroluz"),
            ("entity-d", "Daeil Steel"),
        ],
        AT + 11,
    );
    assert_eq!(review_states(&store), ["deferred", "running"]);
    defer(&store, &running, AT + 12);
    assert_eq!(review_states(&store), ["deferred", "deferred", "pending"]);
    let next = begin(&store, AT + 10 + SIX_HOURS);
    assert_eq!(pages(&next), ["Daeil ramp", "Ferroluz ramp"]);
}

#[test]
fn a_company_confirmed_while_a_review_is_queued_is_rechecked_when_the_claim_defers() {
    let (_dir, store) = setup();
    too_many_principles(&store);
    let (k, h) = (source(&store, "Filing K"), source(&store, "Filing H"));
    let text = "Margins fell during a capacity ramp at 60 percent utilization.";
    let kestrel = named("Kestrel ramp", "Kestrel Foundry");
    assert!(put_in(&store, "chat-k", "mk", kestrel, text, &[k], AT + 50).is_none());
    let halvorsen = named("Halvorsen ramp", "Halvorsen Cellworks");
    assert!(put_in(&store, "chat-h", "mh", halvorsen, text, &[h], AT + 51).is_none());
    two_chats(&store, AT + 100).expect("queued");
    // The confirmation lands while the review is queued, so it only re-admits.
    reseal_with(
        &store,
        &[
            ("entity-k", "Kestrel Foundry"),
            ("entity-h", "Halvorsen Cellworks"),
        ],
        AT + 101,
    );
    assert_eq!(review_states(&store), ["queued"]);
    // The claim defers before inference; the settle still sees the confirmation.
    assert!(try_claim(&store, AT + 102).is_none());
    assert_eq!(
        reviews(&store)[0].reason.as_deref(),
        Some("principles_exceed_input_limit")
    );
    assert_eq!(review_states(&store), ["deferred", "queued"]);
}

#[test]
fn evidence_a_started_review_left_out_goes_to_the_next_review() {
    let (_dir, store) = setup();
    store
        .transact(|w| save_settings(w.tx, LOUPIST, false))
        .unwrap();
    let long = "Utilization, ramp cost and margin moved together. ".repeat(20);
    let sources: Vec<_> = (0..3)
        .map(|i| source(&store, &format!("Filing {i}")))
        .collect();
    for i in 0..20 {
        turn_with(
            &store,
            &format!("chat-{}", i % 3),
            &format!("m{i:02}"),
            i % 4,
            &long,
            &[sources[i % 3].clone()],
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
    let first = begin(&store, AT + 101);
    assert!(first.input.observations.len() < 20);
    defer(&store, &first, AT + 102);
    // What did not fit waits for the rate limit, then gets its own review.
    assert_eq!(review_states(&store), ["deferred", "pending"]);
    assert!(try_claim(&store, AT + 103).is_none());
    let second = begin(&store, AT + 101 + SIX_HOURS);
    let shown: BTreeSet<_> = ids(&first).into_iter().collect();
    let next: BTreeSet<_> = ids(&second).into_iter().collect();
    let again: Vec<_> = shown.intersection(&next).collect();
    assert!(again.is_empty(), "shown twice: {again:?}");
    let filed: BTreeSet<String> = store
        .read(|w| {
            let mut q =
                w.tx.prepare("SELECT op_id FROM knowledge_lens_log WHERE origin='turn'")?;
            let rows = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(&shown | &next, filed);
    // Once every observation was shown, nothing requeues.
    defer(&store, &second, AT + 102 + SIX_HOURS);
    assert_eq!(review_states(&store), ["deferred", "deferred"]);
}

#[test]
fn a_refine_sees_the_whole_method_and_never_replaces_text_it_was_not_shown() {
    let (_dir, store) = setup();
    two_chats(&store, AT + 2).expect("queued");
    let a = begin(&store, AT + 4);
    complete(&store, &a, output(&a), AT + 5);
    let id = reviews(&store)[0].lens_record_id.clone().unwrap();
    // The Loupist adds a safeguard of its own in a Chat, citing no user message.
    let write = |key: &str, text: &str, now: i64| {
        store
            .transact(|w| {
                w.put_section(
                    LOUPIST,
                    &id,
                    key,
                    text,
                    &turn_origin("chat-method", &format!("m-{key}-{now}")),
                    now,
                )
            })
            .unwrap();
    };
    let limits = || {
        store
            .read_page(LOUPIST, &id)
            .unwrap()
            .body
            .sections()
            .iter()
            .find(|s| s.key.as_str() == "limits")
            .unwrap()
            .markdown
            .clone()
    };
    write(
        "limits",
        "Never apply to lessors: leases hide ramp cost.",
        AT + 6,
    );
    two_chats(&store, AT + SIX_HOURS + 5);
    let b = begin(&store, AT + SIX_HOURS + 6);
    let sent = request(&b).unwrap().to_string();
    assert!(sent.contains("Never apply to lessors"), "{sent}");
    let mut refined = output(&b);
    refined["action"] = json!("refine");
    refined["targetRecordId"] = json!(id);
    refined["limitations"] =
        json!("Never apply to lessors: leases hide ramp cost. Two manufacturers only.");
    complete(&store, &b, refined.clone(), AT + SIX_HOURS + 7);
    assert_eq!(reviews(&store)[0].status, "refined");
    // A method too long to show in full is listed, but cannot be refined.
    for key in ["use-when", "steps", "checks", "limits"] {
        let long = format!("{key} ").repeat(1900 / (key.len() + 1));
        write(key, &long, AT + SIX_HOURS + 8);
    }
    let before = limits();
    assert!(before.starts_with("limits limits"), "{before}");
    two_chats(&store, AT + 2 * SIX_HOURS + 9);
    let c = begin(&store, AT + 2 * SIX_HOURS + 10);
    let sent = request(&c).unwrap().to_string();
    assert!(
        sent.contains("Check capacity ramp margin pressure"),
        "{sent}"
    );
    assert!(!sent.contains("limits limits"), "{sent}");
    refined["evidenceIds"] = json!(ids(&c));
    refined["limitations"] = json!("Learning's narrower limit.");
    complete(&store, &c, refined, AT + 2 * SIX_HOURS + 11);
    assert_eq!(reviews(&store)[0].reason.as_deref(), Some("invalid_output"));
    assert_eq!(limits(), before);
}
