use super::*;

fn manifest() -> KisManifest {
    KisManifest::parse(KIS_MANIFEST_JSON).unwrap()
}

fn complete_profile() -> KisAccountProfile {
    KisAccountProfile::from_values(BTreeMap::from([
        ("account_number".to_owned(), "12345678".to_owned()),
        ("account_product_code".to_owned(), "01".to_owned()),
        ("account_password".to_owned(), "native-password".to_owned()),
        (
            "customer_identity_number".to_owned(),
            "identity-number".to_owned(),
        ),
        ("home_net_id".to_owned(), "home-net-id".to_owned()),
        ("hts_id".to_owned(), "hts-user".to_owned()),
    ]))
    .unwrap()
}

fn account_only_profile() -> KisAccountProfile {
    KisAccountProfile::from_values(BTreeMap::from([
        ("account_number".to_owned(), "12345678".to_owned()),
        ("account_product_code".to_owned(), "01".to_owned()),
    ]))
    .unwrap()
}

fn tool_query_for(operation: &KisOperation, environment: KisEnvironment) -> KisToolQuery {
    let selected_rule = operation
        .tr_id_rules
        .iter()
        .find(|rule| rule.when.get("environment").map(String::as_str) == Some(environment.as_str()))
        .unwrap();
    let mut params = BTreeMap::new();
    for parameter in operation
        .parameters
        .iter()
        .filter(|parameter| parameter.source == KisParameterSource::Tool)
    {
        if let Some(value) = selected_rule.when.get(&parameter.id) {
            params.insert(parameter.id.clone(), value.clone());
        } else if parameter.required {
            params.insert(parameter.id.clone(), "X".to_owned());
        }
    }
    KisToolQuery {
        operation_id: operation.id.clone(),
        params,
    }
}

fn price_request(manifest: &KisManifest) -> PreparedKisRequest<'_> {
    let query = KisToolQuery {
        operation_id: "domestic_stock.inquire_price".to_owned(),
        params: BTreeMap::from([
            ("fid_cond_mrkt_div_code".to_owned(), "J".to_owned()),
            ("fid_input_iscd".to_owned(), "005930".to_owned()),
        ]),
    };
    prepare_kis_request(
        manifest,
        &query,
        KisEnvironment::Real,
        None,
        KisRequestAuthority::AgentRead,
    )
    .unwrap()
}

fn raw_response(prepared: &PreparedKisRequest<'_>, bytes: &[u8]) -> RawProviderResponse {
    RawProviderResponse {
        bytes: bytes.to_vec(),
        source_url: prepared.url.to_string(),
        retrieved_at: "2026-08-13T00:00:00.000Z".to_owned(),
        continuation: None,
    }
}

fn one_page(raw: RawProviderResponse) -> KisPages {
    KisPages {
        pages: vec![raw],
        stop: KisPageStop::Complete,
    }
}

#[test]
fn manifest_excludes_writes_and_every_read_prepares_on_its_fixed_host() {
    let manifest = manifest();
    assert!(manifest
        .operations
        .iter()
        .all(|operation| operation.http_method == "GET"));
    for excluded in &manifest.excluded_write_operation_ids {
        assert!(manifest.operation(excluded).is_none(), "{excluded}");
    }
    let order = KisToolQuery {
        operation_id: "domestic_stock.order_cash".to_owned(),
        params: BTreeMap::new(),
    };
    assert!(prepare_kis_request(
        &manifest,
        &order,
        KisEnvironment::Real,
        None,
        KisRequestAuthority::AgentRead,
    )
    .is_err());

    let profile = complete_profile();
    for operation in &manifest.operations {
        for (environment, host) in [
            (KisEnvironment::Real, "openapi.koreainvestment.com"),
            (KisEnvironment::Demo, "openapivts.koreainvestment.com"),
        ] {
            let query = tool_query_for(operation, environment);
            let prepared = prepare_kis_request(
                &manifest,
                &query,
                environment,
                Some(&profile),
                KisRequestAuthority::AgentRead,
            )
            .unwrap_or_else(|error| panic!("{} {}: {error}", operation.id, environment.as_str()));
            assert_eq!(prepared.url.host_str(), Some(host));
        }
        if operation.requires_account_profile() {
            let query = tool_query_for(operation, KisEnvironment::Real);
            assert!(prepare_kis_request(
                &manifest,
                &query,
                KisEnvironment::Real,
                None,
                KisRequestAuthority::AgentRead,
            )
            .is_err());
        }
    }

    let encoded = price_request(&manifest).url.to_string();
    for secret in ["appkey", "appsecret", "token"] {
        assert!(!encoded.contains(secret));
    }
}

#[test]
fn account_profile_values_cannot_be_supplied_as_tool_parameters() {
    let manifest = manifest();
    for (operation_id, parameter, value) in [
        ("domestic_stock.inquire_account_balance", "cano", "87654321"),
        ("domestic_stock.psearch_result", "user_id", "model-user"),
    ] {
        let operation = manifest.operation(operation_id).unwrap();
        let mut query = tool_query_for(operation, KisEnvironment::Real);
        query.params.insert(parameter.to_owned(), value.to_owned());
        assert!(prepare_kis_request(
            &manifest,
            &query,
            KisEnvironment::Real,
            Some(&complete_profile()),
            KisRequestAuthority::AgentRead,
        )
        .is_err());
    }
}

#[test]
fn operation_search_exposes_account_reads_only_with_a_native_profile() {
    let service = FinanceDataService::new().unwrap();
    let public = service
        .kis_operation_search(json!({"query": "", "limit": "20"}), None)
        .unwrap();
    let operations = public["operations"].as_array().unwrap();
    assert!(!operations.is_empty());
    for operation in operations {
        assert_ne!(operation["scope"], "account");
        for parameter in operation["parameters"].as_array().unwrap() {
            assert!(!matches!(
                parameter["id"].as_str(),
                Some("cano" | "acnt_prdt_cd" | "user_id" | "acnt_pwd")
            ));
        }
    }

    let profiled = service
        .kis_operation_search(
            json!({"query": "inquire_balance", "limit": "20"}),
            Some(&complete_profile()),
        )
        .unwrap();
    assert!(profiled["operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| operation["scope"] == "account"));
}

#[test]
fn market_normalization_rejects_sensitive_fields_and_drops_unreviewed_ones() {
    let manifest = manifest();
    let prepared = price_request(&manifest);
    let sensitive = raw_response(
        &prepared,
        br#"{"rt_cd":"0","msg_cd":"MCA00000","msg1":"ok","output":{"stck_shrn_iscd":"005930","CANO":"12345678"}}"#,
    );
    assert!(matches!(
        normalize_kis_market_response(&prepared, &one_page(sensitive), None),
        Err(FinanceDataError::KisResponseContract { .. })
    ));

    let additive = raw_response(
        &prepared,
        br#"{"rt_cd":"0","msg_cd":"MCA00000","msg1":"ok","provider_meta":"ignored","output":{"stck_shrn_iscd":"005930","future_field":"ignored"}}"#,
    );
    let result = normalize_kis_market_response(&prepared, &one_page(additive), None).unwrap();
    let response = &result["data"]["response"];
    assert_eq!(response["output"]["stck_shrn_iscd"], "005930");
    assert!(response.get("provider_meta").is_none());
    assert!(response["output"].get("future_field").is_none());
    assert_eq!(result["provenance"]["raw_persisted"], false);
}

#[test]
fn account_normalization_redacts_identity_and_rejects_hidden_profile_values() {
    let manifest = manifest();
    let profile = complete_profile();
    let operation = manifest
        .operation("domestic_futureoption.inquire_balance")
        .unwrap();
    let query = tool_query_for(operation, KisEnvironment::Real);
    let prepared = prepare_kis_request(
        &manifest,
        &query,
        KisEnvironment::Real,
        Some(&profile),
        KisRequestAuthority::AgentRead,
    )
    .unwrap();
    let balance = raw_response(
        &prepared,
        br#"{"rt_cd":"0","msg_cd":"MCA00000","msg1":"ok","ctx_area_fk200":"","ctx_area_nk200":"","output1":[{"cano":"12345678","evlu_amt":"1000000"}],"output2":[]}"#,
    );
    let result =
        normalize_kis_account_response(&prepared, &one_page(balance), Some(&profile)).unwrap();
    assert_eq!(
        result["data"]["response"]["output1"][0]["evlu_amt"],
        "1000000"
    );
    assert!(!serde_json::to_string(&result).unwrap().contains("12345678"));

    let hidden = raw_response(
        &prepared,
        br#"{"rt_cd":"0","msg_cd":"MCA00000","msg1":"ok","ctx_area_fk200":"","ctx_area_nk200":"","output1":[{"evlu_amt":"prefix-hts-user-suffix"}],"output2":[]}"#,
    );
    assert!(matches!(
        normalize_kis_account_response(&prepared, &one_page(hidden), Some(&profile)),
        Err(FinanceDataError::InvalidResponse)
    ));
}

#[test]
fn credential_echo_is_rejected_before_a_provider_result_is_admitted() {
    let app_key = "test-kis-application-key";
    let app_secret = "test-kis-application-secret";
    let access_token = "test-kis-access-token-value";

    for body in [
        format!(r#"{{"rt_cd":"0","output":{{"app_key":"{app_key}","price":"1"}}}}"#),
        format!(r#"{{"rt_cd":"0","output":{{"provider_value":"prefix-{app_secret}-suffix"}}}}"#),
        format!(r#"{{"rt_cd":"0","output":{{"Authorization":"Bearer {access_token}"}}}}"#),
    ] {
        assert!(matches!(
            reject_kis_credential_echo(body.as_bytes(), app_key, app_secret, access_token,),
            Err(FinanceDataError::InvalidResponse)
        ));
    }
    assert!(reject_kis_credential_echo(
        br#"{"rt_cd":"0","output":{"stck_prpr":"70000"}}"#,
        app_key,
        app_secret,
        access_token,
    )
    .is_ok());
}

#[tokio::test]
async fn deleting_kis_credentials_can_clear_every_ephemeral_access_token() {
    let service = FinanceDataService::new().unwrap();
    service.kis_tokens.lock().await.tokens.insert(
        "test-fingerprint".to_owned(),
        KisAccessToken {
            value: "test-kis-access-token-value".to_owned(),
            environment: KisEnvironment::Real,
            expires_at: Utc::now() + ChronoDuration::hours(1),
        },
    );
    service.clear_kis_token_cache().await;
    assert!(service.kis_tokens.lock().await.tokens.is_empty());
}

// Hold an inert local proxy open so a token request terminates through its
// short HTTP timeout without contacting KIS or any external network service.
fn service_with_unresponsive_local_proxy() -> (FinanceDataService, std::net::TcpListener) {
    let proxy = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut service = FinanceDataService::new().unwrap();
    service.client = Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://{}", proxy.local_addr().unwrap())).unwrap())
        .connect_timeout(Duration::from_millis(100))
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    (service, proxy)
}

#[tokio::test]
async fn synchronous_scope_invalidation_prevents_reusing_tokens_and_cooldowns() {
    let (service, _proxy) = service_with_unresponsive_local_proxy();
    let app_key = "test-kis-application-key";
    let app_secret = "test-kis-application-secret";
    let fingerprint = kis_credential_fingerprint(KisEnvironment::Real, app_key, app_secret);
    let mut cache = service.kis_tokens.lock().await;
    cache.tokens.insert(
        fingerprint.clone(),
        KisAccessToken {
            value: "old-scope-test-token".into(),
            environment: KisEnvironment::Real,
            expires_at: Utc::now() + ChronoDuration::hours(1),
        },
    );
    cache
        .issuance_blocked_until
        .insert(fingerprint, Utc::now() + KIS_TOKEN_ISSUANCE_COOLDOWN);
    // A synchronous reconciliation must invalidate even while async work
    // holds the mutex. The next (cold) access must not deadlock on the cache.
    service.invalidate_kis_token_cache();
    drop(cache);
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        service.kis_access_token(KisEnvironment::Real, app_key, app_secret),
    )
    .await
    .expect("invalidated token access must finish its bounded request");
    assert!(matches!(
        result,
        Err(FinanceDataError::NetworkSafe(KIS_SOURCE_ID))
    ));
    let cache = service.kis_token_cache().await;
    assert!(cache.tokens.is_empty());
    assert!(cache.issuance_blocked_until.is_empty());
}

#[tokio::test]
async fn kis_tokens_are_reused_and_reissue_waits_out_the_cooldown() {
    let service = FinanceDataService::new().unwrap();
    let app_key = "test-kis-application-key";
    let app_secret = "test-kis-application-secret";
    let remembered = KisAccessToken {
        value: "cached-after-verify-access-token".to_owned(),
        environment: KisEnvironment::Demo,
        expires_at: Utc::now() + ChronoDuration::hours(1),
    };
    service
        .remember_kis_access_token(
            KisEnvironment::Demo,
            app_key,
            app_secret,
            remembered.clone(),
        )
        .await;
    let reused = service
        .kis_access_token(KisEnvironment::Demo, app_key, app_secret)
        .await
        .expect("cached KIS token should be reusable");
    assert_eq!(reused.value, remembered.value);

    let fingerprint = kis_credential_fingerprint(KisEnvironment::Real, app_key, app_secret);
    service
        .kis_tokens
        .lock()
        .await
        .issuance_blocked_until
        .insert(fingerprint, Utc::now() + ChronoDuration::seconds(60));
    assert!(matches!(
        service
            .kis_access_token(KisEnvironment::Real, app_key, app_secret)
            .await,
        Err(FinanceDataError::KisRateLimited(_))
    ));
}

#[test]
fn token_error_bodies_are_typed_and_never_echo_the_secret() {
    let rejected = match parse_kis_token_response(
        br#"{"error_code":"EGW00123","error_description":"The app key and secret do not match."}"#,
        KisEnvironment::Real,
        "test-app-key",
        "test-app-secret",
    ) {
        Err(error) => error,
        Ok(_) => panic!("KIS error response unexpectedly produced a token"),
    };
    assert!(matches!(
        rejected,
        FinanceDataError::KisCredentialRejected(_)
    ));
    assert!(matches!(
        classify_kis_token_error(
            StatusCode::FORBIDDEN,
            r#"{"error_code":"EGW00133","error_description":"1분당 1회만 발급 가능합니다."}"#
                .as_bytes(),
            "test-app-key",
            "test-app-secret",
        ),
        FinanceDataError::KisRateLimited(_)
    ));
    let redacted = classify_kis_token_error(
        StatusCode::BAD_REQUEST,
        br#"{"error_code":"EGW00123","error_description":"test-app-secret was rejected"}"#,
        "test-app-key",
        "test-app-secret",
    );
    assert!(!redacted.to_string().contains("test-app-secret"));
    assert!(matches!(
        redacted,
        FinanceDataError::KisCredentialRejected(diagnostic)
            if diagnostic.summary() == "EGW00123"
    ));
}

#[test]
fn user_context_kis_recorded_balance_import() {
    let manifest = manifest();
    let profile = account_only_profile();
    let query: KisToolQuery = serde_json::from_value(kis_holdings_query()).unwrap();
    let prepared = prepare_kis_request(
        &manifest,
        &query,
        KisEnvironment::Real,
        Some(&profile),
        KisRequestAuthority::AgentRead,
    )
    .unwrap();
    let raw = RawProviderResponse {
        bytes: include_bytes!("fixtures/holdings_balance.json").to_vec(),
        source_url: prepared.url.to_string(),
        retrieved_at: "2026-10-28T00:00:00Z".into(),
        continuation: None,
    };
    let result =
        normalize_kis_account_response(&prepared, &one_page(raw.clone()), Some(&profile)).unwrap();
    let positions = kis_holdings_positions(&result).unwrap();
    assert_eq!(positions[0].quantity, "10");
    assert_eq!(positions[0].cost_basis.as_deref(), Some("65000.125"));
    let store = crate::store::SqliteStore::open_in_memory().unwrap();
    let snapshot = store
        .save_holdings_snapshot(crate::store::user_context::HoldingsSource::Kis, positions)
        .unwrap();
    assert_eq!(store.holdings_latest().unwrap(), Some(snapshot));
    let capped = KisPages {
        pages: vec![raw],
        stop: KisPageStop::PageCap,
    };
    let partial = normalize_kis_account_response(&prepared, &capped, Some(&profile)).unwrap();
    assert!(kis_holdings_positions(&partial).is_err());
    assert!(prepare_kis_request(
        &manifest,
        &query,
        KisEnvironment::Real,
        None,
        KisRequestAuthority::AgentRead
    )
    .is_err());
}

fn prepared_for<'a>(
    manifest: &'a KisManifest,
    operation_id: &str,
    params: &[(&str, &str)],
    profile: Option<&KisAccountProfile>,
) -> PreparedKisRequest<'a> {
    let query = match params {
        [] => tool_query_for(
            manifest.operation(operation_id).unwrap(),
            KisEnvironment::Real,
        ),
        params => KisToolQuery {
            operation_id: operation_id.to_owned(),
            params: params
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        },
    };
    prepare_kis_request(
        manifest,
        &query,
        KisEnvironment::Real,
        profile,
        KisRequestAuthority::AgentRead,
    )
    .unwrap()
}

fn page(body: Value, tr_cont: &str, retrieved_at: &str) -> RawProviderResponse {
    RawProviderResponse {
        bytes: serde_json::to_vec(&body).unwrap(),
        source_url: "https://openapi.koreainvestment.com:9443/uapi/fixture".into(),
        retrieved_at: retrieved_at.into(),
        continuation: Some(tr_cont.into()),
    }
}

type FixtureResult = Result<RawProviderResponse, FinanceDataError>;

/// Serves fixture pages in order and records each request URL and whether
/// it carried `tr_cont: N`.
struct FixturePages {
    pages: std::sync::Mutex<std::collections::VecDeque<FixtureResult>>,
    requests: std::sync::Mutex<Vec<(Url, bool)>>,
}

impl FixturePages {
    fn new(pages: Vec<FixtureResult>) -> Self {
        Self {
            pages: std::sync::Mutex::new(pages.into()),
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    async fn collect(
        &self,
        prepared: &PreparedKisRequest<'_>,
        paced: &std::sync::atomic::AtomicUsize,
    ) -> Result<KisPages, FinanceDataError> {
        let pacer = || {
            paced.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {}) as futures_util::future::BoxFuture<'_, ()>
        };
        collect_kis_pages(
            prepared,
            |url, continuing| {
                self.requests.lock().unwrap().push((url, continuing));
                let next = self
                    .pages
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("the loop requested more pages than the fixture has");
                async move { next }
            },
            &pacer,
        )
        .await
    }

    fn query(&self, index: usize, wire_name: &str) -> Option<String> {
        self.requests.lock().unwrap()[index]
            .0
            .query_pairs()
            .find(|(key, _)| key == wire_name)
            .map(|(_, value)| value.into_owned())
    }

    fn continuing(&self) -> Vec<bool> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(_, continuing)| *continuing)
            .collect()
    }
}

fn holiday_page(days: &[&str], fk: &str, tr_cont: &str) -> RawProviderResponse {
    page(
        json!({
            "rt_cd": "0", "msg_cd": "MCA00000", "msg1": "ok",
            "ctx_area_fk": fk, "ctx_area_nk": format!("{fk}-nk"),
            "output": days.iter().map(|day| json!({"bass_dt": day, "opnd_yn": "Y"})).collect::<Vec<_>>()
        }),
        tr_cont,
        "2026-11-02T01:15:00.000Z",
    )
}

#[tokio::test]
async fn continuation_follows_cursor_keys_and_merges_rows_in_page_order() {
    let manifest = manifest();
    let prepared = prepared_for(
        &manifest,
        "domestic_stock.chk_holiday",
        &[("bass_dt", "20261102")],
        None,
    );
    let provider = FixturePages::new(vec![
        Ok(holiday_page(&["20261102", "20261103"], "cursor one  ", "M")),
        Ok(holiday_page(&["20261104"], "", "D")),
    ]);
    let paced = std::sync::atomic::AtomicUsize::new(0);
    let pages = provider.collect(&prepared, &paced).await.unwrap();
    assert_eq!(pages.stop, KisPageStop::Complete);
    assert_eq!(provider.continuing(), vec![false, true]);
    assert_eq!(provider.query(0, "CTX_AREA_FK").as_deref(), Some(""));
    assert_eq!(
        provider.query(1, "CTX_AREA_FK").as_deref(),
        Some("cursor one  ")
    );
    assert_eq!(
        provider.query(1, "CTX_AREA_NK").as_deref(),
        Some("cursor one  -nk")
    );
    assert_eq!(provider.query(1, "BASS_DT").as_deref(), Some("20261102"));
    assert_eq!(paced.load(std::sync::atomic::Ordering::SeqCst), 1);

    let result = normalize_kis_market_response(&prepared, &pages, None).unwrap();
    let days = result["data"]["response"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["bass_dt"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(days, ["20261102", "20261103", "20261104"]);
    // Cursors are transport state, not data.
    assert!(result["data"]["response"].get("ctx_area_fk").is_none());
    assert_eq!(
        result["page"],
        json!({"returned": 3, "available": 3, "truncated": false, "provider_pages": 2,
               "page_limit": MAX_KIS_PAGES, "stop_reason": "complete"})
    );
    assert_eq!(
        result["provenance"]["retrieved_at"],
        "2026-11-02T01:15:00.000Z"
    );
    assert_eq!(result["provenance"]["timezone"], "Asia/Seoul");
    assert_eq!(result["provenance"]["session_at_retrieval"], "regular");
    assert_eq!(
        result["provenance"]["retrieved_at_local"],
        "2026-11-02T10:15:00.000+09:00"
    );
}

#[tokio::test]
async fn continuation_stops_at_the_page_cap_and_reports_truncation() {
    let manifest = manifest();
    let prepared = prepared_for(
        &manifest,
        "domestic_stock.chk_holiday",
        &[("bass_dt", "20261102")],
        None,
    );
    let days = (0..MAX_KIS_PAGES + 2)
        .map(|index| format!("202611{:02}", index + 1))
        .collect::<Vec<_>>();
    let provider = FixturePages::new(
        days.iter()
            .map(|day| Ok(holiday_page(&[day.as_str()], day, "M")))
            .collect(),
    );
    let paced = std::sync::atomic::AtomicUsize::new(0);
    let pages = provider.collect(&prepared, &paced).await.unwrap();
    assert_eq!(pages.stop, KisPageStop::PageCap);
    assert_eq!(pages.pages.len(), MAX_KIS_PAGES);
    assert_eq!(provider.continuing().len(), MAX_KIS_PAGES);
    assert_eq!(
        paced.load(std::sync::atomic::Ordering::SeqCst),
        MAX_KIS_PAGES - 1
    );
    let result = normalize_kis_market_response(&prepared, &pages, None).unwrap();
    assert_eq!(result["page"]["truncated"], true);
    assert_eq!(result["page"]["returned"], MAX_KIS_PAGES);
    assert_eq!(result["page"]["stop_reason"], "page_cap");
    assert_eq!(result["warnings"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn continuation_stops_honestly_without_a_cursor_on_a_repeat_or_a_failed_page() {
    let manifest = manifest();
    let paced = std::sync::atomic::AtomicUsize::new(0);

    // CTS is not echoed in the response: no next request is possible.
    let loans = prepared_for(
        &manifest,
        "domestic_stock.daily_loan_trans",
        &[("mrkt_div_cls_code", "1"), ("mksc_shrn_iscd", "005930")],
        None,
    );
    let provider = FixturePages::new(vec![Ok(page(
        json!({"rt_cd": "0", "msg_cd": "MCA00000", "msg1": "ok",
               "output1": [{"bsop_date": "20261030", "rmnd_stcn": "100"}]}),
        "M",
        "2026-11-02T01:15:00.000Z",
    ))]);
    let pages = provider.collect(&loans, &paced).await.unwrap();
    assert_eq!(pages.stop, KisPageStop::MissingCursor);
    assert_eq!(provider.continuing(), vec![false]);
    let result = normalize_kis_market_response(&loans, &pages, None).unwrap();
    assert_eq!(result["page"]["truncated"], true);

    // Header-only continuation repeats the query with tr_cont N; an
    // identical page stops the loop instead of duplicating rows.
    let ranking = prepared_for(&manifest, "domestic_stock.volume_rank", &[], None);
    let ranked = page(
        json!({"rt_cd": "0", "msg_cd": "MCA00000", "msg1": "ok",
               "output": [{"mksc_shrn_iscd": "005930", "data_rank": "1"}]}),
        "M",
        "2026-11-02T01:15:00.000Z",
    );
    let provider = FixturePages::new(vec![Ok(ranked.clone()), Ok(ranked)]);
    let pages = provider.collect(&ranking, &paced).await.unwrap();
    assert_eq!(pages.stop, KisPageStop::RepeatedPage);
    assert_eq!(pages.pages.len(), 1);
    assert_eq!(provider.continuing(), vec![false, true]);
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(requests[0].0, requests[1].0);

    // A rate-limited later page keeps the earlier rows; a credential echo
    // (rejected inside fetch) still fails the whole read.
    let holidays = prepared_for(
        &manifest,
        "domestic_stock.chk_holiday",
        &[("bass_dt", "20261102")],
        None,
    );
    let provider = FixturePages::new(vec![
        Ok(holiday_page(&["20261102"], "next", "M")),
        Ok(page(
            json!({"rt_cd": "1", "msg_cd": "EGW00201", "msg1": "초당 거래건수를 초과하였습니다."}),
            "D",
            "2026-11-02T01:15:01.000Z",
        )),
    ]);
    let pages = provider.collect(&holidays, &paced).await.unwrap();
    assert_eq!(pages.stop, KisPageStop::LaterPageFailed);
    assert_eq!(pages.pages.len(), 1);
    let provider = FixturePages::new(vec![
        Ok(holiday_page(&["20261102"], "next", "M")),
        Err(FinanceDataError::InvalidResponse),
    ]);
    assert!(matches!(
        provider.collect(&holidays, &paced).await,
        Err(FinanceDataError::InvalidResponse)
    ));
}

#[tokio::test]
async fn multi_page_balances_import_every_position() {
    let manifest = manifest();
    let profile = account_only_profile();
    let query: KisToolQuery = serde_json::from_value(kis_holdings_query()).unwrap();
    let prepared = prepare_kis_request(
        &manifest,
        &query,
        KisEnvironment::Real,
        Some(&profile),
        KisRequestAuthority::AgentRead,
    )
    .unwrap();
    let balance = |symbol: &str, cursor: &str| {
        json!({"rt_cd": "0", "msg_cd": "MCA00000", "msg1": "ok",
               // Real cursors can embed the account number; they never reach the result.
               "ctx_area_fk100": cursor, "ctx_area_nk100": cursor,
               "output1": [{"pdno": symbol, "prdt_name": "종목", "hldg_qty": "1",
                            "pchs_avg_pric": "100", "evlu_amt": "100"}],
               "output2": [{"tot_evlu_amt": "200"}]})
    };
    let provider = FixturePages::new(vec![
        Ok(page(
            balance("005930", "12345678^01^005930"),
            "M",
            "2026-11-02T01:15:00.000Z",
        )),
        Ok(page(balance("000660", ""), "E", "2026-11-02T01:15:01.000Z")),
    ]);
    let paced = std::sync::atomic::AtomicUsize::new(0);
    let pages = provider.collect(&prepared, &paced).await.unwrap();
    assert_eq!(
        provider.query(1, "CTX_AREA_FK100").as_deref(),
        Some("12345678^01^005930")
    );
    let result = normalize_kis_account_response(&prepared, &pages, Some(&profile)).unwrap();
    assert!(!serde_json::to_string(&result).unwrap().contains("12345678"));
    assert_eq!(
        result["provenance"]["last_page_retrieved_at"],
        "2026-11-02T01:15:01.000Z"
    );
    let positions = kis_holdings_positions(&result).unwrap();
    assert_eq!(
        positions
            .iter()
            .map(|position| position.symbol.as_str())
            .collect::<Vec<_>>(),
        ["005930", "000660"]
    );
    // The account summary repeats on every page; it is one summary, not two.
    assert_eq!(
        result["data"]["response"]["output2"],
        json!([{"tot_evlu_amt": "200"}])
    );
}

#[tokio::test]
async fn continuation_keeps_a_per_request_summary_object_once() {
    let manifest = manifest();
    let prepared = prepared_for(
        &manifest,
        "domestic_stock.inquire_daily_indexchartprice",
        &[
            ("fid_cond_mrkt_div_code", "U"),
            ("fid_input_iscd", "0001"),
            ("fid_input_date_1", "20261001"),
            ("fid_input_date_2", "20261030"),
            ("fid_period_div_code", "D"),
        ],
        None,
    );
    let chart = |days: &[&str], index_close: &str| {
        json!({"rt_cd": "0", "msg_cd": "MCA00000", "msg1": "ok",
               "output1": {"hts_kor_isnm": "종합", "bstp_nmix_prpr": index_close},
               "output2": days.iter().map(|day| json!({"stck_bsop_date": day, "bstp_nmix_prpr": "2500.00"})).collect::<Vec<_>>()})
    };
    let provider = FixturePages::new(vec![
        Ok(page(
            chart(&["20261030", "20261029"], "2510.00"),
            "M",
            "2026-11-02T01:15:00.000Z",
        )),
        // A later page may restate the summary; the first page's stays.
        Ok(page(
            chart(&["20261028"], "2511.00"),
            "D",
            "2026-11-02T01:15:01.000Z",
        )),
    ]);
    let paced = std::sync::atomic::AtomicUsize::new(0);
    let pages = provider.collect(&prepared, &paced).await.unwrap();
    assert_eq!(pages.pages.len(), 2);
    let result = normalize_kis_market_response(&prepared, &pages, None).unwrap();
    let response = &result["data"]["response"];
    assert_eq!(
        response["output1"],
        json!({"hts_kor_isnm": "종합", "bstp_nmix_prpr": "2510.00"})
    );
    assert_eq!(
        response["output2"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["stck_bsop_date"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["20261030", "20261029", "20261028"]
    );
}

#[test]
fn krx_sessions_follow_seoul_trading_day_hours() {
    let at = |text: &str| chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap();
    // 2026-11-02 is a Monday; 2026-11-07 a Saturday; 2026-10-09 a KRX holiday (Hangeul Day).
    for (time, session) in [
        ("2026-11-02 07:59", "closed"),
        ("2026-11-02 08:00", "pre_market"),
        ("2026-11-02 08:59", "pre_market"),
        ("2026-11-02 09:00", "regular"),
        ("2026-11-02 15:29", "regular"),
        ("2026-11-02 15:30", "after_hours"),
        ("2026-11-02 17:59", "after_hours"),
        ("2026-11-02 18:00", "closed"),
        ("2026-11-07 10:00", "closed"),
        ("2026-10-09 10:00", "closed"),
    ] {
        assert_eq!(krx_equity_session(at(time)), session, "{time}");
    }
    // UTC retrieval converts to Seoul before labeling.
    let domestic = kis_market_time("domestic_stock", "2026-11-02T06:45:00Z");
    assert_eq!(domestic["session_at_retrieval"], "after_hours");
    assert_eq!(domestic["timezone"], "Asia/Seoul");
    let overseas = kis_market_time("overseas_stock", "2026-11-02T06:45:00Z");
    assert!(overseas["session_at_retrieval"].is_null());
    assert_eq!(overseas["timezone"], "Asia/Seoul");
}
