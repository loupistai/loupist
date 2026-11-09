//! Search only a bounded list of individually admitted entries. Search engines
//! never walk the host tree themselves or read ambient git configuration.

use std::{
    fs, io,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde_json::{json, Value};

use super::{decide, Exposure, NativeDecision, NativeRequest, NativeRoots, NativeTool};

const MAX_ENTRIES: usize = 10_000;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_IGNORE_BYTES: u64 = 64 * 1024;
const MAX_DEPTH: usize = 64;

#[cfg(unix)]
pub fn identity(metadata: &fs::Metadata) -> Value {
    use std::os::unix::fs::MetadataExt;
    json!({"dev": metadata.dev().to_string(), "ino": metadata.ino().to_string()})
}

#[cfg(not(unix))]
pub fn identity(_metadata: &fs::Metadata) -> Value {
    Value::Null
}

fn allowed(path: &Path, roots: &NativeRoots, exposure: Exposure) -> bool {
    matches!(
        decide(
            &NativeRequest {
                tool: NativeTool::Read,
                target: Some(path.to_path_buf()),
            },
            roots,
            exposure
        ),
        NativeDecision::AllowFile { .. }
    )
}

fn ignore_rules(directory: &Path, roots: &NativeRoots, exposure: Exposure) -> Gitignore {
    let mut builder = GitignoreBuilder::new(directory);
    // No global config, parent directories, .git/info/exclude, or symlinked
    // ignore files: those could otherwise read a protected path during a search.
    for name in [".gitignore", ".ignore"] {
        let path = directory.join(name);
        if !allowed(&path, roots, exposure) {
            continue;
        }
        let Ok(before) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !before.is_file() || before.len() > MAX_IGNORE_BYTES {
            continue;
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let Ok(file) = options.open(&path) else {
            continue;
        };
        let Ok(after) = file.metadata() else { continue };
        if !after.is_file() || identity(&before) != identity(&after) {
            continue;
        }
        let mut text = String::new();
        if file
            .take(MAX_IGNORE_BYTES + 1)
            .read_to_string(&mut text)
            .is_err()
            || text.len() > MAX_IGNORE_BYTES as usize
        {
            continue;
        }
        for line in text.lines() {
            let _ = builder.add_line(Some(path.clone()), line);
        }
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

struct Scan<'a> {
    roots: &'a NativeRoots,
    exposure: Exposure,
    entries: Vec<Value>,
    scanned: usize,
    bytes: usize,
    started: Instant,
    truncated: bool,
}

impl Scan<'_> {
    fn budget(&mut self, depth: usize) -> bool {
        if self.scanned >= MAX_ENTRIES
            || self.bytes >= MAX_MANIFEST_BYTES
            || depth > MAX_DEPTH
            || self.started.elapsed() > Duration::from_secs(5)
        {
            self.truncated = true;
            return false;
        }
        true
    }

    fn add(&mut self, path: &Path, metadata: &fs::Metadata) {
        let entry =
            json!({"path": path, "directory": metadata.is_dir(), "identity": identity(metadata)});
        self.bytes += entry.to_string().len();
        self.entries.push(entry);
    }

    fn walk(
        &mut self,
        directory: &Path,
        recursive: bool,
        parents: &[Gitignore],
        depth: usize,
    ) -> io::Result<()> {
        if !self.budget(depth) || !allowed(directory, self.roots, self.exposure) {
            return Ok(());
        }
        let mut rules = parents.to_vec();
        if recursive {
            rules.push(ignore_rules(directory, self.roots, self.exposure));
        }
        // Stream entries instead of collecting an unbounded directory first.
        for entry in fs::read_dir(directory)? {
            if !self.budget(depth) {
                break;
            }
            self.scanned += 1;
            let entry = entry?;
            let path = entry.path();
            if !allowed(&path, self.roots, self.exposure) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)?;
            // Do not traverse links or expose devices, FIFOs or sockets. Direct
            // file reads can resolve an admitted link through the normal gate.
            if !(metadata.is_file() || metadata.is_dir()) {
                continue;
            }
            if recursive {
                let ignored = rules
                    .iter()
                    .rev()
                    .find_map(|rule| {
                        let found = rule.matched(&path, metadata.is_dir());
                        (!found.is_none()).then(|| found.is_ignore())
                    })
                    .unwrap_or(false);
                if ignored || entry.file_name() == "node_modules" {
                    continue;
                }
            }
            self.add(&path, &metadata);
            if recursive && metadata.is_dir() {
                self.walk(&path, true, &rules, depth + 1)?;
            }
        }
        Ok(())
    }
}

pub fn snapshot(
    root: &Path,
    roots: &NativeRoots,
    exposure: Exposure,
    recursive: bool,
) -> io::Result<Value> {
    let metadata = fs::metadata(root)?;
    let mut scan = Scan {
        roots,
        exposure,
        entries: Vec::new(),
        scanned: 0,
        bytes: 0,
        started: Instant::now(),
        truncated: false,
    };
    if metadata.is_dir() {
        scan.walk(root, recursive, &[], 0)?;
    } else if metadata.is_file() && allowed(root, roots, exposure) {
        scan.add(root, &metadata);
    } else {
        return Err(io::Error::other(
            "Search target is not a regular file or directory",
        ));
    }
    scan.entries
        .sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(
        json!({"directory": metadata.is_dir(), "entries": scan.entries, "truncated": scan.truncated}),
    )
}
