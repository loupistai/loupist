use super::*;
use crate::{
    commands::assets::wire::{PriceReadState, SavedPriceRead, SavedReadCall},
    datasets::DatasetRef,
    knowledge::assets::saved_read::{self, SaveRequest, StagedPriceRead},
};

fn native_price_method(tool: &str) -> Option<ToolMethod> {
    ToolMethod::from_tool_name(tool).filter(|method| {
        method.producer().is_some_and(|producer| {
            producer.origin == RunResultOrigin::AcquiredOriginal
                && producer.retention.keeps_dataset()
        }) && crate::briefings::policy::allowed_method(*method)
            && method.safety().memory == crate::broker::tool_class::MemoryWrite::NoWrite
    })
}

fn saved_read_error() -> BrokerError {
    BrokerError::invalid(
        "citation_url",
        "Use a retained native or bundled data read from this turn.",
    )
}

fn identity(payload: &Value, fallback: Option<&str>) -> (String, Option<String>) {
    let provider = payload
        .pointer("/provenance/provider")
        .or_else(|| payload.get("provider"))
        .or_else(|| payload.get("source_id"))
        .and_then(Value::as_str)
        .or(fallback)
        .unwrap_or_default();
    let provider = evidence::bounded(provider, 128);
    let source = [
        "/provenance/source_url",
        "/source_url",
        "/receipt/source_url",
        "/url",
    ]
    .into_iter()
    .filter_map(|field| payload.pointer(field).and_then(Value::as_str))
    .find_map(evidence::safe_url);
    (provider, source)
}

impl AppToolExecutor {
    fn retained_price_payload(
        &self,
        result: &Value,
    ) -> Result<(Value, Value, DatasetRef), BrokerError> {
        let reference: DatasetRef = serde_json::from_value(result["dataset_ref"].clone())
            .map_err(|_| saved_read_error())?;
        let (loaded, payload, metadata) = self
            .state
            .store
            .dataset(&self.loupist_id, &reference.dataset_id, reference.revision)
            .map_err(command_error)?;
        if loaded != reference {
            return Err(saved_read_error());
        }
        Ok((payload, metadata, loaded))
    }

    async fn cited_price_call(
        &self,
        url: &str,
    ) -> Result<(SavedReadCall, Value, String, Option<String>, DatasetRef), BrokerError> {
        let entry = {
            let results = self.capture.run_results.lock().await;
            evidence::cited_result(&results, url)
                .map_err(|_| saved_read_error())?
                .clone()
        };
        let (payload, metadata, dataset) = self.retained_price_payload(&entry.payload)?;
        // A Call receipt is the only authority for the replay. Host-supplied
        // dataset slices never inherit the provider's replay authority.

        let request = metadata.get("request").ok_or_else(saved_read_error)?;
        if sha256(request.to_string().as_bytes()) != entry.request_digest
            || request.get("as_of").is_some()
            || metadata.get("as_of").is_some_and(|as_of| !as_of.is_null())
            || crate::privacy::account_fields::is_account_result(&payload)
        {
            return Err(saved_read_error());
        }
        let (server, arguments) = if native_price_method(&entry.producer.tool_name)
            .and_then(ToolMethod::producer)
            .is_some_and(|producer| producer.runtime == entry.producer.runtime_id)
        {
            let method = native_price_method(&entry.producer.tool_name).expect("native producer");
            if let Some(vendor) = crate::finance_data::serving_vendor(method) {
                self.require_capability(vendor.source_id)?;
            }
            (None, request.clone())
        } else {
            let server = &entry.producer.runtime_id;
            if !self.state.artifacts.mcp_runtimes.contains_key(server)
                || request["server_id"] != server.as_str()
                || request["tool_name"] != entry.producer.tool_name
                || entry.producer.origin != RunResultOrigin::AcquiredOriginal
                || !request["arguments"].is_object()
            {
                return Err(saved_read_error());
            }
            (Some(server.clone()), request["arguments"].clone())
        };
        let (provider, source_url) = identity(&payload, entry.producer.provider.as_deref());
        let call = SavedReadCall {
            tool: entry.producer.tool_name,
            server,
            arguments,
            fields: super::super::assets::wire::PriceFields {
                value: String::new(),
                date: String::new(),
                currency: None,
                rows: None,
            },
            rolling: None,
        };
        Ok((call, payload, provider, source_url, dataset))
    }

    pub(super) async fn stage_price_read(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        if !policy.propose_memory_updates {
            return Err(BrokerError::ProposalDisabled);
        }
        if self.capture.as_of_turn() {
            return Err(BrokerError::MethodDenied);
        }
        let input: SaveRequest =
            serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        let asset = self
            .state
            .store
            .asset_by_id(&self.loupist_id, &input.asset_id)
            .map_err(store_error)?
            .filter(|asset| asset.followed_at_ms.is_some());
        let asset = match asset {
            Some(asset) => asset,
            None => self
                .capture
                .staged_setup
                .lock()
                .await
                .as_ref()
                .and_then(|setup| {
                    setup
                        .follow
                        .iter()
                        .find(|asset| asset.asset_id == input.asset_id)
                })
                .cloned()
                .ok_or_else(|| BrokerError::invalid("assetId", "Use a followed asset ID."))?,
        };
        let (mut current, payload, provider, source_url, dataset) =
            self.cited_price_call(&input.citation_url).await?;
        current.fields = input.fields;
        if let Some(rolling) = input.rolling {
            saved_read::rolling_arguments(&current.arguments, &rolling, Utc::now().date_naive())
                .map_err(knowledge_error)?;
            current.rolling = Some(rolling);
        }
        let price = saved_read::price(&payload, &current.fields, provider, source_url, dataset)
            .map_err(knowledge_error)?;
        let (history, points) = if let Some(input) = input.history {
            let (mut history, payload, _, _, _) =
                self.cited_price_call(&input.citation_url).await?;
            history.fields = input.fields;
            saved_read::rolling_arguments(
                &history.arguments,
                &input.rolling,
                Utc::now().date_naive(),
            )
            .map_err(knowledge_error)?;
            history.rolling = Some(input.rolling);
            let points = saved_read::history(&payload, &history.fields).map_err(knowledge_error)?;
            (Some(history), points)
        } else {
            (None, Vec::new())
        };
        let saved = StagedPriceRead {
            asset_id: asset.asset_id.clone(),
            read: SavedPriceRead {
                state: PriceReadState::Ready,
                current,
                history,
            },
            price,
            history: points,
        };
        let result = json!({"assetId":saved.asset_id,"price":saved.price,"savedRead":saved.read});
        self.capture
            .stage(delivery_id, |pending| {
                pending.price_reads.push((asset, saved))
            })
            .await;
        Ok(result)
    }

    pub(in crate::commands) async fn replay_price_call(
        &self,
        policy: &ToolPolicy,
        call: &SavedReadCall,
        max_age: Option<u64>,
    ) -> Result<(Value, String, Option<String>, DatasetRef), PriceReadState> {
        // An eval replays a frozen market, so its window ends on the scenario's day.
        #[cfg(feature = "eval-harness")]
        let today = crate::eval_harness::clock::now().date_naive();
        #[cfg(not(feature = "eval-harness"))]
        let today = Utc::now().date_naive();
        let mut arguments = match &call.rolling {
            Some(rolling) => saved_read::rolling_arguments(&call.arguments, rolling, today)
                .map_err(|_| PriceReadState::Failed)?,
            None => call.arguments.clone(),
        };
        let error = |error| match error {
            BrokerError::MethodDenied | BrokerError::ProviderApprovalRequired { .. } => {
                PriceReadState::NeedsConnection
            }
            _ => PriceReadState::Failed,
        };
        let (result, fallback) = if let Some(server) = &call.server {
            let catalog =
                crate::marketplace::bundled_catalog().map_err(|_| PriceReadState::Failed)?;
            let provider = arguments
                .get("provider")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if !self.state.artifacts.mcp_runtimes.contains_key(server)
                || !catalog.entries.iter().any(|entry| {
                    self.capability_ids.contains(&entry.id)
                        && entry.runtime.server_id.as_deref() == Some(server)
                        && provider
                            .as_ref()
                            .is_none_or(|provider| entry.runtime.provider_ids.contains(provider))
                })
            {
                return Err(PriceReadState::NeedsConnection);
            }
            self.host_connect(policy, server, &[&call.tool])
                .await
                .map_err(error)?;
            if let Some(max_age) = max_age {
                arguments["max_age"] = json!(max_age);
            }
            let result = self
                .host_call(
                    policy,
                    ToolMethod::McpCall,
                    json!({
                        "server_id":server,"tool_name":call.tool,"arguments":arguments,
                    }),
                )
                .await
                .map_err(error)?;
            (result["result"].clone(), provider)
        } else {
            let method = native_price_method(&call.tool).ok_or(PriceReadState::Failed)?;
            if let Some(vendor) = crate::finance_data::serving_vendor(method) {
                self.require_capability(vendor.source_id).map_err(error)?;
            }
            if let Some(max_age) = max_age.filter(|_| {
                method
                    .producer()
                    .is_some_and(|producer| producer.retention == Retention::DatasetMaxAge)
            }) {
                arguments["max_age"] = json!(max_age);
            }
            (
                self.host_call(policy, method, arguments)
                    .await
                    .map_err(error)?,
                None,
            )
        };
        result["citation_url"]
            .as_str()
            .ok_or(PriceReadState::Failed)?;
        let (payload, _, dataset) = self
            .retained_price_payload(&result)
            .map_err(|_| PriceReadState::Failed)?;
        let (provider, source_url) = identity(&payload, fallback.as_deref());
        Ok((payload, provider, source_url, dataset))
    }
}

pub(in crate::commands) fn commit_price_reads(
    tx: &rusqlite::Transaction<'_>,
    loupist: &str,
    at_ms: i64,
    reads: &[(crate::commands::assets::wire::AssetRow, StagedPriceRead)],
) -> crate::knowledge::KnowledgeResult<()> {
    for (asset, read) in reads {
        crate::knowledge::assets::ensure_asset_page(tx, loupist, asset, at_ms)?;
        saved_read::save(tx, loupist, read)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finance_data::KRX_SOURCE_ID;

    #[tokio::test]
    async fn only_an_acknowledged_retained_provider_call_can_be_saved() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(loupist, &root.path().join("workspace"), 1),
        );
        let chat = crate::chat::home::ensure_home_thread(&state, loupist).unwrap();
        let executor =
            AppToolExecutor::for_host(&state, loupist, [KRX_SOURCE_ID.to_owned()].into());
        let policy = ToolPolicy {
            loupist_id: loupist.into(),
            session_id: chat.id,
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let asset = state
            .store
            .seed_followed_listing(loupist, "005930", "KRX", Some("Samsung"))
            .unwrap();
        let request =
            json!({"symbol":"005930","market":"kospi","start":"2026-03-02","end":"2026-03-04"});
        let data = json!({"source_id":KRX_SOURCE_ID, "provenance":{"provider":"KRX","source_url":"https://data.krx.co.kr/"}, "data":{"bars":[
            {"close":"73100","date":"2026-03-02","currency":"KRW"},
            {"close":"73400","date":"2026-03-03","currency":"KRW"}
        ]}});
        let (dataset, _) = executor
            .retain_dataset("sample-price", &data, json!({"request":request}))
            .unwrap();
        let delivered = {
            let mut data = data;
            data["dataset_ref"] = json!(dataset);
            data
        };
        let read_ref = executor
            .capture
            .stage_run_result_with_delivery(
                "read",
                run_result_producer(ToolMethod::KrxDailyQuote, &request, &delivered).unwrap(),
                &request,
                delivered.clone(),
                Vec::new(),
                None,
            )
            .await
            .unwrap();
        let args = |url| {
            json!({"assetId":asset.asset_id,"citation_url":url,
            "fields":{"rows":"/data/bars","value":"/close","date":"/date","currency":"/currency"}})
        };
        assert!(executor
            .execute(
                &policy,
                ToolMethod::PriceReadSave,
                args(evidence::citation_url(&read_ref))
            )
            .await
            .is_err());

        let unrelated = executor
            .capture
            .stage_run_result_with_delivery(
                "unrelated",
                run_result_producer(ToolMethod::DatasetQuery, &request, &delivered).unwrap(),
                &request,
                delivered,
                Vec::new(),
                None,
            )
            .await
            .unwrap();
        executor.capture.commit_delivery("unrelated").await;
        assert!(executor
            .execute(
                &policy,
                ToolMethod::PriceReadSave,
                args(evidence::citation_url(&unrelated))
            )
            .await
            .is_err());
        executor.capture.commit_delivery("read").await;
        // A rolling window must name two of the call's own ISO date arguments.
        let rolling = |from: &str| {
            let mut args = args(evidence::citation_url(&read_ref));
            args["rolling"] = json!({"fromArg":from,"toArg":"end","days":10});
            args
        };
        assert!(executor
            .execute(&policy, ToolMethod::PriceReadSave, rolling("date"))
            .await
            .is_err());
        let result = executor
            .execute(&policy, ToolMethod::PriceReadSave, rolling("start"))
            .await
            .unwrap();
        assert_eq!(result["price"]["value"], 73400.0);
        assert_eq!(result["price"]["date"], "2026-03-03");
        assert_eq!(result["price"]["provider"], "KRX");
        assert_eq!(result["savedRead"]["current"]["arguments"], request);
        assert_eq!(
            result["savedRead"]["current"]["rolling"],
            json!({"fromArg":"start","toArg":"end","days":10})
        );

        let reads = executor.capture.staged_price_reads.lock().await.clone();
        crate::commands::assets::with_assets(&state, loupist, true, |tx| {
            commit_price_reads(tx, loupist, 2, &reads)
        })
        .unwrap();
        let no_grant = AppToolExecutor::for_host(&state, loupist, BTreeSet::new());
        let call = &reads[0].1.read.current;
        assert_eq!(
            no_grant
                .replay_price_call(&policy, call, Some(900))
                .await
                .unwrap_err(),
            PriceReadState::NeedsConnection
        );
    }

    #[cfg(feature = "eval-harness")]
    #[tokio::test]
    async fn native_retained_price_replays_to_the_page_through_the_host() {
        use crate::commands::assets::{
            page_for, replay_current, wire::AssetOpenRequest, with_assets,
        };
        use crate::eval_harness::replay::Replay;

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(loupist, &root.path().join("workspace"), 1),
        );
        let chat = crate::chat::home::ensure_home_thread(&state, loupist).unwrap();
        let asset = state
            .store
            .seed_followed_listing(loupist, "990184", "KRX", Some("Noeul Paper"))
            .unwrap();
        let policy = ToolPolicy {
            loupist_id: loupist.into(),
            session_id: chat.id,
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let mut binding = crate::domain::LoupistCapabilityBinding {
            loupist_id: loupist.to_owned(),
            entry_id: KRX_SOURCE_ID.to_owned(),
            enabled: true,
            granted_permissions: vec!["execute".into()],
            config: Default::default(),
            updated_at_ms: crate::commands::now_ms(),
        };
        state.store.save_loupist_capability(&binding).unwrap();
        let mut fixture: Value = serde_json::from_str(include_str!(
            "../../../../evals/agent/suites/replay/krx-noeul-2026-03-03.json"
        ))
        .unwrap();
        let executor =
            AppToolExecutor::for_host(&state, loupist, [KRX_SOURCE_ID.to_owned()].into());
        {
            let _transport = serde_json::from_value::<Replay>(fixture.clone())
                .unwrap()
                .install(&state.credential_environment)
                .unwrap();
            let read = executor
                .host_call(
                    &policy,
                    ToolMethod::KrxDailyQuote,
                    json!({"symbol":"990184","date":"2026-03-03","market":"kospi"}),
                )
                .await
                .unwrap();
            let result = executor.execute(&policy, ToolMethod::PriceReadSave, json!({
                "assetId":asset.asset_id,"citation_url":read["citation_url"],
                "fields":{"value":"/data/bars/0/close","date":"/data/bars/0/date","currency":"/data/instrument/currency"}
            })).await.unwrap();
            assert_eq!(result["price"]["value"], 73400.0);
            let staged = executor.capture.staged_price_reads.lock().await.clone();
            with_assets(&state, loupist, true, |tx| {
                commit_price_reads(tx, loupist, 2, &staged)
            })
            .unwrap();
        }

        fixture["requests"][0]["response"]["body"]["OutBlock_1"][0]["TDD_CLSPRC"] = json!("73900");
        let _transport = serde_json::from_value::<Replay>(fixture)
            .unwrap()
            .install(&state.credential_environment)
            .unwrap();
        let page = || {
            page_for(
                &state,
                AssetOpenRequest {
                    loupist_id: loupist.into(),
                    asset_id: Some(asset.asset_id.clone()),
                    ticker: None,
                    entity_id: None,
                    stored_only: None,
                },
            )
            .unwrap()
        };
        replay_current(&state, loupist, &asset.asset_id, None)
            .await
            .unwrap();
        let price = page().current_price.unwrap();
        assert_eq!(price.value, 73900.0);
        assert_eq!(price.date, "2026-03-03");
        assert_eq!(price.currency.as_deref(), Some("KRW"));
        let (reference, retained, _) = state
            .store
            .dataset(loupist, &price.dataset.dataset_id, price.dataset.revision)
            .unwrap();
        assert_eq!(reference, price.dataset);
        assert_eq!(retained["data"]["bars"][0]["close"], "73900");
        let chart = with_assets(&state, loupist, false, |tx| {
            saved_read::points(tx, loupist, &asset.asset_id)
        })
        .unwrap();
        assert_eq!(chart.len(), 1);
        assert_eq!((&chart[0].date, chart[0].value), (&price.date, price.value));

        binding.enabled = false;
        binding.granted_permissions.clear();
        binding.updated_at_ms += 1;
        state.store.save_loupist_capability(&binding).unwrap();
        replay_current(&state, loupist, &asset.asset_id, Some(900))
            .await
            .unwrap();
        let page = page();
        assert_eq!(
            page.saved_read.unwrap().state,
            PriceReadState::NeedsConnection
        );
        assert_eq!(page.current_price.unwrap(), price);
    }
}
