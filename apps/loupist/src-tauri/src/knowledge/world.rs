use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension, Transaction};

use super::authority::{require_edit, require_record};
use super::canonical::{canonical_string, digest_json, from_json, to_value};
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    mint_id, validate_knowledge_id, validate_loupist_id, validate_text, validate_time,
    validate_time_point, validate_token, DependencyRole, EvidenceRevision, GraphClaim,
    GraphCoverage, GraphEdge, GraphNode, IndexingState, QueryScan, RecordKind, SealWorld,
    WorldGraphQuery, WorldGraphView, WorldRevision, WorldSnapshotBody, MAX_WORLD_CLAIMS,
    MAX_WORLD_NODES,
};
use super::revision::{normalize_dependencies, publish_revision, visible_by_digest};
use super::world_filter::{filter as filter_world, WorldFilters};
use crate::store::user_context::MapContextItem;

pub(super) fn seal_world(
    tx: &Transaction<'_>,
    request: SealWorld,
) -> KnowledgeResult<WorldRevision> {
    seal_world_with_actor(tx, request, "user", None, None, &[])
}

/// `followed` seeds discovery; a caller without the app's watchlist passes none.
pub(super) fn seal_world_with_actor(
    tx: &Transaction<'_>,
    request: SealWorld,
    actor: &str,
    origin_chat_id: Option<&str>,
    origin_message_id: Option<&str>,
    followed: &[MapContextItem],
) -> KnowledgeResult<WorldRevision> {
    seal_world_with_origin(
        tx,
        request,
        actor,
        origin_chat_id,
        origin_message_id,
        None,
        followed,
    )
}

/// A publication without a Chat origin whose growth events carry `origin`
/// (a shared pack's import, for example).
pub(super) fn seal_world_with_provenance(
    tx: &Transaction<'_>,
    request: SealWorld,
    actor: &str,
    origin: Option<serde_json::Value>,
) -> KnowledgeResult<WorldRevision> {
    seal_world_with_origin(tx, request, actor, None, None, origin, &[])
}

fn seal_world_with_origin(
    tx: &Transaction<'_>,
    request: SealWorld,
    actor: &str,
    origin_chat_id: Option<&str>,
    origin_message_id: Option<&str>,
    origin: Option<serde_json::Value>,
    followed: &[MapContextItem],
) -> KnowledgeResult<WorldRevision> {
    validate_loupist_id(&request.loupist_id)?;
    validate_knowledge_id(&request.record_id, "rec")?;
    validate_time(request.at_ms)?;
    let module = require_record(tx, &request.loupist_id, &request.record_id)?;
    if module.kind != RecordKind::World {
        return Err(KnowledgeError::Invalid("record is not World"));
    }
    require_edit(&module, &request.loupist_id)?;
    let body = normalize_world(WorldSnapshotBody {
        nodes: request.nodes,
        claims: request.claims,
        annotations: request.annotations,
        relation_vocabulary: Vec::new(),
    })?;
    authorize_spans(tx, &request.loupist_id, &body)?;
    let dependencies = validate_world_dependencies(tx, &request.loupist_id, &request.dependencies)?;
    let payload = to_value(&body)?;
    let previous = module
        .head_revision_id
        .as_deref()
        .map(|id| {
            super::revision::load_revision(tx, id)
                .and_then(|revision| from_json::<WorldSnapshotBody>(&revision.payload_json))
        })
        .transpose()?;
    let published = publish_revision(
        tx,
        &module,
        &request.loupist_id,
        request.expected_revision_id.as_deref(),
        &payload,
        &dependencies,
        request.at_ms,
    )?;
    let mut event = super::growth::GrowthEvent::new(
        &request.loupist_id,
        &module.record_id,
        request.at_ms,
        "claim_added",
        actor,
        "World publication",
    );
    // A Chat turn's Source keeps its message on the growth events this publication adds.
    // A Map add has no origin, so those events stay unattributed.
    event.origin_chat_id = origin_chat_id.map(str::to_owned);
    event.origin_message_id = origin_message_id.map(str::to_owned);
    event.origin = origin;
    super::growth::world_diff(tx, &event, previous.as_ref(), &body)?;
    super::world_followups::after_seal(
        tx,
        &request.loupist_id,
        previous.as_ref(),
        &body,
        followed,
        request.at_ms,
    )?;
    let snapshot_id = mint_id("snp");
    insert_snapshot(
        tx,
        &snapshot_id,
        &module.record_id,
        &published.revision_id,
        &published.revision_digest,
        &canonical_string(&payload),
        request.at_ms,
    )?;
    super::search_index::index_payload(
        tx,
        None,
        &module.record_id,
        &published.revision_id,
        RecordKind::World,
        &module.title,
        &payload,
    )?;
    Ok(WorldRevision {
        snapshot_id,
        record_id: module.record_id,
        revision_id: published.revision_id,
        revision_digest: published.revision_digest,
    })
}

pub(super) fn query_world(
    tx: &Transaction<'_>,
    query: WorldGraphQuery,
) -> KnowledgeResult<WorldGraphView> {
    let (node_limit, edge_limit, byte_limit) = query.limits()?;
    world_filters(&query).validate()?;
    let (digest, body) =
        authorized_snapshot(tx, &query.loupist_id, &query.record_id, &query.revision_id)?;
    if query.center_node_id.is_some() && query.center_claim_id.is_some() {
        return Err(KnowledgeError::Invalid(
            "choose a node or a relationship focus",
        ));
    }
    let mode = if query.center_claim_id.is_some() {
        "claim"
    } else if query.center_node_id.is_some() {
        "center"
    } else {
        "browse"
    };
    let filter = serde_json::json!({
        "bindingRoles": query.binding_roles,
        "centerNodeId": query.center_node_id,
        "centerClaimId": query.center_claim_id,
        "kinds": query.kinds,
        "mode": mode,
        "relations": query.relations,
        "search": query.search,
        "statuses": query.statuses,
        "publicationTimes": query.publication_times,
        "publishedBy": query.published_by,
    });
    let filter_hash = digest_json(&filter)?;
    let after = match &query.cursor {
        None => None,
        Some(cursor) => Some(decode_cursor(
            cursor,
            &query.record_id,
            &query.revision_id,
            &filter_hash,
            mode,
        )?),
    };
    page_claims(
        &query,
        &body,
        &digest,
        &filter_hash,
        mode,
        after.as_deref(),
        node_limit,
        edge_limit,
        byte_limit,
    )
}

// The sealed query binds its cursor digest and independent node, edge, and byte limits.
#[allow(clippy::too_many_arguments)]
fn page_claims(
    query: &WorldGraphQuery,
    body: &WorldSnapshotBody,
    digest: &str,
    filter_hash: &str,
    mode: &str,
    after: Option<&str>,
    node_limit: u32,
    edge_limit: u32,
    byte_limit: u32,
) -> KnowledgeResult<WorldGraphView> {
    let scan = QueryScan {
        strategy: "sealed_snapshot_json".to_string(),
        nodes_scanned: u32::try_from(body.nodes.len()).unwrap_or(u32::MAX),
        claims_scanned: u32::try_from(body.claims.len()).unwrap_or(u32::MAX),
    };
    let filtered = filter_world(body, &world_filters(query));
    let (mut nodes_by_id, mut claims) = if let Some(center) = &query.center_claim_id {
        (
            body.nodes
                .iter()
                .map(|node| (node.id.as_str(), node))
                .collect(),
            body.claims
                .iter()
                .filter(|claim| &claim.id == center)
                .collect::<Vec<_>>(),
        )
    } else {
        (filtered.nodes, filtered.claims)
    };
    if let Some(center) = &query.center_node_id {
        validate_token(center, 128, "node id is invalid")?;
        let node = body
            .nodes
            .iter()
            .find(|node| &node.id == center)
            .ok_or(KnowledgeError::NotFound)?;
        nodes_by_id.insert(node.id.as_str(), node);
    }
    if let Some(center) = &query.center_claim_id {
        validate_token(center, 128, "claim id is invalid")?;
        if !body.claims.iter().any(|claim| &claim.id == center) {
            return Err(KnowledgeError::NotFound);
        }
    }
    if let Some(center) = &query.center_node_id {
        claims.retain(|claim| {
            claim
                .endpoints
                .iter()
                .any(|endpoint| &endpoint.node_id == center)
        });
    }
    claims.sort_by(|left, right| left.id.cmp(&right.id));
    if let Some(last) = after {
        claims.retain(|claim| claim.id.as_str() > last);
    }
    let mut chosen_nodes = std::collections::BTreeMap::new();
    if let Some(center) = &query.center_node_id {
        chosen_nodes.insert(center.as_str(), nodes_by_id[center.as_str()]);
    }
    let mut chosen_edges = Vec::new();
    for claim in &claims {
        if chosen_edges.len() as u32 >= edge_limit {
            break;
        }
        let mut trial_nodes = chosen_nodes.clone();
        for endpoint in &claim.endpoints {
            trial_nodes
                .entry(endpoint.node_id.as_str())
                .or_insert(nodes_by_id[endpoint.node_id.as_str()]);
        }
        if trial_nodes.len() as u32 > node_limit {
            if chosen_edges.is_empty() && query.center_node_id.is_none() {
                return Err(KnowledgeError::ItemTooLarge);
            }
            break;
        }
        let mut trial_edges = chosen_edges.clone();
        trial_edges.push(edge_from_claim(claim));
        let would_truncate = claims.len() > chosen_edges.len() + 1;
        let cursor = if would_truncate {
            Some(encode_cursor(
                &query.record_id,
                &query.revision_id,
                filter_hash,
                mode,
                &claim.id,
            ))
        } else {
            None
        };
        let view = assemble_view(
            query,
            digest,
            &trial_nodes,
            &trial_edges,
            cursor,
            would_truncate,
            &scan,
        );
        if canonical_string(&to_value(&view)?).len() as u32 > byte_limit {
            if chosen_edges.is_empty() {
                return Err(KnowledgeError::ItemTooLarge);
            }
            break;
        }
        chosen_nodes = trial_nodes;
        chosen_edges = trial_edges;
    }
    let mut node_cursor = None;
    if chosen_edges.is_empty()
        && query.center_node_id.is_none()
        && (!query.search.is_empty() || !query.kinds.is_empty())
    {
        let mut ids: Vec<_> = nodes_by_id.keys().copied().collect();
        if let Some(last) = after {
            ids.retain(|id| *id > last);
        }
        let available = ids.len();
        for id in ids {
            if chosen_nodes.len() as u32 >= node_limit {
                break;
            }
            chosen_nodes.insert(id, nodes_by_id[id]);
            let would_truncate = chosen_nodes.len() < available;
            let cursor = would_truncate.then(|| {
                encode_cursor(&query.record_id, &query.revision_id, filter_hash, mode, id)
            });
            let view = assemble_view(
                query,
                digest,
                &chosen_nodes,
                &chosen_edges,
                cursor,
                would_truncate,
                &scan,
            );
            if canonical_string(&to_value(&view)?).len() as u32 > byte_limit {
                chosen_nodes.remove(id);
                if chosen_nodes.is_empty() {
                    return Err(KnowledgeError::ItemTooLarge);
                }
                break;
            }
        }
        if chosen_nodes.len() < available {
            node_cursor = chosen_nodes.keys().next_back().map(|id| id.to_string());
        }
    }
    if chosen_edges.is_empty() && query.center_node_id.is_some() {
        let view = assemble_view(
            query,
            digest,
            &chosen_nodes,
            &chosen_edges,
            None,
            false,
            &scan,
        );
        if canonical_string(&to_value(&view)?).len() as u32 > byte_limit {
            return Err(KnowledgeError::ItemTooLarge);
        }
    }
    let truncated = chosen_edges.len() < claims.len() || node_cursor.is_some();
    let next_cursor = if let Some(node_id) = node_cursor {
        Some(encode_cursor(
            &query.record_id,
            &query.revision_id,
            filter_hash,
            mode,
            &node_id,
        ))
    } else if truncated {
        chosen_edges.last().map(|edge| {
            encode_cursor(
                &query.record_id,
                &query.revision_id,
                filter_hash,
                mode,
                &edge.id,
            )
        })
    } else {
        None
    };
    Ok(assemble_view(
        query,
        digest,
        &chosen_nodes,
        &chosen_edges,
        next_cursor,
        truncated,
        &scan,
    ))
}

fn edge_from_claim(claim: &GraphClaim) -> GraphEdge {
    GraphEdge {
        id: claim.id.clone(),
        relation: claim.relation.clone(),
        endpoints: claim.endpoints.clone(),
        status: claim.status,
        claim: claim.clone(),
    }
}

fn assemble_view(
    query: &WorldGraphQuery,
    digest: &str,
    nodes: &std::collections::BTreeMap<&str, &GraphNode>,
    edges: &[GraphEdge],
    next_cursor: Option<String>,
    truncated: bool,
    scan: &QueryScan,
) -> WorldGraphView {
    WorldGraphView {
        source_targets: Default::default(),
        record_id: query.record_id.clone(),
        revision_id: query.revision_id.clone(),
        revision_digest: digest.to_string(),
        nodes: nodes.values().map(|node| (**node).clone()).collect(),
        edges: edges.to_vec(),
        next_cursor,
        coverage: GraphCoverage::SealedSnapshot,
        indexing_state: IndexingState::SnapshotOnly,
        truncated,
        scan: scan.clone(),
    }
}

pub(super) fn normalize_world(mut body: WorldSnapshotBody) -> KnowledgeResult<WorldSnapshotBody> {
    if body.nodes.len() > MAX_WORLD_NODES || body.claims.len() > MAX_WORLD_CLAIMS {
        return Err(KnowledgeError::Invalid("world snapshot is too large"));
    }
    if body.annotations.len() > 500 {
        return Err(KnowledgeError::Invalid("world snapshot is too large"));
    }
    let mut node_ids = BTreeSet::new();
    for node in &mut body.nodes {
        validate_token(&node.id, 128, "node id is invalid")?;
        if !node_ids.insert(node.id.clone()) {
            return Err(KnowledgeError::Invalid("node id is duplicated"));
        }
        validate_text(&node.name, 200, false, "node name is invalid")?;
        validate_text(
            &node.description,
            4_000,
            true,
            "node description is invalid",
        )?;
        validate_token(&node.kind, 128, "node kind is invalid")?;
        validate_token(&node.source_role, 64, "source role is invalid")?;
        if node.aliases.len() > 16 {
            return Err(KnowledgeError::Invalid("node aliases are invalid"));
        }
        for alias in &node.aliases {
            validate_text(alias, 200, false, "node alias is invalid")?;
        }
        node.aliases.sort();
        node.aliases.dedup();
        if node.properties.len() > 32 {
            return Err(KnowledgeError::Invalid("node properties are invalid"));
        }
        for (key, value) in &mut node.properties {
            validate_token(key, 64, "property key is invalid")?;
            *value = canonical_property(value)?;
        }
    }
    let mut claim_ids = BTreeSet::new();
    for claim in &mut body.claims {
        validate_token(&claim.id, 128, "claim id is invalid")?;
        if !claim_ids.insert(claim.id.clone()) {
            return Err(KnowledgeError::Invalid("claim id is duplicated"));
        }
        validate_text(
            &claim.proposition,
            8_000,
            false,
            "claim proposition is invalid",
        )?;
        validate_text(
            &claim.attribution,
            500,
            false,
            "claim attribution is invalid",
        )?;
        validate_token(&claim.relation, 64, "relation is invalid")?;
        if claim.endpoints.is_empty() || claim.endpoints.len() > 16 {
            return Err(KnowledgeError::Invalid("claim endpoints are invalid"));
        }
        let mut seen_endpoints = BTreeSet::new();
        for endpoint in &claim.endpoints {
            validate_token(&endpoint.role, 64, "endpoint role is invalid")?;
            if !node_ids.contains(&endpoint.node_id) {
                return Err(KnowledgeError::Invalid("claim endpoint is unknown"));
            }
            if !seen_endpoints.insert((endpoint.node_id.clone(), endpoint.role.clone())) {
                return Err(KnowledgeError::Invalid("claim endpoint is duplicated"));
            }
        }
        if claim.qualifiers.len() > 16 || claim.evidence.len() > 32 {
            return Err(KnowledgeError::Invalid("claim qualifiers are invalid"));
        }
        let mut qualifier_keys = BTreeSet::new();
        for qualifier in &claim.qualifiers {
            validate_token(&qualifier.key, 64, "qualifier key is invalid")?;
            validate_text(&qualifier.value, 512, true, "qualifier value is invalid")?;
            if !qualifier_keys.insert(qualifier.key.clone()) {
                return Err(KnowledgeError::Invalid("qualifier key is duplicated"));
            }
        }
        for evidence in &claim.evidence {
            validate_knowledge_id(&evidence.span_id, "spn")?;
            validate_text(
                &evidence.attribution,
                500,
                false,
                "evidence attribution is invalid",
            )?;
        }
        if let Some(recorded) = claim.recorded_at_ms {
            validate_time(recorded)?;
        }
        if let Some(time) = &mut claim.publication_time {
            *time = validate_time_point(time)?;
        }
        if let Some(time) = &mut claim.effective_time {
            *time = validate_time_point(time)?;
        }
        if let Some(time) = &mut claim.valid_at {
            *time = validate_time_point(time)?;
        }
        if let Some(time) = &mut claim.invalid_at {
            *time = validate_time_point(time)?;
        }
        if let Some(time) = &mut claim.expired_at {
            *time = validate_time_point(time)?;
        }
    }
    let mut topics = BTreeSet::new();
    for annotation in &mut body.annotations {
        validate_token(&annotation.topic_id, 128, "topic id is invalid")?;
        if !topics.insert(annotation.topic_id.clone()) {
            return Err(KnowledgeError::Invalid("topic id is duplicated"));
        }
        validate_text(&annotation.markdown, 100_000, true, "annotation is invalid")?;
        if annotation.source_span_ids.len() > 32 {
            return Err(KnowledgeError::Invalid("annotation spans are invalid"));
        }
        for span_id in &annotation.source_span_ids {
            validate_knowledge_id(span_id, "spn")?;
        }
        annotation.source_span_ids.sort();
        annotation.source_span_ids.dedup();
    }
    body.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    body.claims.sort_by(|left, right| left.id.cmp(&right.id));
    body.annotations
        .sort_by(|left, right| left.topic_id.cmp(&right.topic_id));
    body.relation_vocabulary = relation_vocabulary(&body.claims);
    Ok(body)
}

fn relation_vocabulary(claims: &[GraphClaim]) -> Vec<super::model::RelationCount> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&str, super::model::RelationCount> = BTreeMap::new();
    for claim in claims {
        let entry =
            counts
                .entry(claim.relation.as_str())
                .or_insert_with(|| super::model::RelationCount {
                    relation: claim.relation.clone(),
                    claims: 0,
                    last_recorded_at_ms: None,
                });
        entry.claims += 1;
        if claim.recorded_at_ms > entry.last_recorded_at_ms {
            entry.last_recorded_at_ms = claim.recorded_at_ms;
        }
    }
    let mut rows: Vec<_> = counts.into_values().collect();
    rows.sort_by(|left, right| {
        right
            .claims
            .cmp(&left.claims)
            .then(left.relation.cmp(&right.relation))
    });
    rows
}

pub(super) fn validate_world_dependencies(
    tx: &Transaction<'_>,
    loupist_id: &str,
    dependencies: &[EvidenceRevision],
) -> KnowledgeResult<Vec<EvidenceRevision>> {
    let dependencies = normalize_dependencies(dependencies, RecordKind::World)?;
    for dependency in &dependencies {
        let module = require_record(tx, loupist_id, &dependency.record_id)?;
        let revision = visible_by_digest(tx, &module, &dependency.revision_digest)?;
        let matches = matches!(
            (dependency.role, revision.kind),
            (DependencyRole::WorldSource, RecordKind::Source)
                | (DependencyRole::ComposedWorld, RecordKind::World)
        );
        if !matches {
            return Err(KnowledgeError::Invalid(
                "world dependency target is invalid",
            ));
        }
    }
    Ok(dependencies)
}

pub(super) fn insert_snapshot(
    tx: &Transaction<'_>,
    snapshot_id: &str,
    record_id: &str,
    revision_id: &str,
    revision_digest: &str,
    snapshot_json: &str,
    at_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "INSERT INTO knowledge_world_snapshots (
            snapshot_id, record_id, revision_id, revision_digest, snapshot_json, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            snapshot_id,
            record_id,
            revision_id,
            revision_digest,
            snapshot_json,
            at_ms
        ],
    )?;
    Ok(())
}

pub(super) fn authorize_spans(
    tx: &Transaction<'_>,
    loupist_id: &str,
    body: &WorldSnapshotBody,
) -> KnowledgeResult<()> {
    for claim in &body.claims {
        for evidence in &claim.evidence {
            super::source::require_any_visible_span(tx, loupist_id, &evidence.span_id)?;
        }
    }
    for annotation in &body.annotations {
        for span_id in &annotation.source_span_ids {
            super::source::require_any_visible_span(tx, loupist_id, span_id)?;
        }
    }
    Ok(())
}

pub(super) fn authorized_snapshot(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    revision_id: &str,
) -> KnowledgeResult<(String, WorldSnapshotBody)> {
    validate_loupist_id(loupist_id)?;
    validate_knowledge_id(record_id, "rec")?;
    validate_knowledge_id(revision_id, "rev")?;
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::World {
        return Err(KnowledgeError::Invalid("record is not World"));
    }
    let stored = super::revision::load_revision(tx, revision_id)?;
    super::authority::revision_in_record(&stored.record_id, &module.record_id)?;
    load_snapshot_body(tx, revision_id)
}

fn load_snapshot_body(
    tx: &Transaction<'_>,
    revision_id: &str,
) -> KnowledgeResult<(String, WorldSnapshotBody)> {
    let row = tx
        .query_row(
            "SELECT revision_digest, snapshot_json FROM knowledge_world_snapshots WHERE revision_id = ?1",
            [revision_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((digest, json)) = row else {
        return Err(KnowledgeError::NotFound);
    };
    Ok((digest, from_json(&json)?))
}

fn world_filters(query: &WorldGraphQuery) -> WorldFilters<'_> {
    WorldFilters {
        search: &query.search,
        kinds: &query.kinds,
        relations: &query.relations,
        binding_roles: &query.binding_roles,
        statuses: &query.statuses,
        publication_times: &query.publication_times,
        published_by: query.published_by.as_deref(),
    }
}

fn encode_cursor(
    record_id: &str,
    revision_id: &str,
    filter_hash: &str,
    mode: &str,
    last_claim_id: &str,
) -> String {
    format!("v1|{record_id}|{revision_id}|{filter_hash}|{mode}|{last_claim_id}")
}

fn decode_cursor(
    cursor: &str,
    record_id: &str,
    revision_id: &str,
    filter_hash: &str,
    mode: &str,
) -> KnowledgeResult<String> {
    let mut parts = cursor.splitn(6, '|');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (
            Some("v1"),
            Some(cursor_module),
            Some(cursor_revision),
            Some(cursor_filter),
            Some(cursor_mode),
            Some(last),
        ) if cursor_module == record_id
            && cursor_revision == revision_id
            && cursor_filter == filter_hash
            && cursor_mode == mode
            && !last.is_empty() =>
        {
            Ok(last.to_string())
        }
        _ => Err(KnowledgeError::CursorStale),
    }
}

fn canonical_property(value: &serde_json::Value) -> KnowledgeResult<serde_json::Value> {
    let canonical = super::canonical::canonicalize_json_text(&canonical_string(value))?;
    if canonical.len() > 4_096 {
        return Err(KnowledgeError::Invalid("node properties are invalid"));
    }
    from_json(&canonical)
}
