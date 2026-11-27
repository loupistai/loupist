use std::collections::BTreeSet;

use super::connections::{self, Candidate, DetectInput, Hop};
use super::error::{KnowledgeError, KnowledgeResult};
use super::identity::{resolve_followed, IdentityMatch};
use super::model::{
    mint_id, validate_token, ClaimStatus, TimePoint, WorldSnapshotBody, END_BASIS,
    SUPERSEDED_BY_CLAIM, SUPERSEDED_BY_SPAN,
};
use super::revision::StoredRevision;
use super::world_episodes::{EndBasis, EpisodeEnd};
use crate::store::user_context::MapContextItem;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[cfg(test)]
const LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    New,
    Shown,
    Opened,
    Dismissed,
    Filed,
}

impl ConnectionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Shown => "shown",
            Self::Opened => "opened",
            Self::Dismissed => "dismissed",
            Self::Filed => "filed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "new" => Some(Self::New),
            "shown" => Some(Self::Shown),
            "opened" => Some(Self::Opened),
            "dismissed" => Some(Self::Dismissed),
            "filed" => Some(Self::Filed),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPath {
    seed_name: String,
    hops: Vec<Hop>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub connection_id: String,
    pub seed_node_id: String,
    pub far_node_id: String,
    pub seed_name: String,
    pub far_name: String,
    pub path: Vec<Hop>,
    pub score: i64,
    pub status: ConnectionStatus,
    pub created_at_ms: i64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Followups {
    pub contested_pages: usize,
    pub superseded_pages: usize,
    pub resolved_anchors: usize,
    pub connections: usize,
    pub truncated: bool,
}

fn fallen_claims(
    previous: Option<&WorldSnapshotBody>,
    next: &WorldSnapshotBody,
) -> BTreeSet<String> {
    let Some(previous) = previous else {
        return BTreeSet::new();
    };
    let next_status: std::collections::HashMap<_, _> = next
        .claims
        .iter()
        .map(|claim| (claim.id.as_str(), claim.status))
        .collect();
    let mut fallen = BTreeSet::new();
    for before in &previous.claims {
        match next_status.get(before.id.as_str()) {
            None if before.status != ClaimStatus::Withdrawn => {
                fallen.insert(before.id.clone());
            }
            Some(after) if *after != before.status && *after != ClaimStatus::Active => {
                fallen.insert(before.id.clone());
            }
            _ => {}
        }
    }
    fallen
}

fn idea_seeds(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<String>> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT a.node_id FROM knowledge_lens_anchors a
         JOIN knowledge_lens_metadata m ON m.record_id = a.record_id
         JOIN knowledge_record_heads h ON h.record_id = m.record_id
         JOIN knowledge_record_revisions v ON v.revision_id = h.revision_id
         WHERE a.loupist_id = ?1 AND a.node_id IS NOT NULL
           AND m.kind = 'idea' AND m.archived_at_ms IS NULL
           AND json_extract(v.payload_json, '$.idea.status') = 'open'
         ORDER BY a.node_id",
    )?;
    let rows = statement.query_map([loupist_id], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

fn validity_changes(
    previous: Option<&WorldSnapshotBody>,
    next: &WorldSnapshotBody,
    at_ms: i64,
) -> BTreeSet<String> {
    let at = TimePoint::from_unix_ms(at_ms);
    let prior: std::collections::BTreeMap<_, _> = previous
        .into_iter()
        .flat_map(|body| &body.claims)
        .map(|claim| (claim.id.as_str(), claim))
        .collect();
    next.claims
        .iter()
        .filter(|claim| {
            if claim.status != ClaimStatus::Active {
                return false;
            }
            let Some(before) = prior.get(claim.id.as_str()) else {
                return !claim.is_current(&at);
            };
            before.is_current(&at) != claim.is_current(&at)
                || (claim.has_ended(&at)
                    && (claim.invalid_at != before.invalid_at
                        || claim.qualifier(SUPERSEDED_BY_CLAIM)
                            != before.qualifier(SUPERSEDED_BY_CLAIM)
                        || claim.qualifier(SUPERSEDED_BY_SPAN)
                            != before.qualifier(SUPERSEDED_BY_SPAN)))
        })
        .map(|claim| claim.id.clone())
        .collect()
}

pub(super) fn last_effective_end(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Option<i64>> {
    Ok(tx.query_row(
        "SELECT MAX(at_ms) FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND record_id = ?2 AND kind = 'claim_ended'
           AND origin -> 'effectiveEnd' IS NOT NULL",
        params![loupist_id, record_id],
        |row| row.get(0),
    )?)
}

/// Reconcile dates crossed on the still-published World head. The recorded
/// transitions are also the watermark for the next evaluation of this head.
pub(super) fn reconcile_effective_ends(
    tx: &Transaction<'_>,
    loupist_id: &str,
    revision: &StoredRevision,
    world: &WorldSnapshotBody,
    at_ms: i64,
) -> KnowledgeResult<()> {
    let from_ms = last_effective_end(tx, loupist_id, &revision.record_id)?
        .unwrap_or(0)
        .max(revision.created_at_ms);
    if at_ms <= from_ms {
        return Ok(());
    }
    let from = TimePoint::from_unix_ms(from_ms);
    let at = TimePoint::from_unix_ms(at_ms);
    let mut ending_ids = BTreeSet::new();
    for claim in &world.claims {
        let Some(end) = claim.invalid_at.as_ref().filter(|end| {
            claim.status == ClaimStatus::Active && !end.at_or_before(&from) && end.at_or_before(&at)
        }) else {
            continue;
        };
        ending_ids.insert(claim.id.clone());
        let mut event = super::growth::GrowthEvent::new(
            loupist_id,
            &revision.record_id,
            at_ms,
            "claim_ended",
            "processing",
            &claim.proposition,
        );
        event.subject_id = Some(claim.id.clone());
        event.publication = Some(super::growth::WorldGrowthPublication {
            revision_id: revision.revision_id.clone(),
            source: None,
            fact_subject_id: if claim.connects_names() {
                None
            } else {
                claim.endpoints.first().map(|end| end.node_id.clone())
            },
        });
        event.origin = Some(serde_json::json!({
            "effectiveEnd": EpisodeEnd {
                claim: claim.id.clone(),
                by: claim.qualifier(SUPERSEDED_BY_CLAIM).map(str::to_owned),
                end: end.clone(),
                basis: if claim.qualifier(END_BASIS) == Some("reference") {
                    EndBasis::Reference
                } else {
                    EndBasis::Stated
                },
            },
            "spanId": claim.qualifier(SUPERSEDED_BY_SPAN),
        }));
        event.insert(tx)?;
    }
    if !ending_ids.is_empty() {
        super::lens::supersede_pages(tx, loupist_id, &ending_ids, &BTreeSet::new(), at_ms)?;
    }
    Ok(())
}

pub(crate) fn reconcile_due_endings(
    tx: &Transaction<'_>,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<Option<i64>> {
    let Some(world) = super::catalog::world_record(tx, loupist_id)? else {
        return Ok(None);
    };
    let Some(revision_id) = world.revision_id else {
        return Ok(None);
    };
    let revision = super::revision::load_revision(tx, &revision_id)?;
    let body: WorldSnapshotBody = super::canonical::from_json(&revision.payload_json)?;
    reconcile_effective_ends(tx, loupist_id, &revision, &body, at_ms)?;
    // Calendar bounds are checked by the existing local-midnight wake.
    let next = body
        .claims
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Active)
        .filter_map(|claim| claim.invalid_at.as_ref())
        .filter(|end| end.precision == super::TimePrecision::Instant)
        .filter_map(|end| chrono::DateTime::parse_from_rfc3339(end.value.as_deref()?).ok())
        .map(|instant| instant.timestamp_millis())
        .filter(|time| *time > at_ms)
        .min();
    Ok(next)
}

pub(super) fn reconcile_worlds(tx: &Transaction<'_>, at_ms: i64) -> KnowledgeResult<Option<i64>> {
    let mut statement =
        tx.prepare("SELECT owner_loupist_id FROM knowledge_records WHERE kind = 'world'")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut next = None;
    for id in ids {
        next = next
            .into_iter()
            .chain(reconcile_due_endings(tx, &id, at_ms)?)
            .min();
    }
    Ok(next)
}

fn recorded_pairs(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeSet<(String, String)>> {
    let mut statement = tx.prepare(
        "SELECT seed_node_id, far_node_id FROM knowledge_connections WHERE loupist_id = ?1",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

fn insert(
    tx: &Transaction<'_>,
    loupist_id: &str,
    candidate: &Candidate,
    at_ms: i64,
) -> KnowledgeResult<usize> {
    let path = StoredPath {
        seed_name: candidate.seed_name.clone(),
        hops: candidate.hops.clone(),
    };
    Ok(tx.execute(
        "INSERT OR IGNORE INTO knowledge_connections
            (connection_id, loupist_id, seed_node_id, far_node_id, path_json, score, status, created_at_ms, status_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'new', ?7, ?7)",
        params![
            mint_id("con"),
            loupist_id,
            candidate.seed_node_id,
            candidate.far_node_id,
            serde_json::to_string(&path)?,
            candidate.score,
            at_ms
        ],
    )?)
}

pub(super) fn after_seal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    previous: Option<&WorldSnapshotBody>,
    next: &WorldSnapshotBody,
    followed: &[MapContextItem],
    at_ms: i64,
) -> KnowledgeResult<Followups> {
    let fallen = fallen_claims(previous, next);
    let changed_endings = validity_changes(previous, next, at_ms);
    let (contested_pages, superseded_pages) = if changed_endings.is_empty() {
        (
            super::lens::contest_pages(tx, loupist_id, &fallen, at_ms)?,
            0,
        )
    } else {
        super::lens::supersede_pages(tx, loupist_id, &changed_endings, &fallen, at_ms)?
    };
    // Who the filed pages are about may have changed: a pending anchor gained
    // a node, a confirmed node's name or aliases changed (learning keys its
    // cases by the World's current spellings), or a page still pending may
    // now be confirmed for learning by the Sources it cites.
    let resolved_anchors = super::lens::resolve_anchors(tx, loupist_id, &next.nodes)?;
    if resolved_anchors > 0
        || super::lens::subject_spellings_changed(tx, loupist_id, previous, next)?
        || super::lens::pending_subjects_exist(tx, loupist_id)?
    {
        super::learning::identity_changed(tx, loupist_id, at_ms)?;
    }
    let mut seeds: Vec<String> = followed
        .iter()
        .filter_map(|item| match resolve_followed(item, &next.nodes) {
            IdentityMatch::Exact(id) => Some(id),
            IdentityMatch::Ambiguous(_) | IdentityMatch::Unmatched => None,
        })
        .collect();
    seeds.extend(idea_seeds(tx, loupist_id)?);
    seeds.sort();
    seeds.dedup();
    if seeds.is_empty() {
        return Ok(Followups {
            contested_pages,
            superseded_pages,
            resolved_anchors,
            ..Followups::default()
        });
    }
    let extension = super::graph_jobs::load_settings(tx, loupist_id)?.extension;
    let category_of = |kind: &str| -> &'static str {
        extension
            .types
            .iter()
            .find(|item| item.name == kind)
            .map(|item| match item.category {
                super::world_schema::ExtensionCategory::Company => "company",
                super::world_schema::ExtensionCategory::Person => "person",
                super::world_schema::ExtensionCategory::Product => "product",
                super::world_schema::ExtensionCategory::Market => "market",
                super::world_schema::ExtensionCategory::Event => "event",
                super::world_schema::ExtensionCategory::Other => "other",
            })
            .unwrap_or_else(|| connections::core_category(kind))
    };
    let already = recorded_pairs(tx, loupist_id)?;
    let detection = connections::detect(&DetectInput {
        previous,
        next,
        seeds: &seeds,
        already: &already,
        category_of: &category_of,
        at_ms,
    });
    let mut inserted = 0;
    for candidate in &detection.candidates {
        inserted += insert(tx, loupist_id, candidate, at_ms)?;
    }
    Ok(Followups {
        contested_pages,
        superseded_pages,
        resolved_anchors,
        connections: inserted,
        truncated: detection.truncated,
    })
}

fn view_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionView> {
    let path_json: String = row.get(4)?;
    let status: String = row.get(6)?;
    let path: StoredPath = serde_json::from_str(&path_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ConnectionView {
        connection_id: row.get(0)?,
        seed_node_id: row.get(2)?,
        far_node_id: row.get(3)?,
        seed_name: path.seed_name,
        far_name: path
            .hops
            .last()
            .map(|hop| hop.name.clone())
            .unwrap_or_default(),
        path: path.hops,
        score: row.get(5)?,
        status: ConnectionStatus::parse(&status).ok_or_else(|| rusqlite::Error::InvalidQuery)?,
        created_at_ms: row.get(7)?,
    })
}

const COLUMNS: &str =
    "connection_id, loupist_id, seed_node_id, far_node_id, path_json, score, status, created_at_ms";

#[cfg(test)]
pub(super) fn list(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<ConnectionView>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM knowledge_connections WHERE loupist_id = ?1
         ORDER BY created_at_ms DESC, score DESC, connection_id LIMIT ?2"
    ))?;
    let rows = statement.query_map(params![loupist_id, LIST_LIMIT as i64], view_from_row)?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

pub(crate) fn connection_in_loupist(
    connection: &Connection,
    loupist_id: &str,
    connection_id: &str,
) -> KnowledgeResult<Option<ConnectionView>> {
    validate_token(connection_id, 128, "connection id is invalid")?;
    Ok(connection
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM knowledge_connections
                      WHERE loupist_id = ?1 AND connection_id = ?2"
            ),
            params![loupist_id, connection_id],
            view_from_row,
        )
        .optional()?)
}

pub(crate) fn set_status(
    tx: &Transaction<'_>,
    loupist_id: &str,
    connection_id: &str,
    status: ConnectionStatus,
    at_ms: i64,
) -> KnowledgeResult<ConnectionView> {
    validate_token(connection_id, 128, "connection id is invalid")?;
    let changed = tx.execute(
        "UPDATE knowledge_connections SET status = ?3, status_at_ms = ?4
         WHERE loupist_id = ?1 AND connection_id = ?2",
        params![loupist_id, connection_id, status.as_str(), at_ms],
    )?;
    if changed == 0 {
        return Err(KnowledgeError::NotFound);
    }
    connection_in_loupist(tx, loupist_id, connection_id)?.ok_or(KnowledgeError::NotFound)
}

/// Bot delivery is disabled until the caller can prove current personal relevance.
/// The Map keeps every detected relation. This transaction has no app-level
/// following/holdings snapshot, and recorded paths alone cannot establish that
/// both ends are followed or that the far end is a dated event.
pub(crate) fn to_show(
    _tx: &Transaction<'_>,
    _loupist_id: &str,
    _at_ms: i64,
) -> KnowledgeResult<Vec<ConnectionView>> {
    Ok(Vec::new())
}

pub(crate) fn mark_shown(
    tx: &Transaction<'_>,
    loupist_id: &str,
    connection_ids: &[String],
    at_ms: i64,
) -> KnowledgeResult<()> {
    for connection_id in connection_ids {
        tx.execute(
            "UPDATE knowledge_connections SET status = 'shown', status_at_ms = ?3, shown_at_ms = ?3
             WHERE loupist_id = ?1 AND connection_id = ?2 AND status = 'new'",
            params![loupist_id, connection_id, at_ms],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::knowledge::lens_page::{
        AnchorInput, Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget,
        SectionKey,
    };
    use crate::knowledge::model::{
        ClaimEndpoint, CreateRecord, GraphClaim, GraphNode, RecordKind, SealWorld,
    };
    use crate::knowledge::KnowledgeStore;

    const LOUPIST: &str = "loupist-a";

    fn open_store() -> (tempfile::TempDir, KnowledgeStore) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).expect("open");
        (directory, store)
    }

    fn node(id: &str, name: &str, kind: &str, ticker: Option<(&str, &str)>) -> GraphNode {
        let mut properties = BTreeMap::new();
        if let Some((symbol, exchange)) = ticker {
            properties.insert("ticker".to_owned(), json!(symbol));
            properties.insert("exchange".to_owned(), json!(exchange));
        }
        GraphNode {
            id: id.into(),
            name: name.into(),
            aliases: Vec::new(),
            description: String::new(),
            kind: kind.into(),
            source_role: "entity".into(),
            properties,
        }
    }

    fn claim(id: &str, from: &str, to: &str) -> GraphClaim {
        GraphClaim {
            id: id.into(),
            proposition: format!("{from} supplies {to}"),
            attribution: "test".into(),
            relation: "SUPPLIES".into(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: from.into(),
                    role: "supplier".into(),
                },
                ClaimEndpoint {
                    node_id: to.into(),
                    role: "customer".into(),
                },
            ],
            qualifiers: Vec::new(),
            evidence: Vec::new(),
            status: ClaimStatus::Active,
            recorded_at_ms: Some(1_000),
            publication_time: None,
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        }
    }

    fn item(symbol: &str, market: &str) -> MapContextItem {
        MapContextItem {
            asset_id: crate::knowledge::assets::asset_id(symbol, market),
            symbol: symbol.into(),
            market: market.into(),
            display_name: None,
        }
    }

    fn world(store: &KnowledgeStore) -> String {
        store
            .create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 1,
            })
            .unwrap()
            .record_id
    }

    fn seal(
        store: &KnowledgeStore,
        record_id: &str,
        nodes: Vec<GraphNode>,
        claims: Vec<GraphClaim>,
        followed: &[MapContextItem],
        at_ms: i64,
    ) {
        store
            .transact(|write| {
                let head = crate::knowledge::catalog::owned_record(write.tx, LOUPIST, record_id)?
                    .revision_id;
                write.seal_world_followed(
                    SealWorld {
                        loupist_id: LOUPIST.into(),
                        record_id: record_id.into(),
                        expected_revision_id: head,
                        at_ms,
                        nodes,
                        claims,
                        annotations: Vec::new(),
                        dependencies: Vec::new(),
                    },
                    followed,
                )
            })
            .unwrap();
    }

    fn connections(store: &KnowledgeStore) -> Vec<ConnectionView> {
        store.read(|write| write.connections(LOUPIST)).unwrap()
    }

    fn base_nodes() -> Vec<GraphNode> {
        vec![
            node("a", "Alpha", "Organization", Some(("A", "KRX"))),
            node("b", "Beta", "Organization", None),
        ]
    }

    #[test]
    fn fallen_claims_preserves_status_transitions_and_missing_claims_at_world_limit() {
        let previous = WorldSnapshotBody {
            nodes: Vec::new(),
            claims: (0..10_000)
                .map(|index| {
                    let mut item = claim(&format!("c{index}"), "a", "b");
                    if index == 0 {
                        item.status = ClaimStatus::Withdrawn;
                    }
                    if index == 1 {
                        item.status = ClaimStatus::Disputed;
                    }
                    item
                })
                .collect(),
            annotations: Vec::new(),
            relation_vocabulary: Vec::new(),
            temporal: Default::default(),
        };
        let mut next = previous.clone();
        next.claims
            .retain(|item| item.id != "c0" && item.id != "c2");
        next.claims[0].status = ClaimStatus::Active;
        next.claims[1].status = ClaimStatus::Disputed;
        next.claims.reverse();
        assert_eq!(
            fallen_claims(Some(&previous), &next),
            BTreeSet::from(["c2".into(), "c3".into()])
        );
        assert!(fallen_claims(None, &next).is_empty());
    }

    #[test]
    fn a_publication_records_one_link_per_pair_and_never_twice() {
        let (_directory, store) = open_store();
        let record = world(&store);
        let followed = [item("A", "KRX")];
        seal(
            &store,
            &record,
            base_nodes(),
            vec![claim("c1", "a", "b")],
            &followed,
            10,
        );
        assert!(connections(&store).is_empty(), "nothing new yet");
        let mut nodes = base_nodes();
        nodes.push(node("c", "Gamma", "Technology", None));
        let claims = vec![claim("c1", "a", "b"), claim("c2", "b", "c")];
        seal(
            &store,
            &record,
            nodes.clone(),
            claims.clone(),
            &followed,
            20,
        );
        let found = connections(&store);
        assert_eq!(found.len(), 1);
        let link = &found[0];
        assert_eq!(
            (link.seed_name.as_str(), link.far_name.as_str()),
            ("Alpha", "Gamma")
        );
        assert_eq!(link.status, ConnectionStatus::New);
        let path: Vec<(&str, &str)> = link
            .path
            .iter()
            .map(|hop| (hop.claim_id.as_str(), hop.node_id.as_str()))
            .collect();
        assert_eq!(path, [("c1", "b"), ("c2", "c")]);
        seal(&store, &record, nodes, claims, &followed, 30);
        assert_eq!(
            connections(&store).len(),
            1,
            "the same snapshot adds no row"
        );
    }

    #[test]
    fn a_followed_symbol_seeds_only_its_market_node() {
        let (_directory, store) = open_store();
        let record = world(&store);
        let nodes = vec![
            node("krx", "Alpha Korea", "Organization", Some(("A", "KRX"))),
            node("nasdaq", "Alpha Inc", "Organization", Some(("A", "NASDAQ"))),
            node("b", "Beta", "Organization", None),
            node("c", "Gamma", "Technology", None),
            node("d", "Delta", "Technology", None),
        ];
        let claims = vec![
            claim("c1", "krx", "b"),
            claim("c2", "b", "c"),
            claim("c3", "nasdaq", "b"),
            claim("c4", "b", "d"),
        ];
        seal(&store, &record, nodes, claims, &[item("A", "KRX")], 10);
        let seeds: BTreeSet<String> = connections(&store)
            .into_iter()
            .map(|link| link.seed_node_id)
            .collect();
        assert_eq!(seeds, BTreeSet::from(["krx".to_owned()]));
    }

    #[test]
    fn extracted_profile_relations_stay_on_the_map_without_bot_posts() {
        let (_directory, store) = open_store();
        let record = world(&store);
        let mut nodes = base_nodes();
        let mut claims = vec![claim("c1", "a", "b")];
        for far in ["Quadro GPUs", "NVIDIA RTX GPUs", "Autonomous vehicles"] {
            let id = far.replace(' ', "-");
            nodes.push(node(&id, far, "Product", None));
            claims.push(claim(&format!("c-{id}"), "b", &id));
        }
        seal(&store, &record, nodes, claims, &[item("A", "KRX")], 10);
        assert_eq!(connections(&store).len(), 3, "Map discovery is retained");
        for at in [20, 86_400_020] {
            assert!(store
                .read(|read| to_show(read.tx, LOUPIST, at))
                .unwrap()
                .is_empty());
        }
        assert!(connections(&store)
            .iter()
            .all(|link| link.status == ConnectionStatus::New));
    }

    #[test]
    fn a_withdrawn_cited_claim_contests_the_section_once() {
        let (_directory, store) = open_store();
        let record = world(&store);
        seal(
            &store,
            &record,
            base_nodes(),
            vec![claim("c1", "a", "b")],
            &[],
            10,
        );
        let filed = store
            .transact(|write| {
                write.file(
                    LOUPIST,
                    &FilingOrigin::User {
                        chat_id: None,
                        message_id: None,
                    },
                    vec![PageDelta {
                        target: PageTarget::Create {
                            kind: PageKind::Subject,
                            title: "Alpha".into(),
                            anchors: vec![AnchorInput::Node {
                                node_id: "a".into(),
                            }],
                        },
                        ops: vec![LensOp::Put {
                            key: SectionKey::new("facts").unwrap(),
                            heading: "Facts".into(),
                            markdown: "Alpha supplies Beta.".into(),
                            basis: Basis {
                                refs: vec![BasisRef::Claim {
                                    claim_id: "c1".into(),
                                }],
                                uncertainty: None,
                            },
                            expected: None,
                        }],
                    }],
                    11,
                )
            })
            .unwrap();
        let page_id = filed[0].record_id.clone();
        let mut withdrawn = claim("c1", "a", "b");
        withdrawn.status = ClaimStatus::Withdrawn;
        seal(
            &store,
            &record,
            base_nodes(),
            vec![withdrawn.clone()],
            &[],
            20,
        );
        let page = store.read_page(LOUPIST, &page_id).unwrap();
        let section = &page.body.sections()[0];
        assert!(section.contested);
        assert_eq!(section.contested_claim_ids, ["c1"]);
        // The contested revision is indexed, so the page stays in Lens search.
        let indexed: i64 = store
            .read(|read| {
                Ok(read.tx.query_row(
                    "SELECT COUNT(*) FROM knowledge_search_documents
                     WHERE record_id = ?1 AND revision_id = ?2",
                    rusqlite::params![page_id, page.revision_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert!(indexed > 0, "a contested page left the search index");
        let events = store
            .growth_events(&crate::knowledge::GrowthEventsQuery {
                loupist_id: LOUPIST.into(),
                since_ms: None,
                until_ms: None,
                world_revision_id: None,
                limit: None,
                cursor: None,
            })
            .unwrap()
            .events;
        let contested: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "lens_contested")
            .collect();
        assert_eq!(contested.len(), 1);
        assert_eq!(contested[0].record_id.as_deref(), Some(page_id.as_str()));
        seal(&store, &record, base_nodes(), vec![withdrawn], &[], 30);
        let again = store.read_page(LOUPIST, &page_id).unwrap();
        assert_eq!(again.revision_id, page.revision_id);
        assert!(again.body.sections()[0].contested);
    }

    #[test]
    fn ending_evidence_controls_the_stored_state_in_either_filing_order() {
        use crate::knowledge::lens_page::{LensBody, LensKind, SectionState};
        for end_first in [false, true] {
            for include_ender in [false, true] {
                let (_directory, store) = open_store();
                let record = world(&store);
                let mut prior = claim("cfo-1", "a", "b");
                let current = claim("cfo-2", "a", "b");
                let valid = |prior: &GraphClaim, at| {
                    seal(
                        &store,
                        &record,
                        base_nodes(),
                        vec![prior.clone(), current.clone()],
                        &[],
                        at,
                    );
                };
                valid(&prior, 1_000);
                let end = |prior: &mut GraphClaim, at| {
                    prior.invalid_at = Some(TimePoint::from_unix_ms(5_000));
                    prior.qualifiers.push(crate::knowledge::Qualifier {
                        key: SUPERSEDED_BY_CLAIM.into(),
                        value: current.id.clone(),
                    });
                    valid(prior, at);
                };
                let filed_at = if end_first {
                    end(&mut prior, 7_000);
                    8_000
                } else {
                    6_000
                };
                let mut refs = vec![BasisRef::Claim {
                    claim_id: prior.id.clone(),
                }];
                if include_ender {
                    refs.push(BasisRef::Claim {
                        claim_id: current.id.clone(),
                    });
                }
                let filing = |target, markdown: &str, expected, at| {
                    store
                        .transact(|write| {
                            write.file(
                                LOUPIST,
                                &FilingOrigin::User {
                                    chat_id: None,
                                    message_id: None,
                                },
                                vec![PageDelta {
                                    target,
                                    ops: vec![LensOp::Put {
                                        key: SectionKey::new("facts").unwrap(),
                                        heading: "Facts".into(),
                                        markdown: markdown.into(),
                                        basis: Basis {
                                            refs: refs.clone(),
                                            uncertainty: None,
                                        },
                                        expected,
                                    }],
                                }],
                                at,
                            )
                        })
                        .unwrap()
                        .remove(0)
                        .record_id
                };
                let page_id = filing(
                    PageTarget::Create {
                        kind: PageKind::Subject,
                        title: "Appointment".into(),
                        anchors: Vec::new(),
                    },
                    "The original appointment.",
                    None,
                    filed_at,
                );
                if !end_first {
                    end(&mut prior, 7_000);
                }
                let stored = || {
                    store
                        .read(|read| {
                            let row = read.owned_record(LOUPIST, &page_id)?;
                            let payload = read
                                .get_revision(LOUPIST, row.revision_id.as_deref().unwrap())?
                                .payload;
                            LensBody::from_payload(LensKind::Subject, payload)
                        })
                        .unwrap()
                        .page()
                        .unwrap()
                        .sections[0]
                        .clone()
                };
                let expected = if include_ender {
                    SectionState::Current
                } else {
                    SectionState::Superseded {
                        since_ms: if end_first { 8_000 } else { 7_000 },
                        claim_ids: vec![prior.id.clone()],
                    }
                };
                assert_eq!(stored().state, expected);
                let digest = super::super::lens_page::section_digest(&stored());
                filing(
                    PageTarget::Existing {
                        record_id: page_id.clone(),
                    },
                    "The appointment in the filing.",
                    Some(digest),
                    9_000,
                );
                assert_eq!(
                    matches!(stored().state, SectionState::Superseded { .. }),
                    !include_ender
                );
                prior.status = ClaimStatus::Withdrawn;
                valid(&prior, 10_000);
                assert!(matches!(stored().state, SectionState::Contested { .. }));
            }
        }
    }

    #[test]
    fn a_pending_name_anchor_resolves_when_a_publication_adds_the_name() {
        let (_directory, store) = open_store();
        let record = world(&store);
        seal(&store, &record, Vec::new(), Vec::new(), &[], 10);
        let filed = store
            .transact(|write| {
                write.file(
                    LOUPIST,
                    &FilingOrigin::User {
                        chat_id: None,
                        message_id: None,
                    },
                    vec![PageDelta {
                        target: PageTarget::Create {
                            kind: PageKind::Subject,
                            title: "Gamma".into(),
                            anchors: vec![AnchorInput::Name {
                                name: "Gamma".into(),
                            }],
                        },
                        ops: vec![LensOp::Put {
                            key: SectionKey::new("overview").unwrap(),
                            heading: "Overview".into(),
                            markdown: "A technology.".into(),
                            basis: Basis {
                                refs: vec![BasisRef::UserSaid {
                                    message_id: "m1".into(),
                                }],
                                uncertainty: None,
                            },
                            expected: None,
                        }],
                    }],
                    11,
                )
            })
            .unwrap();
        let page_id = filed[0].record_id.clone();
        assert!(store.read_page(LOUPIST, &page_id).unwrap().anchors[0]
            .node_id
            .is_none());
        seal(
            &store,
            &record,
            vec![node("c", "Gamma", "Technology", None)],
            Vec::new(),
            &[],
            20,
        );
        assert_eq!(
            store.read_page(LOUPIST, &page_id).unwrap().anchors[0]
                .node_id
                .as_deref(),
            Some("c")
        );
    }
}
