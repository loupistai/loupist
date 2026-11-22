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
//! such as sharing, provider status, artifact history or each Loupist's
//! follows, which the catalog creates when it opens) are outside the
//! ladder: a database may or may not contain them, and their module creates
//! them on first use at their current shape. Their DDL must never change shape
//! in place. A shape change is a ladder step that alters the table only when it
//! exists, together with the module's DDL for databases that never created it.
//!
//! Retired ownership tables (`retired_*`) are also outside the ladder. Fresh
//! databases have none; schema convergence ignores them and folder admission
//! accepts them. No ladder step may drop them: only the later export task may
//! read and export their rows, then drop them. Store 7→8 only renames nonempty
//! ownership tables, preserving every field and their foreign-key relationships.
//! Store 8→9 replaces the followed ticker rows with durable registry handles.
//! Store 9→10 records identifier assertions and identity events.
//! Store 10→11 removes embedded chat cards from saved answers.
//! Store 11→12 keeps asset names and references and drops the app-wide followed list.
//! Knowledge 7→8 binds existing prices to each page's legacy listing.
//! Knowledge 9→10 adds where a user-provided Source capture came from.
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
const STORE_STEPS: &[Step] = &[
    store_4_to_5,
    store_5_to_6,
    store_6_to_7,
    store_7_to_8,
    store_8_to_9,
    store_9_to_10,
    store_10_to_11,
    store_11_to_12,
];
/// Knowledge steps; index 0 upgrades `KNOWLEDGE_BASELINE_VERSION` by one.
const KNOWLEDGE_STEPS: &[Step] = &[
    knowledge_6_to_7,
    knowledge_7_to_8,
    knowledge_8_to_9,
    knowledge_9_to_10,
    knowledge_10_to_11,
];

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

// --- Store 5 → 6 ------------------------------------------------------

/// Cover the global-id lookups used by Source span and representation readers.
/// Ids can occur in more than one retained record, so these are not unique.
fn store_5_to_6(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "CREATE INDEX knowledge_spans_by_id ON knowledge_source_spans(span_id, record_id);
         CREATE INDEX knowledge_representations_by_id
             ON knowledge_source_representations(representation_id, record_id);",
    )?;
    Ok(())
}

// --- Store 6 → 7 ------------------------------------------------------

/// Preserve the latest held identities as ordinary follows, keeping existing follows.
fn store_6_to_7(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "INSERT OR IGNORE INTO user_watchlist(symbol, market, display_name, added_at_ms)
         SELECT p.symbol, p.market, p.display_name, s.imported_at_ms
         FROM (SELECT s.snapshot_id, s.source, s.wallet_id, s.imported_at_ms
               FROM user_holdings_snapshots s
               WHERE NOT EXISTS (SELECT 1 FROM user_holdings_snapshots t
                   WHERE t.source = s.source AND t.wallet_id IS s.wallet_id
                   AND (t.imported_at_ms > s.imported_at_ms
                       OR (t.imported_at_ms = s.imported_at_ms AND t.rowid > s.rowid)))) s
         JOIN user_holdings_positions p ON p.snapshot_id = s.snapshot_id
         ORDER BY p.display_name IS NULL, s.imported_at_ms;",
    )?;
    Ok(())
}

/// Frozen store-8 identities: never consult the current registry or class table.
fn store_8_to_9(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        r#"CREATE TABLE instruments (
  instrument_id TEXT PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL,
  form TEXT,
  name TEXT NOT NULL,
  state TEXT NOT NULL,
  status TEXT NOT NULL,
  ended_on TEXT,
  ended_reason TEXT,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE assets (
  asset_id TEXT PRIMARY KEY NOT NULL,
  instrument_id TEXT NOT NULL REFERENCES instruments(instrument_id),
  pricing_ref TEXT,
  user_name TEXT,
  superseded_by TEXT,
  origin TEXT NOT NULL,
  revision INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE listings (
  listing_id TEXT PRIMARY KEY NOT NULL,
  instrument_id TEXT NOT NULL REFERENCES instruments(instrument_id),
  venue TEXT,
  venue_level TEXT,
  symbol TEXT,
  route_market TEXT,
  currency TEXT,
  state TEXT NOT NULL,
  status TEXT NOT NULL,
  ended_on TEXT,
  ended_reason TEXT,
  revision INTEGER NOT NULL,
  hold TEXT,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE user_follows (
  asset_id TEXT PRIMARY KEY NOT NULL REFERENCES assets(asset_id),
  added_at_ms INTEGER NOT NULL
) STRICT;"#,
    )?;
    let mut statement = transaction.prepare("SELECT symbol,market,display_name,added_at_ms FROM user_watchlist ORDER BY added_at_ms,rowid")?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    for (symbol, market, name, at) in rows {
        let normalized_symbol = symbol.trim().to_uppercase();
        let normalized_market = market.trim().to_uppercase();
        let digest =
            crate::util::sha256(format!("{normalized_symbol}:{normalized_market}").as_bytes());
        let h = &digest[..16];
        if !seen.insert(h.to_owned()) {
            continue;
        }
        let currencies = [
            "USD", "EUR", "JPY", "GBP", "KRW", "CNY", "CNH", "CHF", "AUD", "CAD", "HKD", "SGD",
            "NZD", "SEK", "NOK", "DKK", "INR", "TWD", "MXN", "BRL", "ZAR",
        ];
        let pair = normalized_symbol.replace(['/', '-', '='], "");
        let is_pair = pair.len() == 6
            && pair.is_ascii()
            && currencies.contains(&&pair[..3])
            && currencies.contains(&&pair[3..]);
        let kind = match normalized_market.as_str() {
            "CRYPTO" | "UPBIT" | "BITHUMB" | "BINANCE" | "COINBASE" | "KRAKEN" | "BYBIT"
            | "OKX" => "crypto",
            "CME" | "COMEX" | "NYMEX" | "ICE" => "commodity",
            "FX" | "FOREX" | "RATES" | "UST" => "rates_fx",
            "REAL" => "real_asset",
            "POLYMARKET" | "KALSHI" => "prediction",
            "PRIVATE" => "other",
            _ if is_pair => "rates_fx",
            _ => "equity",
        };
        let asset = format!("ast_{h}");
        let instrument = format!("ins_{h}");
        let listing = format!("lst_{h}");
        let instrument_name = name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&symbol);
        let venue = (!market.is_empty() && market != "UNKNOWN").then_some(&market);
        transaction.execute(
            "INSERT INTO instruments VALUES (?1,?2,NULL,?3,'unverified','live',NULL,NULL,?4,?4)",
            rusqlite::params![instrument, kind, instrument_name, at],
        )?;
        transaction.execute("INSERT INTO listings VALUES (?1,?2,?3,NULL,?4,?5,NULL,'unverified','live',NULL,NULL,1,NULL,?6,?6)", rusqlite::params![listing,instrument,venue,symbol,market,at])?;
        transaction.execute(
            "INSERT INTO assets VALUES (?1,?2,?3,?4,NULL,'migration',1,?5,?5)",
            rusqlite::params![asset, instrument, listing, name, at],
        )?;
        transaction.execute(
            "INSERT INTO user_follows VALUES (?1,?2)",
            rusqlite::params![asset, at],
        )?;
    }
    transaction.execute_batch("DROP TABLE user_watchlist;")?;
    Ok(())
}

/// Preserve what this database knew, without assigning effective dates.
fn store_9_to_10(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "CREATE TABLE identifiers (
  identifier_id TEXT PRIMARY KEY NOT NULL,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  scheme TEXT NOT NULL,
  value TEXT NOT NULL,
  qualifier TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL,
  valid_from TEXT,
  valid_to TEXT,
  observed_from_ms INTEGER NOT NULL,
  observed_until_ms INTEGER,
  supersedes TEXT,
  source TEXT NOT NULL,
  evidence_json TEXT
) STRICT;
CREATE INDEX identifiers_by_value ON identifiers(scheme,value,qualifier);
CREATE INDEX identifiers_by_subject ON identifiers(subject_kind,subject_id);
CREATE TABLE events (
  event_id TEXT PRIMARY KEY NOT NULL,
  asset_id TEXT,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  effective_on TEXT,
  detected_on TEXT NOT NULL,
  facts_json TEXT NOT NULL,
  source TEXT NOT NULL,
  evidence_json TEXT,
  state TEXT NOT NULL,
  supersedes TEXT,
  recorded_at_ms INTEGER NOT NULL
) STRICT;

INSERT INTO identifiers
SELECT 'id_migration_ticker_' || l.listing_id, 'listing', l.listing_id, 'ticker',
       l.symbol, CASE WHEN l.venue IS NULL OR upper(trim(l.venue))='UNKNOWN' THEN '' ELSE l.venue END,
       'accepted', NULL, NULL,
       COALESCE((SELECT min(f.added_at_ms) FROM assets a JOIN user_follows f USING(asset_id)
                 WHERE a.pricing_ref=l.listing_id), l.created_at_ms),
       NULL, NULL, 'migration', NULL
FROM listings l WHERE trim(l.symbol)<>'';

INSERT INTO identifiers
SELECT 'id_migration_name_' || i.instrument_id, 'instrument', i.instrument_id, 'name',
       i.name, '', 'accepted', NULL, NULL, i.created_at_ms, NULL, NULL, 'migration', NULL
FROM instruments i
WHERE trim(i.name)<>'' AND i.state IN ('local','identified');

-- Store 9 copied personal labels into the unverified instrument projection.
-- The current personal label remains in assets.user_name.
UPDATE instruments
SET name=COALESCE((SELECT l.symbol FROM listings l
                  WHERE l.instrument_id=instruments.instrument_id AND trim(l.symbol)<>''
                  ORDER BY l.listing_id LIMIT 1), '')
WHERE state='unverified';",
    )?;
    Ok(())
}

/// Remove only the two store 10 message fields. Work on concrete array
/// indices, including saved versions, and update only rows with those keys.
/// SQLite preserves the compact writer's member order, string escapes and
/// number tokens. Chat writes compare the resulting text byte for byte.
fn store_10_to_11(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "WITH RECURSIVE
         chats(row_id, value) AS NOT MATERIALIZED (
             SELECT rowid, CASE WHEN json_valid(data_json) THEN data_json END
             FROM chat_sessions
         ),
         message_paths(row_id, path, value) AS (
             SELECT c.row_id, message.fullkey,
                    CASE WHEN message.type = 'object' THEN message.value END
             FROM chats c, json_each(c.value, '$.messages') message
             WHERE typeof(message.key) = 'integer' AND message.type = 'object'
             UNION ALL
             SELECT c.row_id, message.fullkey,
                    CASE WHEN message.type = 'object' THEN message.value END
             FROM chats c,
                  json_each(c.value, '$.versions') version,
                  json_each(c.value, version.fullkey || '.messages') message
             WHERE typeof(version.key) = 'integer' AND version.type = 'object'
               AND typeof(message.key) = 'integer' AND message.type = 'object'
         ),
         targets AS (
             SELECT row_id, path,
                    row_number() OVER (PARTITION BY row_id ORDER BY path) AS step,
                    count(*) OVER (PARTITION BY row_id) AS total
             FROM message_paths
             WHERE json_type(value, '$.assetSnapshots') IS NOT NULL
                OR json_type(value, '$.sensitivityCards') IS NOT NULL
         ),
         stripped(row_id, value, step, total) AS (
             SELECT c.rowid, jsonb(c.data_json), 0, t.total
             FROM targets t JOIN chat_sessions c ON c.rowid = t.row_id
             WHERE t.step = 1
             UNION ALL
             SELECT s.row_id,
                    jsonb_remove(s.value, t.path || '.assetSnapshots', t.path || '.sensitivityCards'),
                    t.step, t.total
             FROM stripped s JOIN targets t ON t.row_id = s.row_id AND t.step = s.step + 1
         )
         UPDATE chat_sessions
         SET data_json = json(stripped.value)
         FROM stripped
         WHERE chat_sessions.rowid = stripped.row_id AND stripped.step = stripped.total;",
    )?;
    Ok(())
}

/// Retires the stored ownership feature without deleting any existing row.
fn store_7_to_8(transaction: &Transaction<'_>) -> StoreResult<()> {
    let has_rows: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM user_wallet_addresses)
             OR EXISTS(SELECT 1 FROM user_holdings_snapshots)
             OR EXISTS(SELECT 1 FROM user_holdings_positions)",
        [],
        |row| row.get(0),
    )?;
    transaction.execute_batch(if has_rows {
        "ALTER TABLE user_wallet_addresses RENAME TO retired_wallet_addresses;
         ALTER TABLE user_holdings_snapshots RENAME TO retired_holdings_snapshots;
         ALTER TABLE user_holdings_positions RENAME TO retired_holdings_positions;"
    } else {
        "DROP TABLE user_holdings_positions;
         DROP TABLE user_holdings_snapshots;
         DROP TABLE user_wallet_addresses;"
    })?;
    transaction.execute_batch(
        "DELETE FROM loupist_capability_bindings WHERE entry_id = 'onchain.wallets';",
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

fn knowledge_7_to_8(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "ALTER TABLE knowledge_asset_observations ADD COLUMN series_ref TEXT;
         ALTER TABLE knowledge_asset_pages ADD COLUMN pricing_ref TEXT;
         ALTER TABLE knowledge_asset_pages ADD COLUMN pricing_rev INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE knowledge_asset_pages ADD COLUMN pricing_hold TEXT;
         ALTER TABLE knowledge_asset_pages ADD COLUMN series_symbol TEXT;
         ALTER TABLE knowledge_asset_pages ADD COLUMN entity_source TEXT;
         ALTER TABLE knowledge_asset_pages ADD COLUMN entity_evidence_json TEXT;
         ALTER TABLE knowledge_asset_pages ADD COLUMN identity_json TEXT NOT NULL DEFAULT '{}';

         UPDATE knowledge_asset_pages
         SET pricing_ref = 'lst_' || substr(asset_id, 5), series_symbol = symbol
         WHERE trim(symbol) <> '';
         UPDATE knowledge_asset_pages SET entity_source = 'auto' WHERE entity_id IS NOT NULL;
         UPDATE knowledge_asset_observations AS observation
         SET series_ref = page.pricing_ref, dedupe_key = page.pricing_ref || '|' || observation.dedupe_key
         FROM knowledge_asset_pages AS page
         WHERE observation.loupist_id = page.loupist_id AND observation.asset_id = page.asset_id
           AND observation.kind IN ('close', 'quote') AND page.pricing_ref IS NOT NULL;",
    )?;
    Ok(())
}

/// Keeps catalog asset handles with their names and references and clears
/// local results jobs and their posts. The app-wide followed list belongs to
/// no Loupist, so it is dropped: each Loupist follows its own assets.
fn store_11_to_12(tx: &Transaction<'_>) -> StoreResult<()> {
    tx.execute_batch("ALTER TABLE assets RENAME TO assets_v11;")?;
    tx.execute_batch(super::user_context::DDL)?;
    tx.execute_batch(
        r#"
        INSERT INTO assets(asset_id,name,reference_json,created_at_ms)
        SELECT a.asset_id,
            coalesce(nullif(trim(a.user_name),''),nullif(trim(i.name),''),nullif(trim(l.symbol),''),a.asset_id),
            CASE WHEN nullif(trim(l.symbol),'') IS NULL AND nullif(trim(l.route_market),'') IS NULL
                 THEN NULL ELSE json_object('ticker',nullif(trim(l.symbol),''),
                    'exchange', CASE WHEN upper(trim(l.route_market))='UNKNOWN'
                        THEN NULL ELSE nullif(trim(l.route_market),'') END,
                    'origin','imported') END,
            a.created_at_ms
        FROM assets_v11 a JOIN instruments i USING(instrument_id)
        LEFT JOIN listings l ON l.listing_id=a.pricing_ref;
        DROP TABLE user_follows;
        DROP TABLE assets_v11;
        DROP TABLE listings;
        DROP TABLE instruments;
        DROP TABLE identifiers;
        DROP TABLE events;
        DELETE FROM scheduled_task_runs WHERE json_extract(data_json,'$.kind')='earnings'
            OR task_id IN (SELECT id FROM scheduled_tasks WHERE json_extract(data_json,'$.kind')='earnings');
        DELETE FROM scheduled_tasks WHERE json_extract(data_json,'$.kind')='earnings';
        UPDATE scheduled_task_runs SET data_json=json_remove(data_json,
            '$.summary_items','$.briefing_context.assets','$.briefing_context.earnings',
            '$.briefing_context.watchEvents')
        WHERE json_type(data_json,'$.briefing_context')='object' OR json_type(data_json,'$.summary_items') IS NOT NULL;
        "#,
    )?;
    // Delete by descending array index so subsequent paths keep their meaning.
    let paths = {
        let mut statement = tx.prepare(
            "SELECT session.rowid, message.fullkey
             FROM chat_sessions session, json_tree(CASE WHEN json_valid(session.data_json) THEN session.data_json END) message
             WHERE message.type='object' AND
               (message.path='$.messages' OR message.path LIKE '$.versions[%].messages')
               AND (json_type(CASE WHEN message.type='object' THEN message.value END,'$.watch')='object' OR json_type(CASE WHEN message.type='object' THEN message.value END,'$.earnings')='object')
             ORDER BY session.rowid, message.id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (id, path) in paths {
        tx.execute("UPDATE chat_sessions SET data_json=json_remove(json_set(data_json,'$.piSessionCache',NULL),?2) WHERE rowid=?1",
            rusqlite::params![id,path])?;
    }
    Ok(())
}

fn knowledge_8_to_9(tx: &Transaction<'_>) -> StoreResult<()> {
    tx.execute_batch("ALTER TABLE knowledge_asset_pages RENAME TO knowledge_asset_pages_v8;")?;
    tx.execute_batch(crate::knowledge::assets::PAGE_DDL)?;
    tx.execute_batch(crate::knowledge::assets::READ_DDL)?;
    tx.execute_batch(
        r#"
        INSERT INTO knowledge_asset_pages(loupist_id,asset_id,display_name,entity_id,verdict,
            verdict_move_id,created_at_ms,revision,entity_source,entity_evidence_json)
        SELECT loupist_id,asset_id,display_name,entity_id,verdict,
            verdict_move_id,created_at_ms,revision,entity_source,entity_evidence_json
        FROM knowledge_asset_pages_v8;
        INSERT OR IGNORE INTO knowledge_asset_line_readings
            (loupist_id,asset_id,journal_id,fact_date,observed,evidence_json,recorded_at_ms)
        SELECT loupist_id,asset_id,json_extract(facts_json,'$.journalId'),fact_date,
            json_extract(facts_json,'$.observed'),
            coalesce(json_extract(facts_json,'$.evidence'),json_array(json(evidence_json))),recorded_at_ms
        FROM knowledge_asset_observations WHERE kind='line' AND
            json_type(facts_json,'$.journalId')='text' AND
            json_type(facts_json,'$.observed') IN ('real','integer');
        DROP TABLE knowledge_asset_pages_v8;
        DROP TABLE knowledge_asset_observations;
        DROP TABLE knowledge_asset_section_deltas;
        DROP TABLE knowledge_watch_assets;
        DROP TABLE knowledge_watch_events;
        DROP TABLE knowledge_watch_routes;
        DROP TABLE knowledge_watch_settings;
        DROP TABLE knowledge_watch_state;
        DROP TABLE knowledge_earnings_notices;
        DROP TABLE knowledge_earnings_rows;
        DROP TABLE knowledge_earnings_tone;
        DROP TABLE knowledge_earnings_events;
        "#,
    )?;
    Ok(())
}

/// The Chat message, paste time and stated link and publisher of a
/// user-provided capture. Existing captures have none.
fn knowledge_9_to_10(tx: &Transaction<'_>) -> StoreResult<()> {
    tx.execute_batch("ALTER TABLE knowledge_source_captures ADD COLUMN provided_json TEXT;")?;
    Ok(())
}

/// Existing growth events have no recorded publication identity.
fn knowledge_10_to_11(tx: &Transaction<'_>) -> StoreResult<()> {
    tx.execute_batch("ALTER TABLE knowledge_growth_events ADD COLUMN publication_json TEXT;")?;
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

/// Builds historical fixture versions through their frozen steps, without a file backup.
#[cfg(test)]
pub(crate) fn upgrade_store_for_test(connection: &mut Connection, version: i64) {
    let ladder = Ladder {
        steps: &STORE_STEPS[..(version - STORE_BASELINE_VERSION) as usize],
        ..STORE
    };
    upgrade_with(connection, None, &[ladder, KNOWLEDGE]).unwrap();
}

/// Test support: gives a current database the baseline shape and versions, as
/// an installed 0.0.1 build left it, keeping its rows.
#[cfg(test)]
pub(crate) fn restore_baseline_for_test(connection: &mut Connection) {
    connection
        .execute_batch("PRAGMA foreign_keys = OFF")
        .unwrap();
    let transaction = connection.transaction().unwrap();
    for table in [
        "user_watchlist",
        "user_wallet_addresses",
        "user_holdings_snapshots",
        "user_holdings_positions",
    ] {
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        if !exists {
            let start = BASELINE_FIXTURE
                .find(&format!("CREATE TABLE {table} ("))
                .unwrap();
            let end =
                start + BASELINE_FIXTURE[start..].find(") STRICT;").unwrap() + ") STRICT;".len();
            transaction
                .execute_batch(&BASELINE_FIXTURE[start..end])
                .unwrap();
        }
    }
    // A 0.0.1 build kept one followed list for the app, in `user_watchlist`.
    let follows: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='asset_follows')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    if follows {
        transaction
            .execute_batch(
                "INSERT INTO user_watchlist
                 SELECT json_extract(a.reference_json,'$.ticker'),
                   coalesce(json_extract(a.reference_json,'$.exchange'),'UNKNOWN'),a.name,f.at
                 FROM assets a JOIN (SELECT asset_id,min(followed_at_ms) AS at FROM asset_follows
                   GROUP BY asset_id) f USING(asset_id)
                 WHERE json_type(a.reference_json,'$.ticker')='text';
                 DROP TABLE asset_follows;",
            )
            .unwrap();
    }
    transaction
        .execute_batch(
            "ALTER TABLE knowledge_asset_pages RENAME TO thin_test_pages;
         DROP TABLE knowledge_asset_line_readings;
         DROP TABLE knowledge_asset_price_points;
         DROP TABLE knowledge_saved_price_reads;",
        )
        .unwrap();
    for table in [
        "knowledge_asset_pages",
        "knowledge_asset_observations",
        "knowledge_asset_section_deltas",
        "knowledge_earnings_events",
        "knowledge_earnings_notices",
        "knowledge_earnings_rows",
        "knowledge_earnings_tone",
        "knowledge_watch_assets",
        "knowledge_watch_events",
        "knowledge_watch_routes",
        "knowledge_watch_settings",
        "knowledge_watch_state",
    ] {
        let start = BASELINE_FIXTURE
            .find(&format!("CREATE TABLE {table} ("))
            .unwrap();
        let end = start + BASELINE_FIXTURE[start..].find(") STRICT;").unwrap() + ") STRICT;".len();
        transaction
            .execute_batch(&BASELINE_FIXTURE[start..end])
            .unwrap();
    }
    for line in BASELINE_FIXTURE.lines().filter(|line| {
        line.starts_with("CREATE INDEX")
            && [
                "knowledge_asset_observations",
                "knowledge_watch_",
                "knowledge_earnings_",
            ]
            .iter()
            .any(|table| line.contains(table))
    }) {
        transaction.execute_batch(line).unwrap();
    }
    transaction.execute_batch(
        "INSERT INTO knowledge_asset_pages
           (loupist_id,asset_id,symbol,market,display_name,class,class_source,entity_id,verdict,verdict_move_id,created_at_ms,revision)
         SELECT loupist_id,asset_id,'','',display_name,'equity','default',entity_id,verdict,verdict_move_id,created_at_ms,revision
         FROM thin_test_pages;
         DROP TABLE thin_test_pages;",
    ).unwrap();
    transaction
        .execute_batch("ALTER TABLE knowledge_growth_events DROP COLUMN publication_json;")
        .unwrap();
    for (table, _) in STORE_5_ENUMERATIONS.iter().chain(KNOWLEDGE_7_ENUMERATIONS) {
        let start = BASELINE_FIXTURE
            .find(&format!("CREATE TABLE {table} ("))
            .unwrap();
        let end = start + BASELINE_FIXTURE[start..].find(") STRICT;").unwrap() + ") STRICT".len();
        rebuild_table(&transaction, table, &BASELINE_FIXTURE[start..end]).unwrap();
    }
    transaction
        .execute_batch(&format!(
            "DROP TABLE assets;
             ALTER TABLE knowledge_source_captures DROP COLUMN provided_json;
             DROP INDEX IF EXISTS knowledge_spans_by_id;
             DROP INDEX IF EXISTS knowledge_representations_by_id;
             PRAGMA user_version = {STORE_BASELINE_VERSION};
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
