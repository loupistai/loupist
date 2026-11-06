//! What the user follows, as Sources each Loupist reads. The ordinary World
//! job reads them, so followed assets arrive on the Map through the same growth
//! log as everything else. Names, symbols and markets only: holdings never carry
//! quantities or values here (D9).
//!
//! Sources are immutable, so each change is a new small Source listing only the
//! assets that no earlier "What you follow" Source of this Loupist lists.
//! Unfollowing removes nothing.

use std::collections::BTreeSet;

use rusqlite::{params, Transaction};

use super::graph_jobs::{self, GraphProcessingSettings, WorldAnchor};
use super::model::{
    Authorship, Coverage, RepresentationRole, RetainMaterial, RetainNewSource, Selector,
    SourceClass,
};
use super::{KnowledgeResult, KnowledgeWrite};

/// The stored title. Like World text it stays English; the interface shows
/// it in its own language through [`FOLLOWED_TITLE_KEY`].
pub const FOLLOWED_SOURCE_TITLE: &str = "What you follow";
/// Marks the Sources the host writes from the watchlist and holdings. Nothing
/// else writes this provider, so it identifies them whatever their title.
pub const FOLLOWED_PROVIDER: &str = "loupist:followed";
/// The display key the interface translates for these Sources' title and origin.
pub const FOLLOWED_TITLE_KEY: &str = "followed";
const FOLLOWED_ORIGIN: &str = "Your watchlist and holdings";
/// The worker ensures at most 24 anchors per episode, so each Source lists at
/// most that many assets and every listed asset is anchored.
pub const FOLLOWED_PER_SOURCE: usize = 24;
/// Settings keep at most 200 anchors.
const MAX_ANCHORS: usize = 200;

/// The interface key for a Source's title, when the host wrote it.
pub fn title_key(provider_identity: &str) -> Option<&'static str> {
    (provider_identity == FOLLOWED_PROVIDER).then_some(FOLLOWED_TITLE_KEY)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedAsset {
    pub name: String,
    pub symbol: String,
    pub market: String,
}

impl FollowedAsset {
    /// The display name, or the symbol when the user gave none.
    pub fn new(symbol: &str, market: &str, display_name: Option<&str>) -> Self {
        let symbol = symbol.trim().to_owned();
        let name = display_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&symbol)
            .to_owned();
        Self {
            name,
            symbol,
            market: market.trim().to_owned(),
        }
    }

    fn key(&self) -> (String, String) {
        key(&self.symbol, &self.market)
    }

    /// The market, unless the user gave none (onboarding saves `UNKNOWN`).
    fn known_market(&self) -> Option<&str> {
        Some(self.market.as_str())
            .filter(|market| !market.is_empty() && !market.eq_ignore_ascii_case(UNKNOWN_MARKET))
    }

    /// `SYMBOL:MARKET`, or the symbol alone when the market is unknown.
    fn identifier(&self) -> String {
        match self.known_market() {
            Some(market) => format!("{}:{market}", self.symbol),
            None => self.symbol.clone(),
        }
    }
}

pub(crate) const UNKNOWN_MARKET: &str = "UNKNOWN";

/// One asset per symbol and market; a missing market is `UNKNOWN`.
fn key(symbol: &str, market: &str) -> (String, String) {
    let market = market.trim().to_uppercase();
    (
        symbol.trim().to_uppercase(),
        if market.is_empty() {
            UNKNOWN_MARKET.to_owned()
        } else {
            market
        },
    )
}

/// Watched assets first, then held ones; one entry per symbol and market.
pub fn followed_assets(
    watching: impl IntoIterator<Item = FollowedAsset>,
    holding: impl IntoIterator<Item = FollowedAsset>,
) -> Vec<FollowedAsset> {
    let mut seen = BTreeSet::new();
    watching
        .into_iter()
        .chain(holding)
        .filter(|asset| !asset.symbol.is_empty())
        .filter(|asset| seen.insert(asset.key()))
        .take(MAX_ANCHORS)
        .collect()
}

/// The Source text: one line per asset, nothing about amounts or positions.
/// The first line says what every line is, so a symbol the model does not
/// recognize is still read as one tradable asset with its ticker.
pub fn followed_text(assets: &[FollowedAsset]) -> String {
    let mut text = String::from(FOLLOWED_LEAD);
    for asset in assets {
        text.push_str(&format!("- {} ({})\n", asset.name, asset.identifier()));
    }
    text
}

const FOLLOWED_LEAD: &str = "Assets the user follows. Each line is one asset (a company's shares, a fund, a crypto asset, a commodity or another instrument) with its ticker in parentheses, written TICKER:MARKET when the market is known.\n";

/// The symbol and market of every asset a [`followed_text`] lists.
fn listed_in(text: &str) -> impl Iterator<Item = (String, String)> + '_ {
    text.lines().filter_map(|line| {
        let entry = line.strip_prefix("- ")?;
        let (_, identity) = entry.rsplit_once(" (")?;
        let identity = identity.strip_suffix(')')?;
        // Retained Sources are immutable; recognize their earlier comma form
        // too so opening an existing Loupist never re-lists the same assets.
        let (symbol, market) = identity
            .split_once(", ")
            .or_else(|| identity.rsplit_once(':'))
            .unwrap_or((identity, ""));
        Some(key(symbol, market))
    })
}

fn followed_source(loupist_id: &str, assets: &[FollowedAsset], at_ms: i64) -> RetainNewSource {
    let bytes = followed_text(assets).into_bytes();
    RetainNewSource {
        origin: None,
        loupist_id: loupist_id.to_owned(),
        at_ms,
        title: FOLLOWED_SOURCE_TITLE.to_owned(),
        source_class: SourceClass::UserStatement,
        origin_locator: FOLLOWED_ORIGIN.to_owned(),
        provider_identity: FOLLOWED_PROVIDER.to_owned(),
        authorship: Authorship::User,
        coverage: Coverage::IncludedFull,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time: None,
        material: Some(RetainMaterial {
            media_type: "text/plain".to_owned(),
            role: RepresentationRole::Original,
            document_id: "followed-assets".to_owned(),
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: Vec::new(),
            selector: Selector::ByteRange {
                start: 0,
                end: bytes.len() as u64,
            },
            context_note: "The assets the user follows, by name".to_owned(),
            bytes,
        }),
    }
}

/// Each followed asset becomes a World anchor: the worker puts it in the index
/// before extraction, so the Source's names resolve to one node each.
pub fn anchor_followed(settings: &mut GraphProcessingSettings, assets: &[FollowedAsset]) {
    for asset in assets {
        if settings.anchors.len() >= MAX_ANCHORS {
            break;
        }
        if settings
            .anchors
            .iter()
            .any(|anchor| anchor.name.eq_ignore_ascii_case(&asset.name))
        {
            continue;
        }
        let mut identifiers = vec![asset.symbol.clone()];
        if asset.known_market().is_some() {
            identifiers.push(asset.identifier());
        }
        // Settings bound anchor names to 200 bytes.
        let mut name = String::new();
        for character in asset.name.chars() {
            if name.len() + character.len_utf8() > 200 {
                break;
            }
            name.push(character);
        }
        settings.anchors.push(WorldAnchor {
            name,
            aliases: Vec::new(),
            identifiers,
        });
    }
}

/// This Loupist's "What you follow" Sources with their current revision, oldest first.
fn followed_records(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Vec<(String, String)>> {
    let mut statement = tx.prepare(
        "SELECT r.record_id, h.revision_id FROM knowledge_records r
         JOIN knowledge_sources s ON s.record_id = r.record_id
         JOIN knowledge_record_heads h ON h.record_id = r.record_id
         WHERE r.owner_loupist_id = ?1 AND r.kind = 'source' AND s.provider_identity = ?2
         ORDER BY r.created_at_ms, r.record_id",
    )?;
    let rows = statement.query_map(params![loupist_id, FOLLOWED_PROVIDER], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

/// Record ids of this Loupist's "What you follow" Sources.
pub(crate) fn followed_record_ids(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeSet<String>> {
    Ok(followed_records(tx, loupist_id)?
        .into_iter()
        .map(|(record_id, _)| record_id)
        .collect())
}

impl KnowledgeWrite<'_> {
    /// Every asset an earlier "What you follow" Source of this Loupist lists.
    fn listed_followed(&self, loupist_id: &str) -> KnowledgeResult<BTreeSet<(String, String)>> {
        let mut listed = BTreeSet::new();
        for (record_id, revision_id) in followed_records(self.tx, loupist_id)? {
            let spans: Vec<String> = self
                .tx
                .prepare("SELECT span_id FROM knowledge_source_spans WHERE record_id = ?1")?
                .query_map([&record_id], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            for span_id in spans {
                let material = super::source::load_revision_span(
                    self.tx,
                    self.blobs,
                    loupist_id,
                    &record_id,
                    &revision_id,
                    &span_id,
                )?;
                if let Ok(text) = std::str::from_utf8(&material.bytes) {
                    listed.extend(listed_in(text));
                }
            }
        }
        Ok(listed)
    }

    /// Brings this Loupist's "What you follow" Sources up to date with `assets`:
    /// the assets no earlier one lists go into new Sources of at most
    /// [`FOLLOWED_PER_SOURCE`] each, anchored and queued as ordinary World jobs.
    /// Returns how many World jobs were queued.
    pub fn sync_followed(
        &self,
        loupist_id: &str,
        assets: &[FollowedAsset],
        at_ms: i64,
    ) -> KnowledgeResult<usize> {
        let listed = self.listed_followed(loupist_id)?;
        let new: Vec<FollowedAsset> = assets
            .iter()
            .filter(|asset| !listed.contains(&asset.key()))
            .cloned()
            .collect();
        if new.is_empty() {
            return Ok(0);
        }
        let mut settings = graph_jobs::load_settings(self.tx, loupist_id)?;
        anchor_followed(&mut settings, &new);
        graph_jobs::store_settings(self.tx, loupist_id, &settings)?;
        let mut queued = 0;
        for batch in new.chunks(FOLLOWED_PER_SOURCE) {
            let saved = self.retain_new_source(followed_source(loupist_id, batch, at_ms))?;
            if self
                .enqueue_source_world(loupist_id, &saved, &settings, at_ms)?
                .is_some()
            {
                queued += 1;
            }
        }
        Ok(queued)
    }

    /// Where reading what the user follows stands, from the jobs of this
    /// Loupist's "What you follow" Sources.
    pub fn followed_status(&self, loupist_id: &str) -> KnowledgeResult<FollowedStatus> {
        let records = followed_records(self.tx, loupist_id)?;
        let assets = self.listed_followed(loupist_id)?.len();
        let mut status = FollowedStatus {
            state: FollowedState::None,
            assets,
            queued_at_ms: None,
        };
        let mut states = Vec::new();
        for (record_id, _) in &records {
            let processing = self.source_processing(loupist_id, record_id)?;
            if matches!(processing.status.as_str(), "queued" | "running") {
                let job = graph_jobs::latest_source_job(self.tx, loupist_id, record_id)?;
                if let Some(job) = job {
                    status.queued_at_ms = Some(
                        status
                            .queued_at_ms
                            .map_or(job.created_at_ms, |at| at.min(job.created_at_ms)),
                    );
                }
            }
            states.push(FollowedState::of(&processing.status));
        }
        status.state = states.into_iter().max().unwrap_or(FollowedState::None);
        Ok(status)
    }
}

/// Ordered so that the most pressing state of several Sources wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowedState {
    /// Nothing followed was written.
    None,
    /// World processing is off or paused, so nothing will read it now.
    Off,
    /// Every Source was read.
    Done,
    /// A read failed or was interrupted and will not be retried by itself.
    Failed,
    /// Written, but World processing cannot read it yet (no model).
    Waiting,
    Queued,
    Running,
}

impl FollowedState {
    fn of(processing: &str) -> Self {
        match processing {
            graph_jobs::JOB_RUNNING => Self::Running,
            graph_jobs::JOB_QUEUED => Self::Queued,
            graph_jobs::JOB_COMPLETED => Self::Done,
            "disabled" | "paused" | graph_jobs::JOB_PAUSED | "no_text" => Self::Off,
            "not_configured" | "saved" => Self::Waiting,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowedStatus {
    pub state: FollowedState,
    /// How many assets the Sources list.
    pub assets: usize,
    /// When the oldest unfinished read was queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_ms: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_names_one_asset_and_reads_back_to_its_symbol_and_market() {
        let assets = [
            FollowedAsset::new("AAPL", "NASDAQ", Some("Apple")),
            // A manual entry: no name, and onboarding saves no market as UNKNOWN.
            FollowedAsset::new("QUAYX", "UNKNOWN", None),
            FollowedAsset::new("GOOGL", "NASDAQ", Some("Alphabet (Class A)")),
            FollowedAsset::new("005930", "KRX", Some("삼성전자")),
            FollowedAsset::new("btc", "crypto", Some("Bitcoin")),
        ];
        let text = followed_text(&assets);
        assert_eq!(
            text,
            format!(
                "{FOLLOWED_LEAD}- Apple (AAPL:NASDAQ)\n- QUAYX (QUAYX)\n- Alphabet (Class A) (GOOGL:NASDAQ)\n- 삼성전자 (005930:KRX)\n- Bitcoin (btc:crypto)\n"
            )
        );
        let listed: Vec<_> = listed_in(&text).collect();
        let expected: Vec<_> = assets.iter().map(FollowedAsset::key).collect();
        assert_eq!(listed, expected);
        assert_eq!(expected[1], ("QUAYX".to_owned(), "UNKNOWN".to_owned()));
        // Anchors carry the symbol, and the market only when the user gave one.
        let mut settings = GraphProcessingSettings::default();
        anchor_followed(&mut settings, &assets);
        let identifiers: Vec<_> = settings
            .anchors
            .iter()
            .map(|anchor| anchor.identifiers.join(" "))
            .collect();
        assert_eq!(
            identifiers,
            [
                "AAPL AAPL:NASDAQ",
                "QUAYX",
                "GOOGL GOOGL:NASDAQ",
                "005930 005930:KRX",
                "btc btc:crypto"
            ]
        );
    }

    #[test]
    fn earlier_and_current_sources_list_the_same_assets() {
        let earlier = "- Apple (AAPL, NASDAQ)\n- QUAYX (QUAYX)\n- Alphabet (Class A) (GOOGL, NASDAQ)\n- 삼성전자 (005930, KRX)\n";
        let current = "- Apple (AAPL:NASDAQ)\n- QUAYX (QUAYX)\n- Alphabet (Class A) (GOOGL:NASDAQ)\n- 삼성전자 (005930:KRX)\n";
        assert_eq!(
            listed_in(earlier).collect::<Vec<_>>(),
            listed_in(current).collect::<Vec<_>>()
        );
    }

    #[test]
    fn several_sources_report_the_most_pressing_read() {
        use FollowedState::*;
        for (states, expected) in [
            (vec![Done, Running], Running),
            (vec![Failed, Queued], Queued),
            (vec![Done, Failed], Failed),
            (vec![Off, Done], Done),
            (vec![Waiting, Failed], Waiting),
        ] {
            assert_eq!(states.into_iter().max(), Some(expected));
        }
        for (job, expected) in [
            ("queued", Queued),
            ("running", Running),
            ("completed", Done),
            ("failed_before_mutation", Failed),
            ("reconcile_required", Failed),
            ("reconciled_without_retry", Failed),
            ("cancelled_before_start", Failed),
            ("paused_budget", Off),
            ("disabled", Off),
            ("paused", Off),
            ("not_configured", Waiting),
            ("saved", Waiting),
        ] {
            assert_eq!(FollowedState::of(job), expected, "{job}");
        }
    }
}
