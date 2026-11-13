//! Admission of new entity names. Existing snapshots are never reclassified.
use regex::Regex;
use std::sync::LazyLock;

// Require a value and a unit, not merely a digit or a reporting period. Model
// numbers, tickers, maturity years and dated event names are legitimate names.
static MEASUREMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?:\b(?:USD|KRW|EUR|GBP|JPY|CNY|RMB|CAD|AUD|CHF)\s*[+−-]?\d|[$€£₩¥]\s*[+−-]?\d|",
        r"\d[\d,.]*\s*(?:%|퍼센트|%p|bps\b|basis points\b|million\b|billion\b|trillion\b|",
        r"thousand\b|percent\b|tonnes\b|tons\b|barrels\b|widgets\b|units\b|",
        r"GWh\b|MWh\b|kWh\b|GW\b|MW\b|kg\b|km\b|억원|조원|만원|천원|원|달러|백만|천만|십억|백억|억|조|만|톤|명|대|개|주|건|배)|",
        r"\d[\d,.]*\s+(?:shares|employees|patients|vehicles|dollars|euros|people)\b|",
        r"\b(?:revenue|net income|operating (?:profit|income|margin)|earnings per share|market share|",
        r"net (?:profit|loss)|sales volume|production capacity|yield)\b\s*[:=]?\s*[+−-]?\d|",
        r"(?:매출액|순이익|영업이익|점유율|생산량|생산능력|주당순이익)\s*[:=]?\s*[+−-]?\d)"
    )).expect("measurement name pattern")
});

static SENTENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?:\b(?:reported|reports|grew|rose|fell|earned|generated|achieved)\b\s+|",
        r"\b(?:is|was|were|are|has|have)\s+(?:a|an|the|been|now|not)\b|",
        r"\s.*(?:했다|하였다|한다|이다|였다|있다|됐다|되었다)[.!。]?$)"
    ))
    .expect("sentence name pattern")
});

static BARE_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}[-/.]\d{1,2}[-/.]\d{1,2}(?:[T ].*)?$").expect("date name pattern")
});

static COUPON_SECURITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\d+(?:\.\d+)?%\s+(?:(?:notes|bonds)\s+(?:due\s+)?20\d{2}|20\d{2}년\s+만기\s+채권)$",
    )
    .expect("coupon security name pattern")
});

static OCCURRENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?:\b(?:contract|agreement|acquisition|merger|takeover|buyout|ipo|offering|issuance|",
        r"launch|lawsuit|ruling|accident|outage|appointment|resignation|termination|",
        r"earnings (?:call|release)|guidance change|rating (?:action|upgrade|downgrade)|rate (?:cut|hike))\b|",
        r"계약|협약|인수|합병|기업공개|출시|소송|판결|사고|정전|선임|사임|해지|실적발표|금리인하|금리인상)"
    )).expect("occurrence name pattern")
});

static CONTRACT_MEASUREMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\b(?:contract|agreement)\s+(?:total\s+)?(?:value|amount|size)\b|계약의?\s*(?:총\s*)?(?:금액|가액|규모))")
        .expect("contract measurement name pattern")
});

static NAME_WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z]+|[가-힣]+").expect("name words pattern"));

/// An actual occurrence can have an amount qualifier or a verb in its title.
/// Require a concrete participant/subject beyond generic action, date and unit
/// words. A measurement of a contract's value is still a measurement.
fn qualified_occurrence(name: &str) -> bool {
    const GENERIC: &str = "a an the and or of for to with between by on at in from as effective dated date reported published signed announced worth valued value amount total supply purchase loan license licensing stock share bond capital earnings call release contract agreement acquisition merger takeover buyout ipo offering issuance launch lawsuit ruling accident outage appointment resignation termination guidance change rating action upgrade downgrade rate cut hike year years fiscal fy q january february march april may june july august september october november december utc gmt usd krw eur gbp jpy cny rmb cad aud chf million billion trillion thousand k m b bn tn co corp corporation inc incorporated ltd limited llc company 공급 구매 계약 공급계약 계약체결 체결 협약 인수 합병 기업공개 출시 소송 판결 사고 정전 선임 사임 해지 실적발표 실적발표회 금리인하 금리인상 원 달러 억원 조원 만원 천원 억 조 만 천 년 월 일 분기";
    OCCURRENCE.is_match(name)
        && !CONTRACT_MEASUREMENT.is_match(name)
        && NAME_WORDS.find_iter(name).any(|word| {
            let word = word.as_str().to_lowercase();
            !GENERIC.split_whitespace().any(|generic| generic == word)
        })
}

pub(super) fn admits_entity_name(name: &str, kind: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || kind == "Metric" || name.contains(['\n', '\r']) {
        return false;
    }
    if kind == "Asset" && COUPON_SECURITY.is_match(name) {
        return true;
    }
    if kind == "Event" && qualified_occurrence(name) {
        return true;
    }
    if MEASUREMENT.is_match(name) || SENTENCE.is_match(name) || BARE_DATE.is_match(name) {
        return false;
    }
    if name.chars().all(|c| c.is_ascii_digit()) {
        // Numeric exchange codes are names of instruments/issuers, not amounts.
        return matches!(kind, "Organization" | "Asset") && name.len() <= 12;
    }
    name.chars().any(char::is_alphabetic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn bilingual_entity_name_corpus() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../evals/world/entity-name-corpus-v1.json"
        ))
        .unwrap();
        for row in corpus.as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            let kind = row["kind"].as_str().unwrap();
            assert_eq!(
                admits_entity_name(name, kind),
                row["entity"].as_bool().unwrap(),
                "{kind}: {name}"
            );
        }
    }
}
