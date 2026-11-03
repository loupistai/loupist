//! Detectors over structured rows (finance-watchers.md §2.1): filing titles
//! route to the most specific kind, insider purchases cluster, Korean investor
//! flows turn. Pure functions; the check decides baselines and delivery.
use chrono::{Duration, NaiveDate};
use serde_json::Value;

use crate::knowledge::assets::observe::parse_day;

/// Where one filing row goes. One filing fires at most one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilingRoute {
    /// A "New filings" event.
    Filings,
    /// A Schedule 13D/13G or a 대량보유 report: "Big holder crossing 5%".
    Holders,
    /// A Form 3/4/5: read through the insider route, never a filing event.
    InsiderForm,
    /// A Korean 임원ㆍ주요주주 ownership report (buy or sell not checked).
    InsiderReport,
    /// A Korean IR notice: its date feeds the results event.
    IrNotice,
    /// A Korean preliminary results disclosure: the results card's release.
    PreliminaryResults,
    /// A Korean dividend decision or record-date notice.
    Dividend,
    /// A form the watchers do not announce.
    Ignore,
}

/// US forms that fire "New filings" (amendments included).
const US_FILING_FORMS: &[&str] = &["8-K", "10-Q", "10-K", "6-K", "20-F", "DEF 14A", "S-1"];

fn base_form(form: &str) -> String {
    let upper = form.trim().to_uppercase();
    upper
        .strip_suffix("/A")
        .map(str::to_owned)
        .unwrap_or(upper)
        .trim()
        .to_owned()
}

/// Routes a filing observation's facts (`form`, `title`).
pub fn classify_filing(facts: &Value, korean: bool) -> FilingRoute {
    let title = facts["title"].as_str().unwrap_or_default();
    let form = facts["form"].as_str().unwrap_or_default();
    if korean {
        let compact: String = title.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains("대량보유") {
            return FilingRoute::Holders;
        }
        if compact.contains("임원ㆍ주요주주")
            || compact.contains("임원·주요주주")
            || compact.contains("임원.주요주주")
        {
            return FilingRoute::InsiderReport;
        }
        if compact.contains("잠정") && compact.contains("실적") {
            return FilingRoute::PreliminaryResults;
        }
        if compact.contains("기업설명회") || compact.contains("(IR)") {
            return FilingRoute::IrNotice;
        }
        if compact.contains("배당") && (compact.contains("결정") || compact.contains("기준일"))
        {
            return FilingRoute::Dividend;
        }
        return FilingRoute::Filings;
    }
    let form = base_form(form);
    if matches!(form.as_str(), "3" | "4" | "5") {
        return FilingRoute::InsiderForm;
    }
    let squeezed = form.replace(' ', "");
    if squeezed.starts_with("SC13D")
        || squeezed.starts_with("SC13G")
        || squeezed.starts_with("SCHEDULE13D")
        || squeezed.starts_with("SCHEDULE13G")
    {
        return FilingRoute::Holders;
    }
    if US_FILING_FORMS.contains(&form.as_str()) {
        return FilingRoute::Filings;
    }
    FilingRoute::Ignore
}

/// Rows of a structured result and the pointer that addresses them.
pub fn rows<'a>(payload: &'a Value, native: &str) -> Option<(&'a Vec<Value>, String)> {
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

fn number(row: &Value, field: &str) -> Option<f64> {
    match row.get(field)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

/// Several insiders buying in the open market within 14 days.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub count: usize,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// The newest row in the window (its pointer cites the read).
    pub pointer: String,
    pub url: Option<String>,
}

/// The window length for insider and report clusters.
pub const CLUSTER_DAYS: i64 = 14;
pub const CLUSTER_MIN: usize = 3;

fn is_purchase(row: &Value) -> bool {
    let code = text(row, &["transaction_code", "code"]).unwrap_or_default();
    if code.eq_ignore_ascii_case("P") {
        return true;
    }
    let kind = text(row, &["transaction_type", "type"])
        .unwrap_or_default()
        .to_lowercase();
    kind == "p"
        || kind.starts_with("p-")
        || kind.starts_with("p -")
        || kind.contains("purchase")
        || kind.contains("open market buy")
}

/// The latest 14-day window with at least three distinct buyers (Form 4 code
/// P, or a provider's "P-Purchase"); sales and awards never count.
pub fn insider_cluster(payload: &Value, today: NaiveDate) -> Option<Cluster> {
    let (rows, prefix) = rows(payload, "")?;
    let mut purchases: Vec<(NaiveDate, String, usize)> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| is_purchase(row))
        .filter_map(|(index, row)| {
            let date = ["transaction_date", "filing_date", "date"]
                .iter()
                .find_map(|field| row.get(*field).and_then(Value::as_str).and_then(parse_day))
                .filter(|date| *date <= today)?;
            let owner = text(
                row,
                &["owner_name", "reporting_name", "owner", "owner_cik", "name"],
            )?
            .to_lowercase();
            Some((date, owner, index))
        })
        .collect();
    purchases.sort_by_key(|purchase| std::cmp::Reverse(purchase.0));
    for (end, _, end_index) in &purchases {
        let start = *end - Duration::days(CLUSTER_DAYS - 1);
        let window: Vec<&(NaiveDate, String, usize)> = purchases
            .iter()
            .filter(|(date, ..)| *date >= start && date <= end)
            .collect();
        let mut owners: Vec<&str> = window.iter().map(|(_, owner, _)| owner.as_str()).collect();
        owners.sort_unstable();
        owners.dedup();
        if owners.len() >= CLUSTER_MIN {
            let row = &rows[*end_index];
            return Some(Cluster {
                count: owners.len(),
                from: window.iter().map(|(date, ..)| *date).min().unwrap_or(*end),
                to: *end,
                pointer: format!("{prefix}/{end_index}"),
                url: text(row, &["filing_url", "url", "link"]).map(str::to_owned),
            });
        }
    }
    None
}

/// At least three Korean ownership reports within 14 days (before W16.4 the
/// title cannot say whether they bought or sold).
pub fn report_cluster(dates: &[NaiveDate]) -> Option<(usize, NaiveDate, NaiveDate)> {
    let mut sorted = dates.to_vec();
    sorted.sort_by(|a, b| b.cmp(a));
    for end in &sorted {
        let start = *end - Duration::days(CLUSTER_DAYS - 1);
        let window: Vec<NaiveDate> = sorted
            .iter()
            .copied()
            .filter(|date| *date >= start && date <= end)
            .collect();
        if window.len() >= CLUSTER_MIN {
            return Some((window.len(), *window.iter().min().unwrap_or(end), *end));
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

/// Foreign plus institution net buying over the last 10 trading days against
/// the 10 before.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowsReading {
    /// The sign of the last 10 days' sum, when it is not zero.
    pub now: Option<Direction>,
    /// The sum changed sign and is at least 1% of the window's traded value.
    pub turned: bool,
    pub to: NaiveDate,
    pub net10: f64,
    pub prior10: f64,
    pub traded10: f64,
    pub pointer: String,
}

const FLOW_DAYS: usize = 10;
const FLOW_SHARE: f64 = 0.01;

fn sign(value: f64) -> Option<Direction> {
    if value > 0.0 {
        Some(Direction::In)
    } else if value < 0.0 {
        Some(Direction::Out)
    } else {
        None
    }
}

/// KIS `investor_trade_by_stock_daily` (`/data/response/output2`): needs 20
/// trading days; amounts are net buying values, never quantities held.
pub fn flows(payload: &Value) -> Option<FlowsReading> {
    let (rows, prefix) = rows(payload, "/data/response/output2")?;
    let mut days: Vec<(NaiveDate, f64, f64, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let date = row
                .get("stck_bsop_date")
                .and_then(Value::as_str)
                .and_then(parse_day)?;
            let net = number(row, "frgn_ntby_tr_pbmn")? + number(row, "orgn_ntby_tr_pbmn")?;
            let traded = number(row, "acml_tr_pbmn").unwrap_or(0.0);
            Some((date, net, traded, index))
        })
        .collect();
    days.sort_by_key(|day| std::cmp::Reverse(day.0));
    days.dedup_by_key(|day| day.0);
    if days.len() < FLOW_DAYS * 2 {
        return None;
    }
    let net10: f64 = days[..FLOW_DAYS].iter().map(|day| day.1).sum();
    let prior10: f64 = days[FLOW_DAYS..FLOW_DAYS * 2].iter().map(|day| day.1).sum();
    let traded10: f64 = days[..FLOW_DAYS].iter().map(|day| day.2).sum();
    let now = sign(net10);
    let turned = now.is_some()
        && sign(prior10).is_some()
        && now != sign(prior10)
        && net10.abs() >= FLOW_SHARE * traded10;
    Some(FlowsReading {
        now,
        turned,
        to: days[0].0,
        net10,
        prior10,
        traded10,
        pointer: format!("{prefix}/{}", days[0].3),
    })
}

/// A calendar date written in a Korean notice: `2026-04-28`, `2026.04.28`,
/// `2026/04/28`, `2026년 4월 28일` or `20260428`.
pub fn dates_in(text: &str) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() || (index > 0 && chars[index - 1].is_ascii_digit()) {
            index += 1;
            continue;
        }
        let rest: String = chars[index..chars.len().min(index + 16)].iter().collect();
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let parsed = if digits.len() == 8 {
            NaiveDate::parse_from_str(&digits, "%Y%m%d").ok()
        } else if digits.len() == 4 {
            let year: i32 = digits.parse().unwrap_or_default();
            let tail: String = rest[4..].to_owned();
            let numbers: Vec<u32> = tail
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .take(2)
                .filter_map(|part| part.parse().ok())
                .collect();
            let separated = tail.starts_with(['-', '.', '/', '년', ' ']);
            match (separated, numbers.as_slice()) {
                (true, [month, day]) => NaiveDate::from_ymd_opt(year, *month, *day),
                _ => None,
            }
        } else {
            None
        };
        if let Some(date) =
            parsed.filter(|date| (1990..=2100).contains(&chrono::Datelike::year(date)))
        {
            out.push(date);
        }
        index += digits.len().max(1);
    }
    out
}

/// The IR date in a 기업설명회(IR)개최 notice: the first date on a line that
/// names the date (일시, 일자, 개최일), on or after the filing and within 90 days.
pub fn ir_date(text: &str, filed: NaiveDate) -> Option<NaiveDate> {
    noticed_date(text, &["개최일시", "일시", "개최일", "일자"], filed, 90)
}

fn noticed_date(text: &str, labels: &[&str], from: NaiveDate, days: i64) -> Option<NaiveDate> {
    let until = from + Duration::days(days);
    for label in labels {
        for line in text.lines().filter(|line| line.contains(label)) {
            if let Some(date) = dates_in(line)
                .into_iter()
                .find(|date| *date >= from && *date <= until)
            {
                return Some(date);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn filing_titles_route_to_the_most_specific_kind() {
        let us = |form: &str| classify_filing(&json!({"form": form, "title": "x"}), false);
        assert_eq!(us("10-Q"), FilingRoute::Filings);
        assert_eq!(us("8-K/A"), FilingRoute::Filings);
        assert_eq!(us("DEF 14A"), FilingRoute::Filings);
        assert_eq!(us("4"), FilingRoute::InsiderForm);
        assert_eq!(us("SC 13D"), FilingRoute::Holders);
        assert_eq!(us("SC 13G/A"), FilingRoute::Holders);
        assert_eq!(us("SCHEDULE 13D"), FilingRoute::Holders);
        assert_eq!(us("424B2"), FilingRoute::Ignore);
        let kr = |title: &str| classify_filing(&json!({"title": title}), true);
        assert_eq!(kr("주식등의대량보유상황보고서(일반)"), FilingRoute::Holders);
        assert_eq!(
            kr("임원ㆍ주요주주특정증권등소유상황보고서"),
            FilingRoute::InsiderReport
        );
        assert_eq!(
            kr("연결재무제표기준영업(잠정)실적(공정공시)"),
            FilingRoute::PreliminaryResults
        );
        assert_eq!(
            kr("영업(잠정)실적(공정공시)"),
            FilingRoute::PreliminaryResults
        );
        assert_eq!(kr("기업설명회(IR)개최(안내공시)"), FilingRoute::IrNotice);
        assert_eq!(kr("현금ㆍ현물배당결정"), FilingRoute::Dividend);
        assert_eq!(kr("분기보고서 (2026.03)"), FilingRoute::Filings);
        assert_eq!(kr("주요사항보고서(유상증자결정)"), FilingRoute::Filings);
    }

    #[test]
    fn three_distinct_buyers_within_14_days_make_a_cluster_and_two_do_not() {
        let today = day("2026-10-31");
        let row = |owner: &str, date: &str, code: &str| json!({"owner_name": owner, "transaction_date": date, "transaction_code": code, "filing_url": "https://www.sec.gov/x"});
        let two = json!({"structuredContent": {"results": [row("A", "2026-10-20", "P"), row("B", "2026-10-25", "P"), row("A", "2026-10-29", "P"), row("C", "2026-10-29", "S")]}});
        assert_eq!(insider_cluster(&two, today), None);
        let three = json!({"structuredContent": {"results": [row("A", "2026-10-20", "P"), row("B", "2026-10-25", "P"), row("C", "2026-10-29", "P")]}});
        let cluster = insider_cluster(&three, today).unwrap();
        assert_eq!(
            (cluster.count, cluster.from, cluster.to),
            (3, day("2026-10-20"), day("2026-10-29"))
        );
        // Fifteen days apart is two windows, not one cluster.
        let spread = json!({"structuredContent": {"results": [row("A", "2026-10-01", "P"), row("B", "2026-10-25", "P"), row("C", "2026-10-29", "P")]}});
        assert_eq!(insider_cluster(&spread, today), None);
        // A provider's "P-Purchase" counts; an award does not.
        let typed = json!({"structuredContent": {"results": [
            {"owner_name": "A", "transaction_date": "2026-10-20", "transaction_type": "P-Purchase"},
            {"owner_name": "B", "transaction_date": "2026-10-21", "transaction_type": "P-Purchase"},
            {"owner_name": "C", "transaction_date": "2026-10-22", "transaction_type": "A-Award"}]}});
        assert_eq!(insider_cluster(&typed, today), None);
        assert_eq!(
            report_cluster(&[day("2026-10-01"), day("2026-10-10"), day("2026-10-14")]).map(|c| c.0),
            Some(3)
        );
        assert_eq!(
            report_cluster(&[day("2026-10-01"), day("2026-10-20")]),
            None
        );
    }

    fn flow_rows(recent: f64, prior: f64, traded: f64) -> Value {
        let rows: Vec<Value> = (0..20)
            .map(|index| {
                let date = day("2026-10-30") - Duration::days(index);
                let net = if index < 10 { recent } else { prior };
                json!({"stck_bsop_date": date.format("%Y%m%d").to_string(), "frgn_ntby_tr_pbmn": format!("{}", net / 2.0), "orgn_ntby_tr_pbmn": format!("{}", net / 2.0), "acml_tr_pbmn": format!("{traded}")})
            })
            .collect();
        json!({"data": {"response": {"output2": rows}}})
    }

    #[test]
    fn flows_turn_only_with_a_sign_change_of_at_least_one_percent_of_traded_value() {
        // 10 days × +60 = +600 against 10 × −50; traded 10 × 5,000 = 50,000 → 1.2%.
        let turned = flows(&flow_rows(60.0, -50.0, 5_000.0)).unwrap();
        assert!(turned.turned);
        assert_eq!(turned.now, Some(Direction::In));
        assert_eq!(turned.to, day("2026-10-30"));
        // +40 a day is 0.8% of the traded value: below the threshold.
        assert!(!flows(&flow_rows(40.0, -50.0, 5_000.0)).unwrap().turned);
        // No sign change.
        assert!(!flows(&flow_rows(60.0, 50.0, 5_000.0)).unwrap().turned);
        let out = flows(&flow_rows(-60.0, 50.0, 5_000.0)).unwrap();
        assert_eq!((out.turned, out.now), (true, Some(Direction::Out)));
        // Fewer than 20 trading days says nothing.
        assert!(flows(&json!({"data": {"response": {"output2": []}}})).is_none());
    }

    #[test]
    fn notice_dates_are_read_from_the_labelled_line() {
        let filed = day("2026-04-20");
        let notice = "1. 개최일시 : 2026년 4월 28일 16:00\n2. 장소: 서울\n공시일 2026-04-20";
        assert_eq!(ir_date(notice, filed), Some(day("2026-04-28")));
        assert_eq!(
            ir_date("일시: 2026.05.02 10:00", filed),
            Some(day("2026-05-02"))
        );
        assert_eq!(ir_date("no date here", filed), None);
        assert_eq!(
            dates_in("20260428 and 2026/05/01"),
            vec![day("2026-04-28"), day("2026-05-01")]
        );
    }
}
