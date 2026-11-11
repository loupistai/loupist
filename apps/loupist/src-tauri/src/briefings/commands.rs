use crate::{
    app::{AppState, CommandError},
    commands::{map_store, now_ms},
    runs::maintenance::MaintenanceActivityKind,
    scheduled_tasks::{
        service,
        types::{
            Run, RunStatus, Schedule, SkippedTool, Task, TaskDraft, TaskKind, TaskStatus,
            TaskTarget,
        },
    },
};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "BriefingsLoupistRequest"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LoupistRequest {
    pub loupist_id: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable), ts(rename = "BriefingSettingsRequest"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsRequest {
    pub loupist_id: String,
    pub enabled: bool,
    #[serde(default, deserialize_with = "exact_schedule")]
    #[cfg_attr(test, ts(as = "Option<Schedule>"))]
    pub schedule: Option<Schedule>,
}

/// A schedule as the renderer sends it, checked exactly. Saved schedules
/// (`Schedule`, in tasks and Chat receipts) are read leniently instead.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ExactSchedule {
    Once {
        at_ms: i64,
    },
    Interval {
        every_minutes: u32,
    },
    Daily {
        hour: u8,
        minute: u8,
        timezone: String,
    },
    Weekly {
        days: Vec<u8>,
        hour: u8,
        minute: u8,
        timezone: String,
    },
}

fn exact_schedule<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Schedule>, D::Error> {
    Ok(
        Option::<ExactSchedule>::deserialize(deserializer)?.map(|exact| match exact {
            ExactSchedule::Once { at_ms } => Schedule::Once { at_ms },
            ExactSchedule::Interval { every_minutes } => Schedule::Interval { every_minutes },
            ExactSchedule::Daily {
                hour,
                minute,
                timezone,
            } => Schedule::Daily {
                hour,
                minute,
                timezone,
            },
            ExactSchedule::Weekly {
                days,
                hour,
                minute,
                timezone,
            } => Schedule::Weekly {
                days,
                hour,
                minute,
                timezone,
            },
        }),
    )
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct BriefingSettings {
    pub enabled: bool,
    pub schedule: Schedule,
    pub task: Option<Task>,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InboxRequest {
    pub loupist_id: String,
    pub cursor: Option<String>,
    pub limit: usize,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadRequest {
    pub loupist_id: String,
    pub run_ids: Vec<String>,
}
#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxItem {
    pub briefing_context: Option<super::BriefingContext>,
    pub error: Option<String>,
    pub run_id: String,
    pub task_id: String,
    pub kind: TaskKind,
    pub title: String,
    pub status: RunStatus,
    pub summary: String,
    pub summary_items: Vec<crate::scheduled_tasks::types::RunSummaryItem>,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub read_at_ms: Option<i64>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub skipped_tools: Vec<SkippedTool>,
}
impl From<Run> for InboxItem {
    fn from(r: Run) -> Self {
        Self {
            briefing_context: r.briefing_context,
            error: r.error,
            run_id: r.run_id,
            task_id: r.task_id,
            kind: r.kind,
            title: r.task_title,
            status: r.status,
            summary: r.summary,
            summary_items: r.summary_items,
            started_at_ms: r.started_at_ms,
            finished_at_ms: r.finished_at_ms,
            read_at_ms: r.read_at_ms,
            thread_id: r.thread_id,
            message_id: r.message_id,
            skipped_tools: r.skipped_tools,
        }
    }
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxPage {
    pub items: Vec<InboxItem>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnreadCount {
    pub loupist_id: String,
    pub count: u64,
}

pub(crate) fn settings_get(
    state: &AppState,
    loupist_id: &str,
) -> Result<BriefingSettings, CommandError> {
    service::ensure_visible(state, loupist_id)?;
    let task = state
        .store
        .scheduled_tasks_list(loupist_id)
        .map_err(map_store)?
        .into_iter()
        .find(|t| t.kind == TaskKind::Briefing);
    let schedule = match &task {
        Some(task) => task.schedule.clone(),
        None => super::default_schedule(
            iana_time_zone::get_timezone()
                .map_err(|_| CommandError::internal("System time zone is unavailable"))?,
        ),
    };
    Ok(BriefingSettings {
        enabled: task
            .as_ref()
            .is_some_and(|t| t.status == TaskStatus::Active),
        schedule,
        task,
    })
}

pub(crate) fn settings_set(
    state: &AppState,
    request: SettingsRequest,
) -> Result<BriefingSettings, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    crate::commands::watch::consent_mode(state, &request.loupist_id)?;
    let previous = settings_get(state, &request.loupist_id)?;
    // A schedule saved while briefings are off still needs a (paused) task to live in.
    let create = request.enabled || request.schedule.is_some();
    let schedule = request.schedule.unwrap_or(previous.schedule);
    super::super::scheduled_tasks::schedule::validate_schedule(&schedule).map_err(map_store)?;
    let status = if request.enabled {
        TaskStatus::Active
    } else {
        TaskStatus::Paused
    };
    if let Some(task) = previous.task {
        let mut draft = task.draft();
        draft.schedule = schedule;
        draft.status = status;
        state
            .store
            .scheduled_task_update(
                &request.loupist_id,
                &task.id,
                task.revision,
                draft,
                now_ms(),
            )
            .map_err(map_store)?;
    } else if create {
        let config = crate::commands::loupist_configuration(state, &request.loupist_id)?;
        let profile = state
            .store
            .get_loupist(&request.loupist_id)
            .map_err(map_store)?
            .ok_or_else(|| CommandError::not_found("Loupist"))?;
        let catalog = state.model_catalog()?;
        // Pick what a new Chat would: the configured model, the Loupist's last visible model, then the
        // first model in the picker. Raw catalog order can start with a provider the user never connected.
        let view = state.model_catalog_view()?;
        let shown = |id: &str| {
            view.models.iter().any(|model| model.id == id)
                && !view
                    .hidden_model_profile_ids
                    .iter()
                    .any(|hidden| hidden == id)
        };
        let model_id = config
            .chat
            .model_profile_id
            .or(profile.last_model_profile_id.filter(|id| shown(id)))
            .or_else(|| {
                view.models
                    .iter()
                    .find(|model| shown(&model.id))
                    .map(|model| model.id.clone())
            })
            .ok_or_else(|| {
                CommandError::invalid("Configure a model before setting up briefings")
            })?;
        let model = catalog.resolve(&model_id)?;
        let thinking = config
            .chat
            .thinking_level
            .filter(|level| model.thinking_levels.contains(level))
            .unwrap_or_else(|| {
                if model.thinking_levels.iter().any(|l| l == "medium") {
                    "medium".into()
                } else {
                    model.thinking_levels[0].clone()
                }
            });
        let run_options = model
            .run_controls
            .iter()
            .map(|control| {
                let choice = config
                    .chat
                    .run_options
                    .get(&control.id)
                    .filter(|choice| {
                        control
                            .choices
                            .iter()
                            .any(|candidate| candidate.id == **choice)
                    })
                    .cloned()
                    .unwrap_or_else(|| control.default_choice.clone());
                (control.id.clone(), choice)
            })
            .collect();
        let draft = TaskDraft {
            title: "Briefings".into(),
            prompt: super::PROMPT.into(),
            schedule,
            target: TaskTarget::Dedicated,
            model_profile_id: model_id,
            thinking_level: thinking,
            run_options,
            use_memory: true,
            update_memory: true,
            status,
        };
        // No provider credential is required at opt-in; ordinary admission owns
        // readiness, including immediately after loupistCreate.
        state
            .store
            .scheduled_task_create_kind(
                &request.loupist_id,
                &format!("briefing-{}", request.loupist_id),
                draft,
                TaskKind::Briefing,
                now_ms(),
            )
            .map_err(map_store)?;
    }
    if request.enabled {
        super::desktop::enable(state)?;
    }
    super::desktop::changed(&request.loupist_id);
    settings_get(state, &request.loupist_id)
}

pub(crate) fn unread_counts(state: &AppState) -> Result<Vec<UnreadCount>, CommandError> {
    state
        .store
        .list_loupists()
        .map_err(map_store)?
        .into_iter()
        // A Loupist whose folder is offline has no readable inbox; it must
        // not hide every other Loupist's count.
        .map(|loupist| {
            Ok(UnreadCount {
                count: state.store.inbox_unread_count(&loupist.id).unwrap_or(0),
                loupist_id: loupist.id,
            })
        })
        .collect()
}

#[tauri::command]
pub(crate) fn briefing_settings_get(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<BriefingSettings, CommandError> {
    settings_get(&state, &request.loupist_id)
}
#[tauri::command]
pub(crate) fn briefing_settings_set(
    request: SettingsRequest,
    state: State<'_, AppState>,
) -> Result<BriefingSettings, CommandError> {
    settings_set(&state, request)
}
#[tauri::command]
pub(crate) fn inbox_list(
    request: InboxRequest,
    state: State<'_, AppState>,
) -> Result<InboxPage, CommandError> {
    service::ensure_visible(&state, &request.loupist_id)?;
    let (runs, next_cursor) = state
        .store
        .inbox_page(
            &request.loupist_id,
            request.cursor.as_deref(),
            request.limit,
        )
        .map_err(map_store)?;
    Ok(InboxPage {
        items: runs.into_iter().map(Into::into).collect(),
        next_cursor,
    })
}
#[tauri::command]
pub(crate) fn inbox_mark_read(
    request: ReadRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    state
        .store
        .inbox_mark_read(&request.loupist_id, &request.run_ids, now_ms())
        .map_err(map_store)?;
    super::desktop::changed(&request.loupist_id);
    Ok(())
}
#[tauri::command]
pub(crate) fn inbox_unread_counts(
    state: State<'_, AppState>,
) -> Result<Vec<UnreadCount>, CommandError> {
    unread_counts(&state)
}
