use crate::{
    broker::tools::TOOL_CATALOG,
    domain::{
        ChatMessage, ChatMessageStatus, ChatToolTrace, ChatToolTraceEntry, MemoryAccess,
        ToolTraceOutcome,
    },
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Instant};

#[path = "progress.rs"]
mod progress;

#[derive(Default)]
pub(crate) struct PiTrace {
    calls: Vec<ToolCall>,
    starts: BTreeMap<String, (usize, Instant)>,
    native_reads: BTreeMap<String, Value>,
    pub usage_seen: bool,
    round_usage: Vec<RoundUsage>,
    compaction_requests: u64,
    error: Option<Value>,
    /// Pi failed to start before the prompt, so the turn reached no model.
    startup_failed: bool,
    /// The host's own reason for a failed turn, such as Pi's exit code.
    failure_reason: Option<String>,
    /// Each assistant message's streamed text, which times the progress notes.
    streamed_text: Vec<progress::StreamedText>,
}

/// Each assistant message_end is one model round, including retries and errors.
/// Reasoning is a provider-reported subset of output, never an extra token cost.
#[derive(Serialize)]
struct RoundUsage {
    output: Option<u64>,
    reasoning: Option<u64>,
}

/// One evaluation call, independent of the host's smaller display trace budget.
#[derive(Clone, Serialize)]
struct ToolCall {
    call_id: Option<String>,
    name: String,
    component: Option<String>,
    args: Value,
    duration_ms: Option<u64>,
    outcome: Option<ToolTraceOutcome>,
    error_code: Option<Value>,
    error_message: Option<String>,
    /// The host's typed reason for a refusal (`needs_approval`,
    /// `connection_dont_save`), when the refusal carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_reason: Option<String>,
    result_bytes: Option<u64>,
    /// Actual Pi-delivered response, separate from the host display trace.
    result: Option<Value>,
    result_redacted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_authorization: Option<Value>,
}

impl ToolCall {
    fn from_host(entry: &ChatToolTraceEntry) -> Self {
        let error = entry.error.as_ref();
        Self {
            call_id: entry.call_id.clone(),
            name: entry.name.clone(),
            component: entry.component.clone(),
            args: bounded_args(entry.args.as_ref().unwrap_or(&Value::Null)),
            duration_ms: Some(entry.duration_ms),
            outcome: Some(entry.outcome),
            error_code: error
                .and_then(|e| e.get("code"))
                .filter(|code| !code.is_null())
                .map(bounded_json),
            error_message: error
                .and_then(|e| e["message"].as_str())
                .map(safe_error_text),
            error_reason: error.and_then(refusal_reason),
            result_bytes: entry.result_bytes,
            result: None,
            result_redacted: None,
            native_authorization: None,
        }
    }

    fn enrich_from_host(&mut self, host: Self) {
        self.component = host.component.or(self.component.take());
        self.duration_ms = host.duration_ms.or(self.duration_ms);
        self.error_code = host.error_code.or(self.error_code.take());
        self.error_message = host.error_message.or(self.error_message.take());
        self.error_reason = host.error_reason.or(self.error_reason.take());
        self.result_bytes = host.result_bytes.or(self.result_bytes);
        // Pi records submitted arguments before the host applies its display cap.
        if self.args.is_null() {
            self.args = host.args;
        }
        self.outcome = self.outcome.or(host.outcome);
    }
}

impl PiTrace {
    /// The Pi failure category, such as `startup_timeout`.
    pub fn error(&self) -> Option<&Value> {
        self.error.as_ref()
    }

    /// Marks a record whose Pi never started (`startup_failed: true`): an
    /// infrastructure failure with no model call, which `--resume` reruns.
    pub fn record_startup_failure(&self, record: &mut Value) {
        if self.startup_failed {
            record["startup_failed"] = json!(true);
        }
        if let Some(reason) = &self.failure_reason {
            record["failure_reason"] = json!(reason);
        }
    }

    pub fn record_round_usage(&self, record: &mut Value) {
        record["model_rounds"] = json!(self.round_usage.len());
        record["compaction_requests"] = json!(self.compaction_requests);
        record["round_usage"] = json!(self.round_usage);
    }

    /// Only the host's admitted native read supplies this evidence, never Pi.
    pub fn native_read_authorized(&mut self, call_id: &str, authorization: Value) {
        if !call_id.is_empty() {
            self.native_reads
                .insert(call_id.into(), bounded_json(&authorization));
        }
    }

    pub fn observe(&mut self, event: &Value) {
        if let Some(diagnostic) =
            crate::pi::e2e_rpc_diagnostic(event).filter(|d| d.get("category").is_some())
        {
            self.error = Some(json!(diagnostic.to_string()));
        }
        let assistant = event.pointer("/message/role").and_then(Value::as_str) == Some("assistant");
        match event["type"].as_str() {
            Some("message_start") if assistant => {
                self.streamed_text.push(progress::StreamedText {
                    at_ms: None,
                    text: String::new(),
                });
            }
            Some("message_update") => {
                let delta = event
                    .pointer("/assistantMessageEvent/delta")
                    .and_then(Value::as_str)
                    .filter(|delta| !delta.is_empty());
                if let (Some(delta), Some(message), Some("text_delta")) = (
                    delta,
                    self.streamed_text.last_mut(),
                    event
                        .pointer("/assistantMessageEvent/type")
                        .and_then(Value::as_str),
                ) {
                    message
                        .at_ms
                        .get_or_insert_with(|| chrono::Utc::now().timestamp_millis());
                    message.text.push_str(delta);
                }
            }
            Some("message_end") if assistant => {
                let usage = event.pointer("/message/usage");
                self.usage_seen |= usage.is_some();
                self.round_usage.push(RoundUsage {
                    output: usage.and_then(|usage| usage["output"].as_u64()),
                    reasoning: usage.and_then(|usage| usage["reasoning"].as_u64()),
                });
            }
            Some("compaction_end") if crate::runs::log::compaction_usage(event).is_some() => {
                self.compaction_requests += 1;
                self.usage_seen = true;
            }
            Some("eval_error") => {
                self.startup_failed |= event["phase"] == "launch";
                if self.error.is_none() {
                    self.error = Some(bounded_json(&event["message"]));
                }
                if self.failure_reason.is_none() {
                    self.failure_reason = event["reason"].as_str().map(safe_error_text);
                }
            }
            Some("tool_execution_start") => {
                let Some(id) = event["toolCallId"].as_str() else {
                    return;
                };
                let name = event["toolName"].as_str().unwrap_or("unknown");
                let component = TOOL_CATALOG
                    .tools
                    .iter()
                    .find(|tool| tool.name == name)
                    .and_then(|tool| tool.component.clone());
                self.starts
                    .insert(id.into(), (self.calls.len(), Instant::now()));
                self.calls.push(ToolCall {
                    call_id: Some(id.into()),
                    name: name.into(),
                    component,
                    args: bounded_args(&event["args"]),
                    duration_ms: None,
                    outcome: None,
                    error_code: None,
                    error_message: None,
                    error_reason: None,
                    result_bytes: None,
                    result: None,
                    result_redacted: None,
                    native_authorization: None,
                });
            }
            Some("tool_execution_end") => {
                let Some((index, start)) = event["toolCallId"]
                    .as_str()
                    .and_then(|id| self.starts.remove(id))
                else {
                    return;
                };
                let call = &mut self.calls[index];
                let failed = event["isError"].as_bool();
                call.duration_ms = Some(start.elapsed().as_millis() as u64);
                call.outcome = failed.map(|failed| {
                    if failed {
                        ToolTraceOutcome::Error
                    } else {
                        ToolTraceOutcome::Ok
                    }
                });
                if let Some(result) = event.get("result") {
                    let redacted = redact(result);
                    call.result_redacted = Some(&redacted != result);
                    call.result = Some(bound_redacted(redacted, MAX_EVAL_RESULT_BYTES));
                    call.result_bytes = serde_json::to_vec(result)
                        .ok()
                        .map(|bytes| bytes.len() as u64);
                    if failed == Some(true) {
                        (call.error_code, call.error_message) = tool_error(result);
                        call.error_reason = refusal_reason(result);
                    }
                }
            }
            _ => {}
        }
    }
}

fn tool_error(result: &Value) -> (Option<Value>, Option<String>) {
    let text = result
        .get("error_message")
        .or_else(|| result.get("message"))
        .and_then(Value::as_str)
        .or_else(|| result.as_str())
        .map(str::to_owned)
        .or_else(|| {
            result["content"].as_array().map(|content| {
                content
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .filter(|text| !text.is_empty());
    let code = result
        .get("error_code")
        .or_else(|| result.get("code"))
        .filter(|code| !code.is_null())
        .map(bounded_json)
        .or_else(|| match text.as_deref() {
            Some(text) if text.starts_with("Tool \"") && text.contains("\" not found") => {
                Some("tool_not_found".into())
            }
            Some(text) if text.starts_with("Validation failed for tool \"") => {
                Some("tool_validation_failed".into())
            }
            _ => None,
        });
    (code, text.as_deref().map(safe_error_text))
}

/// A refusal's typed reason from the host's error body, as the broker writes it
/// and as a structured tool result delivers it. Free text never becomes one.
fn refusal_reason(body: &Value) -> Option<String> {
    let reason = body.get("reason").and_then(Value::as_str)?;
    let typed = !reason.is_empty()
        && reason.len() <= 128
        && reason
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    typed.then(|| reason.to_owned())
}

fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let lower = key.to_lowercase();
                    let compound: String =
                        lower.chars().filter(char::is_ascii_alphanumeric).collect();
                    let sensitive = [
                        "secret",
                        "token",
                        "password",
                        "passwd",
                        "authorization",
                        "credential",
                        "cookie",
                    ]
                    .iter()
                    .any(|word| lower.contains(word))
                        || [
                            "apikey",
                            "accesskey",
                            "privatekey",
                            "clientkey",
                            "sessionkey",
                            "authkey",
                            "signingkey",
                            "appkey",
                            "crtfckey",
                        ]
                        .iter()
                        .any(|word| compound.contains(word))
                        || (lower == "key"
                            && value.as_str().is_some_and(|text| {
                                let encoded = text.len() >= 20
                                    && text
                                        .bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b));
                                // A bare JWT has three base64url segments; the
                                // text detector also handles prefixed Bearer tokens.
                                let parts: Vec<_> = text.split('.').collect();
                                let jwt = text.len() >= 20
                                    && text.starts_with("eyJ")
                                    && parts.len() == 3
                                    && parts.iter().all(|part| {
                                        !part.is_empty()
                                            && part.bytes().all(|b| {
                                                b.is_ascii_alphanumeric() || b"-_=".contains(&b)
                                            })
                                    });
                                encoded || jwt
                            }));
                    (
                        key.clone(),
                        if sensitive {
                            json!("[redacted]")
                        } else {
                            redact(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact).collect()),
        Value::String(text) if crate::util::credential_text::contains_credential_text(text) => {
            json!("[redacted]")
        }
        _ => value.clone(),
    }
}
pub(crate) fn bounded_json(value: &Value) -> Value {
    bounded_value(value, MAX_DIAGNOSTIC_BYTES)
}

/// Keep a saved task's identity and its prompt up to the 8 KiB a saved request
/// may hold, with the same redaction as other diagnostics. A longer prompt keeps
/// the hash of the whole text and says it was cut.
pub(super) fn scheduled_task(task: &crate::scheduled_tasks::types::Task) -> Value {
    let mut snapshot = redact(&json!({
        "task_id": task.id, "title": task.title, "status": task.status, "revision": task.revision,
        "schedule": task.schedule, "target": task.target, "session_thread_id": task.session_thread_id,
        "model_profile_id": task.model_profile_id, "thinking_level": task.thinking_level,
        "use_memory": task.use_memory, "update_memory": task.update_memory,
        "next_run_at_ms": task.next_run_at_ms, "awaiting_confirmation": task.awaiting_confirmation,
        "prompt": task.prompt,
    }));
    if let Some(prompt) = snapshot["prompt"].as_str().map(str::to_owned) {
        let kept = prefix_within(&prompt, MAX_TRACE_PROMPT_BYTES);
        if kept.len() < prompt.len() {
            snapshot["prompt_truncated"] = json!(true);
            snapshot["prompt_bytes"] = json!(prompt.len());
            snapshot["prompt"] = json!(kept);
        }
    }
    snapshot["prompt_sha256"] = json!(crate::util::sha256(task.prompt.as_bytes()));
    snapshot
}

/// The longest saved request text (`MAX_LEARNING_BRIEF_BYTES`, the Bot question's
/// request) a trace keeps whole.
const MAX_TRACE_PROMPT_BYTES: usize = 8 * 1024;

/// The longest prefix of `text` within `max_bytes`, ending on a character boundary.
fn prefix_within(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// Eval evidence needs complete Journal/note arguments, which routinely exceed
// the UI trace budget. Keep a separate finite cap; redaction still comes first.
const MAX_EVAL_ARGS_BYTES: usize = 64 * 1024;
const MAX_EVAL_RESULT_BYTES: usize = 256 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 2048;
fn bounded_args(value: &Value) -> Value {
    bounded_value(value, MAX_EVAL_ARGS_BYTES)
}

fn bounded_value(value: &Value, max_bytes: usize) -> Value {
    bound_redacted(redact(value), max_bytes)
}

fn bound_redacted(value: Value, max_bytes: usize) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_default();
    if text.len() <= max_bytes {
        value
    } else {
        json!({"truncated":true,"original_bytes":text.len()})
    }
}
fn bounded_text(text: &str) -> String {
    let value = redact(&json!(text));
    let text = value.as_str().unwrap_or("[redacted]");
    if serde_json::to_vec(text).is_ok_and(|bytes| bytes.len() <= MAX_DIAGNOSTIC_BYTES) {
        return text.into();
    }
    let mut truncated = text.chars().take(2000).collect::<String>();
    while serde_json::to_vec(&format!("{truncated} [truncated]"))
        .map_or(true, |bytes| bytes.len() > MAX_DIAGNOSTIC_BYTES)
    {
        if truncated.pop().is_none() {
            break;
        }
    }
    format!("{truncated} [truncated]")
}
fn safe_error_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if [
        "api_key",
        "api-key",
        "authorization",
        "password",
        "appsecret",
        "access_token",
        "credential",
        "bearer ",
    ]
    .iter()
    .any(|key| lower.contains(key))
    {
        "[redacted diagnostic]".into()
    } else {
        bounded_text(text)
    }
}
/// Pi events define call order and include validation failures that never reach
/// the broker. Persisted host entries enrich those calls by ID; unmatched durable
/// entries are retained too, so an absent/lost Pi event never erases a host call.
fn tool_calls(stored: Option<&ChatToolTrace>, pi: &PiTrace) -> Vec<ToolCall> {
    let mut calls = pi.calls.clone();
    for entry in stored.into_iter().flat_map(|trace| &trace.entries) {
        let host = ToolCall::from_host(entry);
        // Missing IDs cannot establish that two calls are the same operation.
        let matching = host
            .call_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .and_then(|id| {
                calls
                    .iter_mut()
                    .find(|call| call.call_id.as_ref() == Some(id))
            });
        if let Some(call) = matching {
            call.enrich_from_host(host);
        } else {
            calls.push(host);
        }
    }
    for call in calls.iter_mut().filter(|call| call.name == "read") {
        call.native_authorization = call
            .call_id
            .as_ref()
            .and_then(|id| pi.native_reads.get(id))
            .cloned();
    }
    calls
}
pub(crate) fn persisted(message: &ChatMessage, pi: &PiTrace) -> Value {
    let commits = message.memory_update.as_ref().map(|update| update.changes.iter().map(|change| {
        let kind = if change.kind == "Research" { "note".into() } else { change.kind.to_lowercase() };
        json!({"kind":kind,"record_id":change.record_id,"title":change.title,"summary":null,"change_summary":bounded_text(&change.lesson),"operation":match change.operation.as_str() { "create"|"revise" => Some(change.operation.as_str()), _=>None }})
    }).collect::<Vec<_>>()).unwrap_or_default();
    let reads = message.memory_refs.iter().map(|read| json!({"kind":read.kind.to_lowercase(),"record_id":read.record_id,"title":read.title,"access":match read.access {MemoryAccess::ExactRead=>"exact_read",MemoryAccess::SearchDiscovered=>"search"}})).collect::<Vec<_>>();
    let receipts = message.lens_receipts.iter().map(|receipt| json!({
        "record_id": receipt.record_id, "revision_id": receipt.revision_id,
        "kind": receipt.kind, "title": bounded_text(&receipt.title), "delivery": receipt.delivery,
    })).collect::<Vec<_>>();
    let performance = message.performance.as_ref();
    let usage = performance.filter(|p| {
        pi.usage_seen
            || p.input_tokens + p.output_tokens + p.cache_read_tokens + p.cache_write_tokens > 0
    });
    let mut record = json!({
        "answer":message.content,
        "failureKind":message.failure_kind,
        "hostComposed":message.host_composed,
        "observed_model":message.execution_model.as_ref().map(|m| format!("{}/{}",m.provider,m.model)),
        "agent_dir_sha256":message.agent_harness.as_ref().and_then(|h| crate::agent_harness::snapshot("chat", &h.skill_ids, &[]).ok()).map(|h|h.digest),
        "status":match message.status {ChatMessageStatus::Complete=>"completed",ChatMessageStatus::Error=>"failed",ChatMessageStatus::Aborted=>"aborted"},
        "error":if message.status == ChatMessageStatus::Error { pi.error.clone().unwrap_or_else(||bounded_json(&json!(message.content))) } else { Value::Null },
        "tool_calls":tool_calls(message.tool_trace.as_ref(),pi),"memory_reads":reads,"lens_receipts":receipts,"memory_commits":commits,
        "number_checks":message.number_checks,"as_of_report":message.as_of_report,
        "usage":{"input":usage.map(|p|p.input_tokens),"output":usage.map(|p|p.output_tokens),"cache_read":usage.map(|p|p.cache_read_tokens),"cache_write":usage.map(|p|p.cache_write_tokens)},
        "performance":{"setup_ms":performance.map(|p|p.setup_ms),"first_text_ms":performance.and_then(|p|p.first_text_ms),"answer_text_ms":performance.and_then(|p|p.answer_text_ms),"total_ms":performance.map(|p|p.total_ms)}
    });
    if let Some(reason) = message
        .memory_update
        .as_ref()
        .and_then(|update| update.skip_reason)
    {
        record["memory_save_skip"] = json!(reason);
    }
    if let Some(notes) = progress::notes(message.progress.as_ref(), &pi.streamed_text) {
        record["progress_notes"] = notes;
    }
    pi.record_startup_failure(&mut record);
    pi.record_round_usage(&mut record);
    record
}

/// Canonical Lens pages after a turn, read from the store rather than the reply,
/// so graders can check what a turn filed and that a read-only turn filed nothing.
pub(crate) fn lens_state(
    state: &crate::app::AppState,
    chat: &crate::domain::ChatSession,
    message_id: Option<&str>,
) -> super::Result<Value> {
    use crate::knowledge::*;
    let store = KnowledgeStore::for_loupist(state.store.clone(), &chat.loupist_id)?;
    let records = store.list_records(&chat.loupist_id)?;
    let mut pages = vec![];
    for record in records.iter().filter(|r| r.kind == RecordKind::Lens) {
        let page = store.read(|read| read.read_page(&chat.loupist_id, &record.record_id))?;
        let body = serde_json::to_value(&page.body)?;
        let sections = body["sections"].as_array().cloned().unwrap_or_default().into_iter().map(|s| {
            let refs = s["basis"]["refs"].as_array().cloned().unwrap_or_default();
            let ref_kinds = refs.iter().filter_map(|r| r["kind"].as_str().map(str::to_owned)).collect::<Vec<_>>();
            // The page view states a Superseded section by when and by which claims.
            let state = if s["contested"] == true {
                json!({"kind":"contested","claim_ids":s["contestedClaimIds"]})
            } else if s["claimIds"].is_array() {
                json!({"kind":"superseded","since_ms":s["sinceMs"],"claim_ids":s["claimIds"]})
            } else {
                json!({"kind":"current"})
            };
            json!({"key":s["key"],"heading":s["heading"],"markdown":s["markdown"],"chip":s["chip"],"origin":s["origin"],
                "contested":s["contested"],"state":state,"basis":ref_kinds,"uncertainty":s["basis"]["uncertainty"]})
        }).collect::<Vec<_>>();
        let ops = page
            .history
            .iter()
            .filter(|e| message_id.is_some() && e.message_id.as_deref() == message_id)
            .map(|e| json!({"op":e.op,"outcome":e.outcome,"key":e.key,"origin":e.origin}))
            .collect::<Vec<_>>();
        pages.push(json!({"record_id":page.record_id,"revision_digest":page.revision_digest,"kind":page.kind,
            "title":page.title,"archived":page.archived,"learned":page.learned,"summary":body["summary"],
            "sections":sections,"ops_this_turn":ops}));
    }
    Ok(json!({"pages":pages}))
}

/// Excerpts come from committed canonical content, never the model's save claim.
pub(crate) fn enrich_commits(
    state: &crate::app::AppState,
    chat: &crate::domain::ChatSession,
    message: &ChatMessage,
    result: &mut Value,
) -> super::Result<()> {
    use crate::knowledge::*;
    let store = KnowledgeStore::for_loupist(state.store.clone(), &chat.loupist_id)?;
    let Some(update) = &message.memory_update else {
        return Ok(());
    };
    for (change, commit) in update
        .changes
        .iter()
        .zip(result["memory_commits"].as_array_mut().unwrap())
    {
        let content = store.read(|read| {
            match commit["kind"].as_str().unwrap_or("") {
                "note" => Ok(json!({"markdown":read.read_working_note(&chat.loupist_id, &change.record_id)?.markdown})),
                "journal" => {
                    let d = read.get_journal(&chat.loupist_id, &change.record_id)?;
                    Ok(json!({"judgment":d.judgment,"reasoning":d.reasoning,"entities":d.entities,"check":d.check,"outcome":d.outcome}))
                }
                "lens" | "principle" | "source" => {
                    // A Source change names its source id; spans belong to the knowledge record.
                    let record_id = change.knowledge.as_ref().map_or(change.record_id.as_str(), |k| k.record_id.as_str());
                    let revision_id = change.knowledge.as_ref().map(|k| k.revision_id.clone())
                        .filter(|id| !id.is_empty()).or_else(||read.list_records(&chat.loupist_id).ok()?.into_iter()
                        .find(|r| r.record_id == record_id)?.revision_id).ok_or(KnowledgeError::NotFound)?;
                    let revision = read.get_revision(&chat.loupist_id, &revision_id)?;
                    if commit["kind"] == "lens" || commit["kind"] == "principle" { return Ok(revision.payload); }
                    let payload: SourceRecordPayload = serde_json::from_value(revision.payload)?;
                    let source = payload.source;
                    let mut excerpts = vec![];
                    for capture in &source.captures {
                        for representation in &capture.representations {
                            if !(representation.media_type.starts_with("text/") || representation.media_type == "application/json") { continue; }
                            if let Some(span) = representation.spans.first() {
                                let (bytes, _) = read.read_revision_span_bytes(record_id, &span.span_id, &span.selected_digest)?;
                                excerpts.push(String::from_utf8_lossy(&bytes).chars().take(1500).collect::<String>());
                            }
                            if !excerpts.is_empty() { break; }
                        }
                        if !excerpts.is_empty() { break; }
                    }
                    Ok(json!({"origin":source.origin_locator,"kind":source.source_class,"coverage":source.captures.iter().map(|c| &c.coverage).collect::<Vec<_>>(),"text":excerpts.join("\n")}))
                }
                _ => Ok(Value::Null),
            }
        });
        match content {
            Ok(content) => {
                commit["summary"] = json!(content_excerpt(&change.title, &content));
                if commit["kind"] == "journal" {
                    for key in ["judgment", "reasoning", "entities", "check", "outcome"] {
                        if let Some(value) = content.get(key).filter(|v| !v.is_null()) {
                            commit[key] = value.clone();
                        }
                    }
                }
            }
            Err(_) => {
                commit["summary"] = Value::Null;
                commit["excerpt_error"] = json!("saved content unavailable");
            }
        }
    }
    Ok(())
}
fn content_excerpt(title: &str, content: &Value) -> String {
    // Allowlist content fields; structured metadata (including credential keys) never leaks.
    let mut parts = vec![format!(
        "title: {}",
        title.chars().take(200).collect::<String>()
    )];
    for key in [
        "applicability",
        "procedure",
        "principles",
        "origin",
        "kind",
        "coverage",
        "text",
        "markdown",
        "conclusion",
        "stance",
        "claim",
        "rationale",
        "uncertainty",
        "alternatives",
    ] {
        if let Some(value) = content.get(key).filter(|v| !v.is_null()) {
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            parts.push(format!(
                "{key}: {}",
                text.chars().take(600).collect::<String>()
            ));
        }
    }
    let text = parts.join("\n");
    if crate::util::credential_text::contains_credential_text(&text) {
        return "[redacted sensitive content]".into();
    }
    text.chars().take(1500).collect()
}

// The runner's staging records: World publications, scheduled occurrences and check dry runs.

/// `world_staged`: the publications staged before this record and the World
/// they left, by claim count, ended claims and claims in conflict.
pub(crate) fn world_staged(
    publications: &[Value],
    claims: usize,
    ended: usize,
    conflicts: usize,
) -> Value {
    json!({"publications": publications, "claims": claims, "ended": ended, "conflicts": conflicts})
}

/// `trigger_dry_run`: the days a check's dry run was met on and the tools it read.
pub(crate) fn trigger_dry_run(days: Vec<String>, tools: Vec<String>) -> Value {
    json!({"days": days, "tools": tools})
}

/// `scheduled_run`: a Learning or check occurrence as the scheduler stored it.
/// A held occurrence has no run row; its hold is recorded instead.
pub(crate) fn scheduled_run(
    run: Option<&crate::scheduled_tasks::types::Run>,
    hold: Option<Value>,
) -> Value {
    match run {
        Some(run) => json!({
            "status": run.status, "quiet": run.quiet, "budget": run.budget, "usage": run.usage,
            "run_id": run.run_id, "kind": run.kind, "hold": hold,
        }),
        None => {
            json!({"status": null, "quiet": false, "budget": null, "usage": null, "hold": hold})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved_task(prompt: &str) -> crate::scheduled_tasks::types::Task {
        use crate::scheduled_tasks::types::{Schedule, TaskDraft, TaskStatus, TaskTarget};

        let store = crate::store::SqliteStore::open_in_memory().unwrap();
        let profile = crate::test_fixtures::loupist_profile("eval-profile");
        store.create_loupist(&profile).unwrap();
        let draft = TaskDraft {
            title: "Market baseline".into(),
            prompt: prompt.into(),
            schedule: Schedule::Interval { every_minutes: 5 },
            target: TaskTarget::NewChat,
            model_profile_id: "fixture/model".into(),
            thinking_level: "off".into(),
            run_options: BTreeMap::new(),
            use_memory: true,
            update_memory: true,
            status: TaskStatus::Active,
        };
        let task = store.scheduled_task_create(&profile.id, draft, 1).unwrap();
        store
            .scheduled_task_get(&profile.id, &task.id)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn scheduled_task_trace_keeps_saved_prompts_whole_up_to_the_stored_limit() {
        // A 3,200-byte Korean request (the old 1,500-character cut lost its end)
        // and a 1,845-character English one survive whole, with their identity.
        let mut korean =
            "알더 레일의 분기 실적을 읽고, 바뀐 점이 없으면 조용히 넘어가 주세요. ".repeat(40);
        korean.truncate(prefix_within(&korean, 3_200).len());
        while korean.len() < 3_200 {
            korean.push('.');
        }
        let english = format!(
            "{}End of the whole request.",
            "Review the Ostrava Grid filing and compare it with the prior quarter. ".repeat(26)
        );
        assert_eq!(korean.len(), 3_200);
        assert_eq!(english.chars().count(), 1_845);
        for prompt in [korean, english] {
            let task = saved_task(&prompt);
            let snapshot = scheduled_task(&task);
            assert_eq!(snapshot["task_id"], task.id);
            assert_eq!(snapshot["status"], "active");
            assert_eq!(snapshot["prompt"], prompt);
            assert_eq!(
                snapshot["prompt_sha256"],
                crate::util::sha256(prompt.as_bytes())
            );
            assert!(snapshot.get("prompt_truncated").is_none());
        }
    }

    #[test]
    fn a_saved_prompt_beyond_the_stored_limit_keeps_its_identity_and_says_it_was_cut() {
        // Three-byte characters straddle the 8 KiB boundary.
        let prompt = "Lies den Bericht von Kraków Mills. 決算を確認する。".repeat(200);
        assert!(prompt.len() > MAX_TRACE_PROMPT_BYTES);
        let mut task = saved_task(&prompt);
        let snapshot = scheduled_task(&task);
        let kept = snapshot["prompt"].as_str().unwrap();
        assert!(kept.len() <= MAX_TRACE_PROMPT_BYTES);
        assert!(kept.len() > MAX_TRACE_PROMPT_BYTES - 4);
        assert!(prompt.starts_with(kept));
        assert_eq!(snapshot["prompt_truncated"], true);
        assert_eq!(snapshot["prompt_bytes"], prompt.len());
        assert_eq!(snapshot["task_id"], task.id);
        assert_eq!(
            snapshot["prompt_sha256"],
            crate::util::sha256(prompt.as_bytes())
        );

        task.prompt
            .push_str("\nAuthorization: Bearer synthetic-fixture");
        let redacted = scheduled_task(&task);
        assert_eq!(redacted["task_id"], task.id);
        assert_eq!(redacted["prompt"], "[redacted]");
        assert!(redacted.get("prompt_truncated").is_none());
    }

    #[test]
    fn refused_calls_keep_the_hosts_typed_reason_from_three_tools() {
        use crate::broker::{error_body, BrokerError, CustomApprovalRefusal};
        // The persisted host trace keeps an approval refusal's reason; Pi sees
        // only the sentence. The body comes from the production error mapping.
        let host_entry = |id: &str, name: &str, error: &BrokerError| {
            let body = error_body(error);
            json!({"call_id":id, "name":name, "component":null, "args":{},
                "start_offset_ms":0, "duration_ms":3, "outcome":"error", "result_bytes":null,
                "error":{"code":body["code"], "message":body["message"], "retryable":body["retryable"],
                    "next":body["next"], "connection_id":body["connection_id"],
                    "tool_name":body["tool_name"], "reason":body["reason"]}})
        };
        let stored: ChatToolTrace = serde_json::from_value(json!({"truncated":false, "entries":[
            host_entry("custom-1", "mcp__ledgerline__filings", &BrokerError::CustomApprovalRefused {
                connection_id: "ledgerline".into(), tool_name: "filings".into(),
                cause: CustomApprovalRefusal::Unattended,
            }),
            host_entry("provider-1", "krx_daily_trading", &BrokerError::ProviderApprovalRequired {
                provider: "krx.market-data".into(),
                services: vec!["KOSDAQ daily trading (ksq_bydd_trd)".into()],
                next: "Approve KOSDAQ daily trading for this key.".into(),
            }),
        ]}))
        .unwrap();
        let mut pi = PiTrace::default();
        // A scripted Pi delivers the broker's error body as the tool result.
        for (id, name, result) in [
            (
                "custom-1",
                "mcp__ledgerline__filings",
                json!({"content":[{"type":"text","text":"Refused."}]}),
            ),
            (
                "provider-1",
                "krx_daily_trading",
                json!({"content":[{"type":"text","text":"Refused."}]}),
            ),
            (
                "save-1",
                "source_save",
                error_body(&BrokerError::ConnectionDontSave),
            ),
            (
                "page-1",
                "lens_file",
                error_body(&BrokerError::invalid(
                    "page",
                    "existing_page:rec_1:Ostrava Grid",
                )),
            ),
        ] {
            pi.observe(
                &json!({"type":"tool_execution_start","toolCallId":id,"toolName":name,"args":{}}),
            );
            pi.observe(&json!({"type":"tool_execution_end","toolCallId":id,"isError":true,"result":result}));
        }
        let calls = json!(tool_calls(Some(&stored), &pi));
        let reasons: Vec<_> = calls
            .as_array()
            .unwrap()
            .iter()
            .map(|call| {
                (
                    call["name"].as_str().unwrap(),
                    call.get("error_reason").cloned(),
                )
            })
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    "mcp__ledgerline__filings",
                    Some(json!("blocked_unattended"))
                ),
                (
                    "krx_daily_trading",
                    Some(json!("provider_service_approval"))
                ),
                ("source_save", Some(json!("connection_dont_save"))),
                // Free text never becomes a typed reason.
                ("lens_file", None),
            ]
        );
        assert_eq!(calls[0]["error_code"], "needs_approval");
        assert_eq!(calls[2]["error_code"], "proposal_disabled");
    }

    #[test]
    fn model_round_usage_ignores_nonterminal_events_and_preserves_unknowns() {
        let mut pi = PiTrace::default();
        for event in [
            json!({"type":"message_update", "message":{"role":"assistant", "usage":{"output":999}}}),
            json!({"type":"message_end", "message":{"role":"user", "usage":{"output":888}}}),
            json!({"type":"message_end", "message":{"role":"assistant", "usage":{"output":0,"reasoning":0}}}),
            json!({"type":"message_end", "message":{"role":"assistant"}}),
            json!({"type":"message_end", "message":{"role":"assistant", "usage":{"output":42}}}),
        ] {
            pi.observe(&event);
        }
        let message = crate::test_fixtures::chat_message(
            "rounds",
            crate::domain::ChatRole::Assistant,
            "Done.",
        );
        let trace = persisted(&message, &pi);
        assert_eq!(trace["model_rounds"], 3);
        assert_eq!(
            trace["round_usage"],
            json!([{"output":0,"reasoning":0},{"output":null,"reasoning":null},{"output":42,"reasoning":null}])
        );
    }

    #[test]
    fn compaction_requests_count_only_terminal_events_with_usage() {
        let mut pi = PiTrace::default();
        for event in [
            json!({"type":"compaction_start","result":{"usage":{"output":9}}}),
            json!({"type":"compaction_end","aborted":true}),
            json!({"type":"compaction_end","result":{"usage":null}}),
            json!({"type":"compaction_end","result":{"usage":{"output":11}}}),
            json!({"type":"message_end","message":{"role":"assistant"}}),
        ] {
            pi.observe(&event);
        }
        let mut trace = json!({});
        pi.record_round_usage(&mut trace);
        assert_eq!(trace["model_rounds"], 1);
        assert_eq!(trace["compaction_requests"], 1);
        assert!(pi.usage_seen);
    }

    #[test]
    fn persisted_trace_keeps_context_and_exact_lens_delivery_receipts() {
        let mut message = crate::test_fixtures::chat_message(
            "answer",
            crate::domain::ChatRole::Assistant,
            "[Rule](lens:rec_rule)",
        );
        for delivery in ["context", "exact_read"] {
            message.lens_receipts.push(crate::domain::LensReceipt {
                record_id: "rec_rule".into(),
                revision_id: "rev_current".into(),
                revision_digest: "a".repeat(64),
                kind: "principle".into(),
                title: "Rule".into(),
                delivery: delivery.into(),
            });
        }
        let trace = persisted(&message, &PiTrace::default());
        assert_eq!(trace["lens_receipts"].as_array().unwrap().len(), 2);
        assert_eq!(trace["lens_receipts"][0]["delivery"], "context");
        assert_eq!(trace["lens_receipts"][1]["delivery"], "exact_read");
        assert_eq!(trace["lens_receipts"][0]["revision_id"], "rev_current");
        assert_eq!(trace["lens_receipts"][0]["kind"], "principle");
        assert!(trace["memory_reads"].as_array().unwrap().is_empty());
    }

    #[test]
    fn skill_authorization_is_host_only_and_bound_to_the_read_call() {
        let mut pi = PiTrace::default();
        // An agent event (or model argument) cannot manufacture host authority.
        pi.observe(&json!({"type":"eval_native_read", "toolCallId":"workspace",
            "native_authorization":{"class":"BundledSkill", "skill_id":"data-analysis"}}));
        for id in ["bundle", "workspace", "unclassified"] {
            pi.observe(
                &json!({"type":"tool_execution_start", "toolCallId":id, "toolName":"read",
                "args":{"path":"/workspace/skills/data-analysis/SKILL.md"}}),
            );
            pi.observe(&json!({"type":"tool_execution_end", "toolCallId":id, "isError":false, "result":{}}));
        }
        pi.native_read_authorized(
            "bundle",
            json!({"class":"BundledSkill", "skill_id":"data-analysis"}),
        );
        pi.native_read_authorized("workspace", json!({"class":"Workspace", "skill_id":null}));
        pi.native_read_authorized(
            "different-call",
            json!({"class":"BundledSkill", "skill_id":"data-analysis"}),
        );
        let calls = json!(tool_calls(None, &pi));
        assert_eq!(
            calls[0]["native_authorization"]["skill_id"],
            "data-analysis"
        );
        assert_eq!(calls[1]["native_authorization"]["class"], "Workspace");
        assert!(calls[2].get("native_authorization").is_none());
    }

    #[test]
    fn credential_field_names_are_redacted_without_hiding_section_metadata() {
        for name in [
            "secret",
            "token",
            "password",
            "passwd",
            "authorization",
            "credential",
            "cookie",
            "api_key",
            "apiKey",
            "API-KEY",
            "access_key",
            "private_key",
            "secret_key",
            "client_key",
            "session_key",
            "auth_key",
            "signing_key",
            "signingKey",
            "appKey",
            "crtfc_key",
        ] {
            let input = json!({"nested": [{name: "synthetic fixture"}]});
            assert_eq!(redact(&input)["nested"][0][name], "[redacted]", "{name}");
        }
        let metadata = json!({
            "key": "overview", "keyword": "cash", "monkey": "x".repeat(40),
            "key_points": ["basic shares", "token usage is not revenue"],
            "ops": [{"key": "use-when"}, {"key": "user-judgment"}],
        });
        assert_eq!(redact(&metadata), metadata);
    }

    #[test]
    fn bare_keys_keep_labels_but_redact_credential_values() {
        for value in [
            "overview",
            "use-when",
            "user-judgment",
            "key_points",
            "keyword",
            "monkey",
            "abcdefghijklmnopqrs",
            "long section label with spaces",
            "operating-margin-review",
        ] {
            let input = json!({"key": value});
            assert_eq!(redact(&input), input, "{value}");
        }
        for value in [
            "api_key=synthetic-fixture",
            "Authorization: Bearer synthetic-fixture",
            "sk-fixture",
            "abcdefghijklmnopqrst",
            "YWJjZGVmZ2hpamtsbW5vcA==",
            "0123456789abcdef0123456789abcdef01234567",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmaXh0dXJlIn0.c3ludGhldGlj",
        ] {
            for name in ["key", "Key", "KEY"] {
                assert_eq!(
                    redact(&json!({name: value}))[name],
                    "[redacted]",
                    "{name}: {value}"
                );
            }
        }
        // Non-string section metadata is traversed, not hidden wholesale.
        assert_eq!(redact(&json!({"key": null})), json!({"key": null}));
        assert_eq!(
            redact(&json!({"key": {"apiKey": "fixture"}})),
            json!({"key": {"apiKey": "[redacted]"}})
        );
    }

    #[test]
    fn host_only_lens_arguments_remain_visible() {
        let args = json!({"pages":[{"ops":[{"op":"put", "key":"user-judgment",
            "markdown":"The user recorded a judgment, not a credential."}]}]});
        let stored: ChatToolTrace = serde_json::from_value(json!({"entries":[{
            "call_id":"host-lens", "name":"lens_file", "component":null, "args":args,
            "start_offset_ms":0, "duration_ms":0, "outcome":"ok", "error":null, "result_bytes":null
        }], "truncated":false}))
        .unwrap();
        let calls = json!(tool_calls(Some(&stored), &PiTrace::default()));
        assert_eq!(calls[0]["args"], args);
    }

    #[test]
    fn scripted_security_turns_keep_comparable_tool_arguments() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../evals/agent/fixtures/security-tool-args.json"
        ))
        .unwrap();
        let turns = fixture["turns"].as_array().unwrap();
        assert_eq!(turns.len(), 5);
        for turn in turns {
            // The old trace lost one S-R05 basis key. Exercise every section in
            // its referenced page; none is presented as a recovered observation.
            let variants = turn["valid_section_keys"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![Value::Null]);
            for variant in variants {
                let mut scripted = turn.clone();
                if let Some(pointer) = turn["unrecoverable_key_pointer"].as_str() {
                    *scripted.pointer_mut(pointer).unwrap() = variant.clone();
                }
                let calls = scripted["calls"].as_array().unwrap();
                let mut pi = PiTrace::default();
                for (index, call) in calls.iter().enumerate() {
                    let id = format!("scripted-{index}");
                    pi.observe(&json!({"type":"tool_execution_start", "toolCallId":id,
                        "toolName":call["name"], "args":call["args"]}));
                    pi.observe(&json!({"type":"tool_execution_end", "toolCallId":id,
                        "isError":call["outcome"] == "error", "result":{"error_code":call["error_code"]}}));
                }
                let message = crate::test_fixtures::chat_message(
                    "scripted-security",
                    crate::domain::ChatRole::Assistant,
                    "Scripted turn complete.",
                );
                let built = persisted(&message, &pi);
                let rebuilt = built["tool_calls"].as_array().unwrap();
                assert_eq!(rebuilt.len(), calls.len());
                for (actual, expected) in rebuilt.iter().zip(calls) {
                    assert_eq!(actual["args"], expected["args"], "{}", turn["scenario_id"]);
                    assert_eq!(actual["outcome"], expected["outcome"]);
                }
                // --nocapture exposes these fictional scripted records for an
                // offline check with the JS grader, without calling a model.
                println!(
                    "REBUILT_SECURITY_TRACE {}",
                    json!({
                        "scenario_id":turn["scenario_id"], "section_key_variant":variant,
                        "tool_calls":built["tool_calls"],
                    })
                );
            }
        }
    }

    #[test]
    fn saved_excerpts_cover_content_and_bound_sensitive_text() {
        for (content, expected) in [
            (
                json!({"applicability":"valuation","procedure":"divide diluted shares"}),
                "divide diluted shares",
            ),
            (
                json!({"origin":"https://fictional.example/report","text":"shares 18"}),
                "shares 18",
            ),
        ] {
            assert!(content_excerpt("title", &content).contains(expected));
        }
        let long = content_excerpt(
            &"한".repeat(300),
            &json!({"applicability":"한".repeat(900),"procedure":"한".repeat(900)}),
        );
        assert!(long.chars().count() <= 1500);
        for secret in [
            "api_key=fixture",
            "Authorization: Bearer fixture",
            "password: fixture",
            "sk-fixture",
        ] {
            assert_eq!(
                content_excerpt("title", &json!({"markdown":secret})),
                "[redacted sensitive content]"
            );
        }
    }

    #[test]
    fn loaded_agent_fingerprint_changes_with_selected_skills() {
        let selectable = crate::agent_harness::default_skill_ids();
        let selected = std::slice::from_ref(selectable.first().expect("selectable skill"));
        let a = crate::agent_harness::snapshot("chat", &[], &[]).unwrap();
        let b = crate::agent_harness::snapshot("chat", selected, &[]).unwrap();
        assert_ne!(a.digest, b.digest);
        assert_eq!(
            b.digest,
            crate::agent_harness::snapshot("chat", selected, &[])
                .unwrap()
                .digest
        );
    }

    #[test]
    fn eval_trace_merges_host_entries_without_losing_pi_validation_failures() {
        let stored: ChatToolTrace = serde_json::from_value(json!({"truncated":false,"entries":[
            {"call_id":"broker-1","name":"web_fetch","component":"web","args":{"url":"https://fictional.example/report","api_key":"fixture-sensitive-value"},"start_offset_ms":0,"duration_ms":12,"outcome":"error","error":{"code":"eval_replay_unmatched","message":"No matching fixture"},"result_bytes":null},
            {"call_id":"host-only","name":"read","component":null,"args":{},"start_offset_ms":0,"duration_ms":0,"outcome":"ok","error":null,"result_bytes":null}
        ]})).unwrap();
        let mut pi = PiTrace::default();
        for (id, name, error) in [
            ("broker-1", "web_fetch", "No fixture"),
            (
                "invalid-2",
                "lens_read",
                "Validation failed for tool \"lens_read\": missing recordId",
            ),
        ] {
            let args = if id == "broker-1" {
                json!({"url":"https://fictional.example/report","api_key":"fixture-sensitive-value"})
            } else {
                json!({})
            };
            pi.observe(
                &json!({"type":"tool_execution_start","toolCallId":id,"toolName":name,"args":args}),
            );
            pi.observe(&json!({"type":"tool_execution_end","toolCallId":id,"toolName":name,"isError":true,"result":{"content":[{"type":"text","text":error}]}}));
        }
        let calls = json!(tool_calls(Some(&stored), &pi));
        let ids: Vec<_> = calls
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["call_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["broker-1", "invalid-2", "host-only"]);
        assert_eq!(calls[0]["error_code"], "eval_replay_unmatched");
        assert_eq!(calls[0]["args"]["api_key"], "[redacted]");
        assert_eq!(calls[1]["error_code"], "tool_validation_failed");
        assert_eq!(
            safe_error_text("Authorization: Bearer fixture-sensitive-value"),
            "[redacted diagnostic]"
        );
    }

    #[test]
    fn eval_args_and_results_redact_credentials_and_bound_size() {
        let args = bounded_args(&json!({"workingNote":{"markdown":"safe text ".repeat(500)},
            "nested":[{"cookie":"fixture","passwd":"fixture","signingKey":"fixture",
                "text":"Authorization: Bearer fixture"}]}));
        assert_eq!(args["workingNote"]["markdown"], "safe text ".repeat(500));
        for field in ["cookie", "passwd", "signingKey", "text"] {
            assert_eq!(args["nested"][0][field], "[redacted]");
        }
        let huge = bounded_args(&json!({"text":"x".repeat(MAX_EVAL_ARGS_BYTES)}));
        assert_eq!(huge["truncated"], true);

        let mut pi = PiTrace::default();
        let evidence = json!({"content":[{"type":"text","text":"Aster reported USD 42 million in Q2, consolidated."}],"details":{"readRef":"read-1"}});
        for (id, result) in [
            ("exact", evidence.clone()),
            (
                "sensitive",
                json!({"api_key":"synthetic-test-value","claim":"revenue"}),
            ),
            ("large", json!({"text":"x".repeat(MAX_EVAL_RESULT_BYTES)})),
        ] {
            pi.observe(&json!({"type":"tool_execution_start","toolCallId":id,"toolName":"world_read","args":{}}));
            pi.observe(&json!({"type":"tool_execution_end","toolCallId":id,"isError":false,"result":result}));
        }
        let calls = json!(tool_calls(None, &pi));
        // Graders see delivered evidence exactly; credentials never reach the report.
        assert_eq!(calls[0]["result"], evidence);
        assert_eq!(calls[1]["result"]["api_key"], "[redacted]");
        assert_eq!(calls[1]["result"]["claim"], "revenue");
        assert_eq!(calls[1]["result_redacted"], true);
        assert_eq!(calls[2]["result"]["truncated"], true);
    }
}
