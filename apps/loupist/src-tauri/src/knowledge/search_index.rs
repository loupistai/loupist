//! Incremental lexical index for the visible Source, Lens, and World revision.
//!
//! Rows are written when a revision is published. Search reads this
//! table and the caller's owned records; it does not walk the blob tree.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::{params, Transaction};
use serde::Deserialize;

use super::blob::BlobStore;
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    PublishedRepresentation, PublishedSpan, RecordKind, Selector, SourceRecordPayload,
    WorldSnapshotBody,
};

const INDEXED_TEXT_CHARS: usize = 64 * 1024;
pub const SEARCH_LIMIT: usize = 20;
/// Ranked discovery window; the extra item detects a bounded result set.
pub const SEARCH_WINDOW_LIMIT: usize = 201;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub kind: String,
    pub record_id: String,
    pub revision_id: String,
    pub span_id: String,
    pub object_id: String,
    pub coverage: String,
    pub title: String,
    pub snippet: String,
    /// Ranking only, by rank fusion when a query has terms: it orders hits
    /// and does not measure how much of the query a hit matched. Search
    /// discoveries still need an acknowledged exact read.
    pub score: u32,
    pub matched_terms: Vec<String>,
    pub retrieval: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub record_ids: Vec<String>,
    /// Exact structured identifiers/labels only; missing metadata does not match.
    pub companies: Vec<String>,
    pub periods: Vec<String>,
    pub source_types: Vec<String>,
    pub include_metadata: bool,
    /// The loaded bundled embedding model; no network or provider calls are
    /// made. Without it, search ranks by words and Map names alone.
    pub semantic_model: Option<super::LocalModel>,
}

/// Owned authorized inputs, with no SQLite transaction or folder lock.
pub struct PreparedSearch {
    candidates: Vec<Candidate>,
    payloads: BTreeMap<String, String>,
    groups: Vec<Group>,
    /// Each Map entity's spellings, for a query that names one of them.
    entities: Arc<Vec<Vec<String>>>,
    query: String,
    limit: usize,
    options: SearchOptions,
}

struct Candidate {
    document: SearchDocument,
    aliases: String,
}

pub struct RankedSearch {
    pub documents: Vec<SearchDocument>,
    cache: Option<super::search_embeddings::EmbeddingCache>,
    cache_path: Option<PathBuf>,
}

impl RankedSearch {
    /// The host supplies freshly authorized visible revisions after scoring.
    /// Cache contents never decide which records are visible.
    pub fn retain_current(&mut self, revisions: &BTreeSet<String>) {
        self.documents
            .retain(|document| revisions.contains(&document.revision_id));
    }

    /// Runs outside the canonical DB transaction; this disposable cache is best effort.
    pub fn prune_cache(&mut self, revisions: &BTreeSet<String>) {
        if let Some(cache) = &mut self.cache {
            let _ = cache.prune(revisions);
        } else if let Some(path) = &self.cache_path {
            let _ = super::search_embeddings::EmbeddingCache::prune_existing(path, revisions);
        }
    }
}

struct Draft {
    kind: &'static str,
    span_id: String,
    object_id: String,
    coverage: &'static str,
    title: String,
    body: String,
    aliases: String,
}

pub(super) fn index_payload(
    tx: &Transaction<'_>,
    blobs: Option<&BlobStore>,
    record_id: &str,
    revision_id: &str,
    kind: RecordKind,
    title: &str,
    payload: &serde_json::Value,
) -> KnowledgeResult<()> {
    tx.execute(
        "DELETE FROM knowledge_search_documents WHERE record_id = ?1 AND revision_id = ?2",
        params![record_id, revision_id],
    )?;
    let mut insert = tx.prepare(
        "INSERT INTO knowledge_search_documents (
            record_id, revision_id, kind, span_id, object_id, coverage, title, body, aliases,
            search_text
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for draft in documents(blobs, kind, title, payload)? {
        // Rust owns these lists (Knowledge schema 7 dropped the SQL whitelists).
        if !matches!(draft.kind, "source" | "lens" | "world")
            || !matches!(
                draft.coverage,
                "included_full" | "metadata_only" | "snippet"
            )
        {
            return Err(KnowledgeError::Invalid("search document is invalid"));
        }
        let search_text = fold(&format!(
            "{}\n{}\n{}",
            draft.title, draft.body, draft.aliases
        ));
        insert.execute(params![
            record_id,
            revision_id,
            draft.kind,
            draft.span_id,
            draft.object_id,
            draft.coverage,
            draft.title,
            draft.body,
            draft.aliases,
            search_text
        ])?;
    }
    Ok(())
}

pub(super) fn prepare_visible(
    tx: &Transaction<'_>,
    loupist_id: &str,
    scope: &str,
    needle: &str,
    limit: usize,
    options: &SearchOptions,
) -> KnowledgeResult<PreparedSearch> {
    if needle.chars().count() > 400 {
        return Err(KnowledgeError::Invalid("knowledge query is too long"));
    }
    if !matches!(scope, "all" | "source" | "lens" | "world") {
        return Err(KnowledgeError::Invalid("unknown search scope"));
    }
    for filter in [
        &options.record_ids,
        &options.companies,
        &options.periods,
        &options.source_types,
    ] {
        if filter.len() > 16
            || filter
                .iter()
                .any(|value| value.is_empty() || value.chars().count() > 128)
        {
            return Err(KnowledgeError::Invalid("knowledge filters are invalid"));
        }
    }
    let groups = query_groups(needle);
    if groups.is_empty() && needle.trim() != "*" {
        return Ok(PreparedSearch {
            candidates: Vec::new(),
            payloads: BTreeMap::new(),
            groups,
            entities: Arc::default(),
            query: needle.into(),
            limit,
            options: options.clone(),
        });
    }
    // The Map's names, parsed once per World revision. A World that cannot be
    // read only costs cross-language matches.
    let entities = if groups.is_empty() {
        Arc::default()
    } else {
        super::lens::world_identity(tx, loupist_id, |world| world.names())
            .ok()
            .flatten()
            .unwrap_or_default()
    };

    // Loupist ownership and current revisions are applied before either lexical
    // scoring or embedding. A semantic cache can never widen authority.
    let structured = !options.companies.is_empty()
        || !options.periods.is_empty()
        || !options.source_types.is_empty();
    // A World payload contains every claim. Joining that payload into every
    // sorted claim row can spill gigabytes to SQLite's temporary storage.
    // Keep the sort narrow and decode each current revision once, on demand.
    let mut payloads = BTreeMap::new();
    let mut statement = tx.prepare(
        "SELECT d.kind, d.record_id, d.revision_id, d.span_id, d.object_id, d.coverage, d.title, d.body, d.aliases
         FROM knowledge_search_documents d
         JOIN knowledge_records m ON m.record_id = d.record_id AND m.owner_loupist_id = ?1
         JOIN knowledge_record_heads h ON h.record_id = d.record_id
         JOIN knowledge_record_revisions r ON r.revision_id = d.revision_id
         WHERE d.revision_id = h.revision_id
           AND (?2 = 'all' OR d.kind = ?2)
           AND NOT EXISTS (
             SELECT 1 FROM knowledge_lens_metadata meta
             WHERE meta.record_id = d.record_id AND meta.archived_at_ms IS NOT NULL
           )
         ORDER BY d.record_id, d.kind, d.span_id, d.object_id",
    )?;
    let mut rows = statement.query(params![loupist_id, scope])?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        let record_id: String = row.get(1)?;
        let coverage: String = row.get(5)?;
        if (!options.record_ids.is_empty() && !options.record_ids.contains(&record_id))
            || (!options.include_metadata && coverage == "metadata_only")
        {
            continue;
        }
        let kind: String = row.get(0)?;
        let title: String = row.get(6)?;
        let body: String = row.get(7)?;
        let aliases: String = row.get(8)?;
        let revision_id: String = row.get(2)?;
        if structured && !payloads.contains_key(&revision_id) {
            let payload: String = tx.query_row(
                "SELECT payload_json FROM knowledge_record_revisions WHERE revision_id = ?1",
                [&revision_id],
                |row| row.get(0),
            )?;
            payloads.insert(revision_id.clone(), payload);
        }
        candidates.push(Candidate {
            aliases,
            document: SearchDocument {
                kind,
                record_id,
                revision_id,
                span_id: row.get(3)?,
                object_id: row.get(4)?,
                coverage,
                title,
                // Delay excerpt construction until the ranked page is selected.
                snippet: body,
                score: 0,
                matched_terms: Vec::new(),
                retrieval: "lexical".into(),
            },
        });
    }
    Ok(PreparedSearch {
        candidates,
        payloads,
        groups,
        entities,
        query: needle.into(),
        limit,
        options: options.clone(),
    })
}

pub(super) fn visible_revisions(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeSet<String>> {
    let mut statement = tx.prepare(
        "SELECT h.revision_id FROM knowledge_record_heads h
         JOIN knowledge_records m ON m.record_id = h.record_id AND m.owner_loupist_id = ?1
         WHERE NOT EXISTS (SELECT 1 FROM knowledge_lens_metadata meta
             WHERE meta.record_id = h.record_id AND meta.archived_at_ms IS NOT NULL)",
    )?;
    let rows = statement.query_map([loupist_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<BTreeSet<String>, _>>()?)
}

impl PreparedSearch {
    /// Tokenization, model loading, filtering, scoring and excerpts run without
    /// holding a canonical connection. An unavailable cache only costs computation.
    pub fn rank(self, cache_path: Option<&std::path::Path>) -> RankedSearch {
        let Self {
            candidates,
            payloads,
            mut groups,
            entities,
            query,
            limit,
            options,
        } = self;
        if groups.is_empty() && query.trim() != "*" {
            return RankedSearch {
                documents: Vec::new(),
                cache: None,
                cache_path: cache_path.map(std::path::Path::to_path_buf),
            };
        }
        let payloads: BTreeMap<_, serde_json::Value> = payloads
            .into_iter()
            .map(|(revision, text)| (revision, serde_json::from_str(&text).unwrap_or_default()))
            .collect();
        let candidates: Vec<Candidate> = candidates
            .into_iter()
            .filter(|Candidate { document, .. }| {
                payloads.get(&document.revision_id).is_none_or(|payload| {
                    structured_matches(
                        &document.kind,
                        &document.object_id,
                        &document.snippet,
                        payload,
                        &options,
                    )
                })
            })
            .collect();
        name_entities(&mut groups, &query, &entities, |stem, names| {
            uses_short_form_for(&candidates, stem, names)
        });
        let query_vector = options
            .semantic_model
            .as_ref()
            .filter(|_| !groups.is_empty())
            .filter(|_| {
                candidates
                    .iter()
                    .any(|candidate| candidate.document.coverage != "metadata_only")
            })
            .and_then(|model| model.embed(&query).ok());
        let mut cache = options
            .semantic_model
            .as_ref()
            .zip(cache_path)
            .filter(|_| query_vector.is_some())
            .and_then(|(model, path)| {
                super::search_embeddings::EmbeddingCache::open(
                    path,
                    model.fingerprint(),
                    model.dimension(),
                )
                .ok()
            });
        let mut hits = Vec::new();
        for Candidate {
            mut document,
            aliases,
        } in candidates
        {
            (document.score, document.matched_terms) =
                lexical_score(&document.title, &document.snippet, &aliases, &groups);
            hits.push(document);
        }
        let mut similarity: Vec<Option<f32>> = vec![None; hits.len()];
        if let Some((query, model)) = query_vector.as_ref().zip(options.semantic_model.as_ref()) {
            // Sources, Lens and World claims all carry vectors; metadata rows do not.
            let embedded: Vec<usize> = (0..hits.len())
                .filter(|index| hits[*index].coverage != "metadata_only")
                .collect();
            for batch in embedded.chunks(128) {
                let texts: Vec<String> = batch
                    .iter()
                    .map(|index| {
                        let hit = &hits[*index];
                        format!("{}\n{}", hit.title, hit.snippet)
                            .chars()
                            .take(8_000)
                            .collect()
                    })
                    .collect();
                let revisions: Vec<&str> = batch
                    .iter()
                    .map(|index| hits[*index].revision_id.as_str())
                    .collect();
                let found = cached_similarities(model, query, &texts, &revisions, cache.as_mut());
                for (index, value) in batch.iter().zip(found) {
                    similarity[*index] = value;
                }
            }
        }
        for hit in &mut hits {
            if hit.coverage == "metadata_only" {
                hit.score /= 2;
            }
        }
        if !groups.is_empty() {
            fuse_ranks(&mut hits, &similarity);
        }
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.record_id.cmp(&b.record_id))
                .then(a.span_id.cmp(&b.span_id))
                .then(a.object_id.cmp(&b.object_id))
        });
        hits.truncate(limit.clamp(1, SEARCH_WINDOW_LIMIT));
        for hit in &mut hits {
            let excerpt = query_excerpt(&hit.snippet, &groups, 800);
            if hit.coverage == "included_full" && excerpt != hit.snippet {
                hit.coverage = "snippet".into();
            }
            hit.snippet = excerpt;
        }
        RankedSearch {
            documents: hits,
            cache,
            cache_path: cache_path.map(std::path::Path::to_path_buf),
        }
    }
}

/// Whether this library uses `stem` mostly for the entity `names` spell: at
/// least half of the Sources and Lens pages using it also spell one of the
/// names in full. The Map's own claims spell every name, so they are no
/// evidence.
fn uses_short_form_for(candidates: &[Candidate], stem: &str, names: &[Name]) -> bool {
    // Only a text that writes the stem in one of its usual casings is folded
    // and checked exactly, so this pass over the library stays cheap.
    let mut casings = vec![stem.to_owned()];
    if stem.is_ascii() {
        casings.push(stem[..1].to_ascii_uppercase() + &stem[1..]);
        casings.push(stem.to_ascii_uppercase());
    }
    let mut records: BTreeMap<&str, bool> = BTreeMap::new();
    for Candidate { document, aliases } in candidates
        .iter()
        .filter(|candidate| candidate.document.kind != "world")
    {
        let fields = [
            document.title.as_str(),
            document.snippet.as_str(),
            aliases.as_str(),
        ];
        if !fields
            .iter()
            .any(|text| casings.iter().any(|casing| text.contains(casing.as_str())))
        {
            continue;
        }
        let folded = fields.map(fold);
        let uses = folded.iter().any(|text| {
            if stem.is_ascii() {
                contains_term(text, stem)
            } else {
                text.contains(stem)
            }
        });
        if uses {
            let spells = fields
                .iter()
                .zip(&folded)
                .any(|(raw, folded)| names.iter().any(|name| name.found_in(folded, raw)));
            *records.entry(&document.record_id).or_default() |= spells;
        }
    }
    let spelling = records.values().filter(|spells| **spells).count();
    !records.is_empty() && spelling * 2 >= records.len()
}

fn structured_matches(
    kind: &str,
    object_id: &str,
    body: &str,
    payload: &serde_json::Value,
    options: &SearchOptions,
) -> bool {
    use serde_json::Value;
    let mut companies = BTreeSet::new();
    let mut periods = BTreeSet::new();
    let mut types = BTreeSet::new();
    fn text(set: &mut BTreeSet<String>, value: Option<&Value>) {
        if let Some(value) = value {
            match value {
                Value::String(value) => {
                    set.insert(fold(value));
                }
                Value::Number(value) => {
                    set.insert(value.to_string());
                }
                Value::Array(items) => {
                    for item in items {
                        text(set, Some(item));
                    }
                }
                _ => {}
            }
        }
    }
    fn fields(
        value: &Value,
        companies: &mut BTreeSet<String>,
        periods: &mut BTreeSet<String>,
        types: &mut BTreeSet<String>,
        depth: usize,
    ) {
        if depth > 12 {
            return;
        }
        match value {
            Value::Object(object) => {
                for key in [
                    "entity_id",
                    "entityId",
                    "corp_code",
                    "cik",
                    "symbol",
                    "ticker",
                    "stock_code",
                ] {
                    text(companies, object.get(key));
                }
                if let Some(company) = object.get("company") {
                    text(companies, Some(company));
                    if let Some(company) = company.as_object() {
                        for key in ["name", "symbol", "ticker", "corp_code", "cik", "stock_code"] {
                            text(companies, company.get(key));
                        }
                    }
                }
                for key in [
                    "period",
                    "period_start",
                    "period_end",
                    "fiscal_year",
                    "report_period",
                ] {
                    text(periods, object.get(key));
                }
                for key in [
                    "source_class",
                    "sourceClass",
                    "media_type",
                    "mediaType",
                    "form",
                    "document_type",
                ] {
                    text(types, object.get(key));
                }
                for child in object.values() {
                    fields(child, companies, periods, types, depth + 1);
                }
            }
            Value::Array(items) => {
                for item in items {
                    fields(item, companies, periods, types, depth + 1);
                }
            }
            _ => {}
        }
    }
    if kind == "source" {
        if let Some(source) = payload
            .get("source")
            .filter(|source| source.get("sourceId").and_then(Value::as_str) == Some(object_id))
        {
            text(&mut types, source.get("sourceClass"));
            fields(source, &mut companies, &mut periods, &mut types, 0);
        }
        // Parse retained structured result bytes only. Never infer tags from prose.
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            fields(&value, &mut companies, &mut periods, &mut types, 0);
        }
    } else if kind == "world" {
        if let Some(claim) = payload
            .get("claims")
            .and_then(Value::as_array)
            .and_then(|claims| {
                claims
                    .iter()
                    .find(|claim| claim.get("id").and_then(Value::as_str) == Some(object_id))
            })
        {
            fields(claim, &mut companies, &mut periods, &mut types, 0);
            if let Some(endpoints) = claim.get("endpoints").and_then(Value::as_array) {
                for endpoint in endpoints {
                    let node_id = endpoint.get("nodeId").and_then(Value::as_str);
                    if let Some(node) =
                        payload
                            .get("nodes")
                            .and_then(Value::as_array)
                            .and_then(|nodes| {
                                nodes
                                    .iter()
                                    .find(|node| node.get("id").and_then(Value::as_str) == node_id)
                            })
                    {
                        fields(node, &mut companies, &mut periods, &mut types, 0);
                        if node
                            .get("kind")
                            .and_then(Value::as_str)
                            .is_some_and(|kind| {
                                fold(kind).contains("company")
                                    || fold(kind).contains("organization")
                            })
                        {
                            text(&mut companies, node.get("name"));
                            text(&mut companies, node.get("aliases"));
                        }
                    }
                }
            }
        }
    }
    let matches = |requested: &[String], candidates: &BTreeSet<String>| {
        requested.is_empty()
            || requested
                .iter()
                .any(|value| candidates.contains(&fold(value)))
    };
    matches(&options.companies, &companies)
        && matches(&options.periods, &periods)
        && matches(&options.source_types, &types)
}

/// The vector list fused with the lexical order: the ten nearest texts from
/// cosine 0.2.
const VECTOR_NEIGHBOURS: usize = 10;
const VECTOR_FLOOR: f32 = 0.2;
/// A fused score is its reciprocal-rank sum times this: first in one list is
/// 1,000,000, first in both 2,000,000, tenth in one 100,000.
const FUSED_SCALE: f64 = 1_000_000.0;

/// Reciprocal rank fusion of the lexical order and the nearest vectors,
/// Graphiti's 1/rank as the Map index fuses BM25 and vectors: a
/// cross-language Source the model puts near the query is not buried under a
/// same-language Source that shares one query word. Each hit's `score`
/// becomes its fused score; a hit in neither list is dropped.
fn fuse_ranks(hits: &mut Vec<SearchDocument>, similarity: &[Option<f32>]) {
    let lexical: Vec<_> = hits
        .iter()
        .map(|hit| (hit.score > 0).then_some(f64::from(hit.score)))
        .collect();
    let vector: Vec<_> = similarity
        .iter()
        .map(|value| value.filter(|value| *value >= VECTOR_FLOOR).map(f64::from))
        .collect();
    let lexical = shared_ranks(&lexical, usize::MAX);
    let vector = shared_ranks(&vector, VECTOR_NEIGHBOURS);
    for ((hit, lexical), vector) in hits.iter_mut().zip(&lexical).zip(&vector) {
        let fused = lexical.map_or(0.0, |rank| 1.0 / rank as f64)
            + vector.map_or(0.0, |rank| 1.0 / rank as f64);
        hit.retrieval = match (lexical, vector) {
            (Some(_), Some(_)) => "hybrid",
            (None, Some(_)) => "semantic",
            _ => "lexical",
        }
        .into();
        hit.score = (fused * FUSED_SCALE).round() as u32;
    }
    hits.retain(|hit| hit.score > 0);
}

/// Each value's rank among the present ones, highest first. Equal values
/// share a rank, one more than the number of higher values, so the order rows
/// were read in never decides one. Ranks past `top` are left out.
fn shared_ranks(values: &[Option<f64>], top: usize) -> Vec<Option<usize>> {
    let mut present: Vec<f64> = values.iter().flatten().copied().collect();
    present.sort_by(|a, b| b.total_cmp(a));
    values
        .iter()
        .map(|value| {
            value.and_then(|value| {
                let rank = 1 + present.partition_point(|other| *other > value);
                (rank <= top).then_some(rank)
            })
        })
        .collect()
}

fn cached_similarities(
    model: &super::LocalModel,
    query: &[f32],
    texts: &[String],
    revisions: &[&str],
    mut cache: Option<&mut super::search_embeddings::EmbeddingCache>,
) -> Vec<Option<f32>> {
    let keys: Vec<String> = texts
        .iter()
        .map(|text| crate::util::sha256(text.as_bytes()))
        .collect();
    let mut vectors: Vec<Option<Vec<f32>>> = keys
        .iter()
        .map(|key| cache.as_ref().and_then(|cache| cache.get(key)))
        .collect();
    let missing: Vec<usize> = vectors
        .iter()
        .enumerate()
        .filter_map(|(index, vector)| vector.is_none().then_some(index))
        .collect();
    if !missing.is_empty() {
        let inputs: Vec<String> = missing.iter().map(|index| texts[*index].clone()).collect();
        if let Ok(loaded) = model.embed_batch(&inputs) {
            for (index, vector) in missing.into_iter().zip(loaded) {
                vectors[index] = Some(vector);
            }
        }
    }
    if let Some(cache) = cache.as_mut() {
        let rows: Vec<_> = revisions
            .iter()
            .zip(&keys)
            .zip(&vectors)
            .filter_map(|((revision, key), vector)| {
                vector
                    .as_ref()
                    .map(|vector| (*revision, key.as_str(), vector.as_slice()))
            })
            .collect();
        let _ = cache.remember(&rows);
    }
    vectors
        .into_iter()
        .map(|vector| {
            let vector = vector?;
            let dot: f32 = query.iter().zip(&vector).map(|(a, b)| a * b).sum();
            let norm = (query.iter().map(|a| a * a).sum::<f32>()
                * vector.iter().map(|a| a * a).sum::<f32>())
            .sqrt();
            (norm > 0.0).then_some(dot / norm)
        })
        .collect()
}

// Small audited concept aliases, not entity inference. Company/ticker aliases
// continue to come from retained titles, context notes, and World node aliases.
const CONCEPTS: &[&[&str]] = &[
    &["operating profit", "operating income", "영업이익"],
    &["revenue", "sales", "매출", "매출액"],
    &["free cash flow", "fcf", "잉여현금흐름"],
    &["cash flow", "현금흐름"],
    &["drawdown", "낙폭", "최대낙폭"],
    &["volatility", "변동성"],
    &["valuation", "기업가치", "가치평가"],
    &["portfolio", "포트폴리오"],
    &["backtest", "backtesting", "백테스트"],
    &["benchmark", "벤치마크"],
    &["dividend", "배당"],
    &["earnings", "순이익"],
    &["risk", "위험", "리스크"],
    &["correlation", "상관관계", "상관계수"],
    &["liquidity", "유동성"],
];

/// One query word, or a whole concept or entity name, as alternatives: any
/// of them in a document counts as the word.
#[derive(Debug, Clone, Default)]
struct Group {
    /// Folded words and concept aliases; the first is the reported match.
    terms: Vec<String>,
    /// The Map's other spellings of an entity this query word names.
    names: Vec<Name>,
    /// Every name, folded, as one pattern: a single pass tells whether a
    /// text can hold any of them before each is checked as a whole name.
    any_name: Option<regex::Regex>,
}

impl Group {
    fn new(terms: Vec<String>) -> Self {
        Self {
            terms,
            ..Self::default()
        }
    }

    fn found_in(&self, folded: &str, raw: &str) -> bool {
        self.terms.iter().any(|term| contains_term(folded, term))
            || (self
                .any_name
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(folded))
                && self.names.iter().any(|name| name.found_in(folded, raw)))
    }

    /// Adds the names that can match where the group's own words do not: a
    /// name holding one of them (`hanbit semiconductor` for `hanbit`) adds
    /// nothing but another pass over every document.
    fn add_names(&mut self, names: &[Name]) {
        for name in names {
            let text = fold(&name.text);
            if !self.names.contains(name)
                && !self.terms.iter().any(|term| contains_term(&text, term))
            {
                self.names.push(name.clone());
            }
        }
        let pattern = self
            .names
            .iter()
            .map(|name| regex::escape(&fold(&name.text)))
            .collect::<Vec<_>>()
            .join("|");
        self.any_name = (!self.names.is_empty())
            .then(|| regex::Regex::new(&pattern).ok())
            .flatten();
    }
}

/// One spelling of a Map entity, matched as a whole name: not inside a
/// longer word, except before Korean particles (`한빛반도체가`) and in Han or
/// kana text, which does not separate words.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Name {
    /// Folded, except a ticker's.
    text: String,
    /// An upper-case code (`ARCQ`) matches only in upper case, so a ticker
    /// spelled like a word (`ALL`, `KEY`) does not match the word.
    ticker: bool,
}

impl Name {
    /// None for a spelling too short to name one entity: a word under three
    /// letters, one Hangul syllable, or a number under six digits, which is
    /// more often a quantity or a year than a listing code.
    fn new(spelling: &str) -> Option<Self> {
        let spelling = spelling.split_whitespace().collect::<Vec<_>>().join(" ");
        let length = spelling.chars().count();
        if spelling.chars().all(|ch| ch.is_ascii_digit()) {
            return (length >= 6).then_some(Self {
                text: spelling,
                ticker: false,
            });
        }
        if spelling.is_ascii() {
            let ticker = length <= 5
                && !spelling.contains(' ')
                && spelling.chars().any(|ch| ch.is_ascii_uppercase())
                && !spelling.chars().any(|ch| ch.is_ascii_lowercase());
            return (length >= 3).then(|| Self {
                text: if ticker {
                    spelling.clone()
                } else {
                    fold(&spelling)
                },
                ticker,
            });
        }
        (length >= 2).then(|| Self {
            text: fold(&spelling),
            ticker: false,
        })
    }

    fn found_in(&self, folded: &str, raw: &str) -> bool {
        let text = if self.ticker { raw } else { folded };
        text.match_indices(self.text.as_str())
            .any(|(start, _)| whole_name(text, start, &self.text))
    }
}

fn is_hangul(ch: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&ch)
}

fn is_han_or_kana(ch: char) -> bool {
    matches!(ch, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}')
}

/// A letter or digit of a script that puts spaces between words.
fn word_char(ch: char) -> bool {
    ch.is_alphanumeric() && !is_hangul(ch) && !is_han_or_kana(ch)
}

/// Syllables Korean particles are written with, so `한빛반도체가` and
/// `아카디아에서는` name the entity while `하이브리드` does not name `하이브`.
const PARTICLE_SYLLABLES: &str = "은는이가을를의에서게께와과도만로으랑하고까지부터보다처럼나며측들";

/// Whether `name`, found in `text` at `start`, is a whole name there.
fn whole_name(text: &str, start: usize, name: &str) -> bool {
    let (Some(first), Some(last)) = (name.chars().next(), name.chars().next_back()) else {
        return false;
    };
    let before = text[..start].chars().next_back();
    let mut after = text[start + name.len()..].chars();
    let starts = is_han_or_kana(first)
        || !before.is_some_and(|ch| word_char(ch) || (is_hangul(first) && is_hangul(ch)));
    let ends = if is_han_or_kana(last) {
        true
    } else if is_hangul(last) {
        let tail: Vec<char> = after.take_while(|ch| is_hangul(*ch)).collect();
        tail.len() <= 4 && tail.iter().all(|ch| PARTICLE_SYLLABLES.contains(*ch))
    } else {
        !after.next().is_some_and(word_char)
    };
    starts && ends
}

/// The start of `word` that is a short form of `name`: the first word of a
/// name of several (`hanbit`, `노스윈드`), or the start of a Korean name
/// written as one word (`파인크레스트` of `파인크레스트제약`), with any
/// particle after it dropped. At least four letters or two syllables.
fn short_form<'a>(word: &'a str, name: &str) -> Option<&'a str> {
    let ends: Vec<usize> = if word.is_ascii() {
        vec![word.len()]
    } else {
        // The word, then the word without one to four trailing particle syllables.
        let mut ends = vec![word.len()];
        for (offset, ch) in word.char_indices().rev().take(4) {
            if !PARTICLE_SYLLABLES.contains(ch) {
                break;
            }
            ends.push(offset);
        }
        ends
    };
    ends.into_iter().map(|end| &word[..end]).find(|stem| {
        let long = if stem.is_ascii() {
            stem.len() >= 4 && stem.chars().all(|ch| ch.is_ascii_alphabetic())
        } else {
            stem.chars().count() >= 2 && stem.chars().all(is_hangul)
        };
        long && *stem != name
            && match name.split_once(' ') {
                Some((first, _)) => first == *stem,
                None => !stem.is_ascii() && name.starts_with(*stem),
            }
    })
}

/// Extends the query's groups with the other spellings of each Map entity the
/// query names, so a Korean name finds English text that spells the entity in
/// English and the reverse. A whole name counts first, the longest where two
/// overlap, then a word that is a short form of one entity's name where
/// `distinctive(stem, names)` holds for this library. A spelling two entities
/// share names neither.
fn name_entities(
    groups: &mut [Group],
    query: &str,
    entities: &[Vec<String>],
    distinctive: impl Fn(&str, &[Name]) -> bool,
) {
    if entities.is_empty() || groups.is_empty() {
        return;
    }
    let folded = fold(query);
    let names: Vec<Vec<Name>> = entities
        .iter()
        .map(|spellings| {
            let mut names: Vec<Name> = spellings.iter().filter_map(|s| Name::new(s)).collect();
            names.sort();
            names.dedup();
            names
        })
        .collect();
    let mut owners: BTreeMap<&Name, usize> = BTreeMap::new();
    for name in names.iter().flatten() {
        *owners.entry(name).or_default() += 1;
    }
    let own = |index: usize| -> Vec<Name> {
        names[index]
            .iter()
            .filter(|name| owners[name] == 1)
            .cloned()
            .collect()
    };
    let mut found: Vec<(usize, usize, usize)> = Vec::new();
    for (index, entity) in names.iter().enumerate() {
        for name in entity.iter().filter(|name| owners[name] == 1) {
            let text = if name.ticker { query } else { &folded };
            for (start, _) in text.match_indices(name.text.as_str()) {
                if whole_name(text, start, &name.text) {
                    found.push((name.text.len(), start, index));
                }
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for (length, start, index) in found {
        let end = start + length;
        if taken.iter().any(|(from, to)| start < *to && *from < end) {
            continue;
        }
        taken.push((start, end));
        let alternatives = own(index);
        // The query words the name was written with, particles included.
        for group in groups.iter_mut() {
            if group.terms.iter().any(|term| {
                folded
                    .match_indices(term.as_str())
                    .any(|(at, _)| at < end && start < at + term.len())
            }) {
                group.add_names(&alternatives);
            }
        }
    }
    for group in groups
        .iter_mut()
        .filter(|group| group.names.is_empty() && group.terms.len() == 1)
    {
        let word = group.terms[0].clone();
        let mut owner = None;
        let mut stem = "";
        let mut ambiguous = false;
        for (index, entity) in names.iter().enumerate() {
            if let Some(found) = entity
                .iter()
                .filter(|name| !name.ticker)
                .find_map(|name| short_form(&word, &name.text))
            {
                ambiguous |= owner.is_some_and(|owner| owner != index);
                owner = Some(index);
                stem = found;
            }
        }
        let Some(index) = owner.filter(|_| !ambiguous) else {
            continue;
        };
        let alternatives = own(index);
        if !alternatives.is_empty() && distinctive(stem, &alternatives) {
            group.add_names(&alternatives);
        }
    }
}

/// How many term groups search splits `query` into; a hit's `matched_terms`
/// lists one term per group it matched.
pub fn query_group_count(query: &str) -> usize {
    query_groups(query).len()
}

fn query_groups(query: &str) -> Vec<Group> {
    let mut remaining = fold(query);
    let mut groups = Vec::new();
    for aliases in CONCEPTS {
        if aliases.iter().any(|alias| contains_term(&remaining, alias)) {
            groups.push(Group::new(
                aliases.iter().map(|alias| alias.to_string()).collect(),
            ));
            for alias in *aliases {
                remaining = remaining.replace(alias, " ");
            }
        }
    }
    let stop = [
        "the",
        "a",
        "an",
        "and",
        "of",
        "in",
        "to",
        "for",
        "is",
        "with",
        "what",
        "how",
        "please",
        "대한",
        "관련",
        "알려줘",
        "분석해줘",
    ];
    let mut seen = BTreeSet::new();
    for term in remaining.split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '_' | '.' | '-'))
    {
        let term = term.trim_matches(['.', '-', '_']);
        if !term.is_empty() && !stop.contains(&term) && seen.insert(term.to_string()) {
            groups.push(Group::new(vec![term.to_string()]));
        }
    }
    groups.truncate(32);
    groups
}

fn contains_term(text: &str, term: &str) -> bool {
    text.match_indices(term).any(|(offset, _)| {
        if !term.is_ascii() {
            return true;
        }
        let before = text[..offset].chars().next_back();
        let after = text[offset + term.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_ascii_alphanumeric())
            && !after.is_some_and(|ch| ch.is_ascii_alphanumeric())
    })
}

fn lexical_score(title: &str, body: &str, aliases: &str, groups: &[Group]) -> (u32, Vec<String>) {
    let fields =
        [(title, 140), (aliases, 110), (body, 80)].map(|(raw, weight)| (raw, fold(raw), weight));
    let mut score = 0;
    let mut matched = Vec::new();
    for group in groups {
        let weight = fields
            .iter()
            .find(|(raw, folded, _)| group.found_in(folded, raw))
            .map_or(0, |(_, _, weight)| *weight);
        if weight > 0 {
            matched.push(group.terms[0].clone());
            score += weight;
        }
    }
    // Matching the entire request dominates a document that repeats one word.
    if !matched.is_empty() {
        score += (300 * matched.len() / groups.len()) as u32;
    }
    if matched.len() == groups.len() {
        score += 300;
    }
    (score, matched)
}

fn query_excerpt(body: &str, groups: &[Group], budget: usize) -> String {
    if body.chars().count() <= budget {
        return body.to_string();
    }
    let lines: Vec<&str> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let ranked = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let (score, _) = lexical_score("", line, "", groups);
            (index, score)
        })
        .max_by_key(|(index, score)| (*score, std::cmp::Reverse(*index)));
    let index = ranked.map(|(index, _)| index).unwrap_or(0);
    let mut chosen = BTreeSet::new();
    if index > 0 {
        chosen.insert(index - 1);
    }
    chosen.insert(index);
    if index + 1 < lines.len() {
        chosen.insert(index + 1);
    }
    // Keep nearby context first and an explicit unresolved note when one fits.
    let uncertainty = [
        "unresolved",
        "unknown",
        "missing",
        "not available",
        "미확인",
        "불확실",
        "누락",
        "미해결",
    ];
    if let Some((index, _)) = lines.iter().enumerate().find(|(_, line)| {
        let folded = fold(line);
        uncertainty.iter().any(|term| folded.contains(term))
    }) {
        chosen.insert(index);
    }
    let mut result = String::new();
    // Prioritize the actual matching line before contextual lines.
    let mut order = vec![index];
    order.extend(chosen.into_iter().filter(|candidate| *candidate != index));
    for candidate in order {
        let remaining =
            budget.saturating_sub(result.chars().count() + if result.is_empty() { 0 } else { 3 });
        if remaining < 40 {
            break;
        }
        let line = lines.get(candidate).copied().unwrap_or(body);
        let excerpt = if line.chars().count() <= remaining {
            line.to_string()
        } else {
            let folded = fold(line);
            let position = groups
                .iter()
                .flat_map(|group| {
                    group
                        .terms
                        .iter()
                        .map(|term| folded.find(term.as_str()))
                        .chain(group.names.iter().map(|name| {
                            if name.ticker {
                                line.find(name.text.as_str())
                            } else {
                                folded.find(name.text.as_str())
                            }
                        }))
                })
                .flatten()
                .min()
                .unwrap_or(0);
            let center = folded[..position].chars().count();
            let start = center.saturating_sub(80);
            line.chars()
                .skip(start)
                .take(remaining.saturating_sub(2))
                .collect::<String>()
                + " …"
        };
        if !result.is_empty() {
            result.push_str(" … ");
        }
        result.push_str(&excerpt);
    }
    result
}

fn documents(
    blobs: Option<&BlobStore>,
    kind: RecordKind,
    title: &str,
    payload: &serde_json::Value,
) -> KnowledgeResult<Vec<Draft>> {
    let mut drafts = vec![metadata(kind, title, title)];
    match kind {
        RecordKind::Source => {
            let parsed = SourceRecordPayload::deserialize(payload)?;
            source_documents(blobs, title, &parsed, &mut drafts)?;
        }
        RecordKind::Lens => {
            let body = match super::principles::PrincipleInput::deserialize(payload) {
                Ok(principle) => [Some(principle.statement), principle.why, principle.example]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(_) => {
                    let page = super::lens_page::Page::deserialize(payload)?;
                    std::iter::once(page.summary.as_str())
                        .chain(page.sections.iter().flat_map(|section| {
                            [section.heading.as_str(), section.markdown.as_str()]
                        }))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            };
            drafts.push(text_document(kind, title, "", "", &body, ""));
        }
        RecordKind::World => {
            let body = WorldSnapshotBody::deserialize(payload)?;
            world_documents(title, &body, &mut drafts);
        }
    }
    Ok(drafts)
}

fn source_documents(
    blobs: Option<&BlobStore>,
    module_title: &str,
    payload: &SourceRecordPayload,
    drafts: &mut Vec<Draft>,
) -> KnowledgeResult<()> {
    let source = &payload.source;
    for capture in &source.captures {
        for representation in &capture.representations {
            let bytes = match blobs {
                Some(blobs)
                    if is_text_media(&representation.media_type)
                        && !representation.spans.is_empty() =>
                {
                    Some(blobs.read(&representation.digest)?)
                }
                _ => None,
            };
            for span in &representation.spans {
                let aliases = format!(
                    "{module_title}\n{}\n{}\n{}",
                    source.title, representation.document_id, span.context_note
                );
                let text = span_text(bytes.as_deref(), representation, span)?;
                if let Some(text) = text {
                    let full = matches!(
                        &span.selector,
                        Selector::ByteRange { start: 0, end }
                            if *end == representation.byte_length
                    );
                    let mut coverage = if full { "included_full" } else { "snippet" };
                    let body = bounded(&text);
                    if body.len() < text.len() {
                        coverage = "snippet";
                    }
                    drafts.push(Draft {
                        kind: "source",
                        span_id: span.span_id.clone(),
                        object_id: source.source_id.clone(),
                        coverage,
                        title: source.title.clone(),
                        body,
                        aliases,
                    });
                } else {
                    drafts.push(Draft {
                        kind: "source",
                        span_id: span.span_id.clone(),
                        object_id: source.source_id.clone(),
                        coverage: "metadata_only",
                        title: source.title.clone(),
                        body: format!(
                            "{}\n{}\n{}",
                            source.title, representation.document_id, span.context_note
                        ),
                        aliases,
                    });
                }
            }
        }
    }

    Ok(())
}

fn world_documents(title: &str, body: &WorldSnapshotBody, drafts: &mut Vec<Draft>) {
    let nodes: BTreeMap<_, _> = body
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for claim in &body.claims {
        let mut aliases = Vec::new();
        for endpoint in &claim.endpoints {
            if let Some(node) = nodes.get(endpoint.node_id.as_str()) {
                aliases.push(node.name.clone());
                aliases.extend(node.aliases.iter().cloned());
            }
        }
        drafts.push(Draft {
            kind: "world",
            span_id: String::new(),
            object_id: claim.id.clone(),
            coverage: "snippet",
            title: title.to_string(),
            body: claim.proposition.clone(),
            aliases: aliases.join("\n"),
        });
    }
    for annotation in &body.annotations {
        drafts.push(Draft {
            kind: "world",
            span_id: String::new(),
            object_id: annotation.topic_id.clone(),
            coverage: "snippet",
            title: title.to_string(),
            body: annotation.markdown.clone(),
            aliases: String::new(),
        });
    }
}

fn span_text(
    bytes: Option<&[u8]>,
    representation: &PublishedRepresentation,
    span: &PublishedSpan,
) -> KnowledgeResult<Option<String>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let selected = super::source::select_bytes(bytes, &span.selector, &representation.document_id)?;
    if super::canonical::digest_bytes(&selected) != span.selected_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    Ok(String::from_utf8(selected).ok())
}

fn is_text_media(media_type: &str) -> bool {
    media_type.starts_with("text/") || media_type == "application/json"
}

fn metadata(kind: RecordKind, title: &str, body: &str) -> Draft {
    Draft {
        kind: kind.as_str(),
        span_id: String::new(),
        object_id: "#title".into(),
        coverage: "metadata_only",
        title: title.to_string(),
        body: body.to_string(),
        aliases: title.to_string(),
    }
}

fn text_document(
    kind: RecordKind,
    title: &str,
    span_id: &str,
    object_id: &str,
    body: &str,
    aliases: &str,
) -> Draft {
    let bounded_body = bounded(body);
    Draft {
        kind: kind.as_str(),
        span_id: span_id.to_string(),
        object_id: object_id.to_string(),
        coverage: if bounded_body.len() == body.len() {
            "included_full"
        } else {
            "snippet"
        },
        title: title.to_string(),
        body: bounded_body,
        aliases: aliases.to_string(),
    }
}

fn bounded(value: &str) -> String {
    value.chars().take(INDEXED_TEXT_CHARS).collect()
}

fn fold(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii() {
                ch.to_ascii_lowercase()
            } else {
                ch
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::knowledge::{
        test_support, Authorship, Coverage, CreateRecord, GraphClaim, GraphNode, KnowledgeStore,
        RecordKind, RepresentationRole, RetainMaterial, RetainNewSource, SealWorld, Selector,
        SourceClass,
    };
    use std::collections::BTreeMap;

    #[test]
    fn search_uses_the_visible_revision_and_does_not_treat_binary_as_text() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, document_id: &str, media_type: &str, bytes: &[u8]| {
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "loupist-a".into(),
                    at_ms: 11,
                    title: title.into(),
                    source_class: SourceClass::UserFile,
                    origin_locator: "selected file".into(),
                    provider_identity: "editor".into(),
                    authorship: Authorship::User,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    publication_time: None,
                    material: Some(RetainMaterial {
                        media_type: media_type.into(),
                        role: RepresentationRole::Original,
                        document_id: document_id.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        bytes: bytes.to_vec(),
                        selector: Selector::ByteRange {
                            start: 0,
                            end: bytes.len() as u64,
                        },
                        context_note: "Original file bytes".into(),
                    }),
                })
                .unwrap()
        };
        let text = retain(
            "Notes",
            "notes.txt",
            "text/plain",
            "수익 품질\nearnings quality".as_bytes(),
        );
        let korean = search(&store, "loupist-a", "all", "수익");
        assert_eq!(korean.len(), 1);
        assert_eq!(korean[0].record_id, text.record_id);
        assert_eq!(korean[0].revision_id, text.revision_id);
        assert_eq!(korean[0].span_id, text.span_id.unwrap());
        assert_eq!(search(&store, "loupist-a", "source", "EARNINGS").len(), 1);
        assert!(search(&store, "loupist-b", "all", "수익").is_empty());

        retain(
            "Filing",
            "filing.pdf",
            "application/pdf",
            b"%PDF SECRETTEXT",
        );
        assert!(search(&store, "loupist-a", "all", "SECRETTEXT").is_empty());
        let filing = search(&store, "loupist-a", "source", "filing.pdf");
        assert_eq!(filing.len(), 1);
        assert_eq!(filing[0].coverage, "metadata_only");
        assert!(search(&store, "loupist-a", "all", "").is_empty());
    }

    fn save_method(store: &KnowledgeStore, title: &str, body: &str) -> String {
        store
            .file_method("loupist-a", title, body, &test_support::user_origin(), 21)
            .unwrap()
            .record_id
    }

    #[test]
    fn detached_search_allows_writes_and_excludes_superseded_archived_and_foreign_revisions() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let kept = save_method(&store, "Kept risk", "Measure risk carefully.");
        let changed = save_method(&store, "Changing risk", "Measure risk carefully.");
        let archived = save_method(&store, "Archived risk", "Measure risk carefully.");
        let prepare = |loupist| {
            store
                .read(|write| {
                    write.prepare_search(loupist, "lens", "risk", 20, &SearchOptions::default())
                })
                .unwrap()
        };
        let prepared = prepare("loupist-a");
        let foreign = prepare("loupist-b");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let search = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            prepared.rank(None)
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        // A search owns its inputs while another operation writes the library.
        store
            .transact(|write| {
                write.put_section(
                    "loupist-a",
                    &changed,
                    "steps",
                    "Updated method",
                    &test_support::user_origin(),
                    30,
                )
            })
            .unwrap();
        store
            .transact(|write| {
                write.set_lens_archived("loupist-a", &archived, true, "Retire method", 31, "user")
            })
            .unwrap();
        resume_tx.send(()).unwrap();
        let mut ranked = search.join().unwrap();
        assert_eq!(ranked.documents.len(), 3);
        let visible = store
            .read(|write| write.visible_search_revisions("loupist-a"))
            .unwrap();
        ranked.retain_current(&visible);
        assert_eq!(ranked.documents.len(), 1);
        assert_eq!(ranked.documents[0].record_id, kept);
        let mut foreign = foreign.rank(None);
        foreign.retain_current(
            &store
                .read(|write| write.visible_search_revisions("loupist-b"))
                .unwrap(),
        );
        assert!(foreign.documents.is_empty());
    }

    #[test]
    fn persistent_and_unavailable_embedding_caches_preserve_exact_ranked_results() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        save_method(
            &store,
            "Bandwidth",
            "Compare HBM bandwidth only with equal interfaces and units.",
        );
        save_method(
            &store,
            "Equipment",
            "SEMES makes semiconductor cleaning and etching equipment.",
        );
        let model =
            crate::knowledge::local_model(&crate::knowledge::staged_embed_assets()).unwrap();
        let prepare = || {
            store
                .read(|write| {
                    write.prepare_search(
                        "loupist-a",
                        "lens",
                        "HBM bandwidth",
                        20,
                        &SearchOptions {
                            semantic_model: Some(model.clone()),
                            ..SearchOptions::default()
                        },
                    )
                })
                .unwrap()
        };
        let expected = prepare().rank(None).documents;
        assert!(!expected.is_empty());
        let cache = directory.path().join("cache/vectors.sqlite3");
        let current = store
            .read(|write| write.visible_search_revisions("loupist-a"))
            .unwrap();
        for _ in 0..2 {
            let mut ranked = prepare().rank(Some(&cache));
            ranked.prune_cache(&current);
            assert_eq!(ranked.documents, expected);
        }
        // A damaged row is recomputed, and an unwritable cache is optional.
        rusqlite::Connection::open(&cache)
            .unwrap()
            .execute("UPDATE vectors SET vector=x'0102'", [])
            .unwrap();
        assert_eq!(prepare().rank(Some(&cache)).documents, expected);
        assert_eq!(prepare().rank(Some(directory.path())).documents, expected);
        let records = store.read(|write| write.list_records("loupist-a")).unwrap();
        for record in records {
            store
                .transact(|write| {
                    write.set_lens_archived(
                        "loupist-a",
                        &record.record_id,
                        true,
                        "Retire method",
                        40,
                        "user",
                    )
                })
                .unwrap();
        }
        let mut empty = prepare().rank(Some(&cache));
        assert!(empty.documents.is_empty());
        empty.prune_cache(
            &store
                .read(|write| write.visible_search_revisions("loupist-a"))
                .unwrap(),
        );
        assert_eq!(
            rusqlite::Connection::open(&cache)
                .unwrap()
                .query_row("SELECT count(*) FROM vectors", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    /// A library searched with an earlier model (another fingerprint and
    /// vector width) keeps its cache file. The current model reads none of
    /// its vectors, embeds every text again and prunes the earlier rows, and
    /// ranks as it would with no cache at all.
    #[test]
    fn a_cache_from_an_earlier_model_is_embedded_again_on_first_use() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        save_method(
            &store,
            "Bandwidth",
            "Compare HBM bandwidth only with equal interfaces and units.",
        );
        save_method(
            &store,
            "Memory packaging",
            "Advanced packaging raises memory bandwidth per stack.",
        );
        let earlier_assets = tempfile::tempdir().unwrap();
        crate::knowledge::fixture_model(earlier_assets.path());
        let earlier = crate::knowledge::local_model(earlier_assets.path()).unwrap();
        let current =
            crate::knowledge::local_model(&crate::knowledge::staged_embed_assets()).unwrap();
        assert_ne!(earlier.dimension(), current.dimension());
        let prepare = |model: &crate::knowledge::LocalModel| {
            store
                .read(|write| {
                    write.prepare_search(
                        "loupist-a",
                        "lens",
                        "memory bandwidth",
                        20,
                        &SearchOptions {
                            semantic_model: Some(model.clone()),
                            ..SearchOptions::default()
                        },
                    )
                })
                .unwrap()
        };
        let visible = store
            .read(|write| write.visible_search_revisions("loupist-a"))
            .unwrap();
        let cache = directory.path().join("cache/vectors.sqlite3");
        prepare(&earlier).rank(Some(&cache)).prune_cache(&visible);
        let rows = |model: &str| -> Vec<usize> {
            let connection = rusqlite::Connection::open(&cache).unwrap();
            let mut statement = connection
                .prepare("SELECT length(vector) FROM vectors WHERE model = ?1")
                .unwrap();
            statement
                .query_map([model], |row| row.get::<_, i64>(0))
                .unwrap()
                .map(|bytes| bytes.unwrap() as usize)
                .collect()
        };
        let earlier_rows = rows(earlier.fingerprint());
        assert!(!earlier_rows.is_empty());
        let expected = prepare(&current).rank(None).documents;
        assert!(expected.iter().any(|hit| hit.retrieval != "lexical"));
        let mut ranked = prepare(&current).rank(Some(&cache));
        assert_eq!(ranked.documents, expected);
        ranked.prune_cache(&visible);
        assert!(rows(earlier.fingerprint()).is_empty());
        let current_rows = rows(current.fingerprint());
        assert_eq!(current_rows.len(), earlier_rows.len());
        assert!(current_rows
            .iter()
            .all(|bytes| *bytes == current.dimension() * 4));
    }

    #[test]
    fn ranks_multitoken_aliases_and_applies_authority_and_module_filters() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        save_method(&store, "Generic risk", "Risk only. No portfolio method.");
        let expected = save_method(
            &store,
            "Stress method",
            "Measure portfolio drawdown, then risk under stressed correlations.",
        );
        let hits = search(&store, "loupist-a", "lens", "포트폴리오 최대낙폭");
        assert_eq!(hits[0].record_id, expected);
        assert_eq!(hits[0].matched_terms.len(), 2);
        assert_eq!(hits[0].retrieval, "lexical");
        let filtered = store
            .read(|write| {
                write.prepare_search(
                    "loupist-a",
                    "lens",
                    "risk",
                    20,
                    &SearchOptions {
                        record_ids: vec![expected.clone()],
                        include_metadata: false,
                        semantic_model: None,
                        ..SearchOptions::default()
                    },
                )
            })
            .unwrap()
            .rank(None)
            .documents;
        assert!(!filtered.is_empty());
        assert!(filtered
            .iter()
            .all(|hit| hit.record_id == expected && hit.coverage != "metadata_only"));
        assert!(search(&store, "loupist-b", "lens", "portfolio risk").is_empty());
        assert!(store
            .read(|write| write.prepare_search(
                "loupist-a",
                "secrets",
                "risk",
                2,
                &SearchOptions::default()
            ))
            .is_err());
    }

    #[test]
    fn query_excerpts_keep_original_units_dates_unknowns_and_complete_refs() {
        let body = format!("{}\nDate: 2026-09-30; currency: KRW million\nOperating profit was 125,000; revenue was 800,000.\nUnresolved: consolidated scope is missing.\n{}",
            "Unrelated preface.\n".repeat(100), "Historical detail.\n".repeat(100));
        let excerpt = query_excerpt(&body, &query_groups("영업이익"), 800);
        assert!(excerpt.contains("125,000"));
        assert!(excerpt.contains("2026-09-30; currency: KRW million"));
        assert!(excerpt.contains("Unresolved: consolidated scope is missing."));
        assert!(excerpt.chars().count() <= 800);
    }

    #[test]
    fn large_world_filters_keep_current_claims_and_loupist_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let record = store
            .create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "Supply relationships".into(),
                at_ms: 10,
            })
            .unwrap();
        let mut snapshot = SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: record.record_id,
            expected_revision_id: None,
            at_ms: 11,
            nodes: vec![GraphNode {
                id: "company-a".into(),
                name: "Company A".into(),
                aliases: vec![],
                description: "Company".into(),
                kind: "finance:Company".into(),
                source_role: "entity".into(),
                properties: BTreeMap::from([("ticker".into(), serde_json::json!("AAA"))]),
            }],
            claims: (0..1500)
                .map(|index| GraphClaim {
                    id: format!("claim-{index}"),
                    proposition: format!(
                        "Supply relationship {index}: {}",
                        "retained evidence ".repeat(40)
                    ),
                    attribution: "Company publication".into(),
                    relation: "supplies".into(),
                    endpoints: vec![crate::knowledge::ClaimEndpoint {
                        node_id: "company-a".into(),
                        role: "subject".into(),
                    }],
                    qualifiers: vec![],
                    evidence: vec![],
                    status: crate::knowledge::ClaimStatus::Active,
                    recorded_at_ms: Some(10),
                    publication_time: None,
                    effective_time: None,
                    valid_at: None,
                    invalid_at: None,
                    expired_at: None,
                })
                .collect(),
            annotations: vec![],
            dependencies: vec![],
        };
        let first = store.seal_world(snapshot.clone()).unwrap();
        snapshot.expected_revision_id = Some(first.revision_id.clone());
        snapshot.at_ms = 12;
        snapshot.claims.remove(0);
        let current = store.seal_world(snapshot).unwrap();
        let options = SearchOptions {
            companies: vec!["AAA".into()],
            ..SearchOptions::default()
        };
        let query = |loupist, options: &SearchOptions| {
            store
                .read(|write| {
                    prepare_visible(
                        write.tx,
                        loupist,
                        "world",
                        "*",
                        SEARCH_WINDOW_LIMIT,
                        options,
                    )
                })
                .unwrap()
                .rank(None)
                .documents
        };
        let hits = query("loupist-a", &options);
        assert_eq!(hits.len(), SEARCH_WINDOW_LIMIT);
        assert!(hits
            .iter()
            .all(|hit| hit.revision_id == current.revision_id && hit.object_id != "claim-0"));
        assert!(query("loupist-b", &options).is_empty());
        assert!(query(
            "loupist-a",
            &SearchOptions {
                companies: vec!["BBB".into()],
                ..options
            }
        )
        .is_empty());
    }

    #[test]
    fn structured_filters_require_declared_company_period_and_source_type() {
        let payload = serde_json::json!({"source":{"sourceId":"source-a","sourceClass":"tool_result","captures":[]}});
        let options = SearchOptions {
            companies: vec!["005930".into()],
            periods: vec!["2024".into()],
            source_types: vec!["tool_result".into()],
            ..SearchOptions::default()
        };
        let body = serde_json::json!({"data":{"company":{"stock_code":"005930"},
            "period":{"fiscal_year":2024},"facts":[{"canonical_concept":"revenue","value":"1000","currency":"KRW"}]}}).to_string();
        assert!(structured_matches(
            "source", "source-a", &body, &payload, &options
        ));
        assert!(!structured_matches(
            "source",
            "source-a",
            "005930 revenue in 2024",
            &payload,
            &options
        ));
        assert!(!structured_matches(
            "source", "source-b", &body, &payload, &options
        ));
        let missing = SearchOptions {
            companies: vec!["MSFT".into()],
            ..options.clone()
        };
        assert!(!structured_matches(
            "source", "source-a", &body, &payload, &missing
        ));
        // A keyword in an unrelated node cannot widen a claim's company filter.
        let world = serde_json::json!({"claims":[{"id":"claim-a","endpoints":[{"nodeId":"node-a"}]}],
            "nodes":[{"id":"node-a","kind":"finance:Company","name":"Apple","properties":{"ticker":"AAPL"}},
                {"id":"node-b","kind":"finance:Company","name":"Microsoft","properties":{"ticker":"MSFT"}}]});
        assert!(!structured_matches(
            "world",
            "claim-a",
            "MSFT",
            &world,
            &SearchOptions {
                companies: vec!["MSFT".into()],
                ..SearchOptions::default()
            }
        ));
        assert!(structured_matches(
            "world",
            "claim-a",
            "",
            &world,
            &SearchOptions {
                companies: vec!["AAPL".into()],
                ..SearchOptions::default()
            }
        ));
    }

    /// The Lens pointer once took a hit matching one group of a longer
    /// request when its lexical-first score, with up to 200 embedding points
    /// and halved for a title row, reached 600. That held exactly when the
    /// one group was the whole request and the row was not a title row: the
    /// rule it reads now from `matched_terms`, since fused scores rank.
    #[test]
    fn a_lone_matched_group_reached_600_only_as_the_whole_request() {
        let documents = [
            (
                "Cash flow discipline",
                "Track cash flow conversion against capex.",
            ),
            (
                "Liquidity buffers",
                "Hold liquidity buffers against covenant stress.",
            ),
            (
                "Dividend policy",
                "Dividends follow free cash flow coverage.",
            ),
            ("포트폴리오 위험", "최대낙폭과 변동성을 함께 본다."),
        ];
        let queries = [
            "cash flow",
            "free cash flow",
            "review cash flow",
            "cash covenant",
            "liquidity stress test",
            "dividend coverage policy",
            "포트폴리오 최대낙폭",
            "변동성",
        ];
        let mut lone = 0;
        for query in queries {
            let groups = query_groups(query);
            for (title, body) in documents {
                for metadata in [false, true] {
                    let (aliases, body) = if metadata { (title, title) } else { ("", body) };
                    let (lexical, matched) = lexical_score(title, body, aliases, &groups);
                    if matched.len() != 1 {
                        continue;
                    }
                    lone += 1;
                    let whole_request = groups.len() == 1 && !metadata;
                    for boost in [0, 200] {
                        let score = lexical + boost;
                        let score = if metadata { score / 2 } else { score };
                        assert_eq!(score >= 600, whole_request, "{query} / {title}");
                    }
                }
            }
        }
        assert!(lone >= 10);
    }

    #[test]
    fn aliases_do_not_match_inside_unrelated_english_words() {
        assert_eq!(
            lexical_score("", "risk assessment", "", &query_groups("risk"))
                .1
                .len(),
            1
        );
        assert_eq!(
            lexical_score("", "brisk business", "", &query_groups("risk")).0,
            0
        );
        assert_eq!(query_groups("operating profit 영업이익").len(), 1);
    }

    fn retain_text(store: &KnowledgeStore, title: &str, text: &str) -> String {
        let bytes = text.as_bytes().to_vec();
        store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "loupist-a".into(),
                at_ms: 11,
                title: title.into(),
                source_class: SourceClass::UserFile,
                origin_locator: "selected file".into(),
                provider_identity: "editor".into(),
                authorship: Authorship::User,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "note.txt".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    bytes,
                    context_note: "Saved article".into(),
                }),
            })
            .unwrap()
            .record_id
    }

    fn seal_names(store: &KnowledgeStore, nodes: &[(&str, &[&str])]) {
        let record = store
            .create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "Map".into(),
                at_ms: 10,
            })
            .unwrap();
        store
            .seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id,
                expected_revision_id: None,
                at_ms: 12,
                nodes: nodes
                    .iter()
                    .enumerate()
                    .map(|(index, (name, aliases))| GraphNode {
                        id: format!("node-{index}"),
                        name: (*name).into(),
                        aliases: aliases.iter().map(|alias| (*alias).into()).collect(),
                        description: String::new(),
                        kind: "finance:Company".into(),
                        source_role: "entity".into(),
                        properties: BTreeMap::new(),
                    })
                    .collect(),
                claims: vec![],
                annotations: vec![],
                dependencies: vec![],
            })
            .unwrap();
    }

    fn found(store: &KnowledgeStore, needle: &str) -> Vec<String> {
        search(store, "loupist-a", "source", needle)
            .into_iter()
            .filter(|hit| hit.coverage != "metadata_only")
            .map(|hit| hit.record_id)
            .collect()
    }

    #[test]
    fn a_name_on_the_map_finds_sources_that_spell_it_in_another_language() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let english = retain_text(
            &store,
            "Hanbit Semiconductor results",
            "Hanbit Semiconductor (KRX: 991230) raised HBM output.",
        );
        let arcadia = retain_text(
            &store,
            "부산 데이터센터",
            "아카디아 클라우드가 부산에 데이터센터를 짓는다.",
        );
        let korean = retain_text(
            &store,
            "반도체 수출",
            "반도체 수출이 늘었다. 하이브리드 차량 판매도 늘었다.",
        );
        let generic = retain_text(
            &store,
            "Market wrap",
            "All chip stocks rose on the key index.",
        );
        // Without a Map, a Korean name reaches only Korean text.
        assert!(found(&store, "한빛반도체가 투자").is_empty());
        seal_names(
            &store,
            &[
                ("Hanbit Semiconductor", &["한빛반도체", "991230"]),
                ("Arcadia Cloud", &["아카디아 클라우드", "ARCQ"]),
                ("HYBE", &["하이브"]),
                ("Allstate", &["ALL"]),
                ("Keycorp", &["KEY"]),
            ],
        );
        // A Korean name, with its particle, finds the English spelling; an
        // English name and a ticker find the Korean one.
        assert_eq!(found(&store, "한빛반도체가 투자"), [english.as_str()]);
        assert_eq!(found(&store, "Arcadia Cloud capex"), [arcadia.as_str()]);
        assert_eq!(found(&store, "ARCQ 데이터센터"), [arcadia.as_str()]);
        // A generic word inside a name expands to nothing: 반도체 still finds
        // the Korean article only, not Hanbit's English one.
        assert_eq!(found(&store, "반도체"), [korean.as_str()]);
        // 하이브리드 does not name 하이브, and a ticker spelled like a word
        // matches only the upper-case code.
        assert!(found(&store, "HYBE").is_empty());
        assert!(found(&store, "allstate").is_empty());
        assert_eq!(found(&store, "all"), [generic.as_str()]);
        assert!(found(&store, "keycorp").is_empty());
    }

    #[test]
    fn a_short_name_expands_only_where_the_library_uses_it_for_that_entity() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let korean = retain_text(&store, "한빛반도체 투자", "한빛반도체가 청주에 투자한다.");
        let english = retain_text(&store, "Hanbit capex", "Hanbit Semiconductor will invest.");
        let bank = retain_text(&store, "한국은행 금리", "한국은행이 기준금리를 내렸다.");
        let lending = retain_text(
            &store,
            "Bank lending",
            "Bank lending slowed as the bank tightened.",
        );
        seal_names(
            &store,
            &[
                ("Hanbit Semiconductor", &["한빛반도체"]),
                ("Bank of Korea", &["한국은행"]),
            ],
        );
        // Hanbit appears here only beside its full name, so it names the entity.
        let hits = found(&store, "Hanbit capex");
        assert_eq!(hits[0], english);
        assert!(hits.contains(&korean));
        assert_eq!(found(&store, "한빛 투자")[0], korean);
        // Bank is a word this library uses for other banks.
        assert_eq!(found(&store, "bank"), [lending]);
        assert!(!found(&store, "bank").contains(&bank));
    }

    fn search(
        store: &KnowledgeStore,
        loupist_id: &str,
        scope: &str,
        needle: &str,
    ) -> Vec<SearchDocument> {
        store
            .read(|write| {
                write.prepare_search(
                    loupist_id,
                    scope,
                    needle,
                    SEARCH_LIMIT,
                    &SearchOptions {
                        include_metadata: true,
                        ..SearchOptions::default()
                    },
                )
            })
            .unwrap()
            .rank(None)
            .documents
    }
}
