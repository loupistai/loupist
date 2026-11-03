use serde_json::Value;

/// Validates Pi's streamed assistant text against the authoritative
/// `message_end.message`. A turn is durable only when the last assistant
/// message ended with `stop` and Pi subsequently emits `agent_settled`.
/// Provider error details are deliberately never surfaced through this type: an
/// error's text is classified into a `ProviderFailure` kind and dropped at once.
#[derive(Debug, Default)]
pub struct AssistantTurnCapture {
    active: bool,
    streamed_text: String,
    completed_text: Option<String>,
    last_stop_reason: Option<String>,
    provider_failure: Option<super::provider_failure::ProviderFailure>,
    /// The latest provider response's timing headers (`loupist_retry_hints`).
    retry_headers: serde_json::Map<String, Value>,
    /// For a busy failure: the wait its hints asked for, bounded.
    retry_after_seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantTurnEnd {
    Stop(String),
    ToolUse(String),
    Length,
    Error,
    Aborted,
}

impl AssistantTurnCapture {
    /// Keeps the allowlisted timing headers of a busy provider response until
    /// the assistant message it ended ends.
    pub fn observe_retry_headers(&mut self, headers: &Value) {
        self.retry_headers = headers
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(name, value)| {
                super::provider_failure::RETRY_HEADERS.contains(&name.as_str())
                    && value.as_str().is_some_and(|text| text.len() <= 128)
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
    }

    pub fn retry_after_seconds(&self) -> Option<u32> {
        self.retry_after_seconds
    }

    pub fn observe_message_start(&mut self, payload: &Value) -> Result<(), &'static str> {
        let Some(message) = payload.get("message") else {
            return Err("Pi message_start omitted its message");
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return Ok(());
        }
        if self.active {
            return Err("Pi started an assistant message before ending the previous message");
        }
        self.active = true;
        self.streamed_text.clear();
        Ok(())
    }

    pub fn observe_text_delta(
        &mut self,
        delta: &str,
        max_output_bytes: usize,
    ) -> Result<(), &'static str> {
        if !self.active {
            return Err("Pi streamed assistant text outside a message");
        }
        if self.streamed_text.len().saturating_add(delta.len()) > max_output_bytes {
            return Err("Pi assistant message exceeded its output limit");
        }
        self.streamed_text.push_str(delta);
        Ok(())
    }

    pub fn observe_message_end(
        &mut self,
        payload: &Value,
        max_output_bytes: usize,
    ) -> Result<Option<AssistantTurnEnd>, &'static str> {
        let message = payload
            .get("message")
            .ok_or("Pi message_end omitted its message")?;
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return Ok(None);
        }
        if !self.active {
            return Err("Pi ended an assistant message that was not started");
        }
        let authoritative = assistant_text(message, max_output_bytes)?;
        if self.streamed_text != authoritative {
            return Err("Pi streamed assistant text did not match message_end");
        }
        let stop_reason = message
            .get("stopReason")
            .and_then(Value::as_str)
            .ok_or("Pi assistant message omitted its stop reason")?;
        if !matches!(
            stop_reason,
            "stop" | "length" | "toolUse" | "error" | "aborted"
        ) {
            return Err("Pi assistant message used an unknown stop reason");
        }
        self.active = false;
        self.last_stop_reason = Some(stop_reason.to_owned());
        self.provider_failure = (stop_reason == "error").then(|| {
            super::provider_failure::ProviderFailure::classify(
                message
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        });
        self.retry_after_seconds = (self.provider_failure
            == Some(super::provider_failure::ProviderFailure::RateLimited))
        .then(|| {
            super::provider_failure::retry_after_seconds(
                &Value::Object(self.retry_headers.clone()),
                message
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                chrono::Utc::now().timestamp_millis(),
            )
        })
        .flatten();
        // Hints belong to the response this message came from.
        self.retry_headers.clear();
        let end = match stop_reason {
            "stop" => {
                self.completed_text = Some(authoritative.clone());
                AssistantTurnEnd::Stop(authoritative)
            }
            "toolUse" => AssistantTurnEnd::ToolUse(authoritative),
            "length" => AssistantTurnEnd::Length,
            "error" => AssistantTurnEnd::Error,
            "aborted" => AssistantTurnEnd::Aborted,
            _ => return Err("Pi assistant message used an unknown stop reason"),
        };
        Ok(Some(end))
    }

    pub fn finish_settled(&self) -> Result<&str, &'static str> {
        if self.active {
            return Err("Pi settled with an unfinished assistant message");
        }
        match self.last_stop_reason.as_deref() {
            Some("stop") => {}
            Some("length") => return Err("Pi assistant response reached its length limit"),
            Some("toolUse") => return Err("Pi settled with an incomplete tool request"),
            Some("error") => {
                return Err(self
                    .provider_failure
                    .unwrap_or(super::provider_failure::ProviderFailure::Other)
                    .host_message())
            }
            Some("aborted") => return Err("Pi assistant turn was aborted"),
            None => return Err("Pi settled without an assistant message"),
            Some(_) => return Err("Pi assistant message used an unknown stop reason"),
        }
        self.completed_text
            .as_deref()
            .ok_or("Pi settled without authoritative assistant text")
    }
}

fn assistant_text(message: &Value, max_output_bytes: usize) -> Result<String, &'static str> {
    let blocks = message
        .get("content")
        .and_then(Value::as_array)
        .ok_or("Pi assistant message content is malformed")?;
    let mut text = String::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let value = block
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("Pi assistant text block is malformed")?;
                if text.len().saturating_add(value.len()) > max_output_bytes {
                    return Err("Pi assistant message exceeded its output limit");
                }
                text.push_str(value);
            }
            Some("thinking") => {
                if block.get("thinking").and_then(Value::as_str).is_none() {
                    return Err("Pi assistant thinking block is malformed");
                }
            }
            Some("toolCall") => {
                if block.get("id").and_then(Value::as_str).is_none()
                    || block.get("name").and_then(Value::as_str).is_none()
                    || !block.get("arguments").is_some_and(Value::is_object)
                {
                    return Err("Pi assistant tool call is malformed");
                }
            }
            _ => return Err("Pi assistant message contains an unknown content block"),
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn start() -> Value {
        json!({"type":"message_start","message":{"role":"assistant"}})
    }

    fn end(text: &str, reason: &str) -> Value {
        json!({
            "type":"message_end",
            "message": {
                "role":"assistant",
                "content":[{"type":"text","text":text}],
                "stopReason":reason
            }
        })
    }

    #[test]
    fn settled_content_comes_from_matching_successful_message_end() {
        let mut capture = AssistantTurnCapture::default();
        capture.observe_message_start(&start()).unwrap();
        capture.observe_text_delta("hel", 16).unwrap();
        capture.observe_text_delta("lo", 16).unwrap();
        capture
            .observe_message_end(&end("hello", "stop"), 16)
            .unwrap()
            .unwrap();
        assert_eq!(capture.finish_settled().unwrap(), "hello");
    }

    /// The extension forwards a busy response's timing headers; a busy failure
    /// keeps the bounded wait, any other end or a later response drops it (B83).
    #[test]
    fn a_busy_failure_keeps_the_providers_retry_hint_only() {
        let failed = |text: &str| {
            let mut message = end("", "error");
            message["message"]["errorMessage"] = json!(text);
            message
        };
        let mut capture = AssistantTurnCapture::default();
        capture.observe_retry_headers(&json!({
            "retry-after": "12", "set-cookie": "secret", "x-request-id": "req_1"
        }));
        assert_eq!(capture.retry_headers.len(), 1);
        capture.observe_message_start(&start()).unwrap();
        capture
            .observe_message_end(&failed("429 Too Many Requests"), 16)
            .unwrap();
        assert_eq!(capture.retry_after_seconds(), Some(12));
        // A sign-in failure is not a busy one, whatever its headers say.
        capture.observe_retry_headers(&json!({"retry-after": "12"}));
        capture.observe_message_start(&start()).unwrap();
        capture
            .observe_message_end(&failed("401 invalid api key"), 16)
            .unwrap();
        assert_eq!(capture.retry_after_seconds(), None);
        // Hints belong to one response: a later busy answer without its own has none.
        capture.observe_message_start(&start()).unwrap();
        capture
            .observe_message_end(&failed("429 Too Many Requests"), 16)
            .unwrap();
        assert_eq!(capture.retry_after_seconds(), None);
    }

    #[test]
    fn a_provider_error_settles_as_its_typed_host_message_only() {
        let mut failed = AssistantTurnCapture::default();
        failed.observe_message_start(&start()).unwrap();
        let mut message = end("", "error");
        message["message"]["errorMessage"] =
            json!("429 Too Many Requests: quota for sk-live-SECRET in org-private");
        failed.observe_message_end(&message, 16).unwrap().unwrap();
        let error = failed.finish_settled().unwrap_err();
        assert_eq!(error, crate::pi::provider_failure::RATE_LIMITED);
        assert!(!error.contains("SECRET") && !error.contains("org-private"));
    }

    #[test]
    fn mismatch_and_non_successful_stop_fail_closed() {
        let mut mismatch = AssistantTurnCapture::default();
        mismatch.observe_message_start(&start()).unwrap();
        mismatch.observe_text_delta("stream", 16).unwrap();
        assert!(mismatch
            .observe_message_end(&end("changed", "stop"), 16)
            .is_err());

        let mut failed = AssistantTurnCapture::default();
        failed.observe_message_start(&start()).unwrap();
        failed.observe_text_delta("partial", 16).unwrap();
        failed
            .observe_message_end(&end("partial", "length"), 16)
            .unwrap()
            .unwrap();
        assert!(failed.finish_settled().is_err());
    }

    #[test]
    fn tool_use_may_be_followed_by_a_final_successful_message() {
        let mut capture = AssistantTurnCapture::default();
        capture.observe_message_start(&start()).unwrap();
        capture
            .observe_message_end(
                &json!({
                    "type":"message_end",
                    "message": {
                        "role":"assistant",
                        "content":[{"type":"toolCall","id":"call-a","name":"world_read","arguments":{}}],
                        "stopReason":"toolUse"
                    }
                }),
                16,
            )
            .unwrap()
            .unwrap();
        assert!(capture.finish_settled().is_err());
        capture.observe_message_start(&start()).unwrap();
        capture.observe_text_delta("done", 16).unwrap();
        capture
            .observe_message_end(&end("done", "stop"), 16)
            .unwrap()
            .unwrap();
        assert_eq!(capture.finish_settled().unwrap(), "done");
    }
}
