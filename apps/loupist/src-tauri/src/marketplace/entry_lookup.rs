//! Resolves what the model calls a connection (`fred.macro`, `FRED`, `KRX`,
//! `한국투자증권`) to one bundled catalog entry, deterministically.
use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::catalog::{bundled_marketplace, MarketplaceEntryDto};

/// The most candidates a failed lookup names.
pub(crate) const MAX_CANDIDATES: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SetupCandidate {
    pub id: String,
    pub name: String,
}

/// Display names the interface shows for a catalog entry: the catalog's
/// English name plus the Korean copy the renderer keeps for it.
fn display_names() -> &'static BTreeMap<String, Vec<String>> {
    static NAMES: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let copy = |text: &str| -> BTreeMap<String, String> {
            serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|value| value.get("catalog").cloned())
                .and_then(|catalog| serde_json::from_value(catalog).ok())
                .unwrap_or_default()
        };
        let english = copy(include_str!(
            "../../../src/i18n/locales/en/connections.json"
        ));
        let korean = copy(include_str!(
            "../../../src/i18n/locales/ko/connections.json"
        ));
        let Ok(market) = bundled_marketplace() else {
            return BTreeMap::new();
        };
        market
            .catalog
            .entries
            .iter()
            .map(|entry| {
                let mut names = vec![entry.name.clone()];
                let translated = english
                    .iter()
                    .find(|(_, value)| *value == &entry.name)
                    .and_then(|(key, _)| korean.get(key));
                if let Some(translated) = translated {
                    names.push(translated.clone());
                }
                (entry.id.clone(), names)
            })
            .collect()
    })
}

fn candidate(entry: &MarketplaceEntryDto) -> SetupCandidate {
    SetupCandidate {
        id: entry.id.clone(),
        name: entry.name.clone(),
    }
}

/// An exact entry id first, then a case-insensitive display name, then a
/// plugin name that owns exactly one entry. Anything else fails with the
/// candidates the model can name instead: the matches when several entries
/// fit, otherwise the start of the catalog.
pub(crate) fn resolve_setup_entry(
    query: &str,
) -> Result<&'static MarketplaceEntryDto, Vec<SetupCandidate>> {
    let entries = &bundled_marketplace()
        .map_err(|_| Vec::new())?
        .catalog
        .entries;
    let query = query.trim();
    if let Some(entry) = entries.iter().find(|entry| entry.id == query) {
        return Ok(entry);
    }
    let folded = query.to_lowercase();
    let names = display_names();
    let by_name = entries
        .iter()
        .filter(|entry| {
            names
                .get(&entry.id)
                .is_some_and(|names| names.iter().any(|name| name.to_lowercase() == folded))
        })
        .collect::<Vec<_>>();
    if let [entry] = by_name.as_slice() {
        return Ok(entry);
    }
    let by_plugin = entries
        .iter()
        .filter(|entry| entry.plugin == folded)
        .collect::<Vec<_>>();
    if let [entry] = by_plugin.as_slice() {
        return Ok(entry);
    }
    let matches = if !by_name.is_empty() {
        by_name
    } else {
        by_plugin
    };
    let listed = if matches.is_empty() {
        entries.iter().collect::<Vec<_>>()
    } else {
        matches
    };
    Err(listed
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(candidate)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_names_in_either_language_and_single_entry_plugins_resolve() {
        for (query, expected) in [
            ("fred.macro", "fred.macro"),
            ("FRED", "fred.macro"),
            ("fred", "fred.macro"),
            (" sec edgar ", "sec.edgar"),
            ("OpenDART", "opendart.disclosures"),
            ("krx", "krx.market-data"),
            ("world-bank", "world-bank.indicators"),
            ("시장 데이터 플랫폼", "openbb.platform"),
            ("웹 리서치", "community.web-research"),
        ] {
            assert_eq!(resolve_setup_entry(query).unwrap().id, expected, "{query}");
        }
    }

    #[test]
    fn a_plugin_with_many_entries_lists_them_and_an_unknown_name_lists_the_catalog() {
        let ambiguous = resolve_setup_entry("openbb").unwrap_err();
        assert!(ambiguous.len() > 1 && ambiguous.len() <= MAX_CANDIDATES);
        assert!(ambiguous
            .iter()
            .any(|candidate| candidate.id == "fred.macro"));
        assert!(ambiguous.iter().all(|candidate| !candidate.name.is_empty()));
        let unknown = resolve_setup_entry("Bloomberg").unwrap_err();
        assert_eq!(unknown.len(), MAX_CANDIDATES);
    }
}
