//! Model and embedding calls for one extraction, with the worker's budgets,
//! message assembly and accounting. Every call goes through the host's
//! inference callback (`graph_inference::dispatch_inference` in production);
//! nothing is retried.

use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use super::super::graph_inference::{InferenceDisposition, InferenceReply};
use super::ontology::{ENGLISH_EXTRACTION_RULE, WORLD_FACT_POLICY};
use super::prompts::Message;
use super::pyjson::Py;
use super::text::replace_unusable;

/// Why an extraction stopped. The worker raised `InferenceStop` for these and a
/// generic exception for an invalid model result; neither is retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stop {
    BudgetExceeded(&'static str),
    NotSent,
    Uncertain,
    Rejected(&'static str),
    /// A model result Pydantic would have rejected.
    Invalid(&'static str),
}

impl From<super::schema::Invalid> for Stop {
    fn from(value: super::schema::Invalid) -> Self {
        Self::Invalid(value.0)
    }
}

pub(crate) type StopResult<T> = Result<T, Stop>;

const ATTRIBUTE_SENTINEL: &str = "<<graphiti.attr_extraction.preamble.v1>>";
const ATTRIBUTE_PREAMBLE: &str = "\n\n<<graphiti.attr_extraction.preamble.v1>>\nATTRIBUTE EXTRACTION: Field descriptions in the response schema describe what a real value LOOKS LIKE — they are NEVER themselves valid values and must NEVER be copied into any field. If you have no value for a field, set it to null; never explain the absence in the field itself.";

/// Default answer tokens (Graphiti's LLMConfig max_tokens for the host client).
pub(crate) const DEFAULT_MAX_TOKENS: u32 = 4096;
pub(crate) const EXTRACT_EDGES_MAX_TOKENS: u32 = 16384;
const MAX_FRAME_BYTES: usize = 2_000_000;
const MAX_EMBED_TEXTS: usize = 256;
const EMBED_CACHE_LIMIT: usize = 1024;
const EMBEDDING_DIMENSION: usize = 128;

/// One model call as the pipeline asks for it.
pub(crate) struct LlmCall<'a> {
    pub stage: &'static str,
    pub messages: Vec<Message>,
    pub schema: Option<&'a Py>,
    pub max_tokens: u32,
    pub small: bool,
    pub attribute_extraction: bool,
}

/// LLMClient.generate_response plus the worker's HostLLM: attribute preamble,
/// schema suffix, English rule, input cleaning and the closing policy message.
pub(crate) fn assemble(
    mut messages: Vec<Message>,
    schema: Option<&Py>,
    attribute_extraction: bool,
) -> Vec<Message> {
    if attribute_extraction && !messages.is_empty() {
        let target = &mut messages[0];
        if !target.content.contains(ATTRIBUTE_SENTINEL) {
            if target.role == "system" {
                target.content.push_str(ATTRIBUTE_PREAMBLE);
            } else {
                target.content =
                    format!("{}\n\n{}", ATTRIBUTE_PREAMBLE.trim_start(), target.content);
            }
        }
    }
    if let (Some(schema), Some(last)) = (schema, messages.last_mut()) {
        last.content
            .push_str("\n\nRespond with a JSON object in the following format:\n\n");
        last.content.push_str(&schema.dumps(true));
    }
    if let Some(first) = messages.first_mut() {
        first.content.push_str(ENGLISH_EXTRACTION_RULE);
    }
    for message in &mut messages {
        message.content = clean_input(&message.content);
    }
    messages.push(Message::new("system", WORLD_FACT_POLICY));
    messages
}

/// LLMClient._clean_input: drop zero-width characters and controls other than
/// newline, carriage return and tab.
pub(crate) fn clean_input(text: &str) -> String {
    text.chars()
        .filter(|ch| {
            !matches!(
                ch,
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{2060}'
            ) && (*ch as u32 >= 32 || matches!(ch, '\n' | '\r' | '\t'))
        })
        .collect()
}

/// One record of the evaluation stage log (no prompt or provider text).
#[derive(Debug, Clone)]
pub(crate) struct StageRecord {
    pub kind: &'static str,
    pub stage: &'static str,
    pub ms: u64,
    pub usage: Value,
    pub texts: usize,
}

/// The host's inference callback: one request at a time, or several sent
/// together and answered in request order.
enum Infer<'a> {
    #[cfg_attr(not(test), allow(dead_code))]
    One(&'a mut dyn FnMut(Value) -> InferenceReply),
    Many(&'a mut dyn FnMut(Vec<Value>) -> Vec<InferenceReply>),
}

impl Infer<'_> {
    fn one(&mut self, request: Value) -> InferenceReply {
        match self {
            Self::One(infer) => infer(request),
            Self::Many(infer) => infer(vec![request])
                .pop()
                .unwrap_or_else(|| InferenceReply {
                    disposition: InferenceDisposition::NotSent,
                    body: json!({"status": "not_sent"}),
                }),
        }
    }
}

pub(crate) struct Bridge<'a> {
    infer: Infer<'a>,
    scope: Value,
    operation_id: String,
    profile: String,
    fast_profile: String,
    timeout_ms: u64,
    max_llm_calls: u32,
    max_embedding_calls: u32,
    call_index: u64,
    pub llm_calls: u32,
    pub embedding_calls: u32,
    /// A sent call whose outcome or usage could not be confirmed.
    pub uncertain: bool,
    cache: HashMap<String, Vec<f32>>,
    pub stages: Vec<StageRecord>,
}

pub(crate) struct BridgeConfig {
    pub scope: Value,
    pub operation_id: String,
    pub profile: String,
    pub fast_profile: String,
    pub timeout_ms: u64,
    pub max_llm_calls: u32,
    pub max_embedding_calls: u32,
}

impl<'a> Bridge<'a> {
    /// Calls go out one at a time; [`Bridge::llm_many`] sends its calls in order
    /// and stops at the first that fails, as separate calls would.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        config: BridgeConfig,
        infer: &'a mut dyn FnMut(Value) -> InferenceReply,
    ) -> Self {
        Self::with_infer(config, Infer::One(infer))
    }

    /// Independent calls of one [`Bridge::llm_many`] go out together.
    pub(crate) fn concurrent(
        config: BridgeConfig,
        infer: &'a mut dyn FnMut(Vec<Value>) -> Vec<InferenceReply>,
    ) -> Self {
        Self::with_infer(config, Infer::Many(infer))
    }

    fn with_infer(config: BridgeConfig, infer: Infer<'a>) -> Self {
        Self {
            infer,
            scope: config.scope,
            operation_id: config.operation_id,
            profile: config.profile,
            fast_profile: config.fast_profile,
            timeout_ms: config.timeout_ms,
            max_llm_calls: config.max_llm_calls,
            max_embedding_calls: config.max_embedding_calls,
            call_index: 0,
            llm_calls: 0,
            embedding_calls: 0,
            uncertain: false,
            cache: HashMap::new(),
            stages: Vec::new(),
        }
    }

    fn callback(&mut self, kind: &str, profile: &str) -> Value {
        self.call_index += 1;
        json!({
            "kind": kind,
            "operation_id": self.operation_id,
            "scope": self.scope,
            "call_index": self.call_index,
            "model_profile": profile,
            "timeout_ms": self.timeout_ms,
            "messages": null,
            "inputs": null,
            "max_tokens": null,
            "response_schema": null,
        })
    }

    /// InferenceBridge.complete: budget, one attempt, accounting.
    fn complete(&mut self, request: Value) -> StopResult<Value> {
        let kind = request["kind"].as_str().unwrap_or("").to_string();
        if kind == "llm" {
            if self.llm_calls >= self.max_llm_calls {
                return Err(Stop::BudgetExceeded("llm call budget is exhausted"));
            }
        } else if self.embedding_calls >= self.max_embedding_calls {
            return Err(Stop::BudgetExceeded("embedding call budget is exhausted"));
        }
        let reply = self.infer.one(request);
        self.settle(&kind, reply)
    }

    fn settle(&mut self, kind: &str, reply: InferenceReply) -> StopResult<Value> {
        match reply.disposition {
            InferenceDisposition::NotSent => Err(Stop::NotSent),
            InferenceDisposition::Uncertain => {
                self.attempted(kind);
                self.uncertain = true;
                Err(Stop::Uncertain)
            }
            InferenceDisposition::Completed => {
                self.attempted(kind);
                if reply
                    .body
                    .get("json_object")
                    .and_then(Value::as_object)
                    .is_none()
                    && reply
                        .body
                        .get("vectors")
                        .and_then(Value::as_array)
                        .is_none()
                {
                    // The host's worker client refused such a reply as uncertain.
                    self.uncertain = true;
                    return Err(Stop::Uncertain);
                }
                Ok(reply.body)
            }
        }
    }

    fn attempted(&mut self, kind: &str) {
        if kind == "llm" {
            self.llm_calls += 1;
        } else {
            self.embedding_calls += 1;
        }
    }

    /// One model call; returns the JSON object the model produced.
    pub(crate) fn llm(&mut self, call: LlmCall<'_>) -> StopResult<Value> {
        self.llm_many(vec![call])
            .pop()
            .unwrap_or(Err(Stop::NotSent))
    }

    /// Model calls whose inputs do not depend on each other, answered in call
    /// order. The call budget is taken in that order, so a call past it is not
    /// sent, exactly as when each call follows the one before. A caller reads
    /// the results in order and stops at the first error.
    pub(crate) fn llm_many(&mut self, calls: Vec<LlmCall<'_>>) -> Vec<StopResult<Value>> {
        let mut results: Vec<Option<StopResult<Value>>> = (0..calls.len()).map(|_| None).collect();
        let mut pending: Vec<(usize, &'static str, Value)> = Vec::new();
        let mut budget = self.max_llm_calls.saturating_sub(self.llm_calls);
        for (index, call) in calls.into_iter().enumerate() {
            if budget == 0 {
                results[index] = Some(Err(Stop::BudgetExceeded("llm call budget is exhausted")));
                continue;
            }
            budget -= 1;
            let stage = call.stage;
            pending.push((index, stage, self.llm_request(call)));
        }
        if matches!(self.infer, Infer::Many(_)) && pending.len() > 1 {
            let started = Instant::now();
            let requests = pending
                .iter()
                .map(|(_, _, request)| request.clone())
                .collect();
            let Infer::Many(infer) = &mut self.infer else {
                unreachable!("checked above")
            };
            let mut replies = infer(requests).into_iter();
            for (index, stage, _) in pending {
                let reply = replies.next().unwrap_or_else(|| InferenceReply {
                    disposition: InferenceDisposition::NotSent,
                    body: json!({"status": "not_sent"}),
                });
                let settled = self.settle("llm", reply);
                results[index] = Some(self.llm_result(stage, started, settled));
            }
        } else {
            for (index, stage, request) in pending {
                let started = Instant::now();
                let settled = self.infer_one_llm(request);
                let result = self.llm_result(stage, started, settled);
                let failed = result.is_err();
                results[index] = Some(result);
                if failed {
                    break;
                }
            }
        }
        results
            .into_iter()
            .map(|result| result.unwrap_or(Err(Stop::NotSent)))
            .collect()
    }

    fn infer_one_llm(&mut self, request: Value) -> StopResult<Value> {
        let reply = self.infer.one(request);
        self.settle("llm", reply)
    }

    fn llm_request(&mut self, call: LlmCall<'_>) -> Value {
        let messages = assemble(call.messages, call.schema, call.attribute_extraction);
        let profile = if call.small && !self.fast_profile.is_empty() {
            self.fast_profile.clone()
        } else {
            self.profile.clone()
        };
        let mut request = self.callback("llm", &profile);
        request["stage"] = json!(call.stage);
        request["messages"] = Value::Array(
            messages
                .iter()
                .map(|message| json!({"role": message.role, "content": message.content}))
                .collect(),
        );
        request["max_tokens"] = json!(call.max_tokens);
        request["response_schema"] = call.schema.map_or(Value::Null, Py::to_value);
        request
    }

    fn llm_result(
        &mut self,
        stage: &'static str,
        started: Instant,
        settled: StopResult<Value>,
    ) -> StopResult<Value> {
        let body = settled?;
        self.stages.push(StageRecord {
            kind: "llm",
            stage,
            ms: started.elapsed().as_millis() as u64,
            usage: body.get("usage").cloned().unwrap_or(Value::Null),
            texts: 0,
        });
        match body.get("json_object") {
            Some(Value::Object(object)) => Ok(Value::Object(object.clone())),
            _ => Err(Stop::Rejected("model result was not an object")),
        }
    }

    /// The worker's HostEmbedder: one wave of texts becomes as few callbacks as
    /// the frame allows; cached texts make no call; zero vectors fall back.
    pub(crate) fn embed(&mut self, texts: &[String]) -> StopResult<Vec<Vec<f32>>> {
        let mut missing: Vec<String> = Vec::new();
        for text in texts {
            if !self.cache.contains_key(text) && !missing.contains(text) {
                missing.push(text.clone());
            }
        }
        let mut fresh: HashMap<String, Vec<f32>> = HashMap::new();
        if !missing.is_empty() {
            for batch in self.plan_batches(&missing)? {
                let mut request = self.callback("embedding", &self.profile.clone());
                request["inputs"] = json!(batch);
                let started = Instant::now();
                let body = self.complete(request)?;
                let vectors = body
                    .get("vectors")
                    .and_then(Value::as_array)
                    .filter(|vectors| vectors.len() == batch.len())
                    .ok_or(Stop::Rejected("embedding result did not match the input"))?;
                self.stages.push(StageRecord {
                    kind: "embedding",
                    stage: "embed",
                    ms: started.elapsed().as_millis() as u64,
                    usage: Value::Null,
                    texts: batch.len(),
                });
                let dimension = vectors
                    .iter()
                    .filter_map(Value::as_array)
                    .map(Vec::len)
                    .find(|len| *len > 0)
                    .unwrap_or(EMBEDDING_DIMENSION);
                for (text, vector) in batch.iter().zip(vectors) {
                    let values: Vec<f32> = vector
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| item.as_f64().unwrap_or(0.0) as f32)
                                .collect()
                        })
                        .unwrap_or_default();
                    let vector = replace_unusable(text, values, dimension);
                    if self.cache.len() < EMBED_CACHE_LIMIT {
                        self.cache.insert(text.clone(), vector.clone());
                    }
                    fresh.insert(text.clone(), vector);
                }
            }
        }
        Ok(texts
            .iter()
            .map(|text| {
                self.cache
                    .get(text)
                    .or_else(|| fresh.get(text))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect())
    }

    /// plan_embedding_batches over the encoded callback frame.
    fn plan_batches(&self, texts: &[String]) -> StopResult<Vec<Vec<String>>> {
        let probe = json!({
            "jsonrpc": "2.0",
            "id": "cb-00000000",
            "method": "inference.complete",
            "params": {
                "call_index": self.call_index + 1,
                "inputs": [],
                "kind": "embedding",
                "max_tokens": null,
                "messages": null,
                "model_profile": self.profile,
                "operation_id": self.operation_id,
                "response_schema": null,
                "scope": self.scope,
                "timeout_ms": self.timeout_ms,
            }
        });
        let overhead = serde_json::to_string(&probe)
            .map(|text| text.len())
            .unwrap_or(0);
        let mut batches = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut current_bytes = overhead;
        for text in texts {
            let encoded = Py::str(text.as_str()).dumps(false).len();
            if overhead + encoded > MAX_FRAME_BYTES {
                return Err(Stop::Rejected("embedding input exceeds the worker frame"));
            }
            let mut added = encoded + usize::from(!current.is_empty());
            if !current.is_empty()
                && (current.len() >= MAX_EMBED_TEXTS || current_bytes + added > MAX_FRAME_BYTES)
            {
                batches.push(std::mem::take(&mut current));
                current_bytes = overhead;
                added = encoded;
            }
            current.push(text.clone());
            current_bytes += added;
        }
        if !current.is_empty() {
            batches.push(current);
        }
        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembly_appends_schema_rule_and_policy_and_cleans_input() {
        let schema = Py::dict([("title", Py::str("삼"))]);
        let messages = assemble(
            vec![
                Message::new("system", "sys\u{200b}"),
                Message::new("user", "body\u{1}"),
            ],
            Some(&schema),
            true,
        );
        assert_eq!(messages.len(), 3);
        assert!(messages[0]
            .content
            .starts_with("sys\n\n<<graphiti.attr_extraction"));
        assert!(messages[0].content.ends_with(ENGLISH_EXTRACTION_RULE));
        assert_eq!(
            messages[1].content,
            "body\n\nRespond with a JSON object in the following format:\n\n{\"title\": \"\\uc0bc\"}"
        );
        assert_eq!(messages[2].content, WORLD_FACT_POLICY);
    }

    #[test]
    fn budgets_stop_before_a_call_and_not_sent_is_not_an_attempt() {
        let mut calls = 0;
        let mut infer = |_request: Value| {
            calls += 1;
            InferenceReply {
                disposition: InferenceDisposition::NotSent,
                body: json!({"status": "not_sent"}),
            }
        };
        let mut bridge = Bridge::new(
            BridgeConfig {
                scope: json!({}),
                operation_id: "job".into(),
                profile: "p".into(),
                fast_profile: String::new(),
                timeout_ms: 1000,
                max_llm_calls: 1,
                max_embedding_calls: 1,
            },
            &mut infer,
        );
        let call = || LlmCall {
            stage: "extract_nodes",
            messages: vec![Message::new("user", "x")],
            schema: None,
            max_tokens: DEFAULT_MAX_TOKENS,
            small: false,
            attribute_extraction: false,
        };
        assert_eq!(bridge.llm(call()), Err(Stop::NotSent));
        assert_eq!(bridge.llm_calls, 0);
        bridge.llm_calls = 1;
        assert_eq!(
            bridge.llm(call()),
            Err(Stop::BudgetExceeded("llm call budget is exhausted"))
        );
        drop(bridge);
        assert_eq!(calls, 1);
    }

    fn config(max_llm_calls: u32) -> BridgeConfig {
        BridgeConfig {
            scope: json!({}),
            operation_id: "job".into(),
            profile: "p".into(),
            fast_profile: String::new(),
            timeout_ms: 1000,
            max_llm_calls,
            max_embedding_calls: 1,
        }
    }

    fn call(stage: &'static str) -> LlmCall<'static> {
        LlmCall {
            stage,
            messages: vec![Message::new("user", stage)],
            schema: None,
            max_tokens: DEFAULT_MAX_TOKENS,
            small: false,
            attribute_extraction: false,
        }
    }

    fn answered(request: &Value) -> InferenceReply {
        InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: json!({"status": "ok", "json_object": {"stage": request["stage"]}}),
        }
    }

    #[test]
    fn independent_calls_go_out_together_and_come_back_in_call_order() {
        let mut batches: Vec<Vec<String>> = Vec::new();
        let mut infer = |requests: Vec<Value>| {
            batches.push(
                requests
                    .iter()
                    .map(|request| request["stage"].as_str().unwrap().to_string())
                    .collect(),
            );
            requests.iter().map(answered).collect::<Vec<_>>()
        };
        let mut bridge = Bridge::concurrent(config(3), &mut infer);
        let results = bridge.llm_many(vec![call("dedupe_nodes"), call("extract_edges")]);
        assert_eq!(results[0], Ok(json!({"stage": "dedupe_nodes"})));
        assert_eq!(results[1], Ok(json!({"stage": "extract_edges"})));
        assert_eq!(bridge.llm_calls, 2);
        // The budget is taken in call order: a call past it is never sent.
        let results = bridge.llm_many(vec![call("resolve_edges"), call("attributes")]);
        assert!(results[0].is_ok());
        assert_eq!(
            results[1],
            Err(Stop::BudgetExceeded("llm call budget is exhausted"))
        );
        drop(bridge);
        assert_eq!(
            batches,
            [vec!["dedupe_nodes", "extract_edges"], vec!["resolve_edges"]]
        );
    }

    #[test]
    fn one_at_a_time_stops_at_the_first_failed_call() {
        let mut sent = Vec::new();
        let mut infer = |request: Value| {
            sent.push(request["stage"].as_str().unwrap().to_string());
            InferenceReply {
                disposition: InferenceDisposition::Uncertain,
                body: json!({"status": "uncertain"}),
            }
        };
        let mut bridge = Bridge::new(config(4), &mut infer);
        let results = bridge.llm_many(vec![call("resolve_edges"), call("attributes")]);
        assert_eq!(results[0], Err(Stop::Uncertain));
        assert!(results[1].is_err());
        assert!(bridge.uncertain);
        drop(bridge);
        assert_eq!(sent, ["resolve_edges"]);
    }
}
