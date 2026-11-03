use super::*;

pub(super) struct Summary {
    pub turns: usize,
    pub failed: usize,
    start: Instant,
}
impl Default for Summary {
    fn default() -> Self {
        Self {
            turns: 0,
            failed: 0,
            start: Instant::now(),
        }
    }
}
impl Drop for Summary {
    fn drop(&mut self) {
        eprintln!(
            "eval summary: turns_run={} completed={} failed={} elapsed_ms={}",
            self.turns,
            self.turns - self.failed,
            self.failed,
            self.start.elapsed().as_millis()
        );
    }
}
impl Summary {
    pub fn observe(&mut self, record: &Value) {
        self.turns += 1;
        self.failed += usize::from(record["status"] != "completed");
        eprintln!(
            "{} trial={} {} turn={} status={} tools={} reads={} commits={}",
            record["scenario_id"].as_str().unwrap_or("unknown"),
            record["trial"],
            record["session_id"].as_str().unwrap_or("unknown"),
            record["turn_index"],
            record["status"].as_str().unwrap_or("unknown"),
            record["tool_calls"].as_array().map_or(0, Vec::len),
            record["memory_reads"].as_array().map_or(0, Vec::len),
            record["memory_commits"].as_array().map_or(0, Vec::len)
        );
    }
}
pub(super) fn records(bytes: &[u8]) -> Result<Vec<Value>> {
    std::str::from_utf8(bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}
pub(super) fn selected(scenario: &Scenario, ids: &[String], tags: &[String]) -> bool {
    (ids.is_empty() || ids.contains(&scenario.id))
        && (tags.is_empty() || tags.iter().any(|tag| scenario.tags.contains(tag)))
}
pub(super) struct ScenarioInput {
    pub sha256: String,
    pub replay: Option<replay::Replay>,
}

impl ScenarioInput {
    pub fn load(scenario: &Scenario, suite_path: &Path, timeout_ms: u64) -> Result<Self> {
        let bytes = if let Some(path) = &scenario.replay {
            relative(path)?;
            Some(fs::read(
                suite_path.parent().unwrap_or(Path::new(".")).join(path),
            )?)
        } else {
            None
        };
        let replay = bytes.as_deref().map(serde_json::from_slice).transpose()?;
        let sha256 = crate::util::sha256(&serde_json::to_vec(&json!({
            "scenario": scenario,
            "replay_sha256": bytes.as_deref().map(crate::util::sha256),
            "turn_timeout_ms": timeout_ms,
        }))?);
        Ok(Self { sha256, replay })
    }
}

pub(super) fn complete(
    records: &[Value],
    suite: &str,
    scenario: &Scenario,
    trial: usize,
    model: &str,
    thinking: &str,
    harness: &Value,
) -> bool {
    // A dirty or unidentified current harness cannot reuse a clean past run.
    if harness["dirty"] != false {
        return false;
    }
    let same_harness = |record: &Value| {
        record["harness"]["dirty"] == false
            && [
                "git_commit",
                "system_prompt_sha256",
                "tool_catalog_sha256",
                "scenario_input_sha256",
            ]
            .iter()
            .all(|&key| {
                harness[key].as_str().is_some_and(|value| !value.is_empty())
                    && record["harness"][key] == harness[key]
            })
    };
    let run_ids: std::collections::BTreeSet<_> = records
        .iter()
        .filter(|r| r["suite"] == suite && r["scenario_id"] == scenario.id && r["trial"] == trial)
        .filter_map(|record| record["run_id"].as_str().filter(|id| !id.is_empty()))
        .collect();
    run_ids.iter().any(|run_id| {
        !scenario.sessions.is_empty()
            && scenario.sessions.iter().all(|session| {
                !session.turns.is_empty()
                    && session.turns.iter().enumerate().all(|(index, turn)| {
                        let mut matching = records.iter().filter(|r| {
                            r["schema"] == "loupist-agent-trace/1"
                                && r["run_id"] == *run_id
                                && r["suite"] == suite
                                && r["scenario_id"] == scenario.id
                                && r["trial"] == trial
                                && r["session_id"] == session.id
                                && r["turn_index"] == index
                        });
                        let Some(r) = matching.next() else {
                            return false;
                        };
                        matching.next().is_none()
                            && r["status"] == "completed"
                            && r["user"] == turn.user
                            && r["session_role"] == session.role
                            && r["memory_mode"] == session.memory
                            && same_harness(r)
                            && r["model"]["thinking"] == thinking
                            && r["observed_model"] == model
                            && format!(
                                "{}/{}",
                                r["model"]["provider"].as_str().unwrap_or(""),
                                r["model"]["model"].as_str().unwrap_or("")
                            ) == model
                    })
            })
    })
}
pub(super) fn provider_rejection(record: &Value) -> bool {
    if record["status"] == "completed" {
        return false;
    }
    let error = record["error"].to_string().to_ascii_lowercase();
    [
        "auth_rejected",
        "missing_auth",
        "rate_limit",
        "quota",
        "unauthorized",
        "authentication",
        "oauth",
        "payment_required",
        "insufficient credit",
        "billing",
        "http 401",
        "http 403",
        "http 429",
    ]
    .iter()
    .any(|word| error.contains(word))
}
pub(super) fn dirty(cwd: &Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ":(top)apps/loupist/agent/",
            ":(top)apps/loupist/src-tauri/src/",
        ])
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Err("cannot determine harness working-tree status".into());
    }
    Ok(!output.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_requires_every_turn_and_exact_execution_identity() {
        let scenario: Scenario = serde_json::from_value(json!({"id":"a","title":"a","tags":["lens"],"sessions":[{"id":"teach","role":"train","memory":"remember","turns":[{"user":"one"},{"user":"two"}]}]})).unwrap();
        let first = json!({"schema":"loupist-agent-trace/1","run_id":"attempt-1","suite":"learning","scenario_id":"a","trial":0,"session_id":"teach","session_role":"train","turn_index":0,"user":"one","status":"completed","memory_mode":"remember","model":{"provider":"p","model":"m","thinking":"medium"},"observed_model":"p/m","harness":{"git_commit":"abc","dirty":false,"system_prompt_sha256":"prompt","tool_catalog_sha256":"catalog","scenario_input_sha256":"inputs"}});
        let mut second = first.clone();
        second["turn_index"] = json!(1);
        second["user"] = json!("two");
        let matches = |rows: &[Value], model, thinking, commit| {
            complete(
                rows,
                "learning",
                &scenario,
                0,
                model,
                thinking,
                &json!({"git_commit":commit,"dirty":false,"system_prompt_sha256":"prompt","tool_catalog_sha256":"catalog","scenario_input_sha256":"inputs"}),
            )
        };
        assert!(!matches(
            std::slice::from_ref(&first),
            "p/m",
            "medium",
            "abc"
        ));
        assert!(matches(
            &[first.clone(), second.clone()],
            "p/m",
            "medium",
            "abc"
        ));
        for (model, thinking, commit) in [
            ("p/other", "medium", "abc"),
            ("p/m", "high", "abc"),
            ("p/m", "medium", "def"),
        ] {
            assert!(!matches(
                &[first.clone(), second.clone()],
                model,
                thinking,
                commit
            ));
        }
        let mut restarted = second.clone();
        restarted["run_id"] = json!("attempt-2");
        assert!(!matches(
            &[first.clone(), restarted.clone()],
            "p/m",
            "medium",
            "abc"
        ));
        let mut restarted_first = first.clone();
        restarted_first["run_id"] = json!("attempt-2");
        assert!(matches(
            &[first.clone(), restarted_first, restarted],
            "p/m",
            "medium",
            "abc"
        ));
        assert!(!matches(
            &[first.clone(), first.clone(), second.clone()],
            "p/m",
            "medium",
            "abc"
        ));
        for (path, value) in [
            ("/harness/dirty", json!(true)),
            ("/harness/dirty", Value::Null),
            ("/memory_mode", json!("off")),
            ("/status", json!("failed")),
            ("/harness/system_prompt_sha256", json!("changed")),
            ("/harness/tool_catalog_sha256", json!("changed")),
            ("/harness/scenario_input_sha256", json!("changed")),
            ("/harness/scenario_input_sha256", Value::Null),
            ("/observed_model", json!("p/other")),
            ("/observed_model", Value::Null),
            ("/user", json!("different request")),
            ("/session_role", json!("probe")),
        ] {
            let mut changed = second.clone();
            *changed.pointer_mut(path).unwrap() = value;
            assert!(
                !matches(&[first.clone(), changed], "p/m", "medium", "abc"),
                "{path}"
            );
        }
        for dirty in [json!(true), Value::Null] {
            let mut harness = first["harness"].clone();
            harness["dirty"] = dirty;
            assert!(!complete(
                &[first.clone(), second.clone()],
                "learning",
                &scenario,
                0,
                "p/m",
                "medium",
                &harness
            ));
        }
        assert!(selected(&scenario, &[], &["lens".into()]));
        assert!(!selected(&scenario, &["other".into()], &["lens".into()]));
        assert!(!selected(&scenario, &[], &["source".into()]));
    }
    #[test]
    fn stops_on_provider_rejection_but_not_tool_failure_or_timeout() {
        for error in [
            "auth_rejected",
            "rate_limit",
            "insufficient_quota",
            "HTTP 401",
        ] {
            assert!(provider_rejection(
                &json!({"status":"failed","error":error})
            ));
        }
        for error in ["timeout", "tool_validation_failed"] {
            assert!(!provider_rejection(
                &json!({"status":"failed","error":error})
            ));
        }
    }
    #[test]
    fn dirtiness_is_scoped_to_agent_and_host_sources() {
        let temp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(temp.path())
                .output()
                .unwrap()
                .status
                .success());
        };
        git(&["init", "-q"]);
        fs::write(temp.path().join("unrelated"), "change").unwrap();
        assert!(!dirty(temp.path()).unwrap());
        for path in [
            "apps/loupist/agent/SYSTEM.md",
            "apps/loupist/src-tauri/src/lib.rs",
        ] {
            let path = temp.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "change").unwrap();
            assert!(dirty(temp.path()).unwrap());
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn scenario_inputs_freeze_replay_and_cover_execution_changes() {
        let root = tempfile::tempdir().unwrap();
        let suite = root.path().join("suite.json");
        let replay = root.path().join("replay.json");
        let initial = json!({"schema":"loupist-connection-replay/1","requests":[]});
        fs::write(&replay, initial.to_string()).unwrap();
        let value = json!({"id":"a","title":"a","replay":"replay.json","workspace_files":{"data.csv":"revenue\n10"},"seed":{"sources":[{"title":"Fact","text":"margin 5","origin":"fixture"}]},"sessions":[{"id":"probe","role":"probe","memory":"remember","turns":[{"user":"Compare the supplied figures","attachments":[{"filename":"note.md","media_type":"text/markdown","text":"margin 5"}]}]}]});
        let scenario: Scenario = serde_json::from_value(value.clone()).unwrap();
        let frozen = ScenarioInput::load(&scenario, &suite, 600_000).unwrap();
        assert_eq!(
            frozen.sha256,
            ScenarioInput::load(&scenario, &suite, 600_000)
                .unwrap()
                .sha256
        );
        for (path, changed) in [
            ("/workspace_files/data.csv", json!("revenue\n20")),
            ("/seed/sources/0/text", json!("margin 6")),
            ("/sessions/0/memory", json!("off")),
            ("/sessions/0/turns/0/attachments/0/text", json!("margin 6")),
            (
                "/sessions/0/turns/0/attachments/0/filename",
                json!("other.md"),
            ),
        ] {
            let mut altered = value.clone();
            *altered.pointer_mut(path).unwrap() = changed;
            let altered: Scenario = serde_json::from_value(altered).unwrap();
            assert_ne!(
                frozen.sha256,
                ScenarioInput::load(&altered, &suite, 600_000)
                    .unwrap()
                    .sha256,
                "{path}"
            );
        }
        let mut connected = scenario;
        connected.connections.push("web".into());
        assert_ne!(
            frozen.sha256,
            ScenarioInput::load(&connected, &suite, 600_000)
                .unwrap()
                .sha256
        );
        connected.connections.clear();
        assert_ne!(
            frozen.sha256,
            ScenarioInput::load(&connected, &suite, 60_000)
                .unwrap()
                .sha256
        );
        let changed = json!({"schema":"loupist-connection-replay/1","requests":[{"provider":"web","request":{},"response":{"status":200,"body":{"value":20}}}]});
        fs::write(&replay, changed.to_string()).unwrap();
        assert_ne!(
            frozen.sha256,
            ScenarioInput::load(&connected, &suite, 600_000)
                .unwrap()
                .sha256
        );
        assert!(frozen.replay.unwrap().requests.is_empty());
    }
}
