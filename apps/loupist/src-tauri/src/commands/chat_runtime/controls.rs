//! Live steering for an active Chat: queued instructions are persisted in the
//! order Pi accepted them, and rejected once the response has ended.
use super::*;
use crate::store::SqliteStore;

const CHAT_CONTROL_SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

pub(in crate::commands) fn persist_chat_control_message(
    store: &SqliteStore,
    loupist_id: &str,
    thread_id: &str,
    prompt: &str,
) -> Result<AcceptedChatControl, CommandError> {
    let expected = store
        .get_chat(thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    if expected.loupist_id != loupist_id {
        return Err(CommandError::conflict(
            "chat thread does not belong to the selected Loupist",
        ));
    }
    let created_at_ms = now_ms().max(expected.updated_at_ms + 1);
    let message_id = new_id("message");
    let mut chat = expected.clone();
    chat.messages.push(ChatMessage {
        tool_refusals: Vec::new(),
        performance: None,
        feedback: None,
        tool_trace: None,
        schedule_receipts: Vec::new(),
        failure_kind: None,
        host_composed: false,
        steer: true,
        retry_after_seconds: None,
        delivery: None,
        replaced_model: None,
        as_of: None,
        number_checks: Vec::new(),
        sensitivity_cards: Vec::new(),
        as_of_report: None,
        alert: None,
        watch: None,
        earnings: None,
        vocabulary: None,
        handoff: None,
        link: None,
        id: message_id.clone(),
        role: ChatRole::User,
        status: ChatMessageStatus::Complete,
        content: prompt.to_owned(),
        created_at_ms,
        lens_receipts: Vec::new(),
        memory_refs: Vec::new(),
        observed_exact_count: 0,
        refs_truncated: false,
        refs_digest: memory_refs_digest(&[]).map_err(map_internal)?,
        memory_update: None,
        memory_revision: None,
        execution_model: None,
        agent_harness: None,
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        evidence_refs: Vec::new(),
        progress: None,
    });
    // Additional user context is now canonical, so any prior cursor can no
    // longer describe the Chat transcript that the active Pi process receives.
    chat.updated_at_ms = created_at_ms;
    store.replace_chat(&expected, &chat).map_err(map_store)?;
    Ok(AcceptedChatControl {
        message_id,
        created_at_ms,
    })
}

pub(in crate::commands) fn settle_chat_control_response(
    store: &SqliteStore,
    loupist_id: &str,
    thread_id: &str,
    control: ChatControlRequest,
    accepted_by_pi: bool,
) -> Result<(), CommandError> {
    if !accepted_by_pi {
        control.complete(Err(ChatControlError::Rejected(
            "Pi rejected the queued instruction".into(),
        )));
        return Ok(());
    }

    let accepted = match persist_chat_control_message(store, loupist_id, thread_id, &control.prompt)
    {
        Ok(accepted) => accepted,
        Err(error) => {
            // Pi accepted the steer, but continuing would produce a response
            // that cannot be replayed from the canonical transcript.
            control.complete(Err(ChatControlError::Rejected(
                "Could not save the queued instruction".into(),
            )));
            return Err(error);
        }
    };
    control.complete(Ok(accepted));
    Ok(())
}

pub(super) fn reject_pending_chat_controls(
    pending_controls: HashMap<u64, (u64, ChatControlRequest)>,
    settled_controls: BTreeMap<u64, (ChatControlRequest, bool)>,
) {
    for (_, (_, control)) in pending_controls {
        control.complete(Err(ChatControlError::Rejected(
            "The active response ended before the queued instruction was accepted".into(),
        )));
    }
    for (_, (control, _)) in settled_controls {
        control.complete(Err(ChatControlError::Rejected(
            "The active response ended before the queued instruction was accepted".into(),
        )));
    }
}

pub(super) fn reject_queued_chat_controls(chat_controls: &mut ChatControlReceiver) {
    while let Some(control) = chat_controls.try_recv() {
        control.complete(Err(ChatControlError::Rejected(
            "The active response ended before the queued instruction was accepted".into(),
        )));
    }
}

pub(super) fn record_chat_control_response(
    pending_controls: &mut HashMap<u64, (u64, ChatControlRequest)>,
    settled_controls: &mut BTreeMap<u64, (ChatControlRequest, bool)>,
    request_id: u64,
    accepted_by_pi: bool,
) -> bool {
    let Some((sequence, control)) = pending_controls.remove(&request_id) else {
        return false;
    };
    settled_controls.insert(sequence, (control, accepted_by_pi));
    true
}

pub(super) fn persist_contiguous_chat_controls(
    store: &SqliteStore,
    loupist_id: &str,
    thread_id: &str,
    settled_controls: &mut BTreeMap<u64, (ChatControlRequest, bool)>,
    next_control_to_persist: &mut u64,
) -> Result<(), CommandError> {
    while let Some((control, accepted_by_pi)) = settled_controls.remove(next_control_to_persist) {
        settle_chat_control_response(store, loupist_id, thread_id, control, accepted_by_pi)?;
        *next_control_to_persist += 1;
    }
    Ok(())
}

pub(in crate::commands) async fn settle_chat_controls_before_completion(
    pi_events: &mut tokio::sync::broadcast::Receiver<PiEvent>,
    pending_controls: &mut HashMap<u64, (u64, ChatControlRequest)>,
    settled_controls: &mut BTreeMap<u64, (ChatControlRequest, bool)>,
    next_control_to_persist: &mut u64,
    store: &SqliteStore,
    loupist_id: &str,
    thread_id: &str,
) -> Result<(), CommandError> {
    let deadline = Instant::now() + CHAT_CONTROL_SETTLE_TIMEOUT;
    loop {
        persist_contiguous_chat_controls(
            store,
            loupist_id,
            thread_id,
            settled_controls,
            next_control_to_persist,
        )?;
        if pending_controls.is_empty() && settled_controls.is_empty() {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CommandError::internal(
                "Pi did not confirm queued instructions before the response ended",
            ));
        }
        match timeout(remaining, pi_events.recv()).await {
            Ok(Ok(PiEvent::Rpc { payload })) => {
                if payload.get("type").and_then(Value::as_str) != Some("response") {
                    continue;
                }
                let Some(request_id) = payload.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                if record_chat_control_response(
                    pending_controls,
                    settled_controls,
                    request_id,
                    payload.get("success").and_then(Value::as_bool) == Some(true),
                ) {
                    continue;
                }
            }
            Ok(Ok(PiEvent::ProtocolError { .. }))
            | Ok(Ok(PiEvent::Exited))
            | Ok(Err(tokio::sync::broadcast::error::RecvError::Closed))
            | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_)))
            | Err(_) => {
                return Err(CommandError::internal(
                    "Pi did not confirm queued instructions before the response ended",
                ));
            }
        }
    }
}
