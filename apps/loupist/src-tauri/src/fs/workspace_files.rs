//! Read-only desktop views of a registered workspace. Agent execution is trusted
//! local execution; these narrower renderer APIs never follow filesystem links.

use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;

use crate::{app::CommandError, fs::loupist_root::BoundLoupistRoot, util::sha256};

pub(crate) const MAX_WORKSPACE_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 1_000;

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkspaceDirectory {
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkspaceFile {
    pub path: String,
    pub size_bytes: u64,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub image_data_url: Option<String>,
    pub content_sha256: Option<String>,
    pub media_type: String,
    pub truncated: bool,
    pub binary: bool,
}

pub(crate) fn relative_path(input: &str) -> Result<String, CommandError> {
    if input.len() > 8 * 1024 || input.contains(['\0', '\\']) || input.chars().any(char::is_control)
    {
        return Err(CommandError::invalid("workspace path is invalid"));
    }
    let mut parts = Vec::new();
    for part in Path::new(input).components() {
        match part {
            Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or_else(|| CommandError::invalid("workspace path must be UTF-8"))?;
                // Also reject Windows drive/stream syntax when requested on Unix.
                if name.contains(':') {
                    return Err(CommandError::invalid("workspace path must be relative"));
                }
                parts.push(name);
            }
            Component::CurDir => {}
            _ => {
                return Err(CommandError::invalid(
                    "workspace path must stay inside the selected folder",
                ))
            }
        }
    }
    Ok(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
}

#[cfg(unix)]
fn open_relative(
    root: &BoundLoupistRoot,
    path: &str,
    directory: bool,
) -> Result<File, CommandError> {
    use rustix::{
        fs::{openat, Mode, OFlags},
        io::fcntl_dupfd_cloexec,
    };
    let mut current = fcntl_dupfd_cloexec(root, 3).map_err(io_error)?;
    if path == "." {
        if !directory {
            return Err(CommandError::invalid("select a workspace file"));
        }
        return Ok(File::from(current));
    }
    let parts = path.split('/').collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        if directory || index + 1 < parts.len() {
            flags |= OFlags::DIRECTORY;
        }
        current = openat(&current, *part, flags, Mode::empty()).map_err(io_error)?;
    }
    let file = File::from(current);
    let metadata = file.metadata().map_err(io_error)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(CommandError::invalid(
            "workspace entry is not a regular file or directory",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn open_relative(
    root: &BoundLoupistRoot,
    path: &str,
    directory: bool,
) -> Result<File, CommandError> {
    let full = root.path().join(path);
    let _ancestors =
        crate::fs::windows::open_parent_directories_no_reparse(&full).map_err(io_error)?;
    if directory {
        crate::fs::windows::open_directory_no_reparse(&full).map_err(io_error)
    } else {
        crate::fs::windows::open_regular_no_reparse(&full).map_err(io_error)
    }
}

#[cfg(not(any(unix, windows)))]
fn open_relative(
    _root: &BoundLoupistRoot,
    _path: &str,
    _directory: bool,
) -> Result<File, CommandError> {
    Err(CommandError::invalid(
        "workspace files are unavailable on this platform",
    ))
}

pub(crate) fn list(
    root: &BoundLoupistRoot,
    input: &str,
) -> Result<WorkspaceDirectory, CommandError> {
    let path = relative_path(input)?;
    let directory = open_relative(root, &path, true)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    #[cfg(unix)]
    {
        use rustix::fs::{statat, AtFlags, Dir, FileType};
        let iterator = Dir::read_from(&directory).map_err(io_error)?;
        let mut seen = 0;
        for entry in iterator {
            let entry = entry.map_err(io_error)?;
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if matches!(name, "." | "..") {
                continue;
            }
            seen += 1;
            if seen > MAX_DIRECTORY_ENTRIES {
                truncated = true;
                break;
            }
            let Ok(metadata) = statat(&directory, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW)
            else {
                continue;
            };
            let kind = match FileType::from_raw_mode(metadata.st_mode) {
                FileType::RegularFile => "file",
                FileType::Directory => "directory",
                _ => continue,
            };
            push_entry(
                &mut entries,
                &path,
                name,
                kind,
                metadata.st_size.max(0) as u64,
            );
        }
    }
    #[cfg(windows)]
    {
        // Windows directory handles omit delete sharing; retained ancestors stop
        // a rename/reparse replacement during this path-based directory walk.
        let full = root.path().join(&path);
        let _ancestors =
            crate::fs::windows::open_parent_directories_no_reparse(&full).map_err(io_error)?;
        for (index, entry) in std::fs::read_dir(&full).map_err(io_error)?.enumerate() {
            if index >= MAX_DIRECTORY_ENTRIES {
                truncated = true;
                break;
            }
            let entry = entry.map_err(io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if crate::fs::windows::metadata_is_reparse(&metadata) {
                continue;
            }
            let kind = if metadata.is_file() {
                "file"
            } else if metadata.is_dir() {
                "directory"
            } else {
                continue;
            };
            push_entry(&mut entries, &path, &name, kind, metadata.len());
        }
        drop(directory);
    }
    entries.sort_by(|left, right| {
        left.kind
            .cmp(right.kind)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(WorkspaceDirectory {
        path,
        entries,
        truncated,
    })
}

fn push_entry(
    entries: &mut Vec<WorkspaceEntry>,
    parent: &str,
    name: &str,
    kind: &'static str,
    size: u64,
) {
    if name.chars().any(char::is_control) || name.contains(['\\', ':']) {
        return;
    }
    entries.push(WorkspaceEntry {
        path: if parent == "." {
            name.into()
        } else {
            format!("{parent}/{name}")
        },
        name: name.into(),
        kind,
        size_bytes: (kind == "file").then_some(size),
    });
}

pub(crate) fn read(root: &BoundLoupistRoot, input: &str) -> Result<WorkspaceFile, CommandError> {
    let path = relative_path(input)?;
    let mut file = open_relative(root, &path, false)?;
    let size_bytes = file.metadata().map_err(io_error)?.len();
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_WORKSPACE_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let truncated = bytes.len() > MAX_WORKSPACE_PREVIEW_BYTES
        || size_bytes > MAX_WORKSPACE_PREVIEW_BYTES as u64;
    let content_sha256 = (!truncated).then(|| sha256(&bytes));
    let raster_type = raster_media_type(&bytes);
    // Encode only a complete bounded read, never an image prefix. Keep raster
    // data separate from text so exact_text cannot retain it as UTF-8 content.
    let image_data_url = raster_type
        .filter(|_| !truncated && bytes.len() as u64 == size_bytes)
        .map(|media_type| format!("data:{media_type};base64,{}", BASE64.encode(&bytes)));
    if truncated {
        bytes.truncate(MAX_WORKSPACE_PREVIEW_BYTES);
    }
    // A preview ending in the middle of a UTF-8 codepoint is still a text file.
    if truncated {
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    let content = if raster_type.is_some() || bytes.contains(&0) {
        None
    } else {
        String::from_utf8(bytes).ok()
    };
    let binary = content.is_none();
    Ok(WorkspaceFile {
        media_type: raster_type
            .unwrap_or_else(|| media_type(&path, binary))
            .into(),
        path,
        size_bytes,
        content,
        image_data_url,
        content_sha256,
        truncated,
        binary,
    })
}

fn raster_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// Read one bounded snapshot through the same no-follow handles as workspace previews.
pub(crate) fn exact_bytes(
    root: &BoundLoupistRoot,
    input: &str,
    limit: usize,
) -> Result<Vec<u8>, CommandError> {
    let path = relative_path(input)?;
    let file = open_relative(root, &path, false)?;
    let before = file.metadata().map_err(io_error)?;
    let limit_error = || {
        let size = if limit >= 1024 * 1024 && limit.is_multiple_of(1024 * 1024) {
            format!("{} MiB", limit / (1024 * 1024))
        } else if limit >= 1024 && limit.is_multiple_of(1024) {
            format!("{} KiB", limit / 1024)
        } else {
            format!("{limit} bytes")
        };
        CommandError::invalid(format!("workspace file exceeds {size}; split the input"))
    };
    if before.len() > limit as u64 {
        return Err(limit_error());
    }
    let mut bytes = Vec::new();
    (&file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let after = file.metadata().map_err(io_error)?;
    if bytes.len() > limit {
        return Err(limit_error());
    }
    if before.len() != after.len()
        || bytes.len() as u64 != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(CommandError::conflict(
            "workspace file changed during import; retry after writing finishes",
        ));
    }
    Ok(bytes)
}

pub(crate) fn exact_text(
    root: &BoundLoupistRoot,
    input: &str,
    expected: Option<&str>,
) -> Result<WorkspaceFile, CommandError> {
    let file = read(root, input)?;
    if file.truncated || file.binary {
        return Err(CommandError::invalid("workspace capture requires a UTF-8 text file up to 256 KiB; retain binary originals through Add source"));
    }
    if expected.is_some_and(|hash| Some(hash) != file.content_sha256.as_deref()) {
        return Err(CommandError::conflict(
            "workspace file changed; read its current contents before capture",
        ));
    }
    Ok(file)
}

pub(crate) fn check_existing(root: &BoundLoupistRoot, input: &str) -> Result<String, CommandError> {
    let path = relative_path(input)?;
    // Try the exact entry as a directory, then as a regular file. Neither path
    // follows a symlink or launches an executable.
    if open_relative(root, &path, true).is_err() {
        open_relative(root, &path, false)?;
    }
    Ok(path)
}

fn media_type(path: &str, binary: bool) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        _ if binary => "application/octet-stream",
        _ => "text/plain",
    }
}

fn io_error(_error: impl std::fmt::Display) -> CommandError {
    CommandError::invalid(
        "Could not read this workspace entry. Check that it still exists and is readable.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, BoundLoupistRoot) {
        let dir = tempfile::tempdir().unwrap();
        let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
        (dir, root)
    }

    #[test]
    fn workspace_errors_do_not_expose_os_details() {
        for detail in [
            "/Users/private/.ssh/id_ed25519",
            "permission denied (os error 13)",
            "token=private",
        ] {
            let error = io_error(detail);
            assert!(!error.message.contains(detail));
            assert_eq!(error.code, "invalid_request");
        }
        let (_dir, root) = fixture();
        assert_eq!(
            read(&root, "missing").unwrap_err().message,
            io_error("").message
        );
        assert!(relative_path("notes/report.md").is_ok());
        assert!(relative_path("./notes/report.md").is_ok());
    }

    #[test]
    fn exact_bytes_reports_the_callers_limit_and_accepts_that_many_bytes() {
        let (dir, root) = fixture();
        std::fs::write(dir.path().join("data.csv"), "1234").unwrap();
        assert_eq!(exact_bytes(&root, "data.csv", 4).unwrap(), b"1234");
        assert_eq!(
            exact_bytes(&root, "data.csv", 3).unwrap_err().message,
            "workspace file exceeds 3 bytes; split the input"
        );
        for (limit, label) in [(32 * 1024 * 1024, "32 MiB"), (512 * 1024, "512 KiB")] {
            std::fs::File::create(dir.path().join("large.csv"))
                .unwrap()
                .set_len(limit as u64 + 1)
                .unwrap();
            assert_eq!(
                exact_bytes(&root, "large.csv", limit).unwrap_err().message,
                format!("workspace file exceeds {label}; split the input")
            );
        }
    }

    #[test]
    fn reads_and_lists_without_modifying_user_files() {
        let (dir, root) = fixture();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        std::fs::write(
            dir.path().join("data/prices.csv"),
            "day,price\n2026-10-09,10\n",
        )
        .unwrap();
        assert_eq!(list(&root, ".").unwrap().entries[0].kind, "directory");
        let file = exact_text(&root, "data/prices.csv", None).unwrap();
        assert_eq!(file.content.as_deref(), Some("day,price\n2026-10-09,10\n"));
        assert_eq!(file.media_type, "text/csv");
        assert!(exact_text(&root, "data/prices.csv", Some("stale")).is_err());
        assert!(exact_text(&root, "data/prices.csv", file.content_sha256.as_deref()).is_ok());
    }

    #[test]
    fn preview_bounds_utf8_and_rejects_non_text_capture() {
        let (dir, root) = fixture();
        std::fs::write(
            dir.path().join("large.txt"),
            "한".repeat(MAX_WORKSPACE_PREVIEW_BYTES),
        )
        .unwrap();
        let file = read(&root, "large.txt").unwrap();
        assert!(file.truncated && !file.binary && file.content_sha256.is_none());
        assert!(file.content.unwrap().len() <= MAX_WORKSPACE_PREVIEW_BYTES);
        assert!(exact_text(&root, "large.txt", None).is_err());
        std::fs::write(dir.path().join("data.bin"), [0, 1, 2]).unwrap();
        assert!(read(&root, "data.bin").unwrap().binary);
        assert!(exact_text(&root, "data.bin", None).is_err());
    }

    fn png() -> Vec<u8> {
        BASE64.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC").unwrap()
    }

    #[test]
    fn previews_complete_raster_bytes_with_actual_mime_and_no_text_capture() {
        let (dir, root) = fixture();
        let bytes = png();
        for path in ["chart.png", "mislabeled.jpg", "image.bin"] {
            std::fs::write(dir.path().join(path), &bytes).unwrap();
            let file = read(&root, path).unwrap();
            assert!(file.binary && !file.truncated);
            assert!(file.content.is_none());
            assert_eq!(file.media_type, "image/png");
            assert_eq!(
                file.content_sha256.as_deref(),
                Some(sha256(&bytes).as_str())
            );
            let encoded = file
                .image_data_url
                .as_deref()
                .unwrap()
                .strip_prefix("data:image/png;base64,")
                .unwrap();
            assert_eq!(BASE64.decode(encoded).unwrap(), bytes);
            assert!(exact_text(&root, path, None).is_err());
        }
        for (signature, media_type) in [
            (&b"\xff\xd8\xff\xe0"[..], "image/jpeg"),
            (&b"GIF87a"[..], "image/gif"),
            (&b"GIF89a"[..], "image/gif"),
            (&b"RIFF\x12\0\0\0WEBPVP8 "[..], "image/webp"),
        ] {
            assert_eq!(raster_media_type(signature), Some(media_type));
        }
    }

    #[test]
    fn raster_preview_rejects_nonimages_and_bounded_prefixes() {
        let (dir, root) = fixture();
        for (path, bytes) in [
            ("not-an-image.png", &b"ordinary text"[..]),
            ("not-an-image.jpg", &b"\0\x01\x02"[..]),
            (
                "vector.svg",
                &b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"[..],
            ),
            ("page.html", &b"<html><body>Chart</body></html>"[..]),
            ("audio.webp", &b"RIFF\x12\0\0\0WAVEfmt "[..]),
        ] {
            std::fs::write(dir.path().join(path), bytes).unwrap();
            let file = read(&root, path).unwrap();
            assert!(file.image_data_url.is_none(), "{path}");
        }
        let mut large = png();
        large.resize(MAX_WORKSPACE_PREVIEW_BYTES + 1, 0);
        std::fs::write(dir.path().join("large.png"), large).unwrap();
        let file = read(&root, "large.png").unwrap();
        assert!(file.truncated && file.binary);
        assert!(file.content.is_none());
        assert!(file.content_sha256.is_none());
        assert!(file.image_data_url.is_none());
        assert!(exact_text(&root, "large.png", None).is_err());
    }

    #[test]
    fn renderer_paths_cannot_escape_the_workspace() {
        for path in [
            "../secret",
            "/etc/passwd",
            "a/../../b",
            "C:\\data",
            "data:stream",
            "a\nb",
        ] {
            assert!(relative_path(path).is_err(), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_links_or_open_special_files() {
        use std::os::unix::fs::symlink;
        let (dir, root) = fixture();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "outside").unwrap();
        symlink(outside.path(), dir.path().join("escape")).unwrap();
        symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        assert!(read(&root, "escape/secret.txt").is_err());
        assert!(read(&root, "link.txt").is_err());
        std::fs::write(outside.path().join("chart.png"), png()).unwrap();
        symlink(
            outside.path().join("chart.png"),
            dir.path().join("chart.png"),
        )
        .unwrap();
        assert!(read(&root, "chart.png").is_err());
        assert!(read(&root, "escape/chart.png").is_err());
        assert!(list(&root, ".").unwrap().entries.is_empty());
    }
}
