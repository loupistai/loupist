use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::assets::class::exchange_time_zone;
use super::GraphNode;
use crate::store::user_context::MapContextItem;

#[derive(Clone, Copy)]
enum Edge {
    Leading,
    Trailing,
}

/// Legal-form words a company name may carry, written as filings write them.
/// A `joined` form also matches with no space before or after it, as Korean
/// and Japanese names write it. Only Korean and Japanese forms lead a name, so
/// "AS Roma" keeps its "AS".
const LEGAL_FORMS: &[(&str, Edge, bool)] = &[
    ("주식회사", Edge::Leading, true),
    ("(주)", Edge::Leading, false),
    ("㈜", Edge::Leading, false),
    ("유한회사", Edge::Leading, true),
    ("株式会社", Edge::Leading, true),
    ("주식회사", Edge::Trailing, true),
    ("(주)", Edge::Trailing, false),
    ("株式会社", Edge::Trailing, true),
    ("inc", Edge::Trailing, false),
    ("incorporated", Edge::Trailing, false),
    ("corp", Edge::Trailing, false),
    ("corporation", Edge::Trailing, false),
    ("co", Edge::Trailing, false),
    ("company", Edge::Trailing, false),
    ("ltd", Edge::Trailing, false),
    ("limited", Edge::Trailing, false),
    ("llc", Edge::Trailing, false),
    ("plc", Edge::Trailing, false),
    ("sa", Edge::Trailing, false),
    ("ag", Edge::Trailing, false),
    ("nv", Edge::Trailing, false),
    ("bv", Edge::Trailing, false),
    ("gmbh", Edge::Trailing, false),
    ("as", Edge::Trailing, false),
    ("asa", Edge::Trailing, false),
    ("ab", Edge::Trailing, false),
    ("oy", Edge::Trailing, false),
    ("oyj", Edge::Trailing, false),
    ("spa", Edge::Trailing, false),
    ("se", Edge::Trailing, false),
    ("kk", Edge::Trailing, false),
    ("pte", Edge::Trailing, false),
    ("pty", Edge::Trailing, false),
    ("srl", Edge::Trailing, false),
    ("sas", Edge::Trailing, false),
];

/// NFKC, casefold, and every character but a letter, a digit or `&` as a
/// space, whitespace collapsed.
fn name_words(name: &str) -> String {
    let folded = super::world_extract::casefold(&name.nfkc().collect::<String>());
    let spaced: String = folded
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '&' {
                c
            } else {
                ' '
            }
        })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn legal_forms() -> &'static [(String, Edge, bool)] {
    static FORMS: OnceLock<Vec<(String, Edge, bool)>> = OnceLock::new();
    FORMS.get_or_init(|| {
        LEGAL_FORMS
            .iter()
            .map(|&(form, edge, joined)| (name_words(form), edge, joined))
            .collect()
    })
}

/// `key` without `form` at `edge`, when something remains.
fn strip_form<'a>(key: &'a str, form: &str, edge: Edge, joined: bool) -> Option<&'a str> {
    let rest = match edge {
        Edge::Leading => key.strip_prefix(form)?,
        Edge::Trailing => key.strip_suffix(form)?,
    };
    let spaced = match edge {
        Edge::Leading => rest.strip_prefix(' '),
        Edge::Trailing => rest.strip_suffix(' '),
    };
    let rest = match spaced {
        Some(rest) => rest,
        None if joined => rest,
        None => return None,
    };
    (!rest.is_empty()).then_some(rest)
}

/// One company's identity across the ways pages spell it: "Acme", "ACME
/// Inc." and "Acme Co., Ltd." share a key. Legal forms are stripped while a
/// name remains; a name that is only a legal form keeps it.
pub(crate) fn entity_name_key(name: &str) -> String {
    let words = name_words(name);
    let mut key = words.as_str();
    while let Some(rest) = legal_forms()
        .iter()
        .find_map(|(form, edge, joined)| strip_form(key, form, *edge, *joined))
    {
        key = rest;
    }
    key.to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdentityMatch {
    Exact(String),
    Ambiguous(Vec<String>),
    Unmatched,
}

const SYMBOL_KEYS: [&str; 9] = [
    "ticker",
    "tickers",
    "symbol",
    "symbols",
    "identifier",
    "identifiers",
    "aliases",
    "ticker_symbol",
    "tickersymbol",
];
const MARKET_KEYS: [&str; 4] = ["exchange", "exchanges", "market", "markets"];

fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn strings(value: &Value, output: &mut BTreeSet<String>) {
    match value {
        Value::String(value) => {
            output.insert(normalized(value));
        }
        Value::Array(values) => values.iter().for_each(|value| strings(value, output)),
        Value::Object(values) => values.values().for_each(|value| strings(value, output)),
        _ => {}
    }
}

fn property_values(node: &GraphNode, keys: &[&str]) -> BTreeSet<String> {
    let mut output = BTreeSet::new();
    for (key, value) in &node.properties {
        if keys.contains(&key.to_lowercase().as_str()) {
            strings(value, &mut output);
        }
    }
    output.retain(|value| !value.is_empty());
    output
}

enum MarketAgreement {
    Agrees,
    Conflicts,
    Undecided,
}

fn market_agreement(stated: &str, followed: &str) -> MarketAgreement {
    if stated.eq_ignore_ascii_case(followed) {
        return MarketAgreement::Agrees;
    }
    match (exchange_time_zone(stated), exchange_time_zone(followed)) {
        (Some(a), Some(b)) if a == b => MarketAgreement::Agrees,
        (Some(_), Some(_)) => MarketAgreement::Conflicts,
        _ => MarketAgreement::Undecided,
    }
}

fn followed_market(item: &MapContextItem) -> &str {
    let market = item.market.trim();
    if market.eq_ignore_ascii_case(super::UNKNOWN_MARKET) {
        ""
    } else {
        market
    }
}

pub(crate) fn resolve_followed(item: &MapContextItem, nodes: &[GraphNode]) -> IdentityMatch {
    let symbol = normalized(&item.symbol);
    let by_symbol = |node: &&GraphNode| {
        let mut ids = node
            .aliases
            .iter()
            .map(|value| normalized(value))
            .collect::<BTreeSet<_>>();
        ids.extend(property_values(node, &SYMBOL_KEYS));
        ids.contains(&symbol)
    };
    let mut hits: Vec<&GraphNode> = nodes.iter().filter(by_symbol).collect();
    if hits.is_empty() {
        if let Some(name) = item.display_name.as_deref().map(normalized) {
            hits = nodes
                .iter()
                .filter(|node| normalized(&node.name) == name)
                .collect();
        }
    }
    let market = followed_market(item);
    let mut agreeing = Vec::new();
    let mut unstated = Vec::new();
    for node in hits {
        let stated = property_values(node, &MARKET_KEYS);
        if stated.is_empty() || market.is_empty() {
            unstated.push(node.id.clone());
            continue;
        }
        let verdicts: Vec<MarketAgreement> = stated
            .iter()
            .map(|value| market_agreement(value, market))
            .collect();
        if verdicts
            .iter()
            .any(|v| matches!(v, MarketAgreement::Agrees))
        {
            agreeing.push(node.id.clone());
        } else if !verdicts
            .iter()
            .all(|v| matches!(v, MarketAgreement::Conflicts))
        {
            unstated.push(node.id.clone());
        }
    }
    let mut ids = if agreeing.is_empty() {
        unstated
    } else {
        agreeing
    };
    ids.sort();
    ids.dedup();
    match ids.len() {
        0 => IdentityMatch::Unmatched,
        1 => IdentityMatch::Exact(ids.remove(0)),
        _ => IdentityMatch::Ambiguous(ids),
    }
}

pub(crate) fn match_items(
    items: &[MapContextItem],
    nodes: &[GraphNode],
) -> (BTreeSet<String>, Vec<MapContextItem>) {
    let mut matched = BTreeSet::new();
    let mut unmatched = Vec::new();
    for item in items {
        match resolve_followed(item, nodes) {
            IdentityMatch::Exact(id) => {
                matched.insert(id);
            }
            _ => unmatched.push(item.clone()),
        }
    }
    unmatched.sort_by(|a, b| {
        (&a.symbol, &a.market, &a.display_name).cmp(&(&b.symbol, &b.market, &b.display_name))
    });
    (matched, unmatched)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use serde_json::json;

    fn node(id: &str, name: &str, ticker: &str, exchange: Option<&str>) -> GraphNode {
        let mut properties = BTreeMap::new();
        properties.insert("ticker".to_owned(), json!(ticker));
        if let Some(exchange) = exchange {
            properties.insert("exchange".to_owned(), json!(exchange));
        }
        GraphNode {
            id: id.into(),
            name: name.into(),
            aliases: vec![],
            description: String::new(),
            kind: "Organization".into(),
            source_role: "entity".into(),
            properties,
        }
    }
    fn item(symbol: &str, market: &str) -> MapContextItem {
        MapContextItem {
            symbol: symbol.into(),
            market: market.into(),
            display_name: None,
        }
    }

    #[test]
    fn legal_forms_and_punctuation_do_not_split_one_company() {
        for (spellings, key) in [
            (
                &[
                    "Acme",
                    "ACME Inc.",
                    "Acme, Inc",
                    "Acme Co., Ltd.",
                    "Acme Corporation",
                ][..],
                "acme",
            ),
            (
                &["Halvorsen Cellworks ASA", "Halvorsen Cellworks"][..],
                "halvorsen cellworks",
            ),
            (
                &["Procter & Gamble Co.", "PROCTER & GAMBLE"][..],
                "procter & gamble",
            ),
            (
                &[
                    "삼성전자 주식회사",
                    "삼성전자(주)",
                    "㈜삼성전자",
                    "주식회사 삼성전자",
                    "삼성전자주식회사",
                ][..],
                "삼성전자",
            ),
            (
                &["トヨタ自動車株式会社", "株式会社トヨタ自動車", "ﾄﾖﾀ自動車"][..],
                "トヨタ自動車",
            ),
            (&["Straße Werke GmbH", "STRASSE WERKE"][..], "strasse werke"),
        ] {
            for spelling in spellings {
                assert_eq!(entity_name_key(spelling), key, "{spelling}");
            }
        }
    }

    #[test]
    fn names_that_are_not_legal_forms_keep_their_words() {
        for (name, key) in [
            ("AS Roma", "as roma"),
            ("Company of Heroes", "company of heroes"),
            ("제주항공", "제주항공"),
            ("3M", "3m"),
            ("AT&T Inc.", "at&t"),
            ("Inc.", "inc"),
            ("Co Ltd", "co"),
            ("주식회사", "주식회사"),
        ] {
            assert_eq!(entity_name_key(name), key, "{name}");
        }
    }

    #[test]
    fn stated_market_picks_the_node() {
        let nodes = [
            node("n1", "Alpha Korea", "A", Some("KRX")),
            node("n2", "Alpha Inc", "A", Some("NASDAQ")),
        ];
        assert_eq!(
            resolve_followed(&item("A", "KRX"), &nodes),
            IdentityMatch::Exact("n1".into())
        );
        assert_eq!(
            resolve_followed(&item("A", "KOSPI"), &nodes),
            IdentityMatch::Exact("n1".into())
        );
        assert_eq!(
            resolve_followed(&item("A", "NYSE"), &nodes),
            IdentityMatch::Exact("n2".into())
        );
        assert_eq!(
            resolve_followed(&item("A", "TSE"), &nodes),
            IdentityMatch::Unmatched
        );
    }

    #[test]
    fn undecided_hits_are_ambiguous_and_match_nothing() {
        let nodes = [
            node("n1", "Alpha Korea", "A", None),
            node("n2", "Alpha Inc", "A", None),
        ];
        assert_eq!(
            resolve_followed(&item("A", "KRX"), &nodes),
            IdentityMatch::Ambiguous(vec!["n1".into(), "n2".into()])
        );
        let (matched, unmatched) = match_items(&[item("A", "KRX")], &nodes);
        assert!(matched.is_empty());
        assert_eq!(unmatched.len(), 1);
    }

    #[test]
    fn an_unknown_or_unrecognized_market_cannot_exclude_a_node() {
        let nodes = [node(
            "n1",
            "Alpha Inc",
            "A",
            Some("New York Stock Exchange"),
        )];
        assert_eq!(
            resolve_followed(&item("A", "UNKNOWN"), &nodes),
            IdentityMatch::Exact("n1".into())
        );
        assert_eq!(
            resolve_followed(&item("A", "NYSE"), &nodes),
            IdentityMatch::Exact("n1".into())
        );
        let listed = [
            node("n1", "Alpha Korea", "A", Some("KRX")),
            node("n2", "Alpha Inc", "A", Some("New York Stock Exchange")),
        ];
        assert_eq!(
            resolve_followed(&item("A", "KRX"), &listed),
            IdentityMatch::Exact("n1".into())
        );
        assert_eq!(
            resolve_followed(&item("A", "NASDAQ"), &listed),
            IdentityMatch::Exact("n2".into())
        );
    }

    #[test]
    fn a_single_node_without_a_market_still_matches() {
        let nodes = [
            node("n1", "Alpha", "A", None),
            node("n2", "Beta", "B", Some("KRX")),
        ];
        assert_eq!(
            resolve_followed(&item("A", "KRX"), &nodes),
            IdentityMatch::Exact("n1".into())
        );
    }
}
