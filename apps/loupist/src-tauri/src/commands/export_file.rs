use crate::app::CommandError;
use serde::Deserialize;
use std::{io::Write, path::Path};
const MAX_BYTES: usize = 200 * 1024 * 1024;
/// Text exports are small; images and recorded Map replays may be large.
const MAX_TEXT_BYTES: usize = 20 * 1024 * 1024;
/// Every file the app saves for the user: Map images and replays, chat transcripts and data tables.
#[derive(Clone, Copy, Deserialize, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ExportKind {
    Png,
    Mp4,
    Webm,
    Md,
    Csv,
}
impl ExportKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Md => "md",
            Self::Csv => "csv",
        }
    }
    fn max_bytes(self) -> usize {
        match self {
            Self::Md | Self::Csv => MAX_TEXT_BYTES,
            Self::Png | Self::Mp4 | Self::Webm => MAX_BYTES,
        }
    }
    fn fallback_stem(self) -> &'static str {
        match self {
            Self::Png | Self::Mp4 | Self::Webm => "map",
            Self::Md => "chat",
            Self::Csv => "data",
        }
    }
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRequest {
    suggested_name: String,
    bytes: Vec<u8>,
    kind: ExportKind,
    /// The Save dialog's title in the interface language, as the renderer
    /// words it (like `pick_file`'s).
    title: String,
}
/// The longest dialog title accepted; a longer one is shortened, not refused.
const MAX_TITLE_CHARS: usize = 80;
/// The Save dialog's title: the renderer's words on one line, bounded, or
/// "Save" when it sent none.
fn dialog_title(title: &str) -> String {
    let line: String = title
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if line.is_empty() {
        return "Save".to_owned();
    }
    line.chars().take(MAX_TITLE_CHARS).collect()
}
fn validate_size(size: usize, kind: ExportKind) -> Result<(), CommandError> {
    if size == 0 || size > kind.max_bytes() {
        return Err(CommandError::invalid(format!(
            "An export must contain between 1 byte and {} MB.",
            kind.max_bytes() / (1024 * 1024)
        )));
    }
    Ok(())
}
fn validate_extension(path: &Path, kind: ExportKind) -> Result<(), CommandError> {
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(kind.extension()))
    {
        return Err(CommandError::invalid(
            "The selected file extension must match the export format.",
        ));
    }
    Ok(())
}
fn suggested_name(name: &str, kind: ExportKind) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(kind.fallback_stem());
    let clean: String = stem
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':'))
        .take(100)
        .collect();
    format!(
        "{}.{}",
        if clean.trim().is_empty() {
            kind.fallback_stem()
        } else {
            &clean
        },
        kind.extension()
    )
}
fn write_selected(path: &Path, bytes: &[u8], kind: ExportKind) -> Result<(), CommandError> {
    validate_size(bytes.len(), kind)?;
    validate_extension(path, kind)?;
    if !path.is_absolute() {
        return Err(CommandError::invalid(
            "The native dialog must select an absolute path.",
        ));
    }
    let io_error = |error: std::io::Error| CommandError::internal(error.to_string());
    #[cfg(unix)]
    let mut file = {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        };
        let parent = std::fs::File::open(
            path.parent()
                .ok_or_else(|| CommandError::invalid("Missing export directory"))?,
        )
        .map_err(io_error)?;
        let name = std::ffi::CString::new(
            path.file_name()
                .ok_or_else(|| CommandError::invalid("Missing export filename"))?
                .as_bytes(),
        )
        .map_err(|_| CommandError::invalid("Invalid export filename"))?;
        // Anchor the directory and refuse symlinks, including a swapped final component.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(CommandError::invalid("Choose a regular file, not a link."));
        }
        file
    };
    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .custom_flags(0x00200000)
            .open(path)
            .map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err(CommandError::invalid("Choose a regular file, not a link."));
        }
        file
    };
    file.set_len(0).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}
/// Saves bytes the renderer prepared to a file the user picks in the native dialog.
/// No caller-provided path is accepted, and the extension must match the kind.
#[tauri::command(rename_all = "snake_case")]
pub async fn export_save(
    state: tauri::State<'_, crate::app::AppState>,
    request: ExportRequest,
) -> Result<Option<String>, CommandError> {
    validate_size(request.bytes.len(), request.kind)?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title(dialog_title(&request.title))
        .set_file_name(suggested_name(&request.suggested_name, request.kind))
        .add_filter(
            request.kind.extension().to_uppercase(),
            &[request.kind.extension()],
        )
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_owned();
    // No caller-provided path is accepted. Only the native chooser supplies it.
    let saved = path.display().to_string();
    // Renderer-composed text has no authoritative origin receipt. Fail closed;
    // images and recordings do not carry model-readable text through `read`.
    if matches!(request.kind, ExportKind::Md | ExportKind::Csv) {
        super::export_trust::record(state.inner(), &request.bytes)?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        write_selected(&path, &request.bytes, request.kind)
    })
    .await
    .map_err(|error| CommandError::internal(error.to_string()))??;
    Ok(Some(saved))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_validates_kind_extension_and_size() {
        assert!(validate_size(0, ExportKind::Png).is_err());
        assert!(validate_size(MAX_BYTES, ExportKind::Png).is_ok());
        assert!(validate_size(MAX_BYTES + 1, ExportKind::Png).is_err());
        assert!(validate_size(MAX_TEXT_BYTES + 1, ExportKind::Md).is_err());
        for (ext, kind) in [
            ("png", ExportKind::Png),
            ("mp4", ExportKind::Mp4),
            ("webm", ExportKind::Webm),
            ("md", ExportKind::Md),
            ("csv", ExportKind::Csv),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("map.{ext}"));
            write_selected(&path, b"map payload", kind).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"map payload");
            assert!(write_selected(&dir.path().join("map.exe"), b"bad", kind).is_err());
            assert!(!dir.path().join("map.exe").exists());
        }
        assert_eq!(suggested_name("../map.exe", ExportKind::Png), "map.png");
        assert_eq!(suggested_name("../notes.exe", ExportKind::Md), "notes.md");
        assert_eq!(suggested_name("", ExportKind::Csv), "data.csv");
    }
    #[test]
    fn the_save_dialog_is_titled_in_the_renderers_words_on_one_bounded_line() {
        assert_eq!(dialog_title("이미지 저장"), "이미지 저장");
        assert_eq!(dialog_title("  Save\nimage\t "), "Save image");
        assert_eq!(dialog_title(" \u{7} "), "Save");
        assert_eq!(
            dialog_title(&"표".repeat(200)).chars().count(),
            MAX_TITLE_CHARS
        );
    }
    #[cfg(unix)]
    #[test]
    fn export_refuses_links_without_touching_their_targets() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("original.png");
        std::fs::write(&target, b"original").unwrap();
        let link = dir.path().join("link.png");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(write_selected(&link, b"replacement", ExportKind::Png).is_err());
        let hard = dir.path().join("hard.png");
        std::fs::hard_link(&target, &hard).unwrap();
        assert!(write_selected(&hard, b"replacement", ExportKind::Png).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"original");
    }
}
