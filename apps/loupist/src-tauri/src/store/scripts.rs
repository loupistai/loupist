use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{SqliteStore, StoreError, StoreResult};

pub(super) const DDL: &str = r#"
CREATE TABLE scripts (
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    code TEXT NOT NULL,
    data_json TEXT NOT NULL,
    PRIMARY KEY (loupist_id, name, version)
) STRICT;
"#;

const MAX_SCRIPTS: i64 = 50;
const MAX_VERSIONS: usize = 20;
const MAX_BYTES: i64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptDraft {
    pub name: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    pub summary: String,
    #[serde(default)]
    #[cfg_attr(
        test,
        ts(
            type = "Record<string, { default: unknown; description: string }>",
            optional
        )
    )]
    pub args: Option<Map<String, Value>>,
    pub code: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptMetadata {
    pub title: String,
    pub summary: String,
    #[cfg_attr(
        test,
        ts(type = "Record<string, { default: unknown; description: string }>")
    )]
    pub args: Map<String, Value>,
    pub origin: ScriptOrigin,
    pub after_outside_text: bool,
    pub reads: Vec<crate::python_run::ledger::ReadSummary>,
    #[cfg_attr(test, ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_test: Option<ScriptTest>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScriptOrigin {
    #[default]
    User,
    Chat {
        chat_id: String,
        message_id: String,
    },
    Run {
        task_id: String,
        run_id: String,
    },
    Template {
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScriptTest {
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: i64,
    pub status: String,
    pub rounds: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScriptRef {
    pub name: String,
    pub version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScriptSummary {
    pub name: String,
    pub version: u32,
    pub sha256: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at_ms: i64,
    pub metadata: ScriptMetadata,
    pub used_by: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersion {
    pub summary: ScriptSummary,
    pub code: String,
    pub versions: Vec<u32>,
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub(crate) manifest: Option<ScriptManifest>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ScriptManifest {
    pub(crate) args_digest: String,
    pub(crate) calls: Vec<crate::python_run::ledger::ReadCall>,
}

#[derive(Serialize, Deserialize)]
struct StoredScript {
    #[serde(flatten)]
    metadata: ScriptMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest: Option<ScriptManifest>,
}

fn invalid(reason: impl Into<String>) -> StoreError {
    StoreError::Invalid(reason.into())
}

pub(crate) fn validate_name(name: &str) -> StoreResult<()> {
    if name.is_empty()
        || name.len() > 48
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid(
            "A script name must be 1–48 lowercase letters, digits or hyphens.",
        ));
    }
    Ok(())
}

pub(crate) fn script_reference(reference: &str) -> StoreResult<(&str, Option<u32>)> {
    let (name, version) = if let Some((name, version)) = reference.split_once('@') {
        let version = version
            .parse::<u32>()
            .ok()
            .filter(|version| *version > 0)
            .ok_or_else(|| invalid("A script version must be a positive integer."))?;
        (name, Some(version))
    } else {
        (reference, None)
    };
    validate_name(name)?;
    Ok((name, version))
}

pub(crate) fn validate_draft(draft: &ScriptDraft) -> StoreResult<()> {
    validate_name(&draft.name)?;
    if draft.code.trim().is_empty() || draft.code.len() > 32 * 1024 || draft.code.contains('\0') {
        return Err(invalid(
            "Script code must contain 1–32768 bytes and no NUL.",
        ));
    }
    if draft.summary.trim().is_empty()
        || draft.summary.chars().count() > 300
        || draft
            .title
            .as_ref()
            .is_some_and(|title| title.trim().is_empty() || title.chars().count() > 120)
    {
        return Err(invalid("Provide a summary of up to 300 characters and an optional title of up to 120 characters."));
    }
    if let Some(args) = &draft.args {
        if serde_json::to_vec(args)?.len() > 8 * 1024
            || args.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > 64
                    || !value.as_object().is_some_and(|spec| {
                        spec.len() == 2
                            && spec.contains_key("default")
                            && spec.get("description").and_then(Value::as_str).is_some_and(
                                |description| {
                                    !description.trim().is_empty()
                                        && !description.contains(['\n', '\r'])
                                },
                            )
                    })
            })
        {
            return Err(invalid(
                "Each argument needs a default and a one-line description, within 8 KiB in total.",
            ));
        }
    }
    Ok(())
}

/// Read references from scheduled task records under the same transaction as
/// pruning/deletion. Pending changes and the current approved code both pin.
fn pins(db: &Connection, loupist: &str, name: &str) -> StoreResult<Vec<(String, Vec<u32>)>> {
    let mut statement = db.prepare(
        "SELECT data_json FROM scheduled_tasks WHERE loupist_id = ?1
         AND json_extract(data_json, '$.trigger.script.name') = ?2 ORDER BY id",
    )?;
    let mut pins = Vec::new();
    for row in statement.query_map(params![loupist, name], |row| row.get::<_, String>(0))? {
        let task: Value = serde_json::from_str(&row?)?;
        let versions = [
            "/trigger/script/version",
            "/trigger/approved/version",
            "/trigger/pending/version",
        ]
        .into_iter()
        .filter_map(|path| task.pointer(path)?.as_u64().and_then(|v| v.try_into().ok()))
        .collect::<Vec<u32>>();
        if !versions.is_empty() {
            pins.push((task["title"].as_str().unwrap_or(name).to_owned(), versions));
        }
    }
    Ok(pins)
}

fn decode_summary(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(ScriptSummary, String, Option<ScriptManifest>)> {
    let json: String = row.get(4)?;
    let data: StoredScript = serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok((
        ScriptSummary {
            name: row.get(0)?,
            version: row.get(1)?,
            sha256: row.get(2)?,
            created_at_ms: row.get(3)?,
            metadata: data.metadata,
            used_by: Vec::new(),
        },
        row.get(5)?,
        data.manifest,
    ))
}

fn read(
    db: &Connection,
    loupist: &str,
    name: &str,
    version: Option<u32>,
) -> StoreResult<ScriptVersion> {
    let (mut summary, code, manifest) = db
        .query_row(
            "SELECT name, version, sha256, created_at_ms, data_json, code FROM scripts
         WHERE loupist_id = ?1 AND name = ?2 AND (?3 IS NULL OR version = ?3)
         ORDER BY version DESC LIMIT 1",
            params![loupist, name, version],
            decode_summary,
        )
        .optional()?
        .ok_or_else(|| invalid("The saved script version could not be found."))?;
    if crate::util::sha256(code.as_bytes()) != summary.sha256 {
        return Err(invalid("The saved script failed its integrity check."));
    }
    summary.used_by = pins(db, loupist, name)?
        .into_iter()
        .map(|(title, _)| title)
        .collect();
    let mut statement = db.prepare(
        "SELECT version FROM scripts WHERE loupist_id = ?1 AND name = ?2 ORDER BY version DESC",
    )?;
    let versions = statement
        .query_map(params![loupist, name], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ScriptVersion {
        summary,
        code,
        versions,
        manifest,
    })
}

impl SqliteStore {
    pub(crate) fn script_list(
        &self,
        loupist: &str,
        query: Option<&str>,
    ) -> StoreResult<Vec<ScriptSummary>> {
        let db = self.lock_loupist(loupist)?;
        let mut statement = db.prepare(
            "SELECT name, max(version), sha256, created_at_ms, data_json, '' FROM scripts
             WHERE loupist_id = ?1 GROUP BY name ORDER BY created_at_ms DESC, name",
        )?;
        let query = query.unwrap_or_default().to_lowercase();
        let mut results = Vec::new();
        for row in statement.query_map([loupist], decode_summary)? {
            let (mut summary, _, _) = row?;
            let searchable = format!(
                "{} {} {}",
                summary.name, summary.metadata.title, summary.metadata.summary
            )
            .to_lowercase();
            if query
                .split_whitespace()
                .all(|word| searchable.contains(word))
            {
                summary.used_by = pins(&db, loupist, &summary.name)?
                    .into_iter()
                    .map(|(title, _)| title)
                    .collect();
                results.push(summary);
            }
        }
        Ok(results)
    }

    pub(crate) fn script_get(
        &self,
        loupist: &str,
        name: &str,
        version: Option<u32>,
    ) -> StoreResult<ScriptVersion> {
        validate_name(name)?;
        let db = self.lock_loupist(loupist)?;
        read(&db, loupist, name, version)
    }

    pub(crate) fn script_save(
        &self,
        loupist: &str,
        draft: ScriptDraft,
        origin: ScriptOrigin,
        after_outside_text: bool,
    ) -> StoreResult<ScriptVersion> {
        validate_draft(&draft)?;
        let mut db = self.lock_loupist(loupist)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = tx.prepare(
            "SELECT version FROM scripts WHERE loupist_id = ?1 AND name = ?2 ORDER BY version DESC",
        )?;
        let versions = statement
            .query_map(params![loupist, draft.name], |row| row.get::<_, u32>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        if versions.is_empty() {
            let count: i64 = tx.query_row(
                "SELECT count(DISTINCT name) FROM scripts WHERE loupist_id = ?1",
                [loupist],
                |r| r.get(0),
            )?;
            if count >= MAX_SCRIPTS {
                return Err(invalid("This Loupist already has 50 scripts."));
            }
        }
        let version_key = format!("script_version:{}:{}", loupist, draft.name);
        let high_water: Option<u32> = tx
            .query_row(
                "SELECT CAST(data_json AS INTEGER) FROM app_settings WHERE id = ?1",
                [&version_key],
                |row| row.get(0),
            )
            .optional()?;
        let next = high_water
            .unwrap_or(0)
            .max(versions.first().copied().unwrap_or(0))
            .checked_add(1)
            .ok_or_else(|| invalid("Script version limit reached."))?;
        let digest = crate::util::sha256(draft.code.as_bytes());
        let previously_untrusted: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM scripts WHERE loupist_id = ?1 AND name = ?2
             AND sha256 = ?3 AND json_extract(data_json, '$.afterOutsideText') = 1)",
            params![loupist, draft.name, digest],
            |row| row.get(0),
        )?;
        let pinned = pins(&tx, loupist, &draft.name)?;
        for version in versions.iter().skip(MAX_VERSIONS - 1) {
            if pinned.iter().all(|(_, kept)| !kept.contains(version)) {
                tx.execute(
                    "DELETE FROM scripts WHERE loupist_id = ?1 AND name = ?2 AND version = ?3",
                    params![loupist, draft.name, version],
                )?;
            }
        }
        let metadata = ScriptMetadata {
            title: draft.title.unwrap_or_else(|| draft.name.clone()),
            summary: draft.summary,
            args: draft.args.unwrap_or_default(),
            origin,
            after_outside_text: after_outside_text || previously_untrusted,
            ..ScriptMetadata::default()
        };
        tx.execute(
            "INSERT INTO scripts (loupist_id, name, version, sha256, created_at_ms, code, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![loupist, draft.name, next, digest,
                chrono::Utc::now().timestamp_millis(), draft.code, serde_json::to_string(&StoredScript { metadata, manifest: None })?],
        )?;
        check_quota(&tx, loupist)?;
        tx.execute(
            "INSERT INTO app_settings (id, data_json) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json",
            params![version_key, next.to_string()],
        )?;
        let saved = read(&tx, loupist, &draft.name, Some(next))?;
        tx.commit()?;
        Ok(saved)
    }

    pub(crate) fn script_delete(
        &self,
        loupist: &str,
        name: &str,
        version: Option<u32>,
    ) -> StoreResult<()> {
        validate_name(name)?;
        let mut db = self.lock_loupist(loupist)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        delete_in(&tx, loupist, name, version)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn script_record_run(
        &self,
        loupist: &str,
        summary: &ScriptSummary,
        reads: Vec<crate::python_run::ledger::ReadSummary>,
        test: ScriptTest,
        manifest: Option<ScriptManifest>,
    ) -> StoreResult<()> {
        let mut db = self.lock_loupist(loupist)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut metadata = summary.metadata.clone();
        metadata.reads = reads;
        let at_ms = test.at_ms;
        metadata.last_test = Some(test);
        tx.execute(
            "UPDATE scripts SET data_json = ?5 WHERE loupist_id = ?1 AND name = ?2 AND version = ?3 AND sha256 = ?4
             AND coalesce(json_extract(data_json, '$.lastTest.atMs'), 0) <= ?6",
            params![loupist, summary.name, summary.version, summary.sha256,
                serde_json::to_string(&StoredScript { metadata, manifest })?, at_ms],
        )?;
        check_quota(&tx, loupist)?;
        tx.commit()?;
        Ok(())
    }
}

fn check_quota(db: &Connection, loupist: &str) -> StoreResult<()> {
    let bytes: i64 = db.query_row(
        "SELECT coalesce(sum(length(CAST(code AS BLOB)) + length(CAST(data_json AS BLOB))), 0)
         FROM scripts WHERE loupist_id = ?1",
        [loupist],
        |row| row.get(0),
    )?;
    if bytes > MAX_BYTES {
        return Err(invalid("This Loupist's scripts exceed 8 MiB."));
    }
    Ok(())
}

pub(crate) fn delete_in(
    db: &Connection,
    loupist: &str,
    name: &str,
    version: Option<u32>,
) -> StoreResult<()> {
    let tasks = pins(db, loupist, name)?
        .into_iter()
        .filter(|(_, versions)| version.is_none_or(|version| versions.contains(&version)))
        .map(|(title, _)| title)
        .collect::<Vec<_>>();
    if !tasks.is_empty() {
        return Err(invalid(format!(
            "This script is used by {}.",
            tasks.join(", ")
        )));
    }
    db.execute(
        "DELETE FROM scripts WHERE loupist_id = ?1 AND name = ?2 AND (?3 IS NULL OR version = ?3)",
        params![loupist, name, version],
    )?;
    Ok(())
}

/// Versions created by the reply still present in the library.
pub(crate) fn learned_for_message(
    db: &Connection,
    loupist: &str,
    message_id: &str,
) -> rusqlite::Result<Vec<ScriptRef>> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'scripts')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare(
        "SELECT name, version FROM scripts WHERE loupist_id = ?1 AND
         (json_extract(data_json, '$.origin.messageId') = ?2 OR
          json_extract(data_json, '$.origin.runId') IN (
             SELECT run_id FROM scheduled_task_runs WHERE loupist_id = ?1
             AND json_extract(data_json, '$.message_id') = ?2
          )) ORDER BY name, version",
    )?;
    let rows = statement.query_map(params![loupist, message_id], |row| {
        Ok(ScriptRef {
            name: row.get(0)?,
            version: row.get(1)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_versions_are_immutable_and_pruned_except_for_pins() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("loupist.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        let loupist = "library";
        store
            .create_loupist(&crate::test_fixtures::loupist_profile(loupist))
            .unwrap();
        let save = |n| {
            store
                .script_save(
                    loupist,
                    ScriptDraft {
                        name: "margins".into(),
                        title: Some("Margin by quarter".into()),
                        summary: "Compute operating margins.".into(),
                        args: None,
                        code: format!("print({n})"),
                    },
                    ScriptOrigin::User,
                    false,
                )
                .unwrap()
        };
        assert_eq!(save(1).summary.version, 1);
        store.lock_loupist(loupist).unwrap().execute(
            "INSERT INTO scheduled_tasks (id, loupist_id, revision, status, updated_at_ms, data_json)
             VALUES ('task-1', ?1, 1, 'paused', 1, ?2)",
            params![loupist, r#"{"title":"Price watch","trigger":{"script":{"name":"margins","version":1}}}"#],
        ).unwrap();
        for n in 2..=23 {
            assert_eq!(save(n).summary.version, n);
        }
        let latest = store.script_get(loupist, "margins", None).unwrap();
        assert_eq!(latest.versions.len(), 21);
        assert_eq!(latest.summary.used_by, ["Price watch"]);
        assert_eq!(
            store.script_get(loupist, "margins", Some(1)).unwrap().code,
            "print(1)"
        );
        assert!(store.script_get(loupist, "margins", Some(2)).is_err());
        assert!(store
            .script_delete(loupist, "margins", None)
            .unwrap_err()
            .to_string()
            .contains("Price watch"));
        store.script_delete(loupist, "margins", Some(23)).unwrap();
        assert_eq!(save(24).summary.version, 24);
        drop(store);
        let store = SqliteStore::open(&path).unwrap();
        let latest = store.script_get(loupist, "margins", None).unwrap();
        assert_eq!(latest.code, "print(24)");
        assert_eq!(latest.summary.version, 24);

        assert_eq!(
            store
                .script_list(loupist, Some("margin quarter"))
                .unwrap()
                .len(),
            1
        );
    }
}
