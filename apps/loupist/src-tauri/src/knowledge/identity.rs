//! Which World nodes a followed symbol or an asset page means.

use std::collections::BTreeSet;

use serde_json::Value;

use super::assets::class::exchange_time_zone;
use super::GraphNode;
use crate::store::user_context::MapContextItem;

/// A hit on a symbol or name never picks among several nodes: `Ambiguous`
/// matches nothing.
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

/// Two markets conflict only when both are recognized venues in different
/// places; an unrecognized name ("New York Stock Exchange") cannot exclude a node.
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

/// Onboarding stores `UNKNOWN` when the user gave no market.
fn followed_market(item: &MapContextItem) -> &str {
    let market = item.market.trim();
    if market.eq_ignore_ascii_case("UNKNOWN") {
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
        if verdicts.iter().any(|v| matches!(v, MarketAgreement::Agrees)) {
            agreeing.push(node.id.clone());
        } else if !verdicts.iter().all(|v| matches!(v, MarketAgreement::Conflicts)) {
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

/// The nodes each followed item resolves to, and the items that resolve to none
/// (an ambiguous item is unmatched).
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
        let nodes = [node("n1", "Alpha Inc", "A", Some("New York Stock Exchange"))];
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
