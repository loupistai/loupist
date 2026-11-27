//! The keys a suite states, checked against what this runner stages. A key the
//! runner would otherwise ignore fails by name, unless it belongs to the judge
//! or the suite marks it optional for a later runner (`optional_keys`).
use super::{Result, Suite};
use serde_json::Value;

const SUITE: &[&str] = &[
    "schema",
    "suite",
    "version",
    "description",
    "scenarios",
    "budget",
    "setup_fixture",
    "optional_keys",
];
const SCENARIO: &[&str] = &[
    "id",
    "title",
    "language",
    "clock",
    "tags",
    "connections",
    "replay",
    "workspace_files",
    "seed",
    "sessions",
    "world",
];
const SEED: &[&str] = &[
    "instructions",
    "lens",
    "sources",
    "principles",
    "watchlist",
    "journal",
    "shared_packs",
    "lens_pages",
    "programs",
    "learning_allowance",
    "asset_links",
    "checks",
];
const SOURCE: &[&str] = &[
    "id",
    "title",
    "text",
    "origin",
    "company",
    "form",
    "publication_time",
    "language",
];
const SESSION: &[&str] = &["id", "role", "memory", "turns"];
const TURN: &[&str] = &[
    "user",
    "attachments",
    "as_of",
    "briefing",
    "replay_price",
    "scheduled",
    "approvals",
];
const SCHEDULED: &[&str] = &[
    "kind",
    "run_existing",
    "clock",
    "program",
    "task",
    "dry_run",
];
const PUBLICATION: &[&str] = &["source", "after", "recorded_time", "script"];
/// Keys only the judge reads; the agent never receives them.
const JUDGE: &[(&str, &str)] = &[
    ("scenario", "split"),
    ("scenario", "hidden"),
    ("scenario", "replay_needs"),
    ("turn", "expect"),
    ("turn", "discriminator"),
];

/// A suite file as this runner stages it, and the keys it marked optional.
pub(super) fn load(bytes: &[u8]) -> Result<(Suite, Vec<String>)> {
    let mut suite: Value = serde_json::from_slice(bytes)?;
    let optional: Vec<String> = match suite.get("optional_keys") {
        None => Vec::new(),
        Some(keys) => serde_json::from_value(keys.clone())
            .map_err(|_| "optional_keys lists <object>.<key> names")?,
    };
    for key in &optional {
        let known = key.split_once('.').is_some_and(|(object, _)| {
            matches!(
                object,
                "suite"
                    | "scenario"
                    | "seed"
                    | "source"
                    | "session"
                    | "turn"
                    | "scheduled"
                    | "publication"
            )
        });
        if !known {
            return Err(format!("optional_keys names an object the runner reads: {key}").into());
        }
    }
    let mut check = Check {
        optional: &optional,
        ignored: Vec::new(),
    };
    check.visit(&mut suite, "suite", SUITE, "suite")?;
    // `get_mut`, not indexing: indexing a missing key would insert a null.
    let scenarios = suite
        .get_mut("scenarios")
        .and_then(Value::as_array_mut)
        .ok_or("a suite lists its scenarios")?;
    for scenario in scenarios {
        let id = scenario["id"].as_str().unwrap_or("a scenario").to_owned();
        check.visit(scenario, "scenario", SCENARIO, &id)?;
        if let Some(seed) = scenario.get_mut("seed") {
            check.visit(seed, "seed", SEED, &id)?;
            for source in seed
                .get_mut("sources")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                check.visit(source, "source", SOURCE, &id)?;
            }
        }
        for publication in scenario
            .pointer_mut("/world/publications")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            check.visit(publication, "publication", PUBLICATION, &id)?;
        }
        for session in scenario
            .get_mut("sessions")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            check.visit(session, "session", SESSION, &id)?;
            for turn in session
                .get_mut("turns")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                check.visit(turn, "turn", TURN, &id)?;
                if let Some(scheduled) = turn.get_mut("scheduled").filter(|value| value.is_object())
                {
                    check.visit(scheduled, "scheduled", SCHEDULED, &id)?;
                }
            }
        }
    }
    let mut ignored = check.ignored;
    ignored.sort();
    ignored.dedup();
    if !ignored.is_empty() {
        eprintln!(
            "the suite marks keys this runner does not stage yet; they are ignored: {}",
            ignored.join(", ")
        );
    }
    Ok((serde_json::from_value(suite)?, optional))
}

struct Check<'a> {
    optional: &'a [String],
    ignored: Vec<String>,
}

impl Check<'_> {
    /// Refuse an unknown key of `value`, and drop one the suite marks optional.
    fn visit(
        &mut self,
        value: &mut Value,
        object: &str,
        keys: &[&str],
        scenario: &str,
    ) -> Result<()> {
        let Value::Object(fields) = value else {
            return Ok(());
        };
        let unknown: Vec<String> = fields
            .keys()
            .filter(|key| !keys.contains(&key.as_str()) && !JUDGE.contains(&(object, key.as_str())))
            .cloned()
            .collect();
        for key in unknown {
            let named = format!("{object}.{key}");
            if !self.optional.contains(&named) {
                return Err(format!(
                    "{scenario}: {object} has key {key}, which this runner does not stage; \
                     a suite lists it in optional_keys to run without it"
                )
                .into());
            }
            fields.remove(&key);
            self.ignored.push(named);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite(seed: Value, optional: Option<Value>) -> Vec<u8> {
        let mut value = serde_json::json!({
            "schema": "loupist-agent-suite/1", "suite": "keys", "version": 2,
            "scenarios": [{"id": "K-1", "title": "keys", "seed": seed, "split": "regression",
                "sessions": [{"id": "s", "role": "probe", "memory": "off",
                    "turns": [{"user": "hello", "expect": {}, "discriminator": "a judge-only note"}]}]}],
        });
        if let Some(optional) = optional {
            value["optional_keys"] = optional;
        }
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn an_unknown_key_fails_by_name_unless_the_suite_marks_it_optional() {
        let corrections = serde_json::json!({"corrections": [{"entity": "Lindenhof", "action": "add_alias", "alias": "LiMa"}]});
        let error = load(&suite(corrections.clone(), None))
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("K-1") && error.contains("seed has key corrections"),
            "{error}"
        );
        let (loaded, optional) = load(&suite(
            corrections,
            Some(serde_json::json!(["seed.corrections"])),
        ))
        .unwrap();
        assert_eq!(optional, ["seed.corrections"]);
        assert_eq!(loaded.scenarios.len(), 1);
        // Judge-owned keys need no marker.
        load(&suite(serde_json::json!({}), None)).unwrap();
        assert!(load(&suite(
            serde_json::json!({}),
            Some(serde_json::json!(["planet.key"]))
        ))
        .is_err());
    }
}
