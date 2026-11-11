//! What a Pi native file tool (`read`, `ls`, `find`, `grep`, `write`, `edit`)
//! may do in one Chat reply, decided from the target's path class and how
//! much third-party text the model has seen (`docs/runtime/permissions.md`).
//! Every decision is Allow or Refuse: nothing here asks the user.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

pub mod search;

const MAX_PATH_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeTool {
    Read,
    Ls,
    Find,
    Grep,
    Write,
    Edit,
}

impl NativeTool {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "read" => Self::Read,
            "ls" => Self::Ls,
            "find" => Self::Find,
            "grep" => Self::Grep,
            "write" => Self::Write,
            "edit" => Self::Edit,
            _ => return None,
        })
    }

    fn writes(self) -> bool {
        matches!(self, Self::Write | Self::Edit)
    }

    fn searches(self) -> bool {
        matches!(self, Self::Find | Self::Grep)
    }
}

/// One `native.authorize` call, parsed once at the broker boundary. A file
/// tool names its resolved target (the search root for `ls`, `find` and
/// `grep`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRequest {
    pub tool: NativeTool,
    pub target: Option<PathBuf>,
}

impl NativeRequest {
    /// `{name, path}`; an unknown name, a missing or oversized field, a NUL
    /// byte or an extra field is malformed.
    pub fn parse(params: &Value) -> Option<Self> {
        let object = params.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "path"))
        {
            return None;
        }
        let tool = NativeTool::parse(object.get("name")?.as_str()?)?;
        let path = object.get("path")?.as_str()?;
        if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
            return None;
        }
        Some(Self {
            tool,
            target: Some(PathBuf::from(path)),
        })
    }
}

/// The folders one turn's policy is decided against, each with its existing
/// part canonicalized. `home` is absent when the host has no home directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoots {
    /// The selected workspace: the Loupist folder.
    pub workspace: PathBuf,
    /// The global instruction folder (`AGENTS.md` and `skills/**` live here).
    pub global: PathBuf,
    /// The app data root: credentials, Pi runtime files.
    pub app_data: PathBuf,
    /// This Loupist's data folder (`<workspace>/.loupist`): its database,
    /// attachments and activity log, beside the instruction files it holds.
    pub loupist_data: PathBuf,
    /// This run's `TMPDIR` under the Chat's Pi runtime folder.
    pub scratch: PathBuf,
    /// This Chat's attachment folder, which the host asks the model to read.
    pub attachments: PathBuf,
    pub home: Option<PathBuf>,
}

impl NativeRoots {
    pub fn new(
        workspace: &Path,
        global: &Path,
        app_data: &Path,
        loupist_data: &Path,
        scratch: &Path,
        attachments: &Path,
        home: Option<&Path>,
    ) -> Self {
        Self {
            workspace: resolved(workspace),
            global: resolved(global),
            app_data: resolved(app_data),
            loupist_data: resolved(loupist_data),
            scratch: resolved(scratch),
            attachments: resolved(attachments),
            home: home.map(resolved),
        }
    }
}

/// First match wins, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathClass {
    /// A file a later turn loads as instructions or a Skill, plus the
    /// workspace's `.loupist/config.toml`.
    Instructions,
    /// This run's temporary folder.
    Scratch,
    /// This Chat's attachments: readable, never written.
    Attachments,
    /// Anything else under the app data root or the Loupist's data folder.
    AppPrivate,
    /// A credential store or shell startup file under the home folder: never
    /// read or written by file tools.
    Credentials,
    /// A git directory inside the workspace (`.git` as a file or folder and
    /// everything in it, the `hooks/**` and `config` of a bare `*.git`): never
    /// read or written.
    GitDir,
    Workspace,
    Outside,
    /// A symlink chain that loops or cannot be read: no tool touches it.
    Unresolvable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub class: PathClass,
    /// The canonical target the decision was made about.
    pub path: PathBuf,
}

pub fn classify(path: &Path, roots: &NativeRoots) -> Classified {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        roots.workspace.join(path)
    };
    let declared_git = git_dir(&lexical(&path), roots);
    let Some(path) = resolve_links(&path) else {
        return Classified {
            class: PathClass::Unresolvable,
            path: lexical(&path),
        };
    };
    let class = if credential(&path, roots.home.as_deref()) {
        PathClass::Credentials
    } else if declared_git
        || git_dir(&path, roots)
        || under(&path, &resolved(&roots.workspace.join(".git")))
    {
        PathClass::GitDir
    } else if instruction_file(&path, roots) {
        PathClass::Instructions
    } else if under(&path, &roots.scratch) {
        PathClass::Scratch
    } else if under(&path, &roots.attachments) {
        PathClass::Attachments
    } else if app_private(&path, roots) || loupist_private(&path, roots) {
        PathClass::AppPrivate
    } else if under(&path, &roots.workspace) {
        PathClass::Workspace
    } else {
        PathClass::Outside
    };
    Classified { class, path }
}

/// The host can create the selected workspace inside its app-data namespace.
/// Only that strictly nested, trusted workspace is public content; its own
/// `.loupist` store remains private, as do every sibling and the app-data root.
fn app_private(target: &Path, roots: &NativeRoots) -> bool {
    let nested_workspace =
        below(&roots.workspace, &roots.app_data).is_some_and(|parts| !parts.is_empty());
    under(target, &roots.app_data) && !(nested_workspace && under(target, &roots.workspace))
}

/// The Loupist's data folder, except its Skills, which workspace discovery
/// loads like any Skill folder.
fn loupist_private(target: &Path, roots: &NativeRoots) -> bool {
    under(target, &roots.loupist_data) && !under(target, &roots.loupist_data.join("skills"))
}

/// How much third-party text can steer this turn's model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    Clean,
    /// This turn raised the untrusted-text tier.
    ThisTurn,
    /// This turn read a custom-connection result (the stronger tier).
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeRefusal {
    /// The app's private data.
    AppPrivate,
    /// A credential store or startup file under the home folder.
    Credentials,
    /// Any access to a git directory.
    GitDir,
    /// A file with another name on disk can alias a protected file.
    HardLink,
    /// A search rooted above the app data root would descend into it.
    SearchCoversAppData,
    /// The target's symlink chain loops or cannot be read.
    Unresolvable,
    /// A file outside the Loupist folder, scratch and this Chat's attachments.
    Outside,
    /// A write to a file a later reply loads as instructions, after this
    /// reply read third-party text.
    InstructionsAfterUntrustedText,
    /// A write to a file that has another name on disk (a hard link), which
    /// can be an instruction file, after this reply read third-party text.
    HardLinkAfterUntrustedText,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeDecision {
    AllowFile {
        /// The canonical target, for the extension's write pin.
        resolved: PathBuf,
    },
    Refuse(NativeRefusal),
}

pub fn decide(request: &NativeRequest, roots: &NativeRoots, exposure: Exposure) -> NativeDecision {
    let Some(target) = request.target.as_deref() else {
        return NativeDecision::Refuse(NativeRefusal::AppPrivate);
    };
    let Classified { class, path } = classify(target, roots);
    let tool = request.tool;
    if tool.searches()
        && !matches!(class, PathClass::Scratch | PathClass::Attachments)
        && under(&roots.app_data, &path)
    {
        return NativeDecision::Refuse(NativeRefusal::SearchCoversAppData);
    }
    let refusal = match class {
        PathClass::Unresolvable => NativeRefusal::Unresolvable,
        PathClass::AppPrivate => NativeRefusal::AppPrivate,
        PathClass::Credentials => NativeRefusal::Credentials,
        PathClass::Outside => NativeRefusal::Outside,
        PathClass::Attachments if tool.writes() => NativeRefusal::AppPrivate,
        PathClass::GitDir => NativeRefusal::GitDir,
        PathClass::Instructions if tool.writes() && exposure != Exposure::Clean => {
            NativeRefusal::InstructionsAfterUntrustedText
        }
        PathClass::Scratch | PathClass::Workspace
            if tool.writes() && exposure != Exposure::Clean && hard_linked(&path) =>
        {
            NativeRefusal::HardLinkAfterUntrustedText
        }
        PathClass::Scratch
        | PathClass::Attachments
        | PathClass::Instructions
        | PathClass::Workspace
            if hard_linked(&path) =>
        {
            NativeRefusal::HardLink
        }
        PathClass::Scratch
        | PathClass::Attachments
        | PathClass::Instructions
        | PathClass::Workspace => {
            return NativeDecision::AllowFile { resolved: path };
        }
    };
    NativeDecision::Refuse(refusal)
}

/// Whether the file has another name on disk. The path classes see one
/// name; a hard link in a writable folder can be an instruction file.
#[cfg(unix)]
fn hard_linked(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.nlink() > 1)
}

#[cfg(not(unix))]
fn hard_linked(_path: &Path) -> bool {
    false
}

const ROOT_INSTRUCTIONS: &[&str] = &["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"];

/// Matches what workspace discovery loads: root instructions,
/// `.loupist/AGENTS.md`, `.loupist/config.toml`, Skill entries (`skills/*.md`,
/// `skills/**/SKILL.md`) under `.loupist`, `.agents` and `.pi`, and the global
/// `AGENTS.md` and everything under the global `skills` folder. Names compare
/// without case so that a case-insensitive volume cannot spell them past the
/// rule; on a case-sensitive one that only refuses a few more writes.
fn instruction_file(target: &Path, roots: &NativeRoots) -> bool {
    let relative = |root: &Path| below(target, root);
    let skill_entry = |rest: &[String]| match rest {
        [file] => file.to_lowercase().ends_with(".md"),
        [.., file] => named(file, "SKILL.md"),
        [] => false,
    };
    let workspace = relative(&roots.workspace).is_some_and(|parts| match parts.as_slice() {
        [file] => ROOT_INSTRUCTIONS.iter().any(|name| named(file, name)),
        [folder, file] if named(folder, ".loupist") => {
            named(file, "AGENTS.md") || named(file, "config.toml")
        }
        [folder, skills, rest @ ..] => {
            [".loupist", ".agents", ".pi"]
                .iter()
                .any(|name| named(folder, name))
                && named(skills, "skills")
                && skill_entry(rest)
        }
        _ => false,
    });
    workspace
        || relative(&roots.global).is_some_and(|parts| match parts.as_slice() {
            [file] => named(file, "AGENTS.md"),
            [skills, ..] => named(skills, "skills"),
            _ => false,
        })
}

/// Whether a workspace-relative path names an ordinary research file. Host
/// tools that read or write a workspace file for the model (`workspace_capture`,
/// `dataset_import`, `dataset_export`) take only these: no dot-folder or
/// dotfile at any depth (`.loupist`, `.git`, `.agents`, `.pi`, `.env`), and a
/// write never creates a root instruction file.
pub fn ordinary_workspace_path(path: &str, write: bool) -> bool {
    !path.split('/').any(|part| part.starts_with('.'))
        && !(write && ROOT_INSTRUCTIONS.iter().any(|name| named(path, name)))
}

/// A path component against a known name, without case.
fn named(part: &str, name: &str) -> bool {
    part.eq_ignore_ascii_case(name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    File,
    Tree,
}

/// Credential stores, cloud configs, keychains, shell startup files (which
/// often export keys) and the container runtimes' folders (their credentials
/// and their control sockets), relative to the home directory. Native file
/// tools never touch them.
pub const CREDENTIAL_HOME: &[(&str, Reach)] = &[
    (".ssh", Reach::Tree),
    (".gnupg", Reach::Tree),
    (".aws", Reach::Tree),
    (".config/gcloud", Reach::Tree),
    (".kube", Reach::Tree),
    (".docker", Reach::Tree),
    (".colima", Reach::Tree),
    (".lima", Reach::Tree),
    (".rd", Reach::Tree),
    (".netrc", Reach::File),
    (".git-credentials", Reach::File),
    (".npmrc", Reach::File),
    (".pypirc", Reach::File),
    (".config/gh", Reach::Tree),
    (".config/git", Reach::Tree),
    ("Library/Keychains", Reach::Tree),
    (".zshrc", Reach::File),
    (".zprofile", Reach::File),
    (".zshenv", Reach::File),
    (".zlogin", Reach::File),
    (".bashrc", Reach::File),
    (".bash_profile", Reach::File),
    (".profile", Reach::File),
    (".config/fish", Reach::Tree),
];

fn credential(target: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else { return false };
    let rest = below(target, home);
    CREDENTIAL_HOME.iter().any(|(entry, reach)| {
        // Also cover a credential store symlinked into the workspace. Path
        // spelling alone loses the protected home name after canonicalization.
        let real = resolved(&home.join(entry));
        let canonical = match reach {
            Reach::Tree => under(target, &real),
            Reach::File => {
                target == real || identity(target).is_some_and(|id| Some(id) == identity(&real))
            }
        };
        canonical
            || rest.as_ref().is_some_and(|rest| {
                let entry: Vec<&str> = entry.split('/').collect();
                rest.len() >= entry.len()
                    && rest[..entry.len()]
                        .iter()
                        .zip(&entry)
                        .all(|(part, name)| named(part, name))
                    && (*reach == Reach::Tree || rest.len() == entry.len())
            })
    })
}

/// A git directory inside the workspace: any path with a `.git` component
/// (the `.git` file or folder itself included), and the `hooks/**` and
/// `config` of a bare `*.git` or a submodule's `modules/<path>`. The host's
/// `git` runs hooks, filters and `core.fsmonitor` from there, so no tool
/// reads or writes it. Another repository's git directory is outside the workspace
/// and refused as such.
fn git_dir(target: &Path, roots: &NativeRoots) -> bool {
    let Some(parts) = below(target, &roots.workspace) else {
        return false;
    };
    parts.iter().enumerate().any(|(at, part)| {
        if named(part, ".git") {
            return true;
        }
        if !part.to_lowercase().ends_with(".git") {
            return false;
        }
        let mut rest = &parts[at + 1..];
        while rest.len() >= 2 && named(&rest[0], "modules") {
            rest = &rest[2..];
        }
        rest.first().is_some_and(|folder| named(folder, "hooks"))
            || (rest.len() == 1 && named(&rest[0], "config"))
    })
}

fn under(target: &Path, root: &Path) -> bool {
    below(target, root).is_some()
}

/// `target`'s components below `root` when one of `target`'s ancestors is
/// `root`: spelled the same, or the same existing folder by filesystem
/// identity (`(dev, ino)`, like `fs/pinned_root.rs`). A firmlink or a case
/// alias of a root therefore counts at whatever depth it spells the root, and
/// a case variant that is a different folder, or none, does not.
fn below(target: &Path, root: &Path) -> Option<Vec<String>> {
    if root.as_os_str().is_empty() {
        return None;
    }
    let root_parts = parts(root);
    let target_parts = parts(target);
    let root_identity = identity(root);
    for (depth, ancestor) in target.ancestors().enumerate() {
        let rest = target_parts.len().checked_sub(depth)?;
        if target_parts[..rest] == root_parts[..]
            || (root_identity.is_some() && identity(ancestor) == root_identity)
        {
            return Some(target_parts[rest..].to_vec());
        }
    }
    None
}

fn parts(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect()
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// A root's resolved form; a root that cannot be resolved keeps its lexical form.
fn resolved(path: &Path) -> PathBuf {
    resolve_links(path).unwrap_or_else(|| lexical(path))
}

/// Symlink hops a target may take before it counts as a loop.
const MAX_LINK_HOPS: usize = 40;

/// Lexically normalized, with the longest existing ancestor canonicalized. A
/// dangling symlink is followed to where it points, since that is where a
/// write through it lands. None for a link chain that loops or cannot be read.
pub fn resolve_links(path: &Path) -> Option<PathBuf> {
    let mut normal = lexical(path);
    for _ in 0..MAX_LINK_HOPS {
        let mut existing = normal.as_path();
        let mut rest: Vec<std::ffi::OsString> = Vec::new();
        loop {
            if let Ok(canonical) = existing.canonicalize() {
                return Some(rest.iter().rev().fold(plain_path(canonical), |path, part| {
                    if part == ".." {
                        path.parent().map(Path::to_path_buf).unwrap_or(path)
                    } else {
                        path.join(part)
                    }
                }));
            }
            if existing
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                let target = std::fs::read_link(existing).ok()?;
                let parent = existing.parent()?.canonicalize().ok()?;
                // `..` inside the link's target stays for the fold above, which
                // applies it after the symlinks before it are resolved.
                normal = rest
                    .iter()
                    .rev()
                    .fold(parent.join(target), |path, part| path.join(part));
                break;
            }
            match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name.to_owned());
                    existing = parent;
                }
                _ => return Some(normal),
            }
        }
    }
    None
}

/// `.` and `..` removed without touching the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
}

/// Windows canonical paths carry a verbatim prefix the extension's realpath
/// does not; both sides compare plain paths.
fn plain_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        _temporary: tempfile::TempDir,
        roots: NativeRoots,
        home: PathBuf,
        other_chat_temp: PathBuf,
    }

    fn fixture() -> Fixture {
        let temporary = tempfile::tempdir().unwrap();
        let folder = |path: PathBuf| {
            std::fs::create_dir_all(&path).unwrap();
            path.canonicalize().unwrap()
        };
        let home = folder(temporary.path().join("home"));
        let app_data = folder(home.join("Library/Application Support/Loupist"));
        let workspace = folder(home.join("Research"));
        let loupist_data = folder(workspace.join(".loupist"));
        let runtime = app_data.join("loupists/loupist-a/pi-runtime");
        let scratch = folder(runtime.join("chat-a/temp"));
        let other_chat_temp = folder(runtime.join("chat-b/temp"));
        let attachments = folder(loupist_data.join("loupists/loupist-a/attachments/chat-a"));
        for path in [
            "loupist.sqlite3",
            "loupists/loupist-a/attachments/chat-b/brief.pdf.md",
            "loupists/loupist-a/activity/run-1.jsonl",
        ] {
            let file = loupist_data.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        for path in [
            "pi/auth.json",
            "pi/models.json",
            "custom-connections/connections.json",
            "skills/personal/SKILL.md",
            "skills/personal/helper.py",
            "AGENTS.md",
            "loupists/loupist-a/pi-runtime/chat-a/.pi/settings.json",
        ] {
            let file = app_data.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        for path in [
            "AGENTS.md",
            ".loupist/AGENTS.md",
            ".loupist/config.toml",
            ".loupist/skills/screen/SKILL.md",
            ".loupist/skills/screen/screen.py",
            "notes/plan.md",
            ".git/config",
            ".git/hooks/pre-commit",
        ] {
            let file = workspace.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        for path in [
            ".ssh/id_ed25519",
            ".zshrc",
            ".config/gcloud/credentials.db",
            "Documents/shared.csv",
            "other-repo/.git/config",
        ] {
            let file = home.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        std::fs::write(attachments.join("brief.pdf.md"), "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(app_data.join("pi"), scratch.join("link-to-pi")).unwrap();
            // Dangling links: their targets do not exist.
            symlink(home.join(".zshenv"), workspace.join("report.md")).unwrap();
            symlink("../.zshenv", workspace.join("relative.md")).unwrap();
            symlink("report.md", workspace.join("chain.md")).unwrap();
            symlink(app_data.join("pi/secret.json"), workspace.join("db.md")).unwrap();
            symlink("loop.md", workspace.join("loop.md")).unwrap();
        }
        let roots = NativeRoots::new(
            &workspace,
            &app_data,
            &app_data,
            &loupist_data,
            &scratch,
            &attachments,
            Some(&home),
        );
        Fixture {
            _temporary: temporary,
            roots,
            home,
            other_chat_temp,
        }
    }

    fn request(tool: &str, path: impl AsRef<Path>) -> NativeRequest {
        NativeRequest::parse(&json!({"name": tool, "path": path.as_ref().to_string_lossy()}))
            .unwrap()
    }

    fn kind(decision: &NativeDecision) -> &'static str {
        match decision {
            NativeDecision::AllowFile { .. } => "allow",
            NativeDecision::Refuse(NativeRefusal::AppPrivate) => "refuse app_private",
            NativeDecision::Refuse(NativeRefusal::Credentials) => "refuse credentials",
            NativeDecision::Refuse(NativeRefusal::GitDir) => "refuse git_dir",
            NativeDecision::Refuse(NativeRefusal::HardLink) => "refuse hard_link_alias",
            NativeDecision::Refuse(NativeRefusal::SearchCoversAppData) => "refuse search",
            NativeDecision::Refuse(NativeRefusal::Unresolvable) => "refuse unresolvable",
            NativeDecision::Refuse(NativeRefusal::Outside) => "refuse outside",
            NativeDecision::Refuse(NativeRefusal::InstructionsAfterUntrustedText) => {
                "refuse instructions"
            }
            NativeDecision::Refuse(NativeRefusal::HardLinkAfterUntrustedText) => "refuse hard_link",
        }
    }

    fn decided(request: &NativeRequest, roots: &NativeRoots, exposure: Exposure) -> &'static str {
        kind(&decide(request, roots, exposure))
    }

    #[test]
    fn managed_workspace_admits_content_but_keeps_app_state_private() {
        let mut f = fixture();
        let managed = f.roots.app_data.join("loupists/portfolio-012345678abc");
        std::fs::rename(&f.roots.workspace, &managed).unwrap();
        f.roots.workspace = managed.clone();
        f.roots.loupist_data = managed.join(".loupist");
        f.roots.attachments = f
            .roots
            .loupist_data
            .join("loupists/loupist-a/attachments/chat-a");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                f.roots.app_data.join("pi"),
                managed.join("notes/private-link"),
            )
            .unwrap();
            std::fs::hard_link(
                f.roots.app_data.join("pi/auth.json"),
                managed.join("notes/auth-alias"),
            )
            .unwrap();
            for exposure in [Exposure::Clean, Exposure::ThisTurn, Exposure::Custom] {
                assert_eq!(
                    decided(
                        &request("read", managed.join("notes/private-link/auth.json")),
                        &f.roots,
                        exposure
                    ),
                    "refuse app_private"
                );
                assert_eq!(
                    decided(
                        &request("read", managed.join("notes/auth-alias")),
                        &f.roots,
                        exposure
                    ),
                    "refuse hard_link_alias"
                );
            }
        }
        for exposure in [Exposure::Clean, Exposure::ThisTurn, Exposure::Custom] {
            for tool in ["read", "write", "edit", "ls", "find", "grep"] {
                assert_eq!(
                    decided(
                        &request(tool, managed.join("notes/plan.md")),
                        &f.roots,
                        exposure
                    ),
                    "allow",
                    "{tool}"
                );
            }
            for private in [
                f.roots.app_data.join("pi/auth.json"),
                f.roots
                    .app_data
                    .join("loupists/another-workspace/outputs/secret.txt"),
                f.other_chat_temp.clone(),
                f.roots.loupist_data.join("loupist.sqlite3"),
                f.roots
                    .loupist_data
                    .join("loupists/loupist-a/attachments/chat-b/brief.pdf.md"),
            ] {
                assert_eq!(
                    decided(&request("read", &private), &f.roots, exposure),
                    "refuse app_private",
                    "{}",
                    private.display()
                );
            }
            assert_eq!(
                decided(
                    &request("read", managed.join(".git/config")),
                    &f.roots,
                    exposure
                ),
                "refuse git_dir"
            );
        }
        assert_eq!(
            decided(
                &request("write", managed.join("AGENTS.md")),
                &f.roots,
                Exposure::ThisTurn
            ),
            "refuse instructions"
        );
        // A workspace equal to or above app-data cannot expose its private tree.
        f.roots.workspace = f.roots.app_data.clone();
        assert_eq!(
            classify(&f.roots.app_data.join("pi/auth.json"), &f.roots).class,
            PathClass::AppPrivate
        );
        f.roots.workspace = f.home.clone();
        assert_eq!(
            classify(&f.roots.app_data.join("pi/auth.json"), &f.roots).class,
            PathClass::AppPrivate
        );
    }

    #[cfg(unix)]
    #[test]
    fn credentials_cannot_be_reached_by_symlinked_store_or_hard_link() {
        use std::os::unix::fs::symlink;
        let f = fixture();
        let workspace = &f.roots.workspace;
        let relocated = workspace.join("notes/keys");
        std::fs::rename(f.home.join(".ssh"), &relocated).unwrap();
        symlink(&relocated, f.home.join(".ssh")).unwrap();
        let alias = workspace.join("notes/key-alias");
        std::fs::hard_link(relocated.join("id_ed25519"), &alias).unwrap();
        for exposure in [Exposure::Clean, Exposure::ThisTurn, Exposure::Custom] {
            for tool in ["read", "ls", "find", "grep", "write", "edit"] {
                for path in [
                    f.home.join(".ssh/id_ed25519"),
                    relocated.join("id_ed25519"),
                    alias.clone(),
                ] {
                    assert!(
                        matches!(
                            decide(&request(tool, &path), &f.roots, exposure),
                            NativeDecision::Refuse(_)
                        ),
                        "{tool} {path:?} {exposure:?}"
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn searches_admit_every_entry_and_never_follow_protected_or_ignore_links() {
        use std::os::unix::fs::symlink;
        let f = fixture();
        let workspace = &f.roots.workspace;
        let private = f.roots.loupist_data.join("activity.log");
        std::fs::write(&private, "private-app-marker").unwrap();
        std::fs::hard_link(
            f.home.join(".ssh/id_ed25519"),
            workspace.join("notes/alias"),
        )
        .unwrap();
        symlink(&f.roots.app_data, workspace.join("notes/app-link")).unwrap();
        // A malicious ignore file would hide the ordinary note if read.
        std::fs::write(f.home.join(".ssh/id_ed25519"), "plan.md\n").unwrap();
        symlink(
            f.home.join(".ssh/id_ed25519"),
            workspace.join("notes/.ignore"),
        )
        .unwrap();
        std::fs::write(workspace.join(".gitignore"), "ignored.csv\nignored-dir/\n").unwrap();
        std::fs::create_dir_all(workspace.join("ignored-dir")).unwrap();
        std::fs::create_dir_all(workspace.join("exports")).unwrap();
        std::fs::write(workspace.join("exports/.gitignore"), "*.csv\n!keep.csv\n").unwrap();
        for name in [
            "root.csv",
            "ignored.csv",
            "exports/keep.csv",
            "exports/hidden.csv",
            "ignored-dir/data.csv",
        ] {
            std::fs::write(workspace.join(name), "value\n1\n").unwrap();
        }
        for exposure in [Exposure::Clean, Exposure::ThisTurn] {
            let result = search::snapshot(workspace, &f.roots, exposure, true).unwrap();
            let paths: Vec<&str> = result["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["path"].as_str().unwrap())
                .collect();
            for refused in [
                ".git/config",
                ".loupist/activity.log",
                "notes/alias",
                "notes/app-link",
                "notes/.ignore",
                "ignored.csv",
                "ignored-dir/data.csv",
                "exports/hidden.csv",
            ] {
                assert!(
                    !paths.contains(&workspace.join(refused).to_str().unwrap()),
                    "{refused}: {paths:?}"
                );
            }
            for admitted in ["root.csv", "exports/keep.csv", "notes/plan.md"] {
                assert!(
                    paths.contains(&workspace.join(admitted).to_str().unwrap()),
                    "{admitted}: {paths:?}"
                );
            }
            assert_eq!(result["truncated"], false);
        }
    }

    /// A hard link in a writable subfolder is another name for its file, so
    /// after third-party text a write to it could rewrite `AGENTS.md` where
    /// the Instructions class sees only `notes/alias.md`. The file is still
    /// read, a clean reply still writes it, and a file with one name is
    /// written in every reply.
    #[cfg(unix)]
    #[test]
    fn a_file_with_another_name_on_disk_is_not_written_after_third_party_text() {
        let f = fixture();
        let ws = &f.roots.workspace;
        std::fs::hard_link(ws.join("AGENTS.md"), ws.join("notes/alias.md")).unwrap();
        for exposure in [Exposure::ThisTurn, Exposure::Custom] {
            for tool in ["write", "edit"] {
                assert_eq!(
                    decided(
                        &request(tool, ws.join("notes/alias.md")),
                        &f.roots,
                        exposure
                    ),
                    "refuse hard_link",
                    "{tool} {exposure:?}"
                );
                assert_eq!(
                    decided(&request(tool, ws.join("notes/plan.md")), &f.roots, exposure),
                    "allow",
                    "{tool} {exposure:?}"
                );
            }
            assert_eq!(
                decided(
                    &request("read", ws.join("notes/alias.md")),
                    &f.roots,
                    exposure
                ),
                "refuse hard_link_alias"
            );
        }
        assert_eq!(
            decided(
                &request("write", ws.join("notes/alias.md")),
                &f.roots,
                Exposure::Clean
            ),
            "refuse hard_link_alias"
        );
    }

    #[test]
    fn paths_classify_by_canonical_location_in_table_order() {
        let f = fixture();
        let home = &f.home;
        let app = &f.roots.app_data;
        let ws = &f.roots.workspace;
        let cases: Vec<(PathBuf, PathClass)> = vec![
            (ws.join("AGENTS.md"), PathClass::Instructions),
            (ws.join("notes/../agents.MD"), PathClass::Instructions),
            (ws.join("claude.md"), PathClass::Instructions),
            (ws.join(".loupist/config.toml"), PathClass::Instructions),
            (
                ws.join(".loupist/skills/screen/SKILL.md"),
                PathClass::Instructions,
            ),
            (
                ws.join(".loupist/skills/screen/screen.py"),
                PathClass::Workspace,
            ),
            (app.join("AGENTS.md"), PathClass::Instructions),
            (
                app.join("skills/personal/helper.py"),
                PathClass::Instructions,
            ),
            (f.roots.scratch.join("deck.pptx"), PathClass::Scratch),
            (
                f.roots.attachments.join("brief.pdf.md"),
                PathClass::Attachments,
            ),
            (f.other_chat_temp.join("note.txt"), PathClass::AppPrivate),
            (
                app.join("loupists/loupist-a/pi-runtime/chat-a/.pi/settings.json"),
                PathClass::AppPrivate,
            ),
            (app.join("pi/auth.json"), PathClass::AppPrivate),
            (app.join("pi/models.json"), PathClass::AppPrivate),
            (
                app.join("custom-connections/connections.json"),
                PathClass::AppPrivate,
            ),
            (
                ws.join("../Library/Application Support/Loupist/pi/auth.json"),
                PathClass::AppPrivate,
            ),
            (ws.join(".loupist/loupist.sqlite3"), PathClass::AppPrivate),
            (
                ws.join(".loupist/loupists/loupist-a/attachments/chat-b/brief.pdf.md"),
                PathClass::AppPrivate,
            ),
            (
                ws.join(".loupist/loupists/loupist-a/activity/run-1.jsonl"),
                PathClass::AppPrivate,
            ),
            (home.join(".ssh/id_ed25519"), PathClass::Credentials),
            (home.join(".SSH/known_hosts"), PathClass::Credentials),
            (home.join(".zshrc"), PathClass::Credentials),
            (
                home.join(".config/gcloud/credentials.db"),
                PathClass::Credentials,
            ),
            (home.join(".config/git/config"), PathClass::Credentials),
            (home.join(".config/gh/hosts.yml"), PathClass::Credentials),
            (home.join(".zshrc.backup"), PathClass::Outside),
            (home.join(".gitconfig"), PathClass::Outside),
            (home.join(".config/other/settings.json"), PathClass::Outside),
            (ws.join(".git/hooks/pre-commit"), PathClass::GitDir),
            (ws.join(".git/config"), PathClass::GitDir),
            (ws.join(".git/config.lock"), PathClass::GitDir),
            (ws.join(".git"), PathClass::GitDir),
            (ws.join("vendor/lib/.git"), PathClass::GitDir),
            (ws.join("vendor/lib/.git/info/exclude"), PathClass::GitDir),
            (ws.join(".g2/config"), PathClass::Workspace),
            (
                ws.join(".git/modules/vendor/hooks/pre-commit"),
                PathClass::GitDir,
            ),
            (ws.join(".git/modules/vendor/HEAD"), PathClass::GitDir),
            (ws.join("mirrors/site.git/config"), PathClass::GitDir),
            (
                ws.join("mirrors/site.git/hooks/post-update"),
                PathClass::GitDir,
            ),
            (ws.join("mirrors/site.git/HEAD"), PathClass::Workspace),
            (ws.join("notes/plan.md"), PathClass::Workspace),
            (ws.join("new/report.md"), PathClass::Workspace),
            (PathBuf::from("relative/report.md"), PathClass::Workspace),
            (home.join("Documents/shared.csv"), PathClass::Outside),
            (home.join("Downloads/.git/config"), PathClass::Outside),
            (home.join("other-repo/.git/config"), PathClass::Outside),
        ];
        for (path, expected) in cases {
            assert_eq!(
                classify(&path, &f.roots).class,
                expected,
                "{}",
                path.display()
            );
        }
        #[cfg(unix)]
        assert_eq!(
            classify(&f.roots.scratch.join("link-to-pi/auth.json"), &f.roots).class,
            PathClass::AppPrivate,
            "a symlink inside scratch resolves to its target"
        );
    }

    /// A write through a dangling link lands where the link points, so the
    /// decision is about that target, and the canonical path the extension
    /// pins the write to is the target too.
    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_is_decided_by_where_it_points() {
        let f = fixture();
        let ws = &f.roots.workspace;
        for (link, target, class) in [
            ("report.md", f.home.join(".zshenv"), PathClass::Credentials),
            (
                "relative.md",
                f.home.join(".zshenv"),
                PathClass::Credentials,
            ),
            ("chain.md", f.home.join(".zshenv"), PathClass::Credentials),
            (
                "db.md",
                f.roots.app_data.join("pi/secret.json"),
                PathClass::AppPrivate,
            ),
        ] {
            let classified = classify(&ws.join(link), &f.roots);
            assert_eq!(classified.class, class, "{link}");
            assert_eq!(classified.path, target, "{link}");
        }
        for exposure in [Exposure::Clean, Exposure::ThisTurn] {
            assert_eq!(
                decided(&request("write", ws.join("report.md")), &f.roots, exposure),
                "refuse credentials"
            );
            assert_eq!(
                decided(&request("write", ws.join("db.md")), &f.roots, exposure),
                "refuse app_private"
            );
            for tool in ["read", "write", "ls"] {
                assert_eq!(
                    decided(&request(tool, ws.join("loop.md")), &f.roots, exposure),
                    "refuse unresolvable",
                    "{tool}"
                );
            }
        }
        // A link's target with `..` after a symlink applies the `..` to the
        // resolved link, as the OS does.
        std::os::unix::fs::symlink(f.home.join("Documents"), ws.join("docs-link")).unwrap();
        std::os::unix::fs::symlink("docs-link/../.zshenv", ws.join("via-docs.md")).unwrap();
        assert_eq!(
            classify(&ws.join("via-docs.md"), &f.roots).path,
            f.home.join(".zshenv")
        );
    }

    /// A macOS firmlink (`/System/Volumes/Data/Users/me`) keeps its spelling
    /// through canonicalization and shares the root's identity. A symlink the
    /// resolver is not run on models it here, two components deeper than home.
    #[cfg(unix)]
    #[test]
    fn an_alias_of_a_root_at_another_depth_still_matches_its_files() {
        let f = fixture();
        let volume = f._temporary.path().join("Volumes/Data");
        std::fs::create_dir_all(&volume).unwrap();
        let home = volume.join("home");
        std::os::unix::fs::symlink(&f.home, &home).unwrap();
        let ws = home.join("Research");
        assert!(credential(&home.join(".ssh/id_ed25519"), Some(&f.home)));
        assert!(credential(&home.join(".zshrc"), Some(&f.home)));
        assert!(!credential(
            &home.join("Documents/shared.csv"),
            Some(&f.home)
        ));
        assert!(instruction_file(&ws.join("AGENTS.md"), &f.roots));
        assert!(instruction_file(
            &ws.join(".loupist/skills/screen/SKILL.md"),
            &f.roots
        ));
        assert!(!instruction_file(&ws.join("notes/plan.md"), &f.roots));
        assert!(under(&ws.join("notes/plan.md"), &f.roots.workspace));
        assert!(!under(&home.join("Documents"), &f.roots.workspace));
    }

    /// A root is found by spelling or by the identity of an existing folder,
    /// never by a case-insensitive comparison: on this case-sensitive
    /// volume `~/RESEARCH` is another folder, or none, so a write there would
    /// land outside the Loupist folder.
    #[test]
    fn a_case_variant_of_a_root_is_outside_on_a_case_sensitive_volume() {
        let f = fixture();
        let variant = f.home.join("RESEARCH");
        let app_variant = f.home.join("library/application support/LOUPIST");
        if variant.exists() || app_variant.exists() {
            eprintln!("skipping: this volume folds case");
            return;
        }
        assert_eq!(
            decide(
                &request("write", variant.join("new/planted.sh")),
                &f.roots,
                Exposure::Clean,
            ),
            NativeDecision::Refuse(NativeRefusal::Outside)
        );
        assert_eq!(
            classify(&app_variant.join("pi/auth.json"), &f.roots).class,
            PathClass::Outside
        );
        std::fs::create_dir_all(variant.join("new")).unwrap();
        assert_eq!(
            classify(&variant.join("new/planted.sh"), &f.roots).class,
            PathClass::Outside,
            "a folder that merely spells the root differently is not the root"
        );
        assert_eq!(
            classify(
                &f.roots.workspace.join(".LOUPIST/loupist.sqlite3"),
                &f.roots
            )
            .class,
            PathClass::Workspace,
            "a differently spelled folder inside the workspace is an ordinary folder"
        );
    }

    #[test]
    fn the_policy_table_decides_each_tool_class_and_exposure() {
        let f = fixture();
        let home = &f.home;
        let app = &f.roots.app_data;
        let ws = &f.roots.workspace;
        let rows: Vec<(NativeRequest, &str, &str)> = vec![
            (
                request("read", app.join("pi/auth.json")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("ls", app.join("pi")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("read", ws.join(".loupist/loupist.sqlite3")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("write", ws.join(".loupist/loupist.sqlite3")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("grep", ws.join(".loupist")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("read", ws.join(".loupist/skills/screen/screen.py")),
                "allow",
                "allow",
            ),
            (
                request("read", home.join(".ssh/id_ed25519")),
                "refuse credentials",
                "refuse credentials",
            ),
            (
                request("ls", home.join(".ssh")),
                "refuse credentials",
                "refuse credentials",
            ),
            (
                request("write", home.join(".zshrc")),
                "refuse credentials",
                "refuse credentials",
            ),
            (request("read", ws.join("AGENTS.md")), "allow", "allow"),
            (
                request("read", f.roots.scratch.join("deck.pptx")),
                "allow",
                "allow",
            ),
            (
                request("read", f.roots.attachments.join("brief.pdf.md")),
                "allow",
                "allow",
            ),
            (request("read", ws.join("notes/plan.md")), "allow", "allow"),
            (
                request("read", ws.join(".git/config")),
                "refuse git_dir",
                "refuse git_dir",
            ),
            (
                request("read", home.join("Documents/shared.csv")),
                "refuse outside",
                "refuse outside",
            ),
            (
                request("ls", home.join("Documents")),
                "refuse outside",
                "refuse outside",
            ),
            (
                request("ls", home.join(".config")),
                "refuse outside",
                "refuse outside",
            ),
            (
                request("grep", app.join("pi")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("find", home.clone()),
                "refuse search",
                "refuse search",
            ),
            (
                request("grep", home.join("..")),
                "refuse search",
                "refuse search",
            ),
            (
                request("grep", home.join(".ssh")),
                "refuse credentials",
                "refuse credentials",
            ),
            (
                request("find", home.join("Documents")),
                "refuse outside",
                "refuse outside",
            ),
            (request("grep", ws.clone()), "allow", "allow"),
            (
                request("grep", ws.join(".git")),
                "refuse git_dir",
                "refuse git_dir",
            ),
            (request("grep", f.roots.scratch.clone()), "allow", "allow"),
            (
                request("find", f.roots.attachments.clone()),
                "allow",
                "allow",
            ),
            (
                request("write", app.join("pi/models.json")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("edit", app.join("custom-connections/connections.json")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("write", f.roots.attachments.join("brief.pdf.md")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("edit", ws.join(".git/hooks/pre-commit")),
                "refuse git_dir",
                "refuse git_dir",
            ),
            (
                request("write", ws.join(".git/config")),
                "refuse git_dir",
                "refuse git_dir",
            ),
            (
                request("write", ws.join(".git")),
                "refuse git_dir",
                "refuse git_dir",
            ),
            (
                request("write", ws.join("vendor/lib/.git/info/exclude")),
                "refuse git_dir",
                "refuse git_dir",
            ),
            (
                request("read", home.join("other-repo/.git/config")),
                "refuse outside",
                "refuse outside",
            ),
            (
                request("write", f.roots.scratch.join("out.csv")),
                "allow",
                "allow",
            ),
            (request("write", ws.join("notes/plan.md")), "allow", "allow"),
            (request("write", ws.join("new/report.md")), "allow", "allow"),
            (
                request("write", ws.join("AGENTS.md")),
                "allow",
                "refuse instructions",
            ),
            (
                request("edit", app.join("skills/personal/SKILL.md")),
                "allow",
                "refuse instructions",
            ),
            (
                request("write", ws.join(".loupist/config.toml")),
                "allow",
                "refuse instructions",
            ),
            (
                request("write", home.join("Documents/out.csv")),
                "refuse outside",
                "refuse outside",
            ),
        ];
        for (request, clean_expected, exposed_expected) in &rows {
            assert_eq!(
                decided(request, &f.roots, Exposure::Clean),
                *clean_expected,
                "clean {request:?}"
            );
            assert_eq!(
                decided(request, &f.roots, Exposure::ThisTurn),
                *exposed_expected,
                "this turn {request:?}"
            );
        }
        // The custom-connection tier decides files like ThisTurn.
        assert_eq!(
            decided(
                &request("write", ws.join("AGENTS.md")),
                &f.roots,
                Exposure::Custom
            ),
            "refuse instructions"
        );
    }

    #[test]
    fn allowed_writes_return_the_canonical_target() {
        let f = fixture();
        let decision = decide(
            &request("write", f.roots.workspace.join("notes/./new/../plan.md")),
            &f.roots,
            Exposure::Clean,
        );
        assert_eq!(
            decision,
            NativeDecision::AllowFile {
                resolved: f.roots.workspace.join("notes/plan.md")
            }
        );
    }

    #[test]
    fn requests_are_parsed_strictly_at_the_boundary() {
        assert_eq!(
            NativeRequest::parse(&json!({"name":"read","path":"/tmp/a"})),
            Some(NativeRequest {
                tool: NativeTool::Read,
                target: Some("/tmp/a".into()),
            })
        );
        for malformed in [
            json!({"name":"browser","path":"/tmp/a"}),
            json!({"name":"bash","command":"ls"}),
            json!({"name":"read"}),
            json!({"name":"bash"}),
            json!({"name":"read","path":"/tmp/a","command":"ls"}),
            json!({"name":"read","path":"/tmp/a\u{0}"}),
            json!({"name":"read","path":"/".repeat(MAX_PATH_BYTES + 1)}),
            json!({"name":"read","path":"/tmp/a","extra":1}),
            json!({"name":"read","path":7}),
            json!("read"),
        ] {
            assert_eq!(NativeRequest::parse(&malformed), None, "{malformed}");
        }
    }
}
