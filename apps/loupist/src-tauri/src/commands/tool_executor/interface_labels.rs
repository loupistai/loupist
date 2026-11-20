use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{agent_harness::interface::InterfaceLocale, broker::BrokerError};

#[derive(Clone, Copy, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
enum LabelKey {
    Bot,
    ResearchSession,
    Map,
    Schedule,
    MarketDataPlatform,
    ResearchSessionsHeading,
    MarketDataGroup,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelRequest {
    keys: Vec<LabelKey>,
}

pub(super) fn read(locale: Option<InterfaceLocale>, params: Value) -> Result<Value, BrokerError> {
    let locale = locale.ok_or(BrokerError::MethodDenied)?;
    let invalid = BrokerError::invalid_interface_labels;
    if !params.is_object() {
        return Err(invalid());
    }
    let request: LabelRequest = serde_json::from_value(params).map_err(|_| invalid())?;
    if !(1..=7).contains(&request.keys.len()) {
        return Err(invalid());
    }
    let keys: BTreeSet<_> = request.keys.iter().copied().collect();
    if keys.len() != request.keys.len() {
        return Err(invalid());
    }

    let catalog = locale.catalog();
    let labels: BTreeMap<_, &str> = keys
        .into_iter()
        .map(|key| {
            let label = match key {
                LabelKey::ResearchSessionsHeading => &catalog.research_sessions_heading,
                LabelKey::MarketDataGroup => &catalog.market_data_group,
                _ => catalog.names[json!(key).as_str().expect("label key serializes")]
                    .as_str()
                    .expect("bundled product name"),
            };
            (key, label)
        })
        .collect();
    Ok(json!(labels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_labels_match_the_registered_keys_and_screen_catalogs() {
        let tool = crate::broker::tools::TOOL_CATALOG
            .tools
            .iter()
            .find(|tool| tool.name == "interface_labels")
            .unwrap();
        assert_eq!(tool.method.as_deref(), Some("interface.labels"));
        let catalog: Value =
            serde_json::from_str(include_str!("../../../../agent/tool-catalog.json")).unwrap();
        let declaration = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "interface_labels")
            .unwrap();
        let keys = &declaration["parameters"]["properties"]["keys"]["items"]["enum"];
        let request = json!({"keys": keys});
        for (locale, expected) in [
            (
                InterfaceLocale::English,
                json!({
                    "bot": "Bot", "research_session": "research session", "map": "Map",
                    "schedule": "Schedule", "market_data_platform": "Market data platform",
                    "research_sessions_heading": "Research sessions", "market_data_group": "Market data"
                }),
            ),
            (
                InterfaceLocale::Korean,
                json!({
                    "bot": "봇", "research_session": "리서치 세션", "map": "지도",
                    "schedule": "예약", "market_data_platform": "시장 데이터 플랫폼",
                    "research_sessions_heading": "리서치 세션", "market_data_group": "시장 데이터"
                }),
            ),
        ] {
            assert_eq!(read(Some(locale), request.clone()).unwrap(), expected);
        }
        assert_eq!(
            read(Some(InterfaceLocale::Korean), json!({"keys": ["map"]})).unwrap(),
            json!({"map": "지도"}),
        );
        assert_eq!(
            read(
                Some(InterfaceLocale::from_language("fr")),
                json!({"keys": ["map"]})
            )
            .unwrap(),
            json!({"map": "Map"}),
        );
    }

    #[test]
    fn invalid_interface_label_arguments_share_one_repair() {
        let expected = json!({
            "code": "malformed_request",
            "message": "interface_labels: Pass only keys, an array of 1 to 7 distinct label keys from the tool definition.",
            "retryable": false,
            "next": ""
        });
        for invalid in [
            json!(null),
            json!([["map"]]),
            json!({}),
            json!({"keys": "map"}),
            json!({"keys": []}),
            json!({"keys": ["map", "map"]}),
            json!({"keys": ["map", "language"]}),
            json!({"keys": vec!["map"; 8]}),
            json!({"keys": ["map"], "language": "ko"}),
        ] {
            let error = read(Some(InterfaceLocale::Korean), invalid).unwrap_err();
            assert_eq!(crate::broker::error_body(&error), expected);
        }
        assert!(matches!(
            read(None, json!({"keys": ["map"]})),
            Err(BrokerError::MethodDenied)
        ));
    }
}
