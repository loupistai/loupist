//! The one Lens page writer and its readers. `lens_page` decides what a filing
//! means; this module stores it: one head revision per page, an append-only
//! op log, links, anchors, growth events and the search index.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::authority::{require_edit, require_record, OwnedRecordState};
use super::canonical::{canonical_string, from_json, to_value};
use super::error::{KnowledgeError, KnowledgeResult};
use super::lens_page::{
    apply, links, op_texts, private_reference, section_digest, text_forbidden, world_link_ids,
    AnchorInput, ApplyContext, Basis, BasisChip, BasisRef, FiledOp, FiledPage, FilingOrigin,
    IdeaState, IdeaStatus, LensBody, LensKind, LensOp, OpOutcome, Origin, PageDelta, PageKind,
    PageSection, PageTarget, SectionKey, SectionState, TextField, TextRefusal,
};
use super::model::{
    mint_id, validate_knowledge_id, validate_loupist_id, validate_time, ClaimStatus, GraphNode,
    RecordKind, WorldSnapshotBody,
};
use super::revision::{load_revision, publish_revision, StoredRevision};

const HISTORY_LIMIT: usize = 20;
const LINK_LIMIT: usize = 64;

pub(super) fn fold_lens_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The stored kind of a Lens record.
pub(super) fn lens_kind(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<LensKind> {
    let kind: Option<String> = tx
        .query_row(
            "SELECT kind FROM knowledge_lens_metadata WHERE record_id = ?1 AND loupist_id = ?2",
            params![record_id, loupist_id],
            |row| row.get(0),
        )
        .optional()?;
    kind.as_deref()
        .and_then(LensKind::parse)
        .ok_or(KnowledgeError::NotFound)
}

/// Whether an active page of `kind` already uses `title`.
pub(super) fn title_taken(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: LensKind,
    title: &str,
) -> KnowledgeResult<bool> {
    Ok(active_by_title(tx, loupist_id, kind, title)?.is_some())
}

fn active_by_title(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: LensKind,
    title: &str,
) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT record_id FROM knowledge_lens_metadata
             WHERE loupist_id = ?1 AND kind = ?2 AND title_fold = ?3 AND archived_at_ms IS NULL",
            params![loupist_id, kind.as_str(), fold_lens_title(title)],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn active_perspective(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT record_id FROM knowledge_lens_metadata
             WHERE loupist_id = ?1 AND kind = 'perspective' AND archived_at_ms IS NULL",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?)
}

struct Loaded {
    record: OwnedRecordState,
    kind: LensKind,
    revision: Option<StoredRevision>,
    body: LensBody,
}

fn load(tx: &Transaction<'_>, loupist_id: &str, record_id: &str) -> KnowledgeResult<Loaded> {
    let record = require_record(tx, loupist_id, record_id)?;
    if record.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    let kind = lens_kind(tx, loupist_id, record_id)?;
    let revision = record
        .head_revision_id
        .as_deref()
        .map(|id| load_revision(tx, id))
        .transpose()?;
    let body = match (&revision, kind.page_kind()) {
        (Some(revision), _) => LensBody::from_payload(kind, from_json(&revision.payload_json)?)?,
        (None, Some(page_kind)) => LensBody::empty(page_kind, 0),
        (None, None) => return Err(KnowledgeError::NotFound),
    };
    Ok(Loaded {
        record,
        kind,
        revision,
        body,
    })
}

/// Node ids, folded names and active claims of the Loupist's current World.
/// A name anchor binds to a node only through an exact folded name or alias:
/// a company key is a hint for learning, never an identity for storage.
#[derive(Default)]
struct WorldIndex {
    ids: BTreeSet<String>,
    names: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
    active_claims: BTreeSet<String>,
}

/// The Loupist's current World snapshot, if one was published.
pub(super) fn world_body(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<WorldSnapshotBody>> {
    let payload: Option<String> = tx
        .query_row(
            "SELECT v.payload_json FROM knowledge_records r
             JOIN knowledge_record_heads h ON h.record_id = r.record_id
             JOIN knowledge_record_revisions v ON v.revision_id = h.revision_id
             WHERE r.owner_loupist_id = ?1 AND r.kind = 'world'",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?;
    payload.map(|payload| from_json(&payload)).transpose()
}

fn world_index(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<WorldIndex> {
    let mut index = WorldIndex::default();
    let Some(body) = world_body(tx, loupist_id)? else {
        return Ok(index);
    };
    index_nodes(&mut index, &body.nodes);
    index.active_claims = body
        .claims
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Active)
        .map(|claim| claim.id.clone())
        .collect();
    Ok(index)
}

/// The head revision of the Loupist's World, if one was published.
fn world_head(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT h.revision_id FROM knowledge_records r
             JOIN knowledge_record_heads h ON h.record_id = r.record_id
             WHERE r.owner_loupist_id = ?1 AND r.kind = 'world'",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?)
}

/// Reads the current World through its identity index
/// ([`super::identity::WorldIdentity`]): the company keys of a node's name and
/// aliases, the component of nodes whose keys meet, and the companies whose
/// names may name a pending page's subject. Method learning reads it to see
/// when two nodes might spell one company and who a pending page is about.
/// None when no World was published. The World is parsed once per revision,
/// not on every filing.
pub(super) fn world_identity<R>(
    tx: &Transaction<'_>,
    loupist_id: &str,
    read: impl FnOnce(&mut super::identity::WorldIdentity) -> R,
) -> KnowledgeResult<Option<R>> {
    let Some(revision) = world_head(tx, loupist_id)? else {
        return Ok(None);
    };
    super::identity::with_world_identity(
        loupist_id,
        &revision,
        || from_json(&load_revision(tx, &revision)?.payload_json),
        read,
    )
    .map(Some)
}

fn index_nodes(index: &mut WorldIndex, nodes: &[GraphNode]) {
    for node in nodes {
        index.ids.insert(node.id.clone());
        index.labels.insert(node.id.clone(), node.name.clone());
        for name in std::iter::once(&node.name).chain(&node.aliases) {
            index
                .names
                .entry(fold_lens_title(name))
                .or_insert_with(|| node.id.clone());
        }
    }
}

/// Keys the user retired on this page and did not re-add since.
fn forgotten_keys(tx: &Transaction<'_>, record_id: &str) -> KnowledgeResult<BTreeSet<SectionKey>> {
    let mut statement = tx.prepare(
        "SELECT section_key, outcome FROM knowledge_lens_log
         WHERE record_id = ?1 AND origin = 'user' AND section_key IS NOT NULL
         ORDER BY created_at_ms, rowid",
    )?;
    let rows = statement.query_map([record_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut forgotten = BTreeSet::new();
    for row in rows {
        let (key, outcome) = row?;
        let Ok(key) = SectionKey::new(&key) else {
            continue;
        };
        match outcome.as_str() {
            "retired" => {
                forgotten.insert(key);
            }
            "added" | "replaced" => {
                forgotten.remove(&key);
            }
            _ => {}
        }
    }
    Ok(forgotten)
}

enum Resolved {
    Found(Box<Loaded>),
    /// No active page matched; the record is minted only once an op changes it.
    Pending {
        kind: PageKind,
        title: String,
        anchors: Vec<AnchorInput>,
    },
}

fn resolve_target(
    tx: &Transaction<'_>,
    loupist_id: &str,
    target: &PageTarget,
    world: &mut Option<WorldIndex>,
) -> KnowledgeResult<Resolved> {
    match target {
        PageTarget::Existing { record_id } => {
            let loaded = load(tx, loupist_id, record_id)?;
            if loaded.record.archived_at_ms.is_some() {
                return Err(KnowledgeError::Invalid(
                    "restore this page before filing to it",
                ));
            }
            Ok(Resolved::Found(Box::new(loaded)))
        }
        PageTarget::Create {
            kind,
            title,
            anchors,
        } => Ok(
            match create_resolves_to(tx, loupist_id, *kind, title, anchors, world)? {
                Some(record_id) => Resolved::Found(Box::new(load(tx, loupist_id, &record_id)?)),
                None => Resolved::Pending {
                    kind: *kind,
                    title: title.trim().to_owned(),
                    anchors: anchors.clone(),
                },
            },
        ),
    }
}

/// The existing page a delta targets when it was archived or removed: the
/// filed page with no outcomes yet.
fn inactive_target(
    tx: &Transaction<'_>,
    loupist_id: &str,
    target: &PageTarget,
) -> KnowledgeResult<Option<FiledPage>> {
    let PageTarget::Existing { record_id } = target else {
        return Ok(None);
    };
    let Some((title, kind, archived, revision_id)) = tx
        .query_row(
            "SELECT m.title, m.kind, m.archived_at_ms IS NOT NULL, h.revision_id
             FROM knowledge_lens_metadata m
             LEFT JOIN knowledge_record_heads h ON h.record_id = m.record_id
             WHERE m.record_id = ?1 AND m.loupist_id = ?2",
            params![record_id, loupist_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    if !archived
        && !super::memory_controls::load(tx, loupist_id)?
            .removed
            .contains(record_id)
    {
        return Ok(None);
    }
    Ok(Some(FiledPage {
        record_id: record_id.clone(),
        revision_id: revision_id.unwrap_or_default(),
        title,
        kind: LensKind::parse(&kind).ok_or(KnowledgeError::NotFound)?,
        created: false,
        changed: false,
        outcomes: Vec::new(),
    }))
}

/// The active page an idempotent create lands on: the perspective, a
/// subject with the same primary anchor, or the page with the same kind and
/// folded title.
fn create_resolves_to(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: PageKind,
    title: &str,
    anchors: &[AnchorInput],
    world: &mut Option<WorldIndex>,
) -> KnowledgeResult<Option<String>> {
    let mut found = if kind == PageKind::Perspective {
        active_perspective(tx, loupist_id)?
    } else {
        None
    };
    if found.is_none() && kind == PageKind::Subject {
        found = subject_by_anchor(tx, loupist_id, anchors, world)?;
    }
    if found.is_none() {
        found = active_by_title(tx, loupist_id, kind.lens_kind(), title)?;
    }
    Ok(found)
}

fn subject_by_anchor(
    tx: &Transaction<'_>,
    loupist_id: &str,
    anchors: &[AnchorInput],
    world: &mut Option<WorldIndex>,
) -> KnowledgeResult<Option<String>> {
    let Some(primary) = anchors.first() else {
        return Ok(None);
    };
    let (node_id, name_fold) = match primary {
        AnchorInput::Node { node_id } => (Some(node_id.clone()), String::new()),
        AnchorInput::Name { name } => {
            let fold = fold_lens_title(name);
            let node = ensure_world(tx, loupist_id, world)?
                .names
                .get(&fold)
                .cloned();
            (node, fold)
        }
    };
    Ok(tx
        .query_row(
            "SELECT a.record_id FROM knowledge_lens_anchors a
             JOIN knowledge_lens_metadata m ON m.record_id = a.record_id
             WHERE a.loupist_id = ?1 AND a.role = 'subject' AND m.kind = 'subject'
               AND m.archived_at_ms IS NULL
               AND ((?2 <> '' AND a.node_id = ?2) OR (?3 <> '' AND a.name_fold = ?3))
             ORDER BY m.updated_at_ms DESC LIMIT 1",
            params![loupist_id, node_id.unwrap_or_default(), name_fold],
            |row| row.get(0),
        )
        .optional()?)
}

fn ensure_world<'a>(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world: &'a mut Option<WorldIndex>,
) -> KnowledgeResult<&'a WorldIndex> {
    if world.is_none() {
        *world = Some(world_index(tx, loupist_id)?);
    }
    Ok(world.as_ref().expect("world index loaded"))
}

fn create_page(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: PageKind,
    title: &str,
    anchors: &[AnchorInput],
    world: &mut Option<WorldIndex>,
    at_ms: i64,
) -> KnowledgeResult<Loaded> {
    let record_id = mint_id("rec");
    let lens_kind = kind.lens_kind();
    tx.execute(
        "INSERT INTO knowledge_records(record_id, kind, owner_loupist_id, title, created_at_ms) VALUES (?1, 'lens', ?2, ?3, ?4)",
        params![record_id, loupist_id, title, at_ms],
    )?;
    tx.execute(
        "INSERT INTO knowledge_lens_metadata (record_id, loupist_id, title, title_fold, kind, archived_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
        params![record_id, loupist_id, title, fold_lens_title(title), lens_kind.as_str(), at_ms],
    )?;
    for anchor in anchors {
        let (node_id, name_fold) = match anchor {
            AnchorInput::Node { node_id } => {
                if !ensure_world(tx, loupist_id, world)?.ids.contains(node_id) {
                    return Err(KnowledgeError::Invalid(
                        "anchor names an unknown World node",
                    ));
                }
                (Some(node_id.clone()), String::new())
            }
            AnchorInput::Name { name } => {
                let fold = fold_lens_title(name);
                let node = ensure_world(tx, loupist_id, world)?
                    .names
                    .get(&fold)
                    .cloned();
                (node, fold)
            }
        };
        tx.execute(
            "INSERT OR IGNORE INTO knowledge_lens_anchors (loupist_id, record_id, role, node_id, name_fold, turn_id, created_at_ms)
             VALUES (?1, ?2, 'subject', ?3, ?4, NULL, ?5)",
            params![loupist_id, record_id, node_id, name_fold, at_ms],
        )?;
    }
    load(tx, loupist_id, &record_id)
}

/// Text policy for any text a page stores: private references never enter a
/// page, and a canonical record or revision id must belong to a Source or a
/// Lens page of this database. A World record or revision id, or an id this
/// database cannot identify, is forbidden. Another Loupist's Source or page
/// stays as an unresolved link.
pub(super) fn check_text(tx: &Transaction<'_>, kind: LensKind, text: &str) -> KnowledgeResult<()> {
    if text_forbidden(kind, text) || unknown_id(tx, text)? {
        return Err(KnowledgeError::LensBodyForbidden);
    }
    Ok(())
}

/// A canonical record or revision id in `text` that is not a Source or a Lens page.
fn unknown_id(tx: &Transaction<'_>, text: &str) -> KnowledgeResult<bool> {
    for record_id in super::lens_page::id_candidates(text, "rec_") {
        let found: Option<String> = tx
            .query_row(
                "SELECT kind FROM knowledge_records WHERE record_id = ?1",
                [&record_id],
                |row| row.get(0),
            )
            .optional()?;
        if !matches!(found.as_deref(), Some("source" | "lens")) {
            return Ok(true);
        }
    }
    for revision_id in super::lens_page::id_candidates(text, "rev_") {
        let found: Option<String> = tx
            .query_row(
                "SELECT m.kind FROM knowledge_record_revisions r
                 JOIN knowledge_records m ON m.record_id = r.record_id
                 WHERE r.revision_id = ?1",
                [&revision_id],
                |row| row.get(0),
            )
            .optional()?;
        if !matches!(found.as_deref(), Some("source" | "lens")) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The text policy for one text of a page of `kind`, with every `world:`
/// link the Markdown reader sees naming a node of this Loupist's World.
fn text_refusal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: LensKind,
    text: &str,
    world: &mut Option<WorldIndex>,
) -> KnowledgeResult<Option<TextRefusal>> {
    if private_reference(text) || unknown_id(tx, text)? {
        return Ok(Some(TextRefusal::Private));
    }
    let world_links = world_link_ids(text);
    if world_links.is_empty() {
        return Ok(None);
    }
    if kind.is_portable() {
        return Ok(Some(TextRefusal::PortableWorldLink));
    }
    let index = ensure_world(tx, loupist_id, world)?;
    Ok(world_links
        .iter()
        .any(|id| !index.ids.contains(id))
        .then_some(TextRefusal::UnknownNode))
}

/// Every text of a delta the policy refuses, in field order: the create's
/// title and `about` nodes, then each op's texts.
fn text_refusals(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: LensKind,
    delta: &PageDelta,
    world: &mut Option<WorldIndex>,
) -> KnowledgeResult<Vec<(TextField, TextRefusal)>> {
    let mut refusals = Vec::new();
    if let PageTarget::Create { title, anchors, .. } = &delta.target {
        if let Some(reason) = text_refusal(tx, loupist_id, kind, title, world)? {
            refusals.push((TextField::Title, reason));
        }
        for (index, anchor) in anchors.iter().enumerate() {
            if let AnchorInput::Node { node_id } = anchor {
                if !ensure_world(tx, loupist_id, world)?.ids.contains(node_id) {
                    refusals.push((TextField::About(index), TextRefusal::UnknownNode));
                }
            }
        }
    }
    for (index, op) in delta.ops.iter().enumerate() {
        for (field, text) in op_texts(op) {
            if let Some(reason) = text_refusal(tx, loupist_id, kind, text, world)? {
                refusals.push((TextField::Op(index, field), reason));
            }
        }
    }
    Ok(refusals)
}

/// What the writer will make of one delta, known before the reply commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The active page a create resolves to.
    pub existing: Option<String>,
    /// Every text the policy refuses, in field order.
    pub refusals: Vec<(TextField, TextRefusal)>,
}

/// Screens one delta as the writer would see it now: the page a create
/// resolves to and the texts the policy refuses.
pub(super) fn screen(
    tx: &Transaction<'_>,
    loupist_id: &str,
    delta: &PageDelta,
) -> KnowledgeResult<Screen> {
    let mut world = None;
    let (kind, existing) = match &delta.target {
        PageTarget::Existing { record_id } => (lens_kind(tx, loupist_id, record_id)?, None),
        PageTarget::Create {
            kind,
            title,
            anchors,
        } => (
            kind.lens_kind(),
            create_resolves_to(tx, loupist_id, *kind, title, anchors, &mut world)?,
        ),
    };
    Ok(Screen {
        existing,
        refusals: text_refusals(tx, loupist_id, kind, delta, &mut world)?,
    })
}

/// Claim ids the delta's puts cite that the live World withdrew, disputed or
/// does not hold.
fn fallen_claims(
    tx: &Transaction<'_>,
    loupist_id: &str,
    ops: &[LensOp],
    world: &mut Option<WorldIndex>,
) -> KnowledgeResult<BTreeSet<String>> {
    let cited: BTreeSet<&str> = ops
        .iter()
        .filter_map(|op| match op {
            LensOp::Put { basis, .. } => Some(basis),
            _ => None,
        })
        .flat_map(|basis| &basis.refs)
        .filter_map(|reference| match reference {
            BasisRef::Claim { claim_id } => Some(claim_id.as_str()),
            _ => None,
        })
        .collect();
    if cited.is_empty() {
        return Ok(BTreeSet::new());
    }
    let active = &ensure_world(tx, loupist_id, world)?.active_claims;
    Ok(cited
        .into_iter()
        .filter(|id| !active.contains(*id))
        .map(str::to_owned)
        .collect())
}

/// One op of a filing as the fold met it.
struct Step {
    outcome: OpOutcome,
    before: LensBody,
}

/// Folds ops one at a time, so each log row records the body its op found.
/// An op the text policy refused changes nothing.
fn fold(
    base: &LensBody,
    ops: &[LensOp],
    refused: &BTreeSet<usize>,
    ctx: &ApplyContext,
) -> KnowledgeResult<(LensBody, Vec<Step>)> {
    let mut body = base.clone();
    let mut steps = Vec::with_capacity(ops.len());
    for (index, op) in ops.iter().enumerate() {
        if refused.contains(&index) {
            steps.push(Step {
                outcome: OpOutcome::RefusedText,
                before: body.clone(),
            });
            continue;
        }
        let (next, mut outcomes) = apply(&body, std::slice::from_ref(op), ctx)?;
        steps.push(Step {
            outcome: outcomes.pop().expect("one outcome per op"),
            before: std::mem::replace(&mut body, next),
        });
    }
    Ok((body, steps))
}

/// The only page writer. Resolves targets (idempotent create), folds every
/// delta with `apply`, publishes one revision per changed page, logs one row
/// per op, rebuilds links and mention anchors and emits one growth event per
/// changed page.
pub(super) fn file(
    tx: &Transaction<'_>,
    loupist_id: &str,
    origin: &FilingOrigin,
    deltas: Vec<PageDelta>,
    at_ms: i64,
) -> KnowledgeResult<Vec<FiledPage>> {
    file_with(tx, loupist_id, origin, deltas, at_ms, None)
}

fn file_with(
    tx: &Transaction<'_>,
    loupist_id: &str,
    origin: &FilingOrigin,
    deltas: Vec<PageDelta>,
    at_ms: i64,
    undoes_op_id: Option<&str>,
) -> KnowledgeResult<Vec<FiledPage>> {
    validate_loupist_id(loupist_id)?;
    validate_time(at_ms)?;
    for delta in &deltas {
        delta.validate()?;
    }
    let mut world: Option<WorldIndex> = None;
    let mut filed = Vec::with_capacity(deltas.len());
    for delta in deltas {
        // The user acts on a page they see and must restore it first; any
        // other filer read the page earlier and only logs the refusal.
        if !matches!(origin, FilingOrigin::User { .. }) {
            if let Some(page) = inactive_target(tx, loupist_id, &delta.target)? {
                let mut outcomes = Vec::with_capacity(delta.ops.len());
                for op in &delta.ops {
                    outcomes.push(FiledOp {
                        op_id: log_op(
                            tx,
                            loupist_id,
                            &page.record_id,
                            op,
                            &OpOutcome::RefusedInactive,
                            None,
                            origin,
                            at_ms,
                            undoes_op_id,
                        )?,
                        outcome: OpOutcome::RefusedInactive,
                    });
                }
                filed.push(FiledPage { outcomes, ..page });
                continue;
            }
        }
        let resolved = resolve_target(tx, loupist_id, &delta.target, &mut world)?;
        let (kind, base, record) = match &resolved {
            Resolved::Found(loaded) => {
                require_edit(&loaded.record, loupist_id)?;
                (
                    loaded.kind,
                    loaded.body.clone(),
                    Some(loaded.record.record_id.clone()),
                )
            }
            Resolved::Pending { kind, .. } => {
                (kind.lens_kind(), LensBody::empty(*kind, at_ms), None)
            }
        };
        // A refused title, or an unknown `about` node on a page about to be
        // minted, refuses the whole delta; any other refusal refuses its op.
        let refusals = text_refusals(tx, loupist_id, kind, &delta, &mut world)?;
        let refused: BTreeSet<usize> = if refusals.iter().any(|(field, _)| match field {
            TextField::Title => true,
            TextField::About(_) => record.is_none(),
            TextField::Op(..) => false,
        }) {
            (0..delta.ops.len()).collect()
        } else {
            refusals
                .iter()
                .filter_map(|(field, _)| match field {
                    TextField::Op(index, _) => Some(*index),
                    _ => None,
                })
                .collect()
        };
        let ctx = ApplyContext {
            origin: origin.section_origin(),
            at_ms,
            forgotten: match &record {
                Some(record_id) => forgotten_keys(tx, record_id)?,
                None => BTreeSet::new(),
            },
            add_only: record.is_some() && matches!(delta.target, PageTarget::Create { .. }),
            fallen_claims: fallen_claims(tx, loupist_id, &delta.ops, &mut world)?,
        };
        let (next, steps) = fold(&base, &delta.ops, &refused, &ctx)?;
        let (loaded, created) = match resolved {
            Resolved::Found(loaded) => (*loaded, false),
            Resolved::Pending {
                kind,
                title,
                anchors,
            } => {
                // A create whose ops all refuse leaves no record behind.
                if !steps.iter().any(|step| step.outcome.changed()) {
                    filed.push(FiledPage {
                        record_id: String::new(),
                        revision_id: String::new(),
                        title,
                        kind: kind.lens_kind(),
                        created: false,
                        changed: false,
                        outcomes: steps
                            .into_iter()
                            .map(|step| FiledOp {
                                op_id: String::new(),
                                outcome: step.outcome,
                            })
                            .collect(),
                    });
                    continue;
                }
                (
                    create_page(tx, loupist_id, kind, &title, &anchors, &mut world, at_ms)?,
                    true,
                )
            }
        };
        let record_id = loaded.record.record_id.clone();
        let changed = created || next != base;
        let mut revision_id = loaded
            .revision
            .as_ref()
            .map(|revision| revision.revision_id.clone())
            .unwrap_or_default();
        if changed {
            let published = publish_revision(
                tx,
                &loaded.record,
                loupist_id,
                loaded.record.head_revision_id.as_deref(),
                &next.to_payload()?,
                &[],
                at_ms,
            )?;
            revision_id = published.revision_id.clone();
            let learned = matches!(
                origin,
                FilingOrigin::Turn { .. } | FilingOrigin::Learning { .. }
            );
            tx.execute(
                "UPDATE knowledge_lens_metadata SET updated_at_ms = ?1, learned = learned OR ?3 WHERE record_id = ?2",
                params![at_ms, record_id, learned],
            )?;
            replace_links(tx, loupist_id, &record_id, &next, at_ms)?;
            super::search_index::index_payload(
                tx,
                None,
                &record_id,
                &published.revision_id,
                RecordKind::Lens,
                &loaded.record.title,
                &next.to_payload()?,
            )?;
            let mut event = super::growth::GrowthEvent::new(
                loupist_id,
                &record_id,
                at_ms,
                if created {
                    "lens_created"
                } else {
                    "lens_revised"
                },
                origin.actor(),
                &loaded.record.title,
            );
            event.before_summary = (!created)
                .then(|| super::growth::one_line(&summary_or_title(&base, &loaded.record.title)));
            event.after_summary = Some(super::growth::one_line(&summary_or_title(
                &next,
                &loaded.record.title,
            )));
            event.origin = growth_origin(origin);
            event.origin_chat_id = origin.chat_id().map(str::to_owned);
            event.origin_message_id = origin.message_id().map(str::to_owned);
            event.insert(tx)?;
        }
        let mut filed_ops = Vec::with_capacity(steps.len());
        for (index, (op, step)) in delta.ops.iter().zip(&steps).enumerate() {
            let after = steps.get(index + 1).map_or(&next, |later| &later.before);
            let op_id = log_op(
                tx,
                loupist_id,
                &record_id,
                op,
                &step.outcome,
                Some((&step.before, after)),
                origin,
                at_ms,
                undoes_op_id,
            )?;
            filed_ops.push(FiledOp {
                op_id,
                outcome: step.outcome.clone(),
            });
        }
        filed.push(FiledPage {
            record_id,
            revision_id,
            title: loaded.record.title,
            kind: loaded.kind,
            created,
            changed,
            outcomes: filed_ops,
        });
    }
    Ok(filed)
}

fn summary_or_title(body: &LensBody, title: &str) -> String {
    let summary = body.summary();
    if summary.trim().is_empty() {
        title.to_owned()
    } else {
        summary
    }
}

fn growth_origin(origin: &FilingOrigin) -> Option<Value> {
    match origin {
        FilingOrigin::Learning { review_id } => {
            Some(json!({"kind": "learning_review", "reviewId": review_id}))
        }
        FilingOrigin::Import { pack } => Some(pack.clone()),
        FilingOrigin::Turn { unattended, .. } => {
            unattended.then(|| json!({"kind": "unattended_turn"}))
        }
        FilingOrigin::User { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn log_op(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    op: &LensOp,
    outcome: &OpOutcome,
    bodies: Option<(&LensBody, &LensBody)>,
    origin: &FilingOrigin,
    at_ms: i64,
    undoes_op_id: Option<&str>,
) -> KnowledgeResult<String> {
    let key = op.key();
    let section_of = |body: &LensBody| {
        key.and_then(|key| body.page().and_then(|page| page.section(key).cloned()))
    };
    let (before_digest, after_digest, before_json) = match (op, bodies) {
        (_, None) => (None, None, None),
        (LensOp::Put { .. } | LensOp::Retire { .. }, Some((before, after))) => {
            let previous = section_of(before);
            (
                previous.as_ref().map(section_digest),
                section_of(after).as_ref().map(section_digest),
                previous.map(|section| to_value(&section)).transpose()?,
            )
        }
        (LensOp::Summary { .. }, Some((before, _))) => (
            None,
            None,
            before.page().map(|page| json!({"summary": page.summary})),
        ),
        (LensOp::IdeaStatus { .. }, Some((before, _))) => (
            None,
            None,
            before.idea().map(|idea| json!({"status": idea.status})),
        ),
    };
    let op_id = mint_id("lop");
    tx.execute(
        "INSERT INTO knowledge_lens_log (op_id, loupist_id, record_id, op_json, outcome, section_key,
            before_digest, after_digest, before_json, origin, chat_id, message_id, created_at_ms, undoes_op_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            op_id,
            loupist_id,
            record_id,
            canonical_string(&to_value(op)?),
            outcome.name(),
            key.map(SectionKey::as_str),
            before_digest,
            after_digest,
            before_json.map(|value| canonical_string(&value)),
            origin.log_name(),
            origin.chat_id(),
            origin.message_id(),
            at_ms,
            undoes_op_id,
        ],
    )?;
    Ok(op_id)
}

/// A principle save through the same log, as a put of its statement.
#[allow(clippy::too_many_arguments)]
pub(super) fn log_principle(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    input: &super::principles::PrincipleInput,
    before: Option<&super::principles::Principle>,
    at_ms: i64,
) -> KnowledgeResult<String> {
    let op = LensOp::Put {
        key: SectionKey::new("statement")?,
        heading: "Statement".into(),
        markdown: input.statement.clone(),
        basis: Basis::default(),
        expected: None,
    };
    let outcome = if before.is_some() {
        OpOutcome::Replaced {
            key: SectionKey::new("statement")?,
        }
    } else {
        OpOutcome::Added {
            key: SectionKey::new("statement")?,
        }
    };
    let op_id = mint_id("lop");
    tx.execute(
        "INSERT INTO knowledge_lens_log (op_id, loupist_id, record_id, op_json, outcome, section_key,
            before_digest, after_digest, before_json, origin, chat_id, message_id, created_at_ms, undoes_op_id)
         VALUES (?1, ?2, ?3, ?4, ?5, 'statement', NULL, NULL, ?6, 'user', ?7, ?8, ?9, NULL)",
        params![
            op_id,
            loupist_id,
            record_id,
            canonical_string(&to_value(&op)?),
            outcome.name(),
            before
                .map(|p| canonical_string(&json!({"statement": p.statement, "why": p.why, "example": p.example}))),
            input.origin_chat_id,
            input.origin_message_id,
            at_ms,
        ],
    )?;
    Ok(op_id)
}

/// Rebuilds the link index and mention anchors of one page from its body.
fn replace_links(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    body: &LensBody,
    at_ms: i64,
) -> KnowledgeResult<()> {
    let (lens, world) = links(body);
    tx.execute(
        "DELETE FROM knowledge_lens_links WHERE source_record_id = ?1",
        [record_id],
    )?;
    for target in lens {
        tx.execute(
            "INSERT INTO knowledge_lens_links (loupist_id, source_record_id, target_record_id) VALUES (?1, ?2, ?3)",
            params![loupist_id, record_id, target],
        )?;
    }
    tx.execute(
        "DELETE FROM knowledge_lens_anchors WHERE record_id = ?1 AND role = 'mention'",
        [record_id],
    )?;
    for node_id in world {
        tx.execute(
            "INSERT OR IGNORE INTO knowledge_lens_anchors (loupist_id, record_id, role, node_id, name_fold, turn_id, created_at_ms)
             VALUES (?1, ?2, 'mention', ?3, '', NULL, ?4)",
            params![loupist_id, record_id, node_id, at_ms],
        )?;
    }
    Ok(())
}

/// A final-answer `lens:` link applied a page to the World nodes of that turn.
pub(super) fn record_applied(
    tx: &Transaction<'_>,
    loupist_id: &str,
    turn_id: &str,
    record_id: &str,
    node_ids: &BTreeSet<String>,
    at_ms: i64,
) -> KnowledgeResult<()> {
    for node_id in node_ids {
        tx.execute(
            "INSERT OR IGNORE INTO knowledge_lens_anchors (loupist_id, record_id, role, node_id, name_fold, turn_id, created_at_ms)
             VALUES (?1, ?2, 'applied', ?3, '', ?4, ?5)",
            params![loupist_id, record_id, node_id, turn_id, at_ms],
        )?;
    }
    Ok(())
}

/// Sections citing one of `claim_ids` become contested, one revision and one
/// `lens_contested` growth event per page. Returns how many pages changed.
/// Discovery calls this after every World publication; no op is logged
/// because the World, not an actor, changed the page.
pub(super) fn contest_pages(
    tx: &Transaction<'_>,
    loupist_id: &str,
    claim_ids: &BTreeSet<String>,
    at_ms: i64,
) -> KnowledgeResult<usize> {
    let mut candidates = BTreeSet::new();
    let mut statement = tx.prepare(
        "SELECT m.record_id FROM knowledge_lens_metadata m
         JOIN knowledge_record_heads h ON h.record_id = m.record_id
         JOIN knowledge_record_revisions v ON v.revision_id = h.revision_id
         WHERE m.loupist_id = ?1 AND m.archived_at_ms IS NULL AND m.kind <> 'principle'
           AND instr(v.payload_json, ?2) > 0",
    )?;
    for claim_id in claim_ids {
        let rows =
            statement.query_map(params![loupist_id, claim_id], |row| row.get::<_, String>(0))?;
        for row in rows {
            candidates.insert(row?);
        }
    }
    drop(statement);
    let mut changed_pages = 0;
    for record_id in candidates {
        let loaded = load(tx, loupist_id, &record_id)?;
        let mut next = loaded.body.clone();
        let changed = next.contest(claim_ids, at_ms);
        if changed.is_empty() {
            continue;
        }
        let payload = next.to_payload()?;
        let published = publish_revision(
            tx,
            &loaded.record,
            loupist_id,
            loaded.record.head_revision_id.as_deref(),
            &payload,
            &[],
            at_ms,
        )?;
        // Publishing drops the record's search documents; without this the
        // page stays out of `lens_search` until its next filing.
        super::search_index::index_payload(
            tx,
            None,
            &record_id,
            &published.revision_id,
            RecordKind::Lens,
            &loaded.record.title,
            &payload,
        )?;
        tx.execute(
            "UPDATE knowledge_lens_metadata SET updated_at_ms = ?1 WHERE record_id = ?2",
            params![at_ms, record_id],
        )?;
        let mut event = super::growth::GrowthEvent::new(
            loupist_id,
            &record_id,
            at_ms,
            "lens_contested",
            "processing",
            &loaded.record.title,
        );
        event.after_summary = Some(super::growth::one_line(
            &changed
                .iter()
                .map(SectionKey::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        ));
        event.insert(tx)?;
        changed_pages += 1;
    }
    Ok(changed_pages)
}

/// Pending subject name anchors resolve once the World knows the name or an
/// alias exactly (mention and applied anchors are born with a node); nothing
/// else ever sets a node, so a shared company key binds no page. Returns how
/// many gained a node. Discovery calls this after every World publication.
pub(super) fn resolve_anchors(
    tx: &Transaction<'_>,
    loupist_id: &str,
    nodes: &[GraphNode],
) -> KnowledgeResult<usize> {
    let mut index = WorldIndex::default();
    index_nodes(&mut index, nodes);
    let mut statement = tx.prepare(
        "SELECT rowid, name_fold FROM knowledge_lens_anchors
         WHERE loupist_id = ?1 AND role = 'subject' AND node_id IS NULL AND name_fold <> ''",
    )?;
    let pending = statement
        .query_map([loupist_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut resolved = 0;
    for (rowid, name_fold) in pending {
        if let Some(node_id) = index.names.get(&name_fold) {
            resolved += tx.execute(
                "UPDATE OR IGNORE knowledge_lens_anchors SET node_id = ?1 WHERE rowid = ?2",
                params![node_id, rowid],
            )?;
        }
    }
    Ok(resolved)
}

/// Whether a live subject or idea page still has a subject anchor the World
/// has not confirmed. Method learning reads such a page against the Sources
/// it cites at read time, so a publication or a settled World job rechecks
/// while any exists.
pub(super) fn pending_subjects_exist(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM knowledge_lens_anchors a
            JOIN knowledge_lens_metadata m ON m.record_id = a.record_id
            WHERE a.loupist_id = ?1 AND a.role = 'subject' AND a.node_id IS NULL
              AND a.name_fold <> '' AND m.archived_at_ms IS NULL
              AND m.kind IN ('subject', 'idea'))",
        [loupist_id],
        |row| row.get(0),
    )?)
}

/// Whether a node that a confirmed subject anchor names spells itself
/// differently in `next` than in `previous`: another name or alias set, or
/// gone. Method learning keys a case by the World's current names, so such a
/// publication can split or fuse cases without resolving any anchor.
pub(super) fn subject_spellings_changed(
    tx: &Transaction<'_>,
    loupist_id: &str,
    previous: Option<&WorldSnapshotBody>,
    next: &WorldSnapshotBody,
) -> KnowledgeResult<bool> {
    let Some(previous) = previous else {
        return Ok(false);
    };
    let mut statement = tx.prepare(
        "SELECT DISTINCT node_id FROM knowledge_lens_anchors
         WHERE loupist_id = ?1 AND role = 'subject' AND node_id IS NOT NULL",
    )?;
    let confirmed: BTreeSet<String> = statement
        .query_map([loupist_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    if confirmed.is_empty() {
        return Ok(false);
    }
    let spellings = |body: &WorldSnapshotBody| -> BTreeMap<String, (String, BTreeSet<String>)> {
        body.nodes
            .iter()
            .filter(|node| confirmed.contains(&node.id))
            .map(|node| {
                let aliases = node.aliases.iter().cloned().collect();
                (node.id.clone(), (node.name.clone(), aliases))
            })
            .collect()
    };
    Ok(spellings(previous) != spellings(next))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AnchorRole {
    Subject,
    Mention,
    Applied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnchorView {
    pub role: AnchorRole,
    pub node_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LensLinkView {
    pub record_id: String,
    pub title: String,
    pub kind: LensKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LensLogEntry {
    pub op_id: String,
    pub op: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub message_id: Option<String>,
    pub created_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub undoes_op_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SectionView {
    pub key: SectionKey,
    pub heading: String,
    pub markdown: String,
    pub digest: String,
    pub basis: Basis,
    pub chip: BasisChip,
    pub origin: Origin,
    pub contested: bool,
    pub contested_claim_ids: Vec<String>,
    pub updated_at_ms: i64,
}

impl SectionView {
    fn of(section: &PageSection) -> Self {
        let (contested, contested_claim_ids) = match &section.state {
            SectionState::Current => (false, Vec::new()),
            SectionState::Contested { claim_ids, .. } => (true, claim_ids.clone()),
        };
        Self {
            key: section.key.clone(),
            heading: section.heading.clone(),
            markdown: section.markdown.clone(),
            digest: section_digest(section),
            basis: section.basis.clone(),
            chip: section.basis.chip(),
            origin: section.origin,
            contested,
            contested_claim_ids,
            updated_at_ms: section.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LensBodyView {
    Principle {
        statement: String,
        why: String,
        example: String,
        origin: String,
    },
    Perspective {
        summary: String,
        sections: Vec<SectionView>,
    },
    Method {
        summary: String,
        sections: Vec<SectionView>,
    },
    Subject {
        summary: String,
        sections: Vec<SectionView>,
    },
    Idea {
        summary: String,
        sections: Vec<SectionView>,
        idea: IdeaState,
    },
}

impl LensBodyView {
    fn of(body: &LensBody) -> Self {
        let views = |page: &super::lens_page::Page| {
            page.sections
                .iter()
                .map(SectionView::of)
                .collect::<Vec<_>>()
        };
        match body {
            LensBody::Principle(p) => Self::Principle {
                statement: p.statement.clone(),
                why: p.why.clone().unwrap_or_default(),
                example: p.example.clone().unwrap_or_default(),
                origin: p.origin.clone(),
            },
            LensBody::Perspective(page) => Self::Perspective {
                summary: page.summary.clone(),
                sections: views(page),
            },
            LensBody::Method(page) => Self::Method {
                summary: page.summary.clone(),
                sections: views(page),
            },
            LensBody::Subject(page) => Self::Subject {
                summary: page.summary.clone(),
                sections: views(page),
            },
            LensBody::Idea(page, idea) => Self::Idea {
                summary: page.summary.clone(),
                sections: views(page),
                idea: idea.clone(),
            },
        }
    }

    pub fn sections(&self) -> &[SectionView] {
        match self {
            Self::Principle { .. } => &[],
            Self::Perspective { sections, .. }
            | Self::Method { sections, .. }
            | Self::Subject { sections, .. }
            | Self::Idea { sections, .. } => sections,
        }
    }

    /// The stored body behind this view, for rendering.
    pub fn to_body(&self) -> LensBody {
        let page = |summary: &str, sections: &[SectionView]| super::lens_page::Page {
            summary: summary.to_owned(),
            sections: sections
                .iter()
                .map(|section| PageSection {
                    key: section.key.clone(),
                    heading: section.heading.clone(),
                    markdown: section.markdown.clone(),
                    basis: section.basis.clone(),
                    origin: section.origin,
                    state: if section.contested {
                        SectionState::Contested {
                            since_ms: section.updated_at_ms,
                            claim_ids: section.contested_claim_ids.clone(),
                        }
                    } else {
                        SectionState::Current
                    },
                    updated_at_ms: section.updated_at_ms,
                })
                .collect(),
        };
        match self {
            Self::Principle {
                statement,
                why,
                example,
                origin,
            } => LensBody::Principle(super::principles::PrincipleInput {
                statement: statement.clone(),
                why: Some(why.clone()).filter(|t| !t.is_empty()),
                example: Some(example.clone()).filter(|t| !t.is_empty()),
                origin: origin.clone(),
                origin_chat_id: None,
                origin_message_id: None,
            }),
            Self::Perspective { summary, sections } => {
                LensBody::Perspective(page(summary, sections))
            }
            Self::Method { summary, sections } => LensBody::Method(page(summary, sections)),
            Self::Subject { summary, sections } => LensBody::Subject(page(summary, sections)),
            Self::Idea {
                summary,
                sections,
                idea,
            } => LensBody::Idea(page(summary, sections), idea.clone()),
        }
    }

    pub fn summary(&self) -> &str {
        match self {
            Self::Principle { statement, .. } => statement,
            Self::Perspective { summary, .. }
            | Self::Method { summary, .. }
            | Self::Subject { summary, .. }
            | Self::Idea { summary, .. } => summary,
        }
    }
}

/// One page as the bridge and the `lens_read` tool deliver it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PageRead {
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub kind: LensKind,
    pub title: String,
    pub archived: bool,
    pub paused: bool,
    pub learned: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub body: LensBodyView,
    /// Suggested section keys for this kind: an ordering hint, not a closed set.
    pub suggested_sections: Vec<String>,
    pub anchors: Vec<AnchorView>,
    pub links: Vec<LensLinkView>,
    pub backlinks: Vec<LensLinkView>,
    /// Newest first, at most 20.
    pub history: Vec<LensLogEntry>,
}

impl PageRead {
    /// The exact model-facing text; its digest binds a read receipt.
    pub fn delivery_text(&self) -> KnowledgeResult<String> {
        Ok(canonical_string(&to_value(self)?))
    }

    pub fn markdown(&self, with_keys: bool) -> String {
        super::lens_page::render_markdown(&self.title, &self.body.to_body(), with_keys)
    }
}

pub(super) fn read_page(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<PageRead> {
    let loaded = load(tx, loupist_id, record_id)?;
    let Some(revision) = loaded.revision else {
        return Err(KnowledgeError::NotFound);
    };
    let (archived, learned, updated_at_ms): (bool, bool, i64) = tx.query_row(
        "SELECT archived_at_ms IS NOT NULL, learned, updated_at_ms FROM knowledge_lens_metadata WHERE record_id = ?1",
        [record_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let created_at_ms: i64 = tx.query_row(
        "SELECT created_at_ms FROM knowledge_records WHERE record_id = ?1",
        [record_id],
        |row| row.get(0),
    )?;
    let paused = super::memory_controls::load(tx, loupist_id)?
        .paused
        .contains(record_id);
    let (lens_links, _) = links(&loaded.body);
    Ok(PageRead {
        record_id: record_id.to_owned(),
        revision_id: revision.revision_id,
        revision_digest: revision.revision_digest,
        kind: loaded.kind,
        title: loaded.record.title,
        archived,
        paused,
        learned,
        created_at_ms,
        updated_at_ms,
        body: LensBodyView::of(&loaded.body),
        suggested_sections: super::lens_page::suggested_sections(loaded.kind)
            .iter()
            .map(|key| (*key).to_owned())
            .collect(),
        anchors: anchors_of(tx, loupist_id, record_id)?,
        links: lookup_links(tx, loupist_id, &lens_links)?,
        backlinks: backlinks_of(tx, loupist_id, record_id)?,
        history: history(tx, record_id, HISTORY_LIMIT)?,
    })
}

fn anchors_of(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Vec<AnchorView>> {
    let mut statement = tx.prepare(
        "SELECT role, node_id, name_fold FROM knowledge_lens_anchors
         WHERE loupist_id = ?1 AND record_id = ?2
         GROUP BY role, node_id, name_fold ORDER BY role, node_id, name_fold",
    )?;
    let rows = statement
        .query_map(params![loupist_id, record_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut labels: Option<BTreeMap<String, String>> = None;
    let mut anchors = Vec::new();
    for (role, node_id, name_fold) in rows {
        let role = match role.as_str() {
            "subject" => AnchorRole::Subject,
            "mention" => AnchorRole::Mention,
            _ => AnchorRole::Applied,
        };
        let name = if !name_fold.is_empty() {
            name_fold
        } else if let Some(node_id) = &node_id {
            if labels.is_none() {
                labels = Some(world_index(tx, loupist_id)?.labels);
            }
            labels
                .as_ref()
                .and_then(|labels| labels.get(node_id).cloned())
                .unwrap_or_default()
        } else {
            String::new()
        };
        anchors.push(AnchorView {
            role,
            node_id,
            name,
        });
    }
    Ok(anchors)
}

fn lookup_links(
    tx: &Transaction<'_>,
    loupist_id: &str,
    ids: &BTreeSet<String>,
) -> KnowledgeResult<Vec<LensLinkView>> {
    let mut found = Vec::new();
    for record_id in ids.iter().take(LINK_LIMIT) {
        let link = tx
            .query_row(
                "SELECT title, kind FROM knowledge_lens_metadata WHERE loupist_id = ?1 AND record_id = ?2",
                params![loupist_id, record_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        if let Some((title, kind)) = link {
            if let Some(kind) = LensKind::parse(&kind) {
                found.push(LensLinkView {
                    record_id: record_id.clone(),
                    title,
                    kind,
                });
            }
        }
    }
    found.sort_by(|a, b| a.title.cmp(&b.title).then(a.record_id.cmp(&b.record_id)));
    Ok(found)
}

fn backlinks_of(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Vec<LensLinkView>> {
    let mut statement = tx.prepare(
        "SELECT link.source_record_id, meta.title, meta.kind
         FROM knowledge_lens_links link
         JOIN knowledge_lens_metadata meta
           ON meta.record_id = link.source_record_id AND meta.loupist_id = link.loupist_id
         WHERE link.loupist_id = ?1 AND link.target_record_id = ?2 AND meta.archived_at_ms IS NULL
         ORDER BY meta.title, link.source_record_id LIMIT ?3",
    )?;
    let rows = statement.query_map(params![loupist_id, record_id, LINK_LIMIT as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut links = Vec::new();
    for row in rows {
        let (record_id, title, kind) = row?;
        if let Some(kind) = LensKind::parse(&kind) {
            links.push(LensLinkView {
                record_id,
                title,
                kind,
            });
        }
    }
    Ok(links)
}

pub(super) fn history(
    tx: &Transaction<'_>,
    record_id: &str,
    limit: usize,
) -> KnowledgeResult<Vec<LensLogEntry>> {
    let mut statement = tx.prepare(
        "SELECT op_id, op_json, outcome, section_key, origin, chat_id, message_id, created_at_ms, undoes_op_id
         FROM knowledge_lens_log WHERE record_id = ?1
         ORDER BY created_at_ms DESC, rowid DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![record_id, limit as i64], |row| {
        Ok(LensLogEntry {
            op_id: row.get(0)?,
            op: row.get(1)?,
            outcome: row.get(2)?,
            key: row.get(3)?,
            origin: row.get(4)?,
            chat_id: row.get(5)?,
            message_id: row.get(6)?,
            created_at_ms: row.get(7)?,
            undoes_op_id: row.get(8)?,
        })
    })?;
    let mut entries = Vec::new();
    for row in rows {
        let mut entry = row?;
        let op: LensOp = from_json(&entry.op)?;
        entry.op = op.name().to_owned();
        entries.push(entry);
    }
    Ok(entries)
}

struct LogRow {
    record_id: String,
    op: LensOp,
    outcome: String,
    section_key: Option<String>,
    after_digest: Option<String>,
    before_json: Option<String>,
}

fn log_row(tx: &Transaction<'_>, loupist_id: &str, op_id: &str) -> KnowledgeResult<LogRow> {
    validate_knowledge_id(op_id, "lop")?;
    let row = tx
        .query_row(
            "SELECT record_id, op_json, outcome, section_key, after_digest, before_json
             FROM knowledge_lens_log WHERE op_id = ?1 AND loupist_id = ?2",
            params![op_id, loupist_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(KnowledgeError::NotFound)?;
    Ok(LogRow {
        record_id: row.0,
        op: from_json(&row.1)?,
        outcome: row.2,
        section_key: row.3,
        after_digest: row.4,
        before_json: row.5,
    })
}

/// Appends the inverse of one logged op. Refuses when a later op changed the
/// same section (or the summary or status), or when the op was undone already.
pub(super) fn undo_op(
    tx: &Transaction<'_>,
    loupist_id: &str,
    op_id: &str,
    at_ms: i64,
) -> KnowledgeResult<PageRead> {
    let row = log_row(tx, loupist_id, op_id)?;
    let undone: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_lens_log WHERE undoes_op_id = ?1)",
        [op_id],
        |row| row.get(0),
    )?;
    if undone {
        return Err(KnowledgeError::Invalid("this change was undone already"));
    }
    let later_changed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_lens_log
         WHERE record_id = ?1 AND op_id <> ?2
           AND rowid > (SELECT rowid FROM knowledge_lens_log WHERE op_id = ?2)
           AND COALESCE(section_key, '') = COALESCE(?3, '')
           AND outcome IN ('added', 'replaced', 'retired', 'summarized', 'status_set')
           AND (?3 IS NOT NULL OR json_extract(op_json, '$.op') = ?4))",
        params![row.record_id, op_id, row.section_key, row.op.name()],
        |row| row.get(0),
    )?;
    if later_changed {
        return Err(KnowledgeError::Invalid(
            "a later change touched this section; undo it first",
        ));
    }
    let before: Option<Value> = row.before_json.as_deref().map(from_json).transpose()?;
    let restore_section = |before: &Option<Value>, expected: Option<String>| {
        let section: PageSection = serde_json::from_value(
            before
                .clone()
                .ok_or(KnowledgeError::Invalid("nothing to restore"))?,
        )?;
        Ok::<_, KnowledgeError>(LensOp::Put {
            key: section.key,
            heading: section.heading,
            markdown: section.markdown,
            basis: section.basis,
            expected,
        })
    };
    let inverse = match (&row.op, row.outcome.as_str()) {
        (LensOp::Put { key, .. }, "added") => LensOp::Retire {
            key: key.clone(),
            expected: row.after_digest.clone().unwrap_or_default(),
            why: "Undone by you".into(),
        },
        (LensOp::Put { .. }, "replaced") => restore_section(&before, row.after_digest.clone())?,
        (LensOp::Retire { .. }, "retired") => restore_section(&before, None)?,
        (LensOp::Summary { .. }, "summarized") => LensOp::Summary {
            text: before
                .as_ref()
                .and_then(|value| value["summary"].as_str())
                .unwrap_or_default()
                .to_owned(),
        },
        (LensOp::IdeaStatus { .. }, "status_set") => LensOp::IdeaStatus {
            status: before
                .as_ref()
                .map(|value| serde_json::from_value::<IdeaStatus>(value["status"].clone()))
                .transpose()?
                .unwrap_or(IdeaStatus::Open),
            why: "Undone by you".into(),
        },
        _ => {
            return Err(KnowledgeError::Invalid(
                "this entry changed nothing to undo",
            ))
        }
    };
    let record_id = row.record_id.clone();
    let filed = file_with(
        tx,
        loupist_id,
        &FilingOrigin::User {
            chat_id: None,
            message_id: None,
        },
        vec![PageDelta {
            target: PageTarget::Existing {
                record_id: record_id.clone(),
            },
            ops: vec![inverse],
        }],
        at_ms,
        Some(op_id),
    )?;
    if !filed.iter().any(|page| page.changed) {
        return Err(KnowledgeError::Invalid(
            "the page no longer holds what this change wrote",
        ));
    }
    read_page(tx, loupist_id, &record_id)
}

/// The user retires a section. Only a user put may bring that key back.
pub(super) fn forget_section(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    key: &str,
    at_ms: i64,
) -> KnowledgeResult<PageRead> {
    let key = SectionKey::new(key)?;
    let loaded = load(tx, loupist_id, record_id)?;
    let section = loaded
        .body
        .page()
        .and_then(|page| page.section(&key))
        .ok_or(KnowledgeError::NotFound)?;
    file(
        tx,
        loupist_id,
        &FilingOrigin::User {
            chat_id: None,
            message_id: None,
        },
        vec![PageDelta {
            target: PageTarget::Existing {
                record_id: record_id.to_owned(),
            },
            ops: vec![LensOp::Retire {
                key,
                expected: section_digest(section),
                why: "Forgotten by you".into(),
            }],
        }],
        at_ms,
    )?;
    read_page(tx, loupist_id, record_id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LearnedOutcome {
    pub op_id: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LearnedPage {
    pub record_id: String,
    pub title: String,
    pub kind: LensKind,
    pub created: bool,
    pub outcomes: Vec<LearnedOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LearnedDigest {
    pub message_id: String,
    pub pages: Vec<LearnedPage>,
}

/// What each message filed, derived from the log.
pub(super) fn learned_digest(
    tx: &Transaction<'_>,
    loupist_id: &str,
    message_ids: &[String],
) -> KnowledgeResult<Vec<LearnedDigest>> {
    let mut digests = Vec::with_capacity(message_ids.len());
    for message_id in message_ids {
        let mut statement = tx.prepare(
            "SELECT l.op_id, l.record_id, l.outcome, l.section_key, m.title, m.kind
             FROM knowledge_lens_log l JOIN knowledge_lens_metadata m ON m.record_id = l.record_id
             WHERE l.loupist_id = ?1 AND l.message_id = ?2
             ORDER BY l.created_at_ms, l.rowid",
        )?;
        let rows = statement
            .query_map(params![loupist_id, message_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut pages: Vec<LearnedPage> = Vec::new();
        for (op_id, record_id, outcome, key, title, kind) in rows {
            let page = match pages.iter_mut().find(|page| page.record_id == record_id) {
                Some(page) => page,
                None => {
                    let created: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM knowledge_growth_events
                         WHERE loupist_id = ?1 AND record_id = ?2 AND kind = 'lens_created' AND origin_message_id = ?3)",
                        params![loupist_id, record_id, message_id],
                        |row| row.get(0),
                    )?;
                    pages.push(LearnedPage {
                        record_id: record_id.clone(),
                        title,
                        kind: LensKind::parse(&kind).unwrap_or(LensKind::Subject),
                        created,
                        outcomes: Vec::new(),
                    });
                    pages.last_mut().expect("pushed")
                }
            };
            page.outcomes.push(LearnedOutcome {
                op_id,
                outcome,
                key,
            });
        }
        digests.push(LearnedDigest {
            message_id: message_id.clone(),
            pages,
        });
    }
    Ok(digests)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PageSummary {
    pub record_id: String,
    pub title: String,
    pub kind: LensKind,
    pub summary: String,
}

/// Active pages anchored to a World node, by any role.
pub(super) fn pages_for_node(
    tx: &Transaction<'_>,
    loupist_id: &str,
    node_id: &str,
) -> KnowledgeResult<Vec<PageSummary>> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT m.record_id, m.title, m.kind FROM knowledge_lens_anchors a
         JOIN knowledge_lens_metadata m ON m.record_id = a.record_id
         WHERE a.loupist_id = ?1 AND a.node_id = ?2 AND m.archived_at_ms IS NULL
         ORDER BY m.updated_at_ms DESC, m.record_id",
    )?;
    let rows = statement
        .query_map(params![loupist_id, node_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let removed = super::memory_controls::load(tx, loupist_id)?.removed;
    let mut pages = Vec::new();
    for (record_id, title, kind) in rows {
        if removed.contains(&record_id) {
            continue;
        }
        let Some(kind) = LensKind::parse(&kind) else {
            continue;
        };
        let summary = load(tx, loupist_id, &record_id)?.body.summary();
        pages.push(PageSummary {
            record_id,
            title,
            kind,
            summary,
        });
    }
    Ok(pages)
}

/// One line of the Lens index: what a turn sees of a page before reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub record_id: String,
    pub kind: LensKind,
    pub title: String,
    pub summary: String,
    pub section_count: usize,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub node_ids: BTreeSet<String>,
    pub map_names: Vec<String>,
    pub learned: bool,
}

/// Every active, unpaused page of the Loupist with its anchors resolved to names.
pub(super) fn index_entries(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Vec<IndexEntry>> {
    validate_loupist_id(loupist_id)?;
    let controls = super::memory_controls::load(tx, loupist_id)?;
    let mut statement = tx.prepare(
        "SELECT m.record_id, m.kind, m.title, m.updated_at_ms, m.learned, r.created_at_ms
         FROM knowledge_lens_metadata m JOIN knowledge_records r ON r.record_id = m.record_id
         WHERE m.loupist_id = ?1 AND m.archived_at_ms IS NULL AND r.owner_loupist_id = ?1
         ORDER BY m.updated_at_ms DESC, m.record_id",
    )?;
    let rows = statement
        .query_map([loupist_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut anchors = tx.prepare(
        "SELECT node_id, name_fold FROM knowledge_lens_anchors
         WHERE loupist_id = ?1 AND record_id = ?2 AND role IN ('subject', 'mention')
         ORDER BY role, created_at_ms",
    )?;
    let labels = world_index(tx, loupist_id)?.labels;
    let mut entries = Vec::new();
    for (record_id, kind, title, updated_at_ms, learned, created_at_ms) in rows {
        if controls.paused.contains(&record_id) || controls.removed.contains(&record_id) {
            continue;
        }
        let Some(kind) = LensKind::parse(&kind) else {
            continue;
        };
        let loaded = load(tx, loupist_id, &record_id)?;
        let mut node_ids = BTreeSet::new();
        let mut map_names = Vec::new();
        for row in anchors.query_map(params![loupist_id, record_id], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?))
        })? {
            let (node_id, name_fold) = row?;
            let name = node_id
                .as_ref()
                .and_then(|id| labels.get(id).cloned())
                .unwrap_or(name_fold);
            if let Some(node_id) = node_id {
                node_ids.insert(node_id);
            }
            if !name.is_empty() && !map_names.contains(&name) && map_names.len() < 3 {
                map_names.push(name);
            }
        }
        entries.push(IndexEntry {
            record_id,
            kind,
            title,
            summary: loaded.body.summary(),
            section_count: loaded.body.page().map_or(0, |page| page.sections.len()),
            created_at_ms,
            updated_at_ms,
            node_ids,
            map_names,
            learned,
        });
    }
    Ok(entries)
}

pub(super) fn set_lens_archived(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    archived: bool,
    rationale: &str,
    at_ms: i64,
    actor: &str,
) -> KnowledgeResult<()> {
    super::model::validate_text(rationale, 16_000, false, "change rationale is invalid")?;
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    if archived == module.archived_at_ms.is_some() {
        return Ok(());
    }
    let kind = lens_kind(tx, loupist_id, record_id)?;
    if !archived {
        if kind == LensKind::Principle {
            let p = super::principles::principle_read(tx, loupist_id, record_id)?;
            super::principles::require_unique_statement(
                tx,
                loupist_id,
                &p.statement,
                Some(record_id),
            )?;
        } else if active_by_title(tx, loupist_id, kind, &module.title)?.is_some()
            || (kind == LensKind::Perspective && active_perspective(tx, loupist_id)?.is_some())
        {
            return Err(KnowledgeError::Invalid(
                "an active page already uses this title",
            ));
        }
    }
    tx.execute(
        "UPDATE knowledge_lens_metadata SET archived_at_ms = ?1, updated_at_ms = ?2 WHERE record_id = ?3 AND loupist_id = ?4",
        params![archived.then_some(at_ms), at_ms, record_id, loupist_id],
    )?;
    let mut event = super::growth::GrowthEvent::new(
        loupist_id,
        record_id,
        at_ms,
        if archived {
            "lens_archived"
        } else {
            "lens_restored"
        },
        actor,
        rationale,
    );
    event.before_summary = Some(super::growth::one_line(&format!(
        "{}: {}",
        module.title,
        if archived { "active" } else { "archived" }
    )));
    event.after_summary = Some(super::growth::one_line(&format!(
        "{}: {}",
        module.title,
        if archived { "archived" } else { "active" }
    )));
    if kind == LensKind::Principle {
        event.origin = Some("settings".into());
    }
    event.insert(tx)
}

pub(super) fn lens_listing(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<(i64, bool, bool)> {
    let (updated_at_ms, archived_at_ms, learned) = tx.query_row(
        "SELECT updated_at_ms, archived_at_ms, learned FROM knowledge_lens_metadata WHERE loupist_id = ?1 AND record_id = ?2",
        params![loupist_id, record_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, bool>(2)?)),
    )?;
    Ok((updated_at_ms, archived_at_ms.is_some(), learned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::lens_page::{BasisRef, PageKind};
    use crate::knowledge::{
        Authorship, ClaimEndpoint, Coverage, CreateRecord, GraphClaim, KnowledgeStore,
        RepresentationRole, RetainMaterial, RetainNewSource, SealWorld, Selector, SourceClass,
    };

    const LOUPIST: &str = "loupist-a";

    fn store() -> (tempfile::TempDir, KnowledgeStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        (directory, store)
    }

    fn turn(message: &str) -> FilingOrigin {
        FilingOrigin::Turn {
            chat_id: "chat-1".into(),
            message_id: message.into(),
            user_message_id: Some("user-1".into()),
            unattended: false,
        }
    }

    fn user() -> FilingOrigin {
        FilingOrigin::User {
            chat_id: None,
            message_id: None,
        }
    }

    fn claim() -> Basis {
        Basis {
            refs: vec![BasisRef::Claim {
                claim_id: "clm_1".into(),
            }],
            uncertainty: None,
        }
    }

    fn put(key: &str, markdown: &str, expected: Option<String>) -> LensOp {
        LensOp::Put {
            key: SectionKey::new(key).unwrap(),
            heading: key.to_uppercase(),
            markdown: markdown.into(),
            basis: claim(),
            expected,
        }
    }

    fn create_subject(title: &str, ops: Vec<LensOp>) -> PageDelta {
        PageDelta {
            target: PageTarget::Create {
                kind: PageKind::Subject,
                title: title.into(),
                anchors: vec![AnchorInput::Name { name: title.into() }],
            },
            ops,
        }
    }

    fn existing(record_id: &str, ops: Vec<LensOp>) -> PageDelta {
        PageDelta {
            target: PageTarget::Existing {
                record_id: record_id.into(),
            },
            ops,
        }
    }

    fn file_in(
        store: &KnowledgeStore,
        origin: &FilingOrigin,
        deltas: Vec<PageDelta>,
        at_ms: i64,
    ) -> Vec<FiledPage> {
        store
            .transact(|write| write.file(LOUPIST, origin, deltas, at_ms))
            .unwrap()
    }

    fn read(store: &KnowledgeStore, record_id: &str) -> PageRead {
        store
            .read(|write| write.read_page(LOUPIST, record_id))
            .unwrap()
    }

    fn digest(page: &PageRead, key: &str) -> String {
        page.body
            .sections()
            .iter()
            .find(|section| section.key.as_str() == key)
            .unwrap()
            .digest
            .clone()
    }

    fn log_rows(store: &KnowledgeStore, record_id: &str) -> Vec<(String, String)> {
        store
            .read(|write| {
                let mut statement = write.tx.prepare(
                    "SELECT outcome, origin FROM knowledge_lens_log WHERE record_id = ?1 ORDER BY created_at_ms, rowid",
                )?;
                let rows = statement
                    .query_map([record_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    fn revision_count(store: &KnowledgeStore, record_id: &str) -> i64 {
        store
            .read(|write| {
                Ok(write.tx.query_row(
                    "SELECT count(*) FROM knowledge_record_revisions WHERE record_id = ?1",
                    [record_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap()
    }

    fn event_count(store: &KnowledgeStore, record_id: &str) -> i64 {
        store
            .read(|write| {
                Ok(write.tx.query_row(
                    "SELECT count(*) FROM knowledge_growth_events WHERE record_id = ?1",
                    [record_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap()
    }

    #[test]
    fn filing_a_sourced_subject_page_reads_back_with_digests_and_log() {
        let (_dir, store) = store();
        let filed = file_in(
            &store,
            &turn("msg-1"),
            vec![create_subject(
                "NVIDIA",
                vec![
                    put("facts", "Data center revenue doubled.", None),
                    LensOp::Summary {
                        text: "GPU maker.".into(),
                    },
                ],
            )],
            100,
        );
        assert_eq!(filed.len(), 1);
        assert!(filed[0].created && filed[0].changed);
        assert_eq!(filed[0].kind, LensKind::Subject);
        assert!(matches!(
            filed[0].outcomes[0].outcome,
            OpOutcome::Added { .. }
        ));
        let page = read(&store, &filed[0].record_id);
        assert_eq!(page.kind, LensKind::Subject);
        assert_eq!(page.title, "NVIDIA");
        assert_eq!(page.body.summary(), "GPU maker.");
        let section = &page.body.sections()[0];
        assert_eq!(section.chip, BasisChip::Source);
        assert_eq!(section.origin, Origin::Loupist);
        assert_eq!(section.digest.len(), 64);
        assert_eq!(page.anchors[0].role, AnchorRole::Subject);
        assert_eq!(page.anchors[0].name, "nvidia");
        assert_eq!(page.history.len(), 2);
        assert_eq!(page.history[0].origin, "turn");
        assert_eq!(page.history[0].message_id.as_deref(), Some("msg-1"));
        assert!(page.learned);
        assert_eq!(revision_count(&store, &page.record_id), 1);
        assert_eq!(event_count(&store, &page.record_id), 1);
        let learned = store
            .read(|write| write.learned_digest(LOUPIST, &["msg-1".into()]))
            .unwrap();
        assert_eq!(learned[0].pages.len(), 1);
        assert!(learned[0].pages[0].created);
        assert_eq!(learned[0].pages[0].outcomes.len(), 2);
    }

    #[test]
    fn disjoint_edits_land_overlap_is_logged_stale_and_replay_is_a_no_op() {
        let (_dir, store) = store();
        let filed = file_in(
            &store,
            &turn("msg-1"),
            vec![create_subject(
                "NVIDIA",
                vec![put("facts", "A", None), put("risks", "B", None)],
            )],
            100,
        );
        let id = filed[0].record_id.clone();
        let page = read(&store, &id);
        let facts = digest(&page, "facts");
        let risks = digest(&page, "risks");
        let first = file_in(
            &store,
            &turn("msg-2"),
            vec![existing(&id, vec![put("facts", "A2", Some(facts.clone()))])],
            200,
        );
        let second = file_in(
            &store,
            &FilingOrigin::Learning {
                review_id: "rev-1".into(),
            },
            vec![existing(
                &id,
                vec![
                    put("risks", "B2", Some(risks)),
                    put("facts", "A3", Some(facts)),
                ],
            )],
            300,
        );
        assert!(matches!(
            first[0].outcomes[0].outcome,
            OpOutcome::Replaced { .. }
        ));
        assert!(matches!(
            second[0].outcomes[0].outcome,
            OpOutcome::Replaced { .. }
        ));
        assert!(matches!(
            second[0].outcomes[1].outcome,
            OpOutcome::SkippedStale { .. }
        ));
        let page = read(&store, &id);
        let text = |key: &str| {
            page.body
                .sections()
                .iter()
                .find(|s| s.key.as_str() == key)
                .unwrap()
                .markdown
                .clone()
        };
        assert_eq!(text("facts"), "A2");
        assert_eq!(text("risks"), "B2");
        let rows = log_rows(&store, &id);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[4], ("skipped_stale".into(), "learning".into()));
        let revisions = revision_count(&store, &id);
        let events = event_count(&store, &id);
        let replay = file_in(
            &store,
            &turn("msg-3"),
            vec![existing(
                &id,
                vec![put("facts", "A2", Some(digest(&page, "facts")))],
            )],
            400,
        );
        assert!(matches!(
            replay[0].outcomes[0].outcome,
            OpOutcome::AlreadyKnown { .. }
        ));
        assert!(!replay[0].changed);
        assert_eq!(revision_count(&store, &id), revisions);
        assert_eq!(event_count(&store, &id), events);
        assert_eq!(log_rows(&store, &id).len(), 6);
    }

    #[test]
    fn undo_restores_what_its_own_op_found_within_one_filing() {
        let (_dir, store) = store();
        let filed = file_in(
            &store,
            &user(),
            vec![create_subject(
                "Undo probe",
                vec![
                    put("facts", "Original fact.", None),
                    LensOp::Summary {
                        text: "Original summary.".into(),
                    },
                ],
            )],
            100,
        );
        let record_id = &filed[0].record_id;
        let revised = file_in(
            &store,
            &turn("msg-2"),
            vec![existing(
                record_id,
                vec![
                    LensOp::Summary {
                        text: "Intermediate summary.".into(),
                    },
                    LensOp::Summary {
                        text: "Final summary.".into(),
                    },
                ],
            )],
            200,
        );
        let undone = store
            .transact(|write| write.undo_op(LOUPIST, &revised[0].outcomes[1].op_id, 300))
            .unwrap();
        assert_eq!(undone.body.summary(), "Intermediate summary.");
        // A summary written over none is undone to none.
        let bare = file_in(
            &store,
            &user(),
            vec![create_subject("Bare", vec![put("facts", "A fact.", None)])],
            400,
        );
        let summarized = file_in(
            &store,
            &turn("msg-3"),
            vec![existing(
                &bare[0].record_id,
                vec![LensOp::Summary {
                    text: "A later summary.".into(),
                }],
            )],
            500,
        );
        let cleared = store
            .transact(|write| write.undo_op(LOUPIST, &summarized[0].outcomes[0].op_id, 600))
            .unwrap();
        assert_eq!(cleared.body.summary(), "");
    }

    #[test]
    fn a_create_landing_on_an_existing_page_only_adds_absent_sections() {
        let (_dir, store) = store();
        let idea = |title: &str, ops: Vec<LensOp>| PageDelta {
            target: PageTarget::Create {
                kind: PageKind::Idea,
                title: title.into(),
                anchors: vec![],
            },
            ops,
        };
        let first = file_in(
            &store,
            &turn("msg-1"),
            vec![idea(
                "Hanbit pricing power",
                vec![
                    put("claim", "Hanbit can raise LFP prices.", None),
                    LensOp::Summary {
                        text: "The user's idea: Hanbit can raise prices.".into(),
                    },
                ],
            )],
            100,
        );
        let id = first[0].record_id.clone();
        let claim_digest = digest(&read(&store, &id), "claim");
        let again = file_in(
            &store,
            &turn("msg-2"),
            vec![idea(
                "hanbit PRICING power",
                vec![
                    LensOp::IdeaStatus {
                        status: IdeaStatus::Refuted,
                        why: "No basis given.".into(),
                    },
                    LensOp::Summary {
                        text: "Refuted.".into(),
                    },
                    put(
                        "claim",
                        "Hanbit cannot raise prices.",
                        Some(claim_digest.clone()),
                    ),
                    LensOp::Retire {
                        key: SectionKey::new("claim").unwrap(),
                        expected: claim_digest,
                        why: "Wrong.".into(),
                    },
                    put("checks", "Watch LFP contract prices.", None),
                ],
            )],
            200,
        );
        assert_eq!(again[0].record_id, id);
        let claim = || Some(SectionKey::new("claim").unwrap());
        assert_eq!(
            again[0]
                .outcomes
                .iter()
                .map(|op| op.outcome.clone())
                .collect::<Vec<_>>(),
            vec![
                OpOutcome::SkippedStale { key: None },
                OpOutcome::SkippedStale { key: None },
                OpOutcome::SkippedStale { key: claim() },
                OpOutcome::SkippedStale { key: claim() },
                OpOutcome::Added {
                    key: SectionKey::new("checks").unwrap()
                },
            ]
        );
        let page = read(&store, &id);
        assert_eq!(
            page.body.summary(),
            "The user's idea: Hanbit can raise prices."
        );
        assert!(
            matches!(&page.body, LensBodyView::Idea { idea, .. } if idea.status == IdeaStatus::Open)
        );
        assert_eq!(
            page.body.sections()[0].markdown,
            "Hanbit can raise LFP prices."
        );
        assert_eq!(page.body.sections().len(), 2);
    }

    #[test]
    fn duplicate_title_create_merges_and_perspective_is_single() {
        let (_dir, store) = store();
        let first = file_in(
            &store,
            &turn("msg-1"),
            vec![create_subject("NVIDIA", vec![put("facts", "A", None)])],
            100,
        );
        let second = file_in(
            &store,
            &turn("msg-2"),
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Subject,
                    title: "  nvidia ".into(),
                    anchors: vec![],
                },
                ops: vec![put("drivers", "B", None)],
            }],
            200,
        );
        assert_eq!(first[0].record_id, second[0].record_id);
        assert!(!second[0].created);
        assert_eq!(read(&store, &first[0].record_id).body.sections().len(), 2);
        let perspective = |title: &str, at| {
            file_in(
                &store,
                &turn("msg-p"),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Perspective,
                        title: title.into(),
                        anchors: vec![],
                    },
                    ops: vec![LensOp::Put {
                        key: SectionKey::new("beliefs").unwrap(),
                        heading: "Beliefs".into(),
                        markdown: format!("Belief {at}"),
                        basis: Basis::default(),
                        expected: None,
                    }],
                }],
                at,
            )
        };
        let a = perspective("Outlook", 300);
        let b = perspective("Different title", 400);
        assert_eq!(a[0].record_id, b[0].record_id);
        assert!(matches!(
            b[0].outcomes[0].outcome,
            OpOutcome::SkippedStale { .. }
        ));
        let all_refused = file_in(
            &store,
            &turn("msg-x"),
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Subject,
                    title: "Empty".into(),
                    anchors: vec![],
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new("facts").unwrap(),
                    heading: "Facts".into(),
                    markdown: "no basis".into(),
                    basis: Basis::default(),
                    expected: None,
                }],
            }],
            500,
        );
        assert!(all_refused[0].record_id.is_empty());
        assert!(matches!(
            all_refused[0].outcomes[0].outcome,
            OpOutcome::RefusedBasis { .. }
        ));
    }

    #[test]
    fn forget_blocks_later_loupist_puts_and_undo_restores_until_a_later_edit() {
        let (_dir, store) = store();
        let filed = file_in(
            &store,
            &turn("msg-1"),
            vec![create_subject("NVIDIA", vec![put("facts", "A", None)])],
            100,
        );
        let id = filed[0].record_id.clone();
        let forgotten = store
            .transact(|write| write.forget_section(LOUPIST, &id, "facts", 200))
            .unwrap();
        assert!(forgotten.body.sections().is_empty());
        let refused = file_in(
            &store,
            &FilingOrigin::Learning {
                review_id: "r".into(),
            },
            vec![existing(&id, vec![put("facts", "A again", None)])],
            300,
        );
        assert!(matches!(
            refused[0].outcomes[0].outcome,
            OpOutcome::RefusedForgotten { .. }
        ));
        let page = read(&store, &id);
        assert_eq!(page.history[0].outcome, "refused_forgotten");
        let forget_op = page.history[1].op_id.clone();
        assert_eq!(page.history[1].outcome, "retired");
        let back = store
            .transact(|write| write.undo_op(LOUPIST, &forget_op, 400))
            .unwrap();
        assert_eq!(back.body.sections()[0].markdown, "A");
        assert_eq!(
            back.history[0].undoes_op_id.as_deref(),
            Some(forget_op.as_str())
        );
        assert!(matches!(
            store.transact(|write| write.undo_op(LOUPIST, &forget_op, 450)),
            Err(KnowledgeError::Invalid(_))
        ));
        // The key is the user's again: a learning put lands.
        let relearned = file_in(
            &store,
            &FilingOrigin::Learning {
                review_id: "r2".into(),
            },
            vec![existing(
                &id,
                vec![put("facts", "A2", Some(digest(&back, "facts")))],
            )],
            500,
        );
        assert!(relearned[0].changed);
        let undo_of_restore =
            store.transact(|write| write.undo_op(LOUPIST, &back.history[0].op_id, 600));
        assert!(matches!(undo_of_restore, Err(KnowledgeError::Invalid(_))));
        // A user put of a new key is undone by retiring it.
        let user_put = file_in(
            &store,
            &user(),
            vec![existing(&id, vec![put("views", "Mine", None)])],
            700,
        );
        let undone = store
            .transact(|write| write.undo_op(LOUPIST, &user_put[0].outcomes[0].op_id, 800))
            .unwrap();
        assert!(undone
            .body
            .sections()
            .iter()
            .all(|s| s.key.as_str() != "views"));
    }

    #[test]
    fn a_name_binds_to_a_node_only_when_the_world_spells_it_so() {
        let (_dir, store) = store();
        let world = store
            .create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 1,
            })
            .unwrap();
        let subject = |message: &str, name: &str, at_ms| {
            let filed = file_in(
                &store,
                &turn(message),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Subject,
                        title: name.to_owned(),
                        anchors: vec![AnchorInput::Name { name: name.into() }],
                    },
                    ops: vec![put("facts", "A", None)],
                }],
                at_ms,
            );
            filed[0].record_id.clone()
        };
        let node_of = |id: &str| read(&store, id).anchors[0].node_id.clone();
        let node = |id: &str, name: &str, aliases: &[&str]| GraphNode {
            id: id.into(),
            name: name.into(),
            aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
            description: String::new(),
            kind: "Organization".into(),
            source_role: "subject".into(),
            properties: Default::default(),
        };
        let seal = |nodes: Vec<GraphNode>, at_ms| {
            let head: Option<String> = store
                .read(|w| {
                    Ok(w.tx
                        .query_row(
                            "SELECT revision_id FROM knowledge_record_heads WHERE record_id = ?1",
                            [&world.record_id],
                            |row| row.get(0),
                        )
                        .optional()?)
                })
                .unwrap();
            store
                .seal_world(SealWorld {
                    loupist_id: LOUPIST.into(),
                    record_id: world.record_id.clone(),
                    expected_revision_id: head,
                    at_ms,
                    nodes,
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                })
                .unwrap();
        };
        seal(vec![node("nod_hana", "Hana", &[])], 2);
        // "Hana" is the World's node. "Hana Co." may be another company the
        // World has not met yet: its page stays its own and its anchor pending.
        let hana = subject("m1", "Hana", 3);
        assert_eq!(node_of(&hana).as_deref(), Some("nod_hana"));
        let hana_co = subject("m2", "Hana Co.", 4);
        assert_ne!(hana_co, hana);
        assert_eq!(node_of(&hana_co), None);
        let suffixed = subject("m3", "Halvorsen Cellworks ASA", 5);
        assert_eq!(node_of(&suffixed), None);
        // A publication that spells the names, as a name or an alias, binds
        // them to the nodes it says, and the pages stay apart.
        seal(
            vec![
                node("nod_hana", "Hana", &[]),
                node("nod_hana_co", "Hana Company", &["Hana Co."]),
                node("nod_h", "Halvorsen Cellworks", &[]),
            ],
            6,
        );
        assert_eq!(node_of(&hana_co).as_deref(), Some("nod_hana_co"));
        assert_eq!(node_of(&suffixed), None);
        assert_eq!(node_of(&hana).as_deref(), Some("nod_hana"));
        assert_eq!(subject("m4", "hana co.", 7), hana_co);
        assert_eq!(subject("m5", "Hana", 8), hana);
    }

    #[test]
    fn world_links_need_known_nodes_and_resolve_pending_anchors() {
        let (_dir, store) = store();
        let world = store
            .create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 1,
            })
            .unwrap();
        let node = GraphNode {
            id: "nod_1".into(),
            name: "NVIDIA".into(),
            aliases: vec!["NVDA".into()],
            description: String::new(),
            kind: "Organization".into(),
            source_role: "subject".into(),
            properties: Default::default(),
        };
        let filed = file_in(
            &store,
            &turn("m"),
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Subject,
                    title: "Chips".into(),
                    anchors: vec![AnchorInput::Name {
                        name: "nvda".into(),
                    }],
                },
                ops: vec![put("facts", "A", None)],
            }],
            5,
        );
        let id = filed[0].record_id.clone();
        assert!(read(&store, &id).anchors[0].node_id.is_none());
        let unknown = file_in(
            &store,
            &turn("m2"),
            vec![existing(&id, vec![put("views", "[x](world:nod_1)", None)])],
            6,
        );
        assert_eq!(unknown[0].outcomes[0].outcome, OpOutcome::RefusedText);
        assert!(!unknown[0].changed);
        store
            .seal_world(SealWorld {
                loupist_id: LOUPIST.into(),
                record_id: world.record_id,
                expected_revision_id: None,
                at_ms: 7,
                nodes: vec![node.clone()],
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap();
        // The publication itself resolved the anchor; a second pass finds nothing pending.
        let resolved = store
            .transact(|write| write.resolve_anchors(LOUPIST, std::slice::from_ref(&node)))
            .unwrap();
        assert_eq!(resolved, 0);
        file_in(
            &store,
            &turn("m3"),
            vec![existing(&id, vec![put("views", "[x](world:nod_1)", None)])],
            8,
        );
        let page = read(&store, &id);
        assert!(page
            .anchors
            .iter()
            .any(|a| a.role == AnchorRole::Mention && a.node_id.as_deref() == Some("nod_1")));
        assert_eq!(page.anchors[0].node_id.as_deref(), Some("nod_1"));
        let pages = store
            .read(|write| write.pages_for_node(LOUPIST, "nod_1"))
            .unwrap();
        assert_eq!(pages[0].record_id, id);
        let portable = file_in(
            &store,
            &turn("m4"),
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Method,
                    title: "Method".into(),
                    anchors: vec![],
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new("steps").unwrap(),
                    heading: "Steps".into(),
                    markdown: "[x](world:nod_1)".into(),
                    basis: Basis::default(),
                    expected: None,
                }],
            }],
            9,
        );
        assert!(portable[0].record_id.is_empty());
        assert_eq!(portable[0].outcomes[0].outcome, OpOutcome::RefusedText);
    }

    fn seal(
        store: &KnowledgeStore,
        loupist: &str,
        node_id: &str,
    ) -> crate::knowledge::WorldRevision {
        let world = store
            .create_record(CreateRecord {
                loupist_id: loupist.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 1,
            })
            .unwrap();
        store
            .seal_world(SealWorld {
                loupist_id: loupist.into(),
                record_id: world.record_id,
                expected_revision_id: None,
                at_ms: 2,
                nodes: vec![GraphNode {
                    id: node_id.into(),
                    name: node_id.to_uppercase(),
                    aliases: vec![],
                    description: String::new(),
                    kind: "Organization".into(),
                    source_role: "subject".into(),
                    properties: Default::default(),
                }],
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap()
    }

    fn retain(store: &KnowledgeStore) -> crate::knowledge::RetainedSource {
        let bytes = b"account text";
        store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: LOUPIST.into(),
                at_ms: 2,
                title: "Notes".into(),
                source_class: SourceClass::UserStatement,
                origin_locator: "pasted".into(),
                provider_identity: "editor".into(),
                authorship: Authorship::User,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes.txt".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    bytes: bytes.to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "Original text".into(),
                }),
            })
            .unwrap()
    }

    /// A delta of `kind` carrying `text` in one field; `page` is an existing
    /// page of that kind when the field needs one.
    fn delta_with(kind: PageKind, field: &str, text: &str, page: Option<&PageRead>) -> PageDelta {
        let basis = if matches!(kind, PageKind::Subject | PageKind::Idea) {
            claim()
        } else {
            Basis::default()
        };
        let target = match page {
            Some(page) => PageTarget::Existing {
                record_id: page.record_id.clone(),
            },
            None => PageTarget::Create {
                kind,
                title: if field == "title" {
                    text.into()
                } else {
                    "Page".into()
                },
                anchors: vec![],
            },
        };
        let op = match field {
            "title" | "markdown" => LensOp::Put {
                key: SectionKey::new("facts").unwrap(),
                heading: "Facts".into(),
                markdown: if field == "markdown" {
                    text.into()
                } else {
                    "ok".into()
                },
                basis,
                expected: None,
            },
            "heading" => LensOp::Put {
                key: SectionKey::new("facts").unwrap(),
                heading: text.into(),
                markdown: "ok".into(),
                basis,
                expected: None,
            },
            "uncertainty" => LensOp::Put {
                key: SectionKey::new("views").unwrap(),
                heading: "Views".into(),
                markdown: "ok".into(),
                basis: Basis {
                    refs: vec![BasisRef::Page {
                        record_id: "rec_x".into(),
                        section_key: None,
                    }],
                    uncertainty: Some(text.into()),
                },
                expected: None,
            },
            "summary" => LensOp::Summary { text: text.into() },
            "why" => LensOp::Retire {
                key: SectionKey::new("facts").unwrap(),
                expected: page
                    .map(|page| digest(page, "facts"))
                    .unwrap_or_else(|| "0".repeat(64)),
                why: text.into(),
            },
            other => panic!("{other}"),
        };
        PageDelta {
            target,
            ops: vec![op],
        }
    }

    #[test]
    fn private_references_are_refused_in_every_text_of_every_kind() {
        let (_dir, store) = store();
        let sealed = seal(&store, LOUPIST, "nod_1");
        let source = retain(&store);
        let forbidden = [
            "See journal://private".to_string(),
            "See [[journal:private]]".to_string(),
            format!("Cite jnl_{}.", "ab".repeat(16)),
            format!("After snapshot {}.", sealed.snapshot_id),
            format!("Open world {}.", sealed.record_id),
            format!("Use revision {}.", sealed.revision_id),
            format!("Open module rec_{}.", "cd".repeat(16)),
            "See world://accounts".to_string(),
            "See [[world:accounts]]".to_string(),
        ];
        let kinds = [
            PageKind::Perspective,
            PageKind::Method,
            PageKind::Subject,
            PageKind::Idea,
        ];
        for kind in kinds {
            let page = file_in(
                &store,
                &user(),
                vec![delta_with(kind, "markdown", "A plain section.", None)],
                3,
            );
            let page = read(&store, &page[0].record_id);
            for text in &forbidden {
                for field in [
                    "title",
                    "heading",
                    "markdown",
                    "uncertainty",
                    "summary",
                    "why",
                ] {
                    let existing = (field != "title").then_some(&page);
                    let refused = file_in(
                        &store,
                        &user(),
                        vec![delta_with(kind, field, text, existing)],
                        4,
                    );
                    assert_eq!(
                        refused[0].outcomes[0].outcome,
                        OpOutcome::RefusedText,
                        "{kind:?} {field} {text}"
                    );
                    assert!(!refused[0].changed, "{kind:?} {field} {text}");
                }
            }
            let stored = read(&store, &page.record_id).delivery_text().unwrap();
            assert!(!stored.contains(&sealed.snapshot_id));
            assert!(!stored.contains(&sealed.revision_id));
            assert!(!stored.contains("journal"));
            // A Source record id is an ordinary link and stays.
            let kept = file_in(
                &store,
                &user(),
                vec![existing(
                    &page.record_id,
                    vec![LensOp::Put {
                        key: SectionKey::new("sources").unwrap(),
                        heading: "Sources".into(),
                        markdown: format!("See {}", source.record_id),
                        basis: if matches!(kind, PageKind::Subject | PageKind::Idea) {
                            claim()
                        } else {
                            Basis::default()
                        },
                        expected: None,
                    }],
                )],
                5,
            );
            assert!(kept[0].changed, "{kind:?}");
            assert!(read(&store, &page.record_id)
                .delivery_text()
                .unwrap()
                .contains(&source.record_id));
        }
        let principle = store.transact(|write| {
            crate::knowledge::principles::principle_save(
                write.tx,
                LOUPIST,
                None,
                None,
                crate::knowledge::PrincipleInput {
                    statement: "Check cash".into(),
                    why: Some(format!("Because of {}", sealed.snapshot_id)),
                    example: None,
                    origin: "settings".into(),
                    origin_chat_id: None,
                    origin_message_id: None,
                },
                6,
            )
        });
        assert!(matches!(principle, Err(KnowledgeError::LensBodyForbidden)));
    }

    #[test]
    fn node_keys_parse_a_world_revision_once_and_know_which_nodes_it_holds() {
        let (_dir, store) = store();
        let loupist = "loupist-keys";
        let revision = seal(&store, loupist, "acme");
        let computed = || super::super::identity::KEY_COMPUTATIONS.with(|count| count.get());
        let keys = |nodes: &[&str]| -> BTreeMap<String, BTreeSet<String>> {
            store
                .read(|read| {
                    world_identity(read.tx, loupist, |world| {
                        nodes
                            .iter()
                            .filter_map(|id| Some(((*id).to_owned(), world.keys(id)?.clone())))
                            .collect()
                    })
                })
                .unwrap()
                .unwrap_or_default()
        };
        let before = computed();
        let first = keys(&["acme", "ghost"]);
        assert_eq!(first.keys().collect::<Vec<_>>(), ["acme"]);
        assert_eq!(first["acme"], BTreeSet::from(["acme".to_owned()]));
        assert_eq!(computed(), before + 1, "one node's keys computed");
        assert_eq!(keys(&["acme"]), first);
        assert_eq!(computed(), before + 1, "the second read reuses them");
        // A new revision is parsed again, and only the nodes asked about are keyed.
        let world = revision.record_id.clone();
        store
            .seal_world(SealWorld {
                loupist_id: loupist.into(),
                record_id: world,
                expected_revision_id: Some(revision.revision_id),
                at_ms: 3,
                nodes: ["acme", "beta"]
                    .iter()
                    .map(|id| GraphNode {
                        id: (*id).into(),
                        name: format!("{} Inc.", id.to_uppercase()),
                        aliases: vec![],
                        description: String::new(),
                        kind: "Organization".into(),
                        source_role: "subject".into(),
                        properties: Default::default(),
                    })
                    .collect(),
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap();
        assert_eq!(keys(&["beta"])["beta"], BTreeSet::from(["beta".to_owned()]));
        assert_eq!(computed(), before + 2);
        // Components key every node of the revision once, then are kept.
        let component = |node: &str| {
            store
                .read(|read| world_identity(read.tx, loupist, |world| world.component(node)))
                .unwrap()
                .flatten()
        };
        assert_eq!(component("acme").as_deref(), Some("acme"));
        assert_eq!(
            computed(),
            before + 3,
            "the other node keyed for the components"
        );
        assert_eq!(component("beta").as_deref(), Some("beta"));
        assert_eq!(component("ghost"), None);
        assert_eq!(computed(), before + 3);
    }

    /// A span of a retained Source, as a turn cites it.
    fn span_of(source: &crate::knowledge::RetainedSource) -> BasisRef {
        BasisRef::Span {
            record_id: source.record_id.clone(),
            revision_digest: source.revision_digest.clone(),
            span_id: source.span_id.clone().unwrap(),
        }
    }

    /// An idea page named after `company`, citing `span`; its anchor stays pending.
    fn pending_page(store: &KnowledgeStore, company: &str, span: &BasisRef) -> String {
        let filed = file_in(
            store,
            &turn(&format!("m-{company}")),
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Idea,
                    title: format!("{company} thesis"),
                    anchors: vec![AnchorInput::Name {
                        name: company.into(),
                    }],
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new("thesis").unwrap(),
                    heading: "Thesis".into(),
                    markdown: "Margins follow utilization.".into(),
                    basis: Basis {
                        refs: vec![span.clone()],
                        uncertainty: None,
                    },
                    expected: None,
                }],
            }],
            5,
        );
        let id = filed[0].record_id.clone();
        assert!(read(store, &id).anchors[0].node_id.is_none());
        id
    }

    fn company_node(id: &str, name: &str) -> GraphNode {
        GraphNode {
            id: id.into(),
            name: name.into(),
            aliases: vec![],
            description: String::new(),
            kind: "Organization".into(),
            source_role: "subject".into(),
            properties: Default::default(),
        }
    }

    /// A claim between `from` and `to` whose evidence cites `span`.
    fn claim_citing(id: &str, from: &str, to: &str, span: &BasisRef) -> GraphClaim {
        let BasisRef::Span { span_id, .. } = span else {
            unreachable!()
        };
        GraphClaim {
            id: id.into(),
            proposition: format!("{from} supplies {to}"),
            attribution: "report".into(),
            relation: "supplies".into(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: from.into(),
                    role: "source".into(),
                },
                ClaimEndpoint {
                    node_id: to.into(),
                    role: "target".into(),
                },
            ],
            qualifiers: Vec::new(),
            evidence: vec![crate::knowledge::EvidenceBinding {
                span_id: span_id.clone(),
                role: crate::knowledge::BindingRole::ExtractedFrom,
                attribution: "report".into(),
            }],
            status: ClaimStatus::Active,
            recorded_at_ms: Some(7),
            publication_time: None,
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        }
    }

    fn seal_with_claims(store: &KnowledgeStore, nodes: Vec<GraphNode>, claims: Vec<GraphClaim>) {
        let world = store
            .create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 1,
            })
            .unwrap();
        store
            .seal_world(SealWorld {
                loupist_id: LOUPIST.into(),
                record_id: world.record_id,
                expected_revision_id: None,
                at_ms: 7,
                nodes,
                claims,
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap();
    }

    fn anchor_node(store: &KnowledgeStore, record_id: &str) -> Option<String> {
        read(store, record_id).anchors[0].node_id.clone()
    }

    #[test]
    fn a_source_the_page_cites_never_binds_its_anchor() {
        let (_dir, store) = store();
        let report = span_of(&retain(&store));
        let page = pending_page(&store, "Halvorsen Cellworks", &report);
        // Extraction canonicalized the company with its legal form and drew a
        // claim from the report the page cites. Only an exact spelling binds:
        // the anchor stays pending, and a later create under the World's name
        // is a page of its own, not this one.
        seal_with_claims(
            &store,
            vec![
                company_node("nod_h", "Halvorsen Cellworks ASA"),
                company_node("nod_m", "Meridian Motors"),
            ],
            vec![claim_citing("clm_1", "nod_h", "nod_m", &report)],
        );
        assert_eq!(anchor_node(&store, &page), None);
        let filed = file_in(
            &store,
            &turn("m-later"),
            vec![create_subject(
                "Halvorsen Cellworks ASA",
                vec![put("facts", "Listed in Oslo.", None)],
            )],
            8,
        );
        assert!(filed[0].created);
        assert_ne!(filed[0].record_id, page);
        assert_eq!(
            anchor_node(&store, &filed[0].record_id).as_deref(),
            Some("nod_h")
        );
        let pages = store
            .read(|write| write.pages_for_node(LOUPIST, "nod_h"))
            .unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].record_id, filed[0].record_id);
        assert_eq!(anchor_node(&store, &page), None);
    }

    #[test]
    fn world_links_are_allowed_only_on_local_pages_to_this_loupists_nodes() {
        let (_dir, store) = store();
        seal(&store, LOUPIST, "nod_1");
        seal(&store, "loupist-b", "nod_b");
        // Every link form a Markdown reader resolves counts, whatever the scheme's case.
        for kind in [PageKind::Perspective, PageKind::Method] {
            for link in [
                "[x](world:nod_1)",
                "See [Ada][a].\n\n[a]: world:nod_1",
                "<world:nod_1>",
                "[x](WORLD:nod_1)",
            ] {
                let refused = file_in(
                    &store,
                    &user(),
                    vec![delta_with(kind, "markdown", link, None)],
                    3,
                );
                assert!(refused[0].record_id.is_empty(), "{kind:?} {link}");
                assert_eq!(
                    refused[0].outcomes[0].outcome,
                    OpOutcome::RefusedText,
                    "{kind:?} {link}"
                );
            }
        }
        for kind in [PageKind::Subject, PageKind::Idea] {
            let filed = file_in(
                &store,
                &user(),
                vec![delta_with(kind, "markdown", "[x](world:nod_1)", None)],
                3,
            );
            let page = read(&store, &filed[0].record_id);
            assert!(page
                .anchors
                .iter()
                .any(|a| a.role == AnchorRole::Mention && a.node_id.as_deref() == Some("nod_1")));
            for link in [
                "[x](world:nod_b)",
                "[x](world:nod_missing)",
                "See [it][b].\n\n[b]: world:nod_missing",
            ] {
                let refused = file_in(
                    &store,
                    &user(),
                    vec![delta_with(kind, "markdown", link, Some(&page))],
                    4,
                );
                assert_eq!(
                    refused[0].outcomes[0].outcome,
                    OpOutcome::RefusedText,
                    "{kind:?} {link}"
                );
            }
        }
    }

    #[test]
    fn archive_hides_backlinks_and_restore_refuses_a_taken_title() {
        let (_dir, store) = store();
        let method = |title: &str, markdown: String| PageDelta {
            target: PageTarget::Create {
                kind: PageKind::Method,
                title: title.into(),
                anchors: vec![],
            },
            ops: vec![LensOp::Put {
                key: SectionKey::new("steps").unwrap(),
                heading: "Steps".into(),
                markdown,
                basis: Basis::default(),
                expected: None,
            }],
        };
        let related = file_in(&store, &user(), vec![method("Related", "x".into())], 1);
        let related_id = related[0].record_id.clone();
        let main = file_in(
            &store,
            &user(),
            vec![method(
                "Main",
                format!("Start from [Related](lens:{related_id})."),
            )],
            2,
        );
        let main_id = main[0].record_id.clone();
        assert_eq!(read(&store, &main_id).links[0].record_id, related_id);
        assert_eq!(read(&store, &related_id).backlinks[0].record_id, main_id);
        store
            .transact(|write| write.set_lens_archived(LOUPIST, &main_id, true, "Hide.", 3, "user"))
            .unwrap();
        assert!(read(&store, &related_id).backlinks.is_empty());
        let again = file_in(&store, &user(), vec![method("main", "fresh".into())], 4);
        assert_ne!(again[0].record_id, main_id);
        assert!(matches!(
            store.transact(
                |write| write.set_lens_archived(LOUPIST, &main_id, false, "Back.", 5, "user")
            ),
            Err(KnowledgeError::Invalid(_))
        ));
    }

    #[test]
    fn index_entries_carry_summary_section_count_and_map_names() {
        let (_dir, store) = store();
        file_in(
            &store,
            &turn("m"),
            vec![create_subject(
                "NVIDIA",
                vec![
                    put("facts", "A", None),
                    LensOp::Summary {
                        text: "GPU maker.".into(),
                    },
                ],
            )],
            1,
        );
        let entries = store.read(|write| write.lens_index(LOUPIST)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "GPU maker.");
        assert_eq!(entries[0].section_count, 1);
        assert_eq!(entries[0].map_names, vec!["nvidia".to_string()]);
    }
}
