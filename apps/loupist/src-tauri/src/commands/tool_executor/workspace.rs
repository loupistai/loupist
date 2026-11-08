use std::path::PathBuf;

use super::approval::{native_approval, AskOutcome};
use super::authority::bounded_text;
use super::*;
use crate::agent_harness::native_policy::{
    self, AskKind, CommandScope, Grant, NativeAsk, NativeDecision, NativeGrants, NativeRefusal,
    NativeRequest, NativeRoots, NativeTool,
};
use crate::chat::approvals::ApprovalDecision;
use crate::custom_connections::taint::{COMMAND_DECLINED_REFUSAL, INSTRUCTION_WRITE_REFUSAL};

/// A call that could not be shown a card: not a refusal by the user, so the
/// kind is not paused.
const UNASKED_NEXT: &str = "No card could be shown for this call: this reply already holds its limit of approval cards, or nobody can answer one here. The user did not refuse it. Group the remaining command or file work into fewer calls, or finish with what is already available.";
const SEARCH_COVERS_APP_DATA_NEXT: &str = "This search root contains the app's private data folder, so it is not searched. Search a narrower folder, such as the workspace or one of its subfolders.";

/// The native policy's state for one turn: the folders it classifies against,
/// whether earlier replies carried third-party text, the Chat the blanket
/// command grant is keyed by, what the user allowed or turned down in this
/// reply, and one card-at-a-time lock per ask kind.
#[derive(Default)]
pub(in crate::commands) struct NativeGate {
    roots: Option<NativeRoots>,
    carried_untrusted: bool,
    chat: Option<(String, String)>,
    grants: std::sync::Mutex<NativeGrants>,
    ask_locks: [tokio::sync::Mutex<()>; AskKind::COUNT],
}

impl NativeGate {
    pub(in crate::commands) fn for_turn(
        roots: NativeRoots,
        carried_untrusted: bool,
        loupist_id: &str,
        thread_id: &str,
    ) -> Self {
        Self {
            roots: Some(roots),
            carried_untrusted,
            chat: Some((loupist_id.to_owned(), thread_id.to_owned())),
            ..Self::default()
        }
    }

    fn chat_commands(&self) -> bool {
        self.chat.as_ref().is_some_and(|(loupist_id, thread_id)| {
            crate::chat::command_grants::granted(loupist_id, thread_id)
        })
    }

    fn decide(
        &self,
        request: &NativeRequest,
        taint: &crate::custom_connections::taint::TurnTaint,
    ) -> Result<NativeDecision, BrokerError> {
        let exposure = taint.exposure(self.carried_untrusted);
        if request.tool == NativeTool::Bash {
            return Ok(native_policy::decide_command(
                request,
                exposure,
                &self.grants(),
                self.chat_commands(),
            ));
        }
        // A run without turn folders cannot classify a path.
        let roots = self.roots.as_ref().ok_or_else(|| {
            BrokerError::Execution("native file tools are unavailable in this run".into())
        })?;
        Ok(native_policy::decide(
            request,
            roots,
            exposure,
            &self.grants(),
            self.chat_commands(),
        ))
    }

    fn grants(&self) -> std::sync::MutexGuard<'_, NativeGrants> {
        self.grants
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn grant(&self, ask: &NativeAsk, decision: ApprovalDecision) {
        if let (
            Grant::Command {
                scope: CommandScope::Chat,
                ..
            },
            ApprovalDecision::Always,
            Some((loupist_id, thread_id)),
        ) = (&ask.grant, decision, &self.chat)
        {
            crate::chat::command_grants::grant(loupist_id, thread_id);
        }
        self.grants().grant(ask, decision);
    }

    fn decline(&self, kind: AskKind) {
        self.grants().decline(kind);
    }
}

fn refusal_error(refusal: NativeRefusal) -> BrokerError {
    match refusal {
        NativeRefusal::AppPrivate => BrokerError::ProtectedPath,
        NativeRefusal::SearchCoversAppData => {
            BrokerError::ProtectedPath.with_next(SEARCH_COVERS_APP_DATA_NEXT)
        }
        NativeRefusal::Declined(kind) => declined_error(kind),
        NativeRefusal::OutboundPaused => BrokerError::OutboundPaused,
    }
}

/// A command decline keeps the paused step the Chat already explains; a file
/// decline is the user's own choice.
fn declined_error(kind: AskKind) -> BrokerError {
    match kind {
        AskKind::Command => BrokerError::UntrustedTextPaused.with_next(COMMAND_DECLINED_REFUSAL),
        AskKind::Instructions => BrokerError::UserDeclined.with_next(INSTRUCTION_WRITE_REFUSAL),
        AskKind::FileRead | AskKind::FileWrite => BrokerError::UserDeclined,
    }
}

impl AppToolExecutor {
    /// Decides one native call: allowed calls never wait; a call the policy
    /// sends to a card takes its kind's lock, decides again (an earlier card
    /// may have covered it), asks, and records the answer. The canonical
    /// target comes back so the extension can pin a write to it.
    pub(super) async fn native_authorize(&self, params: &Value) -> Result<Value, BrokerError> {
        let request = NativeRequest::parse(params).ok_or(BrokerError::Malformed)?;
        let gate = &self.capture.native;
        let taint = &self.capture.turn_taint;
        let mut held = None;
        let mut resolved: Option<PathBuf> = None;
        for _ in 0..4 {
            match gate.decide(&request, taint)? {
                NativeDecision::Allow { resolved: target } => {
                    resolved = target;
                    held = None;
                    break;
                }
                NativeDecision::Refuse(refusal) => return Err(refusal_error(refusal)),
                NativeDecision::Ask(ask) => {
                    if held.is_none() {
                        held = Some(gate.ask_locks[ask.kind.index()].lock().await);
                        continue;
                    }
                    match self.ask_user(native_approval(&ask)).await {
                        AskOutcome::Answered(
                            decision @ (ApprovalDecision::Once | ApprovalDecision::Always),
                        ) => gate.grant(&ask, decision),
                        AskOutcome::Answered(ApprovalDecision::Deny) | AskOutcome::NoAnswer => {
                            gate.decline(ask.kind);
                            return Err(declined_error(ask.kind));
                        }
                        AskOutcome::Unasked => {
                            return Err(BrokerError::Execution(
                                "the user could not be shown a card for this call".into(),
                            )
                            .with_next(UNASKED_NEXT))
                        }
                    }
                }
            }
        }
        if held.is_some() {
            return Err(BrokerError::Execution(
                "native authorization did not settle".into(),
            ));
        }
        let mut result = json!({"allowed": true});
        if let Some(resolved) = resolved {
            result["resolved"] = json!(resolved);
        }
        if request.tool == NativeTool::Read {
            if let Some(path) = request.target.as_deref() {
                if crate::commands::export_trust::tagged_file(&self.state, path)
                    .map_err(|error| BrokerError::Execution(error.to_string()))?
                {
                    self.capture.turn_taint.mark_untrusted_text();
                    result["untrusted_text"] =
                        crate::custom_connections::taint::native_read_delimiters();
                }
            }
        }
        Ok(result)
    }

    pub(super) fn workspace_capture(&self, params: Value) -> Result<Value, BrokerError> {
        let object = exact_object(&params, &["path"], &["expected_sha256"])?;
        let path = bounded_text(object.get("path"), 1, 8 * 1024)?;
        let expected = object
            .get("expected_sha256")
            .map(|value| bounded_text(Some(value), 64, 64))
            .transpose()?;
        if expected
            .as_ref()
            .is_some_and(|hash| !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(BrokerError::Malformed);
        }
        let profile = self
            .state
            .store
            .get_loupist(&self.loupist_id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?
            .ok_or_else(|| BrokerError::Execution("Loupist does not exist".into()))?;
        let root = crate::fs::loupist_root::profile_workspace(&profile)
            .map_err(|error| BrokerError::Execution(error.message))?;
        let file = crate::fs::workspace_files::exact_text(&root, &path, expected.as_deref())
            .map_err(|error| BrokerError::Execution(error.message))?;
        let untrusted = crate::commands::export_trust::contains(
            &self.state,
            file.content_sha256
                .as_deref()
                .ok_or(BrokerError::Malformed)?,
        )
        .map_err(|error| BrokerError::Execution(error.message))?;
        Ok(json!({
            "path": file.path,
            "content": file.content,
            "content_sha256": file.content_sha256,
            "media_type": file.media_type,
            "size_bytes": file.size_bytes,
            "source_class": "user_file",
            "trust": if untrusted { "third_party_text" } else { "user_file" },
            "warnings": ["Local file snapshot. Authorship and external origin are unverified; this does not establish independent corroboration."],
        }))
    }
}
