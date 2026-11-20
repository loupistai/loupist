//! `pasted_capture`: original material the user pasted into this Chat (a
//! filing excerpt, an article, a notice, a table), captured as a
//! user-provided run result that the answer can cite and `source_save` can
//! keep. The host copies the bytes from the user's own message, so the model
//! can locate text but never supply it: its own words, a tool result and
//! another Chat's messages are not found.
use super::arguments::{bounded_text, optional_bounded_text};
use super::*;
use crate::domain::{ChatMessage, ChatMessageStatus, ChatRole};

/// What a capture records about the message its text came from.
pub(super) const PASTED_MESSAGE_FIELD: &str = "pasted";

const CAPTURED_NEXT: &str = "Captured, not retained. If the answer takes facts from this material or the user asked to keep it, call source_save with this result_ref before replying. Link what the answer takes from it to citation_url.";

impl AppToolExecutor {
    pub(super) fn pasted_capture(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        // The Chat whose messages are searched is the host's binding, never
        // a tool argument.
        if self.capture.bound_chat_id.as_deref() != Some(policy.session_id.as_str())
            || self.capture.unattended_run_id.is_some()
            || self.capture.host_handoff
        {
            return Err(BrokerError::MethodDenied);
        }
        let object = exact_object(&params, &["text"], &["url", "publisher"])?;
        let wanted = bounded_text(object.get("text"), 1, crate::commands::MAX_PROMPT_BYTES)?;
        let url = optional_bounded_text(object.get("url"), 1, 2_048)?;
        let publisher = optional_bounded_text(object.get("publisher"), 1, 200)?;
        let chat = self
            .state
            .store
            .get_chat(&policy.session_id)
            .map_err(store_error)?
            .ok_or_else(|| BrokerError::Execution("Chat is unavailable".into()))?;
        let typed: Vec<&ChatMessage> = chat
            .messages
            .iter()
            .rev()
            .filter(|message| typed_by_user(message))
            .collect();
        let (message, text) = typed
            .iter()
            .find_map(|message| verbatim(&message.content, &wanted).map(|text| (*message, text)))
            .ok_or_else(|| {
                BrokerError::invalid(
                    "text",
                    "not found in a message the user sent in this Chat. Copy the passage exactly from the user's message, changing nothing but spacing; your own text, assistant replies, tool results, attachments and other Chats cannot be captured here",
                )
            })?;
        if let Some(url) = &url {
            if evidence::safe_url(url).is_none() {
                return Err(BrokerError::invalid(
                    "url",
                    "an http or https link without user information",
                ));
            }
            if !typed
                .iter()
                .any(|message| message.content.contains(url.as_str()))
            {
                return Err(BrokerError::invalid(
                    "url",
                    "the user did not write this link in this Chat. Pass it exactly as the user wrote it, or omit it",
                ));
            }
        }
        if let Some(publisher) = &publisher {
            let folded = fold(publisher).text.to_lowercase();
            if folded.trim().is_empty()
                || !typed.iter().any(|message| {
                    fold(&message.content)
                        .text
                        .to_lowercase()
                        .contains(folded.trim())
                })
            {
                return Err(BrokerError::invalid(
                    "publisher",
                    "the user did not name this publisher in this Chat. Pass the name as the user wrote it, or omit it",
                ));
            }
        }
        let pasted_at = DateTime::<Utc>::from_timestamp_millis(message.created_at_ms)
            .ok_or(BrokerError::Malformed)?
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut result = json!({
            "status": "captured",
            "source_class": crate::knowledge::SourceClass::UserProvided.as_str(),
            "content": text,
            "content_sha256": sha256(text.as_bytes()),
            "size_bytes": text.len(),
            "media_type": "text/plain",
            "pasted_at": pasted_at,
            PASTED_MESSAGE_FIELD: {
                "message_id": message.id,
                "created_at_ms": message.created_at_ms,
            },
        });
        if let Some(url) = url {
            result["stated_url"] = json!(url);
        }
        if let Some(publisher) = publisher {
            result["stated_publisher"] = json!(publisher.trim());
        }
        if policy.propose_memory_updates {
            result["next"] = json!(CAPTURED_NEXT);
        }
        Ok(result)
    }
}

/// The origin locator of a user-provided Source the user gave no link for.
/// The renderer shows it in the reader's language.
pub(super) const PASTED_TEXT_LOCATOR: &str = "Pasted text";

impl AppToolExecutor {
    /// Where a `pasted_capture` result was pasted, for `source_save`; `None`
    /// for every other result. The run result is the host's own record of
    /// the user's message, so nothing here comes from model arguments. Memory
    /// keeps no credential the user pasted by mistake.
    pub(super) fn provided_origin(
        &self,
        policy: &ToolPolicy,
        result: &RunResult,
    ) -> Result<Option<crate::knowledge::ProvidedOrigin>, BrokerError> {
        if result.producer.origin != RunResultOrigin::UserProvided {
            return Ok(None);
        }
        if self.capture.bound_chat_id.as_deref() != Some(policy.session_id.as_str()) {
            return Err(BrokerError::MethodDenied);
        }
        let payload = &result.payload;
        let content = payload
            .get("content")
            .and_then(Value::as_str)
            .ok_or(BrokerError::Malformed)?;
        if crate::util::credential_text::contains_credential_text(content) {
            return Err(sensitive_memory_refusal(
                &self.capture,
                ToolMethod::SourceSave,
            ));
        }
        let pasted = &payload[PASTED_MESSAGE_FIELD];
        let text = |key: &str| payload.get(key).and_then(Value::as_str).map(str::to_owned);
        Ok(Some(crate::knowledge::ProvidedOrigin {
            chat_id: policy.session_id.clone(),
            message_id: pasted
                .get("message_id")
                .and_then(Value::as_str)
                .ok_or(BrokerError::Malformed)?
                .to_owned(),
            pasted_at_ms: pasted
                .get("created_at_ms")
                .and_then(Value::as_i64)
                .ok_or(BrokerError::Malformed)?,
            stated_url: text("stated_url"),
            stated_publisher: text("stated_publisher"),
        }))
    }
}

/// A message the user wrote in this Chat: typed, pasted or sent as guidance
/// while a reply ran. A hand-off's opening request, a Map link and a
/// scheduled task's prompt are composed by the host or an earlier reply, so
/// they never count.
fn typed_by_user(message: &ChatMessage) -> bool {
    message.role == ChatRole::User
        && message.status == ChatMessageStatus::Complete
        && !message.host_composed
        && message.handoff_note.is_none()
        && message.handoff_action.is_none()
        && message.scheduled.is_none()
}

/// Text with each run of whitespace read as one space, and, per output
/// character, its byte offset there and the byte range it came from.
struct Folded {
    text: String,
    spans: Vec<(usize, usize, usize)>,
}

fn fold(text: &str) -> Folded {
    let mut folded = String::with_capacity(text.len());
    let mut spans = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        let mut end = start + character.len_utf8();
        if character.is_whitespace() {
            while let Some(&(next, following)) = chars.peek() {
                if !following.is_whitespace() {
                    break;
                }
                end = next + following.len_utf8();
                chars.next();
            }
            spans.push((folded.len(), start, end));
            folded.push(' ');
        } else {
            spans.push((folded.len(), start, end));
            folded.push(character);
        }
    }
    Folded {
        text: folded,
        spans,
    }
}

/// The exact bytes of `message` that `wanted` names, when `wanted` appears in
/// it with only whitespace differing (line breaks, tabs, repeated spaces, or
/// whitespace at either end). The match is returned from the user's message,
/// never from `wanted`.
fn verbatim<'a>(message: &'a str, wanted: &str) -> Option<&'a str> {
    let needle = fold(wanted).text;
    let needle = needle.trim_matches(' ');
    if needle.is_empty() {
        return None;
    }
    let haystack = fold(message);
    let at = haystack.text.find(needle)?;
    let end = at + needle.len();
    let first = haystack
        .spans
        .binary_search_by_key(&at, |span| span.0)
        .ok()?;
    let last = haystack.spans.partition_point(|span| span.0 < end) - 1;
    message.get(haystack.spans[first].1..haystack.spans[last].2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passage_is_found_with_only_whitespace_differing() {
        let message = "솔숲식품이 알린 공시야. 본문은 이렇다.\n\n솔숲식품은 2025년 3월 12일 현재\n수주잔고 수치가 184.6이고   단위는 십억 원이라고 알렸다. 정리해 줘.";
        let found = verbatim(
            message,
            "  솔숲식품은 2025년 3월 12일 현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다. ",
        )
        .unwrap();
        // The bytes are the user's, line break and spacing included.
        assert_eq!(
            found,
            "솔숲식품은 2025년 3월 12일 현재\n수주잔고 수치가 184.6이고   단위는 십억 원이라고 알렸다."
        );
        assert_eq!(verbatim(message, message), Some(message));
    }

    #[test]
    fn a_changed_word_or_number_is_not_verbatim() {
        let message = "Marrow Glass original FY2024 gross margin was 40.0 percent.";
        assert!(verbatim(
            message,
            "Marrow Glass original FY2024 gross margin was 40.0%."
        )
        .is_none());
        assert!(verbatim(message, "gross margin was 41.0 percent").is_none());
        assert!(verbatim(message, "marrow glass original").is_none());
        assert!(verbatim(message, " \n\t ").is_none());
        assert_eq!(
            verbatim(message, "gross margin was 40.0 percent"),
            Some("gross margin was 40.0 percent")
        );
    }
}
