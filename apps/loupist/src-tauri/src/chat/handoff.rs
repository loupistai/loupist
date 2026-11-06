//! A research hand-off from the Bot. `research_open` stages it; the Bot's
//! reply commits one `chat_handoffs` row and the session Chat in its own
//! transaction; the host then runs that session once, unattended, and posts
//! the result into the Bot. The row is the hand-off's whole lifecycle:
//! `created` (committed, not yet sent), `running`, then `done`, `failed` or
//! `stopped`. A run that stopped is reported, never sent again.

use std::collections::BTreeMap;

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
    store::{LoupistStore, StoreError, StoreResult},
};

pub(crate) const DDL: &str = r#"
CREATE TABLE chat_handoffs (
    id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    request_key TEXT NOT NULL,
    chat_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('created','running','done','failed','stopped')),
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
/// How much of a finished session's answer the Bot shows.
const SUMMARY_CHARS: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Memory the session should read first: a Lens record or a World node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum HandoffPin {
    Lens { lens: String },
    World { world: String },
}

/// What the Bot asks a session to do, checked at the tool and bridge boundary.
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
    /// Validates the tool's or the bridge's arguments; an error names the field
    /// and what it expects.
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

    /// The session's first user message: the question, the brief and the pins
    /// as read targets, so the session's first turn sees all three.
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

/// The run settings a hand-off inherits from the Bot turn that staged it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffRun {
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
}

/// A hand-off the broker accepted; it becomes a row when the reply commits.
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
    /// The session's first user message, sent when the run starts.
    pub prompt: String,
    pub run: HandoffRun,
    pub status: HandoffStatus,
    pub run_id: Option<String>,
    /// The session's final reply once the run finished.
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

/// The hand-off this Chat is running for, if any.
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

/// Commits a staged hand-off inside the Bot reply's transaction: the row and
/// the session Chat. A request key already used returns that hand-off and
/// creates nothing. The one way to delegate, for the tool and the bridge.
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

/// Commits a hand-off outside a Chat turn (the bridge's `research_handoff_start`).
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

/// Moves `created` to `running` once the run is registered. False when another
/// pass already sent it.
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
    // A run reports its end once; a later signal cannot rewrite it.
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

/// Its own task: a hand-off starts a Chat turn, and a delivery pass runs where
/// a Chat turn ends. The sessions that started reach the Bot on the pass after.
fn send_created(state: &AppState, loupist_id: &str) {
    let state = state.clone();
    let loupist_id = loupist_id.to_owned();
    tauri::async_runtime::spawn(async move {
        match launch_created(&state, &loupist_id).await {
            Ok(0) => {}
            Ok(_) => super::surface::deliver(&state, &loupist_id),
            Err(error) => eprintln!("Research hand-off could not start ({error})"),
        }
    });
}

/// Sends every committed hand-off that has not started: a first send after a
/// crash between commit and launch, never a retry of a run that started.
async fn launch_created(state: &AppState, loupist_id: &str) -> Result<usize, CommandError> {
    let mut launched = 0;
    for handoff in list(state, loupist_id)? {
        if handoff.status != HandoffStatus::Created {
            continue;
        }
        match launch(state, &handoff).await {
            Ok(true) => launched += 1,
            Ok(false) => {}
            // A busy session or model stays `created` for the next pass.
            Err(error) => eprintln!("Research hand-off {} waits ({error})", handoff.id),
        }
    }
    Ok(launched)
}

/// Starts one delegated run through ordinary Chat admission, as a scheduled
/// run does: the unattended tool policy, the session as the run's target and
/// the Bot turn's model and Memory flags.
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
            // The Bot card changes once the run's lease drops (`ChatSurface::after_turn`).
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

/// The delivery pass's hand-off step. Every committed hand-off that has not
/// started is sent, on its own task: the Bot reply's own, or one a crash left
/// between commit and launch. A `running` row whose run no longer exists (the
/// app stopped mid-run) is marked stopped: reported, never sent again. Returns
/// every hand-off that started, for the Bot to show.
pub(crate) fn for_delivery(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<Handoff>, CommandError> {
    // Rows before runs: a launch registers its run before it marks the row
    // `running`, so a row read as running has its run in this list.
    let rows = list(state, loupist_id)?;
    let live = state
        .run_coordinator
        .activities()?
        .into_iter()
        .map(|activity| activity.run_id)
        .collect::<std::collections::HashSet<_>>();
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
            handoff.status = HandoffStatus::Stopped;
        }
        shown.push(handoff);
    }
    Ok(shown)
}

/// What a message filed into Memory, for the card that reports it. None
/// when nothing was filed or the digest cannot be read: the post still lands.
pub(crate) fn learned_digest(
    state: &AppState,
    loupist_id: &str,
    message_ids: &[String],
) -> Option<Value> {
    crate::commands::knowledge::learned_digests(state, loupist_id, message_ids)
        .ok()?
        .into_iter()
        .find(|digest| !digest.pages.is_empty())
        .and_then(|digest| serde_json::to_value(digest).ok())
}

/// The first paragraph of the session's final reply, for the Bot.
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
                .iter()
                .find(|message| &message.id == message_id)
                .map(|message| message.content.clone())
        })
        .map(|content| {
            content
                .trim()
                .split("\n\n")
                .next()
                .unwrap_or_default()
                .chars()
                .take(SUMMARY_CHARS)
                .collect()
        })
        .unwrap_or_default()
}

/// The Bot message for one hand-off. Posted once at `running`, then rewritten
/// in place as the run ends, so the status stays visible on one card.
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
        .and_then(|id| learned_digest(state, &handoff.loupist_id, std::slice::from_ref(id)));
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
    use serde_json::json;

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
