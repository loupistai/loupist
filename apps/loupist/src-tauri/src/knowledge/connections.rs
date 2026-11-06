use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::model::{ClaimStatus, GraphClaim, WorldSnapshotBody};

pub const HUB_DEGREE: usize = 40;
const MAX_INCIDENCES: usize = 64;
const MAX_EXPANSIONS: usize = 4_000;
const MAX_CANDIDATES: usize = 50;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub claim_id: String,
    pub relation: String,
    pub from_role: String,
    pub to_role: String,
    pub reversed: bool,
    pub node_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub seed_node_id: String,
    pub seed_name: String,
    pub far_node_id: String,
    pub hops: Vec<Hop>,
    pub score: i64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Detection {
    pub candidates: Vec<Candidate>,
    pub truncated: bool,
}

pub struct DetectInput<'a> {
    pub previous: Option<&'a WorldSnapshotBody>,
    pub next: &'a WorldSnapshotBody,
    pub seeds: &'a [String],
    pub already: &'a BTreeSet<(String, String)>,
    pub category_of: &'a dyn Fn(&str) -> &'static str,
    pub at_ms: i64,
}

struct Edge<'a> {
    claim: &'a GraphClaim,
    from_role: &'a str,
    to: &'a str,
    to_role: &'a str,
    reversed: bool,
}

fn adjacency(body: &WorldSnapshotBody) -> BTreeMap<&str, Vec<Edge<'_>>> {
    let mut out: BTreeMap<&str, Vec<Edge<'_>>> = BTreeMap::new();
    for claim in &body.claims {
        if claim.status == ClaimStatus::Withdrawn {
            continue;
        }
        for (from_index, from) in claim.endpoints.iter().enumerate() {
            for (to_index, to) in claim.endpoints.iter().enumerate() {
                if from.node_id == to.node_id {
                    continue;
                }
                out.entry(from.node_id.as_str()).or_default().push(Edge {
                    claim,
                    from_role: &from.role,
                    to: &to.node_id,
                    to_role: &to.role,
                    reversed: to_index < from_index,
                });
            }
        }
    }
    out
}

fn neighbours<'a>(edges: Option<&Vec<Edge<'a>>>) -> BTreeSet<&'a str> {
    edges.into_iter().flatten().map(|edge| edge.to).collect()
}

fn within_two(adjacency: &BTreeMap<&str, Vec<Edge<'_>>>, seed: &str) -> BTreeSet<String> {
    let mut reach = BTreeSet::new();
    reach.insert(seed.to_owned());
    for first in neighbours(adjacency.get(seed)) {
        reach.insert(first.to_owned());
        for second in neighbours(adjacency.get(first)) {
            reach.insert(second.to_owned());
        }
    }
    reach
}

struct Walk<'a> {
    adjacency: &'a BTreeMap<&'a str, Vec<Edge<'a>>>,
    degree: BTreeMap<&'a str, usize>,
    names: BTreeMap<&'a str, &'a str>,
    prior_claims: BTreeSet<&'a str>,
    recorded: BTreeMap<&'a str, Option<i64>>,
    expansions: usize,
    truncated: bool,
}

impl<'a> Walk<'a> {
    fn recency(&self, hops: &[Hop], at_ms: i64) -> i64 {
        let newest = hops
            .iter()
            .filter_map(|hop| self.recorded.get(hop.claim_id.as_str()).copied().flatten())
            .max();
        match newest {
            Some(recorded) if at_ms - recorded <= 7 * DAY_MS => 15,
            Some(recorded) if at_ms - recorded <= 30 * DAY_MS => 8,
            _ => 0,
        }
    }

    fn support(&self, from: &str, to: &str) -> i64 {
        let count = self
            .adjacency
            .get(from)
            .into_iter()
            .flatten()
            .filter(|edge| edge.to == to)
            .count();
        count.min(4) as i64
    }

    fn score(&self, input: &DetectInput<'_>, seed: &str, hops: &[Hop]) -> i64 {
        let kind = |id: &str| {
            input
                .next
                .nodes
                .iter()
                .find(|node| node.id == id)
                .map(|node| node.kind.as_str())
                .unwrap_or("")
        };
        let far = hops.last().map_or("", |hop| hop.node_id.as_str());
        let mut score = 100 - 20 * (hops.len() as i64 - 2);
        if (input.category_of)(kind(seed)) != (input.category_of)(kind(far)) {
            score += 40;
        }
        for middle in &hops[..hops.len() - 1] {
            score -= (*self.degree.get(middle.node_id.as_str()).unwrap_or(&0) as i64) / 2;
        }
        let mut from = seed;
        for hop in hops {
            score += 5 * (self.support(from, &hop.node_id) - 1);
            from = &hop.node_id;
        }
        score + self.recency(hops, input.at_ms)
    }

    fn walk(
        &mut self,
        input: &DetectInput<'_>,
        seed: &str,
        prior_reach: &BTreeSet<String>,
        found: &mut BTreeMap<String, Candidate>,
    ) {
        let direct = neighbours(self.adjacency.get(seed));
        let mut stack: Vec<(String, Vec<Hop>, bool)> = vec![(seed.to_owned(), Vec::new(), false)];
        while let Some((at, hops, has_new)) = stack.pop() {
            if hops.len() >= 3 {
                continue;
            }
            if self.expansions >= MAX_EXPANSIONS {
                self.truncated = true;
                return;
            }
            self.expansions += 1;
            let Some(edges) = self.adjacency.get(at.as_str()) else {
                continue;
            };
            if edges.len() > MAX_INCIDENCES {
                self.truncated = true;
            }
            for edge in edges.iter().take(MAX_INCIDENCES).rev() {
                let to = edge.to;
                if to == seed || hops.iter().any(|hop| hop.node_id == to) {
                    continue;
                }
                let is_new = has_new || !self.prior_claims.contains(edge.claim.id.as_str());
                let mut next_hops = hops.clone();
                next_hops.push(Hop {
                    claim_id: edge.claim.id.clone(),
                    relation: edge.claim.relation.clone(),
                    from_role: edge.from_role.to_owned(),
                    to_role: edge.to_role.to_owned(),
                    reversed: edge.reversed,
                    node_id: to.to_owned(),
                    name: self.names.get(to).copied().unwrap_or(to).to_owned(),
                });
                let ends_here = next_hops.len() >= 2
                    && is_new
                    && !prior_reach.contains(to)
                    && !direct.contains(to)
                    && !input.already.contains(&(seed.to_owned(), to.to_owned()));
                if ends_here {
                    let score = self.score(input, seed, &next_hops);
                    let better = found.get(to).is_none_or(|current| {
                        (score, std::cmp::Reverse(next_hops.len()))
                            > (current.score, std::cmp::Reverse(current.hops.len()))
                    });
                    if better {
                        found.insert(
                            to.to_owned(),
                            Candidate {
                                seed_node_id: seed.to_owned(),
                                seed_name: self.names.get(seed).copied().unwrap_or(seed).to_owned(),
                                far_node_id: to.to_owned(),
                                hops: next_hops.clone(),
                                score,
                            },
                        );
                    }
                }
                if next_hops.len() < 3 && *self.degree.get(to).unwrap_or(&0) <= HUB_DEGREE {
                    stack.push((to.to_owned(), next_hops, is_new));
                }
            }
        }
    }
}

pub fn detect(input: &DetectInput<'_>) -> Detection {
    let next_adjacency = adjacency(input.next);
    let previous_adjacency = input.previous.map(adjacency);
    let degree = next_adjacency
        .iter()
        .map(|(node, edges)| (*node, neighbours(Some(edges)).len()))
        .collect();
    let names = input
        .next
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let prior_claims = input
        .previous
        .into_iter()
        .flat_map(|body| &body.claims)
        .map(|claim| claim.id.as_str())
        .collect();
    let mut walk = Walk {
        adjacency: &next_adjacency,
        degree,
        names,
        prior_claims,
        recorded: input
            .next
            .claims
            .iter()
            .map(|claim| (claim.id.as_str(), claim.recorded_at_ms))
            .collect(),
        expansions: 0,
        truncated: false,
    };
    let seeds: BTreeSet<&str> = input
        .seeds
        .iter()
        .map(String::as_str)
        .filter(|seed| walk.names.contains_key(seed))
        .collect();
    let mut candidates = Vec::new();
    for seed in seeds {
        let prior_reach = previous_adjacency
            .as_ref()
            .map(|adjacency| within_two(adjacency, seed))
            .unwrap_or_default();
        let mut found = BTreeMap::new();
        walk.walk(input, seed, &prior_reach, &mut found);
        candidates.extend(found.into_values());
    }
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.hops.len().cmp(&b.hops.len()))
            .then(a.seed_node_id.cmp(&b.seed_node_id))
            .then(a.far_node_id.cmp(&b.far_node_id))
    });
    let truncated = walk.truncated || candidates.len() > MAX_CANDIDATES;
    candidates.truncate(MAX_CANDIDATES);
    Detection {
        candidates,
        truncated,
    }
}

pub fn core_category(kind: &str) -> &'static str {
    match kind.trim().to_lowercase().as_str() {
        "organization" => "company",
        "person" => "person",
        "product" => "product",
        "technology" => "technology",
        "segment" => "industry",
        "asset" => "market",
        "event" | "metric" => "event",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::model::{ClaimEndpoint, GraphNode};

    fn node(id: &str, kind: &str) -> GraphNode {
        GraphNode {
            id: id.into(),
            name: id.to_uppercase(),
            aliases: Vec::new(),
            description: String::new(),
            kind: kind.into(),
            source_role: "entity".into(),
            properties: Default::default(),
        }
    }

    fn claim(id: &str, from: &str, to: &str, status: ClaimStatus) -> GraphClaim {
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
            status,
            recorded_at_ms: Some(1_000),
            publication_time: None,
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        }
    }

    fn body(nodes: Vec<GraphNode>, claims: Vec<GraphClaim>) -> WorldSnapshotBody {
        WorldSnapshotBody {
            nodes,
            claims,
            annotations: Vec::new(),
            relation_vocabulary: Vec::new(),
        }
    }

    fn run(previous: Option<&WorldSnapshotBody>, next: &WorldSnapshotBody) -> Detection {
        detect(&DetectInput {
            previous,
            next,
            seeds: &["a".into()],
            already: &BTreeSet::new(),
            category_of: &core_category,
            at_ms: 2_000,
        })
    }

    #[test]
    fn a_new_claim_links_the_followed_name_to_a_far_name_through_the_middle() {
        let previous = body(
            vec![node("a", "Organization"), node("b", "Organization")],
            vec![claim("c1", "a", "b", ClaimStatus::Active)],
        );
        let next = body(
            vec![
                node("a", "Organization"),
                node("b", "Organization"),
                node("c", "Technology"),
            ],
            vec![
                claim("c1", "a", "b", ClaimStatus::Active),
                claim("c2", "c", "b", ClaimStatus::Active),
            ],
        );
        let found = run(Some(&previous), &next);
        assert!(!found.truncated);
        assert_eq!(found.candidates.len(), 1);
        let candidate = &found.candidates[0];
        assert_eq!(candidate.seed_node_id, "a");
        assert_eq!(candidate.far_node_id, "c");
        assert_eq!(candidate.hops.last().unwrap().name, "C");
        let path: Vec<(&str, &str, &str, &str, bool)> = candidate
            .hops
            .iter()
            .map(|hop| {
                (
                    hop.claim_id.as_str(),
                    hop.node_id.as_str(),
                    hop.from_role.as_str(),
                    hop.to_role.as_str(),
                    hop.reversed,
                )
            })
            .collect();
        assert_eq!(
            path,
            [
                ("c1", "b", "supplier", "customer", false),
                ("c2", "c", "customer", "supplier", true)
            ]
        );
        assert!(
            candidate.score > 100,
            "crossing categories scores above base"
        );
    }

    #[test]
    fn nothing_new_or_already_reachable_yields_nothing() {
        let previous = body(
            vec![
                node("a", "Organization"),
                node("b", "Organization"),
                node("c", "Technology"),
            ],
            vec![
                claim("c1", "a", "b", ClaimStatus::Active),
                claim("c2", "b", "c", ClaimStatus::Active),
            ],
        );
        assert!(run(Some(&previous), &previous).candidates.is_empty());
        let mut already = BTreeSet::new();
        already.insert(("a".to_owned(), "c".to_owned()));
        let next = body(
            previous.nodes.clone(),
            vec![
                claim("c1", "a", "b", ClaimStatus::Active),
                claim("c3", "b", "c", ClaimStatus::Active),
            ],
        );
        let found = detect(&DetectInput {
            previous: Some(&previous),
            next: &next,
            seeds: &["a".into()],
            already: &already,
            category_of: &core_category,
            at_ms: 2_000,
        });
        assert!(found.candidates.is_empty(), "one candidate per pair ever");
    }

    #[test]
    fn a_hub_in_the_middle_or_a_withdrawn_claim_breaks_the_path() {
        let mut nodes = vec![
            node("a", "Organization"),
            node("hub", "Organization"),
            node("c", "Technology"),
        ];
        let mut claims = vec![
            claim("c1", "a", "hub", ClaimStatus::Active),
            claim("c2", "hub", "c", ClaimStatus::Active),
        ];
        for index in 0..HUB_DEGREE {
            let id = format!("x{index}");
            nodes.push(node(&id, "Organization"));
            claims.push(claim(
                &format!("cx{index}"),
                "hub",
                &id,
                ClaimStatus::Active,
            ));
        }
        assert!(run(None, &body(nodes, claims)).candidates.is_empty());
        let withdrawn = body(
            vec![
                node("a", "Organization"),
                node("b", "Organization"),
                node("c", "Technology"),
            ],
            vec![
                claim("c1", "a", "b", ClaimStatus::Withdrawn),
                claim("c2", "b", "c", ClaimStatus::Active),
            ],
        );
        assert!(run(None, &withdrawn).candidates.is_empty());
    }

    #[test]
    fn three_hops_reach_further_and_a_seed_absent_from_the_world_is_skipped() {
        let next = body(
            vec![
                node("a", "Organization"),
                node("b", "Organization"),
                node("c", "Organization"),
                node("d", "Asset"),
            ],
            vec![
                claim("c1", "a", "b", ClaimStatus::Active),
                claim("c2", "b", "c", ClaimStatus::Active),
                claim("c3", "c", "d", ClaimStatus::Active),
            ],
        );
        let found = run(None, &next);
        let far: Vec<&str> = found
            .candidates
            .iter()
            .map(|candidate| candidate.far_node_id.as_str())
            .collect();
        assert_eq!(far, ["d", "c"], "the category crossing ranks first");
        assert_eq!(found.candidates[0].hops.len(), 3);
        let none = detect(&DetectInput {
            previous: None,
            next: &next,
            seeds: &["zzz".into()],
            already: &BTreeSet::new(),
            category_of: &core_category,
            at_ms: 2_000,
        });
        assert!(none.candidates.is_empty());
    }
}
