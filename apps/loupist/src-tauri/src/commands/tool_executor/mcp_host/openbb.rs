//! Host-owned presentation of the bundled OpenBB runtime to the model.
//!
//! Upstream docstrings and parameter text are long and provider-neutral, so the
//! host writes each card's description: what the route returns, its key
//! arguments, and which granted providers are official versus best effort.
//! Authority (which tools and providers exist) is decided elsewhere; this only
//! shapes text, provider order, and the host-handled `max_age` argument.
use serde_json::{json, Value};

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

pub(super) const BEST_EFFORT_FAILURE_NEXT: &str = "This provider scrapes a website and just failed; treat it as unavailable for this turn. Use an official provider listed for the tool (for example sec, nasdaq, tmx or cboe) instead of retrying it, or report that the data is unavailable.";

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

fn curated_summary(tool_name: &str) -> Option<&'static str> {
    Some(match tool_name {
        "equity_search" => "Find listed companies by name or ticker; returns symbols, names and identifiers.",
        "equity_profile" => "Company profile: name, sector, industry, exchange, listing details and a business description.",
        "equity_price_quote" => "Latest quote (last price, change, volume) for one or more symbols. Latest only; not a historical value.",
        "equity_price_historical" => "Daily or intraday OHLCV price bars for a symbol over a date range.",
        "etf_historical" => "Daily or intraday OHLCV price bars for an ETF over a date range.",
        "index_price_historical" => "Daily or intraday price bars for an index over a date range.",
        "crypto_price_historical" => "Daily or intraday price bars for a crypto pair over a date range.",
        "currency_price_historical" => "Daily or intraday price bars for a currency pair over a date range.",
        "derivatives_futures_historical" => "Daily price bars for a futures contract over a date range.",
        "equity_fundamental_income" => "Income statement by fiscal period: revenue, operating income, net income and related lines.",
        "equity_fundamental_balance" => "Balance sheet by fiscal period: assets, liabilities, equity and related lines.",
        "equity_fundamental_cash" => "Cash flow statement by fiscal period: operating, investing and financing cash flows.",
        "equity_fundamental_dividends" => "Dividend history: ex-dividend date and amount per share, plus record and payment dates when available.",
        "equity_fundamental_filings" => "Company filings: filing date, form type and a link to each document.",
        "equity_fundamental_metrics" => "Key valuation and profitability metrics such as market cap, P/E and margins.",
        "equity_calendar_earnings" => "Earnings announcement dates for a date range.",
        "equity_calendar_dividend" => "Dividend calendar (ex, record and payment dates) for a date range.",
        "etf_info" => "ETF profile: issuer, inception, expense ratio and fund details.",
        "etf_holdings" => "ETF holdings with weights as reported by the provider.",
        "currency_reference_rates" => "Official daily euro reference exchange rates.",
        "fixedincome_government_treasury_rates" => "Daily US Treasury yields by maturity.",
        "news_company" => "Company news headlines with publication time and source link.",
        _ => return None,
    })
}

fn control_summary(tool_name: &str) -> Option<&'static str> {
    Some(match tool_name {
        "available_categories" => "List OpenBB data categories and subcategories with tool counts. Common price, fundamentals, dividend and filing tools are already active; use this only to find others.",
        "available_tools" => "List the tools in one category (optional subcategory) with one-line summaries and whether each is active. Activate a tool before calling it.",
        "activate_tools" => "Make the named OpenBB tools callable for this turn. Pass exact names from available_tools.",
        "deactivate_tools" => "Remove named OpenBB tools that are no longer needed this turn.",
        "activate_category" => "Activate every tool in a category or subcategory. Prefer activate_tools for one or two tools.",
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

fn key_arguments(tool: &McpTool) -> Vec<String> {
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object);
    let required = tool
        .input_schema
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

/// Rewrites one agent card for the bundled OpenBB runtime.
pub(super) fn shape_card(card: &mut Value, tool: &McpTool, control: bool, local: bool) {
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
    let summary = curated_summary(&tool.name)
        .map(str::to_owned)
        .or_else(|| upstream_sentence(tool.description.as_deref()))
        .unwrap_or_else(|| format!("OpenBB {} data.", tool.name.replace('_', " ")));
    let mut parts = vec![summary];
    let arguments = key_arguments(tool);
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
        shape_card(&mut card, &source, false, false);
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
        shape_card(&mut card, &source, false, false);
        let description = card["description"].as_str().unwrap();
        assert!(description.starts_with("Get price performance."));
        assert!(description.contains("finviz"));
        assert!(is_best_effort("finviz") && !is_best_effort("tmx"));
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
        shape_card(&mut card, &source, false, true);
        // Local analytics fetch nothing, so they take no max_age.
        assert_eq!(card["parameters"], schema);
        assert!(card["description"]
            .as_str()
            .unwrap()
            .starts_with("Calculate the simple moving average."));

        let source = tool("available_tools", "upstream", json!({"type": "object"}));
        let mut card = json!({"description": "upstream", "parameters": {"type": "object"}});
        shape_card(&mut card, &source, true, false);
        assert_ne!(card["description"], "upstream");
        assert_eq!(card["parameters"], json!({"type": "object"}));
    }
}
