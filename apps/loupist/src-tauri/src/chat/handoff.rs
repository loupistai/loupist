use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    app::{AppState, CommandError},
    commands::{
        chat_runtime::{send_chat, ChatAdmission, ChatEventSink},
        map_internal, map_store, new_id, now_ms,
        types::{chat_dto, ChatSendRequest, ChatStreamEvent, HandoffRunDto},
    },
    custom_connections::taint::{CustomOrigin, TurnTaint},
    domain::{
        ChatMessage, ChatMessageStatus, ChatRole, ChatSession, HandoffAction, HandoffNote,
        HomeHandoff, MemoryPolicy,
    },
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
    fn is_active(self) -> bool {
        matches!(self, Self::Created | Self::Running)
    }

    /// Bot posts a hand-off once it reaches one of these; nothing while it works.
    pub(crate) fn is_finished(self) -> bool {
        !self.is_active()
    }

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
#[serde(untagged, deny_unknown_fields)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HandoffStopReason {
    BeforeUpdate,
    OriginUnavailable,
    InvalidAuthority,
    Interrupted,
}

impl HandoffStopReason {
    fn message(self) -> &'static str {
        match self {
            Self::BeforeUpdate => "Opened before an update. Ask again to run it.",
            Self::OriginUnavailable => {
                "The original request or link is unavailable. Ask again to run it."
            }
            Self::InvalidAuthority => {
                "The request's restrictions could not be verified. Ask again to run it."
            }
            Self::Interrupted => "The app stopped before the session finished",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Exposure {
    Clean,
    Text,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HandoffTrust {
    exposure: Exposure,
    dont_save: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connection: Option<CustomOrigin>,
}

impl HandoffTrust {
    fn from_source(taint: &TurnTaint) -> Self {
        use crate::agent_harness::native_policy::Exposure as TurnExposure;
        Self {
            exposure: match taint.exposure() {
                TurnExposure::Clean => Exposure::Clean,
                TurnExposure::ThisTurn => Exposure::Text,
                TurnExposure::Custom => Exposure::Custom,
            },
            dont_save: taint.memory_blocked(),
            connection: taint.origin(),
        }
    }

    fn text() -> Self {
        Self {
            exposure: Exposure::Text,
            dont_save: false,
            connection: None,
        }
    }

    fn valid(&self) -> bool {
        match (self.exposure, &self.connection) {
            (Exposure::Custom, Some(connection)) => {
                connection.kind == "custom_connection"
                    && crate::custom_connections::service::valid_id(&connection.connection_id)
                    && !connection.name.trim().is_empty()
                    && connection.name.len() <= 128
                    && !connection.name.chars().any(char::is_control)
            }
            (Exposure::Clean | Exposure::Text, None) => !self.dont_save,
            _ => false,
        }
    }

    pub(crate) fn untrusted(&self) -> bool {
        self.exposure != Exposure::Clean
    }

    pub(crate) fn dont_save(&self) -> bool {
        self.dont_save
    }

    pub(crate) fn restore_custom(&self, taint: &TurnTaint) {
        if let Some(connection) = &self.connection {
            taint.mark(&connection.connection_id, &connection.name, self.dont_save);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CardActionKind {
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum HandoffOrigin {
    UserMessage {
        chat_id: String,
        message_id: String,
    },
    CardAction {
        kind: CardActionKind,
        connection_id: String,
    },
}

/// Keep malformed authority or context readable so a single bad row can be stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum StoredAuthority<T> {
    Valid(T),
    Invalid(Value),
}

impl<T> StoredAuthority<T> {
    fn valid(&self) -> Option<&T> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Invalid(_) => None,
        }
    }
}

impl Default for StoredAuthority<Vec<HandoffPin>> {
    fn default() -> Self {
        Self::Valid(Vec::new())
    }
}

impl StoredAuthority<Vec<HandoffPin>> {
    fn is_empty(&self) -> bool {
        self.valid().is_some_and(Vec::is_empty)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LinkOptions {
    pub model_profile_id: Option<String>,
    pub thinking_level: Option<String>,
    pub run_options: Option<BTreeMap<String, String>>,
    pub use_memory: Option<bool>,
    pub update_memory: Option<bool>,
}

pub(crate) struct HandoffAdmission {
    pub id: String,
    pub trust: HandoffTrust,
    pub note: HandoffNote,
    pub action: Option<HandoffAction>,
    pub pins: Vec<HandoffPin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Handoff {
    pub id: String,
    pub loupist_id: String,
    pub request_key: String,
    pub chat_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<StoredAuthority<HandoffOrigin>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trust: Option<StoredAuthority<HandoffTrust>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<StoredAuthority<HandoffNote>>,
    #[serde(default, skip_serializing_if = "StoredAuthority::is_empty")]
    pins: StoredAuthority<Vec<HandoffPin>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link_options: Option<StoredAuthority<LinkOptions>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<HandoffStopReason>,
    pub run: HandoffRun,
    pub status: HandoffStatus,
    pub run_id: Option<String>,
    pub message_id: Option<String>,
    pub error: Option<String>,
    pub created_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

impl Handoff {
    pub(crate) fn link_connection_id(&self) -> Option<&str> {
        match self.origin.as_ref().and_then(StoredAuthority::valid) {
            Some(HandoffOrigin::CardAction {
                kind: CardActionKind::Link,
                connection_id,
            }) => Some(connection_id),
            _ => None,
        }
    }
}

fn invalid(message: &str) -> StoreError {
    StoreError::Invalid(message.into())
}

fn load_all(connection: &Connection, loupist_id: &str) -> StoreResult<Vec<Handoff>> {
    let mut statement = connection.prepare(
        "SELECT data_json FROM chat_handoffs WHERE loupist_id=?1 ORDER BY created_at_ms ASC, id ASC",
    )?;
    let rows = statement.query_map([loupist_id], |row| row.get::<_, String>(0))?;
    let mut handoffs = Vec::new();
    for row in rows {
        match serde_json::from_str::<Handoff>(&row?) {
            Ok(handoff) if handoff.loupist_id == loupist_id => handoffs.push(handoff),
            _ => eprintln!("Loupist skipped an unreadable research hand-off"),
        }
    }
    Ok(handoffs)
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

fn stored_chat(
    connection: &Connection,
    loupist_id: &str,
    chat_id: &str,
) -> StoreResult<Option<ChatSession>> {
    let body = connection
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE loupist_id=?1 AND id=?2",
            params![loupist_id, chat_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(body.and_then(|body| {
        let chat: ChatSession = serde_json::from_str(&body).ok()?;
        (chat.id == chat_id && chat.loupist_id == loupist_id).then_some(chat)
    }))
}

fn retained_source_messages(chat: &ChatSession) -> impl Iterator<Item = &ChatMessage> {
    chat.messages
        .iter()
        .chain(chat.versions.iter().flat_map(|version| &version.messages))
}

fn source_prompt(
    connection: &Connection,
    loupist_id: &str,
    chat_id: &str,
    message_id: &str,
) -> StoreResult<Option<String>> {
    if chat_id != super::home::home_thread_id(loupist_id) {
        return Ok(None);
    }
    let Some(chat) = stored_chat(connection, loupist_id, chat_id)? else {
        return Ok(None);
    };
    let mut messages = retained_source_messages(&chat).filter(|message| message.id == message_id);
    Ok(messages
        .next()
        .filter(|message| {
            messages.next().is_none()
                && message.role == ChatRole::User
                && message.status == ChatMessageStatus::Complete
                && !message.host_composed
                && !message.steer
                && !message.content.trim().is_empty()
        })
        .map(|message| message.content.clone()))
}

fn by_request_key(
    transaction: &Connection,
    loupist_id: &str,
    request_key: &str,
) -> StoreResult<Option<Handoff>> {
    transaction
        .query_row(
            "SELECT data_json FROM chat_handoffs WHERE loupist_id=?1 AND request_key=?2",
            params![loupist_id, request_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|body| serde_json::from_str(&body).map_err(Into::into))
        .transpose()
}

pub(crate) fn commit_bot(
    transaction: &Connection,
    loupist_id: &str,
    staged: &StagedHandoff,
    source: (&str, &str),
    taint: &TurnTaint,
    now: i64,
) -> StoreResult<Handoff> {
    let (chat_id, message_id) = source;
    let trust = HandoffTrust::from_source(taint);
    if chat_id != super::home::home_thread_id(loupist_id) || message_id.is_empty() {
        return Err(invalid("The original Bot request is unavailable"));
    }
    let origin = HandoffOrigin::UserMessage {
        chat_id: chat_id.into(),
        message_id: message_id.into(),
    };
    commit(transaction, loupist_id, staged, origin, trust, None, now)
}

pub(crate) const LINK_CAPTION: &str = "The user chose “Look into it” on this Map link.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkPath {
    pub seed: String,
    pub far: String,
    pub path: Vec<LinkClaim>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkClaim {
    pub from: String,
    pub relation: String,
    pub to: String,
}

impl From<&crate::domain::HomeLink> for LinkPath {
    fn from(link: &crate::domain::HomeLink) -> Self {
        let mut previous = &link.seed_name;
        let path = link
            .path
            .iter()
            .map(|hop| {
                let next = &hop.name;
                let (from, to) = if hop.reversed {
                    (next, previous)
                } else {
                    (previous, next)
                };
                previous = next;
                LinkClaim {
                    from: from.clone(),
                    relation: hop.relation.clone(),
                    to: to.clone(),
                }
            })
            .collect();
        Self {
            seed: link.seed_name.clone(),
            far: link.far_name.clone(),
            path,
        }
    }
}

impl LinkPath {
    pub(crate) fn title(&self, language: &str) -> String {
        use crate::agent_harness::interface::InterfaceLocale;
        let (seed, far) = (&self.seed, &self.far);
        match InterfaceLocale::from_language(language) {
            InterfaceLocale::English => format!("Link: {seed} and {far}"),
            InterfaceLocale::Korean => format!("연결: {seed} ↔ {far}"),
        }
    }

    pub(crate) fn from_note(note: &HandoffNote) -> Option<Self> {
        serde_json::from_str(note.brief.as_deref()?).ok()
    }
}

fn link_origin(connection_id: &str) -> HandoffOrigin {
    HandoffOrigin::CardAction {
        kind: CardActionKind::Link,
        connection_id: connection_id.into(),
    }
}

fn link_candidate(
    transaction: &Connection,
    loupist_id: &str,
    connection_id: &str,
) -> StoreResult<Option<Handoff>> {
    let key = format!("link:{connection_id}");
    let row = by_request_key(transaction, loupist_id, &key)?;
    if row.as_ref().is_some_and(|row| {
        row.loupist_id != loupist_id
            || row.request_key != key
            || row.link_connection_id() != Some(connection_id)
    }) {
        return Err(invalid(
            "This link's research request could not be verified",
        ));
    }
    Ok(row)
}

pub(crate) fn link_retry(
    transaction: &Connection,
    loupist_id: &str,
    connection_id: &str,
    options: &LinkOptions,
) -> StoreResult<Option<Handoff>> {
    let Some(row) = link_candidate(transaction, loupist_id, connection_id)?
        .filter(|row| row.status.is_active())
    else {
        return Ok(None);
    };
    repeat_active_link(row, options).map(Some)
}

fn repeat_active_link(row: Handoff, options: &LinkOptions) -> StoreResult<Handoff> {
    if row.link_options.as_ref().and_then(StoredAuthority::valid) == Some(options) {
        return Ok(row);
    }
    Err(StoreError::Conflict(
        "This link already has a research request with different settings",
    ))
}

pub(crate) fn retained_link(
    transaction: &Connection,
    loupist_id: &str,
    connection_id: &str,
) -> StoreResult<Option<crate::knowledge::ConnectionView>> {
    use crate::knowledge::ConnectionStatus;
    let Some(chat) = stored_chat(
        transaction,
        loupist_id,
        &super::home::home_thread_id(loupist_id),
    )?
    else {
        return Ok(None);
    };
    let posted = retained_source_messages(&chat).any(|message| {
        message.role == ChatRole::Assistant
            && message.status == ChatMessageStatus::Complete
            && message.host_composed
            && message
                .link
                .as_ref()
                .is_some_and(|link| link.connection_id == connection_id)
    });
    if !posted {
        return Ok(None);
    }
    let link = crate::knowledge::connection_in_loupist(transaction, loupist_id, connection_id)
        .map_err(link_store_error)?;
    Ok(link.filter(|link| {
        matches!(
            link.status,
            ConnectionStatus::Shown | ConnectionStatus::Opened
        )
    }))
}

fn link_store_error(error: crate::knowledge::KnowledgeError) -> StoreError {
    use crate::knowledge::KnowledgeError;
    match error {
        KnowledgeError::Sqlite(error) => error.into(),
        error => invalid(&error.to_string()),
    }
}

pub(crate) fn commit_link(
    transaction: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    link: &crate::knowledge::ConnectionView,
    options: LinkOptions,
    run: HandoffRun,
    language: &str,
    now: i64,
) -> StoreResult<Handoff> {
    if let Some(row) = link_candidate(transaction, loupist_id, &link.connection_id)? {
        if row.status.is_active() {
            return repeat_active_link(row, &options);
        }
        // Keep the old session and its receipt. Free the dedupe slot in the
        // same transaction as the new session and opened mark.
        transaction.execute(
            "UPDATE chat_handoffs SET request_key=?3 WHERE loupist_id=?1 AND id=?2",
            params![loupist_id, row.id, format!("link-history:{}", row.id)],
        )?;
    }
    let link_details = crate::domain::HomeLink::from(link);
    let path = LinkPath::from(&link_details);
    let mut seen = HashSet::new();
    let staged = StagedHandoff {
        request: HandoffRequest {
            title: path.title(language),
            question: String::new(),
            brief: serde_json::to_value(&path)?.to_string(),
            pins: link_details
                .node_ids
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .take(MAX_PINS)
                .map(|world| HandoffPin::World { world })
                .collect(),
            request_key: format!("link:{}", link.connection_id),
        },
        run,
    };
    let handoff = commit(
        transaction,
        loupist_id,
        &staged,
        link_origin(&link.connection_id),
        HandoffTrust::text(),
        Some(options),
        now,
    )?;
    crate::knowledge::set_connection_status(
        transaction,
        loupist_id,
        &link.connection_id,
        crate::knowledge::ConnectionStatus::Opened,
        now,
    )
    .map_err(link_store_error)?;
    Ok(handoff)
}

fn commit(
    transaction: &Connection,
    loupist_id: &str,
    staged: &StagedHandoff,
    origin: HandoffOrigin,
    trust: HandoffTrust,
    link_options: Option<LinkOptions>,
    now: i64,
) -> StoreResult<Handoff> {
    let storage_key = match &origin {
        HandoffOrigin::UserMessage { message_id, .. } => format!("bot-turn:{message_id}"),
        HandoffOrigin::CardAction { .. } => staged.request.request_key.clone(),
    };
    let origin = StoredAuthority::Valid(origin);
    if !trust.valid() {
        return Err(invalid(
            "The source turn's restrictions could not be recorded",
        ));
    }
    let mut run = staged.run.clone();
    run.update_memory &= !trust.dont_save();
    let mut note = HandoffNote {
        title: staged.request.title.clone(),
        question: (!staged.request.question.is_empty()).then(|| staged.request.question.clone()),
        brief: (!staged.request.brief.is_empty()).then(|| staged.request.brief.clone()),
        untrusted_id: None,
    };
    let untrusted = trust.untrusted();
    let trust = StoredAuthority::Valid(trust);
    let mut existing = by_request_key(transaction, loupist_id, &storage_key)?;
    if existing.is_none() && link_options.is_none() {
        // A receipt from before turn-scoped keys still dedupes the same turn.
        let old_key = format!("bot:{}", staged.request.request_key);
        existing = by_request_key(transaction, loupist_id, &old_key)?
            .filter(|row| row.origin.as_ref() == Some(&origin));
    }
    if let Some(row) = existing {
        let same_note = row
            .note
            .as_ref()
            .and_then(StoredAuthority::valid)
            .is_some_and(|stored| {
                stored.title == note.title
                    && stored.question == note.question
                    && stored.brief == note.brief
            });
        if row.origin.as_ref() == Some(&origin)
            && row.trust.as_ref() == Some(&trust)
            && row.run == run
            && row.request_key == staged.request.request_key
            && row.pins.valid() == Some(&staged.request.pins)
            && same_note
        {
            let owns_target = row.loupist_id == loupist_id
                && transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE id=?1 AND loupist_id=?2)",
                    params![row.chat_id, loupist_id],
                    |r| r.get::<_, bool>(0),
                )?;
            if !owns_target {
                return Err(invalid("The original research session is unavailable"));
            }
            return Ok(row);
        }
        return Err(StoreError::Conflict(
            "This research request key already belongs to another hand-off",
        ));
    }
    note.untrusted_id = untrusted.then(crate::custom_connections::taint::fresh_id);
    let chat = ChatSession {
        id: new_id("chat"),
        loupist_id: loupist_id.into(),
        pi_session_id: Uuid::new_v4().to_string(),
        pi_session_cache: None,
        title: staged.request.title.clone(),
        memory_policy: MemoryPolicy {
            use_memory: run.use_memory,
            update_memory: run.update_memory,
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
        origin: Some(origin),
        trust: Some(trust),
        note: Some(StoredAuthority::Valid(note)),
        pins: StoredAuthority::Valid(staged.request.pins.clone()),
        link_options: link_options.map(StoredAuthority::Valid),
        stop_reason: None,
        run,
        status: HandoffStatus::Created,
        run_id: None,
        message_id: None,
        error: None,
        created_at_ms: now,
        started_at_ms: None,
        finished_at_ms: None,
    };
    transaction.execute(
        "INSERT INTO chat_handoffs(id,loupist_id,request_key,chat_id,status,run_id,created_at_ms,data_json) VALUES(?1,?2,?3,?4,?5,NULL,?6,?7)",
        params![
            handoff.id,
            handoff.loupist_id,
            storage_key,
            handoff.chat_id,
            handoff.status.as_str(),
            handoff.created_at_ms,
            serde_json::to_string(&handoff)?
        ],
    )?;
    Ok(handoff)
}

#[cfg(test)]
pub(crate) fn commit_standalone(
    state: &AppState,
    loupist_id: &str,
    staged: &StagedHandoff,
) -> Result<Handoff, CommandError> {
    let mut bot = super::home::ensure_home_thread(state, loupist_id)?;
    let message_id = format!("fixture-{}", staged.request.request_key);
    if !bot.messages.iter().any(|message| message.id == message_id) {
        let before = bot.clone();
        bot.messages.push(crate::test_fixtures::chat_message(
            &message_id,
            ChatRole::User,
            &staged.request.question,
        ));
        state.store.replace_chat(&before, &bot).map_err(map_store)?;
    }
    let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_internal)?;
    let row = commit_bot(
        &transaction,
        loupist_id,
        staged,
        (&bot.id, &message_id),
        &TurnTaint::default(),
        now_ms(),
    )
    .map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(row)
}

pub(crate) fn list(state: &AppState, loupist_id: &str) -> Result<Vec<Handoff>, CommandError> {
    let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    load_all(&connection, loupist_id).map_err(map_store)
}

fn authority(row: &Handoff) -> Result<VerifiedHandoff<'_>, HandoffStopReason> {
    use HandoffStopReason::{BeforeUpdate, InvalidAuthority};
    let origin = row
        .origin
        .as_ref()
        .ok_or(BeforeUpdate)?
        .valid()
        .ok_or(InvalidAuthority)?;
    let trust = row
        .trust
        .as_ref()
        .ok_or(BeforeUpdate)?
        .valid()
        .ok_or(InvalidAuthority)?;
    let note = row
        .note
        .as_ref()
        .and_then(StoredAuthority::valid)
        .ok_or(InvalidAuthority)?;
    let pins = row.pins.valid().ok_or(InvalidAuthority)?;
    if !trust.valid()
        || trust.untrusted() != note.untrusted_id.is_some()
        || note
            .untrusted_id
            .as_ref()
            .is_some_and(|id| id.len() != 12 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        || pins.len() > MAX_PINS
        || (trust.dont_save() && row.run.update_memory)
    {
        return Err(InvalidAuthority);
    }
    match origin {
        HandoffOrigin::UserMessage {
            chat_id,
            message_id,
        } if chat_id == &super::home::home_thread_id(&row.loupist_id)
            && !message_id.is_empty()
            && note.question.is_some()
            && row.link_options.is_none() => {}
        HandoffOrigin::CardAction {
            kind: CardActionKind::Link,
            connection_id,
        } if trust.exposure == Exposure::Text
            && row
                .link_options
                .as_ref()
                .and_then(StoredAuthority::valid)
                .is_some()
            && row.request_key == format!("link:{connection_id}")
            && note.question.is_none()
            && LinkPath::from_note(note).is_some() => {}
        _ => return Err(InvalidAuthority),
    }
    Ok(VerifiedHandoff {
        origin,
        trust,
        note,
        pins,
    })
}

struct VerifiedHandoff<'a> {
    origin: &'a HandoffOrigin,
    trust: &'a HandoffTrust,
    note: &'a HandoffNote,
    pins: &'a [HandoffPin],
}

fn stop_created(
    transaction: &Connection,
    row: &mut Handoff,
    reason: HandoffStopReason,
) -> StoreResult<()> {
    row.status = HandoffStatus::Stopped;
    row.error = Some(reason.message().into());
    row.stop_reason = Some(reason);
    row.finished_at_ms = Some(now_ms());
    save(transaction, row)
}

enum LaunchClaim {
    Skipped,
    Stopped,
    Started {
        admission: Box<ChatAdmission>,
        prompt: String,
    },
}

fn claim(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    run_id: &str,
) -> Result<LaunchClaim, CommandError> {
    let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_internal)?;
    let Some(mut row) = load(&transaction, loupist_id, id).map_err(map_store)? else {
        return Ok(LaunchClaim::Skipped);
    };
    if row.status != HandoffStatus::Created {
        return Ok(LaunchClaim::Skipped);
    }
    let VerifiedHandoff {
        origin,
        trust,
        note,
        pins,
    } = match authority(&row) {
        Ok(authority) => authority,
        Err(reason) => {
            stop_created(&transaction, &mut row, reason).map_err(map_store)?;
            transaction.commit().map_err(map_internal)?;
            return Ok(LaunchClaim::Stopped);
        }
    };
    let (prompt, action) = match origin {
        HandoffOrigin::UserMessage {
            chat_id,
            message_id,
        } => (
            source_prompt(&transaction, loupist_id, chat_id, message_id).map_err(map_store)?,
            None,
        ),
        HandoffOrigin::CardAction {
            kind: CardActionKind::Link,
            connection_id,
        } => (
            retained_link(&transaction, loupist_id, connection_id)
                .map_err(map_store)?
                .map(|_| LINK_CAPTION.to_owned()),
            Some(HandoffAction::Link {
                connection_id: connection_id.clone(),
            }),
        ),
    };
    let Some(prompt) = prompt else {
        stop_created(&transaction, &mut row, HandoffStopReason::OriginUnavailable)
            .map_err(map_store)?;
        transaction.commit().map_err(map_internal)?;
        return Ok(LaunchClaim::Stopped);
    };
    let mut admission = ChatAdmission::new(state, run_id, loupist_id, &row.chat_id)?;
    admission.handoff = Some(HandoffAdmission {
        id: row.id.clone(),
        trust: trust.clone(),
        note: note.clone(),
        action,
        pins: pins.to_vec(),
    });
    row.status = HandoffStatus::Running;
    row.run_id = Some(run_id.to_owned());
    row.started_at_ms = Some(now_ms());
    save(&transaction, &row).map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(LaunchClaim::Started {
        admission: Box::new(admission),
        prompt,
    })
}

#[cfg(test)]
pub(crate) fn claim_fixture(
    state: &AppState,
    row: &Handoff,
    run_id: &str,
) -> (ChatAdmission, String) {
    match claim(state, &row.loupist_id, &row.id, run_id).unwrap() {
        LaunchClaim::Started { admission, prompt } => (*admission, prompt),
        _ => panic!("fixture hand-off was not claimed"),
    }
}

/// Ends a claimed fixture hand-off with its child's final message, as the runtime does.
#[cfg(all(
    test,
    feature = "eval-harness",
    target_os = "macos",
    target_arch = "aarch64",
    debug_assertions
))]
pub(crate) fn finish_done_fixture(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    message_id: String,
) -> Result<(), CommandError> {
    finish(
        state,
        loupist_id,
        id,
        Outcome::Done(Some(message_id)),
        now_ms(),
    )
}

fn bounded_error(value: &str) -> String {
    value.replace('\0', "").chars().take(2_000).collect()
}

fn finish(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    outcome: Outcome,
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
    record(&mut handoff, outcome, now);
    save(&transaction, &handoff).map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(())
}

fn record(handoff: &mut Handoff, outcome: Outcome, now: i64) {
    match outcome {
        Outcome::Done(message_id) => {
            handoff.status = HandoffStatus::Done;
            handoff.message_id = message_id;
        }
        Outcome::Failed(error) => {
            handoff.status = HandoffStatus::Failed;
            handoff.error = error.as_deref().map(bounded_error);
        }
        Outcome::Stopped(reason) => {
            handoff.status = HandoffStatus::Stopped;
            handoff.error = Some(
                reason
                    .map_or("Stopped before completion", HandoffStopReason::message)
                    .into(),
            );
            handoff.stop_reason = reason;
        }
    }
    handoff.finished_at_ms = Some(now);
}

/// Settles a Running row whose run this app no longer holds: the app quit while
/// it ran, or its run ended without reporting. An answer the session already
/// committed is that run's result, so the row is Done with it; otherwise the
/// run was interrupted. A row a sink settled meanwhile keeps its outcome.
fn settle_unheld(
    state: &AppState,
    loupist_id: &str,
    id: &str,
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
    let answer = stored_chat(&transaction, loupist_id, &handoff.chat_id)
        .map_err(map_store)?
        .and_then(|session| committed_answer(&session));
    let outcome = match answer {
        Some(message_id) => Outcome::Done(Some(message_id)),
        None => Outcome::Stopped(Some(HandoffStopReason::Interrupted)),
    };
    record(&mut handoff, outcome, now);
    save(&transaction, &handoff).map_err(map_store)?;
    transaction.commit().map_err(map_internal)?;
    Ok(())
}

/// The run's answer, when the session committed one: the reply that follows the
/// session's opening request (the message carrying the hand-off note), past any
/// guidance sent while it ran. A stopped or failed reply is no answer.
fn committed_answer(session: &ChatSession) -> Option<String> {
    let mut after_request = session
        .messages
        .iter()
        .skip_while(|message| message.role != ChatRole::User || message.handoff_note.is_none());
    after_request.next()?;
    after_request
        .find(|message| !(message.role == ChatRole::User && message.steer))
        .filter(|reply| {
            reply.role == ChatRole::Assistant && reply.status == ChatMessageStatus::Complete
        })
        .map(|reply| reply.id.clone())
}

enum Outcome {
    Done(Option<String>),
    Failed(Option<String>),
    Stopped(Option<HandoffStopReason>),
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
    let (admission, prompt) = match claim(state, &handoff.loupist_id, &handoff.id, &run_id)? {
        LaunchClaim::Skipped => return Ok(false),
        LaunchClaim::Stopped => return Ok(true),
        LaunchClaim::Started { admission, prompt } => (admission, prompt),
    };
    // The sink holds the lease, which marks the run as this app's until the sink
    // has recorded how it ended. The run's own registration is released earlier,
    // before its answer commits.
    let lease = state
        .live_handoffs
        .start(&handoff.loupist_id, &handoff.chat_id, &run_id);
    let cancel = admission.cancellation();
    let live = lease.chat.clone();
    let baseline = live.clone();
    let running = HandoffRunDto {
        handoff_id: handoff.id.clone(),
        run_id: run_id.clone(),
        connection_id: handoff.link_connection_id().map(str::to_owned),
    };
    let sink = {
        let state = state.clone();
        let loupist_id = handoff.loupist_id.clone();
        let id = handoff.id.clone();
        ChatEventSink::new(move |event| {
            let _lease = &lease;
            let outcome = match &event {
                ChatStreamEvent::Completed { message_id, .. } => {
                    Some(Outcome::Done(Some(message_id.clone())))
                }
                ChatStreamEvent::Aborted { .. } => Some(Outcome::Stopped(
                    state
                        .run_coordinator
                        .is_shutting_down()
                        .then_some(HandoffStopReason::Interrupted),
                )),
                ChatStreamEvent::Error { message, .. } => {
                    Some(Outcome::Failed(Some(message.clone())))
                }
                _ => None,
            };
            let terminal = outcome.is_some();
            if let Some(outcome) = outcome {
                if let Err(error) = finish(&state, &loupist_id, &id, outcome, now_ms()) {
                    eprintln!("Research hand-off outcome could not be recorded ({error})");
                }
            }
            live.publish(event);
            if terminal {
                let _ = super::home::deliver_finished_runs(&state, &loupist_id);
            }
            Ok(())
        })
        .with_baseline({
            let loupist_id = handoff.loupist_id.clone();
            move |chat| {
                if let Ok(mut thread) = chat_dto(chat) {
                    thread.handoff_run = Some(running.clone());
                    baseline.set_baseline(thread);
                }
                // Bot posts nothing while the session works: this refresh brings
                // the session's first message and its run to the renderer.
                crate::briefings::desktop::changed(&loupist_id);
            }
        })
    };
    let request = ChatSendRequest {
        run_id,
        loupist_id: handoff.loupist_id.clone(),
        thread_id: handoff.chat_id.clone(),
        prompt,
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
    if let Err(error) = send_chat(request, sink.clone(), state, Some(*admission)).await {
        let outcome = if *cancel.borrow() {
            Outcome::Stopped(
                state
                    .run_coordinator
                    .is_shutting_down()
                    .then_some(HandoffStopReason::Interrupted),
            )
        } else {
            Outcome::Failed(Some(error.message))
        };
        finish(state, &handoff.loupist_id, &handoff.id, outcome, now_ms())?;
        let _ = super::home::deliver_finished_runs(state, &handoff.loupist_id);
    }
    Ok(true)
}

/// Chat runs the coordinator holds now.
fn registered_runs(state: &AppState) -> Result<HashSet<String>, CommandError> {
    Ok(state
        .run_coordinator
        .activities()?
        .into_iter()
        .map(|activity| activity.run_id)
        .collect())
}

/// Whether this app still runs a Running row: its Chat run is registered, or
/// the sink that records how it ends still holds its lease. The run releases
/// its registration before its answer commits; only the lease covers the
/// commit and the outcome that follows it.
fn held(state: &AppState, row: &Handoff, registered: &HashSet<String>) -> bool {
    row.run_id.as_deref().is_some_and(|run_id| {
        registered.contains(run_id)
            || state
                .live_handoffs
                .holds(&row.loupist_id, &row.chat_id, run_id)
    })
}

/// The hand-off each research session is running now, by session.
pub(crate) fn running(
    state: &AppState,
    loupist_id: &str,
) -> Result<HashMap<String, HandoffRunDto>, CommandError> {
    let rows = list(state, loupist_id)?;
    let registered = registered_runs(state)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.status == HandoffStatus::Running && held(state, row, &registered))
        .filter_map(|row| {
            let run = HandoffRunDto {
                handoff_id: row.id.clone(),
                run_id: row.run_id.clone()?,
                connection_id: row.link_connection_id().map(str::to_owned),
            };
            Some((row.chat_id, run))
        })
        .collect())
}

/// Finished hand-offs for Bot to post. A queued one is sent, and a Running one
/// this app no longer runs is settled first.
pub(crate) fn for_delivery(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<Handoff>, CommandError> {
    let rows = list(state, loupist_id)?;
    let registered = registered_runs(state)?;
    delivery_rows(state, loupist_id, rows, &registered)
}

fn delivery_rows(
    state: &AppState,
    loupist_id: &str,
    rows: Vec<Handoff>,
    registered: &HashSet<String>,
) -> Result<Vec<Handoff>, CommandError> {
    let mut shown = Vec::new();
    let mut pending = false;
    for mut handoff in rows {
        if handoff.status == HandoffStatus::Created {
            if authority(&handoff).is_ok() {
                pending = true;
                continue;
            }
            let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
            let transaction = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(map_internal)?;
            let Some(mut current) =
                load(&transaction, loupist_id, &handoff.id).map_err(map_store)?
            else {
                continue;
            };
            if current.status == HandoffStatus::Created {
                match authority(&current) {
                    Err(reason) => {
                        stop_created(&transaction, &mut current, reason).map_err(map_store)?
                    }
                    Ok(_) => {
                        pending = true;
                        continue;
                    }
                }
            }
            transaction.commit().map_err(map_internal)?;
            // The live-run snapshot predates this second load.
            handoff = current;
        } else if handoff.status == HandoffStatus::Running {
            if held(state, &handoff, registered) {
                continue;
            }
            settle_unheld(state, loupist_id, &handoff.id, now_ms())?;
            // A sink may have recorded the outcome since the initial list:
            // deliver what is stored, not our snapshot.
            let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
            let Some(current) = load(&connection, loupist_id, &handoff.id).map_err(map_store)?
            else {
                continue;
            };
            handoff = current;
        }
        if handoff.status.is_finished() {
            shown.push(handoff);
        }
    }
    if pending {
        send_created(state, loupist_id);
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
    let connection_id = handoff.link_connection_id();
    // Keep the existing Bot history caption. Link names are delivered as
    // structured data for the renderer and the delegated Map envelope.
    let title = if connection_id.is_some() {
        "Look into it"
    } else {
        &handoff.title
    };
    let content = match handoff.status {
        HandoffStatus::Done => {
            let summary = summary(state, handoff);
            if summary.is_empty() {
                format!("Researched: {title}.")
            } else {
                summary
            }
        }
        HandoffStatus::Failed | HandoffStatus::Stopped => {
            format!("Research on {title} could not finish.")
        }
        HandoffStatus::Running | HandoffStatus::Created => {
            return Err(CommandError::internal(
                "a research hand-off at work has no Bot post",
            ))
        }
    };
    let mut message = match existing {
        Some(existing) => {
            let mut message = existing.clone();
            message.content = content;
            message
        }
        // The result arrives when the session ends, so the post sits there.
        None => super::home::host_message(
            "handoff",
            content,
            handoff.finished_at_ms.unwrap_or(handoff.created_at_ms),
        )?,
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
        connection_id: connection_id.map(str::to_owned),
        status: handoff.status,
        stop_reason: handoff.stop_reason,
        learned,
    });
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::surface::ChatSurface;
    use serde_json::json;

    fn state_with_loupist(id: &str) -> (tempfile::TempDir, AppState) {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let bound =
            crate::fs::loupist_root::BoundLoupistRoot::open_unbound(workspace.clone()).unwrap();
        let state = AppState::for_persistent_test(root.path().join("app"));
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
        let LaunchClaim::Started { admission, .. } =
            claim(&state, loupist, &handoff.id, "handoff-signal").unwrap()
        else {
            panic!("fixture did not claim the hand-off")
        };
        let told = || crate::briefings::desktop::changed_count(loupist);
        let deliver = || super::super::home::deliver_finished_runs(&state, loupist).unwrap();

        // A pass from any caller (opening the Loupist runs one) posts nothing for a
        // session at work, and says nothing.
        assert_eq!(deliver(), 0);
        assert_eq!(told(), 0);

        let stale_running = list(&state, loupist).unwrap();
        finish(&state, loupist, &handoff.id, Outcome::Done(None), now_ms()).unwrap();
        drop(admission);
        let reconciled = delivery_rows(&state, loupist, stale_running, &HashSet::new()).unwrap();
        assert_eq!(
            reconciled[0].status,
            HandoffStatus::Done,
            "completion won after the initial row read"
        );
        assert_eq!(deliver(), 1, "the finished session posts its result");
        assert_eq!(told(), 1);
        assert_eq!(deliver(), 0);
        assert_eq!(told(), 1, "a pass that writes nothing stays quiet");

        // The run's end changed its own session even though the Bot has nothing new.
        ChatSurface::Delegated {
            handoff_id: handoff.id.clone(),
        }
        .after_turn(&state, loupist);
        assert_eq!(told(), 2);
    }

    /// The session's opening request, as the run appends it, then what followed it.
    fn append_session(state: &AppState, row: &Handoff, after: Vec<ChatMessage>) {
        let before = state.store.get_chat(&row.chat_id).unwrap().unwrap();
        let mut session = before.clone();
        let mut request = crate::test_fixtures::chat_message(
            &format!("{}-request", row.id),
            ChatRole::User,
            "Check the original request.",
        );
        request.handoff_note = Some(HandoffNote {
            title: row.title.clone(),
            question: Some("Check the original request.".into()),
            brief: None,
            untrusted_id: None,
        });
        session.messages.push(request);
        session.messages.extend(after);
        state.store.replace_chat(&before, &session).unwrap();
    }

    fn guidance(id: &str) -> ChatMessage {
        let mut steer =
            crate::test_fixtures::chat_message(id, ChatRole::User, "Put the delivery dates first.");
        steer.steer = true;
        steer
    }

    fn bot_cards(state: &AppState, loupist: &str) -> Vec<ChatMessage> {
        super::super::home::ensure_home_thread(state, loupist)
            .unwrap()
            .messages
            .into_iter()
            .filter(|message| message.handoff.is_some())
            .collect()
    }

    #[test]
    fn a_run_that_released_its_registration_is_not_interrupted_before_its_outcome_arrives() {
        let loupist = "loupist-handoff-race";
        let (_root, state) = state_with_loupist(loupist);
        let row = commit_standalone(&state, loupist, &staged_fixture("race")).unwrap();
        let (admission, _) = claim_fixture(&state, &row, "handoff-race");
        // `launch` starts the lease its sink holds until the sink is dropped.
        let lease = state
            .live_handoffs
            .start(loupist, &row.chat_id, "handoff-race");
        append_session(&state, &row, vec![guidance("race-guidance")]);
        let deliver = || super::super::home::deliver_finished_runs(&state, loupist).unwrap();
        let status = || list(&state, loupist).unwrap()[0].status;

        // The turn hands its Chat registration to the Memory writer before the
        // answer commits: the run is gone from the coordinator.
        drop(admission);
        assert!(state.run_coordinator.activities().unwrap().is_empty());
        // Any caller's pass in that window (a refresh, another run's end, the
        // Loupist's listing) leaves the row running and posts nothing.
        assert_eq!(deliver(), 0);
        assert_eq!(status(), HandoffStatus::Running);

        // The answer commits; a pass runs again before the sink hears Completed.
        let mut before = state.store.get_chat(&row.chat_id).unwrap().unwrap();
        let mut answered = before.clone();
        answered.messages.push(crate::test_fixtures::chat_message(
            "race-answer",
            ChatRole::Assistant,
            "HBM4 supply widens in 2027.\n\nDetails.",
        ));
        state.store.replace_chat(&before, &answered).unwrap();
        assert_eq!(deliver(), 0);
        assert_eq!(status(), HandoffStatus::Running);
        assert!(bot_cards(&state, loupist).is_empty());

        // Completed arrives: the sink records it, then delivers.
        finish(
            &state,
            loupist,
            &row.id,
            Outcome::Done(Some("race-answer".into())),
            now_ms(),
        )
        .unwrap();
        assert_eq!(deliver(), 1);
        let done = list(&state, loupist).unwrap().remove(0);
        assert_eq!(done.status, HandoffStatus::Done);
        assert_eq!(done.message_id.as_deref(), Some("race-answer"));
        assert_eq!((done.stop_reason, done.error), (None, None));
        let cards = bot_cards(&state, loupist);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].content, "HBM4 supply widens in 2027.");
        assert_eq!(
            cards[0].handoff.as_ref().unwrap().status,
            HandoffStatus::Done
        );
        assert_eq!(cards[0].created_at_ms, done.finished_at_ms.unwrap());

        // The sink goes; nothing is left to settle.
        drop(lease);
        assert_eq!(deliver(), 0);
        before = state.store.get_chat(&row.chat_id).unwrap().unwrap();
        assert_eq!(before.messages.last().unwrap().id, "race-answer");
    }

    #[test]
    fn after_a_restart_a_running_row_is_done_with_its_committed_answer_or_interrupted() {
        let id = "loupist-restart-answer";
        let (root, state) = state_with_loupist(id);
        let answered = commit_standalone(&state, id, &staged_fixture("answered")).unwrap();
        let unanswered = commit_standalone(&state, id, &staged_fixture("unanswered")).unwrap();
        let failed = commit_standalone(&state, id, &staged_fixture("failed-reply")).unwrap();
        let mut admissions = Vec::new();
        for (row, run) in [
            (&answered, "run-answered"),
            (&unanswered, "run-unanswered"),
            (&failed, "run-failed"),
        ] {
            admissions.push(claim_fixture(&state, row, run).0);
        }
        append_session(
            &state,
            &answered,
            vec![
                guidance("answered-guidance"),
                crate::test_fixtures::chat_message(
                    "answered-reply",
                    ChatRole::Assistant,
                    "Sales increased.\n\nDetails.",
                ),
            ],
        );
        append_session(&state, &unanswered, vec![guidance("unanswered-guidance")]);
        let mut error = crate::test_fixtures::chat_message(
            "failed-reply",
            ChatRole::Assistant,
            "The reply failed.",
        );
        error.status = ChatMessageStatus::Error;
        append_session(&state, &failed, vec![error]);
        // The app quits: no run, no lease and no outcome recorded.
        drop(admissions);
        drop(state);

        let state = AppState::for_persistent_test(root.path().join("app"));
        assert_eq!(
            super::super::home::deliver_finished_runs(&state, id).unwrap(),
            3
        );
        let rows = list(&state, id).unwrap();
        let get = |row: &Handoff| {
            rows.iter()
                .find(|value| value.id == row.id)
                .unwrap()
                .clone()
        };
        let done = get(&answered);
        assert_eq!(done.status, HandoffStatus::Done);
        assert_eq!(done.message_id.as_deref(), Some("answered-reply"));
        assert_eq!((done.stop_reason, done.error.as_deref()), (None, None));
        for row in [&unanswered, &failed] {
            let stopped = get(row);
            assert_eq!(stopped.status, HandoffStatus::Stopped);
            assert_eq!(stopped.stop_reason, Some(HandoffStopReason::Interrupted));
            assert_eq!(
                stopped.error.as_deref(),
                Some("The app stopped before the session finished")
            );
            assert!(stopped.message_id.is_none());
        }
        let cards = bot_cards(&state, id);
        let card = |row: &Handoff| {
            cards
                .iter()
                .find(|message| message.handoff.as_ref().unwrap().handoff_id == row.id)
                .unwrap()
        };
        assert_eq!(card(&answered).content, "Sales increased.");
        assert_eq!(
            card(&answered).handoff.as_ref().unwrap().status,
            HandoffStatus::Done
        );
        assert_eq!(
            card(&unanswered).handoff.as_ref().unwrap().stop_reason,
            Some(HandoffStopReason::Interrupted)
        );
        assert_eq!(
            super::super::home::deliver_finished_runs(&state, id).unwrap(),
            0
        );
    }

    #[test]
    fn completed_handoff_previews_prose_or_its_title_and_keeps_the_original() {
        let loupist = "loupist-handoff-preview";
        let (_root, state) = state_with_loupist(loupist);
        let mut handoff = commit_standalone(
            &state,
            loupist,
            &StagedHandoff {
                request: HandoffRequest::parse(&json!({
                    "title": "Alder Rail",
                    "question": "What changed?",
                    "requestKey": "alder-preview"
                }))
                .unwrap(),
                run: HandoffRun {
                    model_profile_id: "fixture".into(),
                    thinking_level: "medium".into(),
                    run_options: BTreeMap::new(),
                    use_memory: false,
                    update_memory: false,
                },
            },
        )
        .unwrap();
        handoff.status = HandoffStatus::Done;
        let table = "| Quarter | Sales |\n| --- | --- |\n| Q3 | 42 |";
        let prose = format!("{table}\n\n**Sales** increased.\n\nDetails.");
        let mut before = state.store.get_chat(&handoff.chat_id).unwrap().unwrap();
        for (index, (answer, preview)) in [
            (table, "Researched: Alder Rail."),
            (prose.as_str(), "Sales increased."),
        ]
        .into_iter()
        .enumerate()
        {
            let mut after = before.clone();
            let message_id = format!("answer-{index}");
            after.messages.push(crate::test_fixtures::chat_message(
                &message_id,
                crate::domain::ChatRole::Assistant,
                answer,
            ));
            state.store.replace_chat(&before, &after).unwrap();
            handoff.message_id = Some(message_id);
            assert_eq!(message(&state, &handoff, None).unwrap().content, preview);
            before = state.store.get_chat(&handoff.chat_id).unwrap().unwrap();
            assert_eq!(before.messages.last().unwrap().content, answer);
        }
    }

    #[test]
    fn home_delivery_reports_pending_news_read_errors_without_posting() {
        for table in [
            "knowledge_schema_changes",
            // Not `knowledge_connections`: extracted connections stay on the Map and
            // are no longer read for Bot posts, so that table cannot fail a delivery.
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

    fn staged_fixture(key: &str) -> StagedHandoff {
        StagedHandoff {
            request: HandoffRequest::parse(&json!({"title":"Fixture research","question":"Check the original request.","requestKey":key})).unwrap(),
            run: HandoffRun {
                model_profile_id: "fixture".into(), thinking_level: "medium".into(),
                run_options: BTreeMap::new(), use_memory: false, update_memory: false,
            },
        }
    }

    #[test]
    fn a_retried_bot_receipt_cannot_link_to_another_loupists_session() {
        let id = "loupist-bot-link";
        let (_root, state) = state_with_loupist(id);
        let staged = staged_fixture("original");
        let original = commit_standalone(&state, id, &staged).unwrap();
        let other = crate::test_fixtures::loupist_profile("other");
        state.store.create_loupist(&other).unwrap();
        let foreign = crate::test_fixtures::chat_session("foreign-chat", &other.id);
        state.store.create_chat(&foreign).unwrap();
        state
            .store
            .lock_loupist(id)
            .unwrap()
            .execute(
                "UPDATE chat_handoffs SET data_json=json_set(data_json,'$.chat_id',?1) WHERE id=?2",
                params![foreign.id, original.id],
            )
            .unwrap();
        assert!(commit_standalone(&state, id, &staged).is_err());
    }

    #[test]
    fn bot_retries_are_scoped_to_the_source_turn() {
        let id = "loupist-bot-retry";
        let (_root, state) = state_with_loupist(id);
        let staged = staged_fixture("stable-key");
        let first = commit_standalone(&state, id, &staged).unwrap();
        let (admission, _) = claim_fixture(&state, &first, "failed-bot-handoff");
        finish(&state, id, &first.id, Outcome::Failed(None), 42).unwrap();
        drop(admission);
        let same_turn = commit_standalone(&state, id, &staged).unwrap();
        assert_eq!(same_turn.id, first.id);
        assert_eq!(same_turn.status, HandoffStatus::Failed);

        let home = super::super::home::ensure_home_thread(&state, id).unwrap();
        let mut retry = home.clone();
        retry.messages.push(crate::test_fixtures::chat_message(
            "later-turn",
            ChatRole::User,
            "Try again.",
        ));
        state.store.replace_chat(&home, &retry).unwrap();
        let mut connection = state.store.lock_loupist(id).unwrap();
        let transaction = connection.transaction().unwrap();
        let second = commit_bot(
            &transaction,
            id,
            &staged,
            (&home.id, "later-turn"),
            &TurnTaint::default(),
            43,
        )
        .unwrap();
        transaction.commit().unwrap();
        assert_ne!(second.chat_id, first.chat_id);
        assert_eq!(second.status, HandoffStatus::Created);
    }

    #[test]
    fn a_terminal_link_frees_its_active_slot_without_replacing_the_old_session() {
        let id = "loupist-link-retry";
        let (_root, state) = state_with_loupist(id);
        let mut connection = state.store.lock_loupist(id).unwrap();
        let transaction = connection.transaction().unwrap();
        let connection_id = "fixture-link";
        let path = json!({"seedName":"Alpha","hops":[{
            "name":"Beta","nodeId":"b","claimId":"claim-a","relation":"SUPPLIES",
            "fromRole":"supplier","toRole":"customer","reversed":false
        }]});
        transaction.execute(
            "INSERT INTO knowledge_connections(connection_id,loupist_id,seed_node_id,far_node_id,
                path_json,score,status,created_at_ms,status_at_ms) VALUES (?1,?2,'a','b',?3,1,'shown',1,1)",
            params![connection_id, id, path.to_string()],
        ).unwrap();
        let link = crate::knowledge::connection_in_loupist(&transaction, id, connection_id)
            .unwrap()
            .unwrap();
        let staged = staged_fixture("link");
        let options = LinkOptions::default();
        let create = |at| {
            commit_link(
                &transaction,
                id,
                &link,
                options.clone(),
                staged.run.clone(),
                "ko",
                at,
            )
            .unwrap()
        };
        let mut active = create(1);
        assert_eq!(active.title, "연결: Alpha ↔ Beta");
        for (index, status) in [
            HandoffStatus::Done,
            HandoffStatus::Failed,
            HandoffStatus::Stopped,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                link_retry(&transaction, id, connection_id, &options)
                    .unwrap()
                    .unwrap()
                    .id,
                active.id
            );
            let conflicting = LinkOptions {
                update_memory: Some(true),
                ..options.clone()
            };
            assert!(matches!(
                link_retry(&transaction, id, connection_id, &conflicting),
                Err(StoreError::Conflict(_))
            ));

            active.status = status;
            save(&transaction, &active).unwrap();
            assert!(link_retry(&transaction, id, connection_id, &options)
                .unwrap()
                .is_none());
            let next = create(2 + index as i64);
            assert_ne!(next.chat_id, active.chat_id);
            assert_eq!(
                load(&transaction, id, &active.id).unwrap().unwrap().status,
                status
            );
            assert!(stored_chat(&transaction, id, &active.chat_id)
                .unwrap()
                .is_some());
            active = next;
        }
        transaction.commit().unwrap();
    }

    #[test]
    fn startup_stops_each_unverifiable_or_interrupted_row_once_and_retains_history() {
        let id = "loupist-restart-handoff";
        let (root, state) = state_with_loupist(id);
        let legacy = commit_standalone(&state, id, &staged_fixture("legacy")).unwrap();
        let malformed = commit_standalone(&state, id, &staged_fixture("malformed")).unwrap();
        let bad_note = commit_standalone(&state, id, &staged_fixture("bad-note")).unwrap();
        let bad_pins = commit_standalone(&state, id, &staged_fixture("bad-pins")).unwrap();
        let bad_options = commit_standalone(&state, id, &staged_fixture("bad-options")).unwrap();
        let running = commit_standalone(&state, id, &staged_fixture("running")).unwrap();
        let done = commit_standalone(&state, id, &staged_fixture("done")).unwrap();
        let (interrupted, _) = claim_fixture(&state, &running, "interrupted-run");
        drop(interrupted);
        let (finished, _) = claim_fixture(&state, &done, "finished-run");
        finish(&state, id, &done.id, Outcome::Done(None), 555).unwrap();
        drop(finished);
        {
            let connection = state.store.lock_loupist(id).unwrap();
            let mut old_json = serde_json::to_value(&legacy).unwrap();
            let old_row = old_json.as_object_mut().unwrap();
            for field in ["trust", "origin", "note"] {
                old_row.remove(field);
            }
            old_json["prompt"] = json!("Old synthetic prompt must never run");
            let mut bad_json = serde_json::to_value(&malformed).unwrap();
            bad_json["trust"] = json!({"exposure":"clean","dont_save":"bogus"});
            let corrupt = |row: &Handoff, field: &str, value: Value| {
                let mut stored = serde_json::to_value(row).unwrap();
                stored[field] = value;
                stored
            };
            for (row, stored) in [
                (&legacy, old_json),
                (&malformed, bad_json),
                (
                    &bad_note,
                    corrupt(&bad_note, "note", json!({"title":false})),
                ),
                (
                    &bad_pins,
                    corrupt(&bad_pins, "pins", json!([{"lens":false}])),
                ),
                (
                    &bad_options,
                    corrupt(&bad_options, "link_options", json!({"useMemory":"maybe"})),
                ),
            ] {
                connection
                    .execute(
                        "UPDATE chat_handoffs SET data_json=?1 WHERE id=?2",
                        params![stored.to_string(), row.id],
                    )
                    .unwrap();
            }
        }
        drop(state);

        let state = AppState::for_persistent_test(root.path().join("app"));
        assert_eq!(
            super::super::home::deliver_finished_runs(&state, id).unwrap(),
            7
        );
        assert!(state.run_coordinator.activities().unwrap().is_empty());
        let rows = list(&state, id).unwrap();
        let get = |row: &Handoff| rows.iter().find(|value| value.id == row.id).unwrap();
        assert_eq!(
            get(&legacy).stop_reason,
            Some(HandoffStopReason::BeforeUpdate)
        );
        assert_eq!(
            get(&legacy).error.as_deref(),
            Some("Opened before an update. Ask again to run it.")
        );
        for row in [&malformed, &bad_note, &bad_pins, &bad_options] {
            assert_eq!(
                get(row).stop_reason,
                Some(HandoffStopReason::InvalidAuthority)
            );
        }
        assert_eq!(get(&running).status, HandoffStatus::Stopped);
        assert_eq!(
            get(&running).stop_reason,
            Some(HandoffStopReason::Interrupted)
        );
        assert_eq!(
            get(&running).error.as_deref(),
            Some("The app stopped before the session finished")
        );
        assert_eq!(get(&done).status, HandoffStatus::Done);
        assert_eq!(get(&done).finished_at_ms, Some(555));
        for row in [&legacy, &malformed, &bad_note, &bad_pins, &bad_options] {
            assert_eq!(get(row).status, HandoffStatus::Stopped);
            assert!(get(row).run_id.is_none());
            let session = state.store.get_chat(&row.chat_id).unwrap().unwrap();
            assert!(session.messages.is_empty());
        }
        let home = super::super::home::ensure_home_thread(&state, id).unwrap();
        assert!(home
            .messages
            .iter()
            .filter_map(|message| message.handoff.as_ref())
            .any(|card| {
                card.handoff_id == legacy.id
                    && card.stop_reason == Some(HandoffStopReason::BeforeUpdate)
            }));
        assert!(home
            .messages
            .iter()
            .filter_map(|message| message.handoff.as_ref())
            .any(|card| {
                card.handoff_id == running.id
                    && card.stop_reason == Some(HandoffStopReason::Interrupted)
            }));
        assert_eq!(
            super::super::home::deliver_finished_runs(&state, id).unwrap(),
            0
        );
        assert_eq!(list(&state, id).unwrap(), rows);
    }

    #[test]
    fn an_archived_origin_resolves_by_id_and_a_removed_origin_stops_before_admission() {
        let id = "loupist-source-versions";
        let (_root, state) = state_with_loupist(id);
        let saved = commit_standalone(&state, id, &staged_fixture("archived")).unwrap();
        let missing = commit_standalone(&state, id, &staged_fixture("deleted")).unwrap();
        let home = super::super::home::ensure_home_thread(&state, id).unwrap();
        let mut next = home.clone();
        let original = next.messages.remove(0);
        next.messages.clear();
        next.messages.push(crate::test_fixtures::chat_message(
            "new-edit",
            ChatRole::User,
            "This edit cannot replace the origin.",
        ));
        next.versions.push(crate::domain::ChatVersion {
            id: "version-original".into(),
            after_message_id: None,
            messages: vec![original.clone()],
            archived_at_ms: now_ms(),
        });
        state.store.replace_chat(&home, &next).unwrap();
        let (admission, prompt) = claim_fixture(&state, &saved, "from-original");
        assert_eq!(prompt, original.content);
        drop(admission);
        assert!(matches!(
            claim(&state, id, &missing.id, "from-missing").unwrap(),
            LaunchClaim::Stopped
        ));
        assert!(state.run_coordinator.activities().unwrap().is_empty());
        let rows = list(&state, id).unwrap();
        let stopped = rows.iter().find(|row| row.id == missing.id).unwrap();
        assert_eq!(
            stopped.stop_reason,
            Some(HandoffStopReason::OriginUnavailable)
        );
        assert!(stopped.run_id.is_none());
    }

    #[test]
    fn parse_checks_each_field() {
        let request = HandoffRequest::parse(&json!({
            "title": "Hynix HBM supply",
            "question": "Who else can supply HBM4 in 2027?",
            "brief": "Check capacity announcements.",
            "pins": [{"lens": "rec_a"}, {"world": "node_b"}],
            "requestKey": "hbm-2027"
        }))
        .unwrap();
        assert_eq!(request.question, "Who else can supply HBM4 in 2027?");
        assert_eq!(request.brief, "Check capacity announcements.");
        assert_eq!(request.pins.len(), 2);
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
