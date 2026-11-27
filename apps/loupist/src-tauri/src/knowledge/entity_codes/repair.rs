//! A World resolved before the host held identifying codes can carry what the
//! rules now refuse: an alias the host refuses ([`Codes::refuses`]: "Stellantis
//! STLA" on Dongfeng Motor 0489.HK), and two nodes the host joins
//! ([`Codes::joins`]: two "Subaru 7270.T"). Every World publication from a
//! Source repairs both before the seal, so the next publication after an
//! update repairs what exists. It touches nothing else: names whose codes
//! differ but agree ("Toyota TM" aliased "Toyota 7203.T"), or whose equal bare
//! trailing capital is all they share ("Google AI", "Meta AI"), stay as the
//! model resolved them.
//!
//! Bounds: the pass reads the World being sealed (at most 10,000 nodes) once
//! per round, compares only nodes that share a code value, and reads growth
//! events only when two nodes share a code. Each round that merges removes at
//! least one node, and a round without a merge ends the pass, so a second pass
//! over its result changes nothing. Claims keep their ids, text, status and
//! evidence and only move to the surviving node. Connections, journal entity
//! ids and Lens `world:` links move with them. Growth events and the op log
//! stay as they were.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{params, Transaction};
use serde_json::Value;

use super::{kinds_join, partition, Codes};
use crate::knowledge::canonical::{canonical_string, from_json, revision_digest};
use crate::knowledge::error::KnowledgeResult;
use crate::knowledge::lens::fold_lens_title;
use crate::knowledge::model::{mint_id, EvidenceRevision, GraphClaim, GraphNode, RecordKind};

const ALIAS_CAP: usize = 16;
const PROPERTY_CAP: usize = 32;
const DESCRIPTION_BYTES: usize = 4_000;

/// What one repair changed, by count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Repaired {
    pub aliases_dropped: usize,
    pub nodes_merged: usize,
    /// Relationships between two merged nodes, now about one entity.
    pub claims_dropped: usize,
}

fn codes(node: &GraphNode) -> Codes {
    codes_with_spellings(node, &node.aliases, None)
}

/// `skip` is an alias under test: it must not count as agreement with itself.
fn codes_with_spellings(node: &GraphNode, aliases: &[String], skip: Option<&str>) -> Codes {
    let mut codes = Codes::of_node(
        &node.name,
        &node.kind,
        node.properties
            .iter()
            .map(|(key, value)| (key.as_str(), value)),
    );
    codes.add_spellings(
        aliases
            .iter()
            .filter(|alias| skip != Some(alias.as_str()))
            .map(String::as_str),
    );
    codes
}

fn clip(text: &str, limit: usize) -> String {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Repairs `nodes` and `claims` before they are sealed, and points Lens
/// anchors, asset pages, connections, journal entities and `world:` links
/// that named a merged node at the node it joined.
pub(crate) fn repair_world(
    tx: &Transaction<'_>,
    loupist_id: &str,
    nodes: &mut Vec<GraphNode>,
    claims: &mut Vec<GraphClaim>,
) -> KnowledgeResult<Repaired> {
    let (repaired, merged) = repair(nodes, claims, &mut || first_seen(tx, loupist_id))?;
    for (merged, survivor) in &merged {
        tx.execute(
            "UPDATE OR IGNORE knowledge_lens_anchors SET node_id = ?3
             WHERE loupist_id = ?1 AND node_id = ?2",
            params![loupist_id, merged, survivor],
        )?;
        // What is left already exists for the surviving node.
        tx.execute(
            "DELETE FROM knowledge_lens_anchors WHERE loupist_id = ?1 AND node_id = ?2",
            params![loupist_id, merged],
        )?;
        tx.execute(
            "UPDATE knowledge_asset_pages SET entity_id = ?3
             WHERE loupist_id = ?1 AND entity_id = ?2",
            params![loupist_id, merged, survivor],
        )?;
    }
    retarget_references(tx, loupist_id, &merged)?;
    Ok(repaired)
}

/// Connections, journal entity ids and Lens `world:` links name the survivor.
/// Growth events and the op log stay as written.
fn retarget_references(
    tx: &Transaction<'_>,
    loupist_id: &str,
    merged: &BTreeMap<String, String>,
) -> KnowledgeResult<()> {
    if merged.is_empty() {
        return Ok(());
    }
    retarget_connections(tx, loupist_id, merged)?;
    retarget_journal_entities(tx, loupist_id, merged)?;
    retarget_world_links(tx, loupist_id, merged)?;
    Ok(())
}

fn map_id<'a>(id: &'a str, merged: &'a BTreeMap<String, String>) -> &'a str {
    merged.get(id).map(String::as_str).unwrap_or(id)
}

fn retarget_json(value: &mut Value, merged: &BTreeMap<String, String>) -> bool {
    match value {
        Value::String(text) => {
            if let Some(next) = merged.get(text.as_str()) {
                *text = next.clone();
                return true;
            }
            let mut changed = false;
            for (old, new) in merged {
                let from = format!("world:{old}");
                if text.contains(&from) {
                    *text = text.replace(&from, &format!("world:{new}"));
                    changed = true;
                }
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= retarget_json(item, merged);
            }
            changed
        }
        Value::Object(map) => {
            let mut changed = false;
            for item in map.values_mut() {
                changed |= retarget_json(item, merged);
            }
            changed
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn retarget_connections(
    tx: &Transaction<'_>,
    loupist_id: &str,
    merged: &BTreeMap<String, String>,
) -> KnowledgeResult<()> {
    let mut statement = tx.prepare(
        "SELECT connection_id, seed_node_id, far_node_id, path_json
         FROM knowledge_connections WHERE loupist_id = ?1 ORDER BY connection_id",
    )?;
    let rows = statement
        .query_map([loupist_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    struct Rewritten {
        connection_id: String,
        seed: String,
        far: String,
        path_json: String,
    }
    let mut rewritten = Vec::new();
    let mut drop_ids = Vec::new();
    for (connection_id, seed, far, path_json) in rows {
        let seed = map_id(&seed, merged).to_string();
        let far = map_id(&far, merged).to_string();
        let path_json = match serde_json::from_str::<Value>(&path_json) {
            Ok(mut path) => {
                retarget_json(&mut path, merged);
                serde_json::to_string(&path)?
            }
            Err(_) => path_json,
        };
        if seed == far {
            drop_ids.push(connection_id);
            continue;
        }
        rewritten.push(Rewritten {
            connection_id,
            seed,
            far,
            path_json,
        });
    }
    rewritten.sort_by(|left, right| {
        (&left.seed, &left.far, &left.connection_id).cmp(&(
            &right.seed,
            &right.far,
            &right.connection_id,
        ))
    });
    let mut seen = BTreeSet::new();
    let mut keep = Vec::new();
    for row in rewritten {
        if seen.insert((row.seed.clone(), row.far.clone())) {
            keep.push(row);
        } else {
            drop_ids.push(row.connection_id);
        }
    }
    for connection_id in &drop_ids {
        tx.execute(
            "DELETE FROM knowledge_connections WHERE loupist_id = ?1 AND connection_id = ?2",
            params![loupist_id, connection_id],
        )?;
    }
    for row in keep {
        tx.execute(
            "UPDATE knowledge_connections
             SET seed_node_id = ?3, far_node_id = ?4, path_json = ?5
             WHERE loupist_id = ?1 AND connection_id = ?2",
            params![
                loupist_id,
                row.connection_id,
                row.seed,
                row.far,
                row.path_json
            ],
        )?;
    }
    Ok(())
}

fn retarget_journal_entities(
    tx: &Transaction<'_>,
    loupist_id: &str,
    merged: &BTreeMap<String, String>,
) -> KnowledgeResult<()> {
    let mut statement =
        tx.prepare("SELECT journal_id, body_json FROM knowledge_journals WHERE loupist_id = ?1")?;
    let rows = statement
        .query_map([loupist_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (journal_id, body) in rows {
        let Ok(mut value) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        if !retarget_json(&mut value, merged) {
            continue;
        }
        if let Some(entities) = value.get_mut("entities").and_then(Value::as_array_mut) {
            let mut seen = BTreeSet::new();
            entities.retain(|entity| entity.as_str().is_none_or(|id| seen.insert(id.to_string())));
        }
        tx.execute(
            "UPDATE knowledge_journals SET body_json = ?1 WHERE journal_id = ?2",
            params![serde_json::to_string(&value)?, journal_id],
        )?;
    }
    Ok(())
}

fn retarget_world_links(
    tx: &Transaction<'_>,
    loupist_id: &str,
    merged: &BTreeMap<String, String>,
) -> KnowledgeResult<()> {
    let mut statement = tx.prepare(
        "SELECT r.revision_id, r.record_id, m.title, r.payload_json, r.dependency_json, r.created_at_ms
         FROM knowledge_record_revisions r
         JOIN knowledge_records m ON m.record_id = r.record_id
         WHERE m.owner_loupist_id = ?1 AND m.kind = 'lens'",
    )?;
    let rows = statement
        .query_map([loupist_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (revision_id, record_id, title, payload_json, dependency_json, created_at_ms) in rows {
        let Ok(mut payload) = serde_json::from_str::<Value>(&payload_json) else {
            continue;
        };
        if !retarget_json(&mut payload, merged) {
            continue;
        }
        let dependencies: Vec<EvidenceRevision> = from_json(&dependency_json)?;
        let digest = revision_digest(RecordKind::Lens, &payload, &dependencies)?;
        let payload_json = canonical_string(&payload);
        let next_id = mint_id("rev");
        // Revisions are immutable. The erasure flag is what lets publication
        // replace a head, and the new row carries the moved `world:` link.
        // Search documents go with the deleted revision, then the page is
        // indexed again the way Lens publication indexes a new head.
        tx.execute("UPDATE knowledge_erasure SET armed = 1 WHERE id = 1", [])?;
        tx.execute(
            "DELETE FROM knowledge_record_heads WHERE record_id = ?1",
            [&record_id],
        )?;
        tx.execute(
            "DELETE FROM knowledge_search_documents WHERE record_id = ?1",
            [&record_id],
        )?;
        tx.execute(
            "DELETE FROM knowledge_record_revisions WHERE revision_id = ?1",
            [&revision_id],
        )?;
        tx.execute("UPDATE knowledge_erasure SET armed = 0 WHERE id = 1", [])?;
        tx.execute(
            "INSERT INTO knowledge_record_revisions (
                revision_id, record_id, revision_digest, payload_json, dependency_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                next_id,
                record_id,
                digest,
                payload_json,
                dependency_json,
                created_at_ms
            ],
        )?;
        tx.execute(
            "INSERT INTO knowledge_record_heads (record_id, revision_id) VALUES (?1, ?2)",
            params![record_id, next_id],
        )?;
        crate::knowledge::search_index::index_payload(
            tx,
            None,
            &record_id,
            &next_id,
            RecordKind::Lens,
            &title,
            &payload,
        )?;
    }
    Ok(())
}

/// When World first saw each node: its first `entity_added`.
fn first_seen(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<HashMap<String, i64>> {
    let mut statement = tx.prepare(
        "SELECT subject_id, MIN(at_ms) FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND kind = 'entity_added' AND subject_id IS NOT NULL
         GROUP BY subject_id",
    )?;
    let rows = statement.query_map([loupist_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The repair itself. `first_seen` is read once, when two nodes share a
/// code: the node World saw first survives, so its history leads. Returns
/// each merged node's id with the id of the node it joined.
pub(crate) fn repair(
    nodes: &mut Vec<GraphNode>,
    claims: &mut Vec<GraphClaim>,
    first_seen: &mut dyn FnMut() -> KnowledgeResult<HashMap<String, i64>>,
) -> KnowledgeResult<(Repaired, BTreeMap<String, String>)> {
    let mut repaired = Repaired::default();
    let mut joined: BTreeMap<String, String> = BTreeMap::new();
    let mut seen: Option<HashMap<String, i64>> = None;
    loop {
        repaired.aliases_dropped += drop_differing_aliases(nodes);
        let all: Vec<Codes> = nodes.iter().map(codes).collect();
        let peered = peered(nodes, &all);
        if peered.is_empty() {
            break;
        }
        if seen.is_none() {
            seen = Some(first_seen()?);
        }
        let seen = seen.as_ref().expect("first seen");
        let peered: Vec<usize> = peered.into_iter().collect();
        // One grouping whatever the order: which nodes become one is a set
        // decision; first-seen order only picks each group's survivor.
        let groups: Vec<Vec<usize>> = partition(
            peered.len(),
            |left, right| all[peered[left]].joins(&all[peered[right]]),
            |left, right| {
                let (left, right) = (peered[left], peered[right]);
                !kinds_join(&nodes[left].kind, &nodes[right].kind)
                    || all[left].differs_from(&all[right])
                    || all[left].keeps_apart(&all[right])
            },
        )
        .into_iter()
        .map(|group| {
            let mut group: Vec<usize> = group.into_iter().map(|item| peered[item]).collect();
            group.sort_by_key(|index| {
                (
                    seen.get(&nodes[*index].id).copied().unwrap_or(i64::MAX),
                    nodes[*index].id.clone(),
                )
            });
            group
        })
        .collect();
        let mut removed = BTreeSet::new();
        for group in groups.iter().filter(|group| group.len() > 1) {
            let survivor = group[0];
            for other in &group[1..] {
                let absorbed = nodes[*other].clone();
                absorb(&mut nodes[survivor], absorbed);
                joined.insert(nodes[*other].id.clone(), nodes[survivor].id.clone());
                removed.insert(*other);
                repaired.nodes_merged += 1;
            }
        }
        if removed.is_empty() {
            break;
        }
        let mut position = 0;
        nodes.retain(|_| {
            let keep = !removed.contains(&position);
            position += 1;
            keep
        });
    }
    // A node merged in an earlier round may have joined one merged later.
    let survivor = |id: &str| {
        let mut id = id;
        while let Some(next) = joined.get(id) {
            id = next;
        }
        id.to_string()
    };
    let joined: BTreeMap<String, String> =
        joined.keys().map(|id| (id.clone(), survivor(id))).collect();
    claims.retain_mut(|claim| {
        let mut moved = false;
        for endpoint in &mut claim.endpoints {
            if let Some(survivor) = joined.get(&endpoint.node_id) {
                endpoint.node_id = survivor.clone();
                moved = true;
            }
        }
        if !moved {
            return true;
        }
        let mut ends = HashSet::new();
        claim
            .endpoints
            .retain(|endpoint| ends.insert((endpoint.node_id.clone(), endpoint.role.clone())));
        if claim.relation != "REPORTS" && !claim.connects_names() {
            repaired.claims_dropped += 1;
            return false;
        }
        true
    });
    Ok((repaired, joined))
}

/// Drops each alias the host refuses beside its node's own name and codes.
fn drop_differing_aliases(nodes: &mut [GraphNode]) -> usize {
    let mut dropped = 0;
    for node in nodes {
        let own = codes(node);
        if own.is_empty() {
            continue;
        }
        let kind = node.kind.clone();
        let aliases = node.aliases.clone();
        let mut kept = Vec::new();
        for alias in &aliases {
            let own = codes_with_spellings(node, &aliases, Some(alias));
            if !Codes::of_name(alias, &kind).refuses(&own) {
                kept.push(alias.clone());
            }
        }
        dropped += aliases.len() - kept.len();
        node.aliases = kept;
    }
    dropped
}

/// Nodes the host joins to another node of a joinable kind.
fn peered(nodes: &[GraphNode], all: &[Codes]) -> BTreeSet<usize> {
    let mut by_value: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, node_codes) in all.iter().enumerate() {
        for value in node_codes.values() {
            let holders = by_value.entry(value).or_default();
            if holders.last() != Some(&index) {
                holders.push(index);
            }
        }
    }
    let mut peered = BTreeSet::new();
    for holders in by_value.values().filter(|holders| holders.len() > 1) {
        for (position, left) in holders.iter().enumerate() {
            for right in &holders[position + 1..] {
                if kinds_join(&nodes[*left].kind, &nodes[*right].kind)
                    && all[*left].joins(&all[*right])
                {
                    peered.insert(*left);
                    peered.insert(*right);
                }
            }
        }
    }
    peered
}

/// `survivor` takes `other`'s name and aliases as aliases (those the host does
/// not refuse beside it, within the cap), the properties it lacks, the summary
/// text it lacks and a kind when it had none.
fn absorb(survivor: &mut GraphNode, other: GraphNode) {
    let own = codes(survivor);
    let mut folds: HashSet<String> = std::iter::once(&survivor.name)
        .chain(&survivor.aliases)
        .map(|text| fold_lens_title(text))
        .collect();
    for spelling in std::iter::once(&other.name).chain(&other.aliases) {
        if survivor.aliases.len() >= ALIAS_CAP {
            break;
        }
        if Codes::of_name(spelling, &survivor.kind).refuses(&own) {
            continue;
        }
        if folds.insert(fold_lens_title(spelling)) {
            survivor.aliases.push(spelling.clone());
        }
    }
    survivor.aliases.sort();
    for (key, value) in other.properties {
        if !survivor.properties.contains_key(&key) && survivor.properties.len() < PROPERTY_CAP {
            survivor.properties.insert(key, value);
        }
    }
    let addition = other.description.trim();
    if !addition.is_empty() && !survivor.description.contains(addition) {
        let joined = if survivor.description.trim().is_empty() {
            addition.to_string()
        } else {
            format!("{}\n{addition}", survivor.description)
        };
        survivor.description = clip(&joined, DESCRIPTION_BYTES);
    }
    if survivor.kind == "Entity" && other.kind != "Entity" {
        survivor.kind = other.kind;
    }
}

#[cfg(test)]
mod tests;
