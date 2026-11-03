use crate::knowledge::graph_jobs::GraphProcessingSettings;
use serde_json::{json, Value};
pub(crate) fn observed_relation_names(
    store: &crate::knowledge::KnowledgeStore,
    loupist_id: &str,
) -> Vec<String> {
    store
        .read(|write| {
            let records = write.list_records(loupist_id)?;
            let Some(world) = records
                .into_iter()
                .find(|record| record.kind == crate::knowledge::model::RecordKind::World)
            else {
                return Ok(Vec::new());
            };
            let Some(revision_id) = world.revision_id else {
                return Ok(Vec::new());
            };
            let stored = crate::knowledge::revision::load_revision(write.tx, &revision_id)?;
            let body: crate::knowledge::model::WorldSnapshotBody =
                serde_json::from_str(&stored.payload_json)?;
            Ok(body
                .relation_vocabulary
                .into_iter()
                .take(16)
                .map(|item| item.relation)
                .collect())
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct FollowedInstrument {
    pub name: String,
    pub symbol: String,
}

/// Whether `text` holds `needle` as a whole word. A Latin name or code needs a
/// boundary on both sides, so a short ticker is not found inside other words;
/// other scripts attach particles (삼성전자는), so any occurrence counts.
fn mentions(text: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return false;
    }
    if !needle.is_ascii() {
        return text.contains(needle);
    }
    let word = |character: Option<char>| character.is_some_and(|c| c.is_ascii_alphanumeric());
    text.match_indices(needle).any(|(at, _)| {
        !word(text[..at].chars().next_back()) && !word(text[at + needle.len()..].chars().next())
    })
}

/// A name in any case; a ticker or code (no lowercase letter, like `A`, `ON` or
/// a symbol the user gave no name for) only as written, so it is not every "a"
/// or "on" in the text.
fn names(body: &str, folded: &str, name: &str) -> bool {
    let code =
        name.chars().any(|c| c.is_ascii_alphanumeric()) && !name.chars().any(char::is_lowercase);
    if code {
        mentions(body, name)
    } else {
        mentions(folded, &name.to_lowercase())
    }
}

pub(crate) fn extraction_hints(
    settings: &GraphProcessingSettings,
    body: &str,
    observed: &[String],
    followed: &[FollowedInstrument],
) -> Value {
    let folded = body.to_lowercase();
    let mut seen_symbols = std::collections::BTreeSet::new();
    let mut followed_hints = Vec::new();
    for item in followed {
        if followed_hints.len() >= 40 {
            break;
        }
        let symbol = item.symbol.trim();
        if symbol.is_empty() || !seen_symbols.insert(symbol.to_lowercase()) {
            continue;
        }
        let name = {
            let trimmed = item.name.trim();
            if trimmed.is_empty() {
                symbol
            } else {
                trimmed
            }
        };
        let symbol_hit = mentions(body, symbol);
        let name_hit = names(body, &folded, name);
        if symbol_hit || name_hit {
            followed_hints.push(json!({ "name": name, "symbol": symbol }));
        }
    }
    let anchors: Vec<Value> = settings
        .anchors
        .iter()
        .filter(|anchor| {
            std::iter::once(anchor.name.as_str())
                .chain(anchor.aliases.iter().map(String::as_str))
                .any(|name| names(body, &folded, name))
                || anchor
                    .identifiers
                    .iter()
                    .any(|identifier| mentions(body, identifier))
        })
        .take(24)
        .map(|anchor| {
            json!({
                "name": anchor.name,
                "aliases": anchor.aliases,
                "identifiers": anchor.identifiers,
            })
        })
        .collect();
    let mut relations = settings.relation_hints.clone();
    for name in observed {
        if relations.len() >= 32 {
            break;
        }
        if !relations.iter().any(|item| item == name) {
            relations.push(name.clone());
        }
    }
    json!({
        "focus": settings.research_focus,
        "relations": relations,
        "anchors": anchors,
        "fast_model": settings.fast_model,
        "extension": settings.extension,
        "followed": followed_hints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::graph_jobs;
    #[test]
    fn extraction_hints_include_only_anchors_and_instruments_named_in_the_episode() {
        let anchor = |name: &str, aliases: &[&str], identifiers: &[&str]| graph_jobs::WorldAnchor {
            name: name.into(),
            aliases: aliases.iter().map(|item| item.to_string()).collect(),
            identifiers: identifiers.iter().map(|item| item.to_string()).collect(),
        };
        let settings = GraphProcessingSettings {
            research_focus: "Korean suppliers".into(),
            relation_hints: vec!["SUPPLIES_TO".into()],
            anchors: vec![
                anchor("Samsung Electronics", &["삼성전자"], &["005930"]),
                anchor("Other Corp", &[], &[]),
                // A short ticker counts only as a whole word written as a ticker.
                anchor("Visa", &[], &["V"]),
                anchor("AA", &[], &["AA"]),
            ],
            ..GraphProcessingSettings::default()
        };
        let followed = |name: &str, symbol: &str| FollowedInstrument {
            name: name.into(),
            symbol: symbol.into(),
        };
        let hints = extraction_hints(
            &settings,
            "삼성전자는 (005930) reported earnings and AAPL rose a bit on volume.",
            &["COMPETES_WITH".into()],
            &[
                followed("Samsung Electronics", "005930"),
                followed("Samsung Electronics", "005930"),
                followed("Other Corp", "OTHER"),
                followed("", "AAPL"),
                followed("Blank", "  "),
                followed("Agilent", "A"),
                followed("", "ON"),
            ],
        );
        assert_eq!(hints["focus"], "Korean suppliers");
        assert_eq!(hints["relations"], json!(["SUPPLIES_TO", "COMPETES_WITH"]));
        let anchors: Vec<_> = hints["anchors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].clone())
            .collect();
        assert_eq!(anchors, [json!("Samsung Electronics")]);
        let symbols: Vec<_> = hints["followed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["symbol"].clone())
            .collect();
        assert_eq!(symbols, [json!("005930"), json!("AAPL")]);
    }
    #[test]
    fn every_followed_asset_is_an_anchor_of_the_episode_that_lists_it() {
        let mut assets: Vec<_> = (0..30)
            .map(|index| {
                crate::knowledge::FollowedAsset::new(
                    &format!("SYM{index:02}"),
                    "KRX",
                    Some(&format!("Company {index:02}")),
                )
            })
            .collect();
        // Short tickers followed earlier do not crowd a later Source's anchors.
        assets[0] = crate::knowledge::FollowedAsset::new("A", "NYSE", None);
        assets[1] = crate::knowledge::FollowedAsset::new("ON", "UNKNOWN", Some("onsemi"));
        let mut settings = GraphProcessingSettings::default();
        crate::knowledge::anchor_followed(&mut settings, &assets);
        // Extraction takes at most 24 anchors per episode, so 30 assets need two Sources.
        let batches: Vec<_> = assets
            .chunks(crate::knowledge::FOLLOWED_PER_SOURCE)
            .collect();
        assert_eq!(batches.len(), 2);
        for batch in batches {
            let body = crate::knowledge::followed_text(batch);
            let hints = extraction_hints(&settings, &body, &[], &[]);
            let anchored: Vec<_> = hints["anchors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|anchor| anchor["name"].as_str().unwrap().to_owned())
                .collect();
            let listed: Vec<_> = batch.iter().map(|asset| asset.name.clone()).collect();
            assert_eq!(anchored, listed);
        }
    }
}
