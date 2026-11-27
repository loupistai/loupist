//! A direct user edit of a learning program from Schedule. The user typed the
//! change, so it needs no proposal. The patched task passes the same
//! `Task::validate` that `learning_program_save` reaches through the store, and
//! the write is the store's revision-checked compare-and-swap. A run already
//! started keeps its frozen snapshot; the next run reads the new fields.
use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::json;

use super::{
    schedule::next_run_after,
    service,
    types::{
        LearningConnections, LearningDepth, LearningKeeps, LearningScope, ReportPolicy, Schedule,
        Task, TaskDelivery, TaskKind, TaskStatus, TaskSubject,
    },
};
use crate::{
    app::{AppState, CommandError, CommandErrorCode},
    commands::now_ms,
    runs::maintenance::MaintenanceActivityKind,
    store::StoreError,
};

/// The fields the editor can change. An absent field stays as saved.
#[derive(Clone, Debug, Default, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProgramPatch {
    pub title: Option<String>,
    /// What to learn: the brief, sent as each run's request.
    pub prompt: Option<String>,
    pub subjects: Option<Vec<TaskSubject>>,
    pub schedule: Option<Schedule>,
    pub depth: Option<LearningDepth>,
    /// The connections a run may read and whether it may use the web.
    pub sources: Option<LearningScope>,
    pub keeps: Option<LearningKeeps>,
    pub report: Option<ReportPolicy>,
    pub delivery: Option<TaskDelivery>,
}

/// One editable field. The order is the order the patch is applied, so a
/// refusal names the first field that made the program invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Title,
    Prompt,
    Subjects,
    Schedule,
    Depth,
    Sources,
    Keeps,
    Report,
    Delivery,
}

impl Field {
    const fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Prompt => "prompt",
            Self::Subjects => "subjects",
            Self::Schedule => "schedule",
            Self::Depth => "depth",
            Self::Sources => "sources",
            Self::Keeps => "keeps",
            Self::Report => "report",
            Self::Delivery => "delivery",
        }
    }
}

impl ProgramPatch {
    fn is_empty(&self) -> bool {
        let Self {
            title,
            prompt,
            subjects,
            schedule,
            depth,
            sources,
            keeps,
            report,
            delivery,
        } = self;
        title.is_none()
            && prompt.is_none()
            && subjects.is_none()
            && schedule.is_none()
            && depth.is_none()
            && sources.is_none()
            && keeps.is_none()
            && report.is_none()
            && delivery.is_none()
    }

    /// Applies each present field that differs from `task` and that `keep`
    /// admits. Returns the fields it changed, in `Field` order.
    fn apply(&self, task: &mut Task, keep: impl Fn(Field) -> bool) -> Vec<Field> {
        let mut changed = Vec::new();
        let mut set = |field: Field, did: &dyn Fn(&mut Task) -> bool, task: &mut Task| {
            if keep(field) && did(task) {
                changed.push(field);
            }
        };
        set(
            Field::Title,
            &|task| replace(&mut task.title, self.title.clone()),
            task,
        );
        set(
            Field::Prompt,
            &|task| replace(&mut task.prompt, self.prompt.clone()),
            task,
        );
        set(
            Field::Subjects,
            &|task| replace(&mut task.watch.subjects, self.subjects.clone().map(Some)),
            task,
        );
        set(
            Field::Schedule,
            &|task| replace(&mut task.schedule, self.schedule.clone()),
            task,
        );
        set(
            Field::Depth,
            &|task| replace(&mut program_of(task).depth, self.depth),
            task,
        );
        set(
            Field::Sources,
            &|task| replace(&mut program_of(task).scope, self.sources.clone()),
            task,
        );
        set(
            Field::Keeps,
            &|task| replace(&mut program_of(task).keeps, self.keeps),
            task,
        );
        set(
            Field::Report,
            &|task| replace(&mut task.watch.report, self.report.map(Some)),
            task,
        );
        set(
            Field::Delivery,
            &|task| replace(&mut task.watch.delivery, self.delivery.map(Some)),
            task,
        );
        changed
    }

    /// The field a refusal of the whole patch names: the first changed field
    /// that is invalid on its own, else the first whose removal makes the
    /// patch valid (fields that conflict only together), else the first.
    fn refused_field(&self, current: &Task, changed: &[Field]) -> Field {
        let valid = |keep: &dyn Fn(Field) -> bool| {
            let mut task = current.clone();
            self.apply(&mut task, keep);
            task.validate().is_ok()
        };
        changed
            .iter()
            .copied()
            .find(|field| !valid(&|candidate| candidate == *field))
            .or_else(|| {
                changed
                    .iter()
                    .copied()
                    .find(|field| valid(&|candidate| candidate != *field))
            })
            .unwrap_or(changed[0])
    }
}

/// Puts `value` in `slot` when it is present and differs; says whether it did.
fn replace<T: PartialEq>(slot: &mut T, value: Option<T>) -> bool {
    match value {
        Some(value) if value != *slot => {
            *slot = value;
            true
        }
        _ => false,
    }
}

fn program_of(task: &mut Task) -> &mut super::types::LearningProgram {
    task.program
        .as_mut()
        .expect("update admits only a learning task, which carries its program")
}

/// `invalid_request` with `{field, reason}`: the editor shows `reason` under
/// the field it names.
fn field_error(field: Field, reason: impl Into<String>) -> CommandError {
    let reason = reason.into();
    CommandError::with_detail(
        CommandErrorCode::InvalidRequest,
        reason.clone(),
        json!({ "field": field.name(), "reason": reason }),
    )
}

fn stale() -> CommandError {
    CommandError::new(
        CommandErrorCode::StaleRevision,
        "This program changed. Reload it before editing.",
    )
}

/// The connections the Loupist grants a run, before a program narrows them:
/// the same set chat setup filters with `learning::allows_connection`. A saved
/// key still waiting for macOS permission is granted; the run only skips it.
fn granted_connections(
    state: &AppState,
    loupist_id: &str,
) -> Result<BTreeSet<String>, CommandError> {
    let authority = crate::commands::capture_chat_connector_authority(state, loupist_id)?;
    Ok(authority
        .capability_ids
        .into_iter()
        .chain(authority.saved_keys_needing_permission)
        .collect())
}

const WEB_RESEARCH: &str = "community.web-research";

/// Whether `next` reaches a connection the Loupist does not grant. What the
/// saved scope already holds is not a widening, so a connection the user
/// turned off later does not lock the program's other fields.
fn widens(current: &LearningScope, next: &LearningScope, granted: &BTreeSet<String>) -> bool {
    let held = |id: &str| match &current.connections {
        LearningConnections::All(_) => false,
        LearningConnections::Selected(ids) => ids.iter().any(|saved| saved == id),
    };
    let connections = match &next.connections {
        LearningConnections::All(_) => false,
        LearningConnections::Selected(ids) => {
            ids.iter().any(|id| !granted.contains(id) && !held(id))
        }
    };
    let web = next.web && !current.web && !granted.contains(WEB_RESEARCH);
    connections || web
}

/// Applies `patch` to the learning program `task_id` saved at `revision`.
pub(crate) fn update(
    state: &AppState,
    loupist_id: &str,
    task_id: &str,
    revision: u64,
    patch: ProgramPatch,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    if patch.is_empty() {
        return Err(CommandError::invalid("Change at least one field"));
    }
    let current = state
        .store
        .scheduled_task_get(loupist_id, task_id)
        .map_err(crate::commands::map_store)?
        .ok_or_else(|| CommandError::not_found("learning program"))?;
    if current.kind != TaskKind::Learning || current.program.is_none() {
        return Err(CommandError::invalid(
            "Only a learning program is edited here",
        ));
    }
    if current.revision != revision {
        return Err(stale());
    }
    let mut task = current.clone();
    let changed = patch.apply(&mut task, |_| true);
    if changed.is_empty() {
        return Ok(service::present(state, current));
    }
    if changed.contains(&Field::Sources) {
        let saved = &current.program.as_ref().expect("checked above").scope;
        let next = &task.program.as_ref().expect("checked above").scope;
        if widens(saved, next, &granted_connections(state, loupist_id)?) {
            return Err(field_error(
                Field::Sources,
                "A program can use only the connections this Loupist allows",
            ));
        }
    }
    if let Err(error) = task.validate() {
        return Err(field_error(
            patch.refused_field(&current, &changed),
            error.to_string(),
        ));
    }
    if task.schedule != current.schedule {
        task.next_run_at_ms = if task.status == TaskStatus::Active {
            let next = next_run_after(&task.schedule, now_ms())
                .map_err(|error| field_error(Field::Schedule, error.to_string()))?;
            Some(next.ok_or_else(|| {
                field_error(
                    Field::Schedule,
                    "an active one-time schedule must be in the future",
                )
            })?)
        } else {
            None
        };
    }
    let saved = state
        .store
        .scheduled_task_write(task, revision, now_ms())
        .map_err(|error| match error {
            StoreError::Conflict(_) => stale(),
            other => crate::commands::map_store(other),
        })?;
    service::saved_task(state, saved)
}

#[cfg(test)]
#[path = "program_edit/tests.rs"]
mod tests;
