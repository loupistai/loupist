use serde_json::{json, Value};

use super::project_worker_artifact;
use crate::knowledge::graph_jobs::tests::world_artifact;

#[test]
fn projection_normalizes_relation_and_caps_evidence() {
    let evidence: Vec<Value> = (0..40)
        .map(|index| {
            json!({
                "span_id": format!("spn_{index}"),
                "role": "extracted_from",
                "attribution": "example-publisher"
            })
        })
        .collect();
    let mut artifact = serde_json::from_str::<Value>(&world_artifact(&["spn_1"])).unwrap();
    let claims = artifact["claims"].as_array_mut().unwrap();
    claims[0]["relation"] = json!("공급한다");
    claims[0]["evidence"] = Value::Array(evidence);
    claims.push(json!({"adapter_id": "not-a-claim"}));
    let (_nodes, projected, _annotations) =
        project_worker_artifact(&artifact.to_string(), &[]).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].relation, "RELATES_TO");
    assert_eq!(projected[0].evidence.len(), 32);
    assert!(projected[0]
        .qualifiers
        .iter()
        .any(|qualifier| qualifier.key == "original_relation" && qualifier.value == "공급한다"));
}

#[test]
fn admission_checks_all_new_names_even_with_custom_kinds_but_preserves_existing_nodes() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../../../evals/world/entity-name-corpus-v1.json"
    ))
    .unwrap();
    let mut artifact: Value = serde_json::from_str(&world_artifact(&["spn_1"])).unwrap();
    let template = artifact["nodes"][0].clone();
    artifact["nodes"] = json!(corpus
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut node = template.clone();
            node["adapter_id"] = json!(format!("nod_case{i}"));
            node["name"] = row["name"].clone();
            node["kind"] = row["kind"].clone();
            node
        })
        .collect::<Vec<_>>());
    let (nodes, claims, _) = project_worker_artifact(&artifact.to_string(), &[]).unwrap();
    assert!(
        claims.is_empty(),
        "claims with dropped endpoints cannot survive"
    );
    for (i, row) in corpus.as_array().unwrap().iter().enumerate() {
        assert_eq!(
            nodes.iter().any(|n| n.id == format!("nod_case{i}")),
            row["entity"].as_bool().unwrap(),
            "{row}"
        );
    }
    let old = crate::knowledge::worker::adapt_worker_node(&artifact["nodes"][0]).unwrap();
    let (preserved, _, _) =
        project_worker_artifact(&artifact.to_string(), std::slice::from_ref(&old)).unwrap();
    assert!(
        preserved.contains(&old),
        "a pre-existing measurement node remains readable"
    );
}

#[test]
fn admission_retains_subject_claim_and_rejects_measurement_before_name_clipping() {
    let mut artifact: Value = serde_json::from_str(&world_artifact(&["spn_1"])).unwrap();
    let subject = artifact["nodes"][0]["adapter_id"].clone();
    artifact["nodes"][1]["name"] = json!(format!("{} USD 42 million", "Long name ".repeat(30)));
    artifact["claims"][0]["endpoints"][1]["node_id"] = subject;
    artifact["claims"][0]["relation"] = json!("REPORTS");
    artifact["claims"][0]["proposition"] =
        json!("Cedar reported Q4 FY2026 GAAP net income USD 42,960 million.");
    let (nodes, claims, _) = project_worker_artifact(&artifact.to_string(), &[]).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(claims.len(), 1);
    assert_eq!(
        claims[0].endpoints[0].node_id,
        claims[0].endpoints[1].node_id
    );
    assert_eq!(claims[0].relation, "REPORTS");
    assert!(claims[0].proposition.contains("USD 42,960 million"));
}

#[test]
fn a_report_to_a_newly_rejected_value_node_stays_on_the_subject() {
    let mut artifact: Value = serde_json::from_str(&world_artifact(&["spn_1"])).unwrap();
    artifact["nodes"][1]["kind"] = json!("Metric");
    artifact["nodes"][1]["name"] = json!("Cedar net income USD 42,960 million");
    artifact["claims"][0]["relation"] = json!("REPORTS");
    artifact["claims"][0]["proposition"] =
        json!("Cedar reported GAAP net income USD 42,960 million for Q4 FY2026.");
    let (nodes, claims, _) = project_worker_artifact(&artifact.to_string(), &[]).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(claims.len(), 1);
    assert_eq!(
        claims[0].endpoints[0].node_id,
        claims[0].endpoints[1].node_id
    );
    assert!(claims[0].proposition.contains("USD 42,960 million"));
    // Legacy nodes and their claims are not converted on a later publication.
    let existing = crate::knowledge::worker::adapt_worker_node(&artifact["nodes"][1]).unwrap();
    let (nodes, claims, _) = project_worker_artifact(&artifact.to_string(), &[existing]).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_ne!(
        claims[0].endpoints[0].node_id,
        claims[0].endpoints[1].node_id
    );
}
