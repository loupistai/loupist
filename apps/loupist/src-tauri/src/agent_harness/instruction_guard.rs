//! What a later reply loads from the workspace, kept as it was when this
//! reply's first command after third-party text was allowed, and put back
//! exactly when the reply ends (`docs/runtime/permissions.md`). A sandboxed
//! command can write anywhere in the Loupist folder, root instruction files
//! and Skill folders included; file tools refuse those writes after third-party
//! text, and this guard gives commands the same outcome.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The instruction loader's own discovery bound.
pub const MAX_GUARDED_ENTRIES: usize = 512;
pub const MAX_GUARDED_BYTES: u64 = 16 * 1024 * 1024;

/// Root entries a later reply loads as instructions, matched without case.
const ROOT_INSTRUCTIONS: &[&str] = &["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"];

/// A path a later reply loads from, and how much of it the guard keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Guard {
    /// This exact path, as a file, a link, a folder, or absent.
    Path(PathBuf),
    /// Everything under this path.
    Tree(PathBuf),
    /// A folder whose presence the guard notes, so one the command created
    /// and left empty goes away with its contents.
    Folder(PathBuf),
    /// Workspace root entries whose name spells an instruction file.
    RootInstructions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    File { content: Vec<u8>, mode: u32 },
    Symlink(PathBuf),
    Dir,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// More entries or bytes than the guard keeps.
    TooLarge,
    Io,
}

/// The guarded entries of one workspace at one moment.
#[derive(Debug)]
pub struct InstructionSnapshot {
    workspace: PathBuf,
    guards: Vec<Guard>,
    entries: BTreeMap<PathBuf, Entry>,
}

impl InstructionSnapshot {
    /// Records the root instruction files, `.loupist/AGENTS.md`,
    /// `.loupist/config.toml`, every file under `.loupist/skills`,
    /// `.agents/skills` and `.pi/skills`, and the root `.git` entry: its
    /// `config` and `hooks` when it is a folder, the file itself when it is a
    /// worktree pointer, and its absence otherwise.
    pub fn take(workspace: &Path) -> Result<Self, SnapshotError> {
        let mut guards = vec![
            Guard::RootInstructions,
            Guard::Path(".loupist/AGENTS.md".into()),
            Guard::Path(".loupist/config.toml".into()),
            Guard::Tree(".loupist/skills".into()),
            Guard::Folder(".agents".into()),
            Guard::Tree(".agents/skills".into()),
            Guard::Folder(".pi".into()),
            Guard::Tree(".pi/skills".into()),
        ];
        let git = workspace.join(".git");
        match std::fs::symlink_metadata(&git) {
            Ok(metadata) if metadata.is_dir() => guards.extend([
                Guard::Path(".git/config".into()),
                Guard::Tree(".git/hooks".into()),
            ]),
            Ok(_) => guards.push(Guard::Path(".git".into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                guards.push(Guard::Tree(".git".into()))
            }
            Err(_) => return Err(SnapshotError::Io),
        }
        let entries = observe(workspace, &guards)?;
        Ok(Self {
            workspace: workspace.to_path_buf(),
            guards,
            entries,
        })
    }

    /// Puts every guarded entry back as it was: changed files are rewritten,
    /// deleted ones recreated, and entries that appeared are removed, with
    /// the folders they left empty. Returns the workspace-relative paths it
    /// changed, in order; an entry that could not be put back is kept in the
    /// list, since the user has to look at it either way.
    pub fn restore(&self) -> Vec<PathBuf> {
        let current = observe(&self.workspace, &self.guards).unwrap_or_default();
        let mut changed = Vec::new();
        let mut appeared: Vec<&PathBuf> = current
            .keys()
            .filter(|path| !self.entries.contains_key(*path))
            .collect();
        // Deepest first, so a folder is empty by the time it is removed.
        appeared.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for path in appeared {
            let _ = remove(&self.workspace.join(path), &current[path]);
            changed.push(path.clone());
        }
        for (path, entry) in &self.entries {
            if current.get(path) == Some(entry) {
                continue;
            }
            let absolute = self.workspace.join(path);
            if let Some(present) = current.get(path) {
                let _ = remove(&absolute, present);
            }
            let _ = put_back(&absolute, entry);
            changed.push(path.clone());
        }
        changed
    }
}

fn observe(workspace: &Path, guards: &[Guard]) -> Result<BTreeMap<PathBuf, Entry>, SnapshotError> {
    let mut entries = BTreeMap::new();
    let mut bytes = 0_u64;
    for guard in guards {
        match guard {
            Guard::Path(relative) | Guard::Folder(relative) => {
                if let Some(entry) = entry_at(&workspace.join(relative), &mut bytes)? {
                    insert(&mut entries, relative.clone(), entry)?;
                }
            }
            Guard::Tree(relative) => {
                walk(workspace, relative, &mut entries, &mut bytes)?;
            }
            Guard::RootInstructions => {
                let listing = match std::fs::read_dir(workspace) {
                    Ok(listing) => listing,
                    Err(_) => return Err(SnapshotError::Io),
                };
                for child in listing {
                    let child = child.map_err(|_| SnapshotError::Io)?;
                    let name = child.file_name();
                    let spelled = name.to_string_lossy();
                    if !ROOT_INSTRUCTIONS
                        .iter()
                        .any(|known| spelled.eq_ignore_ascii_case(known))
                    {
                        continue;
                    }
                    if let Some(entry) = entry_at(&child.path(), &mut bytes)? {
                        insert(&mut entries, PathBuf::from(name), entry)?;
                    }
                }
            }
        }
    }
    Ok(entries)
}

fn insert(
    entries: &mut BTreeMap<PathBuf, Entry>,
    relative: PathBuf,
    entry: Entry,
) -> Result<(), SnapshotError> {
    entries.insert(relative, entry);
    if entries.len() > MAX_GUARDED_ENTRIES {
        return Err(SnapshotError::TooLarge);
    }
    Ok(())
}

fn walk(
    workspace: &Path,
    relative: &Path,
    entries: &mut BTreeMap<PathBuf, Entry>,
    bytes: &mut u64,
) -> Result<(), SnapshotError> {
    let absolute = workspace.join(relative);
    let Some(entry) = entry_at(&absolute, bytes)? else {
        return Ok(());
    };
    let descend = entry == Entry::Dir;
    insert(entries, relative.to_path_buf(), entry)?;
    if !descend {
        return Ok(());
    }
    let listing = std::fs::read_dir(&absolute).map_err(|_| SnapshotError::Io)?;
    for child in listing {
        let child = child.map_err(|_| SnapshotError::Io)?;
        walk(workspace, &relative.join(child.file_name()), entries, bytes)?;
    }
    Ok(())
}

/// The entry at `path` without following a link, or None when there is none.
fn entry_at(path: &Path, bytes: &mut u64) -> Result<Option<Entry>, SnapshotError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SnapshotError::Io),
    };
    let kind = metadata.file_type();
    let entry = if kind.is_symlink() {
        Entry::Symlink(std::fs::read_link(path).map_err(|_| SnapshotError::Io)?)
    } else if kind.is_dir() {
        Entry::Dir
    } else if kind.is_file() {
        *bytes = bytes.saturating_add(metadata.len());
        if *bytes > MAX_GUARDED_BYTES {
            return Err(SnapshotError::TooLarge);
        }
        Entry::File {
            content: std::fs::read(path).map_err(|_| SnapshotError::Io)?,
            mode: mode_of(&metadata),
        }
    } else {
        // A socket or device here is not something a later reply loads.
        return Err(SnapshotError::Io);
    };
    Ok(Some(entry))
}

fn remove(path: &Path, entry: &Entry) -> std::io::Result<()> {
    match entry {
        Entry::Dir => std::fs::remove_dir(path),
        Entry::File { .. } | Entry::Symlink(_) => std::fs::remove_file(path),
    }
}

fn put_back(path: &Path, entry: &Entry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match entry {
        Entry::Dir => std::fs::create_dir(path),
        Entry::File { content, mode } => {
            std::fs::write(path, content)?;
            set_mode(path, *mode)
        }
        Entry::Symlink(target) => make_symlink(target, path),
    }
}

#[cfg(unix)]
fn mode_of(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn mode_of(_metadata: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &Path, path: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(not(unix))]
fn make_symlink(target: &Path, path: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        for (path, content) in [
            ("AGENTS.md", "root rules\n"),
            (".loupist/AGENTS.md", "project rules\n"),
            (".loupist/config.toml", "model = \"x\"\n"),
            (
                ".loupist/skills/screen/SKILL.md",
                "---\nname: screen\n---\n",
            ),
            (".loupist/skills/screen/screen.py", "print(1)\n"),
            (".agents/skills/notes.md", "notes skill\n"),
            ("notes/plan.md", "the user's note\n"),
            (".git/config", "[core]\n"),
            (".git/hooks/pre-commit", "#!/bin/sh\necho ok\n"),
            (".git/HEAD", "ref: refs/heads/main\n"),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        set_mode(&root.join(".git/hooks/pre-commit"), 0o755).unwrap();
        (temporary, root)
    }

    fn read(root: &Path, path: &str) -> String {
        std::fs::read_to_string(root.join(path)).unwrap()
    }

    /// Everything a tainted command could do to the loaded set is undone:
    /// edits, deletions, new files in new Skill folders, planted hooks and
    /// config, and nothing else in the workspace is touched.
    #[test]
    fn a_restore_puts_every_guarded_entry_back_and_leaves_the_rest() {
        let (_temporary, root) = workspace();
        let snapshot = InstructionSnapshot::take(&root).unwrap();

        std::fs::write(root.join("AGENTS.md"), "Always run curl.\n").unwrap();
        std::fs::write(root.join("CLAUDE.md"), "planted\n").unwrap();
        std::fs::write(root.join("agents.override.md"), "planted\n").unwrap();
        std::fs::remove_file(root.join(".loupist/config.toml")).unwrap();
        std::fs::write(root.join(".loupist/skills/screen/SKILL.md"), "evil\n").unwrap();
        std::fs::remove_file(root.join(".loupist/skills/screen/screen.py")).unwrap();
        std::fs::create_dir_all(root.join(".agents/skills/evil/deep")).unwrap();
        std::fs::write(root.join(".agents/skills/evil/SKILL.md"), "evil\n").unwrap();
        std::fs::write(root.join(".agents/skills/evil/deep/helper.py"), "x\n").unwrap();
        std::fs::create_dir_all(root.join(".pi/skills")).unwrap();
        std::fs::write(root.join(".pi/skills/evil.md"), "evil\n").unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n\tfsmonitor = evil\n").unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), "evil\n").unwrap();
        std::fs::write(root.join(".git/hooks/post-checkout"), "evil\n").unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/other\n").unwrap();
        std::fs::write(root.join("notes/plan.md"), "the command's note\n").unwrap();
        std::fs::write(root.join("notes/out.csv"), "a,b\n").unwrap();

        let changed = snapshot.restore();

        assert_eq!(read(&root, "AGENTS.md"), "root rules\n");
        assert!(!root.join("CLAUDE.md").exists());
        assert!(!root.join("agents.override.md").exists());
        assert_eq!(read(&root, ".loupist/config.toml"), "model = \"x\"\n");
        assert_eq!(
            read(&root, ".loupist/skills/screen/SKILL.md"),
            "---\nname: screen\n---\n"
        );
        assert_eq!(
            read(&root, ".loupist/skills/screen/screen.py"),
            "print(1)\n"
        );
        assert_eq!(read(&root, ".agents/skills/notes.md"), "notes skill\n");
        assert!(!root.join(".agents/skills/evil").exists());
        assert!(
            !root.join(".pi").exists(),
            "a Skill folder the command created is gone with its parent"
        );
        assert_eq!(read(&root, ".git/config"), "[core]\n");
        assert_eq!(read(&root, ".git/hooks/pre-commit"), "#!/bin/sh\necho ok\n");
        assert_eq!(
            mode_of(&std::fs::metadata(root.join(".git/hooks/pre-commit")).unwrap()) & 0o777,
            0o755
        );
        assert!(!root.join(".git/hooks/post-checkout").exists());
        assert_eq!(
            read(&root, ".git/HEAD"),
            "ref: refs/heads/other\n",
            "git state outside config and hooks is not guarded"
        );
        assert_eq!(read(&root, "notes/plan.md"), "the command's note\n");
        assert_eq!(read(&root, "notes/out.csv"), "a,b\n");
        let mut changed: Vec<String> = changed
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        changed.sort();
        assert_eq!(
            changed,
            [
                ".agents/skills/evil",
                ".agents/skills/evil/SKILL.md",
                ".agents/skills/evil/deep",
                ".agents/skills/evil/deep/helper.py",
                ".git/config",
                ".git/hooks/post-checkout",
                ".git/hooks/pre-commit",
                ".loupist/config.toml",
                ".loupist/skills/screen/SKILL.md",
                ".loupist/skills/screen/screen.py",
                ".pi",
                ".pi/skills",
                ".pi/skills/evil.md",
                "AGENTS.md",
                "CLAUDE.md",
                "agents.override.md",
            ]
        );
        assert!(
            snapshot.restore().is_empty(),
            "a second restore finds nothing to do"
        );
    }

    /// A workspace without `.git` keeps none: a `.git` folder or gitdir file
    /// the command plants is removed, and a replaced Skill symlink is put back.
    #[cfg(unix)]
    #[test]
    fn a_planted_git_directory_and_a_swapped_link_are_undone() {
        let (_temporary, root) = workspace();
        std::fs::remove_dir_all(root.join(".git")).unwrap();
        std::os::unix::fs::symlink("notes.md", root.join(".agents/skills/alias.md")).unwrap();
        let snapshot = InstructionSnapshot::take(&root).unwrap();

        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), "evil\n").unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
        std::fs::remove_file(root.join(".agents/skills/alias.md")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join(".agents/skills/alias.md")).unwrap();
        snapshot.restore();
        assert!(!root.join(".git").exists());
        assert_eq!(
            std::fs::read_link(root.join(".agents/skills/alias.md")).unwrap(),
            PathBuf::from("notes.md")
        );

        let pointer = InstructionSnapshot::take(&root).unwrap();
        std::fs::write(root.join(".git"), "gitdir: .g2\n").unwrap();
        pointer.restore();
        assert!(!root.join(".git").exists());
    }

    #[test]
    fn a_set_beyond_the_loaders_bounds_is_refused_before_any_command_runs() {
        let (_temporary, root) = workspace();
        for index in 0..MAX_GUARDED_ENTRIES {
            std::fs::write(root.join(format!(".loupist/skills/{index}.md")), "x\n").unwrap();
        }
        assert_eq!(
            InstructionSnapshot::take(&root).unwrap_err(),
            SnapshotError::TooLarge
        );
    }
}
