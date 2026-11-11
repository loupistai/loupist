//! Ordered N→N+1 upgrades for Loupist databases.
//!
//! The global catalog and every folder's `loupist.sqlite3` share one schema
//! (`SqliteStore::initialize_schema`), so they share these ladders: the store
//! schema (`PRAGMA user_version`) and the Knowledge schema
//! (`knowledge_schema.version`). A fresh database is created at the current
//! versions; a database at a ladder version is upgraded one step at a time, each
//! step in its own transaction, after a private `VACUUM INTO` backup of the
//! file. Versions older than a baseline are development formats from before
//! the ladder and stay refused (`EarlierVersion`); a newer version is refused
//! (`NewerVersion`) and never touched.
//!
//! Steps are frozen: each names exactly what it changes in the shape its `from`
//! version had. Fresh DDL always describes the current shape, and
//! `store/tests/upgrade.rs` asserts that the baseline fixture upgraded through
//! every step matches a fresh database.
//!
//! Lazily created tables (`CREATE TABLE IF NOT EXISTS` in their owning module,
//! such as sharing, provider status or artifact history) are outside the
//! ladder: a database may or may not contain them, and their module creates
//! them on first use at their current shape. Their DDL must never change shape
//! in place. A shape change is a ladder step that alters the table only when it
//! exists, together with the module's DDL for databases that never created it.
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::schema::STORE_SCHEMA_VERSION;
use super::{StoreError, StoreResult};
use crate::fs::artifact_trust::{
    ensure_private_regular_file, harden_private_regular_file_if_exists,
};
use crate::knowledge::KNOWLEDGE_SCHEMA_VERSION;

/// The oldest store schema the ladder upgrades (the format 0.0.1 installs use).
pub(crate) const STORE_BASELINE_VERSION: i64 = 4;
/// The oldest Knowledge schema the ladder upgrades.
pub(crate) const KNOWLEDGE_BASELINE_VERSION: i64 = 6;

type Step = fn(&Transaction<'_>) -> StoreResult<()>;

/// Store steps; index 0 upgrades `STORE_BASELINE_VERSION` by one.
const STORE_STEPS: &[Step] = &[store_4_to_5];
/// Knowledge steps; index 0 upgrades `KNOWLEDGE_BASELINE_VERSION` by one.
const KNOWLEDGE_STEPS: &[Step] = &[knowledge_6_to_7];

const _: () = assert!(STORE_BASELINE_VERSION + STORE_STEPS.len() as i64 == STORE_SCHEMA_VERSION);
const _: () =
    assert!(KNOWLEDGE_BASELINE_VERSION + KNOWLEDGE_STEPS.len() as i64 == KNOWLEDGE_SCHEMA_VERSION);

#[derive(Clone, Copy)]
pub(super) struct Ladder {
    name: &'static str,
    baseline: i64,
    steps: &'static [Step],
    read: fn(&Connection) -> StoreResult<Option<i64>>,
    write: fn(&Transaction<'_>, i64) -> StoreResult<()>,
}

impl Ladder {
    fn current(&self) -> i64 {
        self.baseline + self.steps.len() as i64
    }
    fn standing(&self, found: i64) -> Standing {
        if found > self.current() {
            Standing::Newer
        } else if found < self.baseline {
            Standing::Earlier
        } else if found < self.current() {
            Standing::Upgradable
        } else {
            Standing::Current
        }
    }
}

pub(super) const STORE: Ladder = Ladder {
    name: "store",
    baseline: STORE_BASELINE_VERSION,
    steps: STORE_STEPS,
    read: read_store_version,
    write: write_store_version,
};

pub(super) const KNOWLEDGE: Ladder = Ladder {
    name: "Knowledge",
    baseline: KNOWLEDGE_BASELINE_VERSION,
    steps: KNOWLEDGE_STEPS,
    read: read_knowledge_version,
    write: write_knowledge_version,
};

/// How one database's schema versions compare with this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Standing {
    Current,
    Upgradable,
    /// A development format from before the ladder: refused, never migrated.
    Earlier,
    /// Written by a newer Loupist: refused and left untouched.
    Newer,
}

/// The most restrictive standing of the database's versioned schemas. A store
/// version of 0 (an uninitialized file) and an absent Knowledge schema count
/// as current; their owners decide what an empty database means.
pub(crate) fn standing(connection: &Connection) -> StoreResult<Standing> {
    let mut result = Standing::Current;
    for ladder in [STORE, KNOWLEDGE] {
        if let Some(found) = (ladder.read)(connection)? {
            result = result.max(ladder.standing(found));
        }
    }
    Ok(result)
}

/// Brings a database opened by `SqliteStore` to the current versions. A file
/// gets a private backup next to it before the first step.
pub(super) fn upgrade(
    connection: &mut Connection,
    database_path: Option<&Path>,
) -> StoreResult<Option<PathBuf>> {
    upgrade_with(connection, database_path, &[STORE, KNOWLEDGE])
}

/// An in-memory, upgraded copy of a database that admission validates without
/// writing to the folder it came from.
pub(crate) fn upgraded_copy(source: &Connection) -> StoreResult<Connection> {
    let mut copy = Connection::open_in_memory()?;
    {
        let backup = rusqlite::backup::Backup::new(source, &mut copy)?;
        if backup.step(-1)? != rusqlite::backup::StepResult::Done {
            return Err(StoreError::Upgrade(
                "the database was busy while it was copied".into(),
            ));
        }
    }
    upgrade_with(&mut copy, None, &[STORE, KNOWLEDGE])?;
    Ok(copy)
}

pub(super) fn upgrade_with(
    connection: &mut Connection,
    database_path: Option<&Path>,
    ladders: &[Ladder],
) -> StoreResult<Option<PathBuf>> {
    let mut pending = false;
    for ladder in ladders {
        if let Some(found) = (ladder.read)(connection)? {
            match ladder.standing(found) {
                Standing::Newer => return Err(StoreError::NewerVersion),
                Standing::Upgradable => pending = true,
                // Owners refuse earlier formats where they matter (folders);
                // the catalog's unused Memory tables are left as they are.
                Standing::Earlier | Standing::Current => {}
            }
        }
    }
    if !pending {
        return Ok(None);
    }
    let backup = database_path
        .map(|path| write_backup(connection, path))
        .transpose()?;
    let foreign_keys: bool = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    // Step 1 of SQLite's ALTER procedure: foreign keys are off for rebuilds, and
    // this pragma has no effect inside a transaction.
    connection.execute_batch("PRAGMA foreign_keys = OFF")?;
    let result = run_steps(connection, ladders);
    connection.execute_batch(if foreign_keys {
        "PRAGMA foreign_keys = ON"
    } else {
        "PRAGMA foreign_keys = OFF"
    })?;
    result.map_err(|error| {
        StoreError::Upgrade(match &backup {
            Some(path) => format!(
                "{}; the database is unchanged by the failed step and a copy is at {}",
                detail(&error),
                path.display()
            ),
            None => detail(&error),
        })
    })?;
    // Only a committed upgrade prunes: a failed one keeps every backup.
    if let (Some(database_path), Some(backup)) = (database_path, &backup) {
        prune_backups(database_path, backup);
    }
    Ok(backup)
}

fn run_steps(connection: &mut Connection, ladders: &[Ladder]) -> StoreResult<()> {
    if connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))? {
        return Err(StoreError::Invalid(
            "foreign keys must be off while tables are rebuilt".into(),
        ));
    }
    for ladder in ladders {
        loop {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let Some(found) = (ladder.read)(&transaction)? else {
                break;
            };
            if ladder.standing(found) != Standing::Upgradable {
                break;
            }
            let step = ladder.steps[(found - ladder.baseline) as usize];
            let broken_before = foreign_key_violations(&transaction)?;
            step(&transaction).map_err(|error| {
                StoreError::Upgrade(format!(
                    "{} schema {found} could not be upgraded: {}",
                    ladder.name,
                    detail(&error)
                ))
            })?;
            // Step 10: the step breaks no reference. A violation the file
            // already had is left for the owner's own checks (folder admission
            // refuses broken references), not turned into a stuck upgrade.
            if !foreign_key_violations(&transaction)?.is_subset(&broken_before) {
                return Err(StoreError::Upgrade(format!(
                    "{} schema {found} upgrade would leave broken references",
                    ladder.name
                )));
            }
            (ladder.write)(&transaction, found + 1)?;
            transaction.commit()?;
        }
    }
    Ok(())
}

fn detail(error: &StoreError) -> String {
    match error {
        StoreError::Upgrade(message) => message.clone(),
        other => other.to_string(),
    }
}

type Violation = (String, Option<i64>, String, i64);

fn foreign_key_violations(
    connection: &Connection,
) -> StoreResult<std::collections::BTreeSet<Violation>> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

fn read_store_version(connection: &Connection) -> StoreResult<Option<i64>> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok((version != 0).then_some(version))
}

fn write_store_version(transaction: &Transaction<'_>, version: i64) -> StoreResult<()> {
    transaction.execute_batch(&format!("PRAGMA user_version = {version}"))?;
    Ok(())
}

fn read_knowledge_version(connection: &Connection) -> StoreResult<Option<i64>> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'knowledge_schema')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    Ok(connection
        .query_row(
            "SELECT version FROM knowledge_schema WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

fn write_knowledge_version(transaction: &Transaction<'_>, version: i64) -> StoreResult<()> {
    if transaction.execute(
        "UPDATE knowledge_schema SET version = ?1 WHERE id = 1",
        [version],
    )? != 1
    {
        return Err(StoreError::Invalid(
            "Knowledge schema row is missing".into(),
        ));
    }
    Ok(())
}

/// Between `<file>` and the UTC stamp in an upgrade backup's name.
const BACKUP_MARKER: &str = ".upgrade-backup-";
/// The stamp's format: fixed-width, so names sort in time order.
const BACKUP_STAMP_FORMAT: &str = "%Y%m%dT%H%M%SZ";
/// The highest `-<n>` suffix a same-second backup may take.
const BACKUP_MAX_ATTEMPT: u32 = 100;
/// Upgrade backups each database keeps after a successful upgrade, the new one
/// included.
pub(super) const KEPT_UPGRADE_BACKUPS: usize = 2;

/// `<file>.upgrade-backup-<UTC stamp>` beside the database, created private
/// before SQLite writes it and synced before any step changes the original.
fn write_backup(connection: &Connection, database_path: &Path) -> StoreResult<PathBuf> {
    let parent = database_path
        .parent()
        .ok_or_else(|| StoreError::Invalid("SQLite path has no parent".into()))?;
    let file_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StoreError::Invalid("SQLite path must be Unicode".into()))?;
    let stamp = chrono::Utc::now().format(BACKUP_STAMP_FORMAT);
    let mut attempt = 0;
    let backup = loop {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let candidate = parent.join(format!("{file_name}{BACKUP_MARKER}{stamp}{suffix}"));
        match std::fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break candidate,
            Err(error) => return Err(StoreError::Invalid(error.to_string())),
            Ok(_) if attempt < BACKUP_MAX_ATTEMPT => attempt += 1,
            Ok(_) => return Err(StoreError::Conflict("upgrade backup already exists")),
        }
    };
    // VACUUM INTO accepts an empty existing file; creating it here keeps the
    // copy owner-only from its first byte.
    ensure_private_regular_file(&backup)?;
    let target = backup
        .to_str()
        .ok_or_else(|| StoreError::Invalid("SQLite path must be Unicode".into()))?;
    if let Err(error) = connection.execute("VACUUM INTO ?1", [target]) {
        let _ = std::fs::remove_file(&backup);
        return Err(StoreError::Upgrade(format!(
            "the pre-upgrade backup could not be written: {error}"
        )));
    }
    ensure_private_regular_file(&backup)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        harden_private_regular_file_if_exists(&super::schema::sqlite_side_path(&backup, suffix))?;
    }
    std::fs::File::open(&backup)
        .and_then(|file| file.sync_all())
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    eprintln!(
        "Loupist backed up {} before upgrading it: {}",
        database_path.display(),
        backup.display()
    );
    Ok(backup)
}

/// Where `name` sorts among `file_name`'s upgrade backups, when it is exactly a
/// name `write_backup` gives: `<file_name>.upgrade-backup-<stamp>`, optionally
/// followed by `-<1..=BACKUP_MAX_ATTEMPT>` without leading zeros.
fn backup_order(file_name: &str, name: &str) -> Option<(String, u32)> {
    let rest = name.strip_prefix(file_name)?.strip_prefix(BACKUP_MARKER)?;
    let (stamp, attempt) = match rest.split_once('-') {
        Some((stamp, attempt)) => {
            let canonical = !attempt.is_empty()
                && !attempt.starts_with('0')
                && attempt.bytes().all(|byte| byte.is_ascii_digit());
            let attempt = attempt.parse::<u32>().ok().filter(|_| canonical)?;
            (stamp, (attempt <= BACKUP_MAX_ATTEMPT).then_some(attempt)?)
        }
        None => (rest, 0),
    };
    // `YYYYMMDDTHHMMSSZ`, as `BACKUP_STAMP_FORMAT` writes it.
    let bytes = stamp.as_bytes();
    let well_formed = bytes.len() == 16
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'Z';
    well_formed.then(|| (stamp.to_owned(), attempt))
}

/// After a successful upgrade, keeps `current` and the newest other upgrade
/// backups of `database_path` up to `KEPT_UPGRADE_BACKUPS`, and deletes the
/// older ones. Candidates are only regular files directly beside the database
/// whose names `backup_order` accepts; symlinks, directories and other names are
/// never touched. The upgrade has committed by now, so a failure is logged and
/// leaves the remaining backups in place.
fn prune_backups(database_path: &Path, current: &Path) {
    let (Some(parent), Some(file_name), Some(current_name)) = (
        current.parent(),
        database_path.file_name().and_then(|name| name.to_str()),
        current.file_name(),
    ) else {
        return;
    };
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!(
                "Loupist kept every upgrade backup of {}: {error}",
                database_path.display()
            );
            return;
        }
    };
    let mut older = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == current_name {
            continue;
        }
        let Some(order) = name.to_str().and_then(|name| backup_order(file_name, name)) else {
            continue;
        };
        // `DirEntry::file_type` describes the entry itself, not a link target.
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            older.push((order, entry.path()));
        }
    }
    older.sort_unstable_by(|left, right| right.0.cmp(&left.0));
    for (_, path) in older.into_iter().skip(KEPT_UPGRADE_BACKUPS - 1) {
        // Unlinking removes the directory entry and never follows a link.
        match std::fs::remove_file(&path) {
            Ok(()) => eprintln!(
                "Loupist removed an older upgrade backup: {}",
                path.display()
            ),
            Err(error) => eprintln!(
                "Loupist could not remove an older upgrade backup {}: {error}",
                path.display()
            ),
        }
    }
}

// --- Store 4 → 5 ----------------------------------------------------------

/// Extensible enumerations validated by Rust at their write sites instead of
/// SQL `CHECK (x IN (...))` lists, so adding a value needs no schema step.
/// Structural checks (booleans, non-empty, ranges, row invariants) stay.
const STORE_5_ENUMERATIONS: &[(&str, &[&str])] = &[
    ("user_wallet_addresses", &["family", "status"]),
    ("user_holdings_snapshots", &["source"]),
    ("app_settings", &["id"]),
    ("chat_artifacts", &["kind"]),
    ("chat_handoffs", &["status"]),
];

fn store_4_to_5(transaction: &Transaction<'_>) -> StoreResult<()> {
    for (table, columns) in STORE_5_ENUMERATIONS {
        rebuild_without_enumerations(transaction, table, columns)?;
    }
    restore_removed_session_cache_keys(transaction)?;
    add_application_keep_awake(transaction)
}

/// Keep-awake joined the lazily created `application_settings` table (catalog
/// only) while store 4 was current. A table created before it gains the
/// column, off; a database without the table is left for its module to create.
fn add_application_keep_awake(transaction: &Transaction<'_>) -> StoreResult<()> {
    let table = transaction
        .prepare(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'application_settings'",
        )?
        .exists([])?;
    let column = transaction
        .prepare(
            "SELECT 1 FROM pragma_table_info('application_settings') WHERE name = 'keep_awake'",
        )?
        .exists([])?;
    if table && !column {
        transaction.execute_batch(
            "ALTER TABLE application_settings
             ADD COLUMN keep_awake INTEGER NOT NULL DEFAULT 0 CHECK(keep_awake IN (0, 1));",
        )?;
    }
    Ok(())
}

/// Builds before store 5 cleared Pi session caches on reconnect and import with
/// `json_remove`, deleting the `piSessionCache` key that a Chat always writes.
/// Chat writes compare the stored text with the re-encoded Chat, so every later
/// write to such a Chat conflicted. Put the key back where serialization writes
/// it: right after `piSessionId` and before `title`. Every field before `title`
/// is a string, and quotes inside a JSON string are escaped, so the first
/// `,"title":` in the text is the top-level key. Frozen SQL, independent of
/// later Chat types.
fn restore_removed_session_cache_keys(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute(
        r#"UPDATE chat_sessions
           SET data_json = substr(data_json, 1, instr(data_json, ',"title":') - 1)
               || ',"piSessionCache":null'
               || substr(data_json, instr(data_json, ',"title":'))
           WHERE json_type(data_json, '$.piSessionCache') IS NULL
             AND json_type(data_json, '$.piSessionId') = 'text'
             AND instr(data_json, ',"title":') > 0"#,
        [],
    )?;
    Ok(())
}

// --- Knowledge 6 → 7 ------------------------------------------------------

/// As `STORE_5_ENUMERATIONS`. The immutable provenance and revision tables
/// (`knowledge_records`, `knowledge_source_captures`,
/// `knowledge_source_representations`, `knowledge_read_receipts`) keep their
/// lists: every Memory reference and retained byte rests on them, their kinds
/// are part of `KNOWLEDGE_FORMAT_VERSION`, and a new value there is a format
/// change rather than a growing list. The append-only growth and Lens logs
/// are rebuilt with their immutability triggers restored in the same step.
const KNOWLEDGE_7_ENUMERATIONS: &[(&str, &[&str])] = &[
    ("knowledge_growth_events", &["kind", "actor"]),
    ("knowledge_learning_reviews", &["status"]),
    ("knowledge_schema_changes", &["producer"]),
    ("knowledge_lens_metadata", &["kind"]),
    ("knowledge_lens_log", &["origin"]),
    ("knowledge_lens_anchors", &["role"]),
    ("knowledge_connections", &["status"]),
    ("knowledge_search_documents", &["kind", "coverage"]),
    (
        "knowledge_asset_pages",
        &["class", "class_source", "verdict"],
    ),
    ("knowledge_asset_observations", &["kind", "origin"]),
    ("knowledge_asset_moves", &["actor"]),
    ("knowledge_earnings_events", &["date_source", "status"]),
    (
        "knowledge_earnings_rows",
        &["side", "unit", "kind", "stage", "origin"],
    ),
    ("knowledge_earnings_notices", &["outcome", "side"]),
    ("knowledge_watch_settings", &["kind"]),
    ("knowledge_watch_routes", &["route", "status"]),
    ("knowledge_watch_events", &["kind", "delivered_in"]),
];

fn knowledge_6_to_7(transaction: &Transaction<'_>) -> StoreResult<()> {
    for (table, columns) in KNOWLEDGE_7_ENUMERATIONS {
        rebuild_without_enumerations(transaction, table, columns)?;
    }
    Ok(())
}

// --- Table rebuilds -------------------------------------------------------

fn shape_error(table: &str, problem: impl std::fmt::Display) -> StoreError {
    StoreError::Upgrade(format!("table {table} {problem}"))
}

/// Rebuilds `table` without the `CHECK (column IN (...))` (or
/// `CHECK (column IS NULL OR column IN (...))`) constraint of each column.
fn rebuild_without_enumerations(
    transaction: &Transaction<'_>,
    table: &str,
    columns: &[&str],
) -> StoreResult<()> {
    let sql: String = transaction
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| shape_error(table, "is missing"))?;
    let mut rebuilt = sql;
    for column in columns {
        rebuilt = drop_enumeration_check(&rebuilt, column).ok_or_else(|| {
            shape_error(
                table,
                format!("has no single enumeration check on {column}"),
            )
        })?;
    }
    rebuild_table(transaction, table, &rebuilt)
}

/// Steps 3–8 of SQLite's twelve-step ALTER TABLE procedure
/// (<https://sqlite.org/lang_altertable.html#otheralter>). The ladder owns
/// steps 1–2 and 10–12: foreign keys off, the transaction, the foreign key
/// check and the commit. `create_sql` is the table's new `CREATE TABLE`.
fn rebuild_table(transaction: &Transaction<'_>, table: &str, create_sql: &str) -> StoreResult<()> {
    let staging = format!("{table}__upgrade");
    let staging_sql = rename_created_table(create_sql, table, &staging)
        .ok_or_else(|| shape_error(table, "has an unexpected CREATE TABLE statement"))?;
    // Step 3: remember the indexes and triggers; this schema has no views.
    let views: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'view'",
        [],
        |row| row.get(0),
    )?;
    if views != 0 {
        return Err(shape_error(table, "cannot be rebuilt while views exist"));
    }
    let dependents = {
        let mut statement = transaction.prepare(
            "SELECT sql FROM sqlite_schema
             WHERE tbl_name = ?1 AND type IN ('index', 'trigger') AND sql IS NOT NULL
             ORDER BY rowid",
        )?;
        let rows = statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let columns = table_columns(transaction, table)?;
    let rows_before: i64 = transaction.query_row(
        &format!("SELECT COUNT(*) FROM {}", quote(table)),
        [],
        |row| row.get(0),
    )?;
    // Step 4.
    transaction.execute_batch(&staging_sql)?;
    if table_columns(transaction, &staging)? != columns {
        return Err(shape_error(
            table,
            "changed its columns while it was rebuilt",
        ));
    }
    // Step 5. Rowids are kept: some reads order by them.
    let names = columns
        .iter()
        .map(|column| quote(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let keep_rowid = if columns.iter().any(|column| column.rowid_alias) {
        ""
    } else {
        "rowid, "
    };
    transaction.execute(
        &format!(
            "INSERT INTO {} ({keep_rowid}{names}) SELECT {keep_rowid}{names} FROM {}",
            quote(&staging),
            quote(table)
        ),
        [],
    )?;
    // Steps 6–7.
    transaction.execute_batch(&format!(
        "DROP TABLE {}; ALTER TABLE {} RENAME TO {};",
        quote(table),
        quote(&staging),
        quote(table)
    ))?;
    // Step 8.
    for sql in dependents {
        transaction.execute_batch(&sql)?;
    }
    let rows_after: i64 = transaction.query_row(
        &format!("SELECT COUNT(*) FROM {}", quote(table)),
        [],
        |row| row.get(0),
    )?;
    if rows_after != rows_before {
        return Err(shape_error(table, "lost rows while it was rebuilt"));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Column {
    name: String,
    declared_type: String,
    not_null: bool,
    default: Option<String>,
    primary_key: i64,
    rowid_alias: bool,
}

fn table_columns(connection: &Connection, table: &str) -> StoreResult<Vec<Column>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({})", quote(table)))?;
    let mut columns = statement
        .query_map([], |row| {
            Ok(Column {
                name: row.get(1)?,
                declared_type: row.get(2)?,
                not_null: row.get(3)?,
                default: row.get(4)?,
                primary_key: row.get(5)?,
                rowid_alias: false,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let keys = columns
        .iter()
        .filter(|column| column.primary_key > 0)
        .count();
    for column in &mut columns {
        column.rowid_alias = keys == 1
            && column.primary_key == 1
            && column.declared_type.eq_ignore_ascii_case("INTEGER");
    }
    Ok(columns)
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Byte ranges of SQL text outside string literals, quoted identifiers and
/// comments, as `(start, end, is_code)` segments covering the whole input.
fn segments(sql: &str) -> Vec<(usize, usize, bool)> {
    let bytes = sql.as_bytes();
    let mut result = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let end = match bytes[index] {
            quote @ (b'\'' | b'"' | b'`') => {
                let mut cursor = index + 1;
                loop {
                    match bytes.get(cursor) {
                        None => break bytes.len(),
                        Some(&byte) if byte == quote => {
                            if bytes.get(cursor + 1) == Some(&quote) {
                                cursor += 2;
                            } else {
                                break cursor + 1;
                            }
                        }
                        Some(_) => cursor += 1,
                    }
                }
            }
            b'[' => sql[index..]
                .find(']')
                .map_or(bytes.len(), |offset| index + offset + 1),
            b'-' if bytes.get(index + 1) == Some(&b'-') => sql[index..]
                .find('\n')
                .map_or(bytes.len(), |offset| index + offset),
            b'/' if bytes.get(index + 1) == Some(&b'*') => sql[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2),
            _ => {
                index += 1;
                continue;
            }
        };
        if start < index {
            result.push((start, index, true));
        }
        result.push((index, end, false));
        start = end;
        index = end;
    }
    if start < bytes.len() {
        result.push((start, bytes.len(), true));
    }
    result
}

/// Whitespace-, case- and identifier-quote-insensitive SQL text, for matching
/// constraint expressions and comparing schema shapes. Comments are dropped.
pub(super) fn normalize_sql(sql: &str) -> String {
    let mut output = String::with_capacity(sql.len());
    let mut pending_space = false;
    let tight = |c: char| matches!(c, '(' | ')' | ',' | '=' | ';');
    for (start, end, code) in segments(sql) {
        let text = &sql[start..end];
        if !code {
            if text.starts_with("--") || text.starts_with("/*") {
                pending_space = true;
                continue;
            }
            let text = if text.starts_with('"') || text.starts_with('`') || text.starts_with('[') {
                text.trim_matches(|c| matches!(c, '"' | '`' | '[' | ']'))
            } else {
                text
            };
            if pending_space && !output.is_empty() && !output.ends_with(tight) {
                output.push(' ');
            }
            pending_space = false;
            output.push_str(&text.to_ascii_uppercase());
            continue;
        }
        for c in text.chars() {
            if c.is_whitespace() {
                pending_space = true;
                continue;
            }
            if pending_space && !output.is_empty() && !output.ends_with(tight) && !tight(c) {
                output.push(' ');
            }
            pending_space = false;
            output.push(c.to_ascii_uppercase());
        }
    }
    output
}

/// Finds `CHECK (...)` constraints in code segments: `(keyword_start,
/// close_paren_end, inner_expression)`.
fn check_constraints(sql: &str) -> Vec<(usize, usize, String)> {
    let segments = segments(sql);
    let is_code = |offset: usize| {
        segments
            .iter()
            .any(|&(start, end, code)| code && (start..end).contains(&offset))
    };
    let bytes = sql.as_bytes();
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut result = Vec::new();
    let mut index = 0;
    while index + 5 <= bytes.len() {
        let candidate = &bytes[index..index + 5];
        let bounded = (index == 0 || !identifier(bytes[index - 1]))
            && bytes.get(index + 5).is_none_or(|&byte| !identifier(byte));
        if !candidate.eq_ignore_ascii_case(b"CHECK") || !bounded || !is_code(index) {
            index += 1;
            continue;
        }
        let mut open = index + 5;
        while open < bytes.len() && bytes[open].is_ascii_whitespace() {
            open += 1;
        }
        if bytes.get(open) != Some(&b'(') {
            index += 5;
            continue;
        }
        let mut depth = 0_i32;
        let mut close = None;
        for (start, end, code) in &segments {
            if *end <= open {
                continue;
            }
            if !code {
                continue;
            }
            let first = (*start).max(open);
            for (offset, byte) in bytes.iter().enumerate().take(*end).skip(first) {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(offset);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if close.is_some() {
                break;
            }
        }
        let Some(close) = close else { break };
        result.push((index, close + 1, sql[open + 1..close].to_owned()));
        index = close + 1;
    }
    result
}

/// `sql` without the one enumeration check on `column`, or `None` when the
/// column has no such check or more than one.
fn drop_enumeration_check(sql: &str, column: &str) -> Option<String> {
    let column = column.to_ascii_uppercase();
    let plain = format!("{column} IN(");
    let nullable = format!("{column} IS NULL OR {column} IN(");
    let matches = check_constraints(sql)
        .into_iter()
        .filter(|(_, _, expression)| {
            let expression = normalize_sql(expression);
            expression.starts_with(&plain) || expression.starts_with(&nullable)
        })
        .collect::<Vec<_>>();
    let [(keyword, end, _)] = matches.as_slice() else {
        return None;
    };
    let before = sql[..*keyword].trim_end();
    let after = sql[*end..].trim_start();
    let mut start = before.len();
    // A table constraint (`..., CHECK (...)`) also takes its separating comma.
    if before.ends_with(',') && (after.starts_with(',') || after.starts_with(')')) {
        start -= 1;
    }
    let mut result = String::with_capacity(sql.len());
    result.push_str(&sql[..start]);
    result.push_str(&sql[*end..]);
    Some(result)
}

/// The `CREATE TABLE` statement with its table name replaced by `to`.
fn rename_created_table(sql: &str, from: &str, to: &str) -> Option<String> {
    let prefix = "CREATE TABLE";
    if !sql.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = &sql[prefix.len()..];
    let name_start = prefix.len() + (rest.len() - rest.trim_start().len());
    let tail = &sql[name_start..];
    let (name, length) = match tail.chars().next()? {
        '"' => {
            let close = tail[1..].find('"')? + 1;
            (tail[1..close].to_owned(), close + 1)
        }
        _ => {
            let length = tail
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(tail.len());
            (tail[..length].to_owned(), length)
        }
    };
    if name != from {
        return None;
    }
    Some(format!(
        "{}{}{}",
        &sql[..name_start],
        quote(to),
        &sql[name_start + length..]
    ))
}

/// The frozen baseline schema (store 4, Knowledge 6) that ladder tests start from.
#[cfg(test)]
pub(crate) const BASELINE_FIXTURE: &str =
    include_str!("tests/fixtures/baseline-store4-knowledge6.sql");

/// Test support: gives a current database the baseline shape and versions, as
/// an installed 0.0.1 build left it, keeping its rows.
#[cfg(test)]
pub(crate) fn restore_baseline_for_test(connection: &mut Connection) {
    connection
        .execute_batch("PRAGMA foreign_keys = OFF")
        .unwrap();
    let transaction = connection.transaction().unwrap();
    for (table, _) in STORE_5_ENUMERATIONS.iter().chain(KNOWLEDGE_7_ENUMERATIONS) {
        let start = BASELINE_FIXTURE
            .find(&format!("CREATE TABLE {table} ("))
            .unwrap();
        let end = start + BASELINE_FIXTURE[start..].find(") STRICT;").unwrap() + ") STRICT".len();
        rebuild_table(&transaction, table, &BASELINE_FIXTURE[start..end]).unwrap();
    }
    transaction
        .execute_batch(&format!(
            "PRAGMA user_version = {STORE_BASELINE_VERSION};
             UPDATE knowledge_schema SET version = {KNOWLEDGE_BASELINE_VERSION};"
        ))
        .unwrap();
    transaction.commit().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys = ON")
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_checks_are_found_outside_literals_and_keep_structural_checks() {
        let sql = "CREATE TABLE t (\n  kind TEXT NOT NULL CHECK (kind IN ('a', 'CHECK (x)')),\n  \
                   flag INTEGER NOT NULL CHECK (flag IN (0,1)),\n  \
                   note TEXT CHECK (note IS NULL OR note IN ('x','y')),\n  \
                   CHECK (kind IN ('a') OR flag = 1)\n) STRICT";
        let once = drop_enumeration_check(sql, "kind");
        assert!(once.is_none(), "two checks start with the kind list");
        let without_note = drop_enumeration_check(sql, "note").unwrap();
        assert_eq!(
            normalize_sql(&without_note),
            normalize_sql(
                "CREATE TABLE t (kind TEXT NOT NULL CHECK (kind IN ('a', 'CHECK (x)')), \
                 flag INTEGER NOT NULL CHECK (flag IN (0,1)), note TEXT, \
                 CHECK (kind IN ('a') OR flag = 1)) STRICT"
            )
        );
        let table_level = "CREATE TABLE s (id TEXT NOT NULL,\n data TEXT NOT NULL,\n \
                           CHECK (id IN ('a') OR (substr(id, 1, 2) = 'b:' AND length(id) > 2))\n) STRICT";
        assert_eq!(
            normalize_sql(&drop_enumeration_check(table_level, "id").unwrap()),
            normalize_sql("CREATE TABLE s (id TEXT NOT NULL, data TEXT NOT NULL) STRICT")
        );
        assert!(drop_enumeration_check(sql, "flag").is_some());
        assert!(drop_enumeration_check(sql, "missing").is_none());
    }

    #[test]
    fn created_tables_are_renamed_by_their_exact_name() {
        assert_eq!(
            rename_created_table("CREATE TABLE \"t\"(a)", "t", "t__upgrade").as_deref(),
            Some("CREATE TABLE \"t__upgrade\"(a)")
        );
        assert_eq!(
            rename_created_table("CREATE TABLE t_x (a)", "t_x", "n").as_deref(),
            Some("CREATE TABLE \"n\" (a)")
        );
        assert!(rename_created_table("CREATE TABLE tt (a)", "t", "n").is_none());
        assert!(rename_created_table("CREATE INDEX t ON x(a)", "t", "n").is_none());
    }
}
