use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::State;
use uuid::Uuid;

use crate::{
    app::{AppState, CommandError, QuarantineSource},
    chat::artifacts::{ChatArtifact, ChatArtifactVersion},
    domain::{ChatRole, ChatSession, ChatVersion, MemoryPolicy, MAX_CHAT_VERSIONS},
    fs::loupist_root::profile_workspace,
    runs::coordinator::{RunKind, RunTarget},
    runs::maintenance::MaintenanceActivityKind,
    store::deletion,
    store::LoupistStore,
};

use super::{
    attachments::{chat_attachment_message_dir, read_chat_attachment_file},
    map_store, new_id, now_ms, require_text,
    types::{
        chat_dto, ChatArtifactListRequest, ChatArtifactReadRequest, ChatArtifactRestoreRequest,
        ChatArtifactViewDto, ChatAttachmentReadDto, ChatAttachmentReadRequest, ChatBranchRequest,
        ChatCreateRequest, ChatDeleteRequest, ChatFeedbackRequest, ChatMemorySetAsideRequest,
        ChatRenameRequest, ChatThreadDto, ChatVersionSwitchRequest, LoupistArtifactItemDto,
        LoupistArtifactListRequest,
    },
    MAX_CHAT_TITLE_BYTES,
};

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_create(
    request: ChatCreateRequest,
    state: State<'_, AppState>,
) -> Result<ChatThreadDto, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    let profile = state
        .store
        .get_loupist(&request.loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    profile_workspace(&profile)?;
    let title = match request.title {
        Some(title) => require_text(&title, "chat title", MAX_CHAT_TITLE_BYTES)?,
        None => "".into(),
    };
    let defaults = super::loupist_configuration(&state, &profile.id)?.chat;
    let timestamp = now_ms();
    let chat = ChatSession {
        id: new_id("chat"),
        loupist_id: profile.id,
        pi_session_id: Uuid::new_v4().to_string(),
        pi_session_cache: None,
        title,
        memory_policy: MemoryPolicy {
            use_memory: defaults.use_memory,
            update_memory: defaults.update_memory,
        },
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: Vec::new(),
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
        bot_memory: Default::default(),
    };
    state.store.create_chat(&chat).map_err(map_store)?;
    chat_dto(&chat)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_rename(
    request: ChatRenameRequest,
    state: State<'_, AppState>,
) -> Result<ChatThreadDto, CommandError> {
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let expected = artifact_chat(&state, &request.loupist_id, &request.thread_id)?;
    if crate::chat::home::is_home(&expected) {
        return Err(CommandError::conflict("the home Thread keeps its name"));
    }
    let mut chat = expected.clone();
    chat.title = require_text(&request.title, "chat title", MAX_CHAT_TITLE_BYTES)?;
    chat.updated_at_ms = now_ms().max(chat.updated_at_ms.saturating_add(1));
    state
        .store
        .replace_chat(&expected, &chat)
        .map_err(map_store)?;
    chat_dto(&chat)
}

/// Edit forks (X18): editing a sent user message keeps everything before it and
/// sets the rest aside as an earlier version of the same chat, so the edited
/// message can be sent in its place. Versions stay in this chat (attachments and
/// artifacts keep resolving); the Pi session restarts from SQLite on the next turn.
#[tauri::command(rename_all = "snake_case")]
pub async fn chat_branch(
    request: ChatBranchRequest,
    state: State<'_, AppState>,
) -> Result<ChatThreadDto, CommandError> {
    branch_chat(&state, request)
}

pub(super) fn branch_chat(
    state: &AppState,
    request: ChatBranchRequest,
) -> Result<ChatThreadDto, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let expected = artifact_chat(state, &request.loupist_id, &request.thread_id)?;
    let by_place = || {
        let position = usize::try_from(request.position?).ok()?;
        let message = expected.messages.get(position)?;
        (message.role == ChatRole::User && Some(&message.content) == request.content.as_ref())
            .then_some(position)
    };
    let position = expected
        .messages
        .iter()
        .position(|message| message.id == request.message_id)
        .or_else(by_place)
        .ok_or_else(|| CommandError::not_found("message"))?;
    if expected.messages[position].role != ChatRole::User {
        return Err(CommandError::invalid(
            "only a message you sent can be edited",
        ));
    }
    let timestamp = now_ms();
    let mut chat = expected.clone();
    let tail = chat.messages.split_off(position);
    keep_version(&mut chat, position, tail, timestamp);
    restart_session(&mut chat, timestamp);
    state
        .store
        .replace_chat(&expected, &chat)
        .map_err(map_store)?;
    chat_dto(&chat)
}

/// Shows another version of the chat's tail ("Earlier version" / "Later
/// version"); the one shown until now is kept as a version in turn.
#[tauri::command(rename_all = "snake_case")]
pub async fn chat_version_switch(
    request: ChatVersionSwitchRequest,
    state: State<'_, AppState>,
) -> Result<ChatThreadDto, CommandError> {
    switch_version(&state, request)
}

pub(super) fn switch_version(
    state: &AppState,
    request: ChatVersionSwitchRequest,
) -> Result<ChatThreadDto, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let expected = artifact_chat(state, &request.loupist_id, &request.thread_id)?;
    let mut chat = expected.clone();
    let index = chat
        .versions
        .iter()
        .position(|version| version.id == request.version_id)
        .ok_or_else(|| CommandError::not_found("chat version"))?;
    let version = chat.versions.remove(index);
    let position = match &version.after_message_id {
        None => 0,
        Some(anchor) => chat
            .messages
            .iter()
            .position(|message| &message.id == anchor)
            .map(|found| found + 1)
            .ok_or_else(|| {
                CommandError::conflict("this version no longer follows the chat as it is now")
            })?,
    };
    let timestamp = now_ms();
    let shown = chat.messages.split_off(position);
    keep_version(&mut chat, position, shown, timestamp);
    chat.messages.extend(version.messages);
    restart_session(&mut chat, timestamp);
    state
        .store
        .replace_chat(&expected, &chat)
        .map_err(map_store)?;
    chat_dto(&chat)
}

/// Keeps a set-aside tail as a version, dropping the oldest beyond the limit.
fn keep_version(
    chat: &mut ChatSession,
    position: usize,
    tail: Vec<crate::domain::ChatMessage>,
    timestamp: i64,
) {
    if tail.is_empty() {
        return;
    }
    let after_message_id = position
        .checked_sub(1)
        .map(|index| chat.messages[index].id.clone());
    chat.versions.push(ChatVersion {
        id: new_id("version"),
        after_message_id,
        messages: tail,
        archived_at_ms: timestamp,
    });
    while chat.versions.len() > MAX_CHAT_VERSIONS {
        chat.versions.remove(0);
    }
}

/// The Pi transcript no longer matches the shown messages; the next turn starts
/// cold and reads this version's history from SQLite (as after a Thread delivery).
fn restart_session(chat: &mut ChatSession, timestamp: i64) {
    chat.pi_session_cache = None;
    chat.updated_at_ms = timestamp.max(chat.updated_at_ms.saturating_add(1));
}

/// "Not this time" for a principle or method that shaped an answer: the host
/// leaves it out of this chat's context until the user brings it back.
#[tauri::command(rename_all = "snake_case")]
pub async fn chat_memory_set_aside(
    request: ChatMemorySetAsideRequest,
    state: State<'_, AppState>,
) -> Result<ChatThreadDto, CommandError> {
    set_aside_memory(&state, request)
}

pub(super) fn set_aside_memory(
    state: &AppState,
    request: ChatMemorySetAsideRequest,
) -> Result<ChatThreadDto, CommandError> {
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let expected = artifact_chat(state, &request.loupist_id, &request.thread_id)?;
    let mut chat = expected.clone();
    let present = chat.set_aside_memory.contains(&request.record_id);
    if request.set_aside && !present {
        if !super::knowledge::is_loupist_lens_record(
            state,
            &request.loupist_id,
            &request.record_id,
        )? {
            return Err(CommandError::not_found("principle or method"));
        }
        if chat.set_aside_memory.len() >= crate::domain::MAX_SET_ASIDE_MEMORY {
            return Err(CommandError::invalid(
                "too many principles and methods are set aside in this chat",
            ));
        }
        chat.set_aside_memory.push(request.record_id.clone());
    } else if !request.set_aside && present {
        chat.set_aside_memory.retain(|id| id != &request.record_id);
    } else {
        return chat_dto(&chat);
    }
    chat.updated_at_ms = now_ms().max(chat.updated_at_ms.saturating_add(1));
    state
        .store
        .replace_chat(&expected, &chat)
        .map_err(map_store)?;
    chat_dto(&chat)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_feedback_set(
    request: ChatFeedbackRequest,
    state: State<'_, AppState>,
) -> Result<crate::domain::ChatFeedback, CommandError> {
    set_feedback(&state, request)
}

pub(super) fn set_feedback(
    state: &AppState,
    request: ChatFeedbackRequest,
) -> Result<crate::domain::ChatFeedback, CommandError> {
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let expected = artifact_chat(state, &request.loupist_id, &request.thread_id)?;
    let mut chat = expected.clone();
    let message = chat
        .messages
        .iter_mut()
        .find(|message| message.id == request.message_id)
        .ok_or_else(|| CommandError::not_found("chat message"))?;
    if message.role != crate::domain::ChatRole::Assistant
        || message.status != crate::domain::ChatMessageStatus::Complete
    {
        return Err(CommandError::conflict(
            "feedback requires a completed assistant message",
        ));
    }
    let correction = request
        .correction
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    if correction
        .as_ref()
        .is_some_and(|text| text.chars().count() > 4000)
    {
        return Err(CommandError::conflict("correction exceeds 4000 characters"));
    }
    let feedback = crate::domain::ChatFeedback {
        rating: request.rating,
        correction,
        updated_at: now_ms(),
    };
    message.feedback = Some(feedback.clone());
    chat.updated_at_ms = feedback
        .updated_at
        .max(chat.updated_at_ms.saturating_add(1));
    state
        .store
        .replace_chat(&expected, &chat)
        .map_err(map_store)?;
    Ok(feedback)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_delete(
    request: ChatDeleteRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let expected = artifact_chat(&state, &request.loupist_id, &request.thread_id)?;
    if crate::chat::home::is_home(&expected) {
        return Err(CommandError::conflict("the home Thread cannot be deleted"));
    }
    let durable_root = state.loupist_files_root(&expected.loupist_id)?;
    let timestamp = now_ms();
    let result = if state.store.is_folder_catalog() {
        deletion::delete_chat_with_runtime(
            state.store.as_ref(),
            durable_root.as_ref(),
            state.artifacts.deletion_root.as_ref(),
            &expected,
            timestamp,
        )
    } else {
        deletion::delete_chat(
            state.store.as_ref(),
            durable_root.as_ref(),
            &expected,
            timestamp,
        )
    };
    if let Err(error) = result {
        if state
            .store
            .get_chat(&expected.id)
            .map_err(map_store)?
            .is_some()
            && deletion::has_pending_for(state.store.as_ref(), &expected.loupist_id, &expected.id)
                .unwrap_or(true)
        {
            state.quarantine_loupist(
                &expected.loupist_id,
                QuarantineSource::Deletion,
                "Chat deletion recovery is required before this Loupist can be used",
            );
        }
        return Err(error);
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub fn chat_attachment_read(
    request: ChatAttachmentReadRequest,
    state: State<'_, AppState>,
) -> Result<ChatAttachmentReadDto, CommandError> {
    let chat = artifact_chat(&state, &request.loupist_id, &request.thread_id)?;
    let message = chat
        .messages
        .iter()
        .find(|message| message.id == request.message_id)
        .ok_or_else(|| CommandError::not_found("chat message"))?;
    let attachment = message
        .attachments
        .iter()
        .find(|attachment| attachment.id == request.attachment_id)
        .ok_or_else(|| CommandError::not_found("chat attachment"))?;
    let path = chat_attachment_message_dir(
        &state,
        &request.loupist_id,
        &request.thread_id,
        &request.message_id,
    )?
    .join(&attachment.id);
    let bytes = read_chat_attachment_file(&path, attachment)?;
    Ok(ChatAttachmentReadDto {
        data_url: format!(
            "data:{};base64,{}",
            attachment.media_type,
            BASE64.encode(bytes)
        ),
    })
}

fn artifact_chat(
    state: &AppState,
    loupist_id: &str,
    thread_id: &str,
) -> Result<ChatSession, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    let chat = state
        .store
        .get_chat(thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    if chat.loupist_id != loupist_id {
        return Err(CommandError::conflict(
            "chat thread does not belong to the selected Loupist",
        ));
    }
    Ok(chat)
}

#[tauri::command(rename_all = "snake_case")]
pub fn chat_artifact_list(
    request: ChatArtifactListRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ChatArtifact>, CommandError> {
    artifact_chat(&state, &request.loupist_id, &request.thread_id)?;
    state
        .store
        .list_chat_artifacts(&request.thread_id)
        .map_err(map_store)
}

/// At most this many rows; the Work list shows 50 and then "Show older".
const MAX_LOUPIST_ARTIFACTS: usize = 200;

/// The Loupist's charts and documents across its chats, newest first, with each chat's title.
#[tauri::command(rename_all = "snake_case")]
pub fn loupist_artifact_list(
    request: LoupistArtifactListRequest,
    state: State<'_, AppState>,
) -> Result<Vec<LoupistArtifactItemDto>, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let limit = request.limit.unwrap_or(50).clamp(1, MAX_LOUPIST_ARTIFACTS);
    Ok(state
        .store
        .list_loupist_artifacts(&request.loupist_id, limit)
        .map_err(map_store)?
        .into_iter()
        .map(|(artifact, chat_title)| LoupistArtifactItemDto {
            artifact,
            chat_title,
        })
        .collect())
}

#[tauri::command(rename_all = "snake_case")]
pub fn chat_artifact_read(
    request: ChatArtifactReadRequest,
    state: State<'_, AppState>,
) -> Result<ChatArtifactViewDto, CommandError> {
    read_artifact(&state, request)
}

fn read_artifact(
    state: &AppState,
    request: ChatArtifactReadRequest,
) -> Result<ChatArtifactViewDto, CommandError> {
    artifact_chat(state, &request.loupist_id, &request.thread_id)?;
    owned_artifact(state, &request.thread_id, &request.artifact_id)?;
    let snapshot = state
        .store
        .read_chat_artifact(&request.artifact_id, request.revision_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("artifact revision"))?;
    Ok(ChatArtifactViewDto::from(snapshot))
}

fn owned_artifact(
    state: &AppState,
    thread_id: &str,
    artifact_id: &str,
) -> Result<ChatArtifact, CommandError> {
    let artifact = state
        .store
        .get_chat_artifact(artifact_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("artifact"))?;
    if artifact.chat_session_id != thread_id {
        return Err(CommandError::conflict(
            "artifact does not belong to this Chat thread",
        ));
    }
    Ok(artifact)
}

#[tauri::command(rename_all = "snake_case")]
pub fn artifact_versions(
    request: ChatArtifactReadRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ChatArtifactVersion>, CommandError> {
    artifact_chat(&state, &request.loupist_id, &request.thread_id)?;
    owned_artifact(&state, &request.thread_id, &request.artifact_id)?;
    state
        .store
        .list_chat_artifact_versions(&request.artifact_id)
        .map_err(map_store)
}

#[tauri::command(rename_all = "snake_case")]
pub fn artifact_restore(
    request: ChatArtifactRestoreRequest,
    state: State<'_, AppState>,
) -> Result<ChatArtifactViewDto, CommandError> {
    let _mutation = state.register_run(
        new_id("artifact-restore"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let chat = artifact_chat(&state, &request.loupist_id, &request.thread_id)?;
    owned_artifact(&state, &request.thread_id, &request.artifact_id)?;
    let snapshot = state
        .store
        .restore_chat_artifact(
            &chat,
            &request.artifact_id,
            request.revision_id,
            request.expected_current_revision,
            now_ms(),
        )
        .map_err(map_store)?;
    Ok(ChatArtifactViewDto::from(snapshot))
}
