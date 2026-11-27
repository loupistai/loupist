//! Asset reads and thesis updates through the ordinary tool executor.
use std::sync::atomic::Ordering;

use chrono::NaiveDate;

use super::arguments::{bounded_text, optional_bounded_text};
use super::*;
use crate::commands::assets as commands;
use crate::knowledge::assets::{self, AssetEvidence, ThesisUpdate, Verdict};

fn stated_day(value: &str) -> Option<NaiveDate> {
    let day = value.get(..10).unwrap_or(value);
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(day, "%Y%m%d"))
        .ok()
}

fn latest_date(payload: &Value, today: NaiveDate) -> Option<NaiveDate> {
    const FIELDS: &[&str] = &[
        "filing_date",
        "filed_at",
        "rcept_dt",
        "date",
        "period_ending",
        "period_end",
        "report_date",
        "ex_dividend_date",
        "observation_date",
        "published_at",
    ];
    fn walk(value: &Value, today: NaiveDate, best: &mut Option<NaiveDate>, depth: usize) {
        if depth > 6 {
            return;
        }
        match value {
            Value::Object(map) => {
                for (key, item) in map {
                    if FIELDS.contains(&key.as_str()) {
                        if let Some(date) = item
                            .as_str()
                            .and_then(stated_day)
                            .filter(|date| *date <= today)
                        {
                            *best = Some(best.map_or(date, |current| current.max(date)));
                        }
                    }
                    walk(item, today, best, depth + 1);
                }
            }
            Value::Array(items) => items
                .iter()
                .take(2_000)
                .for_each(|item| walk(item, today, best, depth + 1)),
            _ => {}
        }
    }
    let mut best = None;
    walk(payload, today, &mut best, 0);
    best
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

    pub(in crate::commands) async fn host_call(
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

    pub(super) async fn asset_page_read(
        &self,
        policy: &ToolPolicy,
        params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.ensure_scope(policy)?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            asset_id: Option<String>,
            asset: Option<String>,
        }
        let input: Input = serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        let query = match (input.asset_id, input.asset) {
            (Some(query), None) | (None, Some(query)) => query,
            _ => return Err(BrokerError::invalid("asset", "Supply one asset reference.")),
        };
        let rows = self
            .state
            .store
            .assets_list(&self.loupist_id, false)
            .map_err(store_error)?;
        let asset = match crate::store::user_context::matching(&rows, &query).as_slice() {
            [asset] => (*asset).clone(),
            [] => return Ok(json!({"exists":false,"candidates":[]})),
            candidates => {
                return Ok(
                    json!({"exists":false,"candidates":candidates.iter().take(20).copied().collect::<Vec<_>>()}),
                )
            }
        };
        let as_of = effective_as_of(policy, &Value::Null)?;
        let asset_id = asset.asset_id.clone();
        let (mut result, since_ms) = commands::with_assets(&self.state, &self.loupist_id, false, |tx| {
            // A first agent read can inspect an empty page without persisting it.
            assets::ensure_asset_page(tx, &self.loupist_id, &asset, crate::commands::now_ms())?;
            let moved_at: Option<i64> = if as_of.is_none() {
                tx.query_row(
                    "SELECT MAX(at_ms) FROM knowledge_asset_moves WHERE loupist_id = ?1 AND asset_id = ?2",
                    rusqlite::params![self.loupist_id, asset_id],
                    |row| row.get(0),
                )?
            } else {
                None
            };
            let mut result = assets::model_read(tx, &self.loupist_id, asset, as_of)?;
            result["exists"] = json!(true);
            Ok((result, moved_at.unwrap_or(0)))
        })
        .map_err(command_error)?;
        if as_of.is_none() {
            if let Ok(block) = crate::commands::knowledge::world_changes::asset_since_read(
                &self.state,
                &self.loupist_id,
                &asset_id,
                since_ms,
            ) {
                result["sinceRead"] = serde_json::to_value(block).unwrap_or(Value::Null);
            }
        }
        Ok(result)
    }

    pub(super) async fn asset_lookup(&self, params: Value) -> Result<Value, BrokerError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            query: String,
        }
        let input: Input = serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        let source = crate::finance_data::openfigi::SOURCE_ID;
        self.require_capability(source)?;
        let lookup =
            crate::finance_data::openfigi::search(&self.state, &self.loupist_id, &input.query)
                .await
                .map_err(command_error)?;
        if let Some(status) = lookup.keychain_status {
            self.capture
                .tool_trace
                .record_connector_keychain_status(status);
        }
        let mut result =
            json!({"candidates":lookup.candidates,"source_id":source,"provider":"OpenFIGI"});
        if lookup.saved_key_unread {
            result["notice"] = json!("The user's saved OpenFIGI API key could not be read, so this search used OpenFIGI's public route, which answers the same way at a lower rate limit. The results stand as they are; mention the key only if a search fails for its limit, and then point the user to Connections to allow access to it.");
        }
        Ok(result)
    }

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
                .filter_map(|upstream| latest_date(&upstream.payload, today))
                .max();
        }
        latest_date(&result.payload, today)
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
                if !policy.use_memory {
                    "thesis_private: Memory reads are off for this Chat. Tell the user it was not changed."
                } else {
                    "thesis_private: Saving is off for this Chat. Tell the user it was not changed."
                },
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
        let (update, checked) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                // A refused update frees the asset for a corrected call.
                self.capture.thesis_assets.lock().await.remove(&asset_id);
                return Err(error);
            }
        };
        let staged = (checked.name, update.clone());
        self.capture
            .stage(delivery_id, move |pending| pending.theses.push(staged))
            .await;
        Ok(json!({
            "status": "staged",
            "assetId": update.asset_id,
            "read": checked.to.map(Verdict::label),
            "lineCrossed": checked.reading.as_ref().map(|reading| reading.crossed),
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
    ) -> Result<(ThesisUpdate, assets::CheckedThesisUpdate), BrokerError> {
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
        let today = crate::local_time::today();
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
        .map_err(|error| {
            let code = error.code.clone();
            BrokerError::invalid(
                "thesis_update",
                format!("{code}: {}", command_message(error)),
            )
        })?;
        Ok((update, checked))
    }
}

impl ToolCapture {
    pub(in crate::commands) fn as_of_turn(&self) -> bool {
        self.turn_as_of.is_some() || self.as_of_turn.load(Ordering::Relaxed)
    }
}
