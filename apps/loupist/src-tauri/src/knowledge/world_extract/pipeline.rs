//! The six extraction stages of Graphiti 0.30.1 `add_episode`, with the worker's
//! hooks: names are never merged without the resolver, temporal bounds come only
//! from full-source extraction, one attribute call per episode, and edge
//! resolutions are batched.
//!
//! 1. extract entities (`extract_nodes.extract_text`)
//! 2. resolve entities (name-vector candidates, `dedupe_nodes.nodes`)
//! 3. extract relations (`extract_edges.edge`)
//! 4. resolve relations (same-endpoint duplicates, hybrid invalidation
//!    candidates, `dedupe_edges.resolve_edge`, temporal rules)
//! 5. attributes (the worker's batch prompt)
//! 6. summaries (Graphiti's fact-append rule, `extract_summaries_batch`)
//!
//! Searches read the World as it stood before the episode, as Graphiti's did
//! against Neo4j; nothing here writes. Node and edge values are an arena so a
//! resolved candidate is one shared object, as in Graphiti.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, FixedOffset, Utc};
use serde_json::Value;

use super::bridge::{
    Bridge, LlmCall, Stop, StopResult, DEFAULT_MAX_TOKENS, EXTRACT_EDGES_MAX_TOKENS,
};
use super::ontology::Schema;
use super::prompts::{self, Message, ATTRIBUTE_INSTRUCTIONS, EDGE_BATCH_INSTRUCTIONS};
use super::pyjson::Py;
use super::schema;
use super::search;
use super::text::{
    isoformat, new_node_labels, normalize_exact, prefix_chars, python_str, to_utc,
    truncate_at_sentence,
};

const ENTITY_TYPE_DESCRIPTION: &str = "A specific, identifiable entity that does not fit any of the other listed types. Must still be a concrete, meaningful thing — specific enough to be uniquely identifiable. GOOD: a named entity not covered by the other types. BAD: \"luck\", \"ideas\", \"tomorrow\", \"things\", \"them\", \"everybody\", \"a sense of wonder\", \"great times\". When in doubt, do not extract the entity.";
const NODE_DEDUP_CANDIDATE_LIMIT: usize = 15;
pub(crate) const SIM_MIN_SCORE: f64 = 0.6;
pub(crate) const SEARCH_LIMIT: usize = 10;
const MAX_SUMMARY_CHARS: usize = 1000;
const MAX_SUMMARY_NODES: usize = 30;
const ATTRIBUTE_MAX_LENGTH: usize = 250;
const MAX_CASE_BYTES: usize = 96 * 1024;
const MAX_CASES: usize = 16;
const MAX_ATTRIBUTE_ENTITIES: usize = 24;

pub(crate) type Time = DateTime<FixedOffset>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    /// Runtime identity: the snapshot id for an existing node, a new UUID otherwise.
    pub uuid: String,
    pub name: String,
    pub labels: Vec<String>,
    pub summary: String,
    /// Attribute values in insertion order.
    pub attributes: Vec<(String, Value)>,
    pub name_embedding: Option<Vec<f32>>,
}

impl Node {
    pub(crate) fn kind(&self) -> Option<&str> {
        self.labels
            .iter()
            .map(String::as_str)
            .find(|label| *label != "Entity")
    }

    fn attribute(&self, key: &str) -> Option<&Value> {
        self.attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Edge {
    pub uuid: String,
    pub source: String,
    pub target: String,
    pub name: String,
    pub fact: String,
    /// Episode identities that support this edge (span ids for stored claims).
    pub episodes: Vec<String>,
    pub valid_at: Option<Time>,
    pub invalid_at: Option<Time>,
    pub expired_at: Option<Time>,
    pub fact_embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Episode {
    /// The current episode's identity on edges (its episode key).
    pub key: String,
    pub content: String,
    pub valid_at: Time,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreviousEpisode {
    pub content: String,
    pub valid_at: Option<Time>,
}

/// The World before this episode, as the derived index holds it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// BM25 over stored claim text (FTS5 in the derived index).
pub(crate) trait ClaimText {
    /// Claim ids matching `query`, best first, among `allowed` when given.
    fn bm25(&self, query: &str, allowed: Option<&HashSet<String>>, limit: usize) -> Vec<String>;
}

pub(crate) struct Input<'a> {
    pub episode: Episode,
    pub previous: Vec<PreviousEpisode>,
    pub schema: &'a Schema,
    pub instructions: String,
    pub graph: Graph,
    pub text: &'a dyn ClaimText,
}

/// Graphiti's add_episode result: hydrated nodes and resolved plus invalidated edges.
#[derive(Debug, Clone, Default)]
pub(crate) struct Output {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

struct Run<'a, 'b> {
    bridge: &'a mut Bridge<'b>,
    schema: &'a Schema,
    episode: Episode,
    previous: Vec<PreviousEpisode>,
    instructions: String,
    graph: Graph,
    text: &'a dyn ClaimText,
    /// Arena of every node object this episode touches.
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    response_models: HashMap<Vec<(String, Fields)>, Fields>,
}

/// Attribute fields as (name, description), in declaration order.
type Fields = Vec<(String, String)>;
/// A node's attributes after one attribute call.
type Assigned = Vec<(usize, Vec<(String, Value)>)>;
/// Stated attribute values per batch row.
type Rows = HashMap<usize, Vec<(String, Option<String>)>>;

pub(crate) fn add_episode(input: Input<'_>, bridge: &mut Bridge<'_>) -> StopResult<Output> {
    let mut run = Run {
        bridge,
        schema: input.schema,
        episode: input.episode,
        previous: input.previous,
        instructions: input.instructions,
        graph: input.graph,
        text: input.text,
        nodes: Vec::new(),
        edges: Vec::new(),
        response_models: HashMap::new(),
    };
    let extracted = run.extract_nodes()?;
    let (resolved, uuid_map) = run.resolve_nodes(&extracted)?;
    let mut extracted_edges = run.extract_edges(&extracted)?;
    for index in &extracted_edges {
        let edge = &mut run.edges[*index];
        if let Some(mapped) = uuid_map.get(&edge.source) {
            edge.source = mapped.clone();
        }
        if let Some(mapped) = uuid_map.get(&edge.target) {
            edge.target = mapped.clone();
        }
    }
    let (resolved_edges, invalidated, new_edges) = run.resolve_edges(&mut extracted_edges)?;
    run.attributes_and_summaries(&resolved, &new_edges)?;
    Ok(Output {
        nodes: resolved
            .iter()
            .map(|index| run.nodes[*index].clone())
            .collect(),
        edges: resolved_edges
            .iter()
            .chain(&invalidated)
            .map(|index| run.edges[*index].clone())
            .collect(),
    })
}

fn previous_context(previous: &[PreviousEpisode]) -> Py {
    Py::List(
        previous
            .iter()
            .map(|episode| {
                Py::dict([
                    ("content", Py::str(episode.content.as_str())),
                    (
                        "timestamp",
                        episode
                            .valid_at
                            .as_ref()
                            .map_or(Py::None, |time| Py::str(isoformat(time))),
                    ),
                ])
            })
            .collect(),
    )
}

fn labels_py(labels: &[String]) -> Py {
    Py::strings(labels.iter())
}

fn attributes_py(attributes: &[(String, Value)]) -> Vec<(String, Py)> {
    attributes
        .iter()
        .map(|(key, value)| (key.clone(), Py::from_value(value)))
        .collect()
}

/// Neo4j's `vector.similarity.cosine`: `(1 + cos) / 2`.
pub(crate) fn similarity(query: &[f32], vector: &[f32]) -> Option<f64> {
    if vector.is_empty() || vector.len() != query.len() {
        return None;
    }
    let mut dot = 0.0_f64;
    let mut query_norm = 0.0_f64;
    let mut vector_norm = 0.0_f64;
    for (a, b) in query.iter().zip(vector) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        dot += a * b;
        query_norm += a * a;
        vector_norm += b * b;
    }
    let query_norm = if query_norm == 0.0 {
        1.0
    } else {
        query_norm.sqrt()
    };
    let vector_norm = if vector_norm == 0.0 {
        1.0
    } else {
        vector_norm.sqrt()
    };
    Some((1.0 + dot / (query_norm * vector_norm)) / 2.0)
}

/// Keys whose similarity exceeds 0.6, best first, ties in stored order.
pub(crate) fn ranked_by_similarity<'k>(
    query: &[f32],
    rows: impl Iterator<Item = (&'k str, &'k [f32])>,
    limit: usize,
) -> Vec<String> {
    let mut scored: Vec<(&str, f64)> = rows
        .filter_map(|(key, vector)| similarity(query, vector).map(|score| (key, score)))
        .filter(|(_, score)| *score > SIM_MIN_SCORE)
        .collect();
    scored.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(key, _)| key.to_string())
        .collect()
}

fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now() -> Time {
    Utc::now().fixed_offset()
}

/// Graphiti's datetime parse of a model-returned timestamp, in UTC.
fn parse_model_time(value: Option<&str>) -> Option<Time> {
    let value = value.filter(|value| !value.is_empty())?;
    super::text::from_isoformat(&value.replace('Z', "+00:00")).map(|time| to_utc(&time))
}

impl Run<'_, '_> {
    fn push_node(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn push_edge(&mut self, edge: Edge) -> usize {
        self.edges.push(edge);
        self.edges.len() - 1
    }

    fn types_context(&self) -> Vec<(i64, String, String)> {
        let mut context = vec![(0, "Entity".to_string(), ENTITY_TYPE_DESCRIPTION.to_string())];
        for (index, item) in self.schema.entity_types.iter().enumerate() {
            context.push((index as i64 + 1, item.name.clone(), item.doc.clone()));
        }
        context
    }

    // --- 1. Entities ---------------------------------------------------------

    fn extract_nodes(&mut self) -> StopResult<Vec<usize>> {
        let types = self.types_context();
        let types_py = Py::List(
            types
                .iter()
                .map(|(id, name, doc)| {
                    Py::dict([
                        ("entity_type_id", Py::Int(*id)),
                        ("entity_type_name", Py::str(name.as_str())),
                        ("entity_type_description", Py::str(doc.as_str())),
                    ])
                })
                .collect(),
        );
        let schema_py = schema::extracted_entities();
        let response = self.bridge.llm(LlmCall {
            stage: "extract_nodes",
            messages: prompts::extract_text(&types_py, &self.episode.content, &self.instructions),
            schema: Some(&schema_py),
            max_tokens: DEFAULT_MAX_TOKENS,
            small: false,
            attribute_extraction: false,
        })?;
        let mut extracted = Vec::new();
        for item in schema::objects(schema::field(&response, "extracted_entities")?)? {
            let name = schema::lax_str(schema::field(item, "name")?)?;
            let type_id = schema::lax_int(schema::field(item, "entity_type_id")?)?;
            if let Some(indices) = item.get("episode_indices") {
                schema::lax_int_list(indices)?;
            }
            if name.trim().is_empty() {
                continue;
            }
            let type_name = usize::try_from(type_id)
                .ok()
                .and_then(|index| types.get(index))
                .map_or("Entity", |(_, name, _)| name.as_str())
                .to_string();
            extracted.push(Node {
                uuid: new_uuid(),
                name,
                labels: new_node_labels(&type_name),
                summary: String::new(),
                attributes: Vec::new(),
                name_embedding: None,
            });
        }
        // Graphiti collapses same-name duplicates, keeping the more specific one.
        let mut chosen: HashMap<String, Node> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for node in extracted {
            let key = normalize_exact(&node.name);
            let Some(existing) = chosen.get(&key) else {
                order.push(key.clone());
                chosen.insert(key, node);
                continue;
            };
            let specific = |node: &Node| {
                node.labels
                    .iter()
                    .filter(|label| *label != "Entity")
                    .count()
            };
            let (new_specific, old_specific) = (specific(&node), specific(existing));
            if new_specific > old_specific
                || (new_specific == old_specific
                    && node.name.trim().chars().count() > existing.name.trim().chars().count())
            {
                chosen.insert(key, node);
            }
        }
        Ok(order
            .into_iter()
            .map(|key| {
                let node = chosen.remove(&key).expect("collapsed node");
                self.push_node(node)
            })
            .collect())
    }

    // --- 2. Entity resolution ------------------------------------------------

    fn resolve_nodes(
        &mut self,
        extracted: &[usize],
    ) -> StopResult<(Vec<usize>, HashMap<String, String>)> {
        if extracted.is_empty() {
            return Ok((Vec::new(), HashMap::new()));
        }
        let queries: Vec<String> = extracted
            .iter()
            .map(|index| self.nodes[*index].name.replace('\n', " "))
            .collect();
        let vectors = self.bridge.embed(&queries)?;
        let candidates: Vec<Vec<String>> = vectors
            .iter()
            .map(|vector| {
                ranked_by_similarity(
                    vector,
                    self.graph.nodes.iter().filter_map(|node| {
                        node.name_embedding
                            .as_deref()
                            .map(|embedding| (node.uuid.as_str(), embedding))
                    }),
                    NODE_DEDUP_CANDIDATE_LIMIT,
                )
            })
            .collect();
        let candidates: Vec<Vec<Node>> = candidates
            .iter()
            .map(|uuids| {
                uuids
                    .iter()
                    .filter_map(|uuid| {
                        self.graph
                            .nodes
                            .iter()
                            .find(|node| node.uuid == *uuid)
                            .cloned()
                    })
                    .collect()
            })
            .collect();
        self.resolve_with_candidates(extracted, candidates)
    }

    /// Graphiti's `_resolve_with_llm` over each extracted node's candidates.
    pub(super) fn resolve_with_candidates(
        &mut self,
        extracted: &[usize],
        candidates: Vec<Vec<Node>>,
    ) -> StopResult<(Vec<usize>, HashMap<String, String>)> {
        let mut uuid_map = HashMap::new();
        let unresolved: Vec<usize> = (0..extracted.len())
            .filter(|index| !candidates[*index].is_empty())
            .collect();
        let mut resolved: Vec<Option<usize>> = vec![None; extracted.len()];
        if !unresolved.is_empty() {
            // One object per candidate, in first-seen order across the unresolved nodes.
            let mut pool: Vec<usize> = Vec::new();
            let mut seen = HashSet::new();
            for index in &unresolved {
                for node in &candidates[*index] {
                    if seen.insert(node.uuid.clone()) {
                        let arena = self.push_node(node.clone());
                        pool.push(arena);
                    }
                }
            }
            let extracted_context = Py::List(
                unresolved
                    .iter()
                    .enumerate()
                    .map(|(relative, original)| {
                        let node = &self.nodes[extracted[*original]];
                        Py::dict([
                            ("id", Py::Int(relative as i64)),
                            ("name", Py::str(node.name.as_str())),
                            ("entity_type", labels_py(&node.labels)),
                            (
                                "entity_type_description",
                                Py::str(self.type_description(&node.labels)),
                            ),
                        ])
                    })
                    .collect(),
            );
            let existing_context = Py::List(
                pool.iter()
                    .enumerate()
                    .map(|(candidate_id, index)| {
                        let node = &self.nodes[*index];
                        let mut items = attributes_py(&node.attributes);
                        let mut set = |key: &str, value: Py| match items
                            .iter_mut()
                            .find(|(name, _)| name == key)
                        {
                            Some(slot) => slot.1 = value,
                            None => items.push((key.to_string(), value)),
                        };
                        set("candidate_id", Py::Int(candidate_id as i64));
                        set("name", Py::str(node.name.as_str()));
                        set("entity_types", labels_py(&node.labels));
                        set("summary", Py::str(prefix_chars(&node.summary, 120)));
                        Py::Dict(items)
                    })
                    .collect(),
            );
            let schema_py = schema::node_resolutions();
            let response = self.bridge.llm(LlmCall {
                stage: "dedupe_nodes",
                messages: prompts::dedupe_nodes(
                    &previous_context(&self.previous),
                    &self.episode.content,
                    &extracted_context,
                    &existing_context,
                    unresolved.len(),
                ),
                schema: Some(&schema_py),
                max_tokens: DEFAULT_MAX_TOKENS,
                small: false,
                attribute_extraction: false,
            })?;
            let mut resolutions = Vec::new();
            for item in schema::objects(schema::field(&response, "entity_resolutions")?)? {
                let id = schema::lax_int(schema::field(item, "id")?)?;
                schema::lax_str(schema::field(item, "name")?)?;
                let duplicate = schema::lax_int(schema::field(item, "duplicate_candidate_id")?)?;
                resolutions.push((id, duplicate));
            }
            let mut processed = HashSet::new();
            for (relative, duplicate) in resolutions {
                let Ok(relative) = usize::try_from(relative) else {
                    continue;
                };
                if relative >= unresolved.len() || !processed.insert(relative) {
                    continue;
                }
                let original = unresolved[relative];
                let extracted_index = extracted[original];
                let target = usize::try_from(duplicate)
                    .ok()
                    .and_then(|candidate| pool.get(candidate).copied());
                let winner = match target {
                    Some(candidate) => {
                        self.promote(extracted_index, candidate);
                        candidate
                    }
                    None => extracted_index,
                };
                resolved[original] = Some(winner);
                uuid_map.insert(
                    self.nodes[extracted_index].uuid.clone(),
                    self.nodes[winner].uuid.clone(),
                );
            }
        }
        let mut out = Vec::with_capacity(extracted.len());
        for (position, index) in extracted.iter().enumerate() {
            let winner = resolved[position].unwrap_or_else(|| {
                let uuid = self.nodes[*index].uuid.clone();
                uuid_map.insert(uuid.clone(), uuid);
                *index
            });
            out.push(winner);
        }
        Ok((out, uuid_map))
    }

    /// `_promote_resolved_node`: a generic candidate takes the duplicate's type.
    fn promote(&mut self, extracted: usize, candidate: usize) {
        if self.nodes[candidate].kind().is_some() {
            return;
        }
        let specific: Vec<String> = self.nodes[extracted]
            .labels
            .iter()
            .filter(|label| *label != "Entity")
            .cloned()
            .collect();
        if specific.is_empty() {
            return;
        }
        let mut labels: Vec<String> = Vec::new();
        for label in std::iter::once("Entity".to_string())
            .chain(self.nodes[candidate].labels.iter().cloned())
            .chain(specific)
        {
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        self.nodes[candidate].labels = labels;
    }

    fn type_description(&self, labels: &[String]) -> String {
        labels
            .iter()
            .find(|label| *label != "Entity")
            .and_then(|name| self.schema.entity_type(name))
            .map(|item| item.doc.clone())
            .filter(|doc| !doc.is_empty())
            .unwrap_or_else(|| "Default Entity Type".to_string())
    }

    // --- 3. Relations ----------------------------------------------------------

    fn extract_edges(&mut self, extracted: &[usize]) -> StopResult<Vec<usize>> {
        let mut signatures: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for (pair, names) in &self.schema.edge_map {
            for name in names {
                match signatures.iter_mut().find(|(key, _)| key == name) {
                    Some((_, list)) => list.push(pair.clone()),
                    None => signatures.push((name.clone(), vec![pair.clone()])),
                }
            }
        }
        let edge_types = Py::List(
            self.schema
                .edge_types
                .iter()
                .map(|edge| {
                    let pairs = signatures
                        .iter()
                        .find(|(name, _)| *name == edge.name)
                        .map(|(_, pairs)| pairs.clone())
                        .unwrap_or_else(|| vec![("Entity".into(), "Entity".into())]);
                    Py::dict([
                        ("fact_type_name", Py::str(edge.name.as_str())),
                        (
                            "fact_type_signatures",
                            Py::List(
                                pairs
                                    .into_iter()
                                    .map(|(source, target)| {
                                        Py::List(vec![Py::str(source), Py::str(target)])
                                    })
                                    .collect(),
                            ),
                        ),
                        ("fact_type_description", Py::str(edge.doc.as_str())),
                    ])
                })
                .collect(),
        );
        let nodes_py = Py::List(
            extracted
                .iter()
                .map(|index| {
                    let node = &self.nodes[*index];
                    Py::dict([
                        ("name", Py::str(node.name.as_str())),
                        ("entity_types", labels_py(&node.labels)),
                    ])
                })
                .collect(),
        );
        let schema_py = schema::extracted_edges();
        let response = self.bridge.llm(LlmCall {
            stage: "extract_edges",
            messages: prompts::extract_edges(
                &previous_context(&self.previous),
                &self.episode.content,
                &nodes_py,
                &python_str(&self.episode.valid_at),
                &edge_types,
                &self.instructions,
            ),
            schema: Some(&schema_py),
            max_tokens: EXTRACT_EDGES_MAX_TOKENS,
            small: false,
            attribute_extraction: false,
        })?;
        // `{node.name: node}`: a later duplicate name wins.
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for index in extracted {
            by_name.insert(self.nodes[*index].name.clone(), *index);
        }
        let mut parsed = Vec::new();
        for item in schema::objects(schema::field(&response, "edges")?)? {
            let source = schema::lax_str(schema::field(item, "source_entity_name")?)?;
            let target = schema::lax_str(schema::field(item, "target_entity_name")?)?;
            let relation = schema::lax_str(schema::field(item, "relation_type")?)?;
            let fact = schema::lax_str(schema::field(item, "fact")?)?;
            let optional = |key: &str| -> Result<Option<String>, schema::Invalid> {
                match item.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(value) => schema::lax_str(value).map(Some),
                }
            };
            let valid_at = optional("valid_at")?;
            let invalid_at = optional("invalid_at")?;
            if let Some(indices) = item.get("episode_indices") {
                schema::lax_int_list(indices)?;
            }
            parsed.push((source, target, relation, fact, valid_at, invalid_at));
        }
        let mut out = Vec::new();
        for (source, target, relation, fact, valid_at, invalid_at) in parsed {
            let (Some(source), Some(target)) = (by_name.get(&source), by_name.get(&target)) else {
                continue;
            };
            let (source, target) = (
                self.nodes[*source].uuid.clone(),
                self.nodes[*target].uuid.clone(),
            );
            if source == target || fact.trim().is_empty() {
                continue;
            }
            let edge = Edge {
                uuid: new_uuid(),
                source,
                target,
                name: relation,
                fact,
                episodes: vec![self.episode.key.clone()],
                valid_at: parse_model_time(valid_at.as_deref()),
                invalid_at: parse_model_time(invalid_at.as_deref()),
                expired_at: None,
                fact_embedding: None,
            };
            out.push(self.push_edge(edge));
        }
        Ok(out)
    }

    // --- 4. Relation resolution ------------------------------------------------

    /// One hybrid edge search over the stored World (EDGE_HYBRID_SEARCH_RRF),
    /// returning fresh copies, as each Neo4j search returned new objects.
    fn edge_search(
        &mut self,
        query: &str,
        vector: &[f32],
        allowed: Option<&HashSet<String>>,
    ) -> Vec<usize> {
        if allowed.is_some_and(HashSet::is_empty) {
            return Vec::new();
        }
        let bm25 = self.text.bm25(query, allowed, 2 * SEARCH_LIMIT);
        let cosine = ranked_by_similarity(
            vector,
            self.graph.edges.iter().filter_map(|edge| {
                let embedding = edge.fact_embedding.as_deref()?;
                (allowed.is_none_or(|allowed| allowed.contains(&edge.uuid)))
                    .then_some((edge.uuid.as_str(), embedding))
            }),
            2 * SEARCH_LIMIT,
        );
        let found: Vec<Edge> = search::rrf(&[bm25, cosine])
            .into_iter()
            .take(SEARCH_LIMIT)
            .filter_map(|uuid| {
                self.graph
                    .edges
                    .iter()
                    .find(|edge| edge.uuid == uuid)
                    .cloned()
            })
            .collect();
        found.into_iter().map(|edge| self.push_edge(edge)).collect()
    }

    #[allow(clippy::type_complexity)]
    fn resolve_edges(
        &mut self,
        extracted: &mut Vec<usize>,
    ) -> StopResult<(Vec<usize>, Vec<usize>, Vec<usize>)> {
        let mut seen = HashSet::new();
        extracted.retain(|index| {
            let edge = &self.edges[*index];
            seen.insert((
                edge.source.clone(),
                edge.target.clone(),
                normalize_exact(&edge.fact),
            ))
        });
        if extracted.is_empty() {
            return Ok((Vec::new(), Vec::new(), Vec::new()));
        }
        let facts: Vec<String> = extracted
            .iter()
            .map(|index| self.edges[*index].fact.clone())
            .collect();
        let vectors = self.bridge.embed(&facts)?;
        for (index, vector) in extracted.iter().zip(vectors) {
            self.edges[*index].fact_embedding = Some(vector);
        }
        // Both searches embed the fact with newlines replaced, in one wave each.
        let queries: Vec<String> = extracted
            .iter()
            .map(|index| self.edges[*index].fact.replace('\n', " "))
            .collect();
        let query_vectors = self.bridge.embed(&queries)?;
        let mut related_lists = Vec::new();
        for (position, index) in extracted.iter().enumerate() {
            let (source, target, fact) = {
                let edge = &self.edges[*index];
                (edge.source.clone(), edge.target.clone(), edge.fact.clone())
            };
            let between: HashSet<String> = self
                .graph
                .edges
                .iter()
                .filter(|edge| edge.source == source && edge.target == target)
                .map(|edge| edge.uuid.clone())
                .collect();
            related_lists.push(self.edge_search(&fact, &query_vectors[position], Some(&between)));
        }
        let query_vectors = self.bridge.embed(&queries)?;
        let mut other_lists = Vec::new();
        for (position, index) in extracted.iter().enumerate() {
            let fact = self.edges[*index].fact.clone();
            let found = self.edge_search(&fact, &query_vectors[position], None);
            let related: HashSet<&str> = related_lists[position]
                .iter()
                .map(|index: &usize| self.edges[*index].uuid.as_str())
                .collect();
            let others: Vec<usize> = found
                .into_iter()
                .filter(|index| !related.contains(self.edges[*index].uuid.as_str()))
                .collect();
            other_lists.push(others);
        }
        self.decide_edges(extracted, related_lists, other_lists)
    }

    /// Graphiti's `resolve_extracted_edge` for every edge, with the worker's
    /// batched model calls, then one embedding wave over the outcome.
    #[allow(clippy::type_complexity)]
    pub(super) fn decide_edges(
        &mut self,
        extracted: &[usize],
        related_lists: Vec<Vec<usize>>,
        other_lists: Vec<Vec<usize>>,
    ) -> StopResult<(Vec<usize>, Vec<usize>, Vec<usize>)> {
        let mut decisions: Vec<Option<Decision>> = (0..extracted.len()).map(|_| None).collect();
        let mut pending: Vec<(usize, Vec<Message>)> = Vec::new();
        for (position, index) in extracted.iter().enumerate() {
            let related = &related_lists[position];
            let others = &other_lists[position];
            if related.is_empty() && others.is_empty() {
                decisions[position] = Some(Decision::Unchanged);
                continue;
            }
            let edge = self.edges[*index].clone();
            let fact = normalize_exact(&edge.fact);
            if let Some(found) = related.iter().copied().find(|candidate| {
                let candidate = &self.edges[*candidate];
                candidate.source == edge.source
                    && candidate.target == edge.target
                    && normalize_exact(&candidate.fact) == fact
            }) {
                if !self.edges[found].episodes.contains(&self.episode.key) {
                    let key = self.episode.key.clone();
                    self.edges[found].episodes.push(key);
                }
                decisions[position] = Some(Decision::Verbatim(found));
                continue;
            }
            let existing = Py::List(
                related
                    .iter()
                    .enumerate()
                    .map(|(idx, candidate)| {
                        Py::dict([
                            ("idx", Py::Int(idx as i64)),
                            ("fact", Py::str(self.edges[*candidate].fact.as_str())),
                        ])
                    })
                    .collect(),
            );
            let invalidation = Py::List(
                others
                    .iter()
                    .enumerate()
                    .map(|(idx, candidate)| {
                        Py::dict([
                            ("idx", Py::Int((related.len() + idx) as i64)),
                            ("fact", Py::str(self.edges[*candidate].fact.as_str())),
                        ])
                    })
                    .collect(),
            );
            pending.push((
                position,
                prompts::resolve_edge(&existing, &invalidation, &edge.fact),
            ));
        }
        for batch in edge_batches(pending) {
            if batch.len() == 1 {
                let (position, messages) = batch.into_iter().next().expect("one case");
                let schema_py = schema::edge_duplicate();
                let response = self.bridge.llm(LlmCall {
                    stage: "resolve_edges",
                    messages,
                    schema: Some(&schema_py),
                    max_tokens: DEFAULT_MAX_TOKENS,
                    small: true,
                    attribute_extraction: false,
                })?;
                decisions[position] = Some(parse_edge_duplicate(&response, false)?);
                continue;
            }
            let schema_py = schema::edge_resolution_batch(batch.len());
            let cases = Py::Dict(
                batch
                    .iter()
                    .enumerate()
                    .map(|(case, (_, messages))| {
                        (
                            format!("case_{case}"),
                            Py::List(
                                messages
                                    .iter()
                                    .map(|message| {
                                        Py::dict([
                                            ("role", Py::str(message.role.as_str())),
                                            ("content", Py::str(message.content.as_str())),
                                        ])
                                    })
                                    .collect(),
                            ),
                        )
                    })
                    .collect(),
            );
            let response = self.bridge.llm(LlmCall {
                stage: "resolve_edges",
                messages: vec![
                    Message::new("system", EDGE_BATCH_INSTRUCTIONS),
                    Message::new("user", cases.dumps(false)),
                ],
                schema: Some(&schema_py),
                max_tokens: DEFAULT_MAX_TOKENS,
                small: true,
                attribute_extraction: false,
            })?;
            let object = response
                .as_object()
                .ok_or(Stop::Rejected("model result was not an object"))?;
            if object.len() != batch.len() {
                return Err(Stop::Invalid("edge batch has missing or extra cases"));
            }
            for (case, (position, _)) in batch.iter().enumerate() {
                let value = object
                    .get(&format!("case_{case}"))
                    .ok_or(Stop::Invalid("edge batch case is missing"))?;
                decisions[*position] = Some(parse_edge_duplicate(value, true)?);
            }
        }
        let mut resolved_edges = Vec::new();
        let mut invalidated_edges = Vec::new();
        let mut new_edges = Vec::new();
        for (position, index) in extracted.iter().enumerate() {
            let related = related_lists[position].clone();
            let others = other_lists[position].clone();
            let decision = decisions[position].take().expect("edge decision");
            let (resolved, invalidated) = match decision {
                Decision::Unchanged => (*index, Vec::new()),
                Decision::Verbatim(found) => (found, Vec::new()),
                Decision::Model(duplicates, contradicted) => {
                    let duplicate_ids: Vec<usize> = duplicates
                        .iter()
                        .filter_map(|id| usize::try_from(*id).ok())
                        .filter(|id| *id < related.len())
                        .collect();
                    let mut resolved = *index;
                    if let Some(first) = duplicate_ids.first() {
                        resolved = related[*first];
                        let key = self.episode.key.clone();
                        self.edges[resolved].episodes.push(key);
                    }
                    let mut candidates = Vec::new();
                    let last = related.len() + others.len();
                    for id in contradicted {
                        let Ok(id) = usize::try_from(id) else {
                            continue;
                        };
                        if id < related.len() {
                            candidates.push(related[id]);
                        } else if id < last {
                            candidates.push(others[id - related.len()]);
                        }
                    }
                    let now = now();
                    {
                        let edge = &mut self.edges[resolved];
                        if edge.invalid_at.is_some() && edge.expired_at.is_none() {
                            edge.expired_at = Some(now);
                        }
                    }
                    if self.edges[resolved].expired_at.is_none() {
                        candidates.sort_by(|left, right| {
                            let key = |index: &usize| {
                                let valid = self.edges[*index].valid_at.as_ref().map(to_utc);
                                (valid.is_none(), valid)
                            };
                            key(left).cmp(&key(right))
                        });
                        let resolved_valid = self.edges[resolved].valid_at.as_ref().map(to_utc);
                        for candidate in &candidates {
                            let candidate_valid = self.edges[*candidate].valid_at;
                            if let (Some(candidate_time), Some(resolved_time)) =
                                (candidate_valid.as_ref().map(to_utc), resolved_valid)
                            {
                                if candidate_time > resolved_time {
                                    let edge = &mut self.edges[resolved];
                                    edge.invalid_at = candidate_valid;
                                    edge.expired_at = Some(now);
                                    break;
                                }
                            }
                        }
                    }
                    let invalidated = self.contradictions(resolved, &candidates);
                    (resolved, invalidated)
                }
            };
            resolved_edges.push(resolved);
            invalidated_edges.extend(invalidated);
            if self.edges[resolved].uuid == self.edges[*index].uuid {
                new_edges.push(resolved);
            }
        }
        // create_entity_edge_embeddings over resolved and invalidated edges, one wave.
        let wave: Vec<usize> = resolved_edges
            .iter()
            .chain(&invalidated_edges)
            .copied()
            .filter(|index| !self.edges[*index].fact.is_empty())
            .collect();
        if !wave.is_empty() {
            let texts: Vec<String> = wave
                .iter()
                .map(|index| self.edges[*index].fact.clone())
                .collect();
            let vectors = self.bridge.embed(&texts)?;
            for (index, vector) in wave.iter().zip(vectors) {
                self.edges[*index].fact_embedding = Some(vector);
            }
        }
        Ok((resolved_edges, invalidated_edges, new_edges))
    }

    /// `resolve_edge_contradictions`.
    fn contradictions(&mut self, resolved: usize, candidates: &[usize]) -> Vec<usize> {
        let mut invalidated = Vec::new();
        let resolved_valid = self.edges[resolved].valid_at.as_ref().map(to_utc);
        let resolved_invalid = self.edges[resolved].invalid_at.as_ref().map(to_utc);
        let resolved_valid_raw = self.edges[resolved].valid_at;
        for candidate in candidates {
            let edge_invalid = self.edges[*candidate].invalid_at.as_ref().map(to_utc);
            let edge_valid = self.edges[*candidate].valid_at.as_ref().map(to_utc);
            if matches!((edge_invalid, resolved_valid), (Some(a), Some(b)) if a <= b)
                || matches!((edge_valid, resolved_invalid), (Some(a), Some(b)) if b <= a)
            {
                continue;
            }
            if matches!((edge_valid, resolved_valid), (Some(a), Some(b)) if a < b) {
                let edge = &mut self.edges[*candidate];
                edge.invalid_at = resolved_valid_raw;
                if edge.expired_at.is_none() {
                    edge.expired_at = Some(now());
                }
                invalidated.push(*candidate);
            }
        }
        invalidated
    }

    // --- 5 and 6. Attributes and summaries ----------------------------------------

    fn attributes_and_summaries(&mut self, nodes: &[usize], new_edges: &[usize]) -> StopResult<()> {
        let typed: Vec<(usize, String)> = nodes
            .iter()
            .filter_map(|index| {
                let kind = self.nodes[*index].kind()?.to_string();
                let entity = self.schema.entity_type(&kind)?;
                (!entity.fields.is_empty()).then_some((*index, kind))
            })
            .collect();
        let mut assigned: Vec<(usize, Vec<(String, Value)>)> = Vec::new();
        for batch in self.attribute_batches(&typed) {
            assigned.extend(self.attribute_call(&batch)?);
        }
        for (index, attributes) in assigned {
            self.nodes[index].attributes = attributes;
        }
        let mut by_node: HashMap<String, Vec<usize>> = HashMap::new();
        for index in new_edges {
            let edge = &self.edges[*index];
            by_node.entry(edge.source.clone()).or_default().push(*index);
            by_node.entry(edge.target.clone()).or_default().push(*index);
        }
        let mut needing = Vec::new();
        for index in nodes {
            let node_edges = by_node
                .get(&self.nodes[*index].uuid)
                .cloned()
                .unwrap_or_default();
            let mut text = self.nodes[*index].summary.clone();
            if !node_edges.is_empty() {
                let facts = node_edges
                    .iter()
                    .map(|edge| self.edges[*edge].fact.as_str())
                    .filter(|fact| !fact.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                text = format!("{text}\n{facts}").trim().to_string();
            }
            if !text.is_empty() && text.chars().count() <= MAX_SUMMARY_CHARS * 2 {
                self.nodes[*index].summary = text;
                continue;
            }
            needing.push(*index);
        }
        for flight in needing.chunks(MAX_SUMMARY_NODES) {
            let entities = Py::List(
                flight
                    .iter()
                    .map(|index| {
                        let node = &self.nodes[*index];
                        Py::dict([
                            ("name", Py::str(node.name.as_str())),
                            ("summary", Py::str(node.summary.as_str())),
                            ("entity_types", labels_py(&node.labels)),
                            ("attributes", Py::Dict(attributes_py(&node.attributes))),
                        ])
                    })
                    .collect(),
            );
            let schema_py = schema::summarized_entities();
            let response = self.bridge.llm(LlmCall {
                stage: "summaries",
                messages: prompts::summaries_batch(
                    &previous_context(&self.previous),
                    &self.episode.content,
                    &entities,
                ),
                schema: Some(&schema_py),
                max_tokens: DEFAULT_MAX_TOKENS,
                small: true,
                attribute_extraction: false,
            })?;
            let mut summaries = Vec::new();
            for item in schema::objects(schema::field(&response, "summaries")?)? {
                let name = schema::lax_str(schema::field(item, "name")?)?;
                let summary = schema::lax_str(schema::field(item, "summary")?)?;
                summaries.push((name, summary));
            }
            for (name, summary) in summaries {
                let key = name.to_lowercase();
                let truncated = truncate_at_sentence(&summary, MAX_SUMMARY_CHARS);
                for index in flight {
                    if self.nodes[*index].name.to_lowercase() == key {
                        self.nodes[*index].summary = truncated.clone();
                    }
                }
            }
        }
        let named: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|index| !self.nodes[*index].name.is_empty())
            .collect();
        if !named.is_empty() {
            let texts: Vec<String> = named
                .iter()
                .map(|index| self.nodes[*index].name.clone())
                .collect();
            let vectors = self.bridge.embed(&texts)?;
            for (index, vector) in named.iter().zip(vectors) {
                self.nodes[*index].name_embedding = Some(vector);
            }
        }
        Ok(())
    }

    fn attribute_payload(&self, batch: &[(usize, String)]) -> Py {
        let entities = Py::List(
            batch
                .iter()
                .enumerate()
                .map(|(position, (index, kind))| {
                    let node = &self.nodes[*index];
                    let entity = self.schema.entity_type(kind).expect("typed entity");
                    Py::dict([
                        ("index", Py::Int(position as i64)),
                        ("name", Py::str(node.name.as_str())),
                        ("type", Py::str(kind.as_str())),
                        ("docstring", Py::str(entity.doc.as_str())),
                        (
                            "fields",
                            Py::Dict(
                                entity
                                    .fields
                                    .iter()
                                    .map(|(name, description)| {
                                        (name.clone(), Py::str(description.as_str()))
                                    })
                                    .collect(),
                            ),
                        ),
                        (
                            "attributes",
                            Py::Dict(
                                entity
                                    .fields
                                    .iter()
                                    .filter_map(|(name, _)| {
                                        node.attribute(name)
                                            .map(|value| (name.clone(), Py::from_value(value)))
                                    })
                                    .collect(),
                            ),
                        ),
                    ])
                })
                .collect(),
        );
        Py::dict([
            ("episode_content", Py::str(self.episode.content.as_str())),
            ("previous_episodes", previous_context(&self.previous)),
            ("entities", entities),
        ])
    }

    fn attribute_messages(&self, batch: &[(usize, String)]) -> Vec<Message> {
        vec![
            Message::new("system", ATTRIBUTE_INSTRUCTIONS),
            Message::new("user", self.attribute_payload(batch).dumps(false)),
        ]
    }

    fn attribute_batches(&self, typed: &[(usize, String)]) -> Vec<Vec<(usize, String)>> {
        let mut batches = Vec::new();
        let mut batch: Vec<(usize, String)> = Vec::new();
        for item in typed {
            if !batch.is_empty() {
                let mut trial = batch.clone();
                trial.push(item.clone());
                if batch.len() >= MAX_ATTRIBUTE_ENTITIES
                    || messages_bytes(&self.attribute_messages(&trial)) > MAX_CASE_BYTES
                {
                    batches.push(std::mem::take(&mut batch));
                }
            }
            batch.push(item.clone());
        }
        if !batch.is_empty() {
            batches.push(batch);
        }
        batches
    }

    /// The worker's response model, cached per operation by its sorted signature.
    fn attribute_fields(&mut self, batch: &[(usize, String)]) -> Vec<(String, String)> {
        let mut key: Vec<(String, Vec<(String, String)>)> = batch
            .iter()
            .map(|(_, kind)| {
                let entity = self.schema.entity_type(kind).expect("typed entity");
                let mut fields = entity.fields.clone();
                fields.sort();
                (kind.clone(), fields)
            })
            .collect();
        key.sort();
        if let Some(fields) = self.response_models.get(&key) {
            return fields.clone();
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        for (_, kind) in batch {
            let entity = self.schema.entity_type(kind).expect("typed entity");
            for (name, description) in &entity.fields {
                if !fields.iter().any(|(existing, _)| existing == name) {
                    fields.push((name.clone(), description.clone()));
                }
            }
        }
        self.response_models.insert(key, fields.clone());
        fields
    }

    fn attribute_call(&mut self, batch: &[(usize, String)]) -> StopResult<Assigned> {
        let fields = self.attribute_fields(batch);
        let schema_py = schema::attribute_batch(&fields);
        let response = self.bridge.llm(LlmCall {
            stage: "attributes",
            messages: self.attribute_messages(batch),
            schema: Some(&schema_py),
            max_tokens: DEFAULT_MAX_TOKENS,
            small: true,
            attribute_extraction: true,
        })?;
        let rows = parse_attribute_batch(&response, batch.len(), &fields)?;
        let mut out = Vec::new();
        for (position, (index, kind)) in batch.iter().enumerate() {
            let node = &self.nodes[*index];
            let Some(values) = rows.get(&position) else {
                out.push((*index, node.attributes.clone()));
                continue;
            };
            let entity = self.schema.entity_type(kind).expect("typed entity");
            let mut merged = node.attributes.clone();
            for (name, value) in values {
                let Some(text) = value else { continue };
                if !entity.fields.iter().any(|(field, _)| field == name)
                    || text.chars().count() > ATTRIBUTE_MAX_LENGTH
                {
                    continue;
                }
                match merged.iter_mut().find(|(key, _)| key == name) {
                    Some(slot) => slot.1 = Value::String(text.clone()),
                    None => merged.push((name.clone(), Value::String(text.clone()))),
                }
            }
            // entity_type(**merged): every type field must be a string or null.
            if merged.iter().any(|(key, value)| {
                entity.fields.iter().any(|(field, _)| field == key)
                    && !(value.is_string() || value.is_null())
            }) {
                return Err(Stop::Invalid("attribute value is not a string"));
            }
            out.push((*index, merged));
        }
        Ok(out)
    }
}

fn messages_bytes(messages: &[Message]) -> usize {
    Py::List(
        messages
            .iter()
            .map(|message| {
                Py::dict([
                    ("role", Py::str(message.role.as_str())),
                    ("content", Py::str(message.content.as_str())),
                ])
            })
            .collect(),
    )
    .dumps(false)
    .len()
}

/// The worker's case batches: at most 16 cases and 96 KiB of encoded messages.
fn edge_batches(pending: Vec<(usize, Vec<Message>)>) -> Vec<Vec<(usize, Vec<Message>)>> {
    let mut batches = Vec::new();
    let mut batch: Vec<(usize, Vec<Message>)> = Vec::new();
    let mut size = 0;
    for item in pending {
        let added = messages_bytes(&item.1) + 32;
        if !batch.is_empty() && (size + added > MAX_CASE_BYTES || batch.len() == MAX_CASES) {
            batches.push(std::mem::take(&mut batch));
            size = 0;
        }
        batch.push(item);
        size += added;
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

enum Decision {
    Unchanged,
    Verbatim(usize),
    Model(Vec<i64>, Vec<i64>),
}

/// `EdgeDuplicate`: lax for a single call, strict inside the batch model.
fn parse_edge_duplicate(value: &Value, strict: bool) -> StopResult<Decision> {
    let list = |key: &str| -> StopResult<Vec<i64>> {
        let field = schema::field(value, key)?;
        Ok(if strict {
            schema::strict_int_list(field)?
        } else {
            schema::lax_int_list(field)?
        })
    };
    Ok(Decision::Model(
        list("duplicate_facts")?,
        list("contradicted_facts")?,
    ))
}

/// `CoreAttributeBatch.model_validate(result, strict=True)` per row index.
fn parse_attribute_batch(
    value: &Value,
    count: usize,
    fields: &[(String, String)],
) -> StopResult<Rows> {
    let invalid = || Stop::Rejected("model result was not an object");
    let object = value.as_object().ok_or_else(invalid)?;
    if object.keys().any(|key| key != "entities") {
        return Err(invalid());
    }
    let rows = object
        .get("entities")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let mut parsed = HashMap::new();
    for row in rows {
        let row = row.as_object().ok_or_else(invalid)?;
        if row.keys().any(|key| key != "index" && key != "attributes") {
            return Err(invalid());
        }
        let index = row
            .get("index")
            .and_then(Value::as_i64)
            .filter(|_| row.get("index").is_some_and(Value::is_i64))
            .ok_or_else(invalid)?;
        let attributes = row
            .get("attributes")
            .and_then(Value::as_object)
            .ok_or_else(invalid)?;
        let mut values = Vec::new();
        for (name, _) in fields {
            let value = match attributes.get(name) {
                None | Some(Value::Null) => None,
                Some(Value::String(text)) => Some(text.clone()),
                Some(_) => return Err(invalid()),
            };
            values.push((name.clone(), value));
        }
        let Ok(index) = usize::try_from(index) else {
            return Err(invalid());
        };
        if index >= count || parsed.contains_key(&index) {
            return Err(invalid());
        }
        parsed.insert(index, values);
    }
    Ok(parsed)
}

/// Stage-level access for the golden tests, which drive each stage with the
/// same inputs the Python worker's functions took.
#[cfg(test)]
pub(super) struct Harness<'a, 'b> {
    run: Run<'a, 'b>,
}

#[cfg(test)]
impl<'a, 'b> Harness<'a, 'b> {
    pub(super) fn new(
        bridge: &'a mut Bridge<'b>,
        schema: &'a Schema,
        episode: Episode,
        previous: Vec<PreviousEpisode>,
        instructions: String,
        graph: Graph,
        text: &'a dyn ClaimText,
    ) -> Self {
        Self {
            run: Run {
                bridge,
                schema,
                episode,
                previous,
                instructions,
                graph,
                text,
                nodes: Vec::new(),
                edges: Vec::new(),
                response_models: HashMap::new(),
            },
        }
    }

    pub(super) fn node(&self, index: usize) -> &Node {
        &self.run.nodes[index]
    }

    pub(super) fn edge(&self, index: usize) -> &Edge {
        &self.run.edges[index]
    }

    pub(super) fn add_node(&mut self, node: Node) -> usize {
        self.run.push_node(node)
    }

    pub(super) fn add_edge(&mut self, edge: Edge) -> usize {
        self.run.push_edge(edge)
    }

    pub(super) fn extract_nodes(&mut self) -> StopResult<Vec<usize>> {
        self.run.extract_nodes()
    }

    /// One hybrid edge search over the stored World, as resolution runs it.
    pub(super) fn search_edges(
        &mut self,
        query: &str,
        vector: &[f32],
        allowed: Option<&HashSet<String>>,
    ) -> Vec<Edge> {
        self.run
            .edge_search(query, vector, allowed)
            .into_iter()
            .map(|index| self.run.edges[index].clone())
            .collect()
    }

    pub(super) fn resolve_with_candidates(
        &mut self,
        extracted: &[usize],
        candidates: Vec<Vec<Node>>,
    ) -> StopResult<(Vec<usize>, HashMap<String, String>)> {
        self.run.resolve_with_candidates(extracted, candidates)
    }

    pub(super) fn extract_edges(&mut self, nodes: &[usize]) -> StopResult<Vec<usize>> {
        self.run.extract_edges(nodes)
    }

    #[allow(clippy::type_complexity)]
    pub(super) fn decide_edges(
        &mut self,
        extracted: &[usize],
        related: Vec<Vec<usize>>,
        others: Vec<Vec<usize>>,
    ) -> StopResult<(Vec<usize>, Vec<usize>, Vec<usize>)> {
        self.run.decide_edges(extracted, related, others)
    }

    pub(super) fn attributes_and_summaries(
        &mut self,
        nodes: &[usize],
        new_edges: &[usize],
    ) -> StopResult<()> {
        self.run.attributes_and_summaries(nodes, new_edges)
    }
}
