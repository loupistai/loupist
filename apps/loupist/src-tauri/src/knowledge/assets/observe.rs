//! Observations from structured tool rows, never from model text. Each route
//! names the rows it reads; every observation carries the row's own date and
//! the evidence that produced it (dataset, row pointer, original URL).
use chrono::NaiveDate;
use serde_json::{json, Value};

use super::{AssetEvidence, ObservationKind};
use crate::datasets::DatasetRef;

/// Closes kept per refresh; the price row needs the latest and one on or before
/// the chosen date, the crypto drawdown 90 days of them.
const MAX_CLOSES: usize = 130;
const MAX_FILINGS: usize = 40;
const TOP_HOLDINGS: usize = 10;

/// One read the host or a turn made, with what it returned.
pub struct ToolRead<'a> {
    pub tool: &'a str,
    pub provider: Option<&'a str>,
    pub payload: &'a Value,
    pub dataset: Option<DatasetRef>,
    pub retrieved_at: Option<&'a str>,
    /// The turn's citation for this result; host refreshes have none.
    pub citation_url: Option<String>,
    pub today: NaiveDate,
    /// The exchange's current day (`class::session_day`): a daily close dated
    /// on or after it may be a session still trading and is not recorded.
    pub session_day: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewObservation {
    pub kind: ObservationKind,
    pub fact_date: String,
    pub dedupe_key: String,
    pub facts: Value,
    pub evidence: AssetEvidence,
}

/// Routes this module reads. Anything else records nothing.
pub fn observes(tool: &str) -> bool {
    matches!(
        tool,
        "dart_filing_search"
            | "dart_financials"
            | "dart_major_report"
            | "dart_dividends"
            | "krx_daily_quote"
            | "equity_fundamental_filings"
            | "equity_fundamental_income"
            | "equity_calendar_earnings"
            | "equity_fundamental_dividends"
            | "equity_price_historical"
            | "etf_historical"
            | "crypto_price_historical"
            | "currency_price_historical"
            | "derivatives_futures_historical"
            | "index_price_historical"
            | "etf_info"
            | "etf_holdings"
            | "fixedincome_government_treasury_rates"
    )
}

/// The rows of a result and the JSON Pointer prefix that addresses them.
pub(crate) fn rows<'a>(payload: &'a Value, native: &str) -> Option<(&'a Vec<Value>, String)> {
    for pointer in [
        native,
        "/structuredContent/results",
        "/result/structuredContent/results",
        "/results",
        "/data",
    ] {
        if pointer.is_empty() {
            continue;
        }
        if let Some(rows) = payload.pointer(pointer).and_then(Value::as_array) {
            return Some((rows, pointer.to_owned()));
        }
    }
    None
}

fn text<'a>(row: &'a Value, fields: &[&str]) -> Option<&'a str> {
    fields.iter().find_map(|field| {
        row.get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

pub(crate) fn number(row: &Value, fields: &[&str]) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| match row.get(*field)? {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().replace(',', "").parse::<f64>().ok(),
            _ => None,
        })
        .filter(|value| value.is_finite())
}

/// `2026-03-18`, `20260318` or an RFC 3339 instant, as a calendar day.
pub fn parse_day(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    if value.len() >= 10 {
        if let Ok(day) = NaiveDate::parse_from_str(&value[..10], "%Y-%m-%d") {
            return Some(day);
        }
    }
    if value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit()) {
        return NaiveDate::parse_from_str(value, "%Y%m%d").ok();
    }
    None
}

pub(crate) fn day(row: &Value, fields: &[&str]) -> Option<NaiveDate> {
    fields
        .iter()
        .find_map(|field| row.get(*field).and_then(Value::as_str).and_then(parse_day))
}

fn safe_url(value: Option<&str>) -> Option<String> {
    let value = value?;
    let parsed = reqwest::Url::parse(value).ok()?;
    (matches!(parsed.scheme(), "https" | "http")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && value.len() <= 2048)
        .then(|| parsed.to_string())
}

fn bounded(text: &str, chars: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(chars)
        .collect()
}

fn source_title(read: &ToolRead<'_>) -> String {
    match (read.tool, read.provider) {
        ("dart_filing_search" | "dart_financials" | "dart_major_report" | "dart_dividends", _) => {
            "OpenDART".into()
        }
        ("krx_daily_quote", _) => "KRX".into(),
        ("kis_read", _) => "Korea Investment".into(),
        ("defillama_read", _) => "DefiLlama".into(),
        ("cftc_cot", _) => "CFTC Commitments of Traders".into(),
        ("commodity_petroleum_status_report", _) => "EIA Weekly Petroleum Status Report".into(),
        ("equity_fundamental_filings", Some("sec")) => "SEC EDGAR".into(),
        (tool, Some(provider)) => format!("{} · {provider}", tool.replace('_', " ")),
        (tool, None) => tool.replace('_', " "),
    }
}

/// The evidence of one row of a read (dataset, row pointer, original URL).
pub fn evidence_for(
    read: &ToolRead<'_>,
    pointer: String,
    url: Option<String>,
    date: NaiveDate,
) -> AssetEvidence {
    evidence(read, pointer, url, date)
}

fn evidence(
    read: &ToolRead<'_>,
    pointer: String,
    url: Option<String>,
    date: NaiveDate,
) -> AssetEvidence {
    AssetEvidence {
        tool: read.tool.to_owned(),
        provider: read.provider.map(str::to_owned),
        title: source_title(read),
        url,
        dataset: read.dataset.clone(),
        pointer: Some(pointer),
        record_id: None,
        world_id: None,
        citation_url: read.citation_url.clone(),
        date: Some(date.to_string()),
        retrieved_at: read.retrieved_at.map(str::to_owned),
    }
}

/// Korean periodic report names, so a 분기보고서 can be compared with the last one.
fn korean_form(title: &str) -> Option<&'static str> {
    ["사업보고서", "반기보고서", "분기보고서", "주요사항보고서"]
        .into_iter()
        .find(|form| title.contains(form))
}

fn dart_filings(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "/data/filings") else {
        return;
    };
    for (index, row) in rows.iter().enumerate().take(MAX_FILINGS) {
        let (Some(id), Some(date)) = (
            text(row, &["filing_id", "rcept_no"]),
            day(row, &["filed_at", "rcept_dt"]),
        ) else {
            continue;
        };
        if date > read.today || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let title = bounded(text(row, &["title", "report_nm"]).unwrap_or(id), 200);
        let form = korean_form(&title);
        let url = safe_url(Some(&format!(
            "https://dart.fss.or.kr/dsaf001/main.do?rcpNo={id}"
        )));
        out.push(NewObservation {
            kind: ObservationKind::Filing,
            fact_date: date.to_string(),
            dedupe_key: format!("filing:dart:{id}"),
            facts: json!({"form": form, "title": title, "filingId": id, "major": title.contains("주요사항보고")}),
            evidence: evidence(read, format!("{prefix}/{index}"), url, date),
        });
    }
}

/// Capital decisions the page shows in plain words (rights offering, bonds
/// that can become shares, buyback). Other decisions stay filing rows.
fn dart_capital(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "/data/rows") else {
        return;
    };
    for (index, row) in rows.iter().enumerate().take(MAX_FILINGS) {
        let (Some(id), Some(date)) = (text(row, &["filing_id"]), day(row, &["filed_at"])) else {
            continue;
        };
        if date > read.today || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let decision = text(row, &["kind"]).unwrap_or_default();
        let mut facts = json!({"decision": decision, "filingId": id, "currency": "KRW"});
        let set = |facts: &mut Value, key: &str, value: Option<f64>| {
            if let Some(value) = value {
                facts[key] = json!(value);
            }
        };
        match decision {
            "rights_offering" => {
                set(&mut facts, "shares", number(row, &["new_shares_common"]));
                facts["method"] = row["method"].clone();
                facts["price"] = json!(number(row, &["issue_price"]));
                facts["paymentDate"] = json!(day(row, &["payment_date"]).map(|d| d.to_string()));
            }
            "convertible_bonds" | "bonds_with_warrants" => {
                set(&mut facts, "amount", number(row, &["face_amount"]));
                set(
                    &mut facts,
                    "conversionPrice",
                    number(row, &["conversion_price"]),
                );
                facts["floor"] = json!(number(row, &["refix_floor"]));
                facts["paymentDate"] = json!(day(row, &["payment_date"]).map(|d| d.to_string()));
            }
            "buyback" => {
                set(
                    &mut facts,
                    "amount",
                    number(row, &["planned_amount_common"]),
                );
                if let Some(by) = day(row, &["buy_until"]) {
                    facts["by"] = json!(by.to_string());
                }
            }
            _ => continue,
        }
        if let Some(value) = number(row, &["dilution_pct"]) {
            let basis = text(row, &["dilution_basis"]).unwrap_or("filing");
            let mut dilution = json!({
                "value": value,
                "display": format!("{}%", text(row, &["dilution_pct"]).unwrap_or_default()),
                "basis": basis,
            });
            if basis == "calculated" {
                dilution["calc"] = row["dilution_calculation"].clone();
            }
            // Bonds: at the starting price as filed, or the most shares at the reset floor.
            if let Some(at) =
                text(row, &["dilution_at"]).filter(|at| matches!(*at, "price" | "floor"))
            {
                dilution["at"] = json!(at);
            }
            facts["dilution"] = dilution;
        }
        let url = safe_url(text(row, &["url"]));
        out.push(NewObservation {
            kind: ObservationKind::Capital,
            fact_date: date.to_string(),
            dedupe_key: format!("capital:{id}"),
            facts,
            evidence: evidence(read, format!("{prefix}/{index}"), url, date),
        });
    }
}

/// The year's dividend per common share and its record date (배당기준일),
/// or that the date is only in the decision notice.
fn dart_dividends(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "/data/rows") else {
        return;
    };
    let year = read.payload.pointer("/data/year").and_then(Value::as_i64);
    let Some((index, row)) = rows.iter().enumerate().find(|(_, row)| {
        row["item"] == "dps_cash"
            && row["share_class"] == "common"
            && row["fiscal_year"].as_i64() == year
    }) else {
        return;
    };
    let (Some(amount), Some(filed)) = (number(row, &["value"]), day(row, &["filed_at"])) else {
        return;
    };
    let record = read.payload.pointer("/data/record_date");
    let record_date = record
        .and_then(|record| record.get("date"))
        .and_then(Value::as_str)
        .and_then(parse_day);
    let notice_filed = record
        .and_then(|record| record.get("filed_at"))
        .and_then(Value::as_str)
        .and_then(parse_day)
        .filter(|date| *date <= read.today);
    let in_notice = record.and_then(|record| record.get("status")) == Some(&json!("in_notice"));
    let mut facts = json!({"amount": amount, "currency": "KRW", "fiscalYear": year});
    if let Some(date) = record_date {
        facts["recordDate"] = json!(date.to_string());
    } else if in_notice {
        facts["recordInNotice"] = json!(true);
    }
    // A record date read from the notice is dated and sourced by the notice.
    let (fact, url, pointer) = match (record_date, notice_filed) {
        (Some(_), Some(notice)) => (
            notice,
            safe_url(record.and_then(|record| record["url"].as_str())),
            "/data/record_date".to_owned(),
        ),
        _ => (
            filed,
            text(row, &["filing_id"]).and_then(|id| {
                safe_url(Some(&format!(
                    "https://dart.fss.or.kr/dsaf001/main.do?rcpNo={id}"
                )))
            }),
            format!("{prefix}/{index}"),
        ),
    };
    if fact > read.today {
        return;
    }
    out.push(NewObservation {
        kind: ObservationKind::Dividend,
        fact_date: fact.to_string(),
        dedupe_key: format!(
            "dividend:dart:{}:{}",
            year.unwrap_or_default(),
            record_date.map_or("unknown".to_owned(), |date| date.to_string())
        ),
        facts,
        evidence: evidence(read, pointer, url, fact),
    });
}

fn sec_filings(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    for (index, row) in rows.iter().enumerate().take(MAX_FILINGS) {
        let Some(date) = day(row, &["filing_date", "date", "filed_at", "accepted_date"]) else {
            continue;
        };
        if date > read.today {
            continue;
        }
        let url = safe_url(text(
            row,
            &[
                "report_url",
                "filing_url",
                "url",
                "link",
                "filing_detail_url",
                "final_link",
            ],
        ));
        let id = text(row, &["accession_number", "accession", "accession_no"])
            .map(str::to_owned)
            .or_else(|| url.clone());
        let Some(id) = id else { continue };
        let form =
            text(row, &["report_type", "form_type", "form", "type"]).map(|form| bounded(form, 24));
        let title = bounded(
            text(row, &["primary_doc_description", "description", "title"])
                .or(form.as_deref())
                .unwrap_or("Filing"),
            200,
        );
        let mut facts = json!({"form": form, "title": title, "filingId": bounded(&id, 300)});
        // 8-K items ("2.02,9.01") say whether a current report is a results release.
        if let Some(items) = text(row, &["items", "item", "report_items"]) {
            facts["items"] = json!(bounded(items, 80));
        }
        out.push(NewObservation {
            kind: ObservationKind::Filing,
            fact_date: date.to_string(),
            dedupe_key: format!("filing:{}", bounded(&id, 300)),
            facts,
            evidence: evidence(read, format!("{prefix}/{index}"), url, date),
        });
    }
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

fn dart_results(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((facts, prefix)) = rows(read.payload, "/data/facts") else {
        return;
    };
    let pick = |concept: &str| {
        facts
            .iter()
            .enumerate()
            .filter(|(_, fact)| fact["canonical_concept"] == concept)
            .find(|(_, fact)| matches!(fact["period_kind"].as_str(), Some("quarter" | "annual")))
            .and_then(|(index, fact)| number(fact, &["value"]).map(|value| (index, fact, value)))
    };
    let Some((revenue_index, revenue_fact, revenue)) = pick("revenue") else {
        return;
    };
    let filed = facts
        .iter()
        .filter_map(|fact| day(fact, &["filed_at"]))
        .filter(|date| *date <= read.today)
        .max();
    let Some(date) = filed else { return };
    let period = &read
        .payload
        .pointer("/data/period")
        .cloned()
        .unwrap_or(Value::Null);
    let year = period["fiscal_year"].as_i64().unwrap_or_default();
    let report = period["report_period"].as_str().unwrap_or("annual");
    let label = match report {
        "q1" => format!("Q1 {year}"),
        "half_year" => format!("H1 {year}"),
        "q3" => format!("Q3 {year}"),
        _ => format!("FY{year}"),
    };
    let (metric, value, unit, index) = match pick("operating_income") {
        Some((index, _, income)) if revenue > 0.0 => (
            "operating_margin",
            round(income / revenue * 100.0, 1),
            "%".to_owned(),
            index,
        ),
        _ => (
            "revenue",
            revenue,
            revenue_fact["unit"].as_str().unwrap_or_default().to_owned(),
            revenue_index,
        ),
    };
    out.push(NewObservation {
        kind: ObservationKind::Results,
        fact_date: date.to_string(),
        dedupe_key: format!(
            "results:dart:{year}:{report}:{}",
            period["basis"].as_str().unwrap_or_default()
        ),
        facts: json!({"period": label, "metric": metric, "value": value, "unit": unit}),
        evidence: evidence(read, format!("{prefix}/{index}"), None, date),
    });
}

fn income_results(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    let mut dated: Vec<(usize, NaiveDate, &Value)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            day(row, &["period_ending", "date", "fiscal_period_end"]).map(|date| (index, date, row))
        })
        .filter(|(_, date, _)| *date <= read.today)
        .collect();
    dated.sort_by_key(|(_, date, _)| std::cmp::Reverse(*date));
    let metric = |row: &Value| -> Option<(&'static str, f64, &'static str)> {
        let revenue = number(row, &["revenue", "total_revenue", "totalRevenue"])?;
        match number(
            row,
            &[
                "operating_income",
                "total_operating_income",
                "operatingIncome",
            ],
        ) {
            Some(income) if revenue > 0.0 => {
                Some(("operating_margin", round(income / revenue * 100.0, 1), "%"))
            }
            _ => Some(("revenue", revenue, "")),
        }
    };
    let Some((index, period_end, row)) = dated.first().copied() else {
        return;
    };
    let Some((name, value, unit)) = metric(row) else {
        return;
    };
    let prior = dated
        .get(1)
        .and_then(|(_, _, row)| metric(row))
        .filter(|(prior_name, ..)| *prior_name == name)
        .map(|(_, value, _)| value);
    let fiscal = text(row, &["fiscal_period"]).map(str::to_owned);
    let year = row
        .get("fiscal_year")
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()));
    let label = match (fiscal, year) {
        (Some(period), Some(year)) => format!("{period} {year}"),
        _ => period_end.format("%Y-%m-%d").to_string(),
    };
    let date = day(row, &["filing_date", "accepted_date"])
        .filter(|date| *date <= read.today)
        .unwrap_or(period_end);
    out.push(NewObservation {
        kind: ObservationKind::Results,
        fact_date: date.to_string(),
        dedupe_key: format!("results:{period_end}"),
        facts: json!({"period": label, "metric": name, "value": value, "unit": unit, "prior": prior}),
        evidence: evidence(read, format!("{prefix}/{index}"), None, date),
    });
}

fn earnings(read: &ToolRead<'_>, symbol: &str, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    for (index, row) in rows.iter().enumerate() {
        if text(row, &["symbol"]).is_some_and(|value| !value.eq_ignore_ascii_case(symbol)) {
            continue;
        }
        let Some(date) = day(row, &["report_date", "date"]) else {
            continue;
        };
        if date < read.today {
            continue;
        }
        let period = text(row, &["fiscal_period", "period"]).map(|value| bounded(value, 24));
        // The fiscal period the date reports, when the calendar says so.
        let period_end = day(
            row,
            &[
                "period_ending",
                "fiscal_period_end",
                "period_end",
                "fiscal_quarter_ending",
            ],
        )
        .filter(|end| *end < date)
        .map(|end| end.to_string());
        // The announcement is what was observed today; its date is in the facts.
        out.push(NewObservation {
            kind: ObservationKind::Earnings,
            fact_date: read.today.to_string(),
            dedupe_key: format!("earnings:{date}"),
            facts: json!({"date": date.to_string(), "period": period, "periodEnd": period_end}),
            evidence: evidence(read, format!("{prefix}/{index}"), None, read.today),
        });
        break;
    }
}

fn dividends(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    let mut seen = 0;
    for (index, row) in rows.iter().enumerate().rev() {
        let (Some(ex_date), Some(amount)) = (
            day(row, &["ex_dividend_date", "ex_date", "date"]),
            number(row, &["amount", "dividend", "value"]),
        ) else {
            continue;
        };
        let fact = day(row, &["declaration_date", "record_date"])
            .filter(|date| *date <= read.today)
            .unwrap_or(ex_date.min(read.today));
        out.push(NewObservation {
            kind: ObservationKind::Dividend,
            fact_date: fact.to_string(),
            dedupe_key: format!("dividend:{ex_date}"),
            facts: json!({"amount": amount, "currency": text(row, &["currency"]), "exDate": ex_date.to_string(), "recordDate": day(row, &["record_date"]).map(|date| date.to_string()), "declared": day(row, &["declaration_date"]).map(|date| date.to_string())}),
            evidence: evidence(read, format!("{prefix}/{index}"), None, fact),
        });
        seen += 1;
        if seen >= 4 {
            break;
        }
    }
}

/// Price series by adjustment, so an adjusted and a raw history never mix.
pub const PRICE: &str = "price";
pub const PRICE_ADJUSTED: &str = "price:adjusted";
pub const PRICE_RAW: &str = "price:raw";

/// Whether a close series is a price (any adjustment) rather than a rate.
pub fn is_price_series(series: &str) -> bool {
    series == PRICE || series.starts_with("price:")
}

/// The adjustment a provider states for a whole result, if any.
fn stated_adjustment(payload: &Value) -> Option<&'static str> {
    let stated = ["/query/adjustment", "/adjustment", "/data/adjustment"]
        .iter()
        .find_map(|pointer| payload.pointer(pointer).and_then(Value::as_str))?
        .to_lowercase();
    match stated.as_str() {
        "raw" | "unadjusted" | "none" => Some(PRICE_RAW),
        "" => None,
        _ => Some(PRICE_ADJUSTED),
    }
}

/// A row that says its bar is final (a completed session).
fn row_final(row: &Value) -> bool {
    ["final", "is_final", "settled"]
        .iter()
        .any(|field| row.get(*field).and_then(Value::as_bool) == Some(true))
}

fn closes(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let (rows, prefix) = match rows(read.payload, "/data/bars") {
        Some(found) => found,
        None => return,
    };
    let currency = read
        .payload
        .pointer("/data/instrument/currency")
        .and_then(Value::as_str)
        .map(str::to_owned);
    // An adjusted close, when the provider gives one, avoids split jumps.
    let adjusted = rows
        .iter()
        .any(|row| number(row, &["adj_close", "adjusted_close"]).is_some());
    let series = if adjusted {
        PRICE_ADJUSTED
    } else {
        stated_adjustment(read.payload).unwrap_or(PRICE)
    };
    let fields: &[&str] = if adjusted {
        &["adj_close", "adjusted_close"]
    } else {
        &["close", "value"]
    };
    let mut points: Vec<(usize, NaiveDate, f64, Option<f64>)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let date = day(row, &["date"])?;
            // Only completed sessions: a bar from today may still be trading.
            if date >= read.session_day && !(row_final(row) && date <= read.today) {
                return None;
            }
            let split = number(row, &["split_ratio", "stock_splits", "split"])
                .filter(|ratio| *ratio > 0.0 && (*ratio - 1.0).abs() > f64::EPSILON);
            Some((index, date, number(row, fields)?, split))
        })
        .collect();
    points.sort_by_key(|(_, date, ..)| *date);
    let skip = points.len().saturating_sub(MAX_CLOSES);
    for (index, date, close, split) in points.into_iter().skip(skip) {
        let mut facts = json!({"series": series, "close": close, "currency": currency});
        if let Some(ratio) = split {
            facts["split"] = json!(ratio);
        }
        out.push(NewObservation {
            kind: ObservationKind::Close,
            fact_date: date.to_string(),
            dedupe_key: format!("close:{series}:{date}"),
            facts,
            evidence: evidence(read, format!("{prefix}/{index}"), None, date),
        });
    }
}

fn treasury(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    let mut dated: Vec<(usize, NaiveDate, &Value)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| Some((index, day(row, &["date"])?, row)))
        // Like prices, only completed days: today's level may still move.
        .filter(|(_, date, _)| *date < read.session_day)
        .collect();
    dated.sort_by_key(|(_, date, _)| *date);
    let skip = dated.len().saturating_sub(MAX_CLOSES);
    for (index, date, row) in dated.into_iter().skip(skip) {
        let two = number(row, &["year_2", "y2"]);
        let ten = number(row, &["year_10", "y10"]);
        let mut push = |series: &str, level: f64| {
            out.push(NewObservation {
                kind: ObservationKind::Close,
                fact_date: date.to_string(),
                dedupe_key: format!("close:{series}:{date}"),
                facts: json!({"series": series, "close": round(level, 4), "unit": "%"}),
                evidence: evidence(read, format!("{prefix}/{index}"), None, date),
            });
        };
        if let Some(level) = two {
            push("2y", level);
        }
        if let Some(level) = ten {
            push("10y", level);
        }
        if let (Some(two), Some(ten)) = (two, ten) {
            push("2s10s", ten - two);
        }
    }
}

fn fees(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    let Some((index, ratio)) = rows.iter().enumerate().find_map(|(index, row)| {
        Some((
            index,
            number(
                row,
                &["net_expense_ratio", "expense_ratio", "gross_expense_ratio"],
            )?,
        ))
    }) else {
        return;
    };
    out.push(NewObservation {
        kind: ObservationKind::Fees,
        fact_date: read.today.to_string(),
        dedupe_key: format!("fees:{}", round(ratio, 6)),
        facts: json!({"ratio": ratio}),
        evidence: evidence(read, format!("{prefix}/{index}"), None, read.today),
    });
}

fn holdings(read: &ToolRead<'_>, out: &mut Vec<NewObservation>) {
    let Some((rows, prefix)) = rows(read.payload, "") else {
        return;
    };
    let mut top: Vec<(String, f64)> = rows
        .iter()
        .filter_map(|row| {
            let name = text(row, &["name", "symbol", "holding"])?;
            Some((
                bounded(name, 80),
                number(row, &["weight", "percent", "pct"])?,
            ))
        })
        .collect();
    top.sort_by(|a, b| b.1.total_cmp(&a.1));
    top.truncate(TOP_HOLDINGS);
    if top.is_empty() {
        return;
    }
    let key = top
        .iter()
        .map(|(name, weight)| format!("{name}={}", round(*weight, 2)))
        .collect::<Vec<_>>()
        .join("|");
    out.push(NewObservation {
        kind: ObservationKind::Holdings,
        fact_date: read.today.to_string(),
        dedupe_key: format!("holdings:{}", &crate::util::sha256(key.as_bytes())[..24]),
        facts: json!({"top": top.iter().map(|(name, weight)| json!({"name": name, "weight": weight})).collect::<Vec<_>>()}),
        evidence: evidence(read, prefix, None, read.today),
    });
}

/// Observations in one read for the asset `symbol`.
pub fn extract(read: &ToolRead<'_>, symbol: &str) -> Vec<NewObservation> {
    let mut out = Vec::new();
    match read.tool {
        "dart_filing_search" => dart_filings(read, &mut out),
        "dart_financials" => dart_results(read, &mut out),
        "dart_major_report" => dart_capital(read, &mut out),
        "dart_dividends" => dart_dividends(read, &mut out),
        "krx_daily_quote" => closes(read, &mut out),
        "equity_fundamental_filings" => sec_filings(read, &mut out),
        "equity_fundamental_income" => income_results(read, &mut out),
        "equity_calendar_earnings" => earnings(read, symbol, &mut out),
        "equity_fundamental_dividends" => dividends(read, &mut out),
        "equity_price_historical"
        | "etf_historical"
        | "crypto_price_historical"
        | "currency_price_historical"
        | "derivatives_futures_historical"
        | "index_price_historical" => closes(read, &mut out),
        "fixedincome_government_treasury_rates" => treasury(read, &mut out),
        "etf_info" => fees(read, &mut out),
        "etf_holdings" => holdings(read, &mut out),
        _ => {}
    }
    // Rust never keeps an observation without a date and a source.
    out.retain(|observation| {
        parse_day(&observation.fact_date).is_some() && observation.evidence.has_source()
    });
    out
}

/// The latest row date at or before `today` in a result, for dating evidence.
pub fn latest_date(payload: &Value, today: NaiveDate) -> Option<NaiveDate> {
    const FIELDS: &[&str] = &[
        "filing_date",
        "filed_at",
        "rcept_dt",
        "date",
        "period_ending",
        "period_end",
        "report_date",
        "ex_dividend_date",
        "observation_date",
        "published_at",
    ];
    fn walk(value: &Value, today: NaiveDate, best: &mut Option<NaiveDate>, depth: usize) {
        if depth > 6 {
            return;
        }
        match value {
            Value::Object(map) => {
                for (key, item) in map {
                    if FIELDS.contains(&key.as_str()) {
                        if let Some(date) = item
                            .as_str()
                            .and_then(parse_day)
                            .filter(|date| *date <= today)
                        {
                            *best = Some(best.map_or(date, |current| current.max(date)));
                        }
                    }
                    walk(item, today, best, depth + 1);
                }
            }
            Value::Array(items) => items
                .iter()
                .take(2_000)
                .for_each(|item| walk(item, today, best, depth + 1)),
            _ => {}
        }
    }
    let mut best = None;
    walk(payload, today, &mut best, 0);
    best
}
