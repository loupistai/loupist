//! The derived World index in each Loupist's own database: name and fact
//! vectors (the bundled potion model, trigram fallback for zero vectors) and
//! FTS5 text, stamped with the sealed World revision it was built from.
//!
//! The canonical World is the sealed snapshot; every row here can be rebuilt
//! from it without a model call. A job writes these rows in the transaction
//! that seals its World, so the index is never ahead of or behind a confirmed
//! head it claims. A stale or missing stamp is rebuilt on first use.
//!
//! Tables are created on first use and carry `loupist_id`, so folder
//! admission's single-owner check covers them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, OptionalExtension, Transaction};

use super::super::error::{KnowledgeError, KnowledgeResult};
use super::super::model::WorldSnapshotBody;
use super::pipeline::ranked_by_similarity;
use super::search;
use super::text::replace_unusable;
use crate::util::sha256;

const DDL: &str = "
CREATE TABLE IF NOT EXISTS world_index_state (
    loupist_id TEXT NOT NULL,
    world_record_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    model TEXT NOT NULL,
    PRIMARY KEY (loupist_id, world_record_id)
) STRICT;
CREATE TABLE IF NOT EXISTS world_index_nodes (
    loupist_id TEXT NOT NULL,
    world_record_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    vector_digest TEXT NOT NULL,
    text_digest TEXT NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY (loupist_id, world_record_id, node_id)
) STRICT;
CREATE TABLE IF NOT EXISTS world_index_claims (
    loupist_id TEXT NOT NULL,
    world_record_id TEXT NOT NULL,
    claim_id TEXT NOT NULL,
    vector_digest TEXT NOT NULL,
    text_digest TEXT NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY (loupist_id, world_record_id, claim_id)
) STRICT;
CREATE VIRTUAL TABLE IF NOT EXISTS world_index_node_text USING fts5(
    loupist_id UNINDEXED, world_record_id UNINDEXED, node_id UNINDEXED,
    name, summary, aliases);
CREATE VIRTUAL TABLE IF NOT EXISTS world_index_claim_text USING fts5(
    loupist_id UNINDEXED, world_record_id UNINDEXED, claim_id UNINDEXED,
    name, fact);
";

/// Index version plus the loaded model's fingerprint; a change rebuilds.
pub(crate) fn model_stamp(assets: &Path) -> KnowledgeResult<String> {
    let fingerprint = super::super::graph_inference::local_english_model_fingerprint(assets)
        .map_err(|_| KnowledgeError::Invalid("local embedding model is unavailable"))?;
    Ok(format!("world-index/v1/trigram-fb1/{fingerprint}"))
}

/// Local vectors for index rows, as the extraction's embedding calls produce them.
pub(crate) fn embed_texts(assets: &Path, texts: &[String]) -> KnowledgeResult<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let mut vectors = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(256) {
        let encoded = super::super::graph_inference::embed_local_english_batch(assets, chunk)
            .map_err(|_| KnowledgeError::Invalid("local embedding failed"))?;
        for (text, vector) in chunk.iter().zip(encoded) {
            vectors.push(replace_unusable(text, vector, 128));
        }
    }
    Ok(vectors)
}

pub(crate) fn ensure(tx: &Transaction<'_>) -> KnowledgeResult<()> {
    tx.execute_batch(DDL)?;
    Ok(())
}

fn tables_exist(tx: &Transaction<'_>) -> KnowledgeResult<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'world_index_state')",
        [],
        |row| row.get(0),
    )?)
}

/// The revision and model the stored index was built from.
pub(crate) fn stamp(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
) -> KnowledgeResult<Option<(String, String)>> {
    if !tables_exist(tx)? {
        return Ok(None);
    }
    Ok(tx
        .query_row(
            "SELECT revision_id, model FROM world_index_state WHERE loupist_id = ?1 AND world_record_id = ?2",
            params![loupist_id, world_record_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

fn node_text(node: &super::super::model::GraphNode) -> (String, String, String) {
    let mut aliases: Vec<&str> = Vec::new();
    for alias in &node.aliases {
        if !alias.trim().is_empty() && !aliases.contains(&alias.as_str()) {
            aliases.push(alias);
        }
    }
    (
        node.name.clone(),
        node.description.clone(),
        aliases.join("\n"),
    )
}

fn digest(parts: &[&str]) -> String {
    sha256(parts.join("\u{0}").as_bytes())
}

/// Texts whose vectors a sync of `body` would need, given the stored rows.
pub(crate) fn missing_texts(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    body: &WorldSnapshotBody,
    model: &str,
) -> KnowledgeResult<Vec<String>> {
    let current =
        matches!(stamp(tx, loupist_id, world_record_id)?, Some((_, stored)) if stored == model);
    let (nodes, claims) = if current {
        stored_digests(tx, loupist_id, world_record_id)?
    } else {
        (HashMap::new(), HashMap::new())
    };
    let mut texts = Vec::new();
    let mut seen = HashSet::new();
    for node in &body.nodes {
        if nodes.get(&node.id).map(|(vector, _)| vector) != Some(&digest(&[&node.name]))
            && seen.insert(node.name.clone())
        {
            texts.push(node.name.clone());
        }
    }
    for claim in &body.claims {
        if claims.get(&claim.id).map(|(vector, _)| vector) != Some(&digest(&[&claim.proposition]))
            && seen.insert(claim.proposition.clone())
        {
            texts.push(claim.proposition.clone());
        }
    }
    Ok(texts)
}

type Digests = HashMap<String, (String, String)>;

fn stored_digests(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
) -> KnowledgeResult<(Digests, Digests)> {
    let read = |table: &str, id: &str| -> KnowledgeResult<Digests> {
        let mut statement = tx.prepare(&format!(
            "SELECT {id}, vector_digest, text_digest FROM {table} WHERE loupist_id = ?1 AND world_record_id = ?2"
        ))?;
        let rows = statement.query_map(params![loupist_id, world_record_id], |row| {
            Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    };
    Ok((
        read("world_index_nodes", "node_id")?,
        read("world_index_claims", "claim_id")?,
    ))
}

fn vector_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn vector_from(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let vector: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("four bytes")))
        .collect();
    vector
        .iter()
        .all(|value| value.is_finite())
        .then_some(vector)
}

/// Bring the index to `body` (the sealed `revision_id`) and stamp it.
///
/// `vectors` holds text → vector for every text `missing_texts` named; a text
/// it lacks is a programming error and fails the transaction.
pub(crate) fn sync(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    revision_id: &str,
    body: &WorldSnapshotBody,
    model: &str,
    vectors: &HashMap<String, Vec<f32>>,
) -> KnowledgeResult<()> {
    ensure(tx)?;
    let current =
        matches!(stamp(tx, loupist_id, world_record_id)?, Some((_, stored)) if stored == model);
    if !current {
        for table in [
            "world_index_nodes",
            "world_index_claims",
            "world_index_node_text",
            "world_index_claim_text",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE loupist_id = ?1 AND world_record_id = ?2"),
                params![loupist_id, world_record_id],
            )?;
        }
    }
    let (stored_nodes, stored_claims) = stored_digests(tx, loupist_id, world_record_id)?;
    let vector_for = |text: &str| -> KnowledgeResult<Vec<u8>> {
        vectors
            .get(text)
            .map(|vector| vector_bytes(vector))
            .ok_or(KnowledgeError::Invalid("World index vector is missing"))
    };
    let mut keep_nodes = HashSet::new();
    for node in &body.nodes {
        keep_nodes.insert(node.id.clone());
        let (name, summary, aliases) = node_text(node);
        let vector_digest = digest(&[&name]);
        let text_digest = digest(&[&name, &summary, &aliases]);
        let stored = stored_nodes.get(&node.id);
        if stored.map(|(vector, _)| vector) != Some(&vector_digest) {
            tx.execute(
                "INSERT INTO world_index_nodes (loupist_id, world_record_id, node_id, vector_digest, text_digest, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(loupist_id, world_record_id, node_id) DO UPDATE SET
                    vector_digest = excluded.vector_digest, vector = excluded.vector",
                params![loupist_id, world_record_id, node.id, vector_digest, "", vector_for(&name)?],
            )?;
        }
        if stored.map(|(_, text)| text) != Some(&text_digest) {
            tx.execute(
                "DELETE FROM world_index_node_text WHERE loupist_id = ?1 AND world_record_id = ?2 AND node_id = ?3",
                params![loupist_id, world_record_id, node.id],
            )?;
            tx.execute(
                "INSERT INTO world_index_node_text (loupist_id, world_record_id, node_id, name, summary, aliases)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![loupist_id, world_record_id, node.id, name, summary, aliases],
            )?;
            tx.execute(
                "UPDATE world_index_nodes SET text_digest = ?4 WHERE loupist_id = ?1 AND world_record_id = ?2 AND node_id = ?3",
                params![loupist_id, world_record_id, node.id, text_digest],
            )?;
        }
    }
    let mut keep_claims = HashSet::new();
    for claim in &body.claims {
        keep_claims.insert(claim.id.clone());
        let vector_digest = digest(&[&claim.proposition]);
        let text_digest = digest(&[&claim.relation, &claim.proposition]);
        let stored = stored_claims.get(&claim.id);
        if stored.map(|(vector, _)| vector) != Some(&vector_digest) {
            tx.execute(
                "INSERT INTO world_index_claims (loupist_id, world_record_id, claim_id, vector_digest, text_digest, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(loupist_id, world_record_id, claim_id) DO UPDATE SET
                    vector_digest = excluded.vector_digest, vector = excluded.vector",
                params![loupist_id, world_record_id, claim.id, vector_digest, "", vector_for(&claim.proposition)?],
            )?;
        }
        if stored.map(|(_, text)| text) != Some(&text_digest) {
            tx.execute(
                "DELETE FROM world_index_claim_text WHERE loupist_id = ?1 AND world_record_id = ?2 AND claim_id = ?3",
                params![loupist_id, world_record_id, claim.id],
            )?;
            tx.execute(
                "INSERT INTO world_index_claim_text (loupist_id, world_record_id, claim_id, name, fact)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![loupist_id, world_record_id, claim.id, claim.relation, claim.proposition],
            )?;
            tx.execute(
                "UPDATE world_index_claims SET text_digest = ?4 WHERE loupist_id = ?1 AND world_record_id = ?2 AND claim_id = ?3",
                params![loupist_id, world_record_id, claim.id, text_digest],
            )?;
        }
    }
    for (id, _) in stored_nodes
        .iter()
        .filter(|(id, _)| !keep_nodes.contains(*id))
    {
        for table in ["world_index_nodes", "world_index_node_text"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE loupist_id = ?1 AND world_record_id = ?2 AND node_id = ?3"),
                params![loupist_id, world_record_id, id],
            )?;
        }
    }
    for (id, _) in stored_claims
        .iter()
        .filter(|(id, _)| !keep_claims.contains(*id))
    {
        for table in ["world_index_claims", "world_index_claim_text"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE loupist_id = ?1 AND world_record_id = ?2 AND claim_id = ?3"),
                params![loupist_id, world_record_id, id],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO world_index_state (loupist_id, world_record_id, revision_id, model) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(loupist_id, world_record_id) DO UPDATE SET revision_id = excluded.revision_id, model = excluded.model",
        params![loupist_id, world_record_id, revision_id, model],
    )?;
    Ok(())
}

/// Stored vectors by id for the stamped revision.
pub(crate) struct Vectors {
    pub nodes: HashMap<String, Vec<f32>>,
    pub claims: HashMap<String, Vec<f32>>,
}

pub(crate) fn load_vectors(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
) -> KnowledgeResult<Vectors> {
    let read = |table: &str, id: &str| -> KnowledgeResult<HashMap<String, Vec<f32>>> {
        let mut statement = tx.prepare(&format!(
            "SELECT {id}, vector FROM {table} WHERE loupist_id = ?1 AND world_record_id = ?2"
        ))?;
        let rows = statement.query_map(params![loupist_id, world_record_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, bytes) = row?;
            if let Some(vector) = vector_from(&bytes) {
                out.insert(id, vector);
            }
        }
        Ok(out)
    };
    Ok(Vectors {
        nodes: read("world_index_nodes", "node_id")?,
        claims: read("world_index_claims", "claim_id")?,
    })
}

/// Hybrid search over the stored index (COMBINED_HYBRID_SEARCH_RRF): ranked
/// claim ids and node ids, each BM25 and cosine (> 0.6) fused with RRF.
pub(crate) fn hybrid_hits(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    query: &str,
    vector: &[f32],
    vectors: &Vectors,
    limit: usize,
) -> (Vec<String>, Vec<String>) {
    let bind = [loupist_id, world_record_id];
    let claim_text = search::bm25(
        tx,
        "SELECT claim_id FROM world_index_claim_text WHERE world_index_claim_text MATCH ?1
           AND loupist_id = ?2 AND world_record_id = ?3 ORDER BY bm25(world_index_claim_text)",
        &bind,
        query,
        None,
        2 * limit,
    );
    let mut claim_rows: Vec<(&String, &Vec<f32>)> = vectors.claims.iter().collect();
    claim_rows.sort_by(|left, right| left.0.cmp(right.0));
    let claim_cosine = ranked_by_similarity(
        vector,
        claim_rows
            .into_iter()
            .map(|(id, vector)| (id.as_str(), vector.as_slice())),
        2 * limit,
    );
    let node_text = search::bm25(
        tx,
        "SELECT node_id FROM world_index_node_text WHERE world_index_node_text MATCH ?1
           AND loupist_id = ?2 AND world_record_id = ?3 ORDER BY bm25(world_index_node_text)",
        &bind,
        query,
        None,
        2 * limit,
    );
    let mut node_rows: Vec<(&String, &Vec<f32>)> = vectors.nodes.iter().collect();
    node_rows.sort_by(|left, right| left.0.cmp(right.0));
    let node_cosine = ranked_by_similarity(
        vector,
        node_rows
            .into_iter()
            .map(|(id, vector)| (id.as_str(), vector.as_slice())),
        2 * limit,
    );
    let edges = search::rrf(&[claim_text, claim_cosine])
        .into_iter()
        .take(limit)
        .collect();
    let nodes = search::rrf(&[node_text, node_cosine])
        .into_iter()
        .take(limit)
        .collect();
    (edges, nodes)
}
