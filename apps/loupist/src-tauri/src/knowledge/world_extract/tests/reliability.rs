//! One bad batch row is dropped, and a name matches the entity list by its keys.

use serde_json::{json, Value};

use crate::knowledge::graph_inference::{InferenceDisposition, InferenceReply};
use crate::knowledge::model::GraphNode;
use crate::knowledge::world_extract::bridge::{Bridge, BridgeConfig};
use crate::knowledge::world_extract::job::stored_node;
use crate::knowledge::world_extract::ontology::merged_schema;
use crate::knowledge::world_extract::pipeline::{
    candidate_search_text, Edge, Episode, Graph, Harness, Node,
};
use crate::knowledge::world_extract::search::MemoryClaimText;
use crate::knowledge::world_extract::text;

fn bridge_config() -> BridgeConfig {
    BridgeConfig {
        scope: json!({}),
        operation_id: "job".into(),
        profile: "p".into(),
        fast_profile: String::new(),
        timeout_ms: 1_000,
        max_llm_calls: 8,
        max_embedding_calls: 8,
    }
}

fn episode() -> Episode {
    Episode {
        key: "ep".into(),
        content: "Acme Corp reported revenue.".into(),
        valid_at: text::from_isoformat("2026-01-01T00:00:00+00:00").unwrap(),
        source: None,
    }
}

fn node(name: &str, kind: &str) -> Node {
    Node {
        uuid: format!("nod-{name}"),
        name: name.into(),
        labels: if kind == "Entity" {
            vec!["Entity".into()]
        } else {
            vec!["Entity".into(), kind.into()]
        },
        summary: String::new(),
        attributes: Vec::new(),
        name_embedding: None,
        spellings: Vec::new(),
    }
}

fn edge(uuid: &str, source: &str, target: &str, fact: &str) -> Edge {
    Edge {
        uuid: uuid.into(),
        source: source.into(),
        target: target.into(),
        name: "SUPPLIES".into(),
        fact: fact.into(),
        episodes: vec!["old".into()],
        valid_at: None,
        invalid_at: None,
        expired_at: None,
        fact_embedding: None,
        assertions: Default::default(),
        ending: None,
        conflicts: Vec::new(),
    }
}

fn reply(value: Value) -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::Completed,
        body: json!({
            "status": "ok",
            "json_object": value,
            "vectors": null,
            "usage": {"certainty": "accounted"}
        }),
    }
}

fn answer(request: &Value) -> InferenceReply {
    if request["kind"] == "embedding" {
        let count = request["inputs"].as_array().map(Vec::len).unwrap_or(0);
        return InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: json!({
                "status": "ok",
                "json_object": null,
                "vectors": vec![vec![0.1_f32; 8]; count]
            }),
        };
    }
    let title = request["response_schema"]["title"].as_str().unwrap_or("");
    reply(match title {
        "EdgeResolutionBatch" => json!({
            "case_0": {"duplicate_facts": [], "contradicted_facts": []},
            "case_1": {"duplicate_facts": "nope", "contradicted_facts": []}
        }),
        "CoreAttributeBatch" => json!({"entities": [
            {"index": 0, "attributes": {"ticker": "ACME"}},
            {"index": 1, "attributes": {"ticker": 4}}
        ]}),
        "SummarizedEntities" => json!({"summaries": []}),
        "ExtractedEntities" => json!({"extracted_entities": [
            {"name": "Acme", "entity_type_id": 1},
            {"name": "Acme Inc.", "entity_type_id": 1},
            {"name": "Beta Motors", "entity_type_id": 1}
        ]}),
        "ExtractedEdges" => json!({"edges": [
            {
                "source_entity_name": "Acme Incorporated",
                "target_entity_name": "Beta Motors",
                "relation_type": "SUPPLIES",
                "fact": "Acme supplies cells to Beta Motors."
            },
            {
                "source_entity_name": "Acme",
                "target_entity_name": "Beta Motors",
                "relation_type": "REPORTS",
                "fact": "Acme reported revenue of USD 1."
            },
            {
                "source_entity_name": "Acme",
                "target_entity_name": "Acme",
                "relation_type": "REPORTS",
                "fact": "Acme reported revenue of USD 2."
            }
        ]}),
        _ => json!({}),
    })
}

#[test]
fn one_bad_batch_row_is_dropped_and_the_rest_are_kept() {
    let schema = merged_schema(&Default::default());
    let mut infer = |request: Value| answer(&request);
    let mut bridge = Bridge::new(bridge_config(), &mut infer);
    let text = MemoryClaimText::new(std::iter::empty()).unwrap();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode(),
        Vec::new(),
        String::new(),
        Graph::default(),
        &text,
    );
    let acme = harness.add_node(node("Acme Corp", "Organization"));
    let beta = harness.add_node(node("Beta Motors", "Organization"));
    let acme_id = harness.node(acme).uuid.clone();
    let beta_id = harness.node(beta).uuid.clone();
    let extracted_left = harness.add_edge(edge(
        "new-left",
        &acme_id,
        &beta_id,
        "Acme Corp supplies cells to Beta Motors.",
    ));
    let extracted_right = harness.add_edge(edge(
        "new-right",
        &beta_id,
        &acme_id,
        "Beta Motors supplies cars to Acme Corp.",
    ));
    let candidate_left = harness.add_edge(edge(
        "old-left",
        &acme_id,
        &beta_id,
        "Acme Corp once discussed cells with Beta Motors.",
    ));
    let candidate_right = harness.add_edge(edge(
        "old-right",
        &beta_id,
        &acme_id,
        "Beta Motors once discussed cars with Acme Corp.",
    ));
    harness
        .decide_edges(
            &[extracted_left, extracted_right],
            vec![vec![candidate_left], vec![candidate_right]],
            vec![Vec::new(), Vec::new()],
        )
        .unwrap();
    harness
        .attributes_and_summaries(&[acme, beta], &[])
        .unwrap();
    assert_eq!(harness.dropped().edges, 1);
    assert_eq!(harness.dropped().attributes, 1);
    assert_eq!(ticker(harness.node(acme)), Some("ACME"));
    assert_eq!(ticker(harness.node(beta)), None);
}

fn ticker(node: &Node) -> Option<&str> {
    node.attributes
        .iter()
        .find(|(name, _)| name == "ticker")
        .and_then(|(_, value)| value.as_str())
}

#[test]
fn reports_needs_one_subject_and_names_match_by_key() {
    let schema = merged_schema(&Default::default());
    let mut infer = |request: Value| answer(&request);
    let mut bridge = Bridge::new(bridge_config(), &mut infer);
    let text = MemoryClaimText::new(std::iter::empty()).unwrap();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode(),
        Vec::new(),
        String::new(),
        Graph::default(),
        &text,
    );
    let extracted = harness.extract_nodes().unwrap();
    assert_eq!(extracted.len(), 2, "Acme and Acme Inc. share a name key");
    let edges = harness.extract_edges(&extracted).unwrap();
    let facts: Vec<&str> = edges
        .iter()
        .map(|index| harness.edge(*index).fact.as_str())
        .collect();
    assert!(facts.contains(&"Acme supplies cells to Beta Motors."));
    assert!(facts.contains(&"Acme reported revenue of USD 2."));
    assert!(!facts.iter().any(|fact| fact.contains("USD 1")));
    let reports = edges
        .iter()
        .map(|index| harness.edge(*index))
        .find(|edge| edge.name == "REPORTS")
        .unwrap();
    assert_eq!(reports.source, reports.target);
}

#[test]
fn stored_nodes_keep_spellings_and_codes_in_the_candidate_text() {
    let schema = merged_schema(&Default::default());
    let stored = GraphNode {
        id: "nod-samsung".into(),
        name: "Samsung Electronics".into(),
        aliases: vec!["삼성전자".into()],
        description: String::new(),
        kind: "Organization".into(),
        source_role: "source".into(),
        properties: [("ticker".into(), json!("005930"))].into(),
    };
    let node = stored_node(&stored, &schema, None);
    assert_eq!(node.spellings, ["삼성전자"]);
    assert!(node
        .attributes
        .iter()
        .any(|(key, value)| key == "ticker" && value == "005930"));
    let codes: Vec<String> = node.codes().values().map(str::to_string).collect();
    let text = candidate_search_text(&node.name, &node.spellings, &codes);
    assert!(text.contains("Samsung Electronics"), "{text}");
    assert!(text.contains("삼성전자"), "{text}");
    assert!(
        codes
            .iter()
            .any(|code| text.split_whitespace().any(|part| part == code)),
        "{text} codes={codes:?}"
    );
}
