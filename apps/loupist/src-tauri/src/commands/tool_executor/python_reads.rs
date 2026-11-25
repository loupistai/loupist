use super::*;
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
        BrokerError::WithNext { source, .. } => error_response(*source),
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
    pub allowed: &'a [crate::scheduled_tasks::types::TriggerToolGrant],
    pub discover_providers: bool,
    pub replay: &'a [Value],
    pub max_rounds: u32,
    pub max_calls: usize,
}

pub(super) const CHECK_READ_TOOLS: &[&str] = &[
    "followed",
    "price",
    "krx_daily_quote",
    "dart_filing_search",
    "mcp__openbb__equity_price_historical",
    "mcp__openbb__equity_fundamental_filings",
    "mcp__openbb__news_company",
    "mcp__openbb__news_world",
];

fn check_memory_tool(call: &ReadCall) -> bool {
    matches!(call.tool.as_str(), "followed" | "price")
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
        if self.capture.turn_taint.has_untrusted_content() {
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
        if check.replay.iter().any(contains_credential_json) {
            return Ok(trigger::errored("sensitive_text", Vec::new(), 0));
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
        for value in check.replay {
            if let Ok(mut entry) = serde_json::from_value::<LedgerEntry>(value.clone()) {
                if !approved(&entry.call) {
                    entry.response = Response::refused("not_approved");
                } else if check_memory_tool(&entry.call) && !policy.use_memory {
                    entry.response = Response::refused("needs_permission");
                } else if policy.as_of.is_some() {
                    entry.response = Response::unavailable("no_data");
                } else if let Response::Ok { value } = &entry.response {
                    let grant = observed_check_grant(&entry.call, value);
                    if !trigger::admit_grant(check.allowed, &grant, check.discover_providers) {
                        entry.response = Response::refused("not_approved");
                    }
                }
                entries.push(entry);
            }
        }
        let mut run = RunLedger {
            id: crate::commands::new_id("check"),
            now: check.now.clone(),
            args: check.args.clone(),
            state: check.state.clone(),
            inputs: Default::default(),
            entries,
            final_round: !check.replay.is_empty(),
        };
        let mut reads = 0u32;
        let mut tools = Vec::new();
        let rounds = check.max_rounds.clamp(1, 3);
        for round in 1..=rounds {
            run.final_round =
                round == rounds || reads as usize >= check.max_calls || !check.replay.is_empty();
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
            if self.capture.turn_taint.has_untrusted_content() {
                return Ok(trigger::errored(
                    &BrokerError::UntrustedTextPaused.to_string(),
                    Vec::new(),
                    reads,
                ));
            }
            let Some(requests) = result.get("_python_pending").cloned() else {
                let mut output = self.screen_check_output(check.code, &run, &result, reads);
                output.tools = tools;
                return Ok(output);
            };
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
                    let response = reader.sealed_read(&policy, &provider_call).await;
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
            // TODO(model-text): Word the credential refusal for check results.
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
            return Ok(json!(rows
                .into_iter()
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
        if let Some(object) = value.as_object_mut() {
            object.remove("next");
        }
        Ok(value)
    }

    /// Resolve an allowlisted read between worker starts. The guest has no live connection.
    async fn sealed_read(&self, policy: &ToolPolicy, call: &ReadCall) -> Response {
        if policy.as_of.is_some()
            && matches!(
                call.tool.as_str(),
                "followed" | "price" | "mcp__openbb__news_company" | "mcp__openbb__news_world"
            )
        {
            return Response::unavailable("no_data");
        }
        let arguments = Value::Object(call.arguments.clone());
        if !CHECK_READ_TOOLS.contains(&call.tool.as_str()) {
            return Response::refused("not_approved");
        }
        if !check_memory_tool(call) && contains_credential_json(&arguments) {
            return Response::refused("sensitive_text");
        }
        let delivery = new_id("check-read");
        let (response, _, _) = HOST_PYTHON_READ
            .scope((), self.python_read(policy, call, &delivery))
            .await;
        self.capture.discard_delivery(&delivery).await;
        response
    }

    fn python_saved_price(
        &self,
        policy: &ToolPolicy,
        arguments: Value,
    ) -> Result<Value, BrokerError> {
        let params = exact_object(&arguments, &["asset_id"], &["days"])?;
        let id = arguments::bounded_text(params.get("asset_id"), 1, 128)?;
        let days = match params.get("days") {
            None => 0,
            Some(days) => days
                .as_u64()
                .filter(|days| *days <= 4000)
                .ok_or_else(|| BrokerError::invalid("days", "an integer between 0 and 4000"))?
                as usize,
        };
        // A current saved quote can include information unavailable by the cutoff.
        if policy.as_of.is_some() {
            return Err(BrokerError::EmptyResult);
        }
        let rows = self
            .state
            .store
            .assets_list(&self.loupist_id, false)
            .map_err(store_error)?;
        let asset = rows
            .into_iter()
            .find(|row| row.asset_id == id)
            .ok_or(BrokerError::EmptyResult)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
            allowed: &allowed,
            discover_providers: false,
            replay: &[],
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
        for tool in [
            "followed",
            "price",
            "mcp__openbb__news_company",
            "mcp__openbb__news_world",
        ] {
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
}
