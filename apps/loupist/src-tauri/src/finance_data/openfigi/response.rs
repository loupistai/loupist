use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AssetLookupCandidate, LookupStatus};

#[derive(Serialize)]
pub(super) struct Search<'a> {
    pub(super) query: &'a str,
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
        Ok(Self { query })
    }
}

#[derive(Deserialize)]
struct ResultEnvelope {
    data: Option<Vec<Value>>,
    error: Option<String>,
    warning: Option<String>,
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

fn listed_equity(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "common stock"
            | "depositary receipt"
            | "depositary receipts"
            | "adr"
            | "gdr"
            | "canadian dr"
            | "etf"
            | "exchange traded fund"
            | "exchange traded funds"
    )
}

struct Listing {
    candidate: AssetLookupCandidate,
    equity: bool,
    exact: bool,
    composite: bool,
    venues: usize,
}

pub(super) fn decode(query: &str, bytes: &[u8]) -> Result<Vec<AssetLookupCandidate>, LookupStatus> {
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
    let data = result.data.ok_or(LookupStatus::Unavailable)?;
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
        let exchange = bounded(row.exch_code, 64);
        let exchange_name = bounded(row.exchange_name, 128)
            .or_else(|| super::exchange_name(exchange.as_deref()?).map(str::to_owned));
        let listing = Listing {
            exact: ticker
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(query)),
            composite: group.as_ref() == Some(&row.figi),
            equity,
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
                if (listing.exact, listing.composite) > (previous.exact, previous.composite) {
                    *previous = listing;
                }
                previous.venues = venues;
                continue;
            }
        }
        listings.push(listing);
    }
    // Preserve provider order on ties. For competing listings with the same
    // ticker, the broader set of returned venues breaks a composite tie.
    let query = query.to_lowercase();
    listings.sort_by_cached_key(|listing| {
        let name = listing.candidate.name.to_lowercase();
        (
            !listing.exact,
            !listing.equity,
            !listing.exact && !name.starts_with(&query),
            !listing.exact && !name.contains(&query),
            !listing.composite,
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
