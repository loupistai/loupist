use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AssetLookupCandidate, LookupStatus};

#[derive(Serialize)]
pub(super) struct Search<'a> {
    pub(super) query: &'a str,
    #[serde(rename = "marketSecDes", skip_serializing_if = "Option::is_none")]
    market_sector: Option<&'static str>,
}

impl<'a> Search<'a> {
    pub(super) fn new(query: &'a str) -> Result<Self, LookupStatus> {
        let query = query.trim();
        if query.is_empty()
            || query.chars().count() > 200
            || query.chars().any(char::is_control)
            || query.split_whitespace().count() > 12
        {
            return Err(LookupStatus::InvalidQuery);
        }
        Ok(Self {
            query,
            market_sector: None,
        })
    }

    /// The same search limited to OpenFIGI's equity market sector.
    pub(super) fn equities(&self) -> Self {
        Self {
            query: self.query,
            market_sector: Some("Equity"),
        }
    }
}

#[derive(Deserialize)]
struct ResultEnvelope {
    data: Option<Vec<Value>>,
    error: Option<String>,
    warning: Option<String>,
    /// Set when OpenFIGI has more results after this page.
    next: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    figi: String,
    ticker: Option<String>,
    name: Option<String>,
    exch_code: Option<String>,
    #[serde(alias = "exchName")]
    exchange_name: Option<String>,
    #[serde(rename = "compositeFIGI")]
    composite_figi: Option<String>,
    security_type: Option<String>,
    security_type2: Option<String>,
    market_sector: Option<String>,
}

fn bounded(value: Option<String>, limit: usize) -> Option<String> {
    value.filter(|value| {
        !value.trim().is_empty()
            && value.chars().count() <= limit
            && !value.chars().any(|c| {
                c.is_control()
                    || matches!(c, '<' | '>' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
    })
}

fn valid_figi(figi: &str) -> bool {
    figi.len() == 12
        && figi
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// The issuer's own shares: common stock and depositary receipts.
fn issuer_share(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "common stock"
            | "depositary receipt"
            | "depositary receipts"
            | "adr"
            | "gdr"
            | "canadian dr"
    )
}

fn listed_equity(kind: &str) -> bool {
    issuer_share(kind)
        || matches!(
            kind.to_ascii_lowercase().as_str(),
            "etf" | "exchange traded fund" | "exchange traded funds"
        )
}

/// A venue of a country: an exchange or a country composite. Trade-reporting
/// and OTC lines (European APAs, `EO`, `EU`) have none in OpenFIGI's list.
fn national_venue(code: Option<&str>) -> bool {
    code.and_then(super::venues).is_some_and(|venues| {
        !venues.is_empty() && venues.iter().all(|(_, country)| country.is_some())
    })
}

/// Whether a full first page shows the issuer's shares only on trade-reporting
/// lines. OpenFIGI fills a large issuer's first page with its bonds and those
/// lines: "Samsung Electronics" returned no listing of 005930 on any exchange
/// on 2026-11-24, while the same search in the equity sector has it.
pub(super) fn wants_equity_page(bytes: &[u8]) -> bool {
    let Ok(result) = serde_json::from_slice::<ResultEnvelope>(bytes) else {
        return false;
    };
    if result.next.is_none() || result.error.is_some() || result.warning.is_some() {
        return false;
    }
    let mut shares = false;
    for value in result.data.iter().flatten() {
        let Ok(row) = Row::deserialize(value) else {
            continue;
        };
        if [row.security_type.as_deref(), row.security_type2.as_deref()]
            .into_iter()
            .flatten()
            .any(issuer_share)
        {
            if national_venue(row.exch_code.as_deref()) {
                return false;
            }
            shares = true;
        }
    }
    shares
}

struct Listing {
    candidate: AssetLookupCandidate,
    equity: bool,
    exact: bool,
    composite: bool,
    /// One of its venues belongs to a country.
    national: bool,
    venues: usize,
}

fn page_rows(bytes: &[u8]) -> Result<Vec<Value>, LookupStatus> {
    let result: ResultEnvelope =
        serde_json::from_slice(bytes).map_err(|_| LookupStatus::Unavailable)?;
    if let Some(message) = result.error.as_deref().or(result.warning.as_deref()) {
        return if message
            .trim_end_matches('.')
            .eq_ignore_ascii_case("no identifier found")
        {
            Ok(Vec::new())
        } else {
            Err(LookupStatus::Unavailable)
        };
    }
    result.data.ok_or(LookupStatus::Unavailable)
}

#[cfg(test)]
fn decode(query: &str, bytes: &[u8]) -> Result<Vec<AssetLookupCandidate>, LookupStatus> {
    decode_pages(query, &[bytes])
}

/// Candidates from the first page and any later page of the same search. A
/// later page that cannot be read leaves the first page's answer.
pub(super) fn decode_pages(
    query: &str,
    pages: &[&[u8]],
) -> Result<Vec<AssetLookupCandidate>, LookupStatus> {
    let mut data = Vec::new();
    for (index, bytes) in pages.iter().enumerate() {
        match page_rows(bytes) {
            Ok(rows) => data.extend(rows),
            Err(status) if index == 0 => return Err(status),
            Err(_) => {}
        }
    }
    let mut seen = BTreeSet::new();
    let mut groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut listings: Vec<Listing> = Vec::new();
    for value in data {
        let Ok(row) = serde_json::from_value::<Row>(value) else {
            continue;
        };
        if !valid_figi(&row.figi) {
            continue;
        }
        let ticker = bounded(row.ticker, 64);
        let Some(name) = bounded(row.name, 256).or_else(|| ticker.clone()) else {
            continue;
        };
        if !seen.insert(row.figi.clone()) {
            continue;
        }

        let equity = [row.security_type.as_deref(), row.security_type2.as_deref()]
            .into_iter()
            .flatten()
            .any(listed_equity)
            || (row.security_type.as_deref() == Some("ETP")
                && row.security_type2.as_deref() == Some("Mutual Fund")
                && row.market_sector.as_deref() == Some("Equity"));
        let group = row.composite_figi.filter(|id| equity && valid_figi(id));
        let national = national_venue(row.exch_code.as_deref());
        let exchange = bounded(row.exch_code, 64);
        let exchange_name = bounded(row.exchange_name, 128)
            .or_else(|| super::exchange_name(exchange.as_deref()?).map(str::to_owned));
        let listing = Listing {
            exact: ticker
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(query)),
            composite: group.as_ref() == Some(&row.figi),
            equity,
            national,
            venues: 1,
            candidate: AssetLookupCandidate {
                figi: row.figi,
                name,
                ticker,
                exchange,
                exchange_name,
                security_type: bounded(row.security_type, 64),
                market_sector: bounded(row.market_sector, 64),
            },
        };
        // A composite identifies one security within a market. A shared ticker
        // or share class alone can also identify different listings.
        if let Some(group) = group {
            let index = *groups.entry(group).or_insert(listings.len());
            if let Some(previous) = listings.get_mut(index) {
                let venues = previous.venues + 1;
                let national = previous.national || listing.national;
                if (listing.exact, listing.composite) > (previous.exact, previous.composite) {
                    *previous = listing;
                }
                previous.venues = venues;
                previous.national = national;
                continue;
            }
        }
        listings.push(listing);
    }
    // Preserve provider order on ties. Between composites, one traded on an
    // exchange or country composite precedes trade-reporting lines; then the
    // broader set of returned venues breaks the tie.
    let query = query.to_lowercase();
    listings.sort_by_cached_key(|listing| {
        let name = listing.candidate.name.to_lowercase();
        (
            !listing.exact,
            !listing.equity,
            !listing.exact && !name.starts_with(&query),
            !listing.exact && !name.contains(&query),
            !listing.composite,
            !listing.national,
            std::cmp::Reverse(listing.venues),
        )
    });
    Ok(listings
        .into_iter()
        .take(100)
        .map(|listing| listing.candidate)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_costco_and_deere_listings_without_losing_debt_or_distinct_issuers() {
        // Selected records in the order OpenFIGI returned for each search.
        let searches: Value = serde_json::from_str(include_str!("search-fixture.json")).unwrap();
        for (query, first, count, equities) in [
            ("Costco", "BBG000F6H8W8", 6, 3),
            ("Deere", "BBG00J3QH561", 5, 2),
            ("COST", "BBG000F6H8W8", 4, 4),
            ("DE", "BBG000BH1NH9", 7, 6),
        ] {
            let bytes = serde_json::to_vec(&searches[query]).unwrap();
            let rows = decode(query, &bytes).unwrap();
            assert_eq!(rows[0].figi, first, "{query}");
            assert_eq!(rows.len(), count, "{query}");
            assert!(
                rows[..equities].iter().all(|row| matches!(
                    row.security_type.as_deref(),
                    Some("Common Stock" | "Canadian DR" | "ETP" | "CEDEAR")
                )),
                "{query}"
            );
            assert!(rows[equities..]
                .iter()
                .all(|row| row.security_type.as_deref() != Some("Common Stock")));
            if query == "DE" {
                assert!(rows[..5]
                    .iter()
                    .all(|row| row.ticker.as_deref() == Some("DE")));
                assert!(rows.iter().any(|row| row.name == "DECISIVE DIVIDEND CORP"));
            }
        }
    }

    /// Recorded 2026-11-24 from OpenFIGI's public route: selected rows, in
    /// OpenFIGI's order, of the first page for "Samsung Electronics", the same
    /// search in the equity sector, and "005930".
    #[test]
    fn an_issuer_shown_only_on_trade_reporting_lines_resolves_to_its_home_listing() {
        let searches: Value = serde_json::from_str(include_str!("search-fixture.json")).unwrap();
        let page = |name: &str| serde_json::to_vec(&searches[name]).unwrap();
        let (first, equities, ticker) = (
            page("Samsung Electronics"),
            page("Samsung Electronics, equity sector"),
            page("005930"),
        );
        let query = "Samsung Electronics";
        // The first page has bonds, preference shares, Korean funds and the
        // shares on European reporting lines, but no listing of 005930.
        assert!(decode(query, &first)
            .unwrap()
            .iter()
            .all(|row| row.ticker.as_deref() != Some("005930")));
        assert!(wants_equity_page(&first));
        for settled in [&equities, &ticker, &page("Costco")] {
            assert!(!wants_equity_page(settled));
        }
        let rows = decode_pages(query, &[&first, &equities]).unwrap();
        assert_eq!(
            (
                rows[0].figi.as_str(),
                rows[0].ticker.as_deref(),
                rows[0].exchange.as_deref()
            ),
            ("BBG000BCY2S8", Some("005930"), Some("KS"))
        );
        assert_eq!(rows[0].exchange_name.as_deref(), Some("South Korea"));
        // KOSPI and Nextrade share its composite; EuroCTP has its own.
        assert_eq!(
            rows.iter()
                .filter(|row| row.ticker.as_deref() == Some("005930"))
                .count(),
            2
        );
        // The ticker still finds the same listing first from one page.
        assert_eq!(decode("005930", &ticker).unwrap()[0].figi, "BBG000BCY2S8");
        // A later page that cannot be read leaves the first page's answer.
        assert_eq!(
            decode_pages(query, &[&first, b"{"]).unwrap().len(),
            decode(query, &first).unwrap().len()
        );
        assert_eq!(
            serde_json::to_value(Search::new(query).unwrap().equities()).unwrap(),
            serde_json::json!({"query": query, "marketSecDes": "Equity"})
        );
        assert_eq!(
            serde_json::to_value(Search::new(query).unwrap()).unwrap(),
            serde_json::json!({"query": query})
        );
    }

    #[test]
    fn listing_projection_keeps_reference_fields_and_drops_external_prose() {
        let rows = decode(
            "NVDA",
            br#"{"data":[
            {"figi":"BBG000BBJQV0","name":"NVIDIA CORP","ticker":"NVDA","exchCode":"US",
             "securityType":"Common Stock","marketSector":"Equity","description":"unrelated text"},
            {"figi":"BBG000BBJQV0","name":"duplicate"},
            {"figi":"no","name":"invalid"},
            {"figi":"BBG000000008","name":"Named venue listing","exchCode":"PW","exchangeName":"Example Exchange"}
        ]}"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].exchange_name.as_deref(), Some("United States"));
        assert_eq!(rows[1].exchange_name.as_deref(), Some("Example Exchange"));
        assert_eq!(rows[0].security_type.as_deref(), Some("Common Stock"));
        assert_eq!(rows[0].exchange.as_deref(), Some("US"));
        assert_eq!(rows[0].name, "NVIDIA CORP");
        assert_eq!(
            Search::new("missing\nquery").err(),
            Some(LookupStatus::InvalidQuery)
        );
        assert!(decode("unknown", br#"{"warning":"No identifier found."}"#)
            .unwrap()
            .is_empty());
    }
}

#[cfg(test)]
mod venue_tests {
    use super::*;

    #[test]
    fn search_uses_published_venue_names_for_unambiguous_codes() {
        for (code, name) in [
            ("EO", Some("OTC Composite")),
            ("PW", Some("Warsaw SE")),
            ("EZ", Some("European Lit Composite")),
            ("X9", Some("Tradeweb APA EU")),
            ("TT", None),
            ("UNKNOWN", None),
        ] {
            let response = serde_json::json!({"data":[{"figi":"BBG000BBJQV0", "name":"Apple", "exchCode":code}]});
            let result = decode("Apple", response.to_string().as_bytes()).unwrap();
            assert_eq!(result[0].exchange_name.as_deref(), name, "{code}");
        }
    }
}
