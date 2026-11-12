use super::*;

/// What the model may publish here, checked exactly: only Markdown. A saved
/// payload (`ChatArtifactPayload`) is read leniently from storage instead.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ModelArtifactPayload {
    Markdown { schema: String, markdown: String },
}

impl From<ModelArtifactPayload> for ChatArtifactPayload {
    fn from(payload: ModelArtifactPayload) -> Self {
        match payload {
            ModelArtifactPayload::Markdown { schema, markdown } => {
                Self::Markdown { schema, markdown }
            }
        }
    }
}

// Select one view contract before deserializing so a line typo never reports
// candlestick's required OHLC fields. Rust remains authoritative for direct calls.
fn validate_view_input(view: &Value) -> Result<(), BrokerError> {
    let object = view.as_object().ok_or_else(|| {
        BrokerError::invalid("view", "expected an object with kind and column mappings")
    })?;
    let (required, optional): (&[&str], &[&str]) = match view.get("kind").and_then(Value::as_str) {
        Some("candlestick") => (
            &["symbol", "interval", "time", "open", "high", "low", "close"],
            &["volume", "turnover", "price_precision"],
        ),
        Some("line" | "area" | "bar" | "scatter") => (
            &["x", "y"],
            &["color", "semantic_types", "title", "subtitle"],
        ),
        _ => {
            return Err(BrokerError::invalid(
                "view.kind",
                "expected line, area, bar, scatter, or candlestick",
            ))
        }
    };
    for field in required {
        if !object.contains_key(*field) {
            return Err(BrokerError::invalid(
                format!("view.{field}"),
                if *field == "y" {
                    "required; supply a column id string or an array of 1–8 column id strings"
                } else {
                    "required; supply a nonempty string"
                },
            ));
        }
    }
    for (field, value) in object {
        let expected = match field.as_str() {
            "kind" => continue,
            field if !required.contains(&field) && !optional.contains(&field) => {
                return Err(BrokerError::invalid(
                    format!("view.{field}"),
                    "not used by this view.kind; omit it",
                ));
            }
            "y" => {
                if value.as_str().is_some_and(|text| !text.trim().is_empty())
                    || value.as_array().is_some_and(|items| {
                        !items.is_empty()
                            && items.len() <= 8
                            && items.iter().all(|item| {
                                item.as_str().is_some_and(|text| !text.trim().is_empty())
                            })
                    })
                {
                    continue;
                }
                "expected a numeric column id string or an array of 1–8 column id strings"
            }
            "price_precision" => {
                if value.as_u64().is_some_and(|number| number <= 12) {
                    continue;
                }
                "expected an integer from 0 to 12"
            }
            "semantic_types" => {
                if value
                    .as_object()
                    .is_some_and(|types| types.values().all(Value::is_string))
                {
                    continue;
                }
                "expected an object mapping column ids to semantic type strings"
            }
            _ => {
                if value.as_str().is_some_and(|text| !text.trim().is_empty()) {
                    continue;
                }
                "expected a nonempty string"
            }
        };
        return Err(BrokerError::invalid(format!("view.{field}"), expected));
    }
    Ok(())
}

fn chart_argument<T: serde::de::DeserializeOwned>(
    params: &Value,
    field: &str,
) -> Result<T, BrokerError> {
    serde_json::from_value(params.get(field).cloned().unwrap_or(Value::Null))
        .map_err(|_| BrokerError::invalid(field, match field {
            "mode" => "expected create or revise",
            "title" | "artifact_id" | "edit_token" | "note" => "expected text",
            "view" => "expected a line, area, bar, scatter, or candlestick view with column mappings",
            "dataset" => "expected exactly one of from_dataset, from_result, or inline with its documented fields",
            "studies" | "custom_series" | "drawings" => "expected an array of objects matching the tool schema",
            _ => "expected the shape specified by the tool schema",
        }))
}

fn validate_view_columns(
    view: &crate::chat::chart::ChartView,
    dataset: &crate::chat::chart::ChartDataset,
) -> Result<(), BrokerError> {
    let missing = view.missing_column_mappings(dataset);
    if missing.is_empty() {
        return Ok(());
    }
    Err(BrokerError::Execution(format!(
        "{}; use ids from dataset.columns",
        missing.join("; ")
    )))
}

fn normalize_chart_note(note: Option<String>) -> Option<String> {
    note.and_then(|note| {
        let trimmed = note.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

/// What to do when a chart cannot be read as of the turn's date.
const CHART_AS_OF_NEXT: &str = "Fetch the data again in this turn, where the as-of cutoff bounds the read, and draw a new chart from that dataset_ref.";

impl AppToolExecutor {
    /// Under an as-of date a chart shows what was known then only when its
    /// rows came from a dataset revision fetched under a cutoff on or before it.
    fn chart_within_cutoff(
        &self,
        policy: &ToolPolicy,
        artifact_id: &str,
        dataset: &crate::chat::chart::ChartDataset,
    ) -> Result<(), BrokerError> {
        let Some(cutoff) = parse_as_of_date(policy.as_of.as_deref())? else {
            return Ok(());
        };
        match &dataset.lineage {
            crate::chat::chart::ChartDatasetLineage::FromDataset {
                dataset_id,
                revision,
                ..
            } => self.ensure_dataset_within_cutoff(policy, dataset_id, *revision),
            _ => Err(BrokerError::Execution(format!(
                "as_of_unsupported: chart {artifact_id} was not drawn from a dataset fetched under an as-of cutoff, so it cannot show only what was known on {cutoff}"
            ))
            .with_next(CHART_AS_OF_NEXT)),
        }
    }

    /// Under an as-of date a report written after it is left out.
    fn report_within_cutoff(
        &self,
        policy: &ToolPolicy,
        artifact_id: &str,
        created_at_ms: i64,
    ) -> Result<(), BrokerError> {
        let Some(cutoff) =
            crate::commands::knowledge::as_of::Cutoff::parse(policy.as_of.as_deref())
        else {
            return Ok(());
        };
        if cutoff.recorded(created_at_ms) {
            return Ok(());
        }
        let mut left = crate::commands::knowledge::as_of::LeftOut::default();
        left.notes.insert(artifact_id.to_owned());
        self.capture.note_left_out(left);
        Err(super::as_of_turn::later_than(cutoff, "report"))
    }

    pub(super) fn artifact_list(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        exact_object(&params, &[], &[])?;
        let artifacts = self
            .state
            .store
            .list_chat_artifacts(&policy.session_id)
            .map_err(store_error)?;
        Ok(json!({
            "artifacts": artifacts.into_iter().map(|artifact| json!({
                "artifact_id": artifact.id,
                "title": artifact.title,
                "kind": artifact.kind,
                "current_revision": artifact.current_revision,
                "updated_at_ms": artifact.updated_at_ms,
            })).collect::<Vec<_>>()
        }))
    }

    pub(super) async fn artifact_read(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let object = exact_object(&params, &["artifact_id"], &["revision_id"])?;
        let artifact_id = object
            .get("artifact_id")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?
            .to_owned();
        let artifact = self
            .state
            .store
            .get_chat_artifact(&artifact_id)
            .map_err(store_error)?
            .ok_or_else(|| BrokerError::Execution("artifact was not found".into()))?;
        if artifact.chat_session_id != policy.session_id {
            return Err(BrokerError::MethodDenied);
        }
        let revision_id = object
            .get("revision_id")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        BrokerError::invalid("revision_id", "expected a positive revision number")
                    })
            })
            .transpose()?;
        let snapshot = self
            .state
            .store
            .read_chat_artifact(&artifact.id, revision_id)
            .map_err(store_error)?
            .ok_or_else(|| BrokerError::Execution("artifact revision was not found".into()))?;
        let artifact = snapshot.artifact;
        let revision = snapshot.revision;
        let dataset = snapshot.chart_dataset;
        match &revision.payload {
            ChatArtifactPayload::Markdown { .. } => {
                self.report_within_cutoff(policy, &artifact.id, revision.created_at_ms)?
            }
            ChatArtifactPayload::Chart { chart, .. } => {
                let dataset = dataset
                    .as_ref()
                    .ok_or_else(|| BrokerError::Execution("chart dataset was not found".into()))?;
                chart
                    .validate_dataset(dataset)
                    .map_err(|error| BrokerError::Execution(error.to_string()))?;
                self.chart_within_cutoff(policy, &artifact.id, dataset)?;
            }
            ChatArtifactPayload::Grid { grid, .. } => {
                self.grid_within_cutoff(policy, &artifact.id, grid)?
            }
        }
        self.capture
            .stage(delivery_id, |pending| {
                pending
                    .artifact_reads
                    .insert((artifact.id.clone(), revision.revision));
            })
            .await;
        match (&revision.payload, dataset) {
            (ChatArtifactPayload::Chart { chart, .. }, Some(dataset)) => Ok(json!({
                "artifact": {
                    "artifact_id": artifact.id,
                    "title": artifact.title,
                    "kind": "chart",
                    "revision": revision.revision,
                    "current_revision": artifact.current_revision,
                    "revision_digest": revision.digest,
                    "updated_at_ms": artifact.updated_at_ms,
                    "edit_token": chart_edit_token(&revision),
                },
                "chart": chart,
                "dataset": dataset.summary(),
                "message": "Chart data stays in native storage. Use chart_query only when exact rows are required."
            })),
            (ChatArtifactPayload::Grid { grid, .. }, _) => Ok(json!({
                "artifact": {
                    "artifact_id": artifact.id,
                    "title": artifact.title,
                    "kind": "grid",
                    "revision": revision.revision,
                    "current_revision": artifact.current_revision,
                    "revision_digest": revision.digest,
                    "updated_at_ms": artifact.updated_at_ms,
                    "edit_token": chart_edit_token(&revision),
                },
                "grid": grid,
                "message": "To change this table, call grid_publish with mode revise, this edit_token and the whole table; every value cites this turn's evidence again."
            })),
            _ => Ok(json!({
                "artifact": artifact,
                "revision": revision,
            })),
        }
    }

    pub(super) async fn artifact_publish(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let object = exact_object(
            &params,
            &["mode", "title", "payload"],
            &["artifact_id", "expected_revision"],
        )?;
        let mode = object
            .get("mode")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        let title = object
            .get("title")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?
            .trim()
            .to_owned();
        let payload = object.get("payload").ok_or(BrokerError::Malformed)?;
        match payload.get("kind").and_then(Value::as_str) {
            Some("chart") => {
                return Err(BrokerError::Execution(
                    "financial and analytic charts must be published with chart_publish".into(),
                ))
            }
            Some("grid") => {
                return Err(BrokerError::Execution(
                    "tables with checked cells must be published with grid_publish".into(),
                ))
            }
            _ => {}
        }
        let payload = ChatArtifactPayload::from(
            ModelArtifactPayload::deserialize(payload).map_err(|_| BrokerError::Malformed)?,
        );
        payload
            .validate()
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let source_message_id = self
            .capture
            .source_message_id
            .clone()
            .ok_or(BrokerError::MethodDenied)?;
        let timestamp = now_ms();

        let commit = match mode {
            "create" => {
                if object.contains_key("artifact_id") || object.contains_key("expected_revision") {
                    return Err(BrokerError::Malformed);
                }
                let artifact_id = new_id("artifact");
                let revision = ChatArtifactRevision::new(
                    artifact_id.clone(),
                    1,
                    payload,
                    source_message_id,
                    timestamp,
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
                ArtifactCommit {
                    artifact: ChatArtifact {
                        id: artifact_id,
                        chat_session_id: policy.session_id.clone(),
                        kind: revision.payload.kind(),
                        title,
                        current_revision: 1,
                        created_at_ms: timestamp,
                        updated_at_ms: timestamp,
                    },
                    revision,
                    datasets: Vec::new(),
                }
            }
            "revise" => {
                let artifact_id = object
                    .get("artifact_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or(BrokerError::Malformed)?;
                let expected_revision = object
                    .get("expected_revision")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or(BrokerError::Malformed)?;
                if !self
                    .capture
                    .artifact_reads
                    .lock()
                    .await
                    .contains(&(artifact_id.clone(), expected_revision))
                {
                    return Err(BrokerError::Execution(
                        "artifact revision must be read before it can be revised".into(),
                    ));
                }
                let mut artifact = self
                    .state
                    .store
                    .get_chat_artifact(&artifact_id)
                    .map_err(store_error)?
                    .ok_or_else(|| BrokerError::Execution("artifact was not found".into()))?;
                if artifact.chat_session_id != policy.session_id {
                    return Err(BrokerError::MethodDenied);
                }
                if artifact.current_revision != expected_revision {
                    return Err(BrokerError::Execution(
                        "artifact revision is stale; read the current revision".into(),
                    ));
                }
                if artifact.kind != payload.kind() {
                    return Err(BrokerError::Execution(
                        "an artifact cannot change kind across revisions".into(),
                    ));
                }
                let created_at_ms = timestamp.max(artifact.updated_at_ms.saturating_add(1));
                artifact.title = title;
                artifact.current_revision = expected_revision.checked_add(1).ok_or_else(|| {
                    BrokerError::Execution("artifact revision limit reached".into())
                })?;
                artifact.updated_at_ms = created_at_ms;
                let revision = ChatArtifactRevision::new(
                    artifact.id.clone(),
                    artifact.current_revision,
                    payload,
                    source_message_id,
                    created_at_ms,
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
                ArtifactCommit {
                    artifact,
                    revision,
                    datasets: Vec::new(),
                }
            }
            _ => return Err(BrokerError::Malformed),
        };
        self.stage_artifact(
            commit,
            delivery_id,
            "The artifact will be saved only if this Chat turn completes.",
        )
        .await
    }

    pub(super) async fn chart_query(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        let request: ChartQueryRequest = serde_json::from_value(params)
            .map_err(|_| BrokerError::invalid("chart_query", "artifact_id, revision, optional offset, limit and columns matching the tool schema"))?;
        if request.limit == 0 || request.limit > MAX_CHART_QUERY_ROWS {
            return Err(BrokerError::invalid(
                "limit",
                "expected an integer from 1 to 200",
            ));
        }
        let artifact = self
            .state
            .store
            .get_chat_artifact(&request.artifact_id)
            .map_err(store_error)?
            .ok_or_else(|| BrokerError::Execution("artifact was not found".into()))?;
        if artifact.chat_session_id != policy.session_id || artifact.kind != ChatArtifactKind::Chart
        {
            return Err(BrokerError::MethodDenied);
        }
        if !self
            .capture
            .artifact_reads
            .lock()
            .await
            .contains(&(artifact.id.clone(), request.revision))
        {
            return Err(BrokerError::Execution(
                "the exact chart revision must be read before its rows can be queried".into(),
            ));
        }
        let snapshot = self
            .state
            .store
            .read_chat_artifact(&artifact.id, Some(request.revision))
            .map_err(store_error)?
            .ok_or_else(|| BrokerError::Execution("artifact revision was not found".into()))?;
        let revision = snapshot.revision;
        let ChatArtifactPayload::Chart { chart, .. } = revision.payload else {
            return Err(BrokerError::Execution("artifact is not a chart".into()));
        };
        let dataset = snapshot
            .chart_dataset
            .ok_or_else(|| BrokerError::Execution("chart dataset was not found".into()))?;
        chart
            .validate_dataset(&dataset)
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        self.chart_within_cutoff(policy, &artifact.id, &dataset)?;
        if request.offset > dataset.rows.len() {
            return Err(BrokerError::invalid(
                "offset",
                "exceeds total_rows; use the previous next_offset or start at 0",
            ));
        }
        let requested_end = request
            .offset
            .saturating_add(request.limit)
            .min(dataset.rows.len());
        let indexes = match request.columns.as_ref() {
            None => (0..dataset.columns.len()).collect::<Vec<_>>(),
            Some(columns) => {
                if columns.is_empty() || columns.len() > dataset.columns.len() {
                    return Err(BrokerError::invalid(
                        "columns",
                        "expected a nonempty selection of dataset column ids",
                    ));
                }
                let mut seen = BTreeSet::new();
                columns
                    .iter()
                    .map(|id| {
                        if !seen.insert(id) {
                            return Err(BrokerError::invalid(
                                "columns",
                                "duplicate column id; request each column once",
                            ));
                        }
                        dataset
                            .columns
                            .iter()
                            .position(|column| &column.id == id)
                            .ok_or_else(|| {
                                BrokerError::invalid(
                                    "columns",
                                    "unknown column id; use the ids returned by artifact_read",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        let rows = dataset.rows[request.offset..requested_end]
            .iter()
            .map(|row| {
                indexes
                    .iter()
                    .map(|index| row[*index].clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let count = bounded_chart_query_end(&rows, 0, rows.len())?;
        let end = request.offset + count;
        let columns = indexes
            .iter()
            .map(|index| &dataset.columns[*index])
            .collect::<Vec<_>>();
        Ok(json!({
            "artifact_id": artifact.id,
            "revision": revision.revision,
            "revision_digest": revision.digest,
            "dataset_id": dataset.id,
            "dataset_digest": dataset.digest,
            "columns": columns,
            "offset": request.offset,
            "rows": &rows[..count],
            "next_offset": (end < dataset.rows.len()).then_some(end),
            "total_rows": dataset.rows.len(),
        }))
    }

    async fn materialize_chart_dataset(
        &self,
        policy: &ToolPolicy,
        input: crate::chat::chart::ChartDatasetInput,
    ) -> Result<crate::chat::chart::ChartDataset, BrokerError> {
        match input {
            crate::chat::chart::ChartDatasetInput::FromDataset(envelope) => {
                let selected = envelope.from_dataset;
                let revision = selected.revision;
                // A chart drawn under a cutoff may only use rows known by then.
                self.ensure_dataset_within_cutoff(policy, &selected.dataset_id, revision)?;
                let (reference, payload, _) = self
                    .state
                    .store
                    .dataset(&self.loupist_id, &selected.dataset_id, revision)
                    .map_err(command_error)?;
                crate::chat::chart::dataset_from_dataset_selection(
                    new_id("dataset"),
                    selected,
                    &payload,
                    reference.digest,
                )
                .map_err(chart_dataset_error)
            }
            crate::chat::chart::ChartDatasetInput::FromResult(envelope) => {
                let result = self
                    .capture
                    .run_result(&envelope.from_result.result_ref)
                    .await
                    .ok_or_else(|| {
                        BrokerError::Execution(
                            "dataset.from_result.result_ref must name a delivered result from this turn".into(),
                        ).with_next("Call run_results_list and use a returned result_ref, or read the source again.")
                    })?;
                crate::chat::chart::dataset_from_result_selection(
                    new_id("dataset"),
                    envelope.from_result,
                    &result.payload,
                    chart_result_receipt(&result),
                )
                .map_err(chart_dataset_error)
            }
            crate::chat::chart::ChartDatasetInput::Inline(envelope) => {
                let mut receipts = Vec::with_capacity(envelope.inline.upstream_result_refs.len());
                for result_ref in &envelope.inline.upstream_result_refs {
                    let result = self.capture.run_result(result_ref).await.ok_or_else(|| {
                        BrokerError::Execution(
                            "chart upstream_result_refs must name delivered results from this turn"
                                .into(),
                        )
                    })?;
                    receipts.push(chart_result_receipt(&result));
                }
                crate::chat::chart::dataset_from_inline(
                    new_id("dataset"),
                    envelope.inline,
                    receipts,
                )
                .map_err(chart_dataset_error)
            }
        }
    }

    pub(super) async fn chart_publish(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        exact_object(
            &params,
            &["mode", "title"],
            &[
                "dataset",
                "view",
                "studies",
                "custom_series",
                "drawings",
                "note",
                "artifact_id",
                "edit_token",
            ],
        )?;
        if let Some(view) = params.get("view") {
            validate_view_input(view)?;
        }
        let request = ChartPublishRequest {
            mode: chart_argument(&params, "mode")?,
            title: chart_argument(&params, "title")?,
            view: chart_argument(&params, "view")?,
            dataset: chart_argument(&params, "dataset")?,
            studies: chart_argument(&params, "studies")?,
            custom_series: chart_argument(&params, "custom_series")?,
            drawings: chart_argument(&params, "drawings")?,
            note: chart_argument(&params, "note")?,
            artifact_id: chart_argument(&params, "artifact_id")?,
            edit_token: chart_argument(&params, "edit_token")?,
        };
        let title = request.title.trim();
        if title.is_empty() || title.len() > 200 {
            return Err(BrokerError::invalid(
                "title",
                "expected 1–200 bytes of nonblank text",
            ));
        }
        let title = title.to_owned();
        let timestamp = now_ms();
        let source_message_id = self
            .capture
            .source_message_id
            .clone()
            .ok_or(BrokerError::MethodDenied)?;
        let commit = match request.mode {
            ChartPublishMode::Create => {
                if request.artifact_id.is_some() || request.edit_token.is_some() {
                    return Err(BrokerError::invalid(
                        "mode",
                        "create does not accept artifact_id or edit_token; omit them or use revise",
                    ));
                }
                let view = request
                    .view
                    .ok_or_else(|| BrokerError::invalid("view", "required when mode is create"))?;
                let dataset = self
                    .materialize_chart_dataset(policy, request.dataset.ok_or_else(|| BrokerError::invalid("dataset", "required when mode is create; supply from_dataset, from_result, or inline"))?)
                    .await?;
                let document = crate::chat::chart::ChartDocument {
                    dataset_id: dataset.id.clone(),
                    dataset_digest: dataset.digest.clone(),
                    view,
                    studies: request.studies.unwrap_or_default(),
                    custom_series: request.custom_series.unwrap_or_default(),
                    drawings: request.drawings.unwrap_or_default(),
                    note: normalize_chart_note(request.note),
                };
                document
                    .validate()
                    .map_err(|error| BrokerError::Execution(error.to_string()))?;
                validate_view_columns(&document.view, &dataset)?;
                let artifact_id = new_id("artifact");
                let revision = ChatArtifactRevision::new(
                    artifact_id.clone(),
                    1,
                    ChatArtifactPayload::Chart {
                        schema: crate::chat::chart::CHART_SCHEMA.into(),
                        chart: Box::new(document),
                    },
                    source_message_id,
                    timestamp,
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
                ArtifactCommit {
                    artifact: ChatArtifact {
                        id: artifact_id,
                        chat_session_id: policy.session_id.clone(),
                        kind: ChatArtifactKind::Chart,
                        title,
                        current_revision: 1,
                        created_at_ms: timestamp,
                        updated_at_ms: timestamp,
                    },
                    revision,
                    datasets: vec![dataset],
                }
            }
            ChartPublishMode::Revise => {
                let artifact_id = request.artifact_id.as_deref().ok_or_else(|| {
                    BrokerError::invalid(
                        "artifact_id",
                        "required when mode is revise; use artifact_read first",
                    )
                })?;
                let edit_token = request.edit_token.as_deref().ok_or_else(|| {
                    BrokerError::invalid(
                        "edit_token",
                        "required when mode is revise; use the token returned by artifact_read",
                    )
                })?;
                let mut artifact = self
                    .state
                    .store
                    .get_chat_artifact(artifact_id)
                    .map_err(store_error)?
                    .ok_or_else(|| BrokerError::Execution("artifact was not found".into()))?;
                if artifact.chat_session_id != policy.session_id
                    || artifact.kind != ChatArtifactKind::Chart
                {
                    return Err(BrokerError::MethodDenied);
                }
                let current = self
                    .state
                    .store
                    .get_chat_artifact_current(&artifact.id)
                    .map_err(store_error)?
                    .ok_or_else(|| {
                        BrokerError::Execution("artifact content was not found".into())
                    })?;
                if chart_edit_token(&current) != edit_token
                    || !self
                        .capture
                        .artifact_reads
                        .lock()
                        .await
                        .contains(&(artifact.id.clone(), current.revision))
                {
                    return Err(BrokerError::Execution(
                        "edit_token does not match an acknowledged current chart read".into(),
                    ).with_next("Call artifact_read for this artifact_id, then revise using its returned edit_token."));
                }
                let ChatArtifactPayload::Chart {
                    chart: current_chart,
                    ..
                } = current.payload
                else {
                    return Err(BrokerError::Execution("artifact is not a chart".into()));
                };
                let dataset = if let Some(dataset) = request.dataset {
                    self.materialize_chart_dataset(policy, dataset).await?
                } else {
                    self.state
                        .store
                        .get_chart_dataset(&current_chart.dataset_id)
                        .map_err(store_error)?
                        .ok_or_else(|| {
                            BrokerError::Execution("chart dataset was not found".into())
                        })?
                };
                let document = crate::chat::chart::ChartDocument {
                    dataset_id: dataset.id.clone(),
                    dataset_digest: dataset.digest.clone(),
                    view: request.view.unwrap_or(current_chart.view),
                    studies: request.studies.unwrap_or(current_chart.studies),
                    custom_series: request.custom_series.unwrap_or(current_chart.custom_series),
                    drawings: request.drawings.unwrap_or(current_chart.drawings),
                    note: match request.note {
                        Some(note) => normalize_chart_note(Some(note)),
                        None => current_chart.note,
                    },
                };
                document
                    .validate()
                    .map_err(|error| BrokerError::Execution(error.to_string()))?;
                validate_view_columns(&document.view, &dataset)?;
                let created_at_ms = timestamp.max(artifact.updated_at_ms.saturating_add(1));
                artifact.title = title;
                artifact.current_revision =
                    artifact.current_revision.checked_add(1).ok_or_else(|| {
                        BrokerError::Execution("artifact revision limit reached".into())
                    })?;
                artifact.updated_at_ms = created_at_ms;
                let revision = ChatArtifactRevision::new(
                    artifact.id.clone(),
                    artifact.current_revision,
                    ChatArtifactPayload::Chart {
                        schema: crate::chat::chart::CHART_SCHEMA.into(),
                        chart: Box::new(document),
                    },
                    source_message_id,
                    created_at_ms,
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
                ArtifactCommit {
                    artifact,
                    revision,
                    datasets: vec![dataset],
                }
            }
        };
        self.stage_artifact(
            commit,
            delivery_id,
            "The chart will be saved only if this Chat turn completes.",
        )
        .await
    }

    pub(super) async fn stage_artifact(
        &self,
        commit: ArtifactCommit,
        delivery_id: &str,
        message: &'static str,
    ) -> Result<Value, BrokerError> {
        commit
            .validate()
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let reference = commit.revision.artifact_ref(commit.artifact.title.clone());
        let mut pending = self.capture.pending_deliveries.lock().await;
        let committed = self.capture.artifacts.lock().await;
        let mut published_ids = BTreeSet::new();
        for existing in committed.iter() {
            published_ids.insert(existing.artifact.id.clone());
        }
        for pending_capture in pending.values() {
            for existing in &pending_capture.artifacts {
                published_ids.insert(existing.artifact.id.clone());
            }
        }
        if published_ids.contains(&commit.artifact.id) {
            return Err(BrokerError::Execution(
                "a Chat turn may not publish the same artifact twice".into(),
            ));
        }
        if published_ids.len() >= crate::chat::artifacts::MAX_CHAT_TURN_ARTIFACTS {
            return Err(BrokerError::Execution(format!(
                "a Chat turn may publish at most {} artifact revisions",
                crate::chat::artifacts::MAX_CHAT_TURN_ARTIFACTS
            )));
        }
        drop(committed);
        pending
            .entry(delivery_id.to_owned())
            .or_default()
            .artifacts
            .push(commit);
        Ok(json!({
            "status": "staged",
            "artifact": reference,
            "message": message,
        }))
    }
}

const CHART_RESHAPE_HINT: &str = "If the selected result is not chart-shaped, export the dataset, transform it with run_python, then use dataset_import and publish from its dataset_ref.";

fn chart_dataset_error(error: crate::chat::artifacts::ChatArtifactError) -> BrokerError {
    BrokerError::Execution(error.to_string()).with_next(CHART_RESHAPE_HINT)
}

fn chart_result_receipt(result: &RunResult) -> crate::chat::chart::ChartResultReceipt {
    crate::chat::chart::ChartResultReceipt {
        result_ref: result.result_ref.clone(),
        runtime_id: result.producer.runtime_id.clone(),
        tool_name: result.producer.tool_name.clone(),
        provider: result.producer.provider.clone(),
        request_digest: result.request_digest.clone(),
        response_digest: result.response_digest.clone(),
        retrieved_at: result.retrieved_at.clone(),
        warnings: result.warnings.clone(),
        upstream_result_refs: result.upstream_result_refs.clone(),
    }
}
