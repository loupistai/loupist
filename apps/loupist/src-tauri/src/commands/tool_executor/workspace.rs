use super::authority::bounded_text;
use super::*;
use crate::agent_harness::native_policy::{
    self, NativeDecision, NativeRefusal, NativeRequest, NativeRoots, NativeTool,
};
use crate::custom_connections::taint::{HARD_LINK_WRITE_REFUSAL, INSTRUCTION_WRITE_REFUSAL};

const SEARCH_COVERS_APP_DATA_NEXT: &str = "This search root contains the app's private data folder, so it is not searched. Search a narrower folder, such as the workspace or one of its subfolders.";
const UNRESOLVABLE_NEXT: &str = "This path goes through a symbolic link that loops or cannot be read, so it is not touched. Use the real file's path.";
const CREDENTIALS_NEXT: &str = "Do not retry this path. Credential stores, container runtime folders and shell startup files are never read or written by the Loupist.";
const GIT_DIR_NEXT: &str = "Do not retry this path. Native file tools do not read or write git metadata, config or hooks. Work with research files outside `.git`.";
const HARD_LINK_NEXT: &str = "This file has another name on disk and may alias protected data. Ask the user for an ordinary copy; do not retry this alias.";

/// The native policy's state for one turn: the folders it classifies against
/// and the current third-party exposure.
#[derive(Default)]
pub(in crate::commands) struct NativeGate {
    roots: Option<NativeRoots>,
}

impl NativeGate {
    pub(in crate::commands) fn for_turn(roots: NativeRoots) -> Self {
        Self { roots: Some(roots) }
    }

    pub(in crate::commands) fn roots(&self) -> Option<&NativeRoots> {
        self.roots.as_ref()
    }

    fn decide(
        &self,
        request: &NativeRequest,
        taint: &crate::custom_connections::taint::TurnTaint,
    ) -> Result<NativeDecision, BrokerError> {
        let exposure = taint.exposure();
        let roots = self.roots.as_ref().ok_or_else(|| {
            BrokerError::Execution("native file tools are unavailable in this run".into())
        })?;
        Ok(native_policy::decide(request, roots, exposure))
    }
}

fn refusal_error(refusal: NativeRefusal) -> BrokerError {
    match refusal {
        NativeRefusal::AppPrivate => BrokerError::ProtectedPath,
        NativeRefusal::Credentials => BrokerError::ProtectedPath.with_next(CREDENTIALS_NEXT),
        NativeRefusal::GitDir => BrokerError::ProtectedPath.with_next(GIT_DIR_NEXT),
        NativeRefusal::HardLink => BrokerError::ProtectedPath.with_next(HARD_LINK_NEXT),
        NativeRefusal::SearchCoversAppData => {
            BrokerError::ProtectedPath.with_next(SEARCH_COVERS_APP_DATA_NEXT)
        }
        NativeRefusal::Unresolvable => BrokerError::ProtectedPath.with_next(UNRESOLVABLE_NEXT),
        NativeRefusal::Outside => BrokerError::OutsideWorkspace,
        NativeRefusal::InstructionsAfterUntrustedText => {
            BrokerError::UntrustedTextPaused.with_next(INSTRUCTION_WRITE_REFUSAL)
        }
        NativeRefusal::HardLinkAfterUntrustedText => {
            BrokerError::UntrustedTextPaused.with_next(HARD_LINK_WRITE_REFUSAL)
        }
    }
}

/// The `(dev, ino)` of the file a read was allowed on, as strings so that no
/// JSON number loses precision: the extension opens the file, compares the
/// descriptor's identity and refuses another file. Null when no file is at
/// the path yet, so nothing that appears later is read. None on a host that
/// reports no file identity, where the extension reads by path.
#[cfg(unix)]
fn read_identity(path: &std::path::Path) -> Option<Value> {
    use std::os::unix::fs::MetadataExt;
    Some(match std::fs::metadata(path) {
        Ok(metadata) => json!({
            "dev": metadata.dev().to_string(),
            "ino": metadata.ino().to_string(),
        }),
        Err(_) => Value::Null,
    })
}

#[cfg(not(unix))]
fn read_identity(_path: &std::path::Path) -> Option<Value> {
    None
}

impl AppToolExecutor {
    /// Decides one native call without waiting on anyone. The canonical
    /// target comes back so the extension can pin a read, write or edit to
    /// it, with the file's identity for a read; a search runs on it.
    pub(super) async fn native_authorize(&self, params: &Value) -> Result<Value, BrokerError> {
        let request = NativeRequest::parse(params).ok_or(BrokerError::Malformed)?;
        let mut result = json!({"allowed": true});
        match self
            .capture
            .native
            .decide(&request, &self.capture.turn_taint)?
        {
            NativeDecision::AllowFile { resolved } => {
                if matches!(
                    request.tool,
                    NativeTool::Read | NativeTool::Write | NativeTool::Edit
                ) {
                    if let Some(identity) = read_identity(&resolved) {
                        result["identity"] = identity;
                    }
                }
                if matches!(
                    request.tool,
                    NativeTool::Ls | NativeTool::Find | NativeTool::Grep
                ) {
                    let roots = self
                        .capture
                        .native
                        .roots
                        .as_ref()
                        .ok_or(BrokerError::ProtectedPath)?;
                    result["search"] = native_policy::search::snapshot(
                        &resolved,
                        roots,
                        self.capture.turn_taint.exposure(),
                        request.tool != NativeTool::Ls,
                    )
                    .map_err(|error| BrokerError::Execution(error.to_string()))?;
                }
                result["resolved"] = json!(resolved);
            }
            NativeDecision::Refuse(refusal) => return Err(refusal_error(refusal)),
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

    pub(super) async fn python_run(&self, params: Value) -> Result<Value, BrokerError> {
        let (code, timeout) = crate::python_run::parse_params(&params)?;
        let roots = self
            .capture
            .native
            .roots()
            .cloned()
            .ok_or_else(|| BrokerError::Execution("Python execution is unavailable".into()))?;
        crate::python_run::execute(
            &roots,
            &self.state.artifacts.app_data_dir,
            &code,
            timeout,
            self.capture.search_cancel.clone(),
        )
        .await
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
