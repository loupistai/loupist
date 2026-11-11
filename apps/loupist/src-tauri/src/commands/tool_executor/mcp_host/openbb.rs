//! Host-owned presentation of the bundled market-data runtime to the model.
//!
//! Upstream docstrings and parameter schemas describe every provider the
//! runtime ships, so the host rewrites each card for the run: a short
//! description (what the route returns, its key arguments, which granted
//! providers are official versus best effort) and a parameter schema pruned
//! to the providers enabled for this run. Authority (which tools and providers
//! exist) is decided elsewhere; this only shapes text, provider order, the
//! schema's size and the host-handled `max_age` argument. The runtime's name
//! never reaches the model: the user sees "market data".
use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use crate::mcp::McpTool;

use super::point_in_time::dated_route;
use crate::commands::tool_executor::market::MAX_AGE_SECONDS_LIMIT;

pub(super) const OPENBB_SERVER_ID: &str = "openbb";

/// Activated when a turn first connects, so a common first question needs no
/// discovery round. Names absent from the granted inventory are ignored.
pub(super) const DEFAULT_ACTIVE_TOOLS: &[&str] = &[
    "equity_search",
    "equity_profile",
    "equity_price_quote",
    "equity_price_historical",
    "equity_fundamental_income",
    "equity_fundamental_balance",
    "equity_fundamental_cash",
    "equity_fundamental_dividends",
    "equity_fundamental_filings",
];

/// The most values a pruned property names as rejected by some provider.
const MAX_REJECTED_VALUES: usize = 12;
/// Keyless providers that scrape a website rather than call a published API.
/// They stay usable, but layouts, consent walls and blocking break them.
const BEST_EFFORT_PROVIDERS: &[&str] = &[
    "yfinance",
    "finviz",
    "seeking_alpha",
    "stockgrid",
    "multpl",
    "wsj",
];

/// Official or exchange-operated sources first where routes overlap.
const PROVIDER_PREFERENCE: &[&str] = &[
    "sec",
    "federal_reserve",
    "ecb",
    "bls",
    "oecd",
    "imf",
    "government_us",
    "cftc",
    "finra",
    "nasdaq",
    "tmx",
    "cboe",
    "famafrench",
    "deribit",
];

/// Recovery text after a scraped provider fails: it names only official
/// providers enabled for this run, so the model never retries with a provider
/// the run cannot use.
pub(super) fn best_effort_failure_next(enabled_provider_ids: &BTreeSet<String>) -> String {
    let official = PROVIDER_PREFERENCE
        .iter()
        .filter(|provider| enabled_provider_ids.contains(**provider))
        .take(4)
        .copied()
        .collect::<Vec<_>>();
    let alternative = if official.is_empty() {
        "Use another official provider listed for the tool if one is enabled".to_owned()
    } else {
        format!(
            "Use an official provider listed for the tool (for example {})",
            official.join(", ")
        )
    };
    format!("This provider scrapes a website and just failed; treat it as unavailable for this turn. {alternative} instead of retrying it, or report that the data is unavailable.")
}

pub(super) fn is_best_effort(provider: &str) -> bool {
    BEST_EFFORT_PROVIDERS.contains(&provider)
}

fn preference_rank(provider: &str) -> usize {
    if is_best_effort(provider) {
        return PROVIDER_PREFERENCE.len() + 1;
    }
    PROVIDER_PREFERENCE
        .iter()
        .position(|candidate| *candidate == provider)
        .unwrap_or(PROVIDER_PREFERENCE.len())
}

/// Routes with a host-written summary. `DEFAULT_ACTIVE_TOOLS` selects those
/// active on load; control tools name the rest for direct activation.
const CURATED_SUMMARIES: &[(&str, &str)] = &[
    ("equity_search", "Find listed companies by name or ticker; returns symbols, names and identifiers."),
    ("equity_profile", "Company profile: name, sector, industry, exchange, listing details and a business description."),
    ("equity_price_quote", "Latest quote (last price, change, volume) for one or more symbols. Latest only; not a historical value."),
    ("equity_price_historical", "Daily or intraday OHLCV price bars for a symbol over a date range."),
    ("etf_historical", "Daily or intraday OHLCV price bars for an ETF over a date range."),
    ("index_price_historical", "Daily or intraday price bars for an index over a date range."),
    ("crypto_price_historical", "Daily or intraday price bars for a crypto pair over a date range."),
    ("currency_price_historical", "Daily or intraday price bars for a currency pair over a date range."),
    ("derivatives_futures_historical", "Daily price bars for a futures contract over a date range."),
    ("equity_fundamental_income", "Income statement by fiscal period: revenue, operating income, net income and related lines."),
    ("equity_fundamental_balance", "Balance sheet by fiscal period: assets, liabilities, equity and related lines."),
    ("equity_fundamental_cash", "Cash flow statement by fiscal period: operating, investing and financing cash flows."),
    ("equity_fundamental_dividends", "Dividend history: ex-dividend date and amount per share, plus record and payment dates when available."),
    ("equity_fundamental_filings", "Company filings: filing date, form type and a link to each document."),
    ("equity_fundamental_metrics", "Key valuation and profitability metrics such as market cap, P/E and margins."),
    ("equity_calendar_earnings", "Earnings announcement dates for a date range."),
    ("equity_calendar_dividend", "Dividend calendar (ex, record and payment dates) for a date range."),
    ("etf_info", "ETF profile: issuer, inception, expense ratio and fund details."),
    ("etf_holdings", "ETF holdings with weights as reported by the provider."),
    ("currency_reference_rates", "Official daily euro reference exchange rates."),
    ("fixedincome_government_treasury_rates", "Daily US Treasury yields by maturity."),
    ("news_company", "Company news headlines with publication time and source link."),
];

fn curated_summary(tool_name: &str) -> Option<&'static str> {
    CURATED_SUMMARIES
        .iter()
        .find(|(name, _)| *name == tool_name)
        .map(|(_, summary)| *summary)
}

/// Curated routes that are not active on load, named so the model can call
/// activate_tools without a listing round.
fn curated_inactive_names() -> String {
    CURATED_SUMMARIES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !DEFAULT_ACTIVE_TOOLS.contains(name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn control_summary(tool_name: &str) -> Option<String> {
    Some(match tool_name {
        "available_categories" => format!(
            "List market data categories and subcategories with tool counts. Common price, fundamentals, dividend and filing tools are already active. These exist too and need no listing, only activate_tools: {}. Use this only to find others.",
            curated_inactive_names()
        ),
        "available_tools" => "List the tools in one category (optional subcategory) with one-line summaries and whether each is active. Activate a tool before calling it.".into(),
        "activate_tools" => format!(
            "Make the named market data tools callable for this turn. Pass exact names from available_tools, or one of these without listing: {}.",
            curated_inactive_names()
        ),
        "deactivate_tools" => "Remove named market data tools that are no longer needed this turn.".into(),
        "activate_category" => "Activate every tool in a category or subcategory. Prefer activate_tools for one or two tools.".into(),
        _ => return None,
    })
}

/// First sentence of upstream text, without markdown, bounded.
fn upstream_sentence(description: Option<&str>) -> Option<String> {
    let text = description?
        .split("\n\n")
        .next()?
        .replace(['`', '*', '#'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    let end = text
        .char_indices()
        .find(|(index, character)| {
            *character == '.'
                && text[index + 1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map(|(index, _)| index + 1)
        .unwrap_or(text.len());
    let mut sentence = text[..end].to_owned();
    if sentence.chars().count() > 180 {
        sentence = sentence.chars().take(179).collect::<String>() + "…";
    }
    Some(sentence)
}

fn key_arguments(parameters: &Value) -> Vec<String> {
    let properties = parameters.get("properties").and_then(Value::as_object);
    let required = parameters
        .get("required")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut arguments = required
        .iter()
        .filter(|name| **name != "provider")
        .map(|name| format!("{name} (required)"))
        .collect::<Vec<_>>();
    for optional in ["start_date", "end_date", "period", "limit", "form_type"] {
        if !required.contains(&optional) && properties.is_some_and(|map| map.contains_key(optional))
        {
            arguments.push(optional.to_owned());
        }
    }
    arguments.truncate(6);
    arguments
}

fn provider_note(providers: &[String]) -> Option<String> {
    if providers.is_empty() {
        return None;
    }
    let (best_effort, official): (Vec<_>, Vec<_>) = providers
        .iter()
        .map(String::as_str)
        .partition(|provider| is_best_effort(provider));
    Some(match (official.is_empty(), best_effort.is_empty()) {
        (false, true) => format!("Providers: {}.", official.join(", ")),
        (false, false) => format!(
            "Providers: {} preferred; {} best effort (scraped website data that can break or be blocked).",
            official.join(", "),
            best_effort.join(", ")
        ),
        (true, _) => format!(
            "Providers: {} best effort only (scraped website data that can break or be blocked); name the provider when citing.",
            best_effort.join(", ")
        ),
    })
}

/// JSON Schema keywords the model needs. Every other key on a property is
/// vendor data (`x-widget_config`, provider-keyed `choices` objects) that the
/// runtime attaches per provider.
const SCHEMA_KEYWORDS: &[&str] = &[
    "type",
    "anyOf",
    "oneOf",
    "allOf",
    "enum",
    "const",
    "default",
    "description",
    "items",
    "properties",
    "required",
    "additionalProperties",
    "format",
    "pattern",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minProperties",
    "maxProperties",
    "multipleOf",
    "nullable",
];

fn provider_token(token: &str) -> bool {
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// The providers a property's upstream `title` names (`"fmp,intrinio,sec"`),
/// or None when the title is a human label such as `"Start Date"`.
fn titled_providers(property: &Map<String, Value>) -> Option<Vec<&str>> {
    let title = property.get("title")?.as_str()?;
    let tokens = title.split(',').map(str::trim).collect::<Vec<_>>();
    tokens
        .iter()
        .all(|token| !token.is_empty() && provider_token(token))
        .then_some(tokens)
}

/// The providers of a property only some providers accept. Upstream titles a
/// standard field with the provider that redefined it (`symbol` is titled
/// `"intrinio"`), so a provider title alone is not enough: every sentence of a
/// provider-only description ends in a `(provider: …)` marker, while a
/// standard field keeps an unmarked first sentence.
fn provider_only(property: &Map<String, Value>) -> Option<Vec<&str>> {
    let titled = titled_providers(property)?;
    let description = property
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    description
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .all(|part| part.contains("(provider:"))
        .then_some(titled)
}

/// Keeps the sentences of an upstream description that apply to an enabled
/// provider. Upstream joins one sentence per provider with `;` and ends each
/// with `(provider: a, b)`; a sentence without that marker applies to all.
fn prune_description(description: &str, enabled: &BTreeSet<String>) -> String {
    let parts = description
        .split(';')
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let marker = |part: &str| -> Option<(String, Vec<String>)> {
        let start = part.rfind("(provider:")?;
        let names = part[start + "(provider:".len()..]
            .trim_end_matches(')')
            .split(',')
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        Some((part[..start].trim().to_owned(), names))
    };
    let kept = parts
        .iter()
        .filter_map(|part| match marker(part) {
            Some((text, names)) => names
                .iter()
                .any(|name| enabled.contains(name))
                .then_some((text, Some(names))),
            None => Some((part.clone(), None)),
        })
        .collect::<Vec<_>>();
    if kept.is_empty() {
        return parts
            .first()
            .map(|part| marker(part).map_or(part.clone(), |(text, _)| text))
            .unwrap_or_default();
    }
    let single = kept.len() == 1;
    kept.into_iter()
        .map(|(text, names)| match names {
            Some(names) if !single => {
                let names = names
                    .into_iter()
                    .filter(|name| enabled.contains(name))
                    .collect::<Vec<_>>();
                format!("{text} (provider: {})", names.join(", "))
            }
            _ => text,
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Removes provider-specific weight from one parameter schema: properties no
/// enabled provider accepts, vendor keys, upstream titles and the description
/// sentences of other providers. An enabled provider's single-value `choices`
/// become the property's enum when upstream gave none.
fn prune_parameters(parameters: &mut Value, enabled: &BTreeSet<String>) {
    let Some(schema) = parameters.as_object_mut() else {
        return;
    };
    schema.remove("title");
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let tool_providers = schema
        .get("properties")
        .and_then(|properties| properties.get("provider"))
        .and_then(|provider| provider.get("enum"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter(|provider| enabled.contains(*provider))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| enabled.iter().cloned().collect());
    let mut dropped = Vec::new();
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        for (name, property) in properties.iter_mut() {
            let Some(property) = property.as_object_mut() else {
                continue;
            };
            if name != "provider" && !required.contains(name) {
                if let Some(only) = provider_only(property) {
                    if !only.iter().any(|provider| enabled.contains(*provider)) {
                        dropped.push(name.clone());
                        continue;
                    }
                }
            }
            // An enum replaces the upstream union only when every enabled
            // provider that accepts this property names its choices; a
            // provider without choices (TMX intervals) accepts other values.
            let accepting = titled_providers(property)
                .filter(|_| provider_only(property).is_some())
                .map(|titled| {
                    titled
                        .into_iter()
                        .filter(|provider| enabled.contains(*provider))
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| tool_providers.clone());
            let all_listed = !accepting.is_empty()
                && accepting.iter().all(|provider| {
                    property.get(provider).is_some_and(|vendor| {
                        vendor.get("choices").is_some_and(Value::is_array)
                            && vendor.get("multiple_items_allowed") != Some(&Value::Bool(true))
                    })
                });
            let listed = accepting
                .iter()
                .filter(|_| all_listed)
                .filter_map(|provider| {
                    let choices = property.get(provider)?.get("choices")?.as_array()?;
                    let names = choices
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    Some((provider.clone(), names))
                })
                .collect::<Vec<_>>();
            let choices = listed
                .iter()
                .flat_map(|(_, names)| names.iter().map(|name| json!(name)))
                .collect::<Vec<_>>();
            // The enum is the union, so a provider that rejects some of its
            // values says which: yfinance rejects TMX's "unadjusted". Long
            // disjoint lists (screener industries) would cost more than the
            // retry they save, so past a few values the note is left out.
            let rejected = listed
                .iter()
                .map(|(provider, names)| {
                    let refused = choices
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|choice| !names.iter().any(|name| name == choice))
                        .fold(Vec::<&str>::new(), |mut refused, choice| {
                            if !refused.contains(&choice) {
                                refused.push(choice);
                            }
                            refused
                        });
                    (provider, refused)
                })
                .filter(|(_, refused)| !refused.is_empty())
                .collect::<Vec<_>>();
            let by_provider = (!rejected.is_empty()
                && rejected
                    .iter()
                    .map(|(_, refused)| refused.len())
                    .sum::<usize>()
                    <= MAX_REJECTED_VALUES)
                .then(|| {
                    let parts = rejected
                        .iter()
                        .map(|(provider, refused)| {
                            format!("{provider} rejects {}", refused.join(", "))
                        })
                        .collect::<Vec<_>>();
                    format!("{}.", parts.join("; "))
                });
            let vendor_keys = property
                .keys()
                .filter(|key| !SCHEMA_KEYWORDS.contains(&key.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            for key in vendor_keys {
                property.remove(&key);
            }
            if !choices.is_empty()
                && !property.contains_key("enum")
                && !property.contains_key("const")
            {
                let mut unique = Vec::new();
                for choice in choices {
                    if !unique.contains(&choice) {
                        unique.push(choice);
                    }
                }
                property.remove("anyOf");
                property.insert("type".into(), json!("string"));
                property.insert("enum".into(), Value::Array(unique));
            }
            if let Some(Value::String(description)) = property.get("description") {
                let pruned = prune_description(description, enabled);
                property.insert("description".into(), Value::String(pruned));
            }
            if let Some(by_provider) = by_provider {
                let description = property
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map_or(by_provider.clone(), |text| format!("{text} {by_provider}"));
                property.insert("description".into(), Value::String(description));
            }
        }
        for name in &dropped {
            properties.remove(name);
        }
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|value| {
            value
                .as_str()
                .is_none_or(|name| !dropped.contains(&name.to_owned()))
        });
    }
}

/// Rewrites one agent card for the bundled market-data runtime.
pub(super) fn shape_card(
    card: &mut Value,
    tool: &McpTool,
    control: bool,
    local: bool,
    enabled_provider_ids: &BTreeSet<String>,
) {
    if control {
        if let Some(summary) = control_summary(&tool.name) {
            card["description"] = json!(summary);
        }
        return;
    }
    let mut providers = card
        .pointer("/parameters/properties/provider/enum")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    providers.sort_by_key(|provider| preference_rank(provider));
    if let Some(values) = card.pointer_mut("/parameters/properties/provider/enum") {
        *values = json!(providers);
    }
    if !local {
        if let Some(parameters) = card.get_mut("parameters") {
            prune_parameters(parameters, enabled_provider_ids);
        }
    }
    let summary = curated_summary(&tool.name)
        .map(str::to_owned)
        .or_else(|| upstream_sentence(tool.description.as_deref()))
        .unwrap_or_else(|| format!("Market data: {}.", tool.name.replace('_', " ")));
    let mut parts = vec![summary];
    let arguments = key_arguments(card.get("parameters").unwrap_or(&Value::Null));
    if !arguments.is_empty() {
        parts.push(format!("Args: {}.", arguments.join(", ")));
    }
    if local {
        parts.push("Computes locally on the data you pass; no provider call.".into());
    } else if let Some(note) = provider_note(&providers) {
        parts.push(note);
    }
    if dated_route(&tool.name).is_some() {
        parts.push("Under a historical as-of cutoff, later rows are removed.".into());
    }
    if !local {
        let declared = tool.input_schema.pointer("/properties/max_age").is_some();
        if let (false, Some(properties)) = (
            declared,
            card.pointer_mut("/parameters/properties")
                .and_then(Value::as_object_mut),
        ) {
            properties.insert(
                "max_age".into(),
                json!({
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_AGE_SECONDS_LIMIT,
                    "description": "Optional seconds. Reuse this Loupist's saved result for the identical request when it was fetched at most this long ago; otherwise fetch again. Omit to always fetch."
                }),
            );
        }
    }
    card["description"] = json!(parts.join(" "));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, description: &str, schema: Value) -> McpTool {
        McpTool {
            name: name.into(),
            title: None,
            description: Some(description.into()),
            input_schema: schema,
            output_schema: None,
            annotations: Some(json!({"readOnlyHint": true})),
            metadata: None,
        }
    }

    fn enabled(providers: &[&str]) -> BTreeSet<String> {
        providers
            .iter()
            .map(|provider| (*provider).to_owned())
            .collect()
    }

    #[test]
    fn cards_rank_official_providers_first_and_replace_upstream_text() {
        let schema = json!({
            "type": "object",
            "properties": {
                "symbol": {"type": "string"},
                "start_date": {"type": "string"},
                "end_date": {"type": "string"},
                "provider": {"type": "string", "enum": ["yfinance", "tmx", "cboe"]}
            },
            "required": ["symbol", "provider"]
        });
        let upstream = format!(
            "Get historical price data for a given stock.\n\n**Query Parameters:** {}",
            "provider notes ".repeat(200)
        );
        let source = tool("equity_price_historical", &upstream, schema.clone());
        let mut card = json!({"description": upstream, "parameters": schema});
        shape_card(
            &mut card,
            &source,
            false,
            false,
            &enabled(&["yfinance", "tmx", "cboe"]),
        );
        assert_eq!(
            card["parameters"]["properties"]["provider"]["enum"],
            json!(["tmx", "cboe", "yfinance"])
        );
        let description = card["description"].as_str().unwrap();
        assert!(!description.contains("Query Parameters"));
        assert!(description.len() < 600);
        for named in ["symbol", "start_date", "tmx", "yfinance"] {
            assert!(description.contains(named), "{description}");
        }
        assert_eq!(
            card["parameters"]["properties"]["max_age"]["type"],
            "integer"
        );
        // A scraper-only route still names its provider.
        let scraped = json!({"type": "object", "properties": {"symbol": {}, "provider": {"enum": ["finviz"]}}, "required": ["symbol", "provider"]});
        let source = tool(
            "equity_price_performance",
            "Get price performance.",
            scraped.clone(),
        );
        let mut card = json!({"description": "", "parameters": scraped});
        shape_card(&mut card, &source, false, false, &enabled(&["finviz"]));
        let description = card["description"].as_str().unwrap();
        assert!(description.starts_with("Get price performance."));
        assert!(description.contains("finviz"));
        assert!(is_best_effort("finviz") && !is_best_effort("tmx"));
    }

    /// The runtime's schema names every provider it ships. The card keeps only
    /// what the run's providers accept: other providers' properties, vendor
    /// keys, titles and per-provider sentences go.
    #[test]
    fn a_standard_field_redefined_by_one_provider_survives_pruning() {
        // Upstream shapes from equity_price_historical: `symbol` carries the
        // redefining provider as its title, and TMX lists no interval choices.
        let mut parameters = json!({
            "type": "object",
            "properties": {
                "provider": {"type": "string", "enum": ["cboe", "tmx", "yfinance"]},
                "symbol": {
                    "type": "string",
                    "title": "intrinio",
                    "description": "Symbol to get data for. Multiple comma separated items allowed for provider(s): cboe, tmx, yfinance.;\n    A Security identifier (Ticker, FIGI, ISIN, CUSIP, Intrinio ID). (provider: intrinio)",
                    "cboe": {"multiple_items_allowed": true}
                },
                "interval": {
                    "anyOf": [{"type": "string"}, {"type": "integer"}],
                    "title": "cboe,tmx,yfinance",
                    "description": "Time interval of the data to return.",
                    "cboe": {"choices": ["1m", "1d"]},
                    "yfinance": {"choices": ["1m", "1d", "1W"]}
                },
                "thea_enabled": {
                    "type": "boolean",
                    "title": "intrinio",
                    "description": "Return trades with the Thea model. (provider: intrinio)"
                }
            },
            "required": ["provider", "symbol"]
        });
        prune_parameters(&mut parameters, &enabled(&["cboe", "tmx", "yfinance"]));
        let properties = &parameters["properties"];
        assert_eq!(properties["symbol"]["type"], "string");
        assert!(!properties["symbol"]["description"]
            .as_str()
            .unwrap()
            .contains("Intrinio"));
        assert_eq!(parameters["required"], json!(["provider", "symbol"]));
        assert!(properties.get("thea_enabled").is_none());
        // TMX accepts intervals outside the listed choices, so no enum narrows them.
        assert!(properties["interval"].get("enum").is_none());
        assert!(properties["interval"].get("anyOf").is_some());
        assert!(properties["interval"].get("cboe").is_none());

        let mut listed = json!({
            "type": "object",
            "properties": {
                "provider": {"type": "string", "enum": ["cboe", "yfinance"]},
                "interval": {
                    "anyOf": [{"type": "string"}],
                    "description": "Time interval of the data to return.",
                    "cboe": {"choices": ["1m", "1d"]},
                    "yfinance": {"choices": ["1d", "1W"]}
                }
            }
        });
        prune_parameters(&mut listed, &enabled(&["cboe", "yfinance"]));
        assert_eq!(
            listed["properties"]["interval"]["enum"],
            json!(["1m", "1d", "1W"])
        );
        // The union enum alone would offer cboe's "1m" to yfinance callers.
        assert_eq!(
            listed["properties"]["interval"]["description"],
            "Time interval of the data to return. cboe rejects 1W; yfinance rejects 1m."
        );

        let mut same = json!({
            "type": "object",
            "properties": {
                "provider": {"type": "string", "enum": ["cboe", "yfinance"]},
                "interval": {
                    "anyOf": [{"type": "string"}],
                    "description": "Time interval of the data to return.",
                    "cboe": {"choices": ["1d", "1W"]},
                    "yfinance": {"choices": ["1W", "1d"]}
                }
            }
        });
        prune_parameters(&mut same, &enabled(&["cboe", "yfinance"]));
        assert_eq!(
            same["properties"]["interval"]["description"],
            "Time interval of the data to return."
        );

        // Disjoint long lists keep the union without a note.
        let many = |prefix: &str| (0..10).map(|n| format!("{prefix}{n}")).collect::<Vec<_>>();
        let mut long = json!({
            "type": "object",
            "properties": {
                "provider": {"type": "string", "enum": ["finviz", "yfinance"]},
                "industry": {
                    "anyOf": [{"type": "string"}],
                    "description": "Industry.",
                    "finviz": {"choices": many("f")},
                    "yfinance": {"choices": many("y")}
                }
            }
        });
        prune_parameters(&mut long, &enabled(&["finviz", "yfinance"]));
        assert_eq!(long["properties"]["industry"]["description"], "Industry.");
        assert_eq!(
            long["properties"]["industry"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            20
        );
    }

    #[test]
    fn parameters_are_pruned_to_the_runs_providers() {
        let schema = json!({
            "type": "object",
            "properties": {
                "provider": {"type": "string", "enum": ["sec", "yfinance"], "title": "Provider"},
                "symbol": {
                    "anyOf": [{"type": "string"}, {"type": "null"}],
                    "title": "Symbol",
                    "description": "Symbol to get data for.",
                    "nasdaq": {"x-widget_config": {"value": "AAPL"}}
                },
                "cik": {"type": "string", "title": "fmp,sec", "description": "CIK number to look up. Overrides symbol. (provider: fmp);\n    Lookup filings by Central Index Key (CIK) instead of by symbol. (provider: sec)"},
                "thea_enabled": {"type": "boolean", "title": "intrinio", "description": "Intrinio only. (provider: intrinio)"},
                "form_type": {
                    "anyOf": [{"type": "string"}, {"type": "null"}],
                    "title": "intrinio,sec",
                    "description": "SEC form type to filter by. (provider: intrinio, sec)",
                    "sec": {"multiple_items_allowed": true, "choices": ["10-K", "10-Q"]}
                },
                "adjustment": {
                    "anyOf": [{"type": "string", "enum": ["splits_only", "splits_and_dividends", "unadjusted"]}, {"type": "string", "enum": ["splits_only", "splits_and_dividends"]}],
                    "title": "fmp,yfinance",
                    "description": "Type of adjustment. (provider: fmp);\n    The adjustment factor to apply. Default is splits only. (provider: yfinance)",
                    "default": "splits_only",
                    "fmp": {"choices": ["splits_only", "splits_and_dividends", "unadjusted"]},
                    "yfinance": {"choices": ["splits_only", "splits_and_dividends"]}
                }
            },
            "required": ["provider"]
        });
        let source = tool("equity_fundamental_filings", "Get filings.", schema.clone());
        let mut card = json!({"description": "", "parameters": schema});
        shape_card(
            &mut card,
            &source,
            false,
            false,
            &enabled(&["sec", "yfinance"]),
        );
        let properties = card["parameters"]["properties"].as_object().unwrap();
        assert!(
            !properties.contains_key("thea_enabled"),
            "intrinio-only property survived"
        );
        assert_eq!(card["parameters"]["required"], json!(["provider"]));
        assert!(properties
            .values()
            .all(|property| !property.get("title").is_some()));
        assert!(properties["symbol"].get("nasdaq").is_none());
        assert!(properties["form_type"].get("sec").is_none());
        // A multi-value field stays a free string rather than a one-value enum.
        assert!(properties["form_type"].get("enum").is_none());
        assert_eq!(
            properties["form_type"]["description"],
            "SEC form type to filter by."
        );
        assert_eq!(
            properties["cik"]["description"],
            "Lookup filings by Central Index Key (CIK) instead of by symbol."
        );
        // The enabled provider's choices replace an anyOf of every provider's enums.
        assert_eq!(
            properties["adjustment"]["enum"],
            json!(["splits_only", "splits_and_dividends"])
        );
        assert!(properties["adjustment"].get("anyOf").is_none());
        assert_eq!(
            properties["adjustment"]["description"],
            "The adjustment factor to apply. Default is splits only."
        );
    }

    #[test]
    fn no_card_or_recovery_text_names_the_runtime() {
        let schema = json!({"type": "object", "properties": {"symbol": {"type": "string"}, "provider": {"enum": ["yfinance"]}}, "required": ["symbol", "provider"]});
        let mut shaped = Vec::new();
        for name in [
            "equity_price_quote",
            "some_new_route",
            "available_categories",
            "activate_tools",
            "deactivate_tools",
            "activate_category",
            "available_tools",
        ] {
            let source = tool(name, "", schema.clone());
            let mut card = json!({"description": "", "parameters": schema.clone()});
            let control = !name.starts_with("equity") && name != "some_new_route";
            shape_card(&mut card, &source, control, false, &enabled(&["yfinance"]));
            shaped.push(card.to_string());
        }
        shaped.push(best_effort_failure_next(&enabled(&["yfinance", "nasdaq"])));
        for text in shaped {
            assert!(!text.to_ascii_lowercase().contains("openbb"), "{text}");
        }
    }

    #[test]
    fn control_tools_name_curated_routes_the_model_can_activate_directly() {
        for control in ["available_categories", "activate_tools"] {
            let summary = control_summary(control).unwrap();
            for name in ["etf_holdings", "news_company", "equity_fundamental_metrics"] {
                assert!(summary.contains(name), "{summary}");
            }
            assert!(!summary.contains("equity_price_quote"), "{summary}");
        }
    }

    #[test]
    fn a_scraped_failure_recommends_only_enabled_official_providers() {
        let next = best_effort_failure_next(&enabled(&["yfinance", "nasdaq", "tmx"]));
        assert!(next.contains("nasdaq, tmx"), "{next}");
        assert!(!next.contains("sec"), "{next}");
        let none = best_effort_failure_next(&enabled(&["yfinance"]));
        assert!(none.contains("if one is enabled"), "{none}");
    }

    #[test]
    fn local_and_control_cards_keep_their_inputs() {
        let schema = json!({"type": "object", "properties": {"data": {"type": "array"}}, "required": ["data"]});
        let source = tool(
            "technical_sma",
            "Calculate the simple moving average.",
            schema.clone(),
        );
        let mut card = json!({"description": "", "parameters": schema.clone()});
        shape_card(&mut card, &source, false, true, &BTreeSet::new());
        // Local analytics fetch nothing, so they take no max_age.
        assert_eq!(card["parameters"], schema);
        assert!(card["description"]
            .as_str()
            .unwrap()
            .starts_with("Calculate the simple moving average."));

        let source = tool("available_tools", "upstream", json!({"type": "object"}));
        let mut card = json!({"description": "upstream", "parameters": {"type": "object"}});
        shape_card(&mut card, &source, true, false, &BTreeSet::new());
        assert_ne!(card["description"], "upstream");
        assert_eq!(card["parameters"], json!({"type": "object"}));
    }
}
