use super::*;
use crate::{providers::catalog::ExecutionModelLock, test_fixtures::chat_message};
use tauri::Manager;

const TURN_ENVELOPE: &str = r#"{"live_time":{"current_date_utc":"2026-08-10"}}"#;

pub(super) fn send_request(
    run_id: &str,
    loupist_id: &str,
    thread_id: &str,
    prompt: &str,
) -> ChatSendRequest {
    ChatSendRequest {
        run_id: run_id.into(),
        loupist_id: loupist_id.into(),
        thread_id: thread_id.into(),
        prompt: prompt.into(),
        use_memory: false,
        update_memory: false,
        as_of: None,
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
        run_options: Default::default(),
        mentioned_connection_ids: Vec::new(),
        attachments: Vec::new(),
        pins: Vec::new(),
        selected_artifact: None,
    }
}

fn attachment_input(filename: &str, media_type: &str, bytes: &[u8]) -> ChatAttachmentInput {
    ChatAttachmentInput {
        filename: filename.into(),
        media_type: media_type.into(),
        data_base64: BASE64.encode(bytes),
    }
}

fn stored_attachment(id: &str, filename: &str, media_type: &str, bytes: &[u8]) -> ChatAttachment {
    ChatAttachment {
        id: id.into(),
        filename: filename.into(),
        media_type: media_type.into(),
        size_bytes: bytes.len() as u64,
        content_sha256: sha256(bytes),
    }
}

fn pi_artifacts(
    executable: std::path::PathBuf,
    runtime_dir: std::path::PathBuf,
) -> crate::app::PiArtifacts {
    let agent_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("agent");
    crate::app::PiArtifacts {
        executable,
        runtime_dir,
        extension: agent_root.join("loupist-extension.mjs"),
        provider_extension: agent_root.join("loupist-provider-extension.mjs"),
        system_prompt: agent_root.join("SYSTEM.md"),
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: Default::default(),
        provider_credential: None,
    }
}

fn execution_model() -> ExecutionModelLock {
    ExecutionModelLock {
        profile_id: "openai-codex/gpt-5.6-luna".into(),
        name: "GPT-5.6 Luna".into(),
        provider: "openai-codex".into(),
        model: "gpt-5.6-luna".into(),
        thinking_level: "max".into(),
        run_options: Default::default(),
    }
}

fn mock_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    assert!(app.manage(state));
    app
}

#[test]
fn chat_pi_launch_config_keeps_a_private_stable_cwd_across_runs() {
    use crate::{
        chat::execution_session::ChatExecutionSession,
        commands::chat_runtime::{bind_chat_pi_session, chat_pi_launch_config},
        fs::secure_delete::SecureDeletionRoot,
    };

    let temporary = tempfile::tempdir().unwrap();
    let root =
        Arc::new(SecureDeletionRoot::open(&temporary.path().canonicalize().unwrap()).unwrap());
    let chat = chat("chat-stable-pi-cwd", "loupist-stable-pi-cwd", 1);
    let artifacts = pi_artifacts(
        temporary.path().join("pi"),
        temporary.path().join("pi-runtime"),
    );
    let app_data_dir = temporary.path().join("app-data");
    let session_id = "123e4567-e89b-42d3-a456-426614174111";
    let config = |session: &ChatExecutionSession, run: &str| {
        bind_chat_pi_session(
            chat_pi_launch_config(
                &artifacts,
                &app_data_dir,
                &chat.loupist_id,
                run,
                temporary.path().join(format!("{run}.sock")),
                format!("token-{run}"),
                session,
            ),
            session,
            session_id,
        )
        .unwrap()
    };
    let mut first = ChatExecutionSession::prepare(root.clone(), &chat).unwrap();
    let runtime_directory = first.runtime_working_directory().to_path_buf();
    std::fs::write(first.session_directory().join("cached.jsonl"), b"cached").unwrap();
    let first_config = config(&first, "run-a");
    first.wipe().unwrap();
    drop(first);
    let second = ChatExecutionSession::prepare(root, &chat).unwrap();
    let second_config = config(&second, "run-b");

    assert_eq!(first_config.working_dir, runtime_directory);
    assert_eq!(second_config.working_dir, runtime_directory);
    assert_eq!(first_config.runtime_working_dir(), runtime_directory);
    assert_ne!(
        first_config.runtime_working_dir(),
        first_config.agent_data_dir
    );
    assert_eq!(first_config.session, second_config.session);
    assert_eq!(first_config.session.as_ref().unwrap().id, session_id);
    assert_ne!(first_config.private_run_dir, second_config.private_run_dir);
    assert!(!second.session_directory().join("cached.jsonl").exists());
}

#[test]
fn chat_attachments_are_validated_extracted_and_bounded_before_persistence() {
    let bytes = b"Revenue quality improved.";
    let prepared =
        prepare_chat_attachments(&[attachment_input("notes.txt", "text/plain", bytes)]).unwrap();
    assert_eq!(prepared[0].bytes, bytes);
    assert_eq!(prepared[0].record.content_sha256, sha256(bytes));
    let workspace = std::path::Path::new("research");
    let prompt = super::super::attachments::attachment_prompt(
        "Cite the note.",
        workspace,
        &workspace.join(".loupist/attachments"),
        "chat",
        "turn",
        &prepared,
    );
    let (_, manifest) = prompt.split_once("<user_attachments>\n").unwrap();
    let (manifest, _) = manifest.split_once("\n</user_attachments>").unwrap();
    let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
    assert_eq!(
        manifest[0]["workspace_capture"],
        serde_json::json!({
            "path": format!(".loupist/attachments/chat/turn/{}", prepared[0].record.id),
            "expected_sha256": sha256(bytes),
        })
    );
    match &prepared[0].extract_status {
        AttachmentExtractStatus::Extracted(extraction) => {
            assert_eq!(extraction.parts, vec![bytes.to_vec()])
        }
        other => panic!("expected extracted text, got {other:?}"),
    }
    let image =
        prepare_chat_attachments(&[attachment_input("chart.png", "image/png", &[1; 8])]).unwrap();
    assert!(matches!(
        image[0].extract_status,
        AttachmentExtractStatus::NotApplicable
    ));

    let too_many =
        vec![attachment_input("notes.txt", "text/plain", bytes); MAX_CHAT_ATTACHMENTS + 1];
    for refused in [
        vec![attachment_input("../escape.txt", "text/plain", bytes)],
        vec![attachment_input("invalid.txt", "text/plain", &[0xff, 0xfe])],
        vec![ChatAttachmentInput {
            data_base64: "not-base64".into(),
            ..attachment_input("notes.txt", "text/plain", bytes)
        }],
        vec![attachment_input(
            "huge.png",
            "image/png",
            &vec![2; MAX_CHAT_IMAGE_ATTACHMENT_BYTES + 1],
        )],
        too_many,
    ] {
        assert!(prepare_chat_attachments(&refused).is_err());
    }
}

/// Files up to 25 MB each are kept; a larger one is refused with a code the
/// composer words in the reader's language (B83).
#[test]
fn each_attachment_may_be_up_to_25_mb_and_a_larger_one_says_so() {
    const MB: usize = 1024 * 1024;
    let file =
        |size: usize| attachment_input("deck.bin", "application/octet-stream", &vec![7; size]);
    let kept = prepare_chat_attachments(&[file(25 * MB)]).unwrap();
    assert_eq!(kept[0].record.size_bytes, 25 * MB as u64);
    // More than the old 5 MB record limit, and several at once.
    let several = prepare_chat_attachments(&[file(6 * MB), file(6 * MB), file(6 * MB)]).unwrap();
    assert_eq!(several.len(), 3);
    let refused = prepare_chat_attachments(&[file(25 * MB + 1)])
        .err()
        .unwrap();
    assert_eq!(refused.code, "attachment_too_large");
    assert!(refused.message.contains("deck.bin"));
    // A payload too long to decode under the limit is refused before decoding.
    let oversized = ChatAttachmentInput {
        data_base64: "A".repeat(34 * MB + 8),
        ..attachment_input("deck.bin", "application/octet-stream", b"x")
    };
    assert_eq!(
        prepare_chat_attachments(&[oversized]).err().unwrap().code,
        "attachment_too_large"
    );
}

#[test]
fn chat_attachment_reads_are_bound_to_the_exact_file_digest() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("attachment-1");
    let bytes = b"exact attachment";
    fs::write(&path, bytes).unwrap();
    let attachment = stored_attachment("attachment-1", "notes.txt", "text/plain", bytes);

    assert_eq!(
        read_chat_attachment_file(&path, &attachment).unwrap(),
        bytes
    );
    #[cfg(unix)]
    {
        let link = temporary.path().join("attachment-link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_chat_attachment_file(&link, &attachment).is_err());
    }
    fs::write(&path, b"other attachment").unwrap();
    assert!(read_chat_attachment_file(&path, &attachment).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn chat_immediate_abort_is_admitted_before_preflight_and_never_starts_pi() {
    use futures_util::FutureExt;

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "chat-abort-preflight");
    let mut state = AppState::for_test(temporary.path().join("app"));
    Arc::make_mut(&mut state.artifacts).pi = Some(pi_artifacts(
        temporary.path().join("must-not-start-pi"),
        temporary.path().join("unused-pi-runtime"),
    ));
    let profile = profile("loupist-chat-abort", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-abort", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();

    let app = mock_app(state);
    let run_id = "chat-immediate-abort";
    let captured_app = app.handle().clone();
    let channel = Channel::new(move |body| {
        let event: serde_json::Value = body.deserialize().unwrap();
        if event["type"] == "started" {
            // Stop at the first event the renderer can observe.
            captured_app
                .state::<AppState>()
                .cancel_run(run_id, RunKind::Chat)
                .now_or_never()
                .expect("Stop admission does not wait for worker preflight")
                .expect("immediate Chat abort addresses the reserved run");
        }
        Ok(())
    });
    let started = chat_send(
        send_request(
            run_id,
            &profile.id,
            &chat.id,
            "This prompt must never reach Pi.",
        ),
        channel,
        app.state(),
    )
    .await;
    assert_eq!(started.unwrap().run_id, run_id);

    let state = app.state::<AppState>();
    assert_eq!(state.run_coordinator.active_count(), 0);
    for directory in [
        &state.artifacts.broker_dir,
        &state.artifacts.process_lease_dir,
    ] {
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(stored.messages.len(), 2);
    assert_eq!(stored.messages[0].role, ChatRole::User);
    assert_eq!(stored.messages[1].status, ChatMessageStatus::Aborted);
}

#[tokio::test]
async fn only_pi_accepted_steers_become_canonical_chat_history() {
    use crate::{
        chat::control::{chat_control_channel, ChatControlError},
        commands::chat_runtime::settle_chat_control_response,
        store::SqliteStore,
    };

    let temporary = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_in_memory().unwrap();
    let profile = profile("loupist-steer", &temporary.path().join("workspace"), 1);
    seed_profile(&store, &profile);
    let chat = chat("chat-steer", &profile.id, 1);
    store.create_chat(&chat).unwrap();

    let (control, mut controls) = chat_control_channel();
    let rejected = tokio::spawn(async move { control.submit("REJECTED-STEER".into()).await });
    let request = controls.recv().await.unwrap();
    settle_chat_control_response(&store, &profile.id, &chat.id, request, false).unwrap();
    assert!(matches!(
        rejected.await.unwrap(),
        Err(ChatControlError::Rejected(_))
    ));
    assert!(store
        .get_chat(&chat.id)
        .unwrap()
        .unwrap()
        .messages
        .is_empty());

    let (control, mut controls) = chat_control_channel();
    let accepted = tokio::spawn(async move { control.submit("ACCEPTED-STEER".into()).await });
    let request = controls.recv().await.unwrap();
    settle_chat_control_response(&store, &profile.id, &chat.id, request, true).unwrap();
    let receipt = accepted.await.unwrap().unwrap();
    let stored = store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(stored.messages.len(), 1);
    assert_eq!(stored.messages[0].id, receipt.message_id);
    assert_eq!(stored.messages[0].content, "ACCEPTED-STEER");
    // A persisted steer is marked in the record, so the transcript shows it as
    // a "Guided" row after the run; it stays a user turn for the model (B80).
    assert_eq!(stored.messages[0].role, ChatRole::User);
    assert!(stored.messages[0].steer);
    let dto = crate::commands::types::chat_dto(&stored).unwrap();
    assert_eq!(
        serde_json::to_value(&dto.messages[0]).unwrap()["steer"],
        serde_json::json!(true)
    );
    // Only a user message can be a steer.
    let mut answer = stored.messages[0].clone();
    answer.role = ChatRole::Assistant;
    assert!(answer.validate().is_err());
}

#[tokio::test]
async fn terminal_control_barrier_persists_out_of_order_acknowledgements_in_submission_order() {
    use crate::{
        chat::control::chat_control_channel,
        commands::chat_runtime::settle_chat_controls_before_completion, pi::PiEvent,
        store::SqliteStore,
    };

    let temporary = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_in_memory().unwrap();
    let profile = profile("loupist-barrier", &temporary.path().join("workspace"), 1);
    seed_profile(&store, &profile);
    let chat = chat("chat-terminal-barrier", &profile.id, 1);
    store.create_chat(&chat).unwrap();

    let (control, mut controls) = chat_control_channel();
    let first = tokio::spawn({
        let control = control.clone();
        async move { control.submit("FIRST-STEER".into()).await }
    });
    let first_request = controls.recv().await.unwrap();
    let second = tokio::spawn(async move { control.submit("SECOND-STEER".into()).await });
    let second_request = controls.recv().await.unwrap();
    let mut pending = std::collections::HashMap::from([
        (10_u64, (0_u64, first_request)),
        (20_u64, (1_u64, second_request)),
    ]);
    let mut settled = std::collections::BTreeMap::new();
    let mut next = 0_u64;
    let (events, mut event_receiver) = tokio::sync::broadcast::channel(4);
    for id in [20, 10] {
        events
            .send(PiEvent::Rpc {
                payload: serde_json::json!({ "type": "response", "id": id, "success": true }),
            })
            .unwrap();
    }

    settle_chat_controls_before_completion(
        &mut event_receiver,
        &mut pending,
        &mut settled,
        &mut next,
        &store,
        &profile.id,
        &chat.id,
    )
    .await
    .unwrap();

    assert!(pending.is_empty() && settled.is_empty());
    assert!(first.await.unwrap().is_ok());
    assert!(second.await.unwrap().is_ok());
    let stored = store.get_chat(&chat.id).unwrap().unwrap();
    let contents = stored
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    assert_eq!(contents, ["FIRST-STEER", "SECOND-STEER"]);
}

#[test]
fn completed_chat_performance_survives_reload_and_matches_live_completion() {
    use crate::{
        commands::chat_runtime::{
            emit_completed_chat, recovered_canonical_completion, CanonicalCompletionExpectation,
        },
        domain::{ChatPerformance, ChatSessionCacheStatus},
        store::SqliteStore,
    };
    use std::sync::Mutex;

    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("loupist.sqlite3");
    let store = SqliteStore::open(&database).unwrap();
    let profile = profile("loupist-recovered", &temporary.path().join("workspace"), 1);
    seed_profile(&store, &profile);
    let mut chat = chat("chat-recovered", &profile.id, 1);
    chat.title = "Recovered title".into();
    store.create_chat(&chat).unwrap();
    let expected = chat.clone();
    let execution_model = execution_model();
    let harness = agent_harness::snapshot("chat", &[], &[]).unwrap();
    let performance = ChatPerformance {
        setup_ms: 120,
        first_text_ms: Some(480),
        generation_ms: 900,
        total_ms: 1_040,
        session_cache: ChatSessionCacheStatus::Warm,
        input_tokens: 320,
        output_tokens: 96,
        cache_read_tokens: 256,
        cache_write_tokens: 64,
    };
    chat.messages.push(ChatMessage {
        created_at_ms: 2,
        memory_revision: Some("memory-tree-revision".into()),
        execution_model: Some(execution_model.clone()),
        agent_harness: Some(harness.clone()),
        ..chat_message(
            "assistant-recovered",
            ChatRole::Assistant,
            "The canonical answer",
        )
    });
    chat.updated_at_ms = 2;
    let mut completed_message = None;
    store
        .replace_chat_with(&expected, &chat, |tx| {
            completed_message = Some(crate::commands::knowledge::write_chat_learning_report(
                tx,
                &chat,
                "assistant-recovered",
                false,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(performance.clone()),
                Vec::new(),
            )?);
            Ok(())
        })
        .unwrap();

    let events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let captured_events = events.clone();
    let channel = Channel::new(move |body| {
        captured_events
            .lock()
            .unwrap()
            .push(body.deserialize().unwrap());
        Ok(())
    });
    emit_completed_chat(
        &channel.into(),
        "run-performance",
        &completed_message.unwrap(),
        None,
        &execution_model,
        &harness,
        performance.clone(),
    );
    let live_performance = events
        .lock()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "completed")
        .unwrap()["performance"]
        .clone();
    drop(store);
    let store = SqliteStore::open(&database).unwrap();

    let recover = |performance: &ChatPerformance| {
        recovered_canonical_completion(
            &store,
            &chat.id,
            "assistant-recovered",
            CanonicalCompletionExpectation {
                content: "The canonical answer",
                memory_revision: &Some("memory-tree-revision".into()),
                execution_model: &execution_model,
                agent_harness: &harness,
                performance,
                title: Some("Recovered title"),
            },
        )
    };
    let (message, title) = recover(&performance).unwrap();
    assert_eq!(message.status, ChatMessageStatus::Complete);
    assert_eq!(title.as_deref(), Some("Recovered title"));
    assert_eq!(message.performance.as_ref(), Some(&performance));
    let reloaded =
        super::super::types::chat_dto(&store.get_chat(&chat.id).unwrap().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&reloaded.messages[0].performance).unwrap(),
        live_performance
    );
    let mut mismatched = performance.clone();
    mismatched.cache_read_tokens += 1;
    assert!(recover(&mismatched).is_none());
}

#[tokio::test]
async fn failed_chat_yields_to_an_accepted_stop_even_when_persistence_fails() {
    use crate::commands::chat_runtime::{
        persist_or_emit_failed_chat_with_stop_precedence, ChatEventSink,
    };
    use std::sync::Mutex;

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "chat-stop-precedence");
    let state = AppState::for_test(temporary.path().join("app"));
    let profile = profile("loupist-stop-precedence", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-stop-precedence", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();

    // The second thread does not exist, so its terminal reply cannot be persisted.
    for (run_id, thread_id) in [
        ("run-stored", chat.id.as_str()),
        ("run-missing", "missing-chat"),
    ] {
        let registration = state
            .register_run(
                run_id.into(),
                profile.id.clone(),
                RunKind::Chat,
                RunTarget::ChatThread(thread_id.into()),
            )
            .unwrap();
        state.cancel_run(run_id, RunKind::Chat).await.unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let sink = ChatEventSink::new(move |event| {
            captured.lock().unwrap().push(event);
            Ok(())
        });
        let started = persist_or_emit_failed_chat_with_stop_precedence(
            &state,
            &sink,
            run_id,
            thread_id,
            &format!("assistant-{run_id}"),
            None,
            execution_model(),
            agent_harness::snapshot("chat", &[], &[]).unwrap(),
            None,
        );
        assert_eq!(started.run_id, run_id);
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [ChatStreamEvent::Aborted { run_id: aborted }] if aborted == run_id
        ));
        drop(registration);
    }
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(stored.messages.len(), 1);
    assert_eq!(stored.messages[0].status, ChatMessageStatus::Aborted);
}

#[cfg(unix)]
#[tokio::test]
async fn chat_activity_storage_failure_does_not_persist_a_user_turn() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "chat-activity-storage-failure");
    let mut state = AppState::for_test(temporary.path().join("app"));
    Arc::make_mut(&mut state.artifacts).pi = Some(pi_artifacts(
        temporary.path().join("must-not-start-pi"),
        temporary.path().join("unused-pi-runtime"),
    ));
    let profile = profile("loupist-chat-activity-failure", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-activity-failure", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();
    let runtime = state
        .artifacts
        .deletion_root
        .absolute_path(&Path::new("loupists").join(&profile.id))
        .unwrap();
    fs::create_dir_all(&runtime).unwrap();
    // A regular file in place of the activities directory must fail preflight.
    fs::write(runtime.join("activities"), b"occupied").unwrap();

    let app = mock_app(state);
    let result = chat_send(
        send_request(
            "chat-activity-storage-failure",
            &profile.id,
            &chat.id,
            "This request must fail before it enters the transcript.",
        ),
        Channel::new(|_| Ok(())),
        app.state(),
    )
    .await;
    assert!(result.is_err());
    let state = app.state::<AppState>();
    assert_eq!(state.store.get_chat(&chat.id).unwrap(), Some(chat));
    assert_eq!(state.run_coordinator.active_count(), 0);
    assert!(fs::read_dir(&state.artifacts.broker_dir)
        .unwrap()
        .next()
        .is_none());
    assert_eq!(fs::read(runtime.join("activities")).unwrap(), b"occupied");
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
pub(super) fn install_chat_rpc_fixture(state: &mut AppState, directory: &Path, source: &str) {
    use crate::pi::{PI_DARWIN_ARM64_ARCHIVE_SHA256, PI_VERSION};

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = directory.join("fixture-pi");
    let script = directory.join("fixture-pi.py");
    fs::write(&script, source).unwrap();
    // A native parent preserves the exact executable identity required by the
    // process lease; its Python child inherits the same owned process group.
    let compiler = std::process::Command::new("/usr/bin/cc")
        .arg("-std=c11")
        .arg("-Wall")
        .arg("-Werror")
        .arg(format!(
            "-DFIXTURE_PYTHON={}",
            serde_json::to_string("/usr/bin/python3").unwrap()
        ))
        .arg(format!(
            "-DFIXTURE_SCRIPT={}",
            serde_json::to_string(&script).unwrap()
        ))
        .arg(manifest.join("src/commands/tests/fixtures/native_activity_launcher.c"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "could not compile the RPC fixture: {}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    // The real Pi transport and Chat event loop still launch this child.
    let runtime_dir = directory.join("fixture-runtime");
    fs::create_dir(&runtime_dir).unwrap();
    for (name, value) in [
        (".pi-version", PI_VERSION.to_owned()),
        (
            ".pi-archive.sha256",
            PI_DARWIN_ARM64_ARCHIVE_SHA256.to_owned(),
        ),
        (
            ".pi-executable.sha256",
            sha256(&fs::read(&executable).unwrap()),
        ),
        (
            "package.json",
            serde_json::json!({"version": PI_VERSION}).to_string(),
        ),
    ] {
        fs::write(runtime_dir.join(name), value).unwrap();
    }
    Arc::make_mut(&mut state.artifacts).pi = Some(pi_artifacts(executable, runtime_dir));
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
pub(super) async fn drain(state: &AppState) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while state.run_coordinator.active_count() != 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture turn did not drain");
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn native_chat_skill_mentions_preserve_requested_memory_permissions() {
    use crate::commands::chat_runtime::{send_chat, ChatEventSink};

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "skill-authority");
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("fixtures/native_activity_pi.py"),
    );
    let profile = profile("loupist-skill-authority", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-skill-authority", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();

    for (index, (prompt, read, write)) in [
        (
            "$world $skill:lens inspect the supplied facts",
            false,
            false,
        ),
        ("$lens compare the method", true, false),
        ("$world inspect these facts", true, true),
    ]
    .into_iter()
    .enumerate()
    {
        let request = ChatSendRequest {
            use_memory: read,
            update_memory: write,
            ..send_request(
                &format!("skill-authority-{index}"),
                &profile.id,
                &chat.id,
                prompt,
            )
        };
        send_chat(request, ChatEventSink::new(|_| Ok(())), &state, None)
            .await
            .unwrap();
        drain(&state).await;
        let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
        assert_eq!(stored.memory_policy.use_memory, read);
        assert_eq!(stored.memory_policy.update_memory, write);
        assert_eq!(
            stored.messages.last().unwrap().status,
            ChatMessageStatus::Complete
        );
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn a_gone_model_is_retried_once_on_auto_within_the_same_turn() {
    use crate::commands::chat_runtime::{send_chat, ChatEventSink};
    use crate::providers::catalog::ConfiguredModel;

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "model-gone");
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("fixtures/model_gone_pi.py"),
    );
    let auth = state.pi_agent_data_dir().unwrap().join("auth.json");
    crate::fs::artifact_trust::ensure_private_regular_file(&auth).unwrap();
    fs::write(
        &auth,
        r#"{"openai":{"type":"api_key","key":"fixture-not-a-real-key"}}"#,
    )
    .unwrap();
    let model = |id: &str, name: &str| ConfiguredModel {
        id: format!("openai/{id}"),
        name: name.into(),
        provider: "openai".into(),
        model: id.into(),
        input: vec!["text".into()],
        reasoning: true,
        context_window: 272_000,
        max_tokens: 32_000,
        thinking_levels: vec!["off".into(), "medium".into(), "high".into()],
        thinking_level_map: Default::default(),
        run_controls: vec![],
    };
    state
        .replace_provider_models(
            "openai",
            vec![
                model("gone-model", "Gone Model"),
                model("gpt-6.1-sol", "GPT-6.1 Sol"),
            ],
        )
        .unwrap();
    let profile = profile("loupist-model-gone", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-model-gone", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = events.clone();
    let sink = ChatEventSink::new(move |event| {
        seen.lock()
            .unwrap()
            .push(serde_json::to_value(&event).unwrap());
        Ok(())
    });
    let request = ChatSendRequest {
        model_profile_id: "openai/gone-model".into(),
        thinking_level: "high".into(),
        ..send_request("model-gone", &profile.id, &chat.id, "Compare the margins.")
    };
    send_chat(request, sink, &state, None).await.unwrap();
    drain(&state).await;

    // Exactly one retry, on the Auto model, inside the one turn.
    assert_eq!(
        fs::read_to_string(temporary.path().join("launches.txt")).unwrap(),
        "gone-model\ngpt-6.1-sol\n"
    );
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(stored.messages.len(), 2, "one request, one answer");
    let answer = stored.messages.last().unwrap();
    assert_eq!(answer.status, ChatMessageStatus::Complete);
    assert_eq!(answer.content, "Answered on Auto.");
    assert_eq!(
        answer.execution_model.as_ref().unwrap().profile_id,
        "openai/gpt-6.1-sol"
    );
    assert_eq!(answer.replaced_model.as_deref(), Some("Gone Model"));
    let events = events.lock().unwrap();
    assert!(events.iter().all(|event| event["type"] != "error"));
    let completed = events
        .iter()
        .find(|event| event["type"] == "completed")
        .unwrap();
    assert_eq!(completed["replaced_model"], "Gone Model");
    // Rust remembers the gone model; the next model refresh leaves it out.
    assert!(state
        .model_catalog()
        .unwrap()
        .resolve("openai/gone-model")
        .is_err());
    assert_eq!(
        state.store.gone_models("openai").unwrap(),
        ["openai/gone-model"]
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn chat_followup_after_first_stop_gets_a_title_and_preserves_a_rename() {
    use crate::commands::chat_runtime::{send_chat, ChatEventSink};
    use futures_util::FutureExt;

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "stopped-title");
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("fixtures/native_activity_pi.py"),
    );
    let profile = profile("loupist-stopped-title", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let mut chat = chat("chat-stopped-title", &profile.id, 1);
    chat.title = "".into();
    state.store.create_chat(&chat).unwrap();

    for (run_id, stop, expected_title) in [
        ("first-stop", true, ""),
        ("followup", false, "Read notes.txt."),
        ("after-rename", false, "My research"),
    ] {
        if run_id == "after-rename" {
            let before = state.store.get_chat(&chat.id).unwrap().unwrap();
            let mut renamed = before.clone();
            renamed.title = "My research".into();
            state.store.replace_chat(&before, &renamed).unwrap();
        }
        let stop_state = state.clone();
        let sink = ChatEventSink::new(move |event| {
            if stop && matches!(event, ChatStreamEvent::Started { .. }) {
                stop_state
                    .cancel_run(run_id, RunKind::Chat)
                    .now_or_never()
                    .unwrap()
                    .unwrap();
            }
            Ok(())
        });
        send_chat(
            send_request(run_id, &profile.id, &chat.id, "Read notes.txt."),
            sink,
            &state,
            None,
        )
        .await
        .unwrap();
        drain(&state).await;
        let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
        assert_eq!(stored.title, expected_title);
        let expected_status = if stop {
            ChatMessageStatus::Aborted
        } else {
            ChatMessageStatus::Complete
        };
        assert_eq!(stored.messages.last().unwrap().status, expected_status);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn native_tool_activity_and_redacted_trace_survive_completed_chat_reload() {
    use crate::{
        chat::progress::{ChatProgressItem, ChatProgressStatus},
        commands::workspace::{chat_activity_read, ChatActivityRequest},
        domain::ToolTraceOutcome,
        store::SqliteStore,
    };

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "native-activity");
    fs::write(temporary.path().join("trace-broker-calls"), "").unwrap();
    fs::write(workspace.join("notes.txt"), "Retained local evidence.\n").unwrap();
    let app_data = temporary.path().join("app");
    let mut state = AppState::for_persistent_test(app_data.clone());
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("fixtures/native_activity_pi.py"),
    );
    let profile = profile("loupist-native-activity", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-native-activity", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();
    let state_for_reload = state.clone();
    let app = mock_app(state);
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let channel = Channel::new(move |body| {
        let event: serde_json::Value = body.deserialize().unwrap();
        events_tx.send(event).unwrap();
        Ok(())
    });
    chat_send(
        send_request("chat-activity", &profile.id, &chat.id, "Read notes.txt."),
        channel,
        app.state(),
    )
    .await
    .unwrap();
    let events = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let mut events = Vec::new();
        while let Some(event) = events_rx.recv().await {
            events.push(event);
        }
        events
    })
    .await
    .expect("scripted Chat turn did not release its event channel");
    assert!(
        events.iter().any(|event| event["type"] == "completed"),
        "Chat did not complete: {events:?}"
    );
    let live_detail_id = events
        .iter()
        .filter_map(|event| event.pointer("/patch/upsertItems")?.as_array())
        .flatten()
        .find_map(|item| item.get("detailId")?.as_str())
        .expect("native tool progress omitted its activity reference")
        .to_owned();
    drop(app);

    // Reload canonical history through a fresh database connection. The mock
    // Tauri manager retains its app lock, so keep the same idle runtime scope.
    let mut reloaded_state = state_for_reload;
    reloaded_state.store = Arc::new(SqliteStore::open(app_data.join("loupist.sqlite3")).unwrap());
    let reloaded = reloaded_state.store.get_chat(&chat.id).unwrap().unwrap();
    let assistant = reloaded.messages.last().unwrap();
    assert_eq!(assistant.status, ChatMessageStatus::Complete);
    let trace = assistant.tool_trace.as_ref().unwrap();
    let entry = |call_id: &str| {
        trace
            .entries
            .iter()
            .find(|entry| entry.call_id.as_deref() == Some(call_id))
            .unwrap()
    };
    assert_eq!(entry("history-ok").outcome, ToolTraceOutcome::Ok);
    assert_eq!(entry("history-bad").outcome, ToolTraceOutcome::Error);
    assert_eq!(
        entry("history-bad").args.as_ref().unwrap()["password"],
        "[REDACTED]"
    );
    assert!(!serde_json::to_string(trace)
        .unwrap()
        .contains("fictional-sensitive-value"));
    let tool_items = assistant
        .progress
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter_map(|item| match item {
            ChatProgressItem::Tool {
                detail_id,
                file_path,
                status,
                ..
            } => Some((detail_id.clone(), file_path.clone(), *status)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_items,
        [(
            Some(live_detail_id.clone()),
            Some("notes.txt".to_owned()),
            ChatProgressStatus::Succeeded
        )]
    );
    let reloaded_app = mock_app(reloaded_state);
    let detail = chat_activity_read(
        ChatActivityRequest {
            loupist_id: profile.id,
            thread_id: chat.id,
            detail_id: live_detail_id.clone(),
        },
        reloaded_app.state(),
    )
    .await
    .unwrap();
    assert_eq!(detail.id, live_detail_id);
    assert_eq!(detail.status, ChatProgressStatus::Succeeded);
    assert_eq!(detail.output, "Retained local evidence.\n");
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn chat_startup_stop_drains_without_relaunch_or_prompt() {
    use crate::commands::chat_runtime::{send_chat, ChatEventSink};
    use std::time::{Duration, Instant};

    for warm in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        initialized_workspace(&workspace, "startup-stop");
        let mut state = AppState::for_test(temporary.path().join("app"));
        install_chat_rpc_fixture(
            &mut state,
            temporary.path(),
            include_str!("fixtures/native_activity_pi.py"),
        );
        let profile = profile("loupist-startup-stop", &workspace, 1);
        seed_profile(state.store.as_ref(), &profile);
        let chat = chat("chat-startup-stop", &profile.id, 1);
        state.store.create_chat(&chat).unwrap();
        let trace_file = temporary.path().join("startup-trace.jsonl");
        fs::write(&trace_file, "").unwrap();
        let request = |run_id: &str| send_request(run_id, &profile.id, &chat.id, "Read notes.txt.");

        if warm {
            send_chat(
                request("warm"),
                ChatEventSink::new(|_| Ok(())),
                &state,
                None,
            )
            .await
            .unwrap();
            drain(&state).await;
            let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
            assert!(
                stored.pi_session_cache.is_some(),
                "initial turn did not seal a warm cache"
            );
        }

        fs::write(temporary.path().join("hold-pi-startup"), "").unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let sink = ChatEventSink::new(move |event| {
            captured_events.lock().unwrap().push(event);
            Ok(())
        });
        let expected_launches = if warm { 2 } else { 1 };
        let read_trace = || {
            fs::read_to_string(&trace_file)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .collect::<Vec<_>>()
        };
        let count = |trace: &[serde_json::Value], name: &str| {
            trace.iter().filter(|event| event["event"] == name).count()
        };
        let run_id = "stop";
        let stop = async {
            tokio::time::timeout(Duration::from_secs(5), async {
                while count(&read_trace(), "get_available_thinking_levels") != expected_launches {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("Pi did not enter the held startup control");
            let stopped_at = Instant::now();
            state.cancel_run(run_id, RunKind::Chat).await.unwrap();
            stopped_at
        };
        let (started, stopped_at) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(send_chat(request(run_id), sink, &state, None), stop)
        })
        .await
        .expect("Stop waited for Pi's startup deadline");
        assert_eq!(started.unwrap().run_id, run_id);
        assert!(stopped_at.elapsed() < Duration::from_secs(3));

        let trace = read_trace();
        assert_eq!(
            count(&trace, "launch"),
            expected_launches,
            "Stop must never trigger a cold fallback"
        );
        assert_eq!(
            count(&trace, "prompt"),
            usize::from(warm),
            "the cancelled startup must never send a model prompt"
        );
        assert_eq!(count(&trace, "abort"), 1);
        for event in trace.iter().filter(|event| event["event"] == "launch") {
            let pid = event["pid"].as_i64().unwrap() as i32;
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "fixture child {pid} survived Stop"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, ChatStreamEvent::Aborted { .. })));
        assert_eq!(state.run_coordinator.active_count(), 0);
        for directory in [
            &state.artifacts.broker_dir,
            &state.artifacts.process_lease_dir,
        ] {
            assert!(
                fs::read_dir(directory).unwrap().next().is_none(),
                "lease remained in {directory:?}"
            );
        }
        let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
        assert_eq!(
            stored.messages.last().unwrap().status,
            ChatMessageStatus::Aborted
        );
        assert!(stored.pi_session_cache.is_none());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn chat_launch_failure_persists_a_sanitized_terminal_error() {
    use crate::commands::chat_runtime::FAILED_CHAT_CONTENT;
    use std::sync::Mutex;

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "chat-launch-failure");
    let mut state = AppState::for_test(temporary.path().join("app"));
    Arc::make_mut(&mut state.artifacts).pi = Some(pi_artifacts(
        temporary.path().join("missing-pi"),
        temporary.path().join("unused-pi-runtime"),
    ));
    let profile = profile("loupist-chat-launch-failure", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    // Exercise the real default Memory path inside a Tokio runtime.
    crate::commands::knowledge::ensure_perspective(
        &state,
        &profile.id,
        "Distinguish reported facts from assumptions.",
    )
    .unwrap();
    let chat = chat("chat-launch-failure", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();

    let app = mock_app(state);
    let events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let captured_events = events.clone();
    let channel = Channel::new(move |body| {
        captured_events
            .lock()
            .unwrap()
            .push(body.deserialize().unwrap());
        Ok(())
    });
    let request = ChatSendRequest {
        use_memory: true,
        as_of: Some("2026-03-03".into()),
        ..send_request(
            "chat-launch-failure",
            &profile.id,
            &chat.id,
            "Persist this request before Pi launch fails.",
        )
    };
    chat_send(request, channel, app.state()).await.unwrap();

    let state = app.state::<AppState>();
    assert_eq!(state.run_coordinator.active_count(), 0);
    let events = events.lock().unwrap();
    let terminal = events.last().unwrap();
    assert_eq!(terminal["type"], "error");
    assert_eq!(terminal["message"], FAILED_CHAT_CONTENT);
    // The renderer hides the placeholder by this flag, never by its text.
    assert_eq!(terminal["placeholder"], true);
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(stored.messages.len(), 2);
    let error = &stored.messages[1];
    assert_eq!(error.status, ChatMessageStatus::Error);
    assert_eq!(error.content, FAILED_CHAT_CONTENT);
    // Stored replies keep only the text; the thread view marks it again.
    let thread = crate::commands::types::chat_dto(&stored).unwrap();
    assert!(thread.messages[1].placeholder && !thread.messages[0].placeholder);
    // The missing executable fails runtime-integrity validation before spawn.
    assert_eq!(
        error.failure_kind,
        Some(crate::domain::ChatFailureKind::ProtocolError)
    );
    assert!(error.memory_update.is_none());
    // A failed turn stores no number checks or as-of report; its request keeps the date for Retry.
    assert!(error.number_checks.is_empty() && error.as_of_report.is_none());
    assert_eq!(stored.messages[0].as_of.as_deref(), Some("2026-03-03"));
    assert!(!temporary.path().join("missing-pi").exists());
}

#[test]
fn cold_bootstrap_restores_history_saved_work_and_attachments() {
    use crate::{
        chat::artifacts::{ChatArtifactKind, ChatArtifactRef},
        chat::progress::{
            ChatProgress, ChatProgressCategory, ChatProgressItem, ChatProgressOperation,
            ChatProgressStatus,
        },
        domain::{ChatEvidenceKind, ChatEvidenceRef},
    };

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let note = b"Revenue quality improved after low-margin churn.";
    let directory = root.join("chat-a/question");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("attachment-a"), note).unwrap();
    fs::write(directory.join("attachment-invalid"), [0xff, 0xfe]).unwrap();
    let digest = "ab".repeat(32);
    let evidence = |index: u32, excerpt: &str| ChatEvidenceRef {
        id: format!("ev_{index:032x}"),
        title: "Samsung 2025 filing".into(),
        kind: ChatEvidenceKind::Original,
        url: None,
        provider: None,
        tool_name: "web_fetch".into(),
        calc_operation: None,
        read_ref: None,
        retrieved_at: "2026-10-11T00:00:00Z".into(),
        published_at: None,
        as_of: None,
        coverage: None,
        excerpt: excerpt.into(),
        excerpt_match: None,
        excerpt_truncated: false,
        content_sha256: digest.clone(),
        warnings: vec![],
        dataset_ref: None,
        model_delivery: None,
        upstream_evidence_ids: vec![],
    };
    let history = vec![
        ChatMessage {
            attachments: vec![
                stored_attachment("attachment-a", "cohort-notes.txt", "text/plain", note),
                stored_attachment(
                    "attachment-invalid",
                    "legacy.txt",
                    "text/plain",
                    &[0xff, 0xfe],
                ),
            ],
            ..chat_message("question", ChatRole::User, "First question")
        },
        ChatMessage {
            created_at_ms: 2,
            artifact_refs: vec![ChatArtifactRef {
                artifact_id: "artifact-chart".into(),
                revision: 1,
                kind: ChatArtifactKind::Chart,
                title: "Quarterly chart".into(),
                digest: digest.clone(),
            }],
            evidence_refs: vec![evidence(1, "Revenue rose."), evidence(2, "Same filing.")],
            progress: Some(ChatProgress {
                started_at_ms: 1,
                finished_at_ms: Some(2),
                items: vec![ChatProgressItem::Tool {
                    id: "tool-1".into(),
                    tool_name: Some("write".into()),
                    category: ChatProgressCategory::Files,
                    operation: ChatProgressOperation::Write,
                    action: "Saved a file".into(),
                    running: None,
                    target: Some("notes/filing.md".into()),
                    href: None,
                    source: None,
                    summary: None,
                    result_ids: Vec::new(),
                    entity_ids: Vec::new(),
                    output_revision: None,
                    detail_id: None,
                    file_path: Some("notes/filing.md".into()),
                    status: ChatProgressStatus::Succeeded,
                    refusal: None,
                    started_at_ms: 1,
                    finished_at_ms: Some(2),
                }],
                plan: None,
                approvals: Vec::new(),
                questions: Vec::new(),
            }),
            ..chat_message("answer", ChatRole::Assistant, "First answer")
        },
    ];
    let cold = |current: &str| {
        pi_chat_turn_prompt(
            current,
            TURN_ENVELOPE,
            Some(ColdChatBootstrap {
                attachments_root: root,
                thread_id: "chat-a",
                history: &history,
                current_image_bytes: 0,
                current_image_count: 0,
                historical_images: &mut Vec::new(),
            }),
        )
        .unwrap()
    };
    let prompt = cold("Follow-up question");
    for expected in [
        "First question",
        "First answer",
        "Follow-up question",
        "cohort-notes.txt",
        "Revenue quality improved after low-margin churn.",
        // Invalid historical text degrades to its metadata.
        "legacy.txt",
        "Quarterly chart",
        "notes/filing.md",
    ] {
        assert!(prompt.contains(expected), "cold prompt omitted {expected}");
    }
    assert_eq!(prompt.matches("Samsung 2025 filing").count(), 1);
    assert!(!prompt.contains("Revenue rose."));

    fs::write(
        directory.join("attachment-a.md"),
        "Extracted prefix from the original note.",
    )
    .unwrap();
    assert!(cold("Again").contains("Extracted prefix from the original note."));

    let warm = pi_chat_turn_prompt("Warm follow-up", TURN_ENVELOPE, None).unwrap();
    assert!(warm.contains("Warm follow-up"));
    assert!(!warm.contains("First answer"));
}

#[test]
fn native_chat_prompt_expands_current_skills_and_preserves_memory_authority() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let temporary = tempfile::tempdir().unwrap();
    let history = vec![chat_message(
        "earlier-request",
        ChatRole::User,
        "/skill:manual-study Compare the 2025 fiscal periods in KRW.",
    )];
    let cases = [
        (
            "/skill:manual-study compare these rows",
            false,
            false,
            false,
        ),
        (
            "/skill:manual-study continue with these rows",
            true,
            true,
            false,
        ),
        (
            "Continue that comparison; use USD instead of KRW.",
            true,
            true,
            true,
        ),
        (
            "Explain the text /skill:manual-study without running it.",
            true,
            false,
            false,
        ),
    ];
    let input = cases
        .iter()
        .map(|(current, cold, read_memory, save_memory)| {
            let mut images = Vec::new();
            let prompt = pi_chat_turn_prompt(
                current,
                TURN_ENVELOPE,
                cold.then_some(ColdChatBootstrap {
                    attachments_root: temporary.path(),
                    thread_id: "chat-native-prompt",
                    history: &history,
                    current_image_bytes: 0,
                    current_image_count: 0,
                    historical_images: &mut images,
                }),
            )
            .unwrap();
            let profile =
                agent_harness::AgentRuntimeProfile::new("chat", *read_memory, *save_memory, &[])
                    .unwrap();
            serde_json::json!({ "prompt": prompt, "runtime_profile": profile })
        })
        .collect::<Vec<_>>();

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/commands/tests/fixtures/native_prompt_probe.mjs");
    let mut command = Command::new("node");
    command
        .arg(fixture)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temporary.path())
        .env("USERPROFILE", temporary.path())
        .env("TMPDIR", temporary.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SYSTEMROOT") {
        command.env("SYSTEMROOT", system_root);
    }
    let mut child = command
        .spawn()
        .expect("the app's Node runtime is required for native Pi tests");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "native prompt probe: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(observed.len(), cases.len());
    for (index, (current, cold, read_memory, save_memory)) in cases.iter().enumerate() {
        let result = &observed[index];
        let user_text = result["user_text"].as_str().unwrap();
        assert!(user_text.contains(current));
        assert_eq!(user_text.contains("MANUAL_STUDY_BODY"), index < 2);
        assert_eq!(result["manual_skill_advertised"], false);
        assert_eq!(result["runtime"]["memory_access_enabled"], *read_memory);
        assert_eq!(result["runtime"]["memory_update_enabled"], *save_memory);
        assert_eq!(result["source_component_loadable"], *save_memory);
        let tools = result["registered_tools"].as_array().unwrap();
        assert!(tools.iter().any(|name| name == "chat_history"));
        for writer in ["source_save", "note_save", "principle_save"] {
            assert_eq!(tools.iter().any(|name| name == writer), *save_memory);
        }
        assert_eq!(user_text.contains(&history[0].content), *cold);
    }
}

#[test]
fn cold_chat_historical_images_respect_the_four_image_turn_budget() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let directory = root.join("chat-a/message-images");
    fs::create_dir_all(&directory).unwrap();
    let attachments = (0..2_u8)
        .map(|index| {
            let id = format!("image-{index}");
            let bytes = vec![index + 1; 8];
            fs::write(directory.join(&id), &bytes).unwrap();
            stored_attachment(&id, &format!("chart-{index}.png"), "image/png", &bytes)
        })
        .collect();
    let history = vec![ChatMessage {
        attachments,
        ..chat_message("message-images", ChatRole::User, "Compare these charts")
    }];

    for (current_count, expected_historical) in [(0, 2), (2, 2), (3, 1), (4, 0)] {
        let mut historical = Vec::new();
        bootstrap_pi_chat_session_from_sqlite(
            root,
            "chat-a",
            &history,
            "Continue",
            TURN_ENVELOPE,
            0,
            current_count,
            &mut historical,
        )
        .unwrap();
        assert_eq!(historical.len(), expected_historical);
    }
}

#[test]
fn answer_feedback_persists_clears_and_enforces_message_authority() {
    use crate::commands::{records::set_feedback, types::ChatFeedbackRequest};
    use crate::domain::ChatFeedbackRating;

    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = profile("loupist-feedback", &directory.path().join("workspace"), 1);
    seed_profile(state.store.as_ref(), &loupist);
    let other_loupist = profile(
        "other-loupist",
        &directory.path().join("other-workspace"),
        1,
    );
    seed_profile(state.store.as_ref(), &other_loupist);
    let mut thread = chat("chat-feedback", &loupist.id, 1);
    thread.messages = vec![
        chat_message("user-feedback", ChatRole::User, "Question"),
        chat_message("answer-feedback", ChatRole::Assistant, "Answer"),
        ChatMessage {
            status: ChatMessageStatus::Error,
            ..chat_message("failed-feedback", ChatRole::Assistant, "Failed")
        },
        ChatMessage {
            status: ChatMessageStatus::Aborted,
            ..chat_message("stopped-feedback", ChatRole::Assistant, "Stopped")
        },
    ];
    state.store.create_chat(&thread).unwrap();
    let request = || ChatFeedbackRequest {
        loupist_id: loupist.id.clone(),
        thread_id: thread.id.clone(),
        message_id: "answer-feedback".into(),
        rating: Some(ChatFeedbackRating::Down),
        correction: Some("  Use consolidated profit.  ".into()),
    };
    let saved = set_feedback(&state, request()).unwrap();
    assert_eq!(
        saved.correction.as_deref(),
        Some("Use consolidated profit.")
    );
    let feedback = || {
        state.store.get_chat(&thread.id).unwrap().unwrap().messages[1]
            .feedback
            .clone()
    };
    assert_eq!(feedback(), Some(saved.clone()));

    let mut refused = vec![
        ChatFeedbackRequest {
            loupist_id: other_loupist.id.clone(),
            ..request()
        },
        ChatFeedbackRequest {
            correction: Some("가".repeat(4001)),
            ..request()
        },
    ];
    for id in [
        "user-feedback",
        "failed-feedback",
        "stopped-feedback",
        "missing",
    ] {
        refused.push(ChatFeedbackRequest {
            message_id: id.into(),
            ..request()
        });
    }
    for invalid in refused {
        assert!(set_feedback(&state, invalid).is_err());
    }
    assert_eq!(feedback(), Some(saved));

    let cleared = set_feedback(
        &state,
        ChatFeedbackRequest {
            rating: None,
            correction: Some("  ".into()),
            ..request()
        },
    )
    .unwrap();
    assert!(cleared.rating.is_none() && cleared.correction.is_none());
    assert_eq!(feedback(), Some(cleared));
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn finished_answers_store_number_checks_and_the_as_of_date() {
    use crate::commands::chat_runtime::{send_chat, ChatEventSink};
    use crate::domain::{NumberCheckKind, NumberCheckStatus};

    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "answer-checks");
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("fixtures/answer_checks_pi.py"),
    );
    let profile = profile("loupist-answer-checks", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let chat = chat("chat-answer-checks", &profile.id, 1);
    state.store.create_chat(&chat).unwrap();

    // A date after today or before 1990 is refused before anything is stored.
    for date in ["2999-01-01", "1989-12-31", "2026-3-3"] {
        let request = ChatSendRequest {
            as_of: Some(date.into()),
            ..send_request("answer-checks-bad", &profile.id, &chat.id, "Margin?")
        };
        assert!(
            send_chat(request, ChatEventSink::new(|_| Ok(())), &state, None)
                .await
                .is_err(),
            "{date}"
        );
    }
    assert!(state
        .store
        .get_chat(&chat.id)
        .unwrap()
        .unwrap()
        .messages
        .is_empty());

    let request = ChatSendRequest {
        as_of: Some("2026-03-03".into()),
        ..send_request(
            "answer-checks-1",
            &profile.id,
            &chat.id,
            "What was the margin?",
        )
    };
    send_chat(request, ChatEventSink::new(|_| Ok(())), &state, None)
        .await
        .unwrap();
    drain(&state).await;
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    let (user, answer) = (&stored.messages[0], stored.messages.last().unwrap());
    assert_eq!(answer.status, ChatMessageStatus::Complete);
    assert_eq!(user.as_of.as_deref(), Some("2026-03-03"));
    assert_eq!(answer.as_of.as_deref(), Some("2026-03-03"));
    assert_eq!(
        answer
            .as_of_report
            .as_ref()
            .map(|report| report.cutoff.as_str()),
        Some("2026-03-03")
    );
    // The as-of envelope reached the model beside the live time.
    let prompt = fs::read_to_string(temporary.path().join("prompt.txt")).unwrap();
    assert!(prompt.contains("loupist-as-of/1"), "{prompt}");
    // Every number link was checked against the calculation; "the calculation" is not a number.
    let checks: Vec<_> = answer
        .number_checks
        .iter()
        .map(|check| (check.text.as_str(), check.kind, check.status))
        .collect();
    assert_eq!(
        checks,
        [
            (
                "8.42%",
                NumberCheckKind::Calculated,
                NumberCheckStatus::Matched
            ),
            (
                "9.1%",
                NumberCheckKind::Calculated,
                NumberCheckStatus::Differs
            ),
        ]
    );
    assert_eq!(answer.number_checks[1].nearest.as_deref(), Some("8.42%"));
    let calc = answer.number_checks[0].calc.as_ref().unwrap();
    assert_eq!(
        calc.formulas,
        ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"]
    );
    assert!(calc.inputs.iter().all(|input| input.evidence_id.is_none()));
    // The renderer reads the same checks.
    let dto = crate::commands::chat_dto(&stored).unwrap();
    let wire = serde_json::to_value(dto.messages.last().unwrap()).unwrap();
    assert_eq!(wire["number_checks"][1]["status"], "differs");
    assert_eq!(wire["as_of"], "2026-03-03");
    assert_eq!(wire["as_of_report"]["cutoff"], "2026-03-03");
}
