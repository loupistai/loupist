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
            // Every declared parameter must occur exactly once, including ignored ones.
            if values.len() != 1 {
                return false;
            }
            let value = values[0];
            let matched = match rule {
                QueryMatch::Ignore => true,
                QueryMatch::AtMost(bound) => value <= bound,
                QueryMatch::AtLeast(bound) => value >= bound,
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
        for entry in &self.requests {
            if !matches!(
                entry.provider.as_str(),
                "opendart.disclosures"
                    | "krx.market-data"
                    | "koreainvestment.market-data"
                    | "world-bank.indicators"
                    | "defillama.tvl"
                    | "web"
                    | "web-search"
            ) {
                return Err("unknown replay provider".into());
            }
            let body_count = usize::from(entry.response.body.is_some())
                + usize::from(entry.response.text.is_some())
                + usize::from(entry.response.body_base64.is_some());
            if body_count != 1 || !(100..=599).contains(&entry.response.status) {
                return Err("replay response requires one body and a valid HTTP status".into());
            }
            entry.response.bytes()?;
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
    Some(
        replay
            .requests
            .iter()
            .find(|entry| entry.provider == provider && entry.matches(request))
            .map(|entry| entry.response.clone())
            .ok_or_else(|| format!("eval_replay_unmatched: {provider} {request}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(unmatched.to_string().contains("eval_replay_unmatched"));
        let source = crate::web::source_from_url("https://fictional.example/unmatched").unwrap();
        assert!(crate::web::fetch(&source)
            .await
            .unwrap_err()
            .to_string()
            .contains("eval_replay_unmatched"));
    }
}
