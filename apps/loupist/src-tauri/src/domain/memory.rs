use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{hex_lower, require_bounded_text, required_option, DomainError};

pub(crate) const MAX_MEMORY_REFS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAccess {
    SearchDiscovered,
    ExactRead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeReference {
    pub kind: String,
    pub record_id: String,
    pub revision_id: String,
    pub object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRefSnapshot {
    pub read_ref: String,
    pub kind: String,
    pub title: String,
    pub excerpt: String,
    #[serde(deserialize_with = "required_option")]
    pub as_of: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub section: Option<String>,
    pub access: MemoryAccess,
    #[serde(deserialize_with = "required_option")]
    pub full_record_digest: Option<String>,
    #[serde(default)]
    pub record_id: String,
    #[serde(default)]
    pub revision_id: String,
    #[serde(default)]
    pub object_id: String,
    #[serde(default)]
    pub span_id: Option<String>,
    #[serde(default)]
    pub receipt_id: Option<String>,
}

impl MemoryRefSnapshot {
    pub fn validate(&self) -> Result<(), DomainError> {
        require_bounded_text(&self.read_ref, 256, "memory reference id is invalid")?;
        let kind = self.kind.to_ascii_lowercase();
        if !matches!(
            kind.as_str(),
            "source" | "world" | "lens" | "journal" | "research"
        ) {
            return Err(DomainError::Invalid("memory reference kind is invalid"));
        }
        if kind == "journal" {
            if self.object_id.is_empty() || self.receipt_id.as_deref().unwrap_or("").is_empty() {
                return Err(DomainError::Invalid(
                    "memory reference must name a private journal receipt",
                ));
            }
        } else if !is_knowledge_id(&self.record_id, "rec_")
            || !is_knowledge_id(&self.revision_id, "rev_")
            || self.object_id.is_empty()
            || self.object_id.len() > 128
        {
            return Err(DomainError::Invalid(
                "memory reference must name a knowledge record, revision, and object",
            ));
        }
        if self.access == MemoryAccess::ExactRead
            && self.receipt_id.as_deref().unwrap_or("").is_empty()
        {
            return Err(DomainError::Invalid(
                "exact knowledge read requires a receipt",
            ));
        }
        require_bounded_text(&self.title, 512, "memory reference title is invalid")?;
        if self.excerpt.len() > 2_048 || self.excerpt.contains('\0') {
            return Err(DomainError::Invalid("memory reference excerpt is invalid"));
        }
        if self
            .as_of
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > 128 || value.contains('\0'))
        {
            return Err(DomainError::Invalid("memory reference as-of is invalid"));
        }
        if self.section.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 512 || value.contains(['\0', '\n', '\r'])
        }) {
            return Err(DomainError::Invalid("memory reference section is invalid"));
        }
        if self.span_id.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 128 || value.contains(['\0', '\n'])
        }) {
            return Err(DomainError::Invalid("memory reference span is invalid"));
        }
        if self
            .full_record_digest
            .as_deref()
            .is_some_and(|digest| !is_sha256_digest(digest))
        {
            return Err(DomainError::Invalid(
                "memory reference full-record digest is invalid",
            ));
        }
        Ok(())
    }
}

fn is_knowledge_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|rest| {
        rest.len() == 32
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn is_sha256_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn memory_refs_digest(memories: &[MemoryRefSnapshot]) -> Result<String, DomainError> {
    let mut canonical = memories.to_vec();
    canonical.sort_by(|left, right| left.read_ref.cmp(&right.read_ref));
    let encoded = serde_json::to_vec(&canonical)
        .map_err(|_| DomainError::Invalid("chat memory references cannot be encoded"))?;
    Ok(hex_lower(&Sha256::digest(encoded)))
}

/// Host-observed Lens delivery. Discovery and model-written links never create one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LensReceipt {
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub kind: String,
    pub title: String,
    pub delivery: String,
}

impl LensReceipt {
    pub fn validate(&self) -> Result<(), DomainError> {
        if !is_knowledge_id(&self.record_id, "rec_")
            || !is_knowledge_id(&self.revision_id, "rev_")
            || !is_sha256_digest(&self.revision_digest)
            || !matches!(
                self.kind.as_str(),
                "principle" | "perspective" | "method" | "subject" | "idea"
            )
            || !matches!(self.delivery.as_str(), "context" | "exact_read")
        {
            return Err(DomainError::Invalid("Lens delivery is invalid"));
        }
        require_bounded_text(
            &self.title,
            if self.kind == "principle" { 1120 } else { 512 },
            "Lens delivery title is invalid",
        )
    }
}
