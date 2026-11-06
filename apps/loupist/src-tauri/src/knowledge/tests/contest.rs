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
