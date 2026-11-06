use super::*;
use crate::domain::{KnowledgeReference, MemoryUpdateChange};

/// What one completed turn retained: the answer's changes and the Sources
/// a same-turn `saveRef` now names.
#[derive(Default)]
pub(in crate::commands) struct SavedTurn {
    pub changes: Vec<MemoryUpdateChange>,
    pub retained: Vec<(String, crate::knowledge::RetainedSource)>,
}

/// The changes alone, for callers that never cite a same-turn save.
#[cfg(any(test, feature = "e2e"))]
#[allow(clippy::too_many_arguments)]
pub(in crate::commands) fn persist_saved_turn(
    tx: &rusqlite::Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    chat_id: &str,
    turn_id: &str,
    at_ms: i64,
    pins: &[crate::commands::ChatKnowledgePin],
    save_learning: bool,
    sources: &[crate::commands::tool_executor::StagedSource],
    reads: &[crate::commands::tool_executor::StagedKnowledgeRead],
    research: Option<&crate::commands::tool_executor::StagedResearch>,
) -> Result<Vec<MemoryUpdateChange>, CommandError> {
    persist_saved_turn_sources(
        tx,
        blobs,
        loupist_id,
        chat_id,
        turn_id,
        at_ms,
        pins,
        save_learning,
        sources,
        reads,
        research,
    )
    .map(|saved| saved.changes)
}

// This boundary keeps the host transaction and exact provenance inputs explicit.
#[allow(clippy::too_many_arguments)]
pub(in crate::commands) fn persist_saved_turn_sources(
    tx: &rusqlite::Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    chat_id: &str,
    turn_id: &str,
    at_ms: i64,
    pins: &[crate::commands::ChatKnowledgePin],
    save_learning: bool,
    sources: &[crate::commands::tool_executor::StagedSource],
    reads: &[crate::commands::tool_executor::StagedKnowledgeRead],
    research: Option<&crate::commands::tool_executor::StagedResearch>,
) -> Result<SavedTurn, CommandError> {
    for (index, pin) in pins.iter().enumerate() {
        let pin_json =
            serde_json::to_string(pin).map_err(|error| CommandError::invalid(error.to_string()))?;
        tx.execute(
            "INSERT INTO knowledge_turn_pins (loupist_id, chat_id, turn_id, pin_index, pin_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![loupist_id, chat_id, turn_id, index as i64, pin_json],
        )
        .map_err(|error| CommandError::internal(error.to_string()))?;
    }
    if !save_learning {
        return Ok(SavedTurn::default());
    }
    let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
    let retained =
        retain_staged_sources(&write, loupist_id, turn_id, sources).map_err(map_knowledge)?;
    let settings = crate::knowledge::load_settings(tx, loupist_id).map_err(map_knowledge)?;
    for (_, saved) in &retained {
        write
            .enqueue_source_world(loupist_id, saved, &settings, at_ms)
            .map_err(map_knowledge)?;
    }
    let mut changes = Vec::new();
    let mut reported_sources = std::collections::BTreeSet::new();
    for (request_ref, retained) in &retained {
        if !reported_sources.insert(retained.source_id.clone()) {
            continue;
        }
        let source = sources
            .iter()
            .find(|source| &source.request_ref == request_ref)
            .ok_or_else(|| CommandError::internal("retained Source lost its input"))?;
        let processing = write
            .source_processing(loupist_id, &retained.record_id)
            .map_err(map_knowledge)?;
        changes.push(MemoryUpdateChange {
            outcomes: Vec::new(),
            world_job_id: processing.job_id,
            world_status: Some(processing.status),
            record_id: retained.source_id.clone(),
            kind: "Source".into(),
            operation: "create".into(),
            title: source.title.clone(),
            lesson: "Retained the exact delivered material.".into(),
            basis: "Acknowledged Chat delivery".into(),
            future_use: "Read the retained Source in later research.".into(),
            knowledge: Some(KnowledgeReference {
                kind: "source".into(),
                record_id: retained.record_id.clone(),
                revision_id: retained.revision_id.clone(),
                object_id: retained.source_id.clone(),
                span_id: retained.span_id.clone(),
                receipt_id: None,
            }),
        });
    }
    let staged_journal = research.and_then(|item| item.journal.as_ref());
    if staged_journal.is_none() && research.is_none() {
        return Ok(SavedTurn { changes, retained });
    }
    let (
        conclusion,
        rationale,
        _uncertainty,
        _alternatives,
        mut source_cites,
        mut world_cites,
        mut lens_cites,
    ) = if let Some(staged) = staged_journal {
        (
            staged.judgment.clone(),
            staged.reasoning.clone(),
            String::new(),
            String::new(),
            staged.source_reads.clone(),
            staged.world_reads.clone(),
            staged.lens_reads.clone(),
        )
    } else {
        (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            research
                .map(|item| item.source_reads.clone())
                .unwrap_or_default(),
            research
                .map(|item| item.world_reads.clone())
                .unwrap_or_default(),
            research
                .map(|item| item.lens_reads.clone())
                .unwrap_or_default(),
        )
    };
    if let Some(note) = research {
        for (all, extra) in [
            (&mut source_cites, &note.source_reads),
            (&mut world_cites, &note.world_reads),
            (&mut lens_cites, &note.lens_reads),
        ] {
            for reference in extra {
                if !all.contains(reference) {
                    all.push(reference.clone());
                }
            }
        }
    }
    let save_journal = staged_journal.is_some();
    if save_journal && conclusion.is_empty() {
        return Err(CommandError::invalid(
            "a saved journal needs the user judgment from this turn",
        ));
    }
    let earlier_cites = staged_journal.map(|item| item.earlier.len()).unwrap_or(0);
    if save_journal
        && staged_journal.is_some_and(|journal| {
            journal.source_reads.is_empty()
                && journal.world_reads.is_empty()
                && journal.lens_reads.is_empty()
        })
        && earlier_cites == 0
    {
        return Err(CommandError::invalid(
            "a saved journal must cite an acknowledged read from this turn",
        ));
    }
    write
        .open_turn(crate::knowledge::OpenTurn {
            loupist_id: loupist_id.to_string(),
            chat_session_id: chat_id.to_string(),
            turn_id: turn_id.to_string(),
            at_ms,
        })
        .map_err(map_knowledge)?;
    let mut receipt_ids = Vec::new();
    for cite in &source_cites {
        let receipt_id = if let Some((request_ref, saved)) =
            retained.iter().find(|(request_ref, _)| request_ref == cite)
        {
            let staged = sources
                .iter()
                .find(|source| &source.request_ref == request_ref)
                .ok_or_else(|| CommandError::invalid("retained source was not staged"))?;
            let _ = request_ref;
            let publication = staged
                .publication_time
                .as_ref()
                .map(|time| serde_json::to_string(time).unwrap_or_default());
            write
                .record_acked_source(
                    loupist_id,
                    chat_id,
                    turn_id,
                    at_ms,
                    &saved.record_id,
                    saved.span_id.as_deref().ok_or_else(|| {
                        CommandError::invalid("retained source has no span to cite")
                    })?,
                    &saved.capture_id,
                    saved.representation_id.as_deref().unwrap_or(""),
                    saved.selected_digest.as_deref().unwrap_or(""),
                    &saved.revision_id,
                    &saved.revision_digest,
                    &staged.provider_identity,
                    &staged.coverage,
                    publication.as_deref(),
                    &staged.media_type,
                    &staged.bytes,
                )
                .map_err(map_knowledge)?
        } else if let Some(read) = reads
            .iter()
            .find(|read| read.scope == "source" && read.read_ref == *cite)
        {
            let crate::commands::tool_executor::AckedPayload::Source {
                revision_digest,
                capture_id,
                representation_id,
                bytes,
                media_type,
                selected_digest,
                coverage,
                attribution,
                publication_time,
            } = &read.payload
            else {
                return Err(CommandError::invalid(
                    "journal cited a source that was not delivered in this turn",
                ));
            };
            let publication = publication_time
                .as_ref()
                .map(|time| serde_json::to_string(time).unwrap_or_default());
            let span_id = read
                .span_id
                .as_deref()
                .ok_or_else(|| CommandError::invalid("acknowledged source has no span"))?;
            // The host captured these exact bytes before a concurrent edit.
            // Admission follows the owning record; old bodies are not retained.
            if write
                .owned_record(loupist_id, &read.record_id)
                .map_err(map_knowledge)?
                .kind
                != RecordKind::Source
            {
                return Err(CommandError::invalid("acknowledged record is not a Source"));
            }
            write
                .record_acked_source(
                    loupist_id,
                    chat_id,
                    turn_id,
                    at_ms,
                    &read.record_id,
                    span_id,
                    capture_id,
                    representation_id,
                    selected_digest,
                    &read.revision_id,
                    revision_digest,
                    attribution,
                    coverage,
                    publication.as_deref(),
                    media_type,
                    bytes,
                )
                .map_err(map_knowledge)?
        } else {
            return Err(CommandError::invalid(
                "journal cited a source that was not delivered in this turn",
            ));
        };
        receipt_ids.push(receipt_id);
    }
    let mut world_ids = Vec::new();
    for cite in &world_cites {
        let read = reads
            .iter()
            .find(|read| read.scope == "world" && read.read_ref == *cite)
            .ok_or_else(|| {
                CommandError::invalid("journal cited a world view that was not delivered")
            })?;
        let crate::commands::tool_executor::AckedPayload::World {
            revision_digest,
            view_json,
            view_digest,
        } = &read.payload
        else {
            return Err(CommandError::invalid(
                "journal cited a world view that was not delivered",
            ));
        };
        if write
            .owned_record(loupist_id, &read.record_id)
            .map_err(map_knowledge)?
            .kind
            != RecordKind::World
        {
            return Err(CommandError::invalid("acknowledged record is not World"));
        }
        let delivery = write
            .record_acked_world(
                loupist_id,
                turn_id,
                at_ms,
                &read.record_id,
                &read.revision_id,
                revision_digest,
                view_json,
                view_digest,
            )
            .map_err(map_knowledge)?;
        world_ids.push(delivery);
    }
    let mut lenses = Vec::new();
    for cite in &lens_cites {
        let read = reads
            .iter()
            .find(|read| read.scope == "lens" && read.read_ref == *cite)
            .ok_or_else(|| CommandError::invalid("journal cited a lens that was not delivered"))?;
        let crate::commands::tool_executor::AckedPayload::Lens {
            revision_digest,
            document_json,
            body_digest,
            status,
        } = &read.payload
        else {
            return Err(CommandError::invalid(
                "journal cited a lens that was not delivered",
            ));
        };
        if write
            .owned_record(loupist_id, &read.record_id)
            .map_err(map_knowledge)?
            .kind
            != RecordKind::Lens
        {
            return Err(CommandError::invalid("acknowledged record is not Lens"));
        }
        let delivery = write
            .record_acked_lens(
                loupist_id,
                turn_id,
                at_ms,
                &read.record_id,
                &read.revision_id,
                revision_digest,
                document_json,
                body_digest,
            )
            .map_err(map_knowledge)?;
        let status = match status.as_str() {
            "considered" => crate::knowledge::LensUseStatus::Considered,
            "applied" => crate::knowledge::LensUseStatus::Applied,
            "adapted" => crate::knowledge::LensUseStatus::Adapted,
            "rejected" => crate::knowledge::LensUseStatus::Rejected,
            _ => {
                return Err(CommandError::invalid("lens use status is invalid"));
            }
        };
        lenses.push(crate::knowledge::LensAcknowledgement {
            delivery_id: delivery,
            status,
        });
    }
    let Some(research) = research else {
        return Ok(SavedTurn { changes, retained });
    };
    let earlier = match staged_journal {
        Some(staged) => staged
            .earlier
            .iter()
            .map(|item| {
                earlier_research(reads, item, |id| write.get_journal(loupist_id, id))
                    .map(|(earlier, _)| earlier)
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    if let Some((_, identity)) = staged_journal.and_then(|journal| journal.thesis.as_ref()) {
        write
            .ensure_asset_page(loupist_id, identity, at_ms)
            .map_err(map_knowledge)?;
    }
    let saved = write
        .save_research(crate::knowledge::SaveResearch {
            note_inputs: Some(crate::knowledge::NoteInputDraft {
                source_receipt_ids: receipt_ids.clone(),
                world_delivery_ids: world_ids.clone(),
                lenses: lenses.clone(),
                previous_note_ids: research.previous_note_ids.clone(),
            }),
            loupist_id: loupist_id.to_string(),
            turn_id: turn_id.to_string(),
            at_ms: staged_journal.map_or(at_ms, |journal| journal.at_ms),
            observations: research.observations.clone(),
            interpretation: research.interpretation.clone(),
            assumptions: research.assumptions.clone(),
            evaluation: research.evaluation.clone(),
            working_note: research.working_note.clone(),
            journal: staged_journal.map(|staged| crate::knowledge::JournalDraft {
                check: staged.check.clone(),
                thesis: staged.thesis.as_ref().map(|(thesis, _)| thesis.clone()),
                outcome: staged.outcome.clone(),
                judgment: conclusion,
                reasoning: rationale,

                source_receipt_ids: receipt_ids
                    .into_iter()
                    .zip(&source_cites)
                    .filter(|(_, reference)| staged.source_reads.contains(reference))
                    .map(|(id, _)| id)
                    .collect(),
                world_delivery_ids: world_ids
                    .into_iter()
                    .zip(&world_cites)
                    .filter(|(_, reference)| staged.world_reads.contains(reference))
                    .map(|(id, _)| id)
                    .collect(),
                lenses: lenses
                    .into_iter()
                    .zip(&lens_cites)
                    .filter(|(_, reference)| staged.lens_reads.contains(reference))
                    .map(|(lens, _)| lens)
                    .collect(),
                earlier,

                entities: staged.entities.clone(),
            }),
        })
        .map_err(map_knowledge)?;
    if let Some(journal) = saved.journal {
        changes.push(MemoryUpdateChange {
            outcomes: Vec::new(),
            world_job_id: None,
            world_status: None,
            record_id: journal.journal_id.clone(),
            kind: "Journal".into(),
            operation: if journal.outcome.is_some() {
                "revise"
            } else {
                "create"
            }
            .into(),
            title: journal.judgment.clone(),
            lesson: if journal.outcome.is_some() {
                "Recorded the observed outcome and its dated evidence."
            } else {
                "Saved the user’s judgment and its exact inputs."
            }
            .into(),
            basis: "Acknowledged research submission".into(),
            future_use: "Revisit this journal entry and its check.".into(),
            knowledge: Some(KnowledgeReference {
                kind: "journal".into(),
                record_id: String::new(),
                revision_id: String::new(),
                object_id: journal.journal_id,
                span_id: None,
                receipt_id: None,
            }),
        });
    } else {
        changes.push(MemoryUpdateChange {
            outcomes: Vec::new(),
            world_job_id: None,
            world_status: None,
            record_id: saved.research_id,
            kind: "Research".into(),
            operation: "create".into(),
            title: research
                .working_note
                .as_ref()
                .map(|note| note.title.clone())
                .unwrap_or_else(|| "Saved work".into()),
            lesson: research
                .working_note
                .as_ref()
                .map(|note| note.change_summary.clone())
                .unwrap_or_else(|| "Saved useful work from this conversation.".into()),
            basis: "Acknowledged research submission".into(),
            future_use: "Resume this work with its goals, assumptions, and results.".into(),
            knowledge: None,
        });
    }
    Ok(SavedTurn { changes, retained })
}

pub(in crate::commands) fn earlier_research<'a>(
    reads: impl IntoIterator<Item = &'a crate::commands::tool_executor::StagedKnowledgeRead>,
    item: &crate::commands::tool_executor::StagedEarlierJournal,
    get_journal: impl FnOnce(&str) -> crate::knowledge::KnowledgeResult<crate::knowledge::JournalRecord>,
) -> Result<
    (
        crate::knowledge::EarlierResearch,
        crate::knowledge::JournalRecord,
    ),
    CommandError,
> {
    let relation = match item.relation.as_str() {
        "review" => crate::knowledge::EarlierRelation::Review,
        "supersedes" => crate::knowledge::EarlierRelation::Supersedes,
        "contradicts" => crate::knowledge::EarlierRelation::Contradicts,
        _ => return Err(CommandError::invalid("earlier journal relation is invalid")),
    };
    let read = reads
        .into_iter()
        .find(|read| read.scope == "research" && read.read_ref == item.read_ref)
        .ok_or_else(|| {
            CommandError::invalid("earlier journal must cite an acknowledged read from this turn")
        })?;
    let crate::commands::tool_executor::AckedPayload::Journal {
        journal_id,
        delivered_text,
        digest,
        coverage,
    } = &read.payload
    else {
        return Err(CommandError::invalid(
            "earlier journal read did not carry the delivered view",
        ));
    };
    if crate::util::sha256(delivered_text.as_bytes()) != *digest {
        return Err(CommandError::invalid(
            "earlier journal read digest does not match the delivered view",
        ));
    }
    let record = get_journal(journal_id).map_err(map_knowledge)?;
    Ok((
        crate::knowledge::EarlierResearch {
            research_id: record.research_id.clone(),
            journal_id: Some(journal_id.clone()),
            relation,
            delivered_text: delivered_text.clone(),
            delivered_digest: digest.clone(),
            coverage: coverage.clone(),
        },
        record,
    ))
}

pub(in crate::commands) fn retain_staged_sources(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    origin_message_id: &str,
    sources: &[crate::commands::tool_executor::StagedSource],
) -> crate::knowledge::KnowledgeResult<Vec<(String, crate::knowledge::RetainedSource)>> {
    let mut retained = Vec::new();
    for source in sources {
        let coverage = crate::knowledge::Coverage::parse(&source.coverage)?;
        let role = match source.material_role.as_str() {
            "excerpt" => crate::knowledge::RepresentationRole::Excerpt,
            "original" => crate::knowledge::RepresentationRole::Original,
            "provider_payload" => crate::knowledge::RepresentationRole::ProviderPayload,
            _ => {
                return Err(crate::knowledge::KnowledgeError::Invalid(
                    "representation role is invalid",
                ))
            }
        };
        if (coverage == Coverage::IncludedExcerpt
            && role != crate::knowledge::RepresentationRole::Excerpt)
            || (coverage == Coverage::IncludedFull
                && role == crate::knowledge::RepresentationRole::Excerpt)
        {
            return Err(crate::knowledge::KnowledgeError::Invalid(
                "excerpt coverage cannot be stored as a full original",
            ));
        }
        let context_note = match role {
            crate::knowledge::RepresentationRole::Excerpt => "Delivered excerpt",
            crate::knowledge::RepresentationRole::Original => "Exact delivered original",
            crate::knowledge::RepresentationRole::ProviderPayload => {
                "Exact delivered provider payload"
            }
            _ => "Exact delivered tool result",
        };
        let material = if coverage.retains_bytes() {
            Some(crate::knowledge::RetainMaterial {
                media_type: source.media_type.clone(),
                role,
                document_id: source.document_id.clone(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                bytes: source.bytes.clone(),
                selector: crate::knowledge::Selector::ByteRange {
                    start: 0,
                    end: source.bytes.len() as u64,
                },
                context_note: context_note.to_string(),
            })
        } else {
            None
        };
        let saved = write.retain_new_source_with_origin(
            crate::knowledge::RetainNewSource {
                origin: source.custom_origin.clone(),
                loupist_id: loupist_id.to_string(),
                at_ms: crate::commands::now_ms(),
                title: source.title.clone(),
                source_class: source.source_class,
                origin_locator: source.origin.clone(),
                provider_identity: source.provider_identity.clone(),
                authorship: source.authorship,
                coverage,
                origin_chat_id: Some(source.chat_id.clone()),
                request_ref: Some(source.result_ref.clone()),
                declared_digest: None,
                publication_time: source.publication_time.clone(),
                material,
            },
            origin_message_id,
        )?;
        retained.push((source.request_ref.clone(), saved));
    }
    Ok(retained)
}

/// Replace the just-written provisional Chat with finalized, canonical targets
/// before the surrounding Chat/Knowledge transaction commits.
pub(in crate::commands) fn write_chat_learning_report(
    tx: &rusqlite::Transaction<'_>,
    provisional: &crate::domain::ChatSession,
    turn_id: &str,
    save_learning: bool,
    changes: Vec<MemoryUpdateChange>,
    performance: Option<crate::domain::ChatPerformance>,
    // Run-log entries the commit itself produced (an asset read it could not save).
    trace_notes: Vec<crate::domain::ChatToolTraceEntry>,
) -> crate::store::StoreResult<crate::domain::ChatMessage> {
    let mut chat = provisional.clone();
    let message = chat
        .messages
        .iter_mut()
        .find(|message| message.id == turn_id)
        .ok_or(crate::store::StoreError::Conflict(
            "finalized Chat turn is missing",
        ))?;
    if !trace_notes.is_empty() {
        let trace = message.tool_trace.get_or_insert_with(Default::default);
        for note in trace_notes {
            trace.push(note);
        }
    }
    message.memory_update = save_learning.then(|| crate::domain::MemoryUpdateResult {
        status: if changes.is_empty() {
            crate::domain::MemoryUpdateStatus::NoChange
        } else {
            crate::domain::MemoryUpdateStatus::Applied
        },
        commit_id: (!changes.is_empty()).then(|| turn_id.to_string()),
        changes,
    });
    if message.host_composed {
        // Only receipts in this same atomic transaction can supply this answer.
        let changes = message
            .memory_update
            .as_ref()
            .map(|update| update.changes.as_slice())
            .unwrap_or(&[]);
        if changes.is_empty() {
            return Err(crate::store::StoreError::Invalid(
                "No committed receipts for an empty answer".into(),
            ));
        }
        message.content = changes
            .iter()
            .map(|change| {
                format!(
                    "Saved {}{}: {}.",
                    change.kind,
                    if change.operation == "proposed" {
                        " proposal"
                    } else {
                        ""
                    },
                    change.title,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    message.performance = performance;
    let completed = message.clone();
    chat.validate()
        .map_err(|error| crate::store::StoreError::Invalid(error.to_string()))?;
    let changed = tx.execute(
        "UPDATE chat_sessions SET data_json = ?1 WHERE id = ?2 AND loupist_id = ?3 AND data_json = ?4",
        rusqlite::params![
            serde_json::to_string(&chat)?,
            chat.id,
            chat.loupist_id,
            serde_json::to_string(provisional)?
        ],
    )?;
    if changed != 1 {
        return Err(crate::store::StoreError::Conflict(
            "finalized Chat compare-and-swap",
        ));
    }
    Ok(completed)
}
