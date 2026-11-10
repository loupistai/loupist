use super::*;

pub(in crate::commands) const PERSPECTIVE_TITLE: &str = "Outlook";

/// The Loupist's one perspective page, kept in step with its focus: creation
/// seeds it, a later focus change from settings rewrites its `focus` section.
pub(in crate::commands) fn ensure_perspective(
    state: &AppState,
    loupist_id: &str,
    focus: &str,
) -> Result<Option<String>, CommandError> {
    let at_ms = crate::commands::now_ms();
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        sync_perspective_focus(
            &crate::knowledge::KnowledgeWrite::bind(tx, blobs),
            loupist_id,
            focus,
            at_ms,
        )
    })
}

/// Make the perspective page's `focus` section hold `focus`, creating the page
/// when there is none. Other sections stay as written. An empty focus files
/// nothing; the first filed perspective then becomes the page.
pub(in crate::commands) fn sync_perspective_focus(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    focus: &str,
    at_ms: i64,
) -> crate::knowledge::KnowledgeResult<Option<String>> {
    use crate::knowledge::lens_page::{
        Basis, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey, MAX_SECTION_CHARS,
    };
    let page = write.active_perspective(loupist_id)?;
    let focus: String = focus.trim().chars().take(MAX_SECTION_CHARS).collect();
    if focus.is_empty() {
        return Ok(page);
    }
    let key = SectionKey::new("focus")?;
    let (target, heading, expected) = match &page {
        Some(record_id) => {
            let read = write.read_page(loupist_id, record_id)?;
            let current = read
                .body
                .sections()
                .iter()
                .find(|section| section.key == key);
            if current.is_some_and(|section| section.markdown == focus) {
                return Ok(page);
            }
            (
                PageTarget::Existing {
                    record_id: record_id.clone(),
                },
                current.map_or_else(|| "Focus".to_owned(), |section| section.heading.clone()),
                current.map(|section| section.digest.clone()),
            )
        }
        None => (
            PageTarget::Create {
                kind: PageKind::Perspective,
                title: PERSPECTIVE_TITLE.into(),
                anchors: Vec::new(),
            },
            "Focus".to_owned(),
            None,
        ),
    };
    let filed = write.file(
        loupist_id,
        &FilingOrigin::User {
            chat_id: None,
            message_id: None,
        },
        vec![PageDelta {
            target,
            ops: vec![LensOp::Put {
                key,
                heading,
                markdown: focus,
                basis: Basis::default(),
                expected,
            }],
        }],
        at_ms,
    )?;
    if filed.iter().any(|page| page.refused_text()) {
        return Err(crate::knowledge::KnowledgeError::LensBodyForbidden);
    }
    Ok(filed
        .into_iter()
        .next()
        .filter(|page| !page.record_id.is_empty())
        .map(|page| page.record_id)
        .or(page))
}

#[derive(Clone)]
pub(in crate::commands) struct HostMemoryEntry {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub summary: String,
    pub as_of: String,
    /// Lens pages only: the page kind, section count and up to three map names.
    pub lens_kind: Option<crate::knowledge::lens_page::LensKind>,
    pub sections: usize,
    pub map_names: Vec<String>,
}

/// The Lens index shows at most this many pages.
pub(in crate::commands) const LENS_INDEX_LINES: usize = 60;
/// Pointers to pages anchored to named World nodes, then lexical matches.
const LENS_POINTERS: usize = 4;

pub(in crate::commands) struct HostMemoryContext {
    pub revision: String,
    pub principles: Vec<crate::knowledge::Principle>,
    pub principle_receipts: Vec<crate::domain::LensReceipt>,
    pub principles_block: String,
    pub charter: Option<String>,
    pub charter_read_ref: Option<String>,
    /// Exact body admitted into this turn's prompt. Not a tool delivery.
    pub(in crate::commands) charter_delivery:
        Option<crate::commands::tool_executor::StagedKnowledgeRead>,
    pub entries: Vec<HostMemoryEntry>,
    pub relevant: Vec<crate::agent_harness::RelevantMemoryPointer>,
    pub lens_shown: usize,
    pub lens_total: usize,
    pub world_shown: usize,
    pub world_total: usize,
    /// What an as-of cutoff left out of this context.
    pub left_out: super::as_of::LeftOut,
    /// This turn's context carries a shared item accepted despite a scan
    /// finding (a principle, the perspective, a catalog entry or a pointer).
    pub shared_pauses: bool,
}

pub(super) fn lens_catalog_preview(markdown: &str) -> String {
    let text = pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Text(text) => Some(text.into_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

/// `set_aside` lists principle and method ids the user set aside for this
/// chat ("Not this time"); they are left out of the catalog, the relevant
/// methods and the principles block.
/// Under an as-of `cutoff`, methods and the perspective appear only when
/// their current version was written by then, and relevant notes only when
/// recorded by then; principles stay as they are today.
pub(in crate::commands) fn host_memory_context_for_turn(
    state: &AppState,
    loupist_id: &str,
    message: &str,
    use_memory: bool,
    set_aside: &[String],
    cutoff: Option<super::as_of::Cutoff>,
) -> Result<Option<HostMemoryContext>, CommandError> {
    if !use_memory {
        return Ok(None);
    }
    let mut context = host_memory_context_excluding(state, loupist_id, message, set_aside)?;
    if let Some(cutoff) = cutoff {
        as_of_context(state, loupist_id, &mut context, cutoff)?;
    }
    Ok(Some(context))
}

/// Leaves out of the automatic context what the Loupist learned after the cutoff.
fn as_of_context(
    state: &AppState,
    loupist_id: &str,
    context: &mut HostMemoryContext,
    cutoff: super::as_of::Cutoff,
) -> Result<(), CommandError> {
    let mut left = super::as_of::LeftOut::default();
    let mut later_lenses = std::collections::BTreeSet::new();
    for entry in context.entries.iter().filter(|entry| entry.kind == "lens") {
        if !super::as_of::lens_known(state, loupist_id, &entry.id, cutoff)? {
            later_lenses.insert(entry.id.clone());
        }
    }
    context
        .entries
        .retain(|entry| !later_lenses.contains(&entry.id));
    if let Some(record_id) = context
        .charter_delivery
        .as_ref()
        .map(|staged| staged.record_id.clone())
        .or_else(|| {
            context.charter.as_deref().and_then(|charter| {
                serde_json::from_str::<serde_json::Value>(charter)
                    .ok()?
                    .get("recordId")?
                    .as_str()
                    .map(str::to_owned)
            })
        })
    {
        if !super::as_of::lens_known(state, loupist_id, &record_id, cutoff)? {
            context.charter = None;
            context.charter_read_ref = None;
            context.charter_delivery = None;
            later_lenses.insert(record_id);
        }
    }
    let mut kept = Vec::new();
    for mut pointer in std::mem::take(&mut context.relevant) {
        // A pointer to a later note names what was learned after the cutoff.
        super::as_of::later_notes_known(&mut pointer.later_notes, cutoff);
        let record_id = pointer.read_args["recordId"].as_str().unwrap_or_default();
        let note_id = pointer.read_args["noteId"].as_str().unwrap_or_default();
        if later_lenses.contains(record_id) {
            continue;
        }
        if !record_id.is_empty() && !super::as_of::lens_known(state, loupist_id, record_id, cutoff)?
        {
            later_lenses.insert(record_id.to_owned());
            continue;
        }
        if !note_id.is_empty() {
            let note = read_working_context(state, loupist_id, note_id)?;
            if !cutoff.recorded(note.recorded_at_ms) {
                left.notes.insert(note_id.to_owned());
                continue;
            }
        }
        kept.push(pointer);
    }
    context.relevant = kept;
    context.lens_shown = context
        .entries
        .iter()
        .filter(|entry| entry.kind == "lens")
        .count();
    context.lens_total = context.lens_total.saturating_sub(later_lenses.len());
    left.methods.extend(later_lenses);
    context.left_out.merge(left);
    Ok(())
}

/// Tests read the whole context without a chat's set-aside list.
#[cfg(test)]
pub(in crate::commands) fn host_memory_context(
    state: &AppState,
    loupist_id: &str,
    message: &str,
) -> Result<HostMemoryContext, CommandError> {
    host_memory_context_excluding(state, loupist_id, message, &[])
}

fn host_memory_context_excluding(
    state: &AppState,
    loupist_id: &str,
    message: &str,
    set_aside: &[String],
) -> Result<HostMemoryContext, CommandError> {
    admit(state, loupist_id)?;
    let controls = with_loupist_knowledge(state, loupist_id, |tx, _| {
        crate::knowledge::memory_controls::load(tx, loupist_id)
    })?;
    let mut excluded = set_aside.to_vec();
    excluded.extend(controls.paused.iter().cloned());
    let set_aside = &excluded;
    let followed = state
        .store
        .map_context()
        .map_err(crate::commands::map_store)?;
    let (modules, perspective, mut entries, anchored) =
        with_loupist_knowledge(state, loupist_id, |tx, blobs| {
            let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
            let modules = write.list_records(loupist_id)?;
            let mut entries = Vec::new();
            for module in &modules {
                if module.kind != RecordKind::World {
                    continue;
                }
                let revision = module
                    .revision_id
                    .as_deref()
                    .map(|id| write.get_revision(loupist_id, id))
                    .transpose()?;
                entries.push(HostMemoryEntry {
                    id: module.record_id.clone(),
                    title: module.title.clone(),
                    kind: module.kind.as_str().to_string(),
                    summary: module.title.clone(),
                    as_of: revision
                        .map(|revision| recorded_at_rfc3339(revision.created_at_ms))
                        .unwrap_or_default(),
                    lens_kind: None,
                    sections: 0,
                    map_names: Vec::new(),
                });
            }
            // Index order: pages anchored to a followed name, then most recently
            // filed. The index excludes principles (their own block) and the
            // perspective (delivered whole).
            let followed_nodes = followed_node_ids(&write, loupist_id, &followed)?;
            let mut pages = write.lens_index(loupist_id)?;
            pages.retain(|page| {
                !set_aside.contains(&page.record_id)
                    && !matches!(
                        page.kind,
                        crate::knowledge::lens_page::LensKind::Principle
                            | crate::knowledge::lens_page::LensKind::Perspective
                    )
            });
            pages.sort_by(|a, b| {
                let followed_a = a.node_ids.iter().any(|id| followed_nodes.contains(id));
                let followed_b = b.node_ids.iter().any(|id| followed_nodes.contains(id));
                followed_b
                    .cmp(&followed_a)
                    .then(b.updated_at_ms.cmp(&a.updated_at_ms))
                    .then(a.record_id.cmp(&b.record_id))
            });
            let anchored = anchored_pages(&write, loupist_id, message, &pages)?;
            for page in pages {
                entries.push(HostMemoryEntry {
                    id: page.record_id.clone(),
                    title: page.title.clone(),
                    kind: "lens".into(),
                    summary: page.summary.chars().take(120).collect(),
                    as_of: recorded_at_rfc3339(page.updated_at_ms),
                    lens_kind: Some(page.kind),
                    sections: page.section_count,
                    map_names: page.map_names.clone(),
                });
            }
            Ok((
                modules,
                write.active_perspective(loupist_id)?,
                entries,
                anchored,
            ))
        })?;
    let mut lines = modules
        .iter()
        .filter_map(|module| {
            module
                .revision_id
                .as_ref()
                .map(|revision| format!("{} {}", module.record_id, revision))
        })
        .collect::<Vec<_>>();
    lines.sort();
    let revision = if lines.is_empty() {
        "none".to_string()
    } else {
        crate::util::sha256(lines.join("\n").as_bytes())
    };
    let perspective = perspective.filter(|id| !set_aside.contains(id));
    let relevant = relevant_memory(state, loupist_id, message, &entries, &anchored)?;
    let lens_total = entries.iter().filter(|entry| entry.kind == "lens").count();
    let mut kept = entries
        .iter()
        .filter(|entry| entry.kind == "lens")
        .take(LENS_INDEX_LINES)
        .cloned()
        .collect::<Vec<_>>();
    let world_total = entries.iter().filter(|entry| entry.kind == "world").count();
    let mut worlds = entries
        .iter()
        .filter(|entry| entry.kind == "world")
        .cloned()
        .collect::<Vec<_>>();
    worlds.sort_by(|left, right| right.as_of.cmp(&left.as_of).then(left.id.cmp(&right.id)));
    worlds.truncate(24);
    let world_shown = worlds.len();
    let lens_shown = kept.len();
    kept.extend(worlds);
    entries = kept;
    let (charter, charter_read_ref, charter_delivery) = if let Some(record_id) = perspective {
        let prepared = host_prepare_read(
            state,
            loupist_id,
            &HostReadRequest {
                as_of: None,
                scope: "lens".into(),
                record_id,
                revision_id: None,
                span_id: None,
                search: String::new(),
                center_claim_id: None,
                center_node_id: None,
                kinds: Vec::new(),
                relations: Vec::new(),
                binding_roles: Vec::new(),
                statuses: Vec::new(),
                node_limit: None,
                edge_limit: None,
                byte_limit: None,
                lens_status: "considered".into(),
            },
        );
        match prepared {
            Ok((document, staged)) if document.len() <= 32 * 1024 => {
                (Some(document), Some(staged.read_ref.clone()), Some(staged))
            }
            Ok((_, staged)) => {
                // Never claim a full read when only an automatic preview fits the prompt.
                (Some(serde_json::json!({
                    "readRequired": true, "recordId": staged.record_id,
                    "revisionId": staged.revision_id, "title": staged.title,
                    "notice": "The perspective exceeds the automatic context budget. Call lens_read before applying or revising it; no read permission was issued."
                }).to_string()), None, None)
            }
            Err(error) => return Err(error),
        }
    } else {
        (None, None, None)
    };
    let mut all_principles = with_loupist_knowledge(state, loupist_id, |tx, _| {
        crate::knowledge::principles::principle_list(tx, loupist_id, false)
    })?;
    all_principles.retain(|principle| !set_aside.contains(&principle.record_id));
    let (principles_block, principles) =
        crate::knowledge::principles::principles_context(&all_principles);
    // Text from shared packs arrives inside its own delimiters. This per-turn
    // context pauses commands only for an item accepted despite a scan finding.
    let shared = crate::sharing::delivery::Shared::load(state, loupist_id);
    let (principles_block, mut shared_pauses) =
        shared.wrap_principles(principles_block, &principles);
    let charter_record = charter
        .as_ref()
        .and(charter_delivery.as_ref())
        .map(|staged| staged.record_id.clone());
    let charter = charter.map(|body| {
        let (body, pauses) = shared.wrap_charter(charter_record.as_deref(), body);
        shared_pauses |= pauses;
        body
    });
    for entry in entries.iter_mut().filter(|entry| entry.kind == "lens") {
        if let Some(title) = shared.wrap_lens_field(&entry.id, "title", &entry.title) {
            entry.title = title;
            shared_pauses |= shared.flagged(&entry.id);
        }
        if let Some(summary) = shared.wrap_lens_field(&entry.id, "summary", &entry.summary) {
            entry.summary = summary;
        }
    }
    let mut relevant = relevant;
    for pointer in &mut relevant {
        let Some(record_id) = pointer.read_args["recordId"].as_str().map(str::to_owned) else {
            continue;
        };
        if let Some(title) = shared.wrap_lens_field(&record_id, "title", &pointer.title) {
            pointer.title = title;
            shared_pauses |= shared.flagged(&record_id);
        }
        if let Some(snippet) = shared.wrap_lens_field(&record_id, "snippet", &pointer.snippet) {
            pointer.snippet = snippet;
        }
    }
    let revision = crate::util::sha256(format!("{revision}\n{principles_block}").as_bytes());
    let principle_receipts = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        principles
            .iter()
            .map(|p| {
                Ok(crate::domain::LensReceipt {
                    record_id: p.record_id.clone(),
                    revision_id: p.revision_id.clone(),
                    revision_digest: write
                        .get_revision(loupist_id, &p.revision_id)?
                        .revision_digest,
                    kind: "principle".into(),
                    title: p.statement.clone(),
                    delivery: "context".into(),
                })
            })
            .collect::<Result<Vec<_>, KnowledgeError>>()
    })?;
    Ok(HostMemoryContext {
        principle_receipts,
        principles,
        principles_block,
        revision,
        charter,
        charter_read_ref,
        charter_delivery,
        entries,
        relevant,
        lens_shown,
        lens_total,
        world_shown,
        world_total,
        left_out: super::as_of::LeftOut::default(),
        shared_pauses,
    })
}

/// World node ids of what the user follows, through the World's names and symbols.
fn followed_node_ids(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    followed: &crate::store::user_context::MapContext,
) -> Result<std::collections::BTreeSet<String>, KnowledgeError> {
    let items: Vec<_> = followed
        .holdings
        .iter()
        .chain(&followed.watchlist)
        .cloned()
        .collect();
    if items.is_empty() {
        return Ok(Default::default());
    }
    let Some(world) = write
        .list_records(loupist_id)?
        .into_iter()
        .find(|module| module.kind == RecordKind::World)
    else {
        return Ok(Default::default());
    };
    let Some(revision_id) = world.revision_id else {
        return Ok(Default::default());
    };
    let body: crate::knowledge::WorldSnapshotBody =
        serde_json::from_value(write.get_revision(loupist_id, &revision_id)?.payload)
            .map_err(|_| KnowledgeError::Invalid("world payload is invalid"))?;
    Ok(crate::knowledge::match_items(&items, &body.nodes).0)
}

/// Pages anchored to World nodes whose name or alias the request names.
fn anchored_pages(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    message: &str,
    pages: &[crate::knowledge::IndexEntry],
) -> Result<Vec<String>, KnowledgeError> {
    let folded = format!(" {} ", crate::knowledge::fold_lens_title(message));
    if folded.trim().is_empty() || pages.iter().all(|page| page.node_ids.is_empty()) {
        return Ok(Vec::new());
    }
    let Some(world) = write
        .list_records(loupist_id)?
        .into_iter()
        .find(|module| module.kind == RecordKind::World)
    else {
        return Ok(Vec::new());
    };
    let Some(revision_id) = world.revision_id else {
        return Ok(Vec::new());
    };
    let body: crate::knowledge::WorldSnapshotBody =
        serde_json::from_value(write.get_revision(loupist_id, &revision_id)?.payload)
            .map_err(|_| KnowledgeError::Invalid("world payload is invalid"))?;
    let named: std::collections::BTreeSet<&str> = body
        .nodes
        .iter()
        .filter(|node| {
            std::iter::once(&node.name)
                .chain(&node.aliases)
                .any(|name| {
                    let name = crate::knowledge::fold_lens_title(name);
                    name.chars().count() >= 2 && folded.contains(&format!(" {name} "))
                })
        })
        .map(|node| node.id.as_str())
        .collect();
    Ok(pages
        .iter()
        .filter(|page| page.node_ids.iter().any(|id| named.contains(id.as_str())))
        .map(|page| page.record_id.clone())
        .take(LENS_POINTERS)
        .collect())
}

// The research search accepts at most 200 UTF-8 bytes (Lens accepts 200
// characters). Reserve the final clause when a paste precedes it, and drop
// any token longer than 32 bytes so one hash cannot fill the budget.
fn memory_query_terms(message: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "a",
        "an",
        "and",
        "are",
        "as",
        "at",
        "be",
        "been",
        "but",
        "by",
        "can",
        "could",
        "did",
        "do",
        "does",
        "for",
        "from",
        "had",
        "has",
        "have",
        "how",
        "i",
        "if",
        "in",
        "is",
        "it",
        "its",
        "me",
        "my",
        "of",
        "on",
        "or",
        "our",
        "should",
        "so",
        "that",
        "the",
        "their",
        "them",
        "there",
        "these",
        "they",
        "this",
        "to",
        "us",
        "was",
        "we",
        "were",
        "what",
        "when",
        "which",
        "who",
        "why",
        "will",
        "with",
        "would",
        "you",
        "your",
        "please",
        "about",
        "using",
        "use",
        "apply",
        "check",
        "explain",
        "analyze",
        "analyse",
        "tell",
        "compare",
        "remember",
        "previous",
        "earlier",
        "saved",
        "message",
        "question",
        "method",
        "note",
        "notes",
        "following",
        "consider",
        "analysis",
        "관련",
        "대한",
        "분석해줘",
        "알려줘",
        "그리고",
        "해주세요",
    ];
    let tokens = memory_tokens(message);
    let mut selected: Vec<(usize, String)> = Vec::new();
    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut bytes = 0usize;
    let keep = |term: &str| term.chars().count() >= 2 && term.len() <= 32 && !STOP.contains(&term);
    if let Some(clause_at) = final_clause_start(message) {
        for (offset, _, term) in tokens.iter().filter(|(offset, _, _)| *offset >= clause_at) {
            if keep(term) {
                take_memory_term(&mut selected, &mut seen, &mut bytes, *offset, term);
            }
        }
    }
    let mut counts = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
    for (offset, index, term) in &tokens {
        if keep(term) {
            counts.entry(term.clone()).or_insert((0, *index, *offset)).0 += 1;
        }
    }
    let mut terms: Vec<_> = counts.into_iter().collect();
    // Unique, longer terms are usually more distinctive than repeated preamble.
    // Cap length's influence so a pasted URL cannot outrank a shorter subject.
    terms.sort_by(|(a, (ac, ai, _)), (b, (bc, bi, _))| {
        ac.cmp(bc)
            .then_with(|| b.chars().count().min(12).cmp(&a.chars().count().min(12)))
            .then_with(|| ai.cmp(bi))
    });
    for (term, (_, _, offset)) in terms {
        take_memory_term(&mut selected, &mut seen, &mut bytes, offset, &term);
    }
    // Retain phrase order for the search engine's audited financial aliases.
    selected.sort_by_key(|(offset, _)| *offset);
    selected.into_iter().map(|(_, term)| term).collect()
}

fn memory_tokens(message: &str) -> Vec<(usize, usize, String)> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut index = 0usize;
    for (offset, ch) in message.char_indices() {
        if ch.is_alphanumeric() {
            if start.is_none() {
                start = Some(offset);
            }
            continue;
        }
        if let Some(from) = start.take() {
            tokens.push((from, index, message[from..offset].to_lowercase()));
            index += 1;
        }
    }
    if let Some(from) = start {
        tokens.push((from, index, message[from..].to_lowercase()));
    }
    tokens
}

/// Byte offset of the final clause, only when an earlier clause contains text.
fn final_clause_start(message: &str) -> Option<usize> {
    let mut previous = None;
    let mut last = None;
    for (offset, ch) in message.char_indices() {
        if matches!(ch, '.' | '?' | '!' | '\n' | '。' | '？') {
            previous = last;
            last = Some(offset + ch.len_utf8());
        }
    }
    let end = last?;
    let start = if message[end..].trim().is_empty() {
        previous?
    } else {
        end
    };
    message[..start]
        .chars()
        .any(|ch| ch.is_alphanumeric())
        .then_some(start)
}

fn take_memory_term(
    selected: &mut Vec<(usize, String)>,
    seen: &mut std::collections::BTreeSet<String>,
    bytes: &mut usize,
    offset: usize,
    term: &str,
) {
    if selected.len() >= 16 || seen.contains(term) {
        return;
    }
    let size = term.len() + usize::from(*bytes > 0);
    if *bytes + size > 200 {
        return;
    }
    *bytes += size;
    seen.insert(term.to_string());
    selected.push((offset, term.to_string()));
}

fn memory_term_matches(text: &str, term: &str) -> bool {
    text.match_indices(term).any(|(offset, _)| {
        !term.is_ascii()
            || (!text[..offset]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric())
                && !text[offset + term.len()..]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphanumeric()))
    })
}

fn memory_snippet(text: &str, terms: &[String]) -> String {
    let line = text
        .lines()
        .max_by_key(|line| {
            let lower = line.to_lowercase();
            terms
                .iter()
                .filter(|term| memory_term_matches(&lower, term))
                .count()
        })
        .unwrap_or(text);
    lens_catalog_preview(line)
}

fn relevant_memory(
    state: &AppState,
    loupist_id: &str,
    message: &str,
    entries: &[HostMemoryEntry],
    anchored: &[String],
) -> Result<Vec<crate::agent_harness::RelevantMemoryPointer>, CommandError> {
    use crate::agent_harness::RelevantMemoryPointer;
    let terms = memory_query_terms(message);
    let mut relevant = Vec::new();
    let pointer = |entry: &HostMemoryEntry, snippet: String, revision: serde_json::Value| {
        RelevantMemoryPointer {
            later_notes: Vec::new(),
            kind: "Lens".into(),
            title: entry.title.clone(),
            as_of: entry.as_of.clone(),
            snippet,
            tool: "lens_read".into(),
            read_args: serde_json::json!({"recordId": entry.id, "revisionId": revision}),
        }
    };
    for id in anchored {
        if let Some(entry) = entries.iter().find(|entry| &entry.id == id) {
            relevant.push(pointer(
                entry,
                lens_catalog_preview(&entry.summary),
                serde_json::Value::Null,
            ));
        }
    }
    if terms.is_empty() {
        return Ok(relevant);
    }
    let hits = host_search(state, loupist_id, &terms.join(" "), "lens")?;
    for hit in hits.as_array().into_iter().flatten() {
        if relevant.len() >= LENS_POINTERS {
            break;
        }
        let Some(id) = hit["recordId"].as_str() else {
            continue;
        };
        // The untruncated index already excludes archived and paused pages.
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.kind == "lens" && entry.id == id)
        else {
            continue;
        };
        let matched = hit["matched_terms"].as_array().map_or(0, Vec::len);
        // A whole financial concept can be one alias group (e.g. cash flow).
        let strong_match = matched >= terms.len().min(2)
            || (matched > 0 && hit["score"].as_u64().unwrap_or_default() >= 600);
        if !strong_match
            || relevant
                .iter()
                .any(|pointer: &RelevantMemoryPointer| pointer.read_args["recordId"] == id)
        {
            continue;
        }
        relevant.push(pointer(
            entry,
            memory_snippet(hit["snippet"].as_str().unwrap_or(&entry.summary), &terms),
            hit["revisionId"].clone(),
        ));
    }
    // Working notes only. Journals and legacy rows are not in this page, so
    // they cannot take one of the three note slots. Require overlap again
    // before promoting a note into the turn's small discovery list.
    let page = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).search_working_notes(
            loupist_id,
            &terms.join(" "),
            Some(20),
        )
    })?;
    let mut notes = Vec::new();
    for hit in &page.hits {
        if hit.journal_id.is_some() {
            continue;
        }
        let id = hit.research_id.as_str();
        // Point lookup supplies the date and full text. It does not stage a delivery.
        let Ok(note) = read_working_context(state, loupist_id, id) else {
            continue;
        };
        let title = note.title.to_lowercase();
        let body = note.markdown.to_lowercase();
        let overlap = terms
            .iter()
            .filter(|term| memory_term_matches(&title, term) || memory_term_matches(&body, term))
            .count();
        if overlap < terms.len().min(2) {
            continue;
        }
        let score = overlap
            + terms
                .iter()
                .filter(|term| memory_term_matches(&title, term))
                .count();
        notes.push((
            score,
            note.recorded_at_ms,
            RelevantMemoryPointer {
                later_notes: note.later_notes,
                kind: "WorkingNote".into(),
                title: note.title,
                as_of: recorded_at_rfc3339(note.recorded_at_ms),
                snippet: memory_snippet(&note.markdown, &terms),
                tool: "conversation_memory".into(),
                read_args: serde_json::json!({"action": "read", "noteId": id}),
            },
        ));
    }
    notes.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.title.cmp(&b.2.title))
    });
    relevant.extend(notes.into_iter().take(3).map(|(_, _, pointer)| pointer));
    Ok(relevant)
}

pub(in crate::commands) fn host_module_count(state: &AppState, loupist_id: &str) -> usize {
    if admit(state, loupist_id).is_err() {
        return 0;
    }
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs)
            .list_records(loupist_id)
            .map(|modules| modules.len())
    })
    .unwrap_or(0)
}

pub(in crate::commands) fn host_search(
    state: &AppState,
    loupist_id: &str,
    query: &str,
    scope: &str,
) -> Result<serde_json::Value, CommandError> {
    host_search_with_options(
        state,
        loupist_id,
        query,
        scope,
        &crate::knowledge::SearchOptions {
            include_metadata: true,
            semantic_assets: state.graph.semantic_assets(),
            ..crate::knowledge::SearchOptions::default()
        },
    )
}

pub(in crate::commands) fn host_search_with_options(
    state: &AppState,
    loupist_id: &str,
    query: &str,
    scope: &str,
    options: &crate::knowledge::SearchOptions,
) -> Result<serde_json::Value, CommandError> {
    admit(state, loupist_id)?;
    let query = query.trim();
    if query.chars().count() > 200 {
        return Err(CommandError::invalid("knowledge query is too long"));
    }
    if !matches!(scope, "all" | "source" | "lens" | "world" | "research") {
        return Err(CommandError::invalid("unknown search scope"));
    }
    let structured_filters = !options.companies.is_empty()
        || !options.periods.is_empty()
        || !options.source_types.is_empty();
    let browse =
        (query.is_empty() || query == "*") && !structured_filters && options.record_ids.is_empty();
    let mut hits = if browse {
        browse_owned_records(state, loupist_id, scope)?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if scope != "research" && !browse {
        let (prepared, cache_key) = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
            let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
            let prepared = write.prepare_search(
                loupist_id,
                scope,
                if query.is_empty() { "*" } else { query },
                crate::knowledge::SEARCH_WINDOW_LIMIT,
                options,
            )?;
            // The private cache is scoped to both this Loupist and its actual folder.
            // Registering a copied folder cannot alias another library's entries.
            let cache_key =
                crate::util::sha256(format!("{}\n{loupist_id}", blobs.root().display()).as_bytes());
            Ok((prepared, cache_key))
        })?;
        let cache_path = state
            .artifacts
            .app_data_dir
            .join("search-embeddings")
            .join(format!("{cache_key}.sqlite3"));
        let mut ranked = prepared.rank(Some(&cache_path));
        admit(state, loupist_id)?;
        let (current, world_payloads, awaiting_world) = with_loupist_knowledge(
            state,
            loupist_id,
            |tx, blobs| {
                let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
                let current = write.visible_search_revisions(loupist_id)?;
                let awaiting_world = write.sources_awaiting_world(loupist_id)?;
                ranked.retain_current(&current);
                let mut world_payloads = std::collections::BTreeMap::<String, String>::new();
                for document in ranked
                    .documents
                    .iter()
                    .filter(|document| document.kind == "world")
                {
                    if !world_payloads.contains_key(&document.revision_id) {
                        let payload = tx.query_row(
                        "SELECT payload_json FROM knowledge_record_revisions WHERE revision_id = ?1",
                        [&document.revision_id], |row| row.get(0),
                    )?;
                        world_payloads.insert(document.revision_id.clone(), payload);
                    }
                }
                Ok((current, world_payloads, awaiting_world))
            },
        )?;
        ranked.prune_cache(&current);
        let mut world_claims = std::collections::BTreeMap::new();
        for (revision_id, payload) in world_payloads {
            let body: crate::knowledge::WorldSnapshotBody = serde_json::from_str(&payload)
                .map_err(|error| CommandError::internal(error.to_string()))?;
            world_claims.insert(
                revision_id,
                body.claims
                    .into_iter()
                    .map(|claim| claim.id)
                    .collect::<std::collections::BTreeSet<_>>(),
            );
        }
        for document in ranked.documents {
            let mut hit = serde_json::json!({
                "scope": document.kind,
                "recordId": document.record_id,
                "revisionId": document.revision_id,
                "spanId": document.span_id,
                "objectId": document.object_id,
                "title": document.title,
                "coverage": document.coverage,
                "snippet": document.snippet,
                "score": document.score,
                "matched_terms": document.matched_terms,
                "retrieval": document.retrieval,
            });
            // A Source still in the World queue: its relationships are not on
            // the Map yet, so an empty World result says nothing about them.
            if document.kind == "source" && awaiting_world.contains(&document.record_id) {
                hit["mapPending"] = serde_json::json!(true);
            }
            // Claim identifiers are open tokens, not a reliable type prefix.
            // Bind the discovered object to this exact canonical revision so
            // following its read hint cannot land on an unrelated browse page.
            if world_claims
                .get(&document.revision_id)
                .is_some_and(|claims| claims.contains(&document.object_id))
            {
                hit["readArgs"] = serde_json::json!({
                    "scope": "world",
                    "recordId": document.record_id,
                    "revisionId": document.revision_id,
                    "centerClaimId": document.object_id,
                });
            }
            hits.push(hit);
        }
    }
    if (scope == "all" || scope == "research")
        && !structured_filters
        && options.record_ids.is_empty()
    {
        let page = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
            crate::knowledge::KnowledgeWrite::bind(tx, blobs).research_history(
                loupist_id,
                if browse { "" } else { query },
                None,
                Some(20),
            )
        })?;
        for hit in page.hits {
            hits.push(serde_json::json!({
                "scope": "research",
                "journalId": hit.journal_id,
                "researchId": hit.research_id,
                "recordedAtMs": hit.recorded_at_ms,
                "laterNotes": hit.later_notes,
                "title": hit.conclusion.lines().next().unwrap_or("Saved work").chars().take(160).collect::<String>(),
                "snippet": hit.conclusion.chars().take(500).collect::<String>(),
                "coverage": "snippet",
            }));
        }
    }
    if !options.record_ids.is_empty() {
        hits.retain(|hit| {
            hit.get("recordId")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| options.record_ids.iter().any(|module| module == id))
        });
    }
    // Reference Sources of a removed shared pack stay hidden.
    let removed = with_loupist_knowledge(state, loupist_id, |tx, _| {
        let mut excluded = crate::sharing::store::removed_sources(tx, loupist_id)?;
        let controls = crate::knowledge::memory_controls::load(tx, loupist_id)?;
        excluded.extend(controls.removed);
        excluded.extend(controls.paused);
        Ok(excluded)
    })?;
    if !removed.is_empty() {
        hits.retain(|hit| {
            hit.get("recordId")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|id| !removed.contains(id))
        });
    }
    Ok(serde_json::Value::Array(hits))
}

pub(in crate::commands) fn browse_owned_records(
    state: &AppState,
    loupist_id: &str,
    scope: &str,
) -> Result<serde_json::Value, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let mut hits = Vec::new();
        for record in write
            .list_records(loupist_id)?
            .into_iter()
            .filter(|record| scope == "all" || record.kind.as_str() == scope)
        {
            if record.kind == RecordKind::Source {
                if let Some(revision_id) = &record.revision_id {
                    let revision = write.get_revision(loupist_id, revision_id)?;
                    let payload: SourceRecordPayload = serde_json::from_value(revision.payload)?;
                    let source = payload.source;
                    for capture in source.captures {
                        for representation in capture.representations {
                            for span in representation.spans {
                                hits.push(serde_json::json!({"scope":"source", "recordId":record.record_id,
                                        "revisionId":revision_id,"objectId":source.source_id,"spanId":span.span_id,
                                        "title":source.title,"coverage":"metadata_only","snippet":source.title}));
                                if hits.len() >= crate::knowledge::SEARCH_WINDOW_LIMIT {
                                    return Ok(serde_json::Value::Array(hits));
                                }
                            }
                        }
                    }
                }
            } else {
                hits.push(serde_json::json!({"scope":record.kind.as_str(), "recordId":record.record_id,
                    "title":record.title,"revisionId":record.revision_id,"coverage":"metadata_only","snippet":record.title}));
            }
            if hits.len() >= crate::knowledge::SEARCH_WINDOW_LIMIT {
                break;
            }
        }
        Ok(serde_json::Value::Array(hits))
    })
}

#[cfg(test)]
pub(in crate::commands) fn host_prepare_journal_read(
    state: &AppState,
    loupist_id: &str,
    journal_id: &str,
) -> Result<(String, crate::commands::tool_executor::StagedKnowledgeRead), CommandError> {
    host_prepare_journal_read_as_of(state, loupist_id, journal_id, None)
        .map(|read| read.expect("no cutoff hides an entry"))
}

/// The exact Journal read; under a cutoff an entry written later is `None`
/// and an outcome observed later is left out (the check reads as pending).
pub(in crate::commands) fn host_prepare_journal_read_as_of(
    state: &AppState,
    loupist_id: &str,
    journal_id: &str,
    cutoff: Option<super::as_of::Cutoff>,
) -> Result<Option<(String, crate::commands::tool_executor::StagedKnowledgeRead)>, CommandError> {
    let record = journal_read(
        state,
        KnowledgeJournalReadRequest {
            loupist_id: loupist_id.to_string(),
            journal_id: journal_id.to_string(),
        },
    )?;
    let record = match cutoff {
        None => record,
        Some(cutoff) => match super::as_of::journal_known(record, cutoff) {
            Some(record) => record,
            None => return Ok(None),
        },
    };
    // Keep the citation preview recognizable, then deliver the complete prediction
    // before a potentially long thesis/rationale consumes the read budget.
    let mut full = format!(
        "Journal: {}\n\nCheck: {}\n\n",
        record.judgment.chars().take(160).collect::<String>(),
        serde_json::to_string(&record.check).map_err(|e| CommandError::internal(e.to_string()))?
    );
    if let Some(outcome) = &record.outcome {
        full.push_str(&format!(
            "Outcome: {}\n\n",
            serde_json::to_string(outcome).map_err(|e| CommandError::internal(e.to_string()))?
        ));
    }
    full.push_str(&format!("{}\n\n{}", record.judgment, record.reasoning));
    let mut text = full.clone();
    let mut end = 8_000.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    let coverage = if text == full {
        "included_full"
    } else {
        "included_excerpt"
    };
    let digest = crate::util::sha256(text.as_bytes());
    let read_ref = format!("rdg_{}", uuid::Uuid::new_v4().simple());
    let title = record
        .judgment
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Private Journal")
        .chars()
        .take(120)
        .collect();
    Ok(Some((
        text.clone(),
        crate::commands::tool_executor::StagedKnowledgeRead {
            read_ref,
            scope: "research".into(),
            record_id: String::new(),
            span_id: None,
            revision_id: String::new(),
            object_id: record.journal_id.clone(),
            title,
            payload: crate::commands::tool_executor::AckedPayload::Journal {
                journal_id: record.journal_id,
                delivered_text: text,
                digest,
                coverage: coverage.into(),
            },
        },
    )))
}

pub(in crate::commands) fn host_prepare_read(
    state: &AppState,
    loupist_id: &str,
    request: &HostReadRequest,
) -> Result<(String, crate::commands::tool_executor::StagedKnowledgeRead), CommandError> {
    admit(state, loupist_id)?;
    let read_ref = format!("rdg_{}", uuid::Uuid::new_v4().simple());
    let (text, revision_id, span_id, object_id, title, payload) =
        with_loupist_knowledge(state, loupist_id, |tx, blobs| {
            let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
            let module = write.owned_record(loupist_id, &request.record_id)?;
            let revision_id = match request
                .revision_id
                .as_deref()
                .filter(|revision_id| !revision_id.is_empty())
            {
                Some(revision_id) => revision_id.to_string(),
                None => module
                    .revision_id
                    .clone()
                    .ok_or(crate::knowledge::KnowledgeError::NotFound)?,
            };
            let revision = write.get_revision(loupist_id, &revision_id)?;
            if revision.record_id != request.record_id {
                return Err(crate::knowledge::KnowledgeError::Invalid(
                    "revision belongs to another record",
                ));
            }
            match request.scope.as_str() {
                "source"
                    if crate::sharing::store::removed_sources(tx, loupist_id)?
                        .contains(&request.record_id) =>
                {
                    Err(crate::knowledge::KnowledgeError::NotFound)
                }
                "source" => capture_source_read(&write, request, &revision),
                "world" => {
                    let (text, revision_id, span_id, payload) =
                        capture_world_read(&write, loupist_id, request, &revision)?;
                    Ok((
                        text,
                        revision_id,
                        span_id,
                        module.record_id,
                        module.title,
                        payload,
                    ))
                }
                "lens" => {
                    crate::knowledge::memory_controls::require_delivery(
                        tx,
                        loupist_id,
                        &request.record_id,
                    )?;
                    let (text, revision_id, span_id, payload) =
                        capture_lens_read(&write, loupist_id, request, &revision)?;
                    Ok((
                        text,
                        revision_id,
                        span_id,
                        module.record_id,
                        module.title,
                        payload,
                    ))
                }
                _ => Err(crate::knowledge::KnowledgeError::Invalid(
                    "knowledge scope is invalid",
                )),
            }
        })?;
    Ok((
        text,
        crate::commands::tool_executor::StagedKnowledgeRead {
            read_ref,
            scope: request.scope.clone(),
            record_id: request.record_id.clone(),
            span_id,
            revision_id,
            object_id,
            title,
            payload,
        },
    ))
}

pub(in crate::commands) fn capture_source_read(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    request: &HostReadRequest,
    revision: &crate::knowledge::RecordRevision,
) -> crate::knowledge::KnowledgeResult<(
    String,
    String,
    Option<String>,
    String,
    String,
    crate::commands::tool_executor::AckedPayload,
)> {
    let span_id = request
        .span_id
        .clone()
        .ok_or(crate::knowledge::KnowledgeError::Invalid(
            "source read needs a span",
        ))?;
    let Some(overlay) = source_overlay(&revision.payload, &span_id) else {
        return Err(crate::knowledge::KnowledgeError::NotFound);
    };
    let (bytes, media_type) =
        write.read_revision_span_bytes(&request.record_id, &span_id, &overlay.selected_digest)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok((
        text,
        revision.revision_id.clone(),
        Some(span_id),
        overlay.source_id,
        overlay.title,
        crate::commands::tool_executor::AckedPayload::Source {
            revision_digest: revision.revision_digest.clone(),
            capture_id: overlay.capture_id,
            representation_id: overlay.representation_id,
            bytes,
            media_type,
            selected_digest: overlay.selected_digest,
            coverage: overlay.coverage,
            attribution: overlay.attribution,
            publication_time: overlay.publication_time,
        },
    ))
}

struct SourceOverlay {
    source_id: String,
    title: String,
    capture_id: String,
    representation_id: String,
    selected_digest: String,
    coverage: String,
    attribution: String,
    publication_time: Option<crate::knowledge::TimePoint>,
}

/// Who authored the retained material behind one Source span, as its
/// revision records it, for the untrusted-text tier
/// (`tool_executor/returning.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::commands) struct SpanAuthorship {
    pub authorship: crate::knowledge::Authorship,
    /// Saved from a custom connection's result.
    pub custom_connection: bool,
    pub title: String,
    pub origin_locator: Option<String>,
    /// The dataset a saved provider payload names (`/dataset_ref/dataset_id`),
    /// read only for publisher JSON payloads.
    pub dataset_id: Option<String>,
}

impl SpanAuthorship {
    pub(in crate::commands) fn untrusted(&self, state: &AppState, loupist_id: &str) -> bool {
        match self.authorship {
            crate::knowledge::Authorship::User | crate::knowledge::Authorship::Analysis => {
                self.custom_connection
            }
            crate::knowledge::Authorship::ImportedContributor => false, // shared delivery owns this class
            crate::knowledge::Authorship::Publisher | crate::knowledge::Authorship::Tool => {
                self.custom_connection
                    || !self
                        .dataset_id
                        .as_deref()
                        .and_then(|id| {
                            state
                                .store
                                .dataset_current_metadata(loupist_id, id)
                                .ok()
                                .flatten()
                        })
                        .is_some_and(|metadata| {
                            crate::custom_connections::taint::retained_trusted(&metadata)
                        })
            }
        }
    }
}

/// Resolve the evidence behind a delivered World view or lexical claim hit.
/// The canonical revision, not extraction text or a Source URL, owns this binding.
pub(in crate::commands) fn returning_world_sources(
    state: &AppState,
    loupist_id: &str,
    result: &serde_json::Value,
    view: Option<&serde_json::Value>,
) -> Result<(Vec<crate::knowledge::WorldSourceTarget>, bool), CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let record_id = result["recordId"]
            .as_str()
            .ok_or(KnowledgeError::Invalid("World record missing"))?;
        let revision_id = result["revisionId"]
            .as_str()
            .ok_or(KnowledgeError::Invalid("World revision missing"))?;
        let revision = write.get_revision(loupist_id, revision_id)?;
        if revision.record_id != record_id {
            return Err(KnowledgeError::Unauthorized("World revision mismatch"));
        }
        let body: crate::knowledge::WorldSnapshotBody = serde_json::from_value(revision.payload)?;
        let view = view.map(|value| value.get("view").unwrap_or(value));
        let edges = view
            .and_then(|value| value.get("edges"))
            .and_then(serde_json::Value::as_array);
        let object_id = result.get("objectId").and_then(serde_json::Value::as_str);
        let spans = body
            .claims
            .iter()
            .filter(|claim| match edges {
                Some(edges) => edges.iter().any(|edge| {
                    edge.pointer("/claim/id")
                        // Indexed graph retrieval uses the canonical claim id
                        // as adapter_id rather than the native view's claim.id.
                        .or_else(|| edge.get("adapter_id"))
                        .and_then(serde_json::Value::as_str)
                        == Some(claim.id.as_str())
                }),
                None => object_id.is_none_or(|id| id == claim.id),
            })
            .flat_map(|claim| {
                claim
                    .evidence
                    .iter()
                    .map(|evidence| evidence.span_id.clone())
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut targets = Vec::new();
        for chunk in spans.chunks(256) {
            targets.extend(
                write
                    .world_source_targets(loupist_id, record_id, revision_id, chunk)?
                    .into_values(),
            );
        }
        let unresolved = targets.len() < spans.len();
        Ok((targets, unresolved))
    })
}

/// The authorship of one Source span at a revision (the current one when
/// `revision_id` is None), or of every span when `span_id` is empty (a hit on
/// the Source's title). Empty when the revision holds no such span.
pub(in crate::commands) fn source_span_authorship(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
    revision_id: Option<&str>,
    span_id: &str,
) -> Result<Vec<SpanAuthorship>, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let revision_id = match revision_id.filter(|id| !id.is_empty()) {
            Some(id) => id.to_owned(),
            None => match write.owned_record(loupist_id, record_id)?.revision_id {
                Some(id) => id,
                None => return Ok(Vec::new()),
            },
        };
        let revision = write.get_revision(loupist_id, &revision_id)?;
        if revision.record_id != record_id {
            return Ok(Vec::new());
        }
        let Ok(parsed) = serde_json::from_value::<SourceRecordPayload>(revision.payload) else {
            return Ok(Vec::new());
        };
        let source = parsed.source;
        let mut found = Vec::new();
        for capture in &source.captures {
            let before = found.len();
            for representation in &capture.representations {
                let spans = representation
                    .spans
                    .iter()
                    .filter(|span| span_id.is_empty() || span.span_id == span_id);
                for span in spans {
                    let payload = capture.authorship == crate::knowledge::Authorship::Publisher
                        && representation.role
                            == crate::knowledge::RepresentationRole::ProviderPayload
                        && representation.media_type == "application/json";
                    let dataset_id = if payload {
                        let (bytes, _) = write.read_revision_span_bytes(
                            record_id,
                            &span.span_id,
                            &span.selected_digest,
                        )?;
                        serde_json::from_slice::<serde_json::Value>(&bytes)
                            .ok()
                            .and_then(|value| {
                                value
                                    .pointer("/dataset_ref/dataset_id")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_owned)
                            })
                    } else {
                        None
                    };
                    found.push(SpanAuthorship {
                        authorship: capture.authorship,
                        custom_connection: source.origin.is_some(),
                        title: source.title.clone(),
                        origin_locator: source.origin_locator.clone(),
                        dataset_id,
                    });
                }
            }
            // A metadata-only capture has no span; a title hit still names it.
            if span_id.is_empty() && found.len() == before {
                found.push(SpanAuthorship {
                    authorship: capture.authorship,
                    custom_connection: source.origin.is_some(),
                    title: source.title.clone(),
                    origin_locator: source.origin_locator.clone(),
                    dataset_id: None,
                });
            }
        }
        Ok(found)
    })
}

fn source_overlay(payload: &serde_json::Value, span_id: &str) -> Option<SourceOverlay> {
    let parsed = serde_json::from_value::<SourceRecordPayload>(payload.clone()).ok()?;
    let source = parsed.source;
    let source_id = source.source_id.clone();
    let title = source.title.clone();
    let origin = source
        .origin_locator
        .as_deref()
        .filter(|origin| !origin.trim().is_empty())
        .unwrap_or(&source.provider_identity);
    let attribution = if origin == source.provider_identity {
        format!("{} — {}", source.title, origin)
    } else {
        format!(
            "{} — {} ({})",
            source.title, origin, source.provider_identity
        )
    };
    for capture in source.captures {
        for representation in capture.representations {
            let Some(span) = representation
                .spans
                .into_iter()
                .find(|span| span.span_id == span_id)
            else {
                continue;
            };
            return Some(SourceOverlay {
                source_id,
                title,
                capture_id: capture.capture_id,
                representation_id: representation.representation_id,
                selected_digest: span.selected_digest,
                coverage: capture.coverage.as_str().to_string(),
                attribution,
                publication_time: capture.publication_time,
            });
        }
    }

    None
}

pub(in crate::commands) fn capture_world_read(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    request: &HostReadRequest,
    revision: &crate::knowledge::RecordRevision,
) -> crate::knowledge::KnowledgeResult<(
    String,
    String,
    Option<String>,
    crate::commands::tool_executor::AckedPayload,
)> {
    let binding_roles = request
        .binding_roles
        .iter()
        .map(|role| BindingRole::parse(role))
        .collect::<Result<Vec<_>, _>>()?;
    let statuses = request
        .statuses
        .iter()
        .map(|status| ClaimStatus::parse(status))
        .collect::<Result<Vec<_>, _>>()?;
    let mut query = WorldGraphQuery {
        loupist_id: loupist_id.to_string(),
        record_id: request.record_id.clone(),
        revision_id: revision.revision_id.clone(),
        search: request.search.clone(),
        center_claim_id: request.center_claim_id.clone(),
        center_node_id: request.center_node_id.clone(),
        kinds: request.kinds.clone(),
        relations: request.relations.clone(),
        binding_roles,
        statuses,
        publication_times: Vec::new(),
        published_by: request.as_of.map(|cutoff| cutoff.to_string()),
        cursor: None,
        node_limit: request.node_limit,
        edge_limit: request.edge_limit,
        byte_limit: request.byte_limit,
    };
    let (_, _, response_limit) = query.limits()?;
    let mut delivered = None;
    for _ in 0..16 {
        let view = world_view_with_sources(write, query.clone())?;
        let mut body = serde_json::to_value(&view.view)?;
        for (span_id, target) in &view.view.source_targets {
            body["sourceTargets"][span_id]["readArgs"] = serde_json::json!({
                "scope": "source",
                "recordId": target.record_id,
                "revisionId": target.revision_id,
                "spanId": target.span_id,
            });
        }
        let encoded = serde_json::to_string(&body)?;
        let response_bytes = serde_json::to_vec(&serde_json::json!({
            "status": "delivered_for_this_turn", "scope": "world",
            "recordId": request.record_id, "revisionId": revision.revision_id,
            "readRef": "rdg_00000000000000000000000000000000", "text": encoded,
        }))?
        .len() as u32;
        if response_bytes <= response_limit {
            delivered = Some(encoded);
            break;
        }
        let budget = query.byte_limit.unwrap_or(response_limit);
        query.byte_limit = Some(
            budget
                .checked_sub(response_bytes - response_limit)
                .filter(|limit| *limit > 0)
                .ok_or(KnowledgeError::ItemTooLarge)?,
        );
    }
    let view_json = delivered.ok_or(KnowledgeError::ItemTooLarge)?;
    let text = view_json.clone();
    let view_digest = crate::util::sha256(view_json.as_bytes());
    Ok((
        text,
        revision.revision_id.clone(),
        None,
        crate::commands::tool_executor::AckedPayload::World {
            revision_digest: revision.revision_digest.clone(),
            view_json,
            view_digest,
        },
    ))
}

pub(in crate::commands) fn capture_lens_read(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    request: &HostReadRequest,
    revision: &crate::knowledge::RecordRevision,
) -> crate::knowledge::KnowledgeResult<(
    String,
    String,
    Option<String>,
    crate::commands::tool_executor::AckedPayload,
)> {
    if revision.kind != RecordKind::Lens {
        return Err(crate::knowledge::KnowledgeError::Invalid(
            "record is not a Lens",
        ));
    }
    if !matches!(
        request.lens_status.as_str(),
        "considered" | "applied" | "adapted" | "rejected"
    ) {
        return Err(crate::knowledge::KnowledgeError::Invalid(
            "lens use status is invalid",
        ));
    }
    let mut page = write.read_page(loupist_id, &request.record_id)?;
    if page.revision_id != revision.revision_id {
        // Only the head is readable: history lives in the log, not in revisions.
        return Err(crate::knowledge::KnowledgeError::NotFound);
    }
    if let Some(cutoff) = request.as_of.map(super::as_of::Cutoff) {
        // The page itself passed `as_of::lens_visible`; what surrounds it keeps to the cutoff too.
        page.history
            .retain(|entry| cutoff.recorded(entry.created_at_ms));
        let mut backlinks = Vec::with_capacity(page.backlinks.len());
        for link in std::mem::take(&mut page.backlinks) {
            if super::as_of::lens_visible(write, loupist_id, &link.record_id, cutoff)? {
                backlinks.push(link);
            }
        }
        page.backlinks = backlinks;
    }
    let document_json = page.delivery_text()?;
    let body_digest = crate::util::sha256(document_json.as_bytes());
    let text = document_json.clone();
    // Record host metadata beside the exact delivered page. Its body digest
    // remains the digest of the model-facing document.
    let mut recorded: serde_json::Value = serde_json::from_str(&document_json)?;
    recorded["lensKind"] = serde_json::json!(page.kind);
    if page.kind == crate::knowledge::lens_page::LensKind::Principle {
        recorded["receiptTitle"] = serde_json::json!(page.body.summary());
    }
    let document_json = serde_json::to_string(&recorded)?;
    Ok((
        text,
        revision.revision_id.clone(),
        None,
        crate::commands::tool_executor::AckedPayload::Lens {
            revision_digest: revision.revision_digest.clone(),
            document_json,
            body_digest,
            status: request.lens_status.clone(),
        },
    ))
}

pub(in crate::commands) fn host_world_job(
    state: &AppState,
    loupist_id: &str,
    job_id: &str,
) -> Result<serde_json::Value, CommandError> {
    admit(state, loupist_id)?;
    let job = with_loupist_knowledge(state, loupist_id, |tx, _| {
        crate::knowledge::get_job(tx, loupist_id, job_id)
    })?;
    serde_json::to_value(job).map_err(|error| CommandError::internal(error.to_string()))
}

#[cfg(test)]
mod tests;
