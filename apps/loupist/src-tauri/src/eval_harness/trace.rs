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

#[derive(Default)]
pub(crate) struct PiTrace {
    calls: Vec<ToolCall>,
    starts: BTreeMap<String, (usize, Instant)>,
    pub usage_seen: bool,
    error: Option<Value>,
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
    result_bytes: Option<u64>,
    /// Actual Pi-delivered response, separate from the host display trace.
    result: Option<Value>,
    result_redacted: Option<bool>,
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
            result_bytes: entry.result_bytes,
            result: None,
            result_redacted: None,
        }
    }

    fn enrich_from_host(&mut self, host: Self) {
        self.component = host.component.or(self.component.take());
        self.duration_ms = host.duration_ms.or(self.duration_ms);
        self.error_code = host.error_code.or(self.error_code.take());
        self.error_message = host.error_message.or(self.error_message.take());
        self.result_bytes = host.result_bytes.or(self.result_bytes);
        // Pi records submitted arguments before the host applies its display cap.
        if self.args.is_null() {
            self.args = host.args;
        }
        self.outcome = self.outcome.or(host.outcome);
    }
}

impl PiTrace {
    pub fn observe(&mut self, event: &Value) {
        self.usage_seen |= event.pointer("/message/usage").is_some();
        if let Some(diagnostic) =
            crate::pi::e2e_rpc_diagnostic(event).filter(|d| d.get("category").is_some())
        {
            self.error = Some(json!(diagnostic.to_string()));
        }
        match event["type"].as_str() {
            Some("eval_error") if self.error.is_none() => {
                self.error = Some(bounded_json(&event["message"]));
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
                    result_bytes: None,
                    result: None,
                    result_redacted: None,
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

fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let lower = key.to_lowercase();
                    let sensitive = [
                        "secret",
                        "token",
                        "key",
                        "password",
                        "passwd",
                        "authorization",
                        "credential",
                        "cookie",
                    ]
                    .iter()
                    .any(|word| lower.contains(word));
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
    calls
}
pub(crate) fn persisted(message: &ChatMessage, pi: &PiTrace) -> Value {
    let commits = message.memory_update.as_ref().map(|update| update.changes.iter().map(|change| {
        let kind = if change.kind == "Research" { "note".into() } else { change.kind.to_lowercase() };
        json!({"kind":kind,"record_id":change.record_id,"title":change.title,"summary":null,"change_summary":bounded_text(&change.lesson),"operation":match change.operation.as_str() { "create"|"revise" => Some(change.operation.as_str()), _=>None }})
    }).collect::<Vec<_>>()).unwrap_or_default();
    let reads = message.memory_refs.iter().map(|read| json!({"kind":read.kind.to_lowercase(),"record_id":read.record_id,"title":read.title,"access":match read.access {MemoryAccess::ExactRead=>"exact_read",MemoryAccess::SearchDiscovered=>"search"}})).collect::<Vec<_>>();
    let performance = message.performance.as_ref();
    let usage = performance.filter(|p| {
        pi.usage_seen
            || p.input_tokens + p.output_tokens + p.cache_read_tokens + p.cache_write_tokens > 0
    });
    json!({
        "answer":message.content,
        "failureKind":message.failure_kind,
        "hostComposed":message.host_composed,
        "observed_model":message.execution_model.as_ref().map(|m| format!("{}/{}",m.provider,m.model)),
        "agent_dir_sha256":message.agent_harness.as_ref().and_then(|h| crate::agent_harness::snapshot("chat", &h.skill_ids, &[]).ok()).map(|h|h.digest),
        "status":match message.status {ChatMessageStatus::Complete=>"completed",ChatMessageStatus::Error=>"failed",ChatMessageStatus::Aborted=>"aborted"},
        "error":if message.status == ChatMessageStatus::Error { pi.error.clone().unwrap_or_else(||bounded_json(&json!(message.content))) } else { Value::Null },
        "tool_calls":tool_calls(message.tool_trace.as_ref(),pi),"memory_reads":reads,"memory_commits":commits,
        "number_checks":message.number_checks,"sensitivity_cards":message.sensitivity_cards,"as_of_report":message.as_of_report,
        "usage":{"input":usage.map(|p|p.input_tokens),"output":usage.map(|p|p.output_tokens),"cache_read":usage.map(|p|p.cache_read_tokens),"cache_write":usage.map(|p|p.cache_write_tokens)},
        "performance":{"setup_ms":performance.map(|p|p.setup_ms),"first_text_ms":performance.and_then(|p|p.first_text_ms),"total_ms":performance.map(|p|p.total_ms)}
    })
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

#[cfg(test)]
mod tests {
    use super::*;
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
