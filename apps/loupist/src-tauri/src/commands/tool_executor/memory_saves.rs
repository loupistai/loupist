//! Memory saves inside the tool executor: retained Sources, Journal entries
//! and research notes, staged with the turn and committed after it.
mod journal_input;
mod receipts;

use super::arguments::{bounded_text, optional_bounded_text, optional_unique_strings};
use super::pasted::PASTED_TEXT_LOCATOR;
use super::*;
use journal_input::{journal_argument_error, parse_research_journal};

use crate::commands::assets::wire::AssetRow;
use crate::knowledge::assets::{self, PageRecord};
use crate::store::user_context::matching;

fn resolve_thesis_target(
    store: &crate::store::SqliteStore,
    loupist_id: &str,
    pages: &[PageRecord],
    follows: &[AssetRow],
    query: &str,
) -> Result<(String, Option<AssetRow>), BrokerError> {
    let mut all = store.assets_list(loupist_id, false).map_err(store_error)?;
    let mut seen: BTreeSet<String> = all.iter().map(|asset| asset.asset_id.clone()).collect();
    all.extend(
        follows
            .iter()
            .filter(|asset| seen.insert(asset.asset_id.clone()))
            .cloned(),
    );
    all.extend(
        pages
            .iter()
            .filter(|page| seen.insert(page.asset_id.clone()))
            .map(|page| AssetRow {
                asset_id: page.asset_id.clone(),
                name: page.name.clone(),
                reference: None,
                followed_at_ms: None,
                created_at_ms: page.created_at_ms,
            }),
    );
    match matching(&all, query).as_slice() {
        [asset] => {
            let needs_page = !pages.iter().any(|page| page.asset_id == asset.asset_id);
            Ok((asset.asset_id.clone(), needs_page.then(|| (*asset).clone())))
        }
        [] => Err(BrokerError::invalid("thesis.asset", "No asset matches.")),
        candidates => Err(BrokerError::invalid(
            "thesis.asset",
            format!("Several assets match: {}", json!(candidates)),
        )),
    }
}

type StagedThesis = (assets::JournalThesis, Option<AssetRow>);

fn same_thesis_target(a: &StagedThesis, b: &StagedThesis) -> bool {
    a.0.asset_id == b.0.asset_id
}

pub(in crate::commands) fn finish_setup_theses(
    store: &crate::store::SqliteStore,
    loupist_id: &str,
    research: &mut StagedResearch,
) -> crate::store::StoreResult<bool> {
    let mut skipped = false;
    for entry in [&mut research.journal, &mut research.thesis_pair] {
        let Some((thesis, Some(_))) = entry.as_ref().and_then(|journal| journal.thesis.as_ref())
        else {
            continue;
        };
        if store.asset_by_id(loupist_id, &thesis.asset_id)?.is_none() {
            *entry = None;
            skipped = true;
        }
    }
    if research.journal.is_none() {
        research.journal = research.thesis_pair.take();
    }
    Ok(skipped)
}

impl AppToolExecutor {
    async fn validate_note_references(
        &self,
        policy: &ToolPolicy,
        research: &StagedResearch,
    ) -> Result<(), BrokerError> {
        let receipts = self.acknowledged_receipts().await;
        for (scope, field, refs) in [
            ("source", "sourceReads", &research.source_reads),
            ("world", "worldReads", &research.world_reads),
            ("lens", "lensReads", &research.lens_reads),
        ]
        .into_iter()
        .chain(research.journal.iter().flat_map(|journal| {
            [
                ("source", "journal.sourceReads", &journal.source_reads),
                ("world", "journal.worldReads", &journal.world_reads),
                ("lens", "journal.lensReads", &journal.lens_reads),
            ]
        })) {
            receipts.validate(scope, field, refs)?;
        }
        let cutoff = parse_as_of_date(policy.as_of.as_deref())?;
        let review_day = |at_ms: i64| {
            crate::knowledge::journal::journal_date(at_ms)
                .map(|day| cutoff.map_or(day, |cutoff| day.min(cutoff)))
                .map_err(journal_argument_error)
        };
        let tool_reads = self.capture.staged_reads.lock().await;
        let context_reads = self.capture.context_reads.lock().await;
        let reads: Vec<_> = tool_reads.iter().chain(context_reads.iter()).collect();
        if let Some(journal) = &research.journal {
            for earlier in &journal.earlier {
                let (_, reviewed) = crate::commands::knowledge::earlier_research(
                    reads.iter().copied(),
                    earlier,
                    |journal_id| {
                        crate::knowledge::KnowledgeStore::for_loupist(
                            self.state.store.clone(),
                            &policy.loupist_id,
                        )?
                        .get_journal(&policy.loupist_id, journal_id)
                    },
                )
                .map_err(|error| {
                    BrokerError::invalid("journal.earlier.readRef", command_message(error))
                })?;
                if let Some(outcome) = journal
                    .outcome
                    .as_ref()
                    .filter(|_| earlier.relation == "review")
                {
                    crate::knowledge::journal::validate_review_check(
                        reviewed.check.as_ref(),
                        outcome,
                        review_day(journal.at_ms)?,
                    )
                    .map_err(journal_argument_error)?;
                    if reviewed.outcome.is_some() || reviewed.archived {
                        return Err(BrokerError::invalid(
                            "journal.outcome",
                            "journal entry is archived or already reviewed",
                        ));
                    }
                    let by_date = reviewed
                        .check
                        .as_ref()
                        .and_then(|check| check.by_date.clone())
                        .expect("validated dated check");
                    let by_date = &by_date;
                    let today = review_day(journal.at_ms)?.to_string();
                    let read_evidence = reads.iter().any(|read| {
                        journal.source_reads.contains(&read.read_ref)
                            && match &read.payload {
                                AckedPayload::Source {
                                    publication_time, ..
                                } => crate::knowledge::journal::dated_evidence(
                                    publication_time.as_ref(),
                                    by_date,
                                    &today,
                                ),
                                _ => false,
                            }
                    });
                    let saved_evidence =
                        self.capture
                            .staged_sources
                            .lock()
                            .await
                            .iter()
                            .any(|source| {
                                journal.source_reads.contains(&source.request_ref)
                                    && crate::knowledge::journal::dated_evidence(
                                        source.publication_time.as_ref(),
                                        by_date,
                                        &today,
                                    )
                            });
                    if !read_evidence && !saved_evidence {
                        return Err(BrokerError::invalid("sourceReads", "review requires acknowledged evidence dated on or after byDate and no later than today"));
                    }
                    crate::knowledge::KnowledgeStore::for_loupist(
                        self.state.store.clone(),
                        &policy.loupist_id,
                    )
                    .and_then(|store| {
                        store.read(|read| {
                            read.require_unreviewed_journal(
                                &policy.loupist_id,
                                &reviewed.journal_id,
                            )
                        })
                    })
                    .map_err(journal_argument_error)?;
                }
            }
        }
        drop(reads);
        drop(context_reads);
        drop(tool_reads);
        let notes = self.capture.note_reads.lock().await;
        for id in &research.previous_note_ids {
            if !notes.contains(id) {
                return Err(BrokerError::Execution(
                    "Previous note must be read with conversation_memory in this turn".into(),
                ));
            }
            let previous = crate::commands::knowledge::read_working_context(
                &self.state,
                &policy.loupist_id,
                id,
            )
            .map_err(command_error)?;
            if research.working_note.is_some() && previous.chat_id == policy.session_id {
                return Err(BrokerError::invalid(
                    "previousNoteIds",
                    "Previous notes must belong to another Chat. To update this Chat's working note, omit this Chat's notes from previousNoteIds and use the current note's id in workingNote.expectedResearchId.",
                ));
            }
        }
        Ok(())
    }

    pub(super) async fn stage_source_save(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates {
            return Err(BrokerError::ProposalDisabled);
        }
        let object = exact_object(
            &params,
            &["result_ref", "title", "coverage"],
            &["publicationTime"],
        )?;
        let result_ref = bounded_text(object.get("result_ref"), 1, 128)?;
        let title = bounded_text(object.get("title"), 1, 180)?;
        let coverage = object
            .get("coverage")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        if crate::knowledge::Coverage::parse(coverage).is_err() {
            return Err(BrokerError::Malformed);
        }
        let result = self.capture.run_result(&result_ref).await.ok_or_else(|| {
            BrokerError::Execution(
                "source result_ref must name a delivered result from this turn".into(),
            )
        })?;
        // Account and balance reads stay on this computer (D9).
        if result.producer.tool_name == "kis_read"
            && crate::privacy::account_fields::is_account_result(&result.payload)
        {
            return Err(BrokerError::Execution(
                crate::privacy::account_fields::NOT_SAVED.into(),
            ));
        }
        if !result.producer.origin.saveable_as_source() {
            return Err(BrokerError::Execution(
                "Only acquired originals can be saved as Sources; derived analysis and saved context belong in a note with their original evidence"
                    .into(),
            ));
        }
        if let Some(origin) = result.origin.as_deref() {
            crate::knowledge::reject_locator_userinfo(origin)
                .map_err(|_| BrokerError::Malformed)?;
        }
        if result
            .payload
            .pointer("/model_delivery/coverage")
            .and_then(Value::as_str)
            == Some("bounded_sample")
        {
            return Err(BrokerError::Execution("Only a summary of this result was delivered. Read a narrower original result before saving its exact payload as a Source".into()));
        }
        let workspace_file = result.producer.origin == RunResultOrigin::WorkspaceFile;
        let title = if workspace_file {
            result.payload["path"]
                .as_str()
                .and_then(|path| self.captured_attachment_snapshot_title(path))
                .and_then(|title| self.capture.file_labels.stored_title(&title))
                .unwrap_or(title)
        } else {
            title
        };
        let provided = self.provided_origin(policy, &result)?;
        let bytes = if workspace_file || provided.is_some() {
            result
                .payload
                .get("content")
                .and_then(Value::as_str)
                .ok_or(BrokerError::Malformed)?
                .as_bytes()
                .to_vec()
        } else {
            serde_json::to_vec(&result.payload).map_err(|_| BrokerError::Malformed)?
        };
        let partial_web_delivery = result.producer.runtime_id == "native-web"
            && result.producer.tool_name == "web_fetch"
            && (result.payload.get("truncated").and_then(Value::as_bool) == Some(true)
                || result
                    .payload
                    .get("extraction_truncated")
                    .and_then(Value::as_bool)
                    == Some(true)
                || result
                    .payload
                    .get("next_offset")
                    .is_some_and(|value| !value.is_null())
                || result
                    .payload
                    .get("content_offset")
                    .and_then(Value::as_u64)
                    .is_some_and(|offset| offset > 0));
        if coverage == "included_full" && partial_web_delivery {
            return Err(BrokerError::Execution(
                "This web result contains only part of the document. Save the delivered slice as included_excerpt or upstream_truncated; unseen pages cannot be retained".into(),
            ));
        }
        let truncated = result.producer.origin == RunResultOrigin::Discovery
            || result.warnings.iter().any(|warning| {
                warning.to_ascii_lowercase().contains("truncat")
                    || warning.to_ascii_lowercase().contains("snippet")
            });
        if coverage == "included_full" && truncated {
            return Err(BrokerError::Execution(
                "a snippet or truncated delivery cannot be stored as a full original".into(),
            ));
        }
        let material_role = if coverage == "included_excerpt" {
            "excerpt"
        } else if workspace_file || provided.is_some() {
            "original"
        } else {
            "provider_payload"
        };
        let document_id = match (&provided, result.origin.as_deref()) {
            (Some(provided), _) => format!(
                "paste.{}",
                &crate::util::sha256(provided.message_id.as_bytes())[..32]
            ),
            (None, Some(origin)) if !origin.is_empty() => {
                format!("loc.{}", &crate::util::sha256(origin.as_bytes())[..32])
            }
            (None, _) => format!(
                "tool.{}",
                &crate::util::sha256(
                    format!(
                        "{}\0{}\0{}\0{}",
                        result.producer.runtime_id,
                        result.producer.tool_name,
                        result.producer.provider.as_deref().unwrap_or(""),
                        result.request_digest
                    )
                    .as_bytes()
                )[..32]
            ),
        };
        let save_ref = format!("save_{}", uuid::Uuid::new_v4().simple());
        let custom_origin = self
            .capture
            .turn_taint
            .origin()
            .filter(|origin| origin.connection_id == result.producer.runtime_id);
        let staged = StagedSource {
            authorship: if provided.is_some() {
                // The user handed it over; a publisher may have written it,
                // but nothing here can vouch for that.
                crate::knowledge::Authorship::User
            } else if workspace_file && result.payload["trust"] == "third_party_text" {
                // An app export is not reclassified as the user's own words.
                // Tool authorship also keeps it private for sharing purposes.
                crate::knowledge::Authorship::Tool
            } else {
                saved_authorship(&result.producer, custom_origin.is_some())
            },
            custom_origin,
            source_class: if provided.is_some() {
                crate::knowledge::SourceClass::UserProvided
            } else if workspace_file {
                crate::knowledge::SourceClass::UserFile
            } else {
                crate::knowledge::SourceClass::ToolResult
            },
            title,
            bytes,
            media_type: if workspace_file || provided.is_some() {
                result
                    .payload
                    .get("media_type")
                    .and_then(Value::as_str)
                    .ok_or(BrokerError::Malformed)?
                    .into()
            } else {
                "application/json".into()
            },
            origin: match &provided {
                Some(provided) => provided
                    .stated_url
                    .clone()
                    .unwrap_or_else(|| PASTED_TEXT_LOCATOR.into()),
                None => result
                    .origin
                    .clone()
                    .unwrap_or_else(|| format!("tool:{}", result.producer.tool_name)),
            },
            coverage: coverage.to_string(),
            chat_id: policy.session_id.clone(),
            request_ref: save_ref.clone(),
            result_ref: result_ref.clone(),
            publication_time: object
                .get("publicationTime")
                .filter(|value| !value.is_null())
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()
                .map_err(|_| BrokerError::Malformed)?,
            provider_identity: if provided.is_some() {
                "user".into()
            } else {
                let identity = result.producer.provider.clone().unwrap_or_else(|| {
                    format!(
                        "{}:{}",
                        result.producer.runtime_id, result.producer.tool_name
                    )
                });
                if identity.len() <= 200 {
                    identity
                } else {
                    format!(
                        "provider-sha256:{}",
                        crate::util::sha256(identity.as_bytes())
                    )
                }
            },
            document_id,
            material_role: material_role.into(),
            provided,
        };
        // Under an as-of date only material published by then is saved; reads
        // under the cutoff already carry no later rows or pages. Undated
        // material is refused: an as-of read could never use it.
        if let Some(cutoff) =
            crate::commands::knowledge::as_of::Cutoff::parse(policy.as_of.as_deref())
        {
            match staged
                .publication_time
                .as_ref()
                .filter(|time| time.value.is_some())
            {
                Some(time) if !cutoff.published(Some(time)) => {
                    return Err(super::as_of_turn::later_than(cutoff, "source"));
                }
                Some(_) => {}
                None => {
                    return Err(BrokerError::invalid(
                        "publicationTime",
                        format!(
                            "as_of_undated: under the as-of date {} only material with a publication date on or before it can be saved; pass the original's publicationTime",
                            cutoff.day()
                        ),
                    ));
                }
            }
        }
        // The same reading the commit makes from the stored bytes
        // (`knowledge::price_data`): a price-data Source never enters World.
        let price_data = match staged.source_class {
            crate::knowledge::SourceClass::UserFile => std::str::from_utf8(&staged.bytes)
                .is_ok_and(|text| {
                    crate::knowledge::price_data::price_file(&staged.media_type, text)
                }),
            crate::knowledge::SourceClass::ToolResult => {
                let metadata = result
                    .payload
                    .pointer("/dataset_ref/dataset_id")
                    .and_then(Value::as_str)
                    .and_then(|dataset_id| {
                        self.state
                            .store
                            .dataset_current_metadata(&self.loupist_id, dataset_id)
                            .ok()
                            .flatten()
                    });
                crate::knowledge::price_data::price_payload(&result.payload, metadata.as_ref())
            }
            _ => false,
        };
        self.capture
            .pending_deliveries
            .lock()
            .await
            .entry(delivery_id.to_owned())
            .or_default()
            .staged_sources
            .push(staged);
        Ok(json!({
            "status": "staged",
            "saveRef": save_ref,
            "durable": false,
            "graph": if price_data { "not_added_price_data" } else { "after_turn_commit" },
            "result_ref": result_ref
        }))
    }

    pub(super) async fn stage_journal(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
        review: bool,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        if !policy.propose_memory_updates {
            return Err(BrokerError::ProposalDisabled);
        }
        if !review
            && (self
                .capture
                .principle_user_message_id
                .as_deref()
                .is_none_or(str::is_empty)
                || self.capture.bound_chat_id.as_deref() != Some(policy.session_id.as_str()))
        {
            return Err(BrokerError::invalid(
                "journal_save",
                "requires the current turn's user message; save only the judgment that user stated",
            ));
        }
        let (mut journal, thesis) = if review {
            let object = exact_object(
                &params,
                &["readRef", "outcome", "sourceReads"],
                &["worldReads"],
            )?;
            let read_ref = bounded_text(object.get("readRef"), 1, 128)?;
            let read = self
                .capture
                .staged_reads
                .lock()
                .await
                .iter()
                .chain(self.capture.context_reads.lock().await.iter())
                .find(|r| r.read_ref == read_ref)
                .cloned()
                .ok_or_else(|| {
                    BrokerError::invalid(
                        "readRef",
                        "exact-read the journal entry with conversation_memory this turn",
                    )
                })?;
            let AckedPayload::Journal {
                journal_id,
                delivered_text,
                digest,
                coverage,
            } = read.payload
            else {
                return Err(BrokerError::invalid("readRef", "requires a journal read"));
            };
            if delivered_text.trim().is_empty()
                || digest != crate::util::sha256(delivered_text.as_bytes())
                || !matches!(coverage.as_str(), "included_full" | "included_excerpt")
            {
                return Err(BrokerError::invalid(
                    "readRef",
                    "journal read digest or coverage is invalid",
                ));
            }
            let record = crate::knowledge::KnowledgeStore::for_loupist(
                self.state.store.clone(),
                &policy.loupist_id,
            )
            .and_then(|store| store.get_journal(&policy.loupist_id, &journal_id))
            .map_err(journal_argument_error)?;
            let cutoff = parse_as_of_date(policy.as_of.as_deref())?;
            if let Some(cutoff) = cutoff {
                let check_day = record
                    .check
                    .as_ref()
                    .and_then(|check| check.by_date.as_deref())
                    .and_then(crate::knowledge::last_day);
                if check_day.is_some_and(|day| cutoff < day) {
                    return Err(BrokerError::invalid(
                        "journal_review",
                        "as_of_before_check: A review needs evidence from on or after the check date",
                    ));
                }
            }
            let mut outcome =
                crate::knowledge::journal::parse_outcome(object.get("outcome").expect("required"))
                    .map_err(journal_argument_error)?;
            // Only the host records the cutoff a review ran under.
            outcome.reviewed_as_of = cutoff.map(|cutoff| cutoff.to_string());
            (
                StagedResearchJournal {
                    thesis: None,
                    at_ms: now_ms(),
                    judgment: record.judgment,
                    reasoning: record.reasoning,
                    entities: record.entities,
                    check: record.check,
                    outcome: Some(outcome),
                    earlier: vec![StagedEarlierJournal {
                        read_ref,
                        relation: "review".into(),
                    }],
                    source_reads: optional_unique_strings(object.get("sourceReads"), 32, 128)?,
                    world_reads: optional_unique_strings(object.get("worldReads"), 8, 128)?,
                    lens_reads: Vec::new(),
                },
                None,
            )
        } else {
            parse_research_journal(&params, now_ms())?
        };
        // Shape validation alone cannot authorize World links. Use the same
        // complete-World check as the canonical writer before reserving a save
        // slot, so an unknown name/id can be repaired in this reply.
        let pages = if thesis.is_some() || !journal.entities.is_empty() {
            let mut connection = self
                .state
                .store
                .lock_loupist(&policy.loupist_id)
                .map_err(store_error)?;
            let tx = connection
                .transaction()
                .map_err(|error| knowledge_error(error.into()))?;
            crate::knowledge::journal::validate_entities(
                &tx,
                &policy.loupist_id,
                &journal.entities,
            )
            .map_err(journal_argument_error)?;
            if thesis.is_some() {
                crate::knowledge::assets::list_pages(&tx, &policy.loupist_id)
                    .map_err(knowledge_error)?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        if let Some((query, role)) = thesis {
            let follows = self
                .capture
                .staged_setup
                .lock()
                .await
                .as_ref()
                .map_or_else(Vec::new, |setup| setup.follow.clone());
            let (asset_id, page) = resolve_thesis_target(
                &self.state.store,
                &self.loupist_id,
                &pages,
                &follows,
                &query,
            )?;
            journal.thesis = Some((
                crate::knowledge::assets::JournalThesis { asset_id, role },
                page,
            ));
        }
        if !review {
            crate::knowledge::journal::validate_draft_fields(
                &journal.judgment,
                &journal.reasoning,
                &journal.entities,
                journal.check.as_ref(),
                journal.thesis.as_ref().map(|(thesis, _)| thesis.role),
            )
            .map_err(journal_argument_error)?;
        }
        let research = StagedResearch {
            thesis_pair: None,
            journal: Some(journal),
            working_note: None,
            observations: String::new(),
            interpretation: String::new(),
            assumptions: String::new(),
            evaluation: String::new(),
            source_reads: Vec::new(),
            world_reads: Vec::new(),
            lens_reads: Vec::new(),
            previous_note_ids: Vec::new(),
        };
        self.validate_note_references(policy, &research).await?;
        let journal = research.journal.as_ref().expect("journal parsed");
        let mut result = json!({"status":"staged", "durable":false, "kind":"journal"});
        let mut reservation = self.capture.journal_delivery.lock().await;
        let paired = !reservation.is_empty();
        if paired
            && (review
                || reservation.len() != 1
                || !reservation[0]
                    .1
                    .thesis
                    .as_ref()
                    .zip(journal.thesis.as_ref())
                    .is_some_and(|(first_target, second_target)| {
                        let (first, _) = first_target;
                        let (second, _) = second_target;
                        reservation[0].1.outcome.is_none()
                            && journal.outcome.is_none()
                            && same_thesis_target(first_target, second_target)
                            && first.role != second.role
                            && (if first.role == crate::knowledge::assets::ThesisRole::Line {
                                &reservation[0].1
                            } else {
                                journal
                            })
                            .check
                            .is_some()
                    }))
        {
            return Err(BrokerError::invalid("journal", "one Journal entry per reply, plus one more only when the two are a reason and a line for the same followed asset. This one was not saved; tell the user which part was not recorded."));
        }
        if let Some(target @ (thesis, _)) = &journal.thesis {
            use crate::knowledge::assets::ThesisRole;
            match thesis.role {
                ThesisRole::Reason => {
                    result["saved_as"] = json!("reason");
                    result["next"] = json!("Saved as a reason; a reason is never tested against figures. If the user also named a level that would change their mind, save it now as a line: journal_save with thesis role line and a check.");
                }
                ThesisRole::Line => {
                    let check = journal.check.as_ref().expect("validated line check");
                    let when = match check.by_date.as_deref() {
                        Some(date) => format!("It comes due for review on {date}."),
                        None => "It has no date, so it never comes due for review.".to_owned(),
                    };
                    let mut next = format!("Saved as a line on this asset's page: {} {} {}{}. {} It is tested whenever a turn reads this metric from a filing, data result or fetched page: record the reading with thesis_update lineReading, and a reading past the level marks the thesis Broken and posts a message in Bot. Tell the user the level saved, how it is tested and whether it has a date, since a bare confirmation does not say whether anything watches the level.", check.metric, check.direction, check.threshold, check.unit.as_deref().unwrap_or(""), when);
                    let staged_reason = reservation.iter().any(|(_, entry)| {
                        entry.thesis.as_ref().is_some_and(|other| {
                            same_thesis_target(other, target) && other.0.role == ThesisRole::Reason
                        })
                    });
                    let has_reason = if staged_reason {
                        true
                    } else {
                        let mut connection = self
                            .state
                            .store
                            .lock_loupist(&policy.loupist_id)
                            .map_err(store_error)?;
                        let tx = connection
                            .transaction()
                            .map_err(|error| knowledge_error(error.into()))?;
                        crate::knowledge::journal::list(&tx, &policy.loupist_id)
                            .map_err(knowledge_error)?
                            .iter()
                            .any(|entry| {
                                !entry.archived
                                    && entry.thesis.as_ref().is_some_and(|other| {
                                        other.asset_id == thesis.asset_id
                                            && other.role == ThesisRole::Reason
                                    })
                            })
                    };
                    if !has_reason {
                        next.push_str(" This asset has no saved reason yet and readings are refused until one exists: ask the user why they follow it.");
                    }
                    result["saved_as"] = json!("line");
                    result["next"] = json!(next);
                }
            }
        }
        reservation.push((delivery_id.to_owned(), journal.clone()));
        self.capture
            .stage(delivery_id, |pending| {
                if paired {
                    pending.thesis_pair = research.journal;
                } else {
                    pending.research = Some(research);
                }
            })
            .await;
        Ok(result)
    }

    pub(super) async fn stage_research(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates {
            return Err(BrokerError::ProposalDisabled);
        }
        let object = exact_object(
            &params,
            &["workingNote"],
            &["sourceReads", "worldReads", "lensReads", "previousNoteIds"],
        )?;
        let research = StagedResearch {
            thesis_pair: None,
            source_reads: optional_unique_strings(object.get("sourceReads"), 32, 128).map_err(
                |_| {
                    BrokerError::invalid(
                        "sourceReads",
                        "at most 32 unique nonempty strings, each at most 128 characters",
                    )
                },
            )?,
            world_reads: optional_unique_strings(object.get("worldReads"), 8, 128).map_err(
                |_| {
                    BrokerError::invalid(
                        "worldReads",
                        "at most 8 unique nonempty strings, each at most 128 characters",
                    )
                },
            )?,
            lens_reads: optional_unique_strings(object.get("lensReads"), 8, 128).map_err(|_| {
                BrokerError::invalid(
                    "lensReads",
                    "at most 8 unique nonempty strings, each at most 128 characters",
                )
            })?,
            previous_note_ids: optional_unique_strings(object.get("previousNoteIds"), 8, 128)
                .map_err(|_| {
                    BrokerError::invalid(
                        "previousNoteIds",
                        "at most 8 unique nonempty strings, each at most 128 characters",
                    )
                })?,
            working_note: object
                .get("workingNote")
                .filter(|value| !value.is_null())
                .map(|value| {
                    serde_json::from_value::<crate::knowledge::WorkingNoteDraft>(value.clone())
                        .map_err(|_| {
                            BrokerError::invalid(
                                "workingNote",
                                "send title, markdown, changeSummary, and expectedResearchId only",
                            )
                        })
                })
                .transpose()?,
            observations: String::new(),
            interpretation: String::new(),
            assumptions: String::new(),
            evaluation: String::new(),
            journal: None,
        };
        self.validate_note_references(policy, &research).await?;
        if research.working_note.is_none() {
            return Err(BrokerError::invalid("workingNote", "required"));
        }
        if let Some(note) = &research.working_note {
            crate::knowledge::validate_working_note(note)
                .map_err(|error| BrokerError::invalid("workingNote", error.to_string()))?;
            crate::commands::knowledge::validate_working_note_revision(
                &self.state,
                &policy.loupist_id,
                &policy.session_id,
                note.expected_research_id.as_deref(),
            )
            .map_err(|error| {
                if error.code == "conflict" {
                    BrokerError::invalid("workingNote.expectedResearchId", error.message)
                } else {
                    command_error(error)
                }
            })?;
        }
        if self
            .capture
            .staged_research
            .lock()
            .await
            .as_ref()
            .is_some_and(|r| r.working_note.is_some())
            || self
                .capture
                .pending_deliveries
                .lock()
                .await
                .values()
                .any(|pending| {
                    pending
                        .research
                        .as_ref()
                        .is_some_and(|r| r.working_note.is_some())
                })
        {
            return Err(BrokerError::Execution(
                "a Chat turn may save only one research record".into(),
            ));
        }
        self.capture
            .stage(delivery_id, |pending| pending.research = Some(research))
            .await;
        Ok(json!({
            "status": "staged",
            "durable": false,
            "journal": false
        }))
    }
}

/// Who authored a saved tool result, decided once when it is saved and kept
/// in the Source record. Workspace files, every Korea Investment read and
/// custom connections record the user: they read the user's own files,
/// accounts or private services. The bundled OpenBB market-data worker
/// records the publisher, and a native tool records the author its
/// `ToolMethod::producer` row declares (the publisher for fetched web pages,
/// DART filings and financials, KRX market data, World Bank series and
/// DefiLlama values). Anything else stays the tool's. Sharing exports only
/// publisher-authored Sources, so an unknown producer stays private.
pub(super) fn saved_authorship(
    producer: &RunResultProducer,
    custom_connection: bool,
) -> crate::knowledge::Authorship {
    use crate::knowledge::Authorship;
    // A provider name is no evidence of authorship: for `web_fetch` it is the
    // page's host, so a site whose name starts like a broker's is still a
    // publisher. Account reads are the user's through their producer row.
    if custom_connection
        || producer.origin == RunResultOrigin::WorkspaceFile
        || producer.runtime_id.starts_with("custom")
    {
        return Authorship::User;
    }
    if producer.runtime_id == "openbb" {
        return Authorship::Publisher;
    }
    ToolMethod::from_tool_name(&producer.tool_name)
        .and_then(ToolMethod::producer)
        .filter(|native| native.runtime == producer.runtime_id)
        .map_or(Authorship::Tool, |native| native.author)
}
