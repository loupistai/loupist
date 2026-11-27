//! Cross-language recall over the app's own search paths.
//!
//! `search_recall.json` is a synthetic corpus: English and Korean financial
//! Sources and an English Map whose nodes carry Korean aliases, as World
//! processing writes them. Its queries go Korean to English and English to
//! Korean, with same-language controls, each with known targets. A
//! cross-language query's answer exists only in the other language.
//!
//! Two paths run as `world_search` runs them:
//! - knowledge search (its default scope): lexical terms, concept aliases and
//!   the Map's entity names in either language, fused by rank with the local
//!   embedding's nearest texts (its vector cache included), over Sources and
//!   Map claims;
//! - the Map index (its `world` scope): BM25 and vectors with the trigram
//!   fallback, fused by RRF, over claims. Only queries with a claim target count.
//!
//! The bundled model must keep its floors. `LOUPIST_RECALL_MODELS` adds other
//! model2vec directories as `name=/dir;other=/dir`; a missing one, or one the
//! app's embedding path rejects (such as another vector width), is skipped.
//! `LOUPIST_RECALL_REPORT=/file.json` writes the numbers, misses and raw
//! query-to-text cosines, and `--nocapture` prints them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::*;

const LOUPIST: &str = "loupist-recall";
const KINDS: [&str; 4] = ["ko-en", "en-ko", "en-en", "ko-ko"];
const CUTOFF: usize = 5;

#[derive(Deserialize)]
struct Corpus {
    sources: Vec<CorpusSource>,
    world: CorpusWorld,
    queries: Vec<CorpusQuery>,
}

#[derive(Deserialize)]
struct CorpusSource {
    key: String,
    title: String,
    text: String,
}

#[derive(Deserialize)]
struct CorpusWorld {
    title: String,
    nodes: Vec<CorpusNode>,
    claims: Vec<CorpusClaim>,
}

#[derive(Deserialize)]
struct CorpusNode {
    key: String,
    name: String,
    aliases: Vec<String>,
    description: String,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct CorpusClaim {
    key: String,
    relation: String,
    subject: String,
    object: String,
    proposition: String,
    from: String,
}

#[derive(Deserialize)]
struct CorpusQuery {
    kind: String,
    q: String,
    targets: Vec<String>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!("search_recall.json")).expect("recall corpus")
}

/// Raw query-to-text cosines per query kind, as knowledge search embeds each
/// text: how many targets clear the semantic admission bar, and how many
/// other texts per query would clear it too.
fn cosines(corpus: &Corpus, assets: &Path) -> serde_json::Value {
    const ADMIT: f32 = 0.55;
    let mut units: Vec<(String, String)> = corpus
        .sources
        .iter()
        .map(|source| {
            (
                source.key.clone(),
                format!("{}\n{}", source.title, source.text),
            )
        })
        .collect();
    units.extend(corpus.world.claims.iter().map(|claim| {
        (
            claim.key.clone(),
            format!("{}\n{}", corpus.world.title, claim.proposition),
        )
    }));
    let texts: Vec<String> = units.iter().map(|(_, text)| text.clone()).collect();
    let vectors = crate::knowledge::embed_local_batch(assets, &texts).unwrap();
    let mut kinds = serde_json::Map::new();
    for kind in KINDS {
        let queries: Vec<&CorpusQuery> = corpus.queries.iter().filter(|q| q.kind == kind).collect();
        let texts: Vec<String> = queries.iter().map(|query| query.q.clone()).collect();
        let query_vectors = crate::knowledge::embed_local_batch(assets, &texts).unwrap();
        let mut targets = Vec::new();
        let mut admitted_targets = 0;
        let mut admitted_others = 0;
        for (query, vector) in queries.iter().zip(&query_vectors) {
            let mut best = f32::MIN;
            for ((key, _), unit) in units.iter().zip(&vectors) {
                let cosine: f32 = vector.iter().zip(unit).map(|(a, b)| a * b).sum();
                if query.targets.contains(key) {
                    best = best.max(cosine);
                } else if cosine >= ADMIT {
                    admitted_others += 1;
                }
            }
            admitted_targets += usize::from(best >= ADMIT);
            targets.push(best);
        }
        targets.sort_by(|a, b| a.total_cmp(b));
        let count = queries.len().max(1) as f64;
        kinds.insert(
            kind.into(),
            serde_json::json!({
                "target_cosine_median": targets.get(targets.len() / 2),
                "target_cosine_p10": targets.get(targets.len() / 10),
                "targets_admitted": admitted_targets as f64 / count,
                "others_admitted_per_query": admitted_others as f64 / count,
            }),
        );
    }
    serde_json::Value::Object(kinds)
}

struct Library {
    directory: tempfile::TempDir,
    store: KnowledgeStore,
    sources: HashMap<String, String>,
    claims: HashSet<String>,
    world_record: String,
    world_revision: String,
}

fn library(corpus: &Corpus) -> Library {
    let directory = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
    let mut sources = HashMap::new();
    let mut retained = HashMap::new();
    for (index, source) in corpus.sources.iter().enumerate() {
        let bytes = source.text.as_bytes().to_vec();
        let kept = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: LOUPIST.into(),
                at_ms: AT + index as i64,
                title: source.title.clone(),
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
                    document_id: format!("{}.txt", source.key),
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
            .unwrap();
        sources.insert(kept.record_id.clone(), source.key.clone());
        retained.insert(source.key.clone(), (kept, source.title.clone()));
    }
    let world = &corpus.world;
    let record = store
        .create_record(CreateRecord {
            loupist_id: LOUPIST.into(),
            kind: RecordKind::World,
            title: world.title.clone(),
            at_ms: AT,
        })
        .unwrap();
    let nodes = world
        .nodes
        .iter()
        .map(|node| GraphNode {
            id: node.key.clone(),
            name: node.name.clone(),
            aliases: node.aliases.clone(),
            description: node.description.clone(),
            kind: node
                .kind
                .clone()
                .unwrap_or_else(|| "finance:Company".into()),
            source_role: "source".into(),
            properties: BTreeMap::new(),
        })
        .collect();
    let mut cited = BTreeMap::new();
    let claims = world
        .claims
        .iter()
        .map(|claim| {
            let (source, title) = &retained[&claim.from];
            cited.insert(source.record_id.clone(), source.revision_digest.clone());
            GraphClaim {
                id: claim.key.clone(),
                proposition: claim.proposition.clone(),
                attribution: title.clone(),
                relation: claim.relation.clone(),
                endpoints: vec![
                    ClaimEndpoint {
                        node_id: claim.subject.clone(),
                        role: "subject".into(),
                    },
                    ClaimEndpoint {
                        node_id: claim.object.clone(),
                        role: "object".into(),
                    },
                ],
                qualifiers: Vec::new(),
                evidence: vec![EvidenceBinding {
                    span_id: source.span_id.clone().unwrap(),
                    role: BindingRole::ExtractedFrom,
                    attribution: title.clone(),
                    figure_check: None,
                    passage: None,
                }],
                status: ClaimStatus::Active,
                recorded_at_ms: Some(AT),
                publication_time: None,
                effective_time: None,
                valid_at: None,
                invalid_at: None,
                expired_at: None,
            }
        })
        .collect();
    let sealed = store
        .seal_world(SealWorld {
            loupist_id: LOUPIST.into(),
            record_id: record.record_id.clone(),
            expected_revision_id: None,
            at_ms: AT,
            nodes,
            claims,
            annotations: Vec::new(),
            dependencies: cited
                .into_iter()
                .map(|(record_id, revision_digest)| EvidenceRevision {
                    record_id,
                    revision_digest,
                    role: DependencyRole::WorldSource,
                })
                .collect(),
        })
        .unwrap();
    Library {
        directory,
        store,
        sources,
        claims: world.claims.iter().map(|claim| claim.key.clone()).collect(),
        world_record: record.record_id,
        world_revision: sealed.revision_id,
    }
}

/// One ranked Source or claim.
struct Hit {
    unit: String,
    /// Only the embedding admitted it.
    semantic: bool,
    /// Knowledge search breaks score ties by random record ids, so a target
    /// is ranked after every other unit with its score. The Map's order is
    /// deterministic and carries no score.
    score: Option<u32>,
}

/// The first target's rank, or `None` when no target was returned.
fn rank_of(hits: &[Hit], targets: &[&String]) -> Option<usize> {
    let index = hits.iter().position(|hit| targets.contains(&&hit.unit))?;
    Some(match hits[index].score {
        Some(score) => {
            1 + hits
                .iter()
                .filter(|hit| !targets.contains(&&hit.unit))
                .filter(|hit| hit.score.is_some_and(|other| other >= score))
                .count()
        }
        None => index + 1,
    })
}

impl Library {
    /// Ranked Sources and claims from knowledge search, as `world_search` keeps them.
    fn knowledge(&self, query: &str, assets: Option<&Path>) -> Vec<Hit> {
        let options = SearchOptions {
            include_metadata: true,
            semantic_model: assets.map(|assets| crate::knowledge::local_model(assets).unwrap()),
            ..SearchOptions::default()
        };
        let prepared = self
            .store
            .read(|write| {
                write.prepare_search(LOUPIST, "all", query, SEARCH_WINDOW_LIMIT, &options)
            })
            .unwrap();
        let mut ranked = prepared.rank(Some(&self.directory.path().join("vectors.sqlite3")));
        let visible = self
            .store
            .read(|write| write.visible_search_revisions(LOUPIST))
            .unwrap();
        ranked.retain_current(&visible);
        ranked.prune_cache(&visible);
        let mut hits: Vec<Hit> = Vec::new();
        for document in ranked.documents {
            let unit = match document.kind.as_str() {
                "source" => self.sources.get(&document.record_id).cloned(),
                "world" if self.claims.contains(&document.object_id) => {
                    Some(document.object_id.clone())
                }
                _ => None,
            };
            // A unit's first document carries its best score.
            if let Some(unit) = unit.filter(|unit| hits.iter().all(|hit| &hit.unit != unit)) {
                hits.push(Hit {
                    unit,
                    semantic: document.retrieval == "semantic",
                    score: Some(document.score),
                });
            }
        }
        hits
    }

    /// Ranked claims from the Map's hybrid index.
    fn map(&self, query: &str, assets: &Path) -> Vec<Hit> {
        let found = crate::knowledge::world_extract::query_world(
            &self.store,
            LOUPIST,
            &self.world_record,
            &self.world_revision,
            query,
            assets,
        )
        .unwrap()
        .expect("the Map head is indexed");
        found["view"]["view"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|edge| {
                edge.get("adapter_id")
                    .or_else(|| edge.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .map(|id| Hit {
                        unit: id.to_owned(),
                        semantic: false,
                        score: None,
                    })
            })
            .collect()
    }
}

#[derive(Default, Clone, Copy)]
struct Tally {
    queries: usize,
    found: usize,
    reciprocal: f64,
    returned: usize,
    /// Non-target results that only the embedding admitted.
    noise: usize,
}

impl Tally {
    fn add(&mut self, rank: Option<usize>, returned: usize, noise: usize) {
        self.queries += 1;
        self.returned += returned;
        self.noise += noise;
        if let Some(rank) = rank {
            self.found += usize::from(rank <= CUTOFF);
            self.reciprocal += 1.0 / rank as f64;
        }
    }

    fn recall(&self) -> f64 {
        self.found as f64 / self.queries.max(1) as f64
    }

    fn mrr(&self) -> f64 {
        self.reciprocal / self.queries.max(1) as f64
    }

    fn per_query(&self, count: usize) -> f64 {
        count as f64 / self.queries.max(1) as f64
    }
}

struct Measured {
    tallies: BTreeMap<(&'static str, &'static str), Tally>,
    misses: Vec<serde_json::Value>,
}

/// One model over every query and both paths; no assets is lexical only.
fn measure(corpus: &Corpus, assets: Option<&Path>) -> Measured {
    let library = library(corpus);
    let mut tallies = BTreeMap::new();
    let mut misses = Vec::new();
    for query in &corpus.queries {
        let kind = KINDS
            .into_iter()
            .find(|kind| *kind == query.kind)
            .expect("known query kind");
        let mut score = |path: &'static str, hits: Vec<Hit>, targets: &[&String]| {
            let rank = rank_of(&hits, targets);
            let noise = hits
                .iter()
                .filter(|hit| hit.semantic && !targets.contains(&&hit.unit))
                .count();
            tallies
                .entry((path, kind))
                .or_insert_with(Tally::default)
                .add(rank, hits.len(), noise);
            if rank.is_none_or(|rank| rank > CUTOFF) {
                misses.push(serde_json::json!({
                    "path": path, "kind": kind, "query": query.q, "targets": targets,
                    "rank": rank,
                    "top": hits.iter().take(CUTOFF).map(|hit| &hit.unit).collect::<Vec<_>>(),
                }));
            }
        };
        let targets: Vec<&String> = query.targets.iter().collect();
        score("knowledge", library.knowledge(&query.q, assets), &targets);
        let claims: Vec<&String> = targets
            .iter()
            .copied()
            .filter(|target| library.claims.contains(*target))
            .collect();
        if let Some(assets) = assets.filter(|_| !claims.is_empty()) {
            score("map", library.map(&query.q, assets), &claims);
        }
    }
    Measured { tallies, misses }
}

fn models() -> Vec<(String, Option<PathBuf>)> {
    let bundled = crate::knowledge::staged_embed_assets();
    let mut models = vec![
        ("lexical".to_owned(), None),
        ("bundled".into(), Some(bundled)),
    ];
    for entry in std::env::var("LOUPIST_RECALL_MODELS")
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (name, directory) = entry.split_once('=').unwrap_or((entry, entry));
        let directory = PathBuf::from(directory.trim());
        if !directory.join("config.json").is_file() {
            println!(
                "search_recall skipped {name}: no model at {}",
                directory.display()
            );
            continue;
        }
        // Search would quietly fall back to lexical matching for a model the
        // embedding path rejects, so such a model is not measured at all.
        match crate::knowledge::embed_local_batch(&directory, &["probe".to_owned()]) {
            Ok(_) => models.push((name.trim().to_owned(), Some(directory))),
            Err(error) => println!("search_recall skipped {name}: {error}"),
        }
    }
    models
}

/// Floors for the bundled model: (path, query kind, queries whose target is
/// in the top five, of all queries of that kind with a target on that path).
/// They record blink-multilingual-64M on this corpus, with the Map's names
/// bridging languages and rank fusion in knowledge search.
const FLOORS: &[(&str, &str, usize, usize)] = &[
    ("knowledge", "ko-en", 29, 34),
    ("knowledge", "en-ko", 20, 24),
    ("knowledge", "en-en", 29, 29),
    ("knowledge", "ko-ko", 25, 25),
    ("map", "ko-en", 13, 13),
    ("map", "en-en", 14, 14),
    ("map", "ko-ko", 3, 3),
];

#[test]
fn cross_language_search_recall_holds_for_the_bundled_model() {
    let corpus = corpus();
    let mut report = serde_json::Map::new();
    for (name, assets) in models() {
        let started = std::time::Instant::now();
        let measured = measure(&corpus, assets.as_deref());
        let elapsed = started.elapsed().as_secs_f64();
        let mut rows = serde_json::Map::new();
        for ((path, kind), tally) in &measured.tallies {
            println!(
                "search_recall model={name} path={path} kind={kind} n={} recall@{CUTOFF}={:.3} mrr={:.3} returned={:.1} semantic_noise={:.1}",
                tally.queries,
                tally.recall(),
                tally.mrr(),
                tally.per_query(tally.returned),
                tally.per_query(tally.noise),
            );
            rows.insert(
                format!("{path}/{kind}"),
                serde_json::json!({"n": tally.queries, "recall_at_5": tally.recall(), "mrr": tally.mrr(),
                    "returned": tally.per_query(tally.returned), "semantic_noise": tally.per_query(tally.noise)}),
            );
        }
        println!("search_recall model={name} seconds={elapsed:.2}");
        let cosines = assets
            .as_deref()
            .filter(|_| std::env::var_os("LOUPIST_RECALL_REPORT").is_some())
            .map(|assets| cosines(&corpus, assets));
        if let Some(cosines) = &cosines {
            println!("search_recall model={name} cosines={cosines}");
        }
        report.insert(
            name.clone(),
            serde_json::json!({"rows": rows, "cosines": cosines, "misses": measured.misses}),
        );
        if name == "bundled" {
            for (path, kind, floor, queries) in FLOORS {
                let tally = measured.tallies[&(*path, *kind)];
                assert_eq!(tally.queries, *queries, "{path} {kind} query count");
                assert!(
                    tally.found >= *floor,
                    "{path} {kind}: {} of {queries} targets in the top {CUTOFF}, below {floor}",
                    tally.found
                );
            }
        }
    }
    if let Ok(path) = std::env::var("LOUPIST_RECALL_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::Value::Object(report)).unwrap(),
        )
        .unwrap();
    }
}
