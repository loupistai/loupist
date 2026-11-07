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

/// Lowercase with `-`, `_`, `.` and runs of whitespace folded to one space,
/// so `World Bank`, `world-bank` and `world_bank` compare equal.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .split(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | '.'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the interface calls an entry, folded: its own names, and its
/// plugin's id and group names. Each in English plus the Korean copy the
/// renderer keeps for it.
struct Aliases {
    entry: Vec<String>,
    plugin: Vec<String>,
}

fn aliases() -> &'static BTreeMap<String, Aliases> {
    static ALIASES: OnceLock<BTreeMap<String, Aliases>> = OnceLock::new();
    ALIASES.get_or_init(|| {
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
        let with_korean = |name: &str| -> Vec<String> {
            let translated = english
                .iter()
                .find(|(_, value)| value.as_str() == name)
                .and_then(|(key, _)| korean.get(key));
            std::iter::once(name)
                .chain(translated.map(String::as_str))
                .map(fold)
                .collect()
        };
        let Ok(market) = bundled_marketplace() else {
            return BTreeMap::new();
        };
        market
            .catalog
            .entries
            .iter()
            .map(|entry| {
                let mut plugin = vec![fold(&entry.plugin)];
                if let Some(summary) = market
                    .plugins
                    .iter()
                    .find(|plugin| plugin.name == entry.plugin)
                {
                    plugin.extend(with_korean(&summary.interface.display_name));
                }
                let aliases = Aliases {
                    entry: with_korean(&entry.name),
                    plugin,
                };
                (entry.id.clone(), aliases)
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

/// An entry id first, then an entry name, then a plugin id or group
/// name that covers exactly one entry, all compared after `fold`. Anything
/// else fails with the candidates the model can name instead: the matches
/// when several entries fit, otherwise the catalog, either way with entries
/// whose own name or id shares a word with the query first.
pub(crate) fn resolve_setup_entry(
    query: &str,
) -> Result<&'static MarketplaceEntryDto, Vec<SetupCandidate>> {
    let entries = &bundled_marketplace()
        .map_err(|_| Vec::new())?
        .catalog
        .entries;
    let folded = fold(query);
    if let Some(entry) = entries.iter().find(|entry| fold(&entry.id) == folded) {
        return Ok(entry);
    }
    let aliases = aliases();
    let matching = |pick: fn(&Aliases) -> &Vec<String>| {
        entries
            .iter()
            .filter(|entry| {
                aliases
                    .get(&entry.id)
                    .is_some_and(|aliases| pick(aliases).contains(&folded))
            })
            .collect::<Vec<_>>()
    };
    let by_name = matching(|aliases| &aliases.entry);
    if let [entry] = by_name.as_slice() {
        return Ok(entry);
    }
    let by_plugin = matching(|aliases| &aliases.plugin);
    if let [entry] = by_plugin.as_slice() {
        return Ok(entry);
    }
    let matches = if !by_name.is_empty() {
        by_name
    } else {
        by_plugin
    };
    let words = folded
        .split(' ')
        .filter(|word| word.chars().count() > 1)
        .collect::<Vec<_>>();
    let shared = |entry: &MarketplaceEntryDto| {
        let mut text = fold(&entry.id);
        if let Some(aliases) = aliases.get(&entry.id) {
            for alias in &aliases.entry {
                text.push(' ');
                text.push_str(alias);
            }
        }
        words.iter().filter(|word| text.contains(*word)).count()
    };
    let mut listed = if matches.is_empty() {
        entries.iter().collect::<Vec<_>>()
    } else {
        matches
    };
    listed.sort_by_key(|entry| std::cmp::Reverse(shared(entry)));
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
            ("FRED_macro", "fred.macro"),
            ("FRED", "fred.macro"),
            ("fred", "fred.macro"),
            (" sec edgar ", "sec.edgar"),
            ("OpenDART", "opendart.disclosures"),
            ("krx", "krx.market-data"),
            ("world-bank", "world-bank.indicators"),
            ("World Bank", "world-bank.indicators"),
            ("세계은행", "world-bank.indicators"),
            ("web_research", "community.web-research"),
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
        // A miss names the entries that share a word with it first.
        let near = resolve_setup_entry("World Bank data").unwrap_err();
        assert_eq!(near[0].id, "world-bank.indicators");
        let group = resolve_setup_entry("시장 데이터").unwrap_err();
        assert!(group
            .iter()
            .any(|candidate| candidate.id == "openbb.platform"));
    }
}
