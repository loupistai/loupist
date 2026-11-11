//! Current Loupist-scoped data shared by connectors, local files and charts.
mod persistence;
#[cfg(test)]
mod tests;
use crate::app::CommandError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub(crate) const MAX_DATASET_BYTES: usize = 32 * 1024 * 1024;
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub(crate) fn id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}
pub(crate) fn error(e: impl std::fmt::Display) -> CommandError {
    CommandError::internal(e.to_string())
}
pub(crate) fn embedded_ddl() -> &'static str {
    r#"
CREATE TABLE datasets (
 loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
 id TEXT NOT NULL, revision INTEGER NOT NULL, digest TEXT NOT NULL,
 request_key TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
 metadata_json TEXT NOT NULL, payload BLOB NOT NULL,
 PRIMARY KEY(loupist_id,id), UNIQUE(loupist_id,request_key));

"#
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DatasetRef {
    pub dataset_id: String,
    pub revision: u32,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetInput {
    pub name: String,
    pub dataset_id: String,
    pub revision: u32,
    #[serde(default)]
    pub pointer: Option<String>,
}
/// Data stays host-owned. This preview is for inspection, never an input replacement.
pub fn preview(value: &Value, rows: usize) -> Value {
    fn walk(v: &Value, rows: usize, depth: usize, nodes: &mut usize, bytes: &mut usize) -> Value {
        if depth > 12 || *nodes == 0 || *bytes < 32 {
            return json!({"omitted":"preview limit"});
        }
        *nodes -= 1;
        match v {
            Value::Array(a) => Value::Array(
                a.iter()
                    .take(rows)
                    .take(*nodes)
                    .map(|v| walk(v, rows, depth + 1, nodes, bytes))
                    .collect(),
            ),
            Value::Object(o) => Value::Object(
                o.iter()
                    .take(80)
                    .take(*nodes)
                    .filter(|(k, _)| k.len() <= 128)
                    .map(|(k, v)| {
                        *bytes = bytes.saturating_sub(k.len());
                        (k.clone(), walk(v, rows, depth + 1, nodes, bytes))
                    })
                    .collect(),
            ),
            Value::String(s) => {
                let text = s.chars().take(800.min(*bytes / 4)).collect::<String>();
                *bytes = bytes.saturating_sub(text.len());
                Value::String(if text.len() < s.len() {
                    text + "…"
                } else {
                    text
                })
            }
            other => {
                *bytes = bytes.saturating_sub(16);
                other.clone()
            }
        }
    }
    walk(value, rows, 0, &mut 2000, &mut (32 * 1024))
}
