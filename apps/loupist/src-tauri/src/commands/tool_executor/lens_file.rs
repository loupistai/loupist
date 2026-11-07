//! `lens_file`: the Loupist files what a turn established into Lens pages.
//! Every handle in the call (a `readRef`, a `saveRef`, `said`, a page id) is
//! resolved here against this turn's acknowledged reads and saves, so a page
//! never cites evidence the model did not read. The pages are written by
//! `KnowledgeWrite::file` inside the reply's own transaction; a stopped or
//! failed turn files nothing.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Transaction;

use super::*;
use crate::knowledge::lens_page::{
    AnchorInput, Basis, BasisRef, FiledPage, FilingOrigin, IdeaStatus, LensOp, PageDelta, PageKind,
    PageTarget, SectionKey, TextField, TextRefusal,
};

/// Pages one turn may file across its `lens_file` calls.
const MAX_TURN_PAGES: usize = 8;
/// Ops one turn may file across its `lens_file` calls.
const MAX_TURN_OPS: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::commands) struct StagedFiling {
    pub deltas: Vec<PageDelta>,
    /// Span refs whose Source is saved in this same turn: the record exists
    /// only once the reply commits, so the ref is completed there.
    pub pending_spans: Vec<PendingSpan>,
    pub rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::commands) struct PendingSpan {
    page: usize,
    op: usize,
    basis_ref: usize,
    save_ref: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    pages: Vec<PageInput>,
    rationale: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageInput {
    page: PageRef,
    ops: Vec<OpInput>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PageRef {
    Existing(ExistingPage),
    Create(CreatePage),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ExistingPage {
    record_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePage {
    create: CreateInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CreateInput {
    kind: PageKind,
    title: String,
    #[serde(default)]
    about: Vec<AboutInput>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AboutInput {
    Node(AboutNode),
    Name(AboutName),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AboutNode {
    node_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AboutName {
    name: String,
}

#[derive(Deserialize)]
#[serde(
    tag = "op",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum OpInput {
    Put {
        key: String,
        heading: String,
        markdown: String,
        #[serde(default)]
        basis: BasisInput,
        expected: Option<String>,
    },
    Retire {
        key: String,
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

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasisInput {
    #[serde(default)]
    refs: Vec<RefInput>,
    uncertainty: Option<String>,
}

/// One distinct required field per handle kind keeps the untagged match exact.
#[derive(Deserialize)]
#[serde(untagged)]
enum RefInput {
    World(WorldRef),
    Source(SourceRef),
    Said(SaidRef),
    Lens(LensRef),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorldRef {
    world: String,
    claim_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SourceRef {
    source: String,
    span_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaidRef {
    said: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LensRef {
    lens: String,
    key: Option<String>,
}

/// An exact Source read: what a span basis needs from it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceHandle {
    record_id: String,
    revision_digest: String,
    span_id: Option<String>,
}

/// The identifiers this turn acknowledged, copied out of the capture so
/// resolution holds no lock. Only acknowledged reads and saves enter.
#[derive(Debug, Default)]
pub(super) struct TurnHandles {
    /// World `readRef` to the claim ids its view delivered.
    world_reads: BTreeMap<String, BTreeSet<String>>,
    source_reads: BTreeMap<String, SourceHandle>,
    /// Pages exact-read by the model this turn, by record id, with their titles.
    lens_reads: BTreeMap<String, String>,
    /// Pages the turn saw: exact reads and the standing context.
    lens_known: BTreeSet<String>,
    save_refs: BTreeSet<String>,
    /// The user message a `said` basis names; None without one or unattended.
    said: Option<String>,
    unattended: bool,
}

impl AppToolExecutor {
    pub(super) async fn stage_lens_file(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let request: Input = serde_json::from_value(params).map_err(|_| {
            BrokerError::invalid(
                "pages",
                "pages: [{page: {recordId} | {create: {kind, title, about?}}, ops: [{op: put|retire|summary|idea_status, ...}]}] and rationale, as the tool describes",
            )
        })?;
        let handles = self.turn_handles().await;
        let (mut filing, pages) = resolve(request, &handles)?;
        self.screen(&policy.loupist_id, &mut filing, &handles)?;
        self.reserve_budget(delivery_id, &filing).await?;
        self.capture
            .stage(delivery_id, |pending| pending.filings.push(filing))
            .await;
        Ok(json!({"status": "staged", "durable": false, "pages": pages}))
    }

    /// Runs the writer's text policy and create resolution now, so what the
    /// commit would refuse is refused here by its field. A create landing on
    /// a page this turn exact-read files as a revision of that page; one
    /// landing on a page it did not read may only add sections.
    fn screen(
        &self,
        loupist_id: &str,
        filing: &mut StagedFiling,
        handles: &TurnHandles,
    ) -> Result<(), BrokerError> {
        let screens =
            crate::commands::knowledge::screen_filing(&self.state, loupist_id, &filing.deltas)
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
        for (index, (delta, screen)) in filing.deltas.iter_mut().zip(screens).enumerate() {
            if let Some((field, reason)) = screen.refusals.first() {
                let field = match field {
                    TextField::Title => format!("pages[{index}].page.create.title"),
                    TextField::About(about) => {
                        format!("pages[{index}].page.create.about[{about}].nodeId")
                    }
                    TextField::Op(op, name) => format!("pages[{index}].ops[{op}].{name}"),
                };
                return Err(BrokerError::invalid(field, refusal_hint(*reason)));
            }
            let Some(record_id) = screen.existing else {
                continue;
            };
            if handles.lens_reads.contains_key(&record_id) {
                delta.target = PageTarget::Existing { record_id };
            } else if let Some(op) = delta
                .ops
                .iter()
                .position(|op| !matches!(op, LensOp::Put { expected: None, .. }))
            {
                return Err(BrokerError::invalid(
                    format!("pages[{index}].ops[{op}].op"),
                    "this create lands on an existing page this turn has not read; read it with lens_read and revise it by page.recordId, or file only new sections",
                ));
            }
        }
        Ok(())
    }

    /// Reserves this call's pages and ops against the turn's budget under one
    /// lock, so calls Pi runs in parallel share it. An undelivered call frees
    /// its share.
    async fn reserve_budget(
        &self,
        delivery_id: &str,
        filing: &StagedFiling,
    ) -> Result<(), BrokerError> {
        let pages = filing.deltas.len();
        let ops = filing
            .deltas
            .iter()
            .map(|delta| delta.ops.len())
            .sum::<usize>();
        let mut reserved = self.capture.lens_file_budget.lock().await;
        let (reserved_pages, reserved_ops) = reserved
            .iter()
            .filter(|(delivery, _)| delivery.as_str() != delivery_id)
            .fold((0, 0), |(pages, ops), (_, used)| {
                (pages + used.0, ops + used.1)
            });
        if reserved_pages + pages > MAX_TURN_PAGES || reserved_ops + ops > MAX_TURN_OPS {
            return Err(BrokerError::invalid(
                "pages",
                format!("at most {MAX_TURN_PAGES} pages and {MAX_TURN_OPS} ops per reply, counting earlier lens_file calls"),
            ));
        }
        reserved.insert(delivery_id.to_owned(), (pages, ops));
        Ok(())
    }

    async fn turn_handles(&self) -> TurnHandles {
        let mut handles = TurnHandles {
            unattended: self.capture.unattended_run_id.is_some(),
            ..TurnHandles::default()
        };
        for read in self.capture.staged_reads.lock().await.iter() {
            match &read.payload {
                AckedPayload::World { view_json, .. } => {
                    handles
                        .world_reads
                        .insert(read.read_ref.clone(), claim_ids(view_json));
                }
                AckedPayload::Source {
                    revision_digest, ..
                } => {
                    handles.source_reads.insert(
                        read.read_ref.clone(),
                        SourceHandle {
                            record_id: read.record_id.clone(),
                            revision_digest: revision_digest.clone(),
                            span_id: read.span_id.clone(),
                        },
                    );
                }
                AckedPayload::Lens { .. } => {
                    handles
                        .lens_reads
                        .insert(read.record_id.clone(), read.title.clone());
                    handles.lens_known.insert(read.record_id.clone());
                }
                AckedPayload::Journal { .. } => {}
            }
        }
        for read in self.capture.context_reads.lock().await.iter() {
            if matches!(read.payload, AckedPayload::Lens { .. }) {
                handles.lens_known.insert(read.record_id.clone());
            }
        }
        handles.save_refs = self
            .capture
            .staged_sources
            .lock()
            .await
            .iter()
            .map(|source| source.request_ref.clone())
            .collect();
        if !handles.unattended {
            handles.said = self.capture.principle_user_message_id.clone();
        }
        handles
    }
}

fn refusal_hint(reason: TextRefusal) -> &'static str {
    match reason {
        TextRefusal::Private => "remove the private reference: Journal links, world:// forms, snapshot or journal ids, and record or revision ids that are not a Source or a Lens page never enter a page",
        TextRefusal::PortableWorldLink => "principle, perspective and method pages are shared and never link world: nodes; write the name as plain text",
        TextRefusal::UnknownNode => "a node id this Loupist's World holds, as world_read delivered it; otherwise write the name as plain text (or about {name} for a page's subject)",
    }
}

/// Claim ids a delivered World view carries, as the model saw them.
fn claim_ids(view_json: &str) -> BTreeSet<String> {
    let Ok(view) = serde_json::from_str::<Value>(view_json) else {
        return BTreeSet::new();
    };
    view["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|edge| {
            [edge["claim"]["id"].as_str(), edge["id"].as_str()]
                .into_iter()
                .flatten()
                .map(str::to_owned)
        })
        .collect()
}

/// Turns the call into deltas the writer accepts, refusing any handle this
/// turn did not acknowledge by the field that carried it.
fn resolve(
    request: Input,
    handles: &TurnHandles,
) -> Result<(StagedFiling, Vec<Value>), BrokerError> {
    if request.pages.is_empty() {
        return Err(BrokerError::invalid("pages", "at least one page"));
    }
    if request.rationale.trim().is_empty()
        || request.rationale.chars().count() > 2_000
        || request
            .rationale
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(BrokerError::invalid(
            "rationale",
            "one to 2,000 characters saying what this turn established, without control characters",
        ));
    }
    let mut deltas = Vec::with_capacity(request.pages.len());
    let mut pending_spans = Vec::new();
    let mut summaries = Vec::with_capacity(request.pages.len());
    for (page_index, page) in request.pages.into_iter().enumerate() {
        let (target, title, kind) = match page.page {
            PageRef::Existing(existing) => {
                let field = format!("pages[{page_index}].page.recordId");
                let Some(title) = handles.lens_reads.get(&existing.record_id) else {
                    return Err(BrokerError::invalid(
                        field,
                        "a page read with lens_read this turn; revise it with the section digests from that readRef, or create a page",
                    ));
                };
                (
                    PageTarget::Existing {
                        record_id: existing.record_id,
                    },
                    title.clone(),
                    None,
                )
            }
            PageRef::Create(CreatePage { create }) => {
                let anchors = create
                    .about
                    .into_iter()
                    .map(|about| match about {
                        AboutInput::Node(node) => AnchorInput::Node {
                            node_id: node.node_id,
                        },
                        AboutInput::Name(name) => AnchorInput::Name { name: name.name },
                    })
                    .collect();
                let title = create.title.trim().to_owned();
                (
                    PageTarget::Create {
                        kind: create.kind,
                        title: title.clone(),
                        anchors,
                    },
                    title,
                    Some(create.kind),
                )
            }
        };
        let creating = matches!(target, PageTarget::Create { .. });
        let mut ops = Vec::with_capacity(page.ops.len());
        for (op_index, op) in page.ops.into_iter().enumerate() {
            let field = |name: &str| format!("pages[{page_index}].ops[{op_index}].{name}");
            let section_key = |key: &str, name: &str| {
                SectionKey::new(key).map_err(|_| {
                    BrokerError::invalid(
                        field(name),
                        "a slug of lowercase letters, digits and hyphens, at most 48 characters",
                    )
                })
            };
            let op = match op {
                OpInput::Put {
                    key,
                    heading,
                    markdown,
                    basis,
                    expected,
                } => {
                    if creating && expected.is_some() {
                        return Err(BrokerError::invalid(
                            field("expected"),
                            "omit expected on a new page; to revise a section, name the page you read as page.recordId",
                        ));
                    }
                    let mut refs = Vec::with_capacity(basis.refs.len());
                    for (ref_index, reference) in basis.refs.into_iter().enumerate() {
                        let resolved = resolve_ref(reference, handles, &|name| {
                            field(&format!("basis.refs[{ref_index}].{name}"))
                        })?;
                        refs.push(match resolved {
                            Resolved::Ready(reference) => reference,
                            Resolved::SavedSource(save_ref) => {
                                pending_spans.push(PendingSpan {
                                    page: page_index,
                                    op: op_index,
                                    basis_ref: ref_index,
                                    save_ref,
                                });
                                BasisRef::Span {
                                    record_id: String::new(),
                                    revision_digest: String::new(),
                                    span_id: String::new(),
                                }
                            }
                        });
                    }
                    LensOp::Put {
                        key: section_key(&key, "key")?,
                        heading,
                        markdown,
                        basis: Basis {
                            refs,
                            uncertainty: basis
                                .uncertainty
                                .map(|text| text.trim().to_owned())
                                .filter(|text| !text.is_empty()),
                        },
                        expected,
                    }
                }
                OpInput::Retire { key, expected, why } => {
                    if creating {
                        return Err(BrokerError::invalid(
                            field("op"),
                            "retire needs the page you read as page.recordId; a new page has nothing to retire",
                        ));
                    }
                    LensOp::Retire {
                        key: section_key(&key, "key")?,
                        expected,
                        why,
                    }
                }
                OpInput::Summary { text } => LensOp::Summary { text },
                OpInput::IdeaStatus { status, why } => LensOp::IdeaStatus { status, why },
            };
            ops.push(op);
        }
        let delta = PageDelta { target, ops };
        delta.validate().map_err(|error| {
            BrokerError::invalid(format!("pages[{page_index}]"), error.to_string())
        })?;
        summaries.push(json!({
            "title": title,
            "kind": kind.map(|kind| kind.lens_kind()),
            "ops": delta.ops.len(),
        }));
        deltas.push(delta);
    }
    Ok((
        StagedFiling {
            deltas,
            pending_spans,
            rationale: request.rationale.trim().to_owned(),
        },
        summaries,
    ))
}

enum Resolved {
    Ready(BasisRef),
    SavedSource(String),
}

fn resolve_ref(
    reference: RefInput,
    handles: &TurnHandles,
    field: &dyn Fn(&str) -> String,
) -> Result<Resolved, BrokerError> {
    Ok(match reference {
        RefInput::World(world) => {
            let Some(claims) = handles.world_reads.get(&world.world) else {
                return Err(BrokerError::invalid(
                    field("world"),
                    "the readRef of a world_read (scope world) acknowledged this turn",
                ));
            };
            if !claims.contains(&world.claim_id) {
                return Err(BrokerError::invalid(
                    field("claimId"),
                    "a claim id (edges[].claim.id) delivered by that world_read",
                ));
            }
            Resolved::Ready(BasisRef::Claim {
                claim_id: world.claim_id,
            })
        }
        RefInput::Source(source) => {
            if let Some(read) = handles.source_reads.get(&source.source) {
                let span_id = match (source.span_id, &read.span_id) {
                    (Some(given), Some(known)) if &given == known => given,
                    (None, Some(known)) => known.clone(),
                    _ => {
                        return Err(BrokerError::invalid(
                            field("spanId"),
                            "the spanId that source read delivered, or omit it",
                        ))
                    }
                };
                Resolved::Ready(BasisRef::Span {
                    record_id: read.record_id.clone(),
                    revision_digest: read.revision_digest.clone(),
                    span_id,
                })
            } else if handles.save_refs.contains(&source.source) {
                if source.span_id.is_some() {
                    return Err(BrokerError::invalid(
                        field("spanId"),
                        "omit spanId for a saveRef; the saved original is the span",
                    ));
                }
                Resolved::SavedSource(source.source)
            } else {
                return Err(BrokerError::invalid(
                    field("source"),
                    "a Source readRef from world_read (scope source) or a saveRef from source_save, acknowledged this turn",
                ));
            }
        }
        RefInput::Said(said) => {
            if !said.said {
                return Err(BrokerError::invalid(field("said"), "true"));
            }
            if handles.unattended {
                return Err(BrokerError::invalid(
                    field("said"),
                    "unavailable in an unattended run; cite a source or world read instead",
                ));
            }
            let Some(message_id) = &handles.said else {
                return Err(BrokerError::invalid(
                    field("said"),
                    "this turn has no user message to cite",
                ));
            };
            Resolved::Ready(BasisRef::UserSaid {
                message_id: message_id.clone(),
            })
        }
        RefInput::Lens(lens) => {
            if !handles.lens_known.contains(&lens.lens) {
                return Err(BrokerError::invalid(
                    field("lens"),
                    "the recordId of a page read with lens_read this turn",
                ));
            }
            let section_key = lens
                .key
                .as_deref()
                .map(SectionKey::new)
                .transpose()
                .map_err(|_| {
                    BrokerError::invalid(field("key"), "a section key from that page, or omit it")
                })?;
            Resolved::Ready(BasisRef::Page {
                record_id: lens.lens,
                section_key,
            })
        }
    })
}

/// Files every staged delta of the turn inside the reply's transaction and
/// reports each filed page once. A `saveRef` span resolves to the Source the
/// same commit retained; when that Source was not retained the ref is
/// dropped and the fold decides what the section may keep.
pub(crate) fn commit_turn(
    tx: &Transaction<'_>,
    blobs: &crate::knowledge::BlobStore,
    loupist_id: &str,
    origin: &FilingOrigin,
    at_ms: i64,
    staged: &[StagedFiling],
    retained: &[(String, crate::knowledge::RetainedSource)],
) -> Result<Vec<crate::domain::MemoryUpdateChange>, crate::knowledge::KnowledgeError> {
    let mut deltas = Vec::new();
    let mut rationales = Vec::new();
    for filing in staged {
        let mut filing_deltas = filing.deltas.clone();
        let mut dropped: BTreeMap<(usize, usize), BTreeSet<usize>> = BTreeMap::new();
        for pending in &filing.pending_spans {
            let saved = retained
                .iter()
                .find(|(save_ref, _)| save_ref == &pending.save_ref)
                .and_then(|(_, saved)| {
                    saved.span_id.clone().map(|span_id| BasisRef::Span {
                        record_id: saved.record_id.clone(),
                        revision_digest: saved.revision_digest.clone(),
                        span_id,
                    })
                });
            let Some(LensOp::Put { basis, .. }) = filing_deltas
                .get_mut(pending.page)
                .and_then(|delta| delta.ops.get_mut(pending.op))
            else {
                continue;
            };
            match saved {
                Some(reference) => {
                    if let Some(slot) = basis.refs.get_mut(pending.basis_ref) {
                        *slot = reference;
                    }
                }
                None => {
                    dropped
                        .entry((pending.page, pending.op))
                        .or_default()
                        .insert(pending.basis_ref);
                }
            }
        }
        for ((page, op), indices) in dropped {
            if let Some(LensOp::Put { basis, .. }) = filing_deltas
                .get_mut(page)
                .and_then(|delta| delta.ops.get_mut(op))
            {
                let mut index = 0;
                basis.refs.retain(|_| {
                    let keep = !indices.contains(&index);
                    index += 1;
                    keep
                });
            }
        }
        rationales.push(filing.rationale.as_str());
        deltas.extend(filing_deltas);
    }
    if deltas.is_empty() {
        return Ok(Vec::new());
    }
    let filed = crate::knowledge::KnowledgeWrite::bind(tx, blobs)
        .file(loupist_id, origin, deltas, at_ms)?;
    crate::knowledge::learning::enqueue(tx, loupist_id, at_ms)?;
    Ok(changes(filed, &rationales.join(" ")))
}

/// One Memory change per filed page: two deltas to the same page in one turn
/// report once, with every op's outcome.
fn changes(filed: Vec<FiledPage>, rationale: &str) -> Vec<crate::domain::MemoryUpdateChange> {
    let mut changes: Vec<crate::domain::MemoryUpdateChange> = Vec::new();
    for page in filed {
        if page.record_id.is_empty() {
            // A create whose ops all refused left no record behind.
            continue;
        }
        let outcomes: Vec<crate::domain::FiledOutcome> = page
            .outcomes
            .into_iter()
            .map(|op| crate::domain::FiledOutcome {
                op_id: op.op_id,
                key: op.outcome.key().map(|key| key.as_str().to_owned()),
                outcome: op.outcome.name().into(),
            })
            .collect();
        if let Some(existing) = changes
            .iter_mut()
            .find(|change| change.record_id == page.record_id)
        {
            existing.outcomes.extend(outcomes);
            existing.lesson = lesson(&existing.outcomes);
            if page.created {
                existing.operation = "create".into();
            }
            continue;
        }
        changes.push(crate::domain::MemoryUpdateChange {
            world_job_id: None,
            world_status: None,
            knowledge: Some(crate::domain::KnowledgeReference {
                kind: "lens".into(),
                record_id: page.record_id.clone(),
                revision_id: page.revision_id.clone(),
                object_id: page.record_id.clone(),
                span_id: None,
                receipt_id: None,
            }),
            record_id: page.record_id,
            kind: "Lens".into(),
            operation: if page.created { "create" } else { "revise" }.into(),
            title: page.title,
            lesson: lesson(&outcomes),
            basis: rationale.chars().take(2_000).collect(),
            future_use: "Read with lens_read in later research.".into(),
            outcomes,
        });
    }
    changes
}

/// "added facts, replaced risks": what the ops did to the page, in order.
fn lesson(outcomes: &[crate::domain::FiledOutcome]) -> String {
    outcomes
        .iter()
        .map(|outcome| {
            let name = outcome.outcome.replace('_', " ");
            match &outcome.key {
                Some(key) => format!("{name} {key}"),
                None => name,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
        .chars()
        .take(2_000)
        .collect()
}
