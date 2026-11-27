//! A saved check through the production scheduler with a scripted Pi: the
//! `world-change` template over a scripted ending is not met before the
//! ending is published, starts no run until it is, and is met after.
use super::tests::{scripted_host, scripted_pi, Kept};
use super::*;

const SUITE: &str = include_str!("../../../evals/agent/suites/change-feed-v1.json");

fn scenario() -> Scenario {
    let suite: Value = serde_json::from_str(SUITE).unwrap();
    let reports = suite["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == "CF-002")
        .unwrap()
        .clone();
    let mut world = reports["world"].clone();
    // The Bot saves the check before the World holds anything, so its turn reads
    // no outside text. The Q2 report is recorded after that turn, and the Q3
    // report, which ends Q2's CFO and supply claims, after the check's first
    // occurrence.
    for publication in world["publications"].as_array_mut().unwrap() {
        if publication["source"] == "alderquay-q2" {
            publication["after"] = json!({"session": "bot", "turn": 0});
            publication["recorded_time"] = json!("2026-10-20T12:00:00+00:00");
        }
        if publication["source"] == "alderquay-q3" {
            publication["after"] = json!({"session": "bot", "turn": 1});
        }
    }
    let mut seed = reports["seed"].clone();
    seed["watchlist"] = json!([{"name": "Alderquay Bearings"}]);
    seed["asset_links"] = json!([{"asset": "Alderquay Bearings", "company": "Alderquay Bearings"}]);
    serde_json::from_value(json!({
        "id": "CHECK-T", "title": "A world-change check", "language": "en",
        "clock": "2026-10-20T09:00:00+00:00", "seed": seed, "world": world,
        "sessions": [{"id": "bot", "role": "probe", "memory": "remember", "turns": [
            {"user": "Tell me when a fact about Alderquay Bearings ends.", "approvals": "accept"},
            {"user": "The check's occurrence before the Q3 report.",
             "scheduled": {"kind": "check", "clock": "2026-10-21T09:00:00+00:00", "dry_run": true}},
            {"user": "The check's occurrence after the Q3 report.",
             "scheduled": {"kind": "check", "clock": "2026-10-23T09:00:00+00:00"}},
        ]}],
    }))
    .unwrap()
}

/// A Bot turn saves a `world-change` check about an asset the user does not
/// follow, so the check's dry run finds no data, and the host refuses to turn
/// it on from its card; the check's occurrences then run nothing.
#[tokio::test]
async fn a_check_the_host_will_not_turn_on_runs_no_occurrence() {
    // Dry runs and evaluations run Python; keep other tests out of its one worker.
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let mut scenario = scenario();
    scenario.seed.watchlist.clear();
    scenario.seed.asset_links.clear();
    validate_seed(&scenario).unwrap();
    for session in &scenario.sessions {
        for turn in &session.turns {
            scheduled::validate(&scenario, session, turn).unwrap();
        }
    }
    let root = tempfile::Builder::new()
        .prefix("eval-check-")
        .tempdir_in("/tmp")
        .unwrap();
    let plan = json!([
        {"calls": [{"name": "trigger_save", "method": "trigger.save", "args": {
            "title": "Alderquay Bearings endings", "prompt": "Report what ended for Alderquay Bearings and since when.",
            "terms": "A fact about Alderquay Bearings ends", "template": "world-change",
            "template_params": {"asset_names": ["Alderquay Bearings"], "kinds": ["ended"]},
            "schedule": {"kind": "interval", "every_minutes": 60},
        }}], "answer": "I proposed a check for endings about Alderquay Bearings."},
    ]);
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
    let [saved, before, after] = kept.0.as_slice() else {
        panic!("three records: {failures} {:?}", kept.0);
    };
    assert_eq!(saved["status"], "completed", "{failures} {saved}");
    let [refused] = saved["approvals"]["refused"].as_array().unwrap().as_slice() else {
        panic!("{}", saved["approvals"]);
    };
    assert_eq!(refused["code"], "conflict", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("successful dry run")),
        "{refused}"
    );
    for occurrence in [before, after] {
        assert_eq!(
            occurrence["error"], "the saved check is not turned on",
            "{occurrence}"
        );
        assert_eq!(occurrence["runs"], json!([]));
        assert_eq!(occurrence["check"]["state"], "proposed", "{occurrence}");
    }
    assert_eq!(before["trigger_dry_run"]["days"], json!([]), "{before}");
    let prompts = fs::read_to_string(root.path().join("prompts.jsonl")).unwrap_or_default();
    assert_eq!(prompts.lines().count(), 1, "only the save turn reached Pi");
}

/// The taint ruling: a Bot turn that reads the feed is marked, and `trigger_save`
/// with a bundled template and no script still saves there, with its dry run.
/// The user accepts its card; the check is then not met before the Q3 report
/// and met after it, and only that occurrence reaches the model.
#[tokio::test]
async fn a_bundled_check_saved_after_reading_the_feed_is_met_on_the_ending_s_day() {
    // Dry runs and evaluations run Python; keep other tests out of its one worker.
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let mut scenario = scenario();
    let world = scenario.world.as_mut().unwrap();
    // Q2 is in the World when the Bot reads the feed; Q3 follows the first occurrence.
    for publication in &mut world.publications {
        if publication.source == "alderquay-q2" {
            publication.after = world::script::After::seed();
            publication.recorded_time = None;
        }
    }
    validate_seed(&scenario).unwrap();
    let root = tempfile::Builder::new()
        .prefix("eval-check-marked-")
        .tempdir_in("/tmp")
        .unwrap();
    let plan = json!([
        {"calls": [
            {"name": "world_changes", "method": "world.changes", "args": {
                "since": "2026-08-01", "about": ["Alderquay Bearings"]}},
            {"name": "trigger_save", "method": "trigger.save", "args": {
                "title": "Alderquay Bearings endings", "prompt": "Report what ended for Alderquay Bearings and since when.",
                "terms": "A fact about Alderquay Bearings ends", "template": "world-change",
                "template_params": {"asset_names": ["Alderquay Bearings"], "kinds": ["ended"]},
                "schedule": {"kind": "interval", "every_minutes": 60},
            }},
        ], "answer": "I proposed a check for endings about Alderquay Bearings."},
        {"answer": "Lila Trent's CFO claim and the Seabright supply ended with the Q3 report."},
    ]);
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
    let [saved, before, after] = kept.0.as_slice() else {
        panic!("three records: {failures} {:?}", kept.0);
    };
    assert_eq!(saved["status"], "completed", "{failures} {saved}");
    assert_eq!(saved["approvals"]["refused"], json!([]), "{saved}");
    assert_eq!(before["runs"], json!([]), "{before}");
    assert_ne!(before["check"]["last"]["outcome"], "met", "{before}");
    assert_eq!(after["check"]["last"]["outcome"], "met", "{after}");
    assert_eq!(after["scheduled_run"]["status"], "succeeded", "{after}");
    let prompts = fs::read_to_string(root.path().join("prompts.jsonl")).unwrap_or_default();
    assert_eq!(
        prompts.lines().count(),
        2,
        "the save turn and the met occurrence"
    );
}

/// R018-08's seed and World: a check saved at the scenario's clock replays the
/// days before it, so the followed asset must be followed by the first
/// publication, and the dry run is met on the guidance update's day.
#[tokio::test]
async fn a_check_saved_at_the_clock_is_met_on_a_seeded_day_before_it() {
    // Dry runs and evaluations run Python; keep other tests out of its one worker.
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let suite: Value = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/release-v018.json"
    ))
    .unwrap();
    let unit = suite["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == "R018-08")
        .unwrap()
        .clone();
    let mut seed = unit["seed"].clone();
    seed["checks"] = json!([{
        "id": "guidance", "title": "Alderquay Bearings guidance",
        "prompt": "Report how Alderquay Bearings changed its guidance.",
        "terms": "Alderquay Bearings changes its guidance", "template": "world-change",
        "template_params": {"asset_names": ["Alderquay Bearings"]},
    }]);
    let scenario: Scenario = serde_json::from_value(json!({
        "id": "CHECK-CLOCK-T", "title": "A check saved at the clock", "language": "en",
        "clock": unit["clock"], "seed": seed, "world": unit["world"],
        "sessions": [{"id": "bot", "role": "probe", "memory": "remember", "turns": [
            {"user": "The check's occurrence at the clock.",
             "scheduled": {"kind": "check", "task": "guidance", "clock": unit["clock"], "dry_run": true}},
        ]}],
    }))
    .unwrap();
    validate_seed(&scenario).unwrap();
    let root = tempfile::Builder::new()
        .prefix("eval-check-clock-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::write(root.path().join("plan.json"), "[]").unwrap();
    let mut host = scripted_host(scripted_pi(
        root.path(),
        include_str!("scripted_plan_pi.py"),
    ));
    let mut kept = Kept::default();
    let _clock = clock::install(scenario.clock.as_deref()).unwrap();
    trial::run_trial(&mut host, &scenario, None, &mut kept)
        .await
        .unwrap();
    let [occurrence] = kept.0.as_slice() else {
        panic!("one record: {:?}", kept.0);
    };
    assert_eq!(occurrence["status"], "completed", "{occurrence}");
    assert_eq!(occurrence["runs"], json!([]), "{occurrence}");
    assert_eq!(
        occurrence["trigger_dry_run"]["days"],
        json!(["2026-11-24"]),
        "{occurrence}"
    );
}

/// A seeded `world-change` check over the scripted ending: not met before the
/// Q3 report is published, so no run and no model call; met after it, when its
/// run starts; and the dry run at that occurrence lists the day it was met.
#[tokio::test]
async fn a_seeded_world_change_check_is_met_on_the_ending_s_day() {
    // Dry runs and evaluations run Python; keep other tests out of its one worker.
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let suite: Value = serde_json::from_str(SUITE).unwrap();
    let reports = suite["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == "CF-002")
        .unwrap()
        .clone();
    let mut world = reports["world"].clone();
    for publication in world["publications"].as_array_mut().unwrap() {
        if publication["source"] == "alderquay-q3" {
            publication["after"] = json!({"session": "bot", "turn": 0});
        }
    }
    let mut seed = reports["seed"].clone();
    seed["watchlist"] = json!([{"name": "Alderquay Bearings"}]);
    seed["asset_links"] = json!([{"asset": "Alderquay Bearings", "company": "Alderquay Bearings"}]);
    seed["checks"] = json!([{
        "id": "endings", "title": "Alderquay Bearings endings",
        "prompt": "Report what ended for Alderquay Bearings and since when.",
        "terms": "A fact about Alderquay Bearings ends", "template": "world-change",
        "template_params": {"asset_names": ["Alderquay Bearings"], "kinds": ["ended"]},
    }]);
    let scenario: Scenario = serde_json::from_value(json!({
        "id": "CHECK-SEED-T", "title": "A seeded world-change check", "language": "en",
        "clock": "2026-10-20T09:00:00+00:00", "seed": seed, "world": world,
        "sessions": [{"id": "bot", "role": "probe", "memory": "remember", "turns": [
            {"user": "The check's occurrence before the Q3 report.",
             "scheduled": {"kind": "check", "task": "endings", "clock": "2026-10-21T09:00:00+00:00"}},
            {"user": "The check's occurrence after the Q3 report.",
             "scheduled": {"kind": "check", "task": "endings", "clock": "2026-10-23T09:00:00+00:00", "dry_run": true}},
        ]}],
    }))
    .unwrap();
    validate_seed(&scenario).unwrap();
    for session in &scenario.sessions {
        for turn in &session.turns {
            scheduled::validate(&scenario, session, turn).unwrap();
        }
    }
    let root = tempfile::Builder::new()
        .prefix("eval-check-seed-")
        .tempdir_in("/tmp")
        .unwrap();
    let plan = json!([{"answer": "Lila Trent's CFO claim and the Seabright supply ended with the Q3 report."}]);
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
    let [before, after] = kept.0.as_slice() else {
        panic!("two records: {:?}", kept.0);
    };
    assert_eq!(before["status"], "completed", "{before}");
    assert!(before["scheduled_run"]["status"].is_null(), "{before}");
    assert_eq!(before["runs"], json!([]));
    assert_ne!(before["check"]["last"]["outcome"], "met", "{before}");
    assert_eq!(after["status"], "completed", "{after}");
    assert_eq!(after["check"]["last"]["outcome"], "met", "{after}");
    assert_eq!(after["scheduled_run"]["status"], "succeeded", "{after}");
    let dry_run = &after["trigger_dry_run"];
    assert!(dry_run["error"].is_null(), "{dry_run}");
    assert!(
        dry_run["days"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-10-22")),
        "{dry_run}"
    );
    assert!(
        dry_run["tools"]
            .as_array()
            .unwrap()
            .contains(&json!("world_changes")),
        "{dry_run}"
    );
    let prompts = fs::read_to_string(root.path().join("prompts.jsonl")).unwrap_or_default();
    assert_eq!(
        prompts.lines().count(),
        1,
        "only the met occurrence reached Pi"
    );
}
