use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, OnceLock, PoisonError};

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::assets::class::exchange_time_zone;
use super::{ClaimStatus, GraphNode, WorldSnapshotBody};
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
    ("s a b", Edge::Trailing, false),
    ("sab", Edge::Trailing, false),
    ("de c v", Edge::Trailing, false),
    ("de cv", Edge::Trailing, false),
    ("s a b de c v", Edge::Trailing, false),
    ("s a de c v", Edge::Trailing, false),
    ("sa de cv", Edge::Trailing, false),
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
/// spaceless key. Returns the worded key.
fn spelling_keys(segment: &str, keys: &mut BTreeSet<String>) -> Option<String> {
    let words = name_words(segment);
    let key = strip_legal_forms(&words);
    if key.is_empty() {
        return None;
    }
    keys.insert(key.chars().filter(|c| *c != ' ').collect());
    keys.insert(key.to_owned());
    Some(key.to_owned())
}

/// The code of `text` when the whole of it is `EXCH:CODE`: two to ten letters,
/// a colon, and one to ten letters, digits, dots or hyphens. A colon anywhere
/// else is punctuation.
fn listing_code(text: &str) -> Option<String> {
    let (exchange, code) = text.trim().split_once(':')?;
    let exchange_ok =
        (2..=10).contains(&exchange.len()) && exchange.chars().all(|c| c.is_ascii_alphabetic());
    let code_ok = (1..=10).contains(&code.len())
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    (exchange_ok && code_ok).then(|| code.to_ascii_lowercase())
}

/// One company's identity across the ways pages spell it: "Acme", "ACME
/// Inc.", "Acme Co., Ltd.", "U.S. Bancorp" and "US Bancorp" share a key. A
/// parenthetical in another script is another spelling, so
/// `삼성전자(Samsung Electronics)` carries a key for each, while a qualifier in
/// the name's own script (`(한국)`, `(Holdings)`, `(Class A)`) is no key, and
/// `(주)`, `(유)`, `(사)`, `(재)` and `㈜` are legal notation. A listing code,
/// parenthesized (`(005930)`, `(NASDAQ:AAPL)`) or after the name, is a key of
/// its own, and a spelling that is only `EXCH:CODE` keeps its words whole.
/// Two companies may share a key ("Bath Spa" and "Bath S.p.A."): a key tells
/// learning when two observations might be one company, never that they are
/// two.
pub(crate) fn entity_name_keys(name: &str) -> BTreeSet<String> {
    keyed(name).keys
}

/// The key that names the company in one spelling: the outer name's words
/// without legal forms, `apple` for "Apple Inc. (NASDAQ:AAPL)", and none for
/// a spelling that is only notation.
pub(crate) fn primary_key(name: &str) -> Option<String> {
    keyed(name).primary
}

/// One spelling's keys, and the one of them that names the company.
struct Keyed {
    primary: Option<String>,
    keys: BTreeSet<String>,
}

fn keyed(name: &str) -> Keyed {
    let text: String = name.nfkc().collect();
    let mut keys = BTreeSet::new();
    // A spelling that is only a listing code ("EXCH:AG") keeps its words
    // whole: its suffix is a code, not a legal form.
    if let Some(code) = listing_code(&text) {
        let plain = name_words(&text);
        keys.insert(plain.clone());
        keys.insert(code);
        return Keyed {
            primary: Some(plain),
            keys,
        };
    }
    let text = PARENTHESIZED_LEGAL_NOTATION
        .iter()
        .fold(text, |text, notation| text.replace(notation, " "));
    let segments = segments(&text);
    let (outer, inner) = segments.split_first().expect("the outer segment");
    // A listing code after the name ("Apple Inc. NASDAQ:AAPL") is a key of
    // its own, not words of the name.
    let outer = outer.trim();
    let outer = match outer.rsplit_once(char::is_whitespace) {
        Some((name, last)) if listing_code(last).is_some() => {
            keys.extend(listing_code(last));
            name
        }
        _ => outer,
    };
    let primary = spelling_keys(outer, &mut keys);
    let outer_scripts = scripts(outer);
    for segment in inner {
        if let Some(code) = listing_code(segment) {
            keys.insert(code);
            continue;
        }
        let inner_scripts = scripts(segment);
        let another_script = !inner_scripts.is_empty() && inner_scripts.is_disjoint(&outer_scripts);
        // A listing code in parentheses ("(005930)") names the issuer as surely as a name does.
        let digits = segment.trim();
        let year = digits.len() == 4 && matches!(digits.parse::<u32>(), Ok(1900..=2099));
        let code = digits.len() >= 4 && digits.chars().all(|c| c.is_ascii_digit()) && !year;
        if another_script || code {
            spelling_keys(segment, &mut keys);
        }
    }
    Keyed { primary, keys }
}

/// One World revision's identity index: each node's spellings and kind, the
/// nodes each Source span confirms, the company keys of a node computed the
/// first time learning asks about it and kept for the filings after, and the
/// components of nodes whose keys meet, computed once for the revision.
pub(crate) struct WorldIdentity {
    nodes: BTreeMap<String, IdentityNode>,
    /// The endpoints of the active claims whose evidence cites each span.
    confirmed_by_span: BTreeMap<String, BTreeSet<String>>,
    keys: BTreeMap<String, BTreeSet<String>>,
    components: Option<BTreeMap<String, String>>,
}

struct IdentityNode {
    spellings: Vec<String>,
    kind: String,
}

#[cfg(test)]
thread_local! {
    /// How many nodes this thread computed keys for.
    pub(crate) static KEY_COMPUTATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl WorldIdentity {
    pub(crate) fn new(world: &WorldSnapshotBody) -> Self {
        let mut confirmed_by_span: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for claim in world
            .claims
            .iter()
            .filter(|claim| claim.status == ClaimStatus::Active)
        {
            for evidence in &claim.evidence {
                confirmed_by_span
                    .entry(evidence.span_id.clone())
                    .or_default()
                    .extend(claim.endpoints.iter().map(|e| e.node_id.clone()));
            }
        }
        Self {
            nodes: world
                .nodes
                .iter()
                .map(|node| {
                    let spellings = std::iter::once(&node.name)
                        .chain(&node.aliases)
                        .cloned()
                        .collect();
                    let node_identity = IdentityNode {
                        spellings,
                        kind: node.kind.clone(),
                    };
                    (node.id.clone(), node_identity)
                })
                .collect(),
            confirmed_by_span,
            keys: BTreeMap::new(),
            components: None,
        }
    }

    /// The keys of `node`'s name and aliases, or None when the World has no
    /// such node.
    pub(crate) fn keys(&mut self, node: &str) -> Option<&BTreeSet<String>> {
        let spellings = &self.nodes.get(node)?.spellings;
        Some(self.keys.entry(node.to_owned()).or_insert_with(|| {
            #[cfg(test)]
            KEY_COMPUTATIONS.with(|count| count.set(count.get() + 1));
            spellings
                .iter()
                .flat_map(|spelling| entity_name_keys(spelling))
                .collect()
        }))
    }

    /// The one company the spans a page cites confirm under `primary`, the
    /// primary key of the anchor's name: an endpoint of an active claim whose
    /// evidence is one of `cited`, of a kind `company` accepts, whose name or
    /// alias has that primary key, when no other node on any of
    /// `source_spans` (every span of the cited Sources) has it. So "Halvorsen
    /// Cellworks ASA", claimed from the report a "Halvorsen Cellworks" page
    /// cites, is its company, while a counterparty claimed from another span
    /// of the report, a Product, a withdrawn claim, or a second "Hana" on the
    /// report confirm nothing.
    pub(crate) fn candidate(
        &self,
        primary: &str,
        cited: &BTreeSet<String>,
        source_spans: &BTreeSet<String>,
        company: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let named = |id: &str| {
            self.nodes.get(id).is_some_and(|node| {
                node.spellings
                    .iter()
                    .filter_map(|spelling| primary_key(spelling))
                    .any(|key| key == primary)
            })
        };
        let on = |spans: &BTreeSet<String>| {
            spans
                .iter()
                .filter_map(|span| self.confirmed_by_span.get(span))
                .flatten()
                .map(String::as_str)
                .collect::<BTreeSet<&str>>()
        };
        let candidates: BTreeSet<&str> = on(cited)
            .into_iter()
            .filter(|id| named(id) && company(&self.nodes[*id].kind))
            .collect();
        let [candidate] = candidates.into_iter().collect::<Vec<_>>()[..] else {
            return None;
        };
        let rival = on(source_spans)
            .into_iter()
            .chain(on(cited))
            .any(|id| id != candidate && named(id));
        (!rival).then(|| candidate.to_owned())
    }

    /// The component `node` belongs to: the smallest node id among the nodes
    /// reachable from it through shared keys, so a node that spells both
    /// `삼성전자` and `Samsung Electronics` joins the nodes spelling one each.
    /// None when the World has no such node.
    pub(crate) fn component(&mut self, node: &str) -> Option<String> {
        if !self.nodes.contains_key(node) {
            return None;
        }
        if self.components.is_none() {
            self.components = Some(self.join_components());
        }
        self.components.as_ref()?.get(node).cloned()
    }

    fn join_components(&mut self) -> BTreeMap<String, String> {
        let ids: Vec<String> = self.nodes.keys().cloned().collect();
        let mut parent: Vec<usize> = (0..ids.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut holder: BTreeMap<String, usize> = BTreeMap::new();
        for (i, id) in ids.iter().enumerate() {
            let keys = self.keys(id).cloned().unwrap_or_default();
            for key in keys {
                match holder.get(&key) {
                    Some(&j) => {
                        let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                        parent[a] = b;
                    }
                    None => {
                        holder.insert(key, i);
                    }
                }
            }
        }
        let mut smallest: BTreeMap<usize, &str> = BTreeMap::new();
        for (i, id) in ids.iter().enumerate() {
            let r = root(&mut parent, i);
            let entry = smallest.entry(r).or_insert(id);
            if id.as_str() < *entry {
                *entry = id;
            }
        }
        ids.iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), smallest[&root(&mut parent, i)].to_owned()))
            .collect()
    }
}

/// The last few World revisions' identity indexes, process-wide, by Loupist
/// and revision.
static WORLD_IDENTITY: Mutex<VecDeque<(String, String, WorldIdentity)>> =
    Mutex::new(VecDeque::new());
const CACHED_REVISIONS: usize = 4;

/// Reads `revision` of `loupist`'s World through its identity index. `load`
/// reads the revision's snapshot the first time it is asked about; later
/// filings on the same revision reuse the index.
pub(crate) fn with_world_identity<R>(
    loupist: &str,
    revision: &str,
    load: impl FnOnce() -> super::KnowledgeResult<WorldSnapshotBody>,
    read: impl FnOnce(&mut WorldIdentity) -> R,
) -> super::KnowledgeResult<R> {
    let cached =
        |entry: &(String, String, WorldIdentity)| entry.0 == loupist && entry.1 == revision;
    let mut cache = WORLD_IDENTITY
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !cache.iter().any(cached) {
        drop(cache);
        let identity = WorldIdentity::new(&load()?);
        cache = WORLD_IDENTITY
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !cache.iter().any(cached) {
            if cache.len() == CACHED_REVISIONS {
                cache.pop_front();
            }
            cache.push_back((loupist.to_owned(), revision.to_owned(), identity));
        }
    }
    let entry = cache
        .iter_mut()
        .find(|entry| cached(entry))
        .expect("the revision was just cached");
    Ok(read(&mut entry.2))
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
            (
                &[
                    "CEMEX",
                    "CEMEX, S.A.B. de C.V.",
                    "CEMEX, S.A. de C.V.",
                    "Cemex SAB de CV",
                    "Cemex SA de CV",
                    "CEMEX S.A.B.",
                ][..],
                "cemex",
            ),
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
                &["samsung electronics", "samsungelectronics", "005930"][..],
            ),
            ("Acme (2024)", &["acme"][..]),
            (
                "Toyota Motor (7203)",
                &["toyota motor", "toyotamotor", "7203"][..],
            ),
            ("Acme (12)", &["acme"][..]),
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
        assert_eq!(keys("EXCH:AG"), set(&["exch ag", "ag"]));
        assert_eq!(keys("EXCH:SE"), set(&["exch se", "se"]));
        assert_eq!(keys("KRX:005930"), set(&["krx 005930", "005930"]));
        assert!(keys("EXCH:AG").is_disjoint(&keys("EXCH:SE")));
        assert_eq!(primary_key("EXCH:AG").as_deref(), Some("exch ag"));
    }

    #[test]
    fn a_listing_code_beside_the_name_is_a_key_of_its_own() {
        for (name, expected) in [
            ("Apple Inc. (NASDAQ:AAPL)", &["apple", "aapl"][..]),
            ("Apple Inc. NASDAQ:AAPL", &["apple", "aapl"][..]),
            ("삼성전자 (KRX:005930)", &["삼성전자", "005930"][..]),
            (
                "Samsung Electronics (KRX:005930)",
                &["samsung electronics", "samsungelectronics", "005930"][..],
            ),
            (
                "Acme: The Sequel Inc.",
                &["acme the sequel", "acmethesequel"][..],
            ),
            ("Acme (Class A) NYSE:ACM.A", &["acme", "acm.a"][..]),
        ] {
            assert_eq!(keys(name), set(expected), "{name}");
        }
        assert!(!keys("Apple").is_disjoint(&keys("Apple Inc. (NASDAQ:AAPL)")));
        assert!(!keys("삼성전자").is_disjoint(&keys("삼성전자 (KRX:005930)")));
    }

    #[test]
    fn the_primary_key_is_the_outer_name_without_legal_forms() {
        for (name, primary) in [
            ("Apple Inc. (NASDAQ:AAPL)", Some("apple")),
            ("Halvorsen Cellworks ASA", Some("halvorsen cellworks")),
            ("Hana Co.", Some("hana")),
            ("U.S. Bancorp", Some("u s bancorp")),
            ("삼성전자(Samsung Electronics)", Some("삼성전자")),
            ("(주)", None),
        ] {
            assert_eq!(primary_key(name).as_deref(), primary, "{name}");
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
