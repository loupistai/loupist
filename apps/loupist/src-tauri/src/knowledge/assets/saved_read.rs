//! Price facts projected from an acknowledged retained provider result.
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::Value;

use super::{asset_error, KnowledgeResult};
use crate::commands::assets::wire::{
    AssetPricePoint, AssetQuoteKind, AssetStoredPrice, PriceFields, PriceReadState, RollingDates,
    SavedPriceRead,
};

#[derive(Clone, Debug)]
pub(crate) struct Stored {
    pub read: SavedPriceRead,
    pub price: Option<AssetStoredPrice>,
    pub generation: String,
}

#[derive(Clone, Debug)]
pub(crate) struct StagedPriceRead {
    pub asset_id: String,
    pub read: SavedPriceRead,
    pub price: AssetStoredPrice,
    pub history: Vec<AssetPricePoint>,
}

pub(crate) fn load(
    tx: &Transaction<'_>,
    loupist: &str,
    asset: &str,
) -> KnowledgeResult<Option<Stored>> {
    let row: Option<(String, Option<String>, String)> = tx
        .query_row(
            "SELECT read_json,price_json,generation FROM knowledge_saved_price_reads
         WHERE loupist_id=?1 AND asset_id=?2",
            params![loupist, asset],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(read, price, generation)| -> KnowledgeResult<Stored> {
        Ok(Stored {
            read: serde_json::from_str(&read)?,
            price: price
                .map(|value| serde_json::from_str(&value))
                .transpose()?,
            generation,
        })
    })
    .transpose()
}

fn add_points(
    tx: &Transaction<'_>,
    loupist: &str,
    asset: &str,
    points: &[AssetPricePoint],
) -> KnowledgeResult<()> {
    let mut statement = tx.prepare_cached(
        "INSERT INTO knowledge_asset_price_points(loupist_id,asset_id,fact_date,value)
         VALUES(?1,?2,?3,?4) ON CONFLICT(loupist_id,asset_id,fact_date) DO UPDATE SET value=excluded.value",
    )?;
    for point in points {
        statement.execute(params![loupist, asset, point.date, point.value])?;
    }
    Ok(())
}

pub(crate) fn points(
    tx: &Transaction<'_>,
    loupist: &str,
    asset: &str,
) -> KnowledgeResult<Vec<AssetPricePoint>> {
    let mut statement = tx.prepare(
        "SELECT fact_date,value FROM knowledge_asset_price_points
         WHERE loupist_id=?1 AND asset_id=?2 ORDER BY fact_date DESC LIMIT 4000",
    )?;
    let rows = statement.query_map(params![loupist, asset], |row| {
        Ok(AssetPricePoint {
            date: row.get(0)?,
            value: row.get(1)?,
        })
    })?;
    let mut points = rows.collect::<Result<Vec<_>, _>>()?;
    points.reverse();
    Ok(points)
}

fn price_date(date: &str) -> Option<NaiveDateTime> {
    let date = date.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(date) {
        return Some(time.naive_local());
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(time) = NaiveDateTime::parse_from_str(date, format) {
            return Some(time);
        }
    }
    let parsed = if date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()) {
        NaiveDate::parse_from_str(date, "%Y%m%d").ok()
    } else if date
        .split(['-', '/'])
        .next()
        .is_some_and(|field| field.len() == 4)
    {
        NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(date, "%Y/%m/%d"))
            .ok()
    } else if date
        .rsplit('/')
        .next()
        .is_some_and(|field| field.len() == 4)
    {
        let month_first = NaiveDate::parse_from_str(date, "%m/%d/%Y").ok();
        let day_first = NaiveDate::parse_from_str(date, "%d/%m/%Y").ok();
        match (month_first, day_first) {
            (Some(a), Some(b)) if a != b => None,
            (a, b) => a.or(b),
        }
    } else {
        None
    };
    parsed?.and_hms_opt(0, 0, 0)
}

fn point(price: &AssetStoredPrice) -> Option<AssetPricePoint> {
    Some(AssetPricePoint {
        date: price_date(&price.date)?.date().to_string(),
        value: price.value,
    })
}

pub(crate) fn save(
    tx: &Transaction<'_>,
    loupist: &str,
    staged: &StagedPriceRead,
) -> KnowledgeResult<()> {
    let asset = &staged.asset_id;
    let previous = load(tx, loupist, asset)?;
    if previous
        .as_ref()
        .is_some_and(|stored| stored.read.current != staged.read.current)
    {
        tx.execute(
            "DELETE FROM knowledge_asset_price_points WHERE loupist_id=?1 AND asset_id=?2",
            params![loupist, asset],
        )?;
    }
    tx.execute(
        "INSERT INTO knowledge_saved_price_reads(loupist_id,asset_id,read_json,price_json,generation)
         VALUES(?1,?2,?3,?4,?5) ON CONFLICT(loupist_id,asset_id) DO UPDATE
         SET read_json=excluded.read_json,price_json=excluded.price_json,generation=excluded.generation",
        params![loupist, asset, serde_json::to_string(&staged.read)?, serde_json::to_string(&staged.price)?, super::mint_id("apr")],
    )?;
    add_points(tx, loupist, asset, &staged.history)?;
    if let Some(point) = point(&staged.price) {
        add_points(tx, loupist, asset, &[point])?;
    }
    super::bump(tx, loupist, asset)
}

pub(crate) fn settle(
    tx: &Transaction<'_>,
    loupist: &str,
    asset: &str,
    expected: &Stored,
    result: Result<AssetStoredPrice, PriceReadState>,
) -> KnowledgeResult<bool> {
    let mut read = expected.read.clone();
    let price = match result {
        Ok(price) => {
            read.state = PriceReadState::Ready;
            Some(price)
        }
        Err(state) => {
            read.state = state;
            expected.price.clone()
        }
    };
    if read == expected.read && price == expected.price {
        return Ok(false);
    }
    if tx.execute(
        "UPDATE knowledge_saved_price_reads SET read_json=?4,price_json=?5,generation=?6
         WHERE loupist_id=?1 AND asset_id=?2 AND generation=?3",
        params![
            loupist,
            asset,
            expected.generation,
            serde_json::to_string(&read)?,
            price.as_ref().map(serde_json::to_string).transpose()?,
            super::mint_id("apr")
        ],
    )? == 0
    {
        return Ok(false);
    }
    if let Some(point) = price
        .as_ref()
        .filter(|_| read.state == PriceReadState::Ready)
        .and_then(point)
    {
        add_points(tx, loupist, asset, &[point])?;
    }
    super::bump(tx, loupist, asset)?;
    Ok(true)
}

pub(crate) fn settle_history(
    tx: &Transaction<'_>,
    loupist: &str,
    asset: &str,
    expected: &Stored,
    points: &[AssetPricePoint],
) -> KnowledgeResult<()> {
    if let Some(stored) = load(tx, loupist, asset)?.filter(|stored| {
        stored.read.current == expected.read.current && stored.read.history == expected.read.history
    }) {
        add_points(tx, loupist, asset, points)?;
        // An overlapping current replay can settle first. Its point still wins that day.
        if let Some(point) = stored.price.as_ref().and_then(point) {
            add_points(tx, loupist, asset, &[point])?;
        }
    }
    Ok(())
}

fn field_rows<'a>(payload: &'a Value, fields: &PriceFields) -> KnowledgeResult<Vec<&'a Value>> {
    let pointer = |value: &str| value.len() <= 512 && (value.is_empty() || value.starts_with('/'));
    if ![
        Some(&fields.value),
        Some(&fields.date),
        fields.currency.as_ref(),
        fields.rows.as_ref(),
    ]
    .into_iter()
    .flatten()
    .all(|value| pointer(value))
    {
        return Err(asset_error(
            "price_fields",
            "Use JSON Pointers into the retained result.",
        ));
    }
    match &fields.rows {
        Some(rows) => payload
            .pointer(rows)
            .and_then(Value::as_array)
            .map(|rows| rows.iter().collect())
            .ok_or_else(|| asset_error("price_fields", "The rows pointer must select an array.")),
        None => Ok(vec![payload]),
    }
}

fn decimal(text: &str) -> Option<f64> {
    if text.len() > 64 {
        return None;
    }
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = unsigned
        .split_once('.')
        .map_or((unsigned, None), |(whole, fraction)| {
            (whole, Some(fraction))
        });
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole)
        || (whole.len() > 1 && whole.starts_with('0'))
        || fraction.is_some_and(|part| !digits(part))
    {
        return None;
    }
    text.parse().ok()
}

/// With rows, a currency the row does not state is read from the whole result, where list
/// providers usually state it once.
fn price_value(row: &Value, root: &Value, fields: &PriceFields) -> Option<(f64, Option<String>)> {
    let raw = row.pointer(&fields.value)?;
    let value = raw
        .as_f64()
        .or_else(|| decimal(raw.as_str()?))
        .filter(|value| value.is_finite())?;
    let currency = match &fields.currency {
        Some(field) => match row.pointer(field).or_else(|| root.pointer(field)) {
            Some(Value::Null) | None => None,
            Some(value) => {
                let text = value.as_str().filter(|text| {
                    text.chars().count() <= 24 && !text.chars().any(char::is_control)
                })?;
                (!text.trim().is_empty()).then(|| text.to_owned())
            }
        },
        None => None,
    };
    Some((value, currency))
}

struct DatedPrice<'a> {
    value: f64,
    date: &'a str,
    parsed: Option<NaiveDateTime>,
    currency: Option<String>,
}

fn dated_prices<'a>(
    payload: &'a Value,
    fields: &PriceFields,
    require_date: bool,
) -> KnowledgeResult<Vec<DatedPrice<'a>>> {
    let mut prices = Vec::new();
    for row in field_rows(payload, fields)? {
        let date = row
            .pointer(&fields.date)
            .and_then(Value::as_str)
            .filter(|date| {
                !date.trim().is_empty()
                    && date.chars().count() <= 96
                    && !date.chars().any(char::is_control)
            });
        let parsed = date.and_then(price_date);
        if require_date && parsed.is_none() {
            return Err(asset_error(
                "price_fields",
                "Use a field with ISO dates (YYYY-MM-DD) or ISO timestamps for the price rows.",
            ));
        }
        if let (Some(date), Some((value, currency))) = (date, price_value(row, payload, fields)) {
            prices.push(DatedPrice {
                value,
                date,
                parsed,
                currency,
            });
        }
    }
    Ok(prices)
}

// The selected JSON Pointer names the field the provider returned. A generic
// numeric field has no quote meaning, even on an otherwise recognizable row.
fn quote_kind(pointer: &str) -> Option<AssetQuoteKind> {
    match pointer.rsplit_once('/')?.1 {
        "open" => Some(AssetQuoteKind::Open),
        "high" => Some(AssetQuoteKind::High),
        "low" => Some(AssetQuoteKind::Low),
        "close" | "close_price" => Some(AssetQuoteKind::Close),
        "adjusted_close" | "adj_close" | "adjClose" => Some(AssetQuoteKind::AdjustedClose),
        "last" | "last_price" | "lastPrice" => Some(AssetQuoteKind::Last),
        "bid" | "bid_price" | "bidPrice" => Some(AssetQuoteKind::Bid),
        "ask" | "ask_price" | "askPrice" => Some(AssetQuoteKind::Ask),
        _ => None,
    }
}

pub(crate) fn price(
    payload: &Value,
    fields: &PriceFields,
    provider: String,
    source_url: Option<String>,
    dataset: crate::datasets::DatasetRef,
) -> KnowledgeResult<AssetStoredPrice> {
    let price = dated_prices(payload, fields, fields.rows.is_some())?
        .into_iter()
        .max_by_key(|price| price.parsed)
        .ok_or_else(|| {
            asset_error(
                "price_fields",
                "The retained result needs a finite number and a stated date.",
            )
        })?;
    Ok(AssetStoredPrice {
        kind: quote_kind(&fields.value),
        value: price.value,
        date: price.date.to_owned(),
        currency: price.currency,
        provider,
        source_url,
        dataset,
    })
}

pub(crate) fn history(
    payload: &Value,
    fields: &PriceFields,
) -> KnowledgeResult<Vec<AssetPricePoint>> {
    let mut prices = dated_prices(payload, fields, true)?;
    prices.sort_by_key(|price| price.parsed);
    let points: BTreeMap<_, _> = prices
        .into_iter()
        .filter_map(|price| Some((price.parsed?.date(), price.value)))
        .collect();
    if points.is_empty() {
        return Err(asset_error("price_fields", "No dated numbers were found."));
    }
    let mut points: Vec<_> = points
        .into_iter()
        .rev()
        .take(4000)
        .map(|(date, value)| AssetPricePoint {
            date: date.to_string(),
            value,
        })
        .collect();
    points.reverse();
    Ok(points)
}

pub(crate) fn rolling_arguments(
    arguments: &Value,
    rolling: &RollingDates,
    today: NaiveDate,
) -> KnowledgeResult<Value> {
    let mut arguments = arguments.clone();
    let object = arguments
        .as_object_mut()
        .ok_or_else(|| asset_error("price_rolling", "Use two ISO date arguments."))?;
    let keys = [&rolling.from_arg, &rolling.to_arg];
    if keys[0] == keys[1]
        || !(1..=3650).contains(&rolling.days)
        || !keys.iter().all(|key| {
            object
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
        })
    {
        return Err(asset_error(
            "price_rolling",
            "Use two existing ISO date arguments and 1–3650 days.",
        ));
    }
    let from = today
        .checked_sub_signed(Duration::days(i64::from(rolling.days)))
        .ok_or_else(|| asset_error("price_rolling", "Invalid rolling date."))?;
    object.insert(rolling.from_arg.clone(), Value::String(from.to_string()));
    object.insert(rolling.to_arg.clone(), Value::String(today.to_string()));
    Ok(arguments)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveRequest {
    #[serde(rename = "assetId")]
    pub asset_id: String,
    pub citation_url: String,
    pub fields: PriceFields,
    pub rolling: Option<RollingDates>,
    pub history: Option<HistoryRequest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryRequest {
    pub citation_url: String,
    pub fields: PriceFields,
    pub rolling: RollingDates,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::assets::wire::SavedReadCall;
    use serde_json::json;

    fn fields() -> PriceFields {
        PriceFields {
            value: "/close".into(),
            date: "/date".into(),
            currency: Some("/currency".into()),
            rows: Some("/bars".into()),
        }
    }

    fn fixture_dataset() -> crate::datasets::DatasetRef {
        crate::datasets::DatasetRef {
            dataset_id: "test-prices".into(),
            revision: 1,
            digest: "test-digest".into(),
        }
    }

    #[test]
    fn a_row_read_takes_a_currency_the_row_lacks_from_the_whole_result() {
        let payload = json!({"instrument":{"currency":"KRW"},"bars":[
            {"close":"72500","date":"2026-02-27"},
            {"close":"73400","date":"2026-03-03"},
        ]});
        let fields = PriceFields {
            value: "/close".into(),
            date: "/date".into(),
            currency: Some("/instrument/currency".into()),
            rows: Some("/bars".into()),
        };
        let price = price(&payload, &fields, "KRX".into(), None, fixture_dataset()).unwrap();
        assert_eq!(
            (price.value, price.date.as_str(), price.currency.as_deref()),
            (73400.0, "2026-03-03", Some("KRW"))
        );
    }

    #[test]
    fn saved_price_uses_provider_date_and_a_strict_finite_value() {
        let payload = json!({"bars":[
            {"close":"73400","date":"2026-03-03","currency":"KRW"},
            {"close":73100,"date":"2026-03-02"},
            {"close":"1e12","date":"2026-03-04"},
            {"close":"01.2","date":"2026-03-05"},
            {"close":"NaN","date":"2026-03-06"},
        ]});
        let price = price(&payload, &fields(), "KRX".into(), None, fixture_dataset()).unwrap();
        assert_eq!(
            (price.value, price.date.as_str(), price.currency.as_deref()),
            (73400.0, "2026-03-03", Some("KRW"))
        );
        assert_eq!(serde_json::to_value(&price).unwrap()["kind"], "close");
        assert_eq!(payload["bars"][0]["close"], "73400");
        let points = history(&payload, &fields()).unwrap();
        assert_eq!(
            points.iter().map(|point| point.value).collect::<Vec<_>>(),
            vec![73100.0, 73400.0]
        );
    }

    #[test]
    fn observation_kind_comes_from_the_selected_provider_field() {
        for (field, expected) in [
            ("bid", Some(AssetQuoteKind::Bid)),
            ("ask_price", Some(AssetQuoteKind::Ask)),
            ("last_price", Some(AssetQuoteKind::Last)),
            ("adjusted_close", Some(AssetQuoteKind::AdjustedClose)),
            ("price", None),
            ("bid_size", None),
        ] {
            let payload = json!({"quote":{field: 12.0}, "date":"2026-03-03"});
            let fields = PriceFields {
                value: format!("/quote/{field}"),
                rows: None,
                ..fields()
            };
            let observation = price(
                &payload,
                &fields,
                "provider".into(),
                None,
                fixture_dataset(),
            )
            .unwrap();
            assert_eq!(observation.kind, expected);
            let mut legacy = serde_json::to_value(observation).unwrap();
            assert_eq!(legacy.get("kind").unwrap().is_null(), expected.is_none());
            legacy.as_object_mut().unwrap().remove("kind");
            assert_eq!(
                serde_json::from_value::<AssetStoredPrice>(legacy)
                    .unwrap()
                    .kind,
                None
            );
        }
    }

    #[test]
    fn price_rows_use_calendar_order_and_reject_ambiguous_dates() {
        let mut payload = json!({"bars":[
            {"close":100,"date":"2026-01-13T15:30:00+09:00"},
            {"close":90,"date":"12/31/2025"},
            {"close":95,"date":"13/01/2026"},
            {"close":92,"date":"20260102"},
        ]});
        let latest = price(&payload, &fields(), "KRX".into(), None, fixture_dataset()).unwrap();
        assert_eq!(latest.value, 100.0);
        assert_eq!(latest.date, "2026-01-13T15:30:00+09:00");
        let chart = history(&payload, &fields()).unwrap();
        assert_eq!(
            chart
                .iter()
                .map(|p| (p.date.as_str(), p.value))
                .collect::<Vec<_>>(),
            vec![
                ("2025-12-31", 90.0),
                ("2026-01-02", 92.0),
                ("2026-01-13", 100.0)
            ]
        );
        payload["bars"][1]["date"] = json!("01/02/2026");
        for error in [
            price(&payload, &fields(), "KRX".into(), None, fixture_dataset()).unwrap_err(),
            history(&payload, &fields()).unwrap_err(),
        ] {
            assert!(
                matches!(error, crate::knowledge::KnowledgeError::AssetField { expected, .. } if expected.contains("ISO dates"))
            );
        }
    }

    #[test]
    fn failed_and_revoked_replays_keep_last_price_and_stale_results_lose() {
        let store = crate::store::SqliteStore::open_in_memory().unwrap();
        store
            .create_loupist(&crate::test_fixtures::loupist_profile("l"))
            .unwrap();
        let asset = store
            .seed_followed_listing("l", "005930", "KRX", Some("Samsung"))
            .unwrap();
        let mut db = store.lock().unwrap();
        let tx = db.transaction().unwrap();
        super::super::ensure_asset_page(&tx, "l", &asset, 1).unwrap();
        let price = AssetStoredPrice {
            kind: Some(AssetQuoteKind::Close),
            value: 73400.0,
            date: "2026-03-03".into(),
            currency: Some("KRW".into()),
            provider: "KRX".into(),
            source_url: None,
            dataset: fixture_dataset(),
        };
        let call = SavedReadCall {
            tool: "krx_daily_quote".into(),
            server: None,
            arguments: json!({"symbol":"005930"}),
            fields: fields(),
            rolling: None,
        };
        save(
            &tx,
            "l",
            &StagedPriceRead {
                asset_id: asset.asset_id.clone(),
                read: SavedPriceRead {
                    state: PriceReadState::Ready,
                    current: call.clone(),
                    history: Some(call),
                },
                price: price.clone(),
                history: Vec::new(),
            },
        )
        .unwrap();
        let id = &asset.asset_id;
        let original = load(&tx, "l", id).unwrap().unwrap();
        assert!(settle(&tx, "l", id, &original, Err(PriceReadState::Failed)).unwrap());
        assert!(!settle(&tx, "l", id, &original, Ok(price.clone())).unwrap());
        let failed = load(&tx, "l", id).unwrap().unwrap();
        assert!(settle(&tx, "l", id, &failed, Err(PriceReadState::NeedsConnection)).unwrap());
        let revoked = load(&tx, "l", id).unwrap().unwrap();
        assert_eq!(revoked.read.state, PriceReadState::NeedsConnection);
        assert_eq!(revoked.price, Some(price.clone()));
        assert_eq!(points(&tx, "l", id).unwrap().len(), 1);

        let newer = AssetStoredPrice {
            value: 74_100.0,
            ..price
        };
        assert!(settle(&tx, "l", id, &revoked, Ok(newer)).unwrap());
        settle_history(
            &tx,
            "l",
            id,
            &revoked,
            &[
                AssetPricePoint {
                    date: "2026-03-02".into(),
                    value: 73_100.0,
                },
                AssetPricePoint {
                    date: "2026-03-03".into(),
                    value: 73_400.0,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            points(&tx, "l", id)
                .unwrap()
                .iter()
                .map(|point| point.value)
                .collect::<Vec<_>>(),
            vec![73_100.0, 74_100.0],
        );
    }
}
