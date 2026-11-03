use serde::Serialize;
use serde_json::{Map, Value};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{EvidenceRevision, RecordKind};
use super::schema::KNOWLEDGE_FORMAT_VERSION;
use crate::util::sha256;

pub(super) fn to_value<T: Serialize>(value: &T) -> KnowledgeResult<Value> {
    Ok(serde_json::to_value(value)?)
}

pub(super) fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> KnowledgeResult<T> {
    Ok(serde_json::from_str(text)?)
}

pub(super) fn canonical_string(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

pub(super) fn digest_json(value: &Value) -> KnowledgeResult<String> {
    require_canonical_round_trip(value)?;
    Ok(sha256(canonical_string(value).as_bytes()))
}

/// Host canonical form is serde_json with sorted object keys.
pub fn canonicalize_json_text(input: &str) -> KnowledgeResult<String> {
    let value: Value = serde_json::from_str(input)
        .map_err(|_| KnowledgeError::Invalid("canonical json is invalid"))?;
    require_canonical_round_trip(&value)?;
    Ok(canonical_string(&value))
}

pub(super) fn digest_bytes(bytes: &[u8]) -> String {
    sha256(bytes)
}

pub(super) fn revision_digest(
    kind: RecordKind,
    payload: &Value,
    dependencies: &[EvidenceRevision],
) -> KnowledgeResult<String> {
    let preimage = serde_json::json!({
        "dependencies": dependencies,
        "formatVersion": KNOWLEDGE_FORMAT_VERSION,
        "kind": kind,
        "payload": payload,
    });
    require_canonical_round_trip(&preimage)?;
    digest_json(&preimage)
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => out.push_str(&json_string(text)),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(map, out),
    }
}

fn write_object(map: &Map<String, Value>, out: &mut String) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(key));
        out.push(':');
        write_canonical(&map[*key], out);
    }
    out.push('}');
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

pub(super) fn require_canonical_round_trip(value: &Value) -> KnowledgeResult<()> {
    let parsed: Value = serde_json::from_str(&canonical_string(value))
        .map_err(|_| KnowledgeError::Invalid("canonical json is invalid"))?;
    if canonical_string(&parsed) != canonical_string(value) {
        return Err(KnowledgeError::Invalid("canonical json is unstable"));
    }
    Ok(())
}
