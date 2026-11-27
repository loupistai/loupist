//! Learning occurrences through the production scheduler with a scripted Pi:
//! allowance, budget levels and the earlier occurrence come from the store.
use super::tests::{scripted_host, scripted_pi, Kept};
use super::*;

const EARLIER: &str = "2026-11-01T06:30:00+00:00";

fn scenario(learning_allowance: Option<u32>) -> Scenario {
    serde_json::from_value(json!({
        "id": "LEARN-T", "title": "Learning occurrence", "language": "en",
        "clock": "2026-11-02T06:30:00+00:00",
        "seed": {
            "watchlist": [{"name": "Vantor Shipping"}],
            "programs": [{
                "id": "vantor", "title": "Track Vantor Shipping daily",
                "prompt": "Keep what this Loupist knows about Vantor Shipping current.",
                "schedule": {"kind": "weekly", "days": [1, 2, 3, 4, 5], "hour": 6, "minute": 30, "timezone": "UTC"},
                "subjects": [{"asset": "Vantor Shipping"}],
                "depth": "standard", "scope": {"connections": "all_enabled", "web": false},
                "keeps": {"sources": true, "lens": true, "asset_reads": false},
                "report": "when_new", "delivery": {"to": "bot"},
                "previous_run": {"clock": EARLIER},
            }],
            "learning_allowance": learning_allowance,
        },
        "sessions": [{"id": "bot", "role": "probe", "memory": "remember", "turns": [{
            "user": "The Vantor program's Monday occurrence.",
            "scheduled": {"kind": "learning", "program": "vantor", "clock": "2026-11-02T06:30:00+00:00"},
        }]}],
    }))
    .unwrap()
}

async fn occurrence(scenario: &Scenario, plan: Value) -> (tempfile::TempDir, Value) {
    validate_seed(scenario).unwrap();
    for session in &scenario.sessions {
        for turn in &session.turns {
            scheduled::validate(scenario, session, turn).unwrap();
        }
    }
    let root = tempfile::Builder::new()
        .prefix("eval-learning-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::write(root.path().join("plan.json"), plan.to_string()).unwrap();
    let mut host = scripted_host(scripted_pi(
        root.path(),
        include_str!("scripted_plan_pi.py"),
    ));
    let mut kept = Kept::default();
    let _clock = clock::install(scenario.clock.as_deref()).unwrap();
    assert!(!trial::run_trial(&mut host, scenario, None, &mut kept)
        .await
        .unwrap());
    let [record] = kept.0.as_slice() else {
        panic!("one record: {:?}", kept.0);
    };
    (root, record.clone())
}

fn prompts(root: &tempfile::TempDir) -> Vec<String> {
    fs::read_to_string(root.path().join("prompts.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn a_learning_occurrence_files_under_its_frozen_budget() {
    // Unattended runs cannot capture workspace files; this run files its page.
    let plan = json!([{"calls": [
        {"name": "lens_file", "method": "lens.file", "args": {
            "pages": [{"page": {"create": {"kind": "subject", "title": "Vantor Shipping"}},
                "ops": [{"op": "put", "key": "open-questions", "heading": "Open questions",
                         "markdown": "Whether the second terminal changes Vantor Shipping's capacity.",
                         "basis": {"refs": []}},
                        {"op": "summary", "text": "What this Loupist follows about Vantor Shipping."}]}],
            "rationale": "The program keeps a page for its subject."}},
    ], "answer": "Filed the open question on Vantor Shipping's page."}]);
    let (root, record) = occurrence(&scenario(None), plan).await;
    let failures = fs::read_to_string(root.path().join("failures.jsonl")).unwrap_or_default();
    assert_eq!(record["status"], "completed", "{failures} {record}");
    let run = &record["scheduled_run"];
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["quiet"], false);
    assert_eq!(run["kind"], "learning");
    let budget = &run["budget"];
    for level in ["requests", "soft", "hard", "left_at_start"] {
        assert!(
            budget[level].as_u64().is_some_and(|value| value > 0),
            "{budget}"
        );
    }
    assert!(
        budget["soft"].as_u64() < budget["hard"].as_u64(),
        "{budget}"
    );
    assert!(
        record["memory_commits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|commit| commit["kind"] == "lens"),
        "{record}"
    );
    // The occurrence follows the seeded earlier one: not a first run.
    let earlier = chrono::DateTime::parse_from_rfc3339(EARLIER)
        .unwrap()
        .timestamp_millis()
        .to_string();
    let [prompt] = prompts(&root).try_into().unwrap();
    assert!(
        prompt.contains(&earlier),
        "the earlier occurrence's start is in the turn"
    );
}

#[tokio::test]
async fn a_learning_occurrence_with_nothing_new_goes_quiet() {
    let plan = json!([{"calls": [
        {"name": "schedule_quiet", "method": "schedule.quiet", "args": {}},
    ], "answer": "Nothing new for Vantor Shipping since the previous run."}]);
    let (_root, record) = occurrence(&scenario(None), plan).await;
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(record["scheduled_run"]["status"], "succeeded", "{record}");
    assert_eq!(record["scheduled_run"]["quiet"], true, "{record}");
    assert_eq!(record["deliveries"], json!([]));
}

#[tokio::test]
async fn the_allowance_holds_an_occurrence_below_its_soft_level() {
    let (root, record) = occurrence(&scenario(Some(3)), json!([])).await;
    assert_eq!(record["status"], "completed", "{record}");
    let run = &record["scheduled_run"];
    assert!(run["status"].is_null(), "{run}");
    assert_eq!(run["hold"]["reason"], "allowance", "{run}");
    assert_eq!(record["runs"], json!([]));
    assert!(
        prompts(&root).is_empty(),
        "a held occurrence never starts Pi"
    );
}

/// CF-001's shape: a Bot turn saves a Learning program and the user accepts
/// its card; the occurrences that follow name no program and run that one.
#[tokio::test]
async fn an_accepted_program_runs_as_the_occurrence_that_names_none() {
    let scenario: Scenario = serde_json::from_value(json!({
        "id": "LEARN-SAVED-T", "title": "A program the Bot saved", "language": "en",
        "clock": "2026-11-02T06:30:00+00:00",
        "sessions": [{"id": "bot", "role": "probe", "memory": "remember", "turns": [
            {"user": "Keep what you know about Vantor Shipping current every weekday morning.", "approvals": "accept"},
            {"user": "The program's first occurrence.", "scheduled": {"kind": "learning", "clock": "2026-11-03T06:30:00+00:00"}},
        ]}],
    }))
    .unwrap();
    let plan = json!([
        {"calls": [{"name": "learning_program_save", "method": "learning_program.save", "args": {
            "request_key": "vantor-daily", "title": "Track Vantor Shipping daily",
            "prompt": "Keep what this Loupist knows about Vantor Shipping current.",
            "schedule": {"kind": "weekly", "days": [1, 2, 3, 4, 5], "hour": 6, "minute": 30, "timezone": "UTC"},
            "subjects": [], "depth": "standard", "scope": {"connections": "all_enabled", "web": false},
            "keeps": {"sources": true, "lens": true, "asset_reads": false},
            "report": "when_new", "delivery": {"to": "bot"},
        }}], "answer": "I proposed a weekday program for Vantor Shipping."},
        // A program's first run always reports.
        {"answer": "First run: nothing about Vantor Shipping is on record yet."},
    ]);
    validate_seed(&scenario).unwrap();
    for session in &scenario.sessions {
        for turn in &session.turns {
            scheduled::validate(&scenario, session, turn).unwrap();
        }
    }
    let root = tempfile::Builder::new()
        .prefix("eval-learning-saved-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::write(root.path().join("plan.json"), plan.to_string()).unwrap();
    let mut host = scripted_host(scripted_pi(
        root.path(),
        include_str!("scripted_plan_pi.py"),
    ));
    let mut kept = Kept::default();
    let _clock = clock::install(scenario.clock.as_deref()).unwrap();
    assert!(!trial::run_trial(&mut host, &scenario, None, &mut kept)
        .await
        .unwrap());
    let failures = fs::read_to_string(root.path().join("failures.jsonl")).unwrap_or_default();
    let [saved, occurrence] = kept.0.as_slice() else {
        panic!("two records: {failures} {:?}", kept.0);
    };
    assert_eq!(
        saved["approvals"]["confirmed_tasks"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "{failures} {saved}"
    );
    assert_eq!(occurrence["status"], "completed", "{failures} {occurrence}");
    assert_eq!(
        occurrence["scheduled_run"]["status"], "succeeded",
        "{occurrence}"
    );
    assert_eq!(occurrence["scheduled_run"]["quiet"], false, "{occurrence}");
    assert_eq!(
        occurrence["saved_task"]["id"],
        saved["approvals"]["confirmed_tasks"][0]
    );
}
