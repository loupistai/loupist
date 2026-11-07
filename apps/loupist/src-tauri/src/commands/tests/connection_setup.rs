use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::*;
use crate::{
    broker::{BrokerError, ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::surface::ChatSurface,
    commands::tool_executor::{AppToolExecutor, ToolCapture},
    domain::LoupistCapabilityBinding,
};

fn executor(temporary: &tempfile::TempDir, surface: Option<ChatSurface>) -> AppToolExecutor {
    let state = AppState::for_test(temporary.path().join("app"));
    let workspace = temporary.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    state
        .store
        .create_chat(&chat("home-loupist-a", "loupist-a", 1))
        .unwrap();
    let mut capture = ToolCapture::for_chat("message-a".into());
    capture.surface = surface;
    AppToolExecutor {
        state,
        capture: Arc::new(capture),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "home-loupist-a".into(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 1,
        as_of: None,
    }
}

async fn setup(executor: &AppToolExecutor, arguments: Value) -> Result<Value, BrokerError> {
    executor
        .execute(&policy(), ToolMethod::ConnectionSetup, arguments)
        .await
}

fn bind(executor: &AppToolExecutor, entry_id: &str, enabled: bool) {
    executor
        .state
        .store
        .save_loupist_capability(&LoupistCapabilityBinding {
            loupist_id: "loupist-a".into(),
            entry_id: entry_id.into(),
            enabled,
            granted_permissions: vec!["execute".into()],
            config: Default::default(),
            updated_at_ms: 1,
        })
        .unwrap();
}

#[tokio::test]
async fn the_card_reports_each_state_from_readiness_and_this_loupists_binding() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, Some(ChatSurface::Bot));
    // KRX needs a key nobody entered.
    let shown = setup(&executor, json!({"id": "krx.market-data"}))
        .await
        .unwrap();
    assert_eq!(shown["status"], "card_shown");
    assert_eq!(shown["id"], "krx.market-data");
    assert_eq!(shown["name"], "KRX Open API");
    assert_eq!(shown["state"], "needs_setup");
    let notice = shown["notice"].as_str().unwrap();
    assert!(
        notice.contains("never repeat one") && notice.contains("next message"),
        "{notice}"
    );

    // World Bank is keyless, so it is ready and the host enabled it for every
    // Loupist; only a binding the user turned off makes it "off here".
    let on_by_default = setup(&executor, json!({"id": "world-bank.indicators"}))
        .await
        .unwrap();
    assert_eq!(on_by_default["state"], "ready");
    bind(&executor, "world-bank.indicators", false);
    assert_eq!(
        setup(&executor, json!({"id": "world-bank.indicators"}))
            .await
            .unwrap()["state"],
        "off_here"
    );
    bind(&executor, "world-bank.indicators", true);
    let ready = setup(&executor, json!({"id": "world-bank.indicators"}))
        .await
        .unwrap();
    assert_eq!(ready["state"], "ready");
    assert!(ready["notice"]
        .as_str()
        .unwrap()
        .contains("capability_load"));
}

#[tokio::test]
async fn names_resolve_and_a_miss_lists_candidates() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, Some(ChatSurface::Session));
    for (query, id) in [
        ("FRED", "fred.macro"),
        ("sec edgar", "sec.edgar"),
        ("opendart", "opendart.disclosures"),
    ] {
        assert_eq!(
            setup(&executor, json!({"id": query})).await.unwrap()["id"],
            id
        );
    }
    let error = setup(&executor, json!({"id": "Bloomberg"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("alpha-vantage.market-data (Alpha Vantage)"),
        "{error}"
    );
    let ambiguous = setup(&executor, json!({"id": "openbb"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        ambiguous.contains("fred.macro (FRED)") && !ambiguous.contains("krx."),
        "{ambiguous}"
    );
}

#[tokio::test]
async fn extra_fields_keys_and_blank_ids_are_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, Some(ChatSurface::Bot));
    for arguments in [
        json!({"id": "fred.macro", "api_key": "abc"}),
        json!({}),
        json!({"id": ""}),
        json!({"id": "x".repeat(97)}),
        json!({"id": "api_key=sk-live-0123456789abcdef"}),
    ] {
        let error = setup(&executor, arguments.clone()).await.unwrap_err();
        assert!(
            matches!(error, BrokerError::InvalidArguments { .. }),
            "{arguments}: {error}"
        );
    }
}

#[tokio::test]
async fn only_interactive_surfaces_show_the_card() {
    for (surface, said) in [
        (Some(ChatSurface::Scheduled), "a scheduled turn"),
        (
            Some(ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            }),
            "a delegated turn",
        ),
        (None, "a host turn"),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let executor = executor(&temporary, surface);
        let error = setup(&executor, json!({"id": "fred.macro"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(said), "{error}");
    }
}
