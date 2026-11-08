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

/// Legal-form words a company name may carry, written as filings write them
/// and as they read once punctuation has become spaces (`S.A.` is `s a`). A
/// `joined` form also matches with no space before or after it, as Korean
/// and Japanese names write it. Only Korean and Japanese forms lead a name, so
/// "AS Roma" keeps its "AS". The longest matching form is stripped first, so
/// "& Co." goes as a whole and leaves no `&` behind. `(주)` and `㈜` go earlier,
/// while their parentheses still mark them: the bare syllable `주` is a word.
const LEGAL_FORMS: &[(&str, Edge, bool)] = &[
    ("주식회사", Edge::Leading, true),
    ("유한회사", Edge::Leading, true),
    ("株式会社", Edge::Leading, true),
    ("주식회사", Edge::Trailing, true),
    ("株式会社", Edge::Trailing, true),
    ("& co", Edge::Trailing, false),
    ("& company", Edge::Trailing, false),
    ("and co", Edge::Trailing, false),
    ("and company", Edge::Trailing, false),
    ("&", Edge::Trailing, false),
    ("inc", Edge::Trailing, false),
    ("incorporated", Edge::Trailing, false),
    ("corp", Edge::Trailing, false),
    ("corporation", Edge::Trailing, false),
    ("co", Edge::Trailing, false),
    ("company", Edge::Trailing, false),
    ("ltd", Edge::Trailing, false),
    ("limited", Edge::Trailing, false),
    ("llc", Edge::Trailing, false),
    ("llp", Edge::Trailing, false),
    ("lp", Edge::Trailing, false),
    ("plc", Edge::Trailing, false),
    ("kg", Edge::Trailing, false),
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
    ("s a", Edge::Trailing, false),
    ("s a s", Edge::Trailing, false),
    ("s p a", Edge::Trailing, false),
    ("s r l", Edge::Trailing, false),
    ("n v", Edge::Trailing, false),
    ("b v", Edge::Trailing, false),
    ("a g", Edge::Trailing, false),
    ("p l c", Edge::Trailing, false),
    ("l l c", Edge::Trailing, false),
    ("l l p", Edge::Trailing, false),
    ("l p", Edge::Trailing, false),
    ("g m b h", Edge::Trailing, false),
    ("k g", Edge::Trailing, false),
];

/// Latin accents as NFKD exposes them, U+0300 to U+036F. Dropping only these
/// folds "Nestlé" to "nestle" and leaves kana voicing marks alone.
fn is_latin_accent(c: &char) -> bool {
    ('\u{300}'..='\u{36f}').contains(c)
}

fn is_apostrophe(c: &char) -> bool {
    matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{02bc}')
}

/// Accents dropped, NFKC, casefold, apostrophes removed, and every other
/// character but a letter, a digit or `&` as a space, whitespace collapsed.
fn name_words(name: &str) -> String {
    let plain: String = name.nfkd().filter(|c| !is_latin_accent(c)).nfkc().collect();
    let folded = super::world_extract::casefold(&plain);
    let spaced: String = folded
        .chars()
        .filter(|c| !is_apostrophe(c))
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

/// [`LEGAL_FORMS`] as words, longest first.
fn legal_forms() -> &'static [(String, Edge, bool)] {
    static FORMS: OnceLock<Vec<(String, Edge, bool)>> = OnceLock::new();
    FORMS.get_or_init(|| {
        let mut forms: Vec<_> = LEGAL_FORMS
            .iter()
            .map(|&(form, edge, joined)| (name_words(form), edge, joined))
            .collect();
        forms.sort_by_key(|(form, _, _)| std::cmp::Reverse(form.len()));
        forms
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

/// `words` without a leading "the" and without legal forms, while a name
/// remains; a name that is only a legal form keeps it.
fn strip_legal_forms(words: &str) -> &str {
    let mut key = words.strip_prefix("the ").unwrap_or(words);
    while let Some(rest) = legal_forms()
        .iter()
        .find_map(|(form, edge, joined)| strip_form(key, form, *edge, *joined))
    {
        key = rest;
    }
    key
}

/// `text` outside its parentheses, then the inside of each parenthetical.
fn segments(text: &str) -> Vec<String> {
    let mut outer = String::new();
    let mut inner = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' if depth == 0 => depth = 1,
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' if depth == 1 => {
                depth = 0;
                inner.push(std::mem::take(&mut current));
            }
            ')' if depth > 1 => {
                depth -= 1;
                current.push(c);
            }
            _ if depth == 0 => outer.push(c),
            _ => current.push(c),
        }
    }
    if depth > 0 {
        inner.push(current);
    }
    std::iter::once(outer).chain(inner).collect()
}

/// Korean legal notation that only counts inside its parentheses: the bare
/// syllable is a word ("엔씨소프트 주"). `㈜` is `(주)` after NFKC.
const PARENTHESIZED_LEGAL_NOTATION: [&str; 4] = ["(주)", "(유)", "(사)", "(재)"];

/// The scripts a name is written in. Kana and Han are one script, since a
/// Japanese name mixes them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Script {
    Latin,
    Hangul,
    Cjk,
    Other,
}

fn script_of(c: char) -> Option<Script> {
    let script = match c as u32 {
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Script::Hangul,
        0x3040..=0x30FF
        | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF66..=0xFF9F
        | 0x20000..=0x2FFFF => Script::Cjk,
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x024F | 0x1E00..=0x1EFF => Script::Latin,
        _ if c.is_alphabetic() => Script::Other,
        _ => return None,
    };
    Some(script)
}

fn scripts(text: &str) -> BTreeSet<Script> {
    text.chars().filter_map(script_of).collect()
}

/// The keys of one spelling: its words without legal forms, and the same
/// with no spaces, so "U.S. Bancorp" meets "US Bancorp" and "J.P. Morgan"
/// meets "JP Morgan" and "JPMorgan". Joining only runs of initials would
/// unify nothing more: two spellings with one initials-joined key have one
/// spaceless key.
fn spelling_keys(segment: &str, keys: &mut BTreeSet<String>) {
    let words = name_words(segment);
    let key = strip_legal_forms(&words);
    if key.is_empty() {
        return;
    }
    keys.insert(key.chars().filter(|c| *c != ' ').collect());
    keys.insert(key.to_owned());
}

/// One company's identity across the ways pages spell it: "Acme", "ACME
/// Inc.", "Acme Co., Ltd.", "U.S. Bancorp" and "US Bancorp" share a key. A
/// parenthetical in another script is another spelling, so
/// `삼성전자(Samsung Electronics)` carries a key for each, while a qualifier in
/// the name's own script (`(한국)`, `(Holdings)`, `(Class A)`) is no key, and
/// `(주)`, `(유)`, `(사)`, `(재)` and `㈜` are legal notation. A ticker-shaped
/// alias (`EXCH:AG`) keeps its words whole, since its suffix is a code, not a
/// legal form. Two companies may share a key ("Bath Spa" and "Bath S.p.A."):
/// a key tells learning when two observations might be one company, never
/// that they are two.
pub(crate) fn entity_name_keys(name: &str) -> BTreeSet<String> {
    let text: String = name.nfkc().collect();
    let mut keys = BTreeSet::new();
    if text.contains(':') {
        let plain = name_words(&text);
        if !plain.is_empty() {
            keys.insert(plain);
        }
        return keys;
    }
    let text = PARENTHESIZED_LEGAL_NOTATION
        .iter()
        .fold(text, |text, notation| text.replace(notation, " "));
    let segments = segments(&text);
    let (outer, inner) = segments.split_first().expect("the outer segment");
    spelling_keys(outer, &mut keys);
    let outer_scripts = scripts(outer);
    for segment in inner {
        let inner_scripts = scripts(segment);
        if !inner_scripts.is_empty() && inner_scripts.is_disjoint(&outer_scripts) {
            spelling_keys(segment, &mut keys);
        }
    }
    keys
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

    fn keys(name: &str) -> BTreeSet<String> {
        entity_name_keys(name)
    }
    fn set(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| (*k).to_owned()).collect()
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
            (&["(유)네이버", "네이버(유)", "네이버"][..], "네이버"),
            (&["(사)한국능률협회", "한국능률협회"][..], "한국능률협회"),
            (&["(재)한국연구재단", "한국연구재단"][..], "한국연구재단"),
            // Notation beside a Latin name would otherwise be another script.
            (&["(주)LG", "LG(주)", "LG"][..], "lg"),
            (&["(유)SK", "SK"][..], "sk"),
            (&["(사)KT", "KT"][..], "kt"),
            (&["(재)KB", "KB"][..], "kb"),
            (
                &["トヨタ自動車株式会社", "株式会社トヨタ自動車", "ﾄﾖﾀ自動車"][..],
                "トヨタ自動車",
            ),
            (&["Straße Werke GmbH", "STRASSE WERKE"][..], "strasse werke"),
            (
                &["Ferroluz S.A.", "Ferroluz SA", "Ferroluz"][..],
                "ferroluz",
            ),
            (&["Philips N.V.", "Philips"][..], "philips"),
            (
                &[
                    "Acme L.L.C.",
                    "Acme, L.L.C.",
                    "Acme LLC",
                    "Acme L.P.",
                    "Acme LP",
                    "Acme L.L.P.",
                    "Acme GmbH & Co. KG",
                    "Acme G.m.b.H.",
                ][..],
                "acme",
            ),
            (
                &[
                    "Wells Fargo & Company",
                    "Wells Fargo & Co.",
                    "Wells Fargo and Company",
                    "Wells Fargo and Co.",
                    "Wells Fargo &",
                    "Wells Fargo",
                ][..],
                "wells fargo",
            ),
            (
                &["McDonald's Corporation", "McDonald\u{2019}s", "McDonalds"][..],
                "mcdonalds",
            ),
            (&["Nestlé S.A.", "Nestle SA", "NESTLÉ"][..], "nestle"),
            (&["The Coca-Cola Company", "Coca-Cola"][..], "coca cola"),
            (
                &[
                    "(주)주성엔지니어링",
                    "㈜주성엔지니어링",
                    "주성엔지니어링(주)",
                    "주성엔지니어링 (주)",
                    "주성엔지니어링",
                ][..],
                "주성엔지니어링",
            ),
        ] {
            let expected = keys(spellings[0]);
            assert!(expected.contains(key), "{key} in {expected:?}");
            for spelling in spellings {
                assert_eq!(keys(spelling), expected, "{spelling}");
            }
        }
    }

    #[test]
    fn initialisms_and_spacing_do_not_split_one_company() {
        for spellings in [
            &["U.S. Bancorp", "US Bancorp", "U S Bancorp"][..],
            &["J.P. Morgan", "JP Morgan", "JPMorgan", "J. P. Morgan"][..],
            &["A.O. Smith Corporation", "AO Smith", "A. O. Smith Corp."][..],
            &["JPMorgan Chase & Co.", "J.P. Morgan Chase"][..],
            &["Super Micro Computer", "Supermicro Computer"][..],
        ] {
            let shared = spellings
                .iter()
                .map(|s| keys(s))
                .reduce(|a, b| &a & &b)
                .unwrap();
            assert!(!shared.is_empty(), "{spellings:?} share no key");
        }
        assert_eq!(keys("U.S. Bancorp"), set(&["u s bancorp", "usbancorp"]));
        assert_eq!(keys("JPMorgan"), set(&["jpmorgan"]));
    }

    #[test]
    fn names_that_are_not_legal_forms_keep_their_words() {
        for (name, key) in [
            ("AS Roma", "as roma"),
            ("Company of Heroes", "company of heroes"),
            ("Johnson & Johnson", "johnson & johnson"),
            ("제주항공", "제주항공"),
            ("주 성엔지니어링", "주 성엔지니어링"),
            ("엔씨소프트 주", "엔씨소프트 주"),
            ("3M", "3m"),
            ("AT&T Inc.", "at&t"),
            ("Inc.", "inc"),
            ("Co Ltd", "co"),
            ("주식회사", "주식회사"),
            ("The", "the"),
        ] {
            assert!(keys(name).contains(key), "{name}: {:?}", keys(name));
        }
    }

    #[test]
    fn a_parenthetical_in_another_script_is_another_spelling_of_the_name() {
        for (name, expected) in [
            (
                "삼성전자(Samsung Electronics)",
                &["삼성전자", "samsung electronics", "samsungelectronics"][..],
            ),
            ("네이버(NAVER)", &["네이버", "naver"][..]),
            ("(Samsung) 삼성전자(주)", &["삼성전자", "samsung"][..]),
            (
                "トヨタ自動車(Toyota Motor)",
                &["トヨタ自動車", "toyota motor", "toyotamotor"][..],
            ),
        ] {
            assert_eq!(keys(name), set(expected), "{name}");
        }
    }

    #[test]
    fn a_qualifier_in_the_names_own_script_is_not_a_key() {
        for (name, expected) in [
            ("삼성전자(한국)", &["삼성전자"][..]),
            ("LG전자(한국)", &["lg전자"][..]),
            ("Acme (Inc.)", &["acme"][..]),
            ("Acme (Holdings)", &["acme"][..]),
            ("Acme (Korea)", &["acme"][..]),
            ("Samsung (Class A)", &["samsung"][..]),
            (
                "Samsung Electronics Co., Ltd. (005930)",
                &["samsung electronics", "samsungelectronics"][..],
            ),
        ] {
            assert_eq!(keys(name), set(expected), "{name}");
        }
        for (a, b) in [
            ("삼성전자(한국)", "LG전자(한국)"),
            ("Acme (Inc.)", "Beta (Inc.)"),
            ("Acme (Korea)", "Beta (Korea)"),
            ("Samsung (Class A)", "Hyundai (Class A)"),
            ("(유)네이버", "(유)카카오"),
        ] {
            assert!(keys(a).is_disjoint(&keys(b)), "{a} / {b}");
        }
    }

    #[test]
    fn a_ticker_shaped_alias_keeps_its_code() {
        assert_eq!(keys("EXCH:AG"), set(&["exch ag"]));
        assert_eq!(keys("EXCH:SE"), set(&["exch se"]));
        assert_eq!(keys("KRX:005930"), set(&["krx 005930"]));
        assert!(keys("EXCH:AG").is_disjoint(&keys("EXCH:SE")));
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
