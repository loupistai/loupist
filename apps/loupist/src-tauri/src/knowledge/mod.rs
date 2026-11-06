//! Current knowledge authority: Source, World, Lens, and private research.
//!
//! Writes take a borrowed SQLite transaction so Chat finalization can later
//! commit retained sources, journals, and graph outbox rows together.
//! Knowledge and Chat use the same Loupist database and transaction authority.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, Transaction, TransactionBehavior};

pub mod assets;
mod authority;
pub mod earnings;
mod folder;
pub mod watch;
pub(crate) use folder::validate_folder;
mod blob;
mod canonical;
mod catalog;
pub(crate) mod connections;
mod error;
mod followed;
mod graph_inference;
#[cfg(test)]
pub(crate) use graph_inference::{embed_local_english_batch, InferenceDisposition, InferenceReply};
mod graph_jobs;
mod graph_projection;
mod graph_service;
mod growth;
pub(crate) mod journal;
pub mod learning;
mod lens;
pub mod lens_page;
pub use lens::{IndexEntry, LearnedDigest, PageRead, PageSummary, Screen};
pub mod memory_controls;
pub mod memory_reset;
pub mod principles;
pub use principles::{Principle, PrincipleInput};
mod identity;
mod map;
pub use growth::{GrowthEventsPage, GrowthEventsQuery};
pub(crate) use identity::{match_items, resolve_followed, IdentityMatch};
pub use map::{KnowledgeMap, KnowledgeMapQuery};
mod model;
pub mod numbers;
mod present;
mod research;
mod revision;
mod schema;
mod search_embeddings;
mod search_index;
pub use search_index::{PreparedSearch, SearchOptions, SEARCH_LIMIT, SEARCH_WINDOW_LIMIT};
mod schema_writer;
pub(crate) mod shared_pack;
mod source;
mod time;
mod worker;
mod world;
pub(crate) mod world_extract;
mod world_followups;
pub(crate) use world_followups::{
    mark_shown as connections_mark_shown, to_show as connections_to_show,
};
pub use world_followups::{ConnectionStatus, ConnectionView};
mod world_filter;
pub(crate) use world_filter::{last_day, published_by};
mod world_schema;
mod world_source;
pub use world_source::{SourceWorldClaim, WorldSourceTarget};

pub use blob::BlobStore;
#[cfg(feature = "eval-harness")]
pub use canonical::canonicalize_json_text;
pub use error::{KnowledgeError, KnowledgeResult};
#[cfg(test)]
pub(crate) use followed::{anchor_followed, followed_text, FOLLOWED_PER_SOURCE};
pub(crate) use followed::{
    followed_assets, title_key, FollowedAsset, FollowedState, FollowedStatus, FOLLOWED_PROVIDER,
    UNKNOWN_MARKET,
};
pub(crate) use graph_jobs::recover_interrupted as recover_interrupted_jobs;
#[cfg(any(test, feature = "eval-harness"))]
pub use graph_jobs::SealedWorldIndex;
pub use graph_jobs::{
    get_job, list_jobs, load_settings, pause_processing, recover_world, recovery_status,
    resume_processing, store_settings, GraphJobView, GraphProcessingSettings, GraphProcessingView,
    GraphPublicSettings, GraphReadiness, ReadinessNote,
};
pub use graph_service::GraphSupervisor;
pub use model::*;
pub use present::present_world;
pub use schema::embedded_ddl;
pub(crate) use schema::made_by_earlier_build;
#[cfg(feature = "eval-harness")]
pub use world_schema::curated_seed;
pub(crate) use world_schema::WorldExtension;
pub(crate) use world_schema::{
    add_kinds, added_labels, apply_preset, live_extension, recent_changes, relation_already_known,
    remove_schema_item, schema_view, type_already_known, undo_change, validate_extension,
    visible_extension, world_kinds_block, Producer, Provenance, SchemaChange,
};
pub use world_schema::{
    ExtensionCategory, ExtensionField, ExtensionLabels, ExtensionRelation, ExtensionType,
    WorldSchemaView,
};
#[cfg(test)]
pub(crate) use {graph_jobs::AddToWorld, growth::GrowthEvent};

pub fn fold_lens_title(title: &str) -> String {
    lens::fold_lens_title(title)
}
pub use source::reject_locator_userinfo;

impl std::fmt::Debug for KnowledgeStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KnowledgeStore")
    }
}

pub struct KnowledgeStore {
    connection: Arc<crate::store::SqliteStore>,
    blobs: BlobStore,
    loupist_id: Option<String>,
}

pub struct KnowledgeWrite<'a> {
    tx: &'a Transaction<'a>,
    blobs: &'a BlobStore,
}

impl KnowledgeStore {
    /// Knowledge rows on the process's existing app database.
    ///
    /// `SqliteStore::open` already creates the knowledge tables. This does not
    /// open a second database file.
    pub fn attach(store: std::sync::Arc<crate::store::SqliteStore>, blobs: BlobStore) -> Self {
        Self {
            connection: store,
            blobs,
            loupist_id: None,
        }
    }

    pub fn attach_loupist(
        store: Arc<crate::store::SqliteStore>,
        blobs: BlobStore,
        loupist_id: &str,
    ) -> Self {
        Self {
            connection: store,
            blobs,
            loupist_id: Some(loupist_id.to_owned()),
        }
    }

    pub fn for_loupist(
        store: Arc<crate::store::SqliteStore>,
        loupist_id: &str,
    ) -> KnowledgeResult<Self> {
        let root = store
            .loupist_data_dir(loupist_id)
            .map_err(|_| KnowledgeError::Invalid("Loupist folder unavailable"))?;
        Ok(Self {
            connection: store,
            blobs: BlobStore::open(root.join("knowledge-blobs"))?,
            loupist_id: Some(loupist_id.to_owned()),
        })
    }

    pub fn transact<T>(
        &self,
        body: impl FnOnce(&KnowledgeWrite<'_>) -> KnowledgeResult<T>,
    ) -> KnowledgeResult<T> {
        self.with_locked(true, body)
    }

    pub fn followed_items(&self) -> Vec<crate::store::user_context::MapContextItem> {
        self.connection
            .map_context()
            .map(|context| {
                context
                    .watchlist
                    .into_iter()
                    .chain(context.holdings)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn read<T>(
        &self,
        body: impl FnOnce(&KnowledgeWrite<'_>) -> KnowledgeResult<T>,
    ) -> KnowledgeResult<T> {
        self.with_locked(false, body)
    }

    fn with_locked<T>(
        &self,
        immediate: bool,
        body: impl FnOnce(&KnowledgeWrite<'_>) -> KnowledgeResult<T>,
    ) -> KnowledgeResult<T> {
        let run = |connection: &mut Connection| -> KnowledgeResult<T> {
            let tx = if immediate {
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?
            } else {
                connection.transaction()?
            };
            let write = KnowledgeWrite {
                tx: &tx,
                blobs: &self.blobs,
            };
            let value = body(&write)?;
            if immediate {
                tx.commit()?;
            }
            Ok(value)
        };
        let mut connection = match self.loupist_id.as_deref() {
            Some(loupist) => self.connection.lock_loupist(loupist),
            None => self.connection.lock(),
        }
        .map_err(|_| KnowledgeError::LockPoisoned)?;
        if self.connection.is_folder_catalog() && self.loupist_id.is_some() {
            let expected = self
                .blobs
                .root()
                .parent()
                .ok_or(KnowledgeError::Invalid("Loupist blob root is invalid"))?
                .join("loupist.sqlite3");
            if connection.path().map(Path::new) != Some(expected.as_path()) {
                return Err(KnowledgeError::Invalid(
                    "Loupist folder changed during this operation",
                ));
            }
        }
        run(&mut connection)
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn create_record(&self, request: CreateRecord) -> KnowledgeResult<OwnedRecord> {
        self.transact(|write| write.create_record(request))
    }

    pub fn get_revision(
        &self,
        loupist_id: &str,
        revision_id: &str,
    ) -> KnowledgeResult<RecordRevision> {
        self.read(|write| write.get_revision(loupist_id, revision_id))
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn list_records(&self, loupist_id: &str) -> KnowledgeResult<Vec<OwnedRecord>> {
        self.read(|write| write.list_records(loupist_id))
    }

    #[cfg(feature = "eval-harness")]
    pub fn record_snapshot(
        &self,
        loupist_id: &str,
        revision_id: &str,
    ) -> KnowledgeResult<RecordSnapshot> {
        self.read(|write| write.record_snapshot(loupist_id, revision_id))
    }

    #[cfg(feature = "eval-harness")]
    pub fn read_span(&self, request: ReadSpan) -> KnowledgeResult<SpanRead> {
        self.transact(|write| write.read_span(request))
    }

    #[cfg(feature = "eval-harness")]
    pub fn read_capture_material(&self, request: ReadCapture) -> KnowledgeResult<SpanRead> {
        self.transact(|write| write.read_capture_material(request))
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn seal_world(&self, request: SealWorld) -> KnowledgeResult<WorldRevision> {
        self.transact(|write| write.seal_world(request))
    }

    pub fn query_world(&self, query: WorldGraphQuery) -> KnowledgeResult<WorldGraphView> {
        self.read(|write| write.query_world(query))
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn graph_accept_sealed_world(
        &self,
        settings: &GraphProcessingSettings,
        request: SealedWorldIndex,
    ) -> KnowledgeResult<GraphJobView> {
        self.transact(|write| {
            graph_jobs::accept_sealed_world(write.tx, &self.blobs, settings, request)
        })
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn graph_jobs(&self, loupist_id: &str) -> KnowledgeResult<Vec<GraphJobView>> {
        self.read(|write| graph_jobs::list_jobs(write.tx, loupist_id))
    }

    pub(crate) fn graph_claim_running(
        &self,
        settings: &GraphProcessingSettings,
        job_id: &str,
        now_ms: i64,
    ) -> KnowledgeResult<graph_jobs::GraphJobClaim> {
        self.transact(|write| graph_jobs::claim_running(write.tx, settings, job_id, now_ms))
    }

    pub fn graph_note_inference(&self, job_id: &str, attempt_id: &str) -> KnowledgeResult<()> {
        self.transact(|write| graph_jobs::note_inference_attempted(write.tx, job_id, attempt_id))
    }

    pub fn graph_finish(
        &self,
        settings: &mut GraphProcessingSettings,
        job_id: &str,
        attempt_id: &str,
        outcome: &str,
        usage_uncertain: bool,
        validated: bool,
    ) -> KnowledgeResult<GraphJobView> {
        self.transact(|write| {
            graph_jobs::finish_job(
                write.tx,
                settings,
                job_id,
                attempt_id,
                outcome,
                usage_uncertain,
                validated,
            )
        })
    }

    // Fixture publication without the derived index; product jobs commit both together.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn graph_publish_and_complete(
        &self,
        settings: &mut GraphProcessingSettings,
        job_id: &str,
        attempt_id: &str,
        artifact_utf8: &str,
        logical_digest: &str,
        at_ms: i64,
        usage_uncertain: bool,
    ) -> KnowledgeResult<GraphJobView> {
        let followed = self.followed_items();
        self.transact(|write| {
            graph_jobs::publish_and_complete(
                write,
                settings,
                job_id,
                attempt_id,
                artifact_utf8,
                logical_digest,
                at_ms,
                usage_uncertain,
                &followed,
            )
        })
    }

    pub fn get_journal(
        &self,
        loupist_id: &str,
        journal_id: &str,
    ) -> KnowledgeResult<JournalRecord> {
        self.read(|write| write.get_journal(loupist_id, journal_id))
    }
}

impl<'a> KnowledgeWrite<'a> {
    pub fn map(
        &self,
        request: &KnowledgeMapQuery,
        context: &crate::store::user_context::MapContext,
    ) -> KnowledgeResult<KnowledgeMap> {
        map::query(self.tx, request, context)
    }

    pub fn retain_new_source_with_origin(
        &self,
        request: RetainNewSource,
        origin_message_id: &str,
    ) -> KnowledgeResult<RetainedSource> {
        source::retain_new_source_with_origin(self.tx, self.blobs, request, Some(origin_message_id))
    }

    pub fn growth_events(&self, request: &GrowthEventsQuery) -> KnowledgeResult<GrowthEventsPage> {
        growth::query(self.tx, request)
    }

    pub fn bind(tx: &'a Transaction<'a>, blobs: &'a BlobStore) -> Self {
        Self { tx, blobs }
    }

    /// The asset page a thesis entry names, created with the turn if new.
    pub fn ensure_asset_page(
        &self,
        loupist_id: &str,
        identity: &assets::AssetIdentity,
        at_ms: i64,
    ) -> KnowledgeResult<assets::PageRecord> {
        assets::ensure_page(self.tx, loupist_id, identity, None, at_ms)
    }

    pub fn create_record(&self, request: CreateRecord) -> KnowledgeResult<OwnedRecord> {
        catalog::create_record(self.tx, request)
    }

    pub fn get_revision(
        &self,
        loupist_id: &str,
        revision_id: &str,
    ) -> KnowledgeResult<RecordRevision> {
        catalog::get_revision(self.tx, loupist_id, revision_id)
    }

    pub fn list_records(&self, loupist_id: &str) -> KnowledgeResult<Vec<OwnedRecord>> {
        catalog::list_records(self.tx, loupist_id)
    }

    pub fn owned_record(&self, loupist_id: &str, record_id: &str) -> KnowledgeResult<OwnedRecord> {
        catalog::owned_record(self.tx, loupist_id, record_id)
    }

    #[cfg(feature = "eval-harness")]
    pub fn record_snapshot(
        &self,
        loupist_id: &str,
        revision_id: &str,
    ) -> KnowledgeResult<RecordSnapshot> {
        catalog::record_snapshot(self.tx, loupist_id, revision_id)
    }

    pub fn retain_new_source(&self, request: RetainNewSource) -> KnowledgeResult<RetainedSource> {
        source::retain_new_source(self.tx, self.blobs, request)
    }

    pub fn source_related_claims(
        &self,
        loupist_id: &str,
        record_id: &str,
        revision_id: &str,
        source_id: &str,
    ) -> KnowledgeResult<Vec<SourceWorldClaim>> {
        world_source::related_claims(self.tx, loupist_id, record_id, revision_id, source_id)
    }

    #[cfg(any(test, feature = "eval-harness", feature = "e2e"))]
    pub fn retain_source(&self, request: RetainSource) -> KnowledgeResult<RetainedSource> {
        source::retain_source(self.tx, self.blobs, request)
    }

    /// Bounded discovery text from the visible revision's derived index. This
    /// is not an exact read receipt and does not verify or open the original.
    pub fn source_catalog_text(
        &self,
        loupist_id: &str,
        record_id: &str,
        revision_id: &str,
        span_id: &str,
    ) -> KnowledgeResult<Option<String>> {
        use rusqlite::OptionalExtension;
        self.owned_record(loupist_id, record_id)?;
        self.tx.query_row(
            "SELECT substr(d.body, 1, 65536) FROM knowledge_search_documents d
             JOIN knowledge_record_heads h ON h.record_id = d.record_id AND h.revision_id = d.revision_id
             WHERE d.kind = 'source' AND d.record_id = ?1 AND d.revision_id = ?2
               AND d.span_id = ?3 AND d.coverage != 'metadata_only' LIMIT 1",
            rusqlite::params![record_id, revision_id, span_id],
            |row| row.get(0),
        ).optional().map_err(KnowledgeError::from)
    }

    /// Copies authorized current rows; rank the returned snapshot after releasing this transaction.
    pub fn prepare_search(
        &self,
        loupist_id: &str,
        scope: &str,
        needle: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> KnowledgeResult<PreparedSearch> {
        search_index::prepare_visible(self.tx, loupist_id, scope, needle, limit, options)
    }

    pub fn visible_search_revisions(
        &self,
        loupist_id: &str,
    ) -> KnowledgeResult<std::collections::BTreeSet<String>> {
        search_index::visible_revisions(self.tx, loupist_id)
    }

    #[cfg(feature = "eval-harness")]
    pub fn read_span(&self, request: ReadSpan) -> KnowledgeResult<SpanRead> {
        source::read_span(self.tx, self.blobs, request)
    }

    pub fn record_permissions(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<(bool, bool)> {
        let module = authority::require_record(self.tx, loupist_id, record_id)?;
        Ok((authority::require_edit(&module, loupist_id).is_ok(), false))
    }

    pub fn world_source_targets(
        &self,
        loupist_id: &str,
        world_record_id: &str,
        world_revision_id: &str,
        span_ids: &[String],
    ) -> KnowledgeResult<std::collections::BTreeMap<String, WorldSourceTarget>> {
        world_source::resolve(
            self.tx,
            loupist_id,
            world_record_id,
            world_revision_id,
            span_ids,
        )
    }

    pub fn world_source_revision(
        &self,
        loupist_id: &str,
        world_record_id: &str,
        world_revision_id: &str,
        span_id: &str,
    ) -> KnowledgeResult<(WorldSourceTarget, RecordRevision)> {
        let target = world_source::resolve(
            self.tx,
            loupist_id,
            world_record_id,
            world_revision_id,
            &[span_id.to_string()],
        )?
        .remove(span_id)
        .ok_or(KnowledgeError::NotFound)?;
        let revision = revision::load_revision(self.tx, &target.revision_id)?.to_model()?;
        Ok((target, revision))
    }

    /// Bytes for a span that the caller already found in one selected revision.
    ///
    /// The expected digest is that revision's span digest. This does not follow
    /// the module head or mint a receipt for the current pin.
    pub fn read_revision_span_bytes(
        &self,
        record_id: &str,
        span_id: &str,
        expected_digest: &str,
    ) -> KnowledgeResult<(Vec<u8>, String)> {
        source::read_span_bytes_unbound(self.tx, self.blobs, record_id, span_id, expected_digest)
    }

    #[cfg(feature = "eval-harness")]
    pub fn read_capture_material(&self, request: ReadCapture) -> KnowledgeResult<SpanRead> {
        source::read_capture_material(self.tx, self.blobs, request)
    }

    pub fn seal_world(&self, request: SealWorld) -> KnowledgeResult<WorldRevision> {
        world::seal_world(self.tx, request)
    }

    #[cfg(test)]
    pub(crate) fn seal_world_followed(
        &self,
        request: SealWorld,
        followed: &[crate::store::user_context::MapContextItem],
    ) -> KnowledgeResult<WorldRevision> {
        world::seal_world_with_actor(self.tx, request, "user", None, None, followed)
    }

    pub fn query_world(&self, query: WorldGraphQuery) -> KnowledgeResult<WorldGraphView> {
        world::query_world(self.tx, query)
    }

    pub fn file(
        &self,
        loupist_id: &str,
        origin: &lens_page::FilingOrigin,
        deltas: Vec<lens_page::PageDelta>,
        at_ms: i64,
    ) -> KnowledgeResult<Vec<lens_page::FiledPage>> {
        lens::file(self.tx, loupist_id, origin, deltas, at_ms)
    }

    pub fn read_page(&self, loupist_id: &str, record_id: &str) -> KnowledgeResult<PageRead> {
        lens::read_page(self.tx, loupist_id, record_id)
    }

    /// What `file` would make of one delta now, for a caller that stages it.
    pub fn screen_delta(
        &self,
        loupist_id: &str,
        delta: &lens_page::PageDelta,
    ) -> KnowledgeResult<Screen> {
        lens::screen(self.tx, loupist_id, delta)
    }

    pub fn lens_kind(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<lens_page::LensKind> {
        lens::lens_kind(self.tx, loupist_id, record_id)
    }

    pub fn undo_op(&self, loupist_id: &str, op_id: &str, at_ms: i64) -> KnowledgeResult<PageRead> {
        lens::undo_op(self.tx, loupist_id, op_id, at_ms)
    }

    pub fn forget_section(
        &self,
        loupist_id: &str,
        record_id: &str,
        key: &str,
        at_ms: i64,
    ) -> KnowledgeResult<PageRead> {
        lens::forget_section(self.tx, loupist_id, record_id, key, at_ms)
    }

    pub fn learned_digest(
        &self,
        loupist_id: &str,
        message_ids: &[String],
    ) -> KnowledgeResult<Vec<LearnedDigest>> {
        lens::learned_digest(self.tx, loupist_id, message_ids)
    }

    #[cfg(test)]
    pub fn resolve_anchors(&self, loupist_id: &str, nodes: &[GraphNode]) -> KnowledgeResult<usize> {
        lens::resolve_anchors(self.tx, loupist_id, nodes)
    }

    pub fn connections(&self, loupist_id: &str) -> KnowledgeResult<Vec<ConnectionView>> {
        world_followups::list(self.tx, loupist_id)
    }

    pub fn connection_set_status(
        &self,
        loupist_id: &str,
        connection_id: &str,
        status: ConnectionStatus,
        at_ms: i64,
    ) -> KnowledgeResult<ConnectionView> {
        world_followups::set_status(self.tx, loupist_id, connection_id, status, at_ms)
    }

    pub fn record_applied(
        &self,
        loupist_id: &str,
        turn_id: &str,
        record_id: &str,
        node_ids: &std::collections::BTreeSet<String>,
        at_ms: i64,
    ) -> KnowledgeResult<()> {
        lens::record_applied(self.tx, loupist_id, turn_id, record_id, node_ids, at_ms)
    }

    pub fn pages_for_node(
        &self,
        loupist_id: &str,
        node_id: &str,
    ) -> KnowledgeResult<Vec<PageSummary>> {
        lens::pages_for_node(self.tx, loupist_id, node_id)
    }

    pub fn lens_index(&self, loupist_id: &str) -> KnowledgeResult<Vec<IndexEntry>> {
        lens::index_entries(self.tx, loupist_id)
    }

    pub fn active_perspective(&self, loupist_id: &str) -> KnowledgeResult<Option<String>> {
        lens::active_perspective(self.tx, loupist_id)
    }

    pub fn set_lens_archived(
        &self,
        loupist_id: &str,
        record_id: &str,
        archived: bool,
        rationale: &str,
        at_ms: i64,
        actor: &str,
    ) -> KnowledgeResult<()> {
        lens::set_lens_archived(
            self.tx, loupist_id, record_id, archived, rationale, at_ms, actor,
        )
    }

    pub fn lens_listing(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<(i64, bool, bool)> {
        lens::lens_listing(self.tx, loupist_id, record_id)
    }

    pub fn save_research(&self, request: SaveResearch) -> KnowledgeResult<SavedResearch> {
        research::save_research(self.tx, request)
    }

    pub fn working_notes(
        &self,
        loupist_id: &str,
        chat_id: &str,
        limit: usize,
    ) -> KnowledgeResult<Vec<WorkingNote>> {
        research::working_notes(self.tx, loupist_id, chat_id, limit)
    }

    pub fn read_working_note(
        &self,
        loupist_id: &str,
        research_id: &str,
    ) -> KnowledgeResult<WorkingNote> {
        research::read_working_note(self.tx, loupist_id, research_id)
    }

    pub fn open_turn(&self, request: OpenTurn) -> KnowledgeResult<()> {
        research::open_turn(self.tx, request)
    }

    // The facade forwards exact source bytes and all receipt provenance to one transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn record_acked_source(
        &self,
        loupist_id: &str,
        chat_id: &str,
        turn_id: &str,
        at_ms: i64,
        record_id: &str,
        span_id: &str,
        capture_id: &str,
        representation_id: &str,
        selected_digest: &str,
        revision_id: &str,
        revision_digest: &str,
        attribution: &str,
        coverage: &str,
        publication_time_json: Option<&str>,
        media_type: &str,
        bytes: &[u8],
    ) -> KnowledgeResult<String> {
        research::record_acked_source(
            self.tx,
            loupist_id,
            chat_id,
            turn_id,
            at_ms,
            record_id,
            span_id,
            capture_id,
            representation_id,
            selected_digest,
            revision_id,
            revision_digest,
            attribution,
            coverage,
            publication_time_json,
            media_type,
            bytes,
        )
    }

    // The facade binds the exact delivered World view and digest to its consuming turn.
    #[allow(clippy::too_many_arguments)]
    pub fn record_acked_world(
        &self,
        loupist_id: &str,
        turn_id: &str,
        at_ms: i64,
        record_id: &str,
        revision_id: &str,
        revision_digest: &str,
        view_json: &str,
        view_digest: &str,
    ) -> KnowledgeResult<String> {
        research::record_acked_world(
            self.tx,
            loupist_id,
            turn_id,
            at_ms,
            record_id,
            revision_id,
            revision_digest,
            view_json,
            view_digest,
        )
    }

    // The facade binds the exact Lens body and digest to its consuming turn.
    #[allow(clippy::too_many_arguments)]
    pub fn record_acked_lens(
        &self,
        loupist_id: &str,
        turn_id: &str,
        at_ms: i64,
        record_id: &str,
        revision_id: &str,
        revision_digest: &str,
        document_json: &str,
        body_digest: &str,
    ) -> KnowledgeResult<String> {
        research::record_acked_lens(
            self.tx,
            loupist_id,
            turn_id,
            at_ms,
            record_id,
            revision_id,
            revision_digest,
            document_json,
            body_digest,
        )
    }

    pub fn research_history(
        &self,
        loupist_id: &str,
        query: &str,
        claim_id: Option<&str>,
        limit: Option<u32>,
    ) -> KnowledgeResult<ResearchSearch> {
        research::research_history(self.tx, loupist_id, query, claim_id, limit)
    }

    pub fn search_working_notes(
        &self,
        loupist_id: &str,
        query: &str,
        limit: Option<u32>,
    ) -> KnowledgeResult<ResearchSearch> {
        research::search_working_notes(self.tx, loupist_id, query, limit)
    }

    pub fn research_history_without_later_notes(
        &self,
        loupist_id: &str,
        query: &str,
        claim_id: Option<&str>,
        limit: Option<u32>,
    ) -> KnowledgeResult<ResearchSearch> {
        research::research_history_without_later_notes(self.tx, loupist_id, query, claim_id, limit)
    }

    pub fn journal_due(
        &self,
        loupist_id: &str,
        today: &str,
    ) -> KnowledgeResult<Vec<JournalRecord>> {
        journal::due(self.tx, loupist_id, today)
    }

    /// Due entries and the outcome record; `known` keeps each entry as it
    /// stood at an as-of date (or drops it), and is the identity otherwise.
    pub fn journal_context(
        &self,
        loupist_id: &str,
        today: &str,
        known: &mut dyn FnMut(JournalRecord) -> Option<JournalRecord>,
    ) -> KnowledgeResult<serde_json::Value> {
        journal::context(self.tx, loupist_id, today, known)
    }

    pub(crate) fn require_unreviewed_journal(
        &self,
        loupist_id: &str,
        journal_id: &str,
    ) -> KnowledgeResult<()> {
        journal::require_unreviewed(self.tx, loupist_id, journal_id)
    }

    pub fn get_journal(
        &self,
        loupist_id: &str,
        journal_id: &str,
    ) -> KnowledgeResult<JournalRecord> {
        research::get_journal(self.tx, loupist_id, journal_id)
    }

    pub fn read_journal_source(
        &self,
        loupist_id: &str,
        journal_id: &str,
        receipt_id: &str,
    ) -> KnowledgeResult<(Vec<u8>, String, JournalSourceInput)> {
        research::read_journal_source(self.tx, self.blobs, loupist_id, journal_id, receipt_id)
    }
}

pub fn note_chat_deleted_in(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
) -> KnowledgeResult<u32> {
    source::note_chat_deleted(tx, loupist_id, chat_id)
}

/// Erases one Loupist's private research, turns, receipts, jobs, and pins.
///
/// Only this Loupist's owned records are removed. Historical tables allow this
/// delete only while the erasure guard is armed.
pub fn erase_loupist_private(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<u32> {
    model::validate_loupist_id(loupist_id)?;
    tx.execute(
        "DELETE FROM app_settings WHERE id = ?1",
        [format!("world_processing:{loupist_id}")],
    )?;
    tx.execute("UPDATE knowledge_erasure SET armed = 1 WHERE id = 1", [])?;
    let erased = erase_guarded_rows(tx, loupist_id);
    tx.execute("UPDATE knowledge_erasure SET armed = 0 WHERE id = 1", [])?;
    erased
}

fn erase_guarded_rows(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<u32> {
    tx.execute(
        "DELETE FROM knowledge_growth_events WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_journals WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    earnings::erase(tx, loupist_id)?;
    watch::erase(tx, loupist_id)?;
    for table in [
        "knowledge_asset_section_deltas",
        "knowledge_asset_moves",
        "knowledge_asset_observations",
        "knowledge_asset_pages",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE loupist_id = ?1"),
            [loupist_id],
        )?;
    }
    tx.execute(
        "DELETE FROM knowledge_research_records WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_world_deliveries WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_lens_deliveries WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_receipt_snapshots
         WHERE receipt_id IN (
            SELECT receipt_id FROM knowledge_read_receipts WHERE loupist_id = ?1
         )",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_read_receipts WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_turns WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_lens_log WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_graph_outbox WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_turn_pins WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    tx.execute(
        "DELETE FROM knowledge_chat_origins WHERE loupist_id = ?1",
        [loupist_id],
    )?;
    for table in [
        "knowledge_learning_reviews",
        "knowledge_learning_settings",
        "knowledge_schema_changes",
        "knowledge_schema_reviews",
        "knowledge_lens_anchors",
        "knowledge_lens_links",
        "knowledge_lens_metadata",
        "knowledge_connections",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE loupist_id = ?1"),
            [loupist_id],
        )?;
    }
    // Remove children before their immutable parent records. Blob files are
    // content addressed and can be shared by independent Loupist captures.
    for table in [
        "knowledge_search_documents",
        "knowledge_capture_lineage",
        "knowledge_source_spans",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE record_id IN (SELECT record_id FROM knowledge_records WHERE owner_loupist_id = ?1)"), [loupist_id])?;
    }
    // RESTRICT is immediate even for rows removed by the same DELETE. Remove
    // derived leaves before their original representation, at every depth.
    loop {
        let deleted = tx.execute(
            "DELETE FROM knowledge_source_representations AS parent
             WHERE parent.record_id IN (
                 SELECT record_id FROM knowledge_records WHERE owner_loupist_id = ?1
             ) AND NOT EXISTS (
                 SELECT 1 FROM knowledge_source_representations AS child
                 WHERE child.record_id = parent.record_id
                   AND child.parent_representation_id = parent.representation_id
             )",
            [loupist_id],
        )?;
        if deleted == 0 {
            break;
        }
    }
    for table in [
        "knowledge_source_captures",
        "knowledge_sources",
        "knowledge_world_snapshots",
        "knowledge_record_heads",
        "knowledge_record_revisions",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE record_id IN (SELECT record_id FROM knowledge_records WHERE owner_loupist_id = ?1)"), [loupist_id])?;
    }
    let count = tx.execute(
        "DELETE FROM knowledge_records WHERE owner_loupist_id = ?1",
        [loupist_id],
    )?;
    Ok(count as u32)
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProcessing {
    pub job_id: Option<String>,
    pub status: String,
    pub error: Option<String>,
}

impl KnowledgeWrite<'_> {
    pub fn source_processing(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<SourceProcessing> {
        let record = self.owned_record(loupist_id, record_id)?;
        if let Some(job) = graph_jobs::latest_source_job(self.tx, loupist_id, record_id)? {
            return Ok(SourceProcessing {
                job_id: Some(job.job_id),
                error: (job.document.usage_uncertain).then(|| {
                    "The helper response was uncertain. Automatic retry is disabled.".into()
                }),
                status: job.status,
            });
        }
        let reflected: bool = self.tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM knowledge_records records JOIN knowledge_record_heads heads ON heads.record_id = records.record_id JOIN knowledge_record_revisions revisions ON revisions.revision_id = heads.revision_id JOIN json_each(revisions.dependency_json) dependency WHERE records.owner_loupist_id = ?1 AND records.kind = 'world' AND json_extract(dependency.value, '$.recordId') = ?2 AND json_extract(dependency.value, '$.role') = 'world_source')",
            rusqlite::params![loupist_id, record_id], |row| row.get(0),
        )?;
        if reflected {
            return Ok(SourceProcessing {
                job_id: None,
                status: "completed".into(),
                error: None,
            });
        }
        let mut text_available = false;
        if let Some(revision_id) = record.revision_id {
            let revision = self.get_revision(loupist_id, &revision_id)?;
            let payload: SourceRecordPayload = serde_json::from_value(revision.payload)?;
            text_available = payload
                .source
                .captures
                .iter()
                .flat_map(|capture| &capture.representations)
                .any(|rep| {
                    (rep.media_type.starts_with("text/")
                        || rep.media_type == "application/json"
                        || rep.media_type.ends_with("+json"))
                        && !rep.spans.is_empty()
                });
        }
        let settings = graph_jobs::load_settings(self.tx, loupist_id)?;
        let status = if !text_available {
            "no_text"
        } else if !settings.enrichment_enabled {
            "disabled"
        } else if settings.helper_model.is_none() {
            "not_configured"
        } else if settings.paused {
            "paused"
        } else {
            "saved"
        };
        Ok(SourceProcessing {
            job_id: None,
            status: status.into(),
            error: None,
        })
    }
}
