//! A chosen model its provider no longer offers (404 `model_not_found`). The
//! provider processed nothing, so within the same turn Rust prunes the model,
//! remembers it as gone, resolves Auto once and asks once more on a fresh Pi
//! (providers spec §3.1). The renderer never resends on its own; the reply
//! names the replaced model.
use super::launch::{LaunchedChatTurn, PiCacheSeal};
use super::stream::StreamedChatTurn;
use super::*;

/// What the one retry needs to ask the same question again from the start.
pub(super) struct GoneRetrySeed {
    pub pi_config: PiLaunchConfig,
    /// The full turn context, before any abbreviation for a warm session.
    pub turn_envelope: String,
    pub current_prompt: String,
    pub chat_id: String,
    /// The Chat before this turn's request, for the cold bootstrap.
    pub history: Vec<ChatMessage>,
    pub chat_attachments: PathBuf,
    pub current_image_bytes: usize,
    pub current_pi_images: Vec<PiImageContent>,
}

/// Retry only a turn that failed before anything happened: no text, no tool
/// call and no accepted steer, and only once.
pub(super) fn retryable(turn: &LaunchedChatTurn, streamed: &StreamedChatTurn) -> bool {
    turn.gone_retry.is_some()
        && streamed.failure.as_deref() == Some(crate::pi::provider_failure::MODEL_GONE)
        && !streamed.completed
        && !streamed.aborted
        && streamed.content.is_empty()
        && streamed.pending_controls.is_empty()
        && streamed.settled_controls.is_empty()
        && !streamed.progress.has_tool_activity()
}

pub(super) enum Relaunch {
    /// The same turn runs again on Auto; stream it.
    Retried(Box<LaunchedChatTurn>),
    /// Auto had nothing to offer; finalize the original failure.
    Unchanged(Box<LaunchedChatTurn>),
    /// Starting the retry failed after the first Pi stopped; the reply is recorded.
    Settled,
}

/// Stops the failed Pi and starts the same turn on Auto.
pub(super) async fn relaunch_on_auto(mut turn: LaunchedChatTurn) -> Relaunch {
    let Some(seed) = turn.gone_retry.take() else {
        return Relaunch::Unchanged(Box::new(turn));
    };
    let gone = turn.execution_model.clone();
    crate::providers::auto::prune_gone(&turn.app_state, &gone.profile_id);
    // A gone choice falls back to Auto with Auto's own controls.
    let Ok(mut execution) = turn.app_state.pi_execution("", "", &BTreeMap::new()) else {
        return Relaunch::Unchanged(Box::new(turn));
    };
    let seed = *seed;
    execution.pi_settings = seed.pi_config.pi_settings.clone();
    let (envelope, sent_envelope_digest) =
        agent_harness::maybe_abbreviate_envelope(&seed.turn_envelope, None, false);
    let envelope = agent_harness::render_turn_context(&envelope);
    let mut images = Vec::new();
    let Ok(prompt) = pi_chat_turn_prompt(
        &seed.current_prompt,
        &envelope,
        Some(ColdChatBootstrap {
            attachments_root: &seed.chat_attachments,
            thread_id: &seed.chat_id,
            history: &seed.history,
            current_image_bytes: seed.current_image_bytes,
            current_image_count: seed.current_pi_images.len(),
            historical_images: &mut images,
        }),
    ) else {
        return Relaunch::Unchanged(Box::new(turn));
    };
    images.extend(seed.current_pi_images);
    let mut config = seed.pi_config;
    config.provider = execution.artifacts.provider.clone();
    config.model = execution.artifacts.model.clone();
    config.thinking_level = execution.artifacts.thinking_level.clone();
    config.run_options = execution.artifacts.run_options.clone();
    config.provider_credential = execution.artifacts.provider_credential.clone();

    // Pi owns the session file: stop the failed child before the cold start
    // wipes it. From here a failure records the reply itself.
    let failed = turn.pi;
    let _ = shutdown_failed_chat_setup(failed, turn.turn_resources.pi_session_mut()).await;
    let started = start_on_auto(
        config,
        &execution,
        &mut turn.turn_resources,
        &mut turn.cancel,
        &prompt,
        &images,
    )
    .await;
    let (pi, pi_events, execution_model, session_state, prompt_request_id) = match started {
        Ok(started) => started,
        Err((kind, error)) => {
            turn.on_event
                .set_failure_kind(classify_turn_failure(&error, kind));
            let _ = turn.broker.shutdown().await;
            turn.capture.shutdown_mcp().await;
            TerminalReply {
                state: &turn.app_state,
                on_event: &turn.on_event,
                run_id: &turn.run_id,
                thread_id: &turn.thread_id,
                message_id: &turn.message_id,
                memory_revision: &turn.memory_revision,
                harness: &turn.harness,
            }
            .failed_with_stop_precedence(execution.model_lock(), None);
            return Relaunch::Settled;
        }
    };
    turn.pi = pi;
    turn.pi_events = pi_events;
    turn.prompt_request_id = prompt_request_id;
    turn.resumed = false;
    turn.provider_wait_started_at = Instant::now();
    turn.cache_seal = PiCacheSeal {
        derived_session_id: session_state.session_id,
        sent_envelope_digest,
        ..turn.cache_seal
    };
    turn.capture.set_chat_provider(&execution_model.provider);
    if let Some((_, record)) = turn
        .on_event
        .run_log
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_mut()
    {
        record.model(&execution_model);
    }
    turn.on_event.set_failure_kind(ChatFailureKind::Other);
    turn.on_event.set_replaced_model(&gone.name);
    turn.execution_model = execution_model;
    Relaunch::Retried(Box::new(turn))
}

type StartedOnAuto = (
    PiProcess,
    tokio::sync::broadcast::Receiver<PiEvent>,
    ExecutionModelLock,
    PiSessionState,
    u64,
);

/// Launches Pi cold on the Auto model and sends the question again.
async fn start_on_auto(
    config: PiLaunchConfig,
    execution: &PiExecutionConfig,
    turn_resources: &mut ChatTurnResources,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    prompt: &str,
    images: &[PiImageContent],
) -> Result<StartedOnAuto, (ChatFailureKind, CommandError)> {
    let (pi, pi_events, execution_model, session_state, _) = launch_chat_pi_resuming(
        config,
        execution,
        turn_resources.pi_session_mut(),
        ChatPiResume::Cold {
            session_id: uuid::Uuid::new_v4().to_string(),
        },
        cancel,
    )
    .await
    .map_err(|error| (ChatFailureKind::PiExited, error))?;
    match pi.prompt_with_images(prompt, images).await {
        Ok(prompt_request_id) => Ok((
            pi,
            pi_events,
            execution_model,
            session_state,
            prompt_request_id,
        )),
        Err(error) => {
            let _ = shutdown_failed_chat_setup(pi, turn_resources.pi_session_mut()).await;
            Err((
                ChatFailureKind::ProtocolError,
                CommandError::internal(error.to_string()),
            ))
        }
    }
}
