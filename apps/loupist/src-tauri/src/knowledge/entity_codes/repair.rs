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
//! over its result changes nothing. Nothing else in the World changes: claims
//! keep their ids, text, status and evidence and only move to the surviving
//! node, and growth events stay as they were.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{params, Transaction};

use super::{kinds_join, Codes};
use crate::knowledge::error::KnowledgeResult;
use crate::knowledge::lens::fold_lens_title;
use crate::knowledge::model::{GraphClaim, GraphNode};

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
    Codes::of_node(
        &node.name,
        &node.kind,
        node.properties
            .iter()
            .map(|(key, value)| (key.as_str(), value)),
    )
}

fn clip(text: &str, limit: usize) -> String {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Repairs `nodes` and `claims` before they are sealed, and points the Lens
/// anchors and asset pages that named a merged node at the node it joined.
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
    Ok(repaired)
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
        let mut ordered: Vec<usize> = peered.into_iter().collect();
        ordered.sort_by_key(|index| {
            (
                seen.get(&nodes[*index].id).copied().unwrap_or(i64::MAX),
                nodes[*index].id.clone(),
            )
        });
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for index in ordered {
            let fits = |group: &Vec<usize>| {
                group.iter().any(|member| all[index].joins(&all[*member]))
                    && group.iter().all(|member| {
                        kinds_join(&nodes[index].kind, &nodes[*member].kind)
                            && !all[index].differs_from(&all[*member])
                    })
            };
            match groups.iter().position(fits) {
                Some(group) => groups[group].push(index),
                None => groups.push(vec![index]),
            }
        }
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
        let before = node.aliases.len();
        let kind = node.kind.clone();
        node.aliases
            .retain(|alias| !Codes::of_name(alias, &kind).refuses(&own));
        dropped += before - node.aliases.len();
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
