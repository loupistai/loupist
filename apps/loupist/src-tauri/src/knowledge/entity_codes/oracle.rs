//! Identity oracle (master plan 3.5 and 6.1): different codes in one market
//! never join; names join only through a stored spelling or an agreeing code.
//!
//! The reference is written from that sentence, not from the code. Each name a
//! generated World carries has a known truth: the listing it writes (issuer and
//! code on one market), or only its issuer. The properties drive the host's own
//! deduplication, resolver and repair:
//!
//! - never-merge: no node ends up holding two listings of one market, or two
//!   issuers when only the host decided, under any order of same-chunk
//!   deduplication, resolver merges and repair;
//! - the partition the host decides does not depend on ingestion order;
//! - after a repair, every Lens anchor, world link and search document names a
//!   live node and the page's current revision.
//!
//! The default run uses 200 seeds per property; `LOUPIST_ORACLE_SEEDS=2000`
//! runs the long form. A failure prints every failing seed and its case.
//!
//! This module is a child of `world_extract::tests`, so it drives the
//! pipeline's own deduplication and resolver through its test harness.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::*;
use crate::knowledge::entity_codes::{repair::repair, Codes, Relation};
use crate::knowledge::model::GraphNode;

// --- Seeds ---------------------------------------------------------------

fn seed_count() -> u64 {
    std::env::var("LOUPIST_ORACLE_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200)
}

/// splitmix64: small, seeded and in-tree.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            items.swap(index, self.below(index + 1));
        }
    }
}

/// Runs `property` for every seed and fails once, naming each failing seed.
fn for_seeds(name: &str, property: impl Fn(u64) -> Result<(), String>) {
    let mut failures = Vec::new();
    for seed in 0..seed_count() {
        if let Err(case) = property(seed) {
            failures.push(format!("seed {seed}: {case}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{name}: {} failing seeds\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// --- The universe: fictional issuers on several markets --------------------

struct Listing {
    code: &'static str,
    /// Exchange suffix, empty when the venue is written only as a prefix.
    suffix: &'static str,
    exchange: &'static str,
    /// The market family the reference expects, written from the design.
    market: &'static str,
}

struct Issuer {
    name: &'static str,
    cik: Option<&'static str>,
    listings: &'static [Listing],
}

const fn listing(
    code: &'static str,
    suffix: &'static str,
    exchange: &'static str,
    market: &'static str,
) -> Listing {
    Listing {
        code,
        suffix,
        exchange,
        market,
    }
}

/// Fictional issuers. Two share classes on one US venue share a CIK; a Tokyo
/// and a Hong Kong issuer share a four-digit code; a Korean and a Shanghai
/// issuer share a six-digit code; one issuer lists the same ticker on two
/// markets.
const ISSUERS: &[Issuer] = &[
    Issuer {
        name: "Kestrel Holdings",
        cik: Some("0001907711"),
        listings: &[
            listing("KSTL.A", "", "NYSE", "US"),
            listing("KSTL.B", "", "NYSE", "US"),
        ],
    },
    Issuer {
        name: "Hanbit Chemical",
        cik: None,
        listings: &[listing("600731", "KS", "KRX", "KRX")],
    },
    Issuer {
        name: "Lianhua Brewing",
        cik: None,
        listings: &[listing("600731", "SS", "SSE", "SSE")],
    },
    Issuer {
        name: "Sakura Seiki",
        cik: None,
        listings: &[listing("6481", "T", "TSE", "JPX")],
    },
    Issuer {
        name: "Harbour Lantern Group",
        cik: None,
        listings: &[listing("6481", "HK", "HKEX", "HKEX")],
    },
    Issuer {
        name: "Rheinwerk Systeme",
        cik: None,
        listings: &[listing("RWS", "DE", "XETRA", "XETRA")],
    },
    Issuer {
        name: "Northgate Rail",
        cik: Some("0001833402"),
        listings: &[
            listing("NGR", "TO", "TSX", "TSX"),
            listing("NGR", "", "NYSE", "US"),
        ],
    },
    Issuer {
        name: "Jinsha Liquor",
        cik: None,
        listings: &[listing("000858", "SZ", "SZSE", "SZSE")],
    },
];

/// What one written name identifies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Truth {
    Listing(usize, usize),
    Issuer(usize),
}

impl Truth {
    fn issuer(self) -> usize {
        match self {
            Truth::Listing(issuer, _) | Truth::Issuer(issuer) => issuer,
        }
    }

    /// Two listings of one market with different codes, or two issuers when
    /// only the host decided.
    fn conflicts(self, other: Truth, host_only: bool) -> bool {
        if let (Truth::Listing(i, l), Truth::Listing(j, m)) = (self, other) {
            let (left, right) = (&ISSUERS[i].listings[l], &ISSUERS[j].listings[m]);
            if left.market == right.market && (i, l) != (j, m) {
                return true;
            }
        }
        host_only && self.issuer() != other.issuer()
    }
}

#[derive(Clone, Debug)]
struct Mention {
    name: String,
    truth: Truth,
    /// Properties a stored node states beside its name.
    properties: BTreeMap<String, Value>,
}

/// A stored node's name and properties, or, `written` only, a name as a
/// chunk writes it: extraction carries no properties.
fn mention(rng: &mut Rng, written: bool) -> Mention {
    let issuer_index = rng.below(ISSUERS.len());
    let issuer = &ISSUERS[issuer_index];
    let mut properties = BTreeMap::new();
    if let Some(cik) = issuer.cik.filter(|_| !written && rng.chance(50)) {
        properties.insert("cik".to_string(), json!(cik));
    }
    if rng.chance(20) {
        return Mention {
            name: issuer.name.to_string(),
            truth: Truth::Issuer(issuer_index),
            properties,
        };
    }
    let listing_index = rng.below(issuer.listings.len());
    let listing = &issuer.listings[listing_index];
    let name = match rng.below(4) {
        0 if !listing.suffix.is_empty() => {
            format!("{} {}.{}", issuer.name, listing.code, listing.suffix)
        }
        1 => format!("{} ({}:{})", issuer.name, listing.exchange, listing.code),
        // The written name, with the listing only in the properties.
        2 if written => {
            return Mention {
                name: issuer.name.to_string(),
                truth: Truth::Issuer(issuer_index),
                properties,
            }
        }
        2 => {
            properties.insert("ticker".to_string(), json!(listing.code));
            properties.insert("exchange".to_string(), json!(listing.exchange));
            issuer.name.to_string()
        }
        _ => format!("{} ({}:{})", issuer.name, listing.exchange, listing.code),
    };
    Mention {
        name,
        truth: Truth::Listing(issuer_index, listing_index),
        properties,
    }
}

fn graph_node(id: &str, mention: &Mention) -> GraphNode {
    GraphNode {
        id: id.to_string(),
        name: mention.name.clone(),
        aliases: Vec::new(),
        description: String::new(),
        kind: "Organization".to_string(),
        source_role: "subject".to_string(),
        properties: mention.properties.clone(),
    }
}

fn pipeline_node(id: &str, mention: &Mention) -> Node {
    Node {
        uuid: id.to_string(),
        name: mention.name.clone(),
        labels: vec!["Entity".to_string(), "Organization".to_string()],
        summary: String::new(),
        attributes: mention
            .properties
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        name_embedding: None,
        spellings: Vec::new(),
    }
}

/// The first two truths in one group that conflict.
fn conflict(groups: &BTreeMap<String, BTreeSet<Truth>>, host_only: bool) -> Option<String> {
    for (node, truths) in groups {
        for left in truths {
            for right in truths {
                if left < right && left.conflicts(*right, host_only) {
                    return Some(format!("{node} holds {left:?} and {right:?}"));
                }
            }
        }
    }
    None
}

// --- Repair -------------------------------------------------------------

/// One repair under a first-seen order. Returns each node's survivor.
fn repaired(
    nodes: &[GraphNode],
    first_seen: &[String],
) -> (Vec<GraphNode>, BTreeMap<String, String>) {
    let mut nodes = nodes.to_vec();
    let mut claims = Vec::new();
    let order: std::collections::HashMap<String, i64> = first_seen
        .iter()
        .enumerate()
        .map(|(at, id)| (id.clone(), at as i64))
        .collect();
    let (_, merged) = repair(&mut nodes, &mut claims, &mut || Ok(order.clone())).unwrap();
    (nodes, merged)
}

fn partition(ids: &[String], merged: &BTreeMap<String, String>) -> BTreeSet<BTreeSet<String>> {
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in ids {
        let survivor = merged.get(id).unwrap_or(id).clone();
        groups.entry(survivor).or_default().insert(id.clone());
    }
    groups.into_values().collect()
}

/// Repair never merges what the reference keeps apart, gives one partition
/// whatever the first-seen order, and changes nothing a second time.
#[test]
fn oracle_repair_never_merges_and_does_not_depend_on_order() {
    for_seeds("repair", |seed| {
        let mut rng = Rng::new(seed);
        let count = 2 + rng.below(6);
        let mentions: Vec<Mention> = (0..count).map(|_| mention(&mut rng, false)).collect();
        let ids: Vec<String> = (0..count).map(|index| format!("nod_{index}")).collect();
        let nodes: Vec<GraphNode> = ids
            .iter()
            .zip(&mentions)
            .map(|(id, mention)| graph_node(id, mention))
            .collect();
        let truths: BTreeMap<&str, Truth> = ids
            .iter()
            .map(String::as_str)
            .zip(mentions.iter().map(|mention| mention.truth))
            .collect();
        let mut seen: Option<(Vec<String>, BTreeSet<BTreeSet<String>>)> = None;
        for _ in 0..3 {
            let mut order = ids.clone();
            rng.shuffle(&mut order);
            let (after, merged) = repaired(&nodes, &order);
            let mut groups: BTreeMap<String, BTreeSet<Truth>> = BTreeMap::new();
            for id in &ids {
                groups
                    .entry(merged.get(id).unwrap_or(id).clone())
                    .or_default()
                    .insert(truths[id.as_str()]);
            }
            if let Some(found) = conflict(&groups, true) {
                return Err(format!(
                    "{found}; names {:?}; order {order:?}",
                    names(&mentions)
                ));
            }
            let (again, merged_again) = repaired(&after, &order);
            if !merged_again.is_empty() || again.len() != after.len() {
                return Err(format!(
                    "a second repair merged again; names {:?}",
                    names(&mentions)
                ));
            }
            let found = partition(&ids, &merged);
            match &seen {
                Some((first_order, first)) if *first != found => {
                    return Err(format!(
                        "partition depends on order: {first:?} under {first_order:?}, {found:?} under {order:?}; names {:?}",
                        names(&mentions)
                    ));
                }
                Some(_) => {}
                None => seen = Some((order, found)),
            }
        }
        Ok(())
    });
}

fn names(mentions: &[Mention]) -> Vec<String> {
    mentions
        .iter()
        .map(|mention| {
            if mention.properties.is_empty() {
                mention.name.clone()
            } else {
                format!("{} {}", mention.name, json!(mention.properties))
            }
        })
        .collect()
}

// --- Deduplication and the resolver -------------------------------------

/// One chunk run through the host's deduplication and resolution. `answer`
/// picks the model's duplicate candidate for each extracted row.
struct Chunk<'a> {
    stored: &'a [Node],
    names: &'a [String],
    answer: &'a dyn Fn(usize, usize) -> i64,
}

/// The node each surviving extracted name ended in, by name, and the stored
/// nodes with the spellings this chunk gave them.
struct Resolved {
    winners: Vec<(String, String)>,
    stored: Vec<Node>,
    new_nodes: Vec<Node>,
}

fn organization_type(schema: &ontology::Schema) -> i64 {
    schema
        .entity_types
        .iter()
        .position(|item| item.name == "Organization")
        .map(|index| index as i64 + 1)
        .expect("core Organization type")
}

fn run_chunk(chunk: Chunk<'_>) -> Resolved {
    let case = case("resolve_nodes");
    let hints = ontology::Hints::from_value(&json!({}));
    let schema = merged_schema(&hints.extension);
    let instructions = extraction_instructions(&hints);
    let (episode, previous) = episode(&case);
    let organization = organization_type(&schema);
    let extract = json!({"extracted_entities": chunk.names.iter().map(|name| json!({
        "name": name, "entity_type_id": organization,
    })).collect::<Vec<_>>()});
    // The resolution reply is built once the deduplicated rows are known.
    let replies = std::cell::RefCell::new(std::collections::VecDeque::from([extract]));
    let mut infer = |request: Value| {
        if request["kind"] == "embedding" {
            let vectors: Vec<Value> = request["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|_| json!(vec![0.0; 128]))
                .collect();
            return InferenceReply {
                disposition: InferenceDisposition::Completed,
                body: json!({"status": "ok", "json_object": null, "vectors": vectors}),
            };
        }
        InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: json!({
                "status": "ok",
                "json_object": replies.borrow_mut().pop_front().expect("a reply"),
                "usage": {"input_tokens": 1, "output_tokens": 1, "certainty": "accounted"},
            }),
        }
    };
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph {
            nodes: chunk.stored.to_vec(),
            edges: Vec::new(),
            links: Vec::new(),
        },
        &text,
    );
    let extracted = harness.extract_nodes().unwrap();
    let candidates = vec![chunk.stored.to_vec(); extracted.len()];
    if !chunk.stored.is_empty() {
        let rows: Vec<Value> = extracted
            .iter()
            .enumerate()
            .map(|(row, index)| {
                json!({
                    "id": row,
                    "name": harness.node(*index).name,
                    "duplicate_candidate_id": (chunk.answer)(row, chunk.stored.len()),
                })
            })
            .collect();
        replies
            .borrow_mut()
            .push_back(json!({"entity_resolutions": rows}));
    }
    let (resolved, _) = harness
        .resolve_with_candidates(&extracted, candidates)
        .unwrap();
    let stored_ids: BTreeSet<&str> = chunk.stored.iter().map(|node| node.uuid.as_str()).collect();
    let mut winners = Vec::new();
    let mut stored: BTreeMap<String, Node> = chunk
        .stored
        .iter()
        .map(|node| (node.uuid.clone(), node.clone()))
        .collect();
    let mut new_nodes = Vec::new();
    for (index, winner) in extracted.iter().zip(&resolved) {
        let node = harness.node(*winner).clone();
        winners.push((harness.node(*index).name.clone(), node.uuid.clone()));
        if stored_ids.contains(node.uuid.as_str()) {
            stored.insert(node.uuid.clone(), node);
        } else if index == winner {
            new_nodes.push(node);
        }
    }
    Resolved {
        winners,
        stored: stored.into_values().collect(),
        new_nodes,
    }
}

/// A written name's truth. A written name identifies one truth.
fn truth_of(mentions: &[Mention], name: &str) -> Option<Truth> {
    mentions
        .iter()
        .find(|mention| mention.name == name)
        .map(|mention| mention.truth)
}

/// Two names one chunk deduplicates into one node, when only the host
/// decides, must not conflict: the real deduplication over each pair.
fn deduplication_conflict(written: &[Mention]) -> Option<String> {
    for (index, left) in written.iter().enumerate() {
        for right in &written[index + 1..] {
            if left.name == right.name || !left.truth.conflicts(right.truth, true) {
                continue;
            }
            let names = [left.name.clone(), right.name.clone()];
            let resolved = run_chunk(Chunk {
                stored: &[],
                names: &names,
                answer: &|_, _| -1,
            });
            if resolved.winners.len() < 2 {
                return Some(format!(
                    "one chunk deduplicates {:?} and {:?} into one node",
                    left.name, right.name
                ));
            }
        }
    }
    None
}

/// Chunks of names over a stored World, with a model that answers at random:
/// whatever it proposes, no node ends up holding two listings of one market.
/// With a model that never proposes a merge, no node holds two issuers and the
/// host's partition does not depend on the order of the names.
#[test]
fn oracle_resolution_never_merges_across_a_market_split() {
    for_seeds("resolution", |seed| {
        let mut rng = Rng::new(seed);
        let stored_mentions: Vec<Mention> = (0..1 + rng.below(3))
            .map(|_| mention(&mut rng, false))
            .collect();
        let stored: Vec<Node> = stored_mentions
            .iter()
            .enumerate()
            .map(|(index, mention)| pipeline_node(&format!("nod_{index}"), mention))
            .collect();
        let chunks: Vec<Vec<Mention>> = (0..2)
            .map(|_| {
                (0..1 + rng.below(4))
                    .map(|_| mention(&mut rng, true))
                    .collect()
            })
            .collect();
        let written: Vec<Mention> = chunks.iter().flatten().cloned().collect();
        if let Some(found) = deduplication_conflict(&written) {
            return Err(found);
        }
        let answers: Vec<u64> = (0..16).map(|_| rng.next()).collect();
        for adversarial in [true, false] {
            let mut orders: Vec<Vec<Vec<String>>> = Vec::new();
            for _ in 0..2 {
                orders.push(
                    chunks
                        .iter()
                        .map(|chunk| {
                            let mut names: Vec<String> =
                                chunk.iter().map(|mention| mention.name.clone()).collect();
                            rng.shuffle(&mut names);
                            names
                        })
                        .collect(),
                );
            }
            let mut partitions = Vec::new();
            for order in &orders {
                let mut world = stored.clone();
                let mut held: BTreeMap<String, BTreeSet<Truth>> = world
                    .iter()
                    .zip(&stored_mentions)
                    .map(|(node, mention)| (node.uuid.clone(), BTreeSet::from([mention.truth])))
                    .collect();
                let mut by_name: BTreeMap<String, String> = BTreeMap::new();
                for names in order {
                    let answer = |row: usize, pool: usize| -> i64 {
                        if !adversarial {
                            return -1;
                        }
                        let pick = answers[row % answers.len()] as usize % (pool + 1);
                        pick as i64 - 1
                    };
                    let resolved = run_chunk(Chunk {
                        stored: &world,
                        names,
                        answer: &answer,
                    });
                    for (name, winner) in &resolved.winners {
                        if let Some(truth) = truth_of(&written, name) {
                            held.entry(winner.clone()).or_default().insert(truth);
                        }
                        by_name.insert(name.clone(), winner.clone());
                    }
                    world = resolved.stored;
                    world.extend(resolved.new_nodes);
                }
                if let Some(found) = conflict(&held, !adversarial) {
                    return Err(format!(
                        "{found}; stored {:?}; chunks {order:?}; model {}",
                        names(&stored_mentions),
                        if adversarial { "random" } else { "abstains" }
                    ));
                }
                let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
                for (name, winner) in by_name {
                    groups.entry(winner).or_default().insert(name);
                }
                partitions.push(groups.into_values().collect::<BTreeSet<_>>());
            }
            if !adversarial && partitions[0] != partitions[1] {
                return Err(format!(
                    "the host's partition depends on order: {:?} under {:?}, {:?} under {:?}",
                    partitions[0], orders[0], partitions[1], orders[1]
                ));
            }
        }
        Ok(())
    });
}

// --- Anchors and search documents after repair --------------------------

/// After a repair, every Lens anchor names a live node, the page's world
/// links name live nodes, and its search document is the current revision.
#[test]
fn oracle_repair_leaves_anchors_and_search_documents_live() {
    let seeds = seed_count();
    let mut failures = Vec::new();
    for seed in 0..seeds {
        let mut rng = Rng::new(seed);
        let count = 2 + rng.below(4);
        let mentions: Vec<Mention> = (0..count).map(|_| mention(&mut rng, false)).collect();
        let mut nodes: Vec<GraphNode> = mentions
            .iter()
            .enumerate()
            .map(|(index, mention)| graph_node(&format!("nod_{index}"), mention))
            .collect();
        let anchored = rng.below(count);
        let linked = rng.below(count);
        let directory = tempfile::tempdir().unwrap();
        let store =
            crate::knowledge::KnowledgeStore::open(directory.path().join("k.sqlite")).unwrap();
        let outcome: Result<(), String> = store
            .transact(|write| {
                write.tx.execute(
                    "INSERT INTO knowledge_records (record_id, kind, owner_loupist_id, title, created_at_ms)
                     VALUES ('rec_page', 'lens', 'loupist-a', 'Page', 1)",
                    [],
                )?;
                write.tx.execute(
                    "INSERT INTO knowledge_record_revisions
                        (revision_id, record_id, revision_digest, payload_json, dependency_json, created_at_ms)
                     VALUES ('rev_page', 'rec_page', 'stale', ?1, '[]', 1)",
                    [format!(r#"{{"summary":"See [it](world:nod_{linked})"}}"#)],
                )?;
                write.tx.execute(
                    "INSERT INTO knowledge_record_heads (record_id, revision_id) VALUES ('rec_page', 'rev_page')",
                    [],
                )?;
                write.tx.execute(
                    "INSERT INTO knowledge_search_documents
                        (record_id, revision_id, kind, span_id, object_id, coverage, title, body, aliases, search_text)
                     VALUES ('rec_page', 'rev_page', 'lens', '', '', 'complete', 'Page', ?1, '', ?1)",
                    [format!("See world:nod_{linked}")],
                )?;
                write.tx.execute(
                    "INSERT INTO knowledge_lens_anchors (loupist_id, record_id, role, node_id, created_at_ms)
                     VALUES ('loupist-a', 'rec_page', 'subject', ?1, 1)",
                    [format!("nod_{anchored}")],
                )?;
                let mut claims = Vec::new();
                crate::knowledge::entity_codes::repair::repair_world(
                    write.tx,
                    "loupist-a",
                    &mut nodes,
                    &mut claims,
                )?;
                let live: BTreeSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
                let anchors: Vec<String> = write
                    .tx
                    .prepare("SELECT node_id FROM knowledge_lens_anchors WHERE node_id IS NOT NULL")?
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<_, _>>()?;
                let head: String = write.tx.query_row(
                    "SELECT revision_id FROM knowledge_record_heads WHERE record_id = 'rec_page'",
                    [],
                    |row| row.get(0),
                )?;
                let payload: String = write.tx.query_row(
                    "SELECT payload_json FROM knowledge_record_revisions WHERE revision_id = ?1",
                    [&head],
                    |row| row.get(0),
                )?;
                let documents: Vec<(String, String)> = write
                    .tx
                    .prepare("SELECT revision_id, body FROM knowledge_search_documents WHERE record_id = 'rec_page'")?
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<_, _>>()?;
                let link_ids = |text: &str| -> Vec<String> {
                    text.split("world:")
                        .skip(1)
                        .map(|rest| {
                            rest.chars()
                                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                                .collect()
                        })
                        .collect()
                };
                let mut problems = Vec::new();
                problems.extend(
                    anchors
                        .iter()
                        .filter(|id| !live.contains(*id))
                        .map(|id| format!("anchor names gone node {id}")),
                );
                problems.extend(
                    link_ids(&payload)
                        .into_iter()
                        .filter(|id| !live.contains(id))
                        .map(|id| format!("page links gone node {id}")),
                );
                for (revision, body) in &documents {
                    if *revision != head {
                        problems.push(format!("search document on {revision}, head {head}"));
                    }
                    problems.extend(
                        link_ids(body)
                            .into_iter()
                            .filter(|id| !live.contains(id))
                            .map(|id| format!("search document names gone node {id}")),
                    );
                }
                Ok(if problems.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{problems:?}; names {:?}", names(&mentions)))
                })
            })
            .unwrap();
        if let Err(case) = outcome {
            failures.push(format!("seed {seed}: {case}"));
        }
    }
    assert!(
        failures.is_empty(),
        "anchors: {} failing seeds\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// --- Codes from many markets ----------------------------------------------

fn codes(name: &str) -> Codes {
    Codes::of_name(name, "Organization")
}

fn node_codes(name: &str, properties: Value) -> Codes {
    let properties = properties.as_object().cloned().unwrap_or_default();
    Codes::of_node(
        name,
        "Organization",
        properties.iter().map(|(key, value)| (key.as_str(), value)),
    )
}

/// The ISIN check digit (Luhn over the letters' values).
fn with_isin_check(body: &str) -> String {
    let digits: String = body
        .chars()
        .map(|c| c.to_digit(36).unwrap().to_string())
        .collect();
    let sum: u32 = digits
        .chars()
        .rev()
        .enumerate()
        .map(|(position, c)| {
            let digit = c.to_digit(10).unwrap();
            if position % 2 == 0 {
                let doubled = digit * 2;
                doubled / 10 + doubled % 10
            } else {
                digit
            }
        })
        .sum();
    format!("{body}{}", (10 - sum % 10) % 10)
}

/// The two LEI check digits (ISO 7064 mod 97-10).
fn with_lei_check(body: &str) -> String {
    let mut remainder = 0u64;
    for c in format!("{body}00").chars() {
        let value = u64::from(c.to_digit(36).unwrap());
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    format!("{body}{:02}", 98 - remainder)
}

/// Suffix and prefix forms of at least twelve exchanges name one market; a
/// code on two markets is two codes; a bare number names no market; an ISIN,
/// LEI, CIK or corporate code joins two names only when both carry it and agree.
#[test]
fn oracle_codes_from_many_markets() {
    let mut failures = Vec::new();
    // (suffix form, prefix form), each beside a name that does not agree.
    let exchanges = [
        ("6481.T", "TSE:6481"),
        ("6481.HK", "HKEX:6481"),
        ("051910.KS", "KRX:051910"),
        ("600731.SS", "SSE:600731"),
        ("000858.SZ", "SZSE:000858"),
        ("BWU.L", "LSE:BWU"),
        ("RWS.DE", "XETRA:RWS"),
        ("MLU.PA", "EPA:MLU"),
        ("NGR.TO", "TSX:NGR"),
        ("SCM.AX", "ASX:SCM"),
        ("SARASWATI.NS", "NSE:SARASWATI"),
        ("M1L.SI", "SGX:M1L"),
        ("ALPB.SW", "SIX:ALPB"),
        ("3008.TW", "TWSE:3008"),
    ];
    for (suffix, prefix) in exchanges {
        let left = codes(&format!("Aurora Kogyo {suffix}"));
        let right = codes(&format!("Mirador Partners ({prefix})"));
        if !left.joins(&right) {
            failures.push(format!("{suffix} and {prefix} are not one listing"));
        }
    }
    for (index, (left, _)) in exchanges.iter().enumerate() {
        for (right, _) in &exchanges[index + 1..] {
            let (left_code, right_code) = (left.split('.').next(), right.split('.').next());
            if left_code != right_code {
                continue;
            }
            let (a, b) = (
                codes(&format!("Aurora Kogyo {left}")),
                codes(&format!("Mirador Partners {right}")),
            );
            if a.joins(&b) || a.relation(&b) != Relation::Different {
                failures.push(format!("{left} and {right} read as one code"));
            }
        }
    }
    // A bare number names no market: beside any suffixed form under names that
    // do not agree, it joins nothing.
    for (bare, listed) in [
        ("600731", "600731.KS"),
        ("600731", "600731.SS"),
        ("600731", "600731.SZ"),
        ("6481", "6481.T"),
        ("6481", "6481.HK"),
    ] {
        let (a, b) = (
            codes(&format!("Aurora Kogyo {bare}")),
            codes(&format!("Mirador Partners {listed}")),
        );
        if a.joins(&b) {
            failures.push(format!(
                "bare {bare} joins {listed} under names that do not agree"
            ));
        }
    }
    let isin = with_isin_check("XS000473921");
    let other_isin = with_isin_check("XS000473922");
    let lei = with_lei_check("5493001KJTIIGC8Y1R");
    for (key, value, other) in [
        ("isin", json!(isin), json!(other_isin)),
        (
            "identifier",
            json!(lei),
            json!(with_lei_check("5493001KJTIIGC8Y1S")),
        ),
        ("cik", json!("0001907711"), json!("0001907712")),
        ("corp_code", json!("00164779"), json!("00164780")),
    ] {
        let both = node_codes("Aurora Kogyo", json!({ key: value.clone() }));
        let same = node_codes("Mirador Partners", json!({ key: value.clone() }));
        let missing = node_codes("Mirador Partners", json!({}));
        let different = node_codes("Mirador Partners", json!({ key: other }));
        if !both.joins(&same) {
            failures.push(format!("an agreeing {key} does not join"));
        }
        if both.joins(&missing) {
            failures.push(format!("a {key} on one side only joins"));
        }
        if both.joins(&different) || !both.refuses(&different) {
            failures.push(format!("two different {key} values are not two entities"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// --- The open findings as named tests -----------------------------------

/// (g) Two share classes named in one chunk stay two nodes: the issuer name
/// they share does not join them before the code check.
#[test]
fn two_share_classes_in_one_chunk_stay_two_nodes() {
    for order in [
        [
            "Berkshire Hathaway (NYSE:BRK.A)",
            "Berkshire Hathaway (NYSE:BRK.B)",
        ],
        [
            "Berkshire Hathaway (NYSE:BRK.B)",
            "Berkshire Hathaway (NYSE:BRK.A)",
        ],
    ] {
        let names: Vec<String> = order.iter().map(|name| name.to_string()).collect();
        let resolved = run_chunk(Chunk {
            stored: &[],
            names: &names,
            answer: &|_, _| -1,
        });
        let kept: BTreeSet<&str> = resolved
            .winners
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(kept.len(), 2, "{order:?} kept {kept:?}");
    }
}

/// (h) A node with the shared issuer code and no ticker never bridges two
/// share classes: not in repair, and not by resolver merges in two chunks.
#[test]
fn a_tickerless_issuer_code_never_bridges_two_share_classes() {
    let kestrel = &ISSUERS[0];
    let issuer = Mention {
        name: kestrel.name.to_string(),
        truth: Truth::Issuer(0),
        properties: BTreeMap::from([("cik".to_string(), json!(kestrel.cik.unwrap()))]),
    };
    let class = |index: usize| Mention {
        name: format!("{} (NYSE:{})", kestrel.name, kestrel.listings[index].code),
        truth: Truth::Listing(0, index),
        properties: BTreeMap::from([("cik".to_string(), json!(kestrel.cik.unwrap()))]),
    };
    // Repair, under every first-seen order.
    let nodes = vec![
        graph_node("nod_issuer", &issuer),
        graph_node("nod_a", &class(0)),
        graph_node("nod_b", &class(1)),
    ];
    let ids: Vec<String> = nodes.iter().map(|node| node.id.clone()).collect();
    for first in &ids {
        for second in &ids {
            if first == second {
                continue;
            }
            let third = ids.iter().find(|id| *id != first && *id != second).unwrap();
            let (_, merged) = repaired(&nodes, &[first.clone(), second.clone(), third.clone()]);
            let survivor = |id: &str| merged.get(id).cloned().unwrap_or_else(|| id.to_string());
            assert_ne!(
                survivor("nod_a"),
                survivor("nod_b"),
                "repair order {first}, {second}"
            );
        }
    }
    // The resolver: the model merges class A into the issuer node in one chunk,
    // then class B into it in the next.
    let stored = vec![pipeline_node("nod_issuer", &issuer)];
    let first = run_chunk(Chunk {
        stored: &stored,
        names: &[class(0).name],
        answer: &|_, _| 0,
    });
    let mut world = first.stored;
    world.extend(first.new_nodes);
    let after_a = first.winners[0].1.clone();
    let issuer_index = world
        .iter()
        .position(|node| node.uuid == "nod_issuer")
        .unwrap() as i64;
    let second = run_chunk(Chunk {
        stored: &world,
        names: &[class(1).name],
        answer: &|_, _| issuer_index,
    });
    let after_b = second.winners[0].1.clone();
    assert!(
        after_a != after_b || after_a != "nod_issuer",
        "both classes merged into the tickerless issuer node"
    );
}
