//! Hybrid World retrieval: BM25 and name/fact-vector cosine fused with RRF,
//! then matched-entity neighborhoods (`rank_hybrid_claims`), paged with both
//! endpoint nodes per claim (`page_claims`).

use std::collections::{HashMap, HashSet, VecDeque};

use rusqlite::{params_from_iter, Connection};
use serde_json::{json, Value};

use super::pipeline::ClaimText;

/// The reciprocal-rank constant `rank_hybrid_claims` and the host fuse with.
const RRF_K: f64 = 60.0;
/// Graphiti's MAX_QUERY_LENGTH guard on fulltext queries.
const MAX_QUERY_WORDS: usize = 128;

/// Graphiti's `rrf` (rank constant 1); ties keep first-seen order.
pub(crate) fn rrf(results: &[Vec<String>]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut scores: HashMap<String, f64> = HashMap::new();
    for result in results {
        for (index, key) in result.iter().enumerate() {
            let score = scores.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                0.0
            });
            *score += 1.0 / (index as f64 + 1.0);
        }
    }
    let mut ranked: Vec<(usize, &String)> = order.iter().enumerate().collect();
    ranked.sort_by(|left, right| {
        scores[right.1]
            .partial_cmp(&scores[left.1])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.0.cmp(&right.0))
    });
    ranked.into_iter().map(|(_, key)| key.clone()).collect()
}

/// Lucene's default OR over the query's words, in FTS5 syntax. Empty means no
/// fulltext search (no words, or a query too long for Graphiti's guard).
pub(crate) fn fts_query(text: &str) -> String {
    let lowered = text.to_lowercase();
    let words: Vec<&str> = lowered
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() || words.len() + 1 >= MAX_QUERY_WORDS {
        return String::new();
    }
    let mut seen = HashSet::new();
    words
        .into_iter()
        .filter(|word| seen.insert(*word))
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// BM25 ids from an FTS5 table, best first, restricted by `allowed` after ranking.
pub(crate) fn bm25(
    connection: &Connection,
    sql: &str,
    bind: &[&str],
    query: &str,
    allowed: Option<&HashSet<String>>,
    limit: usize,
) -> Vec<String> {
    let expression = fts_query(query);
    if expression.is_empty() {
        return Vec::new();
    }
    let mut values: Vec<String> = vec![expression];
    values.extend(bind.iter().map(|value| value.to_string()));
    let Ok(mut statement) = connection.prepare(sql) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map(params_from_iter(values.iter()), |row| {
        row.get::<_, String>(0)
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .filter(|id| allowed.is_none_or(|allowed| allowed.contains(id)))
        .take(limit)
        .collect()
}

/// An in-memory FTS5 copy of the stored claim text for one extraction, so the
/// pipeline never holds the Loupist database while it waits on a model.
pub(crate) struct MemoryClaimText {
    connection: Connection,
}

impl MemoryClaimText {
    pub(crate) fn new<'a>(
        rows: impl Iterator<Item = (&'a str, &'a str, &'a str)>,
    ) -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "CREATE VIRTUAL TABLE claim_text USING fts5(claim_id UNINDEXED, name, fact);",
        )?;
        {
            let tx = connection.unchecked_transaction()?;
            let mut insert =
                tx.prepare("INSERT INTO claim_text (claim_id, name, fact) VALUES (?1, ?2, ?3)")?;
            for (id, name, fact) in rows {
                insert.execute([id, name, fact])?;
            }
            drop(insert);
            tx.commit()?;
        }
        Ok(Self { connection })
    }
}

impl ClaimText for MemoryClaimText {
    fn bm25(&self, query: &str, allowed: Option<&HashSet<String>>, limit: usize) -> Vec<String> {
        bm25(
            &self.connection,
            "SELECT claim_id FROM claim_text WHERE claim_text MATCH ?1 ORDER BY bm25(claim_text)",
            &[],
            query,
            allowed,
            limit,
        )
    }
}

/// Fuse relationship hits with the neighborhoods of entity hits.
///
/// An entity match makes every incident claim reachable, not just the first
/// in snapshot order. Neighborhoods expand in ranked-node rounds so a hub
/// cannot consume the whole node channel before another matched entity.
/// Inside a neighborhood, direct relationship hits come first, then stable ids.
pub(crate) fn rank_hybrid_claims(
    edge_hits: &[String],
    node_hits: &[String],
    claims: &[Value],
    limit: usize,
) -> Vec<Value> {
    let by_id: HashMap<&str, &Value> = claims
        .iter()
        .filter_map(|claim| claim["adapter_id"].as_str().map(|id| (id, claim)))
        .collect();
    let mut edge_ids: Vec<String> = Vec::new();
    for id in edge_hits {
        if by_id.contains_key(id.as_str()) && !edge_ids.contains(id) {
            edge_ids.push(id.clone());
        }
    }
    let mut matched: Vec<(String, Vec<String>)> = Vec::new();
    for id in node_hits {
        if !matched.iter().any(|(node, _)| node == id) {
            matched.push((id.clone(), Vec::new()));
        }
    }
    for claim in claims {
        let Some(claim_id) = claim["adapter_id"].as_str() else {
            continue;
        };
        let endpoints: HashSet<&str> = claim["endpoints"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["node_id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        for (node, incident) in matched.iter_mut() {
            if endpoints.contains(node.as_str()) {
                incident.push(claim_id.to_string());
            }
        }
    }
    let edge_rank: HashMap<&str, usize> = edge_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect();
    let within = |id: &String| {
        (
            edge_rank
                .get(id.as_str())
                .copied()
                .unwrap_or(edge_rank.len()),
            id.clone(),
        )
    };
    let mut neighborhoods: VecDeque<std::vec::IntoIter<String>> = matched
        .into_iter()
        .map(|(_, mut incident)| {
            incident.sort_by_key(within);
            incident.into_iter()
        })
        .collect();
    let mut node_ids: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    while let Some(mut neighborhood) = neighborhoods.pop_front() {
        if let Some(id) = neighborhood.next() {
            neighborhoods.push_back(neighborhood);
            if seen.insert(id.clone()) {
                node_ids.push(id);
            }
        }
    }
    let mut order: Vec<String> = Vec::new();
    let mut scores: HashMap<String, f64> = HashMap::new();
    for ranked in [&edge_ids, &node_ids] {
        for (index, id) in ranked.iter().enumerate() {
            let score = scores.entry(id.clone()).or_insert_with(|| {
                order.push(id.clone());
                0.0
            });
            *score += 1.0 / (RRF_K + index as f64 + 1.0);
        }
    }
    let mut ranked: Vec<(usize, String)> = order.into_iter().enumerate().collect();
    ranked.sort_by(|left, right| {
        scores[&right.1]
            .partial_cmp(&scores[&left.1])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.0.cmp(&right.0))
    });
    ranked
        .into_iter()
        .take(limit)
        .filter_map(|(_, id)| by_id.get(id.as_str()).map(|claim| (*claim).clone()))
        .collect()
}

pub(crate) struct PageLimits {
    pub node_limit: usize,
    pub edge_limit: usize,
    pub byte_limit: usize,
}

/// One bounded page of ranked claims, in rank order, with both endpoint nodes.
pub(crate) fn page_claims(
    record_id: &str,
    revision_id: &str,
    nodes: &[Value],
    ranked: &[Value],
    limits: &PageLimits,
) -> Value {
    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|node| node["adapter_id"].as_str().map(|id| (id, node)))
        .collect();
    let mut chosen: Vec<Value> = Vec::new();
    let mut chosen_nodes: Vec<(String, Value)> = Vec::new();
    let mut index = 0;
    while index < ranked.len() && chosen.len() < limits.edge_limit {
        let claim = &ranked[index];
        let endpoint_ids: Vec<&str> = claim["endpoints"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["node_id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if endpoint_ids.iter().any(|id| !by_id.contains_key(id)) {
            index += 1;
            continue;
        }
        let mut trial_nodes = chosen_nodes.clone();
        for id in &endpoint_ids {
            if !trial_nodes.iter().any(|(existing, _)| existing == id) {
                trial_nodes.push((id.to_string(), (*by_id[id]).clone()));
            }
        }
        let mut trial_claims = chosen.clone();
        trial_claims.push(claim.clone());
        let preview = json!({
            "record_id": record_id,
            "revision_id": revision_id,
            "nodes": trial_nodes.iter().map(|(_, node)| node.clone()).collect::<Vec<_>>(),
            "edges": trial_claims,
        });
        if trial_nodes.len() > limits.node_limit
            || super::super::canonical::canonical_string(&preview).len() > limits.byte_limit
        {
            break;
        }
        chosen = trial_claims;
        chosen_nodes = trial_nodes;
        index += 1;
    }
    chosen_nodes.sort_by(|left, right| left.0.cmp(&right.0));
    json!({
        "record_id": record_id,
        "revision_id": revision_id,
        "revision_digest": null,
        "nodes": chosen_nodes.into_iter().map(|(_, node)| node).collect::<Vec<_>>(),
        "edges": chosen,
        "next_cursor": null,
        "coverage": "live_generation",
        "indexing_state": "ready",
        "truncated": index < ranked.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_and_queries_follow_graphiti() {
        assert_eq!(
            rrf(&[vec!["a".into(), "b".into()], vec!["b".into(), "c".into()]]),
            vec!["b".to_string(), "a".to_string(), "c".to_string()]
        );
        assert_eq!(
            fts_query("Hanbit's LFP, LFP 양극재"),
            "\"hanbit\" OR \"s\" OR \"lfp\" OR \"양극재\""
        );
        assert_eq!(fts_query("!!"), "");
        assert_eq!(fts_query(&"word ".repeat(127)), "");
        let text = MemoryClaimText::new(
            [
                ("c1", "SUPPLIES", "Hanbit supplies Northgate"),
                ("c2", "LEADS", "Kim leads Hanbit"),
                ("c3", "USES", "Nothing related"),
            ]
            .into_iter(),
        )
        .unwrap();
        let hits = text.bm25("Hanbit supplies", None, 10);
        assert_eq!(hits.first().map(String::as_str), Some("c1"));
        assert!(hits.contains(&"c2".to_string()));
        let allowed: HashSet<String> = ["c2".to_string()].into();
        assert_eq!(
            text.bm25("Hanbit supplies", Some(&allowed), 10),
            vec!["c2".to_string()]
        );
    }

    #[test]
    fn neighborhoods_interleave_and_direct_hits_lead() {
        let claim = |id: &str, a: &str, b: &str| json!({"adapter_id": id, "endpoints": [{"node_id": a, "role": "source"}, {"node_id": b, "role": "target"}]});
        let claims = vec![
            claim("c1", "hub", "x"),
            claim("c2", "hub", "y"),
            claim("c3", "hub", "z"),
            claim("c4", "other", "w"),
        ];
        let ranked =
            rank_hybrid_claims(&["c3".into()], &["hub".into(), "other".into()], &claims, 10);
        let ids: Vec<&str> = ranked
            .iter()
            .map(|claim| claim["adapter_id"].as_str().unwrap())
            .collect();
        // c3 is both a direct and a neighborhood hit; c4 enters before the hub's tail.
        assert_eq!(ids, vec!["c3", "c4", "c1", "c2"]);
    }
}
