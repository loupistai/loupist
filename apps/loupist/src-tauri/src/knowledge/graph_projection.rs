//! Convert canonical World values into the in-process extraction/search representation.
use super::canonical::canonical_string;
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{GraphClaim, GraphNode, WorldSnapshotBody};
use serde_json::{json, Value};
pub(super) fn worker_annotations(body: &WorldSnapshotBody) -> Vec<Value> {
    let mut items: Vec<Value> = body
        .annotations
        .iter()
        .map(|annotation| {
            json!({
                "markdown": annotation.markdown,
                "source_span_ids": annotation.source_span_ids,
                "topic_id": annotation.topic_id,
            })
        })
        .collect();
    items.sort_by_key(canonical_string);
    items
}

pub(super) fn worker_node(node: &GraphNode) -> KnowledgeResult<Value> {
    if !matches!(node.source_role.as_str(), "source" | "analysis") {
        return Err(KnowledgeError::Invalid(
            "node source role cannot be indexed without reinterpretation",
        ));
    }
    let mut aliases = node.aliases.clone();
    aliases.sort();
    Ok(json!({
        "adapter_id": node.id,
        "aliases": aliases,
        "description": node.description,
        "kind": node.kind,
        "name": node.name,
        "properties": node.properties,
        "source_role": node.source_role,
        "topic_id": Value::Null,
    }))
}

pub(super) fn worker_claim(claim: &GraphClaim) -> KnowledgeResult<Value> {
    if claim.evidence.is_empty() {
        return Err(KnowledgeError::Invalid(
            "claim evidence is required to index a sealed world",
        ));
    }
    if claim.endpoints.len() != 2
        || !matches!(
            (
                claim.endpoints[0].role.as_str(),
                claim.endpoints[1].role.as_str()
            ),
            ("source", "target") | ("subject", "object")
        )
    {
        return Err(KnowledgeError::Invalid(
            "claim endpoints cannot be indexed without reinterpretation",
        ));
    }
    if claim.recorded_at_ms.is_some_and(|value| value < 0) {
        return Err(KnowledgeError::Invalid("claim recorded time is invalid"));
    }
    let mut qualifiers = claim.qualifiers.clone();
    qualifiers.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(json!({
        "adapter_id": claim.id,
        "attribution": claim.attribution,
        "effective_time": wire_time(claim.effective_time.as_ref())?,
        "endpoints": claim.endpoints.iter().map(|endpoint| json!({
            "node_id": endpoint.node_id,
            "role": endpoint.role,
        })).collect::<Vec<_>>(),
        "evidence": claim.evidence.iter().map(|evidence| json!({
            "attribution": evidence.attribution,
            "role": evidence.role.as_str(),
            "span_id": evidence.span_id,
        })).collect::<Vec<_>>(),
        "expired_at": wire_time(claim.expired_at.as_ref())?,
        "invalid_at": wire_time(claim.invalid_at.as_ref())?,
        "proposition": claim.proposition,
        "publication_time": required_publication(claim.publication_time.as_ref())?,
        "qualifiers": qualifiers.iter().map(|qualifier| json!({
            "key": qualifier.key,
            "value": qualifier.value,
        })).collect::<Vec<_>>(),
        "recorded_at_ms": claim.recorded_at_ms,
        "relation": claim.relation,
        "status": claim.status.as_str(),
        "valid_at": wire_time(claim.valid_at.as_ref())?,
    }))
}

fn wire_time(time: Option<&super::model::TimePoint>) -> KnowledgeResult<Value> {
    match time {
        None => Ok(Value::Null),
        Some(time) => {
            // Source metadata retains its original representation. Normalize
            // the extraction representation before hashing.
            let time = super::time::normalize_time_point(time)?;
            Ok(json!({
                "approximate": time.approximate,
                "precision": time.precision.as_str(),
                "value": time.value,
            }))
        }
    }
}

fn required_publication(time: Option<&super::model::TimePoint>) -> KnowledgeResult<Value> {
    match time {
        None => Ok(json!({"approximate": false, "precision": "unknown", "value": Value::Null})),
        Some(time) => wire_time(Some(time)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{TimePoint, TimePrecision};

    #[test]
    fn directed_claim_roles_survive_projection_without_reinterpretation() {
        let mut claim: GraphClaim = serde_json::from_value(json!({
            "id": "clm_a", "proposition": "A supplies B", "attribution": "publisher",
            "relation": "SUPPLIES", "endpoints": [
                {"nodeId": "nod_a", "role": "subject"},
                {"nodeId": "nod_b", "role": "object"}
            ], "qualifiers": [], "evidence": [
                {"spanId": "spn_a", "role": "extracted_from", "attribution": "publisher"}
            ], "status": "active", "recordedAtMs": 1,
            "publicationTime": null, "effectiveTime": null,
            "validAt": null, "invalidAt": null, "expiredAt": null
        }))
        .unwrap();
        for (first, second) in [("subject", "object"), ("source", "target")] {
            claim.endpoints[0].role = first.into();
            claim.endpoints[1].role = second.into();
            let projected = worker_claim(&claim).unwrap();
            assert_eq!(
                projected["endpoints"],
                json!([
                    {"node_id": "nod_a", "role": first},
                    {"node_id": "nod_b", "role": second}
                ])
            );
        }
        claim.recorded_at_ms = None;
        assert_eq!(worker_claim(&claim).unwrap()["recorded_at_ms"], Value::Null);
        claim.recorded_at_ms = Some(-1);
        assert!(worker_claim(&claim).is_err());
        claim.recorded_at_ms = Some(1);
        for (first, second) in [
            ("object", "subject"),
            ("target", "source"),
            ("source", "object"),
            ("subject", "target"),
        ] {
            claim.endpoints[0].role = first.into();
            claim.endpoints[1].role = second.into();
            assert!(worker_claim(&claim).is_err());
        }
    }

    #[test]
    fn projected_times_preserve_precision_and_reject_invalid_metadata() {
        for (precision, input, expected, approximate) in [
            (
                TimePrecision::Instant,
                "2026-08-05T18:00:00.123+09:00",
                "2026-08-05T18:00:00+09:00",
                false,
            ),
            (TimePrecision::Date, "2026-08-05", "2026-08-05", false),
            (TimePrecision::Month, "2026-08", "2026-08", true),
            (TimePrecision::Year, "2026", "2026", true),
        ] {
            let time = TimePoint {
                precision,
                value: Some(input.into()),
                approximate,
            };
            let projected = wire_time(Some(&time)).unwrap();
            assert_eq!(projected["value"], expected);
            assert_eq!(projected["precision"], precision.as_str());
            assert_eq!(projected["approximate"], approximate);
        }
        assert_eq!(wire_time(None).unwrap(), Value::Null);
        assert_eq!(
            required_publication(None).unwrap(),
            json!({"approximate": false, "precision": "unknown", "value": Value::Null})
        );
        // The worker normalizes `Z` to `+00:00`; the host digest must already match it.
        let utc = TimePoint {
            precision: TimePrecision::Instant,
            value: Some("2026-08-05T09:00:00Z".into()),
            approximate: false,
        };
        assert_eq!(
            required_publication(Some(&utc)).unwrap()["value"],
            "2026-08-05T09:00:00+00:00"
        );
        assert!(wire_time(Some(&TimePoint {
            precision: TimePrecision::Instant,
            value: Some("2026-08-05T09:00:00".into()),
            approximate: false,
        }))
        .is_err());
    }
}
