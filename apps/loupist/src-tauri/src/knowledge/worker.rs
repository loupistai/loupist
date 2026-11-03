use serde_json::{Map, Value};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    validate_text, validate_time, validate_time_point, validate_token, BindingRole, ClaimEndpoint,
    ClaimStatus, EvidenceBinding, GraphClaim, GraphNode, Qualifier, TimePoint, TimePrecision,
};

/// Map one private Graphiti snapshot claim onto the host claim.
///
/// `adapter_id` becomes the host id. Embeddings stay out of the public claim.
/// `invalid_at` is fact-validity end and `expired_at` is Graphiti system time.
pub fn adapt_worker_claim(value: &Value) -> KnowledgeResult<GraphClaim> {
    let raw = object(value, "worker claim")?;
    let id = text_field(raw, "adapter_id", 128, false)?;
    validate_token(&id, 128, "claim id is invalid")?;
    let endpoints = endpoints(raw.get("endpoints").unwrap_or(&Value::Null))?;
    let evidence = evidence_list(raw.get("evidence").unwrap_or(&Value::Null))?;
    let mut qualifiers = qualifiers(raw.get("qualifiers").unwrap_or(&Value::Null))?;
    qualifiers.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(GraphClaim {
        id,
        proposition: text_field(raw, "proposition", 8_000, false)?,
        attribution: text_field(raw, "attribution", 500, false)?,
        relation: {
            let relation = text_field(raw, "relation", 200, false)?;
            validate_token(&relation, 64, "relation is invalid")?;
            relation
        },
        endpoints,
        qualifiers,
        evidence,
        status: ClaimStatus::parse(&text_field(raw, "status", 32, false)?)?,
        recorded_at_ms: recorded_at(raw.get("recorded_at_ms"))?,
        publication_time: time_field(raw, "publication_time")?,
        effective_time: time_field(raw, "effective_time")?,
        valid_at: time_field(raw, "valid_at")?,
        invalid_at: time_field(raw, "invalid_at")?,
        expired_at: time_field(raw, "expired_at")?,
    })
}

pub fn adapt_worker_node(value: &Value) -> KnowledgeResult<GraphNode> {
    let raw = object(value, "worker node")?;
    let id = text_field(raw, "adapter_id", 128, false)?;
    validate_token(&id, 128, "node id is invalid")?;
    let aliases = string_list(raw.get("aliases").unwrap_or(&Value::Array(Vec::new())), 32)?;
    let properties = match raw.get("properties") {
        None | Some(Value::Null) => Default::default(),
        Some(Value::Object(entries)) => {
            let mut mapped = std::collections::BTreeMap::new();
            for (key, value) in entries {
                validate_token(key, 64, "property key is invalid")?;
                mapped.insert(key.clone(), value.clone());
            }
            mapped
        }
        Some(_) => return Err(KnowledgeError::Invalid("worker properties are invalid")),
    };
    Ok(GraphNode {
        id,
        name: text_field(raw, "name", 500, false)?,
        aliases,
        description: text_field(raw, "description", 8_000, true)?,
        kind: text_field(raw, "kind", 128, false)?,
        source_role: text_field(raw, "source_role", 64, false)?,
        properties,
    })
}

fn object<'a>(value: &'a Value, label: &'static str) -> KnowledgeResult<&'a Map<String, Value>> {
    value.as_object().ok_or(KnowledgeError::Invalid(label))
}

fn text_field(
    raw: &Map<String, Value>,
    key: &str,
    limit: usize,
    allow_empty: bool,
) -> KnowledgeResult<String> {
    let value = raw
        .get(key)
        .and_then(Value::as_str)
        .ok_or(KnowledgeError::Invalid("worker field is invalid"))?;
    validate_text(value, limit, allow_empty, "worker field is invalid")?;
    Ok(value.to_string())
}

fn endpoints(value: &Value) -> KnowledgeResult<Vec<ClaimEndpoint>> {
    let items = value
        .as_array()
        .ok_or(KnowledgeError::Invalid("worker endpoints are invalid"))?;
    if items.is_empty() || items.len() > 16 {
        return Err(KnowledgeError::Invalid("worker endpoints are invalid"));
    }
    let mut endpoints = Vec::new();
    for item in items {
        let raw = object(item, "worker endpoints are invalid")?;
        let node_id = raw
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or(KnowledgeError::Invalid("worker endpoints are invalid"))?;
        let role = raw
            .get("role")
            .and_then(Value::as_str)
            .ok_or(KnowledgeError::Invalid("worker endpoints are invalid"))?;
        validate_token(node_id, 128, "node id is invalid")?;
        validate_token(role, 64, "endpoint role is invalid")?;
        endpoints.push(ClaimEndpoint {
            node_id: node_id.to_string(),
            role: role.to_string(),
        });
    }
    Ok(endpoints)
}

fn evidence_list(value: &Value) -> KnowledgeResult<Vec<EvidenceBinding>> {
    let items = value
        .as_array()
        .ok_or(KnowledgeError::Invalid("worker evidence is invalid"))?;
    if items.len() > 32 {
        return Err(KnowledgeError::Invalid("worker evidence is invalid"));
    }
    let mut evidence = Vec::new();
    for item in items {
        let raw = object(item, "worker evidence is invalid")?;
        let span_id = text_field(raw, "span_id", 128, false)?;
        let role = BindingRole::parse(&text_field(raw, "role", 64, false)?)?;
        let attribution = text_field(raw, "attribution", 500, false)?;
        evidence.push(EvidenceBinding {
            span_id,
            role,
            attribution,
        });
    }
    Ok(evidence)
}

fn qualifiers(value: &Value) -> KnowledgeResult<Vec<Qualifier>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let items = value
        .as_array()
        .ok_or(KnowledgeError::Invalid("worker qualifiers are invalid"))?;
    let mut qualifiers = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        let raw = object(item, "worker qualifiers are invalid")?;
        let key = text_field(raw, "key", 128, false)?;
        let value = text_field(raw, "value", 2_000, true)?;
        if !seen.insert(key.clone()) {
            return Err(KnowledgeError::Invalid("qualifier key is duplicated"));
        }
        qualifiers.push(Qualifier { key, value });
    }
    Ok(qualifiers)
}

fn string_list(value: &Value, limit: usize) -> KnowledgeResult<Vec<String>> {
    let items = value
        .as_array()
        .ok_or(KnowledgeError::Invalid("worker list is invalid"))?;
    if items.len() > limit {
        return Err(KnowledgeError::Invalid("worker list is invalid"));
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or(KnowledgeError::Invalid("worker list is invalid"))
        })
        .collect()
}

fn recorded_at(value: Option<&Value>) -> KnowledgeResult<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let recorded = number
                .as_i64()
                .ok_or(KnowledgeError::Invalid("recorded time is invalid"))?;
            validate_time(recorded)?;
            Ok(Some(recorded))
        }
        Some(_) => Err(KnowledgeError::Invalid("recorded time is invalid")),
    }
}

fn time_field(raw: &Map<String, Value>, key: &str) -> KnowledgeResult<Option<TimePoint>> {
    match raw.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(adapt_time(value)?)),
    }
}

fn adapt_time(value: &Value) -> KnowledgeResult<TimePoint> {
    let raw = object(value, "worker time is invalid")?;
    let precision_name = raw
        .get("precision")
        .and_then(Value::as_str)
        .ok_or(KnowledgeError::Invalid("worker time is invalid"))?;
    let approximate = match raw.get("approximate") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(KnowledgeError::Invalid("worker time is invalid")),
    };
    let stored = match raw.get("value") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err(KnowledgeError::Invalid("worker time is invalid")),
    };
    let (precision, approximate) = if precision_name == "approximate" {
        let inferred = infer_precision(stored.as_deref().unwrap_or(""));
        (inferred, true)
    } else {
        (TimePrecision::parse(precision_name)?, approximate)
    };
    validate_time_point(&TimePoint {
        precision,
        value: stored,
        approximate,
    })
}

fn infer_precision(value: &str) -> TimePrecision {
    if value.contains('T') {
        TimePrecision::Instant
    } else if value.len() == 10 {
        TimePrecision::Date
    } else if value.len() == 7 {
        TimePrecision::Month
    } else {
        TimePrecision::Year
    }
}
