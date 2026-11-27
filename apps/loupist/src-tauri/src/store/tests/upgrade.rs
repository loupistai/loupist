//! The upgrade ladder from the frozen baseline (store 4, Knowledge 6).
use super::*;
use crate::knowledge::KNOWLEDGE_SCHEMA_VERSION;
use crate::store::upgrade::{normalize_sql, BASELINE_FIXTURE};
use crate::store::STORE_SCHEMA_VERSION;
use rusqlite::{types::Value, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SEED: &str = "
INSERT INTO loupist_profiles VALUES ('loupist-a', 1, '/research/a', NULL, NULL, '{}');
INSERT INTO chat_sessions VALUES ('chat-a', 'loupist-a', 2, '{\"messages\":[]}');
INSERT INTO chat_artifacts VALUES ('artifact-a', 'chat-a', 'chart', 'Margins', 1, 3, 3, '{}', '{}');
INSERT INTO chart_artifact_datasets VALUES ('dataset-a', 'artifact-a', 'digest', '{}');
INSERT INTO chat_handoffs VALUES ('handoff-a', 'loupist-a', 'request', 'chat-a', 'running', 'run-a', 4, '{}');
INSERT INTO user_wallet_addresses VALUES ('wallet-a', 'evm', '0xabc', 'Main', 5, 6, 6, 'ok', 0);
INSERT INTO user_holdings_snapshots VALUES ('snapshot-a', 'wallet', 'wallet-a', 7, 'USD', NULL);
INSERT INTO user_holdings_snapshots VALUES ('snapshot-b', 'csv', NULL, 8, 'USD', 'broker');
INSERT INTO user_holdings_positions VALUES ('snapshot-a', 'ETH', 'CRYPTO', 'Ether', '2', NULL, NULL, 'USD');
INSERT INTO app_settings VALUES ('model', '{}');
INSERT INTO app_settings VALUES ('world_processing:loupist-a', '{\"paused\":true}');
INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, actor, summary)
    VALUES ('gev-b', 'loupist-a', 9, 'claim_added', 'processing', 'Added a claim');
INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, actor, summary)
    VALUES ('gev-a', 'loupist-a', 10, 'lens_created', 'user', 'Created a method');
INSERT INTO knowledge_records VALUES ('lens-a', 'lens', 'loupist-a', 'Method', 11);
INSERT INTO knowledge_lens_metadata VALUES ('lens-a', 'loupist-a', 'Method', 'method', 'perspective', NULL, NULL, 0, 12);
INSERT INTO knowledge_lens_log (op_id, loupist_id, record_id, op_json, outcome, origin, created_at_ms)
    VALUES ('op-a', 'loupist-a', 'lens-a', '{}', 'added', 'turn', 13);
INSERT INTO knowledge_lens_anchors (loupist_id, record_id, role, name_fold, created_at_ms)
    VALUES ('loupist-a', 'lens-a', 'mention', 'zeta', 14);
INSERT INTO knowledge_lens_anchors (loupist_id, record_id, role, name_fold, created_at_ms)
    VALUES ('loupist-a', 'lens-a', 'mention', 'alpha', 15);
INSERT INTO knowledge_connections VALUES ('connection-a', 'loupist-a', 'n1', 'n2', '[]', 3, 'shown', 16, 16, 16);
INSERT INTO knowledge_learning_reviews (review_id, loupist_id, world_job_id, status, created_at_ms,
    through_event, settings_json, call_budget)
    VALUES ('review-a', 'loupist-a', 'job-a', 'running', 17, 1, '{}', 1);
INSERT INTO knowledge_watch_events (event_id, loupist_id, asset_id, kind, natural_key, fact_date,
    facts_json, evidence_json, detected_at_ms, delivered_in)
    VALUES ('watch-a', 'loupist-a', 'asset-a', 'depeg', 'key', '2026-11-01', '{}', '[]', 18, 'morning');
INSERT INTO knowledge_earnings_notices VALUES ('loupist-a', 'notice-a', 'event-a', 'reported', 'after', 19, NULL);
";

fn baseline_database(directory: &Path, fixture: &str) -> PathBuf {
    let path = directory.join("loupist.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(fixture).unwrap();
    connection.execute_batch(SEED).unwrap();
    path
}

fn versions(connection: &Connection) -> (i64, i64) {
    (
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        connection
            .query_row("SELECT version FROM knowledge_schema", [], |row| row.get(0))
            .unwrap(),
    )
}

/// Every row of every table, by rowid, except the version row itself.
fn data(connection: &Connection) -> BTreeMap<String, Vec<Vec<Value>>> {
    let mut rows = rows(connection);
    rows.remove("knowledge_schema");
    rows
}

/// Every row of every table, by rowid.
fn rows(connection: &Connection) -> BTreeMap<String, Vec<Vec<Value>>> {
    let tables = {
        let mut statement = connection
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        names
    };
    tables
        .into_iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT rowid, * FROM \"{table}\" ORDER BY rowid"))
                .unwrap();
            let width = statement.column_count();
            let values = statement
                .query_map([], |row| (0..width).map(|index| row.get(index)).collect())
                .unwrap()
                .collect::<Result<Vec<Vec<Value>>, _>>()
                .unwrap();
            (table, values)
        })
        .collect()
}

/// Every schema object with its whitespace-, case- and quote-normalized SQL.
fn shape(connection: &Connection) -> BTreeMap<(String, String), Option<String>> {
    let mut statement = connection
        .prepare("SELECT type, name, sql FROM sqlite_schema WHERE tbl_name NOT LIKE 'retired_%'")
        .unwrap();
    let shape = statement
        .query_map([], |row| {
            Ok((
                (row.get(0)?, row.get(1)?),
                row.get::<_, Option<String>>(2)?
                    .map(|sql| normalize_sql(&sql)),
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    shape
}

fn backups(directory: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            (name.starts_with("loupist.sqlite3.upgrade-backup-")
                || name.starts_with("catalog.sqlite3.upgrade-backup-"))
            .then_some(path.clone())
        })
        .collect()
}

/// A backup an earlier upgrade left, named for `suffix` (its stamp and any
/// same-second attempt).
fn prior_backup(directory: &Path, suffix: &str) -> PathBuf {
    let path = directory.join(format!("loupist.sqlite3.upgrade-backup-{suffix}"));
    std::fs::write(&path, suffix).unwrap();
    path
}

/// The backup this upgrade wrote: the only one stamped in the present.
fn new_backup(directory: &Path) -> PathBuf {
    let year = chrono::Utc::now().format("%Y").to_string();
    let [backup] = backups(directory)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(&format!("-backup-{year}")))
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    backup
}

/// The baseline with a shape the store step does not expect in its fifth
/// table, after it has rebuilt four: the whole step must roll back.
fn unexpected_baseline() -> String {
    let unexpected = BASELINE_FIXTURE.replacen(
        "status TEXT NOT NULL CHECK (status IN ('created','running','done','failed','stopped')),",
        "status TEXT NOT NULL,",
        1,
    );
    assert_ne!(unexpected, BASELINE_FIXTURE);
    unexpected
}

#[test]
fn a_baseline_database_upgrades_to_the_fresh_schema_with_its_data_intact() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    let before = rows(&Connection::open(&path).unwrap());
    let mut data_before = data(&Connection::open(&path).unwrap());
    data_before.remove("user_watchlist");
    for (old, retired) in [
        ("user_wallet_addresses", "retired_wallet_addresses"),
        ("user_holdings_snapshots", "retired_holdings_snapshots"),
        ("user_holdings_positions", "retired_holdings_positions"),
    ] {
        let rows = data_before.remove(old).unwrap();
        data_before.insert(retired.into(), rows);
    }

    drop(SqliteStore::open(&path).unwrap());

    let upgraded = Connection::open(&path).unwrap();
    assert_eq!(
        versions(&upgraded),
        (
            STORE_SCHEMA_VERSION,
            crate::knowledge::KNOWLEDGE_SCHEMA_VERSION
        )
    );
    let mut actual = data(&upgraded);
    for name in actual.keys() {
        assert!(!matches!(
            name.as_str(),
            "instruments" | "listings" | "identifiers" | "events" | "user_follows"
        ));
    }
    data_before.retain(|name, _| {
        !name.starts_with("knowledge_watch_")
            && !name.starts_with("knowledge_earnings_")
            && !matches!(
                name.as_str(),
                "knowledge_asset_observations" | "knowledge_asset_section_deltas"
            )
    });
    for table in [
        "assets",
        "knowledge_asset_line_readings",
        "knowledge_asset_price_points",
        "knowledge_saved_price_reads",
    ] {
        actual.remove(table);
    }
    // The app-wide followed list had no Loupist, so no Loupist follows it.
    assert_eq!(actual.remove("asset_follows"), Some(Vec::new()));
    assert_eq!(actual.remove("scripts"), Some(Vec::new()));
    assert_eq!(actual.remove("knowledge_world_episodes"), Some(Vec::new()));
    for event in data_before.get_mut("knowledge_growth_events").unwrap() {
        event.push(Value::Null); // Earlier events did not record their publication.
    }
    assert_eq!(actual, data_before, "unrelated rows and rowids are kept");
    let id = crate::knowledge::assets::asset_id("ETH", "CRYPTO");
    assert_eq!(
        upgraded.query_row(
            "SELECT asset_id,name,json_extract(reference_json,'$.ticker'),json_extract(reference_json,'$.exchange') FROM assets",
            [], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?))
        ).unwrap(),
        (id, "Ether".into(), "ETH".into(), "CRYPTO".into()),
    );
    assert!(!rows(&upgraded).contains_key("user_watchlist"));
    // An upgraded install has exactly the shape a fresh install has.
    let fresh = SqliteStore::open_in_memory().unwrap();
    assert_eq!(shape(&upgraded), shape(&fresh.lock().unwrap()));
    // Immutability triggers, partial indexes and references were rebuilt.
    let immutable = upgraded
        .execute("UPDATE knowledge_growth_events SET summary = 'changed'", [])
        .unwrap_err();
    assert!(immutable.to_string().contains("immutable"), "{immutable}");
    assert!(upgraded
        .execute(
            "INSERT INTO knowledge_records VALUES ('lens-b', 'lens', 'loupist-a', 'Other', 20)",
            [],
        )
        .is_ok());
    assert!(upgraded
        .execute(
            "INSERT INTO knowledge_lens_metadata VALUES ('lens-b', 'loupist-a', 'Other', 'other', 'perspective', NULL, NULL, 0, 20)",
            [],
        )
        .is_err());
    upgraded.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    assert!(upgraded
        .execute(
            "INSERT INTO chart_artifact_datasets VALUES ('dataset-x', 'missing', 'digest', '{}')",
            [],
        )
        .is_err());
    // Extensible enumerations are Rust's to validate now; structure stays SQL's.
    upgraded
        .execute(
            "INSERT INTO chat_artifacts VALUES ('artifact-b', 'chat-a', 'table', 'Grid', 1, 3, 3, '{}', '{}')",
            [],
        )
        .unwrap();
    assert!(upgraded
        .execute(
            "INSERT INTO retired_holdings_snapshots VALUES ('snapshot-c', 'wallet', NULL, 9, 'USD', NULL)",
            [],
        )
        .is_err());
    // The backup is the untouched baseline, owner-only, beside the database.
    let [backup] = backups(&directory).try_into().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let backup = Connection::open(&backup).unwrap();
    assert_eq!(versions(&backup), (4, 6));
    assert_eq!(rows(&backup), before);
    let after = rows(&upgraded);
    // Opening the current database again writes no further backup.
    drop(SqliteStore::open(&path).unwrap());
    assert_eq!(backups(&directory).len(), 1);
    assert_eq!(rows(&Connection::open(&path).unwrap()), after);
}

#[test]
fn world_episode_upgrade_recovers_only_proven_publications() {
    use crate::knowledge::world_episodes;
    use crate::knowledge::{BlobStore, CreateRecord, KnowledgeStore, RecordKind, SealWorld};
    use rusqlite::params;
    use serde_json::{json, Value};
    use std::sync::Arc;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("loupist.sqlite3");
    let store = Arc::new(SqliteStore::open(&path).unwrap());
    let blobs = || BlobStore::open(directory.path().join("blobs")).unwrap();
    let knowledge = KnowledgeStore::attach(store.clone(), blobs());
    let world = knowledge
        .create_record(CreateRecord {
            loupist_id: "owner".into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: 1,
        })
        .unwrap();
    let publish = |knowledge: &KnowledgeStore, expected_revision_id, at_ms| {
        knowledge
            .seal_world(SealWorld {
                loupist_id: "owner".into(),
                record_id: world.record_id.clone(),
                expected_revision_id,
                at_ms,
                nodes: Vec::new(),
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap()
    };
    let head = publish(&knowledge, None, 2);
    drop(knowledge);
    let id = |kind: &str, n: u32| format!("{kind}_{n:032x}");
    let publication = json!({"precision":"instant", "value":"2026-03-15T15:00:00+09:00"});
    let payload = |n, seal, kind: &str| {
        json!({
            "episodeKind":kind,
            "sourceBinding": {
                "sourceId":id("src", n), "captureId":id("cap", n),
                "representationId":id("rep", n), "spanId":id("spn", n),
                "payloadDigest":"a".repeat(64), "sourceRecordId":id("rec", n),
                "sourceRevisionId":id("rev", n),
            },
            "sealedRevisionId":id("rev", seal),
        })
    };
    {
        let db = store.lock().unwrap();
        // The malformed row represents damaged legacy data. Remove the two
        // expression indexes before introducing it.
        let indexes = {
            let mut statement = db.prepare(
                "SELECT name FROM sqlite_schema WHERE tbl_name='knowledge_graph_outbox' AND sql LIKE '%json_extract%'",
            ).unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        for index in indexes {
            db.execute_batch(&format!("DROP INDEX {index}")).unwrap();
        }
        db.execute_batch(
            "DROP TABLE knowledge_world_episodes;
             ALTER TABLE knowledge_capture_lineage DROP COLUMN allowance_origin;
             UPDATE knowledge_schema SET version = 11;",
        )
        .unwrap();
        let mut documented = payload(1, 11, "source_span");
        documented["chunkIndex"] = json!(1);
        documented["chunkCount"] = json!(2);
        documented["charStart"] = json!(200);
        documented["charEnd"] = json!(369);
        documented["publicationTime"] = publication.clone();
        documented["referenceTime"] = publication["value"].clone();
        documented["extraction"] = json!({"modelProfile":"test", "thinkingLevel":"high"});
        let mut invalid_binding = payload(8, 18, "source_span");
        invalid_binding["sourceBinding"]["spanId"] = Value::String(String::new());
        for (n, status, body) in [
            (1, "completed", documented.to_string()),
            (2, "completed", payload(2, 12, "source_span").to_string()),
            (3, "completed", "{malformed".into()),
            (4, "completed", payload(4, 14, "source_span").to_string()),
            (5, "completed", payload(5, 14, "source_span").to_string()),
            (
                6,
                "reconcile_required",
                payload(6, 16, "source_span").to_string(),
            ),
            (7, "completed", payload(7, 17, "world_recovery").to_string()),
            (8, "completed", invalid_binding.to_string()),
        ] {
            db.execute(
                "INSERT INTO knowledge_graph_outbox VALUES (?1,'owner',?2,?3,?4,?5,'digest',3)",
                params![
                    id("job", n),
                    world.record_id,
                    head.revision_id,
                    status,
                    body
                ],
            )
            .unwrap();
        }
        for (subject, owner, record, seal, kind) in [
            (
                "right-claim",
                "owner",
                world.record_id.as_str(),
                11,
                "claim_added",
            ),
            (
                "right-entity",
                "owner",
                world.record_id.as_str(),
                11,
                "entity_added",
            ),
            (
                "wrong-owner",
                "other",
                world.record_id.as_str(),
                11,
                "claim_added",
            ),
            ("wrong-world", "owner", "other-world", 11, "claim_added"),
            (
                "wrong-revision",
                "owner",
                world.record_id.as_str(),
                4,
                "entity_added",
            ),
        ] {
            db.execute(
                "INSERT INTO knowledge_growth_events
                 (event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary, publication_json)
                 VALUES (?1, ?2, 4, ?3, ?4, ?1, 'processing', 'growth', ?5)",
                params![subject, owner, kind, record, json!({"revisionId":id("rev", seal)}).to_string()],
            ).unwrap();
        }
    }
    drop(store);

    let store = Arc::new(SqliteStore::open(&path).unwrap());
    let count = |db: &Connection| {
        db.query_row("SELECT COUNT(*) FROM knowledge_world_episodes", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    {
        let mut db = store.lock().unwrap();
        assert_eq!(versions(&db).1, 12);
        assert_eq!(count(&db), 2);
        for sql in [
            "UPDATE knowledge_world_episodes SET created_at_ms = 42",
            "DELETE FROM knowledge_world_episodes",
        ] {
            assert!(db
                .execute(sql, [])
                .unwrap_err()
                .to_string()
                .contains("immutable"));
        }
        let tx = db.transaction().unwrap();
        let known = world_episodes::by_source(&tx, "owner", &id("rec", 1)).unwrap();
        let unknown = world_episodes::by_source(&tx, "owner", &id("rec", 2)).unwrap();
        assert_eq!(known[0].job_id, id("job", 1));
        assert_eq!(unknown[0].job_id, id("job", 2));
        assert!(known[0].backfilled && unknown[0].backfilled);
        assert_eq!(known[0].chunk_index, Some(1));
        assert_eq!(known[0].char_end, Some(369));
        assert_eq!(known[0].changes.created, ["right-claim"]);
        assert_eq!(known[0].changes.entities_added, ["right-entity"]);
        assert_eq!(
            known[0].publication_time.as_ref().unwrap().value.as_deref(),
            publication["value"].as_str()
        );
        assert_eq!(unknown[0].chunk_index, None);
        assert_eq!(unknown[0].reference_time, None);
        assert_eq!(unknown[0].reference_basis, None);
        assert_eq!(unknown[0].model, None);
        assert_eq!(
            world_episodes::for_claim(&tx, "owner", "right-claim").unwrap(),
            known
        );
        assert!(world_episodes::since(&tx, "owner", 0).unwrap().is_empty());
        let mut settings = crate::knowledge::GraphProcessingSettings::default();
        settings.memory_controls.removed.insert(id("rec", 1));
        crate::knowledge::store_settings(&tx, "owner", &settings).unwrap();
        assert!(world_episodes::for_claim(&tx, "owner", "right-claim")
            .unwrap()
            .is_empty());
        assert!(world_episodes::by_source(&tx, "owner", &id("rec", 1))
            .unwrap()
            .is_empty());
        assert_eq!(
            world_episodes::by_source(&tx, "owner", &id("rec", 2)).unwrap(),
            unknown
        );
        tx.rollback().unwrap();
    }

    let knowledge = KnowledgeStore::attach(store.clone(), blobs());
    publish(&knowledge, Some(head.revision_id), 5);
    let mut db = store.lock().unwrap();
    assert_eq!(count(&db), 2);
    let tx = db.transaction().unwrap();
    crate::knowledge::erase_loupist_private(&tx, "owner").unwrap();
    assert_eq!(count(&tx), 0);
}

#[test]
fn the_upgrade_restores_a_session_cache_key_an_earlier_reconnect_removed() {
    // Before store 5 a reconnect cleared caches with `json_remove`, and the
    // Chat's text compare-and-swap then refused every later write.
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    let chat = crate::test_fixtures::chat_session("chat-cleared", "loupist-a");
    let written = serde_json::to_string(&chat).unwrap();
    assert!(
        written.contains(r#""piSessionCache":null,"title":"#),
        "{written}"
    );
    Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO chat_sessions VALUES (?1, ?2, ?3, json_remove(?4, '$.piSessionCache'))",
            rusqlite::params![chat.id, chat.loupist_id, chat.updated_at_ms, written],
        )
        .unwrap();

    let store = SqliteStore::open(&path).unwrap();
    let stored: String = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE id = 'chat-cleared'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stored, written,
        "the key is back where serialization writes it"
    );
    let current = store.get_chat(&chat.id).unwrap().unwrap();
    let renamed = crate::domain::ChatSession {
        title: "Renamed".into(),
        updated_at_ms: current.updated_at_ms + 1,
        ..current.clone()
    };
    store.replace_chat(&current, &renamed).unwrap();
    // The baseline seed's minimal row has no `title` and is left as it was.
    let seed: String = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE id = 'chat-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(seed, r#"{"messages":[]}"#);
}

#[test]
fn the_upgrade_gives_an_earlier_settings_table_keep_awake_off() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE application_settings (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                notice_acknowledged_version INTEGER NOT NULL DEFAULT 0
                    CHECK(notice_acknowledged_version BETWEEN 0 AND 4294967295)
             ) STRICT;
             INSERT INTO application_settings (id, notice_acknowledged_version) VALUES (1, 1);",
        )
        .unwrap();

    let store = SqliteStore::open(&path).unwrap();
    assert!(!store.keep_awake_enabled().unwrap());
    assert_eq!(
        store.notice_settings().unwrap().notice_acknowledged_version,
        1
    );
    store.set_keep_awake(true).unwrap();
    assert!(store.keep_awake_enabled().unwrap());
    assert!(store
        .lock()
        .unwrap()
        .execute("UPDATE application_settings SET keep_awake = 2", [])
        .is_err());
}

#[test]
fn a_failed_step_leaves_the_database_unchanged_beside_its_backup() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, &unexpected_baseline());
    let original = Connection::open(&path).unwrap();
    let (before_rows, before_shape) = (rows(&original), shape(&original));
    drop(original);

    let error = SqliteStore::open(&path).err().expect("the upgrade fails");
    assert!(
        matches!(&error, StoreError::Upgrade(message) if message.contains("chat_handoffs")),
        "{error}"
    );

    let unchanged = Connection::open(&path).unwrap();
    assert_eq!(versions(&unchanged), (4, 6));
    assert_eq!(rows(&unchanged), before_rows);
    assert_eq!(shape(&unchanged), before_shape);
    let [backup] = backups(&directory).try_into().unwrap();
    assert!(error.to_string().contains(backup.to_str().unwrap()));
    assert_eq!(rows(&Connection::open(&backup).unwrap()), before_rows);
}

#[test]
fn a_successful_upgrade_keeps_its_backup_and_the_newest_prior_one() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    // A same-second attempt orders by its number, not its spelling.
    let oldest = prior_backup(&directory, "20010101T000000Z");
    let older = prior_backup(&directory, "20010102T000000Z-2");
    let newest = prior_backup(&directory, "20010102T000000Z-10");

    drop(SqliteStore::open(&path).unwrap());

    let backup = new_backup(&directory);
    let mut kept = backups(&directory);
    kept.sort();
    let mut expected = vec![newest.clone(), backup.clone()];
    expected.sort();
    assert_eq!(kept, expected);
    assert_eq!(kept.len(), crate::store::upgrade::KEPT_UPGRADE_BACKUPS);
    assert!(!oldest.exists() && !older.exists());
    assert_eq!(std::fs::read(&newest).unwrap(), b"20010102T000000Z-10");
    assert_eq!(versions(&Connection::open(&backup).unwrap()), (4, 6));
}

#[cfg(unix)]
#[test]
fn pruning_never_touches_a_symlink_or_a_name_it_did_not_write() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    // Newer than every candidate below, so any of them counted as a backup
    // would be the one deleted.
    let prior = prior_backup(&directory, "20010101T000000Z");
    let outside = temporary.path().join("outside.sqlite3");
    std::fs::write(&outside, "outside").unwrap();
    let link = directory.join("loupist.sqlite3.upgrade-backup-20000101T000000Z");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let folder = directory.join("loupist.sqlite3.upgrade-backup-20000102T000000Z");
    std::fs::create_dir(&folder).unwrap();
    let nested_directory = directory.join("nested");
    std::fs::create_dir(&nested_directory).unwrap();
    let nested = prior_backup(&nested_directory, "20000103T000000Z");
    let untouched = [
        "loupist.sqlite3.upgrade-backup-2000",
        "loupist.sqlite3.upgrade-backup-20000101T000000",
        "loupist.sqlite3.upgrade-backup-20000101-000000Z",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z.old",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-journal",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-0",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-01",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-101",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-1-1",
        "other.sqlite3.upgrade-backup-20000101T000000Z",
        "xloupist.sqlite3.upgrade-backup-20000101T000000Z",
        // A distinct date: on a case-insensitive volume this spelling would
        // name the symlink above.
        "loupist.sqlite3.upgrade-backup-20000104t000000z",
    ]
    .map(|name| {
        let path = directory.join(name);
        std::fs::write(&path, name).unwrap();
        path
    });

    drop(SqliteStore::open(&path).unwrap());

    new_backup(&directory);
    assert_eq!(std::fs::read(&prior).unwrap(), b"20010101T000000Z");
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
    assert!(folder.is_dir());
    assert!(nested.is_file());
    for path in untouched {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        assert_eq!(std::fs::read(&path).unwrap(), name.as_bytes(), "{name}");
    }
}

#[test]
fn a_failed_step_prunes_no_backup() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, &unexpected_baseline());
    let priors = ["20010101T000000Z", "20010102T000000Z", "20010103T000000Z"]
        .map(|stamp| prior_backup(&directory, stamp));

    assert!(matches!(
        SqliteStore::open(&path),
        Err(StoreError::Upgrade(_))
    ));

    new_backup(&directory);
    assert_eq!(backups(&directory).len(), priors.len() + 1);
    for prior in priors {
        assert!(prior.is_file(), "{}", prior.display());
    }
}

#[test]
fn newer_data_is_refused_and_left_untouched() {
    for newer in [
        format!("PRAGMA user_version = {};", STORE_SCHEMA_VERSION + 1),
        format!(
            "UPDATE knowledge_schema SET version = {};",
            crate::knowledge::KNOWLEDGE_SCHEMA_VERSION + 1
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("loupist.sqlite3");
        drop(SqliteStore::open(&path).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute_batch(&newer)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            matches!(SqliteStore::open(&path), Err(StoreError::NewerVersion)),
            "{newer}"
        );
        assert!(
            matches!(
                SqliteStore::open_catalog(&path),
                Err(StoreError::NewerVersion)
            ),
            "{newer}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{newer}");
        assert!(backups(temporary.path()).is_empty());
    }
}

#[test]
fn a_restored_baseline_round_trips_through_the_ladder() {
    // The folder tests rely on this test helper reproducing the baseline.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("loupist.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    let mut connection = Connection::open(&path).unwrap();
    let before = data(&connection);
    crate::store::upgrade::restore_baseline_for_test(&mut connection);
    let baseline = Connection::open_in_memory().unwrap();
    baseline.execute_batch(BASELINE_FIXTURE).unwrap();
    assert_eq!(shape(&connection), shape(&baseline));
    assert_eq!(versions(&connection), (4, 6));
    drop(connection);
    let upgraded = SqliteStore::open(&path).unwrap();
    assert_eq!(data(&upgraded.lock().unwrap()), before);
}

#[test]
fn store_five_adds_source_lookup_indexes_without_rewriting_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("loupist.sqlite3");
    let mut prior = Connection::open(&path).unwrap();
    prior.execute_batch(BASELINE_FIXTURE).unwrap();
    crate::store::upgrade::upgrade_store_for_test(&mut prior, 5);
    let mut before = data(&prior);
    for table in [
        "user_watchlist",
        "user_wallet_addresses",
        "user_holdings_snapshots",
        "user_holdings_positions",
    ] {
        assert!(before.remove(table).unwrap().is_empty());
    }
    drop(prior);
    let upgraded = SqliteStore::open(&path).unwrap();
    let connection = upgraded.lock().unwrap();
    assert_eq!(
        versions(&connection),
        (
            STORE_SCHEMA_VERSION,
            crate::knowledge::KNOWLEDGE_SCHEMA_VERSION
        )
    );
    let mut after = data(&connection);
    for table in [
        "assets",
        "knowledge_asset_line_readings",
        "knowledge_asset_price_points",
        "knowledge_saved_price_reads",
    ] {
        before.remove(table);
        after.remove(table);
    }
    // The catalog creates each Loupist's follows when it opens; none carry over.
    assert_eq!(after.remove("asset_follows"), Some(Vec::new()));
    assert_eq!(after.remove("scripts"), Some(Vec::new()));
    assert_eq!(after, before);
    for (table, column, index) in [
        ("knowledge_source_spans", "span_id", "knowledge_spans_by_id"),
        (
            "knowledge_source_representations",
            "representation_id",
            "knowledge_representations_by_id",
        ),
    ] {
        let plan: String = connection
            .query_row(
                &format!("EXPLAIN QUERY PLAN SELECT record_id FROM {table} WHERE {column}=?1"),
                ["missing"],
                |row| row.get(3),
            )
            .unwrap();
        assert!(plan.contains(index), "{plan}");
    }
    assert_eq!(backups(root.path()).len(), 1);
}

#[test]
fn held_identity_upgrade_keeps_an_existing_follows_name_and_follows_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let path = baseline_database(temporary.path(), BASELINE_FIXTURE);
    Connection::open(&path)
        .unwrap()
        .execute_batch("INSERT INTO user_watchlist VALUES ('ETH', 'CRYPTO', 'My Ether', 3);")
        .unwrap();
    let store = SqliteStore::open(&path).unwrap();
    let entries = store.assets_list("loupist-a", false).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(Some(entries[0].name.as_str()), Some("My Ether"));
    // The app-wide list belonged to no Loupist.
    assert_eq!(entries[0].followed_at_ms, None);
}

#[test]
fn held_identity_upgrade_keeps_only_the_latest_snapshot_of_a_source() {
    let temporary = tempfile::tempdir().unwrap();
    let path = baseline_database(temporary.path(), BASELINE_FIXTURE);
    Connection::open(&path).unwrap().execute_batch(
        "INSERT INTO user_holdings_snapshots VALUES ('snapshot-new', 'wallet', 'wallet-a', 9, 'USD', NULL);
         INSERT INTO user_holdings_positions VALUES ('snapshot-new', 'BTC', 'CRYPTO', 'Bitcoin', '1', NULL, NULL, 'USD');"
    ).unwrap();
    let store = SqliteStore::open(&path).unwrap();
    let entries = store.assets_list("loupist-a", false).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]
            .reference
            .as_ref()
            .unwrap()
            .fields
            .ticker
            .as_deref(),
        Some("BTC")
    );
    assert_eq!(Some(entries[0].name.as_str()), Some("Bitcoin"));
    assert_eq!(entries[0].followed_at_ms, None);
    drop(store);
    let before = rows(&Connection::open(&path).unwrap());
    drop(SqliteStore::open(&path).unwrap());
    assert_eq!(rows(&Connection::open(&path).unwrap()), before);
}

fn store_seven(directory: &Path, seed: &str) -> PathBuf {
    let path = directory.join("loupist.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    db.execute_batch(BASELINE_FIXTURE).unwrap();
    crate::store::upgrade::upgrade_store_for_test(&mut db, 7);
    db.execute_batch(seed).unwrap();
    assert_eq!(versions(&db).0, 7);
    path
}

#[test]
fn ownership_retirement_keeps_all_sources_fields_and_foreign_keys() {
    let root = tempfile::tempdir().unwrap();
    let path = store_seven(
        root.path(),
        r#"
        INSERT INTO loupist_profiles VALUES ('desk', 1, '/research/desk', NULL, NULL, '{}');
        INSERT INTO loupist_capability_bindings VALUES ('desk','onchain.wallets',2,'{}');
        INSERT INTO loupist_capability_bindings VALUES ('desk','defillama.tvl',3,'{}');
        INSERT INTO user_wallet_addresses VALUES ('unread','evm','0xunread',NULL,1,NULL,NULL,'unread',0);
        INSERT INTO user_wallet_addresses VALUES ('first','evm','0xfirst','First',2,3,3,'ok',1);
        INSERT INTO user_wallet_addresses VALUES ('second','bitcoin','bc1second','Second',4,5,5,'failed',2);
        INSERT INTO user_holdings_snapshots VALUES ('file','csv',NULL,10,'USD','exact file');
        INSERT INTO user_holdings_snapshots VALUES ('broker','kis',NULL,11,'KRW',NULL);
        INSERT INTO user_holdings_snapshots VALUES ('exchange','crypto_file',NULL,12,'','mixed');
        INSERT INTO user_holdings_snapshots VALUES ('empty','csv',NULL,13,'USD','empty');
        INSERT INTO user_holdings_snapshots VALUES ('wallet-first','wallet','first',14,'',NULL);
        INSERT INTO user_holdings_snapshots VALUES ('wallet-second','wallet','second',15,'BTC',NULL);
        INSERT INTO user_holdings_positions VALUES ('file','AAA','US','Alpha','1.230000','77.010','94.7223','USD');
        INSERT INTO user_holdings_positions VALUES ('broker','990001','KRX',NULL,'1337','36061.2566','48213900','KRW');
        INSERT INTO user_holdings_positions VALUES ('exchange','ETH','CRYPTO','Ether','0.4271000000',NULL,NULL,'');
        INSERT INTO user_holdings_positions VALUES ('wallet-first','ETH','CRYPTO',NULL,'3.14159265',NULL,NULL,'');
        INSERT INTO user_holdings_positions VALUES ('wallet-second','BTC','CRYPTO',NULL,'0.00000001',NULL,NULL,'BTC');
    "#,
    );
    let before = rows(&Connection::open(&path).unwrap());
    let store = SqliteStore::open(&path).unwrap();
    let db = store.lock().unwrap();
    let after = rows(&db);
    for (old, retired) in [
        ("user_wallet_addresses", "retired_wallet_addresses"),
        ("user_holdings_snapshots", "retired_holdings_snapshots"),
        ("user_holdings_positions", "retired_holdings_positions"),
    ] {
        assert!(!after.contains_key(old));
        assert_eq!(
            after.get(retired),
            before.get(old),
            "every field and rowid in {old}"
        );
    }
    let bindings: Vec<String> = db
        .prepare("SELECT entry_id FROM loupist_capability_bindings ORDER BY entry_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(bindings, ["defillama.tvl"]);
    assert_eq!(
        after["loupist_capability_bindings"],
        before["loupist_capability_bindings"]
            .iter()
            .filter(|row| row[2] != Value::Text("onchain.wallets".into()))
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE sql LIKE '%user_holdings_%' OR sql LIKE '%user_wallet_addresses%'", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
    assert_eq!(db.query_row("SELECT count(*) FROM retired_holdings_positions p JOIN retired_holdings_snapshots s USING(snapshot_id) LEFT JOIN retired_wallet_addresses w ON w.id=s.wallet_id", [], |r| r.get::<_,i64>(0)).unwrap(), 5);
    for (table, parent) in [
        ("retired_holdings_snapshots", "retired_wallet_addresses"),
        ("retired_holdings_positions", "retired_holdings_snapshots"),
    ] {
        let parents: Vec<String> = db
            .prepare(&format!("PRAGMA foreign_key_list({table})"))
            .unwrap()
            .query_map([], |r| r.get(2))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(parents, [parent]);
    }
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
    assert_eq!(versions(&db).0, STORE_SCHEMA_VERSION);
    // Admission's read-only upgrade copy accepts extra retired tables too.
    let copy = crate::store::upgrade::upgraded_copy(&db).unwrap();
    assert_eq!(rows(&copy), after);
    drop(db);
    drop(store);
    // A folder uses the same upgrade entry point, and a second opening is a no-op.
    let folder = SqliteStore::open_folder(&path).unwrap();
    assert_eq!(rows(&folder.lock().unwrap()), after);
}

#[test]
fn ownership_retirement_drops_only_empty_tables() {
    for seed in ["", "INSERT INTO user_wallet_addresses VALUES ('unread','evm','0xunread',NULL,1,NULL,NULL,'unread',0);"] {
        let root = tempfile::tempdir().unwrap();
        let path = store_seven(root.path(), seed);
        let store = SqliteStore::open(&path).unwrap();
        let actual = rows(&store.lock().unwrap());
        for table in ["wallet_addresses", "holdings_snapshots", "holdings_positions"] {
            assert!(!actual.contains_key(&format!("user_{table}")));
            assert_eq!(actual.contains_key(&format!("retired_{table}")), !seed.is_empty());
        }
    }
}

#[test]
fn ownership_retirement_runs_in_admission_copy_and_folder_open_without_losing_rows() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().canonicalize().unwrap();
    let path = store_seven(&directory, "");
    let original = Connection::open(&path).unwrap();
    let mut profile = crate::test_fixtures::loupist_profile("folder-a");
    profile.workspace_root = directory.display().to_string();
    original
        .execute(
            "INSERT INTO loupist_profiles VALUES (?1,?2,?3,NULL,NULL,?4)",
            rusqlite::params![
                profile.id,
                profile.updated_at_ms,
                profile.workspace_root,
                serde_json::to_string(&profile).unwrap()
            ],
        )
        .unwrap();
    original.execute_batch("INSERT INTO user_wallet_addresses VALUES ('wallet','evm','0xfixture',NULL,1,NULL,NULL,'unread',0);").unwrap();
    crate::knowledge::validate_folder(&directory).unwrap();
    let before = rows(&original);
    let copy = crate::store::upgrade::upgraded_copy(&original).unwrap();
    assert_eq!(versions(&copy).0, STORE_SCHEMA_VERSION);
    assert_eq!(
        rows(&original),
        before,
        "admission does not mutate the source"
    );
    let expected = rows(&copy);
    drop(original);
    let folder = SqliteStore::open_folder(&path).unwrap();
    assert_eq!(rows(&folder.lock().unwrap()), expected);
    crate::knowledge::validate_folder(&directory).unwrap();
}

fn store_ten(path: &Path) -> Connection {
    let mut db = Connection::open(path).unwrap();
    db.execute_batch(BASELINE_FIXTURE).unwrap();
    crate::store::upgrade::upgrade_store_for_test(&mut db, 10);
    let profile = crate::test_fixtures::loupist_profile("upgrade");
    db.execute(
        "INSERT INTO loupist_profiles VALUES (?1,1,?2,NULL,NULL,?3)",
        rusqlite::params![
            profile.id,
            profile.workspace_root,
            serde_json::to_string(&profile).unwrap()
        ],
    )
    .unwrap();
    db
}

fn chat_card_rows(db: &Connection) -> BTreeMap<String, String> {
    db.prepare("SELECT id, data_json FROM chat_sessions")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn card_retirement_preserves_message_bytes_across_catalog_folder_and_admission() {
    // These fragments use compact writer encoding. The objects inside
    // evidence and the prose contain the very same key spellings.
    let evidence = r#","evidenceRefs":[{"assetSnapshots":[1.25],"sensitivityCards":[-0.0,1e-7],"text":"\"quoted\" \\ path\n증거"}]"#;
    let current = r#"{"status":"complete","content":"[증거](#evidence-ev_one)\n\n\"assetSnapshots\"","artifactRefs":[{"revision":2}],"numberChecks":[{"link_index":1}]"#;
    let old_current = format!(
        r#"{current}{evidence},"sensitivityCards":[{{"values":[[1.75]]}}],"assetSnapshots":[{{"price":3.5}}]}}"#
    );
    let old_aborted = r#"{"status":"aborted","assetSnapshots":[{}],"content":"stopped"}"#;
    let old_error = r#"{"status":"error","sensitivityCards":[{}],"content":"failed"}"#;
    let aborted = r#"{"status":"aborted","content":"stopped"}"#;
    let error = r#"{"status":"error","content":"failed"}"#;
    let old_session = format!(
        r#"{{"versions":[{{"messages":[{old_aborted},{old_error}]}},{{}},{{"messages":[]}},{{"messages":[{old_current}]}}],"messages":[{{"content":"한글"}},{old_current}]}}"#
    );
    let expected_session = format!(
        r#"{{"versions":[{{"messages":[{aborted},{error}]}},{{}},{{"messages":[]}},{{"messages":[{current}{evidence}}}]}}],"messages":[{{"content":"한글"}},{current}{evidence}}}]}}"#
    );
    let old_bot = format!(r#"{{"messages":[{old_aborted},{old_error}]}}"#);
    let expected_bot = format!(r#"{{"messages":[{aborted},{error}]}}"#);
    for name in ["catalog.sqlite3", "loupist.sqlite3"] {
        let folder = name == "loupist.sqlite3";
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().canonicalize().unwrap();
        let path = directory.join(name);
        let db = store_ten(&path);
        let fixtures = [
            ("session", old_session.as_str(), expected_session.as_str()),
            ("home-upgrade", old_bot.as_str(), expected_bot.as_str()),
            (
                "no-messages",
                r#"{"assetSnapshots":[{}]}"#,
                r#"{"assetSnapshots":[{}]}"#,
            ),
            ("empty", "{ \"messages\": [] }", "{ \"messages\": [] }"),
            (
                "mixed-messages",
                r#"{"messages":["plain text",null,{"assetSnapshots":null,"content":"kept"}],"versions":[null,"plain text",{"messages":["archived text",{"sensitivityCards":null,"content":"archived"}]}]}"#,
                r#"{"messages":["plain text",null,{"content":"kept"}],"versions":[null,"plain text",{"messages":["archived text",{"content":"archived"}]}]}"#,
            ),
            ("unreadable", "{ unreadable", "{ unreadable"),
        ];
        let mut original = BTreeMap::new();
        let mut expected = BTreeMap::new();
        for (id, old, new) in fixtures {
            db.execute(
                "INSERT INTO chat_sessions VALUES (?1, 'upgrade', 2, ?2)",
                [id, old],
            )
            .unwrap();
            original.insert(id.to_owned(), old.to_owned());
            expected.insert(id.to_owned(), new.to_owned());
        }
        if folder {
            let bytes = std::fs::read(&path).unwrap();
            crate::knowledge::validate_folder(&directory).unwrap();
            let copy = crate::store::upgraded_copy(&db).unwrap();
            assert_eq!(
                versions(&copy),
                (STORE_SCHEMA_VERSION, KNOWLEDGE_SCHEMA_VERSION)
            );
            assert_eq!(chat_card_rows(&copy), expected);
            assert_eq!(versions(&db), (10, KNOWLEDGE_SCHEMA_VERSION));
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(backups(root.path()).is_empty());
        }
        drop(db);
        let open = || {
            if folder {
                SqliteStore::open_folder(&path)
            } else {
                SqliteStore::open_catalog(&path)
            }
            .unwrap()
        };
        for _ in 0..2 {
            let store = open();
            let db = store.lock().unwrap();
            assert_eq!(
                versions(&db),
                (STORE_SCHEMA_VERSION, KNOWLEDGE_SCHEMA_VERSION)
            );
            assert_eq!(chat_card_rows(&db), expected);
            assert_eq!(backups(root.path()).len(), 1);
        }
        let backup = Connection::open(&backups(root.path())[0]).unwrap();
        assert_eq!(versions(&backup), (10, KNOWLEDGE_SCHEMA_VERSION));
        assert_eq!(chat_card_rows(&backup), original);
    }
}

#[test]
fn a_failed_card_removal_leaves_store_ten_and_the_original_answer_in_the_backup() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("catalog.sqlite3");
    let db = store_ten(&path);
    db.execute_batch(
        r#"INSERT INTO chat_sessions VALUES ('answer', 'upgrade', 2, '{"messages":[{"sensitivityCards":[{}]}]}');
           CREATE TRIGGER fail_chat_upgrade AFTER UPDATE OF data_json ON chat_sessions
           BEGIN SELECT RAISE(FAIL, 'fixture failure after writing'); END;"#,
    )
    .unwrap();
    let original = chat_card_rows(&db);
    let error = SqliteStore::open_catalog(&path).err().unwrap();
    assert!(matches!(error, StoreError::Upgrade(_)));
    assert!(error.to_string().contains("fixture failure after writing"));
    assert_eq!(versions(&db), (10, KNOWLEDGE_SCHEMA_VERSION));
    assert_eq!(chat_card_rows(&db), original);
    let backup_path = &backups(root.path())[0];
    assert!(error.to_string().contains(backup_path.to_str().unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(backup_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let backup = Connection::open(backup_path).unwrap();
    assert_eq!(versions(&backup), (10, KNOWLEDGE_SCHEMA_VERSION));
    assert_eq!(chat_card_rows(&backup), original);
}

#[test]
fn thin_host_upgrade_keeps_theses_and_drops_host_observations_and_posts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("catalog.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    db.execute_batch(BASELINE_FIXTURE).unwrap();
    db.execute_batch(r#"
        INSERT INTO loupist_profiles VALUES('l',1,'/research/l',NULL,NULL,'{}');
        INSERT INTO user_watchlist VALUES('NVIDIA','US','Nvidia',7);
        INSERT INTO chat_sessions VALUES('c','l',2,'{"messages":[{"content":"watch","watch":{}},{"content":"retained reply"}],"versions":[{"messages":[{"content":"results","earnings":{}}]}]}');
    "#).unwrap();
    let asset_id = crate::knowledge::assets::asset_id("NVIDIA", "US");
    db.execute(
        "INSERT INTO knowledge_asset_pages (loupist_id,asset_id,symbol,market,display_name,class,class_source,created_at_ms,revision)
         VALUES ('l',?1,'NVIDIA','US','Nvidia','equity','market',7,1)",[&asset_id],
    ).unwrap();
    db.execute(
        "INSERT INTO knowledge_asset_moves (move_id,loupist_id,asset_id,from_verdict,to_verdict,fact_date,reason,evidence_json,actor,at_ms)
         VALUES ('move','l',?1,'holding','weakening','2026-03-01','Reasoned thesis move','[]','loupist',8)",[&asset_id],
    ).unwrap();
    crate::store::upgrade::upgrade_store_for_test(&mut db, 11);
    drop(db);
    let store = SqliteStore::open(&path).unwrap();
    let row = store.asset_by_id("l", &asset_id).unwrap().unwrap();
    assert_eq!(row.name, "Nvidia");
    // The page keeps its thesis; the app-wide follow belonged to no Loupist.
    assert_eq!(row.followed_at_ms, None);
    assert_eq!(
        row.reference.unwrap().fields.ticker.as_deref(),
        Some("NVIDIA")
    );
    let db = store.lock().unwrap();
    for table in [
        "instruments",
        "knowledge_asset_observations",
        "knowledge_watch_events",
        "knowledge_earnings_events",
    ] {
        assert!(!rows(&db).contains_key(table));
    }
    let moved: String = db
        .query_row(
            "SELECT reason FROM knowledge_asset_moves WHERE asset_id=?1",
            [&asset_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(moved, "Reasoned thesis move");
    let chat: String = db
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE id='c'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chat: serde_json::Value = serde_json::from_str(&chat).unwrap();
    assert_eq!(
        chat["messages"],
        serde_json::json!([{"content":"retained reply"}])
    );
    assert_eq!(chat["versions"][0]["messages"], serde_json::json!([]));
}

fn knowledge_eleven(directory: &Path) -> PathBuf {
    let path = baseline_database(directory, BASELINE_FIXTURE);
    let mut db = Connection::open(&path).unwrap();
    crate::store::upgrade::upgrade_ladders_for_test(&mut db, STORE_SCHEMA_VERSION, 11);
    db.execute_batch(crate::store::user_context::FOLLOWS_DDL)
        .unwrap();
    assert_eq!(versions(&db), (STORE_SCHEMA_VERSION, 11));
    path
}

fn without_schema_version(
    mut tables: BTreeMap<String, Vec<Vec<Value>>>,
) -> BTreeMap<String, Vec<Vec<Value>>> {
    tables.remove("knowledge_schema");
    tables
}

/// Opens `path`, which is at Knowledge 11, and checks the 11→12 step.
fn assert_knowledge_twelve(
    directory: &Path,
    path: &Path,
    before: &BTreeMap<String, Vec<Vec<Value>>>,
    skipped: u32,
    episodes: i64,
) {
    crate::store::upgrade::reset_knowledge_12_unreadable_for_test();
    drop(SqliteStore::open(path).unwrap());
    assert_eq!(
        crate::store::upgrade::knowledge_12_unreadable_for_test(),
        skipped,
        "unreadable rows are counted"
    );
    let upgraded = Connection::open(path).unwrap();
    assert_eq!(versions(&upgraded).1, 12, "schema version is 12");
    assert_eq!(versions(&upgraded).0, STORE_SCHEMA_VERSION);
    let mut after = rows(&upgraded);
    let episode_rows = after
        .remove("knowledge_world_episodes")
        .expect("episodes table");
    assert_eq!(episode_rows.len() as i64, episodes);
    assert_eq!(
        without_schema_version(after.clone()),
        without_schema_version(before.clone()),
        "row counts and contents are unchanged"
    );
    let backup_path = new_backup(directory);
    let backup = Connection::open(&backup_path).unwrap();
    assert_eq!(
        versions(&backup),
        (STORE_SCHEMA_VERSION, 11),
        "the backup is the database from before the step"
    );
    assert!(
        !rows(&backup).contains_key("knowledge_world_episodes"),
        "the backup was written before the episodes table existed"
    );
    assert_eq!(rows(&backup), *before);
    let settled = rows(&upgraded);
    drop(upgraded);
    crate::store::upgrade::reset_knowledge_12_unreadable_for_test();
    drop(SqliteStore::open(path).unwrap());
    assert_eq!(
        crate::store::upgrade::knowledge_12_unreadable_for_test(),
        0,
        "a second run does not read the step again"
    );
    assert_eq!(backups(directory).len(), 1);
    assert_eq!(rows(&Connection::open(path).unwrap()), settled);
}

#[test]
fn knowledge_eleven_to_twelve_keeps_the_baseline_profile_and_a_mixed_profile() {
    let baseline_root = tempfile::tempdir().unwrap();
    let baseline_dir = baseline_root.path().join("app-data");
    std::fs::create_dir(&baseline_dir).unwrap();
    let baseline = knowledge_eleven(&baseline_dir);
    let before = rows(&Connection::open(&baseline).unwrap());
    assert_knowledge_twelve(&baseline_dir, &baseline, &before, 0, 0);

    let mixed_root = tempfile::tempdir().unwrap();
    let mixed_dir = mixed_root.path().join("app-data");
    std::fs::create_dir(&mixed_dir).unwrap();
    let mixed = knowledge_eleven(&mixed_dir);
    let db = Connection::open(&mixed).unwrap();
    let indexes = {
        let mut statement = db
            .prepare(
                "SELECT name FROM sqlite_schema WHERE tbl_name='knowledge_graph_outbox' AND sql LIKE '%json_extract%'",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    for index in indexes {
        db.execute_batch(&format!("DROP INDEX {index}")).unwrap();
    }
    let world = format!("rec_{}", "a".repeat(32));
    let head = format!("rev_{}", "b".repeat(32));
    let sealed = format!("rev_{}", "c".repeat(32));
    let old_claim =
        r#"{"claims":[{"id":"clm_oldclaim","proposition":"The old claim has no episode"}]}"#;
    db.execute(
        "INSERT INTO knowledge_records (record_id, kind, owner_loupist_id, title, created_at_ms) VALUES (?1, 'world', 'loupist-a', 'Old World', 26)",
        [&world],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_record_revisions (revision_id, record_id, revision_digest, payload_json, dependency_json, created_at_ms) VALUES (?1, ?2, 'digest', ?3, '[]', 26)",
        rusqlite::params![head, world, old_claim],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_record_heads (record_id, revision_id) VALUES (?1, ?2)",
        rusqlite::params![world, head],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_world_snapshots (snapshot_id, record_id, revision_id, revision_digest, snapshot_json, created_at_ms) VALUES ('snp-old', ?1, ?2, 'digest', ?3, 26)",
        rusqlite::params![world, head, old_claim],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_asset_pages (loupist_id, asset_id, display_name, created_at_ms, revision) VALUES ('loupist-a', 'ast_0123456789abcdef', 'Northwind', 25, 1)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_journals (journal_id, research_id, loupist_id, body_json, input_digest, created_at_ms) VALUES ('jrn-old', 'research-old', 'loupist-a', '{\"entry\":\"kept journal\"}', 'digest', 21)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_connections (connection_id, loupist_id, seed_node_id, far_node_id, path_json, score, status, created_at_ms, status_at_ms, shown_at_ms) VALUES ('connection-old', 'loupist-a', 'n-old', 'n-far', '[]', 2, 'shown', 16, 16, 16)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO scheduled_tasks (id, loupist_id, revision, status, next_run_at_ms, updated_at_ms, data_json) VALUES ('task-old', 'loupist-a', 1, 'paused', NULL, 22, '{\"kind\":\"custom\",\"title\":\"Morning check\"}')",
        [],
    )
    .unwrap();
    let pending = r#"{"messages":[{"id":"msg-pending-save","role":"assistant","content":"Hold this save","memoryUpdate":{"status":"pending","runId":"run-pending"}}]}"#;
    db.execute(
        "INSERT INTO chat_sessions (id, loupist_id, updated_at_ms, data_json) VALUES ('chat-pending', 'loupist-a', 23, ?1)",
        [pending],
    )
    .unwrap();
    db.execute(
        "INSERT INTO chat_handoffs (id, loupist_id, request_key, chat_id, status, run_id, created_at_ms, data_json) VALUES ('handoff-pending', 'loupist-a', 'pending-save', 'chat-pending', 'running', 'run-pending', 24, '{\"pending\":true}')",
        [],
    )
    .unwrap();
    let binding = |n: char, seal: &str, kind: &str| {
        serde_json::json!({
            "episodeKind": kind,
            "sourceBinding": {
                "sourceId": format!("src_{}", n.to_string().repeat(32)),
                "captureId": format!("cap_{}", n.to_string().repeat(32)),
                "representationId": format!("rep_{}", n.to_string().repeat(32)),
                "spanId": format!("spn_{}", n.to_string().repeat(32)),
                "payloadDigest": "ab".repeat(32),
                "sourceRecordId": format!("rec_{}", n.to_string().repeat(32)),
                "sourceRevisionId": format!("rev_{}", n.to_string().repeat(32)),
            },
            "sealedRevisionId": seal,
        })
    };
    let readable = binding('d', &sealed, "source_span");
    let ignored = binding('e', &format!("rev_{}", "d".repeat(32)), "world_recovery");
    for (job, status, body) in [
        ("job-kept", "completed", readable.to_string()),
        ("job-ignored", "completed", ignored.to_string()),
        ("job-unreadable", "completed", "{malformed".into()),
    ] {
        db.execute(
            "INSERT INTO knowledge_graph_outbox (job_id, loupist_id, world_record_id, world_revision_id, status, payload_json, payload_digest, created_at_ms) VALUES (?1, 'loupist-a', ?2, ?3, ?4, ?5, 'digest', 30)",
            rusqlite::params![job, world, head, status, body],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary, publication_json) VALUES ('gev-kept', 'loupist-a', 31, 'claim_added', ?1, 'kept-claim', 'processing', 'Kept claim', ?2)",
        rusqlite::params![world, serde_json::json!({"revisionId": sealed}).to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, record_id, subject_id, actor, summary, publication_json) VALUES ('gev-unreadable', 'loupist-a', 32, 'claim_added', ?1, 'unread-claim', 'processing', 'Unreadable growth', '{not json')",
        [&world],
    )
    .unwrap();
    let before = rows(&db);
    drop(db);
    assert_knowledge_twelve(&mixed_dir, &mixed, &before, 2, 1);
    let upgraded = Connection::open(&mixed).unwrap();
    let changes: String = upgraded
        .query_row(
            "SELECT changes_json FROM knowledge_world_episodes",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let changes: serde_json::Value = serde_json::from_str(&changes).unwrap();
    assert_eq!(changes["created"], serde_json::json!(["kept-claim"]));
    assert!(changes["created"]
        .as_array()
        .unwrap()
        .iter()
        .all(|id| id != "clm_oldclaim" && id != "unread-claim"));
    let snapshot: String = upgraded
        .query_row(
            "SELECT snapshot_json FROM knowledge_world_snapshots WHERE snapshot_id = 'snp-old'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(snapshot, old_claim);
}
