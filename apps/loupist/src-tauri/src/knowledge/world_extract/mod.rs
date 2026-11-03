//! World extraction and World graph retrieval in Rust.
//!
//! This replaces the Python Graphiti worker and its Neo4j index with the same
//! stages, prompts (vendored from graphiti-core 0.30.1 under Apache-2.0, see
//! `prompts/NOTICE`), response schemas and model path. The derived index
//! lives in the Loupist's database beside the canonical World it is built from.
//!
mod bridge;
pub(crate) mod hints;
mod index;
mod job;
mod ontology;
mod pipeline;
mod projection;
mod prompts;
mod pyjson;
mod schema;
mod search;
mod text;

use std::path::Path;

use serde_json::{json, Value};

use super::error::KnowledgeResult;
use super::model::WorldSnapshotBody;
use super::KnowledgeStore;

pub(crate) use job::drive_job;

const QUERY_LIMIT: usize = 25;

/// Hybrid World search over the derived index for one sealed revision.
/// Returns `None` when that revision is not the
/// indexed head, so the caller answers from the canonical snapshot instead.
pub(crate) fn query_world(
    store: &KnowledgeStore,
    loupist_id: &str,
    world_record_id: &str,
    revision_id: &str,
    search_text: &str,
    assets: &Path,
) -> KnowledgeResult<Option<Value>> {
    if !job::ensure_index(store, loupist_id, world_record_id, revision_id, assets)? {
        return Ok(None);
    }
    let query = search_text.replace('\n', " ");
    let vector = index::embed_texts(assets, std::slice::from_ref(&query))?
        .pop()
        .unwrap_or_default();
    let model = index::model_stamp(assets)?;
    store.read(|read| {
        match index::stamp(read.tx, loupist_id, world_record_id)? {
            Some((stamped, stored)) if stamped == revision_id && stored == model => {}
            _ => return Ok(None),
        }
        let stored = super::revision::load_revision(read.tx, revision_id)?;
        let body: WorldSnapshotBody = serde_json::from_str(&stored.payload_json)?;
        let mut nodes = Vec::with_capacity(body.nodes.len());
        for node in &body.nodes {
            nodes.push(super::graph_projection::worker_node(node)?);
        }
        let mut claims = Vec::with_capacity(body.claims.len());
        for claim in &body.claims {
            claims.push(super::graph_projection::worker_claim(claim)?);
        }
        let ranked = if claims.is_empty() {
            Vec::new()
        } else {
            let vectors = index::load_vectors(read.tx, loupist_id, world_record_id)?;
            let (edge_hits, node_hits) = index::hybrid_hits(
                read.tx,
                loupist_id,
                world_record_id,
                search_text,
                &vector,
                &vectors,
                QUERY_LIMIT,
            );
            search::rank_hybrid_claims(&edge_hits, &node_hits, &claims, QUERY_LIMIT)
        };
        let view = search::page_claims(
            world_record_id,
            revision_id,
            &nodes,
            &ranked,
            &search::PageLimits {
                node_limit: QUERY_LIMIT,
                edge_limit: QUERY_LIMIT,
                byte_limit: 262_144,
            },
        );
        Ok(Some(json!({
            "mode": "hybrid",
            "indexed": true,
            "revisionId": revision_id,
            "generationId": null,
            "view": {
                "view": view,
                "usage": {
                    "llm_calls": 0,
                    "embedding_calls": 1,
                    "input_tokens": 0,
                    "output_tokens": 0,
                    "certainty": "accounted",
                },
            },
        })))
    })
}

#[cfg(test)]
mod tests;
