//! S-R09 in Rust (finance-watchers.md §7.3): a Korea Investment balance read
//! through the real broker path never shows the user's held quantity, value
//! or price paid to the model, its evidence, its dataset or a briefing prompt.
use super::*;

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn an_account_read_never_shows_held_quantities_to_the_model() {
    let root = tempfile::Builder::new()
        .prefix("eval-holdings-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    super::tests::scripted(&mut state, root.path());
    let replay: replay::Replay = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/replay/kis-bada-balance.json"
    ))
    .unwrap();
    let _replay = replay.install(&state.credential_environment).unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    enable_connections(&state, &loupist.id, &["koreainvestment".into()]).unwrap();
    // The canaries are the user's own imported holdings.
    let preview = state
        .store
        .preview_csv_bytes(b"symbol,market,name,quantity,cost,value,currency\n990417,KRX,Bada Bank,1337,36061.2566,48213900,KRW\n")
        .unwrap();
    state
        .store
        .holdings_commit_csv(&preview.preview_id)
        .unwrap();
    fs::write(
        root.path().join("replay-case.json"),
        json!({"name": "kis_read", "method": "kis.read", "args": {"operation_id": "domestic_stock.inquire_balance", "params": {"afhr_flpr_yn": "N", "inqr_dvsn": "02", "unpr_dvsn": "01", "fund_sttl_icld_yn": "N", "fncg_amt_auto_rdpt_yn": "N", "prcs_dvsn": "00"}}}).to_string(),
    )
    .unwrap();
    let chat = new_chat(&state, &loupist.id, "holdings").unwrap();
    let session = Session {
        id: "holdings".into(),
        role: "probe".into(),
        memory: "remember".into(),
        turns: vec![],
    };
    let result = run_turn(
        &state,
        &chat,
        &session,
        &Turn {
            user: "REPLAY_CASE_EVAL: How is my Korea Investment account doing?".into(),
            ..Default::default()
        },
        "fixture",
        "medium",
        600000,
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    // The fixture's answer is the tool result exactly as the model received it.
    let delivered = result["answer"].as_str().unwrap();
    assert!(
        delivered.contains(crate::privacy::holdings::HIDDEN),
        "{delivered}"
    );
    for canary in ["1337", "1,337", "48213900", "48,213,900", "36061"] {
        assert!(
            !delivered.contains(canary),
            "{canary} reached the model: {delivered}"
        );
    }
    // Identity and the market's own price stay.
    assert!(
        delivered.contains("990417") && delivered.contains("51200"),
        "{delivered}"
    );
    let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
    let kept = serde_json::to_string(&persisted.messages).unwrap();
    assert_eq!(crate::privacy::holdings::scan(&state.store, &kept), None);
    let datasets: Vec<Vec<u8>> = {
        let connection = state.store.lock_loupist(&loupist.id).unwrap();
        let mut statement = connection
            .prepare("SELECT payload FROM datasets WHERE loupist_id=?1")
            .unwrap();
        let rows = statement
            .query_map([&loupist.id], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert!(!datasets.is_empty());
    for payload in &datasets {
        assert_eq!(
            crate::privacy::holdings::scan(&state.store, &String::from_utf8_lossy(payload)),
            None
        );
    }
    // A briefing prompt for this Loupist names the holding, never its numbers.
    let (prompt, _) =
        crate::briefings::build_prompt(&state, &loupist.id, None, crate::commands::now_ms())
            .unwrap();
    assert_eq!(crate::privacy::holdings::scan(&state.store, &prompt), None);
    assert!(crate::privacy::holdings::scan(&state.store, "you hold 1,337 shares").is_some());
    state.shutdown().await.unwrap();
}
