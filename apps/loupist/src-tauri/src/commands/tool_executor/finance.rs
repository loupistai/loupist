use super::*;
use futures_util::future::BoxFuture;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

const OPENDART: &str = "opendart.disclosures";
const KRX: &str = "krx.market-data";
const WORLD_BANK: &str = "world-bank.indicators";
const DEFILLAMA: &str = "defillama.tvl";

/// Shared cap for parallel connection reads. Pi already issues tool calls
/// concurrently; this keeps one provider from being stampeded.
pub(crate) struct ProviderPace {
    capacity: Arc<Semaphore>,
    clocks: HashMap<String, Arc<Mutex<Instant>>>,
}

impl Default for ProviderPace {
    fn default() -> Self {
        Self {
            capacity: Arc::new(Semaphore::new(2)),
            clocks: HashMap::new(),
        }
    }
}

impl ProviderPace {
    async fn handles(pace: &Mutex<Self>, provider: &str) -> (Arc<Semaphore>, Arc<Mutex<Instant>>) {
        let mut state = pace.lock().await;
        let clock = state
            .clocks
            .entry(provider.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(Instant::now())))
            .clone();
        (state.capacity.clone(), clock)
    }

    async fn space(clock: &Mutex<Instant>) {
        let mut next = clock.lock().await;
        let now = Instant::now();
        if *next > now {
            tokio::time::sleep(*next - now).await;
        }
        // Conservative floor; connector-side rate limits still apply.
        *next = Instant::now() + Duration::from_millis(500);
    }

    async fn acquire(
        pace: &Mutex<Self>,
        provider: &str,
    ) -> Result<OwnedSemaphorePermit, BrokerError> {
        let (capacity, clock) = Self::handles(pace, provider).await;
        let permit = capacity
            .acquire_owned()
            .await
            .map_err(|_| BrokerError::Execution("finance reads are closed".into()))?;
        Self::space(&clock).await;
        Ok(permit)
    }

    /// Spacing for an additional request inside a call that already holds
    /// its concurrency permit (KRX range dates, KIS continuation pages).
    async fn wait(pace: &Mutex<Self>, provider: &'static str) {
        let (_, clock) = Self::handles(pace, provider).await;
        Self::space(&clock).await;
    }
}

/// Provider failures keep their text; a read that never reached the provider
/// becomes the typed `offline` category (that provider was unreachable) the
/// renderer can surface.
fn finance_error(error: crate::finance_data::FinanceDataError) -> BrokerError {
    match error {
        crate::finance_data::FinanceDataError::Offline(provider) => {
            BrokerError::Offline(provider.to_owned())
        }
        crate::finance_data::FinanceDataError::ServiceApprovalRequired { provider, services } => {
            BrokerError::ProviderApprovalRequired {
                provider: provider.to_owned(),
                services: services.into_iter().map(str::to_owned).collect(),
            }
        }
        error => BrokerError::Execution(error.to_string()),
    }
}

fn dart_filing_code(value: &str) -> Result<&'static str, BrokerError> {
    match value {
        "periodic" => Ok("A"),
        "major_event" => Ok("B"),
        "issuance" => Ok("C"),
        "ownership" => Ok("D"),
        "other" => Ok("E"),
        "audit" => Ok("F"),
        "fund" => Ok("G"),
        "securitization" => Ok("H"),
        "exchange" => Ok("I"),
        "fair_trade" => Ok("J"),
        _ => Err(BrokerError::invalid("filing_type", "expected periodic, major_event, issuance, ownership, other, audit, fund, securitization, exchange, or fair_trade")),
    }
}

fn company_candidates(matches: &[Value]) -> String {
    matches
        .iter()
        .take(8)
        .map(|item| {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let code = item.get("corp_code").and_then(Value::as_str).unwrap_or("?");
            match item
                .get("symbol")
                .and_then(Value::as_str)
                .filter(|symbol| !symbol.is_empty())
            {
                Some(symbol) => format!("{name} ({code}, {symbol})"),
                None => format!("{name} ({code})"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn company_error(company: &str, matches: &[Value]) -> BrokerError {
    if matches.is_empty() {
        return BrokerError::Execution(format!(
            "No OpenDART company matched {company}. Use dart_company_lookup with a broader name."
        ));
    }
    BrokerError::Execution(format!(
        "OpenDART company {company} is ambiguous. Pass one 8-digit corp_code. Candidates: {}",
        company_candidates(matches)
    ))
}

fn select_corp_code(company: &str, matches: &[Value]) -> Result<String, BrokerError> {
    let grade = if company.len() == 6 && company.bytes().all(|byte| byte.is_ascii_digit()) {
        "ticker_exact"
    } else {
        "name_exact"
    };
    let exact = matches
        .iter()
        .filter(|item| item.get("match_grade").and_then(Value::as_str) == Some(grade))
        .cloned()
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return exact[0]
            .get("corp_code")
            .and_then(Value::as_str)
            .filter(|code| code.len() == 8 && code.bytes().all(|byte| byte.is_ascii_digit()))
            .map(str::to_owned)
            .ok_or_else(|| BrokerError::Execution("OpenDART match has no corp_code".into()));
    }
    if exact.is_empty() {
        Err(company_error(company, matches))
    } else {
        Err(company_error(company, &exact))
    }
}

impl AppToolExecutor {
    async fn pace(&self, provider: &str) -> Result<OwnedSemaphorePermit, BrokerError> {
        ProviderPace::acquire(&self.capture.provider_pace, provider).await
    }

    fn request_pacer<'a>(
        &'a self,
        provider: &'static str,
    ) -> impl Fn() -> BoxFuture<'a, ()> + Send + Sync + 'a {
        let pace = &self.capture.provider_pace;
        move || Box::pin(ProviderPace::wait(pace, provider))
    }

    async fn resolve_dart_company(&self, company: &str) -> Result<String, BrokerError> {
        let company = company.trim();
        if company.len() == 8 && company.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(company.to_owned());
        }
        if company.is_empty() || company.len() > 200 {
            return Err(BrokerError::invalid(
                "company",
                "expected a company name, ticker, or 8-digit corp_code (1–200 bytes)",
            ));
        }
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let discovery = self
            .state
            .finance_data
            .resolve_entity(company, 8, Some(credential.as_str()))
            .await
            .map_err(finance_error)?;
        drop(credential);
        let matches = discovery
            .get("matches")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        select_corp_code(company, &matches)
    }

    pub(super) async fn dart_company_lookup(&self, params: Value) -> Result<Value, BrokerError> {
        let object = exact_object(&params, &["query"], &["limit"])?;
        let query = object
            .get("query")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 200)
            .ok_or_else(|| {
                BrokerError::invalid(
                    "query",
                    "expected a nonempty company name or ticker up to 200 bytes",
                )
            })?;
        let limit = object.get("limit").and_then(Value::as_u64).unwrap_or(8);
        if !(1..=20).contains(&limit) {
            return Err(BrokerError::invalid(
                "limit",
                "expected an integer from 1 to 20",
            ));
        }
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let result = self
            .state
            .finance_data
            .resolve_entity(query, limit as usize, Some(credential.as_str()))
            .await
            .map_err(finance_error);
        drop(credential);
        result
    }

    pub(super) async fn run_results_list(&self, params: Value) -> Result<Value, BrokerError> {
        exact_object(&params, &[], &[])?;
        let registry = self.capture.run_results.lock().await;
        Ok(json!({
            "results": registry.values().map(|result| json!({
                "result_ref": result.result_ref,
                "citation_url": super::evidence::citation_url(&result.result_ref),
                "dataset_ref": result.payload.get("dataset_ref"),
                "runtime_id": result.producer.runtime_id,
                "tool_name": result.producer.tool_name,
                "origin": result.producer.origin,
                "source_save_allowed": matches!(result.producer.origin, RunResultOrigin::AcquiredOriginal | RunResultOrigin::Discovery | RunResultOrigin::WorkspaceFile),
                "provider": result.producer.provider,
                "request_digest": result.request_digest,
                "response_digest": result.response_digest,
                "retrieved_at": result.retrieved_at,
                "warnings": result.warnings,
                "upstream_result_refs": result.upstream_result_refs,
            })).collect::<Vec<_>>()
        }))
    }

    pub(super) async fn provider_credential_bundle(
        &self,
        entry_id: &'static str,
    ) -> Result<crate::finance_credentials::ActiveCredentialBundle, BrokerError> {
        self.require_capability(entry_id)?;
        let credential_environment = self.state.credential_environment.clone();
        let credential = tokio::task::spawn_blocking(move || {
            crate::finance_credentials::get(&credential_environment, entry_id)
        })
        .await
        .map_err(|_| BrokerError::Execution("credential lookup failed".into()))?
        .map_err(|_| BrokerError::Execution("credential lookup failed".into()))?;
        credential.ok_or_else(|| {
            BrokerError::Execution(
                "the enabled finance connection has no configured credential".into(),
            )
            .with_next(
                "Ask the user to configure this connection in Connections, then start a new turn.",
            )
        })
    }

    pub(super) async fn provider_credential(
        &self,
        entry_id: &'static str,
    ) -> Result<String, BrokerError> {
        let credential = self.provider_credential_bundle(entry_id).await?;
        credential
            .get("api_key")
            .map(str::to_owned)
            .ok_or_else(|| BrokerError::Execution("credential lookup failed".into()))
    }

    /// Which KIS credential revision, account and environment would serve a
    /// read, as a digest for reuse keys. `None` when no credential is usable;
    /// the read itself then reports why.
    pub(super) async fn kis_reuse_identity(&self) -> Option<String> {
        let credential = self
            .provider_credential_bundle(crate::finance_data::KIS_SOURCE_ID)
            .await
            .ok()?;
        let environment = crate::marketplace::connector_config_value(
            &self.state,
            crate::finance_data::KIS_SOURCE_ID,
            "environment",
        )
        .ok()
        .flatten();
        let identity = json!({
            "schema": "kis-reuse-identity/1",
            "revision": credential.revision(),
            "environment": environment,
            "app_key": credential.get("app_key"),
            "account_number": credential.get("account_number"),
            "account_product_code": credential.get("account_product_code"),
        });
        Some(sha256(identity.to_string().as_bytes()))
    }

    pub(super) fn kis_account_profile(
        credential: &crate::finance_credentials::ActiveCredentialBundle,
    ) -> Result<Option<crate::finance_data::KisAccountProfile>, BrokerError> {
        let values = [
            "account_number",
            "account_product_code",
            "account_password",
            "customer_identity_number",
            "home_net_id",
            "hts_id",
        ]
        .into_iter()
        .filter_map(|key| {
            credential
                .get(key)
                .map(|value| (key.to_owned(), value.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
        if values.is_empty() {
            return Ok(None);
        }
        crate::finance_data::KisAccountProfile::from_values(values)
            .map(Some)
            .map_err(|_| BrokerError::Execution("KIS account profile is unavailable".into()))
    }

    pub(super) async fn worldbank_series(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        exact_object(
            &params,
            &["economy", "indicator", "start_year", "end_year"],
            &[],
        )?;
        self.require_capability(WORLD_BANK)?;
        if let Some(object) = params.as_object_mut() {
            object.insert("provider".into(), json!(WORLD_BANK));
        }
        let _permit = self.pace(WORLD_BANK).await?;
        let mut output = self
            .state
            .finance_data
            .macro_data(params)
            .await
            .map_err(finance_error)?;
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "worldbank_series", WORLD_BANK)
    }

    /// Dated daily DefiLlama rows; a day's value is known at its end, so `as_of`
    /// keeps rows on or before the cutoff like other daily series.
    pub(super) async fn defillama_read(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        exact_object(&params, &["route", "id"], &["start", "end"])?;
        self.require_capability(DEFILLAMA)?;
        let _permit = self.pace(DEFILLAMA).await?;
        let mut output = self
            .state
            .finance_data
            .defillama_read(params)
            .await
            .map_err(finance_error)?;
        if let Some(cutoff) = cutoff {
            apply_observation_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "defillama_read", DEFILLAMA)
    }

    pub(super) async fn krx_daily_quote(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        exact_object(&params, &["symbol"], &["date", "start", "end", "market"])?;
        let credential = self.provider_credential(KRX).await?;
        let _permit = self.pace(KRX).await?;
        let mut output = self
            .state
            .finance_data
            .krx_market_data(params, &credential, &self.request_pacer(KRX))
            .await
            .map_err(|error| match error {
                crate::finance_data::FinanceDataError::NoData(_) => finance_error(error).with_next(
                    "No KRX row matched. Weekends, exchange holidays and days before next-day publication have no rows; auto mode checks KOSPI then KOSDAQ. Check the 6-digit code or the dates.",
                ),
                crate::finance_data::FinanceDataError::CredentialRejected(_) => {
                    finance_error(error).with_next(
                        "KRX approves each API service separately. Ask the user to check the key and its approvals for the KOSPI and KOSDAQ daily trading services (유가증권/코스닥 일별매매정보), or pass market for the approved one.",
                    )
                }
                error => finance_error(error),
            })?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            // A daily bar is known at the close of its date.
            apply_observation_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "krx_daily_quote", KRX)
    }

    pub(super) async fn kis_operation_search(&self, params: Value) -> Result<Value, BrokerError> {
        let object = exact_object(&params, &["query"], &["product", "limit"])?;
        let mut search = object.clone();
        if let Some(limit) = search.get("limit").and_then(Value::as_u64) {
            if !(1..=20).contains(&limit) {
                return Err(BrokerError::invalid(
                    "limit",
                    "expected an integer from 1 to 20",
                ));
            }
            search["limit"] = json!(limit.to_string());
        }
        self.require_capability(crate::finance_data::KIS_SOURCE_ID)?;
        let credential = self
            .provider_credential_bundle(crate::finance_data::KIS_SOURCE_ID)
            .await?;
        let profile = Self::kis_account_profile(&credential)?;
        let result = self
            .state
            .finance_data
            .kis_operation_search(Value::Object(search), profile.as_ref())
            .map_err(finance_error);
        drop(credential);
        result
    }

    pub(super) async fn kis_read(
        &self,
        policy: &ToolPolicy,
        params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        if cutoff.is_some() {
            return Err(BrokerError::Execution(
                "as_of_unsupported: this KIS read endpoint cannot verify publication at a historical as-of cutoff"
                    .into(),
            ));
        }
        let mut params = params;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(&params, &["operation_id", "params"], &[])?;
        let operation_id = object
            .get("operation_id")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        if operation_id == "catalog.search" {
            return Err(BrokerError::Execution(
                "use kis_operation_search to find an operation".into(),
            ));
        }
        let operation_params = object
            .get("params")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(BrokerError::Malformed)?;
        // An account read nobody reviewed for holdings is never made (D9).
        let review = self.state.finance_data.kis_account_review(operation_id);
        if review == Some(crate::privacy::holdings::AccountReview::Unreviewed) {
            return Err(BrokerError::Execution(
                crate::privacy::holdings::UNREVIEWED.into(),
            ));
        }
        self.require_capability(crate::finance_data::KIS_SOURCE_ID)?;
        let credential = self
            .provider_credential_bundle(crate::finance_data::KIS_SOURCE_ID)
            .await?;
        let profile = Self::kis_account_profile(&credential)?;
        let environment = crate::marketplace::connector_config_value(
            &self.state,
            crate::finance_data::KIS_SOURCE_ID,
            "environment",
        )
        .map_err(|_| BrokerError::Execution("KIS configuration is unavailable".into()))?
        .ok_or_else(|| BrokerError::Execution("the KIS connector needs an environment".into()))?;
        let app_key = credential
            .get("app_key")
            .map(str::to_owned)
            .ok_or_else(|| BrokerError::Execution("credential lookup failed".into()))?;
        let app_secret = credential
            .get("app_secret")
            .map(str::to_owned)
            .ok_or_else(|| BrokerError::Execution("credential lookup failed".into()))?;
        let query = json!({
            "operation_id": operation_id,
            "params": operation_params
        });
        let _permit = self.pace(crate::finance_data::KIS_SOURCE_ID).await?;
        let output = self
            .state
            .finance_data
            .kis_read_data(
                query,
                &environment,
                &app_key,
                &app_secret,
                profile.as_ref(),
                &self.request_pacer(crate::finance_data::KIS_SOURCE_ID),
            )
            .await
            .map_err(finance_error)?;
        drop((credential, app_key, app_secret));
        // Quantities, amounts, prices paid and their rates stay here: hidden
        // before the result reaches Pi, a dataset, a result ref or an export.
        let mut output = output;
        if let Some(crate::privacy::holdings::AccountReview::Account(fields)) = review {
            crate::privacy::holdings::redact_fields(&mut output, &fields);
        }
        Ok(output)
    }

    pub(super) async fn dart_financials(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(
            &params,
            &["company", "fiscal_year", "report_period", "basis"],
            &[],
        )?;
        let company = object
            .get("company")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        let corp_code = self.resolve_dart_company(company).await?;
        let query = json!({
            "corp_code": corp_code,
            "fiscal_year": object.get("fiscal_year").cloned().unwrap_or(Value::Null),
            "report_period": object.get("report_period").cloned().unwrap_or(Value::Null),
            "basis": object.get("basis").cloned().unwrap_or(Value::Null),
        });
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let consolidated = query["basis"] == "consolidated";
        let mut output = self
            .state
            .finance_data
            .opendart_company_facts(query, &credential, &self.request_pacer(OPENDART))
            .await
            .map_err(|error| match error {
                crate::finance_data::FinanceDataError::NoData(_) if consolidated => {
                    finance_error(error).with_next(
                        "Companies without subsidiaries file no consolidated statements; retry with basis separate and say so.",
                    )
                }
                error => finance_error(error),
            })?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "dart_financials", OPENDART)
    }

    pub(super) async fn dart_filing_search(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(
            &params,
            &["company", "start", "end", "limit"],
            &["filing_type"],
        )?;
        let company = object
            .get("company")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        let forms = match object.get("filing_type").and_then(Value::as_str) {
            Some(value) => vec![dart_filing_code(value)?],
            None => Vec::new(),
        };
        let corp_code = self.resolve_dart_company(company).await?;
        let query = json!({
            "corp_code": corp_code,
            "start": object.get("start").cloned().unwrap_or(Value::Null),
            "end": object.get("end").cloned().unwrap_or(Value::Null),
            "forms": forms,
            "limit": object.get("limit").cloned().unwrap_or(Value::Null),
        });
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let mut output = self
            .state
            .finance_data
            .opendart_filing_search(query, &credential)
            .await
            .map_err(finance_error)?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "dart_filing_search", OPENDART)
    }

    pub(super) async fn dart_filing_read(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(&params, &["rcept_no"], &[])?;
        let query = json!({
            "rcept_no": object.get("rcept_no").cloned().unwrap_or(Value::Null),
        });
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let mut output = self
            .state
            .finance_data
            .opendart_filing_read(query, &credential)
            .await
            .map_err(finance_error)?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "dart_filing_read", OPENDART)
    }

    /// One kind of capital decision (주요사항보고) filed in a date range.
    pub(super) async fn dart_major_report(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(&params, &["company", "kind", "start", "end"], &[])?;
        let company = object
            .get("company")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        let corp_code = self.resolve_dart_company(company).await?;
        let query = json!({
            "corp_code": corp_code,
            "kind": object.get("kind").cloned().unwrap_or(Value::Null),
            "start": object.get("start").cloned().unwrap_or(Value::Null),
            "end": object.get("end").cloned().unwrap_or(Value::Null),
        });
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let mut output = self
            .state
            .finance_data
            .opendart_major_report(query, &credential)
            .await
            .map_err(finance_error)?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "dart_major_report", OPENDART)
    }

    /// 5% holders' or insiders' ownership reports, newest first.
    pub(super) async fn dart_ownership(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(&params, &["company", "kind"], &["limit"])?;
        let company = object
            .get("company")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        let corp_code = self.resolve_dart_company(company).await?;
        let query = json!({
            "corp_code": corp_code,
            "kind": object.get("kind").cloned().unwrap_or(Value::Null),
            "limit": object.get("limit").cloned().unwrap_or(json!(20)),
        });
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let mut output = self
            .state
            .finance_data
            .opendart_ownership(query, &credential)
            .await
            .map_err(finance_error)?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
        }
        self.validate_provider_tool_output(output, "dart_ownership", OPENDART)
    }

    /// A year's dividends from the annual report and the record date from
    /// the decision notice; under a cutoff a later notice is left out.
    pub(super) async fn dart_dividends(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
    ) -> Result<Value, BrokerError> {
        let cutoff = effective_as_of(policy, &params)?;
        let _ = take_as_of(&mut params)?;
        let object = exact_object(&params, &["company", "year"], &[])?;
        let company = object
            .get("company")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        let corp_code = self.resolve_dart_company(company).await?;
        let query = json!({
            "corp_code": corp_code,
            "year": object.get("year").cloned().unwrap_or(Value::Null),
        });
        let credential = self.provider_credential(OPENDART).await?;
        let _permit = self.pace(OPENDART).await?;
        let mut output = self
            .state
            .finance_data
            .opendart_dividends(query, &credential, &self.request_pacer(OPENDART))
            .await
            .map_err(finance_error)?;
        drop(credential);
        if let Some(cutoff) = cutoff {
            apply_as_of_cutoff(&mut output, cutoff)?;
            leave_out_later_notice(&mut output, cutoff);
        }
        self.validate_provider_tool_output(output, "dart_dividends", OPENDART)
    }
}

/// A dividend notice filed after the as-of date was not known then.
fn leave_out_later_notice(output: &mut Value, cutoff: chrono::NaiveDate) {
    let Some(notice) = output.pointer_mut("/data/record_date") else {
        return;
    };
    let later = notice
        .get("filed_at")
        .and_then(Value::as_str)
        .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .is_some_and(|filed| filed > cutoff);
    if later {
        *notice = json!({"status": "no_notice", "date": null});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(name: &str, symbol: Option<&str>, corp_code: &str, grade: &str) -> Value {
        json!({
            "name": name,
            "symbol": symbol,
            "corp_code": corp_code,
            "match_grade": grade
        })
    }

    #[test]
    fn ticker_and_exact_name_resolve_and_ambiguity_lists_candidates() {
        let matches = vec![
            matched("삼성전자", Some("005930"), "00126380", "ticker_exact"),
            matched("삼성전자", Some("005930"), "00126380", "name_exact"),
            matched("삼성물산", Some("028260"), "00126362", "name_contains"),
        ];
        assert_eq!(select_corp_code("005930", &matches).unwrap(), "00126380");
        assert_eq!(select_corp_code("삼성전자", &matches).unwrap(), "00126380");
        let ambiguous = select_corp_code(
            "삼성",
            &[
                matched("삼성전자", Some("005930"), "00126380", "name_contains"),
                matched("삼성물산", Some("028260"), "00126362", "name_contains"),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(ambiguous.contains("00126380"));
        assert!(ambiguous.contains("00126362"));
    }
}
