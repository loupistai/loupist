//! The earnings and watcher eval seeds build their host prompts from replays,
//! with no model call (A-004 and A-007 in `evals/agent/suites/assets-v1.json`).
use super::*;

fn scenario(id: &str) -> Scenario {
    let suite: Value =
        serde_json::from_str(include_str!("../../../evals/agent/suites/assets-v1.json")).unwrap();
    let found = suite["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == id)
        .unwrap()
        .clone();
    serde_json::from_value(found).unwrap()
}

#[tokio::test]
async fn the_after_procedure_carries_the_release_and_the_seeded_guidance() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_eval(root.path().join("app"));
    let scenario = scenario("A-004");
    let replay: replay::Replay = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/replay/opendart-solsup-q1-prelim.json"
    ))
    .unwrap();
    let _replay = replay.install(&state.credential_environment).unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    seed(&state, &loupist.id, &scenario.seed).unwrap();
    let chat = new_chat(&state, &loupist.id, "probe").unwrap();
    let turn = &scenario.sessions[0].turns[0];
    let prompt = turn_prompt(&state, &chat, turn, 0).unwrap();
    assert!(prompt.contains("Record what was reported"), "{prompt}");
    assert!(
        prompt.contains("20260428990221") && prompt.contains("operatingIncome"),
        "{prompt}"
    );
    assert!(prompt.ends_with(&turn.user));
    // The before procedure is a different text.
    let before = Turn {
        user: "x".into(),
        earnings_prompt: Some(EarningsPromptSeed {
            symbol: "990221".into(),
            market: "KRX".into(),
            period: "Q1 2026".into(),
            side: "before".into(),
        }),
        ..Default::default()
    };
    assert!(turn_prompt(&state, &chat, &before, 0)
        .unwrap()
        .contains("Record what was expected"));
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_watch_check_before_the_briefing_turn_puts_the_filing_alert_in_its_prompt() {
    assert_watch_check_prompt(chrono::Utc::now()).await;
}

#[tokio::test]
async fn a_watch_check_at_seoul_midnight_puts_the_filing_alert_in_its_prompt() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-02T01:00:00+09:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_watch_check_prompt(now).await;
}

async fn assert_watch_check_prompt(now: chrono::DateTime<chrono::Utc>) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_eval(root.path().join("app"));
    let scenario = scenario("A-007");
    let replay: replay::Replay = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/replay/opendart-byeolmuri-capacity.json"
    ))
    .unwrap();
    let _replay = replay.install(&state.credential_environment).unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    seed(&state, &loupist.id, &scenario.seed).unwrap();
    enable_connections(&state, &loupist.id, &scenario.connections).unwrap();
    seed_watch_check(
        &state,
        &loupist.id,
        scenario.seed.watch_check.as_ref().unwrap(),
        now,
    )
    .await
    .unwrap();
    let chat = new_chat(&state, &loupist.id, "probe").unwrap();
    let turn = &scenario.sessions[0].turns[0];
    let prompt = turn_prompt(&state, &chat, turn, 0).unwrap();
    assert!(prompt.contains("\"watchAlerts\""), "{prompt}");
    assert!(
        prompt.contains("포장라인 증설") && prompt.contains("2026-03-18"),
        "{prompt}"
    );
    assert!(prompt.contains("rcpNo=20260318990308"), "{prompt}");
    state.shutdown().await.unwrap();
}
