//! The last close of an asset, for the `$` asset card above an answer: the
//! newest one its asset page recorded, or one a price read just returned.
//! Picking a quote never records it.
use serde::Serialize;
use serde_json::Value;

use super::{observe, AssetEvidence, ObservationKind, StoredObservation};

/// A completed session's close with the source that reported it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetQuote {
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// The session the close belongs to (`YYYY-MM-DD`).
    pub date: String,
    /// When the source returned it (RFC 3339), when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieved_at: Option<String>,
    /// The row's source, as the asset page cites it.
    pub evidence: AssetEvidence,
    /// Read for this card just now, rather than taken from the asset page.
    pub fresh: bool,
}

/// A close of a price series (any adjustment); rate levels are not prices.
fn price_close(kind: ObservationKind, facts: &Value) -> Option<f64> {
    if kind != ObservationKind::Close
        || !observe::is_price_series(facts["series"].as_str().unwrap_or(observe::PRICE))
    {
        return None;
    }
    facts["close"].as_f64().filter(|value| value.is_finite())
}

/// Newest session first; on the same session an unadjusted close, which is the
/// price the market printed, wins over an adjusted one.
fn rank(date: &str, facts: &Value) -> (String, bool) {
    (
        date.to_owned(),
        facts["series"].as_str() != Some(observe::PRICE_ADJUSTED),
    )
}

fn quote(
    value: f64,
    date: &str,
    facts: &Value,
    evidence: &AssetEvidence,
    retrieved_at: Option<String>,
    fresh: bool,
) -> AssetQuote {
    AssetQuote {
        value,
        currency: facts["currency"]
            .as_str()
            .map(str::trim)
            .filter(|currency| !currency.is_empty())
            .map(str::to_owned),
        date: date.to_owned(),
        retrieved_at: evidence.retrieved_at.clone().or(retrieved_at),
        evidence: evidence.clone(),
        fresh,
    }
}

/// The newest close the asset page recorded, with when it was recorded when its
/// source gave no retrieval time.
pub fn stored(observed: &[StoredObservation]) -> Option<AssetQuote> {
    let best = observed
        .iter()
        .filter_map(|item| Some((item, price_close(item.kind, &item.facts)?)))
        .max_by(|(left, _), (right, _)| {
            rank(&left.fact_date, &left.facts)
                .cmp(&rank(&right.fact_date, &right.facts))
                .then(left.recorded_at_ms.cmp(&right.recorded_at_ms))
        })?;
    let (item, value) = best;
    let recorded = chrono::DateTime::from_timestamp_millis(item.recorded_at_ms)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    Some(quote(
        value,
        &item.fact_date,
        &item.facts,
        &item.evidence,
        recorded,
        false,
    ))
}

/// The newest close among observations a price read just returned.
pub fn from_read(found: &[observe::NewObservation]) -> Option<AssetQuote> {
    let (item, value) = found
        .iter()
        .filter_map(|item| Some((item, price_close(item.kind, &item.facts)?)))
        .max_by(|(left, _), (right, _)| {
            rank(&left.fact_date, &left.facts).cmp(&rank(&right.fact_date, &right.facts))
        })?;
    Some(quote(
        value,
        &item.fact_date,
        &item.facts,
        &item.evidence,
        None,
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn evidence(title: &str) -> AssetEvidence {
        AssetEvidence {
            tool: "krx_daily_quote".into(),
            title: title.into(),
            date: Some("2026-10-28".into()),
            ..AssetEvidence::default()
        }
    }

    fn stored_close(date: &str, series: &str, close: f64, recorded: i64) -> StoredObservation {
        StoredObservation {
            observation_id: format!("{date}:{series}:{recorded}"),
            kind: ObservationKind::Close,
            fact_date: date.into(),
            recorded_at_ms: recorded,
            facts: json!({"series": series, "close": close, "currency": "KRW"}),
            evidence: evidence("KRX"),
            baseline: false,
            dedupe_key: format!("close:{series}:{date}"),
        }
    }

    #[test]
    fn the_stored_quote_is_the_newest_printed_close_with_its_source() {
        let observed = vec![
            stored_close("2026-10-27", observe::PRICE, 41_200.0, 10),
            stored_close("2026-10-28", observe::PRICE_ADJUSTED, 39_800.0, 20),
            stored_close("2026-10-28", observe::PRICE, 39_920.0, 20),
            // A rate level is not a price.
            StoredObservation {
                facts: json!({"series": "rate:10y", "close": 4.1}),
                fact_date: "2026-10-29".into(),
                ..stored_close("2026-10-29", observe::PRICE, 0.0, 30)
            },
        ];
        let quote = stored(&observed).unwrap();
        assert_eq!(quote.value, 39_920.0);
        assert_eq!(quote.date, "2026-10-28");
        assert_eq!(quote.currency.as_deref(), Some("KRW"));
        assert_eq!(quote.evidence.title, "KRX");
        assert!(!quote.fresh);
        // Without a retrieval time from the source, the time it was recorded stands in.
        assert_eq!(quote.retrieved_at.as_deref(), Some("1970-01-01T00:00:00Z"));
        let encoded = serde_json::to_value(&quote).unwrap();
        assert_eq!(encoded["evidence"]["tool"], "krx_daily_quote");
        assert_eq!(encoded["retrievedAt"], "1970-01-01T00:00:00Z");
        assert!(stored(&[]).is_none());
    }

    #[test]
    fn a_fresh_read_quotes_its_latest_completed_session() {
        let bars = json!({"data": {"instrument": {"currency": "KRW"}, "bars": [
            {"date": "2026-10-27", "close": "41200"},
            {"date": "2026-10-28", "close": "39920"},
            // Today's bar may still be trading and is not a close yet.
            {"date": "2026-10-29", "close": "40500"}
        ]}});
        let day = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap();
        let found = observe::extract(
            &observe::ToolRead {
                tool: "krx_daily_quote",
                provider: None,
                payload: &bars,
                // A row is recorded only with a source: here the retained read.
                dataset: Some(crate::datasets::DatasetRef {
                    dataset_id: "dataset-1".into(),
                    revision: 1,
                    digest: "d".repeat(64),
                }),
                retrieved_at: Some("2026-10-29T00:12:00Z"),
                citation_url: None,
                today: day("2026-10-29"),
                session_day: day("2026-10-29"),
            },
            "990308",
        );
        let quote = from_read(&found).unwrap();
        assert_eq!((quote.value, quote.date.as_str()), (39_920.0, "2026-10-28"));
        assert_eq!(quote.retrieved_at.as_deref(), Some("2026-10-29T00:12:00Z"));
        assert_eq!(quote.evidence.title, "KRX");
        assert!(quote.fresh);
        assert!(from_read(&[]).is_none());
    }
}
