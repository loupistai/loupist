//! Asset pages inside the tool executor: the host refresh's reads (no model),
//! the model's exact page read, and staged thesis updates. A thesis update is
//! checked here against this turn's acknowledged evidence and committed with
//! the turn; Rust decides the step, the fact date and whether a line crossed.
use std::sync::atomic::Ordering;

use chrono::NaiveDate;

use super::arguments::{bounded_text, optional_bounded_text};
use super::*;
use crate::knowledge::assets::{
    self, observe, plan::PlannedRead, AssetEvidence, ThesisUpdate, Verdict,
};

/// Reuse identical host reads made in the last 30 minutes.
pub(in crate::commands) const HOST_MAX_AGE_SECONDS: u64 = 1800;
/// Providers tried for OpenBB reads when the tool does not name one.
const PREFERRED_PROVIDERS: &[&str] = &[
    "sec",
    "fmp",
    "intrinio",
    "tiingo",
    "polygon",
    "tmx",
    "cboe",
    "nasdaq",
    "federal_reserve",
    "yfinance",
];

/// A planned native read is a retained provider read that `max_age` reuses.
fn native_method(tool: &str) -> Option<ToolMethod> {
    ToolMethod::from_tool_name(tool).filter(|method| {
        method
            .producer()
            .is_some_and(|producer| producer.retention == Retention::DatasetMaxAge)
    })
}

/// Why a host read did not return rows.
#[derive(Debug)]
pub(in crate::commands) enum HostReadError {
    /// No provider the read accepts is enabled for this Loupist.
    NoProvider,
    Failed(BrokerError),
}

impl AppToolExecutor {
    /// An executor for host reads outside a turn: no model, the Loupist's grants.
    pub(in crate::commands) fn for_host(
        state: &AppState,
        loupist_id: &str,
        capability_ids: BTreeSet<String>,
    ) -> Self {
        let capture = ToolCapture {
            mcp_pool: Some(state.mcp_pool.clone()),
            ..ToolCapture::default()
        };
        Self {
            state: state.clone(),
            capture: Arc::new(capture),
            loupist_id: loupist_id.to_owned(),
            capability_ids,
        }
    }

    pub(in crate::commands) async fn host_shutdown(&self) {
        self.capture.shutdown_mcp().await;
    }

    async fn host_call(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError> {
        let delivery = new_id("asset-refresh");
        let result = self.execute_staged(policy, method, params, &delivery).await;
        match &result {
            Ok(_) => self.capture.commit_delivery(&delivery).await,
            Err(_) => self.capture.discard_delivery(&delivery).await,
        }
        result
    }

    /// Connects the bundled market-data runtime and activates `tools` for this refresh.
    pub(in crate::commands) async fn host_connect(
        &self,
        policy: &ToolPolicy,
        server: &str,
        tools: &[&str],
    ) -> Result<(), BrokerError> {
        let connected = self
            .host_call(policy, ToolMethod::McpConnect, json!({"server_id": server}))
            .await?;
        let inactive: Vec<&str> = tools
            .iter()
            .copied()
            .filter(|tool| {
                !connected["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|card| card["mcp_name"].as_str() == Some(tool))
            })
            .collect();
        if !inactive.is_empty() {
            // Unknown names are ignored by the runtime; a refused read reports itself.
            let _ = self
                .host_call(
                    policy,
                    ToolMethod::McpCall,
                    json!({"server_id": server, "tool_name": "activate_tools", "arguments": {"tool_names": inactive}}),
                )
                .await;
        }
        Ok(())
    }

    /// One planned read through the ordinary executor path (connection grants,
    /// the unattended allowlist, retention and `max_age` reuse). Returns the
    /// retained payload and the provider that served it.
    pub(in crate::commands) async fn host_read(
        &self,
        policy: &ToolPolicy,
        read: &PlannedRead,
        max_age: Option<u64>,
    ) -> Result<(Value, Option<String>), BrokerError> {
        let mut arguments = read.arguments.clone();
        if let Some(max_age) = max_age {
            arguments["max_age"] = json!(max_age);
        }
        match read.server {
            None => {
                let method = native_method(read.tool).ok_or(BrokerError::MethodDenied)?;
                if !crate::briefings::policy::allowed_method(method) {
                    return Err(BrokerError::MethodDenied);
                }
                let result = self.host_call(policy, method, arguments).await?;
                let payload = self.retained(&result).await.unwrap_or(result);
                let provider = payload
                    .get("source_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Ok((payload, provider))
            }
            Some(server) => {
                let requested = arguments
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let provider = self
                    .host_provider(server, read.tool, requested.as_deref(), PREFERRED_PROVIDERS)
                    .await;
                match &provider {
                    Some(provider) => arguments["provider"] = json!(provider),
                    None => {
                        if let Some(object) = arguments.as_object_mut() {
                            object.remove("provider");
                        }
                    }
                }
                let response = self
                    .host_call(
                        policy,
                        ToolMethod::McpCall,
                        json!({"server_id": server, "tool_name": read.tool, "arguments": arguments}),
                    )
                    .await?;
                if response.get("call_error").and_then(Value::as_bool) == Some(true) {
                    return Err(BrokerError::Execution(
                        response["message"]
                            .as_str()
                            .unwrap_or("the market-data read failed")
                            .to_owned(),
                    ));
                }
                let delivered = response.get("result").cloned().unwrap_or(response);
                // Observations come from the retained result, not the
                // delimited view the model would receive.
                let payload = self.retained(&delivered).await.ok_or_else(|| {
                    BrokerError::Execution("the market-data read was not retained".into())
                })?;
                if payload.get("isError").and_then(Value::as_bool) == Some(true) {
                    return Err(BrokerError::Execution("the market-data read failed".into()));
                }
                Ok((payload, provider))
            }
        }
    }

    /// A host read that only some providers may serve (consensus only from
    /// FMP or Intrinio; never a scraped provider for expectations). Without
    /// one of them the read is not made.
    pub(in crate::commands) async fn host_read_among(
        &self,
        policy: &ToolPolicy,
        read: &PlannedRead,
        max_age: Option<u64>,
        allowed: &[&str],
    ) -> Result<(Value, Option<String>), HostReadError> {
        let Some(server) = read.server else {
            return self
                .host_read(policy, read, max_age)
                .await
                .map_err(HostReadError::Failed);
        };
        let provider = self
            .host_provider(server, read.tool, None, allowed)
            .await
            .filter(|provider| allowed.contains(&provider.as_str()))
            .ok_or(HostReadError::NoProvider)?;
        let mut pinned = read.clone();
        pinned.arguments["provider"] = json!(provider);
        self.host_read(policy, &pinned, max_age)
            .await
            .map_err(HostReadError::Failed)
    }

    /// The complete result a delivery acknowledged, by its `result_ref`.
    async fn retained(&self, delivered: &Value) -> Option<Value> {
        let reference = delivered.get("result_ref").and_then(Value::as_str)?;
        self.capture
            .run_result(reference)
            .await
            .map(|result| result.payload)
    }

    /// `asset_page_read`: the user's reasons and lines, the read, and the last
    /// 20 dated rows with their sources. Never changes the page.
    pub(super) async fn asset_page_read(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MethodDenied);
        }
        let object = exact_object(&params, &[], &["assetId", "symbol", "market"])?;
        let asset_id = match (
            object.get("assetId").and_then(Value::as_str),
            object.get("symbol").and_then(Value::as_str),
            object.get("market").and_then(Value::as_str),
        ) {
            (Some(id), None, None) if assets::valid_asset_id(id) => id.to_owned(),
            (None, Some(symbol), Some(market))
                if !symbol.trim().is_empty() && !market.trim().is_empty() =>
            {
                assets::asset_id(symbol, market)
            }
            _ => {
                return Err(BrokerError::invalid(
                    "assetId",
                    "pass assetId, or symbol and market",
                ))
            }
        };
        // Every row carries its fact date, so an as-of turn reads only what was
        // dated on or before its cutoff.
        let as_of = super::market::parse_as_of_date(policy.as_of.as_deref())?;
        let context = crate::commands::assets::view_context(&self.state).map_err(command_error)?;
        let loupist = policy.loupist_id.clone();
        let page = crate::commands::assets::with_assets(&self.state, &loupist, false, |tx| {
            if assets::get_page(tx, &loupist, &asset_id)?.is_none() {
                return Ok(json!({
                    "assetId": asset_id,
                    "exists": false,
                    "note": "No asset page yet: the user has written no reasons and nothing has been checked for it."
                }));
            }
            let mut value = assets::model_read(tx, &loupist, &asset_id, &context, as_of)?;
            value["title"] = json!(format!(
                "Asset page: {}",
                value["name"].as_str().unwrap_or_default()
            ));
            // The watchers' last alerts for this asset, dated and sourced.
            let events: Vec<Value> =
                crate::knowledge::watch::asset_events(tx, &loupist, &asset_id, 10)?
                    .into_iter()
                    .filter(|event| {
                        as_of.is_none_or(|cutoff| {
                            observe::parse_day(&event.fact_date).is_some_and(|day| day <= cutoff)
                        })
                    })
                    .map(|event| json!({"kind": event.kind, "factDate": event.fact_date, "facts": event.facts, "sources": event.evidence.iter().map(|item| json!({"title": item.title, "url": item.url})).collect::<Vec<_>>()}))
                    .collect();
            value["watchAlerts"] = json!(events);
            Ok(value)
        })
        .map_err(command_error)?;
        // The latest two results snapshots, each a citable saved read whose
        // link renders the results card in the answer. Not under an as-of date.
        let mut value = page;
        if as_of.is_none() && value["exists"] != false {
            let events = crate::commands::assets::with_assets(&self.state, &loupist, false, |tx| {
                crate::knowledge::earnings::events_for(tx, &loupist, &asset_id)
            })
            .map_err(command_error)?;
            let today = chrono::Local::now().date_naive();
            let mut results = Vec::new();
            for event in events
                .iter()
                .filter(|event| event.report_date <= today + chrono::Duration::days(14))
                .take(2)
            {
                let Ok(Some(snapshot)) = crate::commands::earnings::snapshot_for(
                    &self.state,
                    &loupist,
                    &asset_id,
                    Some(&event.event_id),
                ) else {
                    continue;
                };
                let model = crate::knowledge::earnings::model_view(&snapshot);
                let payload = json!({
                    "title": format!("Results · {} · {}", snapshot.period, snapshot.name),
                    "readRef": format!("earnings:{}:{}", asset_id, snapshot.event_id),
                    "text": serde_json::to_string_pretty(&model).unwrap_or_default(),
                    "snapshot": model,
                });
                let reference = self
                    .capture
                    .stage_run_result_with_delivery(
                        delivery_id,
                        RunResultProducer {
                            origin: RunResultOrigin::SavedContext,
                            runtime_id: "knowledge".into(),
                            tool_name: "earnings_snapshot".into(),
                            provider: None,
                        },
                        &json!({"assetId": asset_id, "eventId": snapshot.event_id}),
                        payload,
                        vec![],
                        None,
                    )
                    .await?;
                let mut entry = model;
                entry["citation_url"] = json!(evidence::citation_url(&reference));
                results.push(entry);
            }
            value["results"] = json!(results);
        }
        Ok(value)
    }

    /// The evidence date of one acknowledged result: a calculation takes its
    /// inputs' latest date; anything else its own latest row or publication date.
    pub(super) fn evidence_day(
        registry: &RunResultRegistry,
        result: &RunResult,
        today: NaiveDate,
    ) -> Option<NaiveDate> {
        if result.producer.tool_name == "calculate" {
            return result
                .upstream_result_refs
                .iter()
                .filter_map(|reference| registry.get(reference))
                .filter_map(|upstream| observe::latest_date(&upstream.payload, today))
                .max();
        }
        observe::latest_date(&result.payload, today)
    }

    fn is_original(registry: &RunResultRegistry, result: &RunResult) -> bool {
        result.producer.origin == RunResultOrigin::AcquiredOriginal
            || (result.producer.tool_name == "calculate"
                && result.upstream_result_refs.iter().any(|reference| {
                    registry.get(reference).is_some_and(|upstream| {
                        upstream.producer.origin == RunResultOrigin::AcquiredOriginal
                    })
                }))
    }

    /// `thesis_update`: one step of the read, or a reading of one of the user's
    /// lines, staged until the reply commits.
    pub(super) async fn stage_thesis_update(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if policy.as_of.is_some() {
            return Err(BrokerError::invalid(
                "thesis_update",
                "thesis_as_of: the asset page always shows today; an as-of answer cannot change it",
            ));
        }
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::invalid(
                "thesis_update",
                "thesis_private: saving is off in this chat, so the read cannot change",
            ));
        }
        let object = exact_object(
            &params,
            &["assetId", "evidence"],
            &["to", "reason", "reasonId", "lineReading"],
        )?;
        let asset_id = object
            .get("assetId")
            .and_then(Value::as_str)
            .filter(|id| assets::valid_asset_id(id))
            .ok_or_else(|| BrokerError::invalid("assetId", "the assetId asset_page_read returned"))?
            .to_owned();
        let to = match object.get("to") {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.as_str().and_then(Verdict::parse).ok_or_else(|| {
                BrokerError::invalid("to", "holding, weakening, challenged or broken")
            })?),
        };
        let reason = optional_bounded_text(object.get("reason"), 1, assets::MAX_REASON_CHARS)
            .map_err(|_| BrokerError::invalid("reason", "1–280 characters"))?;
        let reason_id = optional_bounded_text(object.get("reasonId"), 1, 128)?;
        let line = match object.get("lineReading") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let reading = exact_object(value, &["journalId", "observed"], &[])?;
                let journal_id = bounded_text(reading.get("journalId"), 1, 128)?;
                let observed = reading
                    .get("observed")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| {
                        BrokerError::invalid("lineReading.observed", "a finite number")
                    })?;
                Some((journal_id, observed))
            }
        };
        let urls: Vec<String> = object
            .get("evidence")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if urls.is_empty() || urls.len() > assets::MAX_EVIDENCE {
            return Err(BrokerError::invalid(
                "evidence",
                "1–3 citation_url values from this turn",
            ));
        }
        // Reserve the asset before any await: parallel calls in one step see
        // each other, not only updates already delivered.
        {
            let mut reserved = self.capture.thesis_assets.lock().await;
            if reserved.contains_key(&asset_id) {
                return Err(BrokerError::invalid(
                    "thesis_update",
                    "thesis_twice: the read moves at most once per asset in one reply",
                ));
            }
            reserved.insert(asset_id.clone(), delivery_id.to_owned());
        }
        let checked = self
            .checked_thesis_update(policy, asset_id.clone(), to, reason, reason_id, line, urls)
            .await;
        let (update, (moved_to, reading)) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                // A refused update frees the asset for a corrected call.
                self.capture.thesis_assets.lock().await.remove(&asset_id);
                return Err(error);
            }
        };
        let staged = update.clone();
        self.capture
            .stage(delivery_id, move |pending| pending.theses.push(staged))
            .await;
        Ok(json!({
            "status": "staged",
            "assetId": update.asset_id,
            "read": moved_to.map(Verdict::label),
            "lineCrossed": reading.as_ref().map(|reading| reading.crossed),
            "factDate": update.fact_date.to_string(),
            "note": "Checked against the page now; it is saved when this reply finishes. If the page changes before then (for example the user undoes a change), it is not saved and the reply's receipt says so. Tell the user what changed and cite the evidence."
        }))
    }

    /// The evidence and page checks for one thesis update, without staging.
    #[allow(clippy::too_many_arguments)]
    async fn checked_thesis_update(
        &self,
        policy: &ToolPolicy,
        asset_id: String,
        to: Option<Verdict>,
        reason: Option<String>,
        reason_id: Option<String>,
        line: Option<(String, f64)>,
        urls: Vec<String>,
    ) -> Result<
        (
            ThesisUpdate,
            (Option<Verdict>, Option<assets::LineCrossing>),
        ),
        BrokerError,
    > {
        let loupist = policy.loupist_id.clone();
        // The observed number is read in the unit the user wrote the line in.
        let line_unit = match &line {
            None => None,
            Some((journal_id, _)) => {
                let unit =
                    crate::commands::assets::with_assets(&self.state, &loupist, false, |tx| {
                        assets::line_unit(tx, &loupist, &asset_id, journal_id)
                    })
                    .map_err(command_error)?
                    .ok_or_else(|| {
                        BrokerError::invalid(
                            "lineReading.journalId",
                            "thesis_line: name one of this asset's lines from asset_page_read",
                        )
                    })?;
                Some(
                    crate::knowledge::numbers::Unit::of_line(unit.as_deref()).map_err(|unit| {
                        BrokerError::invalid(
                            "lineReading",
                            format!("thesis_line_unit: the line's unit \"{}\" is ambiguous, so no reading can be checked against it; ask the user to restate the line", unit.0),
                        )
                    })?,
                )
            }
        };
        let today = chrono::Local::now().date_naive();
        let registry = self.capture.run_results.lock().await;
        let mut evidence = Vec::new();
        let mut dates = Vec::new();
        let mut original = false;
        let mut found_observed = line.is_none();
        for (index, url) in urls.iter().enumerate() {
            let result = evidence::cited_result(&registry, url).map_err(|why| {
                BrokerError::invalid("evidence", format!("evidence[{index}]: {why}"))
            })?;
            let Some(date) = Self::evidence_day(&registry, result, today) else {
                return Err(BrokerError::invalid(
                    "evidence",
                    format!("thesis_evidence_undated: evidence[{index}] carries no publication, filing or observation date"),
                ));
            };
            let is_original = Self::is_original(&registry, result);
            original |= is_original;
            // The number must be printed in an original (a filing, a data read,
            // a fetched page) or a host calculation over one; a search snippet
            // or a Memory receipt never confirms it.
            if let (Some((_, observed)), Some(unit), true) = (&line, line_unit, is_original) {
                found_observed |=
                    crate::knowledge::numbers::find(*observed, unit, &result.payload).is_some();
            }
            let projected = evidence::project(result);
            evidence.push(AssetEvidence {
                tool: projected.tool_name,
                provider: projected.provider,
                title: projected.title,
                url: projected.url,
                dataset: projected.dataset_ref,
                pointer: None,
                record_id: None,
                world_id: None,
                citation_url: Some(url.clone()),
                date: Some(date.to_string()),
                retrieved_at: Some(projected.retrieved_at),
            });
            dates.push(date);
        }
        drop(registry);
        if !original {
            return Err(BrokerError::invalid(
                "evidence",
                "thesis_evidence_original: cite at least one original read (a filing, a data result or a fetched page) or a calculation over one; search hits, Memory and your own notes are not enough",
            ));
        }
        if !found_observed {
            return Err(BrokerError::invalid(
                "lineReading.observed",
                "thesis_line_not_found: the observed number, in the line's unit, is not printed exactly in a cited original (a filing, a data read or a fetched page)",
            ));
        }
        let update = ThesisUpdate {
            asset_id,
            to,
            reason,
            reason_id,
            line,
            evidence,
            fact_date: dates.into_iter().max().unwrap_or(today),
        };
        // The same rules the commit applies, so the tool result is truthful.
        let checked = crate::commands::assets::with_assets(&self.state, &loupist, false, |tx| {
            assets::check_update(tx, &loupist, &update)
        })
        .map_err(|error| BrokerError::invalid("thesis_update", command_error(error).to_string()))?;
        Ok((update, checked))
    }
}

impl ToolCapture {
    /// Whether this turn answers under an as-of date: the turn's own date, or
    /// any tool call that ran under one.
    pub(in crate::commands) fn as_of_turn(&self) -> bool {
        self.turn_as_of.is_some() || self.as_of_turn.load(Ordering::Relaxed)
    }

    /// Observations from this turn's acknowledged reads of assets the user
    /// follows, from the same routes the host refresh reads. Model text is
    /// never read; each observation cites the turn's result.
    pub(in crate::commands) async fn turn_observations(
        &self,
        followed: &[assets::AssetIdentity],
    ) -> Vec<(assets::AssetIdentity, Vec<assets::NewObservation>)> {
        let today = chrono::Local::now().date_naive();
        let registry = self.run_results.lock().await;
        let mut out: Vec<(assets::AssetIdentity, Vec<assets::NewObservation>)> = Vec::new();
        for result in registry.values() {
            let tool = result.producer.tool_name.as_str();
            if !observe::observes(tool) {
                continue;
            }
            let payload = &result.payload;
            let mut symbols: Vec<&str> = [
                "/query/symbol",
                "/data/company/stock_code",
                "/data/instrument/symbol",
                "/data/filings/0/stock_code",
            ]
            .iter()
            .filter_map(|pointer| payload.pointer(pointer).and_then(Value::as_str))
            .collect();
            if let Some(origin) = result
                .origin
                .as_deref()
                .filter(|origin| !origin.contains(':'))
            {
                symbols.push(origin);
            }
            let Some(identity) = followed.iter().find(|asset| {
                symbols
                    .iter()
                    .any(|symbol| symbol.eq_ignore_ascii_case(&asset.symbol))
            }) else {
                continue;
            };
            let read = observe::ToolRead {
                tool,
                provider: result.producer.provider.as_deref(),
                payload,
                dataset: payload
                    .get("dataset_ref")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok()),
                retrieved_at: Some(&result.retrieved_at),
                citation_url: Some(evidence::citation_url(&result.result_ref)),
                today,
                session_day: assets::class::session_day(&identity.market, chrono::Utc::now()),
            };
            let observations = observe::extract(&read, &identity.symbol);
            if observations.is_empty() {
                continue;
            }
            match out.iter_mut().find(|(known, _)| known == identity) {
                Some((_, list)) => list.extend(observations),
                None => out.push((identity.clone(), observations)),
            }
        }
        out
    }
}
