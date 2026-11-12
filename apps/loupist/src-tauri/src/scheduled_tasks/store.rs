use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::{
    domain::{ChatSession, MemoryPolicy},
    store::{SqliteStore, StoreError, StoreResult},
};

use super::{schedule::next_run_after, types::*};

pub(crate) const DDL: &str = r#"
CREATE TABLE scheduled_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'completed')),
    next_run_at_ms INTEGER CHECK (next_run_at_ms >= 0),
    target_thread_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    data_json TEXT NOT NULL,
    UNIQUE (id, loupist_id),
    CHECK ((status = 'active') = (next_run_at_ms IS NOT NULL))
) STRICT;
CREATE TABLE scheduled_task_runs (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    task_revision INTEGER NOT NULL CHECK (task_revision > 0),
    thread_id TEXT REFERENCES chat_sessions(id) ON DELETE SET NULL,
    run_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'interrupted')),
    scheduled_for_ms INTEGER NOT NULL CHECK (scheduled_for_ms >= 0),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    finished_at_ms INTEGER CHECK (finished_at_ms >= started_at_ms),
    data_json TEXT NOT NULL,
    task_snapshot_json TEXT NOT NULL,
    FOREIGN KEY (task_id, loupist_id) REFERENCES scheduled_tasks(id, loupist_id) ON DELETE CASCADE,
    CHECK ((status = 'running') = (finished_at_ms IS NULL)),
    CHECK (status <> 'running' OR thread_id IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX one_briefing_per_loupist ON scheduled_tasks(loupist_id) WHERE json_extract(data_json,'$.kind')='briefing';
CREATE INDEX scheduled_tasks_by_loupist ON scheduled_tasks(loupist_id, updated_at_ms DESC, id);
CREATE INDEX scheduled_tasks_by_due ON scheduled_tasks(next_run_at_ms, id) WHERE status = 'active';
CREATE INDEX scheduled_task_runs_by_task ON scheduled_task_runs(task_id, started_at_ms DESC, id);
CREATE UNIQUE INDEX scheduled_task_one_running ON scheduled_task_runs(task_id) WHERE status = 'running';
"#;

const TASK_COLUMNS: &str =
    "id,loupist_id,revision,status,next_run_at_ms,target_thread_id,updated_at_ms,data_json";
const RUN_COLUMNS: &str = "id,task_id,loupist_id,task_revision,thread_id,run_id,status,scheduled_for_ms,started_at_ms,finished_at_ms,data_json,task_snapshot_json";
const MAX_TASK_JSON_BYTES: usize = MAX_PROMPT_BYTES * 6 + 32 * 1024;
type RawTask = (
    String,
    String,
    i64,
    String,
    Option<i64>,
    Option<String>,
    i64,
    String,
);
type RawRun = (
    String,
    String,
    String,
    i64,
    Option<String>,
    String,
    String,
    i64,
    i64,
    Option<i64>,
    String,
    String,
);

impl SqliteStore {
    pub(crate) fn inbox_runs(
        &self,
        loupist_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<Run>> {
        Ok(self.inbox_page(loupist_id, cursor, limit)?.0)
    }

    /// One inbox page and the cursor after it, `None` on the last page. A run
    /// that cannot be read is left out, and the cursor still moves past it.
    pub(crate) fn inbox_page(
        &self,
        loupist_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> StoreResult<(Vec<Run>, Option<String>)> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        if !(1..=100).contains(&limit) {
            return Err(invalid("inbox limit must be between 1 and 100"));
        }
        let boundary = cursor
            .map(|id| {
                identifier(id)?;
                connection
                    .query_row(
                        "SELECT started_at_ms,run_id FROM scheduled_task_runs WHERE run_id=?1 AND loupist_id=?2",
                        params![id, loupist_id],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?
                    .ok_or_else(|| invalid("inbox cursor not found"))
            })
            .transpose()?;
        let mut statement = connection.prepare(&format!("SELECT {RUN_COLUMNS} FROM scheduled_task_runs WHERE loupist_id=?1 AND (?2 IS NULL OR (started_at_ms,run_id)<(?2,?3)) ORDER BY started_at_ms DESC,run_id DESC LIMIT ?4"))?;
        let rows = statement
            .query_map(
                params![
                    loupist_id,
                    boundary.as_ref().map(|(started, _)| started),
                    boundary.as_ref().map(|(_, run_id)| run_id),
                    limit as i64
                ],
                raw_run,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let next = (rows.len() == limit)
            .then(|| rows.last().map(|row| row.5.clone()))
            .flatten();
        let runs = rows
            .into_iter()
            .filter_map(|row| readable(decode_run(&connection, row)))
            .collect();
        Ok((runs, next))
    }

    pub(crate) fn inbox_unread_count(&self, loupist_id: &str) -> StoreResult<u64> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        Ok(connection.query_row("SELECT count(*) FROM scheduled_task_runs WHERE loupist_id=?1 AND status<>'running' AND json_extract(data_json,'$.read_at_ms') IS NULL",[loupist_id],|row| row.get::<_,i64>(0))? as u64)
    }

    pub(crate) fn inbox_mark_read(
        &self,
        loupist_id: &str,
        ids: &[String],
        now: i64,
    ) -> StoreResult<()> {
        validate_now(now)?;
        if ids.len() > 100 {
            return Err(invalid("too many inbox items"));
        }
        let mut connection = self.lock_loupist(loupist_id)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_loupist(&tx, loupist_id)?;
        for id in ids {
            let mut run =
                load_run(&tx, loupist_id, id)?.ok_or_else(|| invalid("inbox item not found"))?;
            if run.status != RunStatus::Running && run.read_at_ms.is_none() {
                run.read_at_ms = Some(now);
                tx.execute(
                    "UPDATE scheduled_task_runs SET data_json=?2 WHERE run_id=?1",
                    params![id, serde_json::to_string(&run)?],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn scheduled_run_briefing_context(
        &self,
        loupist_id: &str,
        id: &str,
        context: crate::briefings::BriefingContext,
    ) -> StoreResult<()> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut run = load_run(&connection, loupist_id, id)?
            .ok_or_else(|| invalid("scheduled run not found"))?;
        run.briefing_context = Some(context);
        connection.execute(
            "UPDATE scheduled_task_runs SET data_json=?2 WHERE run_id=?1",
            params![id, serde_json::to_string(&run)?],
        )?;
        Ok(())
    }

    pub(crate) fn scheduled_run_result(
        &self,
        loupist_id: &str,
        id: &str,
        text: &str,
        message_id: &str,
    ) -> StoreResult<()> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut run = load_run(&connection, loupist_id, id)?
            .ok_or_else(|| invalid("scheduled run not found"))?;
        run.summary = crate::briefings::summary(text);
        run.message_id = Some(message_id.to_owned());
        connection.execute(
            "UPDATE scheduled_task_runs SET data_json=?2 WHERE run_id=?1",
            params![id, serde_json::to_string(&run)?],
        )?;
        Ok(())
    }

    /// A host run's outcomes as structured items the renderer words (the
    /// earnings task has no reply message).
    pub(crate) fn scheduled_run_summary(
        &self,
        loupist_id: &str,
        id: &str,
        items: Vec<super::types::RunSummaryItem>,
    ) -> StoreResult<()> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut run = load_run(&connection, loupist_id, id)?
            .ok_or_else(|| invalid("scheduled run not found"))?;
        run.summary_items = items;
        run.validate()?;
        connection.execute(
            "UPDATE scheduled_task_runs SET data_json=?2 WHERE run_id=?1",
            params![id, serde_json::to_string(&run)?],
        )?;
        Ok(())
    }

    /// The earnings task looks again when its next capture is due. Only an
    /// active task moves, and its revision stays (no user edit happened).
    pub(crate) fn scheduled_task_reschedule(
        &self,
        loupist_id: &str,
        id: &str,
        next_run_at_ms: i64,
    ) -> StoreResult<()> {
        validate_now(next_run_at_ms)?;
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(mut task) = load_task(&transaction, loupist_id, id)? else {
            return Ok(());
        };
        if task.status != TaskStatus::Active || task.kind != TaskKind::Earnings {
            return Ok(());
        }
        task.next_run_at_ms = Some(next_run_at_ms);
        task.validate()?;
        transaction.execute(
            "UPDATE scheduled_tasks SET next_run_at_ms=?3, data_json=json_set(?4,'$.failure_streak_started_at_ms',coalesce(json_extract(data_json,'$.failure_streak_started_at_ms'),0),'$.failure_streak_started_revision',coalesce(json_extract(data_json,'$.failure_streak_started_revision'),0)) WHERE id=?1 AND loupist_id=?2 AND status='active'",
            params![id, loupist_id, next_run_at_ms, serde_json::to_string(&task)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn scheduled_run_skip_tool(
        &self,
        loupist_id: &str,
        id: &str,
        name: &str,
        reason: &str,
    ) -> StoreResult<()> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut run = load_run(&connection, loupist_id, id)?
            .ok_or_else(|| invalid("scheduled run not found"))?;
        if run.skipped_tools.len() < 100 {
            run.skipped_tools.push(SkippedTool {
                name: name.chars().take(128).collect(),
                reason: reason.chars().take(256).collect(),
            });
            connection.execute(
                "UPDATE scheduled_task_runs SET data_json=?2 WHERE run_id=?1",
                params![id, serde_json::to_string(&run)?],
            )?;
        }
        Ok(())
    }

    pub fn scheduled_tasks_list(&self, loupist_id: &str) -> StoreResult<Vec<Task>> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        let mut statement = connection.prepare(&format!("SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE loupist_id=?1 ORDER BY updated_at_ms DESC,id"))?;
        let rows = statement
            .query_map([loupist_id], raw_task)?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > MAX_TASKS_PER_LOUPIST {
            return Err(invalid("too many saved scheduled tasks"));
        }
        let rows = rows
            .into_iter()
            .map(|task| (task, None))
            .collect::<Vec<_>>();
        let references = BatchReferences::load(&connection, &rows)?;
        Ok(rows
            .into_iter()
            .filter_map(|(row, _)| {
                readable(decode_task_with_references(
                    ReadReferences::Batched(&references),
                    row,
                ))
            })
            .collect())
    }

    pub fn scheduled_tasks_list_with_latest(
        &self,
        loupist_id: &str,
    ) -> StoreResult<Vec<TaskListItem>> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        // Pick one indexed occurrence per task before decoding any snapshots.
        // The tie-break matches full history, even when two runs share a clock
        // millisecond. A single statement observes a consistent task/run view.
        let qualify = |columns: &str, alias: &str| {
            columns
                .split(',')
                .map(|column| format!("{alias}.{column}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut statement = connection.prepare(&format!(
            "SELECT {},{} FROM scheduled_tasks AS task LEFT JOIN scheduled_task_runs AS latest ON latest.rowid=(SELECT occurrence.rowid FROM scheduled_task_runs AS occurrence WHERE occurrence.task_id=task.id ORDER BY occurrence.started_at_ms DESC,occurrence.rowid DESC LIMIT 1) WHERE task.loupist_id=?1 ORDER BY task.updated_at_ms DESC,task.id LIMIT ?2",
            qualify(TASK_COLUMNS, "task"), qualify(RUN_COLUMNS, "latest")
        ))?;
        let rows = statement
            .query_map(
                params![loupist_id, (MAX_TASKS_PER_LOUPIST + 1) as i64],
                |row| {
                    let latest_run = if row.get::<_, Option<String>>(8)?.is_some() {
                        Some(raw_run_at(row, 8)?)
                    } else {
                        None
                    };
                    Ok((raw_task(row)?, latest_run))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > MAX_TASKS_PER_LOUPIST {
            return Err(invalid("too many saved scheduled tasks"));
        }
        let references = BatchReferences::load(&connection, &rows)?;
        Ok(rows
            .into_iter()
            .filter_map(|(task, latest_run)| {
                let task = readable(decode_task_with_references(
                    ReadReferences::Batched(&references),
                    task,
                ))?;
                // An unreadable latest run leaves its task listed without one.
                let latest_run = latest_run
                    .and_then(|row| {
                        readable(decode_run_with_references(
                            ReadReferences::Batched(&references),
                            row,
                        ))
                    })
                    .filter(|run| {
                        let own = run.task_id == task.id && run.loupist_id == task.loupist_id;
                        if !own {
                            eprintln!(
                                "Loupist skipped a scheduled run that belongs to another task"
                            );
                        }
                        own
                    });
                Some(TaskListItem { task, latest_run })
            })
            .collect())
    }

    pub fn scheduled_task_get(&self, loupist_id: &str, id: &str) -> StoreResult<Option<Task>> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        load_task(&connection, loupist_id, id)
    }

    /// Earliest supported occurrence, excluding tasks that already own a run.
    pub(crate) fn scheduled_tasks_next_due(&self) -> StoreResult<Option<i64>> {
        let runnable = serde_json::to_string(&TaskKind::KNOWN)?;
        let pages = self.for_each_readable_database(|_, connection| {
            Ok(connection.query_row(
                "SELECT MIN(next_run_at_ms) FROM scheduled_tasks AS task WHERE status='active' AND json_extract(data_json,'$.kind') IN (SELECT value FROM json_each(?1)) AND NOT EXISTS (SELECT 1 FROM scheduled_task_runs AS run WHERE run.task_id=task.id AND run.status='running')",
                [&runnable], |row| row.get::<_, Option<i64>>(0),
            )?)
        })?;
        Ok(pages.into_iter().flatten().min())
    }

    #[cfg(test)]
    pub fn scheduled_tasks_due(&self, now: i64, limit: usize) -> StoreResult<Vec<Task>> {
        let loupists = self
            .list_loupists()?
            .into_iter()
            .map(|loupist| loupist.id)
            .collect::<Vec<_>>();
        self.scheduled_tasks_due_for_loupists(now, limit, &loupists)
    }

    pub(crate) fn scheduled_tasks_due_for_loupists(
        &self,
        now: i64,
        limit: usize,
        loupist_ids: &[String],
    ) -> StoreResult<Vec<Task>> {
        validate_now(now)?;
        let eligible = serde_json::to_string(loupist_ids)?;
        // A kind another version wrote is never due here, so it cannot take a slot.
        let runnable = serde_json::to_string(&TaskKind::KNOWN)?;
        let pages = self.for_each_readable_database(|_, connection| {
            let mut statement = connection.prepare(&format!("SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE status='active' AND next_run_at_ms<=?1 AND loupist_id IN (SELECT value FROM json_each(?3)) AND json_extract(data_json,'$.kind') IN (SELECT value FROM json_each(?4)) ORDER BY next_run_at_ms,id LIMIT ?2"))?;
            let rows = statement.query_map(params![now, limit.min(MAX_TASKS_PER_LOUPIST) as i64, eligible, runnable], raw_task)?.collect::<Result<Vec<_>, _>>()?;
            // Skipped silently: this scan runs on each scheduler wake, and the task list
            // logs the same row when it is shown.
            Ok(rows
                .into_iter()
                .filter_map(|row| decode_task(connection, row).ok())
                .filter(|task| task.kind.is_known())
                .collect::<Vec<_>>())
        })?;
        let mut tasks = pages.into_iter().flatten().collect::<Vec<_>>();
        tasks.sort_by_key(|t| (t.next_run_at_ms, t.id.clone()));
        tasks.truncate(limit);
        Ok(tasks)
    }

    pub fn scheduled_tasks_activity(&self, loupist_id: &str) -> StoreResult<Vec<Run>> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        // Durable receipts also reveal runs that began and ended between UI polls.
        // Only one receipt per task crosses the bridge, without task prompts.
        let mut statement = connection.prepare(&format!("SELECT {RUN_COLUMNS} FROM scheduled_task_runs WHERE loupist_id=?1 AND rowid IN (SELECT (SELECT occurrence.rowid FROM scheduled_task_runs AS occurrence WHERE occurrence.task_id=task.id ORDER BY occurrence.started_at_ms DESC,occurrence.rowid DESC LIMIT 1) FROM scheduled_tasks AS task WHERE task.loupist_id=?1) ORDER BY started_at_ms,id"))?;
        let rows = statement
            .query_map([loupist_id], raw_run)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|row| readable(decode_run(&connection, row)))
            .collect())
    }

    /// Unread finished runs for a Loupist: count, whether any briefing found conflicting
    /// evidence, and the newest finish time.
    pub fn unread_runs_summary(&self, loupist_id: &str) -> StoreResult<(u64, bool, i64)> {
        let connection = self.lock_loupist(loupist_id)?;
        let (count, disputed, latest): (i64, i64, i64) = connection.query_row(
            "SELECT count(*),
                    coalesce(max(coalesce(json_array_length(json_extract(data_json, '$.briefing_context.disputedEntityIds')), 0)), 0),
                    coalesce(max(finished_at_ms), 0)
             FROM scheduled_task_runs
             WHERE loupist_id = ?1 AND status <> 'running' AND json_extract(data_json, '$.read_at_ms') IS NULL",
            [loupist_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((count.max(0) as u64, disputed > 0, latest))
    }

    pub fn scheduled_task_create(
        &self,
        loupist_id: &str,
        draft: TaskDraft,
        now: i64,
    ) -> StoreResult<Task> {
        self.scheduled_task_create_idempotent(loupist_id, &new_id("scheduled"), draft, now)
    }

    /// Retrying a host-derived request key returns the same definition.
    pub(crate) fn scheduled_task_create_idempotent(
        &self,
        loupist_id: &str,
        id: &str,
        draft: TaskDraft,
        now: i64,
    ) -> StoreResult<Task> {
        self.scheduled_task_create_kind(loupist_id, id, draft, TaskKind::Custom, now)
    }

    pub(crate) fn scheduled_task_create_kind(
        &self,
        loupist_id: &str,
        id: &str,
        draft: TaskDraft,
        kind: TaskKind,
        now: i64,
    ) -> StoreResult<Task> {
        validate_now(now)?;
        identifier(id)?;
        draft.validate()?;
        if !kind.is_known() {
            return Err(invalid("scheduled task kind is invalid"));
        }
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_loupist(&transaction, loupist_id)?;
        let existing = transaction
            .query_row(
                &format!(
                    "SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE id=?1 AND loupist_id=?2"
                ),
                params![id, loupist_id],
                raw_task,
            )
            .optional()?;
        if let Some(existing) = existing {
            let task = decode_task(&transaction, existing)?;
            if task.draft() != draft || task.kind != kind {
                return Err(StoreError::Conflict(
                    "schedule request key already used; inspect the existing task",
                ));
            }
            return Ok(task);
        }
        require_target(&transaction, loupist_id, &draft.target, false)?;
        let count: i64 = transaction.query_row(
            "SELECT count(*) FROM scheduled_tasks WHERE loupist_id=?1",
            [loupist_id],
            |row| row.get(0),
        )?;
        if count >= MAX_TASKS_PER_LOUPIST as i64 {
            return Err(invalid("scheduled task limit reached for this Loupist"));
        }
        let next_run_at_ms = next_for_status(&draft, now)?;
        let task = Task {
            kind,
            id: id.into(),
            loupist_id: loupist_id.into(),
            revision: 1,
            title: draft.title,
            prompt: draft.prompt,
            schedule: draft.schedule,
            target: draft.target,
            session_thread_id: None,
            model_profile_id: draft.model_profile_id,
            thinking_level: draft.thinking_level,
            run_options: draft.run_options,
            use_memory: draft.use_memory,
            update_memory: draft.update_memory,
            status: draft.status,
            next_run_at_ms,
            last_run_at_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
            last_error: None,
            awaiting_confirmation: false,
            proposed_in_message_id: None,
        };
        task.validate()?;
        transaction.execute("INSERT INTO scheduled_tasks(id,loupist_id,revision,status,next_run_at_ms,target_thread_id,updated_at_ms,data_json) VALUES(?1,?2,?3,?4,?5,?6,?7,json_set(?8,'$.failure_streak_started_at_ms',?7,'$.failure_streak_started_revision',?3))",
            params![task.id,task.loupist_id,task.revision as i64,task.status.as_str(),task.next_run_at_ms,target_thread(&task),task.updated_at_ms,serde_json::to_string(&task)?])?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn scheduled_task_update(
        &self,
        loupist_id: &str,
        id: &str,
        expected_revision: u64,
        draft: TaskDraft,
        now: i64,
    ) -> StoreResult<Task> {
        validate_now(now)?;
        draft.validate()?;
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = require_task(&transaction, loupist_id, id)?;
        require_revision(&task, expected_revision)?;
        require_known_kind(&task)?;
        require_target(&transaction, loupist_id, &draft.target, false)?;
        if task.schedule != draft.schedule || task.status != draft.status {
            task.next_run_at_ms = next_for_status(&draft, now)?;
        }
        task.title = draft.title;
        task.prompt = draft.prompt;
        task.schedule = draft.schedule;
        if task.target != draft.target {
            task.session_thread_id = None;
        }
        task.target = draft.target;
        task.model_profile_id = draft.model_profile_id;
        task.thinking_level = draft.thinking_level;
        task.run_options = draft.run_options;
        task.use_memory = draft.use_memory;
        task.update_memory = draft.update_memory;
        task.status = draft.status;
        task.last_error = None;
        touch(&mut task, now)?;
        update_task(&transaction, &task, expected_revision)?;
        reset_failure_streak(&transaction, &task, now)?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn scheduled_task_set_status(
        &self,
        loupist_id: &str,
        id: &str,
        expected_revision: u64,
        status: TaskStatus,
        now: i64,
    ) -> StoreResult<Task> {
        validate_now(now)?;
        if status == TaskStatus::Completed {
            return Err(invalid("only execution can complete a scheduled task"));
        }
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = require_task(&transaction, loupist_id, id)?;
        require_revision(&task, expected_revision)?;
        require_known_kind(&task)?;
        if task.status == TaskStatus::Completed {
            return Err(StoreError::Conflict(
                "edit a completed one-time schedule before reactivating it",
            ));
        }
        if status == TaskStatus::Active {
            require_target(&transaction, loupist_id, &task.target, false)?;
        }
        let mut draft = task.draft();
        draft.status = status;
        task.next_run_at_ms = next_for_status(&draft, now)?;
        task.status = status;
        task.last_error = None;
        task.awaiting_confirmation = false;
        task.proposed_in_message_id = None;
        touch(&mut task, now)?;
        update_task(&transaction, &task, expected_revision)?;
        if status == TaskStatus::Active {
            reset_failure_streak(&transaction, &task, now)?;
        }
        transaction.commit()?;
        Ok(task)
    }

    /// The user's one-tap OK for a task proposed from a conversation: it becomes active.
    pub(crate) fn scheduled_task_confirm(
        &self,
        loupist_id: &str,
        id: &str,
        now: i64,
    ) -> StoreResult<Task> {
        let task = self
            .scheduled_task_get(loupist_id, id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if !task.awaiting_confirmation {
            return Err(StoreError::Conflict(
                "this scheduled task is not waiting for confirmation",
            ));
        }
        self.scheduled_task_set_status(loupist_id, id, task.revision, TaskStatus::Active, now)
    }

    /// Discards a task that was proposed from a conversation and never confirmed.
    pub(crate) fn scheduled_task_discard(&self, loupist_id: &str, id: &str) -> StoreResult<()> {
        let task = self
            .scheduled_task_get(loupist_id, id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if !task.awaiting_confirmation {
            return Err(StoreError::Conflict(
                "only a task waiting for confirmation can be discarded here",
            ));
        }
        self.scheduled_task_delete(loupist_id, id, task.revision)
    }

    pub fn scheduled_task_delete(
        &self,
        loupist_id: &str,
        id: &str,
        expected_revision: u64,
    ) -> StoreResult<()> {
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = require_task(&transaction, loupist_id, id)?;
        require_revision(&task, expected_revision)?;
        if has_running(&transaction, id)? {
            return Err(StoreError::Conflict(
                "stop the running scheduled task before deleting it",
            ));
        }
        let changed = transaction.execute(
            "DELETE FROM scheduled_tasks WHERE id=?1 AND loupist_id=?2 AND revision=?3",
            params![id, loupist_id, expected_revision as i64],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict(
                "scheduled task changed before deletion",
            ));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn scheduled_task_runs(&self, loupist_id: &str, id: &str) -> StoreResult<Vec<Run>> {
        let connection = self.lock_loupist(loupist_id)?;
        require_task(&connection, loupist_id, id)?;
        let mut statement = connection.prepare(&format!("SELECT {RUN_COLUMNS} FROM scheduled_task_runs WHERE task_id=?1 AND loupist_id=?2 ORDER BY started_at_ms DESC,rowid DESC LIMIT ?3"))?;
        let rows = statement
            .query_map(params![id, loupist_id, MAX_RUN_HISTORY as i64], raw_run)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|row| readable(decode_run(&connection, row)))
            .collect())
    }

    pub fn scheduled_task_run_get(
        &self,
        loupist_id: &str,
        run_id: &str,
    ) -> StoreResult<Option<Run>> {
        let connection = self.lock_loupist(loupist_id)?;
        require_loupist(&connection, loupist_id)?;
        load_run(&connection, loupist_id, run_id)
    }

    /// Reserve execution before publishing the occurrence. The callback runs
    /// while the store transaction is held and must not re-enter this store.
    /// Its resource is returned only after commit; any error rolls back the
    /// occurrence and drops an already-acquired admission resource.
    pub fn scheduled_task_claim_with<T>(
        &self,
        loupist_id: &str,
        id: &str,
        now: i64,
        manual: bool,
        admit: impl FnOnce(&Claim) -> StoreResult<T>,
    ) -> StoreResult<Option<(Claim, T)>> {
        validate_now(now)?;
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let snapshot = require_task(&transaction, loupist_id, id)?;
        require_known_kind(&snapshot)?;
        if !manual
            && (snapshot.status != TaskStatus::Active
                || snapshot.next_run_at_ms.is_none_or(|at| at > now))
        {
            return Ok(None);
        }
        if has_running(&transaction, id)? {
            return Ok(None);
        }
        require_target(&transaction, loupist_id, &snapshot.target, false)?;
        let thread_id = match &snapshot.target {
            TaskTarget::ExistingChat { thread_id } => thread_id.clone(),
            TaskTarget::Dedicated if snapshot.session_thread_id.is_some() => {
                snapshot.session_thread_id.clone().unwrap()
            }
            TaskTarget::NewChat | TaskTarget::Dedicated => {
                let chat = ChatSession {
                    id: new_id("chat"),
                    loupist_id: loupist_id.into(),
                    pi_session_id: Uuid::new_v4().to_string(),
                    pi_session_cache: None,
                    title: snapshot.title.clone(),
                    memory_policy: MemoryPolicy {
                        use_memory: snapshot.use_memory,
                        update_memory: snapshot.update_memory,
                    },
                    set_aside_memory: Vec::new(),
                    versions: Vec::new(),
                    messages: Vec::new(),
                    created_at_ms: now,
                    updated_at_ms: now,
                    bot_memory: Default::default(),
                };
                chat.validate()
                    .map_err(|error| invalid(&error.to_string()))?;
                transaction.execute("INSERT INTO chat_sessions(id,loupist_id,updated_at_ms,data_json) VALUES(?1,?2,?3,?4)",params![chat.id,chat.loupist_id,chat.updated_at_ms,serde_json::to_string(&chat)?])?;
                chat.id
            }
        };
        let run = Run {
            briefing_context: None,
            kind: snapshot.kind.clone(),
            read_at_ms: None,
            summary: String::new(),
            summary_items: Vec::new(),
            message_id: None,
            skipped_tools: Vec::new(),
            id: new_id("scheduled-run"),
            task_id: id.into(),
            loupist_id: loupist_id.into(),
            task_revision: snapshot.revision,
            task_title: snapshot.title.clone(),
            thread_id: Some(thread_id.clone()),
            run_id: new_id("scheduled"),
            scheduled_for_ms: if manual {
                now
            } else {
                snapshot.next_run_at_ms.expect("due task has a date")
            },
            started_at_ms: now,
            finished_at_ms: None,
            status: RunStatus::Running,
            error: None,
        };
        run.validate()?;
        let mut task = snapshot.clone();
        if task.target == TaskTarget::Dedicated {
            task.session_thread_id = Some(thread_id);
        }
        task.last_run_at_ms = Some(now);
        task.last_error = None;
        if matches!(task.schedule, Schedule::Once { .. }) {
            task.status = TaskStatus::Completed;
            task.next_run_at_ms = None;
        } else if !manual {
            task.next_run_at_ms = next_run_after(&task.schedule, now)?;
        }
        touch(&mut task, now)?;
        update_task(&transaction, &task, snapshot.revision)?;
        transaction.execute("INSERT INTO scheduled_task_runs(id,task_id,loupist_id,task_revision,thread_id,run_id,status,scheduled_for_ms,started_at_ms,finished_at_ms,data_json,task_snapshot_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10,?11)",
            params![run.id,run.task_id,run.loupist_id,run.task_revision as i64,run.thread_id,run.run_id,run.status.as_str(),run.scheduled_for_ms,run.started_at_ms,serde_json::to_string(&run)?,serde_json::to_string(&snapshot)?])?;
        transaction.execute("DELETE FROM scheduled_task_runs WHERE task_id=?1 AND id NOT IN (SELECT id FROM scheduled_task_runs WHERE task_id=?1 ORDER BY (status='running') DESC,started_at_ms DESC,rowid DESC LIMIT ?2)",params![id,MAX_RUN_HISTORY as i64])?;
        let claim = Claim {
            task: snapshot,
            run,
        };
        let admission = admit(&claim)?;
        transaction.commit()?;
        Ok(Some((claim, admission)))
    }

    pub fn scheduled_task_finish(
        &self,
        loupist_id: &str,
        run_id: &str,
        status: RunStatus,
        error: Option<&str>,
        now: i64,
    ) -> StoreResult<Run> {
        validate_now(now)?;
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let was_running = load_run(&transaction, loupist_id, run_id)?
            .is_some_and(|run| run.status == RunStatus::Running);
        let run = finish_run(
            &transaction,
            loupist_id,
            run_id,
            status,
            error.map(str::to_owned),
            now,
        )?;
        transaction.commit()?;
        drop(connection);
        if was_running {
            crate::runs::log::record_scheduled(self, &run);
        }
        Ok(run)
    }

    pub fn scheduled_tasks_recover_running(&self, now: i64) -> StoreResult<usize> {
        self.scheduled_tasks_recover_running_except(now, &HashSet::new())
    }

    /// Admission must be held while taking the live snapshot and reconciling it.
    /// A folder that was absent at startup is checked again when it returns.
    pub(crate) fn scheduled_tasks_recover_running_except(
        &self,
        now: i64,
        live_run_ids: &HashSet<String>,
    ) -> StoreResult<usize> {
        validate_now(now)?;
        // A broken receipt in one folder must not stop the other Loupists.
        // The store keeps each connection locked throughout this pass.
        let recovered = self.for_each_readable_database(|_, connection| {
            let runs = load_running(connection)?
                .into_iter()
                .filter(|run| !live_run_ids.contains(&run.run_id))
                .collect::<Vec<_>>();
            if runs.is_empty() {
                return Ok(Vec::new());
            }
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut finished = Vec::new();
            for run in &runs {
                finished.push(finish_run(
                    &transaction,
                    &run.loupist_id,
                    &run.run_id,
                    RunStatus::Interrupted,
                    Some("Interrupted unexpectedly before completion.".into()),
                    now,
                )?);
            }
            transaction.commit()?;
            Ok(finished)
        })?;
        let mut count = 0;
        for run in recovered.into_iter().flatten() {
            crate::runs::log::record_scheduled(self, &run);
            count += 1;
        }
        Ok(count)
    }
}

fn finish_run(
    connection: &Connection,
    loupist_id: &str,
    run_id: &str,
    status: RunStatus,
    error: Option<String>,
    now: i64,
) -> StoreResult<Run> {
    if status == RunStatus::Running {
        return Err(invalid(
            "a scheduled completion must have a terminal status",
        ));
    }
    if let Some(error) = &error {
        bounded_error(error)?;
    }
    let mut run =
        load_run(connection, loupist_id, run_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if run.status != RunStatus::Running {
        return if run.status == status {
            Ok(run)
        } else {
            Err(StoreError::Conflict("scheduled run is already terminal"))
        };
    }
    run.status = status;
    run.finished_at_ms = Some(now.max(run.started_at_ms));
    run.error = if status == RunStatus::Succeeded {
        None
    } else {
        Some(error.unwrap_or_else(|| {
            if status == RunStatus::Interrupted {
                "Scheduled run was interrupted.".into()
            } else {
                "Scheduled run failed.".into()
            }
        }))
    };
    run.validate()?;
    connection.execute("UPDATE scheduled_task_runs SET status=?2,finished_at_ms=?3,data_json=?4 WHERE id=?1 AND status='running'",params![run.id,run.status.as_str(),run.finished_at_ms,serde_json::to_string(&run)?])?;
    let mut task = require_task(connection, loupist_id, &run.task_id)?;
    // A late result must never overwrite a user's edit, pause, or resume.
    if run.task_revision.checked_add(1) == Some(task.revision) {
        let expected = task.revision;
        task.last_error = run.error.clone();
        if status != RunStatus::Succeeded
            && task.status == TaskStatus::Active
            && !matches!(task.schedule, Schedule::Once { .. })
        {
            // The revision disambiguates old and new runs at the same millisecond.
            // Deliberate Stops and graceful shutdowns neither count nor reset it.
            let failures = connection.prepare(
                "SELECT run.status FROM scheduled_task_runs AS run JOIN scheduled_tasks AS task ON task.id=run.task_id WHERE run.task_id=?1 AND run.status<>'running' AND run.started_at_ms>=coalesce(json_extract(task.data_json,'$.failure_streak_started_at_ms'),0) AND run.task_revision>=coalesce(json_extract(task.data_json,'$.failure_streak_started_revision'),0) AND NOT (run.status='interrupted' AND coalesce(json_extract(run.data_json,'$.error'),'') IN (?2,?3,?4)) ORDER BY run.rowid DESC LIMIT 3",
            )?.query_map(params![task.id, super::USER_STOPPED, super::APP_QUIT, super::LEGACY_INTERRUPTED], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter().take_while(|status| status != "succeeded").count();
            if failures >= 3 || run.error.as_deref().is_some_and(requires_setup) {
                task.status = TaskStatus::Paused;
                task.next_run_at_ms = None;
            } else {
                task.next_run_at_ms = next_run_after(&task.schedule, now)?;
            }
        }
        touch(&mut task, now)?;
        update_task(connection, &task, expected)?;
    }
    Ok(run)
}

fn requires_setup(error: &str) -> bool {
    matches!(
        error,
        crate::pi::provider_failure::NEEDS_RECONNECT
            | crate::pi::provider_failure::MODEL_GONE
            | "the selected Pi model is hidden from Chat"
            | "Connect an AI account in Settings › Models & providers before chatting."
    )
}

fn require_loupist(connection: &Connection, loupist_id: &str) -> StoreResult<()> {
    identifier(loupist_id)?;
    if !connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM loupist_profiles WHERE id=?1)",
        [loupist_id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

fn require_target(
    connection: &Connection,
    loupist_id: &str,
    target: &TaskTarget,
    allow_missing: bool,
) -> StoreResult<()> {
    if let TaskTarget::ExistingChat { thread_id } = target {
        let owner: Option<String> = connection
            .query_row(
                "SELECT loupist_id FROM chat_sessions WHERE id=?1",
                [thread_id],
                |row| row.get(0),
            )
            .optional()?;
        match owner {
            Some(owner) if owner == loupist_id => {}
            None if allow_missing => {}
            _ => {
                return Err(StoreError::Conflict(
                    "scheduled target Chat is missing or belongs to another Loupist",
                ))
            }
        }
    }
    Ok(())
}

fn next_for_status(draft: &TaskDraft, now: i64) -> StoreResult<Option<i64>> {
    if draft.status != TaskStatus::Active {
        return Ok(None);
    }
    next_run_after(&draft.schedule, now)?
        .map(Some)
        .ok_or_else(|| invalid("an active one-time schedule must be in the future"))
}

fn touch(task: &mut Task, now: i64) -> StoreResult<()> {
    task.revision = task
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| invalid("scheduled task revision limit reached"))?;
    task.updated_at_ms = now.max(task.updated_at_ms);
    Ok(())
}

/// A task of a kind another version wrote is listed and can be deleted, but
/// this version neither runs it nor changes its definition or status.
fn require_known_kind(task: &Task) -> StoreResult<()> {
    if task.kind.is_known() {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "another version of Loupist made this scheduled task; this version cannot run or change it",
        ))
    }
}

/// A listing or scan leaves out a row it cannot read and keeps the others; the
/// row stays stored as it is. Admission, recovery and writes stay strict.
fn readable<T>(row: StoreResult<T>) -> Option<T> {
    match row {
        Ok(value) => Some(value),
        // Like the Chat listing: no stored text reaches the log.
        Err(_) => {
            eprintln!("Loupist skipped a scheduled task record it could not read");
            None
        }
    }
}

fn require_revision(task: &Task, revision: u64) -> StoreResult<()> {
    if task.revision != revision {
        return Err(StoreError::Conflict(
            "scheduled task changed; reload before editing",
        ));
    }
    Ok(())
}

fn target_thread(task: &Task) -> Option<&str> {
    match &task.target {
        TaskTarget::NewChat => None,
        TaskTarget::Dedicated => task.session_thread_id.as_deref(),
        TaskTarget::ExistingChat { thread_id } => Some(thread_id),
    }
}
fn validate_now(now: i64) -> StoreResult<()> {
    if now < 0 {
        Err(invalid("scheduled timestamp is invalid"))
    } else {
        Ok(())
    }
}
fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn update_task(connection: &Connection, task: &Task, expected_revision: u64) -> StoreResult<()> {
    task.validate()?;
    let changed=connection.execute("UPDATE scheduled_tasks SET revision=?3,status=?4,next_run_at_ms=?5,target_thread_id=?6,updated_at_ms=?7,data_json=json_set(?8,'$.failure_streak_started_at_ms',coalesce(json_extract(data_json,'$.failure_streak_started_at_ms'),0),'$.failure_streak_started_revision',coalesce(json_extract(data_json,'$.failure_streak_started_revision'),0)) WHERE id=?1 AND loupist_id=?2 AND revision=?9",params![task.id,task.loupist_id,task.revision as i64,task.status.as_str(),task.next_run_at_ms,target_thread(task),task.updated_at_ms,serde_json::to_string(task)?,expected_revision as i64])?;
    if changed != 1 {
        return Err(StoreError::Conflict("scheduled task compare-and-swap"));
    }
    Ok(())
}

fn reset_failure_streak(connection: &Connection, task: &Task, now: i64) -> StoreResult<()> {
    connection.execute(
        "UPDATE scheduled_tasks SET data_json=json_set(data_json,'$.failure_streak_started_at_ms',?3,'$.failure_streak_started_revision',?4) WHERE id=?1 AND loupist_id=?2",
        params![task.id, task.loupist_id, now, task.revision as i64],
    )?;
    Ok(())
}

fn raw_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawTask> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}
fn raw_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRun> {
    raw_run_at(row, 0)
}
fn raw_run_at(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<RawRun> {
    Ok((
        row.get(offset)?,
        row.get(offset + 1)?,
        row.get(offset + 2)?,
        row.get(offset + 3)?,
        row.get(offset + 4)?,
        row.get(offset + 5)?,
        row.get(offset + 6)?,
        row.get(offset + 7)?,
        row.get(offset + 8)?,
        row.get(offset + 9)?,
        row.get(offset + 10)?,
        row.get(offset + 11)?,
    ))
}

// A list validates every referenced owner with two bounded batch queries,
// rather than reading the profile and Chat table again for each returned row.
struct BatchReferences {
    loupists: HashSet<String>,
    chats: HashMap<String, String>,
}

impl BatchReferences {
    fn load(connection: &Connection, rows: &[(RawTask, Option<RawRun>)]) -> StoreResult<Self> {
        let mut loupists = HashSet::new();
        let mut chats = HashSet::new();
        for (task, run) in rows {
            loupists.insert(task.1.clone());
            if task.7.len() <= MAX_TASK_JSON_BYTES {
                if let Ok(task) = serde_json::from_str::<Task>(&task.7) {
                    if let TaskTarget::ExistingChat { thread_id } = task.target {
                        chats.insert(thread_id);
                    }
                    chats.extend(task.session_thread_id);
                }
            }
            if let Some(run) = run {
                loupists.insert(run.2.clone());
                chats.extend(run.4.clone());
            }
        }
        let mut statement = connection.prepare(
            "SELECT id FROM loupist_profiles WHERE id IN (SELECT value FROM json_each(?1))",
        )?;
        let loupists = statement
            .query_map([serde_json::to_string(&loupists)?], |row| row.get(0))?
            .collect::<Result<HashSet<String>, _>>()?;
        let mut statement = connection.prepare(
            "SELECT id,loupist_id FROM chat_sessions WHERE id IN (SELECT value FROM json_each(?1))",
        )?;
        let chats = statement
            .query_map([serde_json::to_string(&chats)?], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<HashMap<String, String>, _>>()?;
        Ok(Self { loupists, chats })
    }
}

#[derive(Clone, Copy)]
enum ReadReferences<'a> {
    Database(&'a Connection),
    Batched(&'a BatchReferences),
}

impl ReadReferences<'_> {
    fn require_loupist(self, loupist_id: &str) -> StoreResult<()> {
        match self {
            Self::Database(connection) => require_loupist(connection, loupist_id),
            Self::Batched(references) => {
                identifier(loupist_id)?;
                if references.loupists.contains(loupist_id) {
                    Ok(())
                } else {
                    Err(rusqlite::Error::QueryReturnedNoRows.into())
                }
            }
        }
    }

    fn require_target(
        self,
        loupist_id: &str,
        target: &TaskTarget,
        allow_missing: bool,
    ) -> StoreResult<()> {
        match self {
            Self::Database(connection) => {
                require_target(connection, loupist_id, target, allow_missing)
            }
            Self::Batched(references) => {
                if let TaskTarget::ExistingChat { thread_id } = target {
                    match references.chats.get(thread_id) {
                        Some(owner) if owner == loupist_id => {}
                        None if allow_missing => {}
                        _ => {
                            return Err(StoreError::Conflict(
                                "scheduled target Chat is missing or belongs to another Loupist",
                            ))
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

fn decode_task(connection: &Connection, row: RawTask) -> StoreResult<Task> {
    decode_task_with_references(ReadReferences::Database(connection), row)
}

fn decode_task_with_references(references: ReadReferences<'_>, row: RawTask) -> StoreResult<Task> {
    let (id, loupist_id, revision, status, next, target, updated, json) = row;
    if json.len() > MAX_TASK_JSON_BYTES {
        return Err(invalid("stored scheduled task exceeds its limit"));
    }
    let task: Task = serde_json::from_str(&json)?;
    task.validate()?;
    if task.id != id
        || task.loupist_id != loupist_id
        || task.revision as i64 != revision
        || task.status.as_str() != status
        || task.next_run_at_ms != next
        || target_thread(&task) != target.as_deref()
        || task.updated_at_ms != updated
    {
        return Err(invalid("scheduled task projections do not match"));
    }
    references.require_loupist(&task.loupist_id)?;
    references.require_target(
        &task.loupist_id,
        &task.target,
        task.status != TaskStatus::Active,
    )?;
    if let Some(thread_id) = &task.session_thread_id {
        references.require_target(
            &task.loupist_id,
            &TaskTarget::ExistingChat {
                thread_id: thread_id.clone(),
            },
            task.status != TaskStatus::Active,
        )?;
    }
    Ok(task)
}

fn decode_run(connection: &Connection, row: RawRun) -> StoreResult<Run> {
    decode_run_with_references(ReadReferences::Database(connection), row)
}

fn decode_run_with_references(references: ReadReferences<'_>, row: RawRun) -> StoreResult<Run> {
    let (
        id,
        task_id,
        loupist_id,
        revision,
        thread_id,
        run_id,
        status,
        due,
        started,
        finished,
        json,
        snapshot,
    ) = row;
    if json.len() > 128 * 1024 || snapshot.len() > MAX_TASK_JSON_BYTES {
        return Err(invalid("stored scheduled run exceeds its limit"));
    }
    let run: Run = serde_json::from_str(&json)?;
    run.validate()?;
    let task: Task = serde_json::from_str(&snapshot)?;
    task.validate()?;
    if run.id != id
        || run.task_id != task_id
        || run.loupist_id != loupist_id
        || run.task_revision as i64 != revision
        || run.thread_id != thread_id
        || run.run_id != run_id
        || run.status.as_str() != status
        || run.scheduled_for_ms != due
        || run.started_at_ms != started
        || run.finished_at_ms != finished
        || task.id != run.task_id
        || task.loupist_id != run.loupist_id
        || task.revision != run.task_revision
        || task.title != run.task_title
        || task.kind != run.kind
    {
        return Err(invalid(
            "scheduled run projections or snapshot do not match",
        ));
    }
    references.require_loupist(&run.loupist_id)?;
    if let Some(thread_id) = &run.thread_id {
        references.require_target(
            &run.loupist_id,
            &TaskTarget::ExistingChat {
                thread_id: thread_id.clone(),
            },
            false,
        )?;
    }
    Ok(run)
}

fn load_task(connection: &Connection, loupist_id: &str, id: &str) -> StoreResult<Option<Task>> {
    identifier(id)?;
    connection
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE id=?1 AND loupist_id=?2"),
            params![id, loupist_id],
            raw_task,
        )
        .optional()?
        .map(|row| decode_task(connection, row))
        .transpose()
}
fn require_task(connection: &Connection, loupist_id: &str, id: &str) -> StoreResult<Task> {
    require_loupist(connection, loupist_id)?;
    load_task(connection, loupist_id, id)?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows.into())
}
fn load_run(connection: &Connection, loupist_id: &str, run_id: &str) -> StoreResult<Option<Run>> {
    identifier(loupist_id)?;
    identifier(run_id)?;
    connection
        .query_row(
            &format!(
                "SELECT {RUN_COLUMNS} FROM scheduled_task_runs WHERE run_id=?1 AND loupist_id=?2"
            ),
            params![run_id, loupist_id],
            raw_run,
        )
        .optional()?
        .map(|row| decode_run(connection, row))
        .transpose()
}
#[derive(Default)]
pub(super) struct RecoveryWarnings(Mutex<HashSet<String>>);

impl RecoveryWarnings {
    pub(super) fn report_once(&self, run_id: &str, report: impl FnOnce()) {
        let first = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(run_id.to_owned());
        if first {
            report();
        }
    }
}

fn load_running(connection: &Connection) -> StoreResult<Vec<Run>> {
    // Recovery retries each second. Keep the warning once per run for this
    // process, while continuing to retry decoding if its bytes are repaired.
    static WARNINGS: OnceLock<RecoveryWarnings> = OnceLock::new();
    let mut statement=connection.prepare(&format!("SELECT {RUN_COLUMNS} FROM scheduled_task_runs WHERE status='running' ORDER BY started_at_ms,id"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(5)?, raw_run(row))))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(run_id, row)| {
            match row
                .map_err(StoreError::from)
                .and_then(|row| decode_run(connection, row))
            {
                Ok(run) => Some(run),
                Err(_) => {
                    WARNINGS
                        .get_or_init(RecoveryWarnings::default)
                        .report_once(&run_id, || {
                            eprintln!("Scheduled recovery skipped unreadable run {run_id}");
                        });
                    None
                }
            }
        })
        .collect())
}
fn has_running(connection: &Connection, id: &str) -> StoreResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM scheduled_task_runs WHERE task_id=?1 AND status='running')",
        [id],
        |row| row.get(0),
    )?)
}

pub(crate) fn forget_chat_in(
    connection: &Connection,
    loupist_id: &str,
    thread_id: &str,
) -> StoreResult<()> {
    let mut statement = connection.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE loupist_id=?1 AND target_thread_id=?2"
    ))?;
    let rows = statement
        .query_map(params![loupist_id, thread_id], raw_task)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for row in rows {
        let mut task = decode_task(connection, row)?;
        let expected = task.revision;
        if task.status != TaskStatus::Completed {
            task.status = TaskStatus::Paused;
        }
        task.next_run_at_ms = None;
        task.last_error = Some(
            if task.target == TaskTarget::Dedicated {
                task.session_thread_id = None;
                "This task’s Chat was deleted. Resume to start a new dedicated Chat."
            } else {
                "The target Chat was deleted. Choose another Chat before resuming."
            }
            .into(),
        );
        let now = task.updated_at_ms;
        touch(&mut task, now)?;
        update_task(connection, &task, expected)?;
    }
    // Canonical history keeps its occurrence even when the user removes a Chat.
    // Loupist/Chat deletion admission normally prevents an in-flight deletion; the
    // durable fallback is interruption rather than a dangling running record.
    connection.execute("UPDATE scheduled_task_runs SET status=CASE WHEN status='running' THEN 'interrupted' ELSE status END, finished_at_ms=CASE WHEN status='running' THEN started_at_ms ELSE finished_at_ms END, data_json=json_set(data_json,'$.thread_id',NULL,'$.status',CASE WHEN status='running' THEN 'interrupted' ELSE status END,'$.finished_at_ms',CASE WHEN status='running' THEN started_at_ms ELSE finished_at_ms END,'$.error',CASE WHEN status='running' THEN 'The Chat was deleted.' ELSE json_extract(data_json,'$.error') END),thread_id=NULL WHERE loupist_id=?1 AND thread_id=?2",params![loupist_id,thread_id])?;
    Ok(())
}

pub(crate) fn erase_loupist_in(connection: &Connection, loupist_id: &str) -> StoreResult<()> {
    connection.execute(
        "DELETE FROM scheduled_tasks WHERE loupist_id=?1",
        [loupist_id],
    )?;
    Ok(())
}

/// A restored library never starts automatic work. Keep definitions and past
/// results, while marking unfinished attempts as interrupted without replay.
pub(crate) fn pause_copied_tasks(connection: &Connection) -> StoreResult<()> {
    validate_saved_tasks(connection)?;
    connection.execute("UPDATE scheduled_tasks SET status=CASE WHEN status='active' THEN 'paused' ELSE status END,next_run_at_ms=NULL,data_json=json_set(data_json,'$.status',CASE WHEN status='active' THEN 'paused' ELSE status END,'$.next_run_at_ms',NULL)",[])?;
    connection.execute("UPDATE scheduled_task_runs SET status='interrupted',finished_at_ms=started_at_ms,data_json=json_set(data_json,'$.status','interrupted','$.finished_at_ms',started_at_ms,'$.error','Interrupted in the copied Loupist folder.') WHERE status='running'",[])?;
    Ok(())
}

pub(crate) fn validate_saved_tasks(connection: &Connection) -> StoreResult<()> {
    let mut statement =
        connection.prepare(&format!("SELECT {TASK_COLUMNS} FROM scheduled_tasks"))?;
    let rows = statement
        .query_map([], raw_task)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for row in rows {
        let task = decode_task(connection, row)?;
        let count = counts.entry(task.loupist_id).or_default();
        *count += 1;
        if *count > MAX_TASKS_PER_LOUPIST {
            return Err(invalid("saved Loupist exceeds the scheduled task limit"));
        }
    }
    let mut statement =
        connection.prepare(&format!("SELECT {RUN_COLUMNS} FROM scheduled_task_runs"))?;
    let rows = statement
        .query_map([], raw_run)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for row in rows {
        let run = decode_run(connection, row)?;
        let count = counts.entry(run.task_id).or_default();
        *count += 1;
        if *count > MAX_RUN_HISTORY {
            return Err(invalid("saved task exceeds the run history limit"));
        }
    }
    Ok(())
}
