//! Worker output: bounds the World snapshot an extraction worker wrote before
//! the host seals it. Relations become ASCII tokens, and evidence, qualifiers,
//! names and aliases are capped. A claim the adapter cannot read is dropped,
//! so one bad claim never blocks the World.

use serde_json::{json, Value};

use super::clip;
use crate::knowledge::error::{KnowledgeError, KnowledgeResult};
use crate::knowledge::model::{GraphClaim, GraphNode, TopicAnnotation};
use crate::knowledge::worker::{adapt_worker_claim, adapt_worker_node};

type WorkerProjection = (Vec<GraphNode>, Vec<GraphClaim>, Vec<TopicAnnotation>);

const EVIDENCE_CAP: usize = 32;
const QUALIFIER_CAP: usize = 16;

/// ASCII SCREAMING_SNAKE within 64 characters. A name with no ASCII letters
/// or digits becomes `RELATES_TO`, and the original label is returned so the
/// seal can keep it as a qualifier instead of rejecting the World.
fn normalize_relation(name: &str) -> (String, Option<String>) {
    let raw = name.trim();
    if raw.is_empty() {
        return ("RELATES_TO".to_string(), None);
    }
    let mut token = String::new();
    let mut prev_lower_or_digit = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_lower_or_digit && !token.ends_with('_') {
                token.push('_');
            }
            token.push(ch.to_ascii_uppercase());
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if !token.is_empty() && !token.ends_with('_') {
            token.push('_');
            prev_lower_or_digit = false;
        }
    }
    while token.ends_with('_') {
        token.pop();
    }
    if token.is_empty() {
        return ("RELATES_TO".to_string(), Some(clip(raw, 512)));
    }
    if token.len() > 64 {
        let mut clipped = clip(&token, 64);
        while clipped.ends_with('_') {
            clipped.pop();
        }
        if clipped.is_empty() {
            return ("RELATES_TO".to_string(), Some(clip(raw, 512)));
        }
        return (clipped, Some(clip(raw, 512)));
    }
    (token, None)
}

fn cap_evidence_array(evidence: &mut Vec<Value>) {
    if evidence.len() <= EVIDENCE_CAP {
        return;
    }
    let earliest = evidence[0].clone();
    let tail_start = evidence.len() - (EVIDENCE_CAP - 1);
    let mut capped = Vec::with_capacity(EVIDENCE_CAP);
    capped.push(earliest);
    capped.extend(evidence[tail_start..].iter().cloned());
    *evidence = capped;
}

fn sanitize_worker_claim(claim: &mut Value) {
    let Some(object) = claim.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(evidence)) = object.get_mut("evidence") {
        cap_evidence_array(evidence);
    }
    let relation = object
        .get("relation")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (token, original) = normalize_relation(&relation);
    object.insert("relation".to_string(), Value::String(token));
    clip_qualifier(object, "original_relation", 512);
    let Some(original) = original else {
        return;
    };
    let qualifiers = object
        .entry("qualifiers".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(items) = qualifiers.as_array_mut() else {
        return;
    };
    if let Some(existing) = items
        .iter_mut()
        .find(|item| item.get("key").and_then(Value::as_str) == Some("original_relation"))
    {
        if let Some(map) = existing.as_object_mut() {
            map.insert("value".to_string(), Value::String(original));
        }
        return;
    }
    if items.len() < QUALIFIER_CAP {
        items.push(json!({
            "key": "original_relation",
            "value": original,
        }));
    }
}

fn clip_qualifier(object: &mut serde_json::Map<String, Value>, key: &str, max: usize) {
    let Some(items) = object.get_mut("qualifiers").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        if item.get("key").and_then(Value::as_str) != Some(key) {
            continue;
        }
        let Some(value) = item.get("value").and_then(Value::as_str) else {
            continue;
        };
        let clipped = clip(value, max);
        if let Some(map) = item.as_object_mut() {
            map.insert("value".to_string(), Value::String(clipped));
        }
    }
}

fn bound_aliases(aliases: &mut Vec<String>) {
    let mut kept = Vec::new();
    for alias in aliases.iter() {
        let alias = clip(alias.trim(), 200);
        if alias.is_empty() || kept.iter().any(|item: &String| item == &alias) {
            continue;
        }
        kept.push(alias);
        if kept.len() == 16 {
            break;
        }
    }
    kept.sort();
    *aliases = kept;
}

/// The worker's snapshot as bounded nodes, claims and topic annotations.
pub(super) fn project_worker_artifact(artifact_utf8: &str) -> KnowledgeResult<WorkerProjection> {
    let snapshot: Value = serde_json::from_str(artifact_utf8)?;
    let mut nodes = Vec::new();
    for node in snapshot
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(KnowledgeError::Invalid("snapshot nodes are invalid"))?
    {
        let mut adapted = adapt_worker_node(node)?;
        adapted.name = clip(&adapted.name, 200);
        adapted.description = clip(&adapted.description, 4_000);
        bound_aliases(&mut adapted.aliases);
        nodes.push(adapted);
    }
    let mut claims = Vec::new();
    for claim in snapshot
        .get("claims")
        .and_then(Value::as_array)
        .ok_or(KnowledgeError::Invalid("snapshot claims are invalid"))?
    {
        let mut claim = claim.clone();
        sanitize_worker_claim(&mut claim);
        match adapt_worker_claim(&claim) {
            Ok(adapted) => claims.push(adapted),
            // One unreadable claim must not stop every later export of this World.
            Err(KnowledgeError::Invalid(_)) => {}
            Err(error) => return Err(error),
        }
    }
    let mut annotations = Vec::new();
    if let Some(items) = snapshot.get("annotations").and_then(Value::as_array) {
        for item in items {
            let topic_id = item
                .get("topic_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let markdown = item
                .get("markdown")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let source_span_ids = item
                .get("source_span_ids")
                .or_else(|| item.get("sourceSpanIds"))
                .and_then(Value::as_array)
                .map(|spans| {
                    spans
                        .iter()
                        .filter_map(|span| span.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if topic_id.is_empty() {
                continue;
            }
            annotations.push(TopicAnnotation {
                topic_id,
                markdown,
                source_span_ids,
            });
        }
    }
    Ok((nodes, claims, annotations))
}

#[cfg(test)]
mod tests;
