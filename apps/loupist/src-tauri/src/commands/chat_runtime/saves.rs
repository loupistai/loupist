//! A reply's own Memory save once its conversation is free for the next turn
//! (docs/runtime/chat-turns.md#saves-that-wait-on-their-own): the sink the
//! save reports through after the reply's stream has ended, the query and
//! event that carry its wait and receipt by message, and the guards that keep
//! history rewrites away from a reply whose save is pending.
use super::*;
use crate::{
    commands::types::{chat_dto_with_memory, ChatMessageDto, ChatSaveState, ChatSavesRequest},
    events::ChatSaveChanged,
    runs::coordinator::{PendingSave, SaveWait},
};

/// The reply a released save belongs to.
#[derive(Clone)]
pub(super) struct ReleasedSave {
    pub state: AppState,
    pub loupist_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub run_id: String,
}

impl ReleasedSave {
    fn publish(&self, pending: Option<ChatSaveState>, message: Option<ChatMessageDto>) {
        publish(ChatSaveChanged {
            loupist_id: self.loupist_id.clone(),
            thread_id: self.thread_id.clone(),
            message_id: self.message_id.clone(),
            run_id: self.run_id.clone(),
            pending,
            message,
        });
    }

    /// Reports the wait, and stores it for a renderer that asks later.
    pub(super) fn waiting(
        &self,
        waiting_for_other_chat: bool,
        blocking_chat_id: Option<String>,
        activity: crate::chat::progress::ChatSaveActivity,
    ) {
        self.state.run_coordinator.note_save_wait(
            &self.run_id,
            SaveWait {
                waiting_for_other_chat,
                activity: Some(activity.clone()),
            },
        );
        self.publish(
            Some(ChatSaveState {
                run_id: self.run_id.clone(),
                thread_id: self.thread_id.clone(),
                message_id: self.message_id.clone(),
                waiting_for_other_chat,
                blocking_chat_id,
                save_activity: activity,
            }),
            None,
        );
    }

    /// The stored reply as the renderer shows it, with its receipt.
    fn stored_message(&self) -> Option<ChatMessageDto> {
        let chat = crate::store::bounded_chat_save(|| {
            self.state
                .store
                .get_chat_in_loupist(&self.loupist_id, &self.thread_id)
        })
        .ok()??;
        chat_dto_with_memory(&self.state, &chat)
            .ok()?
            .messages
            .into_iter()
            .find(|message| message.id == self.message_id)
    }

    fn forward(&self, event: ChatStreamEvent) -> Result<(), ()> {
        match event {
            ChatStreamEvent::Saving {
                waiting_for_other_chat,
                blocking_chat_id,
                save_activity: Some(activity),
                ..
            } => self.waiting(waiting_for_other_chat, blocking_chat_id, activity),
            // Each ends the save once its receipt is on the reply: only now
            // does the save stop holding back edits of that reply.
            ChatStreamEvent::Completed { .. }
            | ChatStreamEvent::Error { .. }
            | ChatStreamEvent::Aborted { .. } => {
                self.state
                    .run_coordinator
                    .note_receipt_written(&self.run_id);
                self.publish(None, self.stored_message());
            }
            _ => {}
        }
        Ok(())
    }
}

impl ChatEventSink {
    /// The sink a save reports through once its reply's stream has ended: it
    /// keeps the turn's run log, refusals and trace, and sends only to the
    /// message-scoped channel. The provider outcome was already recorded.
    pub(super) fn released(&self, save: ReleasedSave) -> Self {
        let record = self
            .run_log
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        Self {
            send: Arc::new(move |event| save.forward(event)),
            baseline: None,
            provider_outcome: None,
            run_log: Arc::new(std::sync::Mutex::new(record)),
            ..self.clone()
        }
    }
}

#[cfg(test)]
static PUBLISHED: std::sync::Mutex<Vec<ChatSaveChanged>> = std::sync::Mutex::new(Vec::new());

/// What this test process published for one save, with each event's place
/// among everything published.
#[cfg(test)]
pub(crate) fn published(run_id: &str) -> Vec<(usize, ChatSaveChanged)> {
    PUBLISHED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .enumerate()
        .filter(|(_, event)| event.run_id == run_id)
        .map(|(index, event)| (index, event.clone()))
        .collect()
}

fn publish(event: ChatSaveChanged) {
    #[cfg(test)]
    PUBLISHED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(event.clone());
    crate::briefings::desktop::emit(crate::events::CHAT_SAVE_CHANGED, event);
}

fn save_state(save: PendingSave) -> Option<ChatSaveState> {
    let activity = save.wait.activity?;
    Some(ChatSaveState {
        run_id: save.run_id,
        thread_id: save.message.thread_id,
        message_id: save.message.message_id,
        waiting_for_other_chat: save.wait.waiting_for_other_chat,
        blocking_chat_id: activity.blocking_chat_id.clone(),
        save_activity: activity,
    })
}

/// The Loupist's replies whose saves wait on their own, oldest first. A
/// renderer reads them after it subscribes to `chat-save-changed`.
#[tauri::command(rename_all = "snake_case")]
pub fn chat_saves(
    request: ChatSavesRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ChatSaveState>, CommandError> {
    pending_save_states(&state, &request.loupist_id)
}

pub(in crate::commands) fn pending_save_states(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<ChatSaveState>, CommandError> {
    require_text(loupist_id, "Loupist", 512)?;
    state.ensure_loupist_available(loupist_id)?;
    Ok(state
        .run_coordinator
        .pending_saves(loupist_id, None)?
        .into_iter()
        .filter_map(save_state)
        .collect())
}

/// The refusal a history rewrite gets while a reply it would move or remove
/// is saving; the same one it got while that reply held the conversation.
fn saving_conflict() -> CommandError {
    CommandError::conflict("this Loupist target already has an active run")
}

/// Refuses a rewrite of the thread (`next`, or deletion with `None`) that
/// would move or remove a reply whose save is pending. Called under the
/// rewrite's own thread registration, so no new save can be released meanwhile.
pub(in crate::commands) fn ensure_saves_kept(
    state: &AppState,
    loupist_id: &str,
    thread_id: &str,
    next: Option<&crate::domain::ChatSession>,
) -> Result<(), CommandError> {
    let pending = state
        .run_coordinator
        .pending_saves(loupist_id, Some(thread_id))?;
    let kept = |message_id: &str| {
        next.is_some_and(|chat| chat.messages.iter().any(|message| message.id == message_id))
    };
    if pending.iter().any(|save| !kept(&save.message.message_id)) {
        return Err(saving_conflict());
    }
    Ok(())
}

/// Refuses a change to one reply while its save is pending; it applies once
/// the save has its receipt.
pub(in crate::commands) fn ensure_not_saving(
    state: &AppState,
    loupist_id: &str,
    thread_id: &str,
    message_id: &str,
) -> Result<(), CommandError> {
    if state
        .run_coordinator
        .pending_saves(loupist_id, Some(thread_id))?
        .iter()
        .any(|save| save.message.message_id == message_id)
    {
        return Err(saving_conflict());
    }
    Ok(())
}

/// What pending saves in this thread hold of Bot memory: the review ids they
/// were offered, which a newer turn is not offered again (a skipped save leaves
/// them queued), and the entries they resolve or forget, which a newer turn
/// does not see. This is the one way a pending save shapes a later turn.
pub(in crate::commands) fn pending_bot_memory(
    state: &AppState,
    loupist_id: &str,
    thread_id: &str,
) -> Result<
    (
        std::collections::BTreeSet<String>,
        crate::chat::bot_memory::Leaving,
    ),
    CommandError,
> {
    let mut review = std::collections::BTreeSet::new();
    let mut leaving = crate::chat::bot_memory::Leaving::default();
    for save in state
        .run_coordinator
        .pending_saves(loupist_id, Some(thread_id))?
    {
        review.extend(save.message.reviewed);
        leaving.extend(save.message.leaving);
    }
    Ok((review, leaving))
}

/// Retries a read-modify-write of one Chat row when another writer won its
/// compare-and-swap. A released save's receipt and the next turn's writes on
/// the same conversation can meet; each re-reads the row and applies its own
/// change again.
pub(in crate::commands) fn retry_chat_cas<T>(
    mut body: impl FnMut() -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    const ATTEMPTS: usize = 4;
    let mut attempt = 1;
    loop {
        match body() {
            Err(error) if attempt < ATTEMPTS && is_chat_cas_conflict(&error) => attempt += 1,
            result => return result,
        }
    }
}

/// A lost Chat compare-and-swap, with or without artifacts (`store.rs`).
pub(in crate::commands) fn is_chat_cas_conflict(error: &CommandError) -> bool {
    error.code == "conflict"
        && matches!(
            error.message.as_str(),
            "Chat compare-and-swap" | "artifact Chat update" | "artifact Chat compare-and-swap"
        )
}

/// Whether `current` differs from `base` only in the receipts and save
/// progress of the same messages: what a pending save writes on its own.
pub(in crate::commands) fn only_receipts_changed(
    base: &crate::domain::ChatSession,
    current: &crate::domain::ChatSession,
) -> bool {
    if base.messages.len() != current.messages.len() {
        return false;
    }
    let mut normalized = current.clone();
    for (message, original) in normalized.messages.iter_mut().zip(&base.messages) {
        if message.id != original.id {
            return false;
        }
        message.memory_update.clone_from(&original.memory_update);
        message.progress.clone_from(&original.progress);
    }
    normalized == *base
}

/// Stores a turn's user message (`chat`, built on `expected`). While the turn
/// set up, an earlier reply's pending save may have written its receipt to
/// the same row: the message is then appended to that newer row. Any other
/// change keeps the conflict.
pub(super) fn append_after_receipts(
    state: &AppState,
    expected: &crate::domain::ChatSession,
    chat: &mut crate::domain::ChatSession,
) -> Result<(), CommandError> {
    let mut base = expected.clone();
    for _ in 0..4 {
        match state.store.replace_chat(&base, chat) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let error = map_store(error);
                if !is_chat_cas_conflict(&error) {
                    return Err(error);
                }
                let current = state
                    .store
                    .get_chat_in_loupist(&chat.loupist_id, &chat.id)
                    .map_err(map_store)?
                    .ok_or_else(|| CommandError::not_found("chat thread"))?;
                if !only_receipts_changed(&base, &current) {
                    return Err(error);
                }
                for (message, stored) in chat.messages.iter_mut().zip(&current.messages) {
                    message.memory_update.clone_from(&stored.memory_update);
                    message.progress.clone_from(&stored.progress);
                }
                chat.updated_at_ms = chat.updated_at_ms.max(current.updated_at_ms);
                base = current;
            }
        }
    }
    Err(CommandError::conflict("Chat compare-and-swap"))
}
