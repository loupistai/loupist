//! Incremental lexical index for the visible Source, Lens, and World revision.
//!
//! Rows are written when a revision is published. Search reads this
//! table and the caller's owned records; it does not walk the blob tree.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
    /// Ranking only; search discoveries still need an acknowledged exact read.
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
    /// Existing bundled English model; no network or provider calls are made.
    pub semantic_assets: Option<PathBuf>,
}

/// Owned authorized inputs, with no SQLite transaction or folder lock.
pub struct PreparedSearch {
    candidates: Vec<Candidate>,
    payloads: BTreeMap<String, String>,
    groups: Vec<Vec<String>>,
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
            query: needle.into(),
            limit,
            options: options.clone(),
        });
    }

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
            groups,
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
        let query_vector = options
            .semantic_assets
            .as_ref()
            .filter(|_| !groups.is_empty())
            .filter(|_| {
                candidates
                    .iter()
                    .any(|candidate| candidate.document.coverage != "metadata_only")
            })
            .and_then(|assets| super::graph_inference::embed_local_english(assets, &query).ok());
        let mut cache = options
            .semantic_assets
            .as_ref()
            .zip(cache_path)
            .filter(|_| query_vector.is_some())
            .and_then(|(assets, path)| {
                let fingerprint =
                    super::graph_inference::local_english_model_fingerprint(assets).ok()?;
                super::search_embeddings::EmbeddingCache::open(path, &fingerprint).ok()
            });
        let mut hits = Vec::new();
        for Candidate {
            mut document,
            aliases,
        } in candidates
        {
            if let Some(payload) = payloads.get(&document.revision_id) {
                if !structured_matches(
                    &document.kind,
                    &document.object_id,
                    &document.snippet,
                    payload,
                    &options,
                ) {
                    continue;
                }
            }
            (document.score, document.matched_terms) =
                lexical_score(&document.title, &document.snippet, &aliases, &groups);
            hits.push(document);
        }
        if let Some((query, assets)) = query_vector.as_ref().zip(options.semantic_assets.as_ref()) {
            // Sources, Lens and World claims all carry vectors; metadata rows do not.
            for batch in hits.chunks_mut(128) {
                let texts: Vec<String> = batch
                    .iter()
                    .filter(|hit| hit.coverage != "metadata_only")
                    .map(|hit| {
                        format!("{}\n{}", hit.title, hit.snippet)
                            .chars()
                            .take(8_000)
                            .collect()
                    })
                    .collect();
                let revisions: Vec<&str> = batch
                    .iter()
                    .filter(|hit| hit.coverage != "metadata_only")
                    .map(|hit| hit.revision_id.as_str())
                    .collect();
                let mut similarities =
                    cached_similarities(assets, query, &texts, &revisions, cache.as_mut())
                        .into_iter();
                for hit in batch {
                    if hit.coverage == "metadata_only" {
                        continue;
                    }
                    let semantic = similarities.next().flatten().unwrap_or(0.0);
                    if hit.score == 0 && semantic < 0.55 {
                        continue;
                    }
                    hit.retrieval = if semantic >= 0.55 {
                        if hit.score > 0 {
                            "hybrid"
                        } else {
                            "semantic"
                        }
                    } else {
                        "lexical"
                    }
                    .into();
                    hit.score += (semantic.max(0.0) * 200.0) as u32;
                }
            }
        }
        // The English model supplements lexical matching; Korean queries retain
        // literal terms and bilingual aliases even without model assets.
        hits.retain(|hit| groups.is_empty() || hit.score > 0);
        for hit in &mut hits {
            if hit.coverage == "metadata_only" {
                hit.score /= 2;
            }
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

fn cached_similarities(
    assets: &std::path::Path,
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
        if let Ok(loaded) = super::graph_inference::embed_local_english_batch(assets, &inputs) {
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

fn query_groups(query: &str) -> Vec<Vec<String>> {
    let mut remaining = fold(query);
    let mut groups = Vec::new();
    for aliases in CONCEPTS {
        if aliases.iter().any(|alias| contains_term(&remaining, alias)) {
            groups.push(aliases.iter().map(|alias| alias.to_string()).collect());
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
            groups.push(vec![term.to_string()]);
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

fn lexical_score(
    title: &str,
    body: &str,
    aliases: &str,
    groups: &[Vec<String>],
) -> (u32, Vec<String>) {
    let title = fold(title);
    let body = fold(body);
    let aliases = fold(aliases);
    let mut score = 0;
    let mut matched = Vec::new();
    for group in groups {
        let weight = group
            .iter()
            .map(|term| {
                if contains_term(&title, term) {
                    140
                } else if contains_term(&aliases, term) {
                    110
                } else if contains_term(&body, term) {
                    80
                } else {
                    0
                }
            })
            .max()
            .unwrap_or(0);
        if weight > 0 {
            matched.push(group[0].clone());
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

fn query_excerpt(body: &str, groups: &[Vec<String>], budget: usize) -> String {
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
                .flatten()
                .filter_map(|term| folded.find(term))
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
            // The kind lives in metadata; a principle payload is the only
            // shape that refuses a page's fields.
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
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../src/knowledge/embed_assets");
        let prepare = || {
            store
                .read(|write| {
                    write.prepare_search(
                        "loupist-a",
                        "lens",
                        "HBM bandwidth",
                        20,
                        &SearchOptions {
                            semantic_assets: Some(assets.clone()),
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
                        semantic_assets: None,
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
