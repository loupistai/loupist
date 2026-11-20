use super::*;
use chrono::{Duration, NaiveDate};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tauri::{Listener, Manager};
use tokio::sync::oneshot;

use crate::{
    broker::{ToolExecutor, ToolMethod},
    commands::tests::{profile, seed_profile},
    finance_data::fx::expected_date,
};

const ON: &str = "loupist-a";
const OFF: &str = "loupist-off";

fn fixture(root: &tempfile::TempDir) -> AppState {
    let (state, _) = warm_fixture_with_tools(root, 0, &[]);
    seed_profile(
        state.store.as_ref(),
        &profile(OFF, &root.path().join("off-workspace"), 1),
    );
    let mut binding = state
        .store
        .get_loupist_capability(OFF, "openbb.platform")
        .unwrap()
        .unwrap();
    binding.enabled = false;
    state.store.save_loupist_capability(&binding).unwrap();
    state
}

fn configured(state: &AppState, currency: &str) {
    std::fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        format!("[appearance]\ndisplay_currency = '{currency}'\n"),
    )
    .unwrap();
}

fn snapshot<F, Fut>(state: &AppState, loupist: &str, now: DateTime<Utc>, fetch: F) -> Value
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<Value, BrokerError>> + Send + 'static,
{
    let request = FxDisplayRequest {
        loupist_id: loupist.into(),
    };
    serde_json::to_value(read(state, request, now, move |_, _, _| fetch()).unwrap()).unwrap()
}

async fn unexpected_fetch() -> Result<Value, BrokerError> {
    panic!("the request must not start a provider read")
}

fn rates(day: NaiveDate) -> Value {
    json!({"date":day.to_string(), "USD":1.1014, "KRW":1470.23})
}

async fn changes(state: &AppState) -> Vec<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let changes = state.fx_reference.take_display_changes();
            if !changes.is_empty() {
                return changes
                    .into_iter()
                    .map(|change| serde_json::to_value(change).unwrap())
                    .collect();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("refresh must publish its invalidation")
}

#[tokio::test]
async fn two_display_reads_return_before_one_refresh_shared_with_a_tool() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&root);
    let now = Utc::now();
    assert_eq!(
        snapshot(&state, ON, now, unexpected_fetch),
        json!({"state":"unset"})
    );
    assert!(!state
        .artifacts
        .app_data_dir
        .join("cache/fx-reference")
        .exists());
    let missing = read(
        &state,
        FxDisplayRequest {
            loupist_id: "missing".into(),
        },
        now,
        |_, _, _| unexpected_fetch(),
    )
    .unwrap_err();
    assert_eq!(missing.code, "not_found");

    configured(&state, "KRW");
    let (started_tx, started) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let count = Arc::new(AtomicUsize::new(0));
    let reads = count.clone();
    let day = expected_date(now);
    let waiting = json!({"state":"waiting","currency":"KRW"});
    assert_eq!(
        snapshot(&state, ON, now, move || async move {
            reads.fetch_add(1, Ordering::SeqCst);
            started_tx.send(()).unwrap();
            released.await.unwrap();
            Ok(rates(day))
        }),
        waiting
    );
    started.await.unwrap();
    assert_eq!(snapshot(&state, ON, now, unexpected_fetch), waiting);
    assert_eq!(
        snapshot(&state, OFF, now, unexpected_fetch),
        json!({
            "state":"unavailable","currency":"KRW","reason":"connection_off"
        })
    );

    let session = crate::chat::home::ensure_home_thread(&state, ON).unwrap();
    let policy = ToolPolicy {
        loupist_id: ON.into(),
        session_id: session.id,
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
        display_currency: Some("KRW".into()),
    };
    let executor = AppToolExecutor::for_host(&state, ON, ["openbb.platform".into()].into());
    let mut tool =
        std::pin::pin!(executor.execute(&policy, ToolMethod::FxRate, json!({"from":"USD"})));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut tool)
            .await
            .is_err()
    );
    release.send(()).unwrap();
    let pair = tool.await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let revision = format!("ecb:{day}");
    assert_eq!(
        changes(&state).await,
        vec![json!({"state":"ready","revision":revision})]
    );
    let ready = snapshot(&state, ON, now, unexpected_fetch);
    let until = ready["usable_until"].as_str().unwrap();
    assert!(until.parse::<DateTime<Utc>>().unwrap() > now);
    assert_eq!(
        ready,
        json!({"state":"ready","currency":"KRW","date":day.to_string(),
            "revision":revision,"source":"European Central Bank euro foreign exchange reference rates",
            "fetchedAt":now.to_rfc3339(),"usable_until":until,"rates":{"EUR":1470.23,"KRW":1.0,"USD":1334.87}
        })
    );
    assert_eq!(pair["to_per_from"], ready["rates"]["USD"]);
    assert_eq!(pair["revision"], ready["revision"]);
    assert_eq!(
        snapshot(&state, OFF, now, unexpected_fetch)["behind"],
        "connection_off"
    );
    assert!(state.fx_reference.take_display_changes().is_empty());
    configured(&state, "ZZZ");
    assert_eq!(
        snapshot(&state, OFF, now, unexpected_fetch),
        json!({
            "state":"unavailable","currency":"ZZZ","reason":"no_rate_for_currency","lastDate":day.to_string()
        })
    );
}

#[tokio::test]
async fn seven_publication_days_are_usable_and_eight_take_precedence() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&root);
    // Berlin changes from CET to CEST during this table's lifetime.
    let now: DateTime<Utc> = "2026-03-23T16:00:00Z".parse().unwrap();
    let day = now.date_naive();
    configured(&state, "KRW");
    state
        .fx_reference
        .resolve(now, true, || async { Ok(rates(day)) })
        .await
        .unwrap();
    state.fx_reference.take_display_changes();
    let first = snapshot(&state, OFF, now, unexpected_fetch);
    let cutoff = first["usable_until"]
        .as_str()
        .unwrap()
        .parse::<DateTime<Utc>>()
        .unwrap();
    assert_eq!(
        cutoff,
        "2026-03-30T22:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    let ready = snapshot(
        &state,
        OFF,
        cutoff - Duration::nanoseconds(1),
        unexpected_fetch,
    );
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["usable_until"], first["usable_until"]);

    let (release, released) = oneshot::channel();
    let old = snapshot(&state, ON, cutoff, move || async move {
        released.await.unwrap();
        Ok(rates(expected_date(cutoff)))
    });
    let expired =
        json!({"state":"unavailable","currency":"KRW","reason":"too_old","lastDate":"2026-03-23"});
    assert_eq!(old, expired);
    assert_eq!(
        snapshot(
            &state,
            OFF,
            cutoff + Duration::nanoseconds(1),
            unexpected_fetch
        ),
        expired
    );
    configured(&state, "ZZZ");
    assert_eq!(
        snapshot(&state, OFF, cutoff, unexpected_fetch)["reason"],
        "too_old"
    );
    release.send(()).unwrap();
    assert_eq!(changes(&state).await[0]["state"], "ready");
}

#[tokio::test]
async fn grant_and_currency_commands_notify_two_windows_only_on_changes() {
    use crate::{
        commands::configuration::{config_read, config_update},
        marketplace,
    };

    let root = tempfile::tempdir().unwrap();
    let state = fixture(&root);
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state.clone());
    *state.fx_reference.event_test_app.lock().unwrap() = Some(app.handle().clone());
    let windows = ["editor", "viewer"].map(|label| {
        let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        window.listen(crate::events::FX_CHANGED, move |event| {
            send.send(serde_json::from_str::<Value>(event.payload()).unwrap())
                .unwrap();
        });
        (window, receive)
    });
    let assert_hints = |expected| {
        for (_, listener) in &windows {
            let events: Vec<_> = listener.try_iter().collect();
            assert_eq!(events, vec![json!({"state":"invalidated"}); expected]);
        }
    };
    let binding = || marketplace::LoupistCapabilityRequest {
        loupist_id: OFF.into(),
        entry_id: "openbb.platform".into(),
    };
    marketplace::loupist_capability_enable(binding(), app.state())
        .await
        .unwrap();
    assert_hints(1);
    marketplace::loupist_capability_enable(binding(), app.state())
        .await
        .unwrap();
    assert_hints(0);
    marketplace::loupist_capability_disable(binding(), app.state())
        .await
        .unwrap();
    assert_hints(1);
    marketplace::loupist_capability_disable(binding(), app.state())
        .await
        .unwrap();
    assert_hints(0);

    let mut config = config_read(
        serde_json::from_value(json!({"scope":{"kind":"global"}})).unwrap(),
        app.state(),
    )
    .unwrap();
    for (set, unset, currency, expected) in [
        (
            json!({"appearance":{"display_currency":"KRW"}}),
            json!([]),
            Some("KRW"),
            1,
        ),
        (
            json!({"appearance":{"display_currency":"KRW"}}),
            json!([]),
            Some("KRW"),
            0,
        ),
        (
            json!({"appearance":{"display_currency":"JPY"}}),
            json!([]),
            Some("JPY"),
            1,
        ),
        (
            json!({"appearance":{"theme":"dark"}}),
            json!([]),
            Some("JPY"),
            0,
        ),
        (
            json!({}),
            json!([["appearance", "display_currency"]]),
            None,
            1,
        ),
        (
            json!({}),
            json!([["appearance", "display_currency"]]),
            None,
            0,
        ),
    ] {
        config = config_update(
            serde_json::from_value(json!({
                "scope":{"kind":"global"}, "expectedRevision":config.revision,
                "set":set, "unset":unset
            }))
            .unwrap(),
            app.state(),
        )
        .await
        .unwrap();
        assert_eq!(
            config.effective.appearance.display_currency.as_deref(),
            currency
        );
        assert_hints(expected);
    }
    assert!(!state
        .artifacts
        .app_data_dir
        .join("cache/fx-reference")
        .exists());
    state.fx_reference.event_test_app.lock().unwrap().take();
}

#[tokio::test]
async fn settled_failure_and_recovery_invalidate_even_at_the_same_revision() {
    for cached in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = fixture(&root);
        let now: DateTime<Utc> = "2026-11-17T16:00:00Z".parse().unwrap();
        let day = expected_date(now) - Duration::days(1);
        let revision = format!("ecb:{day}");
        configured(&state, "KRW");
        if cached {
            state
                .fx_reference
                .resolve(now - Duration::hours(3), true, || async { Ok(rates(day)) })
                .await
                .unwrap();
            state.fx_reference.take_display_changes();
        }
        let before = snapshot(&state, ON, now, || async {
            Err(BrokerError::Offline("ecb".into()).with_next("retry later"))
        });
        assert_eq!(before["state"], if cached { "ready" } else { "waiting" });
        let offline_event = if cached {
            json!({"state":"ready","revision":revision})
        } else {
            json!({"state":"unavailable"})
        };
        assert_eq!(changes(&state).await, vec![offline_event.clone()]);
        let reason = if cached { "behind" } else { "reason" };
        assert_eq!(
            snapshot(&state, ON, now, unexpected_fetch)[reason],
            "offline"
        );

        let retry = now + Duration::hours(3);
        let during = snapshot(&state, ON, retry, || async {
            Err(BrokerError::Execution("HTTP 503".into()))
        });
        assert_eq!(during["state"], if cached { "ready" } else { "waiting" });
        if cached {
            assert_eq!(during["behind"], "offline");
        }
        assert_eq!(changes(&state).await, vec![offline_event]);
        assert_eq!(
            snapshot(&state, ON, retry, unexpected_fetch)[reason],
            "provider_failed"
        );

        // Let the tool own the next refresh. An old successful publication
        // clears the failure and leaves the same cached revision.
        let recovery = retry + Duration::hours(3);
        let result = state
            .fx_reference
            .resolve(recovery, true, || async { Ok(rates(day)) })
            .await
            .unwrap();
        assert!(result.behind.is_none());
        assert_eq!(
            changes(&state).await,
            vec![json!({"state":"ready","revision":revision})]
        );
        let ready = snapshot(&state, ON, recovery, unexpected_fetch);
        assert_eq!(ready["state"], "ready");
        assert!(ready.get("behind").is_none());
        assert!(state.fx_reference.take_display_changes().is_empty());
    }
}

#[cfg(unix)]
fn warm_fixture_with_tools(
    temporary: &tempfile::TempDir,
    delay_ms: u64,
    extra_tools: &[serde_json::Value],
) -> (AppState, std::path::PathBuf) {
    use crate::commands::tests::{profile, seed_profile};
    let mut state = AppState::for_test(temporary.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", &temporary.path().join("workspace"), 1),
    );
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../openbb/runtime-manifest.json")).unwrap();
    let providers = manifest["providers"].as_array().unwrap();
    let mut runtime = crate::mcp::BundledMcpRuntime {
        executable: std::path::PathBuf::new(),
        lease_dir: temporary.path().join("leases"),
        allowed_categories: serde_json::from_value(manifest["allowed_categories"].clone()).unwrap(),
        provider_ids: providers
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect(),
        provider_network_hosts: providers
            .iter()
            .map(|p| {
                (
                    p["id"].as_str().unwrap().to_owned(),
                    serde_json::from_value(p["network_hosts"].clone()).unwrap(),
                )
            })
            .collect(),
        provider_config_fields: Default::default(),
        providerless_tool_policy: crate::mcp::ProviderlessToolPolicy::default(),
        provider_receipt_pointer: "/structuredContent/provider".into(),
        tool_activation: None,
        control_tool_names: serde_json::from_value(
            manifest["protocol"]["control_tool_names"].clone(),
        )
        .unwrap(),
    };
    let transcript = temporary.path().join("requests");
    let executable = temporary.path().join("market-fixture");
    let tools: Vec<_> = runtime
        .control_tool_names
        .iter()
        .map(|name| json!({"name": name, "inputSchema": {"type": "object"}}))
        .collect();
    let active_tools: Vec<_> = tools.iter().chain(extra_tools).collect();
    // A native fixture preserves the executable identity required by process leases.
    // A shebang script would run as its interpreter and fail that check.
    let source = temporary.path().join("market-fixture.c");
    let source_text = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(void) {
    char *line = NULL;
    size_t capacity = 0;
    int activated = 0;
    FILE *transcript = fopen(TRANSCRIPT, "a");
    if (!transcript) return 1;
    while (getline(&line, &capacity, stdin) >= 0) {
        fputs(line, transcript); fflush(transcript);
        char *id = strstr(line, "\"id\":\"");
        if (!id) continue;
        id += 6;
        char *end = strchr(id, '"');
        if (!end) return 2;
        int length = (int)(end - id);
        if (strstr(line, "\"method\":\"initialize\"")) {
            usleep(STARTUP_DELAY * 1000);
            printf("{\"jsonrpc\":\"2.0\",\"id\":\"%.*s\",\"result\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"warm-fixture\",\"version\":\"1\"}}}\n", length, id);
        } else if (strstr(line, "\"method\":\"tools/list\"")) {
            printf("{\"jsonrpc\":\"2.0\",\"id\":\"%.*s\",\"result\":{\"tools\":%s}}\n", length, id, activated ? ACTIVE_TOOLS : INITIAL_TOOLS);
        } else if (strstr(line, "\"method\":\"tools/call\"")) {
            if (strstr(line, "\"name\":\"activate_tools\"")) activated = 1;
            printf("{\"jsonrpc\":\"2.0\",\"id\":\"%.*s\",\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{}\"}]}}\n", length, id);
        }
        fflush(stdout);
    }
    free(line); fclose(transcript); return 0;
}
"#;
    std::fs::write(
        &source,
        source_text
            .replace("STARTUP_DELAY", &delay_ms.to_string())
            .replace(
                "TRANSCRIPT",
                &serde_json::to_string(&transcript.to_string_lossy()).unwrap(),
            )
            .replace(
                "INITIAL_TOOLS",
                &serde_json::to_string(&serde_json::to_string(&tools).unwrap()).unwrap(),
            )
            .replace(
                "ACTIVE_TOOLS",
                &serde_json::to_string(&serde_json::to_string(&active_tools).unwrap()).unwrap(),
            ),
    )
    .unwrap();
    let compiled = std::process::Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    runtime.executable = executable;
    runtime.tool_activation = None;
    std::sync::Arc::make_mut(&mut state.artifacts)
        .mcp_runtimes
        .insert("openbb".into(), runtime);
    let pool_root = temporary.path().join("pool");
    crate::fs::artifact_trust::ensure_private_directory(&pool_root).unwrap();
    state.mcp_pool = crate::mcp::pool::McpProcessPool::with_limits(
        pool_root,
        std::time::Duration::from_millis(20),
        4,
    );
    (state, transcript)
}
