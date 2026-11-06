use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::chat::home::{deliver_finished_runs, home_thread_id};
use crate::knowledge::{
    ClaimEndpoint, ClaimStatus, CreateRecord, GraphClaim, GraphNode, KnowledgeWrite, RecordKind,
    SealWorld,
};
use crate::store::user_context::MapContextItem;

fn node(id: &str, name: &str, kind: &str, ticker: Option<&str>) -> GraphNode {
    let mut properties = BTreeMap::new();
    if let Some(symbol) = ticker {
        properties.insert("ticker".to_owned(), json!(symbol));
    }
    GraphNode {
        id: id.into(),
        name: name.into(),
        aliases: Vec::new(),
        description: String::new(),
        kind: kind.into(),
        source_role: "entity".into(),
        properties,
    }
}

fn claim(id: &str, from: &str, to: &str) -> GraphClaim {
    GraphClaim {
        id: id.into(),
        proposition: format!("{from} supplies {to}"),
        attribution: "test".into(),
        relation: "SUPPLIES".into(),
        endpoints: vec![
            ClaimEndpoint {
                node_id: from.into(),
                role: "supplier".into(),
            },
            ClaimEndpoint {
                node_id: to.into(),
                role: "customer".into(),
            },
        ],
        qualifiers: Vec::new(),
        evidence: Vec::new(),
        status: ClaimStatus::Active,
        recorded_at_ms: Some(1),
        publication_time: None,
        effective_time: None,
        valid_at: None,
        invalid_at: None,
        expired_at: None,
    }
}

/// A World with one followed name whose new claims reach four far names.
fn publish_four_links(state: &AppState) {
    let blobs =
        crate::commands::knowledge::knowledge_blobs_for_loupist(state, "loupist-a").unwrap();
    let mut nodes = vec![
        node("a", "Alpha", "Organization", Some("A")),
        node("b", "Beta", "Organization", None),
    ];
    let mut claims = vec![claim("c1", "a", "b")];
    for far in ["c", "d", "e", "f"] {
        nodes.push(node(far, &far.to_uppercase(), "Technology", None));
        claims.push(claim(&format!("c-{far}"), "b", far));
    }
    crate::commands::assets::with_assets(state, "loupist-a", true, |tx| {
        let write = KnowledgeWrite::bind(tx, &blobs);
        let world = write.create_record(CreateRecord {
            loupist_id: "loupist-a".into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: 1,
        })?;
        write.seal_world_followed(
            SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: world.record_id,
                expected_revision_id: None,
                at_ms: 2,
                nodes,
                claims,
                annotations: Vec::new(),
                dependencies: Vec::new(),
            },
            &[MapContextItem {
                symbol: "A".into(),
                market: "KRX".into(),
                display_name: None,
            }],
        )?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_bot_shows_three_links_a_day_and_keeps_the_fourth_for_later() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", &temporary.path().join("workspace"), 1),
    );
    publish_four_links(&state);
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 3);
    assert_eq!(
        deliver_finished_runs(&state, "loupist-a").unwrap(),
        0,
        "the day's allowance is spent"
    );
    let thread = state
        .store
        .get_chat(&home_thread_id("loupist-a"))
        .unwrap()
        .unwrap();
    let posts: Vec<_> = thread
        .messages
        .iter()
        .filter_map(|message| message.link.clone())
        .collect();
    assert_eq!(posts.len(), 3);
    assert!(posts.iter().all(|post| post.seed_name == "Alpha"
        && post.path.len() == 2
        && post.path[0].name == "Beta"
        && post.node_ids.len() == 3));
    let stored = crate::commands::assets::with_assets(&state, "loupist-a", false, |tx| {
        let blobs = crate::knowledge::BlobStore::open(temporary.path().join("blobs"))?;
        KnowledgeWrite::bind(tx, &blobs).connections("loupist-a")
    })
    .unwrap();
    assert_eq!(stored.len(), 4);
    assert_eq!(
        stored
            .iter()
            .filter(|link| link.status == crate::knowledge::ConnectionStatus::New)
            .count(),
        1,
        "the fourth is stored, not posted"
    );
}
