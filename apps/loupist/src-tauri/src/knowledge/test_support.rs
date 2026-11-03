//! Test-only conveniences. Each method opens one transaction and forwards to the
//! `KnowledgeWrite` method the app calls, except where noted.

use std::path::Path;
use std::sync::Arc;

use super::*;

impl KnowledgeStore {
    /// Fixtures exercise the same schema and connection authority as the app.
    pub(crate) fn open(path: impl AsRef<Path>) -> KnowledgeResult<Self> {
        let path = path.as_ref();
        let store = crate::store::SqliteStore::open(path)
            .map_err(|_| KnowledgeError::Invalid("could not open test app database"))?;
        let blobs = BlobStore::open(schema::blob_root_for(path)?)?;
        Ok(Self::attach(Arc::new(store), blobs))
    }

    pub(crate) fn growth_events(
        &self,
        request: &GrowthEventsQuery,
    ) -> KnowledgeResult<GrowthEventsPage> {
        self.transact(|write| write.growth_events(request))
    }

    pub(crate) fn retain_new_source(
        &self,
        request: RetainNewSource,
    ) -> KnowledgeResult<RetainedSource> {
        self.transact(|write| write.retain_new_source(request))
    }

    /// Appends a capture to an existing Source; only fixtures and evals do this.
    pub(crate) fn retain_source(&self, request: RetainSource) -> KnowledgeResult<RetainedSource> {
        self.transact(|write| write.retain_source(request))
    }

    pub(crate) fn list_sources(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<Vec<SourceSummary>> {
        self.read(|write| write.list_sources(loupist_id, record_id))
    }

    pub(crate) fn revise_lens(&self, request: ReviseLens) -> KnowledgeResult<LensView> {
        self.transact(|write| write.revise_lens(request))
    }

    pub(crate) fn read_lens(&self, loupist_id: &str, record_id: &str) -> KnowledgeResult<LensView> {
        self.read(|write| write.read_lens(loupist_id, record_id))
    }

    #[cfg(feature = "eval-harness")]
    pub(crate) fn list_lens_changes(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<Vec<LensChangeRecord>> {
        self.read(|write| write.list_lens_changes(loupist_id, record_id))
    }

    #[cfg(feature = "eval-harness")]
    pub(crate) fn open_turn(&self, request: OpenTurn) -> KnowledgeResult<()> {
        self.transact(|write| write.open_turn(request))
    }

    #[cfg(feature = "eval-harness")]
    pub(crate) fn save_research(&self, request: SaveResearch) -> KnowledgeResult<SavedResearch> {
        self.transact(|write| write.save_research(request))
    }

    pub(crate) fn research_history(
        &self,
        loupist_id: &str,
        query: &str,
        claim_id: Option<&str>,
        limit: Option<u32>,
    ) -> KnowledgeResult<ResearchSearch> {
        self.read(|write| write.research_history(loupist_id, query, claim_id, limit))
    }
}

// Source listing has no app caller; fixtures use it to find capture ids.
impl KnowledgeWrite<'_> {
    pub(crate) fn list_sources(
        &self,
        loupist_id: &str,
        record_id: &str,
    ) -> KnowledgeResult<Vec<SourceSummary>> {
        source::list_sources(self.tx, loupist_id, record_id)
    }
}
