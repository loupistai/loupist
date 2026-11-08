//! `project_extraction`: map an extraction onto snapshot nodes and claims.
//!
//! Identity follows the runtime id: an adapter id is derived from a new one on
//! first sight, and a stored node or claim keeps its id. A later summary or
//! invalidation updates the prior object. An invalidated edge keeps its earlier
//! source spans and can retain endpoints absent from the current extraction;
//! the current episode is not stamped onto it. Existing qualifiers and curator
//! status survive. Temporal invalidation is not a withdrawn verdict.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use super::pipeline::{Edge, Node, Time};
use crate::knowledge::lens::fold_lens_title;
use crate::util::sha256;

const EVIDENCE_CAP: usize = 32;
const QUALIFIER_CAP: usize = 16;
const ALIAS_CAP: usize = 16;
const ORIGINAL_RELATION: &str = "original_relation";
/// Neo4j's alias cache key; never a canonical property.
const DERIVED_ALIAS_PROPERTY: &str = "_loupist_search_aliases_v1";

pub(crate) fn adapter_id(prefix: &str, runtime: &str) -> String {
    format!("{prefix}{}", &sha256(runtime.as_bytes())[..32])
}

fn utf8_prefix(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// 16 aliases, each at most 200 UTF-8 bytes, sorted.
pub(crate) fn cap_aliases(aliases: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for item in aliases {
        let text = utf8_prefix(item.trim(), 200);
        if !text.is_empty() && !kept.contains(&text) {
            kept.push(text);
        }
        if kept.len() == ALIAS_CAP {
            break;
        }
    }
    kept.sort();
    kept
}

/// ASCII SCREAMING_SNAKE within 64 characters, and the original label when it changed.
pub(crate) fn normalize_relation(name: &str) -> (String, Option<String>) {
    let raw = name.trim();
    if raw.is_empty() {
        return ("RELATES_TO".into(), None);
    }
    // re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", raw): non-overlapping, left to right.
    let chars: Vec<char> = raw.chars().collect();
    let mut spaced = String::with_capacity(raw.len() + 8);
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if (ch.is_ascii_lowercase() || ch.is_ascii_digit())
            && chars.get(index + 1).is_some_and(char::is_ascii_uppercase)
        {
            spaced.push(ch);
            spaced.push('_');
            spaced.push(chars[index + 1]);
            index += 2;
            continue;
        }
        spaced.push(ch);
        index += 1;
    }
    let token = spaced
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|piece| !piece.is_empty())
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>()
        .join("_");
    if !token.is_empty() && token.len() <= 64 {
        return (token, None);
    }
    let clipped = token[..token.len().min(64)]
        .trim_end_matches('_')
        .to_string();
    let clipped = if clipped.is_empty() {
        "RELATES_TO".to_string()
    } else {
        clipped
    };
    (clipped, Some(utf8_prefix(raw, 512)))
}

fn cap_evidence(evidence: Vec<Value>) -> Vec<Value> {
    if evidence.len() <= EVIDENCE_CAP {
        return evidence;
    }
    let mut capped = vec![evidence[0].clone()];
    capped.extend_from_slice(&evidence[evidence.len() - (EVIDENCE_CAP - 1)..]);
    capped
}

fn with_original_relation(mut qualifiers: Vec<Value>, original: Option<String>) -> Vec<Value> {
    let Some(original) = original else {
        return qualifiers;
    };
    if let Some(item) = qualifiers
        .iter_mut()
        .find(|item| item["key"] == ORIGINAL_RELATION)
    {
        item["value"] = json!(original);
        return qualifiers;
    }
    if qualifiers.len() < QUALIFIER_CAP {
        qualifiers.push(json!({"key": ORIGINAL_RELATION, "value": original}));
    }
    qualifiers
}

fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// Overlay stated attributes on stored properties; blanks never stay.
pub(crate) fn merged_node_properties(
    attributes: &[(String, Value)],
    prior: Option<&Value>,
) -> Map<String, Value> {
    let mut merged: Vec<(String, Value)> = prior
        .and_then(|prior| prior.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    for (key, value) in attributes {
        if key == DERIVED_ALIAS_PROPERTY || blank(value) {
            continue;
        }
        match merged.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = value.clone(),
            None => merged.push((key.clone(), value.clone())),
        }
    }
    merged
        .into_iter()
        .filter(|(_, value)| !blank(value))
        .collect()
}

/// The worker's canonical instant: whole seconds, the value's own offset.
pub(crate) fn graph_time(value: Option<&Time>) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    let offset = value.offset().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.abs() / 60;
    json!({
        "precision": "instant",
        "value": format!(
            "{}{sign}{:02}:{:02}",
            value.format("%Y-%m-%dT%H:%M:%S"),
            minutes / 60,
            minutes % 60
        ),
        "approximate": false,
    })
}

pub(crate) struct Binding<'a> {
    pub span_id: &'a str,
    pub binding_role: &'a str,
    pub attribution: &'a str,
}

pub(crate) struct Context<'a> {
    pub source_role: &'a str,
    pub binding: Binding<'a>,
    pub publication_time: &'a Value,
    pub effective_time: Option<&'a Value>,
    pub recorded_at_ms: i64,
    /// Stored nodes and claims by runtime id (worker form).
    pub prior_nodes: &'a HashMap<String, Value>,
    pub prior_claims: &'a HashMap<String, Value>,
    pub episode_runtime: &'a str,
}

#[derive(Debug, Default)]
pub(crate) struct Projection {
    pub nodes: Vec<Value>,
    pub claims: Vec<Value>,
}

pub(crate) fn project_extraction(
    context: &Context<'_>,
    nodes: &[Node],
    edges: &[Edge],
) -> Projection {
    let mut projected_nodes: Vec<Value> = Vec::new();
    let mut node_runtime: HashMap<String, String> = HashMap::new();
    for node in nodes {
        if node.name.is_empty() || node.uuid.is_empty() {
            continue;
        }
        let prior = context.prior_nodes.get(&node.uuid);
        let field = |key: &str| prior.and_then(|prior| prior.get(key)).cloned();
        let kind = node
            .kind()
            .map(str::to_string)
            .or_else(|| {
                field("kind")
                    .and_then(|kind| kind.as_str().map(str::to_string))
                    .filter(|kind| !kind.is_empty())
            })
            .unwrap_or_else(|| "Entity".into());
        let properties = merged_node_properties(&node.attributes, prior);
        let adapter = field("adapter_id")
            .and_then(|id| id.as_str().map(str::to_string))
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| adapter_id("nod_", &node.uuid));
        node_runtime.insert(node.uuid.clone(), adapter.clone());
        let mut aliases: Vec<String> = field("aliases")
            .and_then(|aliases| {
                aliases.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
            })
            .unwrap_or_default();
        let original = properties
            .get("original_name")
            .and_then(Value::as_str)
            .map(str::to_string);
        let prior_name = field("name").and_then(|name| name.as_str().map(str::to_string));
        for alias in [prior_name, original].into_iter().flatten() {
            if !alias.is_empty() && alias != node.name && !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
        // A spelling the resolver confirmed as this node stays an alias, within
        // the cap, unless a name or alias already folds to it.
        let mut folds: HashSet<String> = std::iter::once(&node.name)
            .chain(&aliases)
            .map(|text| fold_lens_title(text))
            .collect();
        for spelling in &node.spellings {
            if aliases.len() >= ALIAS_CAP {
                break;
            }
            if folds.insert(fold_lens_title(spelling)) {
                aliases.push(spelling.clone());
            }
        }
        let source_role = field("source_role")
            .and_then(|role| role.as_str().map(str::to_string))
            .filter(|role| !role.is_empty())
            .unwrap_or_else(|| context.source_role.to_string());
        projected_nodes.push(json!({
            "adapter_id": adapter,
            "topic_id": field("topic_id").unwrap_or(Value::Null),
            "name": node.name,
            "aliases": cap_aliases(&aliases),
            "description": node.summary.chars().take(8000).collect::<String>(),
            "kind": kind.chars().take(128).collect::<String>(),
            "source_role": source_role,
            "properties": Value::Object(properties),
        }));
    }
    let mut endpoint_adapters: HashMap<String, String> = context
        .prior_nodes
        .iter()
        .filter_map(|(runtime, node)| {
            node.get("adapter_id")
                .and_then(Value::as_str)
                .map(|id| (runtime.clone(), id.to_string()))
        })
        .collect();
    endpoint_adapters.extend(node_runtime.clone());
    let mut projected_ids: std::collections::HashSet<String> = projected_nodes
        .iter()
        .filter_map(|node| node["adapter_id"].as_str().map(str::to_string))
        .collect();
    let mut projected_claims = Vec::new();
    for edge in edges {
        let (Some(source), Some(target)) = (
            endpoint_adapters.get(&edge.source).cloned(),
            endpoint_adapters.get(&edge.target).cloned(),
        ) else {
            continue;
        };
        if edge.uuid.is_empty() {
            continue;
        }
        let prior_claim = context.prior_claims.get(&edge.uuid);
        let owned_by_current = edge.episodes.is_empty()
            || (!context.episode_runtime.is_empty()
                && edge
                    .episodes
                    .iter()
                    .any(|item| item == context.episode_runtime));
        let valid_at = graph_time(edge.valid_at.as_ref());
        let invalid_at = graph_time(edge.invalid_at.as_ref());
        let expired_at = graph_time(edge.expired_at.as_ref());
        let mut support = json!({
            "span_id": context.binding.span_id,
            "role": context.binding.binding_role,
            "attribution": context.binding.attribution,
        });
        let mut claim = if let Some(prior) = prior_claim {
            let mut evidence: Vec<Value> =
                prior["evidence"].as_array().cloned().unwrap_or_default();
            if support["role"] == "extracted_from"
                && evidence
                    .iter()
                    .any(|item| item["span_id"] != support["span_id"])
            {
                support["role"] = json!("supports");
            }
            if owned_by_current
                && invalid_at.is_null()
                && expired_at.is_null()
                && !evidence.iter().any(|item| {
                    item["span_id"] == support["span_id"] && item["role"] == support["role"]
                })
            {
                evidence.push(support.clone());
            }
            json!({
                "adapter_id": prior["adapter_id"],
                "attribution": prior["attribution"],
                "evidence": cap_evidence(evidence),
                "status": prior.get("status").cloned().unwrap_or_else(|| json!("active")),
                "recorded_at_ms": prior["recorded_at_ms"],
                "publication_time": prior["publication_time"],
                "effective_time": prior.get("effective_time").cloned().unwrap_or(Value::Null),
            })
        } else if owned_by_current {
            json!({
                "adapter_id": adapter_id("clm_", &edge.uuid),
                "attribution": context.binding.attribution,
                "evidence": [support],
                "status": "active",
                "recorded_at_ms": context.recorded_at_ms,
                "publication_time": context.publication_time,
                "effective_time": context.effective_time.cloned().unwrap_or(Value::Null),
            })
        } else {
            continue;
        };
        // Invalidated old edges come back beside current nodes; keep their known endpoints.
        for (runtime, endpoint) in [(&edge.source, &source), (&edge.target, &target)] {
            if !projected_ids.contains(endpoint) {
                if let Some(prior) = context.prior_nodes.get(runtime) {
                    projected_nodes.push(prior.clone());
                }
                projected_ids.insert(endpoint.clone());
            }
        }
        let (relation, original) = normalize_relation(&edge.name);
        let mut qualifiers = with_original_relation(
            prior_claim
                .and_then(|prior| prior.get("qualifiers"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            original,
        );
        if let Some(prior) = prior_claim {
            if !invalid_at.is_null()
                && prior.get("invalid_at").is_none_or(blank)
                && !qualifiers
                    .iter()
                    .any(|item| item["key"] == "superseded_by_span")
                && qualifiers.len() < QUALIFIER_CAP
            {
                qualifiers
                    .push(json!({"key": "superseded_by_span", "value": context.binding.span_id}));
            }
        }
        let mut endpoints = json!([
            {"node_id": source, "role": "source"},
            {"node_id": target, "role": "target"},
        ]);
        if let Some(prior) = prior_claim {
            let ids: Vec<&str> = prior["endpoints"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item["node_id"].as_str())
                        .collect()
                })
                .unwrap_or_default();
            if ids == [source.as_str(), target.as_str()] {
                endpoints = prior["endpoints"].clone();
            }
        }
        let object = claim.as_object_mut().expect("claim object");
        object.insert(
            "proposition".into(),
            json!(edge.fact.chars().take(8000).collect::<String>()),
        );
        object.insert("relation".into(), json!(relation));
        object.insert("endpoints".into(), endpoints);
        object.insert("qualifiers".into(), Value::Array(qualifiers));
        object.insert("valid_at".into(), valid_at);
        object.insert("invalid_at".into(), invalid_at);
        object.insert("expired_at".into(), expired_at);
        projected_claims.push(claim);
    }
    Projection {
        nodes: projected_nodes,
        claims: projected_claims,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_normalize_like_the_worker() {
        assert_eq!(normalize_relation("supplies"), ("SUPPLIES".into(), None));
        assert_eq!(
            normalize_relation("signedContractWith"),
            ("SIGNED_CONTRACT_WITH".into(), None)
        );
        assert_eq!(normalize_relation("a1B2C"), ("A1_B2_C".into(), None));
        assert_eq!(
            normalize_relation("공급"),
            ("RELATES_TO".into(), Some("공급".into()))
        );
        assert_eq!(normalize_relation(" "), ("RELATES_TO".into(), None));
        assert_eq!(
            cap_aliases(&[" b ".into(), "a".into(), "b".into(), String::new()]),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
