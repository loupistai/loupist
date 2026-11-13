//! Approvals a running Chat waits for (composer spec D3).
//!
//! When an interactive turn calls a read-only custom-connection tool that has no
//! approval yet, the tool executor parks the call here, shows an approval card in
//! the reply's progress, and resumes on the user's decision. Decisions arrive
//! through `chat_approval_decide`; "Always" is stored by the connection service
//! per Loupist, connection and tool, like an approval given in Settings.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::domain::DomainError;

/// How long a call waits for the user before the run continues without it.
pub const APPROVAL_WAIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const MAX_TARGET_CHARS: usize = 160;
const MAX_TARGET_PARTS: usize = 3;
const MAX_PART_CHARS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Once,
    Always,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Waiting,
    AllowedOnce,
    AlwaysAllowed,
    Declined,
    /// The run stopped or the wait ran out before the user decided.
    Expired,
}

/// One approval the reply asked for, shown as a card at its step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatApproval {
    #[serde(rename = "requestId")]
    pub request_id: String,
    /// The progress step that waits; absent when the call carried no tool call id.
    #[serde(rename = "itemId", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub item_id: Option<String>,
    #[serde(rename = "connectionId")]
    pub connection_id: String,
    #[serde(rename = "connectionName")]
    pub connection_name: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    /// Plain values from the call's arguments (company, period), never raw JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    pub state: ApprovalState,
    #[serde(rename = "requestedAtMs")]
    pub requested_at_ms: i64,
}

impl ChatApproval {
    pub fn validate(&self, durable: bool) -> Result<(), DomainError> {
        let short = |value: &str, max: usize| {
            !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
        };
        if !valid_request_id(&self.request_id)
            || !short(&self.connection_id, 128)
            || !short(&self.connection_name, 200)
            || !short(&self.tool_name, 200)
            || self
                .target
                .as_deref()
                .is_some_and(|target| !short(target, MAX_TARGET_CHARS * 4))
            || self.requested_at_ms < 0
            || (durable && self.state == ApprovalState::Waiting)
        {
            return Err(DomainError::Invalid("chat approval is invalid"));
        }
        Ok(())
    }
}

pub(super) fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// A short, human line from the call's arguments: string and number values only,
/// skipping anything that looks like a secret.
pub fn target_summary(arguments: &Value) -> Option<String> {
    let object = arguments.as_object()?;
    let mut parts = Vec::new();
    for (key, value) in object {
        let lower = key.to_ascii_lowercase();
        if ["key", "token", "secret", "password", "auth", "cookie"]
            .iter()
            .any(|word| lower.contains(word))
        {
            continue;
        }
        let text = match value {
            Value::String(text) => text.trim().to_owned(),
            Value::Number(number) => number.to_string(),
            _ => continue,
        };
        if text.is_empty() || text.contains(['\n', '\0']) {
            continue;
        }
        parts.push(text.chars().take(MAX_PART_CHARS).collect::<String>());
        if parts.len() == MAX_TARGET_PARTS {
            break;
        }
    }
    let joined = parts.join(" · ");
    (!joined.is_empty()).then(|| joined.chars().take(MAX_TARGET_CHARS).collect())
}

struct Pending {
    token: Arc<()>,
    loupist_id: String,
    sender: oneshot::Sender<ApprovalDecision>,
}

fn registry() -> &'static Mutex<HashMap<String, Pending>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// A parked response removes its own registry entry even when its future is cancelled.
pub struct Response<T> {
    receiver: oneshot::Receiver<T>,
    cleanup: Box<dyn Fn() + Send + Sync>,
}

impl<T> Response<T> {
    pub(super) fn new(
        receiver: oneshot::Receiver<T>,
        cleanup: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            receiver,
            cleanup: Box::new(cleanup),
        }
    }

    #[cfg(test)]
    pub fn try_recv(&mut self) -> Result<T, oneshot::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl<T> Future for Response<T> {
    type Output = Result<T, oneshot::error::RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(cx)
    }
}

impl<T> Drop for Response<T> {
    fn drop(&mut self) {
        (self.cleanup)();
    }
}

/// Parks a call until the user decides, the wait expires or the caller cancels.
pub fn register(request_id: &str, loupist_id: &str) -> Response<ApprovalDecision> {
    let (sender, receiver) = oneshot::channel();
    let token = Arc::new(());
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            request_id.to_owned(),
            Pending {
                token: token.clone(),
                loupist_id: loupist_id.to_owned(),
                sender,
            },
        );
    let request_id = request_id.to_owned();
    Response::new(receiver, move || {
        let mut entries = registry().lock().unwrap_or_else(|error| error.into_inner());
        if entries
            .get(&request_id)
            .is_some_and(|entry| Arc::ptr_eq(&entry.token, &token))
        {
            entries.remove(&request_id);
        }
    })
}

pub fn forget(request_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(request_id);
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecideError {
    /// The request was decided, expired or never existed.
    NotWaiting,
}

/// Delivers the user's decision to the waiting call of this Loupist.
pub fn decide(
    loupist_id: &str,
    request_id: &str,
    decision: ApprovalDecision,
) -> Result<(), DecideError> {
    let mut pending = registry().lock().unwrap_or_else(|error| error.into_inner());
    // Another Loupist's request id is not this caller's to answer.
    if pending
        .get(request_id)
        .is_none_or(|entry| entry.loupist_id != loupist_id)
    {
        return Err(DecideError::NotWaiting);
    }
    let entry = pending.remove(request_id).expect("checked above");
    entry
        .sender
        .send(decision)
        .map_err(|_| DecideError::NotWaiting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_decision_reaches_only_the_waiting_call_of_the_same_loupist() {
        let saved: ChatApproval = serde_json::from_value(json!({"requestId":"req-0",
            "connectionId":"custom-a","connectionName":"A","toolName":"read","state":"declined",
            "requestedAtMs":1}))
        .unwrap();
        assert_eq!(serde_json::to_value(&saved).unwrap()["state"], "declined");

        let mut receiver = register("req-1", "loupist-a");
        assert_eq!(
            decide("loupist-b", "req-1", ApprovalDecision::Always),
            Err(DecideError::NotWaiting)
        );
        decide("loupist-a", "req-1", ApprovalDecision::Once).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), ApprovalDecision::Once);
        assert_eq!(
            decide("loupist-a", "req-1", ApprovalDecision::Once),
            Err(DecideError::NotWaiting)
        );
    }

    #[test]
    fn cancelling_a_wait_removes_only_its_registration() {
        let waiting = register("approval-cancel", "loupist-a");
        assert!(registry().lock().unwrap().contains_key("approval-cancel"));
        drop(waiting);
        assert!(!registry().lock().unwrap().contains_key("approval-cancel"));
        let old = register("approval-replaced", "loupist-a");
        let mut current = register("approval-replaced", "loupist-a");
        drop(old);
        decide("loupist-a", "approval-replaced", ApprovalDecision::Once).unwrap();
        assert_eq!(current.try_recv().unwrap(), ApprovalDecision::Once);
    }

    #[test]
    fn the_target_shows_plain_values_and_never_secrets() {
        let summary = target_summary(&json!({
            "company": "Hanul Precision", "period": "2026 Q3", "api_key": "sk-test-123", "limit": 5, "filters": {"x": 1}
        }))
        .unwrap();
        assert!(summary.contains("Hanul Precision"));
        assert!(summary.contains("2026 Q3"));
        assert!(!summary.contains("sk-test"));
        assert!(target_summary(&json!({})).is_none());
    }
}
