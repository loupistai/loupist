//! A section's contested state is derived from the live World whenever it is
//! written, not only when a World publication withdraws a claim.
use super::*;
use crate::knowledge::lens_page::{
    AnchorInput, Basis, BasisRef, FilingOrigin, LensOp, OpOutcome, PageDelta, PageKind, PageTarget,
    SectionKey,
};

const LOUPIST: &str = "loupist-a";

fn user() -> FilingOrigin {
    FilingOrigin::User {
        chat_id: None,
        message_id: None,
    }
}

fn nodes() -> Vec<GraphNode> {
    vec![
        node("a", "Alpha", "Organization", BTreeMap::new()),
        node("b", "Beta", "Organization", BTreeMap::new()),
    ]
}

fn withdrawn(id: &str) -> GraphClaim {
    GraphClaim {
        status: ClaimStatus::Withdrawn,
        ..claim(id, "Alpha supplies Beta", "a", "b")
    }
}

fn put(key: &str, markdown: &str, claim_id: &str, expected: Option<String>) -> LensOp {
    LensOp::Put {
        key: SectionKey::new(key).unwrap(),
        heading: "Facts".into(),
        markdown: markdown.into(),
        basis: Basis {
            refs: vec![BasisRef::Claim {
                claim_id: claim_id.into(),
            }],
            uncertainty: None,
        },
        expected,
    }
}

fn alpha(target: Option<&str>, ops: Vec<LensOp>) -> PageDelta {
    PageDelta {
        target: match target {
            Some(record_id) => PageTarget::Existing {
                record_id: record_id.into(),
            },
            None => PageTarget::Create {
                kind: PageKind::Subject,
                title: "Alpha".into(),
                anchors: vec![AnchorInput::Node {
                    node_id: "a".into(),
                }],
            },
        },
        ops,
    }
}

fn file(store: &KnowledgeStore, delta: PageDelta, at_ms: i64) -> lens_page::FiledPage {
    store
        .transact(|write| write.file(LOUPIST, &user(), vec![delta], at_ms))
        .unwrap()
        .remove(0)
}

#[test]
fn a_scheduled_ending_is_recorded_when_due_without_a_new_world_publication() {
    use crate::knowledge::world_changes::{self, Axis, FeedQuery, Since};

    let (directory, store) = open_store();
    let due = AT + 1_000;
    let mut ending = claim("c1", "Alpha supplies Beta", "a", "b");
    ending.invalid_at = Some(TimePoint::from_unix_ms(due));
    let world = new_world(&store, LOUPIST, nodes(), vec![ending]);
    let record = file(
        &store,
        alpha(None, vec![put("facts", "Alpha supplies Beta.", "c1", None)]),
        AT + 100,
    )
    .record_id;
    let query = FeedQuery {
        loupist_id: LOUPIST.into(),
        since: Since::Millis {
            ms: due - 500,
            inclusive: false,
        },
        axis: Axis::Recorded,
        axis_explicit: true,
        about: Vec::new(),
        kinds: None,
        limit: 8,
        first_run: false,
        cutoff: None,
    };
    let cursor = store
        .transact(|write| {
            assert_eq!(
                crate::knowledge::reconcile_due_endings(write.tx, LOUPIST, due - 1)?,
                Some(due)
            );
            let before = write.read_lens_page_for_cutoff(LOUPIST, &record)?;
            assert_eq!(before.body.sections()[0].since_ms, None);
            let page = world_changes::finish(world_changes::collect(write.tx, &query)?);
            assert_eq!(page.body["changes"], json!([]));
            assert_eq!(page.body["lens"], json!([]));
            Ok(page.body["next"].as_str().unwrap().to_owned())
        })
        .unwrap();
    // The head stays fixed, but a later check sees the transition once.
    store
        .transact(|write| {
            assert_eq!(
                crate::knowledge::reconcile_due_endings(write.tx, LOUPIST, due)?,
                None
            );
            Ok(())
        })
        .unwrap();
    drop(store);
    KnowledgeStore::open(directory.path().join("knowledge.sqlite"))
        .unwrap()
        .transact(|write| {
            assert_eq!(
                crate::knowledge::reconcile_due_endings(write.tx, LOUPIST, due + 1)?,
                None
            );
            let after = write.read_lens_page_for_cutoff(LOUPIST, &record)?;
            let section = &after.body.sections()[0];
            assert_eq!(section.since_ms, Some(due));
            assert_eq!(section.claim_ids.as_deref().unwrap(), ["c1"]);
            let page = world_changes::finish(world_changes::collect(
                write.tx,
                &FeedQuery {
                    since: Since::Cursor(cursor),
                    ..query
                },
            )?);
            assert_eq!(page.revision_id, world.revision_id);
            assert_eq!(page.body["changes"].as_array().unwrap().len(), 1);
            assert_eq!(page.body["changes"][0]["kind"], "ended");
            assert_eq!(page.body["changes"][0]["recordedAtMs"], due);
            assert_eq!(page.body["lens"].as_array().unwrap().len(), 1);
            assert_eq!(page.body["lens"][0]["state"], "superseded");
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_section_citing_a_withdrawn_or_missing_claim_is_written_contested() {
    let (_directory, store) = open_store();
    let first = new_world(
        &store,
        LOUPIST,
        nodes(),
        vec![claim("c1", "Alpha supplies Beta", "a", "b")],
    );
    seal(
        &store,
        LOUPIST,
        &first.record_id,
        Some(first.revision_id.clone()),
        nodes(),
        vec![withdrawn("c1")],
    );
    let filed = file(
        &store,
        alpha(
            None,
            vec![
                put("facts", "Alpha supplies Beta.", "c1", None),
                put("ghost", "Alpha supplies nobody.", "c-never-existed", None),
            ],
        ),
        AT + 30,
    );
    let page = store.read_page(LOUPIST, &filed.record_id).unwrap();
    let contested: Vec<(&str, bool, Vec<String>)> = page
        .body
        .sections()
        .iter()
        .map(|section| {
            (
                section.key.as_str(),
                section.contested,
                section.contested_claim_ids.clone(),
            )
        })
        .collect();
    assert_eq!(
        contested,
        vec![
            ("facts", true, vec!["c1".to_owned()]),
            ("ghost", true, vec!["c-never-existed".to_owned()]),
        ]
    );
}

#[test]
fn undoing_a_replace_keeps_the_restored_section_contested() {
    let (_directory, store) = open_store();
    let world = new_world(
        &store,
        LOUPIST,
        nodes(),
        vec![claim("c1", "Alpha supplies Beta", "a", "b")],
    );
    let first = file(
        &store,
        alpha(None, vec![put("facts", "Alpha supplies Beta.", "c1", None)]),
        AT + 11,
    );
    let page_id = first.record_id;
    let current = store.read_page(LOUPIST, &page_id).unwrap();
    assert!(!current.body.sections()[0].contested);
    let digest = current.body.sections()[0].digest.clone();
    let replaced = file(
        &store,
        alpha(
            Some(&page_id),
            vec![put(
                "facts",
                "Alpha supplies Beta since 2024.",
                "c1",
                Some(digest),
            )],
        ),
        AT + 12,
    );
    assert!(matches!(
        replaced.outcomes[0].outcome,
        OpOutcome::Replaced { .. }
    ));
    seal(
        &store,
        LOUPIST,
        &world.record_id,
        Some(world.revision_id.clone()),
        nodes(),
        vec![withdrawn("c1")],
    );
    assert!(store.read_page(LOUPIST, &page_id).unwrap().body.sections()[0].contested);
    let undone = store
        .transact(|write| write.undo_op(LOUPIST, &replaced.outcomes[0].op_id, AT + 30))
        .unwrap();
    let section = &undone.body.sections()[0];
    assert_eq!(section.markdown, "Alpha supplies Beta.");
    assert!(section.contested);
    assert_eq!(section.contested_claim_ids, ["c1"]);
}
