use std::collections::{BTreeMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::surface::ChatSurface;
use crate::{
    app::{AppState, CommandError},
    commands::{
        chat_runtime::{send_chat, ChatAdmission, ChatEventSink},
        map_internal, map_store, new_id, now_ms,
        types::{ChatSendRequest, ChatStreamEvent},
    },
    domain::{ChatMessage, ChatSession, HomeHandoff, MemoryPolicy},
    store::{StoreError, StoreResult},
};

pub(crate) const DDL: &str = r#"
CREATE TABLE chat_handoffs (
    id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    request_key TEXT NOT NULL,
    chat_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    run_id TEXT,
    created_at_ms INTEGER NOT NULL,
    data_json TEXT NOT NULL,
    UNIQUE (loupist_id, request_key)
) STRICT;
CREATE INDEX chat_handoffs_by_loupist ON chat_handoffs(loupist_id, created_at_ms ASC);
"#;

pub(crate) const MAX_TITLE_CHARS: usize = 120;
pub(crate) const MAX_QUESTION_CHARS: usize = 2_000;
pub(crate) const MAX_BRIEF_CHARS: usize = 8_000;
pub(crate) const MAX_PINS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    Created,
    Running,
    Done,
    Failed,
    Stopped,
}

impl HandoffStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum HandoffPin {
    Lens { lens: String },
    World { world: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffRequest {
    pub title: String,
    pub question: String,
    pub brief: String,
    pub pins: Vec<HandoffPin>,
    pub request_key: String,
}

fn text_field(
    value: &Value,
    field: &str,
    max_chars: usize,
    required: bool,
) -> Result<String, (String, String)> {
    let text = match value.get(field) {
        None | Some(Value::Null) if !required => return Ok(String::new()),
        Some(Value::String(text)) => text.trim(),
        _ => {
            return Err((
                field.into(),
                format!("a string of 1 to {max_chars} characters"),
            ))
        }
    };
    if (required && text.is_empty()) || text.chars().count() > max_chars || text.contains('\0') {
        return Err((
            field.into(),
            format!("a string of 1 to {max_chars} characters"),
        ));
    }
    Ok(text.to_owned())
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl HandoffRequest {
    pub(crate) fn parse(value: &Value) -> Result<Self, (String, String)> {
        let object = value.as_object().ok_or_else(|| {
            (
                "arguments".to_owned(),
                "an object with title, question, brief, pins and requestKey".to_owned(),
            )
        })?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "title" | "question" | "brief" | "pins" | "requestKey"
            ) {
                return Err((key.clone(), "no such argument".into()));
            }
        }
        let title = text_field(value, "title", MAX_TITLE_CHARS, true)?;
        let question = text_field(value, "question", MAX_QUESTION_CHARS, true)?;
        let brief = text_field(value, "brief", MAX_BRIEF_CHARS, false)?;
        let request_key = text_field(value, "requestKey", 64, true)?;
        if !identifier(&request_key, 64) {
            return Err((
                "requestKey".into(),
                "1 to 64 letters, digits, hyphens or underscores; distinct per hand-off, reused only on a retry".into(),
            ));
        }
        let pins = match value.get("pins") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) if items.len() <= MAX_PINS => items
                .iter()
                .map(|item| {
                    serde_json::from_value::<HandoffPin>(item.clone())
                        .ok()
                        .filter(|pin| match pin {
                            HandoffPin::Lens { lens } => identifier(lens, 128),
                            HandoffPin::World { world } => identifier(world, 128),
                        })
                        .ok_or_else(|| {
                            (
                                "pins".to_owned(),
                                format!("at most {MAX_PINS} items of {{lens: recordId}} or {{world: nodeId}}"),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => {
                return Err((
                    "pins".into(),
                    format!("at most {MAX_PINS} items of {{lens: recordId}} or {{world: nodeId}}"),
                ))
            }
        };
        Ok(Self {
            title,
            question,
            brief,
            pins,
            request_key,
        })
    }

    pub(crate) fn first_message(&self) -> String {
        let mut text = self.question.clone();
        if !self.brief.is_empty() {
            text.push_str("\n\n");
            text.push_str(&self.brief);
        }
        if !self.pins.is_empty() {
            text.push_str("\n\nRead these first:");
            for pin in &self.pins {
                match pin {
                    HandoffPin::Lens { lens } => text.push_str(&format!("\n- lens_read {lens}")),
                    HandoffPin::World { world } => {
                        text.push_str(&format!("\n- world_read node {world}"))
                    }
                }
            }
        }
        text
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffRun {
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StagedHandoff {
    pub request: HandoffRequest,
    pub run: HandoffRun,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Handoff {
    pub id: String,
    pub loupist_id: String,
    pub request_key: String,
    pub chat_id: String,
    pub title: String,
    pub prompt: String,
    pub run: HandoffRun,
    pub status: HandoffStatus,
    pub run_id: Option<String>,
    pub message_id: Option<String>,
    pub error: Option<String>,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

fn invalid(message: &str) -> StoreError {
    StoreError::Invalid(message.into())
}

fn load_all(connection: &Connection, loupist_id: &str) -> StoreResult<Vec<Handoff>> {
    let mut statement = connection.prepare(
        "SELECT data_json FROM chat_handoffs WHERE loupist_id=?1 ORDER BY created_at_ms ASC, id ASC",
    )?;
    let rows = statement.query_map([loupist_id], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        serde_json::from_str::<Handoff>(&row?).map_err(|error| invalid(&error.to_string()))
    })
    .collect()
}

fn load(connection: &Connection, loupist_id: &str, id: &str) -> StoreResult<Option<Handoff>> {
    connection
        .query_row(
            "SELECT data_json FROM chat_handoffs WHERE loupist_id=?1 AND id=?2",
            params![loupist_id, id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|body| serde_json::from_str(&body).map_err(|error| invalid(&error.to_string())))
        .transpose()
}

fn save(connection: &Connection, handoff: &Handoff) -> StoreResult<()> {
    connection.execute(
        "UPDATE chat_handoffs SET status=?3, run_id=?4, data_json=?5 WHERE loupist_id=?1 AND id=?2",
        params![
            handoff.loupist_id,
            handoff.id,
            handoff.status.as_str(),
            handoff.run_id,
            serde_json::to_string(handoff)?
        ],
    )?;
    Ok(())
}

pub(crate) fn open_for_chat(
    state: &AppState,
    loupist_id: &str,
    chat_id: &str,
) -> Result<Option<String>, CommandError> {
    let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    connection
        .query_row(
            "SELECT id FROM chat_handoffs WHERE loupist_id=?1 AND chat_id=?2 AND status IN ('created','running')",
            params![loupist_id, chat_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_internal)
}

pub(crate) fn commit(
    transaction: &Connection,
    loupist_id: &str,
    staged: &StagedHandoff,
    now: i64,
) -> StoreResult<Handoff> {
    let existing = transaction
        .query_row(
            "SELECT data_json FROM chat_handoffs WHERE loupist_id=?1 AND request_key=?2",
            params![loupist_id, staged.request.request_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(body) = existing {
        return serde_json::from_str(&body).map_err(|error| invalid(&error.to_string()));
    }
    let chat = ChatSession {
        id: new_id("chat"),
        loupist_id: loupist_id.into(),
        pi_session_id: Uuid::new_v4().to_string(),
        pi_session_cache: None,
        title: staged.request.title.clone(),
        memory_policy: MemoryPolicy {
            use_memory: staged.run.use_memory,
            update_memory: staged.run.update_memory,
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
    transaction.execute(
        "INSERT INTO chat_sessions(id,loupist_id,updated_at_ms,data_json) VALUES(?1,?2,?3,?4)",
        params![
            chat.id,
            chat.loupist_id,
            chat.updated_at_ms,
            serde_json::to_string(&chat)?
        ],
    )?;
    let handoff = Handoff {
        id: new_id("handoff"),
        loupist_id: loupist_id.into(),
        request_key: staged.request.request_key.clone(),
        chat_id: chat.id,
        title: staged.request.title.clone(),
        prompt: staged.request.first_message(),
        run: staged.run.clone(),
        status: HandoffStatus::Created,
        run_id: None,
        message_id: None,
        error: None,
        created_at_ms: now,
        finished_at_ms: None,
    };
    transaction.execute(
        "INSERT INTO chat_handoffs(id,loupist_id,request_key,chat_id,status,run_id,created_at_ms,data_json) VALUES(?1,?2,?3,?4,?5,NULL,?6,?7)",
        params![
            handoff.id,
            handoff.loupist_id,
            handoff.request_key,
            handoff.chat_id,
            handoff.status.as_str(),
            handoff.created_at_ms,
            serde_json::to_string(&handoff)?
        ],
    )?;
    Ok(handoff)
}

pub(crate) fn commit_standalone(
    state: &AppState,
    loupist_id: &str,
    staged: &StagedHandoff,
) -> Result<Handoff, CommandError> {
    let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_internal)?;
    let handoff = commit(&transaction, loupist_id, staged, now_ms()).map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(handoff)
}

pub(crate) fn list(state: &AppState, loupist_id: &str) -> Result<Vec<Handoff>, CommandError> {
    let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    load_all(&connection, loupist_id).map_err(map_store)
}

fn mark_running(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    run_id: &str,
) -> Result<bool, CommandError> {
    let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_internal)?;
    let Some(mut handoff) = load(&transaction, loupist_id, id).map_err(map_store)? else {
        return Ok(false);
    };
    if handoff.status != HandoffStatus::Created {
        return Ok(false);
    }
    handoff.status = HandoffStatus::Running;
    handoff.run_id = Some(run_id.to_owned());
    save(&transaction, &handoff).map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(true)
}

fn bounded_error(value: &str) -> String {
    value.replace('\0', "").chars().take(2_000).collect()
}

pub(crate) fn finish(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    status: HandoffStatus,
    message_id: Option<String>,
    error: Option<String>,
    now: i64,
) -> Result<(), CommandError> {
    let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_internal)?;
    let Some(mut handoff) = load(&transaction, loupist_id, id).map_err(map_store)? else {
        return Ok(());
    };
    if handoff.status != HandoffStatus::Running {
        return Ok(());
    }
    handoff.status = status;
    handoff.message_id = message_id;
    handoff.error = error.as_deref().map(bounded_error);
    handoff.finished_at_ms = Some(now);
    save(&transaction, &handoff).map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(())
}

fn send_created(state: &AppState, loupist_id: &str) {
    let state = state.clone();
    let loupist_id = loupist_id.to_owned();
    tauri::async_runtime::spawn(async move {
        match launch_created(&state, &loupist_id).await {
            Ok(0) => {}
            Ok(_) => {
                let _ = super::home::deliver_finished_runs(&state, &loupist_id);
            }
            Err(error) => eprintln!("Research hand-off could not start ({error})"),
        }
    });
}

async fn launch_created(state: &AppState, loupist_id: &str) -> Result<usize, CommandError> {
    let mut launched = 0;
    for handoff in list(state, loupist_id)? {
        if handoff.status != HandoffStatus::Created {
            continue;
        }
        match launch(state, &handoff).await {
            Ok(true) => launched += 1,
            Ok(false) => {}
            Err(error) => eprintln!("Research hand-off {} waits ({error})", handoff.id),
        }
    }
    Ok(launched)
}

async fn launch(state: &AppState, handoff: &Handoff) -> Result<bool, CommandError> {
    // Short: the run id names the broker's Unix socket, which allows 103 bytes.
    let run_id = format!("handoff-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let admission = ChatAdmission::new(state, &run_id, &handoff.loupist_id, &handoff.chat_id)?;
    if !mark_running(state, &handoff.loupist_id, &handoff.id, &run_id)? {
        return Ok(false);
    }
    let sink = {
        let state = state.clone();
        let loupist_id = handoff.loupist_id.clone();
        let id = handoff.id.clone();
        ChatEventSink::new(move |event| {
            let outcome = match &event {
                ChatStreamEvent::Completed { message_id, .. } => {
                    Some((HandoffStatus::Done, Some(message_id.clone()), None))
                }
                ChatStreamEvent::Aborted { .. } => Some((
                    HandoffStatus::Stopped,
                    None,
                    Some("Stopped before completion".to_owned()),
                )),
                ChatStreamEvent::Error { message, .. } => {
                    Some((HandoffStatus::Failed, None, Some(message.clone())))
                }
                _ => None,
            };
            if let Some((status, message_id, error)) = outcome {
                if let Err(error) = finish(
                    &state,
                    &loupist_id,
                    &id,
                    status,
                    message_id,
                    error,
                    now_ms(),
                ) {
                    eprintln!("Research hand-off outcome could not be recorded ({error})");
                }
            }
            Ok(())
        })
    };
    let request = ChatSendRequest {
        run_id,
        loupist_id: handoff.loupist_id.clone(),
        thread_id: handoff.chat_id.clone(),
        prompt: handoff.prompt.clone(),
        use_memory: handoff.run.use_memory,
        update_memory: handoff.run.update_memory,
        as_of: None,
        model_profile_id: handoff.run.model_profile_id.clone(),
        thinking_level: handoff.run.thinking_level.clone(),
        run_options: handoff.run.run_options.clone(),
        mentioned_connection_ids: Vec::new(),
        attachments: Vec::new(),
        pins: Vec::new(),
        selected_artifact: None,
    };
    if let Err(error) = send_chat(request, sink, state, Some(admission)).await {
        finish(
            state,
            &handoff.loupist_id,
            &handoff.id,
            HandoffStatus::Failed,
            None,
            Some(error.message),
            now_ms(),
        )?;
        ChatSurface::Delegated {
            handoff_id: handoff.id.clone(),
        }
        .after_turn(state, &handoff.loupist_id);
    }
    Ok(true)
}

pub(crate) fn for_delivery(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<Handoff>, CommandError> {
    let rows = list(state, loupist_id)?;
    let live = state
        .run_coordinator
        .activities()?
        .into_iter()
        .map(|activity| activity.run_id)
        .collect::<std::collections::HashSet<_>>();
    delivery_rows(state, loupist_id, rows, &live)
}

fn delivery_rows(
    state: &AppState,
    loupist_id: &str,
    rows: Vec<Handoff>,
    live: &HashSet<String>,
) -> Result<Vec<Handoff>, CommandError> {
    if rows.iter().any(|row| row.status == HandoffStatus::Created) {
        send_created(state, loupist_id);
    }
    let mut shown = Vec::new();
    for mut handoff in rows {
        if handoff.status == HandoffStatus::Created {
            continue;
        }
        if handoff.status == HandoffStatus::Running
            && handoff
                .run_id
                .as_ref()
                .is_none_or(|run| !live.contains(run))
        {
            finish(
                state,
                loupist_id,
                &handoff.id,
                HandoffStatus::Stopped,
                None,
                Some("The app stopped before the session finished".into()),
                now_ms(),
            )?;
            // `finish` only transitions Running rows: completion may have won
            // since the initial list. Deliver that stored outcome, not our snapshot.
            let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
            let Some(current) = load(&connection, loupist_id, &handoff.id).map_err(map_store)?
            else {
                continue;
            };
            handoff = current;
        }
        shown.push(handoff);
    }
    Ok(shown)
}

/// What the first of these messages that filed anything filed; `None` when
/// none did.
pub(crate) fn learned_digest(
    state: &AppState,
    loupist_id: &str,
    message_ids: &[String],
) -> Result<Option<crate::knowledge::LearnedDigest>, CommandError> {
    Ok(
        crate::commands::knowledge::learned_digests(state, loupist_id, message_ids)?
            .into_iter()
            .find(|digest| !digest.pages.is_empty()),
    )
}

/// A card's digest: a failed read is logged and the card renders without one.
pub(crate) fn card_learned(
    state: &AppState,
    loupist_id: &str,
    message_id: &str,
) -> Option<crate::knowledge::LearnedDigest> {
    learned_digest(state, loupist_id, &[message_id.to_owned()]).unwrap_or_else(|error| {
        eprintln!("What a run learned could not be read ({error})");
        None
    })
}

fn summary(state: &AppState, handoff: &Handoff) -> String {
    let Some(message_id) = &handoff.message_id else {
        return String::new();
    };
    state
        .store
        .get_chat(&handoff.chat_id)
        .ok()
        .flatten()
        .and_then(|chat| {
            chat.messages
                .into_iter()
                .find(|message| &message.id == message_id)
        })
        .map(|message| crate::briefings::summary(&message.content))
        .unwrap_or_default()
}

pub(crate) fn message(
    state: &AppState,
    handoff: &Handoff,
    existing: Option<&ChatMessage>,
) -> Result<ChatMessage, CommandError> {
    let content = match handoff.status {
        HandoffStatus::Done => {
            let summary = summary(state, handoff);
            if summary.is_empty() {
                format!("Researched: {}.", handoff.title)
            } else {
                summary
            }
        }
        HandoffStatus::Running | HandoffStatus::Created => {
            format!("Researching: {}.", handoff.title)
        }
        HandoffStatus::Failed | HandoffStatus::Stopped => {
            format!("Research on {} could not finish.", handoff.title)
        }
    };
    let mut message = match existing {
        Some(existing) => {
            let mut message = existing.clone();
            message.content = content;
            message
        }
        None => super::home::host_message("handoff", content, handoff.created_at_ms)?,
    };
    let learned = handoff
        .message_id
        .as_ref()
        .filter(|_| handoff.status == HandoffStatus::Done)
        .and_then(|id| card_learned(state, &handoff.loupist_id, id));
    message.handoff = Some(HomeHandoff {
        handoff_id: handoff.id.clone(),
        chat_id: handoff.chat_id.clone(),
        title: handoff.title.clone(),
        status: handoff.status,
        learned,
    });
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::coordinator::{RunKind, RunTarget};
    use serde_json::json;

    fn state_with_loupist(id: &str) -> (tempfile::TempDir, AppState) {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let bound =
            crate::fs::loupist_root::BoundLoupistRoot::open_unbound(workspace.clone()).unwrap();
        let state = AppState::for_test(root.path().join("app"));
        state
            .store
            .create_loupist(&crate::domain::LoupistProfile {
                workspace_root: workspace.to_string_lossy().into_owned(),
                root_filesystem_identity: bound.identity(),
                ..crate::test_fixtures::loupist_profile(id)
            })
            .unwrap();
        super::super::home::ensure_home_thread(&state, id).unwrap();
        (root, state)
    }

    #[test]
    fn every_host_write_of_a_hand_off_reaches_the_renderer() {
        let loupist = "loupist-handoff-signal";
        let (_root, state) = state_with_loupist(loupist);
        let staged = StagedHandoff {
            request: HandoffRequest::parse(&json!({
                "title": "Hynix HBM supply",
                "question": "Who else can supply HBM4 in 2027?",
                "requestKey": "hbm-2027"
            }))
            .unwrap(),
            run: HandoffRun {
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: BTreeMap::new(),
                use_memory: false,
                update_memory: false,
            },
        };
        let handoff = commit_standalone(&state, loupist, &staged).unwrap();
        let run = state
            .register_run(
                "handoff-signal".into(),
                loupist.into(),
                RunKind::Chat,
                RunTarget::ChatThread(handoff.chat_id.clone()),
            )
            .unwrap();
        assert!(mark_running(&state, loupist, &handoff.id, "handoff-signal").unwrap());
        let told = || crate::briefings::desktop::changed_count(loupist);
        let deliver = || super::super::home::deliver_finished_runs(&state, loupist).unwrap();

        // A pass from any caller (opening the Loupist runs one) posts the card and says so.
        assert_eq!(deliver(), 1);
        assert_eq!(told(), 1);

        let stale_running = list(&state, loupist).unwrap();
        finish(
            &state,
            loupist,
            &handoff.id,
            HandoffStatus::Done,
            None,
            None,
            now_ms(),
        )
        .unwrap();
        drop(run);
        let reconciled = delivery_rows(&state, loupist, stale_running, &HashSet::new()).unwrap();
        assert_eq!(
            reconciled[0].status,
            HandoffStatus::Done,
            "completion won after the initial row read"
        );
        assert_eq!(deliver(), 1, "the running card turns done in place");
        assert_eq!(told(), 2);
        assert_eq!(deliver(), 0);
        assert_eq!(told(), 2, "a pass that writes nothing stays quiet");

        // The run's end changed its own session even though the Bot has nothing new.
        ChatSurface::Delegated {
            handoff_id: handoff.id.clone(),
        }
        .after_turn(&state, loupist);
        assert_eq!(told(), 3);
    }

    #[test]
    fn home_delivery_reports_pending_news_read_errors_without_posting() {
        for table in [
            "knowledge_earnings_notices",
            "knowledge_watch_events",
            "knowledge_schema_changes",
            "knowledge_connections",
            "knowledge_asset_pages",
        ] {
            let id = "loupist-news-read-failure";
            let (_root, state) = state_with_loupist(id);
            let before = super::super::home::ensure_home_thread(&state, id).unwrap();
            state
                .store
                .lock_loupist(id)
                .unwrap()
                .execute_batch(&format!("DROP TABLE {table}"))
                .unwrap();
            assert!(
                super::super::home::deliver_finished_runs(&state, id).is_err(),
                "{table}"
            );
            let after = state.store.get_chat(&before.id).unwrap().unwrap();
            assert_eq!(after.messages, before.messages, "{table}");
        }
    }

    #[test]
    fn a_failed_earnings_snapshot_does_not_block_handoff_delivery() {
        let id = "loupist-snapshot-failure";
        let (_root, state) = state_with_loupist(id);
        state
            .store
            .watchlist_add("MORR", "NASDAQ", Some("Morrow"))
            .unwrap();
        let page = crate::commands::watch::followed_pages(&state, id)
            .unwrap()
            .remove(0);
        crate::commands::assets::with_assets(&state, id, true, |tx| {
            let (event, _) = crate::knowledge::earnings::upsert_event(
                tx,
                id,
                &page.asset_id,
                "Q3 2026",
                None,
                chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
                crate::knowledge::earnings::DateSource::Calendar,
                1,
            )?;
            crate::knowledge::earnings::add_notice(
                tx,
                id,
                &event.event_id,
                "reported",
                Some("after"),
                1,
            )
        })
        .unwrap();
        state
            .store
            .lock_loupist(id)
            .unwrap()
            .execute_batch("DROP TABLE knowledge_earnings_rows")
            .unwrap();
        let handoff = commit_standalone(&state, id, &StagedHandoff {
            request: HandoffRequest::parse(&json!({"title": "Research", "question": "What changed?", "requestKey": "snapshot-failure"})).unwrap(),
            run: HandoffRun { model_profile_id: "fixture".into(), thinking_level: "medium".into(), run_options: BTreeMap::new(), use_memory: false, update_memory: false },
        }).unwrap();
        mark_running(&state, id, &handoff.id, "finished-fixture").unwrap();
        finish(
            &state,
            id,
            &handoff.id,
            HandoffStatus::Done,
            None,
            None,
            now_ms(),
        )
        .unwrap();
        assert!(super::super::home::deliver_finished_runs(&state, id).unwrap() > 0);
        let home = super::super::home::ensure_home_thread(&state, id).unwrap();
        assert!(home
            .messages
            .iter()
            .any(
                |message| message.handoff.as_ref().is_some_and(|card| card.handoff_id
                    == handoff.id
                    && card.status == HandoffStatus::Done)
            ));
        crate::commands::assets::with_assets(&state, id, false, |tx| {
            assert_eq!(
                crate::knowledge::earnings::pending_notices(tx, id)?.len(),
                1
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn parse_checks_each_field_and_renders_the_first_message() {
        let request = HandoffRequest::parse(&json!({
            "title": "Hynix HBM supply",
            "question": "Who else can supply HBM4 in 2027?",
            "brief": "Check capacity announcements.",
            "pins": [{"lens": "rec_a"}, {"world": "node_b"}],
            "requestKey": "hbm-2027"
        }))
        .unwrap();
        assert_eq!(
            request.first_message(),
            "Who else can supply HBM4 in 2027?\n\nCheck capacity announcements.\n\nRead these first:\n- lens_read rec_a\n- world_read node node_b"
        );
        for (arguments, field) in [
            (json!({"question": "q", "requestKey": "k"}), "title"),
            (json!({"title": "t", "requestKey": "k"}), "question"),
            (json!({"title": "t", "question": "q"}), "requestKey"),
            (
                json!({"title": "t", "question": "q", "requestKey": "bad key"}),
                "requestKey",
            ),
            (
                json!({"title": "t", "question": "q", "requestKey": "k", "pins": [{"note": "x"}]}),
                "pins",
            ),
            (
                json!({"title": "t", "question": "q", "requestKey": "k", "extra": 1}),
                "extra",
            ),
            (
                json!({"title": "t".repeat(MAX_TITLE_CHARS + 1), "question": "q", "requestKey": "k"}),
                "title",
            ),
        ] {
            assert_eq!(HandoffRequest::parse(&arguments).unwrap_err().0, field);
        }
    }
}
