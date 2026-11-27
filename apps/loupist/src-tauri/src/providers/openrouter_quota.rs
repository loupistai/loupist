//! One read of the OpenRouter key the user already stored.
//!
//! The key is the `Authorization` header of `GET https://openrouter.ai/api/v1/key`
//! and the identity of an in-memory cache. It is not logged, and the response's
//! label and account identifiers are dropped. A failed read is remembered for
//! the same five minutes as a success, so a miss does not retry.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::util::sha256;

pub(crate) const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const MAX_BODY: usize = 64 * 1024;
const FRESH: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterQuota {
    pub limit_usd: f64,
    pub remaining_usd: f64,
    pub usage_daily_usd: f64,
    pub usage_monthly_usd: f64,
    pub reset: Option<String>,
}

#[derive(Default)]
pub(crate) struct QuotaCache {
    entries: Mutex<HashMap<String, (Instant, Option<OpenRouterQuota>)>>,
}

impl QuotaCache {
    /// `Some` when this key was read within the last five minutes, including a
    /// failed read. `None` when a request is allowed.
    fn fresh(&self, key: &str, now: Instant) -> Option<Option<OpenRouterQuota>> {
        let id = sha256(key.as_bytes());
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (at, value) = entries.get(&id)?;
        now.saturating_duration_since(*at)
            .lt(&FRESH)
            .then(|| value.clone())
    }

    fn store(&self, key: &str, now: Instant, value: Option<OpenRouterQuota>) {
        let id = sha256(key.as_bytes());
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.insert(id, (now, value));
    }

    #[cfg(test)]
    fn read(
        &self,
        key: &str,
        now: Instant,
        fetch: impl FnOnce() -> Option<OpenRouterQuota>,
    ) -> Option<OpenRouterQuota> {
        if let Some(cached) = self.fresh(key, now) {
            return cached;
        }
        let value = fetch();
        self.store(key, now, value.clone());
        value
    }
}

pub(crate) struct KeyReply {
    pub status: u16,
    pub redirected: bool,
    pub body: Vec<u8>,
}

/// Numbers from one reply. A redirect, an error status, an oversized body, a
/// body that is not the key object, or a key with no limit is `None`.
pub(crate) fn quota_from_reply(reply: KeyReply) -> Option<OpenRouterQuota> {
    if reply.redirected || !(200..300).contains(&reply.status) || reply.body.len() > MAX_BODY {
        return None;
    }
    let value: Value = serde_json::from_slice(&reply.body).ok()?;
    let data = value
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(&value);
    let limit = data.get("limit")?;
    if limit.is_null() {
        return None;
    }
    Some(OpenRouterQuota {
        limit_usd: finite(limit)?,
        remaining_usd: finite(data.get("limit_remaining")?)?,
        usage_daily_usd: data.get("usage_daily").and_then(finite).unwrap_or(0.0),
        usage_monthly_usd: data.get("usage_monthly").and_then(finite).unwrap_or(0.0),
        reset: data
            .get("limit_reset")
            .and_then(Value::as_str)
            .filter(|reset| matches!(*reset, "daily" | "monthly" | "weekly"))
            .map(str::to_owned),
    })
}

fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

pub(crate) async fn current(key: &str) -> Option<OpenRouterQuota> {
    static CACHE: std::sync::OnceLock<QuotaCache> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(QuotaCache::default);
    let now = Instant::now();
    if let Some(cached) = cache.fresh(key, now) {
        return cached;
    }
    let value = http_quota(key).await;
    cache.store(key, Instant::now(), value.clone());
    value
}

async fn http_quota(key: &str) -> Option<OpenRouterQuota> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let mut response = client
        .get(KEY_URL)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .ok()?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > MAX_BODY {
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    quota_from_reply(KeyReply {
        status,
        redirected: false,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, redirected: bool, body: &str) -> KeyReply {
        KeyReply {
            status,
            redirected,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_key_reply_keeps_numbers_and_a_second_read_makes_no_request() {
        let full = r#"{"data":{"label":"sk-or-v1-secret","limit":50,"limit_remaining":12.5,"limit_reset":"monthly","usage":37.5,"usage_daily":1.25,"usage_monthly":37.5,"is_free_tier":false,"creator_user_id":"user_123","organization_id":"org_9","workspace_id":"ws_4"}}"#;
        let cases = [
            (reply(200, false, full), true),
            (
                reply(
                    200,
                    false,
                    r#"{"data":{"label":"unlimited","limit":null,"usage_daily":1}}"#,
                ),
                false,
            ),
            (reply(200, false, full), true),
            (reply(200, false, &" ".repeat(super::MAX_BODY + 1)), false),
            (reply(302, true, full), false),
            (reply(401, false, full), false),
        ];
        let expected = OpenRouterQuota {
            limit_usd: 50.0,
            remaining_usd: 12.5,
            usage_daily_usd: 1.25,
            usage_monthly_usd: 37.5,
            reset: Some("monthly".into()),
        };
        for (index, (scripted, ok)) in cases.into_iter().enumerate() {
            let cache = QuotaCache::default();
            let mut calls = 0usize;
            let now = Instant::now();
            let mut read = |when| {
                cache.read("stored-key", when, || {
                    calls += 1;
                    quota_from_reply(KeyReply {
                        status: scripted.status,
                        redirected: scripted.redirected,
                        body: scripted.body.clone(),
                    })
                })
            };
            let first = read(now);
            let second = read(now + Duration::from_secs(60));
            assert_eq!(calls, 1, "case {index}");
            assert_eq!(first, second, "case {index}");
            if ok {
                assert_eq!(first.as_ref(), Some(&expected), "case {index}");
                let kept = serde_json::to_string(first.as_ref().unwrap()).unwrap();
                assert!(!kept.contains("sk-or"));
                assert!(!kept.contains("user_123"));
                assert!(!kept.contains("org_9"));
                assert!(!kept.contains("ws_4"));
                assert!(!kept.contains("label"));
            } else {
                assert!(first.is_none(), "case {index}");
            }
        }
        assert_eq!(KEY_URL, "https://openrouter.ai/api/v1/key");
    }
}
