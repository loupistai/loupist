//! Learned marks: the sections of a Lens page whose newest op a learning run
//! filed. Computed on read from `knowledge_lens_log`; nothing is stored. The
//! op's chat and message identify the run, as `complete_scheduled_chat`
//! records them on the run, and the run names its program.
use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::lens::LensBodyView;
use super::model::validate_loupist_id;
use super::KnowledgeResult;

/// The run that filed a section's newest op, while no later op changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LearnedMark {
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: i64,
    pub task_id: String,
    /// The run's `run_id`, which Schedule opens.
    pub run_id: String,
    /// The program's current title.
    pub program_title: String,
}

/// Marks for `record_id`, keyed by section key. Section ops are the outcomes
/// `touched_after` counts as a change to a section. An undo is filed as such
/// an op by the user, so it clears the mark of the op it undid.
pub(crate) fn learned_marks(
    connection: &Connection,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<BTreeMap<String, LearnedMark>> {
    validate_loupist_id(loupist_id)?;
    let mut statement = connection.prepare(
        "WITH newest AS (
             SELECT section_key, chat_id, message_id, created_at_ms,
                    ROW_NUMBER() OVER (PARTITION BY section_key ORDER BY rowid DESC) AS position
             FROM knowledge_lens_log
             WHERE loupist_id = ?1 AND record_id = ?2 AND section_key IS NOT NULL
               AND outcome IN ('added', 'replaced', 'retired', 'summarized', 'status_set')
         )
         SELECT newest.section_key, newest.created_at_ms, run.task_id, run.run_id,
                json_extract(task.data_json, '$.title')
         FROM newest
         JOIN scheduled_task_runs AS run
           ON run.loupist_id = ?1
          AND run.thread_id = newest.chat_id
          AND json_extract(run.data_json, '$.message_id') = newest.message_id
          AND json_extract(run.data_json, '$.kind') = 'learning'
         JOIN scheduled_tasks AS task
           ON task.id = run.task_id AND task.loupist_id = run.loupist_id
         WHERE newest.position = 1",
    )?;
    let rows = statement.query_map(params![loupist_id, record_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            LearnedMark {
                at_ms: row.get(1)?,
                task_id: row.get(2)?,
                run_id: row.get(3)?,
                program_title: row.get(4)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

impl LensBodyView {
    /// Puts each section's mark on the view `read_page` returns.
    pub(super) fn with_learned(mut self, mut marks: BTreeMap<String, LearnedMark>) -> Self {
        let sections = match &mut self {
            Self::Principle { .. } => return self,
            Self::Perspective { sections, .. }
            | Self::Method { sections, .. }
            | Self::Subject { sections, .. }
            | Self::Idea { sections, .. } => sections,
        };
        for section in sections {
            section.learned = marks.remove(section.key.as_str());
        }
        self
    }
}

#[cfg(test)]
#[path = "learned_marks/tests.rs"]
mod tests;
