mod journal_input;
mod lens_discovery;
mod receipts;

use super::*;
use journal_input::{journal_argument_error, parse_research_journal};
use receipts::ReceiptPurpose;

impl AppToolExecutor {
    pub(super) async fn lens_search(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let object = exact_object(&params, &["query"], &["offset"])?;
        let query = bounded_text(object.get("query"), 1, 200)?;
        let offset = search_offset(object.get("offset"))?;
        let mut hits = crate::commands::knowledge::host_search(
            &self.state,
            &policy.loupist_id,
            &query,
            "lens",
        )
        .map_err(|error| BrokerError::Execution(error.to_string()))?;
        self.as_of_hits(policy, &mut hits)?;
        discovery_page(hits, offset, false)
    }

    pub(super) async fn lens_read(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        exact_object(&params, &["recordId"], &["revisionId", "status"])?;
        params
            .as_object_mut()
            .ok_or(BrokerError::Malformed)?
            .insert("scope".into(), json!("lens"));
        let mut delivered = self.read_record(policy, params, delivery_id).await?;
        let record_id = delivered["recordId"]
            .as_str()
            .ok_or(BrokerError::Malformed)?
            .to_owned();
        let view = crate::commands::knowledge::lens_read(
            &self.state,
            crate::commands::knowledge::KnowledgeLensReadRequest {
                include_navigation: None,
                loupist_id: policy.loupist_id.clone(),
                record_id: record_id.clone(),
                revision_id: Some(
                    delivered["revisionId"]
                        .as_str()
                        .ok_or(BrokerError::Malformed)?
                        .to_string(),
                ),
            },
        )
        .map_err(|error| BrokerError::Execution(error.to_string()))?;
        // Discovery metadata must never enter the exact six-field document or its receipt.
        let navigation = serde_json::to_value(view.navigation)
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        delivered
            .as_object_mut()
            .ok_or(BrokerError::Malformed)?
            .extend(
                navigation
                    .as_object()
                    .ok_or(BrokerError::Malformed)?
                    .clone(),
            );
        delivered["lens_url"] = json!(format!("lens:{record_id}"));
        Ok(delivered)
    }

    pub(super) async fn world_job(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let object = exact_object(&params, &[], &["saveRef", "jobId"])?;
        let save_ref = optional_bounded_text(object.get("saveRef"), 1, 128)?;
        let job_id = optional_bounded_text(object.get("jobId"), 1, 128)?;
        if save_ref.is_some() && job_id.is_some() {
            return Err(BrokerError::Malformed);
        }
        if let Some(save_ref) = save_ref {
            if !self
                .capture
                .staged_sources
                .lock()
                .await
                .iter()
                .any(|source| source.request_ref == save_ref)
            {
                return Err(BrokerError::Execution(
                    "saveRef must name an acknowledged source_save in this turn".into(),
                ));
            }
            return Ok(
                json!({"saveRef":save_ref,"status":"waiting_for_turn_commit","durable":false,"retryThisTurn":false}),
            );
        }
        let summarize = |job: &Value| {
            json!({
                "jobId":job["jobId"], "status":job["status"], "createdAtMs":job["createdAtMs"],
                "sourceId":job.pointer("/document/sourceBinding/sourceId"),
                "sourceRecordId":job.pointer("/document/sourceBinding/sourceRecordId"),
                "inferenceAttempted":job.pointer("/document/inferenceAttempted"),
                "usageUncertain":job.pointer("/document/usageUncertain"), "automaticRetry":false,
            })
        };
        if let Some(id) = job_id {
            let job =
                crate::commands::knowledge::host_world_job(&self.state, &policy.loupist_id, &id)
                    .map_err(|error| BrokerError::Execution(error.to_string()))?;
            return Ok(summarize(&job));
        }
        let jobs = crate::commands::knowledge::host_world_jobs(&self.state, &policy.loupist_id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        Ok(json!(jobs
            .as_array()
            .ok_or(BrokerError::Malformed)?
            .iter()
            .map(summarize)
            .collect::<Vec<_>>()))
    }

    async fn validate_note_references(
        &self,
        policy: &ToolPolicy,
        research: &StagedResearch,
    ) -> Result<(), BrokerError> {
        let receipts = self.acknowledged_receipts(ReceiptPurpose::Research).await;
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
        if let Some(review) = &research.research_review {
            crate::knowledge::validate_research_review(review)
                .map_err(|error| BrokerError::invalid("researchReview", error.to_string()))?;
            for check in &review.checks {
                if check.evidence.iter().any(|reference| {
                    !research.source_reads.contains(reference)
                        && !research.world_reads.contains(reference)
                }) || check
                    .method_read
                    .as_ref()
                    .is_some_and(|reference| !research.lens_reads.contains(reference))
                {
                    return Err(BrokerError::invalid("researchReview", "cite only the enclosing note's acknowledged Source/World evidence and exact Lens reads"));
                }
            }
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
                .map_err(|error| BrokerError::invalid("journal.earlier.readRef", error.message))?;
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
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
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
            return Err(BrokerError::Execution(
                "Save learning is off, so this turn cannot retain a Source".into(),
            ));
        }
        let object = exact_object(
            &params,
            &["result_ref", "title"],
            &["coverage", "publicationTime"],
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
            && crate::privacy::holdings::is_account_result(&result.payload)
        {
            return Err(BrokerError::Execution(
                crate::privacy::holdings::NOT_SAVED.into(),
            ));
        }
        if !matches!(
            result.producer.origin,
            RunResultOrigin::AcquiredOriginal
                | RunResultOrigin::Discovery
                | RunResultOrigin::WorkspaceFile
        ) {
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
        let bytes = if workspace_file {
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
        let truncated = result
            .payload
            .pointer("/model_delivery/coverage")
            .and_then(Value::as_str)
            == Some("bounded_sample")
            || result.producer.origin == RunResultOrigin::Discovery
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
        } else if workspace_file {
            "original"
        } else {
            "provider_payload"
        };
        let document_id = match result.origin.as_deref() {
            Some(origin) if !origin.is_empty() => {
                format!("loc.{}", &crate::util::sha256(origin.as_bytes())[..32])
            }
            _ => format!(
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
            authorship: if workspace_file && result.payload["trust"] == "third_party_text" {
                // An app export is not reclassified as the user's own words.
                // Tool authorship also keeps it private for sharing purposes.
                crate::knowledge::Authorship::Tool
            } else {
                saved_authorship(&result.producer, custom_origin.is_some())
            },
            custom_origin,
            source_class: if workspace_file {
                crate::knowledge::SourceClass::UserFile
            } else {
                crate::knowledge::SourceClass::ToolResult
            },
            title,
            bytes,
            media_type: if workspace_file {
                result
                    .payload
                    .get("media_type")
                    .and_then(Value::as_str)
                    .ok_or(BrokerError::Malformed)?
                    .into()
            } else {
                "application/json".into()
            },
            origin: result
                .origin
                .clone()
                .unwrap_or_else(|| format!("tool:{}", result.producer.tool_name)),
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
            provider_identity: {
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
            "graph": "after_turn_commit",
            "result_ref": result_ref
        }))
    }

    pub(super) async fn world_search(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let object = exact_object(
            &params,
            &["query"],
            &[
                "scope",
                "records",
                "companies",
                "periods",
                "source_types",
                "offset",
            ],
        )?;
        let query = bounded_text(object.get("query"), 1, 200)?;
        let offset = search_offset(object.get("offset"))?;
        let scope = object.get("scope").and_then(Value::as_str).unwrap_or("all");
        if !matches!(scope, "all" | "world" | "source") {
            return Err(BrokerError::Malformed);
        }
        let filter = |name: &str| -> Result<Vec<String>, BrokerError> {
            let Some(value) = object.get(name) else {
                return Ok(Vec::new());
            };
            let rows = value
                .as_array()
                .filter(|rows| rows.len() <= 16)
                .ok_or(BrokerError::Malformed)?;
            rows.iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| !value.is_empty() && value.chars().count() <= 128)
                        .map(str::to_owned)
                        .ok_or(BrokerError::Malformed)
                })
                .collect()
        };
        let options = crate::knowledge::SearchOptions {
            record_ids: filter("records")?,
            companies: filter("companies")?,
            periods: filter("periods")?,
            source_types: filter("source_types")?,
            include_metadata: true,
            semantic_assets: self.state.graph.semantic_assets(),
        };
        let state = self.state.clone();
        let loupist_id = policy.loupist_id.clone();
        let owned_query = query.clone();
        let owned_scope = scope.to_owned();
        let owned_options = options.clone();
        // Local embeddings and SQL are synchronous; keep them off the async broker.
        let mut hits = tokio::task::spawn_blocking(move || {
            crate::commands::knowledge::host_search_with_options(
                &state,
                &loupist_id,
                &owned_query,
                &owned_scope,
                &owned_options,
            )
        })
        .await
        .map_err(|_| BrokerError::Execution("knowledge search failed".into()))?
        .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let capped = hits
            .as_array()
            .is_some_and(|rows| rows.len() >= crate::knowledge::SEARCH_WINDOW_LIMIT);
        if let Some(rows) = hits.as_array_mut() {
            rows.retain(|hit| {
                matches!(
                    hit.get("scope").and_then(Value::as_str),
                    Some("source" | "world")
                )
            });
        }
        let mut retrieval_warnings = Vec::new();
        // Only an explicit World scope starts the graph worker. The default
        // "all" search stays local so a relationship query is opt-in.
        // Structured filters must never be widened by unfiltered graph results.
        if scope == "world"
            && options.companies.is_empty()
            && options.periods.is_empty()
            && options.source_types.is_empty()
        {
            (hits, retrieval_warnings) = merge_graph_retrieval(
                &self.state,
                &policy.loupist_id,
                &query,
                hits,
                &options.record_ids,
            )
            .await;
            if let Some(rows) = hits.as_array_mut() {
                if !options.record_ids.is_empty() {
                    rows.retain(|hit| {
                        hit.get("recordId")
                            .and_then(Value::as_str)
                            .is_some_and(|id| options.record_ids.iter().any(|module| module == id))
                    });
                }
            }
        }
        self.as_of_hits(policy, &mut hits)?;
        let mut page = discovery_page(hits, offset, capped)?;
        page["partial"] = json!(!retrieval_warnings.is_empty());
        page["warnings"] = json!(retrieval_warnings);
        Ok(page)
    }

    pub(super) async fn world_read_journal(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let object = exact_object(&params, &["scope", "journalId"], &[])?;
        let journal_id = bounded_text(object.get("journalId"), 1, 128)?;
        let cutoff = crate::commands::knowledge::as_of::Cutoff::parse(policy.as_of.as_deref());
        let (text, staged) = crate::commands::knowledge::host_prepare_journal_read_as_of(
            &self.state,
            &policy.loupist_id,
            &journal_id,
            cutoff,
        )
        .map_err(|error| BrokerError::Execution(error.to_string()))?
        .ok_or_else(|| {
            let mut left = crate::commands::knowledge::as_of::LeftOut::default();
            left.notes.insert(journal_id.clone());
            self.capture.note_left_out(left);
            super::as_of_turn::later_than(
                cutoff.expect("only a cutoff hides an entry"),
                "Journal entry",
            )
        })?;
        let read_ref = staged.read_ref.clone();
        let title = staged.title.clone();
        let object_id = staged.object_id.clone();
        let (digest, coverage) = match &staged.payload {
            AckedPayload::Journal {
                digest, coverage, ..
            } => (digest.clone(), coverage.clone()),
            _ => {
                return Err(BrokerError::Execution(
                    "journal read did not carry a delivered view".into(),
                ))
            }
        };
        self.capture
            .stage(delivery_id, |pending| pending.staged_reads.push(staged))
            .await;
        let memory = crate::domain::MemoryRefSnapshot {
            read_ref: read_ref.clone(),
            kind: "journal".into(),
            title,
            excerpt: text.chars().take(180).collect(),
            as_of: None,
            section: None,
            access: crate::domain::MemoryAccess::ExactRead,
            full_record_digest: (coverage == "included_full").then(|| digest.clone()),
            record_id: String::new(),
            revision_id: String::new(),
            object_id,
            span_id: None,
            receipt_id: Some(read_ref.clone()),
        };
        self.stage_memory(delivery_id, memory).await?;
        Ok(json!({
            "status": "delivered_for_this_turn",
            "scope": "research",
            "journalId": journal_id,
            "readRef": read_ref,
            "coverage": coverage,
            "digest": digest,
            "text": text
        }))
    }

    pub(super) async fn world_read(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let scope = params
            .get("scope")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        if !matches!(scope, "world" | "source") {
            return Err(BrokerError::Malformed);
        }
        self.read_record(policy, params, delivery_id).await
    }

    async fn read_record(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let object = exact_object(
            &params,
            &["scope", "recordId"],
            &[
                "revisionId",
                "spanId",
                "search",
                "centerNodeId",
                "centerClaimId",
                "kinds",
                "relations",
                "bindingRoles",
                "statuses",
                "nodeLimit",
                "edgeLimit",
                "byteLimit",
                "status",
            ],
        )?;
        let scope = bounded_text(object.get("scope"), 1, 16)?;
        let record_id = bounded_text(object.get("recordId"), 1, 128)?;
        let request = crate::commands::knowledge::HostReadRequest {
            as_of: parse_as_of_date(policy.as_of.as_deref())?,
            scope: scope.clone(),
            record_id: record_id.clone(),
            revision_id: object
                .get("revisionId")
                .and_then(Value::as_str)
                .map(str::to_string),
            span_id: object
                .get("spanId")
                .and_then(Value::as_str)
                .map(str::to_string),
            search: object
                .get("search")
                .map(|value| bounded_text(Some(value), 0, 200))
                .transpose()?
                .unwrap_or_default(),
            center_claim_id: optional_bounded_text(object.get("centerClaimId"), 1, 128)?,
            center_node_id: object
                .get("centerNodeId")
                .and_then(Value::as_str)
                .map(str::to_string),
            kinds: optional_unique_strings(object.get("kinds"), 16, 64)?,
            relations: optional_unique_strings(object.get("relations"), 16, 64)?,
            binding_roles: optional_unique_strings(object.get("bindingRoles"), 8, 64)?,
            statuses: optional_unique_strings(object.get("statuses"), 8, 64)?,
            node_limit: optional_limit(object.get("nodeLimit"))?,
            edge_limit: optional_limit(object.get("edgeLimit"))?,
            byte_limit: optional_limit(object.get("byteLimit"))?,
            lens_status: object
                .get("status")
                .map(|value| bounded_text(Some(value), 1, 16))
                .transpose()?
                .unwrap_or_else(|| "considered".to_string()),
        };
        if let Some(cutoff) = request.as_of.map(crate::commands::knowledge::as_of::Cutoff) {
            self.as_of_read(policy, &request, cutoff)?;
        }
        let (text, staged) = crate::commands::knowledge::host_prepare_read(
            &self.state,
            &policy.loupist_id,
            &request,
        )
        .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let read_ref = staged.read_ref.clone();
        let revision_id = staged.revision_id.clone();
        let span_id = staged.span_id.clone();
        let object_id = staged.object_id.clone();
        let title = staged.title.clone();
        let lens_kind = staged
            .lens_receipt("exact_read")
            .map(|receipt| receipt.kind);
        let source = match &staged.payload {
            super::AckedPayload::Source {
                capture_id,
                representation_id,
                selected_digest,
                coverage,
                attribution,
                publication_time,
                ..
            } => Some(json!({
                "spanId": span_id,
                "captureId": capture_id,
                "representationId": representation_id,
                "selectedDigest": selected_digest,
                "coverage": coverage,
                "attribution": attribution,
                "publicationTime": publication_time
            })),
            _ => None,
        };
        let disagreement = match &staged.payload {
            super::AckedPayload::World { view_json, .. } => {
                super::grid::sources_disagree(view_json)
            }
            _ => None,
        };
        self.capture
            .stage(delivery_id, |pending| pending.staged_reads.push(staged))
            .await;
        let memory = crate::domain::MemoryRefSnapshot {
            read_ref: read_ref.clone(),
            kind: scope.clone(),
            title,
            excerpt: text.chars().take(180).collect(),
            as_of: None,
            section: None,
            access: crate::domain::MemoryAccess::ExactRead,
            full_record_digest: None,
            record_id: record_id.clone(),
            revision_id: revision_id.clone(),
            object_id,
            span_id,
            receipt_id: Some(read_ref.clone()),
        };
        self.stage_memory(delivery_id, memory).await?;
        let mut delivered = json!({
            "status": "delivered_for_this_turn",
            "scope": scope,
            "recordId": record_id,
            "revisionId": revision_id,
            "readRef": read_ref,
            "text": text
        });
        if let Some(kind) = lens_kind {
            delivered["lensKind"] = json!(kind);
        }
        if let Some(source) = source {
            delivered["source"] = source;
        }
        if let Some(disagreement) = disagreement {
            delivered["sources_disagree"] = disagreement;
        }
        Ok(delivered)
    }

    /// Under a cutoff, a later Source or method is left out and a World read
    /// counts the relationships published later.
    fn as_of_read(
        &self,
        policy: &ToolPolicy,
        request: &crate::commands::knowledge::HostReadRequest,
        cutoff: crate::commands::knowledge::as_of::Cutoff,
    ) -> Result<(), BrokerError> {
        use crate::commands::knowledge::as_of;
        let execution = |error: crate::app::CommandError| BrokerError::Execution(error.to_string());
        let mut left = as_of::LeftOut::default();
        let refused = match request.scope.as_str() {
            "source" => {
                let known = as_of::source_known(
                    &self.state,
                    &policy.loupist_id,
                    &request.record_id,
                    request.revision_id.as_deref(),
                    request.span_id.as_deref(),
                    cutoff,
                )
                .map_err(execution)?;
                (!known).then(|| {
                    left.sources.insert(request.record_id.clone());
                    "saved Source"
                })
            }
            "lens" => {
                let known =
                    as_of::lens_known(&self.state, &policy.loupist_id, &request.record_id, cutoff)
                        .map_err(execution)?;
                (!known).then(|| {
                    left.methods.insert(request.record_id.clone());
                    "method"
                })
            }
            "world" => {
                left.facts.extend(
                    as_of::world_later_claims(
                        &self.state,
                        &policy.loupist_id,
                        &request.record_id,
                        request.revision_id.as_deref(),
                        cutoff,
                    )
                    .map_err(execution)?,
                );
                None
            }
            _ => None,
        };
        self.capture.note_left_out(left);
        match refused {
            Some(what) => Err(super::as_of_turn::later_than(cutoff, what)),
            None => Ok(()),
        }
    }

    pub(super) async fn stage_journal(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
        review: bool,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
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
        let journal = if review {
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
            }
        } else {
            parse_research_journal(&params, now_ms())?
        };
        let research = StagedResearch {
            research_review: None,
            as_of: None,
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
        if self
            .capture
            .staged_research
            .lock()
            .await
            .as_ref()
            .is_some_and(|r| r.journal.is_some())
            || self
                .capture
                .pending_deliveries
                .lock()
                .await
                .values()
                .any(|p| p.research.as_ref().is_some_and(|r| r.journal.is_some()))
        {
            return Err(BrokerError::invalid(
                "journal",
                "one journal save or review per turn",
            ));
        }
        self.capture
            .stage(delivery_id, |pending| pending.research = Some(research))
            .await;
        Ok(json!({"status":"staged", "durable":false, "kind":"journal"}))
    }

    pub(super) async fn stage_research(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::Execution(
                "Save learning is off, so this turn cannot save research".into(),
            ));
        }
        let object = exact_object(
            &params,
            &["workingNote"],
            &[
                "sourceReads",
                "worldReads",
                "lensReads",
                "previousNoteIds",
                "researchReview",
            ],
        )?;
        let research = StagedResearch {
            research_review: object
                .get("researchReview")
                .map(|value| {
                    serde_json::from_value(value.clone()).map_err(|_| {
                        BrokerError::invalid("researchReview", "invalid research review")
                    })
                })
                .transpose()?,
            as_of: policy.as_of.clone(),
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
                    BrokerError::Execution(error.to_string())
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
        let included_journal = research.journal.is_some();
        self.capture
            .stage(delivery_id, |pending| pending.research = Some(research))
            .await;
        Ok(json!({
            "status": "staged",
            "durable": false,
            "journal": included_journal
        }))
    }

    pub(super) async fn stage_lens_update(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates {
            return Err(BrokerError::Execution(
                "Save learning is off, so this turn cannot revise a Lens".into(),
            ));
        }
        let object = exact_object(
            &params,
            &["document", "rationale"],
            &[
                "recordId",
                "expectedRevisionId",
                "title",
                "kind",
                "sourceReads",
                "worldReads",
                "journalReads",
                "previousNoteIds",
                "feedback",
            ],
        )?;
        let document = object
            .get("document")
            .cloned()
            .ok_or(BrokerError::Malformed)
            .and_then(|value| {
                serde_json::from_value::<crate::knowledge::LensDocument>(value)
                    .map_err(|_| BrokerError::Malformed)
            })?;
        if document.markdown.trim().is_empty() {
            return Err(BrokerError::Malformed);
        }
        let record_id = optional_bounded_text(object.get("recordId"), 1, 128)?;
        let expected = optional_bounded_text(object.get("expectedRevisionId"), 1, 128)?;
        let title = optional_bounded_text(object.get("title"), 1, 180)?;
        let kind = optional_bounded_text(object.get("kind"), 1, 16)?;
        let creating = record_id.is_none();
        if record_id.is_some() != expected.is_some() {
            return Err(BrokerError::InvalidArguments {
                field: "expectedRevisionId".into(),
                expected: "An edit requires recordId and expectedRevisionId from lens_read. For a new Lens, omit both.".into(),
            });
        }
        if creating && title.is_none() {
            return Err(BrokerError::InvalidArguments {
                field: "title".into(),
                expected: "A new Lens requires its method or perspective title.".into(),
            });
        }
        if !creating && kind.is_some() {
            return Err(BrokerError::InvalidArguments {
                field: "kind".into(),
                expected: "Omit kind when editing an existing Lens; its kind is fixed. Keep recordId, expectedRevisionId, document and rationale, then retry.".into(),
            });
        }
        if kind
            .as_deref()
            .is_some_and(|kind| !matches!(kind, "perspective" | "method"))
        {
            return Err(BrokerError::InvalidArguments {
                field: "kind".into(),
                expected: "A new Lens kind must be method or perspective.".into(),
            });
        }
        let record_id = if creating {
            crate::knowledge::mint_lens_record_id()
        } else {
            record_id.unwrap_or_default()
        };
        let mut pending_ids = std::collections::BTreeSet::new();
        pending_ids.insert(record_id.clone());
        for staged in self.capture.staged_lenses.lock().await.iter() {
            if staged.expected_revision_id.is_empty() {
                pending_ids.insert(staged.record_id.clone());
            }
        }
        crate::commands::knowledge::validate_staged_lens(
            &self.state,
            &policy.loupist_id,
            if creating {
                None
            } else {
                Some(record_id.as_str())
            },
            title.as_deref(),
            &document,
            &pending_ids,
        )
        .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let revision = StagedLensRevision {
            title,
            kind,
            record_id: record_id.clone(),
            expected_revision_id: expected.unwrap_or_default(),
            document,
            rationale: bounded_text(object.get("rationale"), 1, 16_000)?,
            source_reads: optional_unique_strings(object.get("sourceReads"), 32, 128)?,
            world_reads: optional_unique_strings(object.get("worldReads"), 8, 128)?,
            journal_reads: optional_unique_strings(object.get("journalReads"), 8, 128)?,
            previous_note_ids: optional_unique_strings(object.get("previousNoteIds"), 8, 128)?,
            feedback: optional_bounded_text(object.get("feedback"), 1, 16_000)?,
        };
        validate_memory_text(&revision.rationale, 16_000, false)?;
        if let Some(feedback) = &revision.feedback {
            validate_memory_text(feedback, 16_000, true)?;
        }
        if creating && revision.kind.as_deref() == Some("perspective") {
            let staged_perspective = self
                .capture
                .staged_lenses
                .lock()
                .await
                .iter()
                .any(|lens| lens.kind.as_deref() == Some("perspective"));
            if staged_perspective
                || crate::commands::knowledge::has_standing_lens(&self.state, &policy.loupist_id)
                    .map_err(|_| {
                        BrokerError::Execution(
                            "Could not check the standing perspective. Try again.".into(),
                        )
                    })?
            {
                return Err(BrokerError::Execution("Read and revise the existing basic perspective, or create kind method instead of another perspective".into()));
            }
        }
        let reads = self.capture.staged_reads.lock().await;
        if !revision.expected_revision_id.is_empty()
            && !reads.iter().any(|read| {
                read.scope == "lens"
                    && read.record_id == revision.record_id
                    && read.revision_id == revision.expected_revision_id
            })
        {
            return Err(BrokerError::Execution(
                "Read and acknowledge the exact Lens revision before editing it".into(),
            ));
        }
        drop(reads);
        // Lens changes require tool acknowledgements, including for an edit's
        // exact revision. Automatic perspective context cannot authorize edits.
        let receipts = self.acknowledged_receipts(ReceiptPurpose::LensChange).await;
        for (scope, field, refs) in [
            ("source", "sourceReads", &revision.source_reads),
            ("world", "worldReads", &revision.world_reads),
            ("research", "journalReads", &revision.journal_reads),
        ] {
            receipts.validate(scope, field, refs)?;
        }
        let notes = self.capture.note_reads.lock().await;
        if revision
            .previous_note_ids
            .iter()
            .any(|id| !notes.contains(id))
        {
            return Err(BrokerError::Execution(
                "Lens change notes must be read and acknowledged in this turn".into(),
            ));
        }
        drop(notes);
        let folded_title = revision
            .title
            .as_deref()
            .map(crate::knowledge::fold_lens_title);
        if let Some(folded_title) = folded_title.as_deref() {
            let staged = self.capture.staged_lenses.lock().await;
            if staged.iter().any(|existing| {
                existing.record_id != record_id
                    && existing.title.as_deref().is_some_and(|pending| {
                        crate::knowledge::fold_lens_title(pending) == folded_title
                    })
            }) {
                return Err(BrokerError::Execution(
                    "an active Lens already uses this title".into(),
                ));
            }
        }
        let warnings = self.lens_overlap_warnings(policy, &revision);
        let mut replaced = self
            .capture
            .staged_lenses
            .lock()
            .await
            .iter()
            .any(|existing| existing.record_id == record_id);
        self.capture
            .stage(delivery_id, |pending| {
                replaced |= pending
                    .lenses
                    .iter()
                    .any(|existing| existing.record_id == record_id);
                pending
                    .lenses
                    .retain(|existing| existing.record_id != record_id);
                pending.lenses.push(revision);
            })
            .await;
        Ok(json!({
            "status": "staged",
            "durable": false,
            "recordId": record_id,
            "creating": creating,
            "replaced": replaced,
            "warnings": warnings
        }))
    }

    pub(super) fn capability_enabled(&self, entry_id: &str) -> bool {
        self.capability_ids.contains(entry_id)
    }

    pub(super) fn require_capability(&self, entry_id: &str) -> Result<(), BrokerError> {
        if self.capability_enabled(entry_id) {
            Ok(())
        } else {
            Err(BrokerError::MethodDenied)
        }
    }

    pub(super) async fn stage_memory(
        &self,
        delivery_id: &str,
        memory: MemoryRefSnapshot,
    ) -> Result<(), BrokerError> {
        memory
            .validate()
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        self.capture
            .stage(delivery_id, |pending| {
                if pending
                    .memories
                    .get(&memory.read_ref)
                    .is_some_and(|existing| {
                        memory_authority_rank(existing) > memory_authority_rank(&memory)
                    })
                {
                    return;
                }
                pending.memories.insert(memory.read_ref.clone(), memory);
            })
            .await;
        Ok(())
    }

    pub(super) fn validate_provider_tool_output(
        &self,
        output: Value,
        expected_tool: &str,
        expected_source_id: &str,
    ) -> Result<Value, BrokerError> {
        validate_provider_result(&output, expected_tool, expected_source_id)?;
        Ok(output)
    }

    pub(super) fn ensure_scope(&self, policy: &ToolPolicy) -> Result<(), BrokerError> {
        if policy.loupist_id != self.loupist_id {
            return Err(BrokerError::Execution(
                "Loupist/tool root scope does not match".into(),
            ));
        }
        let chat = self
            .state
            .store
            .get_chat(&policy.session_id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?
            .ok_or_else(|| BrokerError::Execution("chat session is missing".into()))?;
        if chat.loupist_id != policy.loupist_id {
            return Err(BrokerError::Execution(
                "Loupist/session scope does not match".into(),
            ));
        }
        Ok(())
    }
}

fn optional_limit(value: Option<&Value>) -> Result<Option<u32>, BrokerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let limit = number.as_u64().ok_or(BrokerError::Malformed)?;
            u32::try_from(limit)
                .ok()
                .filter(|limit| *limit > 0)
                .map(Some)
                .ok_or(BrokerError::Malformed)
        }
        Some(_) => Err(BrokerError::Malformed),
    }
}

pub(super) fn bounded_text(
    value: Option<&Value>,
    min: usize,
    max: usize,
) -> Result<String, BrokerError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| {
            let chars = text.chars().count();
            (min..=max).contains(&chars) && !text.contains('\0')
        })
        .map(str::to_owned)
        .ok_or(BrokerError::Malformed)
}

pub(super) fn optional_bounded_text(
    value: Option<&Value>,
    min: usize,
    max: usize,
) -> Result<Option<String>, BrokerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(_) => bounded_text(value, min, max).map(Some),
    }
}

fn optional_unique_strings(
    value: Option<&Value>,
    max_items: usize,
    max_chars: usize,
) -> Result<Vec<String>, BrokerError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .filter(|items| items.len() <= max_items)
        .ok_or(BrokerError::Malformed)?;
    let mut unique = BTreeSet::new();
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let text = bounded_text(Some(item), 1, max_chars)?;
        if !unique.insert(text.clone()) {
            return Err(BrokerError::Malformed);
        }
        values.push(text);
    }
    Ok(values)
}

async fn merge_graph_retrieval(
    state: &crate::app::AppState,
    loupist_id: &str,
    query: &str,
    mut hits: Value,
    record_ids: &[String],
) -> (Value, Vec<&'static str>) {
    const UNAVAILABLE: &str = "World graph retrieval is unavailable. These results contain saved local matches only; an empty result does not establish that the information is absent. Read the relevant Sources or research further.";
    let mut warnings = Vec::new();
    let worlds = match crate::commands::knowledge::host_search(state, loupist_id, "*", "world") {
        Ok(value) => value,
        Err(_) => return (hits, vec![UNAVAILABLE]),
    };
    let Some(rows) = worlds.as_array().cloned() else {
        return (hits, vec![UNAVAILABLE]);
    };
    let Some(items) = hits.as_array_mut() else {
        return (hits, vec![UNAVAILABLE]);
    };
    for world in rows
        .iter()
        .filter(|world| {
            record_ids.is_empty()
                || world
                    .get("recordId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| record_ids.iter().any(|module| module == id))
        })
        .take(4)
    {
        let record_id = world.get("recordId").and_then(Value::as_str).unwrap_or("");
        let revision_id = world
            .get("revisionId")
            .and_then(Value::as_str)
            .unwrap_or("");
        if record_id.is_empty() || revision_id.is_empty() {
            continue;
        }
        let Ok(result) = state
            .graph
            .query_indexed(loupist_id, record_id, revision_id, query)
            .await
        else {
            warnings.push(UNAVAILABLE);
            continue;
        };
        if result.get("indexed").and_then(Value::as_bool) != Some(true) {
            warnings.push(UNAVAILABLE);
            continue;
        }
        fuse_ranked_world_hits(
            items,
            record_id,
            revision_id,
            world
                .get("title")
                .cloned()
                .unwrap_or(Value::String(record_id.to_string())),
            result
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("hybrid"),
            result.get("view").cloned().unwrap_or(Value::Null),
        );
    }
    warnings.dedup();
    (hits, warnings)
}

const RRF_K: f64 = 60.0;

fn edge_claim_id(edge: &Value) -> Option<&str> {
    edge.get("adapter_id")
        .or_else(|| edge.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

fn rrf_score(ranks: &[Option<usize>]) -> f64 {
    ranks
        .iter()
        .flatten()
        .map(|rank| 1.0 / (RRF_K + *rank as f64))
        .sum()
}

/// Keep lexical World hits and fold Graphiti's ranked claims in by claim id.
/// A claim present in both lists keeps one row, ordered by reciprocal rank fusion.
pub(super) fn fuse_ranked_world_hits(
    items: &mut Vec<Value>,
    record_id: &str,
    revision_id: &str,
    title: Value,
    mode: &str,
    mut view: Value,
) {
    // graph.query returns {view, usage}; retain its accounting while ranking
    // the inner view. Canonical views and focused fixtures are already flat.
    let graph_view = view.get("view").unwrap_or(&view);
    let edges = graph_view
        .get("edges")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut lexical_rank = BTreeMap::new();
    let mut next_rank = 1usize;
    for hit in items.iter() {
        if hit.get("scope").and_then(Value::as_str) != Some("world")
            || hit.get("recordId").and_then(Value::as_str) != Some(record_id)
        {
            continue;
        }
        let Some(object_id) = hit.get("objectId").and_then(Value::as_str) else {
            continue;
        };
        if object_id.is_empty() || lexical_rank.contains_key(object_id) {
            continue;
        }
        lexical_rank.insert(object_id.to_string(), next_rank);
        next_rank += 1;
    }
    let mut hybrid_rank = BTreeMap::new();
    for (index, edge) in edges.iter().enumerate() {
        if let Some(id) = edge_claim_id(edge) {
            hybrid_rank.entry(id.to_string()).or_insert(index + 1);
        }
    }
    let fused = |id: &str| -> f64 {
        rrf_score(&[lexical_rank.get(id).copied(), hybrid_rank.get(id).copied()])
    };
    if !hybrid_rank.is_empty() {
        items.retain(|hit| {
            if hit.get("scope").and_then(Value::as_str) != Some("world")
                || hit.get("recordId").and_then(Value::as_str) != Some(record_id)
            {
                return true;
            }
            !matches!(
                hit.get("objectId").and_then(Value::as_str),
                Some(id) if hybrid_rank.contains_key(id)
            )
        });
    }
    let mut ranked_edges = edges;
    ranked_edges.sort_by(|left, right| {
        let left_score = edge_claim_id(left).map(fused).unwrap_or(0.0);
        let right_score = edge_claim_id(right).map(fused).unwrap_or(0.0);
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let graph_view = if view.get("view").is_some() {
        view.get_mut("view").expect("query envelope view")
    } else {
        &mut view
    };
    if let Some(slot) = graph_view.get_mut("edges") {
        *slot = Value::Array(ranked_edges);
    }
    let best = hybrid_rank
        .keys()
        .map(|id| (fused(id) * 1_000_000.0).round() as u64)
        .max()
        .unwrap_or(0);
    items.push(json!({
        "scope": "world",
        "recordId": record_id,
        "revisionId": revision_id,
        "mode": mode,
        "indexed": true,
        "retrieval": "rrf",
        "coverage": "snippet",
        "title": title,
        "score": best,
        "view": view,
    }));
    for hit in items.iter_mut() {
        if hit.get("scope").and_then(Value::as_str) != Some("world")
            || hit.get("recordId").and_then(Value::as_str) != Some(record_id)
            || hit.get("indexed").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let Some(id) = hit.get("objectId").and_then(Value::as_str) else {
            continue;
        };
        let score = (fused(id) * 1_000_000.0).round() as u64;
        if let Some(map) = hit.as_object_mut() {
            map.insert("score".to_string(), json!(score));
            map.insert("retrieval".to_string(), json!("rrf"));
        }
    }
    items.sort_by(|left, right| {
        let score = |hit: &Value| hit.get("score").and_then(Value::as_u64).unwrap_or(0);
        score(right).cmp(&score(left))
    });
}

fn search_offset(value: Option<&Value>) -> Result<usize, BrokerError> {
    value.map_or(Ok(0), |value| {
        value
            .as_u64()
            .filter(|offset| *offset < 200)
            .map(|offset| offset as usize)
            .ok_or(BrokerError::Malformed)
    })
}

/// Who authored a saved tool result, decided once when it is saved and kept
/// in the Source record. Public data tools (fetched web pages, DART filings
/// and financials, KRX market data, World Bank series and the bundled OpenBB
/// market-data worker) record the publisher. Workspace files, every
/// Korea Investment read and custom connections record the user: they read
/// the user's own files, accounts or private services. Anything else stays
/// the tool's. Sharing exports only publisher-authored Sources, so a new or
/// unknown producer stays private by default.
pub(super) fn saved_authorship(
    producer: &RunResultProducer,
    custom_connection: bool,
) -> crate::knowledge::Authorship {
    use crate::knowledge::Authorship;
    let provider = producer.provider.as_deref().unwrap_or_default();
    let account = producer.tool_name.starts_with("kis_")
        || producer.runtime_id.contains("koreainvestment")
        || provider.starts_with("koreainvestment")
        || provider.starts_with("kis");
    if custom_connection
        || account
        || producer.origin == RunResultOrigin::WorkspaceFile
        || producer.runtime_id.starts_with("custom")
    {
        return Authorship::User;
    }
    let public = matches!(
        (producer.runtime_id.as_str(), producer.tool_name.as_str()),
        ("native-web", "web_fetch")
            | (
                "native-finance",
                "dart_financials"
                    | "dart_filing_read"
                    | "dart_filing_search"
                    | "dart_company_lookup"
                    | "dart_major_report"
                    | "dart_ownership"
                    | "dart_dividends"
                    | "krx_daily_quote"
                    | "worldbank_series"
                    | "defillama_read"
            )
            | ("openbb", _)
    );
    if public {
        Authorship::Publisher
    } else {
        Authorship::Tool
    }
}

fn discovery_page(hits: Value, offset: usize, capped: bool) -> Result<Value, BrokerError> {
    let rows = hits.as_array().ok_or(BrokerError::Malformed)?;
    let capped = capped || rows.len() >= crate::knowledge::SEARCH_WINDOW_LIMIT;
    let available = rows.len().min(200);
    if offset > available {
        return Err(BrokerError::Malformed);
    }
    let mut page = Vec::new();
    let mut bytes = 0;
    for row in rows
        .iter()
        .take(available)
        .skip(offset)
        .take(crate::knowledge::SEARCH_LIMIT)
    {
        let mut hit = row.clone();
        let read_args = match hit["scope"].as_str() {
            Some("lens") => json!({"recordId":hit["recordId"],"revisionId":hit["revisionId"]}),
            Some("source") if hit["spanId"].as_str().is_some_and(|span| !span.is_empty()) => {
                json!({"scope":"source","recordId":hit["recordId"],"revisionId":hit["revisionId"],"spanId":hit["spanId"]})
            }
            Some("world") => {
                let mut args = hit.get("readArgs").cloned().unwrap_or_else(|| {
                    json!({"scope":"world","recordId":hit["recordId"],"revisionId":hit["revisionId"]})
                });
                let graph_view = hit.get("view").map(|view| view.get("view").unwrap_or(view));
                if let Some(claim_id) = graph_view
                    .and_then(|view| view.get("edges"))
                    .and_then(Value::as_array)
                    .and_then(|edges| edges.first())
                    .and_then(edge_claim_id)
                {
                    args["centerClaimId"] = json!(claim_id);
                }
                args
            }
            _ => Value::Null,
        };
        if serde_json::to_vec(&hit)
            .map_err(|_| BrokerError::Malformed)?
            .len()
            > 10_000
        {
            // Indexed graph data is only discovery. A large view is fetched with an exact world_read.
            if let Some(object) = hit.as_object_mut() {
                object.remove("view");
                object.insert("notice".into(), json!("Indexed graph matched this query; world_read opens the exact bounded view."));
            }
        }
        if !read_args.is_null() {
            hit["readArgs"] = read_args;
            if hit["scope"] == "source" {
                // Source object ids are not record ids. Expose the complete canonical
                // read arguments instead of a second, unusable source identity.
                if let Some(object) = hit.as_object_mut() {
                    object.remove("objectId");
                }
            }
        }
        let size = serde_json::to_vec(&hit)
            .map_err(|_| BrokerError::Malformed)?
            .len();
        if !page.is_empty() && bytes + size > 12_000 {
            break;
        }
        bytes += size;
        page.push(hit);
    }
    let next = offset + page.len();
    Ok(
        json!({"hits":page,"offset":offset,"nextOffset":if next < available {Some(next)} else {None},
        "hasMore": next < available || capped,"capped":capped,"windowLimit":200,"notEvidence":true,
        "notice":if capped {"Only the first 200 ranked discoveries are available; narrow the query or filter records for more."} else {"Search hits are discovery only. Read exact records before relying on them."}}),
    )
}

fn validate_memory_text(
    text: &str,
    max_bytes: usize,
    empty_allowed: bool,
) -> Result<(), BrokerError> {
    crate::knowledge::validate_text(text, max_bytes, empty_allowed, "invalid Memory text")
        .map_err(|_| BrokerError::Execution(format!("Memory text must fit {max_bytes} UTF-8 bytes and contain no unsupported control characters")))
}
