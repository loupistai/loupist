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

    /// Files one method page section through the only Lens writer: creates
    /// the page when `title` is new, otherwise replaces that section.
    pub(crate) fn file_method(
        &self,
        loupist_id: &str,
        title: &str,
        markdown: &str,
        origin: &lens_page::FilingOrigin,
        at_ms: i64,
    ) -> KnowledgeResult<PageRead> {
        self.transact(|write| {
            write.file_section(
                loupist_id,
                lens_page::PageKind::Method,
                title,
                "steps",
                markdown,
                origin,
                at_ms,
            )
        })
    }

    pub(crate) fn read_page(&self, loupist_id: &str, record_id: &str) -> KnowledgeResult<PageRead> {
        self.read(|write| write.read_page(loupist_id, record_id))
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

/// The user filing from the bridge.
pub(crate) fn user_origin() -> lens_page::FilingOrigin {
    lens_page::FilingOrigin::User {
        chat_id: None,
        message_id: None,
    }
}

/// The Loupist filing at a turn's commit.
pub(crate) fn turn_origin(chat_id: &str, message_id: &str) -> lens_page::FilingOrigin {
    lens_page::FilingOrigin::Turn {
        chat_id: chat_id.into(),
        message_id: message_id.into(),
        user_message_id: None,
        unattended: false,
    }
}

impl KnowledgeWrite<'_> {
    /// Creates or merges into the page `(kind, title)` and puts one section,
    /// replacing it when the key already exists.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn file_section(
        &self,
        loupist_id: &str,
        kind: lens_page::PageKind,
        title: &str,
        key: &str,
        markdown: &str,
        origin: &lens_page::FilingOrigin,
        at_ms: i64,
    ) -> KnowledgeResult<PageRead> {
        use lens_page::{Basis, LensOp, OpOutcome, PageDelta, PageTarget, SectionKey};
        let put = |expected: Option<String>| LensOp::Put {
            key: SectionKey::new(key).expect("test key"),
            heading: key.to_uppercase(),
            markdown: markdown.into(),
            basis: Basis::default(),
            expected,
        };
        let filed = self.file(
            loupist_id,
            origin,
            vec![PageDelta {
                target: PageTarget::Create {
                    kind,
                    title: title.into(),
                    anchors: Vec::new(),
                },
                ops: vec![put(None)],
            }],
            at_ms,
        )?;
        let page = filed.into_iter().next().ok_or(KnowledgeError::NotFound)?;
        if page.record_id.is_empty() {
            return Err(KnowledgeError::Invalid("nothing filed"));
        }
        if matches!(page.outcomes[0].outcome, OpOutcome::SkippedStale { .. }) {
            let current = self.read_page(loupist_id, &page.record_id)?;
            let digest = current
                .body
                .sections()
                .iter()
                .find(|section| section.key.as_str() == key)
                .map(|section| section.digest.clone());
            self.file(
                loupist_id,
                origin,
                vec![PageDelta {
                    target: PageTarget::Existing {
                        record_id: page.record_id.clone(),
                    },
                    ops: vec![put(digest)],
                }],
                at_ms,
            )?;
        }
        self.read_page(loupist_id, &page.record_id)
    }

    /// Replaces or adds one section of an existing page.
    pub(crate) fn put_section(
        &self,
        loupist_id: &str,
        record_id: &str,
        key: &str,
        markdown: &str,
        origin: &lens_page::FilingOrigin,
        at_ms: i64,
    ) -> KnowledgeResult<Vec<lens_page::FiledPage>> {
        use lens_page::{Basis, LensOp, PageDelta, PageTarget, SectionKey};
        let current = self.read_page(loupist_id, record_id)?;
        let expected = current
            .body
            .sections()
            .iter()
            .find(|section| section.key.as_str() == key)
            .map(|section| section.digest.clone());
        self.file(
            loupist_id,
            origin,
            vec![PageDelta {
                target: PageTarget::Existing {
                    record_id: record_id.into(),
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new(key).expect("test key"),
                    heading: key.to_uppercase(),
                    markdown: markdown.into(),
                    basis: Basis::default(),
                    expected,
                }],
            }],
            at_ms,
        )
    }
}
