//! The morning message's event strip (W15.9e): what is coming up in the next
//! two weeks for the assets the user follows, and the Journal checks they set,
//! soonest first. Dates come only from stored results events, dividend
//! observations and Journal checks; no model call, no holdings quantities.
use chrono::{Duration, NaiveDate};
use rusqlite::Transaction;
use serde::Serialize;

use super::{
    bounded_text, list_pages, match_entity, observations, observe, world, AssetClass,
    ObservationKind, PageRecord, ViewContext,
};
use crate::knowledge::{
    earnings::{self, DateSource},
    journal,
    model::WorldSnapshotBody,
    KnowledgeResult,
};

/// Days ahead the strip looks, today included: the results card's own window.
pub const AHEAD_DAYS: i64 = earnings::AHEAD_DAYS;
/// The strip lists at most this many events.
pub const MAX_UPCOMING: usize = 5;
/// A check's claim is one line in the strip; the Journal keeps the rest.
const MAX_CLAIM_CHARS: usize = 280;

/// Ordered as rows of one day are listed: results, then dividends, then checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum UpcomingKind {
    Results,
    ExDividend,
    RecordDate,
    Check,
}

/// The page an event belongs to; the renderer names it in the reader's language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpcomingAsset {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub entity_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpcomingEvent {
    pub kind: UpcomingKind,
    /// The calendar day, YYYY-MM-DD.
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub asset: Option<UpcomingAsset>,
    /// Results: where the date comes from; a legal deadline is a bound, never an expected date.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub date_source: Option<DateSource>,
    /// Results: the event the asset page's results card opens on.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub period: Option<String>,
    /// Check: the Journal entry it opens.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub journal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub claim: Option<String>,
}

impl UpcomingEvent {
    fn new(kind: UpcomingKind, date: NaiveDate, asset: Option<UpcomingAsset>) -> Self {
        Self {
            kind,
            date: date.to_string(),
            asset,
            date_source: None,
            event_id: None,
            period: None,
            journal_id: None,
            claim: None,
        }
    }

    /// Soonest first; one day lists results, dividends, then checks, each by name.
    fn order_key(&self) -> (&str, UpcomingKind, String, &str) {
        let name = self
            .asset
            .as_ref()
            .map(|asset| asset.name.as_str())
            .or(self.claim.as_deref())
            .unwrap_or_default()
            .to_lowercase();
        let id = self
            .event_id
            .as_deref()
            .or(self.journal_id.as_deref())
            .or(self.asset.as_ref().map(|asset| asset.asset_id.as_str()))
            .unwrap_or_default();
        (&self.date, self.kind, name, id)
    }
}

/// The World snapshot is read once, and only when a page has no entity yet.
struct Entities<'a, 'tx> {
    tx: &'a Transaction<'tx>,
    loupist_id: &'a str,
    world: Option<Option<WorldSnapshotBody>>,
}

impl Entities<'_, '_> {
    fn asset(&mut self, page: &PageRecord) -> KnowledgeResult<UpcomingAsset> {
        let entity_id = match &page.entity_id {
            Some(id) => Some(id.clone()),
            None => {
                if self.world.is_none() {
                    self.world = Some(world(self.tx, self.loupist_id)?);
                }
                self.world
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|body| match_entity(body, page))
                    .map(|(id, _)| id)
            }
        };
        Ok(UpcomingAsset {
            asset_id: page.asset_id.clone(),
            symbol: page.symbol.clone(),
            market: page.market.clone(),
            name: page.name.clone(),
            entity_id,
        })
    }
}

/// At most `MAX_UPCOMING` events from today through `AHEAD_DAYS` ahead: the
/// next unreported results date of each followed equity, the next ex-dividend
/// or record date of each followed asset, and every open dated Journal check.
pub fn upcoming(
    tx: &Transaction<'_>,
    loupist_id: &str,
    context: &ViewContext,
) -> KnowledgeResult<Vec<UpcomingEvent>> {
    let today = context.today();
    let last = today + Duration::days(AHEAD_DAYS);
    let ahead = |date: NaiveDate| date >= today && date <= last;
    let pages = list_pages(tx, loupist_id)?;
    let mut entities = Entities {
        tx,
        loupist_id,
        world: None,
    };
    let mut out = Vec::new();
    for page in &pages {
        // Followed means on the watchlist or in holdings; an opened page alone is not.
        if context.watch(&page.symbol, &page.market) == "none" {
            continue;
        }
        let results = if page.class == AssetClass::Equity {
            earnings::events_for(tx, loupist_id, &page.asset_id)?
                .into_iter()
                .filter(|event| {
                    event.release.is_none()
                        && matches!(
                            event.status,
                            earnings::Status::Upcoming | earnings::Status::BeforeCaptured
                        )
                        && ahead(event.report_date)
                })
                .min_by_key(|event| event.report_date)
        } else {
            None
        };
        // An ex-date comes before its record date; the sooner of the two stands for the dividend.
        let dividend = observations(tx, loupist_id, &page.asset_id)?
            .iter()
            .filter(|item| item.kind == ObservationKind::Dividend)
            .flat_map(|item| {
                [
                    ("exDate", UpcomingKind::ExDividend),
                    ("recordDate", UpcomingKind::RecordDate),
                ]
                .into_iter()
                .filter_map(|(key, kind)| {
                    item.facts[key]
                        .as_str()
                        .and_then(observe::parse_day)
                        .map(|date| (date, kind))
                })
            })
            .filter(|(date, _)| ahead(*date))
            .min();
        if results.is_none() && dividend.is_none() {
            continue;
        }
        let asset = entities.asset(page)?;
        if let Some(event) = results {
            out.push(UpcomingEvent {
                date_source: Some(event.date_source),
                event_id: Some(event.event_id),
                period: Some(event.period),
                ..UpcomingEvent::new(
                    UpcomingKind::Results,
                    event.report_date,
                    Some(asset.clone()),
                )
            });
        }
        if let Some((date, kind)) = dividend {
            out.push(UpcomingEvent::new(kind, date, Some(asset)));
        }
    }
    for entry in journal::list(tx, loupist_id)? {
        if entry.archived || entry.outcome.is_some() {
            continue;
        }
        let Some(check) = &entry.check else { continue };
        let Some(date) = check.by_date.as_deref().and_then(observe::parse_day) else {
            continue;
        };
        if !ahead(date) {
            continue;
        }
        let page = entry
            .thesis
            .as_ref()
            .and_then(|thesis| pages.iter().find(|page| page.asset_id == thesis.asset_id));
        let asset = page.map(|page| entities.asset(page)).transpose()?;
        let claim = if check.claim.trim().is_empty() {
            &entry.judgment
        } else {
            &check.claim
        };
        out.push(UpcomingEvent {
            journal_id: Some(entry.journal_id.clone()),
            claim: Some(bounded_text(claim, MAX_CLAIM_CHARS)),
            ..UpcomingEvent::new(UpcomingKind::Check, date, asset)
        });
    }
    out.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
    out.truncate(MAX_UPCOMING);
    Ok(out)
}
