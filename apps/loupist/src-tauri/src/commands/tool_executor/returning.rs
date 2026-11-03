//! Third-party text that comes back from retained material in a later turn
//! (B87). A saved Source a publisher or a search wrote, a custom connection's
//! saved result, and a dataset retained from a text route return delimited and
//! raise the untrusted-text tier, as they did when they first arrived. The
//! user's own files and statements, a shared pack's Sources (delimited by
//! `sharing/delivery.rs`) and reviewed numeric data return as they are.
use super::*;
use crate::custom_connections::taint::{retained_trusted, Returning};

impl AppToolExecutor {
    /// The third-party text this result brings back, if any. Called before the
    /// result is delivered, so the tier rises first.
    pub(super) fn returning_untrusted(
        &self,
        method: ToolMethod,
        result: &Value,
    ) -> Result<Option<Returning>, BrokerError> {
        match method {
            ToolMethod::ChartQuery | ToolMethod::ArtifactRead => {
                self.returning_artifact(method, result)
            }
            ToolMethod::AssetPageRead => {
                let fields = ["rows", "watchAlerts"]
                    .into_iter()
                    .filter(|key| {
                        result
                            .get(*key)
                            .and_then(Value::as_array)
                            .is_some_and(|rows| !rows.is_empty())
                    })
                    .map(|key| (format!("/{key}"), "asset watch observations".to_owned()))
                    .collect::<BTreeMap<_, _>>();
                Ok((!fields.is_empty()).then_some(Returning::Fields(fields)))
            }
            ToolMethod::ConversationMemory | ToolMethod::ChatHistory => {
                self.returning_messages(method, result)
            }
            ToolMethod::WorldRead if result["scope"] == "world" => {
                let view = result
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .ok_or(BrokerError::Malformed)?;
                Ok(self
                    .third_party_world(result, Some(&view))?
                    .map(Returning::Whole))
            }
            ToolMethod::DatasetDescribe | ToolMethod::DatasetQuery | ToolMethod::DatasetImport => {
                let reference = result
                    .get(if method == ToolMethod::DatasetImport {
                        "dataset_ref"
                    } else {
                        "ref"
                    })
                    .ok_or(BrokerError::Malformed)?;
                let (Some(dataset_id), Some(revision)) = (
                    reference.get("dataset_id").and_then(Value::as_str),
                    reference
                        .get("revision")
                        .and_then(Value::as_u64)
                        .and_then(|revision| u32::try_from(revision).ok()),
                ) else {
                    return Err(BrokerError::Malformed);
                };
                let metadata = self
                    .state
                    .store
                    .dataset_metadata(&self.loupist_id, dataset_id, revision)
                    .map_err(|error| BrokerError::Execution(error.to_string()))?;
                Ok((!retained_trusted(&metadata)).then(|| {
                    let label = dataset_label(dataset_id, &metadata);
                    if method == ToolMethod::DatasetImport {
                        Returning::Fields(BTreeMap::from([("/data_summary".into(), label)]))
                    } else {
                        Returning::Whole(label)
                    }
                }))
            }
            ToolMethod::WorkspaceCapture if result["trust"] == "third_party_text" => {
                Ok(Some(Returning::Fields(BTreeMap::from([(
                    "/content".into(),
                    "app-exported text".into(),
                )]))))
            }
            ToolMethod::WorldRead
                if result.get("scope").and_then(Value::as_str) == Some("source") =>
            {
                let span = result.pointer("/source/spanId").and_then(Value::as_str);
                let (Some(record_id), Some(span_id)) =
                    (result.get("recordId").and_then(Value::as_str), span)
                else {
                    return Ok(None);
                };
                let revision_id = result.get("revisionId").and_then(Value::as_str);
                Ok(self
                    .third_party_source(record_id, revision_id, span_id)?
                    .map(Returning::Whole))
            }
            ToolMethod::WorldSearch => {
                let mut sources = BTreeMap::new();
                let hits = result.get("hits").and_then(Value::as_array);
                let mut checked = BTreeSet::new();
                for hit in hits.into_iter().flatten() {
                    if !matches!(
                        hit.get("scope").and_then(Value::as_str),
                        Some("source" | "world")
                    ) {
                        continue;
                    }
                    let text = |key: &str| hit.get(key).and_then(Value::as_str);
                    let Some(record_id) = text("recordId") else {
                        continue;
                    };
                    // A hit on the Source's title carries no span: judge the
                    // whole Source. Hits are wrapped per Source.
                    let span_id = text("spanId").unwrap_or_default();
                    if !checked.insert((record_id, text("objectId").unwrap_or(span_id)))
                        || sources.contains_key(record_id)
                    {
                        continue;
                    }
                    let label = if hit["scope"] == "world" {
                        self.third_party_world(hit, hit.get("view"))?
                    } else {
                        self.third_party_source(record_id, text("revisionId"), span_id)?
                    };
                    if let Some(label) = label {
                        sources.insert(record_id.to_owned(), label);
                    }
                }
                Ok((!sources.is_empty()).then_some(Returning::Hits(sources)))
            }
            _ => Ok(None),
        }
    }

    fn returning_artifact(
        &self,
        method: ToolMethod,
        result: &Value,
    ) -> Result<Option<Returning>, BrokerError> {
        let dataset_id = if method == ToolMethod::ChartQuery {
            result.get("dataset_id")
        } else {
            result.pointer("/chart/dataset_id")
        }
        .and_then(Value::as_str);
        let mut untrusted = false;
        if let Some(id) = dataset_id {
            let dataset = self
                .state
                .store
                .get_chart_dataset(id)
                .map_err(|e| BrokerError::Execution(e.to_string()))?
                .ok_or(BrokerError::Malformed)?;
            if let crate::chat::chart::ChartDatasetLineage::FromDataset {
                dataset_id,
                revision,
                ..
            } = &dataset.lineage
            {
                // The chart keeps its rows after its input revision is replaced.
                // An unavailable original class must never become trusted.
                untrusted = !self
                    .state
                    .store
                    .dataset_metadata(&self.loupist_id, dataset_id, *revision)
                    .is_ok_and(|metadata| retained_trusted(&metadata));
            }
        }
        let artifact_id = result
            .get("artifact_id")
            .or_else(|| result.pointer("/artifact/artifact_id"))
            .or_else(|| result.pointer("/artifact/id"))
            .and_then(Value::as_str);
        if let Some(id) = artifact_id {
            if let Some(artifact) = self
                .state
                .store
                .get_chat_artifact(id)
                .map_err(|e| BrokerError::Execution(e.to_string()))?
            {
                if let Some(revision) = self
                    .state
                    .store
                    .get_chat_artifact_current(id)
                    .map_err(|e| BrokerError::Execution(e.to_string()))?
                {
                    untrusted |= self.message_untrusted(
                        &artifact.chat_session_id,
                        &revision.source_message_id,
                    )?;
                }
            }
        }
        if !untrusted {
            return Ok(None);
        }
        let pointers: &[&str] = if method == ToolMethod::ChartQuery {
            &["/columns", "/rows"]
        } else {
            &[
                "/artifact/title",
                "/chart",
                "/dataset",
                "/grid",
                "/revision/payload",
            ]
        };
        Ok(Some(Returning::Fields(
            pointers
                .iter()
                .filter(|p| result.pointer(p).is_some())
                .map(|p| {
                    (
                        (*p).to_owned(),
                        "saved artifact from third-party text".into(),
                    )
                })
                .collect(),
        )))
    }

    fn message_untrusted(&self, chat_id: &str, message_id: &str) -> Result<bool, BrokerError> {
        let chat = self
            .state
            .store
            .get_chat(chat_id)
            .map_err(|e| BrokerError::Execution(e.to_string()))?;
        Ok(chat
            .filter(|chat| chat.loupist_id == self.loupist_id)
            .is_some_and(|chat| {
                chat.messages
                    .iter()
                    .find(|message| message.id == message_id)
                    .is_some_and(crate::domain::ChatMessage::has_untrusted_text)
            }))
    }

    fn returning_messages(
        &self,
        method: ToolMethod,
        result: &Value,
    ) -> Result<Option<Returning>, BrokerError> {
        let mut fields = BTreeMap::new();
        let mut inspect = |entry: &Value, prefix: &str| -> Result<(), BrokerError> {
            let (chat, message, keys) = if method == ToolMethod::ChatHistory {
                (
                    self.capture.bound_chat_id.as_deref(),
                    entry.get("message_id").and_then(Value::as_str),
                    &["content"][..],
                )
            } else {
                (
                    entry.get("chatId").and_then(Value::as_str),
                    entry.get("messageId").and_then(Value::as_str),
                    &["text", "snippet"][..],
                )
            };
            if let (Some(chat), Some(message)) = (chat, message) {
                if self.message_untrusted(chat, message)? {
                    for key in keys {
                        if entry.get(*key).is_some() {
                            fields.insert(
                                format!("{prefix}/{key}"),
                                format!("earlier reply {message}"),
                            );
                        }
                    }
                }
            }
            Ok(())
        };
        inspect(result, "")?;
        for (index, entry) in result
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            inspect(entry, &format!("/messages/{index}"))?;
        }
        Ok((!fields.is_empty()).then_some(Returning::Fields(fields)))
    }

    fn third_party_world(
        &self,
        result: &Value,
        view: Option<&Value>,
    ) -> Result<Option<String>, BrokerError> {
        let (targets, unresolved) = crate::commands::knowledge::returning_world_sources(
            &self.state,
            &self.loupist_id,
            result,
            view,
        )
        .map_err(|e| BrokerError::Execution(e.to_string()))?;
        if unresolved {
            return Ok(Some("World claim with unavailable Source".into()));
        }
        for target in targets {
            if let Some(label) = self.third_party_source(
                &target.record_id,
                Some(&target.revision_id),
                &target.span_id,
            )? {
                return Ok(Some(label));
            }
        }
        Ok(None)
    }

    /// The source label of a Source span (every span, for an empty `span_id`)
    /// someone else wrote: a publisher, a search provider or a custom
    /// connection. A saved provider payload whose dataset is recorded as
    /// numeric is data, not text; a lookup that cannot tell fails closed. The
    /// Source's own site does not become a site this turn has seen: its author
    /// may own it, and the user did not ask to open it now.
    fn third_party_source(
        &self,
        record_id: &str,
        revision_id: Option<&str>,
        span_id: &str,
    ) -> Result<Option<String>, BrokerError> {
        let spans = crate::commands::knowledge::source_span_authorship(
            &self.state,
            &self.loupist_id,
            record_id,
            revision_id,
            span_id,
        )
        .map_err(|error| BrokerError::Execution(error.to_string()))?;
        if spans.is_empty() {
            return Ok(Some(format!("saved Source {record_id}")));
        }
        let Some(span) = spans
            .into_iter()
            .find(|span| span.untrusted(&self.state, &self.loupist_id))
        else {
            return Ok(None);
        };
        let locator = span
            .origin_locator
            .filter(|locator| locator.starts_with("http://") || locator.starts_with("https://"));
        Ok(Some(match locator {
            Some(locator) => format!("saved Source: {locator}"),
            None => format!("saved Source: {}", span.title),
        }))
    }
}

/// Lens bodies, Journal entries and notes are the user's own words: a URL that
/// carries them is refused after third-party text (B88). Principles and the
/// standing perspective are protected when the turn's context is built.
pub(super) fn protect_private_text(
    taint: &crate::custom_connections::taint::TurnTaint,
    method: ToolMethod,
    result: &Value,
) {
    fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
        match value {
            Value::String(text) => out.push(text),
            Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
            Value::Object(map) => map.values().for_each(|item| strings(item, out)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    match method {
        ToolMethod::LensRead => strings(&result["text"], &mut texts),
        ToolMethod::ConversationMemory => {
            if result.get("scope").and_then(Value::as_str) == Some("research") {
                strings(&result["text"], &mut texts);
            }
            strings(&result["note"], &mut texts);
        }
        _ => {}
    }
    for text in texts {
        taint.protect_private_text(text);
    }
}

/// `dataset {id} ({tool})`, naming the route that produced it when recorded.
fn dataset_label(dataset_id: &str, metadata: &Value) -> String {
    let producer = metadata
        .pointer("/request/tool_name")
        .or_else(|| metadata.get("producer"))
        .or_else(|| metadata.get("origin"))
        .and_then(Value::as_str)
        .unwrap_or("unknown origin");
    format!("dataset {dataset_id} ({producer})")
}
