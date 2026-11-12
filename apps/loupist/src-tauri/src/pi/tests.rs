use super::*;

pub(super) fn launch_config() -> PiLaunchConfig {
    PiLaunchConfig {
        executable: "pi".into(),
        runtime_dir: "runtime".into(),
        extension: "extension.mjs".into(),
        system_prompt: "SYSTEM.md".into(),
        agent_data_dir: "agent-data".into(),
        working_dir: "workbench".into(),
        private_run_dir: "run".into(),
        lease_dir: "leases".into(),
        broker_socket: "broker.sock".into(),
        broker_token: "secret".into(),
        provider: "openai".into(),
        model: "openai/gpt-5".into(),
        thinking_level: "medium".into(),
        pi_settings: Default::default(),
        run_options: std::collections::BTreeMap::new(),
        provider_credential: None,
        host_context: Some("{\"agent_runtime\":{\"mode\":\"chat\"}}".into()),
        skill_files: Vec::new(),
        session: None,
    }
}

#[test]
fn e2e_diagnostics_reconstruct_only_fixed_fields_and_http_status() {
    let private_marker = "must-not-appear-in-diagnostics";
    let response = json!({
        "type": "loupist_diagnostic", "phase": "provider_response", "status": 401,
        "headers": {"authorization": private_marker}, "error": private_marker,
        "prompt": private_marker,
    });
    assert_eq!(
        e2e_rpc_diagnostic(&response),
        Some(json!({"phase":"provider_response", "status":401}))
    );
    for status in [
        json!(99),
        json!(600),
        json!(401.5),
        json!("401"),
        Value::Null,
    ] {
        let mut invalid = response.clone();
        invalid["status"] = status;
        assert!(e2e_rpc_diagnostic(&invalid).is_none());
    }
    let mut invalid = response.clone();
    invalid["phase"] = json!(private_marker);
    assert!(e2e_rpc_diagnostic(&invalid).is_none());
    assert_eq!(
        e2e_rpc_diagnostic(&json!({
            "type":"response", "success":false, "command":private_marker, "error":private_marker,
        })),
        Some(json!({"phase":"rpc_rejected", "category":"other"}))
    );
    assert_eq!(
        e2e_rpc_diagnostic(&json!({
            "type":"message_end", "message": {"role":"assistant", "stopReason":"error", "errorMessage":private_marker},
        })),
        Some(json!({"phase":"assistant_end", "category":"unknown", "detail":"unclassified"}))
    );
    assert!(e2e_rpc_diagnostic(&json!({"type":"message_update", "text":private_marker})).is_none());
}

#[test]
fn e2e_provider_failures_emit_only_fixed_categories_and_details() {
    let private_marker = "private-token-never-emit";
    for (message, category, detail) in [
        (
            "No API key for provider: private-token-never-emit",
            "auth_rejected",
            "unclassified",
        ),
        (
            "401: private-token-never-emit",
            "auth_rejected",
            "unclassified",
        ),
        (
            "429 Too Many Requests private-token-never-emit",
            "rate_limit",
            "unclassified",
        ),
        (
            "fetch failed private-token-never-emit",
            "transport",
            "unclassified",
        ),
        (
            "OAuth refresh failed for private-token-never-emit: private-token-never-emit",
            "unknown",
            "oauth_refresh",
        ),
        (
            "Cannot find module /private-token-never-emit",
            "unknown",
            "module_load",
        ),
        (private_marker, "unknown", "unclassified"),
    ] {
        let diagnostic = e2e_rpc_diagnostic(&json!({
            "type": "message_end",
            "message": {
                "role": "assistant", "stopReason": "error", "errorMessage": message,
                "content": [{"text": private_marker}],
            },
        }))
        .unwrap();
        assert_eq!(
            (&diagnostic["category"], &diagnostic["detail"]),
            (&json!(category), &json!(detail)),
            "{message}"
        );
        assert!(!diagnostic.to_string().contains(private_marker));
    }
    for ordinary in [
        "Read the credential setup instructions",
        "Cannot find module /local/runtime.mjs",
        "Cannot read properties of undefined",
        "The model comparison is complete",
        "retry-after: 20",
        "HTTP 200 OK",
    ] {
        assert_eq!(
            e2e_provider_failure_category(ordinary),
            "unknown",
            "{ordinary}"
        );
    }
    let oversized = format!("{}invalid token", "x".repeat(8192));
    assert_eq!(e2e_provider_failure_category(&oversized), "unknown");
    assert_eq!(
        e2e_failure_category("private-token in provider error response"),
        "other"
    );
}

use tokio::io::{duplex, AsyncWriteExt};

#[cfg(unix)]
mod process_ownership {
    use super::*;
    use crate::process_lease::confirm_process_group_exit;

    fn python() -> PathBuf {
        crate::process_lease::test_python_executable()
    }

    async fn fixture(root: &Path, leased: bool, leader_exits: bool) -> PiProcess {
        let ready = root.join("ready");
        let script = r#"import os, signal, sys, time
child = os.fork()
if child == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    with open(sys.argv[1], 'w') as ready:
        ready.write(str(os.getpid()))
    while True:
        time.sleep(0.05)
if sys.argv[2] == 'exit':
    os._exit(0)
while True:
    time.sleep(0.05)
"#;
        let mut command = Command::new(python());
        command
            .args(["-c", script])
            .arg(&ready)
            .arg(if leader_exits { "exit" } else { "hold" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let group = PiProcessGroup::own(&child).unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (events, _) = broadcast::channel(8);
        let event_writer = events.clone();
        let reader = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            while read_rpc_frame(&mut stdout)
                .await
                .is_ok_and(|frame| frame.is_some())
            {}
            let _ = event_writer.send(PiEvent::Exited);
        });
        let mut process = PiProcess {
            group,
            child,
            stdin: Arc::new(Mutex::new(BufWriter::new(stdin))),
            events,
            reader,
            next_id: AtomicU64::new(1),
            host_context_file: None,
            broker_token: None,
        };
        timeout(Duration::from_secs(3), async {
            while !ready.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned descendant ready");
        if leased {
            process.group.lease = Some(
                ChildProcessLease::register(
                    &root.join("leases"),
                    ProcessKind::Pi,
                    process.group.process_group_id,
                    process.group.process_group_id,
                    &python(),
                )
                .unwrap(),
            );
        }
        process
    }

    async fn stopped(group: i32, root: &Path, leased: bool) {
        assert!(confirm_process_group_exit(group, Duration::from_secs(3))
            .await
            .is_confirmed());
        if leased {
            timeout(Duration::from_secs(3), async {
                while std::fs::read_dir(root.join("leases"))
                    .unwrap()
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().starts_with("lease-"))
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("confirmed group lease cleared");
        }
    }

    #[tokio::test]
    async fn dropping_pi_stops_its_leader_and_term_resistant_descendant() {
        let root = tempfile::tempdir().unwrap();
        let process = fixture(root.path(), true, false).await;
        let group = process.process_group_id();
        drop(process);
        stopped(group, root.path(), true).await;
    }

    #[tokio::test]
    async fn cancelled_unleased_startup_stops_the_whole_pi_group() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_path_buf();
        let (ready, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let process = fixture(&path, false, false).await;
            ready.send(process.process_group_id()).unwrap();
            std::future::pending::<()>().await;
            drop(process);
        });
        let group = started.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        stopped(group, root.path(), false).await;
    }

    #[tokio::test]
    async fn cancelled_pi_shutdown_stops_descendants_and_clears_the_confirmed_lease() {
        let root = tempfile::tempdir().unwrap();
        let process = fixture(root.path(), true, false).await;
        let group = process.process_group_id();
        let task = tokio::spawn(process.shutdown_settled());
        tokio::time::sleep(Duration::from_millis(40)).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        stopped(group, root.path(), true).await;
    }

    #[tokio::test]
    async fn pi_shutdown_confirms_descendants_before_clearing_its_lease() {
        let root = tempfile::tempdir().unwrap();
        let process = fixture(root.path(), true, false).await;
        let group = process.process_group_id();
        process.shutdown_settled().await.unwrap();
        stopped(group, root.path(), true).await;
    }

    #[tokio::test]
    async fn early_leader_exit_keeps_identity_reserved_and_disarmed_drop_cannot_signal_reuse() {
        let root = tempfile::tempdir().unwrap();
        let mut process = fixture(root.path(), false, true).await;
        assert_eq!(process.try_exit_code(), None);
        assert!(
            process.child.id().is_some(),
            "the live group leader was reaped"
        );
        process.force_stop_owned_process().await.unwrap();
        assert!(!process.group.armed);
        assert!(process.child.id().is_none());

        let mut command = Command::new("/bin/sleep");
        command.arg("30").kill_on_drop(true).process_group(0);
        let mut sentinel = command.spawn().unwrap();
        let mut sentinel_group = PiProcessGroup::own(&sentinel).unwrap();
        // Model PID reuse without relying on the kernel recycling a PID.
        process.group.process_group_id = sentinel_group.process_group_id;
        drop(process);
        assert!(!process_group_has_exited(sentinel_group.process_group_id).unwrap());
        sentinel_group
            .signal_and_confirm(libc::SIGKILL)
            .await
            .unwrap();
        sentinel.wait().await.unwrap();
    }
}

#[test]
fn launch_is_offline_and_exposes_only_explicit_extension_tools() {
    let config = launch_config();
    let args = config.rpc_arguments();
    for required in [
        "--mode",
        "rpc",
        "--no-session",
        "--no-builtin-tools",
        "--no-extensions",
        "--extension",
        "--no-skills",
        "--no-context-files",
        "--approve",
        "--offline",
        "--thinking",
    ] {
        assert!(args.iter().any(|arg| arg == required));
    }
    assert!(!args.iter().any(|arg| arg == "--api-key"));
    let system_prompt_index = args
        .iter()
        .position(|arg| arg == "--system-prompt")
        .expect("system prompt flag");
    assert_eq!(
        args[system_prompt_index + 1],
        LOUPIST_SYSTEM_PROMPT
            .replace("<!-- safety:locked -->", "")
            .replace("<!-- /safety:locked -->", "")
    );
    assert!(!args[system_prompt_index + 1].contains("safety:locked"));
    assert!(args[system_prompt_index + 1].contains("The current user defines the task."));
    assert_ne!(args[system_prompt_index + 1], "SYSTEM.md");
    let thinking_index = args
        .iter()
        .position(|arg| arg == "--thinking")
        .expect("thinking flag");
    assert_eq!(args[thinking_index + 1], "medium");
    let append_prompt_index = args
        .iter()
        .position(|arg| arg == "--append-system-prompt")
        .expect("append prompt suppression flag");
    assert_eq!(args[append_prompt_index + 1], "");
    assert!(!args
        .iter()
        .any(|arg| Some(arg) == config.host_context.as_ref()));
}

#[test]
fn support_launch_starts_on_the_manifest_bootstrap_with_only_a_placeholder_key() {
    let config = PiSupportLaunchConfig {
        executable: "pi".into(),
        runtime_dir: "runtime".into(),
        extension: "provider-extension.mjs".into(),
        agent_data_dir: "agent-data".into(),
        private_working_dir: "work".into(),
        lease_dir: "leases".into(),
        result_file: "result.json".into(),
        request_file: None,
        provider_credential: None,
        mutation_api_key: Some("mutation-key-private-fixture".into()),
    };
    let args = config.rpc_arguments();
    let value = |flag: &str| args[args.iter().position(|arg| arg == flag).unwrap() + 1].clone();
    // `agent/provider-manifest.test.mjs` checks this model in the pinned Pi catalog.
    assert_eq!(
        (value("--provider"), value("--model")),
        ("google".to_owned(), "gemini-2.5-flash".to_owned())
    );
    let env = config.environment();
    assert!(!env
        .values()
        .any(|value| value.contains("mutation-key-private-fixture")));
    assert!(!env.contains_key("LOUPIST_PROVIDER_API_KEY"));
    assert!(!args
        .iter()
        .any(|arg| arg.contains("mutation-key-private-fixture")));
    assert_eq!(
        env.get("GEMINI_API_KEY").map(String::as_str),
        Some(SUPPORT_BOOTSTRAP_KEY)
    );
    assert_eq!(
        env.keys()
            .filter(|name| is_credential_environment(name))
            .count(),
        1
    );
}

#[test]
fn chat_private_settings_install_each_execution_snapshot_without_touching_shared_auth() {
    let temporary = tempfile::tempdir().unwrap();
    let working_dir = temporary.path().join("pi-runtime");
    ensure_private_directory(&working_dir).unwrap();
    let config = PiLaunchConfig {
        agent_data_dir: temporary.path().join("agent-data"),
        working_dir: working_dir.clone(),
        private_run_dir: temporary.path().join("run"),
        lease_dir: temporary.path().join("leases"),
        broker_socket: temporary.path().join("broker.sock"),
        provider: "openai-codex".into(),
        model: "gpt-5.6-luna".into(),
        thinking_level: "max".into(),
        host_context: Some("{}".into()),
        ..launch_config()
    };

    ensure_pi_project_settings(&config).unwrap();
    let settings_path = config.runtime_working_dir().join(".pi/settings.json");
    let actual: Value = serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(actual, pi_settings_value(&config.pi_settings));
    assert!(!config.agent_data_dir.join("settings.json").exists());
    assert!(working_dir.join(".pi/settings.json").exists());
    ensure_pi_project_settings(&config).unwrap();

    let mut other = config.clone();
    other.private_run_dir = temporary.path().join("other-run");
    other.working_dir = temporary.path().join("other-chat-runtime");
    other.pi_settings.retry.enabled = false;
    other.pi_settings.compaction.reserve_tokens = 8192;
    ensure_pi_project_settings(&other).unwrap();
    let other_settings: Value = serde_json::from_slice(
        &std::fs::read(other.runtime_working_dir().join(".pi/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(other_settings["retry"]["enabled"], false);
    assert_eq!(other_settings["compaction"]["reserveTokens"], 8192);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&settings_path).unwrap()).unwrap(),
        actual
    );
    ensure_pi_project_settings(&config).unwrap();

    let mut next_turn = config.clone();
    next_turn.private_run_dir = temporary.path().join("next-turn");
    next_turn.pi_settings.retry.enabled = false;
    ensure_pi_project_settings(&next_turn).unwrap();
    assert_eq!(
        next_turn.runtime_working_dir(),
        config.runtime_working_dir()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&settings_path).unwrap()).unwrap()["retry"]
            ["enabled"],
        false
    );

    for unsupported in [
        b"{}".as_slice(),
        b"{\"transport\":\"auto\",\"httpIdleTimeoutMs\":1}\n".as_slice(),
    ] {
        std::fs::write(&settings_path, unsupported).unwrap();
        ensure_pi_project_settings(&config).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&settings_path).unwrap()).unwrap(),
            actual
        );
    }
}

#[test]
fn persistent_session_uses_exact_directory_and_id_without_mutating_its_name() {
    let temporary = tempfile::tempdir().unwrap();
    let mut config = PiLaunchConfig {
        host_context: Some("{}".into()),
        ..launch_config()
    };
    let session_id = "123e4567-e89b-42d3-a456-426614174000";
    config.session = Some(PiSessionConfig {
        id: session_id.into(),
        directory: temporary.path().to_path_buf(),
    });
    let args = config.rpc_arguments();
    assert!(!args.iter().any(|arg| arg == "--no-session"));
    assert!(!args.iter().any(|arg| arg == "--name"));
    let directory = args.iter().position(|arg| arg == "--session-dir").unwrap();
    assert_eq!(args[directory + 1], temporary.path().to_string_lossy());
    let id = args.iter().position(|arg| arg == "--session-id").unwrap();
    assert_eq!(args[id + 1], session_id);
}

#[test]
fn launch_disables_discovery_but_passes_only_exact_bundled_skills() {
    let agent_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../agent");
    let skill_files = agent_harness::resolve_skill_paths(
        &agent_root,
        &agent_harness::run_skill_ids(&[agent_harness::RESEARCH_SKILL_ID.to_owned()]).unwrap(),
    )
    .unwrap();
    let config = PiLaunchConfig {
        extension: agent_root.join("loupist-extension.mjs"),
        system_prompt: agent_root.join("SYSTEM.md"),
        ..launch_config()
    }
    .with_skills(skill_files.clone())
    .unwrap();
    let args = config.rpc_arguments();
    assert!(args.iter().any(|arg| arg == "--no-skills"));
    let skill_flags: Vec<&String> = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| (arg == "--skill").then(|| args.get(index + 1)).flatten())
        .collect();
    assert_eq!(
        skill_flags
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        skill_files
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
    );
    assert!(!skill_files.is_empty());
    let environment = config.environment(Path::new("host-context.json"), None);
    assert!(!environment.contains_key("LOUPIST_SKILL_FILES"));

    let temporary = tempfile::tempdir().unwrap();
    let copied = temporary.path().join("SKILL.md");
    std::fs::write(
        &copied,
        include_bytes!("../../../agent/skills/research/SKILL.md"),
    )
    .unwrap();
    assert!(config.clone().with_skills(vec![copied]).is_err());
}

#[test]
fn host_context_is_bounded_and_written_to_a_private_extension_file() {
    let temporary = tempfile::tempdir().unwrap();
    let private_run_dir = temporary.path().join("run");
    std::fs::create_dir(&private_run_dir).unwrap();
    let config = PiLaunchConfig {
        working_dir: temporary.path().join("workbench"),
        private_run_dir,
        host_context: None,
        ..launch_config()
    }
    .with_host_context("{\"agent_runtime\":{\"mode\":\"chat\"}}".into())
    .unwrap();
    let args = config.rpc_arguments();
    assert!(!args
        .iter()
        .any(|arg| Some(arg) == config.host_context.as_ref()));
    let context_file = write_host_context_file(&config).unwrap();
    assert_eq!(
        std::fs::read_to_string(context_file.path()).unwrap(),
        "{\"agent_runtime\":{\"mode\":\"chat\"}}"
    );
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(context_file.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        config
            .environment(context_file.path(), None)
            .get("LOUPIST_HOST_CONTEXT_FILE")
            .map(String::as_str),
        context_file.path().to_str()
    );
    assert!(config
        .clone()
        .with_host_context("x".repeat(MAX_HOST_CONTEXT_BYTES + 1))
        .is_err());

    let continuation = "original request and current host envelope";
    let config = config
        .with_continuation_prompt(continuation.into())
        .unwrap();
    let context_file = write_host_context_file(&config).unwrap();
    let parsed: Value =
        serde_json::from_slice(&std::fs::read(context_file.path()).unwrap()).unwrap();
    assert_eq!(parsed["continuation_prompt"], continuation);
    assert!(!config
        .rpc_arguments()
        .iter()
        .any(|arg| arg.contains(continuation)));
    assert!(!config
        .environment(context_file.path(), None)
        .values()
        .any(|value| value.contains(continuation)));
    for prompt in [
        " ".into(),
        "a\0b".into(),
        "한".repeat(MAX_CONTINUATION_PROMPT_BYTES / 3 + 1),
    ] {
        assert!(config.clone().with_continuation_prompt(prompt).is_err());
    }
    for invalid in [
        serde_json::json!({"continuation_prompt": 4}),
        serde_json::json!({"continuation_prompt": "valid", "extra": "x".repeat(MAX_HOST_CONTEXT_BYTES)}),
    ] {
        assert!(config
            .clone()
            .with_host_context(invalid.to_string())
            .is_err());
    }
}

/// `ps -E` is how a same-user command reads a process environment. The broker
/// token must not be there or in argv, and
/// a host `*_API_KEY` must not reach Pi when the build does not forward keys.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn broker_token_reaches_pi_only_on_an_inherited_pipe() {
    struct HostVariable(&'static str);
    impl Drop for HostVariable {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }
    let temporary = tempfile::tempdir().unwrap();
    let token = "fake-broker-token-5d0c1e";
    let host_key = "fake-host-api-key-6f1e2d";
    // A unique name no other test reads; Pi's spawn must not inherit it.
    let _host_variable = HostVariable("LOUPIST_TEST_HOST_API_KEY");
    std::env::set_var("LOUPIST_TEST_HOST_API_KEY", host_key);
    let config = PiLaunchConfig {
        broker_token: token.into(),
        // The release configuration: host keys are present but not forwarded.
        provider_credential: crate::providers::catalog::host_provider_credential(
            "openai",
            false,
            |_| Some(host_key.to_owned()),
        ),
        ..launch_config()
    };
    let mut environment = config.environment(Path::new("host-context.json"), None);
    let handoff = BrokerTokenHandoff::prepare(&config, &mut environment).unwrap();
    assert!(environment.values().all(|value| !value.contains(token)));
    assert!(!environment.keys().any(|name| name.ends_with("_API_KEY")));
    assert!(config
        .rpc_arguments()
        .iter()
        .all(|value| !value.contains(token)));

    // Stand-in for Pi: record the environment it was started with and the
    // exact bytes on fd 3, close it like the extension, then stay alive.
    let received = temporary.path().join("received");
    let started_with = temporary.path().join("environment");
    let script = r#"env > "$2"; cat <&3 > "$1.part"; exec 3<&-; mv "$1.part" "$1"; exec sleep 30"#;
    let mut child = pi_command(
        Path::new("/bin/sh"),
        vec![
            "-c".into(),
            script.into(),
            "pi".into(),
            received.to_string_lossy().into_owned(),
            started_with.to_string_lossy().into_owned(),
        ],
        temporary.path(),
        environment,
        Some(&handoff),
    )
    .spawn()
    .unwrap();
    assert!(handoff.after_spawn().is_none());
    timeout(Duration::from_secs(5), async {
        while std::fs::read_to_string(&received).map_or(true, |value| value.is_empty()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("stand-in read the token pipe");
    // One newline-terminated line, so the reader never waits for EOF.
    assert_eq!(
        std::fs::read_to_string(&received).unwrap(),
        format!("{token}\n")
    );
    let started_with = std::fs::read_to_string(&started_with).unwrap();
    assert!(started_with.contains("LOUPIST_BROKER_TOKEN_FD=3"));
    assert!(!started_with.contains(token));
    assert!(std::env::var("LOUPIST_TEST_HOST_API_KEY").is_ok());
    assert!(!started_with.contains(host_key), "{started_with}");
    assert!(
        !started_with.lines().any(|line| line
            .split('=')
            .next()
            .unwrap_or_default()
            .ends_with("_API_KEY")),
        "{started_with}"
    );

    let shown = std::process::Command::new("/bin/ps")
        .args(["-E", "-ww", "-o", "command=", "-p"])
        .arg(child.id().unwrap().to_string())
        .output()
        .unwrap();
    let shown = String::from_utf8_lossy(&shown.stdout);
    // Sandboxed shells can hide other processes' environments; when the
    // listing does include it, the token must still be absent.
    if shown.contains("LOUPIST_BROKER_TOKEN_FD=3") {
        assert!(!shown.contains(token), "{shown}");
    }
    child.kill().await.unwrap();
}

#[tokio::test]
async fn rpc_reader_uses_lf_and_strips_only_an_optional_cr() {
    let (mut writer, reader) = duplex(128);
    writer
        .write_all(b"{\"type\":\"one\"}\r\nrest\n")
        .await
        .unwrap();
    drop(writer);
    let mut reader = BufReader::new(reader);
    assert_eq!(
        read_rpc_frame(&mut reader).await.unwrap().unwrap(),
        br#"{"type":"one"}"#
    );
    assert_eq!(read_rpc_frame(&mut reader).await.unwrap().unwrap(), b"rest");
    assert!(read_rpc_frame(&mut reader).await.unwrap().is_none());
}

#[test]
fn child_environment_is_an_allowlist() {
    let config = PiLaunchConfig {
        provider_credential: Some(("OPENAI_API_KEY".into(), "credential".into())),
        host_context: None,
        ..launch_config()
    };
    let env = config.environment(Path::new("host-context.json"), None);
    assert_eq!(
        env.get("LOUPIST_E2E_CHAT_DIAGNOSTICS").map(String::as_str),
        cfg!(feature = "e2e").then_some("1"),
    );
    assert_eq!(env.get("PI_OFFLINE").map(String::as_str), Some("1"));
    assert_eq!(
        env.get("OPENAI_API_KEY").map(String::as_str),
        Some("credential")
    );
    assert_eq!(env.get("HOME"), std::env::var("HOME").ok().as_ref());
    let paths = std::env::split_paths(env.get("PATH").unwrap()).collect::<Vec<_>>();
    assert_eq!(paths[0], config.runtime_dir.join("workspace-tools"));
    assert_eq!(
        paths[1..],
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default()
    );
    assert!(!env.contains_key("BASH_ENV"));
    assert!(!env.contains_key("NODE_OPTIONS"));
    assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"));
    for name in ["TMPDIR", "TMP", "TEMP"] {
        assert_eq!(
            env.get(name),
            Some(
                &Path::new("workbench")
                    .join("temp")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }
    assert_eq!(
        env.get("LOUPIST_HOST_CONTEXT_FILE").map(String::as_str),
        Some("host-context.json")
    );
}

#[test]
fn rpc_text_and_image_requests_reject_unsupported_input() {
    assert_eq!(
        text_request("steer", "Challenge the cutoff assumption.").unwrap(),
        json!({"type": "steer", "message": "Challenge the cutoff assumption."})
    );
    for (command, message) in [("steer", " \n "), ("follow_up", "obsolete command")] {
        assert!(matches!(
            text_request(command, message),
            Err(PiError::InvalidLaunchValue)
        ));
    }
    let image = |mime_type: &str| PiImageContent {
        data: "iVBORw==".into(),
        mime_type: mime_type.into(),
    };
    assert_eq!(
        prompt_request("Inspect this chart.", &[image("image/png")]).unwrap()["images"][0]
            ["mimeType"],
        "image/png"
    );
    assert!(prompt_request("Inspect this image.", &[image("image/svg+xml")]).is_err());
}

#[tokio::test]
async fn rpc_write_times_out_when_the_child_stops_reading() {
    let (writer, _reader) = duplex(8);
    let writer = Mutex::new(writer);
    let bytes = vec![b'x'; 64 * 1024];
    assert!(matches!(
        write_rpc_bytes(&writer, &bytes, Duration::from_millis(10)).await,
        Err(PiError::WriteTimeout)
    ));
}

#[cfg(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64")
))]
#[test]
fn launch_validation_rejects_unpinned_runtime_metadata() {
    let temporary = tempfile::tempdir().unwrap();
    let executable = temporary.path().join("pi");
    let runtime = temporary.path().join("runtime");
    let agent_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../agent");
    let extension = agent_root.join("loupist-extension.mjs");
    let prompt = agent_root.join("SYSTEM.md");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::write(&executable, b"fixture").unwrap();
    std::fs::write(runtime.join(".pi-version"), PI_VERSION).unwrap();
    std::fs::write(
        runtime.join(".pi-archive.sha256"),
        pinned_pi_archive_sha256().unwrap(),
    )
    .unwrap();
    std::fs::write(
        runtime.join(".pi-executable.sha256"),
        file_sha256(&executable).unwrap(),
    )
    .unwrap();
    std::fs::write(
        runtime.join("package.json"),
        format!(r#"{{"version":"{PI_VERSION}"}}"#),
    )
    .unwrap();
    let config = PiLaunchConfig {
        executable,
        runtime_dir: runtime,
        extension,
        system_prompt: prompt,
        agent_data_dir: temporary.path().join("agent-data"),
        working_dir: temporary.path().join("workbench"),
        private_run_dir: temporary.path().join("run"),
        lease_dir: temporary.path().join("leases"),
        broker_socket: temporary.path().join("broker.sock"),
        ..launch_config()
    };
    assert!(config.validate().is_ok());
    assert!(verify_pi_runtime(&config.executable, &config.runtime_dir).is_ok());
    let mut missing_context = config.clone();
    missing_context.host_context = None;
    assert!(matches!(
        missing_context.validate(),
        Err(PiError::InvalidLaunchValue)
    ));
    std::fs::write(&config.executable, b"substituted").unwrap();
    assert!(matches!(
        verify_pi_runtime(&config.executable, &config.runtime_dir),
        Err(PiError::UntrustedRuntime)
    ));
    std::fs::write(&config.executable, b"fixture").unwrap();
    std::fs::write(config.runtime_dir.join(".pi-version"), "0.0.0").unwrap();
    assert!(matches!(
        verify_pi_runtime(&config.executable, &config.runtime_dir),
        Err(PiError::UntrustedRuntime)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_verification_does_not_block_the_async_worker() {
    let (release, wait) = std::sync::mpsc::channel();
    let verification = verify_runtime_on_blocking_pool(move || {
        wait.recv_timeout(Duration::from_secs(1))
            .map_err(|_| PiError::UntrustedRuntime)
    });
    tokio::pin!(verification);
    assert!(futures_util::poll!(&mut verification).is_pending());
    release.send(()).unwrap();
    verification.await.unwrap();
}
