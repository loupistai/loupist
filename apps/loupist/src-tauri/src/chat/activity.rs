//! Durable, bounded local-tool inspection outside the model transcript. The
//! progress stream carries references; opening one never changes model context.

use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    app::CommandError,
    chat::progress::ChatProgressStatus,
    fs::secure_delete::{PrivateDirectoryGuard, SecureDeletionRoot},
};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
// JSON can encode one control byte as a six-byte Unicode escape.
const MAX_RECORD_BYTES: usize = 6 * (MAX_INPUT_BYTES + MAX_OUTPUT_BYTES) + 16 * 1024;
const UPDATE_INTERVAL_MS: i64 = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatActivityDetail {
    pub id: String,
    pub tool_name: String,
    pub status: ChatProgressStatus,
    pub input: String,
    pub output: String,
    pub truncated: bool,
    pub file_path: Option<String>,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredActivity {
    loupist_id: String,
    thread_id: String,
    run_id: String,
    detail: ChatActivityDetail,
}

struct ActiveActivity {
    stored: StoredActivity,
    last_write_ms: i64,
}

pub(crate) struct ActivityRecorder {
    directory: PrivateDirectoryGuard,
    path: PathBuf,
    workspace: PathBuf,
    loupist_id: String,
    thread_id: String,
    run_id: String,
    active: HashMap<String, ActiveActivity>,
}

#[derive(Debug)]
pub(crate) struct ActivityReference {
    pub detail_id: String,
    pub file_path: Option<String>,
}

impl ActivityRecorder {
    pub(crate) fn new(
        root: &SecureDeletionRoot,
        loupist_id: &str,
        thread_id: &str,
        run_id: &str,
        workspace: &Path,
    ) -> Result<Self, CommandError> {
        let relative = directory_relative(loupist_id, thread_id)?;
        Ok(Self {
            directory: root.ensure_private_subdirectory(&relative)?,
            path: root.absolute_path(&relative)?,
            workspace: workspace.into(),
            loupist_id: loupist_id.into(),
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            active: HashMap::new(),
        })
    }

    pub(crate) fn start(
        &mut self,
        call_id: &str,
        tool_name: &str,
        args: &Value,
        at_ms: i64,
    ) -> Result<Option<ActivityReference>, CommandError> {
        if !matches!(
            tool_name,
            "read" | "write" | "edit" | "ls" | "find" | "grep" | "run_python"
        ) {
            return Ok(None);
        }
        if self.active.contains_key(call_id) {
            return Err(CommandError::conflict("duplicate local tool activity"));
        }
        let raw_input = serde_json::to_string_pretty(args).map_err(internal)?;
        let file_path = if matches!(tool_name, "read" | "write" | "edit") {
            args.get("path")
                .and_then(Value::as_str)
                .and_then(|path| workspace_relative(&self.workspace, path))
        } else {
            None
        };
        let detail = ChatActivityDetail {
            id: uuid::Uuid::new_v4().simple().to_string(),
            tool_name: tool_name.into(),
            status: ChatProgressStatus::Running,
            input: bound(&raw_input, MAX_INPUT_BYTES).into(),
            output: String::new(),
            truncated: raw_input.len() > MAX_INPUT_BYTES,
            file_path,
            started_at_ms: at_ms.max(0),
            finished_at_ms: None,
        };
        let reference = ActivityReference {
            detail_id: detail.id.clone(),
            file_path: detail.file_path.clone(),
        };
        let stored = StoredActivity {
            loupist_id: self.loupist_id.clone(),
            thread_id: self.thread_id.clone(),
            run_id: self.run_id.clone(),
            detail,
        };
        write_record(&self.directory, &self.path, &stored, true)?;
        self.active.insert(
            call_id.into(),
            ActiveActivity {
                stored,
                last_write_ms: at_ms,
            },
        );
        Ok(Some(reference))
    }

    pub(crate) fn update(
        &mut self,
        call_id: &str,
        result: &Value,
        at_ms: i64,
    ) -> Result<bool, CommandError> {
        let Some(active) = self.active.get_mut(call_id) else {
            return Ok(false);
        };
        assign_output(&mut active.stored.detail, result);
        if at_ms.saturating_sub(active.last_write_ms) >= UPDATE_INTERVAL_MS {
            write_record(&self.directory, &self.path, &active.stored, false)?;
            active.last_write_ms = at_ms;
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn finish(
        &mut self,
        call_id: &str,
        result: &Value,
        failed: bool,
        at_ms: i64,
    ) -> Result<bool, CommandError> {
        let Some(mut active) = self.active.remove(call_id) else {
            return Ok(false);
        };
        assign_output(&mut active.stored.detail, result);
        let python_failed = active.stored.detail.tool_name == "run_python"
            && python_result(result).is_some_and(|value| !python_completed(&value));
        active.stored.detail.status = if failed || python_failed {
            ChatProgressStatus::Failed
        } else {
            ChatProgressStatus::Succeeded
        };
        active.stored.detail.finished_at_ms = Some(at_ms.max(active.stored.detail.started_at_ms));
        write_record(&self.directory, &self.path, &active.stored, true)?;
        Ok(true)
    }

    pub(crate) fn finish_remaining(
        &mut self,
        stopped: bool,
        at_ms: i64,
    ) -> Result<Vec<String>, CommandError> {
        let active = std::mem::take(&mut self.active);
        let mut finished = Vec::new();
        for (call_id, mut activity) in active {
            finished.push(call_id);
            activity.stored.detail.status = if stopped {
                ChatProgressStatus::Stopped
            } else {
                ChatProgressStatus::Failed
            };
            activity.stored.detail.finished_at_ms =
                Some(at_ms.max(activity.stored.detail.started_at_ms));
            write_record(&self.directory, &self.path, &activity.stored, true)?;
        }
        Ok(finished)
    }
}

pub(crate) fn read(
    root: &SecureDeletionRoot,
    loupist_id: &str,
    thread_id: &str,
    id: &str,
    active_run: impl FnOnce(&str) -> bool,
) -> Result<ChatActivityDetail, CommandError> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CommandError::invalid("tool activity reference is invalid"));
    }
    let relative = directory_relative(loupist_id, thread_id)?;
    let directory = root.open_private_subdirectory(&relative)?;
    let filename = format!("{id}.json");
    #[cfg(unix)]
    let mut file = {
        use rustix::fs::{openat, Mode, OFlags};
        File::from(
            openat(
                directory.file(),
                filename.as_str(),
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(internal)?,
        )
    };
    #[cfg(windows)]
    let mut file = crate::fs::windows::open_atomic_snapshot_no_reparse(
        &root.absolute_path(&relative)?.join(&filename),
    )
    .map_err(internal)?;
    #[cfg(not(any(unix, windows)))]
    let mut file = File::open(root.absolute_path(&relative)?.join(&filename)).map_err(internal)?;
    let metadata = file.metadata().map_err(internal)?;
    if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES as u64 {
        return Err(CommandError::invalid(
            "tool activity exceeds its read limit",
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(internal)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(CommandError::invalid(
            "tool activity exceeds its read limit",
        ));
    }
    let mut stored: StoredActivity = serde_json::from_slice(&bytes).map_err(internal)?;
    if stored.loupist_id != loupist_id || stored.thread_id != thread_id || stored.detail.id != id {
        return Err(CommandError::conflict(
            "tool activity does not belong to this conversation",
        ));
    }
    if stored.detail.status == ChatProgressStatus::Running && !active_run(&stored.run_id) {
        // An unfinished record after an app crash is visibly interrupted. Do not
        // invent a completion time or silently replay the command.
        stored.detail.status = ChatProgressStatus::Stopped;
    }
    Ok(stored.detail)
}

fn directory_relative(loupist_id: &str, thread_id: &str) -> Result<PathBuf, CommandError> {
    for id in [loupist_id, thread_id] {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(CommandError::invalid("tool activity owner is invalid"));
        }
    }
    Ok(PathBuf::from("loupists")
        .join(loupist_id)
        .join("activities")
        .join(thread_id))
}

fn workspace_relative(workspace: &Path, input: &str) -> Option<String> {
    let path = Path::new(input);
    #[cfg(windows)]
    let normalized_absolute = path
        .is_absolute()
        .then(|| windows_workspace_relative(workspace.to_str()?, input))
        .flatten();
    #[cfg(windows)]
    let relative = if path.is_absolute() {
        normalized_absolute.as_deref()?
    } else {
        input
    };
    #[cfg(not(windows))]
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace).ok()?.to_str()?
    } else {
        input
    };
    #[cfg(windows)]
    let relative = relative.replace('\\', "/");
    #[cfg(windows)]
    let relative = relative.as_str();
    let normalized = crate::fs::workspace_files::relative_path(relative).ok()?;
    (normalized != ".").then_some(normalized)
}

#[cfg(any(windows, test))]
fn windows_workspace_relative(workspace: &str, input: &str) -> Option<String> {
    fn ordinary(path: &str) -> String {
        let path = path.replace('\\', "/");
        if path
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("//?/UNC/"))
        {
            format!("//{}", &path[8..])
        } else {
            path.strip_prefix("//?/").unwrap_or(&path).into()
        }
    }
    let workspace = ordinary(workspace);
    let prefix = format!("{}/", workspace.trim_end_matches('/'));
    let input = ordinary(input);
    if !input.get(..prefix.len())?.eq_ignore_ascii_case(&prefix) {
        return None;
    }
    Some(input.get(prefix.len()..)?.into())
}

fn assign_output(detail: &mut ChatActivityDetail, result: &Value) {
    // Match the output Pi persists, not structuredContent (which Pi 0.99 can
    // expose live but omit from JSONL). Live and reopened details agree.
    let mut text = String::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for block in content {
            if !text.is_empty() {
                text.push('\n');
            }
            if let Some(value) = block.get("text").and_then(Value::as_str) {
                text.push_str(value);
            } else if block.get("type").and_then(Value::as_str) == Some("image") {
                text.push_str("[Image output — open the source file to inspect it]");
            }
            if text.len() > MAX_OUTPUT_BYTES {
                break;
            }
        }
    } else if let Some(value) = result.as_str() {
        text.push_str(value);
    }
    if detail.tool_name == "run_python" {
        if let Some(value) = python_result(result) {
            detail.file_path = python_artifact_path(&value);
            detail.truncated |= value.get("stdout_truncated").and_then(Value::as_bool)
                == Some(true)
                || value.get("stderr_truncated").and_then(Value::as_bool) == Some(true);
            text = serde_json::to_string_pretty(&value).unwrap_or(text);
        }
    }
    detail.truncated |= text.len() > MAX_OUTPUT_BYTES
        || result
            .pointer("/details/truncation/truncated")
            .and_then(Value::as_bool)
            == Some(true);
    detail.output = bound(&text, MAX_OUTPUT_BYTES).into();
}

pub(crate) fn python_result(result: &Value) -> Option<Value> {
    let text = result.pointer("/content/0/text")?.as_str()?;
    let value: Value = serde_json::from_str(text).ok()?;
    (value.get("status")?.is_string() && value.get("exit_code")?.is_i64()).then_some(value)
}

pub(crate) fn python_completed(value: &Value) -> bool {
    value.get("status").and_then(Value::as_str) == Some("completed")
        && value.get("exit_code").and_then(Value::as_i64) == Some(0)
}

pub(crate) fn python_artifact_path(value: &Value) -> Option<String> {
    value
        .get("artifacts")?
        .as_array()?
        .iter()
        .find_map(|artifact| {
            let path = artifact.get("path")?.as_str()?;
            let relative = crate::fs::workspace_files::relative_path(path).ok()?;
            (relative != ".").then_some(relative)
        })
}

fn bound(value: &str, max: usize) -> &str {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn write_record(
    directory: &PrivateDirectoryGuard,
    path: &Path,
    stored: &StoredActivity,
    durable: bool,
) -> Result<(), CommandError> {
    let bytes = serde_json::to_vec(stored).map_err(internal)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(CommandError::invalid(
            "tool activity exceeds its storage limit",
        ));
    }
    let name = format!("{}.json", stored.detail.id);
    directory.write_atomic(path, &name, &bytes, durable)
}

fn internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(format!("could not retain local tool activity: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn python_activity_retains_code_result_and_generated_file_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = SecureDeletionRoot::open(&temp.path().canonicalize().unwrap()).unwrap();
        let mut recorder =
            ActivityRecorder::new(&root, "loupist-a", "chat-a", "run-a", temp.path()).unwrap();
        let reference = recorder
            .start(
                "python",
                "run_python",
                &json!({"code":"print(391)","timeout_seconds":60}),
                1,
            )
            .unwrap()
            .unwrap();
        let value = json!({"status":"timed_out", "exit_code":124, "stdout":"391", "stderr":"", "stdout_truncated":true,
            "artifacts":[{"path":"../private.txt"},{"path":"outputs/chart.png"}]});
        recorder
            .finish(
                "python",
                &json!({"content":[{"type":"text", "text":value.to_string()}]}),
                false,
                2,
            )
            .unwrap();
        drop(recorder);
        let detail = read(&root, "loupist-a", "chat-a", &reference.detail_id, |_| {
            false
        })
        .unwrap();
        assert!(detail.input.contains("print(391)"));
        assert!(detail.output.contains("391"));
        assert_eq!(detail.status, ChatProgressStatus::Failed);
        assert_eq!(detail.file_path.as_deref(), Some("outputs/chart.png"));
        assert!(detail.truncated);
    }

    #[test]
    fn activity_survives_restart_with_exact_scope_and_without_hidden_structured_output() {
        let temp = tempfile::tempdir().unwrap();
        let root = SecureDeletionRoot::open(&temp.path().canonicalize().unwrap()).unwrap();
        let workspace = temp.path().join("workspace");
        let mut recorder =
            ActivityRecorder::new(&root, "loupist-a", "chat-a", "run-a", &workspace).unwrap();
        let reference = recorder
            .start("call", "grep", &json!({"pattern":"python analysis.py"}), 1)
            .unwrap()
            .unwrap();
        recorder
            .update(
                "call",
                &json!({"content":[{"type":"text","text":"working"}]}),
                501,
            )
            .unwrap();
        assert_eq!(
            read(&root, "loupist-a", "chat-a", &reference.detail_id, |_| true)
                .unwrap()
                .output,
            "working"
        );
        recorder.finish("call", &json!({"content":[{"type":"text","text":"done"}],"structuredContent":{"output":"unpersisted"}}), false, 600).unwrap();
        drop(recorder);
        let detail = read(&root, "loupist-a", "chat-a", &reference.detail_id, |_| {
            false
        })
        .unwrap();
        assert_eq!(detail.output, "done");
        assert_eq!(detail.status, ChatProgressStatus::Succeeded);
        assert!(
            read(&root, "loupist-b", "chat-a", &reference.detail_id, |_| {
                false
            })
            .is_err()
        );
        assert!(
            read(&root, "loupist-a", "chat-b", &reference.detail_id, |_| {
                false
            })
            .is_err()
        );
        assert!(read(&root, "loupist-a", "chat-a", "../escape", |_| false).is_err());
    }

    #[test]
    fn interruption_and_file_links_remain_honest() {
        let temp = tempfile::tempdir().unwrap();
        let root = SecureDeletionRoot::open(&temp.path().canonicalize().unwrap()).unwrap();
        let workspace = temp.path().join("workspace");
        let mut recorder =
            ActivityRecorder::new(&root, "loupist-a", "chat-a", "run-a", &workspace).unwrap();
        let reference = recorder
            .start(
                "call",
                "write",
                &json!({"path":workspace.join("report.md"),"content":"report"}),
                10,
            )
            .unwrap()
            .unwrap();
        assert_eq!(reference.file_path.as_deref(), Some("report.md"));
        let interrupted = read(&root, "loupist-a", "chat-a", &reference.detail_id, |_| {
            false
        })
        .unwrap();
        assert_eq!(interrupted.status, ChatProgressStatus::Stopped);
        assert!(interrupted.finished_at_ms.is_none());
        recorder.finish_remaining(true, 20).unwrap();
        assert_eq!(
            read(&root, "loupist-a", "chat-a", &reference.detail_id, |_| {
                false
            })
            .unwrap()
            .finished_at_ms,
            Some(20)
        );
        assert!(workspace_relative(&workspace, "/outside/file").is_none());
    }

    #[test]
    fn unicode_output_is_bounded_and_marked() {
        let mut detail = ChatActivityDetail {
            id: "a".repeat(32),
            tool_name: "read".into(),
            status: ChatProgressStatus::Running,
            input: String::new(),
            output: String::new(),
            truncated: false,
            file_path: None,
            started_at_ms: 0,
            finished_at_ms: None,
        };
        assign_output(
            &mut detail,
            &json!({"content":[{"type":"text","text":"한".repeat(MAX_OUTPUT_BYTES)}]}),
        );
        assert!(detail.truncated && detail.output.len() <= MAX_OUTPUT_BYTES);
    }

    #[test]
    fn escaped_control_output_fits_the_durable_record_limit() {
        let temp = tempfile::tempdir().unwrap();
        let root = SecureDeletionRoot::open(&temp.path().canonicalize().unwrap()).unwrap();
        let mut recorder =
            ActivityRecorder::new(&root, "loupist-a", "chat-a", "run-a", temp.path()).unwrap();
        let reference = recorder
            .start("call", "grep", &json!({"pattern":"report"}), 1)
            .unwrap()
            .unwrap();
        let output = "\u{0001}".repeat(MAX_OUTPUT_BYTES);
        recorder
            .finish(
                "call",
                &json!({"content":[{"type":"text","text":output}]}),
                false,
                2,
            )
            .unwrap();
        assert_eq!(
            read(&root, "loupist-a", "chat-a", &reference.detail_id, |_| {
                false
            })
            .unwrap()
            .output,
            output
        );
    }

    #[test]
    fn windows_activity_links_match_verbatim_and_ordinary_roots() {
        assert_eq!(
            windows_workspace_relative(r"\\?\C:\Research", r"c:\research\reports\result.md")
                .as_deref(),
            Some("reports/result.md")
        );
        assert_eq!(
            windows_workspace_relative(
                r"\\?\UNC\server\share\Research",
                r"\\server\share\research\report.md"
            )
            .as_deref(),
            Some("report.md")
        );
        assert_eq!(
            windows_workspace_relative(r"C:\Research", r"\\?\C:\Research\report.md").as_deref(),
            Some("report.md")
        );
        assert!(
            windows_workspace_relative(r"C:\Research", r"C:\Research-other\secret.md").is_none()
        );
        assert!(windows_workspace_relative(r"C:\Research", r"D:\Research\report.md").is_none());
    }
}
