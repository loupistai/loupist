use crate::fs::artifact_trust::ArtifactTrustError;
use thiserror::Error;

/// Authority and storage failures for the current knowledge schema.
#[derive(Debug, Error)]
pub enum KnowledgeError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("private storage boundary failed: {0}")]
    PrivateStorage(#[from] ArtifactTrustError),
    #[error("stored JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("knowledge storage error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unauthorized: {0}")]
    Unauthorized(&'static str),
    #[error("You already have this principle.")]
    DuplicatePrinciple,
    #[error("revision conflict")]
    RevisionConflict {
        expected: Option<String>,
        actual: Option<String>,
    },
    #[error("metadata-only source has no retained bytes")]
    MetadataOnly,
    #[error("digest mismatch")]
    DigestMismatch,
    #[error("retained bytes are missing")]
    MissingBytes,
    #[error("renderer inspection receipt cannot authorize a journal citation")]
    InspectionReceiptNotCitable,
    #[error("lens body contains a forbidden reference")]
    LensBodyForbidden,
    #[error("conflicting required revisions of one record")]
    ConflictingDependencyRevision,
    #[error("query cursor does not match this revision and filter")]
    CursorStale,
    #[error("the first query item exceeds the byte or count budget")]
    ItemTooLarge,
    #[error("not found")]
    NotFound,
    #[error("invalid knowledge value: {0}")]
    Invalid(&'static str),
    #[error("the map already tracks a kind with that name")]
    KindAlreadyKnown,
    #[error("{code}: {expected}")]
    JournalField {
        code: &'static str,
        expected: &'static str,
    },
    #[error("{code}: {expected}")]
    AssetField {
        code: &'static str,
        expected: &'static str,
    },
    #[error("store lock was poisoned")]
    LockPoisoned,
    #[error("this Loupist was made by an earlier version of the app")]
    EarlierVersion,
    #[error("this Loupist was saved by a newer version of the app")]
    NewerVersion,
}

impl KnowledgeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "sqlite",
            Self::PrivateStorage(_) => "private_storage",
            Self::Json(_) => "json",
            Self::Io(_) => "io",
            Self::Unauthorized(_) => "unauthorized",
            Self::RevisionConflict { .. } => "revision_conflict",
            Self::DuplicatePrinciple => "duplicate_principle",
            Self::MetadataOnly => "metadata_only",
            Self::DigestMismatch => "digest_mismatch",
            Self::MissingBytes => "missing_bytes",
            Self::InspectionReceiptNotCitable => "inspection_receipt_not_citable",
            Self::LensBodyForbidden => "lens_body_forbidden",
            Self::ConflictingDependencyRevision => "conflicting_dependency_revision",
            Self::CursorStale => "cursor_stale",
            Self::ItemTooLarge => "item_too_large",
            Self::NotFound => "not_found",
            Self::Invalid(_) => "invalid",
            Self::KindAlreadyKnown => "kind_already_known",
            Self::JournalField { code, .. } | Self::AssetField { code, .. } => code,
            Self::LockPoisoned => "lock_poisoned",
            Self::EarlierVersion => "loupist_earlier_version",
            Self::NewerVersion => "loupist_newer_version",
        }
    }
}

pub type KnowledgeResult<T> = Result<T, KnowledgeError>;
