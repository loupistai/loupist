//! Asking the user from inside a running tool call (composer spec D3): a
//! custom connection's unapproved read-only tool, or a new website after
//! third-party text.
use super::*;
use crate::chat::approvals::{self, ApprovalDecision, ApprovalKind, ApprovalState, ChatApproval};

impl AppToolExecutor {
    /// Shows `approval` as a waiting card at the calling step and waits for the
    /// user, the wait limit or the run stopping; the card then shows the
    /// outcome. None when nobody can answer (an unattended run, a run without
    /// cards) or nobody did. A website is allowed for this turn only, so
    /// "always" counts as once.
    pub(super) async fn ask_user(&self, mut approval: ChatApproval) -> Option<ApprovalDecision> {
        if self.capture.unattended_run_id.is_some() {
            return None;
        }
        let sink = self.capture.approval_sink.as_ref()?;
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
        let decision = match (approval.kind, decision) {
            (Some(ApprovalKind::WebSite), Some(ApprovalDecision::Always)) => {
                Some(ApprovalDecision::Once)
            }
            (_, decision) => decision,
        };
        approval.state = match decision {
            Some(ApprovalDecision::Once) => ApprovalState::AllowedOnce,
            Some(ApprovalDecision::Always) => ApprovalState::AlwaysAllowed,
            Some(ApprovalDecision::Deny) => ApprovalState::Declined,
            None => ApprovalState::Expired,
        };
        sink(call_id, approval);
        decision
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
