//! The observation and row shapes retained from data connections.
use serde_json::Value;

pub(crate) struct SavedRows<'a> {
    pub payload: &'a Value,
    pub pointer: &'static str,
    pub rows: &'a [Value],
}

impl<'a> SavedRows<'a> {
    pub fn parse(payload: &'a Value) -> Option<Self> {
        if !payload["dataset_ref"].is_object() {
            return None;
        }
        let (pointer, rows) = [
            "/data/observations",
            "/data/rows",
            "/structuredContent/results",
            "/data",
            "/rows",
        ]
        .into_iter()
        .find_map(|pointer| Some((pointer, payload.pointer(pointer)?.as_array()?)))
        .filter(|(_, rows)| !rows.is_empty() && rows.iter().all(Value::is_object))?;
        Some(Self {
            payload,
            pointer,
            rows,
        })
    }

    pub fn series(&self) -> Option<&'a Value> {
        (self.pointer == "/data/observations").then(|| &self.payload["data"]["series"])
    }

    pub fn cell(&self, pointer: &str) -> Option<(usize, String)> {
        let rest = pointer.strip_prefix(self.pointer)?.strip_prefix('/')?;
        let (row, key) = rest.split_once('/')?;
        if key.contains('/') {
            return None;
        }
        let row = row.parse::<usize>().ok()?;
        let key = key.replace("~1", "/").replace("~0", "~");
        let value = self.rows.get(row)?.get(&key)?;
        (!value.is_array() && !value.is_object() && visible_field(&key)).then_some((row, key))
    }

    pub fn column_label(&self, key: &str) -> Option<&'a str> {
        if key == "value" {
            if let Some(name) = self
                .series()
                .and_then(|series| series["indicator_name"].as_str())
            {
                return Some(name);
            }
        }
        let column = [&self.payload["data"]["columns"], &self.payload["columns"]]
            .into_iter()
            .find_map(|columns| {
                columns.get(key).or_else(|| {
                    columns
                        .as_array()?
                        .iter()
                        .find(|column| column["id"] == key || column["name"] == key)
                })
            });
        column
            .and_then(|column| column.as_str().or(column["label"].as_str()))
            .or_else(|| {
                self.payload["structuredContent"]["extra"]["results_metadata"][key]["title"]
                    .as_str()
            })
            .filter(|label| !label.trim().is_empty())
    }
}

pub(crate) fn visible_field(key: &str) -> bool {
    let folded = key.to_ascii_lowercase().replace(['_', '-'], "");
    !folded.ends_with("digest")
        && !folded.ends_with("hash")
        && !key.ends_with("_id")
        && !key.ends_with("Id")
        && !key.ends_with("ID")
        && !key.ends_with("_ref")
        && !key.ends_with("Ref")
        && !matches!(
            folded.as_str(),
            "id" | "revision"
                | "metadata"
                | "storage"
                | "datastorage"
                | "modeldelivery"
                | "provenance"
                | "datasetid"
                | "datasetref"
                | "recordid"
                | "spanid"
                | "representationid"
                | "sourceid"
                | "captureid"
                | "loupistid"
                | "providerupdatedat"
                | "columns"
                | "schema"
                | "schemaversion"
                | "data"
                | "rows"
                | "results"
                | "query"
                | "page"
                | "coverage"
                | "status"
                | "operation"
                | "tool"
                | "source"
                | "fetchedat"
                | "retrievedat"
                | "returned"
                | "available"
                | "truncated"
                | "offset"
                | "nextoffset"
                | "totalrows"
                | "warnings"
        )
}
