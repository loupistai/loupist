//! Replay is opt-in, fail-closed, and installed only by the serial runner.
//! Unit tests scope it to their Tokio runtime so unrelated tests and their
//! local HTTP servers cannot be intercepted by another test's fixture.
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock, RwLock};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub schema: String,
    pub requests: Vec<Entry>,
    #[serde(default)]
    pub otherwise: Vec<Fallback>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fallback {
    pub provider: String,
    pub url_prefix: String,
    pub response: Response,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub provider: String,
    pub request: Value,
    #[serde(default)]
    pub query_match: BTreeMap<String, QueryMatch>,
    pub response: Response,
}
/// Explicit exceptions to exact URL-query matching; other fields stay exact.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryMatch {
    Ignore,
    Optional,
    AtMost(String),
    AtLeast(String),
    RangeIncludes { start: i32, end: i32 },
}
impl Entry {
    fn matches(&self, request: &Value) -> bool {
        if self.query_match.is_empty() {
            return self.request == *request;
        }
        let urls = self
            .request
            .get("url")
            .and_then(Value::as_str)
            .zip(request.get("url").and_then(Value::as_str))
            .and_then(|(a, b)| Some((reqwest::Url::parse(a).ok()?, reqwest::Url::parse(b).ok()?)));
        let Some((mut expected, mut actual)) = urls else {
            return false;
        };
        let mut expected_query = expected
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<Vec<_>>();
        let mut actual_query = actual
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<Vec<_>>();
        for (key, rule) in &self.query_match {
            let values = actual_query
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v)
                .collect::<Vec<_>>();
            // Optional filters may be absent; duplicate keys are always refused.
            if values.len() > 1 || (values.is_empty() && !matches!(rule, QueryMatch::Optional)) {
                return false;
            }
            let value = values.first().map(|value| value.as_str()).unwrap_or("");
            let matched = match rule {
                QueryMatch::Ignore | QueryMatch::Optional => true,
                QueryMatch::AtMost(bound) => value <= bound.as_str(),
                QueryMatch::AtLeast(bound) => value >= bound.as_str(),
                QueryMatch::RangeIncludes { start, end } => value
                    .split_once(':')
                    .and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?)))
                    .is_some_and(|(a, b)| a <= *start && b >= *end && a <= b),
            };
            if !matched {
                return false;
            }
            expected_query.retain(|(k, _)| k != key);
            actual_query.retain(|(k, _)| k != key);
        }
        expected_query.sort();
        actual_query.sort();
        expected.set_query(None);
        actual.set_query(None);
        let mut expected_request = self.request.clone();
        let mut actual_request = request.clone();
        expected_request["url"] = Value::String(expected.into());
        actual_request["url"] = Value::String(actual.into());
        expected_request == actual_request && expected_query == actual_query
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub status: u16,
    #[serde(default = "json_content_type")]
    pub content_type: String,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub body_base64: Option<String>,
    #[serde(default)]
    pub continuation: Option<String>,
}
fn json_content_type() -> String {
    "application/json".into()
}
impl Response {
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        use base64::Engine;
        if let Some(encoded) = &self.body_base64 {
            return base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| "invalid replay base64".into());
        }
        if let Some(text) = &self.text {
            return Ok(text.as_bytes().to_vec());
        }
        serde_json::to_vec(
            self.body
                .as_ref()
                .ok_or("replay response needs body, text or body_base64")?,
        )
        .map_err(|e| e.to_string())
    }
}
type ReplayScope = Option<tokio::runtime::Id>;
type ActiveReplays = HashMap<ReplayScope, (Replay, String)>;
static ACTIVE: OnceLock<RwLock<ActiveReplays>> = OnceLock::new();
static FALLBACKS: OnceLock<Mutex<HashMap<ReplayScope, Vec<Value>>>> = OnceLock::new();
static SERIAL: Mutex<()> = Mutex::new(());
fn active() -> &'static RwLock<ActiveReplays> {
    ACTIVE.get_or_init(|| RwLock::new(HashMap::new()))
}
fn scope() -> ReplayScope {
    if cfg!(test) {
        tokio::runtime::Handle::try_current()
            .ok()
            .map(|handle| handle.id())
    } else {
        // A production eval's Tauri tasks can run on a second runtime.
        None
    }
}
pub struct Guard {
    scope: ReplayScope,
    _serial: Option<std::sync::MutexGuard<'static, ()>>,
}
impl Replay {
    pub fn install(self, environment: &str) -> Result<Guard, String> {
        if self.schema != "loupist-connection-replay/1" {
            return Err("unsupported replay schema".into());
        }
        for (provider, response) in self
            .requests
            .iter()
            .map(|entry| (&entry.provider, &entry.response))
            .chain(
                self.otherwise
                    .iter()
                    .map(|entry| (&entry.provider, &entry.response)),
            )
        {
            if crate::finance_data::native_vendor(provider).is_none()
                && !matches!(provider.as_str(), "web" | "web-search")
            {
                return Err("unknown replay provider".into());
            }
            let body_count = usize::from(response.body.is_some())
                + usize::from(response.text.is_some())
                + usize::from(response.body_base64.is_some());
            if body_count != 1 || !(100..=599).contains(&response.status) {
                return Err("replay response requires one body and a valid HTTP status".into());
            }
            response.bytes()?;
        }
        for fallback in &self.otherwise {
            if !(fallback.provider == "web-search" && fallback.url_prefix.is_empty())
                && reqwest::Url::parse(&fallback.url_prefix).is_err()
            {
                return Err("fallback needs an absolute URL prefix".into());
            }
        }
        let serial = if cfg!(test) {
            None
        } else {
            Some(SERIAL.lock().map_err(|_| "replay lock poisoned")?)
        };
        let scope = scope();
        let mut active = active().write().unwrap();
        if active.contains_key(&scope) {
            return Err("a replay is already installed in this runtime".into());
        }
        active.insert(scope, (self, environment.into()));
        Ok(Guard {
            scope,
            _serial: serial,
        })
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        active().write().unwrap().remove(&self.scope);
        FALLBACKS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .remove(&self.scope);
    }
}
pub(crate) fn credential_environment(environment: &str, entry: &str) -> bool {
    if !matches!(
        entry,
        "opendart.disclosures" | "krx.market-data" | "koreainvestment.market-data"
    ) {
        return false;
    }
    active()
        .read()
        .unwrap()
        .get(&scope())
        .is_some_and(|(_, expected)| expected == environment)
}
/// The active replay answers a Korea Investment account (trading) read.
pub(crate) fn wants_kis_account() -> bool {
    active()
        .read()
        .unwrap()
        .get(&scope())
        .is_some_and(|(replay, _)| {
            replay.requests.iter().any(|entry| {
                entry.provider == "koreainvestment.market-data"
                    && entry
                        .request
                        .get("url")
                        .and_then(Value::as_str)
                        .is_some_and(|url| url.contains("/trading/"))
            })
        })
}
pub(crate) fn enabled() -> bool {
    active().read().unwrap().contains_key(&scope())
}
pub(crate) fn lookup(provider: &str, request: &Value) -> Option<Result<Response, String>> {
    let active = active().read().unwrap();
    let (replay, _) = active.get(&scope())?;
    if let Some(entry) = replay
        .requests
        .iter()
        .find(|entry| entry.provider == provider && entry.matches(request))
    {
        return Some(Ok(entry.response.clone()));
    }
    let url = request.get("url").and_then(Value::as_str);
    if let Some(fallback) = replay.otherwise.iter().find(|entry| {
        entry.provider == provider
            && match url {
                Some(url) => url.starts_with(&entry.url_prefix),
                None => {
                    provider == "web-search"
                        && entry.url_prefix.is_empty()
                        && request.get("query").is_some_and(Value::is_string)
                }
            }
    }) {
        FALLBACKS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .entry(scope())
            .or_default()
            .push(serde_json::json!({"provider":provider,"url":url}));
        return Some(Ok(fallback.response.clone()));
    }
    Some(Err(format!("eval_replay_unmatched: {provider} {request}")))
}

pub(crate) fn take_fallbacks() -> Vec<Value> {
    FALLBACKS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(&scope())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_query_filters_allow_absence_but_never_duplicate_or_unknown_keys() {
        let entry: Entry = serde_json::from_value(serde_json::json!({"provider":"opendart.disclosures",
            "request":{"url":"https://fixture.example/list.json?corp=one"},"query_match":{"filter":"optional"},
            "response":{"status":200,"body":{}}})).unwrap();
        for url in [
            "https://fixture.example/list.json?corp=one",
            "https://fixture.example/list.json?corp=one&filter=periodic",
        ] {
            assert!(entry.matches(&serde_json::json!({"url":url})), "{url}");
        }
        for url in [
            "https://fixture.example/list.json?corp=other",
            "https://other.example/list.json?corp=one",
            "https://fixture.example/list.json?corp=one&filter=a&filter=b",
            "https://fixture.example/list.json?corp=one&extra=1",
        ] {
            assert!(!entry.matches(&serde_json::json!({"url":url})), "{url}");
        }
    }

    #[tokio::test]
    async fn fallback_is_provider_and_endpoint_scoped_and_observable_after_exact_matches() {
        let replay: Replay = serde_json::from_value(serde_json::json!({"schema":"loupist-connection-replay/1",
            "requests":[{"provider":"web","request":{"url":"https://fixture.example/page?id=one"},"response":{"status":200,"text":"exact"}}],
            "otherwise":[{"provider":"web","url_prefix":"https://fixture.example/page?","response":{"status":200,"text":"empty"}}]})).unwrap();
        let _guard = replay.install("fixture").unwrap();
        assert_eq!(
            lookup(
                "web",
                &serde_json::json!({"url":"https://fixture.example/page?id=one"})
            )
            .unwrap()
            .unwrap()
            .text
            .as_deref(),
            Some("exact")
        );
        assert!(take_fallbacks().is_empty());
        assert_eq!(
            lookup(
                "web",
                &serde_json::json!({"url":"https://fixture.example/page?id=other"})
            )
            .unwrap()
            .unwrap()
            .text
            .as_deref(),
            Some("empty")
        );
        assert_eq!(
            take_fallbacks(),
            vec![
                serde_json::json!({"provider":"web","url":"https://fixture.example/page?id=other"})
            ]
        );
        for (provider, url) in [
            ("unknown", "https://fixture.example/page?id=one"),
            ("web", "https://fixture.example/unknown"),
            ("web", "https://fixture.example/page-other?id=one"),
            ("web", "https://other.example/page?id=one"),
        ] {
            assert!(lookup(provider, &serde_json::json!({"url":url}))
                .unwrap()
                .is_err());
        }
    }

    #[tokio::test]
    async fn search_fallback_handles_the_hosts_query_object_without_a_url() {
        let replay: Replay = serde_json::from_value(serde_json::json!({"schema":"loupist-connection-replay/1", "requests":[],
            "otherwise":[{"provider":"web-search","url_prefix":"","response":{"status":200,"body":{"results":[]}}}]})).unwrap();
        let _guard = replay.install("fixture").unwrap();
        assert!(lookup(
            "web-search",
            &serde_json::json!({"provider":"exa_public","query":"issuer","limit":5})
        )
        .unwrap()
        .is_ok());
        assert_eq!(
            take_fallbacks(),
            vec![serde_json::json!({"provider":"web-search","url":null})]
        );
        for provider in ["web", "unknown"] {
            assert!(lookup(provider, &serde_json::json!({"query":"issuer"}))
                .unwrap()
                .is_err());
        }
        assert!(lookup("web-search", &serde_json::json!({"query":42}))
            .unwrap()
            .is_err());
    }

    #[test]
    fn every_agent_fixture_covers_list_receipts_and_contains_event_windows() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/agent/suites/replay");
        for path in std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
        {
            let replay: Replay = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            for entry in &replay.requests {
                let Some(url) = entry.request.get("url").and_then(Value::as_str) else {
                    continue;
                };
                let parsed = reqwest::Url::parse(url).unwrap();
                if !parsed.path().ends_with("/list.json") {
                    continue;
                }
                let rows = entry
                    .response
                    .body
                    .as_ref()
                    .and_then(|body| body["list"].as_array());
                for row in rows.into_iter().flatten() {
                    let Some(receipt) = row["rcept_no"].as_str() else {
                        continue;
                    };
                    let document =
                        format!("https://opendart.fss.or.kr/api/document.xml?rcept_no={receipt}");
                    assert!(
                        replay
                            .requests
                            .iter()
                            .any(|candidate| candidate.provider == entry.provider
                                && candidate.matches(&serde_json::json!({"url":document})))
                            || replay
                                .otherwise
                                .iter()
                                .any(|fallback| fallback.provider == entry.provider
                                    && document.starts_with(&fallback.url_prefix)),
                        "{} missing {document}",
                        path.display()
                    );
                }
                let dates = rows
                    .into_iter()
                    .flatten()
                    .filter_map(|row| row["rcept_dt"].as_str())
                    .collect::<Vec<_>>();
                if dates.is_empty() {
                    continue;
                }
                let first = *dates.iter().min().unwrap();
                let last = *dates.iter().max().unwrap();
                let year = &first[..4];
                let month = &first[..6];
                let clock = "20261113";
                for (start, end) in [
                    (format!("{year}0101"), clock.to_string()),
                    ("20251113".to_string(), clock.to_string()),
                    (format!("{month}01"), format!("{}31", &last[..6])),
                ] {
                    let mut request_url = parsed.clone();
                    let query = parsed
                        .query_pairs()
                        .map(|(key, value)| (key.into_owned(), value.into_owned()))
                        .collect::<Vec<_>>();
                    request_url
                        .query_pairs_mut()
                        .clear()
                        .extend_pairs(query.iter().map(|(key, value)| {
                            (
                                key.as_str(),
                                match key.as_str() {
                                    "bgn_de" => start.as_str(),
                                    "end_de" => end.as_str(),
                                    _ => value.as_str(),
                                },
                            )
                        }));
                    assert_eq!(
                        entry.matches(&serde_json::json!({"url":request_url.as_str()})),
                        start.as_str() <= first && end.as_str() >= last,
                        "{} {} containment for {start}..{end}",
                        path.display(),
                        url
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn as_of_lists_cover_partial_windows_without_future_receipts() {
        let replay: Replay = serde_json::from_str(include_str!(
            "../../../evals/agent/suites/replay/opendart-byeolmuri-as-of.json"
        ))
        .unwrap();
        let _guard = replay.install("fixture").unwrap();
        for (start, end, filter, expected) in [
            (
                "20260101",
                "20260331",
                "",
                vec!["20260318990308", "20260305990308"],
            ),
            ("20260101", "20260315", "", vec!["20260305990308"]),
            (
                "20260101",
                "20260315",
                "&pblntf_ty=I",
                vec!["20260305990308"],
            ),
            (
                "20260101",
                "20260315",
                "&pblntf_ty=B",
                vec!["20260305990308"],
            ),
            ("20260310", "20260320", "", vec!["20260318990308"]),
            ("20260305", "20260305", "", vec!["20260305990308"]),
            ("20260318", "20260318", "", vec!["20260318990308"]),
            ("20260306", "20260317", "", vec![]),
            ("20260101", "20260304", "", vec![]),
        ] {
            let url = format!("https://opendart.fss.or.kr/api/list.json?corp_code=99030800&bgn_de={start}&end_de={end}&last_reprt_at=N&sort=date&sort_mth=desc&page_no=1&page_count=100{filter}");
            let response = lookup("opendart.disclosures", &serde_json::json!({"url":url}))
                .unwrap()
                .unwrap();
            let body = response.body.unwrap();
            let receipts = body["list"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|row| row["rcept_no"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(receipts, expected, "{start}..{end}{filter}");
        }
        assert_eq!(take_fallbacks().len(), 2);
    }

    #[tokio::test]
    async fn as_of_financial_fixture_closes_neighbouring_list_requests() {
        let replay: Replay = serde_json::from_str(include_str!(
            "../../../evals/agent/suites/replay/opendart-solsup-as-of.json"
        ))
        .unwrap();
        let _guard = replay.install("fixture").unwrap();
        let url = "https://opendart.fss.or.kr/api/list.json?corp_code=99022100&bgn_de=20260101&end_de=20260310&last_reprt_at=N&sort=date&sort_mth=desc&page_no=1&page_count=100";
        let response = lookup("opendart.disclosures", &serde_json::json!({"url":url}))
            .unwrap()
            .unwrap();
        assert_eq!(response.body.unwrap()["status"], "013");
        assert_eq!(take_fallbacks().len(), 1);
        for (provider, url) in [
            ("unknown", url),
            (
                "opendart.disclosures",
                "https://opendart.fss.or.kr/api/uncovered.json",
            ),
            (
                "opendart.disclosures",
                "https://other.example/api/list.json",
            ),
        ] {
            assert!(lookup(provider, &serde_json::json!({"url":url}))
                .unwrap()
                .err()
                .unwrap()
                .to_string()
                .contains("eval_replay_unmatched"));
        }
    }

    #[tokio::test]
    async fn every_opendart_fixture_closes_unknown_company_list_requests() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/agent/suites/replay");
        for path in std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
        {
            let replay: Replay = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if !replay
                .requests
                .iter()
                .any(|entry| entry.provider == "opendart.disclosures")
            {
                continue;
            }
            let _guard = replay.install("fixture").unwrap();
            let url = "https://opendart.fss.or.kr/api/list.json?corp_code=00000000&bgn_de=20260101&end_de=20260310&last_reprt_at=N&sort=date&sort_mth=desc&page_no=1&page_count=100";
            let response = lookup("opendart.disclosures", &serde_json::json!({"url":url}))
                .unwrap()
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                response.body.unwrap()["status"],
                "013",
                "{}",
                path.display()
            );
            assert_eq!(take_fallbacks().len(), 1, "{}", path.display());
        }
    }

    #[tokio::test]
    async fn every_web_fixture_closes_searches_but_rejects_unknown_fetches() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/agent/suites/replay");
        for path in std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
        {
            let replay: Replay = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if !replay
                .requests
                .iter()
                .any(|entry| matches!(entry.provider.as_str(), "web" | "web-search"))
            {
                continue;
            }
            let _guard = replay.install("fixture").unwrap();
            let response = lookup("web-search", &serde_json::json!({"provider":"exa_public","query":"unrecorded neighbouring query","limit":5}))
                .unwrap()
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                response.body.unwrap()["results"],
                serde_json::json!([]),
                "{}",
                path.display()
            );
            assert_eq!(take_fallbacks().len(), 1, "{}", path.display());
            for (provider, request) in [
                (
                    "web",
                    serde_json::json!({"url":"https://not-a-fixture.example/unrecorded"}),
                ),
                ("web-search", serde_json::json!({"query":42})),
            ] {
                assert!(
                    lookup(provider, &request).unwrap().is_err(),
                    "{}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn replays_are_independent_across_parallel_test_runtimes() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        std::thread::scope(|threads| {
            for identity in ["first", "second", "without-replay"] {
                let barrier = barrier.clone();
                threads.spawn(move || {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(async {
                        let replay: Replay = serde_json::from_value(serde_json::json!({
                            "schema": "loupist-connection-replay/1",
                            "requests": [{"provider": "web", "request": {"url": "https://fixture.example/"},
                                "response": {"status": 200, "text": identity}}]
                        })).unwrap();
                        let guard = (identity != "without-replay")
                            .then(|| replay.install(identity).unwrap());
                        barrier.wait();
                        // Spawned tasks and blocking work must see their own
                        // runtime's replay, including on another worker thread.
                        tokio::spawn(async move {
                            let result = lookup("web", &serde_json::json!({"url": "https://fixture.example/"}));
                            if identity == "without-replay" {
                                assert!(result.is_none());
                                assert!(!enabled());
                            } else {
                                assert_eq!(result.unwrap().unwrap().text.as_deref(), Some(identity));
                                assert!(credential_environment(identity, "krx.market-data"));
                            }
                            assert_eq!(tokio::task::spawn_blocking(enabled).await.unwrap(), identity != "without-replay");
                        }).await.unwrap();
                        barrier.wait();
                        drop(guard);
                        assert!(!enabled());
                    });
                });
            }
        });
    }

    #[test]
    fn eval_replay_query_rules_preserve_identity_and_bounds() {
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "provider":"opendart.disclosures",
            "request":{"url":"https://fictional.example/list?corp=99&start=20260101&end=20260331&limit=1"},
            "query_match":{"start":{"at_most":"20260318"},"end":{"at_least":"20260318"},"limit":"ignore"},
            "response":{"status":200,"body":{}}
        })).unwrap();
        assert!(entry.matches(&serde_json::json!({"url":"https://fictional.example/list?limit=100&end=20261231&start=20260101&corp=99"})));
        for url in [
            "https://fictional.example/list?corp=88&start=20260101&end=20260331&limit=1",
            "https://fictional.example/other?corp=99&start=20260101&end=20260331&limit=1",
            "https://fictional.example/list?corp=99&start=20260401&end=20261231&limit=1",
            "https://fictional.example/list?corp=99&start=20260101&end=20260301&limit=1",
            "https://fictional.example/list?corp=99&start=20260101&end=20260331",
            "https://fictional.example/list?corp=99&start=20260101&end=20260331&limit=1&limit=2",
            "https://fictional.example/list?corp=99&start=20260101&end=20260331&limit=1&extra=1",
        ] {
            assert!(!entry.matches(&serde_json::json!({"url":url})), "{url}");
        }
        assert!(
            !entry.matches(&serde_json::json!({"url":entry.request["url"],"tr_id":"unexpected"}))
        );
        let range: Entry = serde_json::from_value(serde_json::json!({
            "provider":"world-bank.indicators", "request":{"url":"https://fictional.example/series?date=2022:2023"},
            "query_match":{"date":{"range_includes":{"start":2022,"end":2023}}},
            "response":{"status":200,"body":{}}
        })).unwrap();
        assert!(range.matches(
            &serde_json::json!({"url":"https://fictional.example/series?date=2000:2025"})
        ));
        for date in [
            "2023:2023",
            "2020:2021",
            "2024:2020",
            "all",
            "2022:2023:2024",
        ] {
            assert!(!range.matches(
                &serde_json::json!({"url":format!("https://fictional.example/series?date={date}")})
            ));
        }
    }
    #[tokio::test]
    async fn eval_replay_matches_provider_and_rejects_unmatched() {
        let replay: Replay = serde_json::from_str(include_str!(
            "../../../evals/agent/suites/replay/opendart-hanbit.json"
        ))
        .unwrap();
        let request = replay.requests[0].request.clone();
        let _guard = replay.install("test-replay").unwrap();
        assert!(lookup("opendart.disclosures", &request)
            .unwrap()
            .unwrap()
            .bytes()
            .unwrap()
            .windows(6)
            .any(|w| w == b"Hanbit"));
        let error = lookup(
            "opendart.disclosures",
            &serde_json::json!({"url":"https://fictional.example/unexpected"}),
        )
        .unwrap()
        .err()
        .unwrap();
        assert!(error.contains("eval_replay_unmatched"));
        // Exercise the real provider parser, rather than substituting a tool result.
        let service = crate::finance_data::FinanceDataService::new().unwrap();
        let result = service.opendart_filing_search(serde_json::json!({"corp_code":"00999999","start":"2020-01-01","end":"2020-12-31","limit":1}), "eval-placeholder").await;
        let result = result.unwrap();
        assert!(result.to_string().contains("Fictional Hanbit Aurora"));
        let unmatched = service.opendart_filing_search(serde_json::json!({"corp_code":"00888888","start":"2020-01-01","end":"2020-12-31","limit":1}), "eval-placeholder").await.unwrap_err();
        assert!(matches!(
            unmatched,
            crate::finance_data::FinanceDataError::NoData(_)
        ));
        assert!(unmatched.to_string().contains("no matching data"));
        let source = crate::web::source_from_url("https://fictional.example/unmatched").unwrap();
        assert!(crate::web::fetch(&source)
            .await
            .unwrap_err()
            .to_string()
            .contains("eval_replay_unmatched"));
    }
}
