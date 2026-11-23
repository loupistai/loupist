//! Identifying codes in World names: a listing ticker with or without its
//! exchange (`0489.HK`, `HKEX:0489`, `STLA`), an ISIN, a FIGI, an LEI, a CIK,
//! a DART corp code or another registry number a node's properties state.
//!
//! Extraction names an entity with the codes its Source gives ("Include
//! explicit identifying codes in entity names"). The host holds two rules over
//! the model's resolution, and nothing else:
//!
//! - It refuses a merge, or an alias, when the codes differ and the names do
//!   not agree ([`Codes::refuses`]): "Stellantis STLA" is never "Dongfeng Motor
//!   0489.HK". Codes that differ under agreeing names ("Toyota TM" and "Toyota
//!   7203.T", "Alphabet GOOG" and "Alphabet GOOGL") are listings of one company
//!   as often as two companies, so the model decides them.
//! - It joins two names when their codes are equal and either the code is
//!   strongly identified or the names agree ([`Codes::joins`]). An equal bare
//!   trailing capital under names that do not agree ("Google AI", "Meta AI")
//!   decides nothing.
//!
//! Names agree when, without their code, legal forms, punctuation and case, they
//! are equal or one's words begin the other's ("Toyota", "Toyota Motor",
//! "Toyota Motor Corporation"). Names in different scripts do not agree.

use std::collections::BTreeSet;

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::identity::{is_legal_form, market_family, name_words, segments, strip_legal_forms};

pub(crate) mod repair;

/// What kind of code a value is. Codes of different schemes never compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Scheme {
    Ticker,
    Isin,
    Figi,
    Lei,
    Cik,
    Dart,
    /// An identifier of no recognized form ("001-34756"). Equal values join
    /// two names; unequal ones say nothing, since two such values may be
    /// numbers from two different registries.
    Registry,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Code {
    pub scheme: Scheme,
    /// Upper case; an all-digit value without its leading zeros.
    pub value: String,
    /// The exchange family a ticker is listed on (`HKEX`, `JPX`, `US`), or an
    /// exchange suffix the families do not name (`ST`). None when unstated.
    pub market: Option<String>,
}

impl Code {
    fn new(scheme: Scheme, value: &str, market: Option<String>) -> Self {
        let value = value.trim().to_uppercase();
        let value = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            let trimmed = value.trim_start_matches('0');
            if trimmed.is_empty() { "0" } else { trimmed }.to_string()
        } else {
            value
        };
        Self {
            scheme,
            value,
            market,
        }
    }

    fn ticker(text: &str, market: Option<String>) -> Self {
        Self::new(Scheme::Ticker, text, market)
    }

    /// Whether an equal pair names one entity on its own: a ticker both sides
    /// write with its exchange, or a checked ISIN, FIGI or LEI, a CIK or a DART
    /// corp code. An unclassified identifier or a ticker without its exchange
    /// does not.
    fn identifies(&self, other: &Code) -> bool {
        match self.scheme {
            Scheme::Isin | Scheme::Figi | Scheme::Lei | Scheme::Cik | Scheme::Dart => true,
            Scheme::Ticker => self.market.is_some() && other.market.is_some(),
            Scheme::Registry => false,
        }
    }

    /// The same code: one scheme, one value, and no two different markets.
    fn agrees(&self, other: &Code) -> bool {
        self.scheme == other.scheme
            && self.value == other.value
            && match (&self.market, &other.market) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            }
    }

    /// A pair symbol and one of its sides (`KRW-BTC` and `BTC`, `USD/KRW` and
    /// `USD`) neither agree nor differ: one names the market of the other.
    fn pairs_with(&self, other: &Code) -> bool {
        let side_of = |pair: &str, side: &str| {
            pair != side && pair.split(['-', '/']).any(|part| part == side)
        };
        self.scheme == Scheme::Ticker
            && other.scheme == Scheme::Ticker
            && (side_of(&self.value, &other.value) || side_of(&other.value, &self.value))
    }
}

/// How two names' codes relate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation {
    /// They share a code: one entity.
    Same,
    /// Both state codes of one scheme and none of them agree: two entities.
    Different,
    /// Codes do not decide it.
    Unknown,
}

/// The codes one World name carries, from its spelling and, for a stored
/// node, its properties.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Codes {
    /// Codes the name or a property states outright.
    stated: Vec<Code>,
    /// A trailing all-capitals word after a written name ("Stellantis STLA",
    /// "Lucid Motors (LCID)"). It reads as a ticker unless the other name has
    /// that word in its own spelling ("Samsung SDI" beside "Samsung SDI
    /// 006400.KS").
    trailing: Vec<Code>,
    /// The name's own words outside its codes, folded.
    words: BTreeSet<String>,
    /// The name outside its parentheses without its code, legal forms,
    /// punctuation and case, word by word ([`super::identity::name_words`]).
    name: Vec<String>,
}

/// `text`'s words as identity keys them, without a leading "the" or legal forms.
fn name_key(text: &str) -> Vec<String> {
    let words = name_words(text);
    strip_legal_forms(&words)
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Core kinds whose names never carry a listing code: numbers in "Model 3",
/// "F-150" or "Q3 2026" name the thing itself.
const UNCODED_KINDS: &[&str] = &[
    "Person",
    "Product",
    "Place",
    "Policy",
    "Event",
    "Metric",
    "Technology",
    "Segment",
];

/// Kinds whose trailing all-capitals word is read as a ticker.
const TICKERED_KINDS: &[&str] = &["Organization", "Asset"];

/// Words before a trailing capital that make it a class or series, not a ticker.
const CLASS_WORDS: &[&str] = &["class", "series", "type", "tranche", "fund"];

/// Small words left out of an abbreviation's initials ("Securities and
/// Exchange Commission (SEC)").
const STOP_WORDS: &[&str] = &["and", "of", "the", "for", "de", "&"];

fn fold_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '&'))
        .filter(|word| !word.is_empty() && *word != "&")
        .map(super::world_extract::casefold)
}

fn upper_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '&' | '-'))
}

/// A roman numeral of the kind a fund or a series takes ("III", "XIV"). Only
/// I, V and X count, so "LI" stays Li Auto's ticker.
fn is_roman(token: &str) -> bool {
    token.len() >= 2 && token.chars().all(|c| matches!(c, 'I' | 'V' | 'X'))
}

fn year(digits: &str) -> bool {
    digits.len() == 4 && matches!(digits.parse::<u32>(), Ok(1900..=2099))
}

/// ISO 6166: two letters, nine alphanumerics and a Luhn check digit.
fn isin(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() != 12
        || !bytes[..2].iter().all(u8::is_ascii_uppercase)
        || !bytes[2..11]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || !bytes[11].is_ascii_digit()
    {
        return false;
    }
    let digits: String = token
        .chars()
        .map(|c| c.to_digit(36).expect("alphanumeric").to_string())
        .collect();
    let sum: u32 = digits
        .chars()
        .rev()
        .enumerate()
        .map(|(position, c)| {
            let digit = c.to_digit(10).expect("digit");
            if position % 2 == 1 {
                let doubled = digit * 2;
                doubled / 10 + doubled % 10
            } else {
                digit
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// A FIGI: two letters, `G`, eight alphanumerics and a check digit.
fn figi(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_uppercase)
        && bytes[2] == b'G'
        && bytes[3..11]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && bytes[11].is_ascii_digit()
}

/// ISO 17442: eighteen alphanumerics and two check digits, mod 97 equal to 1.
fn lei(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() != 20
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || !bytes[18..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let mut remainder = 0u64;
    for c in token.chars() {
        let value = u64::from(c.to_digit(36).expect("alphanumeric"));
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

/// `BASE.SUFFIX` with a one-to-four-letter exchange suffix (`0489.HK`,
/// `VOLCAR-B.ST`, `BRK.B`).
fn suffixed(token: &str) -> Option<Code> {
    let (base, suffix) = token.rsplit_once('.')?;
    let base_ok = !base.is_empty()
        && base.len() <= 12
        && upper_token(base)
        && base
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    let suffix_ok =
        (1..=4).contains(&suffix.len()) && suffix.chars().all(|c| c.is_ascii_uppercase());
    (base_ok && suffix_ok).then(|| {
        let market = market_family(suffix)
            .map(str::to_string)
            .unwrap_or_else(|| suffix.to_string());
        Code::ticker(base, Some(market))
    })
}

/// `EXCH:CODE` or `EXCH: CODE`, with the exchange's family when it has one.
fn listing(text: &str) -> Option<Code> {
    let (exchange, code) = text.trim().split_once(':')?;
    let (exchange, code) = (exchange.trim(), code.trim());
    let exchange_ok =
        (2..=10).contains(&exchange.len()) && exchange.chars().all(|c| c.is_ascii_alphabetic());
    let code_ok = (1..=12).contains(&code.len())
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '&'));
    if !(exchange_ok && code_ok) {
        return None;
    }
    let code = code.to_uppercase();
    if let Some(mut found) = suffixed(&code) {
        found.market = found
            .market
            .or_else(|| market_family(exchange).map(str::to_string));
        return Some(found);
    }
    Some(Code::ticker(
        &code,
        market_family(exchange).map(str::to_string),
    ))
}

/// A code a name states outright: a listing form, an ISIN, a FIGI, an LEI,
/// an exchange-suffixed ticker, or four to six digits that are not a year.
fn stated_code(token: &str) -> Option<Code> {
    let token = token.trim_matches(|c: char| matches!(c, ',' | ';' | '\u{3001}'));
    if token.contains(':') {
        return listing(token);
    }
    if isin(token) {
        return Some(Code::new(Scheme::Isin, token, None));
    }
    if figi(token) {
        return Some(Code::new(Scheme::Figi, token, None));
    }
    if lei(token) {
        return Some(Code::new(Scheme::Lei, token, None));
    }
    if token.contains('.') {
        return suffixed(token);
    }
    let digits = (4..=6).contains(&token.len())
        && token.bytes().all(|byte| byte.is_ascii_digit())
        && !year(token);
    digits.then(|| Code::ticker(token, None))
}

/// The initials of `words`, with and without their small words.
fn initials(words: &[&str]) -> [String; 2] {
    let first = |word: &&str| word.chars().next().map(|c| c.to_uppercase().to_string());
    [
        words.iter().filter_map(first).collect(),
        words
            .iter()
            .filter(|word| !STOP_WORDS.contains(&word.to_lowercase().as_str()))
            .filter_map(first)
            .collect(),
    ]
}

/// A capital word after a written name that reads as a ticker: one to six
/// capitals or digits with a letter, after a word with a lower-case letter,
/// and not a legal form, a roman numeral, a share class or the initials of
/// the name before it.
fn trailing_code(token: &str, before: &[&str]) -> Option<Code> {
    let written = before
        .iter()
        .any(|word| word.chars().any(char::is_lowercase));
    let shaped = (1..=6).contains(&token.len())
        && upper_token(token)
        && token.chars().any(|c| c.is_ascii_uppercase());
    if !written || !shaped || is_legal_form(&token.to_lowercase()) || is_roman(token) {
        return None;
    }
    if before
        .last()
        .is_some_and(|word| CLASS_WORDS.contains(&word.to_lowercase().as_str()))
    {
        return None;
    }
    if token.len() >= 2 && initials(before).iter().any(|initials| initials == token) {
        return None;
    }
    Some(Code::ticker(token, None))
}

impl Codes {
    /// The codes `name` carries for a node of `kind`.
    pub(crate) fn of_name(name: &str, kind: &str) -> Self {
        let mut codes = Codes::default();
        let text: String = name.nfkc().collect();
        let parts = segments(&text);
        let (outer, inner) = parts.split_first().expect("the outer segment");
        if UNCODED_KINDS.contains(&kind) {
            codes.words = fold_words(&text).collect();
            codes.name = name_key(outer);
            return codes;
        }
        let tickered = TICKERED_KINDS.contains(&kind);
        let tokens: Vec<&str> = outer.split_whitespace().collect();
        let mut kept: Vec<&str> = tokens.clone();
        if let Some((last, before)) = tokens.split_last().filter(|(_, before)| !before.is_empty()) {
            if let Some(code) = stated_code(last) {
                codes.stated.push(code);
                kept.pop();
            } else if let Some(code) = tickered.then(|| trailing_code(last, before)).flatten() {
                codes.trailing.push(code);
                kept.pop();
            }
        }
        let outer_words: Vec<&str> = kept.clone();
        for segment in inner {
            let segment = segment.trim();
            if let Some(code) = stated_code(segment) {
                codes.stated.push(code);
                continue;
            }
            let own_word = outer_words.iter().any(|word| word == &segment);
            if tickered && !own_word && !segment.contains(' ') {
                if let Some(code) = trailing_code(segment, &outer_words) {
                    codes.trailing.push(code);
                    continue;
                }
            }
            codes.words.extend(fold_words(segment));
        }
        codes
            .words
            .extend(kept.iter().flat_map(|word| fold_words(word)));
        codes.name = name_key(&kept.join(" "));
        codes
    }

    /// The codes of a stored node: its name's, and those its properties state
    /// (`ticker` with its `exchange`, `symbol`, `isin`, `figi`, `lei`, `cik`,
    /// `corp_code`, `identifier`).
    pub(crate) fn of_node<'a>(
        name: &str,
        kind: &str,
        properties: impl IntoIterator<Item = (&'a str, &'a Value)>,
    ) -> Self {
        let mut codes = Self::of_name(name, kind);
        let properties: Vec<(String, &Value)> = properties
            .into_iter()
            .map(|(key, value)| (key.to_lowercase(), value))
            .collect();
        let texts = |keys: &[&str]| -> Vec<String> {
            let mut out = Vec::new();
            for (key, value) in &properties {
                if !keys.contains(&key.as_str()) {
                    continue;
                }
                match value {
                    Value::String(text) => out.push(text.trim().to_string()),
                    Value::Number(number) => out.push(number.to_string()),
                    Value::Array(items) => out.extend(
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|text| text.trim().to_string()),
                    ),
                    _ => {}
                }
            }
            out.retain(|text| !text.is_empty() && text.chars().count() <= 64);
            out
        };
        let exchange = texts(&["exchange", "exchanges", "market"])
            .iter()
            .find_map(|exchange| market_family(exchange))
            .map(str::to_string);
        for ticker in texts(&["ticker", "tickers", "symbol", "symbols", "ticker_symbol"]) {
            let ticker = ticker.trim_start_matches('$').to_uppercase();
            if ticker.contains(char::is_whitespace) {
                continue;
            }
            let code = if ticker.contains(':') {
                listing(&ticker)
            } else {
                suffixed(&ticker)
            };
            let mut code = code.unwrap_or_else(|| Code::ticker(&ticker, None));
            code.market = code.market.or_else(|| exchange.clone());
            codes.stated.push(code);
        }
        for (keys, scheme) in [
            (&["isin"][..], Scheme::Isin),
            (&["figi"][..], Scheme::Figi),
            (&["lei"][..], Scheme::Lei),
            (&["cik"][..], Scheme::Cik),
            (
                &["corp_code", "dart_corp_code", "dartcorpcode"][..],
                Scheme::Dart,
            ),
        ] {
            for value in texts(keys) {
                codes.stated.push(Code::new(scheme, &value, None));
            }
        }
        for value in texts(&["identifier", "identifiers"]) {
            let token = value.to_uppercase();
            let scheme = if isin(&token) {
                Scheme::Isin
            } else if figi(&token) {
                Scheme::Figi
            } else if lei(&token) {
                Scheme::Lei
            } else {
                Scheme::Registry
            };
            let normalized: String = token.chars().filter(char::is_ascii_alphanumeric).collect();
            if !normalized.is_empty() {
                codes.stated.push(Code::new(scheme, &normalized, None));
            }
        }
        codes.stated.sort();
        codes.stated.dedup();
        codes
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.stated.is_empty() && self.trailing.is_empty()
    }

    /// Every value this name could match another name's code by.
    pub(crate) fn values(&self) -> impl Iterator<Item = &str> {
        self.stated
            .iter()
            .chain(&self.trailing)
            .map(|code| code.value.as_str())
    }

    /// Its codes as read beside `other`: a trailing capital `other` spells as
    /// one of its own words is a word here too.
    fn beside<'a>(&'a self, other: &Codes) -> Vec<&'a Code> {
        self.stated
            .iter()
            .chain(
                self.trailing
                    .iter()
                    .filter(|code| !other.words.contains(&code.value.to_lowercase())),
            )
            .collect()
    }

    /// The pairs of its codes and `other`'s that are one code.
    fn agreeing<'a>(&'a self, other: &'a Codes) -> Vec<(&'a Code, &'a Code)> {
        let theirs = other.beside(self);
        self.beside(other)
            .into_iter()
            .flat_map(|left| {
                theirs
                    .iter()
                    .filter(move |right| left.agrees(right))
                    .map(move |right| (left, *right))
            })
            .collect()
    }

    pub(crate) fn relation(&self, other: &Codes) -> Relation {
        if !self.agreeing(other).is_empty() {
            return Relation::Same;
        }
        let mine = self.beside(other);
        let theirs = other.beside(self);
        let differs = mine.iter().any(|left| {
            left.scheme != Scheme::Registry
                && theirs
                    .iter()
                    .any(|right| right.scheme == left.scheme && !left.pairs_with(right))
        });
        if differs {
            Relation::Different
        } else {
            Relation::Unknown
        }
    }

    pub(crate) fn differs_from(&self, other: &Codes) -> bool {
        self.relation(other) == Relation::Different
    }

    /// Whether the two names are equal, or one's words begin the other's,
    /// once codes, legal forms, punctuation and case are set aside.
    pub(crate) fn names_agree(&self, other: &Codes) -> bool {
        let (short, long) = if self.name.len() <= other.name.len() {
            (&self.name, &other.name)
        } else {
            (&other.name, &self.name)
        };
        !short.is_empty() && long.starts_with(short)
    }

    /// Whether the host refuses to treat the two as one entity: their codes
    /// differ and their names do not agree.
    pub(crate) fn refuses(&self, other: &Codes) -> bool {
        self.differs_from(other) && !self.names_agree(other)
    }

    /// Whether the host joins the two without the model: a code is equal, and
    /// it identifies the entity on its own or the names agree.
    pub(crate) fn joins(&self, other: &Codes) -> bool {
        let pairs = self.agreeing(other);
        !pairs.is_empty()
            && (pairs.iter().any(|(left, right)| left.identifies(right)) || self.names_agree(other))
    }
}

/// Whether a node of `left` kind and one of `right` kind may be one entity
/// by their codes: the same kind, or one of them untyped.
pub(crate) fn kinds_join(left: &str, right: &str) -> bool {
    left == right || left == "Entity" || right == "Entity"
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn org(name: &str) -> Codes {
        Codes::of_name(name, "Organization")
    }

    fn node(name: &str, properties: Value) -> Codes {
        let properties = properties.as_object().cloned().unwrap_or_default();
        Codes::of_node(
            name,
            "Organization",
            properties.iter().map(|(key, value)| (key.as_str(), value)),
        )
    }

    #[test]
    fn names_state_tickers_with_or_without_their_exchange() {
        for (name, value, market) in [
            ("Dongfeng Motor 0489.HK", "489", Some("HKEX")),
            ("Subaru 7270.T", "7270", Some("JPX")),
            ("Seres Group 601127.SS", "601127", Some("SSE")),
            ("Volvo Car VOLCAR-B.ST", "VOLCAR-B", Some("ST")),
            ("Mahindra & Mahindra M&M.NS", "M&M", Some("NSE")),
            ("Yulon Motor Company 2201", "2201", None),
            ("Apple Inc. (NASDAQ:AAPL)", "AAPL", Some("US")),
            ("Samsung Electronics (KRX: 005930)", "5930", Some("KRX")),
            ("Samsung Electronics Co., Ltd. (005930)", "5930", None),
        ] {
            assert_eq!(
                org(name).stated,
                vec![Code {
                    scheme: Scheme::Ticker,
                    value: value.into(),
                    market: market.map(str::to_string),
                }],
                "{name}"
            );
        }
        for (name, value) in [
            ("Stellantis STLA", "STLA"),
            ("Ford F", "F"),
            ("Lucid Motors (LCID)", "LCID"),
            ("Honda HMC", "HMC"),
            ("Li Auto LI", "LI"),
        ] {
            assert_eq!(org(name).trailing[0].value, value, "{name}");
        }
    }

    #[test]
    fn words_that_only_look_like_codes_are_words() {
        for name in [
            "General Motors GM",
            "Jiangling Motors Corporation (JMC)",
            "Securities and Exchange Commission (SEC)",
            "Volkswagen AG",
            "Acme Partners III",
            "Alphabet Class A",
            "NIO",
            "NIO (NIO)",
            "SK IET",
            "TSLA",
            "GAC (Guangzhou Automobile Group) 601238.SS",
            "Barclays 2026",
        ] {
            assert!(org(name).trailing.is_empty(), "{name}: {:?}", org(name));
        }
        assert!(Codes::of_name("Model 3", "Product").is_empty());
        assert!(Codes::of_name("Tesla Q3 2026 earnings 2026", "Event").is_empty());
        assert!(Codes::of_name("Barclays 2026", "Organization").is_empty());
    }

    #[test]
    fn different_codes_keep_names_apart_and_equal_codes_join_them() {
        let dongfeng = node("Dongfeng Motor 0489.HK", json!({"ticker": "0489.HK"}));
        assert_eq!(
            org("Stellantis STLA").relation(&dongfeng),
            Relation::Different
        );
        assert_eq!(
            org("Dongfeng Motor 0489.HK").relation(&dongfeng),
            Relation::Same
        );
        assert_eq!(
            org("Dongfeng Motor (HKEX:489)").relation(&dongfeng),
            Relation::Same
        );
        assert_eq!(org("Dongfeng Motor").relation(&dongfeng), Relation::Unknown);
        let ford = node(
            "Ford Motor Company",
            json!({"ticker": "F", "identifier": "0000037996"}),
        );
        assert_eq!(org("Ford F").relation(&ford), Relation::Same);
        assert_eq!(
            org("Ford Otosan FROTO.IS").relation(&ford),
            Relation::Different
        );
        // A trailing capital the other name spells as its own word is that word.
        let sdi = node("Samsung SDI 006400.KS", json!({}));
        assert_eq!(org("Samsung SDI").relation(&sdi), Relation::Unknown);
        assert_eq!(
            org("Samsung SDI").relation(&node("Samsung Electronics 005930.KS", json!({}))),
            Relation::Different
        );
        // One ticker on two exchanges is two listings.
        let korea = node("Alpha Korea", json!({"ticker": "A", "exchange": "KRX"}));
        let us = node("Alpha Inc", json!({"ticker": "A", "exchange": "NASDAQ"}));
        assert_eq!(korea.relation(&us), Relation::Different);
        assert_eq!(org("Alpha A").relation(&us), Relation::Same);
        // A pair symbol and its side, or two registries' numbers, decide nothing.
        let bitcoin = Codes::of_node("Bitcoin", "Asset", [("symbol", &json!("BTC"))]);
        let pair = Codes::of_node("Bitcoin", "Asset", [("symbol", &json!("KRW-BTC"))]);
        assert_eq!(bitcoin.relation(&pair), Relation::Unknown);
        let tesla = node(
            "Tesla, Inc.",
            json!({"ticker": "TSLA", "identifier": "001-34756"}),
        );
        let filer = node("Tesla", json!({"identifier": "0001318605"}));
        assert_eq!(tesla.relation(&filer), Relation::Unknown);
        assert_eq!(org("Tesla TSLA").relation(&tesla), Relation::Same);
    }

    #[test]
    fn the_host_refuses_and_joins_only_by_the_decided_rules() {
        for (left, right) in [
            ("Toyota TM", "Toyota Motor 7203.T"),
            ("Toyota", "Toyota Motor Corporation"),
            ("Alphabet GOOG", "Alphabet Inc. GOOGL"),
            ("Tesla TSLA", "Tesla Inc TSLA"),
            ("Apple Inc. (NASDAQ:AAPL)", "Apple"),
        ] {
            assert!(org(left).names_agree(&org(right)), "{left} / {right}");
        }
        for (left, right) in [
            ("Stellantis STLA", "Dongfeng Motor 0489.HK"),
            ("Motor Toyota", "Toyota Motor"),
            ("삼성전자 005930.KS", "Samsung Electronics 005930.KS"),
            ("Google AI", "Meta AI"),
        ] {
            assert!(!org(left).names_agree(&org(right)), "{left} / {right}");
        }
        // Refused: the codes differ and the names do not agree.
        let dongfeng = node("Dongfeng Motor 0489.HK", json!({"ticker": "0489.HK"}));
        assert!(org("Stellantis STLA").refuses(&dongfeng));
        assert!(org("Seres Group 601127.SS").refuses(&org("Subaru 7270.T")));
        // Listings of one company under agreeing names stay the model's.
        assert!(!org("Toyota TM").refuses(&org("Toyota 7203.T")));
        assert!(!org("Alphabet GOOG").refuses(&org("Alphabet GOOGL")));
        assert!(!org("Toyota TM").joins(&org("Toyota 7203.T")));
        // Joined: an equal code that identifies on its own, or agreeing names.
        assert!(org("Tesla TSLA").joins(&org("Tesla Inc TSLA")));
        assert!(org("Subaru 7270.T").joins(&org("Subaru 7270.T")));
        assert!(org("삼성전자 005930.KS").joins(&org("Samsung Electronics (KRX:005930)")));
        assert!(org("Yulon Motor Company 2201").joins(&org("Yulon Motor 2201.TW")));
        assert!(!org("Morinaga 2201").joins(&org("Yulon Motor 2201.TW")));
        // An equal bare trailing capital under names that do not agree decides
        // nothing either way.
        for (left, right) in [
            ("Google AI", "Meta AI"),
            ("Apple TV", "Samsung TV"),
            ("Samsung SDI", "Hanwha SDI"),
        ] {
            assert!(!org(left).joins(&org(right)), "{left} / {right}");
            assert!(!org(left).refuses(&org(right)), "{left} / {right}");
        }
    }

    #[test]
    fn registry_forms_are_checked_before_they_count() {
        assert!(isin("US0378331005"));
        assert!(!isin("US0378331006"));
        assert!(figi("BBG000B9XRY4"));
        assert!(lei("HWUPKR0MPOU8FGXBT394"));
        assert!(!lei("HWUPKR0MPOU8FGXBT395"));
        let apple = node("Apple", json!({"identifier": "US0378331005"}));
        assert_eq!(org("Apple (US0378331005)").relation(&apple), Relation::Same);
        assert_eq!(
            org("Apple (US5949181045)").relation(&apple),
            Relation::Different
        );
    }
}
