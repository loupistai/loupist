use super::*;
use crate::store::SqliteStore;
use rusqlite::{params, OptionalExtension};

impl SqliteStore {
    pub fn dataset_put(
        &self,
        loupist: &str,
        request_key: &str,
        payload: &Value,
        metadata: Value,
    ) -> Result<DatasetRef, CommandError> {
        let bytes = serde_json::to_vec(payload).map_err(error)?;
        if bytes.len() > MAX_DATASET_BYTES {
            return Err(CommandError::invalid(
                "dataset exceeds 32 MiB; split the input",
            ));
        }
        let digest = crate::util::sha256(&bytes);
        let metadata_json = serde_json::to_string(&metadata).map_err(error)?;
        let mut db = self.lock_loupist(loupist).map_err(error)?;
        let tx = db.transaction().map_err(error)?;
        let previous:Option<(String,u32,String,String)>=tx.query_row("SELECT id,revision,digest,metadata_json FROM datasets WHERE loupist_id=?1 AND request_key=?2 ORDER BY revision DESC LIMIT 1",params![loupist,request_key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(error)?;
        let (dataset_id, revision) = if let Some((id, rev, old, old_metadata)) = previous {
            if old == digest
                && (metadata.get("origin").and_then(Value::as_str) != Some("local_file")
                    || old_metadata == metadata_json)
            {
                // Identical bytes keep their revision; a fresh fetch still
                // records when those bytes were last confirmed.
                if old_metadata != metadata_json {
                    tx.execute(
                        "UPDATE datasets SET metadata_json=?3 WHERE loupist_id=?1 AND id=?2",
                        params![loupist, id, metadata_json],
                    )
                    .map_err(error)?;
                    tx.commit().map_err(error)?;
                }
                return Ok(DatasetRef {
                    dataset_id: id,
                    revision: rev,
                    digest,
                });
            }
            (
                id,
                rev.checked_add(1)
                    .ok_or_else(|| CommandError::conflict("dataset revision limit reached"))?,
            )
        } else {
            (id("dataset"), 1)
        };
        tx.execute(
            "DELETE FROM datasets WHERE loupist_id = ?1 AND id = ?2",
            params![loupist, dataset_id],
        )
        .map_err(error)?;
        tx.execute("INSERT INTO datasets(loupist_id,id,revision,digest,request_key,created_at_ms,metadata_json,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![loupist,dataset_id,revision,digest,request_key,now(),metadata_json,bytes]).map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(DatasetRef {
            dataset_id,
            revision,
            digest,
        })
    }

    /// The current revision for a request when its recorded `fetched_at` is at
    /// most `max_age_seconds` old. Older, undated or missing revisions return
    /// `None` so the caller fetches again.
    pub fn dataset_fresh(
        &self,
        loupist: &str,
        request_key: &str,
        max_age_seconds: u64,
    ) -> Result<Option<(DatasetRef, Value, Value)>, CommandError> {
        let db = self.lock_loupist(loupist).map_err(error)?;
        let current: Option<(String, u32, String)> = db.query_row(
                "SELECT id,revision,metadata_json FROM datasets WHERE loupist_id=?1 AND request_key=?2 ORDER BY revision DESC LIMIT 1",
                params![loupist, request_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(error)?;
        let Some((id, revision, metadata)) = current else {
            return Ok(None);
        };
        let metadata: Value = serde_json::from_str(&metadata).map_err(error)?;
        let fresh = metadata
            .get("fetched_at")
            .and_then(Value::as_str)
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|fetched| {
                let age = chrono::Utc::now().signed_duration_since(fetched);
                age >= chrono::Duration::zero()
                    && age <= chrono::Duration::seconds(max_age_seconds.min(i64::MAX as u64) as i64)
            });
        if !fresh {
            return Ok(None);
        }
        let (digest, bytes): (String, Vec<u8>) = db
            .query_row(
                "SELECT digest,payload FROM datasets WHERE loupist_id=?1 AND id=?2 AND revision=?3",
                params![loupist, id, revision],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(error)?;
        if crate::util::sha256(&bytes) != digest {
            return Err(CommandError::conflict("dataset integrity check failed"));
        }
        Ok(Some((
            DatasetRef {
                dataset_id: id,
                revision,
                digest,
            },
            serde_json::from_slice(&bytes).map_err(error)?,
            metadata,
        )))
    }

    pub fn dataset(
        &self,
        loupist: &str,
        id: &str,
        revision: u32,
    ) -> Result<(DatasetRef, Value, Value), CommandError> {
        let db = self.lock_loupist(loupist).map_err(error)?;
        let row:Option<(String,String,Vec<u8>)>=db.query_row("SELECT digest,metadata_json,payload FROM datasets WHERE loupist_id=?1 AND id=?2 AND revision=?3",params![loupist,id,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(error)?;
        let (digest, metadata, bytes) =
            row.ok_or_else(|| CommandError::not_found("dataset revision"))?;
        if crate::util::sha256(&bytes) != digest {
            return Err(CommandError::conflict("dataset integrity check failed"));
        }
        Ok((
            DatasetRef {
                dataset_id: id.into(),
                revision,
                digest,
            },
            serde_json::from_slice(&bytes).map_err(error)?,
            serde_json::from_str(&metadata).map_err(error)?,
        ))
    }
    /// The metadata of a dataset's current revision, if the dataset still
    /// exists. A request keeps one dataset id across revisions, so its trust
    /// class holds for every revision.
    pub fn dataset_current_metadata(
        &self,
        loupist: &str,
        id: &str,
    ) -> Result<Option<Value>, CommandError> {
        let db = self.lock_loupist(loupist).map_err(error)?;
        let metadata: Option<String> = db
            .query_row(
                "SELECT metadata_json FROM datasets WHERE loupist_id=?1 AND id=?2 ORDER BY revision DESC LIMIT 1",
                params![loupist, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        metadata
            .map(|metadata| serde_json::from_str(&metadata).map_err(error))
            .transpose()
    }
    /// A revision's recorded metadata, without reading its payload.
    pub fn dataset_metadata(
        &self,
        loupist: &str,
        id: &str,
        revision: u32,
    ) -> Result<Value, CommandError> {
        let db = self.lock_loupist(loupist).map_err(error)?;
        let metadata: Option<String> = db
            .query_row(
                "SELECT metadata_json FROM datasets WHERE loupist_id=?1 AND id=?2 AND revision=?3",
                params![loupist, id, revision],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        let metadata = metadata.ok_or_else(|| CommandError::not_found("dataset revision"))?;
        serde_json::from_str(&metadata).map_err(error)
    }

    pub fn dataset_describe(
        &self,
        loupist: &str,
        id: &str,
        revision: u32,
    ) -> Result<Value, CommandError> {
        const RESPONSE_BYTES: usize = 64 * 1024;
        let (reference, payload, metadata) = self.dataset(loupist, id, revision)?;
        fn arrays(
            v: &Value,
            path: String,
            out: &mut Vec<Value>,
            visited: &mut usize,
            truncated: &mut bool,
        ) {
            if out.len() >= 80 || *visited >= 10_000 {
                *truncated = true;
                return;
            }
            *visited += 1;
            match v {
                Value::Array(a) => {
                    let fields = a.first().and_then(Value::as_object);
                    let columns = fields.map(|fields| {
                        fields
                            .keys()
                            .filter(|key| key.len() <= 256)
                            .take(32)
                            .cloned()
                            .collect::<Vec<_>>()
                    });
                    let omitted = fields.map_or(0, |fields| {
                        fields
                            .len()
                            .saturating_sub(columns.as_ref().map_or(0, Vec::len))
                    });
                    if omitted > 0 {
                        *truncated = true;
                    }
                    out.push(json!({"pointer":path,"rows":a.len(),"columns":columns,"columns_omitted":omitted}));
                }
                Value::Object(o) => {
                    for (k, v) in o {
                        if *visited >= 10_000 || out.len() >= 80 {
                            *truncated = true;
                            break;
                        }
                        arrays(
                            v,
                            format!("{}/{}", path, k.replace('~', "~0").replace('/', "~1")),
                            out,
                            visited,
                            truncated,
                        )
                    }
                }
                _ => {}
            }
        }
        let mut sections = vec![];
        let mut truncated = false;
        arrays(
            &payload,
            String::new(),
            &mut sections,
            &mut 0,
            &mut truncated,
        );
        // Metadata and preview share one budget; section samples must not each get
        // another independent 32 KiB allowance and overflow the broker frame.
        let bounded = preview(&json!({"metadata":metadata,"sample":payload}), 3);
        let mut result = json!({"ref":reference,"fetched_at":metadata.get("fetched_at"),"metadata":bounded.get("metadata"),"sections":[],"sample":bounded.get("sample"),
   "coverage":{"retained":"complete","delivered":"globally bounded schema and sample","computation":"pinned DatasetRef; export exact bytes for local analysis"},
   "schema_truncated":false,"sections_omitted":0});
        // JSON escaping can expand a byte-counted preview sixfold. Preserve exact
        // refs/schema and explicitly omit oversized preview bodies if that occurs.
        if serde_json::to_vec(&result).map_err(error)?.len() > RESPONSE_BYTES - 512 {
            result["sample"] = json!({"omitted":"serialized preview byte budget"});
        }
        if serde_json::to_vec(&result).map_err(error)?.len() > RESPONSE_BYTES - 512 {
            result["metadata"] = json!({"omitted":"serialized preview byte budget"});
        }
        let mut used = serde_json::to_vec(&result).map_err(error)?.len();
        let mut accepted = Vec::new();
        let mut omitted = 0;
        for section in sections {
            let bytes = serde_json::to_vec(&section).map_err(error)?.len() + 1;
            if used.saturating_add(bytes) > RESPONSE_BYTES - 512 {
                omitted += 1;
                truncated = true;
                continue;
            }
            used += bytes;
            accepted.push(section);
        }
        result["sections"] = json!(accepted);
        result["schema_truncated"] = json!(truncated);
        result["sections_omitted"] = json!(omitted);
        Ok(result)
    }
    // Keep ownership/revision explicit at every bounded slice read.
    #[allow(clippy::too_many_arguments)]
    pub fn dataset_query(
        &self,
        loupist: &str,
        id: &str,
        revision: u32,
        pointer: Option<&str>,
        offset: usize,
        limit: usize,
        columns: &[String],
    ) -> Result<Value, CommandError> {
        if limit == 0 || limit > 1000 || columns.len() > 80 {
            return Err(CommandError::invalid("query limit must be 1..1000"));
        }
        let (reference, payload, metadata) = self.dataset(loupist, id, revision)?;
        let selected = match pointer {
            Some(p) => payload
                .pointer(p)
                .ok_or_else(|| CommandError::invalid("dataset pointer is absent"))?,
            None => &payload,
        };
        let (total, rows) = if let Some(items) = selected.as_array() {
            (
                items.len(),
                items
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(|v| {
                        if columns.is_empty() {
                            v.clone()
                        } else {
                            Value::Object(
                                columns
                                    .iter()
                                    .filter_map(|c| v.get(c).map(|v| (c.clone(), v.clone())))
                                    .collect(),
                            )
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            (
                1,
                if offset == 0 {
                    vec![selected.clone()]
                } else {
                    vec![]
                },
            )
        };
        let result = json!({"ref":reference,"fetched_at":metadata.get("fetched_at"),"total":total,"offset":offset,"rows":rows,"next_offset":if offset.saturating_add(limit)<total {Some(offset+limit)}else{None},"metadata":preview(&metadata,3)});
        if result.to_string().len() > 1024 * 1024 {
            return Err(CommandError::invalid(
                "query result exceeds 1 MiB; select a narrower pointer, columns or limit",
            ));
        }
        Ok(result)
    }
}
