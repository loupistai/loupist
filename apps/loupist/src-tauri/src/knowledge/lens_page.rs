//! Lens pages as pure data: kinds, keyed sections, the write language and its
//! fold. No SQL lives here; `lens.rs` stores what this module decides.

use std::collections::BTreeSet;

use pulldown_cmark::{Event, Parser, Tag};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::canonical::canonical_string;
use super::error::{KnowledgeError, KnowledgeResult};
use super::principles::PrincipleInput;
use crate::util::sha256;

pub const MAX_SUMMARY_CHARS: usize = 400;
pub const MAX_SECTIONS: usize = 16;
pub const MAX_KEY_LEN: usize = 48;
pub const MAX_HEADING_CHARS: usize = 80;
pub const MAX_SECTION_CHARS: usize = 2_000;
pub const MAX_BASIS_REFS: usize = 8;
pub const MAX_UNCERTAINTY_CHARS: usize = 300;
pub const MAX_OPS_PER_DELTA: usize = 12;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_WHY_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LensKind {
    Principle,
    Perspective,
    Method,
    Subject,
    Idea,
}

impl LensKind {
    pub const ALL: [LensKind; 5] = [
        Self::Principle,
        Self::Perspective,
        Self::Method,
        Self::Subject,
        Self::Idea,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Principle => "principle",
            Self::Perspective => "perspective",
            Self::Method => "method",
            Self::Subject => "subject",
            Self::Idea => "idea",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Kinds a shared pack may carry. Subject and idea pages stay local.
    pub fn is_portable(self) -> bool {
        matches!(self, Self::Principle | Self::Perspective | Self::Method)
    }

    pub fn page_kind(self) -> Option<PageKind> {
        match self {
            Self::Principle => None,
            Self::Perspective => Some(PageKind::Perspective),
            Self::Method => Some(PageKind::Method),
            Self::Subject => Some(PageKind::Subject),
            Self::Idea => Some(PageKind::Idea),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    Perspective,
    Method,
    Subject,
    Idea,
}

impl PageKind {
    pub fn lens_kind(self) -> LensKind {
        match self {
            Self::Perspective => LensKind::Perspective,
            Self::Method => LensKind::Method,
            Self::Subject => LensKind::Subject,
            Self::Idea => LensKind::Idea,
        }
    }
}

/// A section's stable identity inside its page: a slug `[a-z0-9-]{1,48}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SectionKey(String);

impl SectionKey {
    pub fn new(value: &str) -> KnowledgeResult<Self> {
        if value.is_empty()
            || value.len() > MAX_KEY_LEN
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(KnowledgeError::Invalid(
                "section key must be a slug of lowercase letters, digits and hyphens",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SectionKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for SectionKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BasisRef {
    /// A World claim this turn read.
    Claim { claim_id: String },
    /// A retained Source span read or saved this turn.
    Span {
        record_id: String,
        revision_digest: String,
        span_id: String,
    },
    /// Host-set: the user message of the filing turn.
    #[serde(rename = "said")]
    UserSaid { message_id: String },
    /// Another Lens page: synthesis, not evidence.
    Page {
        record_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section_key: Option<SectionKey>,
    },
}

impl BasisRef {
    pub fn is_evidence(&self) -> bool {
        matches!(
            self,
            Self::Claim { .. } | Self::Span { .. } | Self::UserSaid { .. }
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Basis {
    #[serde(default)]
    pub refs: Vec<BasisRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<String>,
}

/// The chip a reader shows for a basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasisChip {
    Source,
    Said,
    View,
    None,
}

impl Basis {
    pub fn is_synthesis(&self) -> bool {
        self.refs.iter().any(|r| matches!(r, BasisRef::Page { .. }))
            || !self.refs.iter().any(BasisRef::is_evidence)
    }

    pub fn chip(&self) -> BasisChip {
        if self.refs.is_empty() && self.uncertainty.is_none() {
            BasisChip::None
        } else if self.is_synthesis() {
            BasisChip::View
        } else if self
            .refs
            .iter()
            .any(|r| matches!(r, BasisRef::Claim { .. } | BasisRef::Span { .. }))
        {
            BasisChip::Source
        } else {
            BasisChip::Said
        }
    }
}

/// Subject and idea sections need a basis; synthesis must say how sure it is.
pub fn basis_allowed(kind: LensKind, basis: &Basis) -> bool {
    let needs_ref = matches!(kind, LensKind::Subject | LensKind::Idea);
    if needs_ref && basis.refs.is_empty() {
        return false;
    }
    if !basis.refs.is_empty() && basis.is_synthesis() {
        return basis
            .uncertainty
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty());
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    User,
    Loupist,
    Learning,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SectionState {
    Current,
    Contested {
        since_ms: i64,
        claim_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSection {
    pub key: SectionKey,
    pub heading: String,
    pub markdown: String,
    #[serde(default)]
    pub basis: Basis,
    pub origin: Origin,
    #[serde(default = "current_state")]
    pub state: SectionState,
    pub updated_at_ms: i64,
}

fn current_state() -> SectionState {
    SectionState::Current
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub sections: Vec<PageSection>,
}

impl Page {
    pub fn section(&self, key: &SectionKey) -> Option<&PageSection> {
        self.sections.iter().find(|section| &section.key == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeaStatus {
    Open,
    Supported,
    Weakened,
    Refuted,
    Retired,
}

impl IdeaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Supported => "supported",
            Self::Weakened => "weakened",
            Self::Refuted => "refuted",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaState {
    pub status: IdeaStatus,
    pub registered_at_ms: i64,
    pub status_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LensBody {
    Principle(PrincipleInput),
    Perspective(Page),
    Method(Page),
    Subject(Page),
    Idea(Page, IdeaState),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdeaPayload {
    #[serde(flatten)]
    page: Page,
    idea: IdeaState,
}

impl LensBody {
    pub fn kind(&self) -> LensKind {
        match self {
            Self::Principle(_) => LensKind::Principle,
            Self::Perspective(_) => LensKind::Perspective,
            Self::Method(_) => LensKind::Method,
            Self::Subject(_) => LensKind::Subject,
            Self::Idea(..) => LensKind::Idea,
        }
    }

    pub fn empty(kind: PageKind, at_ms: i64) -> Self {
        match kind {
            PageKind::Perspective => Self::Perspective(Page::default()),
            PageKind::Method => Self::Method(Page::default()),
            PageKind::Subject => Self::Subject(Page::default()),
            PageKind::Idea => Self::Idea(
                Page::default(),
                IdeaState {
                    status: IdeaStatus::Open,
                    registered_at_ms: at_ms,
                    status_at_ms: at_ms,
                },
            ),
        }
    }

    pub fn page(&self) -> Option<&Page> {
        match self {
            Self::Principle(_) => None,
            Self::Perspective(page) | Self::Method(page) | Self::Subject(page) => Some(page),
            Self::Idea(page, _) => Some(page),
        }
    }

    fn page_mut(&mut self) -> Option<&mut Page> {
        match self {
            Self::Principle(_) => None,
            Self::Perspective(page) | Self::Method(page) | Self::Subject(page) => Some(page),
            Self::Idea(page, _) => Some(page),
        }
    }

    /// Marks every section whose claim basis names one of `claim_ids` as
    /// contested, keeping an earlier `since_ms`. Returns the keys that changed.
    pub fn contest(&mut self, claim_ids: &BTreeSet<String>, at_ms: i64) -> Vec<SectionKey> {
        let mut changed = Vec::new();
        let Some(page) = self.page_mut() else {
            return changed;
        };
        for section in &mut page.sections {
            let cited: Vec<String> = section
                .basis
                .refs
                .iter()
                .filter_map(|r| match r {
                    BasisRef::Claim { claim_id } if claim_ids.contains(claim_id) => {
                        Some(claim_id.clone())
                    }
                    _ => None,
                })
                .collect();
            if cited.is_empty() {
                continue;
            }
            let (since_ms, mut ids) = match &section.state {
                SectionState::Current => (at_ms, Vec::new()),
                SectionState::Contested {
                    since_ms,
                    claim_ids,
                } => (*since_ms, claim_ids.clone()),
            };
            let before = ids.len();
            ids.extend(cited);
            ids.sort();
            ids.dedup();
            if ids.len() == before && matches!(section.state, SectionState::Contested { .. }) {
                continue;
            }
            section.state = SectionState::Contested {
                since_ms,
                claim_ids: ids,
            };
            changed.push(section.key.clone());
        }
        changed
    }

    pub fn idea(&self) -> Option<&IdeaState> {
        match self {
            Self::Idea(_, state) => Some(state),
            _ => None,
        }
    }

    /// The stored payload. The kind lives in `knowledge_lens_metadata.kind`
    /// and never repeats here.
    pub fn to_payload(&self) -> KnowledgeResult<Value> {
        Ok(match self {
            Self::Principle(body) => serde_json::to_value(body)?,
            Self::Perspective(page) | Self::Method(page) | Self::Subject(page) => {
                serde_json::to_value(page)?
            }
            Self::Idea(page, idea) => serde_json::to_value(IdeaPayload {
                page: page.clone(),
                idea: idea.clone(),
            })?,
        })
    }

    pub fn from_payload(kind: LensKind, payload: Value) -> KnowledgeResult<Self> {
        Ok(match kind {
            LensKind::Principle => Self::Principle(serde_json::from_value(payload)?),
            LensKind::Perspective => Self::Perspective(serde_json::from_value(payload)?),
            LensKind::Method => Self::Method(serde_json::from_value(payload)?),
            LensKind::Subject => Self::Subject(serde_json::from_value(payload)?),
            LensKind::Idea => {
                let IdeaPayload { page, idea } = serde_json::from_value(payload)?;
                Self::Idea(page, idea)
            }
        })
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Principle(body) => body.statement.clone(),
            _ => self
                .page()
                .map(|page| page.summary.clone())
                .unwrap_or_default(),
        }
    }

    /// Every text a reader or a scan sees, title excluded.
    pub fn texts(&self) -> Vec<&str> {
        match self {
            Self::Principle(body) => [
                Some(body.statement.as_str()),
                body.why.as_deref(),
                body.example.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            _ => {
                let page = self.page().expect("page body");
                std::iter::once(page.summary.as_str())
                    .chain(page.sections.iter().flat_map(|section| {
                        [
                            section.heading.as_str(),
                            section.markdown.as_str(),
                            section.basis.uncertainty.as_deref().unwrap_or(""),
                        ]
                    }))
                    .collect()
            }
        }
    }
}

/// Suggested keys per kind: a hint for the tool description and the reader's
/// ordering, never a closed set.
pub fn suggested_sections(kind: LensKind) -> &'static [&'static str] {
    match kind {
        LensKind::Principle => &[],
        LensKind::Perspective => &["beliefs", "priorities", "avoid"],
        LensKind::Method => &["use-when", "steps", "checks", "limits"],
        LensKind::Subject => &[
            "overview",
            "facts",
            "value-chain",
            "drivers",
            "risks",
            "views",
            "open-questions",
        ],
        LensKind::Idea => &[
            "claim",
            "mechanism",
            "evidence-for",
            "evidence-against",
            "would-break",
            "checks",
        ],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AnchorInput {
    Node { node_id: String },
    Name { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PageTarget {
    Existing {
        record_id: String,
    },
    /// Idempotent create: resolves to the active page with the same
    /// (kind, title_fold), or for subject the same primary anchor, before
    /// minting a record.
    Create {
        kind: PageKind,
        title: String,
        #[serde(default)]
        anchors: Vec<AnchorInput>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum LensOp {
    /// Add or replace a section. `expected` is the digest from the exact read;
    /// None means "add only" (the key must be absent).
    Put {
        key: SectionKey,
        heading: String,
        markdown: String,
        #[serde(default)]
        basis: Basis,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected: Option<String>,
    },
    Retire {
        key: SectionKey,
        expected: String,
        why: String,
    },
    Summary {
        text: String,
    },
    IdeaStatus {
        status: IdeaStatus,
        why: String,
    },
}

impl LensOp {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Put { .. } => "put",
            Self::Retire { .. } => "retire",
            Self::Summary { .. } => "summary",
            Self::IdeaStatus { .. } => "idea_status",
        }
    }

    pub fn key(&self) -> Option<&SectionKey> {
        match self {
            Self::Put { key, .. } | Self::Retire { key, .. } => Some(key),
            _ => None,
        }
    }

    /// Limits only. Semantics (stale, forgotten, basis) are outcomes of `apply`.
    pub fn validate(&self) -> KnowledgeResult<()> {
        match self {
            Self::Put {
                heading,
                markdown,
                basis,
                expected,
                ..
            } => {
                let heading_ok = !heading.trim().is_empty()
                    && heading.chars().count() <= MAX_HEADING_CHARS
                    && !heading.contains(['\n', '\r', '\0']);
                if !heading_ok {
                    return Err(KnowledgeError::Invalid("section heading is invalid"));
                }
                if markdown.trim().is_empty()
                    || markdown.chars().count() > MAX_SECTION_CHARS
                    || markdown.contains('\0')
                {
                    return Err(KnowledgeError::Invalid("section markdown is invalid"));
                }
                if basis.refs.len() > MAX_BASIS_REFS {
                    return Err(KnowledgeError::Invalid("section cites too many references"));
                }
                if basis
                    .uncertainty
                    .as_deref()
                    .is_some_and(|text| text.chars().count() > MAX_UNCERTAINTY_CHARS)
                {
                    return Err(KnowledgeError::Invalid("section uncertainty is too long"));
                }
                if expected.as_deref().is_some_and(|d| d.len() != 64) {
                    return Err(KnowledgeError::Invalid("expected digest is invalid"));
                }
                Ok(())
            }
            Self::Retire { expected, why, .. } => {
                if expected.len() != 64 || why.chars().count() > MAX_WHY_CHARS {
                    return Err(KnowledgeError::Invalid("retire op is invalid"));
                }
                Ok(())
            }
            Self::Summary { text } => {
                if text.trim().is_empty()
                    || text.chars().count() > MAX_SUMMARY_CHARS
                    || text.contains(['\n', '\r', '\0'])
                {
                    return Err(KnowledgeError::Invalid("summary is invalid"));
                }
                Ok(())
            }
            Self::IdeaStatus { why, .. } => {
                if why.chars().count() > MAX_WHY_CHARS {
                    return Err(KnowledgeError::Invalid("status reason is too long"));
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDelta {
    pub target: PageTarget,
    pub ops: Vec<LensOp>,
}

impl PageDelta {
    pub fn validate(&self) -> KnowledgeResult<()> {
        if self.ops.is_empty() || self.ops.len() > MAX_OPS_PER_DELTA {
            return Err(KnowledgeError::Invalid("a page delta carries 1 to 12 ops"));
        }
        if let PageTarget::Create { title, anchors, .. } = &self.target {
            if title.trim().is_empty()
                || title.chars().count() > MAX_TITLE_CHARS
                || title.contains(['\n', '\r', '\0'])
            {
                return Err(KnowledgeError::Invalid("page title is invalid"));
            }
            if anchors.len() > 8 {
                return Err(KnowledgeError::Invalid("a page names at most 8 anchors"));
            }
        }
        self.ops.iter().try_for_each(LensOp::validate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OpOutcome {
    Added {
        key: SectionKey,
    },
    Replaced {
        key: SectionKey,
    },
    Retired {
        key: SectionKey,
    },
    Summarized,
    StatusSet {
        status: IdeaStatus,
    },
    /// Identical content.
    AlreadyKnown {
        key: SectionKey,
    },
    /// Digest mismatch or key missing; the text stays in the log.
    SkippedStale {
        key: SectionKey,
    },
    /// The user retired this key; only origin user may re-add it.
    RefusedForgotten {
        key: SectionKey,
    },
    RefusedBasis {
        key: SectionKey,
    },
    RefusedFull,
    RefusedKind,
}

impl OpOutcome {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Added { .. } => "added",
            Self::Replaced { .. } => "replaced",
            Self::Retired { .. } => "retired",
            Self::Summarized => "summarized",
            Self::StatusSet { .. } => "status_set",
            Self::AlreadyKnown { .. } => "already_known",
            Self::SkippedStale { .. } => "skipped_stale",
            Self::RefusedForgotten { .. } => "refused_forgotten",
            Self::RefusedBasis { .. } => "refused_basis",
            Self::RefusedFull => "refused_full",
            Self::RefusedKind => "refused_kind",
        }
    }

    pub fn key(&self) -> Option<&SectionKey> {
        match self {
            Self::Added { key }
            | Self::Replaced { key }
            | Self::Retired { key }
            | Self::AlreadyKnown { key }
            | Self::SkippedStale { key }
            | Self::RefusedForgotten { key }
            | Self::RefusedBasis { key } => Some(key),
            Self::Summarized | Self::StatusSet { .. } | Self::RefusedFull | Self::RefusedKind => {
                None
            }
        }
    }

    /// The page changed because of this op.
    pub fn changed(&self) -> bool {
        matches!(
            self,
            Self::Added { .. }
                | Self::Replaced { .. }
                | Self::Retired { .. }
                | Self::Summarized
                | Self::StatusSet { .. }
        )
    }
}

pub struct ApplyContext {
    pub origin: Origin,
    pub at_ms: i64,
    /// Keys the user retired on this page and did not re-add.
    pub forgotten: BTreeSet<SectionKey>,
}

/// Folds ops into a body. Never fails on one bad op; every op gets an outcome.
pub fn apply(
    body: &LensBody,
    ops: &[LensOp],
    ctx: &ApplyContext,
) -> KnowledgeResult<(LensBody, Vec<OpOutcome>)> {
    let kind = body.kind();
    let mut next = body.clone();
    let mut outcomes = Vec::with_capacity(ops.len());
    for op in ops {
        op.validate()?;
        let outcome = match op {
            LensOp::Put {
                key,
                heading,
                markdown,
                basis,
                expected,
            } => {
                let Some(page) = next.page_mut() else {
                    outcomes.push(OpOutcome::RefusedKind);
                    continue;
                };
                if ctx.origin != Origin::User && ctx.forgotten.contains(key) {
                    OpOutcome::RefusedForgotten { key: key.clone() }
                } else if !basis_allowed(kind, basis) {
                    OpOutcome::RefusedBasis { key: key.clone() }
                } else {
                    let incoming = PageSection {
                        key: key.clone(),
                        heading: heading.clone(),
                        markdown: markdown.clone(),
                        basis: basis.clone(),
                        origin: ctx.origin,
                        state: SectionState::Current,
                        updated_at_ms: ctx.at_ms,
                    };
                    match page.sections.iter().position(|s| &s.key == key) {
                        Some(index) => {
                            let current = &page.sections[index];
                            let current_digest = section_digest(current);
                            if current_digest == section_digest(&incoming) {
                                OpOutcome::AlreadyKnown { key: key.clone() }
                            } else if expected.as_deref() == Some(current_digest.as_str()) {
                                page.sections[index] = incoming;
                                OpOutcome::Replaced { key: key.clone() }
                            } else {
                                OpOutcome::SkippedStale { key: key.clone() }
                            }
                        }
                        None if expected.is_some() => OpOutcome::SkippedStale { key: key.clone() },
                        None if page.sections.len() >= MAX_SECTIONS => OpOutcome::RefusedFull,
                        None => {
                            page.sections.push(incoming);
                            OpOutcome::Added { key: key.clone() }
                        }
                    }
                }
            }
            LensOp::Retire { key, expected, .. } => {
                let Some(page) = next.page_mut() else {
                    outcomes.push(OpOutcome::RefusedKind);
                    continue;
                };
                match page.sections.iter().position(|s| &s.key == key) {
                    Some(index) if section_digest(&page.sections[index]) == *expected => {
                        page.sections.remove(index);
                        OpOutcome::Retired { key: key.clone() }
                    }
                    _ => OpOutcome::SkippedStale { key: key.clone() },
                }
            }
            LensOp::Summary { text } => match next.page_mut() {
                Some(page) => {
                    page.summary = text.trim().to_owned();
                    OpOutcome::Summarized
                }
                None => OpOutcome::RefusedKind,
            },
            LensOp::IdeaStatus { status, .. } => match &mut next {
                LensBody::Idea(_, state) => {
                    if state.status != *status {
                        state.status = *status;
                        state.status_at_ms = ctx.at_ms;
                    }
                    OpOutcome::StatusSet { status: *status }
                }
                _ => OpOutcome::RefusedKind,
            },
        };
        outcomes.push(outcome);
    }
    Ok((next, outcomes))
}

/// sha256 over key, heading, markdown and basis. Origin, state and time are
/// not identity: two actors editing the same text agree on the digest.
pub fn section_digest(section: &PageSection) -> String {
    let value = serde_json::json!({
        "key": section.key,
        "heading": section.heading,
        "markdown": section.markdown,
        "basis": section.basis,
    });
    sha256(canonical_string(&value).as_bytes())
}

pub fn render_markdown(title: &str, body: &LensBody, with_keys: bool) -> String {
    let mut out = format!("# {}\n", title.trim());
    match body {
        LensBody::Principle(principle) => {
            out.push_str(&format!("\n{}\n", principle.statement.trim()));
            for (heading, text) in [("Why", &principle.why), ("Example", &principle.example)] {
                if let Some(text) = text.as_deref().filter(|t| !t.trim().is_empty()) {
                    out.push_str(&format!("\n## {heading}\n\n{}\n", text.trim()));
                }
            }
        }
        _ => {
            let page = body.page().expect("page body");
            if let Some(idea) = body.idea() {
                out.push_str(&format!("\nStatus: {}\n", idea.status.as_str()));
            }
            if !page.summary.trim().is_empty() {
                out.push_str(&format!("\n{}\n", page.summary.trim()));
            }
            for section in &page.sections {
                if with_keys {
                    out.push_str(&format!(
                        "\n## {} {{#{}}}\n\n{}\n",
                        section.heading.trim(),
                        section.key,
                        section.markdown.trim()
                    ));
                } else {
                    out.push_str(&format!(
                        "\n## {}\n\n{}\n",
                        section.heading.trim(),
                        section.markdown.trim()
                    ));
                }
            }
        }
    }
    out
}

fn link_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '?' | '#'))
}

/// `lens:` record ids and `world:` node ids linked from a body.
pub fn links(body: &LensBody) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut lens = BTreeSet::new();
    let mut world = BTreeSet::new();
    for text in body.texts() {
        for event in Parser::new(text) {
            let Event::Start(Tag::Link { dest_url, .. }) = event else {
                continue;
            };
            if let Some(id) = dest_url.strip_prefix("lens:").filter(|id| link_id_ok(id)) {
                lens.insert(id.to_owned());
            } else if let Some(id) = dest_url.strip_prefix("world:").filter(|id| link_id_ok(id)) {
                world.insert(id.to_owned());
            }
        }
    }
    (lens, world)
}

/// Text no Lens body may carry: Journal references, private snapshot or
/// journal ids and the private `world://` forms. Portable kinds may not link
/// `world:` nodes either; subject and idea pages may.
pub fn text_forbidden(kind: LensKind, text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let private = lower.contains("journal://")
        || lower.contains("[[journal:")
        || lower.contains("world://")
        || lower.contains("[[world:")
        || !id_candidates(text, "jnl_").is_empty()
        || !id_candidates(text, "snp_").is_empty();
    if private {
        return true;
    }
    kind.is_portable() && (lower.contains("](world:") || lower.contains("<world:"))
}

/// Canonical knowledge ids (`prefix` + 32 lowercase hex) found in `text`.
pub fn id_candidates(text: &str, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let width = prefix.len() + 32;
    let mut index = 0;
    while index + width <= bytes.len() {
        if bytes[index..].starts_with(prefix.as_bytes())
            && boundary_before(text, index)
            && bytes[index + prefix.len()..index + width]
                .iter()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && boundary_after(text, index + width)
        {
            found.push(text[index..index + width].to_string());
            index += width;
            continue;
        }
        index += 1;
    }
    found
}

fn boundary_before(text: &str, index: usize) -> bool {
    index == 0 || !is_id_character(text[..index].chars().next_back().unwrap_or(' '))
}

fn boundary_after(text: &str, index: usize) -> bool {
    text[index..]
        .chars()
        .next()
        .is_none_or(|character| !is_id_character(character))
}

fn is_id_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

/// Who filed, as the writer records it in the log and on sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilingOrigin {
    /// Filed by the `lens_file` tool at turn commit (the harness unit).
    #[allow(dead_code)]
    Turn {
        chat_id: String,
        message_id: String,
        user_message_id: Option<String>,
        unattended: bool,
    },
    User {
        chat_id: Option<String>,
        message_id: Option<String>,
    },
    Learning {
        review_id: String,
        world_job_id: String,
    },
    Import {
        pack: Value,
    },
}

impl FilingOrigin {
    pub fn section_origin(&self) -> Origin {
        match self {
            Self::Turn { .. } => Origin::Loupist,
            Self::User { .. } => Origin::User,
            Self::Learning { .. } => Origin::Learning,
            Self::Import { .. } => Origin::Shared,
        }
    }

    /// The log's origin column.
    pub fn log_name(&self) -> &'static str {
        match self {
            Self::Turn { .. } => "turn",
            Self::User { .. } => "user",
            Self::Learning { .. } => "learning",
            Self::Import { .. } => "import",
        }
    }

    /// The growth event actor.
    pub fn actor(&self) -> &'static str {
        match self {
            Self::Turn { .. } | Self::Learning { .. } => "loupist",
            Self::User { .. } => "user",
            Self::Import { .. } => "import",
        }
    }

    pub fn chat_id(&self) -> Option<&str> {
        match self {
            Self::Turn { chat_id, .. } => Some(chat_id),
            Self::User { chat_id, .. } => chat_id.as_deref(),
            _ => None,
        }
    }

    pub fn message_id(&self) -> Option<&str> {
        match self {
            Self::Turn { message_id, .. } => Some(message_id),
            Self::User { message_id, .. } => message_id.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiledOp {
    pub op_id: String,
    #[serde(flatten)]
    pub outcome: OpOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiledPage {
    pub record_id: String,
    pub revision_id: String,
    pub title: String,
    pub kind: LensKind,
    pub created: bool,
    /// The page body changed in this filing.
    pub changed: bool,
    pub outcomes: Vec<FiledOp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> SectionKey {
        SectionKey::new(value).unwrap()
    }

    fn claim() -> Basis {
        Basis {
            refs: vec![BasisRef::Claim {
                claim_id: "clm_1".into(),
            }],
            uncertainty: None,
        }
    }

    fn put(k: &str, markdown: &str, basis: Basis, expected: Option<String>) -> LensOp {
        LensOp::Put {
            key: key(k),
            heading: k.to_uppercase(),
            markdown: markdown.into(),
            basis,
            expected,
        }
    }

    fn ctx(origin: Origin) -> ApplyContext {
        ApplyContext {
            origin,
            at_ms: 10,
            forgotten: BTreeSet::new(),
        }
    }

    fn subject_with(sections: &[(&str, &str)]) -> LensBody {
        let (body, outcomes) = apply(
            &LensBody::empty(PageKind::Subject, 1),
            &sections
                .iter()
                .map(|(k, text)| put(k, text, claim(), None))
                .collect::<Vec<_>>(),
            &ctx(Origin::Loupist),
        )
        .unwrap();
        assert!(outcomes
            .iter()
            .all(|o| matches!(o, OpOutcome::Added { .. })));
        body
    }

    fn digest_of(body: &LensBody, k: &str) -> String {
        section_digest(body.page().unwrap().section(&key(k)).unwrap())
    }

    #[test]
    fn section_keys_are_validated_slugs() {
        assert!(SectionKey::new("value-chain-2").is_ok());
        for bad in ["", "Facts", "a b", "ü", &"a".repeat(49)] {
            assert!(SectionKey::new(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<SectionKey>("\"Bad Key\"").is_err());
    }

    #[test]
    fn put_adds_replaces_by_digest_and_reports_known_or_stale() {
        let body = subject_with(&[("facts", "Revenue rose.")]);
        let digest = digest_of(&body, "facts");
        let (next, outcomes) = apply(
            &body,
            &[
                put("facts", "Revenue rose 10%.", claim(), Some(digest.clone())),
                put("facts", "Revenue rose.", claim(), Some(digest.clone())),
                put("drivers", "Pricing.", claim(), None),
            ],
            &ctx(Origin::Loupist),
        )
        .unwrap();
        assert_eq!(
            outcomes,
            vec![
                OpOutcome::Replaced { key: key("facts") },
                OpOutcome::SkippedStale { key: key("facts") },
                OpOutcome::Added {
                    key: key("drivers")
                },
            ]
        );
        assert_eq!(
            next.page()
                .unwrap()
                .section(&key("facts"))
                .unwrap()
                .markdown,
            "Revenue rose 10%."
        );
        let (_, again) = apply(
            &next,
            &[
                put("facts", "Revenue rose 10%.", claim(), None),
                put("missing", "x", claim(), Some(digest.clone())),
                put("facts", "Different.", claim(), None),
            ],
            &ctx(Origin::Loupist),
        )
        .unwrap();
        assert_eq!(
            again,
            vec![
                OpOutcome::AlreadyKnown { key: key("facts") },
                OpOutcome::SkippedStale {
                    key: key("missing")
                },
                OpOutcome::SkippedStale { key: key("facts") },
            ]
        );
    }

    #[test]
    fn disjoint_edits_from_the_same_read_both_land() {
        let body = subject_with(&[("facts", "A"), ("risks", "B")]);
        let facts = digest_of(&body, "facts");
        let risks = digest_of(&body, "risks");
        let (after_first, first) = apply(
            &body,
            &[put("facts", "A2", claim(), Some(facts))],
            &ctx(Origin::Loupist),
        )
        .unwrap();
        let (after_second, second) = apply(
            &after_first,
            &[put("risks", "B2", claim(), Some(risks))],
            &ctx(Origin::Learning),
        )
        .unwrap();
        assert!(matches!(first[0], OpOutcome::Replaced { .. }));
        assert!(matches!(second[0], OpOutcome::Replaced { .. }));
        let page = after_second.page().unwrap();
        assert_eq!(page.section(&key("facts")).unwrap().markdown, "A2");
        assert_eq!(page.section(&key("risks")).unwrap().markdown, "B2");
        assert_eq!(
            page.section(&key("risks")).unwrap().origin,
            Origin::Learning
        );
    }

    #[test]
    fn forgotten_keys_refuse_everyone_but_the_user() {
        let body = subject_with(&[("facts", "A")]);
        let mut context = ctx(Origin::Learning);
        context.forgotten.insert(key("facts"));
        context.forgotten.insert(key("risks"));
        let (_, outcomes) = apply(&body, &[put("risks", "B", claim(), None)], &context).unwrap();
        assert_eq!(
            outcomes,
            vec![OpOutcome::RefusedForgotten { key: key("risks") }]
        );
        let mut user = ctx(Origin::User);
        user.forgotten.insert(key("risks"));
        let (_, outcomes) = apply(&body, &[put("risks", "B", claim(), None)], &user).unwrap();
        assert_eq!(outcomes, vec![OpOutcome::Added { key: key("risks") }]);
    }

    #[test]
    fn basis_rule_refuses_evidence_free_subject_and_uncertain_synthesis() {
        let body = LensBody::empty(PageKind::Subject, 1);
        let synthesis = Basis {
            refs: vec![BasisRef::Page {
                record_id: "rec_x".into(),
                section_key: None,
            }],
            uncertainty: None,
        };
        let mut sure = synthesis.clone();
        sure.uncertainty = Some("Inferred from two pages.".into());
        let (_, outcomes) = apply(
            &body,
            &[
                put("facts", "A", Basis::default(), None),
                put("views", "B", synthesis, None),
                put("views", "B", sure, None),
            ],
            &ctx(Origin::Loupist),
        )
        .unwrap();
        assert_eq!(
            outcomes,
            vec![
                OpOutcome::RefusedBasis { key: key("facts") },
                OpOutcome::RefusedBasis { key: key("views") },
                OpOutcome::Added { key: key("views") },
            ]
        );
        assert!(basis_allowed(LensKind::Method, &Basis::default()));
        assert!(basis_allowed(LensKind::Perspective, &Basis::default()));
        assert!(!basis_allowed(LensKind::Idea, &Basis::default()));
        assert_eq!(claim().chip(), BasisChip::Source);
        assert_eq!(Basis::default().chip(), BasisChip::None);
    }

    #[test]
    fn full_page_refuses_a_seventeenth_section() {
        let keys: Vec<String> = (0..MAX_SECTIONS).map(|i| format!("s{i}")).collect();
        let sections: Vec<(&str, &str)> = keys.iter().map(|k| (k.as_str(), "x")).collect();
        let body = subject_with(&sections);
        let (_, outcomes) = apply(
            &body,
            &[put("one-more", "y", claim(), None)],
            &ctx(Origin::Loupist),
        )
        .unwrap();
        assert_eq!(outcomes, vec![OpOutcome::RefusedFull]);
    }

    #[test]
    fn retire_summary_and_status_outcomes() {
        let body = subject_with(&[("facts", "A")]);
        let digest = digest_of(&body, "facts");
        let (next, outcomes) = apply(
            &body,
            &[
                LensOp::Summary {
                    text: "One line.".into(),
                },
                LensOp::IdeaStatus {
                    status: IdeaStatus::Supported,
                    why: String::new(),
                },
                LensOp::Retire {
                    key: key("facts"),
                    expected: "0".repeat(64),
                    why: "wrong".into(),
                },
                LensOp::Retire {
                    key: key("facts"),
                    expected: digest,
                    why: "wrong".into(),
                },
            ],
            &ctx(Origin::User),
        )
        .unwrap();
        assert_eq!(
            outcomes,
            vec![
                OpOutcome::Summarized,
                OpOutcome::RefusedKind,
                OpOutcome::SkippedStale { key: key("facts") },
                OpOutcome::Retired { key: key("facts") },
            ]
        );
        assert_eq!(next.page().unwrap().summary, "One line.");
        assert!(next.page().unwrap().sections.is_empty());
        let idea = LensBody::empty(PageKind::Idea, 5);
        let (idea, outcomes) = apply(
            &idea,
            &[LensOp::IdeaStatus {
                status: IdeaStatus::Weakened,
                why: "counter-evidence".into(),
            }],
            &ctx(Origin::Loupist),
        )
        .unwrap();
        assert_eq!(
            outcomes,
            vec![OpOutcome::StatusSet {
                status: IdeaStatus::Weakened
            }]
        );
        assert_eq!(idea.idea().unwrap().status_at_ms, 10);
        let principle = LensBody::Principle(PrincipleInput {
            statement: "Check cash".into(),
            why: None,
            example: None,
            origin: "settings".into(),
            origin_chat_id: None,
            origin_message_id: None,
        });
        let (_, outcomes) = apply(
            &principle,
            &[put("facts", "A", claim(), None)],
            &ctx(Origin::User),
        )
        .unwrap();
        assert_eq!(outcomes, vec![OpOutcome::RefusedKind]);
    }

    #[test]
    fn payload_round_trips_without_repeating_the_kind() {
        let body = subject_with(&[("facts", "A")]);
        let payload = body.to_payload().unwrap();
        assert!(payload.get("kind").is_none());
        assert_eq!(
            LensBody::from_payload(LensKind::Subject, payload.clone()).unwrap(),
            body
        );
        let idea = LensBody::empty(PageKind::Idea, 3);
        let payload = idea.to_payload().unwrap();
        assert_eq!(payload["idea"]["status"], "open");
        assert_eq!(
            LensBody::from_payload(LensKind::Idea, payload).unwrap(),
            idea
        );
    }

    #[test]
    fn render_links_and_forbidden_text() {
        let body = subject_with(&[("facts", "See [NVDA](world:nod_1) and [method](lens:rec_2).")]);
        let text = render_markdown("NVIDIA", &body, true);
        assert!(text.starts_with("# NVIDIA\n"));
        assert!(text.contains("## FACTS {#facts}"));
        assert!(!render_markdown("NVIDIA", &body, false).contains("{#facts}"));
        let (lens, world) = links(&body);
        assert_eq!(lens.into_iter().collect::<Vec<_>>(), vec!["rec_2"]);
        assert_eq!(world.into_iter().collect::<Vec<_>>(), vec!["nod_1"]);
        assert!(text_forbidden(LensKind::Method, "[x](world:nod_1)"));
        assert!(!text_forbidden(LensKind::Subject, "[x](world:nod_1)"));
        assert!(text_forbidden(LensKind::Subject, "see world://accounts"));
        assert!(text_forbidden(LensKind::Idea, "[[world:accounts]]"));
        assert!(text_forbidden(
            LensKind::Subject,
            &format!("jnl_{}", "ab".repeat(16))
        ));
        assert!(!text_forbidden(LensKind::Subject, "plain prose"));
        assert_eq!(suggested_sections(LensKind::Method)[0], "use-when");
    }
}
