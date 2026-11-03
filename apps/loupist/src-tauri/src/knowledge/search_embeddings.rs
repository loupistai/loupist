//! Disposable, host-private vectors. Canonical revisions remain the authority.
//!
//! Keep vectors used by current revisions instead of a fixed-size scan cache:
//! an LRU smaller than a full scan would evict its own next query's working set.

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::KnowledgeResult;

pub(super) struct EmbeddingCache {
    connection: Connection,
    model: String,
}

impl EmbeddingCache {
    pub(super) fn open(path: &Path, model: &str) -> KnowledgeResult<Self> {
        crate::fs::artifact_trust::ensure_private_directory(
            path.parent()
                .ok_or(super::KnowledgeError::Invalid("cache path has no parent"))?,
        )?;
        crate::fs::artifact_trust::ensure_private_regular_file(path)?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_millis(100))?;
        // Only this rebuildable cache uses WAL/NORMAL. Canonical Loupist databases
        // retain their full-synchronous rollback-journal durability contract.
        connection.execute_batch(
            "PRAGMA auto_vacuum = INCREMENTAL;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS vectors (
                model TEXT NOT NULL, digest TEXT NOT NULL, vector BLOB NOT NULL,
                PRIMARY KEY (model, digest));
             CREATE TABLE IF NOT EXISTS revision_vectors (
                revision TEXT NOT NULL, model TEXT NOT NULL, digest TEXT NOT NULL,
                PRIMARY KEY (revision, model, digest));
             CREATE INDEX IF NOT EXISTS revision_vectors_content ON revision_vectors(model, digest);",
        )?;
        Ok(Self {
            connection,
            model: model.into(),
        })
    }

    pub(super) fn get(&self, digest: &str) -> Option<Vec<f32>> {
        let bytes: Vec<u8> = self
            .connection
            .prepare_cached("SELECT vector FROM vectors WHERE model = ?1 AND digest = ?2")
            .ok()?
            .query_row(params![self.model, digest], |row| row.get(0))
            .optional()
            .ok()??;
        if bytes.len() != 128 * 4 {
            return None;
        }
        let vector: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four bytes")))
            .collect();
        vector
            .iter()
            .all(|value| value.is_finite())
            .then_some(vector)
    }

    /// One short cache transaction after inference, never around CPU work.
    pub(super) fn remember(&mut self, rows: &[(&str, &str, &[f32])]) -> KnowledgeResult<()> {
        let tx = self.connection.transaction()?;
        let mut insert_vector = tx.prepare_cached(
            "INSERT INTO vectors VALUES (?1, ?2, ?3)
             ON CONFLICT(model, digest) DO UPDATE SET vector=excluded.vector
             WHERE vector != excluded.vector",
        )?;
        let mut insert_reference =
            tx.prepare_cached("INSERT OR IGNORE INTO revision_vectors VALUES (?1, ?2, ?3)")?;
        for (revision, digest, vector) in rows {
            if vector.len() != 128 || !vector.iter().all(|value| value.is_finite()) {
                continue;
            }
            let bytes: Vec<u8> = vector
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            insert_vector.execute(params![self.model, digest, bytes])?;
            insert_reference.execute(params![revision, self.model, digest])?;
        }
        drop(insert_vector);
        drop(insert_reference);
        tx.commit()?;
        Ok(())
    }

    /// Call with all currently visible revisions for this cache's Loupist, even
    /// when the query was filtered. Concurrent older readers can only cause a
    /// later cache miss, never make an obsolete revision visible to a search.
    pub(super) fn prune(&mut self, current: &BTreeSet<String>) -> KnowledgeResult<()> {
        self.prune_revisions(current, true)
    }

    /// An empty or lexical-only search must also retire obsolete references,
    /// without loading a model or creating a cache that did not already exist.
    pub(super) fn prune_existing(path: &Path, current: &BTreeSet<String>) -> KnowledgeResult<()> {
        if !path.try_exists()? {
            return Ok(());
        }
        Self::open(path, "")?.prune_revisions(current, false)
    }

    fn prune_revisions(
        &mut self,
        current: &BTreeSet<String>,
        prune_models: bool,
    ) -> KnowledgeResult<()> {
        let tx = self.connection.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS current_revisions (id TEXT PRIMARY KEY);
                          DELETE FROM current_revisions;",
        )?;
        for revision in current {
            tx.execute("INSERT INTO current_revisions VALUES (?1)", [revision])?;
        }
        tx.execute(
            "DELETE FROM revision_vectors WHERE (?2 AND model != ?1) OR
                    NOT EXISTS (SELECT 1 FROM current_revisions WHERE id = revision)",
            params![self.model, prune_models],
        )?;
        let removed = tx.execute("DELETE FROM vectors WHERE NOT EXISTS (
            SELECT 1 FROM revision_vectors r WHERE r.model = vectors.model AND r.digest = vectors.digest)", [])?;
        tx.commit()?;
        if removed > 0 {
            self.connection
                .execute_batch("PRAGMA incremental_vacuum(256)")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_revisions_reuse_unchanged_text_and_collect_unreferenced_vectors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vectors.sqlite3");
        let mut cache = EmbeddingCache::open(&path, "model-a").unwrap();
        let vector = vec![0.25; 128];
        cache
            .remember(&[
                ("old", "unchanged", &vector),
                ("old", "removed", &vector),
                ("other-current", "not-in-this-query", &vector),
            ])
            .unwrap();
        assert_eq!(cache.get("unchanged"), Some(vector.clone()));
        cache
            .remember(&[("new", "unchanged", &vector), ("new", "added", &vector)])
            .unwrap();
        cache
            .prune(&BTreeSet::from(["new".into(), "other-current".into()]))
            .unwrap();
        assert_eq!(cache.get("unchanged"), Some(vector.clone()));
        assert_eq!(cache.get("added"), Some(vector.clone()));
        assert_eq!(cache.get("not-in-this-query"), Some(vector.clone()));
        assert_eq!(cache.get("removed"), None);
        assert_eq!(
            cache
                .connection
                .query_row(
                    "SELECT count(*) FROM revision_vectors WHERE revision='old'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        cache.prune(&BTreeSet::new()).unwrap();
        assert!(cache.get("unchanged").is_none());
    }

    #[test]
    fn model_changes_and_invalid_vectors_are_cache_misses() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vectors.sqlite3");
        let mut old = EmbeddingCache::open(&path, "old-model").unwrap();
        old.remember(&[("current", "text", &[0.5; 128])]).unwrap();
        let mut new = EmbeddingCache::open(&path, "new-model").unwrap();
        assert_eq!(new.get("text"), None);
        new.remember(&[("current", "text", &[0.25; 128])]).unwrap();
        assert_eq!(new.get("text"), Some(vec![0.25; 128]));
        new.prune(&BTreeSet::from(["current".into()])).unwrap();
        assert_eq!(old.get("text"), None);
        new.connection
            .execute("UPDATE vectors SET vector=x'0102'", [])
            .unwrap();
        assert_eq!(new.get("text"), None);
        new.remember(&[("current", "text", &[0.25; 128])]).unwrap();
        let non_finite: Vec<_> = [f32::NAN; 128]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        new.connection
            .execute("UPDATE vectors SET vector=?1", [non_finite])
            .unwrap();
        assert_eq!(new.get("text"), None);
    }
}
