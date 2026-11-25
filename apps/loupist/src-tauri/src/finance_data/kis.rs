use super::*;

mod dividends;
mod manifest;
mod protocol;
mod symbols;

pub(super) use manifest::*;
pub(super) use protocol::*;

/// The app has one KIS connection. Cloned services and all of its turns share
/// the market and account GET clocks for that connection.
#[derive(Default)]
pub(super) struct KisReadPace {
    demo: Mutex<Option<tokio::time::Instant>>,
    real: Mutex<Option<tokio::time::Instant>>,
}

impl KisReadPace {
    async fn wait(&self, environment: KisEnvironment) {
        let (clock, interval) = match environment {
            KisEnvironment::Demo => (&self.demo, Duration::from_secs(1)),
            KisEnvironment::Real => (&self.real, Duration::from_millis(500)),
        };
        let mut next = clock.lock().await;
        if let Some(next) = *next {
            tokio::time::sleep_until(next).await;
        }
        *next = Some(tokio::time::Instant::now() + interval);
    }
}

struct KisGet<'a> {
    url: Url,
    tr_id: &'a str,
    continuation: Option<(&'a str, &'static str)>,
}

/// Native-only values required by KIS account and profile-scoped reads.
///
/// This type intentionally has no `Debug` or serialization implementation.
/// React and Pi cannot construct it; the broker may derive it only from the
/// active credential bundle already verified by the native Marketplace flow.
pub(crate) struct KisAccountProfile {
    values: BTreeMap<String, String>,
}

impl KisAccountProfile {
    pub(crate) fn from_values(values: BTreeMap<String, String>) -> Result<Self, FinanceDataError> {
        const KEYS: [&str; 6] = [
            "account_number",
            "account_product_code",
            "account_password",
            "customer_identity_number",
            "home_net_id",
            "hts_id",
        ];
        if values.is_empty()
            || values.len() > KEYS.len()
            || values.keys().any(|key| !KEYS.contains(&key.as_str()))
            || values.values().any(|value| {
                value.len() > 128 || value.chars().any(|character| character.is_control())
            })
            || values.get("account_number").is_some_and(|value| {
                value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit())
            })
            || values.get("account_product_code").is_some_and(|value| {
                value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Err(FinanceDataError::InvalidQuery(
                "KIS account profile is invalid",
            ));
        }
        Ok(Self { values })
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn has_required_values_for(&self, operation: &KisOperation) -> bool {
        operation.parameters.iter().all(|parameter| {
            if parameter.source != KisParameterSource::AccountProfile || !parameter.required {
                return true;
            }
            parameter
                .profile_key
                .as_deref()
                .and_then(|key| self.get(key))
                .or_else(|| parameter.default.as_ref().and_then(Value::as_str))
                .is_some_and(|value| !value.is_empty())
        })
    }

    fn sensitive_echo_values(&self) -> impl Iterator<Item = &str> {
        self.values.iter().filter_map(|(key, value)| {
            (key != "account_product_code" && value.len() >= 4).then_some(value.as_str())
        })
    }
}
impl FinanceDataService {
    /// How the reviewed catalog treats one KIS operation (account-field redaction).
    pub(crate) fn kis_account_review(
        &self,
        operation_id: &str,
    ) -> Option<crate::privacy::account_fields::AccountReview> {
        use crate::privacy::account_fields::AccountReview;
        let operation = self.kis_manifest.operation(operation_id)?;
        Some(match operation.scope {
            KisOperationScope::Market => AccountReview::Market,
            KisOperationScope::Account => match &operation.holding_fields {
                Some(fields) => AccountReview::Account(fields.clone()),
                None => AccountReview::Unreviewed,
            },
        })
    }

    pub(crate) fn kis_operation_search(
        &self,
        params: Value,
        environment: &str,
        profile: Option<&KisAccountProfile>,
    ) -> Result<Value, FinanceDataError> {
        let environment = KisEnvironment::parse(environment)?;
        let object = params.as_object().ok_or(FinanceDataError::InvalidQuery(
            "expected the KIS operation search schema",
        ))?;
        if object.keys().any(|key| {
            !matches!(key.as_str(), "query" | "product" | "limit")
                || !object.get(key).is_some_and(Value::is_string)
        }) {
            return Err(FinanceDataError::InvalidQuery(
                "expected the KIS operation search schema",
            ));
        }
        let query = object
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if query.len() > 128 || query.chars().any(char::is_control) {
            return Err(FinanceDataError::InvalidQuery(
                "KIS operation search text is invalid",
            ));
        }
        let product = object
            .get("product")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        if product.is_some_and(|value| {
            value.len() > 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        }) {
            return Err(FinanceDataError::InvalidQuery(
                "KIS operation search product is invalid",
            ));
        }
        let limit = object
            .get("limit")
            .and_then(Value::as_str)
            .unwrap_or("10")
            .parse::<usize>()
            .ok()
            .filter(|limit| (1..=MAX_KIS_OPERATION_SEARCH_RESULTS).contains(limit))
            .ok_or(FinanceDataError::InvalidQuery(
                "KIS operation search limit must be from 1 through 20",
            ))?;
        let query_terms = query
            .split_whitespace()
            .map(|term| term.to_lowercase())
            .collect::<Vec<_>>();
        let matches = self
            .kis_manifest
            .operations
            .iter()
            .filter(|operation| {
                operation.available_in(environment) && operation.available_to_agent(profile)
            })
            .filter(|operation| product.is_none_or(|product| operation.product == product))
            .filter(|operation| {
                let haystack = format!(
                    "{} {} {} {}",
                    operation.id, operation.product, operation.category, operation.name
                )
                .to_lowercase();
                query_terms.iter().all(|term| haystack.contains(term))
            })
            .take(limit)
            .map(kis_operation_descriptor)
            .collect::<Vec<_>>();
        let returned = matches.len();
        let account_reads = self.kis_manifest.agent_scope_operation_count(
            environment,
            profile,
            KisOperationScope::Account,
        );
        Ok(json!({
            "schema_version": "loupist-kis-operation-search/1",
            "source_id": KIS_SOURCE_ID,
            "query": query,
            "product": product,
            "operations": matches,
            "page": {
                "returned": returned,
                "limit": limit
            },
            "policy": {
                "public_reads_available_v1": self.kis_manifest.public_operation_count(environment),
                "profile_gated_market_reads": self.kis_manifest.profile_gated_market_operation_count(environment),
                "account_reads_available_v1": account_reads,
                "account_reads_need": (account_reads == 0).then_some(
                    "Account and balance reads need the optional account profile on the Korea Investment connection in Settings; until then only public market reads are available."
                ),
                "agent_reads_available": self.kis_manifest.agent_operation_count(environment, profile),
                "orders_available": false
            }
        }))
    }

    pub(crate) async fn clear_kis_token_cache(&self) {
        self.invalidate_kis_token_cache();
        drop(self.kis_token_cache().await);
    }

    /// Configuration reconciliation is synchronous. Mark the cache unusable
    /// immediately; the next async access clears it while holding its lock.
    pub(crate) fn invalidate_kis_token_cache(&self) {
        self.kis_tokens_invalidated
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn kis_token_cache(&self) -> tokio::sync::MutexGuard<'_, KisTokenCache> {
        let mut cache = self.kis_tokens.lock().await;
        if self
            .kis_tokens_invalidated
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            cache.tokens.clear();
        }
        cache
    }

    pub(crate) async fn kis_read_data(
        &self,
        params: Value,
        environment: &str,
        app_key: &str,
        app_secret: &str,
        profile: Option<&KisAccountProfile>,
    ) -> Result<Value, FinanceDataError> {
        let environment = KisEnvironment::parse(environment)?;
        let query: KisToolQuery = strict_query(params, "expected the exact KIS read schema")?;
        // Reject operations and parameters at the native authority boundary
        // before credentials are used for a provider token request.
        let prepared = prepare_kis_request(
            &self.kis_manifest,
            &query,
            environment,
            profile,
            KisRequestAuthority::AgentRead,
        )?;
        let fetch = |url, continuing| {
            let continuation = prepared
                .operation
                .continuation
                .as_ref()
                .and_then(|spec| spec.request_header.as_deref())
                .map(|header| (header, if continuing { "N" } else { "" }));
            self.fetch_kis_get(
                KisGet {
                    url,
                    tr_id: prepared.tr_id,
                    continuation,
                },
                environment,
                app_key,
                app_secret,
            )
        };
        let pages = collect_kis_pages(&prepared, fetch).await?;
        match prepared.operation.scope {
            KisOperationScope::Market => normalize_kis_market_response(&prepared, &pages, profile),
            KisOperationScope::Account => {
                normalize_kis_account_response(&prepared, &pages, profile)
            }
        }
    }

    async fn fetch_kis_with_token<F, Fut>(
        &self,
        environment: KisEnvironment,
        app_key: &str,
        app_secret: &str,
        mut fetch: F,
    ) -> Result<RawProviderResponse, FinanceDataError>
    where
        F: FnMut(KisAccessToken) -> Fut,
        Fut: std::future::Future<Output = Result<RawProviderResponse, FinanceDataError>>,
    {
        let fingerprint = kis_credential_fingerprint(environment, app_key, app_secret);
        let mut may_retry = true;
        loop {
            let token = self
                .kis_access_token(environment, app_key, app_secret)
                .await?;
            let result = fetch(token.clone()).await;
            if !matches!(
                &result,
                Err(FinanceDataError::KisRead(KisReadError::TokenRejected))
            ) {
                return result;
            }
            let mut cache = self.kis_token_cache().await;
            if cache
                .tokens
                .get(&fingerprint)
                .is_some_and(|cached| cached.value == token.value)
            {
                cache.tokens.remove(&fingerprint);
            }
            if !may_retry {
                return result;
            }
            may_retry = false;
        }
    }

    async fn fetch_kis_get(
        &self,
        get: KisGet<'_>,
        environment: KisEnvironment,
        app_key: &str,
        app_secret: &str,
    ) -> Result<RawProviderResponse, FinanceDataError> {
        self.fetch_kis_with_token(environment, app_key, app_secret, |token| {
            let mut request = self
                .client
                .get(get.url.clone())
                .bearer_auth(&token.value)
                .header("appkey", app_key)
                .header("appsecret", app_secret)
                .header("tr_id", get.tr_id)
                .header("custtype", "P");
            if let Some((header, value)) = get.continuation {
                request = request.header(header, value);
            }
            let mut source_url = get.url.clone();
            source_url.set_query(None);
            async move {
                self.kis_read_pace.wait(token.environment).await;
                let (status, raw) = self
                    .send_provider_bounded(
                        request,
                        KIS_SOURCE_ID,
                        source_url.into(),
                        MAX_KIS_RESPONSE_BYTES,
                    )
                    .await?;
                let raw = interpret_kis_read_status(status, raw)?;
                reject_kis_credential_echo(&raw.bytes, app_key, app_secret, &token.value)?;
                Ok(raw)
            }
        })
        .await
    }

    async fn kis_access_token(
        &self,
        environment: KisEnvironment,
        app_key: &str,
        app_secret: &str,
    ) -> Result<KisAccessToken, FinanceDataError> {
        validate_api_key(app_key)?;
        validate_api_key(app_secret)?;
        let fingerprint = kis_credential_fingerprint(environment, app_key, app_secret);
        let mut cache = self.kis_token_cache().await;
        if let Some(token) = cache
            .tokens
            .get(&fingerprint)
            .filter(|token| token.expires_at > Utc::now() + ChronoDuration::seconds(30))
        {
            return Ok(token.clone());
        }
        cache.tokens.remove(&fingerprint);
        // Keep issuance serialized across reads and credential verification.
        let token = self
            .issue_kis_token_locked(environment, app_key, app_secret, &mut cache)
            .await?;
        if cache.tokens.len() >= MAX_KIS_TOKEN_CACHE_ENTRIES {
            cache.tokens.clear();
        }
        cache.tokens.insert(fingerprint, token.clone());
        Ok(token)
    }

    async fn issue_kis_token_locked(
        &self,
        environment: KisEnvironment,
        app_key: &str,
        app_secret: &str,
        cache: &mut KisTokenCache,
    ) -> Result<KisAccessToken, FinanceDataError> {
        #[cfg(feature = "eval-harness")]
        if crate::eval_harness::replay::enabled() {
            return Ok(KisAccessToken {
                value: "eval-placeholder".into(),
                environment,
                expires_at: Utc::now() + ChronoDuration::hours(1),
            });
        }
        let fingerprint = kis_credential_fingerprint(environment, app_key, app_secret);
        let now = tokio::time::Instant::now();
        cache.issuance_blocked_until.retain(|_, until| *until > now);
        if cache.issuance_blocked_until.contains_key(&fingerprint) {
            return Err(FinanceDataError::KisRateLimited(
                SafeProviderDiagnostic::bounded("EGW00133", "1분당 1회만 발급 가능합니다."),
            ));
        }
        let mut url =
            Url::parse(environment.api_root()).map_err(|_| FinanceDataError::InvalidResponse)?;
        url.set_path(KIS_TOKEN_PATH);
        url.set_query(None);
        cache
            .issuance_blocked_until
            .insert(fingerprint, now + KIS_TOKEN_ISSUANCE_COOLDOWN);
        let (status, raw) = self
            .send_provider_bounded(
                self.client.post(url.clone()).json(&json!({
                    "grant_type": "client_credentials",
                    "appkey": app_key,
                    "appsecret": app_secret
                })),
                KIS_SOURCE_ID,
                url.to_string(),
                64 * 1024,
            )
            .await?;
        if !status.is_success() {
            return Err(classify_kis_token_error(
                status, &raw.bytes, app_key, app_secret,
            ));
        }
        parse_kis_token_response(&raw.bytes, environment, app_key, app_secret)
    }
}

#[cfg(test)]
mod tests;
