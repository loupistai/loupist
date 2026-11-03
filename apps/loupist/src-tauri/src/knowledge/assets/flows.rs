//! Korean investor flows for the asset page (finance-korea-retail-disagreement.md
//! §2.1, §3.1): ten-day net buying by foreign investors, institutions and
//! individuals, the short-selling share of volume and the change in the
//! lending balance, each computed through `calculate` with its formula and
//! dated by the last day it covers. Flows are not prices.
use std::sync::Arc;

use chrono::NaiveDate;
use serde_json::{json, Value};

use super::{observe, AssetEvidence, NewObservation, ObservationKind, StoredObservation};

/// Trading days the section sums.
pub const FLOW_DAYS: usize = 10;
/// KIS reports net buying values (`*_ntby_tr_pbmn`) in millions of won.
pub const KIS_AMOUNT_SCALE: f64 = 1_000_000.0;
const MAX_FACTS_BYTES: usize = 4000;

/// The three Korea Investment reads behind the section.
pub struct FlowReads<'a> {
    /// `domestic_stock.investor_trade_by_stock_daily`.
    pub investor: observe::ToolRead<'a>,
    /// `domestic_stock.daily_short_sale`.
    pub short: Option<observe::ToolRead<'a>>,
    /// `domestic_stock.daily_loan_trans`.
    pub loan: Option<observe::ToolRead<'a>>,
}

struct NoDatasets;

impl crate::calculations::DatasetSource for NoDatasets {
    fn dataset(
        &mut self,
        _: &str,
        _: u32,
    ) -> Result<(crate::datasets::DatasetRef, Arc<Value>), String> {
        Err("this calculation reads no dataset".into())
    }
}

fn number(row: &Value, field: &str) -> Option<f64> {
    match row.get(field)? {
        Value::String(text) => text.trim().replace(',', "").parse::<f64>().ok(),
        Value::Number(number) => number.as_f64(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

/// Dated rows of one KIS container, newest first, one per day.
fn dated_rows(payload: &Value, pointer: &str, date_field: &str) -> Vec<(NaiveDate, usize, Value)> {
    let mut rows: Vec<(NaiveDate, usize, Value)> = payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, row)| {
            let date = row
                .get(date_field)
                .and_then(Value::as_str)
                .and_then(observe::parse_day)?;
            Some((date, index, row.clone()))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    rows.dedup_by_key(|row| row.0);
    rows
}

fn calculate(request: Value) -> Option<Value> {
    crate::calculations::calculate(&request, &mut NoDatasets).ok()
}

fn value_of(result: &Value, name: &str) -> Option<f64> {
    result["values"]
        .as_array()?
        .iter()
        .find(|item| item["name"] == name)?["value"]
        .as_f64()
}

fn formulas(result: &Value, take: usize) -> Vec<Value> {
    result["formulas"]
        .as_array()
        .into_iter()
        .flatten()
        .take(take)
        .cloned()
        .collect()
}

/// A ten-day sum of one net-buying column, in KIS's millions of won.
fn net_row(
    key: &str,
    title: &str,
    field: &str,
    window: &[(NaiveDate, usize, Value)],
) -> Option<Value> {
    let ascending: Vec<_> = window.iter().rev().collect();
    let series: Vec<f64> = ascending
        .iter()
        .map(|(_, _, row)| number(row, field))
        .collect::<Option<_>>()?;
    let dates: Vec<String> = ascending
        .iter()
        .map(|(date, ..)| date.to_string())
        .collect();
    let result = calculate(json!({
        "operation": "sum", "series": series, "dates": dates, "unit": "KRW millions"
    }))?;
    let total = value_of(&result, "total")?;
    Some(json!({
        "key": key,
        "value": total * KIS_AMOUNT_SCALE,
        "unit": "KRW",
        "source": "investor",
        "calc": {
            "title": title,
            "formulas": formulas(&result, 1),
            "inputs": [{"name": "trading days", "display": series.len().to_string()}],
            "display": format!("{} KRW millions", result["values"][0]["display"].as_str().unwrap_or_default()),
            "value": total,
            "unit": "KRW millions",
        },
    }))
}

/// Short volume ÷ volume over the window's days.
fn short_row(window: &[NaiveDate], payload: &Value) -> Option<(Value, usize, NaiveDate)> {
    let rows = dated_rows(payload, "/data/response/output2", "stck_bsop_date");
    let days: Vec<_> = rows
        .iter()
        .filter(|(date, ..)| window.contains(date))
        .collect();
    let latest = days.first()?;
    let short: f64 = days
        .iter()
        .map(|(_, _, row)| number(row, "ssts_cntg_qty"))
        .sum::<Option<f64>>()?;
    let volume: f64 = days
        .iter()
        .map(|(_, _, row)| number(row, "acml_vol"))
        .sum::<Option<f64>>()?;
    let result = calculate(json!({
        "operation": "ratio", "as_percent": true,
        "inputs": {"numerator": short, "denominator": volume},
        "labels": {"numerator": "Short-sale volume", "denominator": "Volume"},
    }))?;
    let share = value_of(&result, "ratio")?;
    Some((
        json!({
            "key": "shortSelling",
            "value": share,
            "unit": "%",
            "source": "short",
            "calc": {
                "title": "Short selling share of volume",
                "formulas": [format!("Sums over {} trading days: {}", days.len(), formulas(&result, 1).first().and_then(Value::as_str).unwrap_or_default())],
                "inputs": [
                    {"name": "short-sale volume", "display": result["inputs"][0]["display"]},
                    {"name": "volume", "display": result["inputs"][1]["display"]},
                ],
                "display": result["values"][0]["display"],
                "value": share,
                "unit": "%",
            },
        }),
        latest.1,
        latest.0,
    ))
}

/// The lending balance (shares) from the window's first day to the latest
/// settled day; balances settle two business days after trading.
fn lending_row(
    first: NaiveDate,
    last_final: NaiveDate,
    payload: &Value,
) -> Option<(Value, usize, NaiveDate)> {
    let rows = dated_rows(payload, "/data/response/output1", "bsop_date");
    let settled: Vec<_> = rows
        .iter()
        .filter(|(date, ..)| *date <= last_final)
        .collect();
    let latest = settled.first()?;
    let start = settled
        .iter()
        .find(|(date, ..)| *date <= first)
        .or_else(|| settled.last())?;
    if start.0 >= latest.0 {
        return None;
    }
    let (from, to) = (
        number(&start.2, "rmnd_stcn")?,
        number(&latest.2, "rmnd_stcn")?,
    );
    let result = calculate(json!({
        "operation": "percent_change",
        "inputs": {"from": from, "to": to},
        "labels": {"from": format!("balance on {}", start.0), "to": format!("balance on {}", latest.0)},
        "unit": "shares",
    }))?;
    let change = value_of(&result, "percent_change")?;
    Some((
        json!({
            "key": "lending",
            "value": change,
            "unit": "%",
            "to": latest.0.to_string(),
            "source": "loan",
            "calc": {
                "title": "Lending balance change",
                "formulas": formulas(&result, 2).into_iter().skip(1).collect::<Vec<_>>(),
                "inputs": [
                    {"name": format!("balance on {}", start.0), "display": result["inputs"][0]["display"]},
                    {"name": format!("balance on {}", latest.0), "display": result["inputs"][1]["display"]},
                ],
                "display": result["values"][1]["display"],
                "value": change,
                "unit": "%",
            },
        }),
        latest.1,
        latest.0,
    ))
}

/// The section's observation: the last `FLOW_DAYS` completed sessions on or
/// before `last_final` (the latest session whose flows are final). `None`
/// when the investor read has no such day.
pub fn summary(reads: &FlowReads<'_>, last_final: NaiveDate) -> Option<NewObservation> {
    let all = dated_rows(
        reads.investor.payload,
        "/data/response/output2",
        "stck_bsop_date",
    );
    let window: Vec<_> = all
        .into_iter()
        .filter(|(date, ..)| *date <= last_final)
        .take(FLOW_DAYS)
        .collect();
    let (to, latest_index, _) = window.first()?.clone();
    let first = window.last()?.0;
    let mut rows = Vec::new();
    for (key, title, field) in [
        ("foreign", "Foreign net buying", "frgn_ntby_tr_pbmn"),
        (
            "institutions",
            "Institutions' net buying",
            "orgn_ntby_tr_pbmn",
        ),
        (
            "individuals",
            "Individuals' net buying",
            "prsn_ntby_tr_pbmn",
        ),
    ] {
        rows.extend(net_row(key, title, field, &window));
    }
    if rows.is_empty() {
        return None;
    }
    let days: Vec<NaiveDate> = window.iter().map(|(date, ..)| *date).collect();
    let mut sources = serde_json::Map::new();
    if let Some(read) = &reads.short {
        if let Some((row, index, date)) = short_row(&days, read.payload) {
            rows.push(row);
            sources.insert(
                "short".into(),
                json!(observe::evidence_for(
                    read,
                    format!("/data/response/output2/{index}"),
                    None,
                    date
                )),
            );
        }
    }
    if let Some(read) = &reads.loan {
        if let Some((row, index, date)) = lending_row(first, last_final, read.payload) {
            rows.push(row);
            sources.insert(
                "loan".into(),
                json!(observe::evidence_for(
                    read,
                    format!("/data/response/output1/{index}"),
                    None,
                    date
                )),
            );
        }
    }
    let mut facts = json!({
        "days": window.len(),
        "to": to.to_string(),
        "rows": rows,
        "sources": sources,
    });
    // Facts are bounded: shorten the formulas before giving up a row.
    if serde_json::to_string(&facts).map_or(0, |text| text.len()) > MAX_FACTS_BYTES {
        for row in facts["rows"].as_array_mut().into_iter().flatten() {
            row["calc"]["inputs"] = json!([]);
        }
    }
    Some(NewObservation {
        kind: ObservationKind::InvestorFlows,
        fact_date: to.to_string(),
        dedupe_key: format!("investor_flows:{to}"),
        facts,
        evidence: observe::evidence_for(
            &reads.investor,
            format!("/data/response/output2/{latest_index}"),
            None,
            to,
        ),
    })
}

/// The latest session whose flows are final: today in Seoul from 18:30 on a
/// KRX trading day, else the trading day before (`market_calendar`: published
/// KRX holidays are skipped; an unpublished year counts every weekday).
pub fn last_final_session(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    use chrono::Timelike;
    let seoul = now.with_timezone(&chrono_tz::Asia::Seoul);
    let today = seoul.date_naive();
    if crate::market_calendar::is_trading_day("KRX", today)
        && (seoul.hour(), seoul.minute()) >= (18, 30)
    {
        today
    } else {
        crate::market_calendar::previous_trading_day("KRX", today)
    }
}

/// A minimum gap between reads when the summary cannot catch up (a failed
/// read, or a holiday in a year KRX has not published).
const RETRY_MS: i64 = 6 * 60 * 60 * 1000;

/// Whether to read the flows now: the stored summary ends before the latest
/// final session, and the last attempt was at least six hours ago.
pub fn due(
    latest: Option<NaiveDate>,
    attempted_ms: Option<i64>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let behind = latest.is_none_or(|to| to < last_final_session(now));
    behind && attempted_ms.is_none_or(|at| now.timestamp_millis() - at >= RETRY_MS)
}

/// The latest summary as the page shows it: each row with its own source.
pub fn view(observed: &[StoredObservation]) -> Option<Value> {
    let latest = observed
        .iter()
        .filter(|item| item.kind == ObservationKind::InvestorFlows)
        .max_by(|a, b| {
            a.fact_date
                .cmp(&b.fact_date)
                .then(a.recorded_at_ms.cmp(&b.recorded_at_ms))
        })?;
    let source = |name: &str| -> Option<AssetEvidence> {
        match name {
            "investor" => Some(latest.evidence.clone()),
            other => serde_json::from_value(latest.facts["sources"][other].clone()).ok(),
        }
    };
    let rows: Vec<Value> = latest.facts["rows"]
        .as_array()?
        .iter()
        .filter_map(|row| {
            let evidence = source(row["source"].as_str().unwrap_or("investor"))?;
            let mut row = row.clone();
            let object = row.as_object_mut()?;
            object.remove("source");
            object.insert("evidence".into(), json!([evidence]));
            Some(row)
        })
        .collect();
    (!rows.is_empty()).then(|| {
        json!({
            "days": latest.facts["days"],
            "to": latest.facts["to"],
            "rows": rows,
        })
    })
}

/// What the model reads: the numbers and their dates, without formulas.
pub fn model_summary(flows: &Value) -> Value {
    json!({
        "tradingDays": flows["days"],
        "to": flows["to"],
        "rows": flows["rows"].as_array().into_iter().flatten().map(|row| json!({
            "key": row["key"], "value": row["value"], "unit": row["unit"], "to": row["to"],
        })).collect::<Vec<_>>(),
        "note": "Net buying is in won over the trading days to the date shown; flows are final after the close and lending balances settle two business days later. Credit and short-sale balances are not read.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read<'a>(tool: &'a str, payload: &'a Value) -> observe::ToolRead<'a> {
        observe::ToolRead {
            tool,
            provider: Some("koreainvestment.market-data"),
            payload,
            dataset: Some(crate::datasets::DatasetRef {
                dataset_id: format!("ds_{tool}"),
                revision: 1,
                digest: "d".repeat(64),
            }),
            retrieved_at: Some("2026-10-29T10:00:00Z"),
            citation_url: None,
            today: NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(),
            session_day: NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(),
        }
    }

    fn day(offset: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 29).unwrap() - chrono::Duration::days(offset)
    }

    #[test]
    fn ten_day_sums_short_share_and_lending_change_carry_their_dates_and_formulas() {
        // Twelve sessions; the newest (Oct 29) is still trading and is left out.
        let investor = json!({"data": {"response": {"output2": (0..12).map(|i| json!({
            "stck_bsop_date": day(i).format("%Y%m%d").to_string(),
            "frgn_ntby_tr_pbmn": (1000 + i).to_string(),
            "orgn_ntby_tr_pbmn": "-400",
            "prsn_ntby_tr_pbmn": (-600 - i).to_string(),
        })).collect::<Vec<_>>()}}});
        let short = json!({"data": {"response": {"output2": (0..12).map(|i| json!({
            "stck_bsop_date": day(i).format("%Y%m%d").to_string(),
            "ssts_cntg_qty": "42", "acml_vol": "1000",
        })).collect::<Vec<_>>()}}});
        let loan = json!({"data": {"response": {"output1": (2..14).map(|i| json!({
            "bsop_date": day(i).format("%Y%m%d").to_string(),
            "rmnd_stcn": (10000 + 100 * i).to_string(),
        })).collect::<Vec<_>>()}}});
        let reads = FlowReads {
            investor: read("kis_read", &investor),
            short: Some(read("kis_read", &short)),
            loan: Some(read("kis_read", &loan)),
        };
        let observation = summary(&reads, day(1)).unwrap();
        assert_eq!(observation.kind, ObservationKind::InvestorFlows);
        assert_eq!(observation.fact_date, "2026-10-28");
        let facts = &observation.facts;
        assert_eq!(facts["days"], 10);
        let row = |key: &str| {
            facts["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap()
                .clone()
        };
        // Foreign: Σ (1000 + i) for i = 1..=10 = 10,055 million won.
        assert_eq!(row("foreign")["value"], 10_055.0 * KIS_AMOUNT_SCALE);
        assert_eq!(row("institutions")["value"], -4_000.0 * KIS_AMOUNT_SCALE);
        assert!(row("foreign")["calc"]["formulas"][0]
            .as_str()
            .unwrap()
            .contains("10 values"));
        assert_eq!(row("shortSelling")["value"], 4.2);
        // Lending: settled to Oct 27 (T-2); from the balance on the window's first day (Oct 19).
        let lending = row("lending");
        assert_eq!(lending["to"], "2026-10-27");
        let expected = (10_200.0 / 11_000.0 - 1.0) * 100.0;
        assert!((lending["value"].as_f64().unwrap() - expected).abs() < 1e-9);
        assert!(serde_json::to_string(facts).unwrap().len() <= 4096);

        let stored = StoredObservation {
            observation_id: "aob_1".into(),
            kind: ObservationKind::InvestorFlows,
            fact_date: observation.fact_date.clone(),
            recorded_at_ms: 1,
            facts: observation.facts.clone(),
            evidence: observation.evidence.clone(),
            baseline: true,
            dedupe_key: observation.dedupe_key.clone(),
        };
        let view = view(&[stored]).unwrap();
        assert_eq!(view["to"], "2026-10-28");
        let rows = view["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 5);
        assert!(rows
            .iter()
            .all(|row| row["evidence"][0]["dataset"].is_object()));
        assert!(rows.iter().all(|row| row.get("source").is_none()));
        assert_eq!(model_summary(&view)["rows"][4]["to"], "2026-10-27");
    }

    #[test]
    fn flows_are_read_once_a_day_after_the_close_in_seoul() {
        let at = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&chrono::Utc)
        };
        let date = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
        // Thursday Oct 29 2026: 18:29 and 18:30 in Seoul.
        assert_eq!(
            last_final_session(at("2026-10-29T18:29:00+09:00")),
            date("2026-10-28")
        );
        assert_eq!(
            last_final_session(at("2026-10-29T18:30:00+09:00")),
            date("2026-10-29")
        );
        // Monday morning reads Friday's flows; Sunday too.
        assert_eq!(
            last_final_session(at("2026-11-02T09:00:00+09:00")),
            date("2026-10-30")
        );
        assert_eq!(
            last_final_session(at("2026-11-01T20:00:00+09:00")),
            date("2026-10-30")
        );
        // A published KRX holiday has no session: the evening of Hangeul Day
        // (Fri Oct 9) and the Tuesday morning after the Monday holiday (Oct 5)
        // both read the trading day before.
        assert_eq!(
            last_final_session(at("2026-10-09T19:00:00+09:00")),
            date("2026-10-08")
        );
        assert_eq!(
            last_final_session(at("2026-10-06T09:00:00+09:00")),
            date("2026-10-02")
        );
        // KRX has not published 2027, so Seollal (Mon Feb 8) counts as a session.
        assert_eq!(
            last_final_session(at("2027-02-08T19:00:00+09:00")),
            date("2027-02-08")
        );
        let now = at("2026-10-29T19:00:00+09:00");
        assert!(due(None, None, now));
        assert!(due(Some(date("2026-10-28")), None, now));
        assert!(!due(Some(date("2026-10-29")), None, now));
        // A read an hour ago that could not catch up waits six hours.
        assert!(!due(
            Some(date("2026-10-28")),
            Some(now.timestamp_millis() - 3_600_000),
            now
        ));
    }

    #[test]
    fn without_a_final_session_or_net_columns_there_is_no_section() {
        let empty = json!({"data": {"response": {"output2": []}}});
        let reads = FlowReads {
            investor: read("kis_read", &empty),
            short: None,
            loan: None,
        };
        assert!(summary(&reads, day(1)).is_none());
        let only_today = json!({"data": {"response": {"output2": [
            {"stck_bsop_date": "20261029", "frgn_ntby_tr_pbmn": "1", "orgn_ntby_tr_pbmn": "1", "prsn_ntby_tr_pbmn": "1"}
        ]}}});
        let reads = FlowReads {
            investor: read("kis_read", &only_today),
            short: None,
            loan: None,
        };
        assert!(summary(&reads, day(1)).is_none());
    }
}
