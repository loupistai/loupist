//! Memory reads inside the tool executor: Lens and World search, exact
//! Lens, World and Source reads, World job status, and the read snapshots
//! a delivered read stages for receipts.
use super::arguments::{bounded_text, optional_bounded_text, optional_unique_strings};
use super::*;

impl AppToolExecutor {
    pub(super) fn lens_search(
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
        .map_err(command_error)?;
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
        let delivered_revision = delivered["revisionId"]
            .as_str()
            .ok_or(BrokerError::Malformed)?
            .to_string();
        crate::commands::knowledge::lens_read(
            &self.state,
            crate::commands::knowledge::KnowledgeLensReadRequest {
                loupist_id: policy.loupist_id.clone(),
                record_id: record_id.clone(),
                // Under a cutoff the delivered revision is the page as it stood
                // then, not necessarily its head.
                revision_id: policy.as_of.is_none().then_some(delivered_revision),
            },
        )
        .map_err(command_error)?;
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
                    .map_err(command_error)?;
            return Ok(summarize(&job));
        }
        let jobs = crate::commands::knowledge::host_world_jobs(&self.state, &policy.loupist_id)
            .map_err(command_error)?;
        Ok(json!(jobs
            .as_array()
            .ok_or(BrokerError::Malformed)?
            .iter()
            .map(summarize)
            .collect::<Vec<_>>()))
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
        let (record_ids, companies, periods, source_types) = (
            filter("records")?,
            filter("companies")?,
            filter("periods")?,
            filter("source_types")?,
        );
        let lookup = self.state.graph.semantic_lookup();
        let model_loading = lookup.loading;
        let options = crate::knowledge::SearchOptions {
            record_ids,
            companies,
            periods,
            source_types,
            include_metadata: true,
            semantic_model: lookup.model,
        };
        let state = self.state.clone();
        let loupist_id = policy.loupist_id.clone();
        let owned_query = query.clone();
        // World adds graph retrieval to the default local search.
        let local_scope = if scope == "world" { "all" } else { scope }.to_owned();
        let owned_options = options.clone();
        let cutoff = crate::commands::knowledge::as_of::Cutoff::parse(policy.as_of.as_deref());
        // Source hits name their origin and sourceClass. SYSTEM.md's Memory
        // paragraph has a Source hit opened before the web, and a data
        // connection serving the official record read over a copy saved from
        // another site (v0.1.6 acceptance: a saved Fed announcement fetched
        // again from the web; excerpts from a third-party site answered in
        // place of the connected OpenDART).
        // Local embeddings and SQL are synchronous; keep them off the async broker.
        let (mut hits, capped) = tokio::task::spawn_blocking(move || {
            let mut hits = crate::commands::knowledge::host_search_with_options(
                &state,
                &loupist_id,
                &owned_query,
                &local_scope,
                &owned_options,
            )?;
            let capped = hits
                .as_array()
                .is_some_and(|rows| rows.len() >= crate::knowledge::SEARCH_WINDOW_LIMIT);
            if let Some(rows) = hits.as_array_mut() {
                crate::commands::knowledge::describe_source_hits(
                    &state,
                    &loupist_id,
                    rows,
                    cutoff,
                )?;
            }
            Ok::<_, crate::app::CommandError>((hits, capped))
        })
        .await
        .map_err(|_| BrokerError::Execution("knowledge search failed".into()))?
        .map_err(command_error)?;
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
        // The search did not wait for a model still loading after launch.
        if model_loading {
            retrieval_warnings.push(MODEL_LOADING);
        }
        self.as_of_hits(policy, &mut hits)?;
        for hit in hits.as_array_mut().into_iter().flatten() {
            if let Some(view) = hit.get_mut("view") {
                figure_on_edges(view);
            }
        }
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
        .map_err(command_error)?
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

    pub(super) async fn world_changes(
        &self,
        policy: &crate::broker::ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let object = exact_object(
            &params,
            &[],
            &["since", "axis", "about", "kinds", "limit", "between"],
        )?;
        if object.contains_key("since") && object.contains_key("between") {
            return Err(BrokerError::invalid(
                "between",
                "pass since or between, not both",
            ));
        }
        if let Some(between) = object.get("between") {
            return self.world_between(policy, between, delivery_id).await;
        }
        let since_text = bounded_text(object.get("since"), 1, 4096)?;
        let axis_explicit = object.contains_key("axis");
        let axis = match object.get("axis").and_then(Value::as_str) {
            None => crate::knowledge::world_changes::Axis::Recorded,
            Some("recorded") => crate::knowledge::world_changes::Axis::Recorded,
            Some("published") => crate::knowledge::world_changes::Axis::Published,
            Some(_) => return Err(BrokerError::invalid("axis", "recorded or published")),
        };
        let since = if let Some(token) = since_text.strip_prefix("wch1.") {
            let _ = token;
            crate::knowledge::world_changes::Since::Cursor(since_text)
        } else if since_text == "previous_run" {
            let Some(ms) = self.capture.previous_run_started_at_ms else {
                return Err(BrokerError::invalid(
                    "since",
                    "since_previous_run_unavailable: this turn has no previous run start",
                ));
            };
            crate::knowledge::world_changes::Since::Millis {
                ms,
                inclusive: true,
            }
        } else if let Ok(date) = chrono::NaiveDate::parse_from_str(&since_text, "%Y-%m-%d") {
            crate::knowledge::world_changes::Since::Date(date)
        } else {
            return Err(BrokerError::invalid(
                "since",
                "a YYYY-MM-DD date, previous_run, or a next cursor",
            ));
        };
        let about = parse_change_about(object.get("about"))?;
        let kinds = parse_change_kinds(object.get("kinds"))?;
        let limit = match object.get("limit") {
            None | Some(Value::Null) => crate::knowledge::world_changes::DEFAULT_LIMIT,
            Some(Value::Number(number)) => number
                .as_u64()
                .and_then(|limit| u32::try_from(limit).ok())
                .filter(|limit| (1..=50).contains(limit))
                .ok_or(BrokerError::Malformed)?,
            Some(_) => return Err(BrokerError::Malformed),
        };
        let read = crate::commands::knowledge::read_changes(
            &self.state,
            &policy.loupist_id,
            crate::knowledge::world_changes::FeedQuery {
                loupist_id: policy.loupist_id.clone(),
                since,
                axis,
                axis_explicit,
                about,
                kinds,
                limit,
                first_run: self.capture.first_run,
                cutoff: crate::commands::knowledge::as_of::Cutoff::parse(policy.as_of.as_deref())
                    .map(|cutoff| cutoff.0),
            },
        )
        .map_err(change_feed_error)?;
        let read_ref = format!("rdg_{}", uuid::Uuid::new_v4().simple());
        let mut body = read.body;
        body["readRef"] = json!(read_ref.clone());
        if !read.revision_id.is_empty() {
            let view_json = read.view_json.clone();
            let view_digest = crate::util::sha256(view_json.as_bytes());
            let staged = super::StagedKnowledgeRead {
                read_ref: read_ref.clone(),
                scope: "world".into(),
                record_id: read.record_id.clone(),
                span_id: None,
                revision_id: read.revision_id.clone(),
                object_id: read.record_id.clone(),
                title: "World changes".into(),
                evidence_origin: None,
                payload: super::AckedPayload::World {
                    revision_digest: read.revision_digest.clone(),
                    view_json,
                    view_digest,
                },
            };
            self.capture
                .stage(delivery_id, |pending| pending.staged_reads.push(staged))
                .await;
            self.stage_memory(
                delivery_id,
                crate::domain::MemoryRefSnapshot {
                    read_ref: read_ref.clone(),
                    kind: "world".into(),
                    title: "World changes".into(),
                    excerpt: String::new(),
                    as_of: None,
                    section: None,
                    access: crate::domain::MemoryAccess::ExactRead,
                    full_record_digest: None,
                    object_id: read.record_id.clone(),
                    record_id: read.record_id,
                    revision_id: read.revision_id,
                    span_id: None,
                    receipt_id: Some(read_ref),
                },
            )
            .await?;
        }
        Ok(body)
    }

    async fn world_between(
        &self,
        policy: &crate::broker::ToolPolicy,
        between: &Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let object = between.as_object().ok_or(BrokerError::Malformed)?;
        if object.keys().any(|key| key != "to" && key != "from") {
            return Err(BrokerError::Malformed);
        }
        let to = bounded_text(object.get("to"), 1, 128)?;
        let from = optional_bounded_text(object.get("from"), 1, 128)?;
        let read = crate::commands::knowledge::read_between(
            &self.state,
            &policy.loupist_id,
            crate::knowledge::world_changes::BetweenQuery {
                loupist_id: policy.loupist_id.clone(),
                to,
                from,
                cutoff: crate::commands::knowledge::as_of::Cutoff::parse(policy.as_of.as_deref())
                    .map(|cutoff| cutoff.0),
            },
        )
        .map_err(change_feed_error)?;
        let read_ref = format!("rdg_{}", uuid::Uuid::new_v4().simple());
        let mut body = read.body;
        body["readRef"] = json!(read_ref.clone());
        if !read.revision_id.is_empty() {
            let view_json = read.view_json.clone();
            let view_digest = crate::util::sha256(view_json.as_bytes());
            let staged = super::StagedKnowledgeRead {
                read_ref: read_ref.clone(),
                scope: "world".into(),
                record_id: read.record_id.clone(),
                span_id: None,
                revision_id: read.revision_id.clone(),
                object_id: read.record_id.clone(),
                title: "World changes".into(),
                evidence_origin: None,
                payload: super::AckedPayload::World {
                    revision_digest: read.revision_digest,
                    view_json,
                    view_digest,
                },
            };
            self.capture
                .stage(delivery_id, |pending| pending.staged_reads.push(staged))
                .await;
            self.stage_memory(
                delivery_id,
                crate::domain::MemoryRefSnapshot {
                    read_ref: read_ref.clone(),
                    kind: "world".into(),
                    title: "World changes".into(),
                    excerpt: String::new(),
                    as_of: None,
                    section: None,
                    access: crate::domain::MemoryAccess::ExactRead,
                    full_record_digest: None,
                    object_id: read.record_id.clone(),
                    record_id: read.record_id,
                    revision_id: read.revision_id,
                    span_id: None,
                    receipt_id: Some(read_ref),
                },
            )
            .await?;
        }
        Ok(body)
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
                "asOf",
                "history",
            ],
        )?;
        let scope = bounded_text(object.get("scope"), 1, 16)?;
        let record_id = bounded_text(object.get("recordId"), 1, 128)?;
        let tool_as_of = match object.get("asOf") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => parse_as_of_date(Some(value))?,
            Some(_) => {
                return Err(BrokerError::invalid(
                    "asOf",
                    "expected a calendar date in YYYY-MM-DD format",
                ))
            }
        };
        let turn_as_of = parse_as_of_date(policy.as_of.as_deref())?;
        let history = object
            .get("history")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let request = crate::commands::knowledge::HostReadRequest {
            as_of: match (turn_as_of, tool_as_of) {
                (Some(turn), Some(tool)) => Some(turn.min(tool)),
                (turn, tool) => turn.or(tool),
            },
            scope: scope.clone(),
            record_id: record_id.clone(),
            revision_id: optional_bounded_text(object.get("revisionId"), 1, 128)?,
            span_id: optional_bounded_text(object.get("spanId"), 1, 128)?,
            search: object
                .get("search")
                .map(|value| bounded_text(Some(value), 0, 200))
                .transpose()?
                .unwrap_or_default(),
            center_claim_id: optional_bounded_text(object.get("centerClaimId"), 1, 128)?,
            center_node_id: optional_bounded_text(object.get("centerNodeId"), 1, 128)?,
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
        let (mut text, mut staged) = crate::commands::knowledge::host_prepare_read(
            &self.state,
            &policy.loupist_id,
            &request,
        )
        .map_err(command_error)?;
        if let super::AckedPayload::World {
            view_json,
            view_digest,
            ..
        } = &mut staged.payload
        {
            // The delivered text and the staged view stay one string, so a
            // citation of this read verifies against what the model saw.
            if let Ok(mut view) = serde_json::from_str::<Value>(view_json) {
                if figure_on_edges(&mut view) {
                    *view_json = view.to_string();
                    *view_digest = crate::util::sha256(view_json.as_bytes());
                    text = view_json.clone();
                }
            }
        }
        let lineage = if history && scope == "world" {
            request.center_claim_id.as_deref().and_then(|claim_id| {
                let replay = request.as_of.map(|day| {
                    (
                        staged.revision_id.as_str(),
                        crate::commands::knowledge::as_of::Cutoff(day),
                    )
                });
                crate::commands::knowledge::read_lineage(
                    &self.state,
                    &policy.loupist_id,
                    claim_id,
                    replay,
                )
                .ok()
                .flatten()
            })
        } else {
            None
        };
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
                provided,
                ..
            } => {
                let mut source = json!({
                    "spanId": span_id,
                    "captureId": capture_id,
                    "representationId": representation_id,
                    "selectedDigest": selected_digest,
                    "coverage": coverage,
                    "attribution": attribution,
                    "publicationTime": publication_time
                });
                // Material the user pasted earlier: its link and publisher are
                // what the user stated, and nothing checked them.
                if let Some(provided) = provided {
                    source["sourceClass"] =
                        json!(crate::knowledge::SourceClass::UserProvided.as_str());
                    source["provided"] = provided_delivery(provided);
                }
                Some(source)
            }
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
        if let Some(lineage) = lineage {
            delivered["lineage"] = lineage;
        }
        Ok(delivered)
    }

    /// Under a cutoff, a later Source or a page made later is left out and a
    /// World read counts the relationships published later. A Lens page that
    /// existed then reads as it stood then.
    fn as_of_read(
        &self,
        policy: &ToolPolicy,
        request: &crate::commands::knowledge::HostReadRequest,
        cutoff: crate::commands::knowledge::as_of::Cutoff,
    ) -> Result<(), BrokerError> {
        use crate::commands::knowledge::as_of;
        let execution = command_error;
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
                let known = as_of::lens_created_by(
                    &self.state,
                    &policy.loupist_id,
                    &request.record_id,
                    cutoff,
                )
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
}

fn change_feed_error(error: crate::app::CommandError) -> BrokerError {
    for code in [
        "cursor_axis_mismatch",
        "since_after_cutoff",
        "between_needs_from",
        "change_cursor_invalid",
        "change_limit_invalid",
        "change_about_invalid",
        "change_kinds_invalid",
        "change_since_invalid",
    ] {
        if error.message.contains(code) {
            return BrokerError::invalid("since", format!("{code}: {}", error.message));
        }
    }
    command_error(error)
}

fn parse_change_about(
    value: Option<&Value>,
) -> Result<Vec<crate::knowledge::world_changes::AboutRef>, BrokerError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let rows = value
        .as_array()
        .filter(|rows| rows.len() <= 16)
        .ok_or(BrokerError::Malformed)?;
    rows.iter()
        .map(|item| match item {
            Value::String(id) if id.starts_with("ast_") => {
                Ok(crate::knowledge::world_changes::AboutRef::Asset { id: id.clone() })
            }
            Value::String(id) if id.starts_with("rec_") => {
                Ok(crate::knowledge::world_changes::AboutRef::Lens { id: id.clone() })
            }
            Value::String(id) => {
                Ok(crate::knowledge::world_changes::AboutRef::Node { id: id.clone() })
            }
            Value::Object(object) => {
                let one = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
                match (one("node"), one("asset"), one("lens")) {
                    (Some(id), None, None) => {
                        Ok(crate::knowledge::world_changes::AboutRef::Node { id })
                    }
                    (None, Some(id), None) => {
                        Ok(crate::knowledge::world_changes::AboutRef::Asset { id })
                    }
                    (None, None, Some(id)) => {
                        Ok(crate::knowledge::world_changes::AboutRef::Lens { id })
                    }
                    _ => Err(BrokerError::invalid(
                        "about",
                        "a node id, an asset id, or a Lens record id",
                    )),
                }
            }
            _ => Err(BrokerError::Malformed),
        })
        .collect()
}

fn parse_change_kinds(
    value: Option<&Value>,
) -> Result<
    Option<std::collections::BTreeSet<crate::knowledge::world_changes::ChangeKind>>,
    BrokerError,
> {
    let Some(value) = value else {
        return Ok(None);
    };
    let rows = value.as_array().ok_or(BrokerError::Malformed)?;
    let mut kinds = std::collections::BTreeSet::new();
    for row in rows {
        let name = row.as_str().ok_or(BrokerError::Malformed)?;
        let kind = crate::knowledge::world_changes::ChangeKind::parse(name).ok_or_else(|| {
            BrokerError::invalid(
                "kinds",
                "added, supported, ended, conflict, disputed, withdrawn, or lens",
            )
        })?;
        kinds.insert(kind);
    }
    Ok(Some(kinds))
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

/// A relationship whose stored support did not find its figure in the cited
/// passage carries `figure: "not_found"`; no other verdict reaches the model.
/// The per-support verdicts are removed and each support keeps its passage.
/// Returns whether the view changed.
fn figure_on_edges(view: &mut Value) -> bool {
    let view = if view.get("view").is_some_and(Value::is_object) {
        &mut view["view"]
    } else {
        view
    };
    let Some(edges) = view.get_mut("edges").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for edge in edges {
        let mut not_found = false;
        for path in ["/evidence", "/claim/evidence"] {
            let Some(evidence) = edge.pointer_mut(path).and_then(Value::as_array_mut) else {
                continue;
            };
            for support in evidence.iter_mut().filter_map(Value::as_object_mut) {
                for key in ["figure_check", "figureCheck"] {
                    if let Some(verdict) = support.remove(key) {
                        changed = true;
                        not_found |= verdict == "not_found";
                    }
                }
            }
        }
        if not_found {
            edge["figure"] = json!("not_found");
        }
    }
    changed
}

const MODEL_LOADING: &str = "The local embedding model is still loading, so these results match words and World names only and can miss a Source written in another language. Search again shortly before concluding the information is absent.";

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
    // World rows ranked here, or by an earlier World's merge, lead by their
    // fused score. The rest keep knowledge search's order: its own fused
    // scores are on another scale (1,000,000 for a first place), so the two
    // are never compared.
    items.sort_by(|left, right| {
        let fused = |hit: &Value| {
            (hit.get("retrieval").and_then(Value::as_str) == Some("rrf"))
                .then(|| hit.get("score").and_then(Value::as_u64).unwrap_or(0))
        };
        fused(right).cmp(&fused(left))
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

/// What a Source read tells the model about a user-provided capture: when it
/// was pasted and what the user stated about it. Chat and message ids stay
/// with the host.
pub(super) fn provided_delivery(provided: &crate::knowledge::ProvidedOrigin) -> Value {
    let mut delivered = json!({
        "pastedAt": DateTime::<Utc>::from_timestamp_millis(provided.pasted_at_ms)
            .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true)),
    });
    if let Some(url) = &provided.stated_url {
        delivered["statedUrl"] = json!(url);
    }
    if let Some(publisher) = &provided.stated_publisher {
        delivered["statedPublisher"] = json!(publisher);
    }
    delivered
}
