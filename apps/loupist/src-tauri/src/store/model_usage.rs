//! Content-free usage history in the global catalog.
use super::{SqliteStore, StoreResult};
use crate::{domain::ChatFailureKind, providers::status::CredentialKind, runs::log::UsageSurface};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const DAY_MS: i64 = 86_400_000;
pub(crate) const USAGE_RETENTION_MS: i64 = 400 * DAY_MS;
static LAST_PRUNE: Mutex<Option<i64>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelUsageKind {
    Chat,
    World,
    Schema,
    Learning,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelUsageStatus {
    Ok,
    Failed,
    Cancelled,
    Deferred,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorldFailure {
    #[serde(rename = "world_job_failed")]
    WorldJob,
    #[serde(rename = "schema_review_failed")]
    SchemaReview,
    #[serde(rename = "learning_store_failed")]
    LearningStore,
    #[serde(rename = "learning_review_failed")]
    LearningReview,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ModelUsageFailure {
    World(WorldFailure),
    Chat(ChatFailureKind),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelUsageData {
    pub kind: ModelUsageKind,
    pub surface: UsageSurface,
    pub provider: String,
    pub credential_kind: Option<CredentialKind>,
    pub model: String,
    pub effort: Option<String>,
    pub chat_id: Option<String>,
    pub run_id: String,
    pub started_at_ms: i64,
    pub duration_ms: u64,
    pub status: ModelUsageStatus,
    pub failure_kind: Option<ModelUsageFailure>,
    #[serde(flatten)]
    pub measures: ModelUsageMeasures,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<ModelUsageAccount>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelUsageMeasures {
    pub requests: Option<u64>,
    pub input: Option<u64>,
    pub cached_input: Option<u64>,
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tokens_partial: bool,
    pub reasoning: Option<u64>,
    pub cost_microusd: Option<u64>,
    pub cost_from_provider: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelUsageAccount {
    pub provider: String,
    pub credential_kind: Option<CredentialKind>,
    #[serde(flatten)]
    pub measures: ModelUsageMeasures,
}

pub(crate) struct ModelUsageRow {
    pub finished_at_ms: i64,
    pub loupist_id: Option<String>,
    pub task_id: Option<String>,
    pub data: ModelUsageData,
}

fn initialize(connection: &Connection) -> StoreResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_usage (
            id TEXT PRIMARY KEY NOT NULL,
            finished_at_ms INTEGER NOT NULL,
            loupist_id TEXT,
            task_id TEXT,
            data_json TEXT NOT NULL
        ) STRICT;
        CREATE INDEX IF NOT EXISTS model_usage_time ON model_usage (finished_at_ms);
        CREATE INDEX IF NOT EXISTS model_usage_task ON model_usage (task_id, finished_at_ms);",
    )?;
    Ok(())
}

pub(super) fn clear_loupist(connection: &Connection, id: &str) -> StoreResult<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'model_usage')",
        [],
        |row| row.get(0),
    )?;
    if exists {
        connection.execute(
            "UPDATE model_usage
             SET loupist_id = NULL,
                 data_json = CASE WHEN json_extract(data_json, '$.surface') = 'bot'
                     THEN json_remove(data_json, '$.chatId') ELSE data_json END
             WHERE loupist_id = ?1
                OR (loupist_id IS NULL AND json_extract(data_json, '$.surface') = 'bot'
                    AND json_extract(data_json, '$.chatId') = ?2)",
            params![id, crate::chat::home::home_thread_id(id)],
        )?;
    }
    Ok(())
}

fn prune_due(connection: &Connection, now: i64, last_prune: &mut Option<i64>) -> StoreResult<()> {
    if last_prune.is_none_or(|last| now.saturating_sub(last) >= DAY_MS) {
        connection.execute(
            "DELETE FROM model_usage WHERE finished_at_ms < ?1",
            [now.saturating_sub(USAGE_RETENTION_MS)],
        )?;
        *last_prune = Some(now);
    }
    Ok(())
}

impl SqliteStore {
    pub(crate) fn put_model_usage(&self, id: &str, row: &ModelUsageRow) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        // A run may finish after its registration was removed.
        connection.execute(
            "INSERT OR REPLACE INTO model_usage (id, finished_at_ms, loupist_id, task_id, data_json)
             VALUES (?1, ?2, (SELECT id FROM loupist_profiles WHERE id = ?3), ?4,
                CASE WHEN ?6 THEN json_remove(?5, '$.chatId') ELSE ?5 END)",
            params![
                id,
                row.finished_at_ms,
                row.loupist_id,
                row.task_id,
                serde_json::to_string(&row.data)?,
                row.data.surface == UsageSurface::Bot,
            ],
        )?;
        let mut last_prune = LAST_PRUNE.lock().unwrap_or_else(|error| error.into_inner());
        prune_due(&connection, crate::commands::now_ms(), &mut last_prune)
    }

    pub(crate) fn model_usage_in_range(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> StoreResult<Vec<ModelUsageRow>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        let mut statement = connection.prepare(
            "SELECT finished_at_ms, loupist_id, task_id, data_json FROM model_usage
             WHERE finished_at_ms >= ?1 AND finished_at_ms < ?2 ORDER BY finished_at_ms, id",
        )?;
        let rows = statement.query_map(params![from_ms, to_ms], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|result| {
            let (finished_at_ms, loupist_id, task_id, json) = result?;
            Ok(ModelUsageRow {
                finished_at_ms,
                loupist_id,
                task_id,
                data: serde_json::from_str(&json)?,
            })
        })
        .collect()
    }
}

#[cfg(test)]
pub(crate) fn fixture_row(at: i64) -> ModelUsageRow {
    ModelUsageRow {
        finished_at_ms: at,
        loupist_id: None,
        task_id: None,
        data: ModelUsageData {
            kind: ModelUsageKind::Chat,
            surface: UsageSurface::Research,
            provider: "openrouter".into(),
            credential_kind: None,
            model: "example/model".into(),
            effort: None,
            chat_id: None,
            run_id: "run".into(),
            started_at_ms: at,
            duration_ms: 0,
            status: ModelUsageStatus::Ok,
            failure_kind: None,
            measures: ModelUsageMeasures {
                requests: Some(1),
                ..Default::default()
            },
            accounts: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_read_creates_catalog_table_without_a_version_change() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
        let table_count = || {
            store
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'model_usage%'",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap()
        };
        assert_eq!(table_count(), 0);
        assert!(store.model_usage_in_range(0, 1).unwrap().is_empty());
        assert_eq!(table_count(), 3);
        let version: i64 = store
            .lock()
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, super::super::STORE_SCHEMA_VERSION);
    }

    #[test]
    fn replace_range_and_unregister_preserve_the_account_total() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
        let profile = crate::test_fixtures::loupist_profile("usage-loupist");
        store.refresh_folder_registration(&profile).unwrap();
        let now = crate::commands::now_ms();
        let mut row = fixture_row(now);
        row.loupist_id = Some(profile.id.clone());
        store.put_model_usage("replace", &row).unwrap();
        row.data.measures.requests = Some(5);
        store.put_model_usage("replace", &row).unwrap();
        assert!(store.model_usage_in_range(now - 1, now).unwrap().is_empty());
        let rows = store.model_usage_in_range(now, now + 1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].loupist_id.as_deref(), Some(profile.id.as_str()));
        assert_eq!(rows[0].data.measures.requests, Some(5));
        store.delete_loupist(&profile).unwrap();
        let rows = store.model_usage_in_range(now, now + 1).unwrap();
        assert!(rows[0].loupist_id.is_none());
        assert_eq!(rows[0].data.measures.requests, Some(5));
        store.put_model_usage("late", &row).unwrap();
        assert!(store
            .model_usage_in_range(now, now + 1)
            .unwrap()
            .iter()
            .all(|row| row.loupist_id.is_none()));
    }

    #[test]
    fn retention_prunes_only_when_due_and_keeps_the_cutoff() {
        let connection = Connection::open_in_memory().unwrap();
        initialize(&connection).unwrap();
        let now = USAGE_RETENTION_MS + 100;
        connection
            .execute_batch(
                "INSERT INTO model_usage (id, finished_at_ms, data_json)
             VALUES ('old', 99, '{}'), ('cutoff', 100, '{}');",
            )
            .unwrap();
        let mut last = None;
        let count = || {
            connection
                .query_row("SELECT count(*) FROM model_usage", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        prune_due(&connection, now, &mut last).unwrap();
        assert_eq!(count(), 1);
        prune_due(&connection, now + DAY_MS - 1, &mut last).unwrap();
        assert_eq!(count(), 1);
        prune_due(&connection, now + DAY_MS, &mut last).unwrap();
        assert_eq!(count(), 0);
    }

    #[test]
    fn bot_usage_omits_home_chat_ids_on_write_and_removal() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
        let profile = crate::test_fixtures::loupist_profile("usage-loupist");
        store.refresh_folder_registration(&profile).unwrap();
        let now = crate::commands::now_ms();
        let mut row = fixture_row(now);
        row.loupist_id = Some(profile.id.clone());
        row.data.surface = UsageSurface::Bot;
        row.data.chat_id = Some(crate::chat::home::home_thread_id(&profile.id));
        let has_chat_id = |id| {
            store.lock().unwrap().query_row(
                "SELECT json_type(data_json, '$.chatId') IS NOT NULL FROM model_usage WHERE id = ?1",
                [id],
                |row| row.get::<_, bool>(0),
            ).unwrap()
        };
        store.put_model_usage("current", &row).unwrap();
        assert!(!has_chat_id("current"));
        store
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO model_usage (id, finished_at_ms, loupist_id, data_json)
             VALUES ('legacy', ?1, ?2, ?3), ('detached', ?1, NULL, ?3)",
                params![now, profile.id, serde_json::to_string(&row.data).unwrap()],
            )
            .unwrap();
        assert!(has_chat_id("legacy"));
        store.delete_loupist(&profile).unwrap();
        store.put_model_usage("late", &row).unwrap();
        for id in ["legacy", "detached", "late"] {
            assert!(!has_chat_id(id), "{id}");
        }
        let rows = store.model_usage_in_range(now, now + 1).unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .all(|row| row.loupist_id.is_none() && row.data.measures.requests == Some(1)));
    }
}
