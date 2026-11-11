//! OpenDART capital decisions (주요사항보고서 주요정보, DS005), ownership
//! reports (지분공시, DS004) and dividends (정기보고서 배당에 관한 사항, DS002).
//! Endpoint and field names follow the OpenDART developer guide. Every row is
//! dated by its receipt (`filed_at`), so an as-of cutoff applies to it.
use super::*;

fn dividend_notice_dates(year: i32, now: DateTime<Utc>) -> Option<(NaiveDate, NaiveDate)> {
    let today = now.with_timezone(&chrono_tz::Asia::Seoul).date_naive();
    let start = NaiveDate::from_ymd_opt(year, 11, 1)?;
    let end = NaiveDate::from_ymd_opt(year + 1, 4, 30)?.min(today);
    (start <= end).then_some((start, end))
}

const MAX_EVENT_ROWS: usize = 100;
const DART_VIEWER: &str = "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=";
/// OpenDART serves DS005 decisions from 2015.
const FIRST_DECISION_YEAR: i32 = 2015;
/// Characters after "배당기준일" in which its date must appear.
const RECORD_DATE_WINDOW: usize = 80;

/// One DS005 decision type, by the tool's `kind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DecisionKind {
    RightsOffering,
    BonusIssue,
    CapitalReduction,
    ConvertibleBonds,
    BondsWithWarrants,
    ExchangeableBonds,
    Buyback,
    TreasurySale,
    Merger,
    Split,
}

impl DecisionKind {
    const ALL: [(&'static str, Self); 10] = [
        ("rights_offering", Self::RightsOffering),
        ("bonus_issue", Self::BonusIssue),
        ("capital_reduction", Self::CapitalReduction),
        ("convertible_bonds", Self::ConvertibleBonds),
        ("bonds_with_warrants", Self::BondsWithWarrants),
        ("exchangeable_bonds", Self::ExchangeableBonds),
        ("buyback", Self::Buyback),
        ("treasury_sale", Self::TreasurySale),
        ("merger", Self::Merger),
        ("split", Self::Split),
    ];

    pub(super) fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == value)
            .map(|(_, kind)| *kind)
    }

    fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, kind)| *kind == self)
            .map(|(name, _)| *name)
            .expect("every decision kind has a name")
    }

    /// The DS005 endpoint, verified against the OpenDART guide (apiId 2020023–2020051).
    fn endpoint(self) -> &'static str {
        match self {
            Self::RightsOffering => "piicDecsn.json",
            Self::BonusIssue => "fricDecsn.json",
            Self::CapitalReduction => "crDecsn.json",
            Self::ConvertibleBonds => "cvbdIsDecsn.json",
            Self::BondsWithWarrants => "bdwtIsDecsn.json",
            Self::ExchangeableBonds => "exbdIsDecsn.json",
            Self::Buyback => "tsstkAqDecsn.json",
            Self::TreasurySale => "tsstkDpDecsn.json",
            Self::Merger => "cmpMgDecsn.json",
            Self::Split => "cmpDvDecsn.json",
        }
    }

    /// The decision as the report names it (주요사항보고서(…)).
    fn title(self) -> &'static str {
        match self {
            Self::RightsOffering => "유상증자결정",
            Self::BonusIssue => "무상증자결정",
            Self::CapitalReduction => "감자결정",
            Self::ConvertibleBonds => "전환사채권발행결정",
            Self::BondsWithWarrants => "신주인수권부사채권발행결정",
            Self::ExchangeableBonds => "교환사채권발행결정",
            Self::Buyback => "자기주식취득결정",
            Self::TreasurySale => "자기주식처분결정",
            Self::Merger => "회사합병결정",
            Self::Split => "회사분할결정",
        }
    }

    /// The kind a 주요사항보고서 title names, if the tool reads it.
    pub(crate) fn from_report_title(title: &str) -> Option<&'static str> {
        let compact: String = title.chars().filter(|c| !c.is_whitespace()).collect();
        if !compact.contains("주요사항보고서") {
            return None;
        }
        // 유무상증자 is neither kind on its own; the filing row covers it.
        if compact.contains("유무상증자") {
            return None;
        }
        Self::ALL
            .iter()
            .find(|(_, kind)| compact.contains(kind.title()))
            .map(|(name, _)| *name)
    }
}

/// A provider text field, trimmed; OpenDART writes "-" for an empty cell.
fn field<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty() && *text != "-")
}

fn bounded_text(row: &Map<String, Value>, key: &str, chars: usize) -> Value {
    field(row, key).map_or(Value::Null, |text| {
        json!(text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(chars)
            .collect::<String>())
    })
}

/// "1,234,000" → "1234000"; "-" and words → null.
fn decimal(row: &Map<String, Value>, key: &str) -> Value {
    field(row, key)
        .and_then(|text| canonical_decimal(&json!(text)))
        .map_or(Value::Null, Value::String)
}

fn number(row: &Map<String, Value>, key: &str) -> Option<f64> {
    field(row, key)
        .and_then(|text| canonical_decimal(&json!(text)))
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

/// A date OpenDART writes as `2026-10-30`, `2026.10.30`, `20261030` or
/// `2026년 10월 30일`.
pub(super) fn korean_date(text: &str) -> Option<NaiveDate> {
    let digits: Vec<u32> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .take(3)
        .filter_map(|part| part.parse().ok())
        .collect();
    match digits.as_slice() {
        [compact] if (19_000_101..=21_001_231).contains(compact) => NaiveDate::from_ymd_opt(
            (*compact / 10_000) as i32,
            (*compact / 100) % 100,
            *compact % 100,
        ),
        [year, month, day, ..] if (1990..=2100).contains(year) => {
            NaiveDate::from_ymd_opt(*year as i32, *month, *day)
        }
        _ => None,
    }
}

fn date(row: &Map<String, Value>, key: &str) -> Value {
    field(row, key)
        .and_then(korean_date)
        .map_or(Value::Null, |date| json!(date.to_string()))
}

/// The receipt date that begins a 14-digit receipt number.
fn receipt_date(rcept_no: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(rcept_no.get(..8)?, "%Y%m%d").ok()
}

fn receipt(row: &Map<String, Value>) -> Result<(String, NaiveDate), FinanceDataError> {
    let rcept_no = field(row, "rcept_no").ok_or(FinanceDataError::InvalidResponse)?;
    let rcept_no = validate_digits(rcept_no, 14, "OpenDART receipt number is invalid")
        .map_err(|_| FinanceDataError::InvalidResponse)?;
    let filed = receipt_date(&rcept_no).ok_or(FinanceDataError::InvalidResponse)?;
    Ok((rcept_no, filed))
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

/// The shares a decision may add: the filing's own ratio when it states one,
/// else new shares ÷ shares before through the host calculation.
fn dilution(filed_ratio: Option<&str>, new_shares: Option<f64>, before: Option<f64>) -> Value {
    if let Some(ratio) = filed_ratio.and_then(|text| canonical_decimal(&json!(text))) {
        return json!({"dilution_pct": ratio, "dilution_basis": "filing"});
    }
    let (Some(new_shares), Some(before)) = (new_shares, before) else {
        return Value::Null;
    };
    if new_shares <= 0.0 || before <= 0.0 {
        return Value::Null;
    }
    let request = json!({
        "operation": "ratio",
        "as_percent": true,
        "inputs": {"numerator": new_shares, "denominator": before},
        "labels": {"numerator": "New shares", "denominator": "Shares before"},
    });
    let Ok(result) = crate::calculations::calculate(&request, &mut NoDatasets) else {
        return Value::Null;
    };
    let Some(value) = result["values"][0]["value"].as_f64() else {
        return Value::Null;
    };
    json!({
        "dilution_pct": canonical_decimal(&json!((value * 100.0).round() / 100.0)),
        "dilution_basis": "calculated",
        "dilution_calculation": {
            "title": "Share count increase",
            "formulas": result["formulas"],
            "inputs": result["inputs"].as_array().into_iter().flatten().map(|input| json!({
                "name": input["label"].as_str().or(input["name"].as_str()).unwrap_or_default(),
                "display": input["display"],
            })).collect::<Vec<_>>(),
            "display": result["values"][0]["display"],
            "value": value,
            "unit": "%",
        },
    })
}

/// The shares bonds may become. The filing states its ratio at the starting
/// conversion (or exercise) price; when that price can be reset down to a
/// floor (최저 조정가액), more shares can come, so the row carries the most:
/// whole shares the face amount buys at the floor, over the shares outstanding
/// the filing's own ratio implies, through the host calculation. Without a
/// floor the filed ratio stands, at the starting price.
fn bond_dilution(
    row: &Map<String, Value>,
    ratio_key: &str,
    shares_key: &str,
    price_key: &str,
) -> Value {
    let filed = field(row, ratio_key).and_then(|text| canonical_decimal(&json!(text)));
    let Value::Object(mut at_price) = dilution(filed.as_deref(), None, None) else {
        return Value::Null;
    };
    at_price.insert("dilution_at".into(), json!("price"));
    let numbers = (
        number(row, "bd_fta"),
        number(row, "act_mktprcfl_cvprc_lwtrsprc"),
        number(row, price_key),
        number(row, shares_key),
        filed.as_deref().and_then(|text| text.parse::<f64>().ok()),
    );
    let (Some(face), Some(floor), Some(price), Some(shares), Some(filed_pct)) = numbers else {
        return Value::Object(at_price);
    };
    if face <= 0.0 || floor <= 0.0 || floor >= price || shares <= 0.0 || filed_pct <= 0.0 {
        return Value::Object(at_price);
    }
    // Shares are whole; a fraction left over is paid in cash.
    let at_floor = (face / floor).floor();
    let outstanding = (shares * 100.0 / filed_pct).round();
    let request = json!({
        "operation": "ratio",
        "as_percent": true,
        "inputs": {"numerator": at_floor, "denominator": outstanding},
        "labels": {"numerator": "New shares at the floor", "denominator": "Shares outstanding"},
    });
    let Ok(result) = crate::calculations::calculate(&request, &mut NoDatasets) else {
        return Value::Object(at_price);
    };
    let Some(value) = result["values"][0]["value"].as_f64() else {
        return Value::Object(at_price);
    };
    let amount = crate::calculations::display_amount;
    let mut formulas = vec![
        json!(format!(
            "Face amount / Floor price = {} / {} = {} new shares at most",
            amount(face),
            amount(floor),
            amount(at_floor)
        )),
        json!(format!(
            "Shares outstanding = Shares at the conversion price / Their filed share = {} / {}% = {}",
            amount(shares),
            amount(filed_pct),
            amount(outstanding)
        )),
    ];
    formulas.extend(result["formulas"].as_array().into_iter().flatten().cloned());
    let input = |name: &str, display: String| json!({"name": name, "display": display});
    json!({
        "dilution_pct": canonical_decimal(&json!((value * 100.0).round() / 100.0)),
        "dilution_basis": "calculated",
        "dilution_at": "floor",
        "dilution_price": canonical_decimal(&json!(floor)),
        "dilution_filed_pct": filed,
        "max_new_shares": canonical_decimal(&json!(at_floor)),
        "dilution_calculation": {
            "title": "Most new shares at the floor",
            "formulas": formulas,
            "inputs": [
                input("Face amount", amount(face)),
                input("Floor price", amount(floor)),
                input("Shares at the conversion price", amount(shares)),
                input("Their filed share of all shares", format!("{}%", amount(filed_pct))),
            ],
            "display": result["values"][0]["display"],
            "value": value,
            "unit": "%",
        },
    })
}

fn sum(values: [Option<f64>; 2]) -> Option<f64> {
    match values {
        [None, None] => None,
        [first, second] => Some(first.unwrap_or(0.0) + second.unwrap_or(0.0)),
    }
}

/// Plain words for how new shares are offered (증자방식).
fn offering_method(text: Option<&str>) -> Value {
    let Some(text) = text else {
        return Value::Null;
    };
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    json!(if compact.contains("제3자") {
        "third_party"
    } else if compact.contains("주주배정") {
        "shareholders"
    } else if compact.contains("일반공모") || compact.contains("공모") {
        "public"
    } else {
        "other"
    })
}

fn purposes(row: &Map<String, Value>) -> Value {
    json!({
        "facilities": decimal(row, "fdpp_fclt"),
        "business_acquisition": decimal(row, "fdpp_bsninh"),
        "operations": decimal(row, "fdpp_op"),
        "debt_repayment": decimal(row, "fdpp_dtrp"),
        "securities_acquisition": decimal(row, "fdpp_ocsa"),
        "other": decimal(row, "fdpp_etc"),
    })
}

/// The shared bond fields of a CB, BW or EB decision.
fn bond(row: &Map<String, Value>, out: &mut Map<String, Value>) {
    out.insert("series".into(), bounded_text(row, "bd_tm", 40));
    out.insert("bond_type".into(), bounded_text(row, "bd_knd", 80));
    out.insert("face_amount".into(), decimal(row, "bd_fta"));
    out.insert("coupon_pct".into(), decimal(row, "bd_intr_ex"));
    out.insert("maturity_pct".into(), decimal(row, "bd_intr_sf"));
    out.insert("maturity_date".into(), date(row, "bd_mtd"));
    out.insert("issue_method".into(), bounded_text(row, "bdis_mthn", 40));
    out.insert("subscription_date".into(), date(row, "sbd"));
    out.insert("payment_date".into(), date(row, "pymd"));
    out.insert("purpose".into(), purposes(row));
}

/// One DS005 row in `loupist-dart-major-report/1` form.
fn decision_row(kind: DecisionKind, row: &Map<String, Value>) -> Result<Value, FinanceDataError> {
    let (rcept_no, filed) = receipt(row)?;
    let mut out = Map::new();
    out.insert("filing_id".into(), json!(rcept_no));
    out.insert("filed_at".into(), json!(filed.to_string()));
    out.insert("kind".into(), json!(kind.name()));
    out.insert(
        "title".into(),
        json!(format!("주요사항보고서({})", kind.title())),
    );
    out.insert("url".into(), json!(format!("{DART_VIEWER}{rcept_no}")));
    let decided = match kind {
        DecisionKind::Buyback => "aq_dd",
        DecisionKind::TreasurySale => "dp_dd",
        _ => "bddd",
    };
    out.insert("decision_date".into(), date(row, decided));
    match kind {
        DecisionKind::RightsOffering | DecisionKind::BonusIssue => {
            let new_shares = sum([number(row, "nstk_ostk_cnt"), number(row, "nstk_estk_cnt")]);
            let before = sum([
                number(row, "bfic_tisstk_ostk"),
                number(row, "bfic_tisstk_estk"),
            ]);
            out.insert("new_shares_common".into(), decimal(row, "nstk_ostk_cnt"));
            out.insert("new_shares_other".into(), decimal(row, "nstk_estk_cnt"));
            out.insert(
                "shares_before_common".into(),
                decimal(row, "bfic_tisstk_ostk"),
            );
            out.insert(
                "shares_before_other".into(),
                decimal(row, "bfic_tisstk_estk"),
            );
            out.insert("par_value".into(), decimal(row, "fv_ps"));
            if kind == DecisionKind::RightsOffering {
                out.insert("method".into(), offering_method(field(row, "ic_mthn")));
                out.insert("method_text".into(), bounded_text(row, "ic_mthn", 80));
                out.insert("purpose".into(), purposes(row));
                // The decision API carries neither; they are in the notice.
                out.insert("issue_price".into(), Value::Null);
                out.insert("payment_date".into(), Value::Null);
            } else {
                out.insert("allotment_record_date".into(), date(row, "nstk_asstd"));
                out.insert(
                    "new_shares_per_share".into(),
                    decimal(row, "nstk_ascnt_ps_ostk"),
                );
                out.insert("listing_date".into(), date(row, "nstk_lstprd"));
            }
            if let Value::Object(found) = dilution(None, new_shares, before) {
                out.extend(found);
            }
        }
        DecisionKind::CapitalReduction => {
            out.insert(
                "reduced_shares_common".into(),
                decimal(row, "crstk_ostk_cnt"),
            );
            out.insert(
                "shares_before_common".into(),
                decimal(row, "bfcr_tisstk_ostk"),
            );
            out.insert(
                "shares_after_common".into(),
                decimal(row, "atcr_tisstk_ostk"),
            );
            out.insert("reduction_pct".into(), decimal(row, "cr_rt_ostk"));
            out.insert("reduction_base_date".into(), date(row, "cr_std"));
            out.insert("method_text".into(), bounded_text(row, "cr_mth", 120));
            out.insert("reason".into(), bounded_text(row, "cr_rs", 200));
            out.insert("listing_date".into(), date(row, "crsc_nstklstprd"));
        }
        DecisionKind::ConvertibleBonds => {
            bond(row, &mut out);
            out.insert("conversion_price".into(), decimal(row, "cv_prc"));
            out.insert("conversion_shares".into(), decimal(row, "cvisstk_cnt"));
            out.insert("conversion_start".into(), date(row, "cvrqpd_bgd"));
            out.insert("conversion_last_day".into(), date(row, "cvrqpd_edd"));
            out.insert(
                "refix_floor".into(),
                decimal(row, "act_mktprcfl_cvprc_lwtrsprc"),
            );
            out.insert(
                "refix_floor_basis".into(),
                bounded_text(row, "act_mktprcfl_cvprc_lwtrsprc_bs", 200),
            );
            if let Value::Object(found) =
                bond_dilution(row, "cvisstk_tisstk_vs", "cvisstk_cnt", "cv_prc")
            {
                out.extend(found);
            }
        }
        DecisionKind::BondsWithWarrants => {
            bond(row, &mut out);
            out.insert("conversion_price".into(), decimal(row, "ex_prc"));
            out.insert("conversion_shares".into(), decimal(row, "nstk_isstk_cnt"));
            out.insert("conversion_start".into(), date(row, "expd_bgd"));
            out.insert("conversion_last_day".into(), date(row, "expd_edd"));
            out.insert(
                "warrants_separable".into(),
                bounded_text(row, "bdwt_div_atn", 40),
            );
            out.insert(
                "refix_floor".into(),
                decimal(row, "act_mktprcfl_cvprc_lwtrsprc"),
            );
            out.insert(
                "refix_floor_basis".into(),
                bounded_text(row, "act_mktprcfl_cvprc_lwtrsprc_bs", 200),
            );
            if let Value::Object(found) =
                bond_dilution(row, "nstk_isstk_tisstk_vs", "nstk_isstk_cnt", "ex_prc")
            {
                out.extend(found);
            }
        }
        DecisionKind::ExchangeableBonds => {
            bond(row, &mut out);
            // Exchange hands over shares that already exist: no new shares.
            out.insert("exchange_price".into(), decimal(row, "ex_prc"));
            out.insert("exchange_target".into(), bounded_text(row, "extg", 80));
            out.insert("exchange_shares".into(), decimal(row, "extg_stkcnt"));
            out.insert(
                "exchange_share_ratio_pct".into(),
                decimal(row, "extg_tisstk_vs"),
            );
            out.insert("exchange_start".into(), date(row, "exrqpd_bgd"));
            out.insert("exchange_last_day".into(), date(row, "exrqpd_edd"));
        }
        DecisionKind::Buyback => {
            out.insert(
                "planned_shares_common".into(),
                decimal(row, "aqpln_stk_ostk"),
            );
            out.insert(
                "planned_amount_common".into(),
                decimal(row, "aqpln_prc_ostk"),
            );
            out.insert("buy_from".into(), date(row, "aqexpd_bgd"));
            out.insert("buy_until".into(), date(row, "aqexpd_edd"));
            out.insert("purpose_text".into(), bounded_text(row, "aq_pp", 120));
            out.insert("method_text".into(), bounded_text(row, "aq_mth", 80));
        }
        DecisionKind::TreasurySale => {
            out.insert(
                "planned_shares_common".into(),
                decimal(row, "dppln_stk_ostk"),
            );
            out.insert("price_per_share".into(), decimal(row, "dpstk_prc_ostk"));
            out.insert(
                "planned_amount_common".into(),
                decimal(row, "dppln_prc_ostk"),
            );
            out.insert("sell_from".into(), date(row, "dpprpd_bgd"));
            out.insert("sell_until".into(), date(row, "dpprpd_edd"));
            out.insert("purpose_text".into(), bounded_text(row, "dp_pp", 120));
        }
        DecisionKind::Merger => {
            out.insert("method_text".into(), bounded_text(row, "mg_mth", 80));
            out.insert("ratio_text".into(), bounded_text(row, "mg_rt", 120));
            out.insert(
                "counterparty".into(),
                bounded_text(row, "mgptncmp_cmpnm", 80),
            );
            out.insert("new_shares_common".into(), decimal(row, "mgnstk_ostk_cnt"));
            out.insert("merger_effective_date".into(), date(row, "mgsc_mgdt"));
            out.insert("listing_date".into(), date(row, "mgsc_nstklstprd"));
        }
        DecisionKind::Split => {
            out.insert("method_text".into(), bounded_text(row, "dv_mth", 80));
            out.insert("ratio_text".into(), bounded_text(row, "dv_rt", 120));
            out.insert("new_company".into(), bounded_text(row, "dvfcmp_cmpnm", 80));
            out.insert("split_effective_date".into(), date(row, "dvdt"));
        }
    }
    Ok(Value::Object(out))
}

fn company_of(records: &[Value], corp_code: &str) -> Value {
    let first = records.first().and_then(Value::as_object);
    json!({
        "corp_code": corp_code,
        "name": first.and_then(|row| field(row, "corp_name")),
        "market": first.and_then(|row| field(row, "corp_cls")).map(|class| match class {
            "Y" => "KOSPI",
            "K" => "KOSDAQ",
            "N" => "KONEX",
            _ => "Other",
        }),
    })
}

fn identity(row: &Map<String, Value>, corp_code: &str) -> Result<(), FinanceDataError> {
    if field(row, "corp_code") != Some(corp_code) {
        return Err(FinanceDataError::InvalidResponse);
    }
    Ok(())
}

pub(super) fn normalize_dart_major_report(
    query: &OpenDartMajorReportQuery,
    kind: DecisionKind,
    corp_code: &str,
    raw: &RawProviderResponse,
) -> Result<Value, FinanceDataError> {
    let root = validate_dart_status_allow_no_data(&raw.bytes)?;
    let start = parse_date(&query.start, "OpenDART decision start is invalid")?;
    let end = parse_date(&query.end, "OpenDART decision end is invalid")?;
    let records = root
        .get("list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if root.get("status").and_then(Value::as_str) == Some("000") && root.get("list").is_none() {
        return Err(FinanceDataError::InvalidResponse);
    }
    let available = records.len();
    let mut rows = Vec::with_capacity(available.min(MAX_EVENT_ROWS));
    for record in &records {
        let object = record
            .as_object()
            .ok_or(FinanceDataError::InvalidResponse)?;
        identity(object, corp_code)?;
        let row = decision_row(kind, object)?;
        let filed = row["filed_at"]
            .as_str()
            .and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
            .ok_or(FinanceDataError::InvalidResponse)?;
        if filed < start || filed > end {
            return Err(FinanceDataError::InvalidResponse);
        }
        rows.push(row);
    }
    rows.sort_by(|a, b| b["filing_id"].as_str().cmp(&a["filing_id"].as_str()));
    rows.truncate(MAX_EVENT_ROWS);
    let returned = rows.len();
    let truncated = available > returned;
    let mut warnings = Vec::new();
    if kind == DecisionKind::RightsOffering && returned > 0 {
        warnings.push(
            "OpenDART's rights-offering decision has no issue price or payment date; read the filing for them."
                .to_owned(),
        );
    }
    if truncated {
        warnings.push("More decisions were filed in this range; narrow the dates.".to_owned());
    }
    finance_result(
        "dart_major_report",
        "filing.major_report",
        OPENDART_SOURCE_ID,
        json!({
            "provider": OPENDART_SOURCE_ID,
            "corp_code": corp_code,
            "kind": kind.name(),
            "start": query.start,
            "end": query.end
        }),
        json!({
            "schema": "loupist-dart-major-report/1",
            "company": company_of(&records, corp_code),
            "kind": kind.name(),
            "rows": rows
        }),
        raw,
        "Financial Supervisory Service",
        "OpenDART API",
        true,
        "official",
        "latest_only",
        if truncated { "warn" } else { "pass" },
        json!([
            {"code": "provider_success", "status": "pass"},
            {"code": "schema_valid", "status": "pass"},
            {"code": "response_identity", "status": "pass"},
            {"code": "requested_range_coverage", "status": "pass"}
        ]),
        warnings,
        returned,
        available,
        truncated,
    )
}

pub(super) fn normalize_dart_ownership(
    query: &OpenDartOwnershipQuery,
    corp_code: &str,
    raw: &RawProviderResponse,
) -> Result<Value, FinanceDataError> {
    let root = validate_dart_status_allow_no_data(&raw.bytes)?;
    let major = match query.kind.as_str() {
        "major_holders" => true,
        "insiders" => false,
        _ => return Err(FinanceDataError::InvalidResponse),
    };
    let records = root
        .get("list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let object = record
            .as_object()
            .ok_or(FinanceDataError::InvalidResponse)?;
        identity(object, corp_code)?;
        let (rcept_no, receipt_day) = receipt(object)?;
        // The report's own receipt date (YYYYMMDD or YYYY-MM-DD); the receipt number agrees.
        let filed = field(object, "rcept_dt")
            .and_then(korean_date)
            .ok_or(FinanceDataError::InvalidResponse)?;
        if filed != receipt_day {
            return Err(FinanceDataError::InvalidResponse);
        }
        let ratio = number(object, if major { "stkrt" } else { "sp_stock_lmp_rate" });
        let change = number(
            object,
            if major {
                "stkrt_irds"
            } else {
                "sp_stock_lmp_irds_rate"
            },
        );
        // Before = after − change, two decimals as OpenDART prints ratios.
        let before = ratio
            .zip(change)
            .map(|(ratio, change)| ((ratio - change) * 100.0).round() / 100.0)
            .and_then(|before| canonical_decimal(&json!(before)));
        let mut row = json!({
            "filing_id": rcept_no,
            "filed_at": filed.to_string(),
            "url": format!("{DART_VIEWER}{rcept_no}"),
            "reporter": bounded_text(object, "repror", 80),
        });
        if major {
            row["report_type"] = bounded_text(object, "report_tp", 40);
            row["reason"] = bounded_text(object, "report_resn", 200);
            row["shares"] = decimal(object, "stkqy");
            row["shares_change"] = decimal(object, "stkqy_irds");
            row["ratio_pct"] = decimal(object, "stkrt");
            row["ratio_change_pp"] = decimal(object, "stkrt_irds");
        } else {
            row["registered"] = bounded_text(object, "isu_exctv_rgist_at", 40);
            row["position"] = bounded_text(object, "isu_exctv_ofcps", 40);
            row["major_holder"] = bounded_text(object, "isu_main_shrholdr", 40);
            row["shares"] = decimal(object, "sp_stock_lmp_cnt");
            row["shares_change"] = decimal(object, "sp_stock_lmp_irds_cnt");
            row["ratio_pct"] = decimal(object, "sp_stock_lmp_rate");
            row["ratio_change_pp"] = decimal(object, "sp_stock_lmp_irds_rate");
        }
        row["ratio_before_pct"] = before.map_or(Value::Null, Value::String);
        rows.push(row);
    }
    rows.sort_by(|a, b| {
        b["filed_at"]
            .as_str()
            .cmp(&a["filed_at"].as_str())
            .then(b["filing_id"].as_str().cmp(&a["filing_id"].as_str()))
    });
    let available = rows.len();
    rows.truncate(query.limit);
    let returned = rows.len();
    let truncated = available > returned;
    finance_result(
        "dart_ownership",
        "filing.ownership",
        OPENDART_SOURCE_ID,
        json!({
            "provider": OPENDART_SOURCE_ID,
            "corp_code": corp_code,
            "kind": query.kind,
            "limit": query.limit
        }),
        json!({
            "schema": "loupist-dart-ownership/1",
            "company": company_of(&records, corp_code),
            "kind": query.kind,
            "rows": rows
        }),
        raw,
        "Financial Supervisory Service",
        "OpenDART API",
        true,
        "official",
        "latest_only",
        "pass",
        json!([
            {"code": "provider_success", "status": "pass"},
            {"code": "schema_valid", "status": "pass"},
            {"code": "response_identity", "status": "pass"}
        ]),
        vec!["ratio_before_pct is the reported ratio minus its reported change.".to_owned()],
        returned,
        available,
        truncated,
    )
}

/// `se` (구분) of 배당에 관한 사항: the item, its basis and unit.
fn dividend_item(
    label: &str,
) -> (
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
) {
    let compact: String = label.chars().filter(|c| !c.is_whitespace()).collect();
    let basis = if compact.contains("(연결)") {
        Some("consolidated")
    } else if compact.contains("(별도)") {
        Some("separate")
    } else {
        None
    };
    let (item, unit) = if compact.contains("주당액면가액") {
        (Some("par_value"), Some("KRW"))
    } else if compact.contains("당기순이익") {
        (Some("net_income"), Some("KRW millions"))
    } else if compact.contains("주당순이익") {
        (Some("eps"), Some("KRW"))
    } else if compact.contains("현금배당금총액") {
        (Some("total_cash_dividends"), Some("KRW millions"))
    } else if compact.contains("주식배당금총액") {
        (Some("total_stock_dividends"), Some("KRW millions"))
    } else if compact.contains("현금배당성향") {
        (Some("payout_ratio"), Some("%"))
    } else if compact.contains("현금배당수익률") {
        (Some("cash_yield"), Some("%"))
    } else if compact.contains("주식배당수익률") {
        (Some("stock_yield"), Some("%"))
    } else if compact.contains("주당현금배당금") {
        (Some("dps_cash"), Some("KRW"))
    } else if compact.contains("주당주식배당") {
        (Some("dps_stock"), Some("shares"))
    } else {
        (None, None)
    };
    (item, basis, unit)
}

fn share_class(text: Option<&str>) -> Value {
    match text {
        Some(text) if text.contains("보통") => json!("common"),
        Some(text) if text.contains("우선") => json!("preferred"),
        Some(text) => json!(text.chars().take(20).collect::<String>()),
        None => Value::Null,
    }
}

/// A 현금ㆍ현물배당결정 notice found for the year, with its text when it was read.
pub(super) struct DividendNotice {
    pub(super) filing_id: String,
    pub(super) filed: NaiveDate,
    pub(super) title: String,
    pub(super) text: Option<String>,
}

/// The record date (배당기준일) a dividend decision notice states.
pub(super) fn notice_record_date(text: &str) -> Option<NaiveDate> {
    let compact: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut search = compact.as_str();
    while let Some(index) = search.find("배당기준일") {
        let after = &search[index + "배당기준일".len()..];
        let window: String = after.chars().take(RECORD_DATE_WINDOW).collect();
        // The first date-shaped run after the marker.
        let start = window.find(|c: char| c.is_ascii_digit());
        if let Some(date) = start.and_then(|start| korean_date(&window[start..])) {
            return Some(date);
        }
        search = after;
    }
    None
}

/// Whether a disclosure title is a cash dividend decision.
pub(super) fn dividend_decision_title(title: &str) -> bool {
    let compact: String = title.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("배당") && compact.contains("결정") && !compact.contains("주식배당결정")
}

pub(super) fn normalize_dart_dividends(
    query: &OpenDartDividendsQuery,
    corp_code: &str,
    raw: &RawProviderResponse,
    notice: Option<&DividendNotice>,
) -> Result<Value, FinanceDataError> {
    let root = validate_dart_status(&raw.bytes)?;
    let records = root
        .get("list")
        .and_then(Value::as_array)
        .ok_or(FinanceDataError::InvalidResponse)?;
    let mut rows = Vec::new();
    for record in records.iter().take(MAX_FACT_ROWS) {
        let object = record
            .as_object()
            .ok_or(FinanceDataError::InvalidResponse)?;
        identity(object, corp_code)?;
        let (rcept_no, filed) = receipt(object)?;
        let label = field(object, "se").unwrap_or_default();
        let (item, basis, unit) = dividend_item(label);
        for (key, offset) in [("thstrm", 0), ("frmtrm", 1), ("lwfr", 2)] {
            let value = decimal(object, key);
            if value.is_null() {
                continue;
            }
            rows.push(json!({
                "filing_id": rcept_no,
                "filed_at": filed.to_string(),
                "item": item,
                "label": label.chars().take(60).collect::<String>(),
                "share_class": share_class(field(object, "stock_knd")),
                "basis": basis,
                "unit": unit,
                "fiscal_year": query.year - offset,
                "value": value,
                "settlement_date": if offset == 0 { date(object, "stlm_dt") } else { Value::Null },
            }));
        }
    }
    if rows.is_empty() {
        return Err(FinanceDataError::NoData(OPENDART_SOURCE_ID));
    }
    let record_date = match notice {
        Some(notice) => {
            let found = notice.text.as_deref().and_then(notice_record_date);
            json!({
                "status": if found.is_some() { "found" } else { "in_notice" },
                "date": found.map(|date| date.to_string()),
                "filing_id": notice.filing_id,
                "filed_at": notice.filed.to_string(),
                "title": notice.title.chars().take(120).collect::<String>(),
                "url": format!("{DART_VIEWER}{}", notice.filing_id),
            })
        }
        None => json!({"status": "no_notice", "date": null}),
    };
    let returned = rows.len();
    finance_result(
        "dart_dividends",
        "company.dividends",
        OPENDART_SOURCE_ID,
        json!({"provider": OPENDART_SOURCE_ID, "corp_code": corp_code, "year": query.year}),
        json!({
            "schema": "loupist-dart-dividends/1",
            "company": company_of(records, corp_code),
            "year": query.year,
            "rows": rows,
            "record_date": record_date
        }),
        raw,
        "Financial Supervisory Service",
        "OpenDART API",
        true,
        "official",
        "provider_revisable",
        "pass",
        json!([
            {"code": "provider_success", "status": "pass"},
            {"code": "schema_valid", "status": "pass"},
            {"code": "response_identity", "status": "pass"},
            {"code": "record_date", "status": if notice.is_some() { "pass" } else { "warn" }}
        ]),
        match record_date["status"].as_str() {
            Some("found") => Vec::new(),
            Some("in_notice") => vec![
                "The record date (배당기준일) is in the dividend decision notice; it could not be read from the notice text."
                    .to_owned(),
            ],
            _ => vec![
                "No dividend decision notice was found for this year, so the record date is unknown."
                    .to_owned(),
            ],
        },
        returned,
        returned,
        false,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OpenDartMajorReportQuery {
    pub(super) corp_code: String,
    pub(super) kind: String,
    pub(super) start: String,
    pub(super) end: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OpenDartOwnershipQuery {
    pub(super) corp_code: String,
    pub(super) kind: String,
    pub(super) limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OpenDartDividendsQuery {
    pub(super) corp_code: String,
    pub(super) year: i32,
}

impl FinanceDataService {
    fn dart_url(&self, endpoint: &str, pairs: &[(&str, String)]) -> Result<Url, FinanceDataError> {
        let mut url = fixed_url(OPENDART_API_ROOT, &["api", endpoint])?;
        {
            let mut query = url.query_pairs_mut();
            for (key, value) in pairs {
                query.append_pair(key, value);
            }
        }
        Ok(url)
    }

    async fn dart_get(
        &self,
        public_url: Url,
        api_key: &str,
        maximum_bytes: usize,
    ) -> Result<RawProviderResponse, FinanceDataError> {
        let mut request_url = public_url.clone();
        request_url
            .query_pairs_mut()
            .append_pair("crtfc_key", api_key);
        self.fetch_provider(
            self.client.get(request_url),
            OPENDART_SOURCE_ID,
            public_url.to_string(),
            maximum_bytes,
        )
        .await
    }

    pub async fn opendart_major_report(
        &self,
        params: Value,
        api_key: &str,
    ) -> Result<Value, FinanceDataError> {
        validate_api_key(api_key)?;
        let query: OpenDartMajorReportQuery =
            strict_query(params, "expected the exact OpenDART major-report schema")?;
        let corp_code = validate_digits(&query.corp_code, 8, "corp_code must be eight digits")?;
        let kind = DecisionKind::parse(&query.kind).ok_or(FinanceDataError::InvalidQuery(
            "kind must be one of the OpenDART capital decisions",
        ))?;
        let (start, end) = validate_date_range(&query.start, &query.end, 366)?;
        if start.year() < FIRST_DECISION_YEAR {
            return Err(FinanceDataError::InvalidQuery(
                "OpenDART serves capital decisions from 2015",
            ));
        }
        let public_url = self.dart_url(
            kind.endpoint(),
            &[
                ("corp_code", corp_code.clone()),
                ("bgn_de", start.format("%Y%m%d").to_string()),
                ("end_de", end.format("%Y%m%d").to_string()),
            ],
        )?;
        let raw = self
            .dart_get(public_url, api_key, MAX_PROVIDER_BYTES)
            .await?;
        normalize_dart_major_report(&query, kind, &corp_code, &raw)
    }

    pub async fn opendart_ownership(
        &self,
        params: Value,
        api_key: &str,
    ) -> Result<Value, FinanceDataError> {
        validate_api_key(api_key)?;
        let query: OpenDartOwnershipQuery =
            strict_query(params, "expected the exact OpenDART ownership schema")?;
        let corp_code = validate_digits(&query.corp_code, 8, "corp_code must be eight digits")?;
        let endpoint = match query.kind.as_str() {
            "major_holders" => "majorstock.json",
            "insiders" => "elestock.json",
            _ => {
                return Err(FinanceDataError::InvalidQuery(
                    "kind must be major_holders or insiders",
                ))
            }
        };
        if !(1..=MAX_EVENT_ROWS).contains(&query.limit) {
            return Err(FinanceDataError::InvalidQuery(
                "limit must be from 1 through 100",
            ));
        }
        let public_url = self.dart_url(endpoint, &[("corp_code", corp_code.clone())])?;
        let raw = self
            .dart_get(public_url, api_key, MAX_LARGE_PROVIDER_BYTES)
            .await?;
        normalize_dart_ownership(&query, &corp_code, &raw)
    }

    /// The year's dividend table from the annual report, then the record
    /// date from the newest cash-dividend decision notice filed from November
    /// of that year through April of the next (best effort: a failed notice
    /// read leaves the record date "in the notice").
    pub async fn opendart_dividends(
        &self,
        params: Value,
        api_key: &str,
        pacer: &RequestPacer<'_>,
    ) -> Result<Value, FinanceDataError> {
        validate_api_key(api_key)?;
        let query: OpenDartDividendsQuery =
            strict_query(params, "expected the exact OpenDART dividends schema")?;
        let corp_code = validate_digits(&query.corp_code, 8, "corp_code must be eight digits")?;
        if query.year < FIRST_DECISION_YEAR || query.year > krx::seoul_today().year() {
            return Err(FinanceDataError::InvalidQuery(
                "year must be from 2015 through this year",
            ));
        }
        let public_url = self.dart_url(
            "alotMatter.json",
            &[
                ("corp_code", corp_code.clone()),
                ("bsns_year", query.year.to_string()),
                ("reprt_code", "11011".to_owned()),
            ],
        )?;
        let raw = self
            .dart_get(public_url, api_key, MAX_PROVIDER_BYTES)
            .await?;
        validate_dart_status(&raw.bytes)?;
        let notice = self
            .dividend_notice(&corp_code, query.year, api_key, pacer)
            .await;
        normalize_dart_dividends(&query, &corp_code, &raw, notice.as_ref())
    }

    async fn dividend_notice(
        &self,
        corp_code: &str,
        year: i32,
        api_key: &str,
        pacer: &RequestPacer<'_>,
    ) -> Option<DividendNotice> {
        let (start, end) = dividend_notice_dates(year, Utc::now())?;
        let search = self
            .dart_url(
                "list.json",
                &[
                    ("corp_code", corp_code.to_owned()),
                    ("bgn_de", start.format("%Y%m%d").to_string()),
                    ("end_de", end.format("%Y%m%d").to_string()),
                    ("last_reprt_at", "N".to_owned()),
                    ("sort", "date".to_owned()),
                    ("sort_mth", "desc".to_owned()),
                    ("page_no", "1".to_owned()),
                    ("page_count", "100".to_owned()),
                    ("pblntf_ty", "I".to_owned()),
                ],
            )
            .ok()?;
        pacer().await;
        let raw = self
            .dart_get(search, api_key, MAX_PROVIDER_BYTES)
            .await
            .ok()?;
        let root = validate_dart_status_allow_no_data(&raw.bytes).ok()?;
        let (filing_id, filed, title) = root
            .get("list")
            .and_then(Value::as_array)?
            .iter()
            .filter_map(Value::as_object)
            .filter(|row| field(row, "corp_code") == Some(corp_code))
            .filter_map(|row| {
                let title = field(row, "report_nm")?;
                let (rcept_no, filed) = receipt(row).ok()?;
                dividend_decision_title(title).then(|| (rcept_no, filed, title.to_owned()))
            })
            .max_by(|a, b| a.0.cmp(&b.0))?;
        let document = self
            .dart_url("document.xml", &[("rcept_no", filing_id.clone())])
            .ok()?;
        pacer().await;
        let text = match self
            .dart_get(document, api_key, MAX_LARGE_PROVIDER_BYTES)
            .await
        {
            Ok(raw) if raw.bytes.first() != Some(&b'{') => {
                provider_korea::extract_dart_document(&raw.bytes)
                    .ok()
                    .map(|(text, _, _)| text)
            }
            _ => None,
        };
        Some(DividendNotice {
            filing_id,
            filed,
            title,
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(url: &str, body: Value) -> RawProviderResponse {
        RawProviderResponse {
            bytes: serde_json::to_vec(&body).unwrap(),
            source_url: url.into(),
            retrieved_at: "2026-11-01T00:00:00Z".into(),
            continuation: None,
        }
    }

    fn major(kind: &str, start: &str, end: &str) -> OpenDartMajorReportQuery {
        OpenDartMajorReportQuery {
            corp_code: "99030800".into(),
            kind: kind.into(),
            start: start.into(),
            end: end.into(),
        }
    }

    fn cb_row() -> Value {
        json!({
            "rcept_no": "20261030990308", "corp_cls": "K", "corp_code": "99030800", "corp_name": "별무리반도체",
            "bd_tm": "3", "bd_knd": "무기명식 이권부 무보증 사모 전환사채", "bd_fta": "30,000,000,000",
            "fdpp_op": "20,000,000,000", "fdpp_fclt": "10,000,000,000", "fdpp_etc": "-",
            "bd_intr_ex": "0.0", "bd_intr_sf": "2.0", "bd_mtd": "2029년 11월 05일", "bdis_mthn": "사모",
            "cv_rt": "100", "cv_prc": "9,100", "cvisstk_knd": "기명식 보통주", "cvisstk_cnt": "3,296,703",
            "cvisstk_tisstk_vs": "6.10", "cvrqpd_bgd": "2027년 11월 05일", "cvrqpd_edd": "2029년 10월 05일",
            "act_mktprcfl_cvprc_lwtrsprc": "6,370", "act_mktprcfl_cvprc_lwtrsprc_bs": "발행당시 전환가액의 70%",
            "sbd": "2026년 11월 05일", "pymd": "2026년 11월 05일", "bddd": "2026년 10월 30일"
        })
    }

    #[test]
    fn convertible_bond_decisions_keep_price_floor_payment_and_the_filing_ratio() {
        let response = raw(
            "https://opendart.fss.or.kr/api/cvbdIsDecsn.json?corp_code=99030800&bgn_de=20261001&end_de=20261031",
            json!({"status": "000", "message": "정상", "list": [cb_row()]}),
        );
        let result = normalize_dart_major_report(
            &major("convertible_bonds", "2026-10-01", "2026-10-31"),
            DecisionKind::ConvertibleBonds,
            "99030800",
            &response,
        )
        .unwrap();
        let row = &result["data"]["rows"][0];
        assert_eq!(result["data"]["schema"], "loupist-dart-major-report/1");
        assert_eq!(result["data"]["company"]["market"], "KOSDAQ");
        assert_eq!(row["filed_at"], "2026-10-30");
        assert_eq!(row["face_amount"], "30000000000");
        assert_eq!(row["conversion_price"], "9100");
        assert_eq!(row["refix_floor"], "6370");
        assert_eq!(row["payment_date"], "2026-11-05");
        assert_eq!(row["maturity_date"], "2029-11-05");
        assert_eq!(row["decision_date"], "2026-10-30");
        assert_eq!(row["dilution_filed_pct"], "6.1");
        assert!(row["purpose"]["other"].is_null());
        assert_eq!(
            row["url"],
            "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261030990308"
        );
    }

    fn convertible(row: Value) -> Value {
        let response = raw(
            "https://opendart.fss.or.kr/api/cvbdIsDecsn.json",
            json!({"status": "000", "message": "정상", "list": [row]}),
        );
        let result = normalize_dart_major_report(
            &major("convertible_bonds", "2026-10-01", "2026-10-31"),
            DecisionKind::ConvertibleBonds,
            "99030800",
            &response,
        )
        .unwrap();
        result["data"]["rows"][0].clone()
    }

    #[test]
    fn a_reset_floor_counts_the_most_shares_the_bonds_can_become() {
        // ₩30bn converts into 3,296,703 shares at ₩9,100, filed as 6.10% of about
        // 54,043,000 shares; reset to ₩6,370 the same bonds become 4,709,576.
        let row = convertible(cb_row());
        assert_eq!(row["dilution_basis"], "calculated");
        assert_eq!(row["dilution_at"], "floor");
        assert_eq!(row["dilution_price"], "6370");
        assert_eq!(row["max_new_shares"], "4709576");
        assert_eq!(row["dilution_pct"], "8.71");
        let on_outstanding = 4_709_576.0 / 54_043_000.0 * 100.0;
        assert_eq!((on_outstanding * 100.0_f64).round() / 100.0, 8.71);
        let calc = &row["dilution_calculation"];
        assert!((calc["value"].as_f64().unwrap() - 8.714).abs() < 0.001);
        let formulas: Vec<&str> = calc["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(formulas.len(), 3);
        assert!(formulas[0].contains("30,000,000,000 / 6,370 = 4,709,576"));
        assert!(formulas[1].contains("3,296,703 / 6.1% = 54,044,311"));
        assert!(formulas[2].contains("4,709,576 / 54,044,311 = 8.71%"));
        assert_eq!(calc["inputs"][1]["name"], "Floor price");
        assert_eq!(calc["inputs"][1]["display"], "6,370");
    }

    #[test]
    fn bonds_without_a_floor_keep_the_filed_ratio_at_the_starting_price() {
        let mut fixed = cb_row();
        fixed["act_mktprcfl_cvprc_lwtrsprc"] = json!("-");
        let row = convertible(fixed);
        assert_eq!(row["dilution_basis"], "filing");
        assert_eq!(row["dilution_at"], "price");
        assert_eq!(row["dilution_pct"], "6.1");
        assert!(row["dilution_calculation"].is_null() && row["max_new_shares"].is_null());
        // A floor at or above the starting price cannot add shares.
        let mut above = cb_row();
        above["act_mktprcfl_cvprc_lwtrsprc"] = json!("9,100");
        assert_eq!(convertible(above)["dilution_at"], "price");
    }

    #[test]
    fn rights_offering_dilution_is_calculated_and_price_stays_in_the_notice() {
        let response = raw(
            "https://opendart.fss.or.kr/api/piicDecsn.json",
            json!({"status": "000", "message": "정상", "list": [{
                "rcept_no": "20260915990308", "corp_cls": "K", "corp_code": "99030800", "corp_name": "별무리반도체",
                "nstk_ostk_cnt": "4,000,000", "nstk_estk_cnt": "-", "fv_ps": "500",
                "bfic_tisstk_ostk": "50,000,000", "bfic_tisstk_estk": "-", "fdpp_op": "30,000,000,000",
                "ic_mthn": "주주배정후 실권주 일반공모", "bddd": "2026.09.15"
            }]}),
        );
        let result = normalize_dart_major_report(
            &major("rights_offering", "2026-09-01", "2026-09-30"),
            DecisionKind::RightsOffering,
            "99030800",
            &response,
        )
        .unwrap();
        let row = &result["data"]["rows"][0];
        assert_eq!(row["method"], "shareholders");
        assert_eq!(row["dilution_basis"], "calculated");
        assert_eq!(row["dilution_pct"], "8");
        assert!(row["issue_price"].is_null() && row["payment_date"].is_null());
        let calc = &row["dilution_calculation"];
        assert_eq!(calc["value"], 8.0);
        assert!(calc["formulas"][0].as_str().unwrap().contains("4,000,000"));
        assert_eq!(calc["inputs"][0]["name"], "New shares");
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn decisions_check_identity_and_range_and_an_empty_window_is_no_rows() {
        let foreign = raw(
            "x",
            json!({"status": "000", "list": [{"rcept_no": "20261030990308", "corp_code": "99030801"}]}),
        );
        assert!(normalize_dart_major_report(
            &major("buyback", "2026-10-01", "2026-10-31"),
            DecisionKind::Buyback,
            "99030800",
            &foreign
        )
        .is_err());
        let mut late = cb_row();
        late["rcept_no"] = json!("20261130990308");
        let outside = raw("x", json!({"status": "000", "list": [late]}));
        assert!(normalize_dart_major_report(
            &major("convertible_bonds", "2026-10-01", "2026-10-31"),
            DecisionKind::ConvertibleBonds,
            "99030800",
            &outside
        )
        .is_err());
        let none = raw(
            "x",
            json!({"status": "013", "message": "조회된 데이타가 없습니다."}),
        );
        let empty = normalize_dart_major_report(
            &major("convertible_bonds", "2026-10-01", "2026-10-31"),
            DecisionKind::ConvertibleBonds,
            "99030800",
            &none,
        )
        .unwrap();
        assert_eq!(empty["data"]["rows"], json!([]));
        assert_eq!(
            DecisionKind::from_report_title("주요사항보고서(전환사채권발행결정)"),
            Some("convertible_bonds")
        );
        assert_eq!(
            DecisionKind::from_report_title("[기재정정]주요사항보고서(유상증자결정)"),
            Some("rights_offering")
        );
        assert_eq!(
            DecisionKind::from_report_title("주요사항보고서(유무상증자결정)"),
            None
        );
        assert_eq!(
            DecisionKind::from_report_title("분기보고서 (2026.09)"),
            None
        );
    }

    #[test]
    fn ownership_reports_state_the_ratio_before_and_after() {
        let response = raw(
            "https://opendart.fss.or.kr/api/majorstock.json?corp_code=99030800",
            json!({"status": "000", "list": [
                {"rcept_no": "20260402990308", "rcept_dt": "20260402", "corp_code": "99030800", "corp_name": "별무리반도체",
                 "report_tp": "일반", "repror": "가상자산운용", "stkqy": "2,600,000", "stkqy_irds": "300,000",
                 "stkrt": "5.20", "stkrt_irds": "0.60", "report_resn": "장내매수"},
                {"rcept_no": "20261020990308", "rcept_dt": "20261020", "corp_code": "99030800", "corp_name": "별무리반도체",
                 "report_tp": "일반", "repror": "가상자산운용", "stkqy": "2,400,000", "stkqy_irds": "-200,000",
                 "stkrt": "4.80", "stkrt_irds": "-0.40", "report_resn": "장내매도"}
            ]}),
        );
        let query = OpenDartOwnershipQuery {
            corp_code: "99030800".into(),
            kind: "major_holders".into(),
            limit: 1,
        };
        let result = normalize_dart_ownership(&query, "99030800", &response).unwrap();
        let row = &result["data"]["rows"][0];
        assert_eq!(row["filed_at"], "2026-10-20");
        assert_eq!(row["ratio_pct"], "4.8");
        assert_eq!(row["ratio_before_pct"], "5.2");
        assert_eq!(row["reporter"], "가상자산운용");
        assert_eq!(result["page"]["truncated"], true);
        let mut mismatched = response.clone();
        mismatched.bytes = serde_json::to_vec(&json!({"status": "000", "list": [
            {"rcept_no": "20261020990308", "rcept_dt": "20261021", "corp_code": "99030800"}
        ]}))
        .unwrap();
        assert!(normalize_dart_ownership(&query, "99030800", &mismatched).is_err());
    }

    #[test]
    fn dividend_notice_search_includes_the_current_seoul_date_before_utc_midnight() {
        let now = DateTime::parse_from_rfc3339("2026-10-31T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (start, end) = dividend_notice_dates(2026, now).unwrap();
        assert_eq!(start.to_string(), "2026-11-01");
        assert_eq!(end, start);
        assert!(dividend_notice_dates(2026, now - ChronoDuration::seconds(1)).is_none());
        let new_year = DateTime::parse_from_rfc3339("2026-12-31T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            dividend_notice_dates(2026, new_year).unwrap().1.to_string(),
            "2027-01-01"
        );
    }

    #[test]
    fn dividends_read_items_by_year_and_the_record_date_from_the_notice() {
        let response = raw(
            "https://opendart.fss.or.kr/api/alotMatter.json?corp_code=99022100&bsns_year=2025&reprt_code=11011",
            json!({"status": "000", "list": [
                {"rcept_no": "20260310990221", "corp_code": "99022100", "corp_name": "솔숲식품", "se": "주당 현금배당금(원)",
                 "stock_knd": "보통주", "thstrm": "300", "frmtrm": "250", "lwfr": "-", "stlm_dt": "2025-12-31"},
                {"rcept_no": "20260310990221", "corp_code": "99022100", "corp_name": "솔숲식품", "se": "(연결)현금배당성향(%)",
                 "thstrm": "31.20", "frmtrm": "28.50", "lwfr": "30.00", "stlm_dt": "2025-12-31"}
            ]}),
        );
        let query = OpenDartDividendsQuery {
            corp_code: "99022100".into(),
            year: 2025,
        };
        let notice = DividendNotice {
            filing_id: "20260205990221".into(),
            filed: NaiveDate::from_ymd_opt(2026, 2, 5).unwrap(),
            title: "현금ㆍ현물배당결정".into(),
            text: Some("1. 배당구분 결산배당 ... 6. 배당기준일 2025-12-31 7. 배당금지급 예정일자 2026-04-10".into()),
        };
        let result =
            normalize_dart_dividends(&query, "99022100", &response, Some(&notice)).unwrap();
        let rows = result["data"]["rows"].as_array().unwrap();
        let dps: Vec<_> = rows
            .iter()
            .filter(|row| row["item"] == "dps_cash")
            .collect();
        assert_eq!(dps.len(), 2);
        assert_eq!(dps[0]["fiscal_year"], 2025);
        assert_eq!(dps[0]["value"], "300");
        assert_eq!(dps[0]["share_class"], "common");
        assert_eq!(dps[1]["fiscal_year"], 2024);
        let payout = rows
            .iter()
            .find(|row| row["item"] == "payout_ratio")
            .unwrap();
        assert_eq!(payout["basis"], "consolidated");
        assert_eq!(payout["unit"], "%");
        assert_eq!(result["data"]["record_date"]["date"], "2025-12-31");
        assert_eq!(result["data"]["record_date"]["status"], "found");
        assert_eq!(rows[0]["filed_at"], "2026-03-10");

        let unread = DividendNotice {
            text: None,
            ..notice
        };
        let in_notice =
            normalize_dart_dividends(&query, "99022100", &response, Some(&unread)).unwrap();
        assert_eq!(in_notice["data"]["record_date"]["status"], "in_notice");
        assert!(in_notice["data"]["record_date"]["date"].is_null());
        let none = normalize_dart_dividends(&query, "99022100", &response, None).unwrap();
        assert_eq!(none["data"]["record_date"]["status"], "no_notice");
        assert_eq!(
            notice_record_date("배당기준일 : 2025년 12월 31일"),
            NaiveDate::from_ymd_opt(2025, 12, 31)
        );
        assert!(dividend_decision_title("현금ㆍ현물배당결정"));
        assert!(!dividend_decision_title("주식배당결정"));
    }
}
