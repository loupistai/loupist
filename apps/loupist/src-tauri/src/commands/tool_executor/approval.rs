//! Asking the user from inside a running tool call (composer spec D3): a
//! custom connection's unapproved read-only tool, or a new website after
//! third-party text.
use std::sync::atomic::Ordering;

use super::*;
use crate::chat::approvals::{self, ApprovalDecision, ApprovalKind, ApprovalState, ChatApproval};

/// How a card ended, as the caller must tell them apart: a decline pauses a
/// kind for the reply, an unasked card refuses only this call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::commands::tool_executor) enum AskOutcome {
    Answered(ApprovalDecision),
    /// The card was shown, and the wait ran out or the run stopped.
    NoAnswer,
    /// No card could be shown: an unattended run, a run without cards, or a
    /// reply that already holds its limit of cards.
    Unasked,
}

impl AppToolExecutor {
    /// Shows `approval` as a waiting card at the calling step and waits for the
    /// user, the wait limit or the run stopping; the card then shows the
    /// outcome. A website is allowed for this reply only, so "always" counts
    /// as once for it.
    pub(super) async fn ask_user(&self, mut approval: ChatApproval) -> AskOutcome {
        if self.capture.unattended_run_id.is_some() {
            return AskOutcome::Unasked;
        }
        let Some(sink) = self.capture.approval_sink.as_ref() else {
            return AskOutcome::Unasked;
        };
        // The stream loop drops a card past the limit, so the call would wait
        // on a card nobody sees; count here instead.
        let shown = self.capture.approvals_shown.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |count| (count < crate::chat::progress::MAX_PROGRESS_APPROVALS).then_some(count + 1),
        );
        if shown.is_err() {
            return AskOutcome::Unasked;
        }
        let call_id = current_tool_call_id();
        let request_id = approval.request_id.clone();
        let receiver = approvals::register(&request_id, &self.loupist_id);
        sink(call_id.clone(), approval.clone());
        let decision = tokio::select! {
            decision = receiver => decision.ok(),
            _ = tokio::time::sleep(approvals::APPROVAL_WAIT) => None,
            _ = self.capture.search_cancel.cancelled() => None,
        };
        approvals::forget(&request_id);
        let decision = match decision {
            Some(ApprovalDecision::Always) if approval.kind.is_some() => {
                Some(ApprovalDecision::Once)
            }
            decision => decision,
        };
        approval.state = match decision {
            Some(ApprovalDecision::Once) => ApprovalState::AllowedOnce,
            Some(ApprovalDecision::Always) => ApprovalState::AlwaysAllowed,
            Some(ApprovalDecision::Deny) => ApprovalState::Declined,
            None => ApprovalState::Expired,
        };
        sink(call_id, approval);
        match decision {
            Some(decision) => AskOutcome::Answered(decision),
            None => AskOutcome::NoAnswer,
        }
    }
}

/// A new website card: `target` is the site and path the fetch would open.
pub(super) fn web_site_approval(target: String) -> ChatApproval {
    ChatApproval {
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: Some(ApprovalKind::WebSite),
        item_id: None,
        connection_id: "web".into(),
        connection_name: "Web".into(),
        tool_name: "web_fetch".into(),
        target: Some(target),
        state: ApprovalState::Waiting,
        requested_at_ms: chrono::Utc::now().timestamp_millis(),
    }
}
