//! Reader and citation labels for files in the current Chat.
use std::{collections::BTreeMap, path::Path};

use chrono::{Datelike, SecondsFormat, TimeZone, Utc};
use serde_json::Value;

use crate::{
    agent_harness::native_policy::{self, NativeRoots, PathClass},
    domain::{ChatEvidenceKind, ChatEvidenceRef, ChatSession},
};

use super::progress::{ChatProgress, ChatProgressCategory, ChatProgressItem};

const PATH_KEYS: &[&str] = &["path", "file", "directory"];

pub(crate) fn path_tool(tool: &str) -> bool {
    matches!(
        tool,
        "read"
            | "write"
            | "edit"
            | "ls"
            | "find"
            | "grep"
            | "workspace_capture"
            | "dataset_import"
            | "dataset_export"
    )
}

pub(crate) struct FileDisplay {
    pub title: String,
    /// Only a real workspace path can open in the workspace reader.
    pub workspace_path: Option<String>,
}

impl FileDisplay {
    fn title_only(title: String) -> Self {
        Self {
            title,
            workspace_path: None,
        }
    }
}

struct Attachment {
    message: String,
    id: String,
    title: String,
    snapshot_title: String,
}

#[derive(Default)]
pub(crate) struct FileLabels {
    loupist_id: String,
    thread_id: String,
    attachments: Vec<Attachment>,
}

impl FileLabels {
    pub(crate) fn for_chat(chat: &ChatSession) -> Self {
        let attachments: Vec<_> = chat
            .messages
            .iter()
            .chain(chat.versions.iter().flat_map(|version| &version.messages))
            .flat_map(|message| {
                message
                    .attachments
                    .iter()
                    .map(move |attachment| (message, attachment))
            })
            .collect();
        let mut names = BTreeMap::<_, BTreeMap<_, _>>::new();
        for (message, file) in &attachments {
            names
                .entry(file.filename.as_str())
                .or_default()
                .insert(message.id.as_str(), message.created_at_ms);
        }
        let labels = attachments
            .iter()
            .map(|(message, attachment)| {
                let name = attachment.filename.as_str();
                let others = &names[name];
                let time = Utc.timestamp_millis_opt(message.created_at_ms).single();
                let title = match time {
                    Some(time) if others.len() > 1 => {
                        let show_year = others.values().any(|&at| {
                            Utc.timestamp_millis_opt(at)
                                .single()
                                .is_some_and(|other| other.year() != time.year())
                        });
                        let show_seconds = others.iter().any(|(&id, &at)| {
                            id != message.id && at / 60_000 == message.created_at_ms / 60_000
                        });
                        let pattern = match (show_year, show_seconds) {
                            (true, true) => "%b %d %Y %H:%M:%S%.3f UTC",
                            (true, false) => "%b %d %Y %H:%M UTC",
                            (false, true) => "%b %d %H:%M:%S%.3f UTC",
                            (false, false) => "%b %d %H:%M UTC",
                        };
                        format!("{name}, attached {}", time.format(pattern))
                    }
                    _ => name.to_owned(),
                };
                // Keep a safe, dated title in saved evidence so that a future
                // same-named upload can disambiguate an earlier citation.
                let snapshot_title = time.map_or_else(
                    || name.to_owned(),
                    |time| {
                        format!(
                            "{name}, attached {}",
                            time.to_rfc3339_opts(SecondsFormat::Millis, true)
                        )
                    },
                );
                Attachment {
                    message: message.id.clone(),
                    id: attachment.id.clone(),
                    title,
                    snapshot_title,
                }
            })
            .collect();
        Self {
            loupist_id: chat.loupist_id.clone(),
            thread_id: chat.id.clone(),
            attachments: labels,
        }
    }

    fn attachment(&self, message: &str, file: &str) -> Option<&Attachment> {
        self.attachments.iter().find(|entry| {
            entry.message == message && (entry.id == file || extracted_from(file, &entry.id))
        })
    }

    pub(crate) fn attachment_snapshot_title(
        &self,
        input: &str,
        roots: &NativeRoots,
    ) -> Option<String> {
        let classified = native_policy::classify(Path::new(input), roots);
        if classified.class != PathClass::Attachments {
            return None;
        }
        let parts = native_policy::below(&classified.path, &roots.attachments)?;
        let [message, file] = parts.as_slice() else {
            return None;
        };
        self.attachment(message, file)
            .map(|entry| entry.snapshot_title.clone())
    }

    pub(crate) fn display(&self, input: &str, roots: &NativeRoots) -> Option<FileDisplay> {
        let classified = native_policy::classify(Path::new(input), roots);
        match classified.class {
            PathClass::Attachments => {
                let parts = native_policy::below(&classified.path, &roots.attachments)?;
                let title = match parts.as_slice() {
                    [message, file] => self
                        .attachment(message, file)
                        .map(|entry| entry.title.clone()),
                    _ => None,
                }
                .unwrap_or_else(|| "Attachment".into());
                Some(FileDisplay::title_only(title))
            }
            PathClass::Scratch => {
                let parts = native_policy::below(&classified.path, &roots.scratch)?;
                let title = ordinary_title(&parts.join("/"))
                    .map_or_else(|| "Scratch".to_owned(), |name| format!("Scratch · {name}"));
                Some(FileDisplay::title_only(title))
            }
            PathClass::Workspace | PathClass::Instructions => {
                let parts = native_policy::below(&classified.path, &roots.workspace)?;
                let title = ordinary_title(&parts.join("/"))?;
                Some(FileDisplay {
                    workspace_path: Some(title.clone()),
                    title,
                })
            }
            _ => None,
        }
    }

    /// Recover the display name of a local-file reference saved before the
    /// label was projected. Match the owning Chat and the stored attachment.
    fn legacy_attachment(&self, input: &str) -> Option<String> {
        let input = if cfg!(windows) {
            std::borrow::Cow::Owned(input.replace('\\', "/"))
        } else {
            std::borrow::Cow::Borrowed(input)
        };
        let parts = input.rsplitn(7, '/').collect::<Vec<_>>();
        match parts.as_slice() {
            [file, message, thread, "attachments", loupist, "loupists", root]
                if *thread == self.thread_id
                    && *loupist == self.loupist_id
                    && (root.ends_with("/.loupist")
                        || *root == ".loupist"
                        || input.starts_with('/')) =>
            {
                self.attachment(message, file)
                    .map(|entry| entry.title.clone())
            }
            _ => None,
        }
    }

    pub(crate) fn stored_title(&self, input: &str) -> Option<String> {
        self.attachments
            .iter()
            .find(|entry| entry.snapshot_title == input || entry.title == input)
            .map(|entry| entry.title.clone())
            .or_else(|| self.legacy_attachment(input))
            .or_else(|| {
                if !input.is_empty()
                    && !input.contains(['/', '\\'])
                    && !input.starts_with('~')
                    && input.as_bytes().get(1) != Some(&b':')
                    && !input.chars().any(char::is_control)
                    && !input.eq_ignore_ascii_case(".loupist")
                {
                    Some(input.to_owned())
                } else {
                    ordinary_title(input)
                }
            })
    }

    pub(crate) fn project_evidence(&self, evidence: &mut ChatEvidenceRef) {
        if evidence.kind == ChatEvidenceKind::LocalFile {
            evidence.title = self
                .stored_title(&evidence.title)
                .unwrap_or_else(|| "Local file".into());
        }
    }

    pub(crate) fn project_activity(&self, detail: &mut super::activity::ChatActivityDetail) {
        let known_title = detail
            .file_path
            .as_deref()
            .and_then(|path| self.stored_title(path));
        detail.file_path = detail.file_path.as_deref().and_then(ordinary_title);
        if !path_tool(&detail.tool_name) {
            return;
        }
        if let Ok(args) = serde_json::from_str(&detail.input) {
            let title = named_path(&args)
                .and_then(|path| self.stored_title(path))
                .or(known_title);
            detail.input = serde_json::to_string_pretty(&displayed_args(&args, title.as_deref()))
                .unwrap_or_default();
        }
    }

    pub(crate) fn project_progress(&self, progress: &mut ChatProgress) {
        for item in &mut progress.items {
            if let ChatProgressItem::Tool {
                tool_name,
                category,
                target,
                file_path,
                ..
            } = item
            {
                if *category == ChatProgressCategory::Files
                    || tool_name.as_deref().is_some_and(path_tool)
                {
                    *target = target
                        .as_deref()
                        .and_then(|value| self.stored_title(value))
                        .or_else(|| {
                            file_path
                                .as_deref()
                                .and_then(|path| self.stored_title(path))
                        });
                }
                *file_path = file_path.as_deref().and_then(ordinary_title);
            }
        }
    }
}

pub(crate) fn origin_label(locator: &str) -> Option<String> {
    match locator.strip_prefix("workspace:") {
        Some(path) => ordinary_title(path),
        None => Some(locator.to_owned()),
    }
}

pub(crate) fn ordinary_title(value: &str) -> Option<String> {
    let path = crate::fs::workspace_files::relative_path(value).ok()?;
    (path != "."
        && !path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".loupist")))
    .then_some(path)
}

pub(crate) fn named_path(args: &Value) -> Option<&str> {
    PATH_KEYS.iter().find_map(|&key| args.get(key)?.as_str())
}

/// Tool execution keeps its exact arguments. Activity receives only labels.
pub(crate) fn displayed_args(args: &Value, title: Option<&str>) -> Value {
    let mut shown = args.clone();
    if let Some(object) = shown.as_object_mut() {
        for key in PATH_KEYS {
            if let Some(value) = object.get_mut(*key) {
                *value = title.map_or(Value::Null, |title| Value::String(title.to_owned()));
            }
        }
    }
    shown
}

fn extracted_from(file: &str, attachment: &str) -> bool {
    file.strip_prefix(attachment).is_some_and(|suffix| {
        suffix == ".md"
            || suffix
                .strip_prefix(".part")
                .and_then(|part| part.strip_suffix(".md"))
                .is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{activity::ActivityRecorder, progress::ChatProgressProjection};
    use serde_json::json;

    #[test]
    fn c014_scratch_read_has_a_safe_label_without_a_workspace_link() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        let app = root.join("app");
        let scratch = app.join("runtime/temp");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        let file = scratch.join("report.csv");
        std::fs::write(&file, "value\n42\n").unwrap();
        let roots = NativeRoots::new(
            &workspace,
            &app,
            &app,
            &workspace.join(".loupist"),
            &scratch,
            &app.join("attachments"),
            None,
        );
        let labels = FileLabels::default();
        let args = json!({"path":file});
        let file = labels.display(named_path(&args).unwrap(), &roots).unwrap();
        assert_eq!(file.title, "Scratch · report.csv");
        assert_eq!(file.workspace_path, None);

        let shown = displayed_args(&args, Some(&file.title));
        let mut progress = ChatProgressProjection::new(1);
        progress.start_tool("local", "read", &args, None, 2);
        progress.show_file_target("local", Some(&file.title));
        let root = crate::fs::secure_delete::SecureDeletionRoot::open(&root).unwrap();
        let mut activity =
            ActivityRecorder::new(&root, "loupist-a", "chat-a", "run-a", &workspace).unwrap();
        let reference = activity
            .start("local", "read", &shown, 2, Some(&file))
            .unwrap()
            .unwrap();
        assert!(reference.file_path.is_none());
        let detail = super::super::activity::read(
            &root,
            "loupist-a",
            "chat-a",
            &reference.detail_id,
            |_| true,
        )
        .unwrap();
        assert!(detail.input.contains("Scratch · report.csv"));
        assert!(!detail.input.contains("runtime"));
        assert!(matches!(&progress.snapshot().items[0],
            ChatProgressItem::Tool { target: Some(label), .. } if label == "Scratch · report.csv"));
    }
}
