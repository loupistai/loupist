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
    let (_nodes, projected, _annotations) = project_worker_artifact(&artifact.to_string()).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].relation, "RELATES_TO");
    assert_eq!(projected[0].evidence.len(), 32);
    assert!(projected[0]
        .qualifiers
        .iter()
        .any(|qualifier| qualifier.key == "original_relation" && qualifier.value == "공급한다"));
}
