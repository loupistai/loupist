//! Sharing rows in each Loupist's own database: the share identity, packs
//! added, their items and the World subjects they introduced. Every table
//! carries `loupist_id`, so folder admission checks single ownership.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::pack::{ItemOrigin, License, OriginEntry};
use crate::knowledge::shared_pack::ImportRefs;

const DDL: &str = "
CREATE TABLE IF NOT EXISTS share_identity (
    loupist_id TEXT PRIMARY KEY NOT NULL,
    pack_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 1),
    content_hash TEXT,
    created_at TEXT NOT NULL,
    byline TEXT,
    repo TEXT,
    repo_url TEXT,
    repo_commit TEXT
) STRICT;
CREATE TABLE IF NOT EXISTS pack_imports (
    import_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    pack_id TEXT NOT NULL,
    name TEXT NOT NULL,
    author TEXT,
    version INTEGER NOT NULL,
    license TEXT NOT NULL,
    source_url TEXT,
    repo TEXT,
    path TEXT,
    commit_sha TEXT,
    mode TEXT NOT NULL CHECK (mode IN ('new', 'merge')),
    added_at_ms INTEGER NOT NULL,
    blocked_reason TEXT,
    removed_at_ms INTEGER,
    origin_json TEXT NOT NULL DEFAULT '[]',
    checked_at_ms INTEGER
) STRICT;
CREATE TABLE IF NOT EXISTS pack_items (
    import_id TEXT NOT NULL REFERENCES pack_imports(import_id),
    loupist_id TEXT NOT NULL,
    key TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('perspective', 'method', 'principle')),
    title TEXT NOT NULL,
    body_json TEXT NOT NULL,
    body_hash TEXT NOT NULL,
    flags TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'accepted', 'skipped', 'removed')),
    lens_record_id TEXT,
    update_json TEXT,
    PRIMARY KEY (import_id, key)
) STRICT;
CREATE TABLE IF NOT EXISTS pack_world_items (
    import_id TEXT NOT NULL REFERENCES pack_imports(import_id),
    loupist_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('node', 'claim', 'source', 'span')),
    subject_id TEXT NOT NULL,
    PRIMARY KEY (import_id, kind, subject_id)
) STRICT;
";

pub(crate) fn ensure(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(DDL)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub pack_id: String,
    pub version: u32,
    pub content_hash: Option<String>,
    pub created_at: String,
    pub byline: Option<String>,
    pub repo: Option<String>,
    pub repo_url: Option<String>,
    /// The commit the last publish made on `repo` (a Gallery listing pins it).
    pub repo_commit: Option<String>,
}

pub(crate) fn identity(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> rusqlite::Result<Option<Identity>> {
    ensure(tx)?;
    tx.query_row(
        "SELECT pack_id, version, content_hash, created_at, byline, repo, repo_url, repo_commit FROM share_identity WHERE loupist_id = ?1",
        [loupist_id],
        |row| {
            Ok(Identity {
                pack_id: row.get(0)?,
                version: row.get(1)?,
                content_hash: row.get(2)?,
                created_at: row.get(3)?,
                byline: row.get(4)?,
                repo: row.get(5)?,
                repo_url: row.get(6)?,
                repo_commit: row.get(7)?,
            })
        },
    )
    .optional()
}

pub(crate) fn save_identity(
    tx: &Transaction<'_>,
    loupist_id: &str,
    identity: &Identity,
) -> rusqlite::Result<()> {
    ensure(tx)?;
    tx.execute(
        "INSERT INTO share_identity (loupist_id, pack_id, version, content_hash, created_at, byline, repo, repo_url, repo_commit)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(loupist_id) DO UPDATE SET pack_id = excluded.pack_id, version = excluded.version,
           content_hash = excluded.content_hash, created_at = excluded.created_at, byline = excluded.byline,
           repo = excluded.repo, repo_url = excluded.repo_url, repo_commit = excluded.repo_commit",
        params![
            loupist_id,
            identity.pack_id,
            identity.version,
            identity.content_hash,
            identity.created_at,
            identity.byline,
            identity.repo,
            identity.repo_url,
            identity.repo_commit
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ImportMode {
    New,
    Merge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportRow {
    pub import_id: String,
    pub pack_id: String,
    pub name: String,
    pub author: Option<String>,
    pub version: u32,
    pub license: License,
    pub source_url: Option<String>,
    pub repo: Option<String>,
    pub path: Option<String>,
    pub commit: Option<String>,
    pub mode: ImportMode,
    pub added_at_ms: i64,
    pub blocked_reason: Option<String>,
    /// The pack's own ancestor chain (its manifest `origin`), nearest first.
    pub origin: Vec<OriginEntry>,
    /// When "Check for updates" last ran for it.
    pub checked_at_ms: Option<i64>,
}

pub(crate) fn insert_import(
    tx: &Transaction<'_>,
    loupist_id: &str,
    row: &ImportRow,
) -> rusqlite::Result<()> {
    ensure(tx)?;
    tx.execute(
        "INSERT INTO pack_imports (import_id, loupist_id, pack_id, name, author, version, license,
            source_url, repo, path, commit_sha, mode, added_at_ms, blocked_reason, origin_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            row.import_id,
            loupist_id,
            row.pack_id,
            row.name,
            row.author,
            row.version,
            row.license.id(),
            row.source_url,
            row.repo,
            row.path,
            row.commit,
            if row.mode == ImportMode::New {
                "new"
            } else {
                "merge"
            },
            row.added_at_ms,
            row.blocked_reason,
            origin_json(&row.origin)
        ],
    )?;
    Ok(())
}

fn origin_json(origin: &[OriginEntry]) -> String {
    serde_json::to_string(origin).unwrap_or_else(|_| "[]".into())
}

fn license_from(id: &str) -> License {
    // Unknown stored rights never become a permissive license.
    License::from_id(id).unwrap_or(License::Personal)
}

/// Packs this Loupist added and has not removed, newest first.
pub(crate) fn imports(tx: &Transaction<'_>, loupist_id: &str) -> rusqlite::Result<Vec<ImportRow>> {
    ensure(tx)?;
    let mut statement = tx.prepare(
        "SELECT import_id, pack_id, name, author, version, license, source_url, repo, path, commit_sha,
                mode, added_at_ms, blocked_reason, origin_json, checked_at_ms
         FROM pack_imports WHERE loupist_id = ?1 AND removed_at_ms IS NULL
         ORDER BY added_at_ms DESC, import_id",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok(ImportRow {
            import_id: row.get(0)?,
            pack_id: row.get(1)?,
            name: row.get(2)?,
            author: row.get(3)?,
            version: row.get(4)?,
            license: license_from(&row.get::<_, String>(5)?),
            source_url: row.get(6)?,
            repo: row.get(7)?,
            path: row.get(8)?,
            commit: row.get(9)?,
            mode: if row.get::<_, String>(10)? == "new" {
                ImportMode::New
            } else {
                ImportMode::Merge
            },
            added_at_ms: row.get(11)?,
            blocked_reason: row.get(12)?,
            origin: serde_json::from_str(&row.get::<_, String>(13)?).unwrap_or_default(),
            checked_at_ms: row.get(14)?,
        })
    })?;
    rows.collect()
}

/// The pack this Loupist was started from ("A new Loupist"), while it is not removed.
pub(crate) fn fork_parent(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> rusqlite::Result<Option<ImportRow>> {
    Ok(imports(tx, loupist_id)?
        .into_iter()
        .filter(|row| row.mode == ImportMode::New)
        .min_by_key(|row| row.added_at_ms))
}

pub(crate) fn set_checked(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    at_ms: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_imports SET checked_at_ms = ?3 WHERE loupist_id = ?1 AND import_id = ?2",
        params![loupist_id, import_id, at_ms],
    )?;
    Ok(())
}

/// The community index's report-and-remove verdict, as last read.
pub(crate) fn set_blocked(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    reason: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_imports SET blocked_reason = ?3 WHERE loupist_id = ?1 AND import_id = ?2",
        params![loupist_id, import_id, reason],
    )?;
    Ok(())
}

/// A newer GitHub commit that carries the same version: the next check starts from it.
pub(crate) fn set_commit(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    commit: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_imports SET commit_sha = ?3 WHERE loupist_id = ?1 AND import_id = ?2",
        params![loupist_id, import_id, commit],
    )?;
    Ok(())
}

/// After an applied update the import stands at the newer version.
pub(crate) fn advance_import(
    tx: &Transaction<'_>,
    loupist_id: &str,
    row: &ImportRow,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_imports SET name = ?3, author = ?4, version = ?5, license = ?6, commit_sha = ?7,
            origin_json = ?8
         WHERE loupist_id = ?1 AND import_id = ?2",
        params![
            loupist_id,
            row.import_id,
            row.name,
            row.author,
            row.version,
            row.license.id(),
            row.commit,
            origin_json(&row.origin)
        ],
    )?;
    Ok(())
}

pub(crate) fn import(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
) -> rusqlite::Result<Option<ImportRow>> {
    Ok(imports(tx, loupist_id)?
        .into_iter()
        .find(|row| row.import_id == import_id))
}

pub(crate) fn mark_removed(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    at_ms: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_imports SET removed_at_ms = ?3 WHERE loupist_id = ?1 AND import_id = ?2",
        params![loupist_id, import_id, at_ms],
    )?;
    tx.execute(
        "UPDATE pack_items SET state = 'removed' WHERE loupist_id = ?1 AND import_id = ?2 AND state = 'pending'",
        params![loupist_id, import_id],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemRow {
    pub import_id: String,
    pub key: String,
    pub kind: String,
    pub title: String,
    pub body_json: String,
    pub body_hash: String,
    pub flags: String,
    pub state: String,
    pub lens_record_id: Option<String>,
    /// An accepted item's newer text from the author, waiting to be accepted
    /// on its own (it carried a scan finding).
    pub update_json: Option<String>,
}

pub(crate) fn insert_item(
    tx: &Transaction<'_>,
    loupist_id: &str,
    item: &ItemRow,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO pack_items (import_id, loupist_id, key, kind, title, body_json, body_hash, flags, state, lens_record_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            item.import_id,
            loupist_id,
            item.key,
            item.kind,
            item.title,
            item.body_json,
            item.body_hash,
            item.flags,
            item.state,
            item.lens_record_id
        ],
    )?;
    Ok(())
}

pub(crate) fn items(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
) -> rusqlite::Result<Vec<ItemRow>> {
    ensure(tx)?;
    let mut statement = tx.prepare(
        "SELECT import_id, key, kind, title, body_json, body_hash, flags, state, lens_record_id, update_json
         FROM pack_items WHERE loupist_id = ?1 AND import_id = ?2
         ORDER BY CASE kind WHEN 'perspective' THEN 0 WHEN 'method' THEN 1 ELSE 2 END, title, key",
    )?;
    let rows = statement.query_map(params![loupist_id, import_id], |row| {
        Ok(ItemRow {
            import_id: row.get(0)?,
            key: row.get(1)?,
            kind: row.get(2)?,
            title: row.get(3)?,
            body_json: row.get(4)?,
            body_hash: row.get(5)?,
            flags: row.get(6)?,
            state: row.get(7)?,
            lens_record_id: row.get(8)?,
            update_json: row.get(9)?,
        })
    })?;
    rows.collect()
}

/// Replaces an item's text with the author's newer one (its state stays).
pub(crate) fn set_item_body(
    tx: &Transaction<'_>,
    loupist_id: &str,
    item: &ItemRow,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_items SET title = ?4, body_json = ?5, body_hash = ?6, flags = ?7, state = ?8,
            update_json = ?9
         WHERE loupist_id = ?1 AND import_id = ?2 AND key = ?3",
        params![
            loupist_id,
            item.import_id,
            item.key,
            item.title,
            item.body_json,
            item.body_hash,
            item.flags,
            item.state,
            item.update_json
        ],
    )?;
    Ok(())
}

pub(crate) fn set_item_state(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    key: &str,
    state: &str,
    lens_record_id: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE pack_items SET state = ?4, lens_record_id = COALESCE(?5, lens_record_id)
         WHERE loupist_id = ?1 AND import_id = ?2 AND key = ?3",
        params![loupist_id, import_id, key, state, lens_record_id],
    )?;
    Ok(())
}

pub(crate) fn insert_world_ref(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    kind: &str,
    subject_id: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO pack_world_items (import_id, loupist_id, kind, subject_id) VALUES (?1, ?2, ?3, ?4)",
        params![import_id, loupist_id, kind, subject_id],
    )?;
    Ok(())
}

pub(crate) fn world_refs(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
) -> rusqlite::Result<ImportRefs> {
    ensure(tx)?;
    let mut refs = ImportRefs::default();
    let mut statement = tx.prepare(
        "SELECT kind, subject_id FROM pack_world_items WHERE loupist_id = ?1 AND import_id = ?2",
    )?;
    let rows = statement.query_map(params![loupist_id, import_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, subject) = row?;
        match kind.as_str() {
            "node" => {
                refs.nodes.insert(subject);
            }
            "claim" => {
                refs.claims.insert(subject);
            }
            "span" => {
                refs.spans.insert(subject);
            }
            "source" => {
                refs.sources.insert(subject);
            }
            _ => {}
        }
    }
    Ok(refs)
}

fn tables_exist(tx: &Transaction<'_>) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'pack_world_items')",
        [],
        |row| row.get(0),
    )
}

/// Reference Sources of packs the Loupist removed. Source records are
/// immutable, so Remove hides them: the Library, search and reads skip them.
pub(crate) fn removed_sources(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> rusqlite::Result<BTreeSet<String>> {
    if !tables_exist(tx)? {
        return Ok(BTreeSet::new());
    }
    let mut statement = tx.prepare(
        "SELECT w.subject_id FROM pack_world_items w
         JOIN pack_imports i ON i.import_id = w.import_id
         WHERE w.loupist_id = ?1 AND w.kind = 'source' AND i.removed_at_ms IS NOT NULL",
    )?;
    let rows = statement.query_map([loupist_id], |row| row.get::<_, String>(0))?;
    rows.collect()
}

/// The pack a Lens record or a Map name came from, if the user added it from one.
pub(crate) fn origin_of(
    tx: &Transaction<'_>,
    loupist_id: &str,
    subject_id: &str,
) -> rusqlite::Result<Option<(String, String, Option<String>)>> {
    ensure(tx)?;
    tx.query_row(
        "SELECT i.import_id, i.name, i.author FROM pack_imports i
         WHERE i.loupist_id = ?1 AND i.removed_at_ms IS NULL AND (
            EXISTS (SELECT 1 FROM pack_items p WHERE p.import_id = i.import_id AND p.state = 'accepted' AND p.lens_record_id = ?2)
            OR EXISTS (SELECT 1 FROM pack_world_items w WHERE w.import_id = i.import_id AND w.kind IN ('node', 'claim') AND w.subject_id = ?2))
         ORDER BY i.added_at_ms LIMIT 1",
        params![loupist_id, subject_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
}

/// Accepted shared Lens items: record id → (pack name, accepted despite a
/// scan finding). Accepted items keep their origin after the pack is removed
/// (Remove may archive them).
pub(crate) fn shared_lens(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> rusqlite::Result<BTreeMap<String, (String, bool)>> {
    ensure(tx)?;
    let mut statement = tx.prepare(
        "SELECT p.lens_record_id, i.name, p.flags FROM pack_items p
         JOIN pack_imports i ON i.import_id = p.import_id
         WHERE p.loupist_id = ?1 AND p.state = 'accepted' AND p.lens_record_id IS NOT NULL",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get::<_, String>(1)?, row.get::<_, String>(2)? != "[]"),
        ))
    })?;
    rows.collect()
}

/// Map subjects (names and relationships), reference Sources and their spans
/// that packs brought, each → pack name. Removed packs count too: a name the
/// Loupist's own relationships still use keeps the pack's text.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedWorld {
    pub subjects: BTreeMap<String, String>,
    pub sources: BTreeMap<String, String>,
    pub spans: BTreeMap<String, String>,
}

pub(crate) fn shared_world(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> rusqlite::Result<SharedWorld> {
    ensure(tx)?;
    let mut statement = tx.prepare(
        "SELECT w.kind, w.subject_id, i.name FROM pack_world_items w
         JOIN pack_imports i ON i.import_id = w.import_id
         WHERE w.loupist_id = ?1",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut world = SharedWorld::default();
    for row in rows {
        let (kind, subject, name) = row?;
        let map = match kind.as_str() {
            "node" | "claim" => &mut world.subjects,
            "source" => &mut world.sources,
            _ => &mut world.spans,
        };
        map.insert(subject, name);
    }
    Ok(world)
}

/// Origins are kept in existing import/item JSON, including every upstream credit.
/// Removing an import does not erase the rights in records the user kept.
pub(crate) fn shared_licenses(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> rusqlite::Result<(BTreeMap<String, ItemOrigin>, BTreeMap<String, ItemOrigin>)> {
    ensure(tx)?;
    let mut lens = BTreeMap::new();
    let mut statement = tx.prepare(
        "SELECT p.lens_record_id, i.pack_id, i.name, i.author, i.license, i.version,
                i.source_url, i.origin_json, p.body_json FROM pack_items p
         JOIN pack_imports i ON i.import_id = p.import_id
         WHERE p.loupist_id = ?1 AND p.state = 'accepted' AND p.lens_record_id IS NOT NULL",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        let mut origin = stored_origin(row)?;
        let body: String = row.get(8)?;
        let item = serde_json::from_str::<super::import::StoredItem>(&body).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        if let Some(upstream) = item.origin {
            origin.chain.extend(upstream.entries());
        }
        dedup_origins(&mut origin.chain);
        Ok((row.get::<_, String>(0)?, origin))
    })?;
    for row in rows {
        let (record, origin) = row?;
        lens.insert(record, origin);
    }
    drop(statement);
    let mut world: BTreeMap<String, ItemOrigin> = BTreeMap::new();
    let mut statement = tx.prepare(
        "SELECT w.subject_id, i.pack_id, i.name, i.author, i.license, i.version,
                i.source_url, i.origin_json, w.kind FROM pack_world_items w
         JOIN pack_imports i ON i.import_id = w.import_id
         WHERE w.loupist_id = ?1 AND w.kind IN ('source', 'node', 'claim')",
    )?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok((
            format!("{}:{}", row.get::<_, String>(8)?, row.get::<_, String>(0)?),
            stored_origin(row)?,
        ))
    })?;
    for row in rows {
        let (record, origin) = row?;
        if let Some(existing) = world.get_mut(&record) {
            existing.chain.extend(origin.entries());
            dedup_origins(&mut existing.chain);
        } else {
            world.insert(record, origin);
        }
    }
    // A create-new import supplies the profile focus too. Keep its rights even
    // after Remove, which deliberately leaves the receiver's profile in place.
    if let Some(origin) = tx
        .query_row(
            "SELECT import_id, pack_id, name, author, license, version, source_url, origin_json
         FROM pack_imports WHERE loupist_id = ?1 AND mode = 'new'
         ORDER BY added_at_ms, import_id LIMIT 1",
            [loupist_id],
            stored_origin,
        )
        .optional()?
    {
        world.insert("profile".into(), origin);
    }
    Ok((lens, world))
}

fn stored_origin(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemOrigin> {
    let raw: String = row.get(7)?;
    let chain = serde_json::from_str(&raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ItemOrigin {
        pack_id: row.get(1)?,
        name: row.get(2)?,
        author: row.get(3)?,
        license: license_from(&row.get::<_, String>(4)?),
        version: row.get(5)?,
        url: row.get(6)?,
        chain,
    })
}

pub(crate) fn dedup_origins(entries: &mut Vec<OriginEntry>) {
    let mut seen = BTreeSet::new();
    entries.retain(|entry| seen.insert((entry.id.clone(), entry.version, entry.license)));
}

pub(crate) fn manifest_origins(manifest: &super::pack::Manifest) -> Vec<OriginEntry> {
    let mut entries = manifest.origin.clone();
    entries.extend(manifest.credits.clone());
    dedup_origins(&mut entries);
    entries
}

/// Additive sharing-owned storage, without changing the database schema version.
/// Each acceptance is tied to a share identity and the exact listed commit.
pub(crate) fn record_listing_acceptance(
    tx: &Transaction<'_>,
    loupist_id: &str,
    pack_id: &str,
    commit: &str,
    terms_url: &str,
    accepted_at: &str,
) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS share_listing_acceptances (
        loupist_id TEXT NOT NULL REFERENCES share_identity(loupist_id), pack_id TEXT NOT NULL,
        commit_sha TEXT NOT NULL, terms_url TEXT NOT NULL, accepted_at TEXT NOT NULL,
        PRIMARY KEY (loupist_id, pack_id, commit_sha, terms_url, accepted_at)
    ) STRICT;",
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO share_listing_acceptances VALUES (?1, ?2, ?3, ?4, ?5)",
        params![loupist_id, pack_id, commit, terms_url, accepted_at],
    )?;
    Ok(())
}
