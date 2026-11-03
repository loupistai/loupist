//! Where shared packs meet canonical Memory (W19). `crate::sharing` owns the
//! format, scans and quarantine; this module reads what an export may include
//! and writes accepted items and pack World content through the ordinary
//! revision, growth and snapshot paths.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};

use super::model::{
    mint_id, Authorship, BindingRole, ClaimEndpoint, ClaimStatus, Coverage, CreateRecord,
    DependencyRole, EvidenceBinding, EvidenceRevision, GraphClaim, GraphNode, LensChangeInput,
    LensDocument, Qualifier, RecordKind, RepresentationRole, RetainMaterial, RetainNewSource,
    ReviseLens, SealWorld, Selector, SourceClass, TimePoint, WorldSnapshotBody,
};
use super::{BlobStore, KnowledgeError, KnowledgeResult, Principle};
use crate::sharing::pack::{ClaimRecord, EntityRecord, Identifiers, SourceRecord};

const DEFAULT_PERSPECTIVE: &str = "This Loupist has not recorded a standing philosophy.";

#[derive(Debug, Clone)]
pub(crate) struct LensRecord {
    pub record_id: String,
    pub title: String,
    pub document: LensDocument,
}

/// The Source behind one evidence span, as an export needs it to decide privacy.
#[derive(Debug, Clone)]
pub(crate) struct SpanSource {
    pub record_id: String,
    pub title: String,
    pub source_class: String,
    pub origin_locator: String,
    pub provider_identity: String,
    pub custom_origin: bool,
    /// The capture's authorship, decided when the Source was saved: only
    /// provider policy and explicit user-authored statements decide what may leave.
    pub authorship: String,
    pub publication_time: Option<TimePoint>,
}

/// Everything an export may draw from. Journal, Chats, notes, datasets and
/// retained bytes are never read here.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExportSnapshot {
    pub perspective: Option<LensRecord>,
    pub methods: Vec<LensRecord>,
    pub principles: Vec<Principle>,
    pub nodes: Vec<GraphNode>,
    pub claims: Vec<GraphClaim>,
    pub spans: BTreeMap<String, SpanSource>,
}

fn world_head(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<(String, Option<String>)>> {
    let record_id: Option<String> = tx
        .query_row(
            "SELECT record_id FROM knowledge_records WHERE owner_loupist_id = ?1 AND kind = 'world'",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(record_id) = record_id else {
        return Ok(None);
    };
    let head = super::authority::require_record(tx, loupist_id, &record_id)?.head_revision_id;
    Ok(Some((record_id, head)))
}

fn world_body(tx: &Transaction<'_>, revision_id: &str) -> KnowledgeResult<WorldSnapshotBody> {
    Ok(serde_json::from_str(
        &super::revision::load_revision(tx, revision_id)?.payload_json,
    )?)
}

pub(crate) fn export_snapshot(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<ExportSnapshot> {
    let mut snapshot = ExportSnapshot::default();
    let standing = super::research::load_standing_selection(tx, loupist_id)?;
    if let Some((record_id, _)) = &standing {
        let module = super::authority::require_record(tx, loupist_id, record_id)?;
        let view = super::lens::read_lens(tx, loupist_id, record_id)?;
        if view.document.markdown.trim() != DEFAULT_PERSPECTIVE {
            snapshot.perspective = Some(LensRecord {
                record_id: record_id.clone(),
                title: module.title,
                document: view.document,
            });
        }
    }
    let mut statement = tx.prepare(
        "SELECT record_id, title FROM knowledge_lens_metadata
         WHERE loupist_id = ?1 AND lens_kind = 'method' AND archived_at_ms IS NULL
         ORDER BY title_fold, record_id",
    )?;
    let methods = statement
        .query_map([loupist_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (record_id, title) in methods {
        if standing.as_ref().is_some_and(|(id, _)| id == &record_id) {
            continue;
        }
        let view = super::lens::read_lens(tx, loupist_id, &record_id)?;
        snapshot.methods.push(LensRecord {
            record_id,
            title,
            document: view.document,
        });
    }
    snapshot.principles = super::principles::principle_list(tx, loupist_id, false)?;
    let Some((_, Some(revision))) = world_head(tx, loupist_id)? else {
        return Ok(snapshot);
    };
    let body = world_body(tx, &revision)?;
    snapshot.nodes = body.nodes;
    snapshot.claims = body
        .claims
        .into_iter()
        .filter(|claim| claim.status != ClaimStatus::Withdrawn)
        .collect();
    let mut statement = tx.prepare(
        "SELECT p.span_id, s.record_id, s.title, s.source_class, s.origin_locator,
                s.provider_identity, s.origin_json IS NOT NULL, c.publication_time_json,
                c.authorship
         FROM knowledge_source_spans p
         JOIN knowledge_sources s ON s.record_id = p.record_id
         JOIN knowledge_records r ON r.record_id = s.record_id
         JOIN knowledge_source_captures c ON c.record_id = p.record_id AND c.capture_id = p.capture_id
         WHERE r.owner_loupist_id = ?1",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            SpanSource {
                record_id: row.get(1)?,
                title: row.get(2)?,
                source_class: row.get(3)?,
                origin_locator: row.get(4)?,
                provider_identity: row.get(5)?,
                custom_origin: row.get(6)?,
                authorship: row.get(8)?,
                publication_time: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|json| serde_json::from_str(&json).ok()),
            },
        ))
    })?;
    for row in rows {
        let (span, source) = row?;
        snapshot.spans.insert(span, source);
    }
    Ok(snapshot)
}

/// Pack World content for one import. Keys are the pack's own.
pub(crate) struct WorldImport<'a> {
    pub import_id: &'a str,
    pub pack_id: &'a str,
    pub pack_name: &'a str,
    pub author: Option<&'a str>,
    pub license: &'a str,
    pub entities: &'a [EntityRecord],
    pub claims: &'a [ClaimRecord],
    pub sources: &'a [SourceRecord],
    pub at_ms: i64,
    /// An update (W19.5): the pack's reference Sources this import already
    /// holds, by origin locator → (span, title); they are reused, not retained again.
    pub known_spans: &'a BTreeMap<String, (String, String)>,
    /// An update: this import's own relationships the author no longer shares.
    pub withdraw: &'a BTreeSet<String>,
    /// An update: the version it brings, recorded in the publication's origin.
    pub version: Option<u32>,
}

/// What an import changed, so Remove can take exactly that back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WorldImported {
    pub nodes_added: Vec<String>,
    pub nodes_matched: Vec<String>,
    pub claims_added: Vec<String>,
    pub claims_supported: Vec<String>,
    pub source_records: Vec<String>,
    pub spans: Vec<String>,
    pub claims_withdrawn: Vec<String>,
}

fn normalized_name(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn property_text(node: &GraphNode, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        node.properties.iter().find_map(|(name, value)| {
            (name.eq_ignore_ascii_case(key))
                .then(|| match value {
                    Value::String(text) => Some(text.trim().to_owned()),
                    Value::Array(items) => items
                        .iter()
                        .find_map(Value::as_str)
                        .map(|text| text.trim().to_owned()),
                    Value::Number(number) => Some(number.to_string()),
                    _ => None,
                })
                .flatten()
                .filter(|text| !text.is_empty())
        })
    })
}

pub(crate) fn node_identifiers(node: &GraphNode) -> Identifiers {
    Identifiers {
        ticker: property_text(node, &["ticker", "symbol", "ticker_symbol"]),
        exchange: property_text(node, &["exchange", "market"]),
        cik: property_text(node, &["cik"]),
        dart_corp_code: property_text(node, &["corp_code", "dart_corp_code", "dartCorpCode"]),
        isin: property_text(node, &["isin"]),
    }
}

fn identifier_keys(identifiers: &Identifiers) -> Vec<String> {
    let mut keys = Vec::new();
    if let (Some(ticker), Some(exchange)) = (&identifiers.ticker, &identifiers.exchange) {
        keys.push(format!(
            "ticker:{}:{}",
            ticker.to_uppercase(),
            exchange.to_uppercase()
        ));
    }
    if let Some(cik) = &identifiers.cik {
        let trimmed = cik.trim_start_matches('0');
        if !trimmed.is_empty() {
            keys.push(format!("cik:{trimmed}"));
        }
    }
    if let Some(code) = &identifiers.dart_corp_code {
        keys.push(format!("dart:{code}"));
    }
    if let Some(isin) = &identifiers.isin {
        keys.push(format!("isin:{}", isin.to_uppercase()));
    }
    keys
}

/// Bounds on one World in the derived index.
const LOAD_NODES: usize = 10_000;
const LOAD_CLAIMS: usize = 10_000;
const LOAD_SPANS: usize = 5_000;
/// A reference Source's retained text stays well under one graph episode (1 MB).
const MAX_MATERIAL: usize = 512 * 1024;

fn source_material(input: &WorldImport<'_>, source: &SourceRecord) -> Vec<u8> {
    let mut size = 0;
    let claims: Vec<Value> = input
        .claims
        .iter()
        .filter(|claim| claim.sources.contains(&source.key))
        .map(|claim| json!({"key": claim.key, "proposition": claim.proposition, "quote": claim.quote}))
        .take_while(|entry| {
            size += entry.to_string().len();
            size <= MAX_MATERIAL
        })
        .collect();
    let mut bytes = serde_json::to_vec_pretty(&json!({
        "sharedPack": {
            "id": input.pack_id,
            "name": input.pack_name,
            "author": input.author,
            "license": input.license,
            "importId": input.import_id,
        },
        "source": source,
        "claims": claims,
    }))
    .expect("source material serializes");
    bytes.push(b'\n');
    bytes
}

/// Where a pack Source points: its URL, or the pack and key when it has none.
fn source_locator(pack_id: &str, source: &SourceRecord) -> String {
    source
        .url
        .clone()
        .unwrap_or_else(|| format!("shared-pack:{pack_id}/{}", source.key))
}

/// The reference Sources one import already holds, by origin locator → (span, title).
pub(crate) fn import_spans(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
) -> KnowledgeResult<BTreeMap<String, (String, String)>> {
    let mut statement = tx.prepare(
        "SELECT s.origin_locator, p.span_id, s.title FROM knowledge_sources s
         JOIN knowledge_records r ON r.record_id = s.record_id
         JOIN knowledge_source_spans p ON p.record_id = s.record_id
         WHERE r.owner_loupist_id = ?1 AND s.provider_identity = ?2
         ORDER BY p.span_id",
    )?;
    let rows = statement.query_map(
        params![loupist_id, format!("shared_pack:{import_id}")],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, String>(1)?, row.get::<_, String>(2)?),
            ))
        },
    )?;
    let mut spans = BTreeMap::new();
    for row in rows {
        let (locator, span) = row?;
        spans.entry(locator).or_insert(span);
    }
    Ok(spans)
}

/// The World id a pack key gets in one import.
pub(crate) fn shared_id(import_id: &str, key: &str) -> String {
    let short: String = import_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .skip(3)
        .take(16)
        .collect();
    format!("shared.{short}.{key}")
}

fn qualifier<'a>(claim: &'a GraphClaim, key: &str) -> Option<&'a str> {
    claim
        .qualifiers
        .iter()
        .find(|qualifier| qualifier.key == key)
        .map(|qualifier| qualifier.value.as_str())
}

fn comparable(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .flat_map(char::to_lowercase)
        .collect()
}

/// A pack's REPORTS claim that states another value than one of the user's
/// for the same subject, metric (the target name) and period. Values compare
/// only when both claims state a value and a period, in the same unit or
/// currency when either names one; anything else is unrelated.
pub(crate) fn differs(existing: &GraphClaim, claim: &GraphClaim) -> bool {
    let both = |key: &str| {
        qualifier(existing, key)
            .zip(qualifier(claim, key))
            .map(|(left, right)| (comparable(left), comparable(right)))
    };
    let same_or_absent = |key: &str| match (qualifier(existing, key), qualifier(claim, key)) {
        (None, None) => true,
        (Some(left), Some(right)) => comparable(left) == comparable(right),
        _ => false,
    };
    existing.status != ClaimStatus::Withdrawn
        && existing.relation == "REPORTS"
        && claim.relation == "REPORTS"
        && same_endpoints(existing, claim)
        && both("period").is_some_and(|(left, right)| left == right)
        && same_or_absent("unit")
        && same_or_absent("currency")
        && both("value").is_some_and(|(left, right)| left != right)
}

/// The note a pack's differing claim carries; the claim already on the map is never changed.
const DIFFERS_NOTE: (&str, &str) = ("shared_note", "Differs from a value already on your map");

fn same_endpoints(left: &GraphClaim, right: &GraphClaim) -> bool {
    let set = |claim: &GraphClaim| {
        claim
            .endpoints
            .iter()
            .map(|endpoint| (endpoint.node_id.clone(), endpoint.role.clone()))
            .collect::<BTreeSet<_>>()
    };
    set(left) == set(right)
}

fn ensure_world(
    tx: &Transaction<'_>,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<(String, String)> {
    let (record_id, head) = match world_head(tx, loupist_id)? {
        Some(found) => found,
        None => {
            let created = super::catalog::create_record(
                tx,
                CreateRecord {
                    loupist_id: loupist_id.into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms,
                },
            )?;
            (created.record_id, None)
        }
    };
    let head = match head {
        Some(head) => head,
        None => {
            super::world::seal_world(
                tx,
                SealWorld {
                    loupist_id: loupist_id.into(),
                    record_id: record_id.clone(),
                    expected_revision_id: None,
                    at_ms,
                    nodes: Vec::new(),
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                },
            )?
            .revision_id
        }
    };
    Ok((record_id, head))
}

/// True while a World job is mid-flight: its publication would race this one.
pub(crate) fn world_job_running(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<bool> {
    Ok(super::graph_jobs::list_jobs(tx, loupist_id)?
        .iter()
        .any(|job| job.status == super::graph_jobs::JOB_RUNNING))
}

/// The Rust-only `shared_pack` publication: no model call. Pack Sources become
/// reference Sources (the pack's own record is their retained text), entities
/// match by identifier and then by name within a type, identical claims gain
/// the pack's support, a pack value that differs from the user's for the same
/// subject, metric and period arrives disputed with a note (the user's claim is
/// untouched), and everything commits as one sealed snapshot. The merged head
/// is then queued to load into the derived graph before any extraction, so the
/// next World job keeps it.
pub(crate) fn import_world(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    input: &WorldImport<'_>,
) -> KnowledgeResult<WorldImported> {
    let mut imported = WorldImported::default();
    if input.entities.is_empty() && input.claims.is_empty() && input.withdraw.is_empty() {
        return Ok(imported);
    }
    if world_job_running(tx, loupist_id)? {
        return Err(KnowledgeError::Invalid("world processing is busy"));
    }
    let (world_record_id, head) = ensure_world(tx, loupist_id, input.at_ms)?;
    let previous = super::revision::load_revision(tx, &head)?;
    let mut body: WorldSnapshotBody = serde_json::from_str(&previous.payload_json)?;
    let mut dependencies: Vec<EvidenceRevision> = serde_json::from_str(&previous.dependency_json)?;
    let mut origin = json!({
        "kind": "shared_pack",
        "importId": input.import_id,
        "name": input.pack_name,
        "author": input.author,
        "license": input.license,
        "redistributable": crate::sharing::pack::License::from_id(input.license).is_some_and(crate::sharing::pack::License::redistributable),
    });
    if let Some(version) = input.version {
        origin["version"] = json!(version);
    }
    // Each span carries one attribution (its Source's title) in every binding,
    // as the derived graph requires.
    let mut spans = BTreeMap::<&str, (String, String)>::new();
    for source in input.sources {
        let locator = source_locator(input.pack_id, source);
        if let Some((span, title)) = input.known_spans.get(&locator) {
            spans.insert(source.key.as_str(), (span.clone(), title.clone()));
            continue;
        }
        let bytes = source_material(input, source);
        let retained = super::source::retain_new_source(
            tx,
            blobs,
            RetainNewSource {
                origin: None,
                loupist_id: loupist_id.into(),
                at_ms: input.at_ms,
                title: source.title.clone(),
                source_class: SourceClass::ImportedContributor,
                origin_locator: locator,
                provider_identity: format!("shared_pack:{}", input.import_id),
                authorship: Authorship::ImportedContributor,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: source.published_at.clone(),
                material: Some(RetainMaterial {
                    media_type: "application/json".into(),
                    role: RepresentationRole::Structured,
                    document_id: "shared-pack-source".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: format!("Reference shared in {}", input.pack_name)
                        .chars()
                        .take(2_000)
                        .collect(),
                    bytes,
                }),
            },
        )?;
        let span = retained.span_id.ok_or(KnowledgeError::NotFound)?;
        dependencies.push(EvidenceRevision {
            record_id: retained.record_id.clone(),
            revision_digest: retained.revision_digest,
            role: DependencyRole::WorldSource,
        });
        imported.source_records.push(retained.record_id);
        imported.spans.push(span.clone());
        spans.insert(source.key.as_str(), (span, source.title.clone()));
    }
    let mut by_identifier = BTreeMap::<String, String>::new();
    let mut by_name = BTreeMap::<(String, String), String>::new();
    for node in &body.nodes {
        for key in identifier_keys(&node_identifiers(node)) {
            by_identifier.entry(key).or_insert_with(|| node.id.clone());
        }
        for name in std::iter::once(&node.name).chain(&node.aliases) {
            by_name
                .entry((node.kind.clone(), normalized_name(name)))
                .or_insert_with(|| node.id.clone());
        }
    }
    let mut entity_nodes = BTreeMap::<&str, String>::new();
    for entity in input.entities {
        let matched = identifier_keys(&entity.identifiers)
            .iter()
            .find_map(|key| by_identifier.get(key).cloned())
            .or_else(|| {
                std::iter::once(&entity.name)
                    .chain(&entity.aliases)
                    .chain(&entity.local_names)
                    .find_map(|name| {
                        by_name
                            .get(&(entity.kind.clone(), normalized_name(name)))
                            .cloned()
                    })
            });
        let node_id = match matched {
            Some(node_id) => {
                if !imported.nodes_matched.contains(&node_id) {
                    imported.nodes_matched.push(node_id.clone());
                }
                node_id
            }
            None => {
                let node_id = shared_id(input.import_id, &entity.key);
                let mut properties = BTreeMap::new();
                let identifiers = &entity.identifiers;
                for (key, value) in [
                    ("ticker", &identifiers.ticker),
                    ("exchange", &identifiers.exchange),
                    ("cik", &identifiers.cik),
                    ("corp_code", &identifiers.dart_corp_code),
                    ("isin", &identifiers.isin),
                ] {
                    if let Some(value) = value {
                        properties.insert(key.to_owned(), Value::String(value.clone()));
                    }
                }
                let mut aliases: Vec<String> = entity
                    .aliases
                    .iter()
                    .chain(&entity.local_names)
                    .cloned()
                    .collect();
                aliases.sort();
                aliases.dedup();
                aliases.truncate(16);
                body.nodes.push(GraphNode {
                    id: node_id.clone(),
                    name: entity.name.clone(),
                    aliases,
                    description: entity.summary.clone(),
                    kind: entity.kind.clone(),
                    source_role: "source".into(),
                    properties,
                });
                for key in identifier_keys(identifiers) {
                    by_identifier.entry(key).or_insert_with(|| node_id.clone());
                }
                by_name
                    .entry((entity.kind.clone(), normalized_name(&entity.name)))
                    .or_insert_with(|| node_id.clone());
                imported.nodes_added.push(node_id.clone());
                node_id
            }
        };
        entity_nodes.insert(entity.key.as_str(), node_id);
    }
    for record in input.claims {
        let endpoints: Vec<ClaimEndpoint> = record
            .endpoints
            .iter()
            .filter_map(|endpoint| {
                entity_nodes
                    .get(endpoint.entity.as_str())
                    .map(|node_id| ClaimEndpoint {
                        node_id: node_id.clone(),
                        role: endpoint.role.clone(),
                    })
            })
            .collect();
        if endpoints.len() != 2 || endpoints[0].node_id == endpoints[1].node_id {
            // Two shared names resolved to one existing name; the relation no longer has its ends.
            continue;
        }
        let evidence: Vec<EvidenceBinding> = record
            .sources
            .iter()
            .filter_map(|key| spans.get(key.as_str()))
            .map(|(span, title)| EvidenceBinding {
                span_id: span.clone(),
                role: BindingRole::Supports,
                attribution: title.clone(),
            })
            .collect();
        if evidence.is_empty() {
            continue;
        }
        let mut claim = GraphClaim {
            id: shared_id(input.import_id, &record.key),
            proposition: record.proposition.clone(),
            attribution: record.attribution.clone(),
            relation: record.relation.clone(),
            endpoints,
            qualifiers: record
                .qualifiers
                .iter()
                .map(|qualifier| Qualifier {
                    key: qualifier.key.clone(),
                    value: qualifier.value.clone(),
                })
                .collect(),
            evidence,
            status: ClaimStatus::Active,
            recorded_at_ms: Some(input.at_ms),
            publication_time: record.published_at.clone(),
            effective_time: record.effective.clone(),
            valid_at: record.valid_at.clone(),
            invalid_at: record.invalid_at.clone(),
            expired_at: None,
        };
        let proposition = normalized_name(&claim.proposition);
        if let Some(existing) = body.claims.iter_mut().find(|existing| {
            existing.status != ClaimStatus::Withdrawn
                && existing.relation == claim.relation
                && same_endpoints(existing, &claim)
                && normalized_name(&existing.proposition) == proposition
        }) {
            for binding in &claim.evidence {
                if existing.evidence.len() < 32 && !existing.evidence.contains(binding) {
                    existing.evidence.push(binding.clone());
                }
            }
            imported.claims_supported.push(existing.id.clone());
            continue;
        }
        if body.claims.iter().any(|existing| differs(existing, &claim)) {
            claim.status = ClaimStatus::Disputed;
            if claim.qualifiers.len() < 16
                && claim.qualifiers.iter().all(|q| q.key != DIFFERS_NOTE.0)
            {
                claim.qualifiers.push(Qualifier {
                    key: DIFFERS_NOTE.0.into(),
                    value: DIFFERS_NOTE.1.into(),
                });
            }
        }
        if body.claims.iter().any(|existing| existing.id == claim.id) {
            continue;
        }
        imported.claims_added.push(claim.id.clone());
        body.claims.push(claim);
    }
    for claim in &mut body.claims {
        if input.withdraw.contains(&claim.id) && claim.status != ClaimStatus::Withdrawn {
            claim.status = ClaimStatus::Withdrawn;
            imported.claims_withdrawn.push(claim.id.clone());
        }
    }
    if imported == WorldImported::default() {
        return Ok(imported);
    }
    let cited: BTreeSet<&str> = body
        .claims
        .iter()
        .flat_map(|claim| {
            claim
                .evidence
                .iter()
                .map(|binding| binding.span_id.as_str())
        })
        .collect();
    if body.nodes.len() > LOAD_NODES || body.claims.len() > LOAD_CLAIMS || cited.len() > LOAD_SPANS
    {
        return Err(KnowledgeError::Invalid(
            "the map would be too large to load",
        ));
    }
    super::world::seal_world_with_provenance(
        tx,
        SealWorld {
            loupist_id: loupist_id.into(),
            record_id: world_record_id,
            expected_revision_id: Some(head),
            at_ms: input.at_ms,
            nodes: body.nodes,
            claims: body.claims,
            annotations: body.annotations,
            dependencies,
        },
        "import",
        Some(origin),
    )?;
    super::graph_jobs::reload_world(tx, blobs, loupist_id, input.at_ms)?;
    Ok(imported)
}

/// The World subjects one import introduced, as recorded at Add.
#[derive(Debug, Clone, Default)]
pub(crate) struct ImportRefs {
    pub nodes: BTreeSet<String>,
    pub claims: BTreeSet<String>,
    pub spans: BTreeSet<String>,
    /// The pack's reference Source records.
    pub sources: BTreeSet<String>,
    /// Reset names its own claims explicitly; removing its support must not
    /// delete a claim authored by the user or with unknown provenance.
    pub preserve_other_claims: bool,
}

/// Takes back what one import added: its own claims, the support it added to
/// claims the Loupist already had, the names nothing else uses, and its
/// reference Sources' place in the World's evidence closure. The Loupist's own
/// claims are never otherwise changed. Returns (names, relationships) removed.
pub(crate) fn remove_world(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    import_id: &str,
    pack_name: &str,
    refs: &ImportRefs,
    at_ms: i64,
) -> KnowledgeResult<(usize, usize)> {
    remove_world_additions(
        tx,
        blobs,
        loupist_id,
        refs,
        at_ms,
        "import",
        Some(json!({"kind": "shared_pack_removed", "importId": import_id, "name": pack_name})),
    )
}

pub(crate) fn remove_world_additions(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    refs: &ImportRefs,
    at_ms: i64,
    actor: &str,
    origin: Option<Value>,
) -> KnowledgeResult<(usize, usize)> {
    let (added_nodes, added_claims, spans) = (&refs.nodes, &refs.claims, &refs.spans);
    let Some((record_id, Some(head))) = world_head(tx, loupist_id)? else {
        return Ok((0, 0));
    };
    if world_job_running(tx, loupist_id)? {
        return Err(KnowledgeError::Invalid("world processing is busy"));
    }
    let previous = super::revision::load_revision(tx, &head)?;
    let mut body: WorldSnapshotBody = serde_json::from_str(&previous.payload_json)?;
    let before_claims = body.claims.len();
    let mut support_removed = false;
    body.claims.retain_mut(|claim| {
        if added_claims.contains(&claim.id) {
            return false;
        }
        let had_evidence = !claim.evidence.is_empty();
        let before = claim.evidence.len();
        claim
            .evidence
            .retain(|binding| !spans.contains(&binding.span_id));
        support_removed |= claim.evidence.len() != before;
        refs.preserve_other_claims || !(had_evidence && claim.evidence.is_empty())
    });
    let used: BTreeSet<String> = body
        .claims
        .iter()
        .flat_map(|claim| claim.endpoints.iter().map(|e| e.node_id.clone()))
        .collect();
    let before_nodes = body.nodes.len();
    body.nodes
        .retain(|node| !added_nodes.contains(&node.id) || used.contains(&node.id));
    let removed = (
        before_nodes - body.nodes.len(),
        before_claims - body.claims.len(),
    );
    let mut dependencies: Vec<EvidenceRevision> = serde_json::from_str(&previous.dependency_json)?;
    let before_dependencies = dependencies.len();
    dependencies.retain(|dependency| !refs.sources.contains(&dependency.record_id));
    if removed == (0, 0) && !support_removed && dependencies.len() == before_dependencies {
        return Ok(removed);
    }
    super::world::seal_world_with_provenance(
        tx,
        SealWorld {
            loupist_id: loupist_id.into(),
            record_id,
            expected_revision_id: Some(head),
            at_ms,
            nodes: body.nodes,
            claims: body.claims,
            annotations: body.annotations,
            dependencies,
        },
        actor,
        origin,
    )?;
    super::graph_jobs::reload_world(tx, blobs, loupist_id, at_ms)?;
    Ok(removed)
}

/// What Remove would take back right now (names nothing else uses, relationships only this pack supports).
pub(crate) fn removable_world(
    tx: &Transaction<'_>,
    loupist_id: &str,
    refs: &ImportRefs,
) -> KnowledgeResult<(usize, usize)> {
    let (added_nodes, added_claims, spans) = (&refs.nodes, &refs.claims, &refs.spans);
    let Some((_, Some(head))) = world_head(tx, loupist_id)? else {
        return Ok((0, 0));
    };
    let body = world_body(tx, &head)?;
    let mut links = 0;
    let mut used = BTreeSet::new();
    for claim in &body.claims {
        let only_pack = added_claims.contains(&claim.id)
            || (!refs.preserve_other_claims
                && !claim.evidence.is_empty()
                && claim
                    .evidence
                    .iter()
                    .all(|binding| spans.contains(&binding.span_id)));
        if only_pack {
            links += 1;
        } else {
            used.extend(claim.endpoints.iter().map(|e| e.node_id.clone()));
        }
    }
    let names = body
        .nodes
        .iter()
        .filter(|node| added_nodes.contains(&node.id) && !used.contains(&node.id))
        .count();
    Ok((names, links))
}

fn unique_title(tx: &Transaction<'_>, loupist_id: &str, title: &str, pack: &str) -> String {
    let candidates = std::iter::once(title.to_owned())
        .chain(std::iter::once(format!("{title} · {pack}")))
        .chain((2..50).map(|n| format!("{title} · {pack} ({n})")));
    for candidate in candidates {
        let candidate: String = candidate.chars().take(200).collect();
        if super::lens::require_unique_lens_title(tx, loupist_id, &candidate, None).is_ok() {
            return candidate;
        }
    }
    format!("{title} · {}", mint_id("shared"))
        .chars()
        .take(200)
        .collect()
}

/// An accepted shared method becomes an ordinary Lens method with shared origin.
pub(crate) fn create_shared_method(
    tx: &Transaction<'_>,
    loupist_id: &str,
    title: &str,
    pack_name: &str,
    document: &LensDocument,
    origin: Value,
    at_ms: i64,
) -> KnowledgeResult<String> {
    let title = unique_title(tx, loupist_id, title, pack_name);
    let record = super::catalog::create_record(
        tx,
        CreateRecord {
            loupist_id: loupist_id.into(),
            kind: RecordKind::Lens,
            title,
            at_ms,
        },
    )?;
    super::lens::revise_lens_with_provenance(
        tx,
        ReviseLens {
            loupist_id: loupist_id.into(),
            record_id: record.record_id.clone(),
            expected_revision_id: None,
            at_ms,
            document: document.clone(),
            dependencies: Vec::new(),
            change: LensChangeInput {
                references: Vec::new(),
                rationale: format!("Accepted from the shared Loupist {pack_name}")
                    .chars()
                    .take(400)
                    .collect(),
                feedback: None,
            },
        },
        "import",
        None,
        None,
        Some(origin),
    )?;
    Ok(record.record_id)
}

/// A create-new pack's accepted perspective replaces the default perspective.
pub(crate) fn replace_perspective(
    tx: &Transaction<'_>,
    loupist_id: &str,
    title: &str,
    pack_name: &str,
    document: &LensDocument,
    origin: Value,
    at_ms: i64,
) -> KnowledgeResult<String> {
    let change = LensChangeInput {
        references: Vec::new(),
        rationale: format!("Perspective accepted from the shared Loupist {pack_name}")
            .chars()
            .take(400)
            .collect(),
        feedback: None,
    };
    if let Some((record_id, revision_id)) =
        super::research::load_standing_selection(tx, loupist_id)?
    {
        let view = super::lens::revise_lens_with_provenance(
            tx,
            ReviseLens {
                loupist_id: loupist_id.into(),
                record_id: record_id.clone(),
                expected_revision_id: Some(revision_id),
                at_ms,
                document: document.clone(),
                dependencies: Vec::new(),
                change,
            },
            "import",
            None,
            None,
            Some(origin),
        )?;
        super::research::select_standing_lens(
            tx,
            loupist_id,
            &record_id,
            &view.revision_id,
            at_ms,
        )?;
        return Ok(record_id);
    }
    let title = unique_title(tx, loupist_id, title, pack_name);
    let record = super::catalog::create_record(
        tx,
        CreateRecord {
            loupist_id: loupist_id.into(),
            kind: RecordKind::Lens,
            title,
            at_ms,
        },
    )?;
    let view = super::lens::revise_lens_with_provenance(
        tx,
        ReviseLens {
            loupist_id: loupist_id.into(),
            record_id: record.record_id.clone(),
            expected_revision_id: None,
            at_ms,
            document: document.clone(),
            dependencies: Vec::new(),
            change,
        },
        "import",
        None,
        None,
        Some(origin),
    )?;
    super::research::select_standing_lens(
        tx,
        loupist_id,
        &record.record_id,
        &view.revision_id,
        at_ms,
    )?;
    Ok(record.record_id)
}

/// An accepted shared principle keeps the `shared` origin. A statement the
/// Loupist already holds returns None.
pub(crate) fn create_shared_principle(
    tx: &Transaction<'_>,
    loupist_id: &str,
    statement: &str,
    why: Option<&str>,
    example: Option<&str>,
    at_ms: i64,
) -> KnowledgeResult<Option<String>> {
    match super::principles::principle_save(
        tx,
        loupist_id,
        None,
        None,
        super::PrincipleInput {
            statement: statement.into(),
            why: why.map(str::to_owned),
            example: example.map(str::to_owned),
            origin: "shared".into(),
            origin_chat_id: None,
            origin_message_id: None,
        },
        at_ms,
    ) {
        Ok(principle) => Ok(Some(principle.record_id)),
        Err(KnowledgeError::DuplicatePrinciple) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn archive_lens(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    at_ms: i64,
) -> KnowledgeResult<()> {
    let archived: bool = tx
        .query_row(
            "SELECT archived_at_ms IS NOT NULL FROM knowledge_lens_metadata WHERE loupist_id = ?1 AND record_id = ?2",
            params![loupist_id, record_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(true);
    if archived
        || super::research::load_standing_selection(tx, loupist_id)?
            .is_some_and(|(id, _)| id == record_id)
    {
        return Ok(());
    }
    super::lens::set_lens_archived(
        tx,
        loupist_id,
        record_id,
        true,
        "Archived with the shared Loupist it came from",
        at_ms,
        "user",
    )
}

/// An accepted shared method or perspective as it stands now; None once archived or gone.
pub(crate) fn shared_lens_document(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Option<LensDocument>> {
    let archived: Option<bool> = tx
        .query_row(
            "SELECT archived_at_ms IS NOT NULL FROM knowledge_lens_metadata WHERE loupist_id = ?1 AND record_id = ?2",
            params![loupist_id, record_id],
            |row| row.get(0),
        )
        .optional()?;
    if archived != Some(false) {
        return Ok(None);
    }
    match super::lens::read_lens(tx, loupist_id, record_id) {
        Ok(view) => Ok(Some(view.document)),
        Err(KnowledgeError::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// An accepted shared principle as it stands now; None once archived or gone.
pub(crate) fn shared_principle_text(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Option<Principle>> {
    match super::principles::principle_read(tx, loupist_id, record_id) {
        Ok(principle) if !principle.archived => Ok(Some(principle)),
        Ok(_) | Err(KnowledgeError::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The author's newer text for an accepted shared method or perspective, chosen
/// in the update diff. The record keeps its title; a standing perspective stays selected.
#[allow(clippy::too_many_arguments)]
pub(crate) fn revise_shared_lens(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    pack_name: &str,
    version: u32,
    document: &LensDocument,
    origin: Value,
    at_ms: i64,
) -> KnowledgeResult<()> {
    let head = super::authority::require_record(tx, loupist_id, record_id)?.head_revision_id;
    let view = super::lens::revise_lens_with_provenance(
        tx,
        ReviseLens {
            loupist_id: loupist_id.into(),
            record_id: record_id.into(),
            expected_revision_id: head,
            at_ms,
            document: document.clone(),
            dependencies: Vec::new(),
            change: LensChangeInput {
                references: Vec::new(),
                rationale: format!(
                    "Updated from the shared Loupist {pack_name} (version {version})"
                )
                .chars()
                .take(400)
                .collect(),
                feedback: None,
            },
        },
        "import",
        None,
        None,
        Some(origin),
    )?;
    if super::research::load_standing_selection(tx, loupist_id)?
        .is_some_and(|(id, _)| id == record_id)
    {
        super::research::select_standing_lens(tx, loupist_id, record_id, &view.revision_id, at_ms)?;
    }
    Ok(())
}

/// The author's newer wording for an accepted shared principle. False when the
/// Loupist already holds that statement elsewhere (the principle stays as it is).
pub(crate) fn revise_shared_principle(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    statement: &str,
    why: Option<&str>,
    example: Option<&str>,
    at_ms: i64,
) -> KnowledgeResult<bool> {
    let current = super::principles::principle_read(tx, loupist_id, record_id)?;
    match super::principles::principle_save(
        tx,
        loupist_id,
        Some(record_id),
        Some(&current.revision_id),
        super::PrincipleInput {
            statement: statement.into(),
            why: why.map(str::to_owned),
            example: example.map(str::to_owned),
            origin: "shared".into(),
            origin_chat_id: None,
            origin_message_id: None,
        },
        at_ms,
    ) {
        Ok(_) => Ok(true),
        Err(KnowledgeError::DuplicatePrinciple) => Ok(false),
        Err(error) => Err(error),
    }
}
