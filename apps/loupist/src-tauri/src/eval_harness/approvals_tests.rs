//! A turn's `approvals` answers the cards its reply leaves: a schedule
//! proposal the Bot saved is confirmed under `accept` and stays a proposal
//! under `decline`, today's behaviour.
use super::tests::{scripted_host, scripted_pi, Kept};
use super::*;

fn proposal(approvals: Option<&str>) -> Scenario {
    let mut turn =
        json!({"user": "Keep what you know about Vantor Shipping current every weekday morning."});
    if let Some(approvals) = approvals {
        turn["approvals"] = json!(approvals);
    }
    serde_json::from_value(json!({
        "id": "CARDS-T", "title": "A proposal card", "language": "en",
        "sessions": [{"id": "bot", "role": "probe", "memory": "remember", "turns": [turn]}],
    }))
    .unwrap()
}

async fn saved_task(approvals: Option<&str>) -> Value {
    let root = tempfile::Builder::new()
        .prefix("eval-cards-")
        .tempdir_in("/tmp")
        .unwrap();
    let plan = json!([{"calls": [{"name": "learning_program_save", "method": "learning_program.save", "args": {
        "request_key": "vantor-daily", "title": "Track Vantor Shipping daily",
        "prompt": "Keep what this Loupist knows about Vantor Shipping current.",
        "schedule": {"kind": "weekly", "days": [1, 2, 3, 4, 5], "hour": 6, "minute": 30, "timezone": "UTC"},
        "subjects": [], "depth": "standard", "scope": {"connections": "all_enabled", "web": false},
        "keeps": {"sources": true, "lens": true, "asset_reads": false},
        "report": "when_new", "delivery": {"to": "bot"},
    }}], "answer": "I proposed a weekday program for Vantor Shipping."}]);
    fs::write(root.path().join("plan.json"), plan.to_string()).unwrap();
    let mut host = scripted_host(scripted_pi(
        root.path(),
        include_str!("scripted_plan_pi.py"),
    ));
    let mut kept = Kept::default();
    assert!(
        !trial::run_trial(&mut host, &proposal(approvals), None, &mut kept)
            .await
            .unwrap()
    );
    let failures = fs::read_to_string(root.path().join("failures.jsonl")).unwrap_or_default();
    let [record] = kept.0.as_slice() else {
        panic!("one record: {:?}", kept.0);
    };
    assert_eq!(record["status"], "completed", "{failures} {record}");
    let [task] = record["scheduled_tasks"].as_array().unwrap().as_slice() else {
        panic!("one saved program: {record}");
    };
    json!({"task": task, "approvals": record["approvals"]})
}

#[tokio::test]
async fn an_accepted_proposal_card_turns_the_program_on() {
    let saved = saved_task(Some("accept")).await;
    assert_eq!(saved["task"]["awaiting_confirmation"], false, "{saved}");
    assert_eq!(saved["task"]["status"], "active", "{saved}");
    assert_eq!(saved["approvals"]["answer"], "accept");
    assert_eq!(
        saved["approvals"]["confirmed_tasks"],
        json!([saved["task"]["task_id"]])
    );
}

#[tokio::test]
async fn a_declined_or_unanswered_proposal_card_stays_a_proposal() {
    for approvals in [Some("decline"), None] {
        let saved = saved_task(approvals).await;
        assert_eq!(saved["task"]["awaiting_confirmation"], true, "{saved}");
        assert_eq!(saved["task"]["status"], "paused", "{saved}");
    }
}
