//! A clarification is delivered to the Bot even when its conversation is busy.
//! It only becomes scheduled work after the Bot receives the missing details.
use crate::{
    app::{AppState, CommandError},
    commands::{map_internal, map_store},
    scheduled_tasks::types::QuestionReceipt,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct BotQuestion {
    pub receipt: QuestionReceipt,
    pub prompt: String,
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
    pub created_at_ms: i64,
}

fn initialize(state: &AppState) -> Result<(), CommandError> {
    state
        .store
        .lock()
        .map_err(map_store)?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS bot_schedule_questions (
        loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
        id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(loupist_id,id)
    ) STRICT;",
        )
        .map_err(map_internal)
}

pub(crate) fn pending(state: &AppState, owner: &str) -> Result<Vec<BotQuestion>, CommandError> {
    initialize(state)?;
    let db = state.store.lock().map_err(map_store)?;
    let mut stmt = db
        .prepare("SELECT body FROM bot_schedule_questions WHERE loupist_id=?1 ORDER BY rowid")
        .map_err(map_internal)?;
    let rows = stmt
        .query_map([owner], |row| row.get::<_, String>(0))
        .map_err(map_internal)?;
    rows.map(|row| serde_json::from_str(&row.map_err(map_internal)?).map_err(map_internal))
        .collect()
}

pub(crate) fn enqueue(
    state: &AppState,
    owner: &str,
    question: BotQuestion,
) -> Result<BotQuestion, CommandError> {
    initialize(state)?;
    let mut db = state.store.lock().map_err(map_store)?;
    let tx = db.transaction().map_err(map_internal)?;
    tx.execute(
        "INSERT OR IGNORE INTO bot_schedule_questions SELECT ?1,?2,?3
        WHERE (SELECT count(*) FROM bot_schedule_questions WHERE loupist_id=?1) < 100",
        params![
            owner,
            question.receipt.task_id,
            serde_json::to_string(&question).map_err(map_internal)?
        ],
    )
    .map_err(map_internal)?;
    let saved: String = tx
        .query_row(
            "SELECT body FROM bot_schedule_questions WHERE loupist_id=?1 AND id=?2",
            params![owner, question.receipt.task_id],
            |row| row.get(0),
        )
        .map_err(|_| CommandError::invalid("Too many pending Bot requests"))?;
    tx.commit().map_err(map_internal)?;
    serde_json::from_str(&saved).map_err(map_internal)
}

pub(crate) fn resolve(state: &AppState, owner: &str, id: &str) -> Result<(), CommandError> {
    state
        .store
        .lock()
        .map_err(map_store)?
        .execute(
            "DELETE FROM bot_schedule_questions WHERE loupist_id=?1 AND id=?2",
            [owner, id],
        )
        .map_err(map_internal)?;
    Ok(())
}
