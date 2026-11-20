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
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
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
    let mut list_depth = 0usize;
    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth -= 1,
            Event::Start(Tag::Paragraph) if list_depth == 0 => {
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
    crate::util::shorten(
        &result.split_whitespace().collect::<Vec<_>>().join(" "),
        280,
    )
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

pub(crate) fn system_language() -> String {
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

pub(crate) fn build_prompt(
    state: &AppState,
    loupist_id: &str,
    previous: Option<i64>,
    now: i64,
) -> Result<(String, BriefingContext), CommandError> {
    let context = state.store.map_context(loupist_id).map_err(map_store)?;
    let disputed_entity_ids =
        conflicts::entity_ids(state, loupist_id, previous.unwrap_or(0), now, &context)?;
    let context = crate::agent_harness::followed_context(state, loupist_id)?;
    let config = crate::commands::loupist_configuration(state, loupist_id)?;
    let mut language = config.appearance.language;
    crate::agent_harness::interface::resolve_language(&mut language);
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
                        summary: crate::util::shorten(&row.get::<_, String>(3)?, 240),
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
    let rows = context["assets"]
        .as_array()
        .expect("followed company projection");
    let mut followed: Vec<String> = rows
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
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
    let briefing_context = BriefingContext {
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
                    title: crate::util::shorten(title, 200),
                    by_date: check["byDate"].as_str().map(str::to_owned),
                })
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
        "worldGrowth":world_growth,
        "dueJournalReviews":due_reviews,
        "disputedEntityIds":briefing_context.disputed_entity_ids,
        "principles":principles,
        "followed": rows
    });
    let prompt = format!(
        "{PROMPT}\n{}",
        serde_json::to_string(&data).map_err(|e| CommandError::internal(e.to_string()))?
    );
    Ok((prompt, briefing_context))
}

#[cfg(test)]
mod tests;
