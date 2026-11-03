//! The derived index and the candidate searches the resolvers read.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use crate::knowledge::world_extract::bridge::{Bridge, BridgeConfig};
use crate::knowledge::world_extract::ontology::{merged_schema, Schema};
use crate::knowledge::world_extract::pipeline::{
    ranked_by_similarity, Edge, Episode, Graph, Harness, SEARCH_LIMIT,
};
use crate::knowledge::world_extract::search::MemoryClaimText;
use crate::knowledge::world_extract::{index, text};
use crate::knowledge::WorldSnapshotBody;

/// A unit vector at `cos` from the first axis, in the first two dimensions.
fn at(cos: f32) -> Vec<f32> {
    let mut vector = vec![0.0; 128];
    vector[0] = cos;
    vector[1] = (1.0 - cos * cos).max(0.0).sqrt();
    vector
}

#[test]
fn name_candidates_keep_the_best_fifteen_above_the_similarity_floor() {
    let query = at(1.0);
    // Normalized similarity (1 + cos) / 2 must exceed 0.6, so cos > 0.2.
    let mut rows: Vec<(String, Vec<f32>)> = (0..20)
        .map(|index| (format!("n{index:02}"), at(1.0 - index as f32 * 0.05)))
        .collect();
    rows.push(("tie".into(), at(1.0)));
    rows.push(("zero".into(), vec![0.0; 128]));
    rows.push(("short".into(), vec![1.0; 3]));
    let ranked = ranked_by_similarity(
        &query,
        rows.iter()
            .map(|(key, vector)| (key.as_str(), vector.as_slice())),
        15,
    );
    assert_eq!(ranked.len(), 15);
    // Equal scores keep stored order; a zero or mismatched vector is never a candidate.
    assert_eq!(&ranked[..2], ["n00", "tie"]);
    assert!(!ranked.contains(&"zero".to_string()) && !ranked.contains(&"short".to_string()));
    let few = ranked_by_similarity(
        &query,
        rows.iter()
            .map(|(key, vector)| (key.as_str(), vector.as_slice())),
        100,
    );
    // cos 0.2 sits exactly on the floor and is excluded.
    assert_eq!(few.len(), 17);
    assert!(few.last().is_some_and(|key| key == "n15"));
}

fn edge(uuid: &str, source: &str, target: &str, fact: &str, cos: f32) -> Edge {
    Edge {
        uuid: uuid.into(),
        source: source.into(),
        target: target.into(),
        name: "SUPPLIES".into(),
        fact: fact.into(),
        episodes: vec!["spn_old".into()],
        valid_at: None,
        invalid_at: None,
        expired_at: None,
        fact_embedding: Some(at(cos)),
    }
}

#[test]
fn relation_candidates_are_directed_pairs_and_hybrid_search_returns_ten() {
    let mut graph = Graph::default();
    for index in 0..14 {
        graph.edges.push(edge(
            &format!("ab{index:02}"),
            "a",
            "b",
            &format!("Acme ships hull steel lot {index} to Bolt"),
            0.9 - index as f32 * 0.01,
        ));
    }
    graph.edges.push(edge(
        "ba",
        "b",
        "a",
        "Bolt ships hull steel back to Acme",
        0.95,
    ));
    graph
        .edges
        .push(edge("far", "c", "d", "Unrelated coffee note", -0.5));
    let text = MemoryClaimText::new(
        graph
            .edges
            .iter()
            .map(|edge| (edge.uuid.as_str(), edge.name.as_str(), edge.fact.as_str())),
    )
    .unwrap();
    let schema: Schema = merged_schema(&Default::default());
    let mut infer = |_| unreachable!("candidate search makes no model call");
    let mut bridge = Bridge::new(
        BridgeConfig {
            scope: json!({}),
            operation_id: "job".into(),
            profile: "p".into(),
            fast_profile: String::new(),
            timeout_ms: 1_000,
            max_llm_calls: 16,
            max_embedding_calls: 64,
        },
        &mut infer,
    );
    let between: HashSet<String> = graph
        .edges
        .iter()
        .filter(|edge| edge.source == "a" && edge.target == "b")
        .map(|edge| edge.uuid.clone())
        .collect();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        Episode {
            key: "ep".into(),
            content: String::new(),
            valid_at: text::from_isoformat("2026-01-01").unwrap(),
        },
        Vec::new(),
        String::new(),
        graph,
        &text,
    );
    let related = harness.search_edges("Acme hull steel", &at(1.0), Some(&between));
    assert_eq!(related.len(), SEARCH_LIMIT);
    assert!(related
        .iter()
        .all(|edge| edge.source == "a" && edge.target == "b"));
    let everything = harness.search_edges("Acme hull steel", &at(1.0), None);
    assert_eq!(everything.len(), SEARCH_LIMIT);
    assert!(everything.iter().any(|edge| edge.uuid == "ba"));
    assert!(!everything.iter().any(|edge| edge.uuid == "far"));
    assert!(harness
        .search_edges("Acme", &at(1.0), Some(&HashSet::new()))
        .is_empty());
}

fn body(nodes: &[(&str, &str, &str)], claims: &[(&str, &str, &str, &str)]) -> WorldSnapshotBody {
    serde_json::from_value(json!({
        "nodes": nodes.iter().map(|(id, name, description)| json!({
            "id": id, "name": name, "aliases": [], "description": description,
            "kind": "Organization", "sourceRole": "source", "properties": {}
        })).collect::<Vec<_>>(),
        "claims": claims.iter().map(|(id, from, to, proposition)| json!({
            "id": id, "proposition": proposition, "attribution": "p", "relation": "SUPPLIES",
            "endpoints": [{"nodeId": from, "role": "source"}, {"nodeId": to, "role": "target"}],
            "qualifiers": [], "evidence": [{"spanId": "spn_1", "role": "extracted_from", "attribution": "p"}],
            "status": "active", "recordedAtMs": 1, "publicationTime": null, "effectiveTime": null,
            "validAt": null, "invalidAt": null, "expiredAt": null
        })).collect::<Vec<_>>(),
        "annotations": []
    }))
    .unwrap()
}

fn vectors_for(texts: &[String]) -> HashMap<String, Vec<f32>> {
    texts
        .iter()
        .enumerate()
        .map(|(index, text)| (text.clone(), at(1.0 - index as f32 * 0.1)))
        .collect()
}

#[test]
fn the_index_follows_the_sealed_world_and_rebuilds_for_a_new_model() {
    let mut connection = rusqlite::Connection::open_in_memory().unwrap();
    let tx = connection.transaction().unwrap();
    let first = body(
        &[
            ("nod_a", "Acme", "Makes steel."),
            ("nod_b", "Bolt", "Builds ships."),
        ],
        &[("clm_1", "nod_a", "nod_b", "Acme supplies steel to Bolt.")],
    );
    let texts = index::missing_texts(&tx, "l", "w", &first, "m1").unwrap();
    assert_eq!(texts, ["Acme", "Bolt", "Acme supplies steel to Bolt."]);
    index::sync(&tx, "l", "w", "rev_1", &first, "m1", &vectors_for(&texts)).unwrap();
    assert_eq!(
        index::stamp(&tx, "l", "w").unwrap(),
        Some(("rev_1".into(), "m1".into()))
    );
    // Only changed text needs a vector; summaries reach the text index without one.
    let second = body(
        &[
            ("nod_b", "Bolt", "Builds tankers in Ulsan."),
            ("nod_c", "Cobalt Ltd", ""),
        ],
        &[(
            "clm_1",
            "nod_a",
            "nod_b",
            "Acme supplies armor steel to Bolt.",
        )],
    );
    let texts = index::missing_texts(&tx, "l", "w", &second, "m1").unwrap();
    assert_eq!(texts, ["Cobalt Ltd", "Acme supplies armor steel to Bolt."]);
    index::sync(&tx, "l", "w", "rev_2", &second, "m1", &vectors_for(&texts)).unwrap();
    let vectors = index::load_vectors(&tx, "l", "w").unwrap();
    let mut nodes: Vec<&String> = vectors.nodes.keys().collect();
    nodes.sort();
    assert_eq!(nodes, ["nod_b", "nod_c"]);
    let (claims, entities) =
        index::hybrid_hits(&tx, "l", "w", "tankers Ulsan armor", &at(0.0), &vectors, 25);
    assert_eq!(claims, ["clm_1"]);
    assert_eq!(entities, ["nod_b"]);
    // Another Loupist's rows never match.
    let foreign = index::load_vectors(&tx, "other", "w").unwrap();
    assert!(foreign.nodes.is_empty() && foreign.claims.is_empty());
    let (claims, nodes) = index::hybrid_hits(&tx, "other", "w", "armor", &at(1.0), &foreign, 25);
    assert!(claims.is_empty() && nodes.is_empty());
    // A different model invalidates every stored vector.
    let texts = index::missing_texts(&tx, "l", "w", &second, "m2").unwrap();
    assert_eq!(texts.len(), 3);
    index::sync(&tx, "l", "w", "rev_2", &second, "m2", &vectors_for(&texts)).unwrap();
    assert_eq!(index::load_vectors(&tx, "l", "w").unwrap().claims.len(), 1);
    // A vector the caller did not supply fails the whole write.
    let third = body(&[("nod_d", "Delta", "")], &[]);
    assert!(index::sync(&tx, "l", "w", "rev_3", &third, "m2", &HashMap::new()).is_err());
}
