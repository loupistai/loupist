-- Frozen baseline of the store schema 4 and Knowledge schema 6 (the first
-- formats the upgrade ladder accepts). Generated from a fresh database at
-- 06e93b0; never edit it. Ladder tests replay it, then upgrade.
CREATE TABLE user_watchlist (
                symbol TEXT NOT NULL, market TEXT NOT NULL, display_name TEXT,
                added_at_ms INTEGER NOT NULL, PRIMARY KEY(symbol, market)
            ) STRICT;
CREATE TABLE user_wallet_addresses (
                id TEXT PRIMARY KEY NOT NULL,
                family TEXT NOT NULL CHECK(family IN ('bitcoin','evm','solana')),
                address TEXT NOT NULL,
                name TEXT,
                added_at_ms INTEGER NOT NULL,
                last_read_at_ms INTEGER,
                last_good_read_at_ms INTEGER,
                status TEXT NOT NULL CHECK(status IN ('unread','ok','failed')),
                unlisted_count INTEGER NOT NULL DEFAULT 0 CHECK(unlisted_count >= 0),
                UNIQUE(family, address)
            ) STRICT;
CREATE TABLE user_holdings_snapshots (
                snapshot_id TEXT PRIMARY KEY NOT NULL,
                source TEXT NOT NULL CHECK(source IN ('csv','kis','crypto_file','wallet')),
                wallet_id TEXT REFERENCES user_wallet_addresses(id) ON DELETE CASCADE,
                imported_at_ms INTEGER NOT NULL, currency TEXT NOT NULL, note TEXT,
                CHECK((source = 'wallet') = (wallet_id IS NOT NULL))
            ) STRICT;
CREATE TABLE user_holdings_positions (
                snapshot_id TEXT NOT NULL REFERENCES user_holdings_snapshots(snapshot_id) ON DELETE CASCADE,
                symbol TEXT NOT NULL, market TEXT NOT NULL, display_name TEXT,
                quantity TEXT NOT NULL, cost_basis TEXT, market_value TEXT, currency TEXT NOT NULL,
                PRIMARY KEY(snapshot_id, symbol, market)
            ) STRICT;
CREATE TABLE app_settings (
                id TEXT PRIMARY KEY NOT NULL,
                data_json TEXT NOT NULL,
                CHECK (id IN ('model', 'model_visibility', 'update')
                    OR (substr(id, 1, 17) = 'world_processing:' AND length(id) > 17))
            ) STRICT;
CREATE TABLE deletion_journals (
                id TEXT PRIMARY KEY NOT NULL,
                loupist_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                UNIQUE (loupist_id, target_id)
            ) STRICT;
CREATE TABLE loupist_profiles (
                id TEXT PRIMARY KEY NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                workspace_root TEXT NOT NULL UNIQUE,
                root_device TEXT,
                root_inode TEXT,
                data_json TEXT NOT NULL,
                CHECK ((root_device IS NULL) = (root_inode IS NULL))
            ) STRICT;
CREATE TABLE loupist_capability_bindings (
                loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
                entry_id TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                PRIMARY KEY (loupist_id, entry_id)
            ) STRICT;
CREATE TABLE chat_sessions (
                id TEXT PRIMARY KEY NOT NULL,
                loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL
            ) STRICT;
CREATE TABLE chat_artifacts (
                id TEXT PRIMARY KEY NOT NULL,
                chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE RESTRICT,
                kind TEXT NOT NULL CHECK (kind IN ('markdown', 'chart', 'grid')),
                title TEXT NOT NULL,
                current_revision INTEGER NOT NULL CHECK (current_revision > 0),
                created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                current_content_json TEXT NOT NULL
            ) STRICT;
CREATE TABLE chart_artifact_datasets (
                id TEXT PRIMARY KEY NOT NULL,
                artifact_id TEXT NOT NULL UNIQUE REFERENCES chat_artifacts(id) ON DELETE RESTRICT,
                digest TEXT NOT NULL,
                data_json TEXT NOT NULL
            ) STRICT;
CREATE INDEX deletion_journals_by_updated_at
                ON deletion_journals(updated_at_ms ASC, id ASC);
CREATE INDEX chat_sessions_by_loupist
                ON chat_sessions(loupist_id, updated_at_ms DESC);
CREATE UNIQUE INDEX loupist_profiles_by_root_identity
                ON loupist_profiles(root_device, root_inode)
                WHERE root_device IS NOT NULL AND root_inode IS NOT NULL;
CREATE INDEX chat_artifacts_by_session
                ON chat_artifacts(chat_session_id, updated_at_ms DESC);
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
            kind TEXT NOT NULL CHECK (kind IN ('entity_added', 'claim_added', 'claim_disputed', 'claim_withdrawn', 'lens_created', 'lens_revised', 'lens_renamed', 'lens_archived', 'lens_restored', 'lens_contested', 'source_retained', 'journal_created', 'journal_updated', 'journal_archived', 'journal_restored', 'journal_reviewed', 'schema_extended', 'schema_reduced', 'thesis_moved', 'growth_truncated')),
            record_id TEXT,
            subject_id TEXT,
            origin_chat_id TEXT,
            origin_message_id TEXT,
            actor TEXT NOT NULL CHECK (actor IN ('user', 'loupist', 'import', 'processing')),
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
            world_job_id TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','queued','running','created','refined','deferred','failed')),
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
            producer TEXT NOT NULL CHECK (producer IN ('preset', 'chat', 'review', 'undo')),
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
            kind TEXT NOT NULL CHECK (kind IN ('principle', 'perspective', 'method', 'subject', 'idea')),
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
            origin TEXT NOT NULL CHECK (origin IN ('turn', 'user', 'learning', 'import')),
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
            role TEXT NOT NULL CHECK (role IN ('subject', 'mention', 'applied')),
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
            status TEXT NOT NULL CHECK (status IN ('new', 'shown', 'opened', 'dismissed', 'filed')),
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
            kind TEXT NOT NULL CHECK (kind IN ('source', 'lens', 'world')),
            span_id TEXT NOT NULL DEFAULT '',
            object_id TEXT NOT NULL DEFAULT '',
            coverage TEXT NOT NULL CHECK (coverage IN ('included_full', 'metadata_only', 'snippet')),
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
CREATE TABLE knowledge_asset_pages (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    market TEXT NOT NULL,
    display_name TEXT NOT NULL,
    class TEXT NOT NULL CHECK (class IN ('equity','crypto','fund','commodity','rates_fx','real_asset','prediction','private')),
    class_source TEXT NOT NULL CHECK (class_source IN ('user','market','lookup','world','default')),
    entity_id TEXT,
    verdict TEXT CHECK (verdict IS NULL OR verdict IN ('holding','weakening','challenged','broken')),
    verdict_move_id TEXT,
    refresh_json TEXT NOT NULL DEFAULT '{}',
    refreshed_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
CREATE TABLE knowledge_asset_observations (
    observation_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('filing','results','earnings','dividend','close','holdings','fees','positioning','line','insiders','holders','flows','capital','investor_flows')),
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    recorded_at_ms INTEGER NOT NULL CHECK (recorded_at_ms >= 0),
    dedupe_key TEXT NOT NULL,
    facts_json TEXT NOT NULL CHECK (length(facts_json) <= 4096),
    evidence_json TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('refresh','chat','briefing')),
    baseline INTEGER NOT NULL CHECK (baseline IN (0,1)),
    origin_chat_id TEXT,
    origin_message_id TEXT,
    UNIQUE (loupist_id, asset_id, dedupe_key)
) STRICT;
CREATE INDEX knowledge_asset_observations_by_time ON knowledge_asset_observations(loupist_id, asset_id, recorded_at_ms);
CREATE TABLE knowledge_asset_moves (
    move_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    from_verdict TEXT,
    to_verdict TEXT NOT NULL,
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    reason TEXT NOT NULL CHECK (length(reason) <= 600),
    evidence_json TEXT NOT NULL,
    reason_id TEXT,
    line_json TEXT,
    actor TEXT NOT NULL CHECK (actor IN ('loupist','user')),
    origin_chat_id TEXT,
    origin_message_id TEXT,
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    undone INTEGER NOT NULL DEFAULT 0 CHECK (undone IN (0,1)),
    undoes_move_id TEXT,
    seen_at_ms INTEGER
) STRICT;
CREATE INDEX knowledge_asset_moves_by_asset ON knowledge_asset_moves(loupist_id, asset_id, at_ms);
CREATE TABLE knowledge_asset_section_deltas (
    delta_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    filing_dedupe_key TEXT NOT NULL,
    source_record_id TEXT NOT NULL,
    previous_record_id TEXT NOT NULL,
    sections_json TEXT NOT NULL,
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    UNIQUE (loupist_id, asset_id, filing_dedupe_key)
) STRICT;
CREATE TABLE knowledge_earnings_events (
    loupist_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    period TEXT NOT NULL CHECK (length(period) BETWEEN 1 AND 40),
    fiscal_period_end TEXT CHECK (fiscal_period_end IS NULL OR length(fiscal_period_end) = 10),
    report_date TEXT NOT NULL CHECK (length(report_date) = 10),
    date_source TEXT NOT NULL CHECK (date_source IN ('calendar','ir_notice','legal_deadline')),
    status TEXT NOT NULL CHECK (status IN ('upcoming','before_captured','reported','final')),
    release_json TEXT,
    final_json TEXT,
    before_at_ms INTEGER,
    before_as_of TEXT CHECK (before_as_of IS NULL OR length(before_as_of) = 10),
    after_at_ms INTEGER,
    final_at_ms INTEGER,
    flags_json TEXT NOT NULL DEFAULT '{}',
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (loupist_id, event_id),
    UNIQUE (loupist_id, asset_id, period)
) STRICT;
CREATE TABLE knowledge_earnings_rows (
    row_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('expected','reported','guidance_new')),
    measure TEXT NOT NULL CHECK (length(measure) BETWEEN 1 AND 80),
    basis TEXT NOT NULL DEFAULT '' CHECK (length(basis) <= 40),
    period TEXT NOT NULL CHECK (length(period) BETWEEN 1 AND 40),
    low TEXT,
    high TEXT,
    point TEXT,
    unit TEXT NOT NULL CHECK (unit IN ('currency','percent','per_share','number')),
    currency TEXT CHECK (currency IS NULL OR length(currency) = 3),
    kind TEXT NOT NULL CHECK (kind IN ('guidance','consensus','broker_estimate','implied','reported')),
    stage TEXT CHECK (stage IS NULL OR stage IN ('preliminary','final')),
    provider TEXT,
    evidence_json TEXT NOT NULL,
    extra_json TEXT NOT NULL DEFAULT '{}' CHECK (length(extra_json) <= 8192),
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    captured_as_of TEXT CHECK (captured_as_of IS NULL OR length(captured_as_of) = 10),
    origin TEXT NOT NULL CHECK (origin IN ('host','chat','briefing')),
    origin_message_id TEXT,
    CHECK (point IS NOT NULL OR (low IS NOT NULL AND high IS NOT NULL))
) STRICT;
CREATE INDEX knowledge_earnings_rows_by_event ON knowledge_earnings_rows(loupist_id, event_id, side);
CREATE TABLE knowledge_earnings_tone (
    loupist_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 800),
    spans_json TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    PRIMARY KEY (loupist_id, event_id)
) STRICT;
CREATE TABLE knowledge_earnings_notices (
    loupist_id TEXT NOT NULL,
    notice_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('reported','failed')),
    side TEXT CHECK (side IS NULL OR side IN ('before','after')),
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    posted_at_ms INTEGER,
    PRIMARY KEY (loupist_id, notice_id)
) STRICT;
CREATE TABLE knowledge_watch_assets (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    scanned_to_ms INTEGER NOT NULL CHECK (scanned_to_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
CREATE TABLE knowledge_watch_settings (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('filings','results','insiders','holders','flows','dividends','cryptoMove','drawdown','depeg','tvl','futuresMove','positioning','oilInventory')),
    enabled INTEGER NOT NULL CHECK (enabled IN (0,1)),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (loupist_id, asset_id, kind)
) STRICT;
CREATE TABLE knowledge_watch_state (
    loupist_id TEXT PRIMARY KEY NOT NULL,
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0,1)),
    last_checked_ms INTEGER,
    last_check_day TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;
CREATE TABLE knowledge_watch_routes (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    route TEXT NOT NULL CHECK (route IN ('filings','insiders','calendar','dividends','flows','price','tvl','positioning','inventory')),
    checked_at_ms INTEGER NOT NULL CHECK (checked_at_ms >= 0),
    status TEXT NOT NULL CHECK (status IN ('ok','failed','needs_connection')),
    connection TEXT NOT NULL,
    baseline_done INTEGER NOT NULL CHECK (baseline_done IN (0,1)),
    PRIMARY KEY (loupist_id, asset_id, route)
) STRICT;
CREATE TABLE knowledge_watch_events (
    event_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('filings','results','insiders','holders','flows','dividends','cryptoMove','drawdown','depeg','tvl','futuresMove','positioning','oilInventory')),
    natural_key TEXT NOT NULL CHECK (length(natural_key) BETWEEN 1 AND 400),
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    facts_json TEXT NOT NULL CHECK (length(facts_json) <= 2048),
    evidence_json TEXT NOT NULL,
    detected_at_ms INTEGER NOT NULL CHECK (detected_at_ms >= 0),
    seen_at_ms INTEGER,
    delivered_in TEXT CHECK (delivered_in IS NULL OR delivered_in IN ('message','morning','baseline')),
    delivered_at_ms INTEGER,
    UNIQUE (loupist_id, asset_id, kind, natural_key)
) STRICT;
CREATE INDEX knowledge_watch_events_by_time ON knowledge_watch_events(loupist_id, detected_at_ms);
CREATE TRIGGER knowledge_growth_events_no_update BEFORE UPDATE ON knowledge_growth_events
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_growth_events_no_delete BEFORE DELETE ON knowledge_growth_events
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_blobs_no_update BEFORE UPDATE ON knowledge_blobs
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_blobs_no_delete BEFORE DELETE ON knowledge_blobs
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_capture_lineage_no_update BEFORE UPDATE ON knowledge_capture_lineage
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_capture_lineage_no_delete BEFORE DELETE ON knowledge_capture_lineage
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_lens_deliveries_no_update BEFORE UPDATE ON knowledge_lens_deliveries
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_lens_deliveries_no_delete BEFORE DELETE ON knowledge_lens_deliveries
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_lens_log_no_update BEFORE UPDATE ON knowledge_lens_log
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_lens_log_no_delete BEFORE DELETE ON knowledge_lens_log
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_record_revisions_no_update BEFORE UPDATE ON knowledge_record_revisions
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_record_revisions_no_delete BEFORE DELETE ON knowledge_record_revisions
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_records_no_update BEFORE UPDATE ON knowledge_records
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_records_no_delete BEFORE DELETE ON knowledge_records
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_read_receipts_no_update BEFORE UPDATE ON knowledge_read_receipts
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_read_receipts_no_delete BEFORE DELETE ON knowledge_read_receipts
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_research_records_no_update BEFORE UPDATE ON knowledge_research_records
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_research_records_no_delete BEFORE DELETE ON knowledge_research_records
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_source_captures_no_update BEFORE UPDATE ON knowledge_source_captures
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_source_captures_no_delete BEFORE DELETE ON knowledge_source_captures
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_source_representations_no_update BEFORE UPDATE ON knowledge_source_representations
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_source_representations_no_delete BEFORE DELETE ON knowledge_source_representations
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_source_spans_no_update BEFORE UPDATE ON knowledge_source_spans
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_source_spans_no_delete BEFORE DELETE ON knowledge_source_spans
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_turns_no_update BEFORE UPDATE ON knowledge_turns
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_turns_no_delete BEFORE DELETE ON knowledge_turns
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_world_deliveries_no_update BEFORE UPDATE ON knowledge_world_deliveries
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_world_deliveries_no_delete BEFORE DELETE ON knowledge_world_deliveries
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TRIGGER knowledge_world_snapshots_no_update BEFORE UPDATE ON knowledge_world_snapshots
             BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER knowledge_world_snapshots_no_delete BEFORE DELETE ON knowledge_world_snapshots
             BEGIN
               SELECT RAISE(ABORT, 'immutable')
               WHERE NOT EXISTS (
                 SELECT 1 FROM knowledge_erasure WHERE id = 1 AND armed = 1
               );
             END;
CREATE TABLE knowledge_receipt_snapshots (
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
         END;
CREATE TABLE datasets (
 loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
 id TEXT NOT NULL, revision INTEGER NOT NULL, digest TEXT NOT NULL,
 request_key TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
 metadata_json TEXT NOT NULL, payload BLOB NOT NULL,
 PRIMARY KEY(loupist_id,id), UNIQUE(loupist_id,request_key));
CREATE TABLE scheduled_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'completed')),
    next_run_at_ms INTEGER CHECK (next_run_at_ms >= 0),
    target_thread_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    data_json TEXT NOT NULL,
    UNIQUE (id, loupist_id),
    CHECK ((status = 'active') = (next_run_at_ms IS NOT NULL))
) STRICT;
CREATE TABLE scheduled_task_runs (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    task_revision INTEGER NOT NULL CHECK (task_revision > 0),
    thread_id TEXT REFERENCES chat_sessions(id) ON DELETE SET NULL,
    run_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'interrupted')),
    scheduled_for_ms INTEGER NOT NULL CHECK (scheduled_for_ms >= 0),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    finished_at_ms INTEGER CHECK (finished_at_ms >= started_at_ms),
    data_json TEXT NOT NULL,
    task_snapshot_json TEXT NOT NULL,
    FOREIGN KEY (task_id, loupist_id) REFERENCES scheduled_tasks(id, loupist_id) ON DELETE CASCADE,
    CHECK ((status = 'running') = (finished_at_ms IS NULL)),
    CHECK (status <> 'running' OR thread_id IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX one_briefing_per_loupist ON scheduled_tasks(loupist_id) WHERE json_extract(data_json,'$.kind')='briefing';
CREATE INDEX scheduled_tasks_by_loupist ON scheduled_tasks(loupist_id, updated_at_ms DESC, id);
CREATE INDEX scheduled_tasks_by_due ON scheduled_tasks(next_run_at_ms, id) WHERE status = 'active';
CREATE INDEX scheduled_task_runs_by_task ON scheduled_task_runs(task_id, started_at_ms DESC, id);
CREATE UNIQUE INDEX scheduled_task_one_running ON scheduled_task_runs(task_id) WHERE status = 'running';
CREATE TABLE chat_handoffs (
    id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
    request_key TEXT NOT NULL,
    chat_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('created','running','done','failed','stopped')),
    run_id TEXT,
    created_at_ms INTEGER NOT NULL,
    data_json TEXT NOT NULL,
    UNIQUE (loupist_id, request_key)
) STRICT;
CREATE INDEX chat_handoffs_by_loupist ON chat_handoffs(loupist_id, created_at_ms ASC);
INSERT INTO knowledge_schema (id, version) VALUES (1, 6);
INSERT INTO knowledge_erasure (id, armed) VALUES (1, 0);
PRAGMA application_id = 1196773973;
PRAGMA user_version = 4;
