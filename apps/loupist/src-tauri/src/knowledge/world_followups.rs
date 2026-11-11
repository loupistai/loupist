use std::collections::BTreeSet;

use super::connections::{self, Candidate, DetectInput, Hop};
use super::error::{KnowledgeError, KnowledgeResult};
use super::identity::{resolve_followed, IdentityMatch};
use super::model::{mint_id, validate_token, ClaimStatus, WorldSnapshotBody};
use crate::store::user_context::MapContextItem;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

pub const SHOWN_PER_DAY: usize = 3;
const LIST_LIMIT: usize = 200;
const DAY_MS: i64 = 86_400_000;

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
    let contested_pages = if fallen.is_empty() {
        0
    } else {
        super::lens::contest_pages(tx, loupist_id, &fallen, at_ms)?
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
        status: ConnectionStatus::parse(&status).unwrap_or(ConnectionStatus::New),
        created_at_ms: row.get(7)?,
    })
}

const COLUMNS: &str =
    "connection_id, loupist_id, seed_node_id, far_node_id, path_json, score, status, created_at_ms";

pub(super) fn list(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<ConnectionView>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM knowledge_connections WHERE loupist_id = ?1
         ORDER BY created_at_ms DESC, score DESC, connection_id LIMIT ?2"
    ))?;
    let rows = statement.query_map(params![loupist_id, LIST_LIMIT as i64], view_from_row)?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

pub(super) fn set_status(
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
    tx.query_row(
        &format!("SELECT {COLUMNS} FROM knowledge_connections WHERE connection_id = ?1"),
        [connection_id],
        view_from_row,
    )
    .optional()?
    .ok_or(KnowledgeError::NotFound)
}

pub(crate) fn to_show(
    tx: &Transaction<'_>,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<Vec<ConnectionView>> {
    let day_start = at_ms - at_ms.rem_euclid(DAY_MS);
    let shown_today: i64 = tx.query_row(
        "SELECT COUNT(*) FROM knowledge_connections
         WHERE loupist_id = ?1 AND shown_at_ms >= ?2",
        params![loupist_id, day_start],
        |row| row.get(0),
    )?;
    let allowance = (SHOWN_PER_DAY as i64 - shown_today).max(0);
    if allowance == 0 {
        return Ok(Vec::new());
    }
    let mut statement = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM knowledge_connections WHERE loupist_id = ?1 AND status = 'new'
         ORDER BY score DESC, created_at_ms, connection_id LIMIT ?2"
    ))?;
    let rows = statement.query_map(params![loupist_id, allowance], view_from_row)?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
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
    fn the_day_allowance_keeps_the_fourth_link_new() {
        let (_directory, store) = open_store();
        let record = world(&store);
        let mut nodes = base_nodes();
        let mut claims = vec![claim("c1", "a", "b")];
        for far in ["c", "d", "e", "f"] {
            nodes.push(node(far, far, "Technology", None));
            claims.push(claim(&format!("c-{far}"), "b", far));
        }
        seal(&store, &record, nodes, claims, &[item("A", "KRX")], 10);
        assert_eq!(connections(&store).len(), 4);
        let shown = store
            .transact(|write| {
                let shown = to_show(write.tx, LOUPIST, 20)?;
                let ids: Vec<String> = shown
                    .iter()
                    .map(|link| link.connection_id.clone())
                    .collect();
                mark_shown(write.tx, LOUPIST, &ids, 20)?;
                Ok(shown)
            })
            .unwrap();
        assert_eq!(shown.len(), SHOWN_PER_DAY);
        assert!(store
            .transact(|write| to_show(write.tx, LOUPIST, 30))
            .unwrap()
            .is_empty());
        let statuses: Vec<ConnectionStatus> = connections(&store)
            .into_iter()
            .map(|link| link.status)
            .collect();
        assert_eq!(
            statuses
                .iter()
                .filter(|s| **s == ConnectionStatus::New)
                .count(),
            1
        );
        let next_day = store
            .transact(|write| to_show(write.tx, LOUPIST, 20 + DAY_MS))
            .unwrap();
        assert_eq!(next_day.len(), 1);
        let opened = store
            .transact(|write| {
                write.connection_set_status(
                    LOUPIST,
                    &next_day[0].connection_id,
                    ConnectionStatus::Opened,
                    40,
                )
            })
            .unwrap();
        assert_eq!(opened.status, ConnectionStatus::Opened);
        assert!(matches!(
            store.transact(|write| write.connection_set_status(
                LOUPIST,
                "con_missing",
                ConnectionStatus::Dismissed,
                41
            )),
            Err(KnowledgeError::NotFound)
        ));
    }

    #[test]
    fn opening_or_dismissing_yesterdays_links_leaves_todays_allowance() {
        let (_directory, store) = open_store();
        let record = world(&store);
        let mut nodes = base_nodes();
        let mut claims = vec![claim("c1", "a", "b")];
        for far in ["c", "d", "e", "f"] {
            nodes.push(node(far, far, "Technology", None));
            claims.push(claim(&format!("c-{far}"), "b", far));
        }
        seal(&store, &record, nodes, claims, &[item("A", "KRX")], 10);
        let yesterday = store
            .transact(|write| {
                let shown = to_show(write.tx, LOUPIST, 20)?;
                let ids: Vec<String> = shown
                    .iter()
                    .map(|link| link.connection_id.clone())
                    .collect();
                mark_shown(write.tx, LOUPIST, &ids, 20)?;
                Ok(shown)
            })
            .unwrap();
        assert_eq!(yesterday.len(), SHOWN_PER_DAY);
        let today = 20 + DAY_MS;
        for (link, status) in yesterday.iter().zip([
            ConnectionStatus::Dismissed,
            ConnectionStatus::Opened,
            ConnectionStatus::Dismissed,
        ]) {
            store
                .transact(|write| {
                    write.connection_set_status(LOUPIST, &link.connection_id, status, today)
                })
                .unwrap();
        }
        let shown_today = store
            .transact(|write| to_show(write.tx, LOUPIST, today + 1))
            .unwrap();
        assert_eq!(
            shown_today.len(),
            1,
            "nothing was shown today, so the fourth link is"
        );
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
