use super::*;
use serde_json::json;

#[test]
fn assistant_attempts_and_measured_compaction_share_the_turn_counter() {
    let mut usage = AssistantTokenUsage {
        credential_kind: Some(CredentialKind::ApiKey),
        ..Default::default()
    };
    for event in [
        json!({"type":"message_end","message":{"role":"assistant","stopReason":"error","usage":{"input":1,"output":2,"cacheRead":3,"cacheWrite":4,"cost":{"total":0.0000016,"totalFromProvider":true}}}}),
        json!({"type":"auto_retry_start"}),
        json!({"type":"message_end","message":{"role":"assistant","stopReason":"toolUse","usage":{"input":5,"output":6,"cacheRead":7,"cacheWrite":8,"reasoning":3,"cost":{"total":0.0000026,"totalFromProvider":true}}}}),
        json!({"type":"message_end","message":{"role":"toolResult","usage":{"input":999}}}),
        json!({"type":"message_end","message":{"role":"user","usage":{"input":999}}}),
        json!({"type":"compaction_end","result":{"usage":{"input":9,"output":10,"cacheRead":11,"cacheWrite":12,"reasoning":4,"cost":{"total":0.0000036}}}}),
        json!({"type":"compaction_end","aborted":true}),
        json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop","usage":{"input":13,"output":14,"cacheRead":15,"cacheWrite":16,"reasoning":5,"cost":{"total":0.0000046,"totalFromProvider":true}}}}),
    ] {
        match event["type"].as_str() {
            Some("message_end") => usage.observe_message_end(&event),
            Some("compaction_end") => usage.observe_compaction_end(&event),
            _ => {}
        }
    }
    assert_eq!(usage.model_requests(), Some(4));
    assert_eq!(
        (
            usage.input,
            usage.output,
            usage.cache_read,
            usage.cache_write
        ),
        (28, 32, 36, 40)
    );
    assert_eq!(usage.reasoning, Some(12));
    assert_eq!(
        usage.cost_estimate(),
        Some(ChatCostEstimate {
            microusd: 14,
            from_provider: false
        })
    );
    usage.credential_kind = Some(CredentialKind::Plan);
    assert_eq!(usage.cost_estimate(), None);
}

#[test]
fn switching_from_a_plan_to_a_key_keeps_requests_and_excludes_plan_cost() {
    let mut usage = AssistantTokenUsage {
        credential_kind: Some(CredentialKind::Plan),
        ..Default::default()
    };
    usage
        .observe_message_end(&json!({"message":{"role":"assistant","usage":{"cost":{"total":1}}}}));
    usage.credential_kind = Some(CredentialKind::ApiKey);
    usage.observe_message_end(&json!({"message":{"role":"assistant","usage":{"cost":{"total":0.01,"totalFromProvider":true}}}}));
    assert_eq!(usage.model_requests(), Some(2));
    assert_eq!(
        usage.cost_estimate(),
        Some(ChatCostEstimate {
            microusd: 10_000,
            from_provider: false
        })
    );
}

#[test]
fn auto_retry_counts_requests_on_each_account_and_only_one_turn() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        crate::store::SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap(),
    );
    crate::test_fixtures::create_folder_loupist(&store, "usage", &dir.path().join("workspace"));
    let now = now_ms();
    let sink = ChatEventSink::new(|_| Ok(()));
    let mut log = crate::runs::log::RunLog::new("chat", "usage", "retried", now);
    log.model = Some("openrouter/replacement".into());
    *sink.run_log.lock().unwrap() = Some((store.clone(), log));
    sink.set_usage_account("openai-codex", CredentialKind::Plan);
    sink.observe_assistant_usage(|usage| {
        usage.observe_message_end(
            &json!({"message":{"role":"assistant","stopReason":"error","usage":{
                "input": 5, "cacheRead": 0, "cacheWrite": 0, "output": 0, "cost": {"total":1}
            }}}),
        );
    });
    sink.set_usage_account("openrouter", CredentialKind::ApiKey);
    let turn = sink.observe_assistant_usage(|usage| {
        usage.observe_message_end(&json!({"message":{"role":"assistant","usage":{
            "input": 8, "cacheRead": 2, "cacheWrite": 1, "output": 4, "cost": {"total":0.01}
        }}}));
    });
    assert_eq!(turn.model_requests(), Some(2));
    sink.send(ChatStreamEvent::Aborted {
        run_id: "retried".into(),
    })
    .unwrap();

    let summary = crate::commands::usage::summarize(
        crate::commands::usage::UsageSummaryRequest {
            from_ms: now,
            to_ms: now_ms() + 1,
            time_zone: "UTC".into(),
        },
        &store,
        |_| CredentialKind::None,
    )
    .unwrap();
    assert_eq!(
        (
            summary.totals.runs,
            summary.totals.requests,
            summary.totals.input_tokens
        ),
        (1, 2, 14)
    );
    assert_eq!(
        (
            summary.surfaces[0].totals.runs,
            summary.surfaces[0].totals.requests
        ),
        (1, 2)
    );
    let [plan, key] = &summary.accounts[..] else {
        panic!("both accounts must have their own usage records");
    };
    assert_eq!(plan.provider, "openai-codex");
    assert_eq!((plan.totals.requests, plan.totals.input_tokens), (1, 5));
    assert!(plan.totals.cost_estimate.is_none());
    assert_eq!(key.provider, "openrouter");
    assert_eq!((key.totals.requests, key.totals.input_tokens), (1, 9));
    assert_eq!(key.totals.cost_estimate.unwrap().microusd, 10_000);
}

#[test]
fn an_unmeasured_retry_makes_the_measured_totals_partial() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        crate::store::SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap(),
    );
    crate::test_fixtures::create_folder_loupist(&store, "usage", &dir.path().join("workspace"));
    let now = now_ms();
    let sink = ChatEventSink::new(|_| Ok(()));
    let mut log = crate::runs::log::RunLog::new("chat", "usage", "partial", now);
    log.model = Some("openrouter/model".into());
    *sink.run_log.lock().unwrap() = Some((store.clone(), log));
    sink.set_usage_account("openrouter", CredentialKind::ApiKey);
    for message in [
        json!({"role":"assistant","usage":{"input":8,"cacheRead":2,"cacheWrite":1,"output":4}}),
        json!({"role":"assistant","stopReason":"error"}),
    ] {
        sink.observe_assistant_usage(|usage| {
            usage.observe_message_end(&json!({"message": message}))
        });
    }
    sink.send(ChatStreamEvent::Aborted {
        run_id: "partial".into(),
    })
    .unwrap();

    let summary = crate::commands::usage::summarize(
        crate::commands::usage::UsageSummaryRequest {
            from_ms: now,
            to_ms: now_ms() + 1,
            time_zone: "UTC".into(),
        },
        &store,
        |_| CredentialKind::None,
    )
    .unwrap();
    for totals in [&summary.totals, &summary.accounts[0].totals] {
        assert_eq!(
            (totals.runs, totals.requests, totals.unknown_runs),
            (1, 2, 1)
        );
        assert_eq!(
            (
                totals.input_tokens,
                totals.cached_input_tokens,
                totals.output_tokens
            ),
            (9, 2, 4)
        );
    }
}

#[test]
fn cancellation_keeps_usage_without_a_completed_answer() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        crate::store::SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap(),
    );
    crate::test_fixtures::create_folder_loupist(&store, "usage", &dir.path().join("workspace"));
    let now = now_ms();
    let mut log = crate::runs::log::RunLog::new("chat", "usage", "cancelled", now);
    log.model = Some("openrouter/model".into());
    let sink = ChatEventSink::new(|_| Ok(()));
    *sink.run_log.lock().unwrap() = Some((store.clone(), log));
    sink.set_usage_account("openrouter", CredentialKind::Environment);
    sink.observe_assistant_usage(|usage| {
        usage.observe_message_end(&json!({"message":{"role":"assistant","stopReason":"error","usage":{
            "input": 8, "cacheRead": 2, "cacheWrite": 1, "output": 4, "reasoning": 1, "cost": {"total":0.01}
        }}}));
    });
    sink.send(ChatStreamEvent::Aborted {
        run_id: "cancelled".into(),
    })
    .unwrap();
    let rows = store.model_usage_in_range(now, now_ms() + 1).unwrap();
    assert_eq!(rows.len(), 1);
    let data = &rows[0].data;
    assert!(matches!(
        data.status,
        crate::store::model_usage::ModelUsageStatus::Cancelled
    ));
    assert_eq!(
        (
            data.measures.requests,
            data.measures.input,
            data.measures.cached_input,
            data.measures.output,
            data.measures.reasoning
        ),
        (Some(1), Some(9), Some(2), Some(4), Some(1))
    );
    assert_eq!(data.measures.cost_microusd, Some(10_000));
}

#[test]
fn performance_from_before_usage_tracking_still_loads() {
    let old = json!({
        "setupMs": 1, "generationMs": 2, "totalMs": 3, "sessionCache": "cold",
        "inputTokens": 4, "outputTokens": 5, "cacheReadTokens": 6, "cacheWriteTokens": 7
    });
    let performance: crate::domain::ChatPerformance = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(performance.model_requests, None);
    assert_eq!(performance.cost_estimate, None);
    assert_eq!(serde_json::to_value(performance).unwrap(), old);
}

#[test]
fn chat_surface_follows_the_admitted_work() {
    use crate::scheduled_tasks::types::TaskKind;
    let loupist = "usage-loupist";
    let home = crate::chat::home::home_thread_id(loupist);
    assert_eq!(
        chat_usage_surface(None, false, loupist, &home),
        (UsageSurface::Bot, None)
    );
    assert_eq!(
        chat_usage_surface(None, false, loupist, "research"),
        (UsageSurface::Research, None)
    );
    assert_eq!(
        chat_usage_surface(None, true, loupist, "research"),
        (UsageSurface::Handoff, None)
    );
    for (task_kind, surface) in [
        (TaskKind::Custom, UsageSurface::Scheduled),
        (TaskKind::Briefing, UsageSurface::Briefing),
    ] {
        let scheduled = crate::domain::ScheduledTurn {
            task_id: "task".into(),
            run_id: "occurrence".into(),
            task_title: "A program".into(),
            task_kind,
        };
        assert_eq!(
            chat_usage_surface(Some(&scheduled), false, loupist, &home),
            (surface, Some(scheduled.task_id)),
        );
    }
}
