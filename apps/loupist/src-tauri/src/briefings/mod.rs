//! Host-authored unattended research, with one schedule and one continuing Chat per Loupist.
pub(crate) mod commands;
mod conflicts;
pub(crate) mod desktop;
pub(crate) mod policy;

use crate::{
    app::{AppState, CommandError},
    commands::map_store,
    scheduled_tasks::types::Schedule,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BriefingContext {
    pub symbols: Vec<String>,
    pub principles_checked: usize,
    pub new_facts: usize,
    pub disputed_entity_ids: Vec<String>,
    /// Host-computed sections for the Schedule reader; the narrative is never parsed.
    #[serde(default)]
    pub changed: BriefingChanged,
    #[serde(default)]
    pub ahead: Vec<BriefingCheck>,
    /// "What you follow": followed assets with a delta since the last briefing.
    #[serde(default)]
    pub assets: Vec<BriefingAsset>,
    /// Results events around this briefing (at most 8): reported ones with two
    /// measures, upcoming ones for "Looking ahead".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earnings: Vec<BriefingEarnings>,
    /// Watcher alerts since the previous briefing (at most 24), as stored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_events: Vec<serde_json::Value>,
}

impl BriefingContext {
    pub(crate) fn carries_untrusted_text(&self) -> bool {
        self.assets.iter().any(|asset| !asset.rows.is_empty()) || !self.watch_events.is_empty()
    }
}

/// One results event in a briefing, computed by Rust at run start.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BriefingEarnings {
    pub asset_id: String,
    pub event_id: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub period: String,
    pub report_date: String,
    pub status: String,
    /// Expectations were captured before the report.
    pub captured: bool,
    /// At most two measures, as the card's table renders them.
    pub measures: Vec<serde_json::Value>,
}

/// At most this many results events and watcher alerts reach a briefing.
const MAX_BRIEFING_EARNINGS: usize = 8;
const MAX_BRIEFING_WATCH_EVENTS: usize = 24;

/// Results events around a briefing and the alerts since the previous one.
/// Names, dates and sourced values only; never holdings.
pub(crate) fn briefing_finance(
    state: &AppState,
    loupist_id: &str,
    since_ms: i64,
    now_ms: i64,
) -> (
    Vec<BriefingEarnings>,
    serde_json::Value,
    Vec<serde_json::Value>,
    serde_json::Value,
) {
    let today = chrono::DateTime::from_timestamp_millis(now_ms)
        .map(|time| time.with_timezone(&chrono::Local).date_naive())
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    let since_day = chrono::DateTime::from_timestamp_millis(since_ms)
        .map(|time| time.with_timezone(&chrono::Local).date_naive())
        .unwrap_or(today);
    let found = crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        let events = crate::knowledge::earnings::events_since(
            tx,
            loupist_id,
            today - chrono::Duration::days(45),
        )?;
        let watch = crate::knowledge::watch::list_events(
            tx,
            loupist_id,
            Some(since_ms),
            MAX_BRIEFING_WATCH_EVENTS,
        )?;
        Ok((events, watch))
    });
    let Ok((events, watch)) = found else {
        return (
            Vec::new(),
            serde_json::json!([]),
            Vec::new(),
            serde_json::json!([]),
        );
    };
    let mut earnings = Vec::new();
    let mut prompt = Vec::new();
    for event in events {
        let reported = event.release.is_some();
        let relevant = if reported {
            event.reported_on() >= since_day.min(today - chrono::Duration::days(1))
        } else {
            // A legal deadline is only a bound, never a date to look ahead to.
            event.date_source != crate::knowledge::earnings::DateSource::LegalDeadline
                && event.report_date >= today
                && event.report_date <= today + chrono::Duration::days(7)
        };
        if !relevant || earnings.len() >= MAX_BRIEFING_EARNINGS {
            continue;
        }
        let Ok(Some(snapshot)) = crate::commands::earnings::snapshot_for(
            state,
            loupist_id,
            &event.asset_id,
            Some(&event.event_id),
        ) else {
            continue;
        };
        let mut measures: Vec<&crate::knowledge::earnings::MeasureView> = snapshot
            .measures
            .iter()
            .filter(|measure| !measure.guidance && !measure.reported.is_empty())
            .collect();
        measures.sort_by_key(|measure| measure.expected.is_empty());
        prompt.push(serde_json::json!({
            "name": snapshot.name, "symbol": snapshot.symbol, "market": snapshot.market,
            "results": crate::knowledge::earnings::model_view(&snapshot),
        }));
        earnings.push(BriefingEarnings {
            asset_id: snapshot.asset_id.clone(),
            event_id: snapshot.event_id.clone(),
            name: snapshot.name.clone(),
            symbol: snapshot.symbol.clone(),
            market: snapshot.market.clone(),
            period: snapshot.period.clone(),
            report_date: snapshot.report_date.clone(),
            status: snapshot.status.as_str().to_owned(),
            captured: snapshot.before.is_some(),
            measures: measures
                .into_iter()
                .take(2)
                .filter_map(|measure| serde_json::to_value(measure).ok())
                .collect(),
        });
    }
    let watch_prompt = serde_json::json!(watch
        .iter()
        .map(|event| serde_json::json!({
            "name": event.name, "symbol": event.symbol, "market": event.market,
            "kind": event.kind, "factDate": event.fact_date, "facts": event.facts,
            "sources": event.evidence.iter().map(|item| serde_json::json!({"title": item.title, "url": item.url})).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>());
    let watch_context = watch
        .iter()
        .filter_map(|event| serde_json::to_value(event).ok())
        .collect();
    (
        earnings,
        serde_json::json!(prompt),
        watch_context,
        watch_prompt,
    )
}

/// One followed asset in a briefing, computed by Rust at run start.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BriefingAsset {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub verdict: Option<String>,
    pub verdict_since: Option<String>,
    /// At most three rows, as the asset page renders them.
    pub rows: Vec<serde_json::Value>,
}

/// What changed in World since the previous briefing, counted by the host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BriefingChanged {
    pub names: usize,
    pub relations: usize,
    pub conflicts: usize,
    /// Newest first, at most `MAX_CHANGE_ROWS`.
    pub rows: Vec<BriefingChange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BriefingChange {
    /// `claim_added`, `claim_disputed` or `source_retained`.
    pub kind: String,
    pub subject_id: Option<String>,
    pub summary: String,
}

/// A Journal check that is due, pointed to rather than quoted in full.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BriefingCheck {
    pub journal_id: String,
    pub title: String,
    pub by_date: Option<String>,
}

const MAX_CHANGE_ROWS: usize = 8;

pub(crate) const PROMPT: &str = include_str!("../../../agent/prompts/briefing.md");

pub(crate) fn default_schedule(timezone: String) -> Schedule {
    Schedule::Weekly {
        days: vec![1, 2, 3, 4, 5],
        hour: 8,
        minute: 0,
        timezone,
    }
}

/// A paragraph this short and wholly bold is a label ("**As of Nov 6**"), not prose.
const LABEL_CHARS: usize = 60;

/// Paragraph boundaries are parsed before stripping Markdown; Unicode scalars,
/// rather than UTF-8 bytes, define the product's 280-character limit. A leading
/// bold label paragraph is skipped when prose follows it.
pub(crate) fn summary(markdown: &str) -> String {
    let mut result = String::new();
    let mut label = None;
    let mut paragraph = false;
    let mut strong = 0usize;
    let mut plain = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Paragraph) => {
                paragraph = true;
                plain = false;
                result.clear();
            }
            Event::End(TagEnd::Paragraph) if paragraph => {
                paragraph = false;
                if plain || label.is_some() || result.trim().chars().count() > LABEL_CHARS {
                    break;
                }
                label = Some(std::mem::take(&mut result));
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Text(text) | Event::Code(text) if paragraph => {
                plain |= strong == 0 && !text.trim().is_empty();
                result.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak if paragraph => result.push(' '),
            _ => {}
        }
    }
    if result.trim().is_empty() {
        result = label.unwrap_or_default();
    }
    result
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(280)
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GrowthItem {
    at_ms: i64,
    kind: String,
    record_id: Option<String>,
    summary: String,
    #[serde(skip)]
    subject_id: Option<String>,
}

/// Due journal checks are pointers; the agent exact-reads before reviewing.
fn due_journal_reviews(
    state: &AppState,
    loupist_id: &str,
    now: i64,
) -> Result<Vec<serde_json::Value>, CommandError> {
    let today = crate::knowledge::journal::journal_date(now)
        .map_err(|e| CommandError::internal(e.to_string()))?
        .to_string();
    crate::knowledge::KnowledgeStore::for_loupist(state.store.clone(), loupist_id)
        .and_then(|store| store.read(|read| read.journal_due(loupist_id, &today)))
        .map(|rows| {
            rows.iter()
                .take(5)
                .map(|record| {
                    crate::agent_harness::journal_review_context(&serde_json::json!(record))
                })
                .collect()
        })
        .map_err(|e| CommandError::internal(e.to_string()))
}

fn system_language() -> String {
    // GUI launches on macOS need not inherit LANG. Read only the OS language
    // preference; no user workspace command or shell is involved.
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    {
        if output.status.success() && output.stdout.len() < 4096 {
            if let Some(language) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.trim().trim_matches(|ch| matches!(ch, '"' | ',')))
                .find(|line| line.as_bytes().first().is_some_and(u8::is_ascii_alphabetic))
            {
                return if language.starts_with("ko") {
                    "ko"
                } else {
                    "en"
                }
                .into();
            }
        }
    }
    let language = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
    if language.is_some_and(|value| value.starts_with("ko")) {
        "ko"
    } else {
        "en"
    }
    .into()
}

/// Gaps remain explicit even when no material delta was produced.
fn coverage(
    state: &AppState,
    loupist_id: &str,
    now: i64,
) -> Result<serde_json::Value, CommandError> {
    let context = crate::commands::assets::view_context(state)?;
    let followed = crate::commands::assets::followed(&context);
    crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        let routes = crate::knowledge::watch::routes(tx, loupist_id)?;
        let mut missing = Vec::new();
        let mut failed = Vec::new();
        for asset in &followed {
            let id = crate::knowledge::assets::asset_id(&asset.symbol, &asset.market);
            let page = crate::knowledge::assets::get_page(tx, loupist_id, &id)?;
            if page
                .as_ref()
                .and_then(|page| page.refreshed_at_ms)
                .is_none_or(|at| at < now.saturating_sub(30 * 60_000))
            {
                missing.push(serde_json::json!({"symbol":asset.symbol,"market":asset.market}));
            }
            for route in routes
                .get(&id)
                .into_iter()
                .flatten()
                .filter(|route| matches!(route.status.as_str(), "failed" | "needs_connection"))
            {
                failed.push(serde_json::json!({"symbol":asset.symbol,"market":asset.market,"route":route.route.as_str(),"status":route.status,"connection":route.connection,"checkedAtMs":route.checked_at_ms}));
            }
        }
        let omitted_assets = missing.len().saturating_sub(60);
        let omitted_routes = failed.len().saturating_sub(60);
        missing.truncate(60);
        failed.truncate(60);
        Ok(
            serde_json::json!({"notChecked":missing,"routes":failed,"omittedAssets":omitted_assets,"omittedRoutes":omitted_routes}),
        )
    })
}

pub(crate) fn build_prompt(
    state: &AppState,
    loupist_id: &str,
    previous: Option<i64>,
    now: i64,
) -> Result<(String, BriefingContext), CommandError> {
    let context = state.store.map_context().map_err(map_store)?;
    let disputed_entity_ids =
        conflicts::entity_ids(state, loupist_id, previous.unwrap_or(0), now, &context)?;
    let context = crate::agent_harness::followed_companies(context);
    let config = crate::commands::loupist_configuration(state, loupist_id)?;
    let language = if config.appearance.language == "system" {
        system_language()
    } else {
        config.appearance.language
    };
    let growth = {
        let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
        let mut statement=connection.prepare("SELECT at_ms,kind,record_id,summary,subject_id FROM knowledge_growth_events WHERE loupist_id=?1 AND at_ms>?2 AND at_ms<=?3 AND kind IN ('claim_added','claim_disputed','source_retained') AND NOT (kind='source_retained' AND IFNULL(record_id,'') IN (SELECT record_id FROM knowledge_sources WHERE provider_identity=?4)) ORDER BY at_ms DESC,event_id DESC LIMIT 40").map_err(|e| CommandError::internal(e.to_string()))?;
        let items = statement
            .query_map(
                // The user's own list of what they follow is not news.
                rusqlite::params![
                    loupist_id,
                    previous.unwrap_or(0),
                    now,
                    crate::knowledge::FOLLOWED_PROVIDER
                ],
                |row| {
                    Ok(GrowthItem {
                        at_ms: row.get(0)?,
                        kind: row.get(1)?,
                        record_id: row.get(2)?,
                        summary: row.get::<_, String>(3)?.chars().take(240).collect(),
                        subject_id: row.get(4)?,
                    })
                },
            )
            .map_err(|e| CommandError::internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CommandError::internal(e.to_string()))?;
        items
    };
    // Shared-pack text reaches the briefing inside the same markers as a Chat turn.
    let shared = crate::sharing::delivery::Shared::load(state, loupist_id);
    let principles = {
        let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
        let tx = connection
            .transaction()
            .map_err(|e| CommandError::internal(e.to_string()))?;
        let all = crate::knowledge::principles::principle_list(&tx, loupist_id, false)
            .map_err(|e| CommandError::internal(e.to_string()))?;
        crate::knowledge::principles::principles_context(&all).1
    };
    let rows = context["symbols"]
        .as_array()
        .expect("followed company projection");
    let mut followed: Vec<String> = rows
        .iter()
        .filter_map(|item| item["symbol"].as_str().map(str::to_owned))
        .collect();
    followed.sort();
    followed.dedup();
    let counts = {
        let connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
        let count = |kind: &str| -> Result<usize, CommandError> {
            connection
                .query_row(
                    "SELECT count(*) FROM knowledge_growth_events WHERE loupist_id=?1 AND at_ms>?2 AND at_ms<=?3 AND kind=?4",
                    rusqlite::params![loupist_id, previous.unwrap_or(0), now, kind],
                    |row| row.get::<_, i64>(0),
                )
                .map(|value| value.max(0) as usize)
                .map_err(|e| CommandError::internal(e.to_string()))
        };
        (
            count("entity_added")?,
            count("claim_added")?,
            count("claim_disputed")?,
        )
    };
    let due_reviews = due_journal_reviews(state, loupist_id, now)?;
    // Older folders without asset pages brief without this section.
    let (asset_deltas, mut asset_prompt) = crate::commands::assets::briefing_assets(
        state,
        loupist_id,
        previous.unwrap_or(now - 7 * 86_400_000),
    )
    .unwrap_or_default();
    crate::custom_connections::taint::delimit_asset_deltas(&mut asset_prompt);
    let (earnings, earnings_prompt, watch_events, mut watch_prompt) = briefing_finance(
        state,
        loupist_id,
        previous.unwrap_or(now - 7 * 86_400_000),
        now,
    );
    if watch_prompt.as_array().is_some_and(|rows| !rows.is_empty()) {
        crate::custom_connections::taint::delimit_returning(
            crate::broker::ToolMethod::AssetPageRead,
            &crate::custom_connections::taint::Returning::Fields(std::collections::BTreeMap::from(
                [(String::new(), "asset watch observations".to_owned())],
            )),
            &mut watch_prompt,
        );
    }
    let briefing_context = BriefingContext {
        earnings,
        watch_events,
        disputed_entity_ids,
        symbols: followed,
        principles_checked: principles.len(),
        new_facts: growth.len(),
        changed: BriefingChanged {
            names: counts.0,
            relations: counts.1,
            conflicts: counts.2,
            rows: growth
                .iter()
                .take(MAX_CHANGE_ROWS)
                .map(|item| BriefingChange {
                    kind: item.kind.clone(),
                    subject_id: item.subject_id.clone(),
                    summary: item.summary.clone(),
                })
                .collect(),
        },
        ahead: due_reviews
            .iter()
            .filter_map(|review| {
                let journal_id = review["journalId"].as_str().filter(|id| !id.is_empty())?;
                let check = &review["check"];
                let title = check["claim"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .or_else(|| review["judgment"].as_str())
                    .unwrap_or_default();
                Some(BriefingCheck {
                    journal_id: journal_id.to_owned(),
                    title: title.chars().take(200).collect(),
                    by_date: check["byDate"].as_str().map(str::to_owned),
                })
            })
            .collect(),
        assets: asset_deltas
            .into_iter()
            .map(|delta| BriefingAsset {
                asset_id: delta.asset_id,
                symbol: delta.symbol,
                market: delta.market,
                name: delta.name,
                verdict: delta.verdict.map(|verdict| verdict.as_str().to_owned()),
                verdict_since: delta.verdict_since,
                rows: delta
                    .rows
                    .iter()
                    .filter_map(|row| serde_json::to_value(row).ok())
                    .collect(),
            })
            .collect(),
    };
    let mut world_growth =
        serde_json::to_value(&growth).map_err(|e| CommandError::internal(e.to_string()))?;
    shared.mark_delivery(&mut world_growth);
    let data = serde_json::json!({
        "date":chrono::DateTime::from_timestamp_millis(now).map(|date|date.with_timezone(&chrono::Local).to_rfc3339()),
        "appLanguage":language,
        "previousBriefingAtMs":previous,
        "timeSincePreviousBriefingMs":previous.map(|at| now.saturating_sub(at)),
        "worldGrowth":world_growth, "dueJournalReviews":due_reviews, "assetDeltas": asset_prompt,
        "earnings": earnings_prompt, "watchAlerts": watch_prompt,
        "coverage": coverage(state, loupist_id, now).unwrap_or_else(|_| serde_json::json!({"unavailable":true}))
    });
    let prompt = format!(
        "{PROMPT}\n{}",
        serde_json::to_string(&data).map_err(|e| CommandError::internal(e.to_string()))?
    );
    #[cfg(debug_assertions)]
    crate::privacy::holdings::debug_scan(&state.store, "a briefing prompt", &prompt);
    Ok((prompt, briefing_context))
}

#[cfg(test)]
mod tests;
