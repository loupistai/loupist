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
        r"net (?:profit|loss)|sales volume|production capacity|yield)\b[^;\n]*\d|",
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

pub(super) fn admits_entity_name(name: &str, kind: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || kind == "Metric" || name.contains(['\n', '\r']) {
        return false;
    }
    if kind == "Asset" && COUPON_SECURITY.is_match(name) {
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
