use super::*;
use chrono::NaiveDate;

use crate::{
    broker::{tool_class::PythonExposure, ProviderFailureKind},
    python_run::{
        self,
        ledger::{self, LedgerEntry, ReadCall, ReadSummary, Response, RunLedger, RunParams},
    },
    scheduled_tasks::trigger::{self, CheckOutput},
    store::scripts::{self, ScriptManifest, ScriptSummary, ScriptTest},
    util::credential_text::contains_credential_json,
};

tokio::task_local! {
    static PYTHON_READ: bool;
    static HOST_PYTHON_READ: ();
}

pub(super) fn nested() -> bool {
    PYTHON_READ.try_with(|nested| *nested).unwrap_or(false)
}

pub(super) fn host_python_read() -> bool {
    HOST_PYTHON_READ.try_with(|()| ()).is_ok()
}

fn error_response(error: BrokerError) -> Response {
    match error {
        BrokerError::WithNext { source, .. } | BrokerError::WithReason { source, .. } => {
            error_response(*source)
        }
        BrokerError::UntrustedTextPaused | BrokerError::OutboundPaused => {
            Response::refused("untrusted_text_paused")
        }
        BrokerError::CustomApprovalRequired { .. }
        | BrokerError::CustomApprovalRefused { .. }
        | BrokerError::ProviderApprovalRequired { .. } => Response::refused("needs_approval"),
        BrokerError::KeychainPermissionRequired
        | BrokerError::KeychainUnavailable
        | BrokerError::KeychainBusy => Response::refused("needs_permission"),
        BrokerError::MethodDenied | BrokerError::MemoryDisabled => {
            Response::refused("needs_connection")
        }
        BrokerError::BudgetExceeded { .. } | BrokerError::ResultTooLarge { .. } => {
            Response::refused("limit")
        }
        BrokerError::EmptyResult => Response::unavailable("no_data"),
        BrokerError::InvalidArguments { ref expected, .. }
            if expected.contains("cursor_axis_mismatch") =>
        {
            Response::refused("cursor_axis_mismatch")
        }
        BrokerError::ProviderFailure {
            kind: ProviderFailureKind::Timeout,
            ..
        } => Response::unavailable("timeout"),
        BrokerError::ProviderFailure {
            kind: ProviderFailureKind::RateLimited,
            retry_after,
            ..
        } => Response::rate_limited(retry_after),
        _ => Response::unavailable("provider_error"),
    }
}

fn prefetchable(call: &ReadCall) -> bool {
    let values = Value::Object(call.arguments.clone());
    // Do not persist a custom target or a request containing secret-like text.
    !call.tool.starts_with("mcp__custom-")
        && !values
            .get("server_id")
            .and_then(Value::as_str)
            .is_some_and(crate::custom_connections::service::valid_id)
        && !any_credential_text(&values)
}

/// One sealed check. The host passes the pinned code, the carried state and
/// the tools the dry run was allowed to call.
pub(in crate::commands) struct CheckRun<'a> {
    pub code: &'a str,
    pub after_outside_text: bool,
    pub use_memory: bool,
    pub args: serde_json::Map<String, Value>,
    pub state: Value,
    pub now: String,
    pub as_of: Option<String>,
    pub stale_after_days: u32,
    pub allowed: &'a [crate::scheduled_tasks::types::TriggerToolGrant],
    pub discover_providers: bool,
    pub replay: Option<&'a [Value]>,
    pub max_rounds: u32,
    pub max_calls: usize,
}

pub(super) const CHECK_READ_TOOLS: &[&str] = &[
    "followed",
    "price",
    "world_changes",
    "krx_daily_quote",
    "dart_filing_search",
    "mcp__openbb__equity_price_historical",
    "mcp__openbb__equity_fundamental_filings",
    "mcp__openbb__news_company",
    "mcp__openbb__news_world",
];

fn check_memory_tool(call: &ReadCall) -> bool {
    matches!(call.tool.as_str(), "followed" | "price" | "world_changes")
}

/// A check keeps its cursor in `state`. Under a replay date the feed is the
/// published axis, so a cursor made on the live recorded axis is refused.
fn prepare_check_read(policy: &ToolPolicy, call: &ReadCall) -> Result<ReadCall, Response> {
    if call.tool != "world_changes" {
        return Ok(call.clone());
    }
    let mut call = call.clone();
    if call.arguments.get("since").and_then(Value::as_str) == Some("previous_run") {
        return Err(Response::refused("since_previous_run"));
    }
    if policy.as_of.is_some() {
        call.arguments.insert("axis".into(), json!("published"));
    }
    Ok(call)
}

fn check_provider_call(call: &ReadCall, check: &CheckRun<'_>) -> Option<ReadCall> {
    let mut call = call.clone();
    if check.discover_providers || check_memory_tool(&call) {
        return Some(call);
    }
    if call.tool.starts_with("mcp__openbb__") {
        let mut providers = check
            .allowed
            .iter()
            .filter(|grant| grant.name == call.tool)
            .filter_map(|grant| grant.provider.as_deref());
        let provider = match call.arguments.get("provider") {
            Some(value) => {
                let requested = value.as_str()?;
                providers.find(|provider| *provider == requested)?
            }
            None => {
                let first = providers.next()?;
                if providers.next().is_some() {
                    return None;
                }
                first
            }
        };
        call.arguments.insert("provider".into(), json!(provider));
    } else if !trigger::admit_grant(
        check.allowed,
        &observed_check_grant(&call, &Value::Null),
        false,
    ) {
        return None;
    }
    Some(call)
}

fn observed_check_grant(
    call: &ReadCall,
    value: &Value,
) -> crate::scheduled_tasks::types::TriggerToolGrant {
    use crate::finance_data::{KRX_SOURCE_ID, OPENDART_SOURCE_ID};
    let provider = match call.tool.as_str() {
        "krx_daily_quote" => Some(KRX_SOURCE_ID),
        "dart_filing_search" => Some(OPENDART_SOURCE_ID),
        name if name.starts_with("mcp__openbb__") => value
            .pointer("/structuredContent/provider")
            .or_else(|| value.get("provider"))
            .or_else(|| call.arguments.get("provider"))
            .and_then(Value::as_str),
        _ => value.get("provider").and_then(Value::as_str),
    };
    crate::scheduled_tasks::types::TriggerToolGrant {
        name: call.tool.clone(),
        provider: provider.map(str::to_owned),
    }
}

fn guard_legacy_verdict(
    id: &str,
    run: &RunLedger,
    output: &mut CheckOutput,
    now: chrono::DateTime<chrono::FixedOffset>,
    days: u32,
) {
    use crate::scheduled_tasks::types::TriggerOutcome;

    if output.outcome != TriggerOutcome::Met {
        return;
    }
    let (events, field) = match id {
        "price-band" => ("moved", "date"),
        "level-cross" => ("crossed", "date"),
        "volume-spike" => ("spiked", "date"),
        "new-item" => (
            "new",
            run.args
                .get("date_field")
                .and_then(Value::as_str)
                .unwrap_or("date"),
        ),
        _ => return,
    };
    let parse_day = |value: &Value| {
        let text = value.as_str()?;
        let day = text.get(..10).unwrap_or(text);
        day.parse::<NaiveDate>()
            .or_else(|_| NaiveDate::parse_from_str(day, "%Y%m%d"))
            .ok()
    };
    let followed = run.entries.iter().find_map(|entry| match &entry.response {
        Response::Ok { value } if entry.call.tool == "followed" => value.as_array(),
        _ => None,
    });
    let dated = output.evidence[events].as_array().and_then(|events| {
        if events.is_empty() {
            return None;
        }
        events
            .iter()
            .map(|event| {
                if id == "new-item" {
                    let day = parse_day(&event[field])?;
                    return (!trigger::outside_data_age(Some(&day.to_string()), now, days))
                        .then_some(day);
                }
                let asset_id = event["id"].as_str()?;
                let reference = followed?.iter().find(|asset| asset["id"] == asset_id)?;
                let exchange = reference
                    .pointer("/reference/exchange")
                    .and_then(Value::as_str);
                let read_market = run.entries.iter().find_map(|entry| match &entry.response {
                    Response::Ok { value }
                        if entry.call.tool == "price"
                            && entry.call.arguments["asset_id"] == asset_id =>
                    {
                        value["market"].as_str()
                    }
                    _ => None,
                });
                let market = read_market
                    .or_else(|| {
                        (run.args.get("tool").and_then(Value::as_str) == Some("krx_daily_quote"))
                            .then_some("KRX")
                    })
                    .or(exchange)
                    .unwrap_or("");
                let key = market.trim().to_uppercase();
                let [latest, previous, prior] =
                    run.bar_dates.get(&key).or_else(|| run.bar_dates.get(""))?;
                if id == "volume-spike" {
                    let day = parse_day(&event[field])?;
                    return (&day.to_string() == latest || &day.to_string() == previous)
                        .then_some(day);
                }
                let to = parse_day(event.pointer("/to/date")?)?;
                let from = parse_day(event.pointer("/from/date")?)?;
                let pair = (&from.to_string(), &to.to_string());
                (pair == (prior, previous) || (pair == (previous, latest) && from != to))
                    .then_some(to)
            })
            .collect::<Option<Vec<_>>>()
    });
    if let Some(day) = dated.and_then(|dates| dates.into_iter().max()) {
        output.as_of = Some(day.to_string());
        output.per_item_dates = true;
    } else {
        output.outcome = TriggerOutcome::Stale;
        output.reading.clear();
        output.as_of = None;
    }
}

impl AppToolExecutor {
    pub(in crate::commands) fn for_host_with_connector_authority(
        state: &AppState,
        loupist_id: &str,
        authority: crate::commands::ChatConnectorAuthority,
    ) -> Self {
        let mut executor = Self::for_host(
            state,
            loupist_id,
            authority.capability_ids.into_iter().collect(),
        );
        Arc::get_mut(&mut executor.capture)
            .expect("new capture")
            .saved_keys_needing_permission = authority.saved_keys_needing_permission;
        executor
    }

    pub(super) async fn python_run(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let request = RunParams::parse(params)?;
        let roots = self
            .capture
            .native
            .roots()
            .ok_or_else(|| BrokerError::Execution("Python execution is unavailable".into()))?;
        self.run_with_ledger(policy, request, roots, delivery_id, true)
            .await
    }

    pub(in crate::commands) async fn test_python_script(
        &self,
        script: &str,
        args: serde_json::Map<String, Value>,
    ) -> Result<Value, BrokerError> {
        let (_workspace, roots) = ledger::empty_roots()?;
        let policy = ToolPolicy {
            loupist_id: self.loupist_id.clone(),
            session_id: String::new(),
            use_memory: true,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: None,
            display_currency: None,
        };
        let request = RunParams::parse(json!({"script":script, "args":args}))?;
        HOST_PYTHON_READ
            .scope(
                (),
                self.run_with_ledger(&policy, request, &roots, "", false),
            )
            .await
    }

    /// A scheduled check: sealed snapshot, approved tools only, at most three rounds.
    pub(in crate::commands) async fn run_check(
        &self,
        check: CheckRun<'_>,
    ) -> Result<CheckOutput, BrokerError> {
        if check.after_outside_text {
            self.capture.turn_taint.mark_untrusted_text();
        }
        if self.check_paused(check.code) {
            return Err(BrokerError::UntrustedTextPaused);
        }
        // A provider read's taint and inventory stay inside this sealed check.
        // Proposal and explanation deliveries mark their own turn before showing the result.
        let mut reads = Self::for_host(&self.state, &self.loupist_id, self.capability_ids.clone());
        let capture = Arc::get_mut(&mut reads.capture).expect("new check capture");
        capture.saved_keys_needing_permission = self.capture.saved_keys_needing_permission.clone();
        capture.search_cancel = self.capture.search_cancel.clone();
        let output = Box::pin(self.execute_check(check, &reads)).await;
        reads.host_shutdown().await;
        output
    }

    async fn execute_check(
        &self,
        check: CheckRun<'_>,
        reader: &Self,
    ) -> Result<CheckOutput, BrokerError> {
        use crate::scheduled_tasks::types::{TRIGGER_QUEUE_WAIT, TRIGGER_TIMEOUT_SECONDS};
        let replaying = check.replay.is_some();
        let replay = check.replay.unwrap_or_default();
        if replay.iter().any(contains_credential_json) {
            return Err(BrokerError::UntrustedTextPaused);
        }
        let policy = ToolPolicy {
            loupist_id: self.loupist_id.clone(),
            session_id: format!("check-{}", self.loupist_id),
            use_memory: check.use_memory,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: check.as_of.clone(),
            display_currency: None,
        };
        let approved = |call: &ReadCall| {
            CHECK_READ_TOOLS.contains(&call.tool.as_str())
                && trigger::admit_call(check.allowed, &call.tool).is_ok()
        };
        let mut entries = Vec::new();
        for value in replay {
            let entry: LedgerEntry =
                serde_json::from_value(value.clone()).map_err(|_| BrokerError::Malformed)?;
            let permitted = approved(&entry.call)
                && (!check_memory_tool(&entry.call) || policy.use_memory)
                && policy.as_of.is_none();
            let same_grant = match &entry.response {
                Response::Ok { value } => {
                    let grant = observed_check_grant(&entry.call, value);
                    trigger::admit_grant(check.allowed, &grant, check.discover_providers)
                }
                _ => true,
            };
            if !permitted || !same_grant {
                return Err(BrokerError::UntrustedTextPaused);
            }
            entries.push(entry);
        }
        let now =
            chrono::DateTime::parse_from_rfc3339(&check.now).map_err(|_| BrokerError::Malformed)?;
        let cutoff = check.as_of.as_deref().and_then(|date| date.parse().ok());
        let mut run = RunLedger {
            // The sealed worker seeds Python's random module from this ID.
            id: crate::util::sha256(check.now.as_bytes()),
            now: check.now.clone(),
            args: check.args.clone(),
            state: check.state.clone(),
            inputs: Default::default(),
            entries,
            final_round: false,
            check_stale_days: Some(check.stale_after_days),
            bar_dates: crate::market_calendar::check_bar_dates(now, cutoff)
                .into_iter()
                .map(|(key, dates)| (key.to_owned(), dates.map(|day| day.to_string())))
                .collect(),
        };
        let mut reads = 0u32;
        let mut tools = Vec::new();
        let rounds = check.max_rounds.clamp(1, 3);
        for round in 1..=rounds {
            // Yield on an unrecorded replay read so even a script that catches
            // refusals cannot turn incomplete inputs into a comparable result.
            run.final_round = !replaying && (round == rounds || reads as usize >= check.max_calls);
            let result = match python_run::execute_sealed(
                &self.state.artifacts.python_cache_root,
                check.code,
                TRIGGER_TIMEOUT_SECONDS,
                self.capture.search_cancel.clone(),
                &run,
                TRIGGER_QUEUE_WAIT,
            )
            .await
            {
                Ok(result) => result,
                Err(error) if reads != 0 => {
                    return Ok(trigger::errored(&error.to_string(), Vec::new(), reads));
                }
                Err(error) => return Err(error),
            };
            if result["untrusted_input"] == true {
                self.capture.turn_taint.mark_untrusted_text();
            }
            if self.check_paused(check.code) {
                if replaying {
                    return Err(BrokerError::UntrustedTextPaused);
                }
                return Ok(trigger::errored(
                    &BrokerError::UntrustedTextPaused.to_string(),
                    Vec::new(),
                    reads,
                ));
            }
            let Some(requests) = result.get("_python_pending").cloned() else {
                if replaying
                    && result["status"] != "completed"
                    && result.get("check_failure").is_none()
                {
                    return Err(BrokerError::Execution(
                        "The recorded check could not be replayed".into(),
                    ));
                }
                let mut output = self.screen_check_output(check.code, &run, &result, reads);
                output.tools = tools;
                output.per_item_dates =
                    crate::commands::triggers::template_has_item_dates(check.code);
                if !output.per_item_dates {
                    if let Some(id) = crate::commands::triggers::legacy_template(check.code) {
                        guard_legacy_verdict(id, &run, &mut output, now, check.stale_after_days);
                    }
                }
                if output.outcome.updates_state()
                    && !output.per_item_dates
                    && trigger::outside_data_age(
                        output.as_of.as_deref(),
                        now,
                        check.stale_after_days,
                    )
                {
                    output.outcome = crate::scheduled_tasks::types::TriggerOutcome::Stale;
                }
                return Ok(output);
            };
            if replaying {
                return Err(BrokerError::Execution(
                    "Recorded inputs do not cover this replay".into(),
                ));
            }
            let calls: Vec<ReadCall> =
                serde_json::from_value(requests).map_err(|_| BrokerError::Malformed)?;
            if calls.is_empty() {
                break;
            }
            for call in calls {
                if reads as usize >= check.max_calls {
                    run.entries.push(LedgerEntry {
                        call,
                        response: Response::refused("limit"),
                    });
                    continue;
                }
                reads += 1;
                let response = if !approved(&call) {
                    Response::refused("not_approved")
                } else if check_memory_tool(&call) && !policy.use_memory {
                    Response::refused("needs_permission")
                } else if let Some(provider_call) = check_provider_call(&call, &check) {
                    let response = if call.tool == "price" && policy.as_of.is_none() {
                        match HOST_PYTHON_READ
                            .scope(
                                (),
                                reader.refresh_check_price(
                                    &policy,
                                    Value::Object(provider_call.arguments.clone()),
                                    &check,
                                    now,
                                ),
                            )
                            .await
                        {
                            Ok(value) => Response::Ok { value },
                            Err(response) => response,
                        }
                    } else {
                        reader.sealed_read(&policy, &provider_call).await
                    };
                    if let Response::Ok { value } = &response {
                        let grant = observed_check_grant(&provider_call, value);
                        if !trigger::admit_grant(check.allowed, &grant, check.discover_providers) {
                            Response::refused("not_approved")
                        } else {
                            if !tools.contains(&grant) {
                                tools.push(grant);
                            }
                            response
                        }
                    } else {
                        response
                    }
                } else {
                    Response::refused("not_approved")
                };
                run.entries.push(LedgerEntry { call, response });
            }
        }
        Ok(self.screen_check_output(check.code, &run, &Value::Null, reads))
    }

    /// Whether this turn pauses `code`: once third-party text reached it, check
    /// code the model wrote does not run. A bundled template's exact code is not
    /// the model's, so it still runs (a dry run, a test), and its readings
    /// return delimited.
    fn check_paused(&self, code: &str) -> bool {
        self.capture.turn_taint.has_untrusted_content()
            && !crate::commands::triggers::is_bundled_template_code(code)
    }

    fn screen_check_output(
        &self,
        code: &str,
        run: &RunLedger,
        result: &Value,
        reads: u32,
    ) -> CheckOutput {
        let mut ledger: Vec<_> = run
            .entries
            .iter()
            .map(|entry| serde_json::to_value(entry).unwrap_or(Value::Null))
            .collect();
        if self.capture.turn_taint.memory_blocked()
            || contains_credential_json(&json!({"code": code, "ledger": run, "result": result}))
            || serde_json::to_vec(&ledger).is_ok_and(|bytes| bytes.len() > 1024 * 1024)
        {
            ledger.clear();
        }
        let output = trigger::output_from_worker(result, ledger, reads);
        if contains_credential_json(&json!({
            "reading": output.reading, "as_of": output.as_of, "evidence": output.evidence,
            "state": output.state, "error": output.error, "refused_tool": output.refused_tool,
        })) {
            return trigger::errored("sensitive_text", Vec::new(), reads);
        }
        output
    }

    async fn run_with_ledger(
        &self,
        policy: &ToolPolicy,
        request: RunParams,
        roots: &crate::agent_harness::native_policy::NativeRoots,
        delivery_id: &str,
        writeback: bool,
    ) -> Result<Value, BrokerError> {
        let script = request
            .script
            .as_deref()
            .map(|reference| {
                let (name, version) = scripts::script_reference(reference).map_err(store_error)?;
                self.state
                    .store
                    .script_get(&self.loupist_id, name, version)
                    .map_err(store_error)
            })
            .transpose()?;
        if script
            .as_ref()
            .is_some_and(|script| script.summary.metadata.after_outside_text)
        {
            self.capture.turn_taint.mark_untrusted_text();
        }
        let mut args = script
            .as_ref()
            .map(|script| {
                script
                    .summary
                    .metadata
                    .args
                    .iter()
                    .map(|(name, spec)| (name.clone(), spec["default"].clone()))
                    .collect::<serde_json::Map<_, _>>()
            })
            .unwrap_or_default();
        args.extend(request.args);
        let code = script
            .as_ref()
            .map(|saved| saved.code.as_str())
            .or(request.code.as_deref())
            .ok_or(BrokerError::Malformed)?;
        let inputs = ledger::named_inputs(request.inputs)?;
        let mut initial = inputs.values().cloned().collect::<Vec<_>>();
        let mut canonical_args = Value::Object(args.clone());
        canonical_args.sort_all_objects();
        let args_digest = sha256(canonical_args.to_string().as_bytes());
        if let Some(manifest) = script
            .as_ref()
            .and_then(|script| script.manifest.as_ref())
            .filter(|manifest| manifest.args_digest == args_digest)
        {
            initial.extend(manifest.calls.iter().cloned());
        }
        let id = new_id("python");
        let mut run = RunLedger {
            id: id.clone(),
            now: policy
                .as_of
                .clone()
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            args,
            state: serde_json::json!({}),
            inputs,
            entries: Vec::new(),
            final_round: false,
            bar_dates: Default::default(),
            check_stale_days: None,
        };
        let mut reads = Vec::new();
        let mut upstream = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut calls = initial;
        let mut ledger_bytes = serde_json::to_vec(&run)
            .map_err(|_| BrokerError::Malformed)?
            .len();
        for round in 1..=ledger::MAX_ROUNDS {
            for call in std::mem::take(&mut calls) {
                if self.capture.search_cancel.is_cancelled() {
                    return Err(BrokerError::Execution(
                        "Python execution was cancelled".into(),
                    ));
                }
                if run.entries.len() >= ledger::MAX_CALLS || !seen.insert(call.key()) {
                    continue;
                }
                let (mut response, mut info, result_ref) =
                    self.python_read(policy, &call, delivery_id).await;
                let bytes = serde_json::to_vec(&response)
                    .map_err(|_| BrokerError::Malformed)?
                    .len()
                    .saturating_add(call.key().len() + 100);
                if ledger_bytes.saturating_add(bytes) > ledger::MAX_LEDGER_BYTES - 1024 * 1024 {
                    response = Response::refused("limit");
                    info.status = "limit".into();
                } else {
                    ledger_bytes += bytes;
                    upstream.extend(result_ref);
                }
                if let Some(run_id) = self
                    .capture
                    .scheduled_run_id()
                    .filter(|_| info.status != "ok")
                {
                    self.state
                        .store
                        .scheduled_run_skip_tool(
                            &self.loupist_id,
                            run_id,
                            "run_python",
                            &info.status,
                        )
                        .map_err(store_error)?;
                }
                self.capture
                    .tool_trace
                    .record_python_read(current_tool_call_id().as_deref(), &info);
                reads.push(info);
                run.entries.push(LedgerEntry { call, response });
            }

            run.final_round = round == ledger::MAX_ROUNDS || run.entries.len() >= ledger::MAX_CALLS;
            // Script provenance and broker reads raise the same turn tier as tagged files.
            let round_writeback = if writeback {
                python_run::RoundWriteback::Allowed {
                    untrusted_inputs: self.capture.turn_taint.has_untrusted_content(),
                }
            } else {
                python_run::RoundWriteback::Disabled
            };
            let mut result = python_run::execute_round(
                roots,
                &self.state.artifacts.python_cache_root,
                code,
                request.timeout_seconds,
                self.capture.search_cancel.clone(),
                &run,
                round_writeback,
            )
            .await?;
            if result["untrusted_input"] == true {
                self.capture.turn_taint.mark_untrusted_text();
            }
            if let Some(requests) = result
                .as_object_mut()
                .and_then(|result| result.remove("_python_pending"))
            {
                if run.final_round {
                    return Err(BrokerError::Execution(
                        "Python reached its read limit".into(),
                    ));
                }
                calls = serde_json::from_value(requests).map_err(|_| BrokerError::Malformed)?;
                if calls.iter().all(|call| seen.contains(&call.key())) {
                    return Err(BrokerError::Execution(
                        "Python did not advance its read ledger".into(),
                    ));
                }
                continue;
            }
            if let Some(script) = &script {
                let calls: Vec<_> = run
                    .entries
                    .iter()
                    .filter(|entry| {
                        matches!(entry.response, Response::Ok { .. }) && prefetchable(&entry.call)
                    })
                    .map(|entry| entry.call.clone())
                    .collect();
                let manifest = (!calls.is_empty()).then_some(ScriptManifest { args_digest, calls });
                self.record_script_run(&script.summary, &reads, &result, round, manifest);
            }
            if self.capture.turn_taint.has_untrusted_content() {
                result["untrusted_input"] = json!(true);
            }
            result["reads"] = json!(reads
                .iter()
                .map(|read| json!({
                    "tool": read.tool, "status": read.status, "citation_url": read.citation_url,
                    "provider":read.provider, "as_of":read.as_of
                }))
                .collect::<Vec<_>>());
            result["rounds"] = json!(round);
            result[RESOLVED_UPSTREAM_FIELD] = json!(upstream);
            if let Some(call_id) = current_tool_call_id() {
                let replay = if self.capture.turn_taint.memory_blocked() {
                    None
                } else {
                    let record = json!({
                        "bootstrap":ledger::bootstrap_digest(),
                        "code":code, "timeout":request.timeout_seconds, "ledger":run,
                        "stdout":result["stdout"], "stderr":result["stderr"], "status":result["status"],
                        "reads":reads, "rounds":round,
                        "script":script.as_ref().map(|saved| format!("{}@{}", saved.summary.name, saved.summary.version))
                    });
                    if contains_credential_json(&record) {
                        None
                    } else {
                        self.state.store.dataset_put(
                            &self.loupist_id,
                            &id,
                            &record,
                            json!({
                                "origin":"python_replay", "chat_id":policy.session_id, "trust":{"kind":"third_party"}
                            }),
                        ).ok()
                    }
                };
                self.capture.tool_trace.record_python_run(
                    &call_id,
                    reads.len() as u32,
                    round,
                    replay,
                );
            }
            return Ok(result);
        }
        Err(BrokerError::Execution(
            "Python reached its round limit".into(),
        ))
    }

    fn record_script_run(
        &self,
        script: &ScriptSummary,
        reads: &[ReadSummary],
        result: &Value,
        rounds: u32,
        manifest: Option<ScriptManifest>,
    ) {
        if self.capture.turn_taint.memory_blocked() {
            return;
        }
        let _ = self.state.store.script_record_run(
            &self.loupist_id,
            script,
            reads.to_owned(),
            ScriptTest {
                at_ms: crate::commands::now_ms(),
                rounds,
                status: result["status"].as_str().unwrap_or("failed").to_owned(),
            },
            manifest,
        );
    }

    async fn python_read(
        &self,
        policy: &ToolPolicy,
        call: &ReadCall,
        delivery_id: &str,
    ) -> (Response, ReadSummary, Option<String>) {
        let mut info = ReadSummary {
            tool: call.tool.clone(),
            status: String::new(),
            citation_url: None,
            provider: None,
            as_of: policy.as_of.clone(),
            dataset: None,
        };
        let mut result_ref = None;
        let response = match PYTHON_READ
            .scope(true, self.dispatch_python_read(policy, call, delivery_id))
            .await
        {
            Ok(value) => {
                let payload = value.get("result").unwrap_or(&value);
                let text = |keys: &[&str], limit: usize| {
                    keys.iter().find_map(|key| {
                        payload
                            .get(*key)
                            .or_else(|| value.get(*key))
                            .and_then(Value::as_str)
                            .filter(|text| text.len() <= limit)
                            .map(str::to_owned)
                    })
                };
                // The guest's selected value may have its own result_ref field.
                // Only a host receipt staged in this delivery is an upstream read.
                if let Some(reference) = value
                    .get("result_ref")
                    .or_else(|| payload.get("result_ref"))
                    .and_then(Value::as_str)
                {
                    let pending = self.capture.pending_deliveries.lock().await;
                    if pending
                        .get(delivery_id)
                        .is_some_and(|entry| entry.run_results.contains_key(reference))
                    {
                        result_ref = Some(reference.to_owned());
                    }
                }
                info.citation_url = result_ref
                    .as_deref()
                    .map(super::evidence::citation_url)
                    .filter(|citation| {
                        value["citation_url"] == citation.as_str()
                            || payload["citation_url"] == citation.as_str()
                    })
                    .or_else(|| {
                        [
                            "/sourceUrl",
                            "/source_url",
                            "/canonicalUrl",
                            "/final_url",
                            "/url",
                            "/provenance/url",
                            "/metadata/url",
                            "/source/url",
                        ]
                        .into_iter()
                        .find_map(|pointer| {
                            payload
                                .pointer(pointer)
                                .and_then(Value::as_str)
                                .and_then(super::evidence::safe_url)
                        })
                    });
                info.provider = text(&["provider", "source_id"], 128);
                info.as_of = text(&["date", "as_of", "asOf", "fetched_at"], 64).or(info.as_of);
                info.dataset = value
                    .get("dataset_ref")
                    .or_else(|| payload.get("dataset_ref"))
                    .or_else(|| value.get("dataset"))
                    .and_then(|reference| serde_json::from_value(reference.clone()).ok());
                let value = if call.tool == "dataset" {
                    value.get("_python_value").cloned().unwrap_or(Value::Null)
                } else if call.tool.starts_with("mcp__") {
                    value.get("result").cloned().unwrap_or(value)
                } else {
                    value
                };
                Response::Ok { value }
            }
            Err(response) => response,
        };
        info.status = response.status().to_owned();
        (response, info, result_ref)
    }

    async fn dispatch_python_read(
        &self,
        policy: &ToolPolicy,
        call: &ReadCall,
        delivery_id: &str,
    ) -> Result<Value, Response> {
        let refused = Response::refused;
        let mut arguments = Value::Object(call.arguments.clone());
        let route = call
            .tool
            .strip_prefix("mcp__")
            .and_then(|name| name.split_once("__"));
        if let Some((server, tool)) = route {
            arguments = json!({"server_id":server,"tool_name":tool,"arguments":arguments});
        }
        let method = match call.tool.as_str() {
            _ if route.is_some() => ToolMethod::McpCall,
            "dataset" => ToolMethod::DatasetDescribe,
            "followed" | "price" => ToolMethod::AssetPageRead,
            name => ToolMethod::from_tool_name(name).ok_or_else(|| refused("not_exposed"))?,
        };
        match method.safety().python {
            PythonExposure::NoPython => return Err(refused("not_exposed")),
            PythonExposure::MemoryRead if !policy.use_memory => {
                return Err(refused("needs_permission"))
            }
            PythonExposure::RouteChecked => {
                let server = arguments["server_id"]
                    .as_str()
                    .ok_or_else(|| refused("not_exposed"))?;
                let name = arguments["tool_name"]
                    .as_str()
                    .ok_or_else(|| refused("not_exposed"))?;
                if crate::custom_connections::service::valid_id(server) {
                    if self.capture.turn_taint.has_untrusted_content() {
                        return Err(refused("untrusted_text_paused"));
                    }
                    if self.capture.unattended_run_id.is_some() || host_python_read() {
                        return Err(refused("not_approved"));
                    }
                } else if server != "openbb" {
                    return Err(refused("not_exposed"));
                } else {
                    // This is the host's inventory initialization, never a guest
                    // request forwarded to a control tool.
                    PYTHON_READ
                        .scope(false, self.host_connect(policy, server, &[name]))
                        .await
                        .map_err(error_response)?;
                }
            }
            _ => {}
        }
        crate::broker::enforce_policy(policy, method).map_err(error_response)?;
        if call.tool == "followed" {
            if !arguments.as_object().is_some_and(serde_json::Map::is_empty) {
                return Err(refused("not_exposed"));
            }
            let rows = self
                .state
                .store
                .assets_list(&self.loupist_id, true)
                .map_err(store_error)
                .map_err(error_response)?;
            let cutoff = effective_as_of(policy, &Value::Null).map_err(error_response)?;
            return Ok(json!(rows
                .into_iter()
                .filter(|asset| cutoff.is_none_or(|day| followed_at_cutoff(asset, day)))
                .map(|row| json!({"id":row.asset_id, "name":row.name, "reference":row.reference}))
                .collect::<Vec<_>>()));
        }
        if call.tool == "price" {
            return self
                .python_saved_price(policy, arguments)
                .map_err(error_response);
        }
        // Keep nested receipts in the original tool's delivery.
        let mut value = self
            .dispatch_staged(policy, method, arguments.clone(), delivery_id)
            .await
            .map_err(error_response)?;
        if call.tool == "dataset" {
            let id = arguments["dataset_id"]
                .as_str()
                .ok_or_else(|| refused("not_exposed"))?;
            let revision = arguments["revision"]
                .as_u64()
                .and_then(|version| u32::try_from(version).ok())
                .ok_or_else(|| refused("not_exposed"))?;
            let (reference, payload, _) = self
                .state
                .store
                .dataset(&self.loupist_id, id, revision)
                .map_err(command_error)
                .map_err(error_response)?;
            let pointer = arguments["pointer"].as_str().unwrap_or_default();
            let selected = payload
                .pointer(pointer)
                .ok_or_else(|| Response::unavailable("no_data"))?;
            return Ok(json!({
                "_python_value":selected, "dataset_ref":reference,
                "result_ref":value["result_ref"], "citation_url":value["citation_url"],
            }));
        }
        if method == ToolMethod::McpCall
            && (value["ok"] == false
                || value["is_error"] == true
                || value["result"]["isError"] == true
                || value["error"].is_object())
        {
            return Err(match value.pointer("/error/code").and_then(Value::as_str) {
                Some("rate_limited") => {
                    Response::rate_limited(value["error"]["retry_after"].as_u64())
                }
                Some("needs_approval") => refused("needs_approval"),
                Some("no_data") => Response::unavailable("no_data"),
                _ => Response::unavailable("provider_error"),
            });
        }
        if method == ToolMethod::McpCall && value["result"]["row_count"] == 0 {
            return Err(Response::unavailable("no_data"));
        }
        // Other reads use `next` as a host instruction. On the change feed it
        // is the cursor a check stores and follows.
        if call.tool != "world_changes" {
            if let Some(object) = value.as_object_mut() {
                object.remove("next");
            }
        }
        Ok(value)
    }

    /// Resolve an allowlisted read between worker starts. The guest has no live connection.
    async fn sealed_read(&self, policy: &ToolPolicy, call: &ReadCall) -> Response {
        if policy.as_of.is_some()
            && matches!(
                call.tool.as_str(),
                "mcp__openbb__news_company" | "mcp__openbb__news_world"
            )
        {
            return Response::unavailable("no_data");
        }
        let call = match prepare_check_read(policy, call) {
            Ok(call) => call,
            Err(response) => return response,
        };
        let arguments = Value::Object(call.arguments.clone());
        if !CHECK_READ_TOOLS.contains(&call.tool.as_str()) {
            return Response::refused("not_approved");
        }
        if !check_memory_tool(&call) && contains_credential_json(&arguments) {
            return Response::refused("sensitive_text");
        }
        let delivery = new_id("check-read");
        let (response, _, _) = HOST_PYTHON_READ
            .scope((), self.python_read(policy, &call, &delivery))
            .await;
        self.capture.discard_delivery(&delivery).await;
        response
    }

    async fn refresh_check_price(
        &self,
        policy: &ToolPolicy,
        arguments: Value,
        check: &CheckRun<'_>,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Result<Value, Response> {
        use crate::commands::assets::{wire::PriceReadState, with_assets};
        use crate::knowledge::assets::saved_read;

        crate::broker::enforce_policy(policy, ToolMethod::AssetPageRead).map_err(error_response)?;
        let (id, days) = price_arguments(&arguments).map_err(error_response)?;
        let no_data = || Response::unavailable("no_data");
        let asset = self
            .state
            .store
            .asset_by_id(&self.loupist_id, &id)
            .map_err(store_error)
            .map_err(error_response)?
            .filter(|asset| asset.followed_at_ms.is_some())
            .ok_or_else(no_data)?;
        let (stored, saved_points) = with_assets(&self.state, &self.loupist_id, false, |tx| {
            let stored = saved_read::load(tx, &self.loupist_id, &id)?;
            let points = if days > 0 {
                saved_read::points(tx, &self.loupist_id, &id)?
            } else {
                Vec::new()
            };
            Ok((stored, points))
        })
        .map_err(command_error)
        .map_err(error_response)?;
        let stored = stored.ok_or_else(no_data)?;
        let previous = stored.price.as_ref().ok_or_else(no_data)?;
        let grant = crate::scheduled_tasks::types::TriggerToolGrant {
            name: "price".into(),
            provider: Some(previous.provider.clone()),
        };
        if !trigger::admit_grant(check.allowed, &grant, check.discover_providers) {
            return Err(Response::refused("not_approved"));
        }
        let current = &stored.read.current;
        // The check may replay a history recipe in the same approved provider
        // route. A saved recipe for a different provider needs its own grant.
        let history = stored.read.history.as_ref().filter(|history| {
            days > 0
                && history.server == current.server
                && if current.server.is_some() {
                    current
                        .arguments
                        .get("provider")
                        .and_then(Value::as_str)
                        .is_some_and(|provider| {
                            history.arguments.get("provider").and_then(Value::as_str)
                                == Some(provider)
                        })
                } else {
                    history.tool == current.tool
                }
        });
        let mut recipe = history.unwrap_or(current).clone();
        let market = if recipe.tool == "krx_daily_quote" {
            "KRX"
        } else {
            asset
                .reference
                .as_ref()
                .and_then(|reference| reference.fields.exchange.as_deref())
                .unwrap_or("")
        };
        let dates = crate::market_calendar::daily_bar_dates(market, now, None);
        if recipe.tool == "krx_daily_quote" {
            let arguments = recipe.arguments.as_object_mut().ok_or_else(no_data)?;
            arguments.remove("date");
            arguments.insert("start".into(), json!(dates[2].to_string()));
            arguments.insert("end".into(), json!(dates[0].to_string()));
            recipe.rolling = None;
            // Older recipes may select the first bar of a single-day result.
            if recipe.fields.rows.is_none() {
                if let (Some(value), Some(date)) = (
                    recipe.fields.value.strip_prefix("/data/bars/0"),
                    recipe.fields.date.strip_prefix("/data/bars/0"),
                ) {
                    recipe.fields.value = value.to_owned();
                    recipe.fields.date = date.to_owned();
                    recipe.fields.rows = Some("/data/bars".into());
                    // This pointer is relative to the root of the retained result.
                    recipe.fields.currency = None;
                }
            }
        }
        let (payload, provider, url, dataset) = self
            .replay_price_call(policy, &recipe, Some(300))
            .await
            .map_err(|state| match state {
                PriceReadState::NeedsConnection => Response::refused("needs_connection"),
                _ => Response::unavailable("provider_error"),
            })?;
        let mut price = saved_read::price(&payload, &recipe.fields, provider, url, dataset)
            .map_err(|_| no_data())?;
        if !trigger::admit_grant(
            check.allowed,
            &crate::scheduled_tasks::types::TriggerToolGrant {
                provider: Some(price.provider.clone()),
                ..grant
            },
            check.discover_providers,
        ) {
            return Err(Response::refused("not_approved"));
        }
        saved_read::complete_currency(
            &mut price,
            stored.read.quoted_currency.as_deref(),
            asset.reference.as_ref().map(|reference| &reference.fields),
        );
        let latest = saved_read::point(&price).ok_or_else(no_data)?;
        if !dates[..2]
            .iter()
            .any(|date| date.to_string() == latest.date)
        {
            return Err(Response::unavailable("stale"));
        }
        if recipe.fields.rows.is_none() {
            // A quote-only provider cannot return yesterday's observation.
            // Keep today's in the existing asset cache for the next check.
            if with_assets(&self.state, &self.loupist_id, true, |tx| {
                saved_read::settle(tx, &self.loupist_id, &id, &stored, Ok(price.clone()))
            })
            .map_err(command_error)
            .map_err(error_response)?
            {
                crate::commands::assets::changed();
            }
        }
        let mut points = if days == 0 {
            Vec::new()
        } else if recipe.fields.rows.is_some() {
            saved_read::history(&payload, &recipe.fields).map_err(|_| no_data())?
        } else {
            saved_points
                .into_iter()
                .filter(|point| point.date < latest.date)
                .collect()
        };
        if days > 0 && points.last() != Some(&latest) {
            points.push(latest);
        }
        points.drain(..points.len().saturating_sub(days));
        let mut response = json!(price);
        response["points"] = json!(points);
        response["market"] = json!(market);
        Ok(response)
    }

    fn python_saved_price(
        &self,
        policy: &ToolPolicy,
        arguments: Value,
    ) -> Result<Value, BrokerError> {
        let (id, days) = price_arguments(&arguments)?;
        let cutoff = effective_as_of(policy, &Value::Null)?;
        let rows = self
            .state
            .store
            .assets_list(&self.loupist_id, false)
            .map_err(store_error)?;
        let asset = rows
            .into_iter()
            .find(|row| row.asset_id == id)
            .ok_or(BrokerError::EmptyResult)?;
        if let Some(cutoff) = cutoff {
            if !followed_at_cutoff(&asset, cutoff) {
                return Err(BrokerError::EmptyResult);
            }
            let value =
                crate::commands::assets::with_assets(&self.state, &self.loupist_id, false, |tx| {
                    // The saved quote's provider receipt may be newer than the
                    // cutoff. Publish only the dated observations in this slice.
                    let mut points = crate::knowledge::assets::saved_read::points_through(
                        tx,
                        &self.loupist_id,
                        &id,
                        Some(cutoff),
                    )?;
                    let latest = points.last().cloned();
                    points.drain(..points.len().saturating_sub(days));
                    Ok(latest.map(|point| {
                        json!({
                            "date": point.date, "value": point.value, "points": points,
                        })
                    }))
                })
                .map_err(command_error)?;
            return value.ok_or(BrokerError::EmptyResult);
        }
        let value =
            crate::commands::assets::with_assets(&self.state, &self.loupist_id, false, |tx| {
                use crate::knowledge::assets;
                assets::ensure_asset_page(tx, &self.loupist_id, &asset, crate::commands::now_ms())?;
                let view = assets::page_view(tx, &self.loupist_id, asset)?;
                let points = if days > 0 {
                    let mut points = assets::saved_read::points(tx, &self.loupist_id, &id)?;
                    points.drain(..points.len().saturating_sub(days));
                    points
                } else {
                    Vec::new()
                };
                Ok(view.current_price.map(|price| {
                    let mut value = json!(price);
                    value["points"] = json!(points);
                    value
                }))
            })
            .map_err(command_error)?;
        value.ok_or(BrokerError::EmptyResult)
    }
}

fn followed_at_cutoff(asset: &crate::commands::assets::wire::AssetRow, day: NaiveDate) -> bool {
    asset
        .followed_at_ms
        .and_then(crate::local_time::date_at_ms)
        .is_some_and(|followed| followed <= day)
}

fn price_arguments(arguments: &Value) -> Result<(String, usize), BrokerError> {
    let params = exact_object(arguments, &["asset_id"], &["days"])?;
    let id = arguments::bounded_text(params.get("asset_id"), 1, 128)?;
    let days = match params.get("days") {
        None => 0,
        Some(days) => days
            .as_u64()
            .filter(|days| *days <= 4000)
            .ok_or_else(|| BrokerError::invalid("days", "an integer between 0 and 4000"))?
            as usize,
    };
    Ok((id, days))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_template_verdicts_are_dated_by_each_met_observation() {
        use crate::scheduled_tasks::types::TriggerOutcome;
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-25T18:00:00+09:00").unwrap();
        let run = RunLedger {
            id: "legacy".into(),
            now: now.to_rfc3339(),
            args: Default::default(),
            state: json!({}),
            inputs: Default::default(),
            entries: vec![LedgerEntry {
                call: ReadCall {
                    tool: "followed".into(),
                    arguments: Default::default(),
                },
                response: Response::Ok {
                    value: json!([{"id":"sample","reference":{"exchange":"KRX"}}]),
                },
            }],
            final_round: true,
            check_stale_days: Some(5),
            bar_dates: crate::market_calendar::check_bar_dates(now, None)
                .into_iter()
                .map(|(key, dates)| (key.to_owned(), dates.map(|day| day.to_string())))
                .collect(),
        };
        for (template, list) in [
            ("price-band", "moved"),
            ("level-cross", "crossed"),
            ("volume-spike", "spiked"),
            ("new-item", "new"),
        ] {
            for (from, to, want) in [
                ("2026-01-01", "2026-01-02", TriggerOutcome::Stale),
                ("2026-11-23", "2026-11-24", TriggerOutcome::Met),
            ] {
                let mut output = trigger::errored("", vec![], 0);
                output.outcome = TriggerOutcome::Met;
                output.as_of = Some("2026-11-25".into());
                output.evidence = json!({(list): [{"id":"sample", "date":to, "from":{"date":from}, "to":{"date":to}}]});
                guard_legacy_verdict(template, &run, &mut output, now, 5);
                assert_eq!(output.outcome, want, "{template}: {from}, {to}");
                assert_eq!(
                    output.as_of.as_deref(),
                    (want == TriggerOutcome::Met).then_some(to)
                );
            }
        }
    }

    #[test]
    fn provider_retry_delay_survives_the_worker_ledger_and_drives_backoff() {
        use crate::scheduled_tasks::types::TriggerOutcome;

        let response = error_response(BrokerError::ProviderFailure {
            kind: ProviderFailureKind::RateLimited,
            message: "Provider busy".into(),
            next: "Try later".into(),
            retry_after: Some(1_337),
        });
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(
            response,
            json!({"status":"unavailable","code":"rate_limited","retry_after":1337})
        );
        // Bundled checks can catch the rate limit and report that no data was usable.
        let output = trigger::output_from_worker(
            &json!({"status":"failed","check_failure":{"kind":"unavailable","code":"no_data"}}),
            vec![json!({"call":{"tool":"krx_daily_quote"}, "response":response})],
            1,
        );
        assert_eq!(output.outcome, TriggerOutcome::RateLimited);
        assert_eq!(output.retry_after_seconds, Some(1_337));
    }

    #[test]
    fn approved_sealed_reads_pin_the_provider_before_dispatch() {
        let tool = "mcp__openbb__equity_price_historical";
        let allowed = [crate::scheduled_tasks::types::TriggerToolGrant {
            name: tool.into(),
            provider: Some("yfinance".into()),
        }];
        let check = CheckRun {
            code: "pass",
            after_outside_text: false,
            use_memory: true,
            args: Default::default(),
            state: json!({}),
            now: String::new(),
            as_of: None,
            stale_after_days: trigger::DEFAULT_STALE_AFTER_DAYS,
            allowed: &allowed,
            discover_providers: false,
            replay: None,
            max_rounds: 3,
            max_calls: 24,
        };
        let mut call = ReadCall {
            tool: tool.into(),
            arguments: Default::default(),
        };
        assert_eq!(
            check_provider_call(&call, &check).unwrap().arguments["provider"],
            "yfinance"
        );
        call.arguments.insert("provider".into(), json!("fmp"));
        assert!(check_provider_call(&call, &check).is_none());
    }

    #[tokio::test]
    async fn sealed_historical_reads_refuse_current_only_routes() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let executor = AppToolExecutor::for_host(&state, "research", BTreeSet::new());
        let policy = ToolPolicy {
            loupist_id: "research".into(),
            session_id: "check".into(),
            use_memory: true,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: Some("2026-11-23".into()),
            display_currency: None,
        };
        for tool in ["mcp__openbb__news_company", "mcp__openbb__news_world"] {
            let call = ReadCall {
                tool: tool.into(),
                arguments: Default::default(),
            };
            assert_eq!(
                executor.sealed_read(&policy, &call).await.status(),
                "no_data"
            );
        }
    }

    #[tokio::test]
    async fn sealed_price_replay_uses_only_follows_and_prices_through_the_cutoff() {
        use crate::commands::assets::{
            wire::{AssetReferenceFields, AssetReferenceOrigin},
            with_assets,
        };
        use chrono::Duration;

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "research";
        crate::test_fixtures::create_folder_loupist(
            &state.store,
            loupist,
            &root.path().join("workspace"),
        );
        let follow = |name: &str, when| {
            state
                .store
                .asset_follow_name(
                    loupist,
                    name,
                    Some(AssetReferenceFields {
                        ticker: Some(name.to_owned()),
                        exchange: Some("NASDAQ".into()),
                        ..Default::default()
                    }),
                    AssetReferenceOrigin::User,
                    when,
                )
                .unwrap()
        };
        let late_at = chrono::DateTime::parse_from_rfc3339("2026-12-01T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let asset = follow("NVDA", 1);
        let late = follow("LATE", late_at);
        let today: NaiveDate = "2026-11-25".parse().unwrap();
        let sessions = (1..60)
            .rev()
            .map(|days| today - Duration::days(days))
            .filter(|day| crate::market_calendar::is_trading_day("NASDAQ", *day))
            .collect::<Vec<_>>();
        with_assets(&state, loupist, true, |tx| {
            for row in [&asset, &late] {
                crate::knowledge::assets::ensure_asset_page(tx, loupist, row, late_at)?;
                for (index, date) in sessions.iter().chain(std::iter::once(&today)).enumerate() {
                    tx.execute(
                        "INSERT INTO knowledge_asset_price_points(loupist_id, asset_id, fact_date, value) VALUES(?1, ?2, ?3, ?4)",
                        rusqlite::params![loupist, row.asset_id, date.to_string(), if index % 2 == 0 { 100.0 } else { 110.0 }],
                    )?;
                }
            }
            Ok(())
        }).unwrap();

        let executor = AppToolExecutor::for_host(&state, loupist, BTreeSet::new());
        let policy = |day: NaiveDate| ToolPolicy {
            loupist_id: loupist.into(),
            session_id: "check".into(),
            use_memory: true,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: Some(day.to_string()),
            display_currency: None,
        };
        let call = |tool: &str, arguments: Value| ReadCall {
            tool: tool.into(),
            arguments: arguments.as_object().unwrap().clone(),
        };
        let follow_read = call("followed", json!({}));
        let price = call("price", json!({"asset_id": asset.asset_id, "days": 7}));
        let late_price = call("price", json!({"asset_id": late.asset_id, "days": 7}));
        for (index, day) in sessions.iter().enumerate().skip(sessions.len() - 20) {
            let policy = policy(*day);
            let Response::Ok { value: followed } =
                executor.sealed_read(&policy, &follow_read).await
            else {
                panic!("followeds unavailable for {day}");
            };
            assert_eq!(followed.as_array().unwrap().len(), 1);
            assert_eq!(followed[0]["id"], asset.asset_id);
            assert_eq!(
                executor.sealed_read(&policy, &late_price).await.status(),
                "no_data"
            );
            let Response::Ok { value } = executor.sealed_read(&policy, &price).await else {
                panic!("prices unavailable for {day}");
            };
            assert_eq!(value["date"], day.to_string());
            assert!(value.get("dataset").is_none());
            assert_eq!(
                value["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|point| point["date"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                sessions[index - 6..=index]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            );
        }
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        let allowed =
            ["followed", "price"].map(|name| crate::scheduled_tasks::types::TriggerToolGrant {
                name: name.into(),
                provider: None,
            });
        let code = crate::commands::triggers::template_source("price-band").unwrap();
        let result = executor
            .run_check(CheckRun {
                code: &code,
                after_outside_text: false,
                use_memory: true,
                args: Default::default(),
                state: json!({}),
                now: "2026-11-24T23:59:59Z".into(),
                as_of: Some("2026-11-24".into()),
                stale_after_days: trigger::DEFAULT_STALE_AFTER_DAYS,
                allowed: &allowed,
                discover_providers: true,
                replay: None,
                max_rounds: 3,
                max_calls: 3,
            })
            .await
            .unwrap();
        assert_eq!(
            result.outcome,
            crate::scheduled_tasks::types::TriggerOutcome::Met,
            "{result:?}"
        );
        assert_eq!(result.as_of.as_deref(), Some("2026-11-24"));
        assert_eq!(result.evidence["moved"][0]["to"]["date"], "2026-11-24");
        assert_eq!(result.reads, 2);
    }

    #[tokio::test]
    async fn sealed_check_provider_reads_require_connector_grants() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(loupist, &workspace, 1),
        );
        let executor = AppToolExecutor::for_host(&state, loupist, BTreeSet::new());
        let policy = ToolPolicy {
            loupist_id: loupist.into(),
            session_id: "check".into(),
            use_memory: false,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: Some("2026-03-10".into()),
            display_currency: None,
        };
        for (tool, arguments) in [
            (
                "krx_daily_quote",
                json!({"symbol":"990184","date":"2026-03-03","market":"kospi"}),
            ),
            (
                "dart_filing_search",
                json!({"company":"99030800","start":"2026-01-01","end":"2026-03-31","limit":20}),
            ),
        ] {
            let call = ReadCall {
                tool: tool.into(),
                arguments: arguments.as_object().unwrap().clone(),
            };
            assert_eq!(
                executor.sealed_read(&policy, &call).await.status(),
                "needs_connection"
            );
        }
    }

    #[cfg(feature = "eval-harness")]
    #[tokio::test]
    async fn sealed_check_reads_admit_dated_volume_and_filing_lists() {
        use crate::eval_harness::replay::Replay;
        use crate::finance_data::{KRX_SOURCE_ID, OPENDART_SOURCE_ID};

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(loupist, &workspace, 1),
        );
        let capabilities = [KRX_SOURCE_ID, OPENDART_SOURCE_ID].map(str::to_owned);
        for entry_id in &capabilities {
            state
                .store
                .save_loupist_capability(&crate::domain::LoupistCapabilityBinding {
                    loupist_id: loupist.into(),
                    entry_id: entry_id.clone(),
                    enabled: true,
                    granted_permissions: vec!["execute".into()],
                    config: Default::default(),
                    updated_at_ms: 1,
                })
                .unwrap();
        }
        let executor = AppToolExecutor::for_host(&state, loupist, capabilities.into());
        let policy = ToolPolicy {
            loupist_id: loupist.into(),
            session_id: "check".into(),
            use_memory: false,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: Some("2026-03-10".into()),
            display_currency: None,
        };
        let call = |tool: &str, arguments: Value| ReadCall {
            tool: tool.into(),
            arguments: arguments.as_object().unwrap().clone(),
        };
        let cases = [
            (
                call(
                    "krx_daily_quote",
                    json!({"symbol":"990184", "date":"2026-03-03", "market":"kospi"}),
                ),
                include_str!("../../../../evals/agent/suites/replay/krx-noeul-2026-03-03.json"),
                "/data/bars",
                "182233",
                "",
            ),
            (
                call(
                    "dart_filing_search",
                    json!({"company":"99030800", "start":"2026-01-01", "end":"2026-03-31", "limit":20}),
                ),
                include_str!("../../../../evals/agent/suites/replay/opendart-byeolmuri-as-of.json"),
                "/data/filings",
                "20260305990308",
                "20260318990308",
            ),
        ];
        for (call, fixture, rows, expected, excluded) in cases {
            let _transport = serde_json::from_str::<Replay>(fixture)
                .unwrap()
                .install(&state.credential_environment)
                .unwrap();
            let response = executor.sealed_read(&policy, &call).await;
            let Response::Ok { value } = response else {
                panic!("{}: {response:?}", call.tool)
            };
            let rows = value
                .pointer(rows)
                .unwrap_or_else(|| panic!("{value}"))
                .to_string();
            assert!(rows.contains(expected), "{rows}");
            assert!(excluded.is_empty() || !rows.contains(excluded), "{rows}");
        }
        let refused = executor
            .sealed_read(&policy, &call("web_fetch", json!({})))
            .await;
        assert_eq!(refused.status(), "not_approved");
    }

    #[tokio::test]
    async fn python_reads_reject_web_writes_and_custom_tools_after_outside_text() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let executor = AppToolExecutor::for_host(&state, "research", BTreeSet::new());
        let policy = ToolPolicy {
            loupist_id: "research".into(),
            session_id: "chat".into(),
            use_memory: false,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        executor.capture.turn_taint.mark_untrusted_text();
        for (name, status) in [
            ("web_fetch", "not_exposed"),
            ("script_save", "not_exposed"),
            ("world_read", "needs_permission"),
            (
                "mcp__custom-0123456789abcdef0123456789abcdef__quote",
                "untrusted_text_paused",
            ),
        ] {
            let call = ReadCall {
                tool: name.into(),
                arguments: Default::default(),
            };
            let (response, _, _) = executor.python_read(&policy, &call, "test").await;
            assert_eq!(response.status(), status, "{name}");
        }
    }

    fn change_day(value: &str) -> crate::knowledge::TimePoint {
        crate::knowledge::TimePoint {
            precision: crate::knowledge::TimePrecision::Date,
            value: Some(value.into()),
            approximate: false,
        }
    }

    fn noon(day: NaiveDate) -> i64 {
        day.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    struct ChangeWorld {
        /// What the `guidance` claim and the `witness` that ends it state.
        texts: (String, String),
        state: AppState,
        loupist: &'static str,
        asset_id: String,
        source_id: String,
        revision_id: String,
        source_digest: String,
        span_id: String,
        world_id: String,
        world_revision: String,
        head_ms: i64,
    }

    impl ChangeWorld {
        fn new(root: &std::path::Path, head: NaiveDate) -> Self {
            Self::stating(root, head, "Guidance was lowered", "The filing says so")
        }

        fn stating(root: &std::path::Path, head: NaiveDate, guidance: &str, witness: &str) -> Self {
            use crate::commands::assets::wire::{AssetReferenceFields, AssetReferenceOrigin};
            use crate::knowledge::{
                Authorship, BindingRole, ClaimEndpoint, ClaimStatus, Coverage, CreateRecord,
                DependencyRole, EvidenceBinding, EvidenceRevision, GraphClaim, GraphNode,
                KnowledgeStore, RecordKind, RepresentationRole, RetainMaterial, RetainNewSource,
                SealWorld, Selector, SourceClass,
            };

            let state = AppState::for_test(root.join("app"));
            let loupist = "loupist-a";
            crate::test_fixtures::create_folder_loupist(
                &state.store,
                loupist,
                &root.join("workspace"),
            );
            let asset = state
                .store
                .asset_follow_name(
                    loupist,
                    "Northwind",
                    Some(AssetReferenceFields {
                        ticker: Some("NW".into()),
                        exchange: Some("NASDAQ".into()),
                        ..Default::default()
                    }),
                    AssetReferenceOrigin::User,
                    noon(head) - 86_400_000,
                )
                .unwrap();
            crate::commands::assets::with_assets(&state, loupist, true, |tx| {
                crate::knowledge::assets::ensure_asset_page(tx, loupist, &asset, noon(head))?;
                tx.execute(
                    "UPDATE knowledge_asset_pages SET entity_id=?3 WHERE loupist_id=?1 AND asset_id=?2",
                    rusqlite::params![loupist, asset.asset_id, "firm"],
                )?;
                Ok(())
            })
            .unwrap();
            let knowledge = KnowledgeStore::attach(
                state.store.clone(),
                crate::commands::knowledge::knowledge_blobs(&state).unwrap(),
            );
            let retained = knowledge
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: loupist.into(),
                    at_ms: noon(head),
                    title: "Filing".into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: "fixture:filing".into(),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: "filing".into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end: 4 },
                        context_note: "filing".into(),
                        bytes: b"text".to_vec(),
                    }),
                    publication_time: Some(change_day(&head.to_string())),
                })
                .unwrap();
            let span = retained.span_id.clone().unwrap();
            let claim = |id: &str, proposition: &str| GraphClaim {
                id: id.into(),
                proposition: proposition.into(),
                attribution: "fixture".into(),
                relation: "reports".into(),
                endpoints: vec![ClaimEndpoint {
                    node_id: "firm".into(),
                    role: "subject".into(),
                }],
                qualifiers: Vec::new(),
                evidence: vec![EvidenceBinding {
                    span_id: span.clone(),
                    role: BindingRole::ExtractedFrom,
                    attribution: "fixture".into(),
                    figure_check: None,
                    passage: None,
                }],
                status: ClaimStatus::Active,
                recorded_at_ms: Some(noon(head)),
                publication_time: Some(change_day(&head.to_string())),
                effective_time: None,
                valid_at: None,
                invalid_at: None,
                expired_at: None,
            };
            let (world_id, world_revision) = knowledge
                .transact(|write| {
                    let record = write.create_record(CreateRecord {
                        loupist_id: loupist.into(),
                        kind: RecordKind::World,
                        title: "World".into(),
                        at_ms: noon(head),
                    })?;
                    let sealed = write.seal_world(SealWorld {
                        loupist_id: loupist.into(),
                        record_id: record.record_id.clone(),
                        expected_revision_id: None,
                        at_ms: noon(head),
                        nodes: vec![GraphNode {
                            id: "firm".into(),
                            name: "Northwind".into(),
                            aliases: Vec::new(),
                            description: String::new(),
                            kind: "company".into(),
                            source_role: "subject".into(),
                            properties: Default::default(),
                        }],
                        claims: vec![
                            claim("guidance", guidance),
                            {
                                let mut management = claim("management", "Management was replaced");
                                management.qualifiers.push(crate::knowledge::Qualifier {
                                    key: crate::knowledge::CONFLICTS_WITH_CLAIM.into(),
                                    value: "guidance".into(),
                                });
                                management
                            },
                            claim("witness", witness),
                        ],
                        annotations: Vec::new(),
                        dependencies: vec![EvidenceRevision {
                            record_id: retained.record_id.clone(),
                            revision_digest: retained.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        }],
                    })?;
                    Ok((record.record_id, sealed.revision_id))
                })
                .unwrap();
            Self {
                texts: (guidance.to_owned(), witness.to_owned()),
                state,
                loupist,
                asset_id: asset.asset_id,
                source_id: retained.record_id,
                revision_id: retained.revision_id,
                source_digest: retained.revision_digest,
                span_id: span,
                world_id,
                world_revision,
                head_ms: noon(head),
            }
        }

        fn knowledge(&self) -> crate::knowledge::KnowledgeStore {
            crate::knowledge::KnowledgeStore::attach(
                self.state.store.clone(),
                crate::commands::knowledge::knowledge_blobs(&self.state).unwrap(),
            )
        }

        /// The feed closes on the World head. A section change dated after that
        /// seal stays out of the window until the head moves to it.
        fn advance_head(&mut self, at: NaiveDate) {
            use crate::knowledge::{
                BindingRole, ClaimEndpoint, ClaimStatus, DependencyRole, EvidenceBinding,
                EvidenceRevision, GraphClaim, GraphNode, SealWorld,
            };
            let span = self.span_id.clone();
            let sealed = self
                .knowledge()
                .transact(|write| {
                    let claim = |id: &str, proposition: &str| GraphClaim {
                        id: id.into(),
                        proposition: proposition.into(),
                        attribution: "fixture".into(),
                        relation: "reports".into(),
                        endpoints: vec![ClaimEndpoint {
                            node_id: "firm".into(),
                            role: "subject".into(),
                        }],
                        qualifiers: Vec::new(),
                        evidence: vec![EvidenceBinding {
                            span_id: span.clone(),
                            role: BindingRole::ExtractedFrom,
                            attribution: "fixture".into(),
                            figure_check: None,
                            passage: None,
                        }],
                        status: ClaimStatus::Active,
                        recorded_at_ms: Some(noon(at)),
                        publication_time: Some(change_day(&at.to_string())),
                        effective_time: None,
                        valid_at: None,
                        invalid_at: None,
                        expired_at: None,
                    };
                    write.seal_world(SealWorld {
                        loupist_id: self.loupist.into(),
                        record_id: self.world_id.clone(),
                        expected_revision_id: Some(self.world_revision.clone()),
                        at_ms: noon(at),
                        nodes: vec![GraphNode {
                            id: "firm".into(),
                            name: "Northwind".into(),
                            aliases: Vec::new(),
                            description: String::new(),
                            kind: "company".into(),
                            source_role: "subject".into(),
                            properties: Default::default(),
                        }],
                        claims: vec![
                            claim("guidance", &self.texts.0),
                            {
                                let mut management = claim("management", "Management was replaced");
                                management.qualifiers.push(crate::knowledge::Qualifier {
                                    key: crate::knowledge::CONFLICTS_WITH_CLAIM.into(),
                                    value: "guidance".into(),
                                });
                                management
                            },
                            claim("witness", &self.texts.1),
                        ],
                        annotations: Vec::new(),
                        dependencies: vec![EvidenceRevision {
                            record_id: self.source_id.clone(),
                            revision_digest: self.source_digest.clone(),
                            role: DependencyRole::WorldSource,
                        }],
                    })
                })
                .unwrap();
            self.world_revision = sealed.revision_id;
        }

        fn set_pending(&self, pending: bool) {
            let connection = self.state.store.lock().unwrap();
            connection
                .execute(
                    "DELETE FROM knowledge_graph_outbox WHERE loupist_id = ?1",
                    [self.loupist],
                )
                .unwrap();
            if pending {
                let payload = format!(
                    "{{\"sourceBinding\":{{\"sourceRecordId\":\"{}\"}}}}",
                    self.source_id
                );
                connection
                    .execute(
                        "INSERT INTO knowledge_graph_outbox (
                            job_id, loupist_id, world_record_id, world_revision_id, status,
                            payload_json, payload_digest, created_at_ms
                         ) VALUES ('job-q', ?1, ?2, 'rev', 'queued', ?3, 'd', 1)",
                        rusqlite::params![self.loupist, self.world_id, payload],
                    )
                    .unwrap();
            }
        }

        fn publish(
            &self,
            id: &str,
            published: NaiveDate,
            changes: crate::knowledge::world_episodes::EpisodeChanges,
        ) {
            use crate::knowledge::world_episodes::{EpisodeModel, ReferenceBasis, WorldEpisode};
            self.knowledge()
                .transact(|write| {
                    write.insert_world_episode(&WorldEpisode {
                        episode_id: id.into(),
                        loupist_id: self.loupist.into(),
                        world_record_id: self.world_id.clone(),
                        job_id: format!("job-{id}"),
                        sealed_revision_id: format!("rev-{id}"),
                        previous_revision_id: None,
                        source_record_id: self.source_id.clone(),
                        source_revision_id: self.revision_id.clone(),
                        span_id: self.span_id.clone(),
                        chunk_index: Some(0),
                        chunk_count: Some(1),
                        char_start: Some(0),
                        char_end: Some(4),
                        reference_time: Some(published.to_string()),
                        reference_basis: Some(ReferenceBasis::Published),
                        publication_time: Some(change_day(&published.to_string())),
                        model: Some(EpisodeModel {
                            model_profile: "fixture".into(),
                            thinking_level: "low".into(),
                        }),
                        changes,
                        backfilled: false,
                        created_at_ms: self.head_ms,
                    })
                })
                .unwrap();
        }
    }

    fn check_policy(loupist: &str, memory: bool, as_of: Option<NaiveDate>) -> ToolPolicy {
        ToolPolicy {
            loupist_id: loupist.into(),
            session_id: "check".into(),
            use_memory: memory,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: as_of.map(|day| day.to_string()),
            display_currency: None,
        }
    }

    #[tokio::test]
    async fn world_changes_check_reads_follow_memory_and_the_replay_date() {
        use crate::knowledge::world_episodes::{EndBasis, EpisodeChanges, EpisodeEnd};
        let root = tempfile::tempdir().unwrap();
        let head = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let world = ChangeWorld::new(root.path(), head);
        let june = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let march = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        world.publish(
            "june-end",
            june,
            EpisodeChanges {
                ended: vec![EpisodeEnd {
                    claim: "guidance".into(),
                    by: Some("witness".into()),
                    end: change_day("2026-06-01"),
                    basis: EndBasis::Stated,
                }],
                ..Default::default()
            },
        );
        let executor = AppToolExecutor::for_host(&world.state, world.loupist, BTreeSet::new());
        let read = |since: &str, axis: Option<&str>| {
            let mut arguments = serde_json::Map::new();
            arguments.insert("since".into(), json!(since));
            arguments.insert("about".into(), json!([world.asset_id]));
            if let Some(axis) = axis {
                arguments.insert("axis".into(), json!(axis));
            }
            ReadCall {
                tool: "world_changes".into(),
                arguments,
            }
        };
        let open = check_policy(world.loupist, true, None);
        let Response::Ok { value } = executor
            .sealed_read(&open, &read("2026-01-01", Some("recorded")))
            .await
        else {
            panic!("memory on");
        };
        assert_eq!(value["window"]["axis"], "recorded");
        let cursor = value["next"].as_str().unwrap().to_owned();
        assert_eq!(
            executor
                .sealed_read(
                    &check_policy(world.loupist, false, None),
                    &read("2026-01-01", None)
                )
                .await
                .status(),
            "needs_permission"
        );
        assert_eq!(
            executor
                .sealed_read(&open, &read("previous_run", None))
                .await
                .status(),
            "since_previous_run"
        );
        let replay = check_policy(world.loupist, true, Some(march));
        let Response::Ok { value } = executor
            .sealed_read(&replay, &read("2026-01-01", Some("recorded")))
            .await
        else {
            panic!("replay date");
        };
        assert_eq!(value["window"]["axis"], "published");
        let endings: Vec<_> = value["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "ended")
            .map(|row| row["claimId"].as_str().unwrap().to_owned())
            .collect();
        assert!(endings.is_empty(), "June ending is after March: {value}");
        assert_eq!(
            executor
                .sealed_read(&replay, &read(&cursor, None))
                .await
                .status(),
            "cursor_axis_mismatch"
        );
    }

    fn replay_days(end: NaiveDate) -> Vec<NaiveDate> {
        use chrono::Datelike;
        let mut dates = Vec::new();
        let mut cursor = end;
        while dates.len() < 20 {
            cursor = cursor.pred_opt().unwrap();
            if cursor.weekday().number_from_monday() <= 5 {
                dates.push(cursor);
            }
        }
        dates.reverse();
        dates
    }

    #[tokio::test]
    async fn replaying_world_changes_is_met_only_on_days_a_fact_was_published() {
        use crate::knowledge::world_episodes::{
            EndBasis, EpisodeChanges, EpisodeConflict, EpisodeEnd,
        };
        let root = tempfile::tempdir().unwrap();
        let days = replay_days(NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
        let mut world = ChangeWorld::new(root.path(), days[0]);
        let ended = days[4];
        let conflict = days[9];
        let lens_day = days[14];
        world.publish(
            "ended",
            ended,
            EpisodeChanges {
                ended: vec![EpisodeEnd {
                    claim: "guidance".into(),
                    by: Some("witness".into()),
                    end: change_day(&ended.to_string()),
                    basis: EndBasis::Stated,
                }],
                ..Default::default()
            },
        );
        world.publish(
            "conflict",
            conflict,
            EpisodeChanges {
                conflicts: vec![EpisodeConflict {
                    claim: "management".into(),
                    with: "guidance".into(),
                }],
                ..Default::default()
            },
        );
        let lens = world
            .knowledge()
            .file_method(
                world.loupist,
                "Northwind notes",
                "The old steps.",
                &crate::knowledge::test_support::turn_origin("chat", "msg"),
                noon(lens_day),
            )
            .unwrap();
        world
            .knowledge()
            .transact(|write| {
                write.supersede_section(
                    world.loupist,
                    &lens.record_id,
                    "steps",
                    vec!["guidance".into()],
                    noon(lens_day),
                )
            })
            .unwrap();
        let published = [ended, conflict, lens_day];
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        let executor = AppToolExecutor::for_host(&world.state, world.loupist, BTreeSet::new());
        let code = crate::commands::triggers::template_source("world-change").unwrap();
        let allowed = ["followed", "world_changes"].map(|name| {
            crate::scheduled_tasks::types::TriggerToolGrant {
                name: name.into(),
                provider: None,
            }
        });
        let mut state = json!({});
        for day in days {
            if day == lens_day {
                world.advance_head(lens_day);
            }
            let output = executor
                .run_check(CheckRun {
                    code: &code,
                    after_outside_text: false,
                    use_memory: true,
                    args: json!({"asset_ids": [world.asset_id]})
                        .as_object()
                        .unwrap()
                        .clone(),
                    state: state.clone(),
                    now: format!("{day}T23:59:59Z"),
                    as_of: Some(day.to_string()),
                    stale_after_days: trigger::DEFAULT_STALE_AFTER_DAYS,
                    allowed: &allowed,
                    discover_providers: true,
                    replay: None,
                    max_rounds: 3,
                    max_calls: 24,
                })
                .await
                .unwrap();
            let met = output.outcome == crate::scheduled_tasks::types::TriggerOutcome::Met;
            assert_eq!(
                met,
                published.contains(&day),
                "{day} met={met} reading={} error={:?} evidence={}",
                output.reading,
                output.error,
                output.evidence
            );
            if output.outcome.updates_state() {
                state = output.state;
            }
        }
    }

    /// Replays the bundled `world-change` check with `direction` over a fact
    /// that a later Source ends, day by day under each replay date. Returns
    /// the days it was met and the evidence of each.
    async fn replay_direction(
        guidance: &str,
        witness: &str,
        direction: &str,
    ) -> (NaiveDate, Vec<(NaiveDate, Value)>) {
        use crate::knowledge::world_episodes::{EndBasis, EpisodeChanges, EpisodeEnd};
        let root = tempfile::tempdir().unwrap();
        let days = replay_days(NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
        let world = ChangeWorld::stating(root.path(), days[0], guidance, witness);
        let ended = days[6];
        world.publish(
            "revised",
            ended,
            EpisodeChanges {
                ended: vec![EpisodeEnd {
                    claim: "guidance".into(),
                    by: Some("witness".into()),
                    end: change_day(&ended.to_string()),
                    basis: EndBasis::Stated,
                }],
                ..Default::default()
            },
        );
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        let executor = AppToolExecutor::for_host(&world.state, world.loupist, BTreeSet::new());
        let code = crate::commands::triggers::template_source("world-change").unwrap();
        let allowed = ["followed", "world_changes"].map(|name| {
            crate::scheduled_tasks::types::TriggerToolGrant {
                name: name.into(),
                provider: None,
            }
        });
        let mut state = json!({});
        let mut met = Vec::new();
        for day in days {
            let output = executor
                .run_check(CheckRun {
                    code: &code,
                    after_outside_text: false,
                    use_memory: true,
                    args: json!({"asset_ids": [world.asset_id], "kinds": ["ended"], "direction": direction})
                        .as_object()
                        .unwrap()
                        .clone(),
                    state: state.clone(),
                    now: format!("{day}T23:59:59Z"),
                    as_of: Some(day.to_string()),
                    stale_after_days: trigger::DEFAULT_STALE_AFTER_DAYS,
                    allowed: &allowed,
                    discover_providers: true,
                    replay: None,
                    max_rounds: 3,
                    max_calls: 24,
                })
                .await
                .unwrap();
            assert_ne!(
                output.outcome,
                crate::scheduled_tasks::types::TriggerOutcome::Error,
                "{day} {:?}",
                output.error
            );
            if output.outcome == crate::scheduled_tasks::types::TriggerOutcome::Met {
                met.push((day, output.evidence.clone()));
            }
            if output.outcome.updates_state() {
                state = output.state;
            }
        }
        (ended, met)
    }

    #[tokio::test]
    async fn a_world_change_check_with_a_direction_is_met_only_when_the_figure_moved_that_way() {
        let lowered = (
            "Northwind guides full-year revenue of USD 64,000 million.",
            "Northwind now guides full-year revenue of USD 58,000 million.",
        );
        let (ended, met) = replay_direction(lowered.0, lowered.1, "lower").await;
        assert_eq!(
            met.iter().map(|(day, _)| *day).collect::<Vec<_>>(),
            vec![ended],
            "lowered guidance is met on the day the later Source ends it, and before it nothing is"
        );
        let row = &met[0].1["rows"][0];
        assert_eq!(row["figures"]["direction"], "lower", "{row}");
        assert!(row.get("direction_unknown").is_none(), "{row}");
        let (_, met) = replay_direction(lowered.0, lowered.1, "higher").await;
        assert!(met.is_empty(), "{met:?}");

        let (ended, met) = replay_direction(
            "Northwind guides revenue of USD 64,000 million and a margin of 12%.",
            "Northwind guides revenue of USD 58,000 million and a margin of 11%.",
            "higher",
        )
        .await;
        assert_eq!(
            met.iter().map(|(day, _)| *day).collect::<Vec<_>>(),
            vec![ended]
        );
        assert_eq!(
            met[0].1["rows"][0]["direction_unknown"], true,
            "{:?}",
            met[0].1
        );
    }

    #[tokio::test]
    async fn a_second_change_page_is_not_lost_on_the_final_worker_round() {
        use crate::knowledge::world_episodes::{EndBasis, EpisodeChanges, EpisodeEnd};
        use crate::scheduled_tasks::types::TriggerOutcome;
        let root = tempfile::tempdir().unwrap();
        let head = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let world = ChangeWorld::new(root.path(), head);
        for index in 0..31 {
            world.publish(
                &format!("row-{index}"),
                head,
                EpisodeChanges {
                    ended: vec![EpisodeEnd {
                        claim: "guidance".into(),
                        by: Some("witness".into()),
                        end: change_day(&head.to_string()),
                        basis: EndBasis::Stated,
                    }],
                    ..Default::default()
                },
            );
        }
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        let executor = AppToolExecutor::for_host(&world.state, world.loupist, BTreeSet::new());
        let code = crate::commands::triggers::template_source("world-change").unwrap();
        let allowed = ["followed", "world_changes"].map(|name| {
            crate::scheduled_tasks::types::TriggerToolGrant {
                name: name.into(),
                provider: None,
            }
        });
        let output = executor
            .run_check(CheckRun {
                code: &code,
                after_outside_text: false,
                use_memory: true,
                args: json!({"asset_ids": [world.asset_id]})
                    .as_object()
                    .unwrap()
                    .clone(),
                state: json!({}),
                now: "2026-11-01T23:59:59Z".into(),
                as_of: Some(head.to_string()),
                stale_after_days: trigger::DEFAULT_STALE_AFTER_DAYS,
                allowed: &allowed,
                discover_providers: true,
                replay: None,
                max_rounds: 3,
                max_calls: 24,
            })
            .await
            .unwrap();
        assert_ne!(output.outcome, TriggerOutcome::Error, "{:?}", output.error);
        assert!(output.state["cursor"]
            .as_str()
            .is_some_and(|token| token.starts_with("wch1.")));
    }

    #[tokio::test]
    async fn world_change_sequences_report_each_publication_once_and_wait_out_pending_jobs() {
        use crate::knowledge::world_episodes::{EndBasis, EpisodeChanges, EpisodeEnd};
        use crate::scheduled_tasks::types::TriggerOutcome;
        let root = tempfile::tempdir().unwrap();
        let head = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let published = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let world = ChangeWorld::new(root.path(), head);
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        let executor = AppToolExecutor::for_host(&world.state, world.loupist, BTreeSet::new());
        let code = crate::commands::triggers::template_source("world-change").unwrap();
        let allowed = ["followed", "world_changes"].map(|name| {
            crate::scheduled_tasks::types::TriggerToolGrant {
                name: name.into(),
                provider: None,
            }
        });
        let run = |state: Value, day: NaiveDate| {
            let code = code.clone();
            let allowed = allowed.clone();
            let asset_id = world.asset_id.clone();
            let executor = &executor;
            async move {
                executor
                    .run_check(CheckRun {
                        code: &code,
                        after_outside_text: false,
                        use_memory: true,
                        args: json!({"asset_ids": [asset_id]})
                            .as_object()
                            .unwrap()
                            .clone(),
                        state,
                        now: format!("{day}T23:59:59Z"),
                        as_of: Some(day.to_string()),
                        stale_after_days: trigger::DEFAULT_STALE_AFTER_DAYS,
                        allowed: &allowed,
                        discover_providers: true,
                        replay: None,
                        max_rounds: 3,
                        max_calls: 24,
                    })
                    .await
                    .unwrap()
            }
        };
        let first = run(json!({}), head).await;
        assert_eq!(first.outcome, TriggerOutcome::NotMet, "{:?}", first.error);
        let cursor = first.state["cursor"].clone();
        assert!(cursor
            .as_str()
            .is_some_and(|token| token.starts_with("wch1.")));
        world.publish(
            "guide",
            published,
            EpisodeChanges {
                ended: vec![EpisodeEnd {
                    claim: "guidance".into(),
                    by: Some("witness".into()),
                    end: change_day(&published.to_string()),
                    basis: EndBasis::Stated,
                }],
                ..Default::default()
            },
        );
        world.set_pending(true);
        let waiting = run(first.state.clone(), published).await;
        assert_eq!(
            waiting.outcome,
            TriggerOutcome::NotMet,
            "{}",
            waiting.evidence
        );
        assert!(
            waiting.evidence["pending"].as_u64().unwrap_or(0) > 0,
            "{}",
            waiting.evidence
        );
        assert_eq!(
            waiting.state["cursor"], cursor,
            "a pending job does not move the cursor"
        );
        world.set_pending(false);
        let reported = run(waiting.state.clone(), published).await;
        assert_eq!(
            reported.outcome,
            TriggerOutcome::Met,
            "{:?} {}",
            reported.error,
            reported.evidence
        );
        assert_eq!(reported.reading, "Guidance was lowered");
        let again = run(reported.state, published).await;
        assert_eq!(again.outcome, TriggerOutcome::NotMet, "{}", again.evidence);
        assert!(again.evidence["rows"]
            .as_array()
            .is_none_or(|rows| rows.is_empty()));
    }
}
