//! Asking the user from inside a running tool call (composer spec D3): a
//! custom connection's unapproved read-only tool, a new website after
//! third-party text, or a native command or file access the policy sends to
//! a card.
use std::sync::atomic::Ordering;

use super::*;
use crate::agent_harness::native_policy::{AskKind, CommandScope, Grant, NativeAsk};
use crate::chat::approvals::{
    self, ApprovalDecision, ApprovalGrant, ApprovalKind, ApprovalState, ChatApproval,
};

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
    /// outcome. A website, a file access and an instruction write are allowed
    /// for this reply only, so "always" counts as once for them.
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
        let once_only = !matches!(approval.kind, None | Some(ApprovalKind::Command));
        let decision = match decision {
            Some(ApprovalDecision::Always) if once_only => Some(ApprovalDecision::Once),
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
        detail: None,
        grant: None,
        state: ApprovalState::Waiting,
        requested_at_ms: chrono::Utc::now().timestamp_millis(),
    }
}

/// A native policy card: the tool, its target line and, for a command, the
/// whole command and the scope a blanket Allow covers.
pub(super) fn native_approval(ask: &NativeAsk) -> ChatApproval {
    let grant = match &ask.grant {
        Grant::Command {
            scope: CommandScope::Reply,
            ..
        } => Some(ApprovalGrant::Reply),
        Grant::Command {
            scope: CommandScope::Chat,
            ..
        } => Some(ApprovalGrant::Chat),
        _ => None,
    };
    ChatApproval {
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: Some(match ask.kind {
            AskKind::Command => ApprovalKind::Command,
            AskKind::FileRead => ApprovalKind::FileRead,
            AskKind::FileWrite => ApprovalKind::FileWrite,
            AskKind::Instructions => ApprovalKind::Instructions,
        }),
        item_id: None,
        connection_id: "workspace".into(),
        connection_name: "Workspace".into(),
        tool_name: ask.tool.name().into(),
        target: Some(ask.target.clone()),
        detail: ask.detail.clone(),
        grant,
        state: ApprovalState::Waiting,
        requested_at_ms: chrono::Utc::now().timestamp_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_harness::native_policy::{
        decide, Exposure, NativeDecision, NativeGrants, NativeRequest, NativeRoots,
    };
    use serde_json::json;

    /// A card the reply could not save would fail the whole reply, so every
    /// native card settles into something `validate` accepts: a bounded,
    /// non-blank target and detail, whatever the command or path looks like.
    #[test]
    fn every_native_card_settles_into_a_saveable_approval() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let app = root.join("app");
        let roots = NativeRoots::new(
            &root.join("workspace"),
            &app,
            &app,
            &root.join("workspace/.loupist"),
            &app.join("scratch"),
            &app.join("attachments"),
            Some(&root.join("home")),
        );
        let settled = |params: serde_json::Value| {
            let request = NativeRequest::parse(&params).unwrap();
            let NativeDecision::Ask(ask) = decide(
                &request,
                &roots,
                Exposure::ThisTurn,
                &NativeGrants::default(),
                false,
            ) else {
                panic!("{params} asks after third-party text");
            };
            let mut card = native_approval(&ask);
            card.state = ApprovalState::AllowedOnce;
            card.validate(true).unwrap_or_else(|_| panic!("{card:?}"));
            card
        };
        let hangul = "한".repeat(300);
        for (command, target) in [
            ("\n\npython3 build.py\nls", "python3 build.py"),
            ("  \n\t\n", "(blank command)"),
        ] {
            let card = settled(json!({"name": "bash", "command": command}));
            assert_eq!(card.target.as_deref(), Some(target), "{command:?}");
        }
        let card = settled(json!({"name": "bash", "command": format!("{hangul}\nls")}));
        let target = card.target.unwrap();
        assert!(target.len() <= 600 && target.ends_with('…'), "{target}");
        assert!(target.starts_with(&"한".repeat(190)));
        let path = root.join("home/Documents").join(format!("{hangul}.csv"));
        let card = settled(json!({"name": "read", "path": path}));
        let target = card.target.unwrap();
        assert!(target.len() <= 600 && target.starts_with('…'), "{target}");
        assert!(target.ends_with("한.csv"));
    }
}
