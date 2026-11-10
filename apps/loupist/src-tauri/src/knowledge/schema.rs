#[cfg(test)]
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

#[cfg(test)]
use super::error::KnowledgeError;
use super::error::KnowledgeResult;
#[cfg(test)]
use crate::fs::artifact_trust::ensure_private_directory;

pub const KNOWLEDGE_SCHEMA_VERSION: i64 = 7;
pub const KNOWLEDGE_FORMAT_VERSION: u32 = 1;

const EXPECTED_TABLES: &[&str] = &[
    "knowledge_asset_moves",
    "knowledge_asset_observations",
    "knowledge_asset_pages",
    "knowledge_asset_section_deltas",
    "knowledge_blobs",
    "knowledge_capture_lineage",
    "knowledge_chat_origins",
    "knowledge_connections",
    "knowledge_earnings_events",
    "knowledge_earnings_notices",
    "knowledge_earnings_rows",
    "knowledge_earnings_tone",
    "knowledge_erasure",
    "knowledge_graph_outbox",
    "knowledge_growth_events",
    "knowledge_journals",
    "knowledge_learning_reviews",
    "knowledge_learning_settings",
    "knowledge_lens_anchors",
    "knowledge_lens_deliveries",
    "knowledge_lens_links",
    "knowledge_lens_log",
    "knowledge_lens_metadata",
    "knowledge_read_receipts",
    "knowledge_receipt_snapshots",
    "knowledge_record_heads",
    "knowledge_record_revisions",
    "knowledge_records",
    "knowledge_research_records",
    "knowledge_schema",
    "knowledge_schema_changes",
    "knowledge_schema_reviews",
    "knowledge_search_documents",
    "knowledge_source_captures",
    "knowledge_source_representations",
    "knowledge_source_spans",
    "knowledge_sources",
    "knowledge_turn_pins",
    "knowledge_turns",
    "knowledge_watch_assets",
    "knowledge_watch_events",
    "knowledge_watch_routes",
    "knowledge_watch_settings",
    "knowledge_watch_state",
    "knowledge_world_deliveries",
    "knowledge_world_snapshots",
];

const IMMUTABLE_TABLES: &[&str] = &[
    "knowledge_growth_events",
    "knowledge_blobs",
    "knowledge_capture_lineage",
    "knowledge_lens_deliveries",
    "knowledge_lens_log",
    "knowledge_record_revisions",
    "knowledge_records",
    "knowledge_read_receipts",
    "knowledge_research_records",
    "knowledge_source_captures",
    "knowledge_source_representations",
    "knowledge_source_spans",
    "knowledge_turns",
    "knowledge_world_deliveries",
    "knowledge_world_snapshots",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SchemaState {
    Absent,
    Current,
    Unsupported(i64),
    Incomplete,
}

pub(super) fn inspect_knowledge(
    connection: &Connection,
    tables: &[String],
) -> KnowledgeResult<SchemaState> {
    let knowledge: Vec<&String> = tables
        .iter()
        .filter(|name| name.starts_with("knowledge_"))
        .collect();
    if knowledge.is_empty() {
        return Ok(SchemaState::Absent);
    }
    let mut found: Vec<&str> = knowledge.iter().map(|name| name.as_str()).collect();
    found.sort_unstable();
    let version = knowledge_version(connection)?;
    if found.as_slice() != EXPECTED_TABLES {
        return Ok(match version {
            Some(found_version) if found_version != KNOWLEDGE_SCHEMA_VERSION => {
                SchemaState::Unsupported(found_version)
            }
            _ => SchemaState::Incomplete,
        });
    }
    Ok(match version {
        Some(KNOWLEDGE_SCHEMA_VERSION) => SchemaState::Current,
        Some(found_version) => SchemaState::Unsupported(found_version),
        None => SchemaState::Incomplete,
    })
}

#[cfg(test)]
pub(super) fn blob_root_for(db_path: &Path) -> KnowledgeResult<PathBuf> {
    let canonical = canonical_leaf(db_path)?;
    Ok(canonical
        .parent()
        .ok_or(KnowledgeError::Invalid("sqlite path has no parent"))?
        .join("knowledge-blobs"))
}

#[cfg(test)]
fn canonical_leaf(path: &Path) -> KnowledgeResult<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(KnowledgeError::Invalid("sqlite path has no parent"))?;
    let file_name = path
        .file_name()
        .ok_or(KnowledgeError::Invalid("sqlite path has no file name"))?;
    if !parent.exists() {
        ensure_private_directory(parent)?;
    }
    Ok(parent.canonicalize()?.join(file_name))
}

fn knowledge_version(connection: &Connection) -> KnowledgeResult<Option<i64>> {
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

fn schema_sql() -> String {
    let mut sql = String::from(
        r#"
        CREATE TABLE knowledge_schema (
            id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
            version INTEGER NOT NULL
        ) STRICT;
        CREATE TABLE knowledge_growth_events (
            rationale TEXT CHECK(rationale IS NULL OR length(rationale)<=400),
            origin TEXT,
            event_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
            kind TEXT NOT NULL,
            record_id TEXT,
            subject_id TEXT,
            origin_chat_id TEXT,
            origin_message_id TEXT,
            actor TEXT NOT NULL,
            summary TEXT NOT NULL CHECK (length(summary) <= 240 AND instr(summary, char(10)) = 0 AND instr(summary, char(13)) = 0),
            before_summary TEXT,
            after_summary TEXT
        ) STRICT;
        CREATE INDEX knowledge_growth_events_loupist_time ON knowledge_growth_events (loupist_id, at_ms, event_id);
        CREATE TABLE knowledge_learning_settings (
            loupist_id TEXT PRIMARY KEY NOT NULL, learn_methods INTEGER NOT NULL CHECK(learn_methods IN (0,1))
        ) STRICT;
        CREATE TABLE knowledge_learning_reviews (
            review_id TEXT PRIMARY KEY NOT NULL, loupist_id TEXT NOT NULL,
            world_job_id TEXT NOT NULL, status TEXT NOT NULL,
            reason TEXT, lens_record_id TEXT, rationale TEXT,
            evidence_count INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL, started_at_ms INTEGER, finished_at_ms INTEGER,
            through_event INTEGER NOT NULL, input_json TEXT, settings_json TEXT NOT NULL,
            usage_json TEXT, output_json TEXT,
            call_budget INTEGER NOT NULL CHECK(call_budget BETWEEN 0 AND 1)
        ) STRICT;
        CREATE UNIQUE INDEX knowledge_learning_active ON knowledge_learning_reviews(loupist_id) WHERE status IN ('pending','queued','running');
        CREATE INDEX knowledge_learning_history ON knowledge_learning_reviews(loupist_id,created_at_ms);
        CREATE TABLE knowledge_schema_changes (
            change_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            before_json TEXT NOT NULL,
            after_json TEXT NOT NULL,
            producer TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            origin_chat_id TEXT,
            origin_message_id TEXT,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE INDEX knowledge_schema_changes_loupist ON knowledge_schema_changes (loupist_id, created_at_ms, change_id);
        CREATE TABLE knowledge_schema_reviews (
            loupist_id TEXT PRIMARY KEY NOT NULL,
            reviewed_at_ms INTEGER NOT NULL CHECK (reviewed_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_records (
            record_id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('source', 'world', 'lens')),
            owner_loupist_id TEXT NOT NULL,
            title TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_record_revisions (
            revision_id TEXT PRIMARY KEY NOT NULL,
            record_id TEXT NOT NULL UNIQUE REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            revision_digest TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            dependency_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_record_heads (
            record_id TEXT PRIMARY KEY NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            revision_id TEXT NOT NULL REFERENCES knowledge_record_revisions(revision_id) ON DELETE RESTRICT
        ) STRICT;
        CREATE TABLE knowledge_sources (
            record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            source_id TEXT NOT NULL,
            title TEXT NOT NULL,
            source_class TEXT NOT NULL,
            origin_locator TEXT,
            provider_identity TEXT NOT NULL,
            origin_json TEXT,
            updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
            PRIMARY KEY (record_id, source_id)
        ) STRICT;
        CREATE TABLE knowledge_source_captures (
            record_id TEXT NOT NULL,
            capture_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            received_at_ms INTEGER NOT NULL CHECK (received_at_ms >= 0),
            coverage TEXT NOT NULL CHECK (coverage IN (
                'included_full', 'included_excerpt', 'upstream_truncated', 'metadata_only', 'omitted'
            )),
            declared_digest TEXT,
            material_digest TEXT,
            authorship TEXT NOT NULL CHECK (authorship IN (
                'publisher', 'user', 'tool', 'analysis', 'imported_contributor'
            )),
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
            publication_time_json TEXT,
            acquisition_identity TEXT,
            origin_locator TEXT NOT NULL,
            provider_identity TEXT NOT NULL,
            PRIMARY KEY (record_id, capture_id),
            FOREIGN KEY (record_id, source_id)
                REFERENCES knowledge_sources(record_id, source_id) ON DELETE RESTRICT,
            CHECK ((coverage IN ('metadata_only', 'omitted')) = (material_digest IS NULL))
        ) STRICT;
        CREATE TABLE knowledge_source_representations (
            record_id TEXT NOT NULL,
            representation_id TEXT NOT NULL,
            capture_id TEXT NOT NULL,
            parent_representation_id TEXT,
            document_id TEXT NOT NULL,
            digest TEXT NOT NULL,
            media_type TEXT NOT NULL,
            byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
            role TEXT NOT NULL CHECK (role IN (
                'original', 'provider_payload', 'extracted_text', 'structured', 'excerpt', 'derived'
            )),
            parser_version TEXT,
            input_spans_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
            PRIMARY KEY (record_id, representation_id),
            FOREIGN KEY (record_id, capture_id)
                REFERENCES knowledge_source_captures(record_id, capture_id) ON DELETE RESTRICT,
            FOREIGN KEY (record_id, parent_representation_id)
                REFERENCES knowledge_source_representations(record_id, representation_id) ON DELETE RESTRICT
        ) STRICT;
        CREATE TABLE knowledge_source_spans (
            record_id TEXT NOT NULL,
            span_id TEXT NOT NULL,
            capture_id TEXT NOT NULL,
            representation_id TEXT NOT NULL,
            selector_json TEXT NOT NULL,
            selected_digest TEXT NOT NULL,
            context_note TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
            PRIMARY KEY (record_id, span_id),
            FOREIGN KEY (record_id, capture_id)
                REFERENCES knowledge_source_captures(record_id, capture_id) ON DELETE RESTRICT,
            FOREIGN KEY (record_id, representation_id)
                REFERENCES knowledge_source_representations(record_id, representation_id) ON DELETE RESTRICT
        ) STRICT;
        CREATE TABLE knowledge_capture_lineage (
            record_id TEXT NOT NULL,
            capture_id TEXT NOT NULL,
            chat_id TEXT,
            request_ref TEXT,
            private_locator TEXT,
            PRIMARY KEY (record_id, capture_id),
            FOREIGN KEY (record_id, capture_id)
                REFERENCES knowledge_source_captures(record_id, capture_id) ON DELETE RESTRICT
        ) STRICT;
        CREATE TABLE knowledge_read_receipts (
            receipt_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            record_id TEXT NOT NULL,
            span_id TEXT NOT NULL,
            capture_id TEXT NOT NULL,
            representation_id TEXT NOT NULL,
            selected_digest TEXT NOT NULL,
            purpose TEXT NOT NULL CHECK (purpose IN ('renderer_inspection', 'chat_turn')),
            chat_session_id TEXT,
            turn_id TEXT,
            issued_at_ms INTEGER NOT NULL CHECK (issued_at_ms >= 0),
            FOREIGN KEY (record_id, span_id)
                REFERENCES knowledge_source_spans(record_id, span_id) ON DELETE RESTRICT,
            FOREIGN KEY (record_id, capture_id)
                REFERENCES knowledge_source_captures(record_id, capture_id) ON DELETE RESTRICT,
            FOREIGN KEY (record_id, representation_id)
                REFERENCES knowledge_source_representations(record_id, representation_id) ON DELETE RESTRICT,
            CHECK (
                (purpose = 'renderer_inspection' AND chat_session_id IS NULL AND turn_id IS NULL)
                OR (purpose = 'chat_turn' AND chat_session_id IS NOT NULL AND turn_id IS NOT NULL)
            )
        ) STRICT;
        CREATE TABLE knowledge_turns (
            turn_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            chat_session_id TEXT NOT NULL,
            opened_at_ms INTEGER NOT NULL CHECK (opened_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_turn_pins (
            loupist_id TEXT NOT NULL,
            chat_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            pin_index INTEGER NOT NULL CHECK (pin_index >= 0),
            pin_json TEXT NOT NULL,
            PRIMARY KEY (loupist_id, chat_id, turn_id, pin_index)
        ) STRICT;
        CREATE TABLE knowledge_erasure (
            id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
            armed INTEGER NOT NULL CHECK (armed IN (0, 1))
        ) STRICT;
        INSERT INTO knowledge_erasure (id, armed) VALUES (1, 0);
        CREATE TABLE knowledge_world_deliveries (
            delivery_id TEXT PRIMARY KEY NOT NULL,
            turn_id TEXT NOT NULL REFERENCES knowledge_turns(turn_id) ON DELETE RESTRICT,
            loupist_id TEXT NOT NULL,
            record_id TEXT NOT NULL,
            revision_id TEXT NOT NULL,
            revision_digest TEXT NOT NULL,
            view_json TEXT NOT NULL,
            view_digest TEXT NOT NULL,
            issued_at_ms INTEGER NOT NULL CHECK (issued_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_lens_deliveries (
            delivery_id TEXT PRIMARY KEY NOT NULL,
            turn_id TEXT NOT NULL REFERENCES knowledge_turns(turn_id) ON DELETE RESTRICT,
            loupist_id TEXT NOT NULL,
            record_id TEXT NOT NULL,
            revision_id TEXT NOT NULL,
            revision_digest TEXT NOT NULL,
            document_json TEXT NOT NULL,
            body_digest TEXT NOT NULL,
            issued_at_ms INTEGER NOT NULL CHECK (issued_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_blobs (
            digest TEXT PRIMARY KEY NOT NULL,
            byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
            media_type TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_world_snapshots (
            snapshot_id TEXT PRIMARY KEY NOT NULL,
            record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            revision_id TEXT NOT NULL UNIQUE REFERENCES knowledge_record_revisions(revision_id) ON DELETE RESTRICT,
            revision_digest TEXT NOT NULL,
            snapshot_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_lens_metadata (
            record_id TEXT PRIMARY KEY NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            loupist_id TEXT NOT NULL,
            title TEXT NOT NULL,
            title_fold TEXT NOT NULL,
            kind TEXT NOT NULL,
            principle_statement_fold TEXT,
            archived_at_ms INTEGER,
            learned INTEGER NOT NULL DEFAULT 0 CHECK (learned IN (0, 1)),
            updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
        ) STRICT;
        CREATE UNIQUE INDEX knowledge_lens_active_title
            ON knowledge_lens_metadata(loupist_id, kind, title_fold)
            WHERE archived_at_ms IS NULL AND kind <> 'principle';
        CREATE UNIQUE INDEX knowledge_principle_active_statement
            ON knowledge_lens_metadata(loupist_id, principle_statement_fold)
            WHERE archived_at_ms IS NULL AND kind = 'principle';
        CREATE UNIQUE INDEX knowledge_lens_one_perspective
            ON knowledge_lens_metadata(loupist_id)
            WHERE archived_at_ms IS NULL AND kind = 'perspective';
        CREATE TABLE knowledge_lens_log (
            op_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            op_json TEXT NOT NULL,
            outcome TEXT NOT NULL,
            section_key TEXT,
            before_digest TEXT,
            after_digest TEXT,
            before_json TEXT,
            origin TEXT NOT NULL,
            chat_id TEXT,
            message_id TEXT,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
            undoes_op_id TEXT
        ) STRICT;
        CREATE INDEX knowledge_lens_log_by_record
            ON knowledge_lens_log(record_id, created_at_ms, op_id);
        CREATE INDEX knowledge_lens_log_by_message
            ON knowledge_lens_log(loupist_id, message_id, created_at_ms, op_id);
        CREATE TABLE knowledge_lens_anchors (
            loupist_id TEXT NOT NULL,
            record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            role TEXT NOT NULL,
            node_id TEXT,
            name_fold TEXT NOT NULL DEFAULT '',
            turn_id TEXT,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE UNIQUE INDEX knowledge_lens_anchors_identity
            ON knowledge_lens_anchors(record_id, role, COALESCE(node_id, ''), name_fold, COALESCE(turn_id, ''));
        CREATE INDEX knowledge_lens_anchors_by_node
            ON knowledge_lens_anchors(loupist_id, node_id, role);
        CREATE TABLE knowledge_connections (
            connection_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            seed_node_id TEXT NOT NULL,
            far_node_id TEXT NOT NULL,
            path_json TEXT NOT NULL,
            score INTEGER NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
            status_at_ms INTEGER NOT NULL CHECK (status_at_ms >= 0),
            shown_at_ms INTEGER CHECK (shown_at_ms IS NULL OR shown_at_ms >= 0)
        ) STRICT;
        CREATE UNIQUE INDEX knowledge_connections_pair
            ON knowledge_connections(loupist_id, seed_node_id, far_node_id);
        CREATE INDEX knowledge_connections_by_status
            ON knowledge_connections(loupist_id, status, status_at_ms);
        CREATE TABLE knowledge_lens_links (
            loupist_id TEXT NOT NULL,
            source_record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
            target_record_id TEXT NOT NULL,
            PRIMARY KEY (source_record_id, target_record_id)
        ) STRICT;
        CREATE INDEX knowledge_lens_links_by_target
            ON knowledge_lens_links(loupist_id, target_record_id);
        CREATE TABLE knowledge_research_records (
            research_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            origin_chat_id TEXT,
            body_json TEXT NOT NULL,
            record_digest TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_journals (
            journal_id TEXT PRIMARY KEY NOT NULL,
            research_id TEXT NOT NULL,
            loupist_id TEXT NOT NULL,
            body_json TEXT NOT NULL,
            input_digest TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_chat_origins (
            loupist_id TEXT NOT NULL,
            chat_id TEXT NOT NULL,
            record_id TEXT NOT NULL,
            capture_id TEXT NOT NULL,
            PRIMARY KEY (loupist_id, chat_id, record_id, capture_id),
            FOREIGN KEY (record_id, capture_id)
                REFERENCES knowledge_source_captures(record_id, capture_id) ON DELETE RESTRICT
        ) STRICT;
        CREATE TABLE knowledge_graph_outbox (
            job_id TEXT PRIMARY KEY NOT NULL,
            loupist_id TEXT NOT NULL,
            world_record_id TEXT NOT NULL,
            world_revision_id TEXT NOT NULL,
            status TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            payload_digest TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
        ) STRICT;
        CREATE TABLE knowledge_search_documents (
            record_id TEXT NOT NULL,
            revision_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            span_id TEXT NOT NULL DEFAULT '',
            object_id TEXT NOT NULL DEFAULT '',
            coverage TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            aliases TEXT NOT NULL DEFAULT '',
            search_text TEXT NOT NULL,
            PRIMARY KEY (record_id, revision_id, kind, span_id, object_id)
        ) STRICT;
        CREATE INDEX knowledge_search_by_module
            ON knowledge_search_documents(record_id, revision_id);
        CREATE UNIQUE INDEX knowledge_source_acquisition_identity
            ON knowledge_source_captures(acquisition_identity)
            WHERE acquisition_identity IS NOT NULL;
        CREATE INDEX knowledge_graph_jobs_by_scope_status
            ON knowledge_graph_outbox(loupist_id, world_record_id, status, created_at_ms);
        CREATE INDEX knowledge_graph_jobs_by_source
            ON knowledge_graph_outbox(loupist_id, json_extract(payload_json, '$.sourceBinding.spanId'), created_at_ms DESC);
        CREATE INDEX knowledge_graph_jobs_by_source_record
            ON knowledge_graph_outbox(loupist_id, json_extract(payload_json, '$.sourceBinding.sourceRecordId'), created_at_ms DESC, job_id DESC);
        CREATE INDEX knowledge_graph_jobs_by_world_status
            ON knowledge_graph_outbox(world_record_id, status);
        CREATE INDEX knowledge_graph_jobs_queue ON knowledge_graph_outbox(status, created_at_ms, job_id);
        CREATE INDEX knowledge_graph_jobs_dedup ON knowledge_graph_outbox(loupist_id, world_record_id, payload_digest, created_at_ms DESC);
        CREATE INDEX knowledge_records_by_owner_kind ON knowledge_records(owner_loupist_id, kind);
        CREATE UNIQUE INDEX knowledge_one_world_per_loupist ON knowledge_records(owner_loupist_id) WHERE kind = 'world';
        CREATE UNIQUE INDEX knowledge_source_per_record ON knowledge_sources(record_id);
        CREATE INDEX knowledge_captures_by_source ON knowledge_source_captures(source_id, capture_id);
        CREATE INDEX knowledge_representations_by_capture
            ON knowledge_source_representations(capture_id, representation_id);
        CREATE INDEX knowledge_spans_by_capture ON knowledge_source_spans(capture_id, span_id);
        CREATE INDEX knowledge_receipts_by_capture ON knowledge_read_receipts(capture_id, receipt_id);
        CREATE UNIQUE INDEX knowledge_current_note_by_chat
            ON knowledge_research_records(loupist_id, origin_chat_id)
            WHERE json_type(body_json, '$.workingNote') = 'object';
        CREATE INDEX knowledge_journals_by_loupist ON knowledge_journals(loupist_id, journal_id);
        CREATE INDEX knowledge_journal_due ON knowledge_journals(loupist_id, json_extract(body_json, '$.check.byDate'), created_at_ms, journal_id);
        CREATE INDEX knowledge_chat_origins_by_capture ON knowledge_chat_origins(capture_id);
        "#,
    );
    sql.push_str(super::assets::DDL);
    sql.push_str(super::earnings::DDL);
    sql.push_str(super::watch::DDL);
    sql.push_str(&format!(
        "INSERT INTO knowledge_schema (id, version) VALUES (1, {KNOWLEDGE_SCHEMA_VERSION});"
    ));
    for table in IMMUTABLE_TABLES {
        sql.push_str(&format!(
            "CREATE TRIGGER {table}_no_update BEFORE UPDATE ON {table}
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
             CREATE TRIGGER {table}_no_delete BEFORE DELETE ON {table}
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;"
        ));
    }
    sql.push_str(
        "CREATE TABLE knowledge_receipt_snapshots (
            receipt_id TEXT PRIMARY KEY NOT NULL,
            revision_id TEXT NOT NULL,
            revision_digest TEXT NOT NULL,
            attribution TEXT NOT NULL,
            coverage TEXT NOT NULL,
            publication_time_json TEXT,
            media_type TEXT NOT NULL,
            selected_bytes BLOB NOT NULL
         );
         CREATE TRIGGER knowledge_receipt_snapshots_no_update
         BEFORE UPDATE ON knowledge_receipt_snapshots
         BEGIN SELECT RAISE(ABORT, 'immutable'); END;
         CREATE TRIGGER knowledge_receipt_snapshots_no_delete
         BEFORE DELETE ON knowledge_receipt_snapshots
         BEGIN
           SELECT RAISE(ABORT, 'immutable')
           WHERE NOT EXISTS (
             SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
           );
         END;",
    );
    sql
}

/// Knowledge DDL without its own transaction, for the app database initializer.
pub fn embedded_ddl() -> String {
    schema_sql()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_five_folder_is_an_earlier_build() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(&schema_sql()).unwrap();
        let tables = |connection: &Connection| {
            let mut statement = connection
                .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            inspect_knowledge(&connection, &tables(&connection)).unwrap(),
            SchemaState::Current
        );
        assert_eq!(
            crate::store::schema_standing(&connection).unwrap(),
            crate::store::Standing::Current
        );
        connection
            .execute_batch(
                "DROP TABLE knowledge_lens_anchors; DROP TABLE knowledge_lens_log;
                 CREATE TABLE knowledge_lens_changes (change_id TEXT PRIMARY KEY NOT NULL) STRICT;
                 CREATE TABLE knowledge_lens_drafts (draft_id TEXT PRIMARY KEY NOT NULL) STRICT;
                 CREATE TABLE knowledge_lens_applications (loupist_id TEXT NOT NULL) STRICT;
                 CREATE TABLE knowledge_standing_selection (loupist_id TEXT PRIMARY KEY NOT NULL);
                 UPDATE knowledge_schema SET version = 5;",
            )
            .unwrap();
        assert_eq!(
            inspect_knowledge(&connection, &tables(&connection)).unwrap(),
            SchemaState::Unsupported(5)
        );
        assert_eq!(
            crate::store::schema_standing(&connection).unwrap(),
            crate::store::Standing::Earlier
        );
        connection
            .execute(
                "UPDATE knowledge_schema SET version = ?1",
                [KNOWLEDGE_SCHEMA_VERSION + 1],
            )
            .unwrap();
        assert_eq!(
            crate::store::schema_standing(&connection).unwrap(),
            crate::store::Standing::Newer
        );
    }
}
