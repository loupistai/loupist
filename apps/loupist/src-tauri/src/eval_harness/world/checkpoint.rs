//! Bounded, step-local file evidence outside the model's writable workspace.
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;

pub(super) fn capture(workspace: &Path, destination: &Path) -> super::Result<Value> {
    let root = workspace.canonicalize()?;
    fs::create_dir_all(destination.parent().ok_or("checkpoint parent missing")?)?;
    if destination
        .parent()
        .unwrap()
        .canonicalize()?
        .starts_with(&root)
    {
        return Err("checkpoint must be outside the model workspace".into());
    }
    fs::create_dir(destination)?;
    let mut pending = vec![PathBuf::new()];
    let mut files = Vec::new();
    let mut omitted = Vec::new();
    let mut entries_seen = 0;
    let mut total_bytes = 0;
    while let Some(relative_dir) = pending.pop() {
        let directory = root.join(&relative_dir).canonicalize()?;
        if !directory.starts_with(&root) || directory.starts_with(root.join(".loupist")) {
            omitted.push(json!({"path":relative_dir,"reason":"outside_workspace"}));
            continue;
        }
        let remaining = MAX_ENTRIES - entries_seen;
        let mut entries = fs::read_dir(directory)?
            .take(remaining + 1)
            .collect::<std::io::Result<Vec<_>>>()?;
        if entries.len() > remaining {
            omitted.push(json!({"path":relative_dir,"reason":"entry_limit"}));
            break;
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            entries_seen += 1;
            let relative = relative_dir.join(entry.file_name());
            if relative.starts_with(".loupist") {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                omitted.push(json!({"path":relative,"reason":"symlink"}));
                continue;
            }
            if metadata.is_dir() {
                pending.push(relative);
                continue;
            }
            if !metadata.is_file() {
                omitted.push(json!({"path":relative,"reason":"not_regular_file"}));
                continue;
            }
            if metadata.len() > MAX_FILE_BYTES || total_bytes + metadata.len() > MAX_TOTAL_BYTES {
                omitted.push(json!({"path":relative,"reason":"byte_limit"}));
                continue;
            }
            // The Chat has settled. Do not follow a file symlink even if it was
            // swapped after the metadata check; canonical Memory is never walked.
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let file = options.open(entry.path())?;
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_FILE_BYTES
                || total_bytes + bytes.len() as u64 > MAX_TOTAL_BYTES
            {
                omitted.push(json!({"path":relative,"reason":"byte_limit"}));
                continue;
            }
            let copy = destination.join(&relative);
            fs::create_dir_all(copy.parent().ok_or("checkpoint file parent missing")?)?;
            fs::write(&copy, &bytes)?;
            total_bytes += bytes.len() as u64;
            files.push(json!({"path":relative,"byte_length":bytes.len(),
                "sha256":crate::util::sha256(&bytes)}));
        }
    }
    Ok(json!({"schema":"loupist-workspace-checkpoint/1",
        "status":if omitted.is_empty(){"completed"}else{"partial"},
        "directory":destination,"files":files,"omitted":omitted,
        "total_bytes":total_bytes,"canonical_memory_excluded":true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_preserve_each_steps_bytes_without_reading_canonical_memory() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(workspace.join("outputs")).unwrap();
        fs::create_dir_all(workspace.join(".loupist")).unwrap();
        fs::write(workspace.join(".loupist/private"), b"must stay private").unwrap();
        fs::write(workspace.join("outputs/result.csv"), b"value\n12\n").unwrap();
        let first = temp.path().join("checkpoints/first");
        let before = capture(&workspace, &first).unwrap();
        fs::write(workspace.join("outputs/result.csv"), b"value\n15\n").unwrap();
        let second = temp.path().join("checkpoints/second");
        let after = capture(&workspace, &second).unwrap();
        assert_eq!(before["status"], "completed");
        assert_eq!(after["status"], "completed");
        assert_eq!(before["files"].as_array().unwrap().len(), 1);
        assert_eq!(
            fs::read(first.join("outputs/result.csv")).unwrap(),
            b"value\n12\n"
        );
        assert_eq!(
            fs::read(second.join("outputs/result.csv")).unwrap(),
            b"value\n15\n"
        );
        assert_eq!(
            before["files"][0]["sha256"],
            crate::util::sha256(b"value\n12\n")
        );
        assert!(!first.join(".loupist").exists());
        assert!(capture(&workspace, &first).is_err());
    }

    #[test]
    fn oversized_files_are_explicitly_unobserved() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let large = fs::File::create(workspace.join("large.bin")).unwrap();
        large.set_len(MAX_FILE_BYTES + 1).unwrap();
        let result = capture(&workspace, &temp.path().join("copy")).unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["omitted"][0]["reason"], "byte_limit");
        assert!(result["files"].as_array().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_does_not_follow_files_or_directories_outside_the_workspace() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(temp.path().join("private"), b"outside bytes").unwrap();
        symlink(temp.path().join("private"), workspace.join("file-link")).unwrap();
        symlink(temp.path(), workspace.join("dir-link")).unwrap();
        let copy = temp.path().join("copy");
        let result = capture(&workspace, &copy).unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["omitted"].as_array().unwrap().len(), 2);
        assert!(result["files"].as_array().unwrap().is_empty());
        assert_eq!(fs::read_dir(copy).unwrap().count(), 0);
    }
}
