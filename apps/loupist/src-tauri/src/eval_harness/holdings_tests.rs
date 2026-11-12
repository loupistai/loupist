//! S-R09 in Rust (finance-watchers.md §7.3): a Korea Investment balance read
//! through the real broker path never shows the user's held quantity, value
//! or price paid to the model, its evidence, its dataset or a briefing prompt.
use super::*;

// Four-digit canaries collide with UUIDs, digests and host timings. Keep the
// literal-substring check strict, using distinctive values in the replay too.
const QUANTITY: &str = "91337133791337";
const COST: &str = "9360612566.125";
const VALUE: &str = "948213900137";
const HOLDINGS_CSV: &[u8] = b"symbol,market,name,quantity,cost,value,currency\n990417,KRX,Bada Bank,91337133791337,9360612566.125,948213900137,KRW\n";

fn seed_holdings(state: &AppState) {
    let preview = state.store.preview_csv_bytes(HOLDINGS_CSV).unwrap();
    state
        .store
        .holdings_commit_csv(&preview.preview_id)
        .unwrap();
}

fn canary_replay() -> replay::Replay {
    fn replace(value: &mut Value) {
        match value {
            Value::String(text) => match text.as_str() {
                "1337" => *text = QUANTITY.into(),
                "36061.2566" => *text = COST.into(),
                "48213900" => *text = VALUE.into(),
                _ => {}
            },
            Value::Array(items) => items.iter_mut().for_each(replace),
            Value::Object(object) => object.values_mut().for_each(replace),
            _ => {}
        }
    }
    let mut replay: replay::Replay = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/replay/kis-bada-balance.json"
    ))
    .unwrap();
    for entry in &mut replay.requests {
        if let Some(body) = &mut entry.response.body {
            replace(body);
        }
    }
    replay
}

fn account_fields(state: &AppState) -> Vec<String> {
    let Some(crate::privacy::holdings::AccountReview::Account(fields)) = state
        .finance_data
        .kis_account_review("domestic_stock.inquire_balance")
    else {
        panic!("fixture needs reviewed account fields")
    };
    fields
}

fn assert_private_fields_hidden(value: &Value, fields: &[String]) {
    let mut copy = value.clone();
    assert_eq!(
        crate::privacy::holdings::redact_fields(&mut copy, fields),
        0,
        "an account field was persisted or delivered without redaction: {value}"
    );
}

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
    let replay = canary_replay();
    let _replay = replay.install(&state.credential_environment).unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    enable_connections(&state, &loupist.id, &["koreainvestment".into()]).unwrap();
    // The canaries are the user's own imported holdings and provider values.
    seed_holdings(&state);
    let fields = account_fields(&state);
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
    let canaries =
        crate::privacy::holdings::canaries(&state.store.holdings_latest_positions().unwrap());
    assert_private_fields_hidden(&serde_json::from_str::<Value>(delivered).unwrap(), &fields);
    for canary in &canaries {
        assert!(
            !delivered.contains(canary.as_str()),
            "{canary} reached the model: {delivered}"
        );
    }
    // Identity and the market's own price stay.
    assert!(
        delivered.contains("990417") && delivered.contains("51200"),
        "{delivered}"
    );
    let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
    for message in &persisted.messages {
        if message.role == crate::domain::ChatRole::Assistant {
            assert_private_fields_hidden(
                &serde_json::from_str::<Value>(&message.content).unwrap(),
                &fields,
            );
        }
    }
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
        assert_private_fields_hidden(&serde_json::from_slice::<Value>(payload).unwrap(), &fields);
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
    for canary in canaries {
        assert!(
            crate::privacy::holdings::scan(&state.store, &format!("you hold {canary}")).is_some()
        );
    }
    state.shutdown().await.unwrap();
}

#[test]
fn holdings_canaries_do_not_confuse_host_ids_and_timings_with_account_values() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    seed_holdings(&state);
    let metadata = json!({"id":"message-90bbec5ad1a523ea5cb031f5c1337584", "created_at_ms":1790000001337_i64,
        "performance":{"total_ms":1337}, "content":{"hldg_qty":crate::privacy::holdings::HIDDEN,"prpr":"51200"}});
    let text = metadata.to_string();
    assert_eq!(
        crate::privacy::holdings::leaked(&text, &["1337".into()]),
        Some("1337")
    );
    assert_eq!(crate::privacy::holdings::scan(&state.store, &text), None);
    let replay = canary_replay();
    let body = replay
        .requests
        .iter()
        .find_map(|entry| entry.response.body.as_ref())
        .unwrap();
    let text = body.to_string();
    for canary in [QUANTITY, COST, VALUE] {
        assert!(text.contains(canary));
    }
    let fields = account_fields(&state);
    let mut body = body.clone();
    assert!(crate::privacy::holdings::redact_fields(&mut body, &fields) > 0);
    assert_private_fields_hidden(&body, &fields);
    for canary in
        crate::privacy::holdings::canaries(&state.store.holdings_latest_positions().unwrap())
    {
        assert!(
            crate::privacy::holdings::scan(&state.store, &format!("you hold {canary}")).is_some()
        );
    }
}
