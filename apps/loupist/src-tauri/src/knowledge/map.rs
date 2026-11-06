//! A deterministic, read-only projection of World, Lens and factual user context.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::identity::match_items;
use super::{
    model::validate_loupist_id, ClaimStatus, KnowledgeError, KnowledgeResult, WorldSnapshotBody,
};
use crate::store::user_context::{MapContext, MapContextItem};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeMapQuery {
    pub loupist_id: String,
    pub max_nodes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeMapNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub id: String,
    pub kind: String,
    pub label: String,
    /// Original-language aliases: up to four trimmed names that contain a letter
    /// outside Latin (a code point above U+024F). Empty for non-entity nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<String>,
    /// Map colour for an extension type. Core kinds keep the shared category map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_override: Option<super::world_schema::ExtensionCategory>,
    /// Display names for an extension type. The inspector uses the UI language, then English.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_labels: Option<super::world_schema::ExtensionLabels>,
    pub degree: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    pub watched: bool,
    pub held: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learned_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learned_rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeMapEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ClaimStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeMapCount {
    pub total: u32,
    pub shown: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeMap {
    pub nodes: Vec<KnowledgeMapNode>,
    pub edges: Vec<KnowledgeMapEdge>,
    pub unmatched_holdings: Vec<MapContextItem>,
    pub unmatched_watchlist: Vec<MapContextItem>,
    /// Counts refer to projected nodes/edges (multi-endpoint claims form a star).
    pub counts: BTreeMap<String, KnowledgeMapCount>,
    pub world_revision_id: Option<String>,
    /// Latest contributing stored timestamp, not wall-clock request time.
    pub generated_at_ms: i64,
}

fn edge(kind: &str, source: &str, target: &str) -> KnowledgeMapEdge {
    KnowledgeMapEdge {
        id: format!(
            "{kind}:{}",
            serde_json::to_string(&(source, target)).expect("string pair")
        ),
        kind: kind.into(),
        source: source.into(),
        target: target.into(),
        relation: None,
        status: None,
        weight: None,
        first_seen_at_ms: None,
    }
}

fn local_names(aliases: &[String]) -> Vec<String> {
    aliases
        .iter()
        .filter_map(|alias| {
            let clipped: String = alias.trim().chars().take(80).collect();
            let outside_latin = clipped
                .chars()
                .any(|ch| (ch as u32) > 0x024F && ch.is_alphabetic());
            outside_latin.then_some(clipped)
        })
        .take(4)
        .collect()
}

fn extension_marks(
    extension: &super::world_schema::WorldExtension,
    kind: &str,
) -> (
    Option<super::world_schema::ExtensionCategory>,
    Option<super::world_schema::ExtensionLabels>,
) {
    extension
        .types
        .iter()
        .find(|item| item.name == kind)
        .map(|item| (Some(item.category), Some(item.labels.clone())))
        .unwrap_or((None, None))
}

pub(super) fn query(
    tx: &Transaction<'_>,
    request: &KnowledgeMapQuery,
    context: &MapContext,
) -> KnowledgeResult<KnowledgeMap> {
    validate_loupist_id(&request.loupist_id)?;
    let limit = request.max_nodes.unwrap_or(1000);
    if !(1..=1500).contains(&limit) {
        return Err(KnowledgeError::Invalid(
            "map maxNodes must be between 1 and 1500",
        ));
    }
    let loupist = &request.loupist_id;
    let extension = super::graph_jobs::load_settings(tx, loupist)?.extension;
    let world = tx.query_row(
        "SELECT snapshot.revision_id, snapshot.snapshot_json, snapshot.created_at_ms FROM knowledge_records record
         JOIN knowledge_record_heads head ON head.record_id=record.record_id
         JOIN knowledge_world_snapshots snapshot ON snapshot.revision_id=head.revision_id
         WHERE record.owner_loupist_id=?1 AND record.kind='world' ORDER BY record.record_id LIMIT 1",
        [loupist], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,i64>(2)?)),
    ).optional()?;
    let body: Option<WorldSnapshotBody> = world
        .as_ref()
        .map(|(_, json, _)| serde_json::from_str(json))
        .transpose()?;
    let mut generated = context
        .updated_at_ms
        .max(world.as_ref().map_or(0, |row| row.2));
    let mut first = BTreeMap::<(String, String), i64>::new();
    let mut statement = tx.prepare("SELECT kind, CASE WHEN kind='lens_created' THEN record_id ELSE subject_id END, MIN(at_ms), MAX(at_ms) FROM knowledge_growth_events WHERE loupist_id=?1 GROUP BY kind, CASE WHEN kind='lens_created' THEN record_id ELSE subject_id END")?;
    for row in statement.query_map([loupist], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })? {
        let (kind, id, at, last) = row?;
        generated = generated.max(last);
        if let Some(id) = id {
            first.insert((kind, id), at);
        }
    }
    let mut nodes = BTreeMap::<String, KnowledgeMapNode>::new();
    let mut edges = Vec::new();
    let empty = Vec::new();
    let world_nodes = body.as_ref().map_or(&empty, |body| &body.nodes);
    let (watched, unmatched_watchlist) = match_items(&context.watchlist, world_nodes);
    let (held, unmatched_holdings) = match_items(&context.holdings, world_nodes);
    let mut incident = BTreeMap::<String, i64>::new();
    let mut degrees = BTreeMap::<String, u32>::new();
    let mut custom_origins = BTreeMap::new();
    let mut statement = tx.prepare("SELECT p.span_id,s.origin_json FROM knowledge_source_spans p JOIN knowledge_sources s ON s.record_id=p.record_id JOIN knowledge_records r ON r.record_id=s.record_id WHERE r.owner_loupist_id=?1 AND s.origin_json IS NOT NULL ORDER BY p.span_id")?;
    let origins = statement
        .query_map([loupist], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| {
            let (span, json) = row?;
            Ok((
                span,
                serde_json::from_str::<crate::custom_connections::taint::CustomOrigin>(&json)?,
            ))
        })
        .collect::<KnowledgeResult<BTreeMap<_, _>>>()?;
    if let Some(body) = &body {
        for claim in &body.claims {
            for id in claim
                .endpoints
                .iter()
                .map(|e| &e.node_id)
                .collect::<BTreeSet<_>>()
            {
                *degrees.entry(id.clone()).or_default() += 1;
                if let Some(origin) = claim
                    .evidence
                    .iter()
                    .filter_map(|e| origins.get(&e.span_id))
                    .min_by(|a, b| a.connection_id.cmp(&b.connection_id))
                {
                    custom_origins
                        .entry(id.clone())
                        .or_insert_with(|| origin.clone());
                }
                if let Some(at) = claim.recorded_at_ms {
                    incident
                        .entry(id.clone())
                        .and_modify(|old| *old = (*old).min(at))
                        .or_insert(at);
                }
            }
            let at = first
                .get(&("claim_added".into(), claim.id.clone()))
                .copied()
                .or(claim.recorded_at_ms);
            generated = generated.max(at.unwrap_or(0));
            if let Some(source) = claim.endpoints.first() {
                let targets = claim
                    .endpoints
                    .iter()
                    .skip(1)
                    .map(|e| &e.node_id)
                    .collect::<BTreeSet<_>>();
                // Unary claims are visible as self-loops; hyperclaims use their ordered first endpoint as the hub.
                let targets = if targets.is_empty() {
                    BTreeSet::from([&source.node_id])
                } else {
                    targets
                };
                for target in &targets {
                    let mut e = edge("claim", &source.node_id, target);
                    e.id = if targets.len() == 1 {
                        claim.id.clone()
                    } else {
                        format!("{}:{}", claim.id, serde_json::to_string(target)?)
                    };
                    e.relation = Some(claim.relation.clone());
                    e.status = Some(claim.status);
                    e.first_seen_at_ms = at;
                    edges.push(e);
                }
            }
        }
        for node in &body.nodes {
            let (category_override, kind_labels) = extension_marks(&extension, &node.kind);
            nodes.insert(
                node.id.clone(),
                KnowledgeMapNode {
                    origin: custom_origins.get(&node.id).cloned(),
                    id: node.id.clone(),
                    kind: "entity".into(),
                    label: node.name.clone(),
                    local_names: local_names(&node.aliases),
                    entity_kind: Some(node.kind.clone()),
                    category_override,
                    kind_labels,
                    degree: degrees.get(&node.id).copied().unwrap_or(0),
                    first_seen_at_ms: first
                        .get(&("entity_added".into(), node.id.clone()))
                        .copied()
                        .or_else(|| incident.get(&node.id).copied()),
                    record_id: None,
                    archived: None,
                    watched: watched.contains(&node.id),
                    held: held.contains(&node.id),
                    learned_at_ms: None,
                    learned_rationale: None,
                },
            );
        }
    }
    let mut statement = tx.prepare("SELECT m.record_id,m.title,m.archived_at_ms,r.created_at_ms,m.updated_at_ms,s.record_id IS NOT NULL FROM knowledge_lens_metadata m JOIN knowledge_records r ON r.record_id=m.record_id LEFT JOIN knowledge_standing_selection s ON s.loupist_id=m.loupist_id AND s.record_id=m.record_id WHERE m.loupist_id=?1 AND r.owner_loupist_id=?1 ORDER BY m.record_id")?;
    let mut standing = BTreeSet::new();
    let removed = super::memory_controls::load(tx, loupist)?.removed;
    for row in statement.query_map([loupist], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, bool>(5)?,
        ))
    })? {
        let (id, label, archived, created, updated, perspective) = row?;
        if removed.contains(&id) {
            continue;
        }
        generated = generated.max(updated).max(created);
        if perspective {
            standing.insert(id.clone());
        }
        let learned: Option<(i64,String)>=tx.query_row("SELECT finished_at_ms,rationale FROM knowledge_learning_reviews WHERE loupist_id=?1 AND lens_record_id=?2 AND status IN ('created','refined') ORDER BY finished_at_ms DESC LIMIT 1",rusqlite::params![loupist,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        nodes.insert(
            id.clone(),
            KnowledgeMapNode {
                origin: None,
                id: id.clone(),
                kind: if perspective {
                    "lens_perspective"
                } else if super::principles::principle_kind(tx, loupist, &id)? {
                    "lens_principle"
                } else {
                    "lens_method"
                }
                .into(),
                label,
                local_names: Vec::new(),
                entity_kind: None,
                category_override: None,
                kind_labels: None,
                degree: 0,
                first_seen_at_ms: Some(
                    first
                        .get(&("lens_created".into(), id.clone()))
                        .copied()
                        .unwrap_or(created),
                ),
                record_id: Some(id),
                archived: Some(archived.is_some()),
                watched: false,
                held: false,
                learned_at_ms: learned.as_ref().map(|v| v.0),
                learned_rationale: learned.map(|v| v.1),
            },
        );
    }
    let mut statement = tx.prepare("SELECT source_record_id,target_record_id FROM knowledge_lens_links WHERE loupist_id=?1 ORDER BY source_record_id,target_record_id")?;
    for row in statement.query_map([loupist], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (source, target) = row?;
        if nodes.get(&source).is_some_and(|n| n.kind != "entity")
            && nodes.get(&target).is_some_and(|n| n.kind != "entity")
        {
            edges.push(edge("lens_link", &source, &target));
        }
    }
    for journal in super::journal::list(tx, loupist)?
        .into_iter()
        .filter(|j| !j.archived)
    {
        generated = generated.max(journal.updated_at_ms);
        for entity in &journal.entities {
            if nodes.get(entity).is_some_and(|n| n.kind == "entity") {
                let mut about = edge("about", &journal.journal_id, entity);
                about.first_seen_at_ms = Some(journal.created_at_ms);
                edges.push(about);
            }
        }
        nodes.insert(
            journal.journal_id.clone(),
            KnowledgeMapNode {
                id: journal.journal_id.clone(),
                kind: "journal".into(),
                label: journal.judgment,
                local_names: Vec::new(),
                origin: None,
                entity_kind: None,
                category_override: None,
                kind_labels: None,
                degree: 0,
                first_seen_at_ms: Some(journal.created_at_ms),
                record_id: Some(journal.journal_id),
                archived: Some(false),
                watched: false,
                held: false,
                learned_at_ms: None,
                learned_rationale: None,
            },
        );
    }
    let (applications, application_total) =
        applications(tx, loupist, &standing, &nodes, &mut generated)?;
    edges.extend(applications);
    for e in &edges {
        if e.kind != "claim" {
            for id in BTreeSet::from([&e.source, &e.target]) {
                if let Some(node) = nodes.get_mut(id) {
                    node.degree += 1;
                }
            }
        }
    }
    let mut counts = BTreeMap::new();
    for kind in [
        "entity",
        "lens_perspective",
        "lens_method",
        "lens_principle",
        "claim",
        "lens_link",
        "applies_to",
        "journal",
        "about",
    ] {
        let total = nodes.values().filter(|n| n.kind == kind).count()
            + edges.iter().filter(|e| e.kind == kind).count();
        counts.insert(
            kind.into(),
            KnowledgeMapCount {
                total: if kind == "applies_to" {
                    application_total
                } else {
                    total as u32
                },
                shown: 0,
                truncated: false,
            },
        );
    }
    let mut chosen = BTreeSet::new();
    // Personal context takes priority; overflow remains visible through per-kind counts.
    for id in watched.union(&held) {
        if chosen.len() < limit as usize {
            chosen.insert(id.clone());
        }
    }
    for kind in [
        "lens_perspective",
        "lens_principle",
        "lens_method",
        "journal",
    ] {
        for node in nodes.values().filter(|n| n.kind == kind) {
            if chosen.len() < limit as usize {
                chosen.insert(node.id.clone());
            }
        }
    }
    let mut ranked: Vec<_> = nodes.values().filter(|n| n.kind == "entity").collect();
    ranked.sort_by(|a, b| {
        degrees
            .get(&b.id)
            .unwrap_or(&0)
            .cmp(degrees.get(&a.id).unwrap_or(&0))
            .then(a.id.cmp(&b.id))
    });
    let ranks: BTreeMap<_, _> = ranked
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut claims: Vec<_> = edges.iter().filter(|e| e.kind == "claim").collect();
    let rank = |e: &KnowledgeMapEdge| {
        let mut r = [
            *ranks.get(e.source.as_str()).unwrap_or(&usize::MAX),
            *ranks.get(e.target.as_str()).unwrap_or(&usize::MAX),
        ];
        r.sort_unstable();
        r
    };
    claims.sort_by_key(|e| (rank(e), e.id.as_str()));
    let mut selected_claims = BTreeSet::new();
    for e in claims {
        if selected_claims.len() == 2000 {
            break;
        }
        let added: BTreeSet<_> = [&e.source, &e.target]
            .into_iter()
            .filter(|id| !chosen.contains(*id) && nodes.contains_key(*id))
            .cloned()
            .collect();
        if chosen.len() + added.len() <= limit as usize {
            chosen.extend(added);
            selected_claims.insert(e.id.clone());
        }
    }
    for node in ranked {
        if chosen.len() < limit as usize {
            chosen.insert(node.id.clone());
        }
    }
    let nodes: Vec<_> = nodes
        .into_values()
        .filter(|n| chosen.contains(&n.id))
        .collect();
    edges.retain(|e| {
        chosen.contains(&e.source)
            && chosen.contains(&e.target)
            && (e.kind != "claim" || selected_claims.contains(&e.id))
    });
    edges.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    for node in &nodes {
        counts.get_mut(&node.kind).expect("node kind").shown += 1;
    }
    for e in &edges {
        counts.get_mut(&e.kind).expect("edge kind").shown += 1;
    }
    for count in counts.values_mut() {
        count.truncated = count.shown < count.total;
    }
    Ok(KnowledgeMap {
        nodes,
        edges,
        unmatched_holdings,
        unmatched_watchlist,
        counts,
        world_revision_id: world.map(|row| row.0),
        generated_at_ms: generated,
    })
}

fn applications(
    tx: &Transaction<'_>,
    loupist: &str,
    standing: &BTreeSet<String>,
    nodes: &BTreeMap<String, KnowledgeMapNode>,
    generated: &mut i64,
) -> KnowledgeResult<(Vec<KnowledgeMapEdge>, u32)> {
    let mut delivered = BTreeMap::<String, BTreeMap<String, i64>>::new();
    let mut statement=tx.prepare("SELECT turn_id,record_id,MIN(issued_at_ms) FROM knowledge_lens_deliveries WHERE loupist_id=?1 AND COALESCE(json_extract(document_json,'$.delivery'),'exact_read') <> 'context' GROUP BY turn_id,record_id ORDER BY turn_id,record_id")?;
    for row in statement.query_map([loupist], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (turn, id, at) = row?;
        if !standing.contains(&id) && nodes.get(&id).is_some_and(|n| n.kind == "lens_method") {
            delivered.entry(turn).or_default().insert(id, at);
        }
    }
    let mut pairs = BTreeMap::<(String, String), BTreeMap<String, i64>>::new();
    let mut statement=tx.prepare("SELECT turn_id,view_json,issued_at_ms FROM knowledge_world_deliveries WHERE loupist_id=?1 ORDER BY turn_id,delivery_id")?;
    for row in statement.query_map([loupist], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (turn, json, at) = row?;
        let Some(lenses) = delivered.get(&turn) else {
            continue;
        };
        let view: serde_json::Value = serde_json::from_str(&json)?;
        let ids = view
            .get("nodes")
            .and_then(|n| n.as_array())
            .into_iter()
            .flatten()
            .filter_map(|n| n.get("id").and_then(|id| id.as_str()))
            .filter(|id| nodes.get(*id).is_some_and(|n| n.kind == "entity"))
            .collect::<BTreeSet<_>>();
        for (lens, lens_at) in lenses {
            let at = at.max(*lens_at); // both deliveries have occurred at this instant
            *generated = (*generated).max(at);
            for id in &ids {
                pairs
                    .entry((lens.clone(), (*id).into()))
                    .or_default()
                    .entry(turn.clone())
                    .and_modify(|old| *old = (*old).min(at))
                    .or_insert(at);
            }
        }
    }
    let mut cited=tx.prepare("SELECT record_id,entity_id,turn_id,at_ms FROM knowledge_lens_applications WHERE loupist_id=?1 ORDER BY record_id,entity_id,turn_id")?;
    for row in cited.query_map([loupist], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })? {
        let (lens, entity, turn, at) = row?;
        if nodes
            .get(&lens)
            .is_some_and(|n| matches!(n.kind.as_str(), "lens_method" | "lens_principle"))
            && nodes.get(&entity).is_some_and(|n| n.kind == "entity")
        {
            *generated = (*generated).max(at);
            pairs
                .entry((lens, entity))
                .or_default()
                .entry(turn)
                .and_modify(|old| *old = (*old).min(at))
                .or_insert(at);
        }
    }
    let total = pairs.len() as u32;
    let mut by_lens = BTreeMap::<String, Vec<KnowledgeMapEdge>>::new();
    for ((lens, entity), turns) in pairs {
        let mut e = edge("applies_to", &lens, &entity);
        e.weight = Some(turns.len() as u32);
        e.first_seen_at_ms = turns.values().copied().min();
        by_lens.entry(lens).or_default().push(e);
    }
    let mut result = Vec::new();
    for mut edges in by_lens.into_values() {
        edges.sort_by(|a, b| {
            b.weight
                .cmp(&a.weight)
                .then(a.first_seen_at_ms.cmp(&b.first_seen_at_ms))
                .then(a.target.cmp(&b.target))
        });
        edges.truncate(30);
        result.extend(edges);
    }
    Ok((result, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use serde_json::json;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(&crate::knowledge::embedded_ddl()).unwrap();
        db.execute_batch(
            "CREATE TABLE app_settings (
                id TEXT PRIMARY KEY NOT NULL,
                data_json TEXT NOT NULL,
                CHECK (id IN ('model', 'model_visibility', 'update')
                    OR (substr(id, 1, 17) = 'world_processing:' AND length(id) > 17))
            ) STRICT;",
        )
        .unwrap();
        db
    }
    fn fixture(n: usize, claims: usize) -> WorldSnapshotBody {
        serde_json::from_value(json!({
            "nodes":(0..n).map(|i|json!({"id":format!("n{i:05}"),"name":format!("Company {i}"),"aliases":[format!("T{i}")],"description":"","kind":"company","sourceRole":"entity","properties":if i==n-1 {json!({"ticker":"LAST"})}else{json!({})}})).collect::<Vec<_>>(),
            "claims":(0..claims).map(|i|json!({"id":format!("c{i:05}"),"proposition":"supplies","attribution":"source","relation":"supplies","endpoints":[{"nodeId":format!("n{:05}",i%n),"role":"subject"},{"nodeId":format!("n{:05}",(i+1)%n),"role":"object"}],"qualifiers":[],"evidence":[],"status":"active","recordedAtMs":100+i as i64,"publicationTime":null,"effectiveTime":null,"validAt":null,"invalidAt":null,"expiredAt":null})).collect::<Vec<_>>(),"annotations":[]
        })).unwrap()
    }
    fn world(tx: &Transaction<'_>, body: &WorldSnapshotBody) {
        tx.execute(
            "INSERT INTO knowledge_records VALUES ('world','world','loupist-a','World',1)",
            [],
        )
        .unwrap();
        tx.execute("INSERT INTO knowledge_record_revisions VALUES ('revision','world','digest','{}','[]',1)",[]).unwrap();
        tx.execute(
            "INSERT INTO knowledge_record_heads VALUES ('world','revision')",
            [],
        )
        .unwrap();
        tx.execute("INSERT INTO knowledge_world_snapshots VALUES ('snapshot','world','revision','digest',?1,1)",[serde_json::to_string(body).unwrap()]).unwrap();
    }
    fn lens(tx: &Transaction<'_>, id: &str, standing: bool) {
        tx.execute(
            "INSERT INTO knowledge_records VALUES (?1,'lens','loupist-a',?1,50)",
            [id],
        )
        .unwrap();
        tx.execute("INSERT INTO knowledge_lens_metadata(record_id,loupist_id,title,title_fold,updated_at_ms) VALUES (?1,'loupist-a',?1,?1,60)",[id]).unwrap();
        if standing {
            tx.execute("INSERT INTO knowledge_standing_selection VALUES ('loupist-a',?1,'lens-revision','standing_philosophy',60)",[id]).unwrap();
        }
    }
    fn event(tx: &Transaction<'_>, kind: &str, id: &str, at: i64) {
        tx.execute("INSERT INTO knowledge_growth_events(event_id,loupist_id,at_ms,kind,record_id,subject_id,actor,summary) VALUES (?1,'loupist-a',?2,?3,?4,?4,'import','added')",params![format!("{kind}-{id}-{at}"),at,kind,id]).unwrap();
    }
    fn item(symbol: &str, name: Option<&str>) -> MapContextItem {
        MapContextItem {
            symbol: symbol.into(),
            market: "NASDAQ".into(),
            display_name: name.map(str::to_owned),
        }
    }
    fn request(limit: Option<u32>) -> KnowledgeMapQuery {
        KnowledgeMapQuery {
            loupist_id: "loupist-a".into(),
            max_nodes: limit,
        }
    }
    fn delivery(tx: &Transaction<'_>, turn: &str, id: &str, at: i64, ids: &[String]) {
        tx.execute(
            "INSERT OR IGNORE INTO knowledge_turns VALUES (?1,'loupist-a','chat',1)",
            [turn],
        )
        .unwrap();
        tx.execute("INSERT INTO knowledge_lens_deliveries VALUES (?1,?2,'loupist-a',?3,'rev','digest','{}','digest',?4)",params![format!("l-{turn}-{id}-{at}"),turn,id,at]).unwrap();
        tx.execute("INSERT INTO knowledge_world_deliveries VALUES (?1,?2,'loupist-a','world','revision','digest',?3,'digest',?4)",params![format!("w-{turn}-{id}-{at}"),turn,serde_json::to_string(&json!({"nodes":ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>()})).unwrap(),at+1]).unwrap();
    }

    #[test]
    fn bounds_force_personal_entities_and_report_unmatched_without_financial_values() {
        let mut db = database();
        let tx = db.transaction().unwrap();
        world(&tx, &fixture(50, 20));
        lens(&tx, "method", false);
        let context = MapContext {
            watchlist: vec![item("last", None), item("missing", None)],
            holdings: vec![
                item("other", Some("  COMPANY   48  ")),
                item("unmatched", None),
            ],
            updated_at_ms: 900,
        };
        let map = query(&tx, &request(Some(5)), &context).unwrap();
        assert_eq!(map.nodes.len(), 5);
        assert!(map.nodes.iter().any(|n| n.id == "n00049" && n.watched));
        assert!(map.nodes.iter().any(|n| n.id == "n00048" && n.held));
        assert_eq!(map.unmatched_watchlist, vec![item("missing", None)]);
        assert_eq!(map.unmatched_holdings, vec![item("unmatched", None)]);
        assert_eq!(map.counts["entity"].total, 50);
        assert!(map.counts["entity"].truncated);
        assert!(map
            .edges
            .iter()
            .all(|e| map.nodes.iter().any(|n| n.id == e.source)
                && map.nodes.iter().any(|n| n.id == e.target)));
        let json = serde_json::to_value(&map).unwrap();
        fn assert_no_finances(value: &serde_json::Value) {
            match value {
                serde_json::Value::Object(object) => {
                    for (key, value) in object {
                        assert!(![
                            "quantity",
                            "value",
                            "marketValue",
                            "costBasis",
                            "positionSize",
                            "targetWeight"
                        ]
                        .contains(&key.as_str()));
                        assert_no_finances(value)
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        assert_no_finances(value)
                    }
                }
                _ => {}
            }
        }
        assert_no_finances(&json);
        assert_eq!(map.generated_at_ms, 900);
        assert!(query(&tx, &request(Some(1501)), &context).is_err());
        assert!(query(&tx, &request(Some(0)), &context).is_err());
        let overflow = query(&tx, &request(Some(1)), &context).unwrap();
        assert_eq!(overflow.nodes.len(), 1);
        assert!(overflow.nodes[0].watched || overflow.nodes[0].held);
    }

    #[test]
    fn replay_uses_events_then_incident_claims_and_record_creation() {
        let mut db = database();
        let tx = db.transaction().unwrap();
        world(&tx, &fixture(4, 2));
        lens(&tx, "method", false);
        lens(&tx, "perspective", true);
        tx.execute(
            "UPDATE knowledge_lens_metadata SET archived_at_ms=55 WHERE record_id='method'",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO knowledge_lens_links VALUES ('loupist-a','method','perspective')",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO knowledge_lens_links VALUES ('loupist-a','method','n00000')",
            [],
        )
        .unwrap();
        event(&tx, "entity_added", "n00000", 20);
        event(&tx, "entity_added", "n00000", 10);
        event(&tx, "claim_added", "c00000", 30);
        event(&tx, "claim_added", "c00000", 40);
        event(&tx, "lens_created", "method", 45);
        let map = query(&tx, &request(None), &MapContext::default()).unwrap();
        let at = |id: &str| {
            map.nodes
                .iter()
                .find(|n| n.id == id)
                .unwrap()
                .first_seen_at_ms
        };
        assert_eq!(at("n00000"), Some(10));
        assert_eq!(at("n00001"), Some(100));
        assert_eq!(at("n00002"), Some(101));
        assert_eq!(at("n00003"), None);
        assert_eq!(at("method"), Some(45));
        assert_eq!(
            map.nodes
                .iter()
                .find(|n| n.id == "method")
                .unwrap()
                .archived,
            Some(true)
        );
        let links = map
            .edges
            .iter()
            .filter(|e| e.kind == "lens_link")
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 1);
        assert_eq!(
            (&links[0].source, &links[0].target),
            (&"method".to_owned(), &"perspective".to_owned())
        );

        assert_eq!(at("perspective"), Some(50));
        assert_eq!(
            map.edges
                .iter()
                .find(|e| e.id == "c00000")
                .unwrap()
                .first_seen_at_ms,
            Some(30)
        );
        assert_eq!(
            map.edges
                .iter()
                .find(|e| e.id == "c00001")
                .unwrap()
                .first_seen_at_ms,
            Some(101)
        );
        let foreign = query(
            &tx,
            &KnowledgeMapQuery {
                loupist_id: "loupist-b".into(),
                max_nodes: None,
            },
            &MapContext::default(),
        )
        .unwrap();
        assert!(foreign.nodes.is_empty());
        assert!(foreign.world_revision_id.is_none());
    }

    #[test]
    fn applications_count_distinct_turns_exclude_perspective_and_keep_top_thirty() {
        let mut db = database();
        let tx = db.transaction().unwrap();
        world(&tx, &fixture(42, 0));
        lens(&tx, "method", false);
        lens(&tx, "perspective", true);
        let ids = (0..40).map(|i| format!("n{i:05}")).collect::<Vec<_>>();
        delivery(&tx, "turn-1", "method", 100, &ids);
        delivery(&tx, "turn-1", "method", 105, &ids);
        delivery(&tx, "turn-1", "perspective", 90, &ids);
        delivery(&tx, "turn-2", "method", 200, &["n00039".into()]);
        delivery(&tx, "turn-3", "perspective", 300, &["n00041".into()]);
        let map = query(&tx, &request(None), &MapContext::default()).unwrap();
        let applications = map
            .edges
            .iter()
            .filter(|e| e.kind == "applies_to")
            .collect::<Vec<_>>();
        assert_eq!(applications.len(), 30);
        assert!(applications.iter().all(|e| e.source == "method"));
        let heavy = applications.iter().find(|e| e.target == "n00039").unwrap();
        assert_eq!(heavy.weight, Some(2));
        assert_eq!(heavy.first_seen_at_ms, Some(100));
        assert!(applications.iter().any(|e| e.target == "n00028"));
        assert!(!applications.iter().any(|e| e.target == "n00029"));
        assert!(applications
            .iter()
            .filter(|e| e.target != "n00039")
            .all(|e| e.weight == Some(1)));
        assert_eq!(map.counts["applies_to"].total, 40);
        assert!(map.counts["applies_to"].truncated);
    }

    #[test]
    fn serialization_is_identical_across_reads_and_snapshot_ordering() {
        let mut db = database();
        let tx = db.transaction().unwrap();
        let body = fixture(80, 150);
        world(&tx, &body);
        lens(&tx, "method", false);
        let first =
            serde_json::to_vec(&query(&tx, &request(Some(20)), &MapContext::default()).unwrap())
                .unwrap();
        let second =
            serde_json::to_vec(&query(&tx, &request(Some(20)), &MapContext::default()).unwrap())
                .unwrap();
        assert_eq!(first, second);
        let mut other = database();
        let tx2 = other.transaction().unwrap();
        let mut reversed = body;
        reversed.nodes.reverse();
        reversed.claims.reverse();
        world(&tx2, &reversed);
        lens(&tx2, "method", false);
        assert_eq!(
            first,
            serde_json::to_vec(&query(&tx2, &request(Some(20)), &MapContext::default()).unwrap())
                .unwrap()
        );
    }

    #[test]
    fn app_context_reads_only_latest_holdings_and_identity_fields() {
        use crate::store::user_context::{HoldingsPosition, HoldingsSource};
        let temp = tempfile::tempdir().unwrap();
        let store = crate::store::SqliteStore::open(temp.path().join("app.sqlite")).unwrap();
        store
            .watchlist_add("t1", "nasdaq", Some("Company 1"))
            .unwrap();
        let position = |symbol: &str| HoldingsPosition {
            symbol: symbol.into(),
            market: "NASDAQ".into(),
            display_name: None,
            quantity: "999.5".into(),
            cost_basis: Some("123.45".into()),
            market_value: Some("4567".into()),
            currency: "USD".into(),
        };
        store
            .save_holdings_snapshot(HoldingsSource::Csv, vec![position("OLD")])
            .unwrap();
        store
            .save_holdings_snapshot(HoldingsSource::Csv, vec![position("NEW")])
            .unwrap();
        let context = store.map_context().unwrap();
        assert_eq!(context.watchlist[0].symbol, "T1");
        assert_eq!(context.holdings, vec![item("NEW", None)]);
        let json = serde_json::to_string(&context.holdings).unwrap();
        assert!(!json.contains("999.5"));
        assert!(!json.contains("quantity"));
    }

    #[test]
    fn local_names_keep_non_latin_aliases_and_skip_other_nodes() {
        let mut db = database();
        let tx = db.transaction().unwrap();
        let hangul: String = "가".repeat(81);
        let body = serde_json::from_value(json!({
            "nodes": [
                {"id":"hanbit","name":"Hanbit Battery","aliases":["412340","한빛배터리","Hanbit"],"description":"","kind":"Organization","sourceRole":"entity","properties":{}},
                {"id":"trimmed","name":"Trimmed","aliases":[hangul, "  서울  ","Hanbit","도쿄","베를린","파리"],"description":"","kind":"Place","sourceRole":"entity","properties":{}}
            ],
            "claims": [],
            "annotations": []
        })).unwrap();
        world(&tx, &body);
        lens(&tx, "method", false);
        let map = query(&tx, &request(None), &MapContext::default()).unwrap();
        let names = |id: &str| &map.nodes.iter().find(|n| n.id == id).unwrap().local_names;
        assert_eq!(names("hanbit"), &["한빛배터리".to_string()]);
        let trimmed = names("trimmed");
        assert!(trimmed.len() < 6 && trimmed[0].chars().count() < 81);
        assert!(trimmed
            .iter()
            .all(|name| name != "Hanbit" && name == name.trim()));
        assert!(names("method").is_empty());
    }
}
