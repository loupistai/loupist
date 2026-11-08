//! Setup phase: validate the request, admit the run, resolve the Loupist,
//! Chat, model and authority, and durably append the user turn. The turn is
//! frozen once here; Pi, broker and cache views derive from it.
use super::*;
use crate::commands::{loupist_configuration, ChatConnectorAuthority};
use crate::{
    agent_harness::AgentHostContext,
    chat::activity::ActivityRecorder,
    commands::{attachments::PreparedChatAttachment, knowledge::HostMemoryContext},
    domain::{ChatSession, LoupistProfile},
    fs::loupist_root::{profile_workspace, BoundLoupistRoot},
    knowledge::WorkingNote,
    providers::catalog::ConfiguredModel,
};

pub(super) struct PreparedChatTurn {
    pub execution: PiExecutionConfig,
    pub context: AgentHostContext,
    pub skill_files: Vec<PathBuf>,
    pub connector_authority: ChatConnectorAuthority,
    pub web_search_policy: crate::web::WebSearchPolicy,
    pub bound_workspace: BoundLoupistRoot,
    pub runtime_policy_sha256: String,
    pub runtime_surface_sha256: String,
}

impl PreparedChatTurn {
    pub fn resolve(
        state: &AppState,
        request: &ChatSendRequest,
        profile: &LoupistProfile,
        mut execution: PiExecutionConfig,
        prompt: &str,
        interactive: bool,
    ) -> Result<Self, CommandError> {
        let config = loupist_configuration(state, &profile.id)?;
        let skill_ids = config.enabled_skill_ids();
        let run_skill_ids = agent_harness::run_skill_ids(&skill_ids).map_err(map_internal)?;
        let agent_root =
            execution.artifacts.system_prompt.parent().ok_or_else(|| {
                CommandError::new("pi_unavailable", "Pi agent resource is invalid")
            })?;
        let skill_files = agent_harness::resolve_skill_paths(agent_root, &run_skill_ids)
            .map_err(|error| CommandError::new("pi_unavailable", error.to_string()))?;
        let connector_authority = capture_chat_connector_authority(state, &profile.id)?;
        let capabilities = &connector_authority.capability_ids;
        let web_search_policy = if capabilities.iter().any(|id| id == "community.web-research") {
            crate::marketplace::web_research_policy(state)?
        } else {
            crate::web::WebSearchPolicy::default()
        };
        let harness =
            agent_harness::snapshot("chat", &skill_ids, capabilities).map_err(map_internal)?;
        let runtime = agent_harness::AgentRuntimeProfile::new(
            "chat",
            request.use_memory,
            request.update_memory,
            capabilities,
        )
        .map_err(map_internal)?;
        let surface = crate::chat::surface::ChatSurface::resolve(
            state,
            &profile.id,
            &request.thread_id,
            interactive,
        )?;
        let runtime = surface.restrict(runtime);
        let preload_component_ids = agent_harness::preload_component_ids_for_mentions(
            &request.mentioned_connection_ids,
            capabilities,
            &runtime,
        )
        .map_err(map_internal)?;
        let bound_workspace = profile_workspace(profile)?;
        let workspace = agent_harness::workspace::workspace_snapshot_with_preferences(
            bound_workspace.path(),
            &state.artifacts.app_data_dir,
            &config.skills,
        )
        .map_err(map_internal)?;
        let runtime_policy_sha256 = pi_session_cache_runtime_policy_sha256(
            request.as_of.as_deref(),
            web_search_policy,
            &workspace.digest,
            &config.execution_digest(),
        )?;
        let runtime_surface_sha256 = crate::util::sha256(
            format!(
                "{}:{}",
                pi_session_cache_runtime_surface_sha256(capabilities)?,
                surface.name()
            )
            .as_bytes(),
        );
        execution.pi_settings = config.pi;
        Ok(Self {
            execution,
            skill_files,
            connector_authority,
            web_search_policy,
            bound_workspace,
            runtime_policy_sha256,
            runtime_surface_sha256,
            context: AgentHostContext {
                agent_harness: harness,
                agent_runtime: runtime,
                workspace,
                preload_component_ids,
                // Parse only raw current user intent, before attachments/history.
                explicit_skill_names: agent_harness::explicit_skill_names(prompt),
                surface,
            },
        })
    }
}

/// Context gathered during setup that launch renders into the turn envelope.
pub(super) struct TurnEnvelopeInputs {
    pub mentioned_connections: Vec<agent_harness::MentionedConnection>,
    pub selected_artifact: Option<Value>,
    pub host_memory: Option<HostMemoryContext>,
    pub followed: Option<Value>,
    pub selected_references: Option<Value>,
    pub working_context: Option<WorkingNote>,
    /// Memory the as-of cutoff left out of the automatic context.
    pub left_out: crate::commands::knowledge::as_of::LeftOut,
}

/// Setup → launch handoff: the admitted run, its durable user turn and the
/// resolved model, authority and context that Pi needs.
///
/// Fields with a drop effect follow the order in which setup creates them, and
/// launch binds them in field order. An early launch exit therefore drops them
/// as the single command function did: turn resources first, then the
/// activity log, the Stop and steer channels, and the sink last.
pub(super) struct AdmittedChatTurn {
    #[cfg(feature = "eval-harness")]
    pub pi_agent_data_dir: PathBuf,
    pub request: ChatSendRequest,
    pub on_event: ChatEventSink,
    pub surface: crate::chat::surface::ChatSurface,
    pub untrusted_context: bool,
    pub chat_started_at: Instant,
    pub run_id: String,
    pub chat_controls: ChatControlReceiver,
    pub cancel: tokio::sync::watch::Receiver<bool>,
    pub progress_started_at_ms: i64,
    pub prompt: String,
    pub prepared_attachments: Vec<PreparedChatAttachment>,
    pub pi_artifacts: crate::app::PiArtifacts,
    pub configured_model: ConfiguredModel,
    pub profile: LoupistProfile,
    pub pi_execution: PiExecutionConfig,
    pub skill_files: Vec<PathBuf>,
    pub connector_authority: ChatConnectorAuthority,
    pub web_search_policy: crate::web::WebSearchPolicy,
    /// Held, unused, until launch ends: the workspace stays pinned while Pi starts.
    pub bound_workspace: BoundLoupistRoot,
    pub runtime_policy_sha256: String,
    pub runtime_surface_sha256: String,
    pub policy: crate::broker::ToolPolicy,
    pub native_roots: crate::agent_harness::native_policy::NativeRoots,
    /// An earlier reply in this Chat read third-party text, so the model's
    /// context may carry it into this clean turn.
    pub carried_untrusted: bool,
    pub host_context: String,
    pub harness: AgentHarnessSnapshot,
    pub chat_attachments: PathBuf,
    pub chat: ChatSession,
    pub had_prior_messages: bool,
    pub envelope: TurnEnvelopeInputs,
    pub activity_recorder: ActivityRecorder,
    pub memory_revision: Option<String>,
    pub turn_resources: ChatTurnResources,
    pub user_message_id: String,
    pub current_prompt: String,
    pub current_image_bytes: usize,
    pub current_pi_images: Vec<PiImageContent>,
    pub intended_cache: Option<PiSessionCache>,
    pub should_generate_title: bool,
    pub assistant_message_id: String,
}

/// An as-of date is a calendar day from 1990-01-01 to today on this device
/// (finance-point-in-time.md §3.1).
pub(in crate::commands) fn validate_turn_as_of(as_of: Option<&str>) -> Result<(), CommandError> {
    let Some(value) = as_of else {
        return Ok(());
    };
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|_| value.len() == 10)
        .ok_or_else(|| CommandError::invalid("as_of must be a date in YYYY-MM-DD format"))?;
    let earliest = chrono::NaiveDate::from_ymd_opt(1990, 1, 1).expect("valid date");
    if date < earliest {
        return Err(CommandError::invalid("as_of must be 1990-01-01 or later"));
    }
    if date > chrono::Local::now().date_naive() {
        return Err(CommandError::invalid("as_of must be today or earlier"));
    }
    Ok(())
}

/// `Err` returns before the user turn is appended. `Break` means the user
/// turn is durable and its reply already ended (failed, or stopped first).
pub(super) fn admit_chat_turn(
    request: ChatSendRequest,
    mut on_event: ChatEventSink,
    state: &AppState,
    admission: Option<ChatAdmission>,
) -> Result<ControlFlow<RunStarted, AdmittedChatTurn>, CommandError> {
    // Resolve the eval/live override before admission, events, scratch files
    // or a durable user message. Launch must not discover this failure after
    // starting the broker.
    #[cfg(feature = "eval-harness")]
    let pi_agent_data_dir = state.pi_agent_data_dir()?;
    let interactive = admission.is_none();
    let chat_started_at = Instant::now();
    on_event.tool_trace = Arc::default();
    state.ensure_loupist_available(&request.loupist_id)?;
    let run_id = validated_chat_run_id(&request.run_id)?;
    require_text(&request.loupist_id, "Loupist", 512)?;
    require_text(&request.thread_id, "Chat thread", 512)?;
    // Stop remains available while subsequent setup awaits external processes.
    let admission = match admission {
        Some(admission)
            if admission.run_id == run_id
                && admission.loupist_id == request.loupist_id
                && admission.thread_id == request.thread_id =>
        {
            admission
        }
        Some(_) => return Err(CommandError::invalid("Chat admission binding changed")),
        None => ChatAdmission::new(state, &run_id, &request.loupist_id, &request.thread_id)?,
    };
    let untrusted_context = admission.untrusted_text;
    let registration = admission.registration;
    let chat_controls = admission.controls;
    let cancel = registration.cancel;
    let run_lease = registration.lease;
    let run_scratch = crate::runs::scratch::RunScratch::create(
        state.artifacts.deletion_root.clone(),
        &request.loupist_id,
        &run_id,
    )?;
    let _ = on_event.send(ChatStreamEvent::Started {
        run_id: run_id.clone(),
    });
    let progress_started_at_ms = now_ms();
    emit_chat_progress_snapshot(
        &on_event,
        &run_id,
        &ChatProgressProjection::new(progress_started_at_ms),
        0,
    );
    let prompt = request.prompt.trim().to_owned();
    if prompt.len() > MAX_PROMPT_BYTES || prompt.contains('\0') {
        return Err(CommandError::invalid(format!(
            "prompt must contain at most {MAX_PROMPT_BYTES} bytes"
        )));
    }
    validate_turn_as_of(request.as_of.as_deref())?;
    let prepared_attachments = prepare_chat_attachments(&request.attachments)?;
    if prompt.is_empty() && prepared_attachments.is_empty() {
        return Err(CommandError::invalid(
            "a prompt or at least one attachment is required",
        ));
    }
    let pi_execution = state.pi_execution(
        &request.model_profile_id,
        request.thinking_level.as_str(),
        &request.run_options,
    )?;
    let pi_artifacts = pi_execution.artifacts.clone();
    let configured_model = pi_execution.model.clone();
    let has_images = prepared_attachments
        .iter()
        .any(|attachment| attachment.record.media_type.starts_with("image/"));
    if has_images && !configured_model.input.iter().any(|input| input == "image") {
        return Err(CommandError::invalid(
            "the selected model does not accept image attachments",
        ));
    }
    if prepared_attachments.iter().any(|attachment| {
        attachment.record.media_type.starts_with("image/")
            && !matches!(
                attachment.record.media_type.as_str(),
                "image/jpeg" | "image/png" | "image/gif" | "image/webp"
            )
    }) {
        return Err(CommandError::invalid(
            "image attachment type is unsupported",
        ));
    }
    let profile = state
        .store
        .get_loupist(&request.loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let setup::PreparedChatTurn {
        execution: pi_execution,
        context,
        skill_files,
        connector_authority,
        web_search_policy,
        bound_workspace,
        runtime_policy_sha256,
        runtime_surface_sha256,
    } = setup::PreparedChatTurn::resolve(
        state,
        &request,
        &profile,
        pi_execution,
        &prompt,
        interactive,
    )?;
    let policy = context.tool_policy(
        profile.id.clone(),
        request.thread_id.clone(),
        request.as_of.clone(),
    );
    let instruction_roots = (
        context.workspace.root.clone(),
        context.workspace.global_root.clone(),
    );
    let host_context = serde_json::to_string(&context).map_err(map_internal)?;
    let mentioned_connections = agent_harness::mentioned_connections(
        &request.mentioned_connection_ids,
        &context.agent_runtime,
    );
    let surface = context.surface;
    let harness = context.agent_harness;
    let chat_attachments = state.loupist_files_root(&profile.id)?.absolute_path(
        &PathBuf::from("loupists")
            .join(&profile.id)
            .join("attachments"),
    )?;
    let mut chat = state
        .store
        .get_chat(&request.thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    if chat.loupist_id != profile.id {
        return Err(CommandError::conflict(
            "chat thread does not belong to the selected Loupist",
        ));
    }
    let selected_artifact = crate::commands::artifact_context::selected_artifact_hint(
        state.store.as_ref(),
        &chat.id,
        request.selected_artifact.as_ref(),
    )?;
    let had_prior_messages = !chat.messages.is_empty();
    let expected_chat = chat.clone();
    crate::commands::knowledge::validate_selected_references(&request.pins)?;
    let cutoff = crate::commands::knowledge::as_of::Cutoff::parse(request.as_of.as_deref());
    let mut left_out = crate::commands::knowledge::as_of::LeftOut::default();
    let mut host_memory = crate::commands::knowledge::host_memory_context_for_turn(
        state,
        &profile.id,
        &prompt,
        request.use_memory,
        &chat.set_aside_memory,
        cutoff,
    )?;
    if let Some(memory) = &mut host_memory {
        left_out.merge(std::mem::take(&mut memory.left_out));
    }
    let followed = if request.use_memory {
        let context = state.store.map_context().map_err(map_store)?;
        // Holdings are today's positions, so an as-of turn never sees them.
        Some(if cutoff.is_some() {
            agent_harness::followed_interests(context)
        } else {
            agent_harness::followed_companies(context)
        })
    } else {
        None
    };
    let selected_references = if request.use_memory {
        crate::commands::knowledge::selected_reference_hints(state, &profile.id, &request.pins)?
    } else {
        None
    };
    let working_context = if request.use_memory {
        crate::commands::knowledge::current_working_context(state, &profile.id, &chat.id)?.filter(
            |note| {
                // Under an as-of date, a working note written later is left out.
                let known = cutoff.is_none_or(|cutoff| cutoff.recorded(note.recorded_at_ms));
                if !known {
                    left_out.notes.insert(note.research_id.clone());
                }
                known
            },
        )
    } else {
        None
    };
    let execution_session =
        ChatExecutionSession::prepare(state.artifacts.deletion_root.clone(), &chat)?;
    // Validate activity storage before accepting the user turn, so a local
    // storage failure cannot leave a canonical request without a response.
    let activity_recorder = crate::chat::activity::ActivityRecorder::new(
        state.loupist_files_root(&profile.id)?.as_ref(),
        &profile.id,
        &chat.id,
        &run_id,
        bound_workspace.path(),
    )?;
    let memory_revision = host_memory.as_ref().map(|memory| memory.revision.clone());
    let turn_resources = ChatTurnResources::new(run_lease, run_scratch, execution_session);
    // The native policy's folders: the Carried window is this whole Chat, since
    // neither the warm Pi session nor the cold transcript records which
    // replies it holds; a clean turn in a Chat that once read third-party
    // text asks before its first command.
    let native_roots = crate::agent_harness::native_policy::NativeRoots::new(
        &instruction_roots.0,
        &instruction_roots.1,
        &state.artifacts.app_data_dir,
        &state
            .store
            .loupist_data_dir(&profile.id)
            .map_err(map_store)?,
        &turn_resources
            .pi_session()
            .runtime_working_directory()
            .join("temp"),
        &chat_attachments.join(&chat.id),
        host_home().as_deref(),
    );
    let carried_untrusted = chat
        .messages
        .iter()
        .any(crate::domain::ChatMessage::has_untrusted_text);
    let timestamp = now_ms().max(chat.updated_at_ms + 1);
    let user_message_id = new_id("message");
    let current_prompt = attachment_prompt(
        &prompt,
        bound_workspace.path(),
        &chat_attachments,
        &chat.id,
        &user_message_id,
        &prepared_attachments,
    );
    let current_image_bytes = prepared_attachments
        .iter()
        .filter(|attachment| attachment.record.media_type.starts_with("image/"))
        .map(|attachment| attachment.bytes.len())
        .sum::<usize>();
    let current_pi_images = prepared_attachments
        .iter()
        .filter(|attachment| attachment.record.media_type.starts_with("image/"))
        .map(|attachment| PiImageContent {
            data: BASE64.encode(&attachment.bytes),
            mime_type: attachment.record.media_type.clone(),
        })
        .collect::<Vec<_>>();
    let requested_execution_model = pi_execution.model_lock();
    if let Some((_, record)) = on_event
        .run_log
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
    {
        record.model(&requested_execution_model);
    }
    let requested_cache_scope = PiSessionCacheScope {
        harness_digest: &harness.digest,
        runtime_policy_sha256: &runtime_policy_sha256,
        runtime_surface_sha256: &runtime_surface_sha256,
        connector_authority_sha256: &connector_authority.sha256,
        memory_access_enabled: request.use_memory,
        memory_update_enabled: request.update_memory,
        execution_model: &requested_execution_model,
    };
    let intended_cache = connector_authority
        .cacheable
        .then(|| chat.pi_session_cache.clone())
        .flatten()
        .filter(|cache| cache.matches(&requested_cache_scope));
    let pending_attachments = persist_chat_attachments(
        state,
        &profile.id,
        &chat.id,
        &user_message_id,
        &prepared_attachments,
    )?;
    chat.messages.push(ChatMessage {
        tool_refusals: Vec::new(),
        performance: None,
        feedback: None,
        tool_trace: None,
        schedule_receipts: Vec::new(),
        failure_kind: None,
        host_composed: false,
        steer: false,
        retry_after_seconds: None,
        delivery: None,
        replaced_model: None,
        as_of: request.as_of.clone(),
        number_checks: Vec::new(),
        sensitivity_cards: Vec::new(),
        as_of_report: None,
        alert: None,
        watch: None,
        earnings: None,
        vocabulary: None,
        handoff: None,
        link: None,
        id: user_message_id.clone(),
        role: ChatRole::User,
        status: ChatMessageStatus::Complete,
        content: prompt.clone(),
        created_at_ms: timestamp,
        lens_receipts: Vec::new(),
        memory_refs: Vec::new(),
        observed_exact_count: 0,
        refs_truncated: false,
        refs_digest: memory_refs_digest(&[]).map_err(map_internal)?,
        memory_update: None,
        memory_revision: None,
        execution_model: None,
        agent_harness: None,
        attachments: prepared_attachments
            .iter()
            .map(|attachment| attachment.record.clone())
            .collect(),
        artifact_refs: Vec::new(),
        evidence_refs: Vec::new(),
        progress: None,
    });
    chat.memory_policy = MemoryPolicy {
        use_memory: request.use_memory,
        update_memory: request.update_memory,
    };
    // The prior Pi cursor describes the transcript before this user turn. The
    // user-message CAS durably marks it dirty so a hard crash cannot resume a
    // generation that Pi may already have mutated. Assistant content and the
    // replacement cache state are sealed together by the later exact CAS.
    chat.pi_session_cache = None;
    chat.updated_at_ms = timestamp;
    if let Err(error) = state.store.replace_chat(&expected_chat, &chat) {
        if let Some(pending) = pending_attachments {
            pending.rollback()?;
        }
        return Err(map_store(error));
    }
    if let Some(pending) = pending_attachments {
        pending.commit();
    }
    on_event.admitted(&chat);
    // A stopped or failed first reply leaves the placeholder in place. The
    // next successful reply should still give that conversation a useful title.
    let should_generate_title = chat.title.is_empty();
    let assistant_message_id = new_id("message");
    if state
        .store
        .set_loupist_last_model_profile(&profile.id, &configured_model.id, timestamp)
        .is_err()
    {
        on_event.set_failure_kind(ChatFailureKind::StoreFailed);
        return Ok(ControlFlow::Break(
            persist_or_emit_failed_chat_with_stop_precedence(
                state,
                &on_event,
                &run_id,
                &chat.id,
                &assistant_message_id,
                memory_revision.clone(),
                pi_execution.model_lock(),
                harness.clone(),
                None,
            ),
        ));
    }

    if *cancel.borrow() {
        return Ok(ControlFlow::Break(
            persist_or_emit_failed_chat_with_stop_precedence(
                state,
                &on_event,
                &run_id,
                &chat.id,
                &assistant_message_id,
                memory_revision.clone(),
                pi_execution.model_lock(),
                harness.clone(),
                None,
            ),
        ));
    }
    Ok(ControlFlow::Continue(AdmittedChatTurn {
        #[cfg(feature = "eval-harness")]
        pi_agent_data_dir,
        request,
        on_event,
        surface,
        untrusted_context,
        chat_started_at,
        run_id,
        chat_controls,
        cancel,
        progress_started_at_ms,
        prompt,
        prepared_attachments,
        pi_artifacts,
        configured_model,
        profile,
        pi_execution,
        skill_files,
        connector_authority,
        web_search_policy,
        bound_workspace,
        runtime_policy_sha256,
        runtime_surface_sha256,
        policy,
        native_roots,
        carried_untrusted,
        host_context,
        harness,
        chat_attachments,
        chat,
        had_prior_messages,
        envelope: TurnEnvelopeInputs {
            mentioned_connections,
            selected_artifact,
            host_memory,
            followed,
            selected_references,
            working_context,
            left_out,
        },
        activity_recorder,
        memory_revision,
        turn_resources,
        user_message_id,
        current_prompt,
        current_image_bytes,
        current_pi_images,
        intended_cache,
        should_generate_title,
        assistant_message_id,
    }))
}

/// The host user's home folder, as the extension resolves `~` (Node's
/// `homedir()` reads the same variable).
fn host_home() -> Option<PathBuf> {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
