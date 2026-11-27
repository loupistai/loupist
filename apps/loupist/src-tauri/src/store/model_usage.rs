//! Content-free usage history in the global catalog.
use super::{SqliteStore, StoreResult};
use crate::{domain::ChatFailureKind, providers::status::CredentialKind, runs::log::UsageSurface};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Mutex};

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

/// A plan-limit or out-of-credit failure whose ledger row was not stored.
#[derive(Clone, Debug)]
pub(crate) struct LostHold {
    pub provider: String,
    pub at_ms: i64,
    pub failure: ChatFailureKind,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_origin: Option<crate::scheduled_tasks::allowance::WorldOrigin>,
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

pub(crate) fn write_model_usage(
    connection: &Connection,
    id: &str,
    row: &ModelUsageRow,
) -> StoreResult<()> {
    initialize(connection)?;
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
    Ok(())
}

pub(crate) fn model_usage_has_run(connection: &Connection, run_id: &str) -> StoreResult<bool> {
    let table: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'model_usage')",
        [],
        |row| row.get(0),
    )?;
    if !table {
        return Ok(false);
    }
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_usage
         WHERE json_valid(data_json) AND json_extract(data_json, '$.run_id') = ?1)",
        [run_id],
        |row| row.get(0),
    )?)
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
    pub(crate) fn initialize_model_usage(&self) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)
    }

    pub(crate) fn put_model_usage(&self, id: &str, row: &ModelUsageRow) -> StoreResult<()> {
        let connection = self.lock()?;
        if let Err(error) = write_model_usage(&connection, id, row) {
            self.remember_lost_hold(row);
            return Err(error);
        }
        let mut last_prune = LAST_PRUNE.lock().unwrap_or_else(|error| error.into_inner());
        prune_due(&connection, crate::commands::now_ms(), &mut last_prune)
    }

    /// A refused row that failed on a plan limit or out of credit still holds
    /// its provider: in memory, until the app restarts.
    fn remember_lost_hold(&self, row: &ModelUsageRow) {
        let Some(ModelUsageFailure::Chat(
            failure @ (ChatFailureKind::PlanLimit | ChatFailureKind::OutOfCredit),
        )) = row.data.failure_kind
        else {
            return;
        };
        self.lost_holds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(LostHold {
                provider: row.data.provider.clone(),
                at_ms: row.finished_at_ms,
                failure,
            });
    }

    /// Provider holds whose ledger rows were refused since this store opened.
    pub(crate) fn lost_holds(&self) -> Vec<LostHold> {
        self.lost_holds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// What a restart forgets, for a test that restarts on the same store.
    #[cfg(test)]
    pub(crate) fn forget_lost_holds_on_restart(&self) {
        self.lost_holds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    pub(crate) fn model_usage_in_range(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> StoreResult<Vec<ModelUsageRow>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        model_usage_in_range(&connection, from_ms, to_ms)
    }

    pub(crate) fn populate_scheduled_usage(
        &self,
        loupist_id: &str,
        runs: &mut [crate::scheduled_tasks::types::Run],
    ) -> StoreResult<()> {
        if runs.is_empty() {
            return Ok(());
        }
        let messages = serde_json::to_string(
            &runs
                .iter()
                .filter_map(|run| run.message_id.as_deref())
                .collect::<Vec<_>>(),
        )?;
        let jobs = {
            let folder = self.lock_loupist(loupist_id)?;
            let mut statement = folder.prepare(
                "SELECT job_id, json_extract(payload_json, '$.originMessageId')
                 FROM knowledge_graph_outbox
                 WHERE loupist_id = ?1 AND json_valid(payload_json)
                   AND json_extract(payload_json, '$.originMessageId') IN (SELECT value FROM json_each(?2))",
            )?;
            let jobs = statement.query_map(params![loupist_id, messages], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            jobs.collect::<Result<BTreeMap<_, _>, _>>()?
        };
        let ids = serde_json::to_string(
            &runs
                .iter()
                .map(|run| &run.run_id)
                .chain(jobs.keys())
                .collect::<Vec<_>>(),
        )?;
        let connection = self.lock()?;
        initialize(&connection)?;
        let mut statement = connection.prepare(
            "SELECT data_json FROM model_usage WHERE loupist_id = ?1
             AND json_extract(data_json, '$.runId') IN (SELECT value FROM json_each(?2))
             ORDER BY finished_at_ms, id",
        )?;
        let mut chats = BTreeMap::new();
        let mut world = BTreeMap::<_, u64>::new();
        for row in statement.query_map(params![loupist_id, ids], |row| row.get::<_, String>(0))? {
            let data: ModelUsageData = serde_json::from_str(&row?)?;
            match data.kind {
                ModelUsageKind::Chat => {
                    chats.insert(data.run_id.clone(), data);
                }
                ModelUsageKind::World => {
                    if let (Some(message), Some(calls)) =
                        (jobs.get(&data.run_id), data.measures.requests)
                    {
                        let total = world.entry(message).or_default();
                        *total = total.saturating_add(calls);
                    }
                }
                _ => {}
            }
        }
        for run in runs {
            if let Some(data) = chats.remove(&run.run_id) {
                run.usage = Some(data.run_usage());
            }
            if let Some(usage) = &mut run.usage {
                usage.world_requests = run
                    .message_id
                    .as_ref()
                    .and_then(|id| world.get(id).copied());
            }
        }
        Ok(())
    }

    pub(crate) fn scheduled_run_ledger_usage(
        &self,
        loupist_id: &str,
        run_id: &str,
    ) -> StoreResult<Option<crate::scheduled_tasks::types::RunUsage>> {
        use rusqlite::OptionalExtension;
        let connection = self.lock()?;
        initialize(&connection)?;
        let data: Option<String> = connection.query_row(
            "SELECT data_json FROM model_usage WHERE loupist_id=?1 AND task_id IS NOT NULL
             AND json_extract(data_json,'$.runId')=?2 ORDER BY finished_at_ms DESC, id DESC LIMIT 1",
            params![loupist_id, run_id],
            |row| row.get(0),
        ).optional()?;
        data.map(|json| serde_json::from_str::<ModelUsageData>(&json).map(|data| data.run_usage()))
            .transpose()
            .map_err(Into::into)
    }
}

impl ModelUsageData {
    fn run_usage(&self) -> crate::scheduled_tasks::types::RunUsage {
        crate::scheduled_tasks::types::RunUsage {
            requests: self.measures.requests,
            input_tokens: self.measures.input,
            cached_input_tokens: self.measures.cached_input,
            output_tokens: self.measures.output,
            duration_ms: self.duration_ms,
            world_requests: None,
            cost_estimate: self
                .measures
                .cost_microusd
                .filter(|_| {
                    matches!(
                        self.credential_kind,
                        Some(CredentialKind::ApiKey | CredentialKind::Environment)
                    )
                })
                .map(|microusd| crate::domain::ChatCostEstimate {
                    microusd,
                    from_provider: self.measures.cost_from_provider == Some(true),
                }),
        }
    }
}

pub(crate) fn model_usage_in_range(
    connection: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> StoreResult<Vec<ModelUsageRow>> {
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

#[cfg(test)]
pub(crate) fn fixture_row(at: i64) -> ModelUsageRow {
    ModelUsageRow {
        finished_at_ms: at,
        loupist_id: None,
        task_id: None,
        data: ModelUsageData {
            kind: ModelUsageKind::Chat,
            world_origin: None,
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
