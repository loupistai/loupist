//! Undo all: every run of one learning program, newest first, through the
//! per-message undo, in the caller's one transaction. A run already undone
//! writes nothing and is skipped; a preview counts the same way and rolls
//! every row back.
use std::collections::BTreeSet;

use rusqlite::params;
use serde::Serialize;

use super::{model::validate_loupist_id, KnowledgeResult, KnowledgeWrite};

/// What Undo all did, or would do with `preview`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProgramUndo {
    /// Runs this call undid. A run already undone, or one that filed
    /// nothing, is not counted.
    pub runs: u32,
    /// Lens ops restored.
    pub undone: u32,
    /// Sections a later change kept, each counted once across runs.
    pub kept: u32,
}

const SECTION_OUTCOMES: &str = "('added', 'replaced', 'retired', 'summarized', 'status_set')";

impl KnowledgeWrite<'_> {
    pub fn undo_program(
        &self,
        loupist_id: &str,
        task_id: &str,
        preview: bool,
        at_ms: i64,
    ) -> KnowledgeResult<ProgramUndo> {
        validate_loupist_id(loupist_id)?;
        if preview {
            self.tx.execute_batch("SAVEPOINT program_undo_preview")?;
        }
        let counted = self.undo_runs(loupist_id, task_id, at_ms);
        if preview {
            self.tx
                .execute_batch("ROLLBACK TO program_undo_preview; RELEASE program_undo_preview")?;
        }
        counted
    }

    fn undo_runs(
        &self,
        loupist_id: &str,
        task_id: &str,
        at_ms: i64,
    ) -> KnowledgeResult<ProgramUndo> {
        let messages = {
            let mut statement = self.tx.prepare(
                "SELECT json_extract(data_json, '$.message_id') FROM scheduled_task_runs
                 WHERE loupist_id = ?1 AND task_id = ?2
                   AND json_extract(data_json, '$.message_id') IS NOT NULL
                 ORDER BY started_at_ms DESC, rowid DESC",
            )?;
            let rows = statement.query_map(params![loupist_id, task_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        let mut result = ProgramUndo {
            runs: 0,
            undone: 0,
            kept: 0,
        };
        let mut kept = BTreeSet::new();
        for message_id in messages {
            let before = self.last_log_row()?;
            self.undo_message(loupist_id, &message_id, at_ms)?;
            if self.last_log_row()? == before {
                continue;
            }
            result.runs += 1;
            result.undone += self.tx.query_row(
                &format!(
                    "SELECT count(*) FROM knowledge_lens_log
                     WHERE loupist_id = ?1 AND rowid > ?2 AND undoes_op_id IS NOT NULL
                       AND outcome IN {SECTION_OUTCOMES}"
                ),
                params![loupist_id, before],
                |row| row.get::<_, u32>(0),
            )?;
            let mut statement = self.tx.prepare(&format!(
                "SELECT filed.record_id, COALESCE(filed.section_key, json_extract(filed.op_json, '$.op'))
                 FROM knowledge_lens_log AS filed
                 WHERE filed.loupist_id = ?1 AND filed.message_id = ?2
                   AND filed.outcome IN {SECTION_OUTCOMES} AND filed.undoes_op_id IS NULL
                   AND NOT EXISTS (
                     SELECT 1 FROM knowledge_lens_log AS undo WHERE undo.undoes_op_id = filed.op_id
                       AND undo.outcome <> 'message_undo'
                   )"
            ))?;
            let rows = statement.query_map(params![loupist_id, message_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                kept.insert(row?);
            }
        }
        result.kept = u32::try_from(kept.len()).unwrap_or(u32::MAX);
        Ok(result)
    }

    fn last_log_row(&self) -> KnowledgeResult<i64> {
        Ok(self.tx.query_row(
            "SELECT COALESCE(max(rowid), 0) FROM knowledge_lens_log",
            [],
            |row| row.get(0),
        )?)
    }
}

#[cfg(test)]
#[path = "program_undo/tests.rs"]
mod tests;
