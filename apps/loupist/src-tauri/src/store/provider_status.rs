//! Host-owned model-provider state (providers spec §3.1): one row per provider
//! in the global catalog database, the models a provider stopped offering, the
//! background probe clock and the single pending "default changed" notice.
//! Credentials never reach these tables.
//!
//! Each change is one transaction that writes only the columns it owns, so a
//! status transition and Auto's remembered model never overwrite each other.
use super::{SqliteStore, StoreError, StoreResult};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

/// The persisted shape; `providers::status` owns its meaning and transitions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProviderStatusRecord {
    pub provider: String,
    pub credential_kind: String,
    pub state: String,
    pub notice_kind: Option<String>,
    pub notice_at_ms: Option<i64>,
    pub last_limit_at_ms: Option<i64>,
    pub connected_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub used_by_json: String,
    pub last_auto_model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelNoticeRecord {
    pub id: String,
    pub kind: String,
    pub model: String,
    pub created_at_ms: i64,
}

/// At most this many gone models are remembered per provider.
const MAX_GONE_PER_PROVIDER: i64 = 256;

fn initialize(connection: &Connection) -> StoreResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_status (
            provider TEXT PRIMARY KEY NOT NULL CHECK(length(provider) BETWEEN 1 AND 128),
            credential_kind TEXT NOT NULL,
            state TEXT NOT NULL,
            notice_kind TEXT,
            notice_at_ms INTEGER,
            last_limit_at_ms INTEGER,
            connected_at_ms INTEGER,
            updated_at_ms INTEGER NOT NULL,
            used_by_json TEXT NOT NULL DEFAULT '{}',
            last_auto_model TEXT
         ) STRICT;
         CREATE TABLE IF NOT EXISTS model_notice (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            notice_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            model TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL
         ) STRICT;
         CREATE TABLE IF NOT EXISTS provider_gone_model (
            model_id TEXT PRIMARY KEY NOT NULL CHECK(length(model_id) BETWEEN 1 AND 640),
            provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 128),
            gone_at_ms INTEGER NOT NULL
         ) STRICT;
         CREATE TABLE IF NOT EXISTS provider_probe (
            provider TEXT PRIMARY KEY NOT NULL CHECK(length(provider) BETWEEN 1 AND 128),
            probed_at_ms INTEGER NOT NULL
         ) STRICT;",
    )?;
    Ok(())
}

fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderStatusRecord> {
    Ok(ProviderStatusRecord {
        provider: row.get(0)?,
        credential_kind: row.get(1)?,
        state: row.get(2)?,
        notice_kind: row.get(3)?,
        notice_at_ms: row.get(4)?,
        last_limit_at_ms: row.get(5)?,
        connected_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        used_by_json: row.get(8)?,
        last_auto_model: row.get(9)?,
    })
}

const COLUMNS: &str = "provider, credential_kind, state, notice_kind, notice_at_ms, last_limit_at_ms, connected_at_ms, updated_at_ms, used_by_json, last_auto_model";

fn valid_provider(provider: &str) -> StoreResult<()> {
    if provider.is_empty() || provider.len() > 128 {
        return Err(StoreError::Invalid("provider status id is invalid".into()));
    }
    Ok(())
}

fn insert(connection: &Connection, value: &ProviderStatusRecord) -> StoreResult<()> {
    connection.execute(
        &format!(
            "INSERT INTO provider_status ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        ),
        params![
            value.provider,
            value.credential_kind,
            value.state,
            value.notice_kind,
            value.notice_at_ms,
            value.last_limit_at_ms,
            value.connected_at_ms,
            value.updated_at_ms,
            value.used_by_json,
            value.last_auto_model,
        ],
    )?;
    Ok(())
}

fn select(connection: &Connection, provider: &str) -> StoreResult<Option<ProviderStatusRecord>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM provider_status WHERE provider = ?1"),
            [provider],
            record,
        )
        .optional()
        .map_err(Into::into)
}

impl SqliteStore {
    pub(crate) fn provider_status_all(&self) -> StoreResult<Vec<ProviderStatusRecord>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM provider_status ORDER BY provider"
        ))?;
        let rows = statement
            .query_map([], record)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub(crate) fn provider_status_get(
        &self,
        provider: &str,
    ) -> StoreResult<Option<ProviderStatusRecord>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        select(&connection, provider)
    }

    /// Reads, changes and writes one provider's status in one transaction.
    /// `next` sees the stored row; `None` deletes it. Only the status columns
    /// are written: Auto's remembered model is never touched here.
    pub(crate) fn provider_status_transition(
        &self,
        provider: &str,
        next: impl FnOnce(Option<ProviderStatusRecord>) -> Option<ProviderStatusRecord>,
    ) -> StoreResult<(Option<ProviderStatusRecord>, Option<ProviderStatusRecord>)> {
        valid_provider(provider)?;
        let mut connection = self.lock()?;
        initialize(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = select(&transaction, provider)?;
        let next = next(current.clone()).map(|mut row| {
            row.provider = provider.to_owned();
            row
        });
        match (&current, &next) {
            (None, None) => {}
            (Some(_), None) => {
                transaction.execute(
                    "DELETE FROM provider_status WHERE provider = ?1",
                    [provider],
                )?;
            }
            (None, Some(row)) => insert(&transaction, row)?,
            (Some(_), Some(row)) => {
                transaction.execute(
                    "UPDATE provider_status SET credential_kind = ?2, state = ?3, notice_kind = ?4,
                       notice_at_ms = ?5, last_limit_at_ms = ?6, connected_at_ms = ?7,
                       updated_at_ms = ?8, used_by_json = ?9
                     WHERE provider = ?1",
                    params![
                        provider,
                        row.credential_kind,
                        row.state,
                        row.notice_kind,
                        row.notice_at_ms,
                        row.last_limit_at_ms,
                        row.connected_at_ms,
                        row.updated_at_ms,
                        row.used_by_json,
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok((current, next))
    }

    /// Stores Auto's resolution for a provider and returns the previous one.
    /// Only that column changes; `fresh` is the row to create when none exists.
    pub(crate) fn provider_status_remember_auto(
        &self,
        fresh: &ProviderStatusRecord,
        model_id: &str,
    ) -> StoreResult<Option<String>> {
        valid_provider(&fresh.provider)?;
        let mut connection = self.lock()?;
        initialize(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = match select(&transaction, &fresh.provider)? {
            Some(current) => {
                transaction.execute(
                    "UPDATE provider_status SET last_auto_model = ?2 WHERE provider = ?1",
                    params![fresh.provider, model_id],
                )?;
                current.last_auto_model
            }
            None => {
                insert(
                    &transaction,
                    &ProviderStatusRecord {
                        last_auto_model: Some(model_id.to_owned()),
                        ..fresh.clone()
                    },
                )?;
                None
            }
        };
        transaction.commit()?;
        Ok(previous)
    }

    pub(crate) fn provider_status_delete(&self, provider: &str) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection.execute(
            "DELETE FROM provider_status WHERE provider = ?1",
            [provider],
        )?;
        Ok(())
    }

    /// A model the provider answered 404 for. Model refreshes leave it out
    /// until the provider is connected again.
    pub(crate) fn gone_model_add(
        &self,
        provider: &str,
        model_id: &str,
        at_ms: i64,
    ) -> StoreResult<()> {
        valid_provider(provider)?;
        let mut connection = self.lock()?;
        initialize(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO provider_gone_model (model_id, provider, gone_at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(model_id) DO UPDATE SET provider = excluded.provider, gone_at_ms = excluded.gone_at_ms",
            params![model_id, provider, at_ms],
        )?;
        transaction.execute(
            "DELETE FROM provider_gone_model WHERE provider = ?1 AND model_id NOT IN (
                SELECT model_id FROM provider_gone_model WHERE provider = ?1
                ORDER BY gone_at_ms DESC, model_id LIMIT ?2)",
            params![provider, MAX_GONE_PER_PROVIDER],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn gone_models(&self, provider: &str) -> StoreResult<Vec<String>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        let mut statement = connection.prepare(
            "SELECT model_id FROM provider_gone_model WHERE provider = ?1 ORDER BY model_id",
        )?;
        let rows = statement
            .query_map([provider], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub(crate) fn gone_models_clear(&self, provider: &str) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection.execute(
            "DELETE FROM provider_gone_model WHERE provider = ?1",
            [provider],
        )?;
        Ok(())
    }

    pub(crate) fn gone_models_delete_unknown(
        &self,
        known: &dyn Fn(&str) -> bool,
    ) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        let providers = connection
            .prepare("SELECT DISTINCT provider FROM provider_gone_model")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for provider in providers.iter().filter(|provider| !known(provider)) {
            connection.execute(
                "DELETE FROM provider_gone_model WHERE provider = ?1",
                [provider],
            )?;
        }
        Ok(())
    }

    /// Claims the one background attempt of this back-off window. True only
    /// for the caller that moved the clock.
    pub(crate) fn provider_probe_claim(
        &self,
        provider: &str,
        now_ms: i64,
        window_ms: i64,
    ) -> StoreResult<bool> {
        valid_provider(provider)?;
        let connection = self.lock()?;
        initialize(&connection)?;
        let changed = connection.execute(
            "INSERT INTO provider_probe (provider, probed_at_ms) VALUES (?1, ?2)
             ON CONFLICT(provider) DO UPDATE SET probed_at_ms = excluded.probed_at_ms
             WHERE provider_probe.probed_at_ms <= ?2 - ?3",
            params![provider, now_ms, window_ms],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn provider_probe_at(&self, provider: &str) -> StoreResult<Option<i64>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection
            .query_row(
                "SELECT probed_at_ms FROM provider_probe WHERE provider = ?1",
                [provider],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn model_notice_get(&self) -> StoreResult<Option<ModelNoticeRecord>> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection
            .query_row(
                "SELECT notice_id, kind, model, created_at_ms FROM model_notice WHERE id = 1",
                [],
                |row| {
                    Ok(ModelNoticeRecord {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        model: row.get(2)?,
                        created_at_ms: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// One pending notice at a time; the newest replaces an unread older one.
    pub(crate) fn model_notice_set(&self, notice: &ModelNoticeRecord) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection.execute(
            "INSERT INTO model_notice (id, notice_id, kind, model, created_at_ms) VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET notice_id = excluded.notice_id, kind = excluded.kind,
               model = excluded.model, created_at_ms = excluded.created_at_ms",
            params![notice.id, notice.kind, notice.model, notice.created_at_ms],
        )?;
        Ok(())
    }

    /// Clears the notice only when it is still the one the renderer showed.
    pub(crate) fn model_notice_ack(&self, id: &str) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection.execute(
            "DELETE FROM model_notice WHERE id = 1 AND notice_id = ?1",
            [id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limited() -> ProviderStatusRecord {
        ProviderStatusRecord {
            provider: "openai".into(),
            credential_kind: "plan".into(),
            state: "connected".into(),
            notice_kind: Some("plan_limit".into()),
            notice_at_ms: Some(5),
            last_limit_at_ms: Some(5),
            connected_at_ms: Some(1),
            updated_at_ms: 5,
            used_by_json: "{}".into(),
            last_auto_model: Some("openai/gpt-6.1-sol".into()),
        }
    }

    #[test]
    fn provider_status_rows_and_the_model_notice_survive_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        let row = limited();
        store
            .provider_status_transition("openai", |_| Some(row.clone()))
            .unwrap();
        store
            .model_notice_set(&ModelNoticeRecord {
                id: "notice-1".into(),
                kind: "now_using".into(),
                model: "GPT-6 Sol".into(),
                created_at_ms: 9,
            })
            .unwrap();
        store.gone_model_add("openai", "openai/gpt-5.4", 3).unwrap();
        drop(store);
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.provider_status_get("openai").unwrap(), Some(row));
        assert_eq!(store.gone_models("openai").unwrap(), ["openai/gpt-5.4"]);
        store.model_notice_ack("stale").unwrap();
        assert!(store.model_notice_get().unwrap().is_some());
        store.model_notice_ack("notice-1").unwrap();
        assert!(store.model_notice_get().unwrap().is_none());
        store.provider_status_delete("openai").unwrap();
        assert!(store.provider_status_all().unwrap().is_empty());
        store.gone_models_clear("openai").unwrap();
        assert!(store.gone_models("openai").unwrap().is_empty());
    }

    #[test]
    fn a_status_change_and_autos_remembered_model_never_overwrite_each_other() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .provider_status_transition("openai", |_| Some(limited()))
            .unwrap();
        // Auto admission remembers its model while the plan limit stands.
        let previous = store
            .provider_status_remember_auto(&ProviderStatusRecord::default(), "openai/gpt-5.6-sol")
            .unwrap_err();
        assert!(matches!(previous, StoreError::Invalid(_)));
        let fresh = ProviderStatusRecord {
            provider: "openai".into(),
            notice_kind: None,
            last_auto_model: None,
            ..limited()
        };
        assert_eq!(
            store
                .provider_status_remember_auto(&fresh, "openai/gpt-5.6-sol")
                .unwrap()
                .as_deref(),
            Some("openai/gpt-6.1-sol")
        );
        let stored = store.provider_status_get("openai").unwrap().unwrap();
        assert_eq!(stored.notice_kind.as_deref(), Some("plan_limit"));
        assert_eq!(
            stored.last_auto_model.as_deref(),
            Some("openai/gpt-5.6-sol")
        );
        // A transition built from a snapshot taken before Auto ran keeps Auto's model.
        store
            .provider_status_transition("openai", |current| {
                let mut next = current.unwrap();
                next.notice_kind = None;
                next.notice_at_ms = None;
                next.last_auto_model = Some("stale".into());
                Some(next)
            })
            .unwrap();
        let stored = store.provider_status_get("openai").unwrap().unwrap();
        assert_eq!(stored.notice_kind, None);
        assert_eq!(
            stored.last_auto_model.as_deref(),
            Some("openai/gpt-5.6-sol")
        );
    }

    #[test]
    fn concurrent_admissions_never_erase_a_plan_limit_notice() {
        let store = std::sync::Arc::new(SqliteStore::open_in_memory().unwrap());
        let fresh = ProviderStatusRecord {
            notice_kind: None,
            notice_at_ms: None,
            last_auto_model: None,
            ..limited()
        };
        store
            .provider_status_transition("openai", |_| Some(fresh.clone()))
            .unwrap();
        let writers = (0..8)
            .map(|index| {
                let store = store.clone();
                let fresh = fresh.clone();
                std::thread::spawn(move || {
                    for round in 0..50 {
                        if index == 0 && round == 25 {
                            store
                                .provider_status_transition("openai", |_| Some(limited()))
                                .unwrap();
                        } else {
                            store
                                .provider_status_remember_auto(
                                    &fresh,
                                    &format!("openai/model-{index}-{round}"),
                                )
                                .unwrap();
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().unwrap();
        }
        let stored = store.provider_status_get("openai").unwrap().unwrap();
        assert_eq!(stored.notice_kind.as_deref(), Some("plan_limit"));
    }

    #[test]
    fn one_probe_per_back_off_window() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert!(store.provider_probe_claim("openai", 1_000, 100).unwrap());
        assert!(!store.provider_probe_claim("openai", 1_050, 100).unwrap());
        assert_eq!(store.provider_probe_at("openai").unwrap(), Some(1_000));
        assert!(store.provider_probe_claim("openai", 1_100, 100).unwrap());
        assert!(store.provider_probe_claim("anthropic", 1_100, 100).unwrap());
    }
}
