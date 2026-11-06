use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(unix)]
use tokio::net::UnixStream;

struct UnusedExecutor;

#[async_trait]
impl ToolExecutor for UnusedExecutor {
    async fn execute(
        &self,
        _policy: &ToolPolicy,
        _method: ToolMethod,
        _params: Value,
    ) -> Result<Value, BrokerError> {
        panic!("unauthenticated connection must not reach the executor")
    }
}

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "session-a".into(),
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
    }
}

#[test]
fn missing_provider_service_approval_names_the_service_without_an_in_chat_permission() {
    let body = error_body(&BrokerError::ProviderApprovalRequired {
        provider: "krx.market-data".into(),
        services: vec!["KOSDAQ daily trading (ksq_bydd_trd)".into()],
    });
    assert_eq!(body["code"], "needs_approval");
    assert_eq!(body["reason"], "provider_service_approval");
    assert_eq!(body["connection_id"], "krx.market-data");
    assert!(body["message"].as_str().unwrap().contains("ksq_bydd_trd"));
    assert_eq!(body["retryable"], false);
    assert!(body.get("tool_name").is_none());
}

#[test]
fn an_offline_read_has_its_own_error_category_and_marks_the_turn_trace() {
    let body = error_body(&BrokerError::Offline("krx.market-data".into()));
    assert_eq!(body["code"], "offline");
    assert_eq!(body["retryable"], false);
    // One unreachable host is named, and other sources stay open this turn.
    let message = body["message"].as_str().unwrap();
    assert!(
        message.starts_with("krx.market-data could not be reached"),
        "{message}"
    );
    let next = body["next"].as_str().unwrap();
    assert!(
        next.contains("Other sources may still be reachable"),
        "{next}"
    );
    assert!(!format!("{message} {next}").contains("device is offline"));
    let provider = error_body(&BrokerError::Execution("provider rejected".into()));
    assert_ne!(provider["code"], body["code"]);
    let entry = |error: Value| crate::domain::ChatToolTraceEntry {
        call_id: None,
        name: "krx_daily_quote".into(),
        component: None,
        args: None,
        start_offset_ms: 0,
        duration_ms: 1,
        outcome: crate::domain::ToolTraceOutcome::Error,
        error: Some(error),
        result_bytes: None,
    };
    let mut trace = crate::domain::ChatToolTrace::default();
    trace.push(entry(provider));
    assert!(!trace.has_offline_read());
    trace.push(entry(body));
    assert!(trace.has_offline_read());
}

#[test]
fn catalog_routes_resolve_and_enforce_the_declared_authority() {
    let mut routes = std::collections::HashSet::new();
    for tool in &crate::broker::tools::TOOL_CATALOG.tools {
        let Some(route) = &tool.method else { continue };
        assert!(routes.insert(route), "ambiguous broker route: {route}");
        let method = ToolMethod::parse(route).expect("catalog route must reach a typed executor");
        assert_eq!(serde_json::to_value(method).unwrap(), tool.name);
        for (read, write) in [(false, false), (true, false), (true, true)] {
            let mut authority = policy();
            authority.use_memory = read;
            authority.propose_memory_updates = write;
            assert_eq!(
                enforce_policy(&authority, method).is_ok(),
                tool.permission.allows(read, write),
                "broker authority diverged for {route}"
            );
        }
    }
    for unknown in ["source_save", "memory.save", "mcp.unknown", ""] {
        assert!(ToolMethod::parse(unknown).is_none());
    }
}

#[test]
fn memory_reads_and_writes_need_their_own_permission() {
    for (method, write) in [
        (ToolMethod::WorldSearch, false),
        (ToolMethod::LensRead, false),
        (ToolMethod::ConversationMemory, false),
        (ToolMethod::SourceSave, true),
        (ToolMethod::NoteSave, true),
        (ToolMethod::LensUpdate, true),
    ] {
        let mut authority = policy();
        assert!(enforce_policy(&authority, method).is_err(), "{method:?}");
        authority.use_memory = true;
        assert_eq!(
            enforce_policy(&authority, method).is_ok(),
            !write,
            "{method:?}"
        );
        authority.propose_memory_updates = true;
        assert!(enforce_policy(&authority, method).is_ok(), "{method:?}");
    }
}

#[cfg(unix)]
async fn broker_request(socket_path: &PathBuf, token: &str, id: &str, method: &str) -> Value {
    let mut stream = UnixStream::connect(socket_path).await.unwrap();
    let request = json!({
        "protocol": PROTOCOL,
        "id": id,
        "token": token,
        "method": method,
        "params": {}
    });
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let response = serde_json::from_str::<Value>(&line).unwrap();
    let ack = json!({
        "protocol": PROTOCOL,
        "id": id,
        "delivered": true
    });
    reader
        .get_mut()
        .write_all(format!("{ack}\n").as_bytes())
        .await
        .unwrap();
    line.clear();
    reader.read_line(&mut line).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["committed"],
        true
    );
    response
}

#[cfg(unix)]
#[tokio::test]
async fn broker_identity_is_idle_between_sequential_instances() {
    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join("reusable-broker.sock");
    let identity = ToolBrokerIdentity::new(socket_path.clone());

    assert!(
        !socket_path.exists(),
        "idle identity must not accept clients"
    );
    let first = identity
        .start(policy(), Arc::new(UnusedExecutor))
        .await
        .unwrap();
    assert!(socket_path.exists());
    assert_eq!(first.token(), identity.token());

    let concurrent = identity.start(policy(), Arc::new(UnusedExecutor)).await;
    assert!(matches!(
        concurrent,
        Err(BrokerError::Io(ref error)) if error.kind() == io::ErrorKind::AddrInUse
    ));

    first.shutdown().await.unwrap();
    assert!(
        !socket_path.exists(),
        "shutdown must remove the accepting endpoint before releasing identity"
    );

    let second = identity
        .start(policy(), Arc::new(UnusedExecutor))
        .await
        .unwrap();
    assert_eq!(second.token(), identity.token());
    second.shutdown().await.unwrap();
    assert!(!socket_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn broker_identity_rejects_a_stale_endpoint_without_removing_it() {
    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join("stale-broker.sock");
    std::fs::write(&socket_path, b"not a broker").unwrap();
    let identity = ToolBrokerIdentity::new(socket_path.clone());

    let result = identity.start(policy(), Arc::new(UnusedExecutor)).await;
    assert!(matches!(
        result,
        Err(BrokerError::Io(ref error)) if error.kind() == io::ErrorKind::AddrInUse
    ));
    assert_eq!(std::fs::read(&socket_path).unwrap(), b"not a broker");
}

#[cfg(unix)]
#[tokio::test]
async fn sequential_brokers_do_not_share_policy_executor_or_cardinality() {
    #[derive(Default)]
    struct Executor {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ToolExecutor for Executor {
        async fn execute(
            &self,
            policy: &ToolPolicy,
            _method: ToolMethod,
            _params: Value,
        ) -> Result<Value, BrokerError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({
                "loupistId": policy.loupist_id,
                "sessionId": policy.session_id,
                "useMemory": policy.use_memory
            }))
        }
    }

    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join("turn-scoped-broker.sock");
    let identity = ToolBrokerIdentity::new(socket_path.clone());
    let first_executor = Arc::new(Executor::default());
    let mut first_policy = policy();
    first_policy.loupist_id = "loupist-first".into();
    first_policy.session_id = "session-first".into();
    first_policy.use_memory = true;
    first_policy.propose_memory_updates = true;
    first_policy.memory_proposal_budget = 3;
    let first = identity
        .start(first_policy, first_executor.clone())
        .await
        .unwrap();

    let first_memory =
        broker_request(&socket_path, first.token(), "first-memory", "world.search").await;
    assert_eq!(first_memory["ok"], true);
    assert_eq!(first_memory["result"]["sessionId"], "session-first");
    let first_journal =
        broker_request(&socket_path, first.token(), "first-journal", "source.save").await;
    assert_eq!(first_journal["ok"], true);
    first.shutdown().await.unwrap();

    let second_executor = Arc::new(Executor::default());
    let mut second_policy = policy();
    second_policy.loupist_id = "loupist-second".into();
    second_policy.session_id = "session-second".into();
    second_policy.use_memory = false;
    second_policy.propose_memory_updates = true;
    second_policy.memory_proposal_budget = 3;
    let second = identity
        .start(second_policy, second_executor.clone())
        .await
        .unwrap();

    let denied_memory = broker_request(
        &socket_path,
        second.token(),
        "second-memory",
        "world.search",
    )
    .await;
    assert_eq!(denied_memory["ok"], false);
    assert_eq!(denied_memory["error"]["code"], "memory_disabled");
    let fresh_journal = broker_request(
        &socket_path,
        second.token(),
        "second-journal",
        "source.save",
    )
    .await;
    assert_eq!(fresh_journal["ok"], true);
    assert_eq!(fresh_journal["result"]["loupistId"], "loupist-second");
    assert_eq!(fresh_journal["result"]["sessionId"], "session-second");
    second.shutdown().await.unwrap();

    assert_eq!(first_executor.calls.load(Ordering::SeqCst), 2);
    assert_eq!(second_executor.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unauthenticated_connections_never_reach_the_executor() {
    let (server, _stalled_client) = tokio::io::duplex(64);
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        handle_connection(
            server,
            "secret",
            &policy(),
            Arc::new(StdMutex::new(TransactionCardinality::default())),
            Arc::new(UnusedExecutor),
        ),
    )
    .await
    .expect("broker did not enforce authentication deadline")
    .unwrap_err();
    assert!(matches!(
        result,
        BrokerError::Io(ref error) if error.kind() == io::ErrorKind::TimedOut
    ));

    let (server, client) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move {
        handle_connection(
            server,
            "secret",
            &policy(),
            Arc::new(StdMutex::new(TransactionCardinality::default())),
            Arc::new(UnusedExecutor),
        )
        .await
    });
    let (response, _) =
        exchange(client, "wrong", "chat.history", "request-wrong-token", None).await;
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "authentication_failed");
    let _ = server_task.await.expect("the executor was never called");
}

/// Sends one request over `client`; when `ack` is set, sends that delivery ack
/// and returns the following frame (the commit barrier) as well.
async fn exchange(
    mut client: tokio::io::DuplexStream,
    token: &str,
    method: &str,
    id: &str,
    ack: Option<Value>,
) -> (Value, Option<String>) {
    let request = json!({
        "protocol": PROTOCOL,
        "id": id,
        "token": token,
        "method": method,
        "params": {"provider": "all"}
    });
    client
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let response = serde_json::from_str(&line).unwrap();
    let Some(ack) = ack else {
        let _ = reader.get_mut().shutdown().await;
        return (response, None);
    };
    reader
        .get_mut()
        .write_all(format!("{ack}\n").as_bytes())
        .await
        .unwrap();
    line.clear();
    reader.read_line(&mut line).await.unwrap();
    (response, Some(line))
}

#[derive(Default)]
struct DeliveryCounters {
    committed: AtomicUsize,
    discarded: AtomicUsize,
}

struct CountingExecutor(Arc<DeliveryCounters>);

#[async_trait]
impl ToolExecutor for CountingExecutor {
    async fn execute(
        &self,
        _policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError> {
        assert_eq!(method, ToolMethod::ChatHistory);
        Ok(json!({"received": params}))
    }

    async fn commit_delivery(
        &self,
        _policy: &ToolPolicy,
        _delivery_id: &str,
    ) -> Result<(), BrokerError> {
        self.0.committed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn discard_delivery(&self, _policy: &ToolPolicy, _delivery_id: &str) {
        self.0.discarded.fetch_add(1, Ordering::SeqCst);
    }
}

fn serve(
    executor: Arc<dyn ToolExecutor>,
) -> (
    tokio::io::DuplexStream,
    tokio::task::JoinHandle<Result<(), BrokerError>>,
) {
    let (server, client) = tokio::io::duplex(4096);
    let task = tokio::spawn(async move {
        handle_connection(
            server,
            "secret",
            &policy(),
            Arc::new(StdMutex::new(TransactionCardinality::default())),
            executor,
        )
        .await
    });
    (client, task)
}

#[tokio::test]
async fn successful_request_commits_only_after_the_delivery_ack() {
    let counters = Arc::new(DeliveryCounters::default());
    let (client, server_task) = serve(Arc::new(CountingExecutor(counters.clone())));
    let ack = json!({"protocol": PROTOCOL, "id": "request-1", "delivered": true});
    let (response, committed) =
        exchange(client, "secret", "chat.history", "request-1", Some(ack)).await;
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["received"]["provider"], "all");
    assert_eq!(
        serde_json::from_str::<Value>(&committed.unwrap()).unwrap(),
        json!({"protocol": PROTOCOL, "id": "request-1", "committed": true})
    );
    server_task.await.unwrap().unwrap();
    assert_eq!(counters.committed.load(Ordering::SeqCst), 1);
    assert_eq!(counters.discarded.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_or_mismatched_delivery_ack_discards_the_staged_result() {
    for ack in [
        None,
        Some(json!({"protocol": PROTOCOL, "id": "wrong", "delivered": true})),
    ] {
        let counters = Arc::new(DeliveryCounters::default());
        let (client, server_task) = serve(Arc::new(CountingExecutor(counters.clone())));
        let mismatched = ack.is_some();
        let (response, _) = exchange(client, "secret", "chat.history", "request-ack", ack).await;
        assert_eq!(response["ok"], true);
        assert!(
            matches!(server_task.await.unwrap(), Err(BrokerError::Malformed)),
            "mismatched ack: {mismatched}"
        );
        assert_eq!(counters.committed.load(Ordering::SeqCst), 0);
        assert_eq!(counters.discarded.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn failed_commit_does_not_emit_the_committed_frame() {
    struct Executor;

    #[async_trait]
    impl ToolExecutor for Executor {
        async fn execute(
            &self,
            _policy: &ToolPolicy,
            _method: ToolMethod,
            _params: Value,
        ) -> Result<Value, BrokerError> {
            Ok(json!({"ok": true}))
        }

        async fn commit_delivery(
            &self,
            _policy: &ToolPolicy,
            _delivery_id: &str,
        ) -> Result<(), BrokerError> {
            Err(BrokerError::Execution(
                "workspace file changed before publication".into(),
            ))
        }

        async fn discard_delivery(&self, _policy: &ToolPolicy, _delivery_id: &str) {}
    }

    let (client, server_task) = serve(Arc::new(Executor));
    let ack = json!({"protocol": PROTOCOL, "id": "request-fail", "delivered": true});
    let (response, after_ack) =
        exchange(client, "secret", "chat.history", "request-fail", Some(ack)).await;
    assert_eq!(response["ok"], true);
    assert!(!after_ack.unwrap().contains("\"committed\":true"));
    assert!(server_task.await.unwrap().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn node_client_interoperates_with_the_real_broker_commit_barrier() {
    struct SlowExecutor(CountingExecutor);

    #[async_trait]
    impl ToolExecutor for SlowExecutor {
        async fn execute(
            &self,
            policy: &ToolPolicy,
            method: ToolMethod,
            params: Value,
        ) -> Result<Value, BrokerError> {
            // A slow executor must still settle before the client's commit barrier.
            tokio::time::sleep(Duration::from_millis(250)).await;
            self.0.execute(policy, method, params).await
        }

        async fn commit_delivery(
            &self,
            policy: &ToolPolicy,
            delivery_id: &str,
        ) -> Result<(), BrokerError> {
            self.0.commit_delivery(policy, delivery_id).await
        }

        async fn discard_delivery(&self, policy: &ToolPolicy, delivery_id: &str) {
            self.0.discard_delivery(policy, delivery_id).await
        }
    }

    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join("broker.sock");
    let counters = Arc::new(DeliveryCounters::default());
    let broker = start_tool_broker(
        socket_path.clone(),
        policy(),
        Arc::new(SlowExecutor(CountingExecutor(counters.clone()))),
    )
    .await
    .unwrap();
    let client_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../agent/broker-client.mjs")
        .canonicalize()
        .unwrap();
    let script = r#"
        import { pathToFileURL } from "node:url";
        const { loadBrokerConnection, requestBroker } = await import(pathToFileURL(process.env.LOUPIST_BROKER_CLIENT).href);
        if (!loadBrokerConnection()) process.exit(3);
        const result = await requestBroker(
          "chat.history",
          { provider: "all" },
          new AbortController().signal,
        );
        if (result?.received?.provider !== "all") process.exitCode = 2;
    "#;
    // The production handoff: the token arrives on fd 3, never in the environment.
    let handoff = crate::pi::BrokerTokenHandoff::pipe(broker.token()).unwrap();
    let mut client = tokio::process::Command::new("node");
    client
        .args(["--input-type=module", "--eval", script])
        .env("LOUPIST_BROKER_CLIENT", client_path)
        .env("LOUPIST_BROKER_SOCKET", &socket_path)
        .env(
            crate::pi::BROKER_TOKEN_FD_ENV,
            crate::pi::BROKER_TOKEN_FD.to_string(),
        );
    handoff.attach(&mut client);
    let output = client.output().await.unwrap();
    drop(handoff);

    assert!(
        output.status.success(),
        "Node broker client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(counters.committed.load(Ordering::SeqCst), 1);
    assert_eq!(counters.discarded.load(Ordering::SeqCst), 0);
    broker.shutdown().await.unwrap();
}

#[test]
fn turn_budgets_bound_saves_and_publications_but_rollbacks_are_free() {
    let policy = policy();
    let mut cardinality = TransactionCardinality::default();
    cardinality
        .consume(ToolMethod::SourceSave, &policy)
        .unwrap();
    cardinality.rollback(ToolMethod::SourceSave);
    for _ in 0..MAX_CHAT_TURN_SOURCES {
        cardinality
            .consume(ToolMethod::SourceSave, &policy)
            .unwrap();
    }
    assert!(matches!(
        cardinality.consume(ToolMethod::SourceSave, &policy),
        Err(BrokerError::BudgetExceeded)
    ));

    for _ in 0..MAX_CHAT_TURN_ARTIFACTS {
        cardinality
            .consume(ToolMethod::ChartPublish, &policy)
            .unwrap();
    }
    assert!(matches!(
        cardinality.consume(ToolMethod::ArtifactPublish, &policy),
        Err(BrokerError::BudgetExceeded)
    ));

    cardinality
        .consume(ToolMethod::ResearchOpen, &policy)
        .unwrap();
    assert!(matches!(
        cardinality.consume(ToolMethod::ResearchOpen, &policy),
        Err(BrokerError::BudgetExceeded)
    ));
    cardinality.rollback(ToolMethod::ResearchOpen);
    cardinality
        .consume(ToolMethod::ResearchOpen, &policy)
        .unwrap();
}

#[tokio::test]
async fn broker_tool_trace_records_model_facing_success_and_errors() {
    struct Executor(Arc<ToolTraceCapture>);
    #[async_trait]
    impl ToolExecutor for Executor {
        fn tool_trace(&self) -> Option<Arc<ToolTraceCapture>> {
            Some(self.0.clone())
        }
        async fn execute(
            &self,
            _: &ToolPolicy,
            _: ToolMethod,
            params: Value,
        ) -> Result<Value, BrokerError> {
            tokio::time::sleep(Duration::from_millis(2)).await;
            match params["mode"].as_str() {
                Some("ok" | "mcp") => Ok(json!({"count": 3})),
                Some("large") => Ok(json!({"body": "x".repeat(MAX_FRAME_BYTES)})),
                _ => Err(BrokerError::Malformed),
            }
        }
    }
    let capture = Arc::new(ToolTraceCapture::default());
    for (index, mode) in ["ok", "bad", "large", "denied", "mcp"]
        .into_iter()
        .enumerate()
    {
        let (server, mut client) = tokio::io::duplex(4096);
        let executor = Arc::new(Executor(capture.clone()));
        let server_task = tokio::spawn(async move {
            handle_connection(
                server,
                "test-auth",
                &policy(),
                Arc::new(StdMutex::new(TransactionCardinality::default())),
                executor,
            )
            .await
        });
        let method = match mode {
            "denied" => "world.search",
            "mcp" => "mcp.call",
            _ => "chat.history",
        };
        let request = json!({
            "protocol": PROTOCOL, "id": format!("request-{index}"),
            "call_id": format!("model-call-{index}"), "token": "test-auth",
            "method": method, "params": {"mode": mode, "server_id": "fictional-connection", "tool_name": "fictional_query", "nested": [{"apiKey": "fake-sensitive-value", "ticker": "FICTION"}]}
        });
        client
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        let ack =
            json!({"protocol": PROTOCOL, "id": format!("request-{index}"), "delivered": true});
        reader
            .get_mut()
            .write_all(format!("{ack}\n").as_bytes())
            .await
            .unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["committed"],
            true
        );
        server_task.await.unwrap().unwrap();
        let trace = capture.snapshot();
        let entry = &trace.entries[index];
        assert_eq!(
            entry.call_id.as_deref(),
            Some(format!("model-call-{index}").as_str())
        );
        assert_eq!(
            entry.name,
            match mode {
                "denied" => "world_search",
                "mcp" => "fictional_query",
                _ => "chat_history",
            }
        );
        if mode != "denied" {
            assert!(entry.duration_ms >= 2);
        }
        if mode == "mcp" {
            assert_eq!(entry.component.as_deref(), Some("fictional-connection"));
        }
        assert_eq!(
            entry.args.as_ref().unwrap()["nested"][0]["apiKey"],
            "[REDACTED]"
        );
        assert_eq!(
            entry.args.as_ref().unwrap()["nested"][0]["ticker"],
            "FICTION"
        );
        if matches!(mode, "ok" | "mcp") {
            assert_eq!(entry.outcome, crate::domain::ToolTraceOutcome::Ok);
            assert_eq!(entry.result_bytes, Some(11));
            assert_eq!(entry.error, None);
        } else {
            assert_eq!(entry.outcome, crate::domain::ToolTraceOutcome::Error);
            assert_eq!(entry.error.as_ref(), response.get("error"));
            assert_eq!(
                entry.error.as_ref().unwrap()["code"],
                match mode {
                    "bad" => "malformed_request",
                    "denied" => "memory_disabled",
                    _ => "frame_too_large",
                }
            );
        }
        let serialized = serde_json::to_string(&trace).unwrap();
        assert!(!serialized.contains("test-auth"));
        assert!(!serialized.contains("fake-sensitive-value"));
        assert!(!serialized.contains("\"body\""));
    }
}

#[test]
fn broker_tool_trace_bounds_redacts_and_marks_truncation() {
    let args = trace_value(
        &json!({"AUTHORIZATION": "fake-bearer", "password": "fake-password", "access_token": "fake-token", "nested": {"private_key": "fake-key"}, "ordinary": 42}),
    );
    assert_eq!(args["ordinary"], 42);
    assert_eq!(args["AUTHORIZATION"], "[REDACTED]");
    assert_eq!(args["nested"]["private_key"], "[REDACTED]");
    assert_eq!(
        trace_value(&json!({"body": "한🙂".repeat(3000)}))["_truncated"],
        true
    );
    assert_eq!(trace_value(&json!([])), json!([]));
    let error = trace_error(
        &json!({"code": "execution_failed", "message": "\u{0001}".repeat(3000), "retryable": false, "next": "Change the request"}),
    );
    assert_eq!(error["code"], "execution_failed");
    assert!(error["message"].as_str().unwrap().contains("[truncated]"));
    assert!(serde_json::to_vec(&error).unwrap().len() <= 2048);

    for large in [false, true] {
        let capture = Arc::new(ToolTraceCapture::default());
        for index in 0..300 {
            capture
                .clone()
                .begin(
                    Some(&format!("call-{index}")),
                    "world_read",
                    None,
                    Some(&json!({"text": if large { "x".repeat(1900) } else { "ok".into() }})),
                )
                .finish(None, Some(5));
        }
        let trace = capture.snapshot();
        assert!(trace.truncated);
        assert!(trace.entries.len() <= crate::domain::MAX_TOOL_TRACE_ENTRIES);
        assert!(serde_json::to_vec(&trace).unwrap().len() <= crate::domain::MAX_TOOL_TRACE_BYTES);
        assert!(!trace.entries.is_empty());
        if large {
            assert!(trace.entries.len() < 256);
        } else {
            assert_eq!(trace.entries.len(), 256);
        }
    }
}

#[test]
fn broker_tool_trace_observes_capabilities_native_tools_and_interruption() {
    let capture = Arc::new(ToolTraceCapture::default());
    capture.observe_start("load", "capability_load", &json!({"id": "opendart"}));
    capture.observe_end("load", false, &Value::Null);
    capture.observe_start(
        "native",
        "bash",
        &json!({"command": "never-copy-this-command"}),
    );
    capture.observe_end("native", true, &Value::Null);
    // Broker-routed Pi events must not produce duplicate entries.
    capture.observe_start("broker", "world_read", &json!({"id": "a"}));
    capture.observe_end("broker", false, &Value::Null);
    drop(
        capture
            .clone()
            .begin(Some("interrupted"), "world_read", None, Some(&json!({}))),
    );
    capture.observe_start("unfinished", "read", &json!({"path": "private-path"}));
    let trace = capture.snapshot();
    assert_eq!(trace.entries.len(), 4);
    assert_eq!(trace.entries[0].component.as_deref(), Some("opendart"));
    assert_eq!(trace.entries[0].args.as_ref().unwrap()["id"], "opendart");
    assert_eq!(trace.entries[1].args, None);
    assert_eq!(
        trace.entries[1].outcome,
        crate::domain::ToolTraceOutcome::Error
    );
    assert_eq!(
        trace.entries[2].error.as_ref().unwrap()["code"],
        "interrupted"
    );
    assert_eq!(
        trace.entries[3].error.as_ref().unwrap()["code"],
        "interrupted"
    );
    let json = serde_json::to_string(&trace).unwrap();
    assert!(!json.contains("never-copy-this-command"));
    assert!(!json.contains("private-path"));
}

#[test]
fn the_trace_tells_which_tool_call_the_untrusted_text_rule_refused() {
    let capture = Arc::new(ToolTraceCapture::default());
    // Native Bash asks the broker first; its authorization carries the model's call id.
    capture
        .clone()
        .begin(
            Some("call-bash"),
            "native.authorize",
            None,
            Some(&json!({"name": "bash"})),
        )
        .finish(Some(&error_body(&BrokerError::UntrustedTextPaused)), None);
    capture
        .clone()
        .begin(
            Some("call-schedule"),
            "schedule_create",
            None,
            Some(&json!({})),
        )
        .finish(
            Some(&error_body(&BrokerError::Execution("bad cron".into()))),
            None,
        );
    capture
        .clone()
        .begin(Some("call-read"), "world_read", None, Some(&json!({})))
        .finish(None, Some(5));
    assert!(capture.refused("call-bash", "untrusted_text_paused"));
    assert!(!capture.refused("call-schedule", "untrusted_text_paused"));
    assert!(!capture.refused("call-read", "untrusted_text_paused"));
    assert!(!capture.refused("call-unknown", "untrusted_text_paused"));
}

#[test]
fn as_of_refusals_keep_their_own_codes_for_the_answers_note() {
    for (error, code) in [
        (
            BrokerError::Execution("as_of_unsupported: kis_read cannot show 2026-03-03".into()),
            "as_of_unsupported",
        ),
        (
            BrokerError::Execution("as_of_empty: no rows on or before 2026-03-03".into()),
            "as_of_empty",
        ),
        (
            BrokerError::invalid(
                "journal_review",
                "as_of_before_check: A review needs evidence from on or after the check date",
            ),
            "as_of_before_check",
        ),
        (
            BrokerError::Execution("as_of_unsupported: x".into()).with_next("Use another source."),
            "as_of_unsupported",
        ),
        // A message that only mentions a code is an ordinary failure.
        (
            BrokerError::Execution("the as_of_unsupported route failed".into()),
            "execution_failed",
        ),
    ] {
        assert_eq!(error_body(&error)["code"], code, "{error}");
    }
}
